use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub(crate) struct SnapshotStore {
    root: PathBuf,
    session_id: String,
    workdir: PathBuf,
}

impl SnapshotStore {
    pub(crate) fn new(root: PathBuf, session_id: String, workdir: PathBuf) -> Self {
        Self {
            root,
            session_id,
            workdir,
        }
    }

    pub(crate) fn track(&self) -> Result<Option<String>> {
        if !self.is_git_worktree() {
            return Ok(None);
        }
        self.ensure_initialized()?;
        let add = self.git_snapshot(["add", "--all", "--", "."])?;
        if !add.status.success() {
            return Ok(None);
        }
        let tree = self.git_snapshot(["write-tree"])?;
        if !tree.status.success() {
            return Ok(None);
        }
        let hash = String::from_utf8_lossy(&tree.stdout).trim().to_string();
        if hash.is_empty() {
            Ok(None)
        } else {
            Ok(Some(hash))
        }
    }

    pub(crate) fn restore(&self, target: &str) -> Result<()> {
        if target.trim().is_empty() {
            return Err(Error::Message("snapshot hash is empty".to_string()));
        }
        let Some(current) = self.track()? else {
            return Err(Error::Message(
                "Git snapshot is unavailable for this session".to_string(),
            ));
        };
        let diff = self.git_snapshot([
            "diff",
            "--name-status",
            "--no-renames",
            target,
            &current,
            "--",
            ".",
        ])?;
        if !diff.status.success() {
            return Err(Error::Message("failed to diff Git snapshots".to_string()));
        }
        for line in String::from_utf8_lossy(&diff.stdout).lines() {
            let mut parts = line.splitn(2, '\t');
            let status = parts.next().unwrap_or_default();
            let Some(path) = parts.next() else {
                continue;
            };
            if status.starts_with('A') {
                remove_worktree_path(&self.workdir.join(path))?;
            } else {
                let checkout = self.git_snapshot(["checkout", target, "--", path])?;
                if !checkout.status.success() {
                    return Err(Error::Message(format!(
                        "failed to restore snapshot path: {path}"
                    )));
                }
            }
        }
        let _ = self.track();
        Ok(())
    }

    fn git_dir(&self) -> PathBuf {
        self.root.join("sessions").join(&self.session_id)
    }

    fn is_git_worktree(&self) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(&self.workdir)
            .args(["rev-parse", "--is-inside-work-tree"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim() == "true")
            .unwrap_or(false)
    }

    fn ensure_initialized(&self) -> Result<()> {
        let git_dir = self.git_dir();
        fs::create_dir_all(&git_dir)?;
        if git_dir.join("HEAD").exists() {
            return Ok(());
        }
        let init = Command::new("git")
            .env("GIT_DIR", &git_dir)
            .env("GIT_WORK_TREE", &self.workdir)
            .arg("init")
            .output()?;
        if !init.status.success() {
            return Err(Error::Message(
                "failed to initialize Git snapshot store".to_string(),
            ));
        }
        for (key, value) in [
            ("core.autocrlf", "false"),
            ("core.longpaths", "true"),
            ("core.quotepath", "false"),
            ("core.fsmonitor", "false"),
        ] {
            let _ = Command::new("git")
                .arg("--git-dir")
                .arg(&git_dir)
                .args(["config", key, value])
                .output();
        }
        Ok(())
    }

    fn git_snapshot<const N: usize>(&self, args: [&str; N]) -> Result<Output> {
        Ok(Command::new("git")
            .arg("--git-dir")
            .arg(self.git_dir())
            .arg("--work-tree")
            .arg(&self.workdir)
            .args(args)
            .output()?)
    }
}

fn remove_worktree_path(path: &Path) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}
