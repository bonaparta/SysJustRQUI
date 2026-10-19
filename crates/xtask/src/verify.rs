use serde::Serialize;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static NEXT_RECORD: AtomicU64 = AtomicU64::new(0);

const STEPS: [(&str, &[&str]); 4] = [
    ("fmt", &["fmt", "--all", "--check"]),
    ("build", &["build", "--workspace", "--all-targets"]),
    ("test", &["test", "--workspace"]),
    (
        "clippy",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    ),
];

#[derive(Debug)]
struct CommandResult {
    success: bool,
    exit_code: Option<i32>,
    stdout: Vec<u8>,
}

trait Runner {
    fn run(&self, program: &str, args: &[&str], cwd: &Path) -> io::Result<CommandResult>;
}

struct SystemRunner {
    cargo_target_dir: Option<PathBuf>,
}

impl SystemRunner {
    fn new(_root: &Path) -> io::Result<Self> {
        #[cfg(windows)]
        let cargo_target_dir = {
            let primary = crate::worktree::local_storage_dir(_root, "verify-build")?;
            let fallback = crate::worktree::local_storage_dir(_root, "verify-build-fallback")?;
            Some(select_build_directory(
                &std::env::current_exe()?.canonicalize()?,
                &primary.canonicalize()?,
                &fallback.canonicalize()?,
            )?)
        };
        #[cfg(not(windows))]
        let cargo_target_dir = None;
        Ok(Self { cargo_target_dir })
    }
}

/// A verifier launched from a previous verification build must also avoid itself.
#[cfg(any(windows, test))]
fn select_build_directory(
    executable: &Path,
    primary: &Path,
    fallback: &Path,
) -> io::Result<PathBuf> {
    [primary, fallback]
        .into_iter()
        .find(|directory| !executable.starts_with(directory))
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            io::Error::other("verification build directories contain the running executable")
        })
}

impl Runner for SystemRunner {
    fn run(&self, program: &str, args: &[&str], cwd: &Path) -> io::Result<CommandResult> {
        if program == "cargo" {
            let mut command = Command::new(program);
            command
                .args(args)
                .current_dir(cwd)
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
            if let Some(directory) = &self.cargo_target_dir {
                // Keep both final artifacts and intermediate outputs away from the
                // running Windows image, including explicit build-dir overrides.
                command
                    .env("CARGO_TARGET_DIR", directory)
                    .env("CARGO_BUILD_BUILD_DIR", directory);
            }
            let status = command.status()?;
            return Ok(CommandResult {
                success: status.success(),
                exit_code: status.code(),
                stdout: Vec::new(),
            });
        }
        let output: Output = Command::new(program).args(args).current_dir(cwd).output()?;
        Ok(CommandResult {
            success: output.status.success(),
            exit_code: output.status.code(),
            stdout: output.stdout,
        })
    }
}

#[derive(Debug, Serialize)]
struct VerificationRecord {
    recorded_at_unix_ns: u128,
    workspace: String,
    branch: String,
    head_sha: String,
    dirty: bool,
    cargo_target_dir: Option<String>,
    steps: Vec<StepRecord>,
    success: bool,
}

#[derive(Debug, Serialize)]
struct StepRecord {
    name: &'static str,
    program: &'static str,
    args: Vec<&'static str>,
    status: &'static str,
    exit_code: Option<i32>,
    duration_ms: u128,
    launch_error: Option<String>,
}

pub fn run(root: &Path) -> io::Result<()> {
    let runner = SystemRunner::new(root)?;
    let head = query_text(&runner, root, &["rev-parse", "--verify", "HEAD"])?;
    let status = runner.run("git", &["status", "--porcelain=v1"], root)?;
    if !status.success {
        return Err(io::Error::other(
            "could not read Git dirty state for verification record",
        ));
    }
    let dirty = !status.stdout.is_empty();
    let branch = runner
        .run("git", &["symbolic-ref", "--quiet", "--short", "HEAD"], root)
        .ok()
        .filter(|result| result.success)
        .and_then(|result| String::from_utf8(result.stdout).ok())
        .map(|branch| branch.trim().to_owned())
        .filter(|branch| !branch.is_empty())
        .unwrap_or_else(|| "detached".to_owned());
    let workspace = root
        .canonicalize()
        .unwrap_or_else(|_| root.to_path_buf())
        .display()
        .to_string();
    let area = crate::worktree::local_storage_dir(root, "verify")?;
    if let Some(directory) = &runner.cargo_target_dir {
        println!(
            "Verification Cargo target directory: {}",
            directory.display()
        );
    }
    let mut record = execute(
        &runner,
        root,
        workspace,
        branch,
        head.trim().to_owned(),
        dirty,
        now_unix_ns(),
    )?;
    record.cargo_target_dir = runner
        .cargo_target_dir
        .map(|path| path.display().to_string());
    let record_path = write_record(&area, &record)?;

    for step in &record.steps {
        match step.status {
            "passed" => println!("verify {}: passed ({} ms)", step.name, step.duration_ms),
            "failed" => println!(
                "verify {}: failed (exit {:?}, {} ms)",
                step.name, step.exit_code, step.duration_ms
            ),
            _ => println!("verify {}: skipped", step.name),
        }
    }
    println!("Verification record: {}", record_path.display());
    if record.success {
        println!("All verification steps passed.");
        Ok(())
    } else {
        Err(io::Error::other(
            "verification failed; see the failed step above",
        ))
    }
}

fn execute(
    runner: &impl Runner,
    root: &Path,
    workspace: String,
    branch: String,
    head_sha: String,
    dirty: bool,
    recorded_at_unix_ns: u128,
) -> io::Result<VerificationRecord> {
    let mut steps = Vec::with_capacity(STEPS.len());
    let mut failed = false;
    for (name, args) in STEPS {
        if failed {
            steps.push(StepRecord {
                name,
                program: "cargo",
                args: args.to_vec(),
                status: "skipped",
                exit_code: None,
                duration_ms: 0,
                launch_error: None,
            });
            continue;
        }
        println!("verify {name}: running cargo {}", args.join(" "));
        let start = Instant::now();
        match runner.run("cargo", args, root) {
            Ok(result) => {
                failed = !result.success;
                steps.push(StepRecord {
                    name,
                    program: "cargo",
                    args: args.to_vec(),
                    status: if result.success { "passed" } else { "failed" },
                    exit_code: result.exit_code,
                    duration_ms: start.elapsed().as_millis(),
                    launch_error: None,
                });
            }
            Err(error) => {
                failed = true;
                steps.push(StepRecord {
                    name,
                    program: "cargo",
                    args: args.to_vec(),
                    status: "failed",
                    exit_code: None,
                    duration_ms: start.elapsed().as_millis(),
                    launch_error: Some(format!("could not launch cargo ({:?})", error.kind())),
                });
            }
        }
    }
    Ok(VerificationRecord {
        recorded_at_unix_ns,
        workspace,
        branch,
        head_sha,
        dirty,
        cargo_target_dir: None,
        success: !failed,
        steps,
    })
}

fn query_text(runner: &impl Runner, root: &Path, args: &[&str]) -> io::Result<String> {
    let result = runner.run("git", args, root)?;
    if !result.success {
        return Err(io::Error::other("could not read Git verification snapshot"));
    }
    String::from_utf8(result.stdout)
        .map_err(|_| io::Error::other("Git returned non-UTF-8 verification data"))
}

fn write_record(directory: &Path, record: &VerificationRecord) -> io::Result<PathBuf> {
    let bytes = serde_json::to_vec_pretty(record)
        .map_err(|_| io::Error::other("could not serialize verification record"))?;
    let sequence = NEXT_RECORD.fetch_add(1, Ordering::Relaxed);
    let path = directory.join(format!(
        "verify-{}-{}-{sequence}.json",
        record.recorded_at_unix_ns,
        std::process::id()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(path)
}

fn now_unix_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

#[cfg(test)]
mod tests {
    use super::{CommandResult, Runner, STEPS, execute, write_record};
    use std::cell::RefCell;
    use std::io;
    use std::path::Path;

    #[test]
    fn build_directory_avoids_the_running_image_and_reuses_other_cache() {
        let primary = Path::new("cache/verify-build");
        let fallback = Path::new("cache/verify-build-fallback");
        for (executable, expected) in [
            (Path::new("target/debug/xtask.exe"), primary),
            (Path::new("cache/verify-build/debug/xtask.exe"), fallback),
            (
                Path::new("cache/verify-build-fallback/debug/xtask.exe"),
                primary,
            ),
        ] {
            assert_eq!(
                super::select_build_directory(executable, primary, fallback).unwrap(),
                expected
            );
        }
        assert!(
            super::select_build_directory(
                Path::new("cache/xtask.exe"),
                Path::new("cache"),
                Path::new("cache")
            )
            .is_err()
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_rebuild_probe() {
        let Some(root) = std::env::var_os("SYSRQUI_VERIFY_LOCK_PROBE") else {
            return;
        };
        let root = std::path::PathBuf::from(root);
        let executable = std::env::current_exe().unwrap();
        // Prove the fixture really is a mapped Windows image, not merely a fake runner.
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(&executable)
                .is_err()
        );
        let bootstrap = root.join("bootstrap");
        let baseline = super::SystemRunner {
            cargo_target_dir: Some(bootstrap.clone()),
        };
        assert!(
            !baseline
                .run("cargo", &["build", "--offline"], &root)
                .unwrap()
                .success
        );
        let isolated = super::select_build_directory(
            &executable.canonicalize().unwrap(),
            &bootstrap.canonicalize().unwrap(),
            &root.join("isolated"),
        )
        .unwrap();
        let runner = super::SystemRunner {
            cargo_target_dir: Some(isolated.clone()),
        };
        assert!(
            runner
                .run("cargo", &["build", "--offline"], &root)
                .unwrap()
                .success
        );
        assert!(isolated.join("debug/lock-probe.exe").is_file());
    }

    #[cfg(windows)]
    #[test]
    fn windows_workspace_build_succeeds_with_locked_bootstrap_executable() {
        let root = std::env::temp_dir().join(format!(
            "sysrqui-verify-lock-{}-{}",
            std::process::id(),
            super::now_unix_ns()
        ));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("bootstrap/debug")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"lock-probe\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        let helper = root.join("bootstrap/debug/lock-probe.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &helper).unwrap();
        let output = std::process::Command::new(&helper)
            .args([
                "--exact",
                "verify::tests::windows_rebuild_probe",
                "--nocapture",
            ])
            .env("SYSRQUI_VERIFY_LOCK_PROBE", &root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "probe failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    struct FakeRunner {
        fail_step: Option<usize>,
        launch_error_step: Option<usize>,
        calls: RefCell<Vec<(String, Vec<String>)>>,
    }

    impl Runner for FakeRunner {
        fn run(&self, program: &str, args: &[&str], _cwd: &Path) -> io::Result<CommandResult> {
            let mut calls = self.calls.borrow_mut();
            let index = calls.len();
            calls.push((
                program.to_owned(),
                args.iter().map(|arg| (*arg).to_owned()).collect(),
            ));
            if self.launch_error_step == Some(index) {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "fake cargo missing",
                ));
            }
            Ok(CommandResult {
                success: self.fail_step != Some(index),
                exit_code: Some(if self.fail_step == Some(index) { 2 } else { 0 }),
                stdout: Vec::new(),
            })
        }
    }

    #[test]
    fn runs_fixed_steps_fail_fast_and_records_skips_and_snapshot() {
        let runner = FakeRunner {
            fail_step: Some(2),
            launch_error_step: None,
            calls: RefCell::new(Vec::new()),
        };
        let report = execute(
            &runner,
            Path::new("workspace"),
            "workspace".into(),
            "codex/issue-9-worktree".into(),
            "head123".into(),
            true,
            1234,
        )
        .unwrap();
        assert!(!report.success);
        assert_eq!(report.head_sha, "head123");
        assert!(report.dirty);
        assert_eq!(
            report
                .steps
                .iter()
                .map(|step| step.status)
                .collect::<Vec<_>>(),
            ["passed", "passed", "failed", "skipped"]
        );
        assert_eq!(
            runner
                .calls
                .borrow()
                .iter()
                .map(|(program, _)| program.as_str())
                .collect::<Vec<_>>(),
            ["cargo", "cargo", "cargo"]
        );
        assert_eq!(report.steps[0].args, STEPS[0].1);
        assert_eq!(report.steps[1].args, STEPS[1].1);
        assert_eq!(report.steps[2].exit_code, Some(2));
        assert_eq!(report.steps[3].exit_code, None);

        let directory = std::env::temp_dir().join(format!("sysrqui-verify-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = write_record(&directory, &report).unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["head_sha"], "head123");
        assert_eq!(saved["dirty"], true);
        assert_eq!(saved["branch"], "codex/issue-9-worktree");
        assert_eq!(saved["workspace"], "workspace");
        assert_eq!(saved["steps"][3]["status"], "skipped");
        let second = write_record(&directory, &report).unwrap();
        assert_ne!(path, second);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn all_steps_are_run_in_the_required_order_on_success() {
        let runner = FakeRunner {
            fail_step: None,
            launch_error_step: None,
            calls: RefCell::new(Vec::new()),
        };
        let report = execute(
            &runner,
            Path::new("workspace"),
            "workspace".into(),
            "branch".into(),
            "head".into(),
            false,
            1,
        )
        .unwrap();
        assert!(report.success);
        assert_eq!(runner.calls.borrow().len(), 4);
        assert!(report.steps.iter().all(|step| step.status == "passed"));
    }

    #[test]
    fn cargo_launch_error_is_recorded_and_remaining_steps_are_skipped() {
        let runner = FakeRunner {
            fail_step: None,
            launch_error_step: Some(1),
            calls: RefCell::new(Vec::new()),
        };
        let report = execute(
            &runner,
            Path::new("workspace"),
            "workspace".into(),
            "branch".into(),
            "head".into(),
            false,
            1,
        )
        .unwrap();
        assert!(!report.success);
        assert_eq!(report.steps[0].status, "passed");
        assert_eq!(report.steps[1].status, "failed");
        assert_eq!(report.steps[1].exit_code, None);
        assert_eq!(
            report.steps[1].launch_error.as_deref(),
            Some("could not launch cargo (NotFound)")
        );
        assert_eq!(report.steps[2].status, "skipped");
        assert_eq!(report.steps[3].status, "skipped");
        let directory =
            std::env::temp_dir().join(format!("sysrqui-verify-launch-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = write_record(&directory, &report).unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["steps"][1]["status"], "failed");
        assert_eq!(saved["steps"][2]["status"], "skipped");
        assert_eq!(saved["success"], false);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
