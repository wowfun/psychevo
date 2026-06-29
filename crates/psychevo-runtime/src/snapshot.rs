use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::error::{Error, Result};
use crate::paths::workspace_snapshot_id;

const SNAPSHOT_PRUNE: &str = "7.days";
const CLEANUP_INTERVAL: Duration = Duration::from_secs(60 * 60);
const STALE_LOCK_AFTER: Duration = Duration::from_secs(30 * 60);
const OPERATION_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const OPERATION_LOCK_POLL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone)]
pub(crate) struct SnapshotStore {
    pub(crate) root: PathBuf,
    pub(crate) cwd: PathBuf,
}

impl SnapshotStore {
    pub(crate) fn new(root: PathBuf, cwd: PathBuf) -> Self {
        Self { root, cwd }
    }

    pub(crate) fn track(&self) -> Result<Option<String>> {
        if !self.is_git_worktree() {
            return Ok(None);
        }
        let _ = maybe_cleanup_snapshot_root(&self.root);
        let _lock = self.acquire_operation_lock()?;
        self.track_locked()
    }

    fn track_locked(&self) -> Result<Option<String>> {
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
        if !self.is_git_worktree() {
            return Err(Error::Message(
                "Git snapshot is unavailable for this session".to_string(),
            ));
        }
        let _ = maybe_cleanup_snapshot_root(&self.root);
        let _lock = self.acquire_operation_lock()?;
        let Some(current) = self.track_locked()? else {
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
                remove_worktree_path(&self.cwd.join(path))?;
            } else {
                let checkout = self.git_snapshot(["checkout", target, "--", path])?;
                if !checkout.status.success() {
                    return Err(Error::Message(format!(
                        "failed to restore snapshot path: {path}"
                    )));
                }
            }
        }
        let _ = self.track_locked();
        Ok(())
    }

    pub(crate) fn workspace_id(&self) -> Result<String> {
        workspace_snapshot_id(&self.cwd)
    }

    pub(crate) fn git_dir(&self) -> Result<PathBuf> {
        Ok(self.root.join("workspaces").join(self.workspace_id()?))
    }

    fn operation_lock_path(&self) -> Result<PathBuf> {
        Ok(self
            .root
            .join("workspaces")
            .join(format!("{}.lock", self.workspace_id()?)))
    }

    fn acquire_operation_lock(&self) -> Result<FileLock> {
        acquire_file_lock(
            &self.operation_lock_path()?,
            STALE_LOCK_AFTER,
            OPERATION_LOCK_TIMEOUT,
        )
        .map_err(|err| match err {
            Error::Message(message) => {
                Error::Message(format!("Git snapshot store is busy: {message}"))
            }
            err => err,
        })
    }

    pub(crate) fn is_git_worktree(&self) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(&self.cwd)
            .args(["rev-parse", "--is-inside-work-tree"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim() == "true")
            .unwrap_or(false)
    }

    pub(crate) fn ensure_initialized(&self) -> Result<()> {
        let git_dir = self.git_dir()?;
        fs::create_dir_all(&git_dir)?;
        if git_dir.join("HEAD").exists() {
            return Ok(());
        }
        let init = Command::new("git")
            .env("GIT_DIR", &git_dir)
            .env("GIT_WORK_TREE", &self.cwd)
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

    pub(crate) fn git_snapshot<const N: usize>(&self, args: [&str; N]) -> Result<Output> {
        let git_dir = self.git_dir()?;
        Ok(Command::new("git")
            .arg("--git-dir")
            .arg(git_dir)
            .arg("--work-tree")
            .arg(&self.cwd)
            .args(args)
            .output()?)
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct SnapshotCleanupReport {
    pub(crate) ran: bool,
    pub(crate) gc_attempted: usize,
    pub(crate) gc_failed: usize,
    pub(crate) locked: usize,
}

pub(crate) fn maybe_cleanup_snapshot_root(root: &Path) -> Result<SnapshotCleanupReport> {
    let now = now_ms();
    if cleanup_marker_fresh(root, now) {
        return Ok(SnapshotCleanupReport::default());
    }
    fs::create_dir_all(root)?;
    let Some(_cleanup_lock) = try_acquire_file_lock(&root.join(".cleanup.lock"), STALE_LOCK_AFTER)?
    else {
        return Ok(SnapshotCleanupReport {
            locked: 1,
            ..SnapshotCleanupReport::default()
        });
    };
    if cleanup_marker_fresh(root, now) {
        return Ok(SnapshotCleanupReport::default());
    }

    let mut report = SnapshotCleanupReport {
        ran: true,
        ..SnapshotCleanupReport::default()
    };
    let workspaces = root.join("workspaces");
    if let Ok(entries) = fs::read_dir(&workspaces) {
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let git_dir = entry.path();
            if !git_dir.join("HEAD").exists() {
                continue;
            }
            let lock_path = git_dir.with_extension("lock");
            let Some(_operation_lock) = try_acquire_file_lock(&lock_path, STALE_LOCK_AFTER)? else {
                report.locked += 1;
                continue;
            };
            report.gc_attempted += 1;
            if !run_git_gc(&git_dir) {
                report.gc_failed += 1;
            }
        }
    }
    write_cleanup_marker(root, now)?;
    Ok(report)
}

fn run_git_gc(git_dir: &Path) -> bool {
    Command::new("git")
        .arg("--git-dir")
        .arg(git_dir)
        .arg("gc")
        .arg(format!("--prune={SNAPSHOT_PRUNE}"))
        .output()
        .ok()
        .filter(|output| output.status.success())
        .is_some()
}

fn cleanup_marker_fresh(root: &Path, now: u64) -> bool {
    let Ok(text) = fs::read_to_string(root.join(".cleanup-last-run")) else {
        return false;
    };
    let Ok(last) = text.trim().parse::<u64>() else {
        return false;
    };
    now.saturating_sub(last) < CLEANUP_INTERVAL.as_millis() as u64
}

fn write_cleanup_marker(root: &Path, now: u64) -> Result<()> {
    fs::write(root.join(".cleanup-last-run"), format!("{now}\n"))?;
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Debug)]
struct FileLock {
    path: PathBuf,
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn acquire_file_lock(path: &Path, stale_after: Duration, timeout: Duration) -> Result<FileLock> {
    let started = Instant::now();
    loop {
        if let Some(lock) = try_acquire_file_lock(path, stale_after)? {
            return Ok(lock);
        }
        if started.elapsed() >= timeout {
            return Err(Error::Message(path.display().to_string()));
        }
        thread::sleep(OPERATION_LOCK_POLL);
    }
}

fn try_acquire_file_lock(path: &Path, stale_after: Duration) -> Result<Option<FileLock>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match create_lock_file(path) {
        Ok(true) => Ok(Some(FileLock {
            path: path.to_path_buf(),
        })),
        Ok(false) => {
            if lock_is_stale(path, stale_after) {
                match fs::remove_file(path) {
                    Ok(()) => {}
                    Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                    Err(_) => return Ok(None),
                }
                if create_lock_file(path)? {
                    return Ok(Some(FileLock {
                        path: path.to_path_buf(),
                    }));
                }
            }
            Ok(None)
        }
        Err(err) => Err(err.into()),
    }
}

fn create_lock_file(path: &Path) -> io::Result<bool> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            let _ = writeln!(file, "{}", now_ms());
            Ok(true)
        }
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(err) => Err(err),
    }
}

fn lock_is_stale(path: &Path, stale_after: Duration) -> bool {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age > stale_after)
}

pub(crate) fn remove_worktree_path(path: &Path) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn init_git_cwd(cwd: &Path) {
        fs::create_dir_all(cwd).expect("cwd");
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(cwd)
                .arg("init")
                .output()
                .expect("git init")
                .status
                .success()
        );
    }

    #[test]
    fn workspace_snapshot_id_is_stable_safe_and_path_opaque() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("work");
        fs::create_dir_all(&cwd).expect("cwd");

        let id = workspace_snapshot_id(&cwd).expect("workspace id");

        assert_eq!(id.len(), 34);
        assert!(id.starts_with("w_"));
        assert!(id[2..].chars().all(|ch| ch.is_ascii_hexdigit()));
        assert!(!id.contains("work"));
        assert_eq!(id, workspace_snapshot_id(&cwd).expect("stable id"));
    }

    #[test]
    fn workspace_git_dir_is_shared_by_cwd_and_not_session_scoped() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("snapshots");
        let cwd = temp.path().join("work");
        fs::create_dir_all(&cwd).expect("cwd");
        let first = SnapshotStore::new(root.clone(), cwd.clone());
        let second = SnapshotStore::new(root.clone(), cwd.clone());
        let other = SnapshotStore::new(root, temp.path().join("other"));

        assert_eq!(
            first.git_dir().expect("first"),
            second.git_dir().expect("second")
        );
        assert_ne!(
            first.workspace_id().expect("first id"),
            other.workspace_id().expect("other id")
        );
    }

    #[test]
    fn track_writes_workspace_store_without_session_directory() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("snapshots");
        let cwd = temp.path().join("work");
        init_git_cwd(&cwd);
        fs::write(cwd.join("tracked.txt"), "base\n").expect("file");
        let snapshots = SnapshotStore::new(root.clone(), cwd);

        let hash = snapshots.track().expect("track").expect("snapshot");

        assert!(!hash.is_empty());
        assert!(snapshots.git_dir().expect("git dir").join("HEAD").exists());
        assert!(!root.join("sessions").exists());
    }

    #[test]
    fn restore_reads_from_workspace_store() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("snapshots");
        let cwd = temp.path().join("work");
        init_git_cwd(&cwd);
        let file = cwd.join("tracked.txt");
        fs::write(&file, "base\n").expect("base");
        let snapshots = SnapshotStore::new(root, cwd);
        let before = snapshots.track().expect("track").expect("snapshot");
        fs::write(&file, "changed\n").expect("changed");

        snapshots.restore(&before).expect("restore");

        assert_eq!(fs::read_to_string(file).expect("file"), "base\n");
    }

    #[test]
    fn cleanup_marker_throttles_workspace_gc() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("snapshots");
        fs::create_dir_all(&root).expect("root");
        write_cleanup_marker(&root, now_ms()).expect("marker");

        let report = maybe_cleanup_snapshot_root(&root).expect("cleanup");

        assert!(!report.ran);
        assert_eq!(report.gc_attempted, 0);
    }

    #[test]
    fn cleanup_runs_after_stale_marker() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("snapshots");
        fs::create_dir_all(&root).expect("root");
        write_cleanup_marker(&root, 0).expect("marker");

        let report = maybe_cleanup_snapshot_root(&root).expect("cleanup");

        assert!(report.ran);
        assert_eq!(report.locked, 0);
        assert!(cleanup_marker_fresh(&root, now_ms()));
    }

    #[test]
    fn cleanup_skips_workspace_with_operation_lock() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("snapshots");
        let workspaces = root.join("workspaces");
        let git_dir = workspaces.join("w_locked");
        fs::create_dir_all(&git_dir).expect("git dir");
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").expect("head");
        fs::write(workspaces.join("w_locked.lock"), "locked\n").expect("lock");
        write_cleanup_marker(&root, 0).expect("marker");

        let report = maybe_cleanup_snapshot_root(&root).expect("cleanup");

        assert!(report.ran);
        assert_eq!(report.locked, 1);
        assert_eq!(report.gc_attempted, 0);
    }
}
