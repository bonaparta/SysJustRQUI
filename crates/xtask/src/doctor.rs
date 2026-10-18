use std::env;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const REQUIRED_TOOLS: [&str; 4] = ["git", "cargo", "rustc", "gh"];
const OPTIONAL_AI_TOOLS: [&str; 4] = ["codex", "claude", "gemini", "grok"];
const VERSION_TIMEOUT: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(25);

pub fn run() -> io::Result<()> {
    let report = collect_report(&SystemProbe);

    println!("System: {} ({})", operating_system(), env::consts::ARCH);
    println!("Required tools:");
    for check in &report.required {
        match &check.result {
            RequiredResult::Version(version) => {
                println!("  {}: OK — {}", check.name, version);
            }
            RequiredResult::Missing => {
                println!(
                    "  {}: MISSING — install it, then reopen the terminal or add it to PATH.",
                    check.name
                );
            }
            RequiredResult::VersionFailed => {
                println!(
                    "  {}: ERROR — found, but its version check failed or timed out; confirm the installation and PATH.",
                    check.name
                );
            }
        }
    }

    println!("Optional AI tools (presence only; login/non-interactive support not checked):");
    for check in &report.optional {
        if check.found {
            println!("  {}: found", check.name);
        } else {
            println!("  {}: not found (optional)", check.name);
        }
    }

    if report.success {
        println!("Doctor checks passed.");
        Ok(())
    } else {
        Err(io::Error::other(
            "one or more required tool checks failed; see the report above",
        ))
    }
}

fn operating_system() -> &'static str {
    match env::consts::OS {
        "windows" => "Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        other => other,
    }
}

#[derive(Debug)]
struct Report {
    required: Vec<RequiredCheck>,
    optional: Vec<OptionalCheck>,
    success: bool,
}

#[derive(Debug)]
struct RequiredCheck {
    name: &'static str,
    result: RequiredResult,
}

#[derive(Debug)]
enum RequiredResult {
    Version(String),
    Missing,
    VersionFailed,
}

#[derive(Debug)]
struct OptionalCheck {
    name: &'static str,
    found: bool,
}

trait Probe {
    fn find(&self, name: &str) -> Option<PathBuf>;
    fn version(&self, path: &Path) -> Result<String, ProbeFailure>;
}

#[derive(Debug)]
struct ProbeFailure;

fn collect_report(probe: &impl Probe) -> Report {
    let required = REQUIRED_TOOLS
        .into_iter()
        .map(|name| {
            let result = match probe.find(name) {
                None => RequiredResult::Missing,
                Some(path) => match probe.version(&path) {
                    Ok(version) => RequiredResult::Version(version),
                    Err(_) => RequiredResult::VersionFailed,
                },
            };
            RequiredCheck { name, result }
        })
        .collect::<Vec<_>>();

    let optional = OPTIONAL_AI_TOOLS
        .into_iter()
        .map(|name| OptionalCheck {
            name,
            found: probe.find(name).is_some(),
        })
        .collect::<Vec<_>>();

    let success = required
        .iter()
        .all(|check| matches!(&check.result, RequiredResult::Version(_)));

    Report {
        required,
        optional,
        success,
    }
}

struct SystemProbe;

impl Probe for SystemProbe {
    fn find(&self, name: &str) -> Option<PathBuf> {
        let path = env::var_os("PATH")?;
        let path_ext = env::var_os("PATHEXT");
        find_in_path(name, &path, path_ext.as_deref(), cfg!(windows))
    }

    fn version(&self, path: &Path) -> Result<String, ProbeFailure> {
        let output = version_output(path).map_err(|_| ProbeFailure)?;
        if !output.status.success() {
            return Err(ProbeFailure);
        }

        let line = String::from_utf8_lossy(&output.stdout)
            .lines()
            .find(|line| !line.trim().is_empty())
            .map(safe_version_line)
            .filter(|line| !line.is_empty())
            .ok_or(ProbeFailure)?;
        Ok(line)
    }
}

fn candidate_names_for(name: &str, path_ext: Option<&OsStr>, windows: bool) -> Vec<String> {
    if !windows || Path::new(name).extension().is_some() {
        return vec![name.to_owned()];
    }

    let configured_extensions = path_ext
        .and_then(OsStr::to_str)
        .unwrap_or(".COM;.EXE;.BAT;.CMD");
    let mut extensions = configured_extensions
        .split(';')
        .map(str::trim)
        .filter(|extension| !extension.is_empty())
        .map(|extension| {
            if extension.starts_with('.') {
                extension.to_owned()
            } else {
                format!(".{extension}")
            }
        })
        .collect::<Vec<_>>();

    // PowerShell wrappers are not included in the usual PATHEXT default.
    for extension in [".COM", ".EXE", ".BAT", ".CMD", ".PS1"] {
        if !extensions
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(extension))
        {
            extensions.push(extension.to_owned());
        }
    }

    let mut names = extensions
        .into_iter()
        .map(|extension| format!("{name}{extension}"))
        .collect::<Vec<_>>();
    names.push(name.to_owned());
    names
}

fn find_in_path(
    name: &str,
    path_value: &OsStr,
    path_ext: Option<&OsStr>,
    windows: bool,
) -> Option<PathBuf> {
    for directory in env::split_paths(path_value) {
        for candidate in candidate_names_for(name, path_ext, windows) {
            let candidate = directory.join(candidate);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn version_output(path: &Path) -> io::Result<Output> {
    let extension = path.extension().and_then(OsStr::to_str).unwrap_or_default();

    #[cfg(windows)]
    let mut command = if ["cmd", "bat"]
        .iter()
        .any(|script| extension.eq_ignore_ascii_case(script))
    {
        let path = path.to_string_lossy();
        let command_line = format!("\"\"{path}\" --version\"");
        let mut command = Command::new("cmd.exe");
        command.args(["/d", "/s", "/c"]).arg(command_line);
        command
    } else if extension.eq_ignore_ascii_case("ps1") {
        let path = path.to_string_lossy().replace('\'', "''");
        let script = format!("& '{path}' --version");
        let mut command = Command::new("powershell.exe");
        command
            .args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(script);
        command
    } else {
        let mut command = Command::new(path);
        command.arg("--version");
        command
    };

    #[cfg(not(windows))]
    let mut command = {
        let _ = extension;
        let mut command = Command::new(path);
        command.arg("--version");
        command
    };

    command.stdout(Stdio::piped()).stderr(Stdio::null());
    run_with_timeout(&mut command, VERSION_TIMEOUT)
}

fn run_with_timeout(command: &mut Command, timeout: Duration) -> io::Result<Output> {
    let child = command.spawn()?;
    wait_until_done(child, Instant::now(), timeout)
}

fn wait_until_done(mut child: Child, start: Instant, timeout: Duration) -> io::Result<Output> {
    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output();
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "version probe timed out",
            ));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn safe_version_line(line: &str) -> String {
    let sanitized = line
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    sanitized.trim().chars().take(160).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        OPTIONAL_AI_TOOLS, Probe, ProbeFailure, REQUIRED_TOOLS, RequiredResult,
        candidate_names_for, collect_report, find_in_path,
    };
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::env;
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};

    #[derive(Default)]
    struct FakeProbe {
        found: HashMap<String, PathBuf>,
        versions: HashMap<String, Result<String, ()>>,
        version_calls: RefCell<Vec<String>>,
    }

    impl Probe for FakeProbe {
        fn find(&self, name: &str) -> Option<PathBuf> {
            self.found.get(name).cloned()
        }

        fn version(&self, path: &Path) -> Result<String, ProbeFailure> {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            self.version_calls.borrow_mut().push(name.clone());
            match self.versions.get(&name) {
                Some(Ok(version)) => Ok(version.clone()),
                _ => Err(ProbeFailure),
            }
        }
    }

    fn complete_probe() -> FakeProbe {
        let mut probe = FakeProbe::default();
        for tool in REQUIRED_TOOLS {
            let path = PathBuf::from(tool);
            probe.found.insert(tool.to_owned(), path.clone());
            probe
                .versions
                .insert(tool.to_owned(), Ok(format!("{tool} version 1.0")));
        }
        probe
    }

    #[test]
    fn required_tools_pass_and_optional_tools_are_presence_only() {
        let mut probe = complete_probe();
        for tool in OPTIONAL_AI_TOOLS {
            probe.found.insert(tool.to_owned(), PathBuf::from(tool));
        }

        let report = collect_report(&probe);

        assert!(report.success);
        assert!(
            report
                .required
                .iter()
                .all(|check| matches!(&check.result, RequiredResult::Version(_)))
        );
        assert!(report.optional.iter().all(|check| check.found));
        assert_eq!(probe.version_calls.borrow().len(), REQUIRED_TOOLS.len());
        assert!(OPTIONAL_AI_TOOLS.iter().all(|tool| {
            !probe
                .version_calls
                .borrow()
                .iter()
                .any(|called| called == tool)
        }));
    }

    #[test]
    fn missing_required_tool_fails_but_missing_optional_tool_does_not() {
        let mut probe = complete_probe();
        probe.found.remove("gh");

        let report = collect_report(&probe);

        assert!(!report.success);
        assert!(matches!(
            &report
                .required
                .iter()
                .find(|check| check.name == "gh")
                .unwrap()
                .result,
            RequiredResult::Missing
        ));
        assert!(report.optional.iter().all(|check| !check.found));
    }

    #[test]
    fn required_version_probe_failure_fails() {
        let mut probe = complete_probe();
        probe.versions.insert("cargo".to_owned(), Err(()));

        let report = collect_report(&probe);

        assert!(!report.success);
        assert!(matches!(
            &report
                .required
                .iter()
                .find(|check| check.name == "cargo")
                .unwrap()
                .result,
            RequiredResult::VersionFailed
        ));
    }

    #[test]
    fn missing_optional_providers_do_not_fail_when_required_tools_pass() {
        let probe = complete_probe();

        let report = collect_report(&probe);

        assert!(report.success);
        assert!(report.optional.iter().all(|check| !check.found));
    }

    #[test]
    fn windows_path_lookup_handles_spaces_and_script_extensions() {
        let temp =
            std::env::temp_dir().join(format!("sysrqui-xtask-path-test-{}", std::process::id()));
        let directory = temp.join("Program Files").join("Tool Wrappers");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("git.CMD"), "wrapper").unwrap();
        std::fs::write(directory.join("cargo.BAT"), "wrapper").unwrap();
        std::fs::write(directory.join("gh.PS1"), "wrapper").unwrap();
        std::fs::write(directory.join("codex.CMD"), "wrapper").unwrap();
        let path_value = env::join_paths([&directory]).unwrap();
        let path_ext = OsStr::new(".EXE;.BAT;.CMD");

        assert_eq!(
            find_in_path("git", &path_value, Some(path_ext), true),
            Some(directory.join("git.CMD"))
        );
        assert_eq!(
            find_in_path("cargo", &path_value, Some(path_ext), true),
            Some(directory.join("cargo.BAT"))
        );
        assert_eq!(
            find_in_path("gh", &path_value, Some(path_ext), true),
            Some(directory.join("gh.PS1"))
        );
        assert!(
            candidate_names_for("codex", Some(path_ext), true)
                .iter()
                .any(|candidate| candidate.ends_with(".CMD"))
        );

        std::fs::remove_dir_all(temp).unwrap();
    }
}
