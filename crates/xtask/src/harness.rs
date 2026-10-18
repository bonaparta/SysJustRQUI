#![cfg_attr(not(windows), allow(dead_code))]

use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
const DEFAULT_REPOSITORY: &str = "bonaparta/SysJustRQUI";
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_TIMEOUT_SECS: u64 = 3_600;
const MAX_ATTEMPTS: u32 = 10;
const MAX_TOTAL_WORKER_SECS: u64 = 3_600;
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
static NEXT_PROBE: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Config {
    issue: u64,
    owner: String,
    repository: String,
    run_id: String,
    session_id: u32,
    branch: String,
    base_sha: String,
    worktree: PathBuf,
    worker: PathBuf,
    timeout_secs: u64,
    max_attempts: u32,
    package: PathBuf,
}

#[derive(Debug)]
enum Operation {
    RunOnce(Args),
    Resume(Args),
    Cancel { issue: u64, owner: String },
}

#[derive(Debug)]
#[allow(dead_code)]
struct Args {
    issue: u64,
    owner: String,
    worker: PathBuf,
    timeout_secs: u64,
    max_attempts: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct WorkerResult {
    protocol_version: u32,
    issue: u64,
    owner: String,
    run_id: String,
    status: String,
    summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkerStatus {
    Completed,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    run_id: String,
    issue: u64,
    owner: String,
    branch: String,
    worktree_path: String,
    base_sha: String,
    head_sha: Option<String>,
    pull_request: String,
    checks: String,
    last_action: String,
    timestamp_unix_secs: u64,
    next_step: String,
    uncommitted_work: String,
    unpushed_commits: String,
    token_usage: String,
    context_usage: String,
    issue_state: String,
    remote_branch: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
enum Event {
    Starting {
        attempt: u32,
        job_name: String,
    },
    Finished {
        attempt: u32,
        outcome: Outcome,
        exit_code: Option<i32>,
    },
    Checkpoint {
        checkpoint: Box<Checkpoint>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Completed,
    Blocked,
    Failed,
    TimedOut,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Default)]
struct JournalState {
    attempts: u32,
    pending: Option<(u32, String)>,
    completed: bool,
}

#[derive(Debug)]
struct Paths {
    directory: PathBuf,
    config: PathBuf,
    events: PathBuf,
    lock: PathBuf,
    cancel: PathBuf,
}

struct RunLock(File);

impl Drop for RunLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

struct ProbeOutput {
    success: bool,
    stdout: Vec<u8>,
}

#[cfg(windows)]
struct BoundedRunner {
    directory: PathBuf,
}

fn bounded_output(
    program: &str,
    args: &[&str],
    cwd: &Path,
    storage: &Path,
) -> io::Result<ProbeOutput> {
    fs::create_dir_all(storage)?;
    let id = NEXT_PROBE.fetch_add(1, Ordering::Relaxed);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let prefix = format!("probe-{}-{nonce}-{id}", std::process::id());
    let stdout_path = storage.join(format!("{prefix}.stdout"));
    let stderr_path = storage.join(format!("{prefix}.stderr"));
    let stdout = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stdout_path)?;
    let stderr = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stderr_path)?;
    let mut child = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let status = loop {
        match child.try_wait()? {
            Some(status) => break Some(status),
            None if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            None => {
                let _ = child.kill();
                let cleanup_deadline = Instant::now() + Duration::from_secs(2);
                loop {
                    if child.try_wait()?.is_some() {
                        break;
                    }
                    if Instant::now() >= cleanup_deadline {
                        return Err(invalid(
                            "StopUnconfirmed: timed-out GitHub/Git probe did not stop",
                        ));
                    }
                    thread::sleep(Duration::from_millis(25));
                }
                break None;
            }
        }
    };
    let mut output = Vec::new();
    if let Ok(file) = File::open(&stdout_path) {
        use std::io::Read;
        file.take(1024 * 1024).read_to_end(&mut output)?;
    }
    let _ = fs::remove_file(stdout_path);
    let _ = fs::remove_file(stderr_path);
    Ok(ProbeOutput {
        success: status.is_some_and(|status| status.success()),
        stdout: output,
    })
}

#[cfg(windows)]
impl crate::task::Runner for BoundedRunner {
    fn run(
        &self,
        program: &str,
        args: &[&str],
        cwd: &Path,
    ) -> io::Result<crate::task::CommandResult> {
        let output = bounded_output(program, args, cwd, &self.directory)?;
        Ok(crate::task::CommandResult {
            success: output.success,
            stdout: output.stdout,
        })
    }
}

trait Executor {
    fn recover(&self, job_name: &str) -> io::Result<()>;
    fn execute(
        &self,
        config: &Config,
        attempt: u32,
        job_name: &str,
        result_path: &Path,
        cancel_path: &Path,
    ) -> io::Result<(Outcome, Option<i32>)>;
}

#[cfg(windows)]
struct SystemExecutor;

#[cfg(windows)]
impl Executor for SystemExecutor {
    fn recover(&self, job_name: &str) -> io::Result<()> {
        #[cfg(windows)]
        {
            windows_job::recover(job_name)
        }
        #[cfg(not(windows))]
        {
            let _ = job_name;
            Err(unsupported())
        }
    }

    fn execute(
        &self,
        config: &Config,
        attempt: u32,
        job_name: &str,
        result_path: &Path,
        cancel_path: &Path,
    ) -> io::Result<(Outcome, Option<i32>)> {
        #[cfg(windows)]
        {
            windows_job::execute(config, attempt, job_name, result_path, cancel_path)
        }
        #[cfg(not(windows))]
        {
            let _ = (config, attempt, job_name, result_path, cancel_path);
            Err(unsupported())
        }
    }
}

pub fn run(args: Vec<String>, root: &Path) -> io::Result<()> {
    match parse_args(args)? {
        Operation::RunOnce(args) => run_once(root, args),
        Operation::Resume(args) => resume(root, args),
        Operation::Cancel { issue, owner } => cancel(root, issue, &owner),
    }
}

fn parse_args(args: Vec<String>) -> io::Result<Operation> {
    let mut args = args.into_iter();
    let operation = args.next().ok_or_else(usage)?;
    if operation == "cancel" {
        let mut issue = None;
        let mut owner = None;
        while let Some(flag) = args.next() {
            let value = args
                .next()
                .ok_or_else(|| invalid(format!("missing value for {flag}")))?;
            match flag.as_str() {
                "--issue" if issue.is_none() => issue = Some(parse_positive(&value, "--issue")?),
                "--owner" if owner.is_none() => owner = Some(validate_owner(value)?),
                "--issue" | "--owner" => return Err(invalid(format!("duplicate argument {flag}"))),
                _ => return Err(invalid(format!("unknown argument {flag}"))),
            }
        }
        return Ok(Operation::Cancel {
            issue: issue.ok_or_else(|| invalid("--issue is required"))?,
            owner: owner.ok_or_else(|| invalid("--owner is required"))?,
        });
    }

    let mut issue = None;
    let mut owner = None;
    let mut worker = None;
    let mut timeout_secs = None;
    let mut max_attempts = None;
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| invalid(format!("missing value for {flag}")))?;
        match flag.as_str() {
            "--issue" if issue.is_none() => issue = Some(parse_positive(&value, "--issue")?),
            "--owner" if owner.is_none() => owner = Some(validate_owner(value)?),
            "--worker" if worker.is_none() => worker = Some(PathBuf::from(value)),
            "--timeout-secs" if timeout_secs.is_none() => {
                timeout_secs = Some(parse_positive(&value, "--timeout-secs")?)
            }
            "--max-attempts" if max_attempts.is_none() => {
                let count = parse_positive(&value, "--max-attempts")?;
                max_attempts =
                    Some(u32::try_from(count).map_err(|_| invalid("--max-attempts is too large"))?);
            }
            "--issue" | "--owner" | "--worker" | "--timeout-secs" | "--max-attempts" => {
                return Err(invalid(format!("duplicate argument {flag}")));
            }
            _ => return Err(invalid(format!("unknown argument {flag}"))),
        }
    }
    let args = Args {
        issue: issue.ok_or_else(|| invalid("--issue is required"))?,
        owner: owner.ok_or_else(|| invalid("--owner is required"))?,
        worker: worker.ok_or_else(|| invalid("--worker is required"))?,
        timeout_secs: timeout_secs.ok_or_else(|| invalid("--timeout-secs is required"))?,
        max_attempts: max_attempts.ok_or_else(|| invalid("--max-attempts is required"))?,
    };
    if args.timeout_secs > u64::from(u32::MAX) / 1000 {
        return Err(invalid(
            "--timeout-secs is too large for the platform timeout API",
        ));
    }
    if args.timeout_secs > MAX_TIMEOUT_SECS
        || args.max_attempts > MAX_ATTEMPTS
        || args
            .timeout_secs
            .saturating_mul(u64::from(args.max_attempts))
            > MAX_TOTAL_WORKER_SECS
    {
        return Err(invalid(
            "worker time budget is limited to one hour total and ten attempts",
        ));
    }
    match operation.as_str() {
        "run-once" => Ok(Operation::RunOnce(args)),
        "resume" => Ok(Operation::Resume(args)),
        _ => Err(usage()),
    }
}

#[cfg(not(windows))]
fn run_once(_root: &Path, _args: Args) -> io::Result<()> {
    Err(unsupported())
}

#[cfg(windows)]
fn run_once(root: &Path, args: Args) -> io::Result<()> {
    let active = crate::worktree::active_worktree(root, args.issue, &args.owner)?;
    let worker = validate_worker(&args.worker)?;
    let harness_dir = crate::worktree::local_storage_dir(root, "harness")?;
    let runner = BoundedRunner {
        directory: harness_dir,
    };
    let package = crate::task::prepare_for_active_worktree_with_runner(
        root,
        args.issue,
        &args.owner,
        &runner,
    )?;
    let config = Config {
        issue: args.issue,
        owner: args.owner,
        repository: DEFAULT_REPOSITORY.to_owned(),
        run_id: active.run_id,
        session_id: current_session_id()?,
        branch: active.branch,
        base_sha: active.base_sha,
        worktree: active.path,
        worker,
        timeout_secs: args.timeout_secs,
        max_attempts: args.max_attempts,
        package: package.json,
    };
    create_run(root, &config)?;
    run_engine(root, &config, false, &SystemExecutor)
}

#[cfg(not(windows))]
fn resume(_root: &Path, _args: Args) -> io::Result<()> {
    Err(unsupported())
}

#[cfg(windows)]
fn resume(root: &Path, args: Args) -> io::Result<()> {
    let worker = validate_worker(&args.worker)?;
    let paths = paths(root, args.issue)?;
    let config: Config = read_json(&paths.config)?;
    let expected = (
        config.owner.as_str(),
        config.worker.as_path(),
        config.timeout_secs,
        config.max_attempts,
    );
    if config.issue != args.issue
        || expected.0 != args.owner
        || expected.1 != worker
        || expected.2 != args.timeout_secs
        || expected.3 != args.max_attempts
    {
        return Err(invalid(
            "resume options do not match the persisted run configuration",
        ));
    }
    if config.session_id != current_session_id()? {
        return Err(invalid(
            "the run belongs to another Windows session; refusing Job recovery",
        ));
    }
    validate_active_assignment(root, &config)?;
    run_engine(root, &config, true, &SystemExecutor)
}

#[cfg(windows)]
fn current_session_id() -> io::Result<u32> {
    windows_job::current_session_id()
}

#[cfg(windows)]
fn validate_active_assignment(root: &Path, config: &Config) -> io::Result<()> {
    let active = crate::worktree::active_worktree(root, config.issue, &config.owner)?;
    if active.run_id != config.run_id
        || active.owner != config.owner
        || active.path != config.worktree
        || active.branch != config.branch
        || active.base_sha != config.base_sha
    {
        return Err(invalid(
            "active worktree no longer matches the persisted run; refusing resume",
        ));
    }
    let storage = crate::worktree::local_storage_dir(root, "harness")?;
    let branch = git_text(&storage, &config.worktree, &["branch", "--show-current"]);
    if branch.as_deref() != Some(config.branch.as_str()) {
        return Err(invalid(
            "current worktree branch differs from the assigned branch; refusing resume",
        ));
    }
    Ok(())
}

fn cancel(root: &Path, issue: u64, owner: &str) -> io::Result<()> {
    let paths = paths(root, issue)?;
    let config: Config = read_json(&paths.config)?;
    if config.owner != owner || config.issue != issue {
        return Err(invalid("run owner does not match the requested owner"));
    }
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&paths.cancel)
    {
        Ok(mut file) => {
            writeln!(
                file,
                "issue={issue}\nowner={owner}\nrun_id={}",
                config.run_id
            )?;
            file.sync_all()?;
            println!("Cancellation requested for issue #{issue}.");
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            println!("Cancellation is already requested for issue #{issue}.");
            Ok(())
        }
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn create_run(root: &Path, config: &Config) -> io::Result<()> {
    let storage = crate::worktree::local_storage_dir(root, "harness")?;
    let paths = paths_for_directory(storage.join(format!("issue-{}", config.issue)));
    fs::create_dir(&paths.directory)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&paths.config)?;
    serde_json::to_writer_pretty(&mut file, config).map_err(json_error)?;
    file.sync_all()?;
    Ok(())
}

fn paths(root: &Path, issue: u64) -> io::Result<Paths> {
    let storage = crate::worktree::local_storage_dir(root, "harness")?;
    let directory = storage.join(format!("issue-{issue}"));
    if !directory.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no harness run exists for this issue",
        ));
    }
    let metadata = fs::symlink_metadata(&directory)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid("harness run path is not a regular directory"));
    }
    Ok(paths_for_directory(directory))
}

fn paths_for_directory(directory: PathBuf) -> Paths {
    Paths {
        config: directory.join("config.json"),
        events: directory.join("events.jsonl"),
        lock: directory.join("run.lock"),
        cancel: directory.join("cancel.request"),
        directory,
    }
}

#[cfg(windows)]
fn run_engine(
    root: &Path,
    config: &Config,
    resuming: bool,
    executor: &impl Executor,
) -> io::Result<()> {
    run_engine_paths(
        Some(root),
        &paths(root, config.issue)?,
        config,
        resuming,
        executor,
    )
}

fn run_engine_paths(
    root: Option<&Path>,
    paths: &Paths,
    config: &Config,
    resuming: bool,
    executor: &impl Executor,
) -> io::Result<()> {
    #[cfg(not(windows))]
    let _ = root;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .truncate(false)
        .create(true)
        .open(&paths.lock)?;
    lock.try_lock()
        .map_err(|_| invalid("another harness process holds the run lock"))?;
    let _lock = RunLock(lock);
    let mut state = read_journal(&paths.events)?;
    if state.completed {
        println!(
            "Issue #{} already has a completed worker result; no worker was started.",
            config.issue
        );
        return Ok(());
    }
    if !resuming && state.attempts > 0 {
        return Err(invalid(
            "run-once cannot be used after an attempt; use resume",
        ));
    }
    if let Some((attempt, job_name)) = state.pending.take() {
        executor
            .recover(&job_name)
            .map_err(|error| invalid(format!("StopUnconfirmed: {error}")))?;
        append_event(
            &paths.events,
            &Event::Finished {
                attempt,
                outcome: Outcome::Interrupted,
                exit_code: None,
            },
        )?;
        append_checkpoint(
            &paths.events,
            checkpoint(
                config,
                &paths.directory,
                "recovered interrupted attempt",
                "resume after stop confirmation",
                "unknown",
                "unknown",
            ),
        )?;
        state.attempts = attempt;
    }
    #[cfg(windows)]
    if resuming && let Some(root) = root {
        validate_active_assignment(root, config)?;
        validate_issue_for_resume(root, paths, config)?;
        reconcile(config, paths)?;
    }
    if state.attempts >= config.max_attempts {
        append_checkpoint(
            &paths.events,
            checkpoint(
                config,
                &paths.directory,
                "attempt budget exhausted",
                "coordinator review",
                "unknown",
                "unknown",
            ),
        )?;
        println!(
            "Issue #{} exhausted its {} persisted attempts.",
            config.issue, config.max_attempts
        );
        return Err(invalid(
            "attempt budget exhausted; checkpoint preserved for coordinator review",
        ));
    }
    if paths.cancel.exists() {
        if resuming {
            let acknowledged = paths.directory.join(format!(
                "cancel-acknowledged-{}.request",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ));
            fs::rename(&paths.cancel, acknowledged)?;
            append_checkpoint(
                &paths.events,
                checkpoint(
                    config,
                    &paths.directory,
                    "previous cancellation acknowledged",
                    "resume after stop confirmation",
                    "unknown",
                    "unknown",
                ),
            )?;
        } else {
            println!("Cancellation requested; no new worker was started.");
            append_checkpoint(
                &paths.events,
                checkpoint(
                    config,
                    &paths.directory,
                    "cancellation marker observed",
                    "resume explicitly to acknowledge cancellation",
                    "unknown",
                    "unknown",
                ),
            )?;
            return Err(invalid("cancellation requested; checkpoint preserved"));
        }
    }
    let mut attempt = state.attempts + 1;
    loop {
        #[cfg(windows)]
        if resuming && let Some(root) = root {
            validate_active_assignment(root, config)?;
            validate_issue_for_resume(root, paths, config)?;
        }
        let job_name = job_name(config, attempt);
        let result_path = paths.directory.join(format!("result-{attempt}.json"));
        if fs::symlink_metadata(&result_path).is_ok() {
            return Err(invalid(
                "attempt result path already exists; refusing to overwrite it",
            ));
        }
        append_event(
            &paths.events,
            &Event::Starting {
                attempt,
                job_name: job_name.clone(),
            },
        )?;
        append_checkpoint(
            &paths.events,
            checkpoint(
                config,
                &paths.directory,
                &format!("starting attempt {attempt}"),
                "worker running",
                "unknown",
                "unknown",
            ),
        )?;
        state.pending = Some((attempt, job_name.clone()));
        state.attempts = attempt;
        let (mut outcome, exit_code) =
            executor.execute(config, attempt, &job_name, &result_path, &paths.cancel)?;
        let mut blocked_reason = None;
        if outcome == Outcome::Completed {
            match validate_result(&result_path, config) {
                Ok(WorkerStatus::Completed) => state.completed = true,
                Ok(WorkerStatus::Blocked) => {
                    outcome = Outcome::Blocked;
                    blocked_reason = read_json::<WorkerResult>(&result_path)
                        .ok()
                        .map(|result| result.summary);
                }
                Err(_) => outcome = Outcome::Failed,
            }
        }
        append_event(
            &paths.events,
            &Event::Finished {
                attempt,
                outcome: outcome.clone(),
                exit_code,
            },
        )?;
        append_checkpoint(
            &paths.events,
            checkpoint(
                config,
                &paths.directory,
                &format!("attempt {attempt} ended as {outcome:?}"),
                blocked_reason
                    .as_deref()
                    .filter(|_| outcome == Outcome::Blocked)
                    .unwrap_or("inspect checkpoint before resume"),
                "unknown",
                "unknown",
            ),
        )?;
        state.pending = None;
        if outcome == Outcome::Completed {
            println!(
                "Worker completed issue #{} (result: {}).",
                config.issue,
                result_path.display()
            );
            return Ok(());
        }
        if outcome == Outcome::Cancelled
            || outcome == Outcome::TimedOut
            || outcome == Outcome::Blocked
            || !resuming
            || attempt >= config.max_attempts
        {
            println!(
                "Worker attempt {attempt} ended as {outcome:?}; attempts used: {attempt}/{}.",
                config.max_attempts
            );
            return Err(invalid(format!(
                "worker attempt {attempt} ended as {outcome:?}; attempts used: {attempt}/{}; checkpoint preserved",
                config.max_attempts
            )));
        }
        if paths.cancel.exists() {
            append_checkpoint(
                &paths.events,
                checkpoint(
                    config,
                    &paths.directory,
                    "cancellation marker observed after attempt",
                    "resume explicitly to acknowledge cancellation",
                    "unknown",
                    "unknown",
                ),
            )?;
            return Err(invalid("cancellation requested; checkpoint preserved"));
        }
        attempt += 1;
    }
}

#[cfg(windows)]
fn validate_issue_for_resume(root: &Path, paths: &Paths, config: &Config) -> io::Result<()> {
    let runner = BoundedRunner {
        directory: paths.directory.clone(),
    };
    crate::task::validate_active_issue_with_runner(root, config.issue, &config.owner, &runner)
}

fn read_journal(path: &Path) -> io::Result<JournalState> {
    if !path.exists() {
        return Ok(JournalState::default());
    }
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut state = JournalState::default();
    let mut line = Vec::new();
    loop {
        line.clear();
        let count = reader.read_until(b'\n', &mut line)?;
        if count == 0 {
            break;
        }
        if line.last() != Some(&b'\n') {
            return Err(invalid(
                "journal has an incomplete tail; refusing to launch",
            ));
        }
        let event: Event = serde_json::from_slice(&line[..line.len() - 1])
            .map_err(|_| invalid("journal is corrupt; refusing to launch"))?;
        match event {
            Event::Starting { attempt, job_name } => {
                if state.pending.is_some()
                    || state.completed
                    || attempt != state.attempts + 1
                    || job_name.is_empty()
                {
                    return Err(invalid("journal sequence is invalid; refusing to launch"));
                }
                state.pending = Some((attempt, job_name));
                state.attempts = attempt;
            }
            Event::Finished {
                attempt, outcome, ..
            } => {
                if state.pending.as_ref().map(|(current, _)| *current) != Some(attempt) {
                    return Err(invalid(
                        "journal completion has no matching start; refusing to launch",
                    ));
                }
                state.pending = None;
                if outcome == Outcome::Completed {
                    state.completed = true;
                }
            }
            Event::Checkpoint { .. } => {}
        }
    }
    Ok(state)
}

fn append_event(path: &Path, event: &Event) -> io::Result<()> {
    let mut file = OpenOptions::new().append(true).create(true).open(path)?;
    serde_json::to_writer(&mut file, event).map_err(json_error)?;
    file.write_all(b"\n")?;
    file.sync_all()
}

fn append_checkpoint(path: &Path, checkpoint: Checkpoint) -> io::Result<()> {
    append_event(
        path,
        &Event::Checkpoint {
            checkpoint: Box::new(checkpoint),
        },
    )
}

fn validate_result(path: &Path, config: &Config) -> io::Result<WorkerStatus> {
    let result: WorkerResult = read_json(path)?;
    if result.protocol_version != 1
        || result.issue != config.issue
        || result.owner != config.owner
        || result.run_id != config.run_id
        || !matches!(result.status.as_str(), "completed" | "blocked")
        || result.summary.len() > 16_384
    {
        return Err(invalid("worker result does not match the run protocol"));
    }
    Ok(if result.status == "blocked" {
        WorkerStatus::Blocked
    } else {
        WorkerStatus::Completed
    })
}

fn checkpoint(
    config: &Config,
    directory: &Path,
    last_action: &str,
    next_step: &str,
    issue_state: &str,
    remote_branch: &str,
) -> Checkpoint {
    let head_sha = git_text(
        directory,
        &config.worktree,
        &["rev-parse", "--verify", "HEAD"],
    );
    let uncommitted_work = git_text(directory, &config.worktree, &["status", "--porcelain"])
        .map(|status| {
            if status.is_empty() {
                "none".to_owned()
            } else {
                status
            }
        })
        .unwrap_or_else(|| "unknown".to_owned());
    let unpushed_commits = git_text(
        directory,
        &config.worktree,
        &["rev-list", "--count", "@{upstream}..HEAD"],
    )
    .map(|count| count.trim().to_owned())
    .unwrap_or_else(|| "unknown".to_owned());
    Checkpoint {
        run_id: config.run_id.clone(),
        issue: config.issue,
        owner: config.owner.clone(),
        branch: config.branch.clone(),
        worktree_path: config.worktree.display().to_string(),
        base_sha: config.base_sha.clone(),
        head_sha,
        pull_request: "none or not queried".to_owned(),
        checks: "unknown".to_owned(),
        last_action: last_action.to_owned(),
        timestamp_unix_secs: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        next_step: next_step.to_owned(),
        uncommitted_work,
        unpushed_commits,
        token_usage: "unknown".to_owned(),
        context_usage: "unknown".to_owned(),
        issue_state: issue_state.to_owned(),
        remote_branch: remote_branch.to_owned(),
    }
}

fn git_text(directory: &Path, cwd: &Path, args: &[&str]) -> Option<String> {
    bounded_output("git", args, cwd, directory)
        .ok()
        .filter(|output| output.success)
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_owned())
}

#[cfg(windows)]
fn reconcile(config: &Config, paths: &Paths) -> io::Result<()> {
    let issue = config.issue.to_string();
    let issue_args = [
        "issue",
        "view",
        &issue,
        "--repo",
        &config.repository,
        "--json",
        "state,labels",
    ];
    let output = bounded_output("gh", &issue_args, &config.worktree, &paths.directory)?;
    if !output.success {
        return Err(invalid("could not reconcile issue state with GitHub"));
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(json_error)?;
    if value["state"].as_str() != Some("OPEN") {
        return Err(invalid("issue is no longer open; refusing to resume"));
    }
    if value["labels"].as_array().is_some_and(|labels| {
        labels.iter().any(|label| {
            label["name"]
                .as_str()
                .is_some_and(|name| name.eq_ignore_ascii_case("blocked"))
        })
    }) {
        return Err(invalid("issue is blocked; refusing to resume"));
    }
    let branch = format!("refs/heads/{}", config.branch);
    let remote = bounded_output(
        "git",
        &["ls-remote", "--heads", "origin", &config.branch],
        &config.worktree,
        &paths.directory,
    )?;
    if !remote.success {
        return Err(invalid("could not reconcile remote branch state"));
    }
    let remote_text = String::from_utf8_lossy(&remote.stdout);
    if remote_text.lines().any(|line| line.ends_with(&branch)) {
        let local = bounded_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            &config.worktree,
            &paths.directory,
        )?;
        if !local.success
            || remote_text
                .lines()
                .find(|line| line.ends_with(&branch))
                .and_then(|line| line.split_whitespace().next())
                != Some(String::from_utf8_lossy(&local.stdout).trim())
        {
            return Err(invalid(
                "remote branch differs from the assigned local branch; refusing to resume",
            ));
        }
    }
    let prs_args = [
        "pr",
        "list",
        "--repo",
        &config.repository,
        "--head",
        &config.branch,
        "--state",
        "all",
        "--json",
        "number,state,url,statusCheckRollup",
    ];
    let prs = bounded_output("gh", &prs_args, &config.worktree, &paths.directory)?;
    if !prs.success {
        return Err(invalid("could not reconcile pull request state"));
    }
    let prs_json: Vec<serde_json::Value> =
        serde_json::from_slice(&prs.stdout).map_err(json_error)?;
    let prs_evidence = if prs_json.is_empty() {
        "none found".to_owned()
    } else {
        serde_json::to_string(&prs_json).map_err(json_error)?
    };
    let mut remote_checkpoint = checkpoint(
        config,
        &paths.directory,
        "resume reconciled remote state",
        "continue only if issue, branch and PR state are safe",
        value["state"].as_str().unwrap_or("unknown"),
        if remote_text.trim().is_empty() {
            "absent"
        } else {
            "present; matches local head"
        },
    );
    remote_checkpoint.pull_request = prs_evidence.clone();
    remote_checkpoint.checks = prs_json
        .iter()
        .map(|pr| {
            serde_json::to_string(&pr["statusCheckRollup"]).unwrap_or_else(|_| "unknown".into())
        })
        .collect::<Vec<_>>()
        .join(", ");
    if remote_checkpoint.checks.is_empty() {
        remote_checkpoint.checks = "no PR checks queried".to_owned();
    }
    append_checkpoint(&paths.events, remote_checkpoint)?;
    if !prs_json.is_empty() {
        append_checkpoint(
            &paths.events,
            checkpoint(
                config,
                &paths.directory,
                "existing pull request discovered",
                &prs_evidence,
                value["state"].as_str().unwrap_or("unknown"),
                "present",
            ),
        )?;
        return Err(invalid(
            "a pull request already exists for the assigned branch; refusing to resume",
        ));
    }
    Ok(())
}

fn job_name(config: &Config, attempt: u32) -> String {
    let safe_run = config
        .run_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("Local\\SysJustRQUI-{}-{safe_run}-{attempt}", config.issue)
}

#[cfg(windows)]
fn validate_worker(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(invalid("--worker must be an absolute executable path"));
    }
    let canonical = fs::canonicalize(path).map_err(|_| invalid("--worker does not exist"))?;
    if !canonical.is_file() {
        return Err(invalid("--worker must name a regular file"));
    }
    Ok(canonical)
}

fn parse_positive(value: &str, flag: &str) -> io::Result<u64> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| invalid(format!("{flag} must be a positive integer")))?;
    if parsed == 0 {
        return Err(invalid(format!("{flag} must be a positive integer")));
    }
    Ok(parsed)
}

fn validate_owner(owner: String) -> io::Result<String> {
    crate::worktree::validate_owner_name(&owner)?;
    Ok(owner)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> io::Result<T> {
    serde_json::from_slice(&fs::read(path)?).map_err(json_error)
}

fn json_error(_: serde_json::Error) -> io::Error {
    invalid("invalid harness JSON")
}
#[cfg(not(windows))]
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "UnsupportedContainment: real worker launches require Windows Job Objects",
    )
}
fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
fn usage() -> io::Error {
    invalid(
        "usage: cargo xtask harness <run-once|resume> --issue N --owner NAME --worker ABS_EXE --timeout-secs N --max-attempts N | harness cancel --issue N --owner NAME",
    )
}

#[cfg(windows)]
mod windows_job {
    use super::{CLEANUP_TIMEOUT, Config, Outcome};
    use std::ffi::OsStr;
    use std::fs;
    use std::io;
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr::{null, null_mut};
    use std::thread;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, GetLastError, HANDLE, SetLastError,
    };
    use windows_sys::Win32::System::JobObjects::{
        CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation, OpenJobObjectW,
        QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows_sys::Win32::System::SystemServices::{JOB_OBJECT_QUERY, JOB_OBJECT_TERMINATE};
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
        GetCurrentProcessId, GetExitCodeProcess, InitializeProcThreadAttributeList,
        PROC_THREAD_ATTRIBUTE_JOB_LIST, PROCESS_INFORMATION, STARTUPINFOEXW,
        UpdateProcThreadAttribute, WaitForSingleObject,
    };

    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    pub(super) fn current_session_id() -> io::Result<u32> {
        let mut session = 0;
        if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) } == 0 {
            return Err(last_error());
        }
        Ok(session)
    }

    pub(super) fn recover(name: &str) -> io::Result<()> {
        let name = wide(name);
        let handle =
            unsafe { OpenJobObjectW(JOB_OBJECT_QUERY | JOB_OBJECT_TERMINATE, 0, name.as_ptr()) };
        if handle.is_null() {
            let error = unsafe { GetLastError() };
            if error == ERROR_FILE_NOT_FOUND {
                return Ok(());
            }
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        let job = Handle(handle);
        terminate_and_confirm(job.0)
    }

    pub(super) fn execute(
        config: &Config,
        attempt: u32,
        job_name: &str,
        result_path: &Path,
        cancel_path: &Path,
    ) -> io::Result<(Outcome, Option<i32>)> {
        let name = wide(job_name);
        unsafe { SetLastError(0) };
        let job = unsafe { CreateJobObjectW(null(), name.as_ptr()) };
        if job.is_null() {
            return Err(last_error());
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(job) };
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "named worker Job already exists; recovery required",
            ));
        }
        let job = Handle(job);
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &mut limits as *mut _ as _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            return Err(last_error());
        }

        let mut attribute_size = 0usize;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut attribute_size);
        }
        let words = attribute_size.div_ceil(size_of::<usize>());
        let mut attributes = vec![0usize; words];
        let list = attributes.as_mut_ptr() as *mut _;
        if unsafe { InitializeProcThreadAttributeList(list, 1, 0, &mut attribute_size) } == 0 {
            return Err(last_error());
        }
        struct AttributeList(*mut std::ffi::c_void);
        impl Drop for AttributeList {
            fn drop(&mut self) {
                unsafe {
                    DeleteProcThreadAttributeList(self.0);
                }
            }
        }
        let _attributes = AttributeList(list);
        let mut job_handle = job.0;
        if unsafe {
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                &mut job_handle as *mut _ as _,
                size_of::<HANDLE>(),
                null_mut(),
                null_mut(),
            )
        } == 0
        {
            return Err(last_error());
        }

        let args = [
            "--work-package".to_owned(),
            config.package.display().to_string(),
            "--result".to_owned(),
            result_path.display().to_string(),
            "--run-id".to_owned(),
            config.run_id.clone(),
            "--issue".to_owned(),
            config.issue.to_string(),
            "--owner".to_owned(),
            config.owner.clone(),
            "--attempt".to_owned(),
            attempt.to_string(),
        ];
        let command_line = quote_command_line(&config.worker, &args);
        let mut command_line = wide(&command_line);
        let cwd = wide(&config.worktree.to_string_lossy());
        let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.lpAttributeList = list;
        let mut process: PROCESS_INFORMATION = unsafe { zeroed() };
        let success = unsafe {
            CreateProcessW(
                config
                    .worker
                    .as_os_str()
                    .encode_wide()
                    .chain(Some(0))
                    .collect::<Vec<_>>()
                    .as_ptr(),
                command_line.as_mut_ptr(),
                null(),
                null(),
                0,
                EXTENDED_STARTUPINFO_PRESENT,
                null(),
                cwd.as_ptr(),
                &startup.StartupInfo,
                &mut process,
            )
        };
        if success == 0 {
            return Err(last_error());
        }
        let root_process = Handle(process.hProcess);
        let root_thread = Handle(process.hThread);
        drop(root_thread);
        let timeout = Duration::from_secs(config.timeout_secs);
        let deadline = Instant::now() + timeout;
        let mut outcome = Outcome::Completed;
        let mut root_exited = false;
        loop {
            if cancel_path.exists() {
                outcome = Outcome::Cancelled;
                break;
            }
            let now = Instant::now();
            if now >= deadline {
                outcome = Outcome::TimedOut;
                break;
            }
            if root_exited {
                if active_processes(job.0)? == 0 {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            } else {
                let wait_ms = deadline
                    .saturating_duration_since(now)
                    .min(Duration::from_millis(100))
                    .as_millis() as u32;
                let wait = unsafe { WaitForSingleObject(root_process.0, wait_ms) };
                if wait == windows_sys::Win32::Foundation::WAIT_OBJECT_0 {
                    root_exited = true;
                    if active_processes(job.0)? == 0 {
                        break;
                    }
                } else if wait != windows_sys::Win32::Foundation::WAIT_TIMEOUT {
                    return Err(last_error());
                }
            }
        }
        if active_processes(job.0)? != 0
            && unsafe { TerminateJobObject(job.0, 1) } == 0
            && active_processes(job.0)? != 0
        {
            return Err(io::Error::other(
                "StopUnconfirmed: TerminateJobObject failed",
            ));
        }
        wait_empty(job.0, CLEANUP_TIMEOUT)?;
        let mut exit_code = 0u32;
        let exit_code = if unsafe { GetExitCodeProcess(root_process.0, &mut exit_code) } != 0 {
            Some(exit_code as i32)
        } else {
            None
        };
        if outcome == Outcome::Completed && exit_code != Some(0) {
            outcome = Outcome::Failed;
        }
        let _ = fs::metadata(result_path);
        Ok((outcome, exit_code))
    }

    fn terminate_and_confirm(job: HANDLE) -> io::Result<()> {
        if active_processes(job)? == 0 {
            return Ok(());
        }
        if unsafe { TerminateJobObject(job, 1) } == 0 && active_processes(job)? != 0 {
            return Err(last_error());
        }
        wait_empty(job, CLEANUP_TIMEOUT)
    }

    fn wait_empty(job: HANDLE, timeout: Duration) -> io::Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            if active_processes(job)? == 0 {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "StopUnconfirmed: worker job did not reach zero active processes",
                ));
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn active_processes(job: HANDLE) -> io::Result<u32> {
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { zeroed() };
        if unsafe {
            QueryInformationJobObject(
                job,
                JobObjectBasicAccountingInformation,
                &mut info as *mut _ as _,
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                null_mut(),
            )
        } == 0
        {
            return Err(last_error());
        }
        Ok(info.ActiveProcesses)
    }

    fn quote_command_line(executable: &Path, args: &[String]) -> String {
        std::iter::once(executable.to_string_lossy().into_owned())
            .chain(args.iter().cloned())
            .map(|arg| quote_arg(&arg))
            .collect::<Vec<_>>()
            .join(" ")
    }
    fn quote_arg(arg: &str) -> String {
        if !arg.is_empty() && !arg.chars().any(|ch| ch.is_whitespace() || ch == '"') {
            return arg.to_owned();
        }
        let mut out = String::from("\"");
        let mut slashes = 0;
        for ch in arg.chars() {
            match ch {
                '\\' => slashes += 1,
                '"' => {
                    out.push_str(&"\\".repeat(slashes * 2 + 1));
                    out.push('"');
                    slashes = 0;
                }
                _ => {
                    out.push_str(&"\\".repeat(slashes));
                    slashes = 0;
                    out.push(ch);
                }
            }
        }
        out.push_str(&"\\".repeat(slashes * 2));
        out.push('"');
        out
    }
    fn wide(value: &str) -> Vec<u16> {
        OsStr::new(value).encode_wide().chain(Some(0)).collect()
    }
    fn last_error() -> io::Error {
        io::Error::from_raw_os_error(unsafe { GetLastError() } as i32)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::harness::validate_result;
        use std::path::PathBuf;
        use std::process::Command;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
        use windows_sys::Win32::System::Threading::STARTUPINFOW;

        static NEXT: AtomicUsize = AtomicUsize::new(0);

        struct TempDir(PathBuf);
        impl TempDir {
            fn new() -> Self {
                let id = NEXT.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir()
                    .join(format!("sysrqui-job-test-{}-{id}", std::process::id()));
                fs::create_dir_all(&path).unwrap();
                Self(path)
            }
        }
        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        fn worker(dir: &Path) -> PathBuf {
            let source = dir.join("worker.rs");
            let exe = dir.join("worker.exe");
            fs::write(
                &source,
                r##"
use std::{env, fs, process::Command, thread, time::Duration};
fn value(args: &[String], name: &str) -> String {
    args.iter().position(|arg| arg == name).map(|i| args[i + 1].clone()).unwrap()
}
fn main() {
    let args: Vec<String> = env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "--child") {
        thread::sleep(Duration::from_secs(60));
        return;
    }
    let run_id = value(&args, "--run-id");
    if run_id == "wait-for-cancel" {
        thread::sleep(Duration::from_secs(60));
        return;
    }
    if run_id == "spawn-descendant" {
        Command::new(env::current_exe().unwrap()).arg("--child").spawn().unwrap();
    }
    let result = value(&args, "--result");
    let issue = value(&args, "--issue");
    let owner = value(&args, "--owner");
    fs::write(result, format!(r#"{{"protocol_version":1,"issue":{issue},"owner":"{owner}","run_id":"{run_id}","status":"completed","summary":"ok"}}"#)).unwrap();
}
"##,
            )
            .unwrap();
            let status = Command::new("rustc")
                .arg(source.as_os_str())
                .arg("-o")
                .arg(exe.as_os_str())
                .status()
                .expect("rustc should be available on the Windows CI host");
            assert!(status.success(), "test worker compilation failed");
            exe
        }

        fn config(dir: &Path, run_id: &str, timeout_secs: u64) -> Config {
            Config {
                issue: 19,
                owner: "Codex".into(),
                repository: "bonaparta/SysJustRQUI".into(),
                run_id: run_id.into(),
                session_id: current_session_id().unwrap(),
                branch: "codex/issue-19".into(),
                base_sha: "base".into(),
                worktree: dir.into(),
                worker: dir.join("worker.exe"),
                timeout_secs,
                max_attempts: 1,
                package: dir.join("package.json"),
            }
        }

        #[test]
        fn immediate_successful_worker_completes() {
            let temp = TempDir::new();
            let worker = worker(&temp.0);
            let mut config = config(&temp.0, "19-run", 3);
            config.worker = worker;
            let result = temp.0.join("result.json");
            let started = Instant::now();
            let (outcome, code) = super::execute(
                &config,
                1,
                "Local\\sysrqui-success-test",
                &result,
                &temp.0.join("cancel"),
            )
            .unwrap();
            assert_eq!(outcome, Outcome::Completed);
            assert_eq!(code, Some(0));
            assert!(started.elapsed() < Duration::from_secs(3));
            validate_result(&result, &config).unwrap();
        }

        #[test]
        fn root_exit_with_live_descendant_is_timed_out_and_tree_is_stopped() {
            let temp = TempDir::new();
            let worker = worker(&temp.0);
            let mut config = config(&temp.0, "spawn-descendant", 2);
            config.worker = worker;
            let result = temp.0.join("result.json");
            let (outcome, code) = super::execute(
                &config,
                1,
                "Local\\sysrqui-descendant-test",
                &result,
                &temp.0.join("cancel"),
            )
            .unwrap();
            assert_eq!(outcome, Outcome::TimedOut);
            assert_eq!(code, Some(0));
        }

        #[test]
        fn cancellation_terminates_the_worker_job() {
            let temp = TempDir::new();
            let worker = worker(&temp.0);
            let mut config = config(&temp.0, "wait-for-cancel", 30);
            config.worker = worker;
            let cancel = temp.0.join("cancel");
            fs::write(&cancel, "cancel").unwrap();
            let (outcome, _) = super::execute(
                &config,
                1,
                "Local\\sysrqui-cancel-test",
                &temp.0.join("result.json"),
                &cancel,
            )
            .unwrap();
            assert_eq!(outcome, Outcome::Cancelled);
        }

        #[test]
        fn recovery_of_existing_empty_job_succeeds() {
            let name = wide("Local\\sysrqui-recovery-empty-test");
            let handle = unsafe { CreateJobObjectW(null(), name.as_ptr()) };
            assert!(!handle.is_null());
            let _handle = Handle(handle);
            recover("Local\\sysrqui-recovery-empty-test").unwrap();
        }

        #[test]
        fn recovery_terminates_an_existing_job_process_tree() {
            let temp = TempDir::new();
            let worker = worker(&temp.0);
            let name_text = format!("Local\\sysrqui-recovery-active-{}", std::process::id());
            let name = wide(&name_text);
            let job = unsafe { CreateJobObjectW(null(), name.as_ptr()) };
            assert!(!job.is_null());
            let _job = Handle(job);
            let mut startup: STARTUPINFOW = unsafe { zeroed() };
            startup.cb = size_of::<STARTUPINFOW>() as u32;
            let mut process: PROCESS_INFORMATION = unsafe { zeroed() };
            let mut command = wide(&format!("\"{}\" --child", worker.display()));
            let exe = worker
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>();
            let cwd = wide(&temp.0.to_string_lossy());
            assert_ne!(
                unsafe {
                    CreateProcessW(
                        exe.as_ptr(),
                        command.as_mut_ptr(),
                        null(),
                        null(),
                        0,
                        0,
                        null(),
                        cwd.as_ptr(),
                        &startup,
                        &mut process,
                    )
                },
                0
            );
            let child = Handle(process.hProcess);
            let _thread = Handle(process.hThread);
            assert_ne!(unsafe { AssignProcessToJobObject(job, child.0) }, 0);
            assert_eq!(active_processes(job).unwrap(), 1);
            recover(&name_text).unwrap();
            assert_eq!(active_processes(job).unwrap(), 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Config, Event, Executor, Outcome, Paths, append_event, job_name, parse_args,
        paths_for_directory, read_journal, run_engine_paths, validate_result,
    };
    use std::cell::RefCell;
    use std::fs::{self, File};
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("sysrqui-harness-test-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn config(path: &Path) -> Config {
        Config {
            issue: 19,
            owner: "Codex".into(),
            repository: "bonaparta/SysJustRQUI".into(),
            run_id: "19-run".into(),
            session_id: 0,
            branch: "codex/issue-19".into(),
            base_sha: "base".into(),
            worktree: path.into(),
            worker: path.join("worker.exe"),
            timeout_secs: 10,
            max_attempts: 3,
            package: path.join("package.json"),
        }
    }

    fn run_paths(path: &Path) -> Paths {
        let directory = path.join("run");
        fs::create_dir_all(&directory).unwrap();
        paths_for_directory(directory)
    }

    #[test]
    fn parses_bounded_commands_and_rejects_shell_arguments() {
        assert!(
            parse_args(
                [
                    "run-once",
                    "--issue",
                    "19",
                    "--owner",
                    "Codex",
                    "--worker",
                    "C:/worker.exe",
                    "--timeout-secs",
                    "30",
                    "--max-attempts",
                    "2"
                ]
                .into_iter()
                .map(str::to_owned)
                .collect()
            )
            .is_ok()
        );
        assert!(
            parse_args(
                [
                    "run-once",
                    "--issue",
                    "19",
                    "--owner",
                    "Codex",
                    "--worker",
                    "worker.exe",
                    "--timeout-secs",
                    "30",
                    "--max-attempts",
                    "2",
                    "--command",
                    "whoami"
                ]
                .into_iter()
                .map(str::to_owned)
                .collect()
            )
            .is_err()
        );
    }

    #[test]
    fn stale_probe_output_does_not_block_a_later_process() {
        let temp = TempDir::new();
        fs::write(temp.0.join("probe-0.stdout"), "left by a prior process").unwrap();
        let output = super::bounded_output("git", &["--version"], &temp.0, &temp.0).unwrap();
        assert!(output.success);
        assert!(String::from_utf8_lossy(&output.stdout).contains("git version"));
        assert_eq!(
            fs::read_to_string(temp.0.join("probe-0.stdout")).unwrap(),
            "left by a prior process"
        );
    }

    #[test]
    fn journal_is_append_only_reconciles_attempts_and_fails_closed_on_torn_tail() {
        let temp = TempDir::new();
        let path = temp.0.join("events.jsonl");
        append_event(
            &path,
            &Event::Starting {
                attempt: 1,
                job_name: "Local\\job-1".into(),
            },
        )
        .unwrap();
        append_event(
            &path,
            &Event::Finished {
                attempt: 1,
                outcome: Outcome::Failed,
                exit_code: Some(1),
            },
        )
        .unwrap();
        append_event(
            &path,
            &Event::Starting {
                attempt: 2,
                job_name: "Local\\job-2".into(),
            },
        )
        .unwrap();
        let state = read_journal(&path).unwrap();
        assert_eq!(state.attempts, 2);
        assert_eq!(state.pending, Some((2, "Local\\job-2".into())));
        use std::io::Write;
        File::options()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{broken")
            .unwrap();
        assert!(
            read_journal(&path)
                .unwrap_err()
                .to_string()
                .contains("incomplete tail")
        );
    }

    struct FakeExecutor {
        outcomes: RefCell<Vec<(Outcome, Option<i32>)>>,
        calls: RefCell<Vec<u32>>,
        recovered: RefCell<Vec<String>>,
        recovery_error: bool,
        write_cancel: bool,
        result_status: String,
    }
    impl Executor for FakeExecutor {
        fn recover(&self, name: &str) -> io::Result<()> {
            self.recovered.borrow_mut().push(name.into());
            if self.recovery_error {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "unknown job state",
                ))
            } else {
                Ok(())
            }
        }
        fn execute(
            &self,
            _config: &Config,
            attempt: u32,
            _job: &str,
            result: &Path,
            cancel: &Path,
        ) -> io::Result<(Outcome, Option<i32>)> {
            self.calls.borrow_mut().push(attempt);
            let outcome = self.outcomes.borrow_mut().remove(0);
            if outcome.0 == Outcome::Completed {
                fs::write(
                    result,
                    format!(
                        r#"{{"protocol_version":1,"issue":19,"owner":"Codex","run_id":"19-run","status":"{}","summary":"worker needs a decision"}}"#,
                        self.result_status
                    ),
                )?;
            }
            if self.write_cancel {
                fs::write(cancel, "cancel\n")?;
            }
            Ok(outcome)
        }
    }

    fn fake(outcomes: &[(Outcome, Option<i32>)]) -> FakeExecutor {
        FakeExecutor {
            outcomes: RefCell::new(outcomes.to_vec()),
            calls: RefCell::new(Vec::new()),
            recovered: RefCell::new(Vec::new()),
            recovery_error: false,
            write_cancel: false,
            result_status: "completed".into(),
        }
    }

    #[test]
    fn success_is_recorded_once_and_leaves_dirty_worktree_untouched() {
        let temp = TempDir::new();
        let config = config(&temp.0);
        let dirty = config.worktree.join("worker-edit.txt");
        fs::write(&dirty, "preserve me\n").unwrap();
        let paths = run_paths(&temp.0);
        let executor = fake(&[(Outcome::Completed, Some(0))]);
        run_engine_paths(None, &paths, &config, false, &executor).unwrap();
        run_engine_paths(None, &paths, &config, true, &executor).unwrap();
        assert_eq!(&*executor.calls.borrow(), &[1]);
        assert_eq!(fs::read_to_string(dirty).unwrap(), "preserve me\n");
        assert!(read_journal(&paths.events).unwrap().completed);
    }

    #[test]
    fn blocked_result_stops_without_retry_and_records_worker_reason() {
        let temp = TempDir::new();
        let config = config(&temp.0);
        let paths = run_paths(&temp.0);
        let mut executor = fake(&[(Outcome::Completed, Some(0)), (Outcome::Completed, Some(0))]);
        executor.result_status = "blocked".into();
        let error = run_engine_paths(None, &paths, &config, true, &executor).unwrap_err();
        assert!(error.to_string().contains("Blocked"));
        assert_eq!(&*executor.calls.borrow(), &[1]);
        let journal = fs::read_to_string(&paths.events).unwrap();
        assert!(journal.contains("worker needs a decision"));
        assert!(journal.contains("\"outcome\":\"blocked\""));
    }

    #[test]
    fn resume_consumes_total_attempts_and_can_complete_after_failure() {
        let temp = TempDir::new();
        let mut config = config(&temp.0);
        config.max_attempts = 2;
        let paths = run_paths(&temp.0);
        let first = fake(&[(Outcome::Failed, Some(1))]);
        assert!(run_engine_paths(None, &paths, &config, false, &first).is_err());
        assert_eq!(&*first.calls.borrow(), &[1]);

        let second = fake(&[(Outcome::Completed, Some(0))]);
        run_engine_paths(None, &paths, &config, true, &second).unwrap();
        assert_eq!(&*second.calls.borrow(), &[2]);
        assert!(read_journal(&paths.events).unwrap().completed);
    }

    #[test]
    fn exhausted_attempts_do_not_duplicate_workers_and_timeout_can_resume() {
        for (outcome, max_attempts) in [(Outcome::Failed, 1), (Outcome::TimedOut, 3)] {
            let temp = TempDir::new();
            let mut config = config(&temp.0);
            config.max_attempts = max_attempts;
            let paths = run_paths(&temp.0);
            let executor = fake(&[(outcome.clone(), Some(1)), (Outcome::Completed, Some(0))]);
            let result = run_engine_paths(None, &paths, &config, false, &executor);
            assert!(result.is_err(), "outcome: {outcome:?}");
            if outcome == Outcome::Failed {
                assert!(run_engine_paths(None, &paths, &config, true, &executor).is_err());
                assert_eq!(&*executor.calls.borrow(), &[1], "exhausted");
            } else {
                run_engine_paths(None, &paths, &config, true, &executor).unwrap();
                assert_eq!(&*executor.calls.borrow(), &[1, 2], "timeout resume");
            }
        }
    }

    #[test]
    fn explicit_resume_acknowledges_cancel_and_preserves_attempt_count() {
        let temp = TempDir::new();
        let mut config = config(&temp.0);
        config.max_attempts = 2;
        let paths = run_paths(&temp.0);
        let first = FakeExecutor {
            outcomes: RefCell::new(vec![(Outcome::Cancelled, None)]),
            calls: RefCell::new(Vec::new()),
            recovered: RefCell::new(Vec::new()),
            recovery_error: false,
            write_cancel: true,
            result_status: "completed".into(),
        };
        assert!(run_engine_paths(None, &paths, &config, false, &first).is_err());
        assert!(paths.cancel.exists());
        let second = fake(&[(Outcome::Completed, Some(0))]);
        run_engine_paths(None, &paths, &config, true, &second).unwrap();
        assert_eq!(&*first.calls.borrow(), &[1]);
        assert_eq!(&*second.calls.borrow(), &[2]);
        assert!(!paths.cancel.exists());
        assert_eq!(
            fs::read_dir(&paths.directory)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("cancel-acknowledged-"))
                .count(),
            1
        );
    }

    #[test]
    fn cancel_marker_prevents_launch_and_duplicate_lock_is_refused() {
        let temp = TempDir::new();
        let config = config(&temp.0);
        let paths = run_paths(&temp.0);
        fs::write(&paths.cancel, "cancel\n").unwrap();
        let executor = fake(&[(Outcome::Completed, Some(0))]);
        assert!(run_engine_paths(None, &paths, &config, false, &executor).is_err());
        assert!(executor.calls.borrow().is_empty());

        fs::remove_file(&paths.cancel).unwrap();
        let held_lock = File::options()
            .read(true)
            .write(true)
            .truncate(false)
            .create(true)
            .open(&paths.lock)
            .unwrap();
        held_lock.try_lock().unwrap();
        assert!(
            run_engine_paths(None, &paths, &config, false, &executor)
                .unwrap_err()
                .to_string()
                .contains("another harness process")
        );
        assert!(executor.calls.borrow().is_empty());
    }

    #[test]
    fn recovery_failure_refuses_to_start_another_worker() {
        let temp = TempDir::new();
        let config = config(&temp.0);
        let paths = run_paths(&temp.0);
        append_event(
            &paths.events,
            &Event::Starting {
                attempt: 1,
                job_name: "Local\\prior-attempt".into(),
            },
        )
        .unwrap();
        let executor = FakeExecutor {
            outcomes: RefCell::new(vec![(Outcome::Completed, Some(0))]),
            calls: RefCell::new(Vec::new()),
            recovered: RefCell::new(Vec::new()),
            recovery_error: true,
            write_cancel: false,
            result_status: "completed".into(),
        };
        assert!(
            run_engine_paths(None, &paths, &config, true, &executor)
                .unwrap_err()
                .to_string()
                .contains("StopUnconfirmed")
        );
        assert!(executor.calls.borrow().is_empty());
        assert_eq!(
            executor.recovered.borrow().as_slice(),
            ["Local\\prior-attempt"]
        );
    }

    #[test]
    fn results_are_bound_to_issue_owner_and_run_id() {
        let temp = TempDir::new();
        let config = config(&temp.0);
        let result = temp.0.join("result.json");
        fs::write(&result, r#"{"protocol_version":1,"issue":19,"owner":"Codex","run_id":"19-run","status":"completed","summary":"ok"}"#).unwrap();
        validate_result(&result, &config).unwrap();
        fs::write(&result, r#"{"protocol_version":1,"issue":20,"owner":"Codex","run_id":"19-run","status":"completed","summary":"ok"}"#).unwrap();
        assert!(validate_result(&result, &config).is_err());
    }

    #[test]
    fn named_job_is_unique_per_attempt() {
        let temp = TempDir::new();
        let config = config(&temp.0);
        assert_ne!(job_name(&config, 1), job_name(&config, 2));
    }
}
