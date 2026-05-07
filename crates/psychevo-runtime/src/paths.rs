use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;

pub(crate) fn canonical_workdir(path: &Path) -> Result<PathBuf> {
    fs::create_dir_all(path)?;
    Ok(path.canonicalize()?)
}

pub fn canonicalize_workdir(path: &Path) -> Result<PathBuf> {
    canonical_workdir(path)
}
