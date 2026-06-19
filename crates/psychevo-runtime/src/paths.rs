use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;
use sha2::{Digest, Sha256};

pub(crate) fn canonical_workdir(path: &Path) -> Result<PathBuf> {
    fs::create_dir_all(path)?;
    Ok(path.canonicalize()?)
}

pub fn canonicalize_workdir(path: &Path) -> Result<PathBuf> {
    canonical_workdir(path)
}

pub fn workspace_snapshot_id(path: &Path) -> Result<String> {
    let workdir = canonical_workdir(path)?;
    Ok(workspace_snapshot_id_for_canonical_path(&workdir))
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
