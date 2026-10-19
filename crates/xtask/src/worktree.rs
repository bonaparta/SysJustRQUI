use serde::{Deserialize, Serialize};
use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
enum Operation {
    Create {
        issue: u64,
        owner: String,
        root: Option<PathBuf>,
    },
    Status {
        issue: u64,
    },
    Remove {
        issue: u64,
        owner: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    version: u32,
    issue: u64,
    owner: String,
    run_id: String,
    branch: String,
    base_sha: String,
    worktree_root: PathBuf,
    path: PathBuf,
    state: String,
    head_sha: Option<String>,
}

#[derive(Debug)]
struct RepoContext {
    main_root: PathBuf,
    registry_dir: PathBuf,
}

#[derive(Debug, Clone)]
struct WorktreeEntry {
    path: PathBuf,
    head: Option<String>,
    branch: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveWorktree {
    pub(crate) issue: u64,
    pub(crate) owner: String,
    pub(crate) run_id: String,
    pub(crate) branch: String,
    pub(crate) base_sha: String,
    pub(crate) head_sha: String,
    pub(crate) path: PathBuf,
}

pub fn run(args: Vec<String>) -> io::Result<()> {
    let operation = parse_args(args)?;
    let cwd = env::current_dir()?;

    match operation {
        Operation::Create { issue, owner, root } => {
            let manifest = create(&cwd, issue, &owner, root.as_deref())?;
            println!(
                "Worktree ready: issue #{}, owner {}, run {}, branch {}, path {}.",
                manifest.issue,
                manifest.owner,
                manifest.run_id,
                manifest.branch,
                manifest.path.display()
            );
        }
        Operation::Status { issue } => {
            let (manifest, head) = status(&cwd, issue)?;
            println!("Issue: #{}", manifest.issue);
            println!("Owner: {}", manifest.owner);
            println!("Run ID: {}", manifest.run_id);
            println!("State: {}", manifest.state);
            println!("Branch: {}", manifest.branch);
            println!("Path: {}", manifest.path.display());
            println!("Base SHA: {}", manifest.base_sha);
            println!(
                "Current head SHA: {}",
                head.as_deref().unwrap_or("unavailable")
            );
        }
        Operation::Remove { issue, owner } => {
            let state = remove(&cwd, issue, &owner)?;
            println!("Issue #{issue} worktree state: {state}.");
        }
    }

    Ok(())
}

fn parse_args(args: Vec<String>) -> io::Result<Operation> {
    let mut args = args.into_iter();
    let operation = args
        .next()
        .ok_or_else(|| invalid("usage: cargo xtask worktree <create|status|remove> ..."))?;

    let mut issue = None;
    let mut owner = None;
    let mut root = None;
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| invalid(format!("missing value for {flag}")))?;
        match flag.as_str() {
            "--issue" if issue.is_none() => {
                let parsed = value
                    .parse::<u64>()
                    .map_err(|_| invalid("--issue must be a positive integer"))?;
                if parsed == 0 {
                    return Err(invalid("--issue must be a positive integer"));
                }
                issue = Some(parsed);
            }
            "--owner" if owner.is_none() => {
                validate_owner(&value)?;
                owner = Some(value);
            }
            "--root" if root.is_none() => root = Some(PathBuf::from(value)),
            "--issue" | "--owner" | "--root" => {
                return Err(invalid(format!("duplicate argument {flag}")));
            }
            _ => return Err(invalid(format!("unknown argument {flag}"))),
        }
    }

    let issue = issue.ok_or_else(|| invalid("--issue is required"))?;
    match operation.as_str() {
        "create" => {
            let owner = owner.ok_or_else(|| invalid("--owner is required for create"))?;
            Ok(Operation::Create { issue, owner, root })
        }
        "status" => {
            if owner.is_some() || root.is_some() {
                return Err(invalid("status accepts only --issue"));
            }
            Ok(Operation::Status { issue })
        }
        "remove" => {
            if root.is_some() {
                return Err(invalid(
                    "remove uses the recorded root; --root is not accepted",
                ));
            }
            let owner = owner.ok_or_else(|| invalid("--owner is required for remove"))?;
            Ok(Operation::Remove { issue, owner })
        }
        _ => Err(invalid(
            "usage: cargo xtask worktree <create|status|remove> ...",
        )),
    }
}

fn validate_owner(owner: &str) -> io::Result<()> {
    if owner.is_empty()
        || owner.len() > 32
        || !owner
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(invalid(
            "--owner must be a 1–32 character ASCII identifier using letters, digits, '-' or '_'",
        ));
    }
    Ok(())
}

pub(crate) fn validate_owner_name(owner: &str) -> io::Result<()> {
    validate_owner(owner)
}

fn create(
    cwd: &Path,
    issue: u64,
    owner: &str,
    requested_root: Option<&Path>,
) -> io::Result<Manifest> {
    validate_owner(owner)?;
    let context = repo_context(cwd)?;
    let latest_sha = fetch_main(&context.main_root)?;
    ensure_registry_dirs(&context)?;
    let worktree_root = resolve_worktree_root(&context, requested_root)?;
    validate_worktree_root(&context.main_root, &worktree_root)?;

    let branch = branch_name(issue);
    let path = worktree_root.join(format!("issue-{issue}"));
    let manifest_path = manifest_path(&context, issue);
    if manifest_path.exists() {
        let mut manifest = read_manifest(&manifest_path, issue)?;
        validate_manifest(&manifest, issue)?;
        if manifest.owner != owner {
            return Err(invalid(format!(
                "issue #{issue} is owned by {}; refusing to reassign it",
                manifest.owner
            )));
        }
        if !same_path(&manifest.worktree_root, &worktree_root) {
            return Err(invalid(
                "issue already has a worktree recorded under a different root",
            ));
        }
        if manifest.state == "removed" || manifest.state == "branch_retained" {
            return Err(invalid(
                "issue has a prior removal record; inspect status and resolve it before creating another run",
            ));
        }
        if manifest.state != "active" && manifest.state != "creating" {
            return Err(invalid(
                "manifest has an unsupported state; manual review required",
            ));
        }

        match registered_entry(&context.main_root, &manifest.path, &manifest.branch)? {
            Some(entry) => {
                let head = verify_registered_worktree(&manifest, &entry)?;
                manifest.state = "active".to_owned();
                manifest.head_sha = Some(head);
                write_manifest(&manifest_path, &manifest)?;
                return Ok(manifest);
            }
            None => {
                if manifest.state == "active" {
                    return Err(invalid(
                        "active manifest has no matching Git worktree registration; refusing to recreate",
                    ));
                }
                if path.exists() {
                    return Err(invalid(
                        "unregistered path already exists; refusing to overwrite it",
                    ));
                }
                if branch_exists(&context.main_root, &manifest.branch)? {
                    return Err(invalid(
                        "branch exists without a matching worktree; manual review required",
                    ));
                }
                manifest.base_sha = latest_sha;
                manifest.path = path;
                create_git_worktree(&context.main_root, &manifest)?;
                let entry = registered_entry(&context.main_root, &manifest.path, &manifest.branch)?
                    .ok_or_else(|| invalid("Git did not register the created worktree"))?;
                let head = verify_registered_worktree(&manifest, &entry)?;
                manifest.state = "active".to_owned();
                manifest.head_sha = Some(head);
                write_manifest(&manifest_path, &manifest)?;
                return Ok(manifest);
            }
        }
    }

    if path.exists() {
        return Err(invalid(
            "target path already exists and is not registered; refusing to overwrite it",
        ));
    }
    if branch_exists(&context.main_root, &branch)? {
        return Err(invalid(
            "target branch already exists without a registry record; manual review required",
        ));
    }

    let run_id = new_run_id(issue)?;
    let mut manifest = Manifest {
        version: 1,
        issue,
        owner: owner.to_owned(),
        run_id,
        branch,
        base_sha: latest_sha,
        worktree_root,
        path,
        state: "creating".to_owned(),
        head_sha: None,
    };
    claim_manifest(&manifest_path, &manifest)?;
    create_git_worktree(&context.main_root, &manifest)?;
    let entry = registered_entry(&context.main_root, &manifest.path, &manifest.branch)?
        .ok_or_else(|| invalid("Git did not register the created worktree"))?;
    let head = verify_registered_worktree(&manifest, &entry)?;
    manifest.state = "active".to_owned();
    manifest.head_sha = Some(head);
    write_manifest(&manifest_path, &manifest)?;
    Ok(manifest)
}

fn status(cwd: &Path, issue: u64) -> io::Result<(Manifest, Option<String>)> {
    let context = repo_context(cwd)?;
    let manifest_path = manifest_path(&context, issue);
    let manifest = read_manifest(&manifest_path, issue)?;
    validate_manifest(&manifest, issue)?;

    let head = match manifest.state.as_str() {
        "active" => {
            let entry = registered_entry(&context.main_root, &manifest.path, &manifest.branch)?
                .ok_or_else(|| invalid("active manifest has no matching Git worktree"))?;
            Some(verify_registered_worktree(&manifest, &entry)?)
        }
        "creating" => registered_entry(&context.main_root, &manifest.path, &manifest.branch)?
            .map(|entry| verify_registered_worktree(&manifest, &entry))
            .transpose()?,
        "branch_retained" => branch_head(&context.main_root, &manifest.branch)?,
        "removed" => None,
        _ => {
            return Err(invalid(
                "manifest has an unsupported state; manual review required",
            ));
        }
    };
    Ok((manifest, head))
}

pub(crate) fn active_worktree(cwd: &Path, issue: u64, owner: &str) -> io::Result<ActiveWorktree> {
    validate_owner(owner)?;
    let context = repo_context(cwd)?;
    let manifest = read_manifest(&manifest_path(&context, issue), issue)?;
    validate_manifest(&manifest, issue)?;
    if manifest.owner != owner {
        return Err(invalid(format!(
            "issue #{issue} is assigned to {}; refusing to prepare for {owner}",
            manifest.owner
        )));
    }
    if manifest.state != "active" {
        return Err(invalid(
            "issue has no active worktree; create or recover the assigned worktree first",
        ));
    }
    let entry = registered_entry(&context.main_root, &manifest.path, &manifest.branch)?
        .ok_or_else(|| invalid("active manifest has no matching Git worktree"))?;
    let head_sha = verify_registered_worktree(&manifest, &entry)?;
    Ok(ActiveWorktree {
        issue,
        owner: manifest.owner,
        run_id: manifest.run_id,
        branch: manifest.branch,
        base_sha: manifest.base_sha,
        head_sha,
        path: manifest.path,
    })
}

pub(crate) fn local_storage_dir(cwd: &Path, area: &str) -> io::Result<PathBuf> {
    if !matches!(
        area,
        "tasks" | "verify" | "harness" | "verify-build" | "verify-build-fallback"
    ) {
        return Err(invalid("unsupported local storage area"));
    }
    let context = repo_context(cwd)?;
    let dot_loop = ensure_directory(&context.main_root, &context.main_root.join(".loop"))?;
    ensure_directory(&context.main_root, &dot_loop.join(area))
}

fn remove(cwd: &Path, issue: u64, owner: &str) -> io::Result<&'static str> {
    validate_owner(owner)?;
    let context = repo_context(cwd)?;
    let manifest_path = manifest_path(&context, issue);
    let mut manifest = read_manifest(&manifest_path, issue)?;
    validate_manifest(&manifest, issue)?;
    if manifest.owner != owner {
        return Err(invalid(format!(
            "issue #{issue} is owned by {}; refusing to remove it",
            manifest.owner
        )));
    }
    if manifest.state == "removed" {
        return Ok("already removed (registry record retained)");
    }
    if manifest.state == "branch_retained" {
        fetch_main(&context.main_root)?;
        cleanup_branch(&context, &mut manifest, &manifest_path)?;
        return if manifest.state == "removed" {
            Ok("worktree and branch removed (registry record retained)")
        } else {
            Ok("worktree removed; branch retained for safe retry")
        };
    }
    if manifest.state != "active" {
        return Err(invalid(
            "worktree is not in an active state; inspect status and resolve it manually",
        ));
    }

    fetch_main(&context.main_root)?;
    let entry = registered_entry(&context.main_root, &manifest.path, &manifest.branch)?;
    let Some(entry) = entry else {
        if manifest.path.exists() {
            return Err(invalid(
                "recorded path exists but is not registered as this Git worktree",
            ));
        }
        return recover_interrupted_remove(&context, &mut manifest, &manifest_path);
    };

    validate_target_path(&manifest)?;
    let head = verify_registered_worktree(&manifest, &entry)?;
    let branch_head = branch_head(&context.main_root, &manifest.branch)?
        .ok_or_else(|| invalid("recorded branch no longer exists"))?;
    if branch_head != head {
        return Err(invalid(
            "worktree HEAD differs from its branch tip; refusing to remove",
        ));
    }

    let dirty = git_output(
        &manifest.path,
        [
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--ignored=matching",
        ],
    )?;
    if !dirty.status.success() {
        return Err(invalid(
            "could not inspect worktree status; nothing was removed",
        ));
    }
    if !dirty.stdout.is_empty() {
        return Err(invalid(
            "worktree contains tracked, untracked, or ignored data; preserve it and resolve manually",
        ));
    }

    if !is_ancestor(&context.main_root, &head, "origin/main")? {
        return Err(invalid(
            "worktree HEAD is not merged into freshly fetched origin/main; nothing was removed",
        ));
    }

    manifest.head_sha = Some(head);
    write_manifest(&manifest_path, &manifest)?;
    git_checked(
        &context.main_root,
        [
            OsString::from("worktree"),
            OsString::from("remove"),
            OsString::from("--"),
            git_path_arg(&manifest.path),
        ],
    )
    .map_err(|error| invalid(format!("git worktree remove failed: {error}")))?;

    cleanup_branch(&context, &mut manifest, &manifest_path)?;
    if manifest.state == "removed" {
        Ok("worktree and branch removed (registry record retained)")
    } else {
        Ok("worktree removed; branch retained for safe retry")
    }
}

fn recover_interrupted_remove(
    context: &RepoContext,
    manifest: &mut Manifest,
    manifest_path: &Path,
) -> io::Result<&'static str> {
    let head = manifest.head_sha.as_deref().ok_or_else(|| {
        invalid("worktree is missing and has no saved head; manual review required")
    })?;
    let Some(branch_head) = branch_head(&context.main_root, &manifest.branch)? else {
        if is_ancestor(&context.main_root, head, "origin/main")? {
            manifest.state = "removed".to_owned();
            write_manifest(manifest_path, manifest)?;
            return Ok("interrupted removal recovered; worktree and branch are absent");
        }
        return Err(invalid(
            "worktree and branch are missing but the saved head cannot be proven merged; manual review required",
        ));
    };
    if branch_head != head || !is_ancestor(&context.main_root, head, "origin/main")? {
        return Err(invalid(
            "worktree is missing but its recorded branch cannot be proven merged; manual review required",
        ));
    }
    cleanup_branch(context, manifest, manifest_path)?;
    if manifest.state == "removed" {
        Ok("interrupted removal recovered; branch removed")
    } else {
        Ok("worktree removed; branch retained for safe retry")
    }
}

fn cleanup_branch(
    context: &RepoContext,
    manifest: &mut Manifest,
    manifest_path: &Path,
) -> io::Result<()> {
    let expected_head = manifest
        .head_sha
        .as_deref()
        .ok_or_else(|| invalid("manifest has no saved head for branch cleanup"))?;
    let actual_head = branch_head(&context.main_root, &manifest.branch)?
        .ok_or_else(|| invalid("recorded branch is missing"))?;
    if actual_head != expected_head
        || !is_ancestor(&context.main_root, expected_head, "origin/main")?
    {
        return Err(invalid(
            "recorded branch changed or is not merged into origin/main; keeping it",
        ));
    }

    let output = git_output(
        &context.main_root,
        ["branch", "-d", "--", manifest.branch.as_str()],
    )?;
    if output.status.success() {
        manifest.state = "removed".to_owned();
    } else {
        manifest.state = "branch_retained".to_owned();
    }
    write_manifest(manifest_path, manifest)?;
    Ok(())
}

fn repo_context(start: &Path) -> io::Result<RepoContext> {
    let top = git_stdout(start, ["rev-parse", "--show-toplevel"])?;
    let top = fs::canonicalize(PathBuf::from(top.trim()))
        .map_err(|_| invalid("could not resolve this checkout's root"))?;
    let common = git_stdout(&top, ["rev-parse", "--git-common-dir"])?;
    let common = PathBuf::from(common.trim());
    let common = if common.is_absolute() {
        common
    } else {
        top.join(common)
    };
    let common =
        fs::canonicalize(common).map_err(|_| invalid("could not resolve Git common directory"))?;
    let main_root = common
        .parent()
        .ok_or_else(|| invalid("Git common directory has no checkout parent"))?;
    let main_root =
        fs::canonicalize(main_root).map_err(|_| invalid("could not resolve primary checkout"))?;
    let main_git_dir = fs::canonicalize(main_root.join(".git")).map_err(|_| {
        invalid("bare repositories, submodules, and unknown checkout layouts are unsupported")
    })?;
    if main_git_dir != common || !main_root.join("Cargo.toml").is_file() {
        return Err(invalid(
            "bare repositories, submodules, and unknown checkout layouts are unsupported",
        ));
    }

    let dot_loop = main_root.join(".loop");
    let registry = dot_loop.join("runs");
    validate_existing_directory(&main_root, &dot_loop)?;
    validate_existing_directory(&main_root, &registry)?;
    let registry_dir = match fs::canonicalize(&registry) {
        Ok(path) => path,
        Err(error) if error.kind() == io::ErrorKind::NotFound => registry,
        Err(error) => return Err(error),
    };
    Ok(RepoContext {
        main_root,
        registry_dir,
    })
}

fn ensure_registry_dirs(context: &RepoContext) -> io::Result<()> {
    let dot_loop = ensure_directory(&context.main_root, &context.main_root.join(".loop"))?;
    let runs = ensure_directory(&context.main_root, &dot_loop.join("runs"))?;
    if runs != context.registry_dir {
        return Err(invalid("registry directory changed during operation"));
    }
    Ok(())
}

fn ensure_directory(container: &Path, path: &Path) -> io::Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(invalid(
                "local .loop directory must not be a symlink or junction",
            ));
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(invalid("local .loop path exists but is not a directory"));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir(path)?,
        Err(error) => return Err(error),
    }
    let canonical = fs::canonicalize(path)?;
    if !canonical.starts_with(container) {
        return Err(invalid("local registry directory escaped the checkout"));
    }
    Ok(canonical)
}

fn validate_existing_directory(container: &Path, path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(invalid(
            "local .loop directory must not be a symlink or junction",
        )),
        Ok(metadata) if !metadata.is_dir() => {
            Err(invalid("local .loop path exists but is not a directory"))
        }
        Ok(_) => {
            let canonical = fs::canonicalize(path)?;
            if canonical.starts_with(container) {
                Ok(())
            } else {
                Err(invalid("local registry directory escaped the checkout"))
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn resolve_worktree_root(context: &RepoContext, requested: Option<&Path>) -> io::Result<PathBuf> {
    let root = match requested {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => context.main_root.join(path),
        None => context.main_root.join(".loop").join("worktrees"),
    };
    if requested.is_none() {
        ensure_directory(&context.main_root, &root)?;
    } else {
        fs::create_dir_all(&root)?;
    }
    fs::canonicalize(root).map_err(|_| invalid("could not resolve worktree root"))
}

fn validate_worktree_root(main_root: &Path, worktree_root: &Path) -> io::Result<()> {
    let ignored_root = main_root.join(".loop");
    if worktree_root.starts_with(main_root) && !worktree_root.starts_with(&ignored_root) {
        return Err(invalid(
            "a worktree root inside the checkout must be under the ignored .loop directory",
        ));
    }
    if worktree_root == main_root {
        return Err(invalid("the main checkout cannot be the worktree root"));
    }
    Ok(())
}

fn validate_target_path(manifest: &Manifest) -> io::Result<PathBuf> {
    let canonical_root = fs::canonicalize(&manifest.worktree_root)
        .map_err(|_| invalid("configured worktree root is missing"))?;
    let expected = canonical_root.join(format!("issue-{}", manifest.issue));
    if !same_path(&manifest.path, &expected) {
        return Err(invalid(
            "manifest path does not match its configured root and issue",
        ));
    }
    if manifest.path == manifest.worktree_root || expected == canonical_root {
        return Err(invalid("the configured root itself cannot be removed"));
    }
    let metadata = fs::symlink_metadata(&manifest.path)
        .map_err(|_| invalid("recorded worktree path is missing"))?;
    if metadata.file_type().is_symlink() {
        return Err(invalid("recorded worktree path is a symlink or junction"));
    }
    let canonical_path = fs::canonicalize(&manifest.path)
        .map_err(|_| invalid("could not resolve recorded worktree path"))?;
    if canonical_path == canonical_root || !canonical_path.starts_with(&canonical_root) {
        return Err(invalid("worktree path escapes its configured root"));
    }
    Ok(canonical_path)
}

fn fetch_main(main_root: &Path) -> io::Result<String> {
    let output = git_output(main_root, ["fetch", "origin", "main"])?;
    if !output.status.success() {
        return Err(invalid(
            "git fetch origin main failed; no create/remove action was taken",
        ));
    }
    git_stdout(main_root, ["rev-parse", "--verify", "origin/main^{commit}"])
        .map(|sha| sha.trim().to_owned())
}

fn create_git_worktree(main_root: &Path, manifest: &Manifest) -> io::Result<()> {
    git_checked(
        main_root,
        [
            OsString::from("worktree"),
            OsString::from("add"),
            OsString::from("-b"),
            OsString::from(&manifest.branch),
            git_path_arg(&manifest.path),
            OsString::from(&manifest.base_sha),
        ],
    )
    .map_err(|error| invalid(format!("git worktree add failed: {error}")))?;
    Ok(())
}

fn verify_registered_worktree(manifest: &Manifest, entry: &WorktreeEntry) -> io::Result<String> {
    let canonical_path = validate_target_path(manifest)?;
    let registered_path = fs::canonicalize(&entry.path)
        .map_err(|_| invalid("registered Git worktree path is missing"))?;
    if canonical_path != registered_path
        || entry.branch.as_deref() != Some(manifest.branch.as_str())
    {
        return Err(invalid(
            "Git worktree registration does not match the manifest",
        ));
    }
    let head = git_stdout(&manifest.path, ["rev-parse", "--verify", "HEAD"])?
        .trim()
        .to_owned();
    if entry.head.as_deref() != Some(head.as_str()) {
        return Err(invalid(
            "Git worktree HEAD disagrees with Git's worktree registry",
        ));
    }
    let actual_branch = git_stdout(
        &manifest.path,
        ["symbolic-ref", "--quiet", "--short", "HEAD"],
    )?;
    if actual_branch.trim() != manifest.branch {
        return Err(invalid("worktree is not attached to its recorded branch"));
    }
    Ok(head)
}

fn registered_entry(
    main_root: &Path,
    target: &Path,
    branch: &str,
) -> io::Result<Option<WorktreeEntry>> {
    let entries = worktree_entries(main_root)?;
    let target_canonical = match fs::canonicalize(target) {
        Ok(path) => path,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let matches = entries
        .into_iter()
        .filter_map(|entry| {
            let path = fs::canonicalize(&entry.path).ok()?;
            (path == target_canonical && entry.branch.as_deref() == Some(branch)).then_some(entry)
        })
        .collect::<Vec<_>>();
    if matches.len() > 1 {
        return Err(invalid("multiple Git worktrees match this registry entry"));
    }
    Ok(matches.into_iter().next())
}

fn worktree_entries(main_root: &Path) -> io::Result<Vec<WorktreeEntry>> {
    let output = git_output(main_root, ["worktree", "list", "--porcelain", "-z"])?;
    if !output.status.success() {
        return Err(invalid("could not read Git worktree registry"));
    }
    let mut entries = Vec::new();
    let mut current: Option<WorktreeEntry> = None;
    for field in output.stdout.split(|byte| *byte == 0) {
        if field.is_empty() {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            continue;
        }
        if let Some(path) = field.strip_prefix(b"worktree ") {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            current = Some(WorktreeEntry {
                path: PathBuf::from(String::from_utf8_lossy(path).into_owned()),
                head: None,
                branch: None,
            });
        } else if let Some(entry) = current.as_mut() {
            if let Some(head) = field.strip_prefix(b"HEAD ") {
                entry.head = Some(String::from_utf8_lossy(head).into_owned());
            } else if let Some(branch) = field.strip_prefix(b"branch refs/heads/") {
                entry.branch = Some(String::from_utf8_lossy(branch).into_owned());
            }
        }
    }
    if let Some(entry) = current {
        entries.push(entry);
    }
    Ok(entries)
}

fn branch_exists(main_root: &Path, branch: &str) -> io::Result<bool> {
    let reference = format!("refs/heads/{branch}");
    let output = git_output(main_root, ["show-ref", "--verify", "--quiet", &reference])?;
    Ok(output.status.success())
}

fn branch_head(main_root: &Path, branch: &str) -> io::Result<Option<String>> {
    if !branch_exists(main_root, branch)? {
        return Ok(None);
    }
    git_stdout(
        main_root,
        [
            "rev-parse",
            "--verify",
            &format!("refs/heads/{branch}^{{commit}}"),
        ],
    )
    .map(|sha| Some(sha.trim().to_owned()))
}

fn is_ancestor(main_root: &Path, commit: &str, upstream: &str) -> io::Result<bool> {
    let output = git_output(main_root, ["merge-base", "--is-ancestor", commit, upstream])?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(invalid("could not verify commit ancestry")),
    }
}

fn branch_name(issue: u64) -> String {
    format!("codex/issue-{issue}-worktree")
}

fn new_run_id(issue: u64) -> io::Result<String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid("system clock is before Unix epoch"))?
        .as_nanos();
    Ok(format!("{issue}-{nanos}-{}", std::process::id()))
}

fn manifest_path(context: &RepoContext, issue: u64) -> PathBuf {
    context.registry_dir.join(format!("issue-{issue}.json"))
}

fn validate_manifest(manifest: &Manifest, issue: u64) -> io::Result<()> {
    if manifest.version != 1
        || manifest.issue != issue
        || manifest.branch != branch_name(issue)
        || validate_owner(&manifest.owner).is_err()
    {
        return Err(invalid(
            "registry manifest does not match the requested issue",
        ));
    }
    let expected_path = manifest
        .worktree_root
        .join(format!("issue-{}", manifest.issue));
    if !same_path(&manifest.path, &expected_path) {
        return Err(invalid("manifest path does not match its configured root"));
    }
    Ok(())
}

fn claim_manifest(path: &Path, manifest: &Manifest) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|_| invalid("could not serialize worktree manifest"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                invalid("another run already claimed this issue; inspect its status")
            } else {
                error
            }
        })?;
    file.write_all(&bytes)?;
    file.sync_all()
}

fn write_manifest(path: &Path, manifest: &Manifest) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| invalid("registry manifest disappeared during the operation"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid("registry manifest is not a regular file"));
    }
    let bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|_| invalid("could not serialize worktree manifest"))?;
    let mut file = OpenOptions::new().write(true).truncate(true).open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()
}

fn read_manifest(path: &Path, issue: u64) -> io::Result<Manifest> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| invalid(format!("no worktree registry record for issue #{issue}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid("registry manifest is not a regular file"));
    }
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(|_| invalid("worktree registry manifest is invalid"))
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn git_stdout<const N: usize>(cwd: &Path, args: [&str; N]) -> io::Result<String> {
    let output = git_output(cwd, args)?;
    if !output.status.success() {
        return Err(invalid("required Git query failed"));
    }
    String::from_utf8(output.stdout).map_err(|_| invalid("Git returned non-UTF-8 output"))
}

fn git_output<I, S>(cwd: &Path, args: I) -> io::Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    Command::new("git").arg("-C").arg(cwd).args(args).output()
}

fn git_checked<I, S>(cwd: &Path, args: I) -> io::Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = git_output(cwd, args)?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(invalid(format!(
            "Git operation failed with exit code {:?}; captured diagnostics were not printed",
            output.status.code()
        )))
    }
}

fn git_path_arg(path: &Path) -> OsString {
    #[cfg(windows)]
    if let Some(path) = path.to_str().and_then(|path| path.strip_prefix(r"\\?\")) {
        if let Some(unc_path) = path.strip_prefix("UNC\\") {
            return OsString::from(format!(r"\\{unc_path}"));
        }
        if path.as_bytes().get(1) == Some(&b':') {
            return OsString::from(path);
        }
    }
    path.as_os_str().to_owned()
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod tests {
    use super::{
        active_worktree, create, git_stdout, parse_args, remove, repo_context, status,
        validate_owner, validate_target_path,
    };
    #[cfg(windows)]
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

    struct Fixture {
        root: PathBuf,
        worktrees: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir()
                .join(format!("sysrqui-worktree-test-{}-{id}", std::process::id()));
            let _origin = root.join("origin.git");
            let checkout = root.join("checkout with spaces");
            let worktrees = root.join("管理 worktrees root");
            fs::create_dir_all(&root).unwrap();
            command(
                root.as_path(),
                [
                    "git",
                    "init",
                    "--bare",
                    "--initial-branch=main",
                    "origin.git",
                ],
            );
            command(
                root.as_path(),
                ["git", "clone", "origin.git", "checkout with spaces"],
            );
            command(&checkout, ["git", "config", "user.name", "Fixture"]);
            command(
                &checkout,
                ["git", "config", "user.email", "fixture@example.invalid"],
            );
            fs::write(checkout.join("README.md"), "fixture\n").unwrap();
            command(&checkout, ["git", "add", "README.md"]);
            command(&checkout, ["git", "commit", "-m", "fixture"]);
            command(&checkout, ["git", "push", "-u", "origin", "main"]);
            command(&checkout, ["git", "fetch", "origin", "main"]);
            fs::write(checkout.join("Cargo.toml"), "[workspace]\n").unwrap();
            command(&checkout, ["git", "add", "Cargo.toml"]);
            command(&checkout, ["git", "commit", "-m", "add workspace marker"]);
            command(&checkout, ["git", "push", "origin", "main"]);
            command(&checkout, ["git", "fetch", "origin", "main"]);
            Self {
                root: checkout,
                worktrees,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let parent = self.root.parent().unwrap();
            let _ = fs::remove_dir_all(parent);
        }
    }

    fn command<const N: usize>(cwd: &Path, args: [&str; N]) {
        let output = Command::new(args[0])
            .args(&args[1..])
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "command {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn parses_only_supported_commands_and_owner_identifiers() {
        assert!(
            parse_args(vec![
                "create".to_owned(),
                "--issue".to_owned(),
                "12".to_owned(),
                "--owner".to_owned(),
                "codex_1".to_owned(),
                "--root".to_owned(),
                "D:\\work trees".to_owned(),
            ])
            .is_ok()
        );
        assert!(
            parse_args(vec![
                "create".to_owned(),
                "--issue".to_owned(),
                "0".to_owned(),
                "--owner".to_owned(),
                "codex".to_owned(),
            ])
            .is_err()
        );
        assert!(validate_owner("codex/worker").is_err());
        assert!(validate_owner("codex-worker_1").is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn git_path_arguments_convert_verbatim_drive_and_unc_paths() {
        assert_eq!(
            super::git_path_arg(Path::new(r"\\?\C:\work trees\專案")),
            OsString::from(r"C:\work trees\專案")
        );
        assert_eq!(
            super::git_path_arg(Path::new(r"\\?\UNC\server\share\work trees")),
            OsString::from(r"\\server\share\work trees")
        );
    }

    #[test]
    fn create_is_idempotent_for_same_owner_and_rejects_other_owner() {
        let fixture = Fixture::new();
        let first = create(&fixture.root, 21, "codex", Some(&fixture.worktrees)).unwrap();
        let second = create(&fixture.root, 21, "codex", Some(&fixture.worktrees)).unwrap();
        assert_eq!(first.run_id, second.run_id);
        assert!(matches!(second.state.as_str(), "active"));
        assert!(create(&fixture.root, 21, "claude", Some(&fixture.worktrees)).is_err());
    }

    #[test]
    fn create_rejects_an_unmanaged_path_collision() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.worktrees.join("issue-22")).unwrap();
        fs::write(fixture.worktrees.join("issue-22").join("keep.txt"), "keep").unwrap();

        assert!(create(&fixture.root, 22, "codex", Some(&fixture.worktrees)).is_err());
        assert_eq!(
            fs::read(fixture.worktrees.join("issue-22").join("keep.txt")).unwrap(),
            b"keep"
        );
    }

    #[test]
    fn status_does_not_create_the_local_registry() {
        let fixture = Fixture::new();
        assert!(status(&fixture.root, 31).is_err());
        assert!(!fixture.root.join(".loop").exists());
    }

    #[test]
    fn status_reads_current_head_and_registry_is_shared_from_linked_worktree() {
        let fixture = Fixture::new();
        let manifest = create(&fixture.root, 23, "codex", Some(&fixture.worktrees)).unwrap();
        let (record, head) = status(&fixture.root, 23).unwrap();
        assert_eq!(head.as_deref(), Some(manifest.base_sha.as_str()));
        assert_eq!(record.owner, "codex");

        let linked = fixture.root.parent().unwrap().join("secondary worktree");
        command(
            &fixture.root,
            [
                "git",
                "worktree",
                "add",
                "--detach",
                "../secondary worktree",
                "origin/main",
            ],
        );
        let linked_context = repo_context(&linked).unwrap();
        let primary_context = repo_context(&fixture.root).unwrap();
        assert_eq!(linked_context.registry_dir, primary_context.registry_dir);
    }

    #[test]
    fn active_worktree_returns_verified_assignment_and_rejects_other_owner() {
        let fixture = Fixture::new();
        let created = create(&fixture.root, 32, "Codex", Some(&fixture.worktrees)).unwrap();
        let assigned = active_worktree(&fixture.root, 32, "Codex").unwrap();
        assert_eq!(assigned.issue, 32);
        assert_eq!(assigned.owner, "Codex");
        assert_eq!(assigned.run_id, created.run_id);
        assert_eq!(assigned.branch, created.branch);
        assert_eq!(assigned.base_sha, created.base_sha);
        assert_eq!(assigned.head_sha, created.head_sha.unwrap());
        assert_eq!(assigned.path, created.path);
        assert!(active_worktree(&fixture.root, 32, "Claude").is_err());
    }

    #[test]
    fn remove_refuses_dirty_or_untracked_worktree() {
        let fixture = Fixture::new();
        let manifest = create(&fixture.root, 24, "codex", Some(&fixture.worktrees)).unwrap();
        fs::write(manifest.path.join("README.md"), "dirty\n").unwrap();
        assert!(remove(&fixture.root, 24, "codex").is_err());
        fs::write(manifest.path.join("README.md"), "fixture\n").unwrap();
        fs::write(manifest.path.join("untracked.txt"), "keep me").unwrap();
        assert!(remove(&fixture.root, 24, "codex").is_err());
        assert!(manifest.path.exists());
        fs::remove_file(manifest.path.join("untracked.txt")).unwrap();
        fs::create_dir_all(manifest.path.join("target")).unwrap();
        fs::write(manifest.path.join("target/ignored.bin"), "keep ignored").unwrap();
        assert!(remove(&fixture.root, 24, "codex").is_err());
        assert!(manifest.path.join("target/ignored.bin").exists());
        assert!(remove(&fixture.root, 24, "claude").is_err());
    }

    #[test]
    fn remove_refuses_unpushed_or_unmerged_head() {
        let fixture = Fixture::new();
        let manifest = create(&fixture.root, 25, "codex", Some(&fixture.worktrees)).unwrap();
        fs::write(manifest.path.join("change.txt"), "local commit\n").unwrap();
        command(&manifest.path, ["git", "add", "change.txt"]);
        command(&manifest.path, ["git", "commit", "-m", "unmerged change"]);

        assert!(remove(&fixture.root, 25, "codex").is_err());
        assert!(manifest.path.exists());
    }

    #[test]
    fn remove_refuses_a_pushed_but_unmerged_branch() {
        let fixture = Fixture::new();
        let manifest = create(&fixture.root, 28, "codex", Some(&fixture.worktrees)).unwrap();
        fs::write(manifest.path.join("change.txt"), "pushed but not merged\n").unwrap();
        command(&manifest.path, ["git", "add", "change.txt"]);
        command(&manifest.path, ["git", "commit", "-m", "unmerged change"]);
        command(
            &manifest.path,
            ["git", "push", "origin", "codex/issue-28-worktree"],
        );

        assert!(remove(&fixture.root, 28, "codex").is_err());
        assert!(manifest.path.exists());
    }

    #[test]
    fn remove_reclaims_clean_merged_worktree_and_retains_registry() {
        let fixture = Fixture::new();
        let manifest = create(&fixture.root, 26, "codex", Some(&fixture.worktrees)).unwrap();
        fs::write(manifest.path.join("change.txt"), "merged change\n").unwrap();
        command(&manifest.path, ["git", "add", "change.txt"]);
        command(&manifest.path, ["git", "commit", "-m", "merged change"]);
        command(
            &fixture.root,
            [
                "git",
                "merge",
                "--no-ff",
                "codex/issue-26-worktree",
                "-m",
                "merge worktree",
            ],
        );
        command(&fixture.root, ["git", "push", "origin", "main"]);

        assert!(remove(&fixture.root, 26, "codex").is_ok());
        assert!(!manifest.path.exists());
        let (record, head) = status(&fixture.root, 26).unwrap();
        assert_eq!(record.state, "removed");
        assert_eq!(head, None);
        assert!(remove(&fixture.root, 26, "codex").is_ok());
    }

    #[test]
    fn remove_recovers_after_worktree_removal_before_manifest_update() {
        let fixture = Fixture::new();
        let mut manifest = create(&fixture.root, 29, "codex", Some(&fixture.worktrees)).unwrap();
        fs::write(manifest.path.join("change.txt"), "merged change\n").unwrap();
        command(&manifest.path, ["git", "add", "change.txt"]);
        command(&manifest.path, ["git", "commit", "-m", "merged change"]);
        command(
            &fixture.root,
            [
                "git",
                "merge",
                "--no-ff",
                "codex/issue-29-worktree",
                "-m",
                "merge worktree",
            ],
        );
        command(&fixture.root, ["git", "push", "origin", "main"]);
        command(&fixture.root, ["git", "fetch", "origin", "main"]);
        manifest.head_sha = Some(
            git_stdout(&manifest.path, ["rev-parse", "HEAD"])
                .unwrap()
                .trim()
                .to_owned(),
        );
        let context = repo_context(&fixture.root).unwrap();
        super::write_manifest(&super::manifest_path(&context, 29), &manifest).unwrap();
        command(
            &fixture.root,
            [
                "git",
                "worktree",
                "remove",
                "--",
                manifest.path.to_str().unwrap(),
            ],
        );

        assert!(remove(&fixture.root, 29, "codex").is_ok());
        let (record, _) = status(&fixture.root, 29).unwrap();
        assert_eq!(record.state, "removed");
    }

    #[test]
    fn remove_recovers_after_branch_deletion_before_manifest_update() {
        let fixture = Fixture::new();
        let mut manifest = create(&fixture.root, 30, "codex", Some(&fixture.worktrees)).unwrap();
        fs::write(manifest.path.join("change.txt"), "merged change\n").unwrap();
        command(&manifest.path, ["git", "add", "change.txt"]);
        command(&manifest.path, ["git", "commit", "-m", "merged change"]);
        command(
            &fixture.root,
            [
                "git",
                "merge",
                "--no-ff",
                "codex/issue-30-worktree",
                "-m",
                "merge worktree",
            ],
        );
        command(&fixture.root, ["git", "push", "origin", "main"]);
        command(&fixture.root, ["git", "fetch", "origin", "main"]);
        manifest.head_sha = Some(
            git_stdout(&manifest.path, ["rev-parse", "HEAD"])
                .unwrap()
                .trim()
                .to_owned(),
        );
        let context = repo_context(&fixture.root).unwrap();
        super::write_manifest(&super::manifest_path(&context, 30), &manifest).unwrap();
        command(
            &fixture.root,
            [
                "git",
                "worktree",
                "remove",
                "--",
                manifest.path.to_str().unwrap(),
            ],
        );
        command(
            &fixture.root,
            ["git", "branch", "-d", "--", "codex/issue-30-worktree"],
        );

        assert!(remove(&fixture.root, 30, "codex").is_ok());
        let (record, _) = status(&fixture.root, 30).unwrap();
        assert_eq!(record.state, "removed");
    }

    #[test]
    fn target_path_validation_rejects_root_and_escape() {
        let fixture = Fixture::new();
        let root = fixture.worktrees.clone();
        fs::create_dir_all(&root).unwrap();
        let mut manifest = create(&fixture.root, 27, "codex", Some(&root)).unwrap();
        manifest.path = root.clone();
        assert!(validate_target_path(&manifest).is_err());
        manifest.path = root.join("..").join("escaped");
        assert!(validate_target_path(&manifest).is_err());
    }

    #[test]
    fn bare_repositories_are_rejected() {
        let fixture = Fixture::new();
        let bare = fixture.root.parent().unwrap().join("origin.git");
        assert!(repo_context(&bare).is_err());
    }
}
