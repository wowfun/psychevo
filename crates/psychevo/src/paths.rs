use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;
use sha2::{Digest, Sha256};

pub(crate) fn canonical_cwd(path: &Path) -> Result<PathBuf> {
    fs::create_dir_all(path)?;
    Ok(normalize_canonical_cwd(path.canonicalize()?))
}

pub(crate) fn normalize_canonical_cwd(path: PathBuf) -> PathBuf {
    crate::host_paths::normalized_native_path(&path)
}

pub fn canonicalize_cwd(path: &Path) -> Result<PathBuf> {
    canonical_cwd(path)
}

pub fn workspace_snapshot_id(path: &Path) -> Result<String> {
    let cwd = canonical_cwd(path)?;
    Ok(workspace_snapshot_id_for_canonical_path(&cwd))
}

pub(crate) fn workspace_snapshot_id_for_canonical_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    let digest = Sha256::digest(text.as_bytes());
    let mut id = String::with_capacity(34);
    id.push_str("w_");
    for byte in digest.iter().take(16) {
        let _ = write!(&mut id, "{byte:02x}");
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_verbatim_windows_canonical_cwd_identity() {
        let cwd = normalize_canonical_cwd(PathBuf::from(r"\\?\C:\Users\Ada\project"));

        assert_eq!(cwd, PathBuf::from(r"C:\Users\Ada\project"));
    }

    #[test]
    fn normalizes_verbatim_unc_canonical_cwd_identity() {
        let cwd = normalize_canonical_cwd(PathBuf::from(r"\\?\UNC\server\share\project"));

        assert_eq!(cwd, PathBuf::from(r"\\server\share\project"));
    }
}
