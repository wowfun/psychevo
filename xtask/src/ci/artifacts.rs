use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn default_artifact_root(root: &Path) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    default_ci_root(root).join(now.to_string())
}

pub(crate) fn default_ci_root(root: &Path) -> PathBuf {
    root.join(".local").join(".psychevo-dev").join("ci")
}

pub(crate) fn display_path(path: &Path) -> String {
    path.display().to_string()
}
