use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

mod doctor;

const GENERATED_FILES: [(&str, &str); 3] = [
    (
        "launch.json",
        r#"{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "lldb",
      "request": "launch",
      "name": "Debug sysrqui",
      "cargo": {
        "args": ["build", "--package", "sysrqui", "--bin", "sysrqui"],
        "filter": {
          "name": "sysrqui",
          "kind": "bin"
        }
      },
      "args": [],
      "cwd": "${workspaceFolder}",
      "sourceLanguages": ["rust"]
    }
  ]
}
"#,
    ),
    (
        "tasks.json",
        r#"{
  "version": "2.0.0",
  "tasks": [
    {
      "label": "cargo build (sysrqui)",
      "type": "process",
      "command": "cargo",
      "args": ["build", "--package", "sysrqui", "--bin", "sysrqui"],
      "options": {
        "cwd": "${workspaceFolder}"
      },
      "problemMatcher": ["$rustc"]
    }
  ]
}
"#,
    ),
    (
        "extensions.json",
        r#"{
  "recommendations": [
    "rust-lang.rust-analyzer",
    "vadimcn.vscode-lldb"
  ]
}
"#,
    ),
];

fn main() {
    if let Err(error) = run(env::args().skip(1), workspace_root()) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("xtask manifest must be two directories below the workspace root")
        .to_path_buf()
}

fn run(args: impl IntoIterator<Item = String>, root: PathBuf) -> io::Result<()> {
    let mut args = args.into_iter();
    match (args.next().as_deref(), args.next()) {
        (Some("debug-setup"), None) => generate_debug_setup(&root),
        (Some("doctor"), None) => doctor::run(),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: cargo xtask <debug-setup|doctor>",
        )),
    }
}

fn generate_debug_setup(root: &Path) -> io::Result<()> {
    let vscode_dir = root.join(".vscode");
    let mut missing = Vec::new();
    let mut conflicts = Vec::new();

    for (name, expected) in GENERATED_FILES {
        let path = vscode_dir.join(name);
        match fs::read(&path) {
            Ok(existing) if existing == expected.as_bytes() => {}
            Ok(_) => conflicts.push(path),
            Err(error) if error.kind() == io::ErrorKind::NotFound => missing.push((path, expected)),
            Err(error) => return Err(error),
        }
    }

    if !conflicts.is_empty() {
        let paths = conflicts
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "existing VS Code file(s) differ from generated content: {paths}; no files were changed"
            ),
        ));
    }

    if missing.is_empty() {
        println!("VS Code debug files are already up to date.");
        return Ok(());
    }

    fs::create_dir_all(&vscode_dir)?;
    let mut reserved: Vec<(PathBuf, File)> = Vec::new();
    for (path, _) in &missing {
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(file) => reserved.push((path.clone(), file)),
            Err(error) => {
                let created = reserved
                    .iter()
                    .map(|(path, _)| path.clone())
                    .collect::<Vec<_>>();
                drop(reserved);
                for created_path in created {
                    let _ = fs::remove_file(created_path);
                }
                return Err(error);
            }
        }
    }

    let write_result = reserved
        .iter_mut()
        .zip(missing.iter())
        .try_for_each(|((_, file), (_, contents))| file.write_all(contents.as_bytes()));
    if let Err(error) = write_result {
        let created = reserved
            .iter()
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>();
        drop(reserved);
        for path in created {
            let _ = fs::remove_file(path);
        }
        return Err(error);
    }

    println!("Created VS Code debug files in {}.", vscode_dir.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{GENERATED_FILES, generate_debug_setup, run};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("sysrqui-xtask-test-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn creates_all_debug_files_and_is_idempotent() {
        let temp = TempDir::new();
        generate_debug_setup(&temp.0).unwrap();
        let generated = GENERATED_FILES
            .iter()
            .map(|(name, _)| fs::read(temp.0.join(".vscode").join(name)).unwrap())
            .collect::<Vec<_>>();

        generate_debug_setup(&temp.0).unwrap();

        let after_rerun = GENERATED_FILES
            .iter()
            .map(|(name, _)| fs::read(temp.0.join(".vscode").join(name)).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(generated, after_rerun);
    }

    #[test]
    fn conflicting_file_prevents_all_changes() {
        let temp = TempDir::new();
        let vscode_dir = temp.0.join(".vscode");
        fs::create_dir_all(&vscode_dir).unwrap();
        fs::write(vscode_dir.join("tasks.json"), "user-owned settings").unwrap();

        assert!(generate_debug_setup(&temp.0).is_err());
        assert!(!vscode_dir.join("launch.json").exists());
        assert_eq!(
            fs::read(vscode_dir.join("tasks.json")).unwrap(),
            b"user-owned settings"
        );
        assert!(!vscode_dir.join("extensions.json").exists());
    }

    #[test]
    fn unknown_arguments_return_usage_error() {
        let temp = TempDir::new();
        assert!(run(["other".to_string()], temp.0.clone()).is_err());
        assert!(
            run(
                ["debug-setup".to_string(), "extra".to_string()],
                temp.0.clone()
            )
            .is_err()
        );
    }
}
