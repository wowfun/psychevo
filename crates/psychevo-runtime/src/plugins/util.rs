use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

pub(crate) fn sanitize_path_segment(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    if out.trim_matches('-').is_empty() {
        "plugin".to_string()
    } else {
        out
    }
}

pub(crate) fn source_slug(source_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source_id.as_bytes());
    let digest = hasher.finalize();
    digest[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn directory_fingerprint(root: &Path) -> Result<String> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort();
    let mut hasher = Sha256::new();
    for relative in files {
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update([0]);
        let path = root.join(&relative);
        let bytes = fs::read(&path)?;
        hasher.update(bytes);
        hasher.update([0]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(crate) fn directory_size(root: &Path) -> Result<u64> {
    let mut total = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            if metadata.file_type().is_symlink() {
                return Err(Error::Config(format!(
                    "plugin package contains unsupported symlink: {}",
                    entry.path().display()
                )));
            }
            if metadata.is_dir() {
                stack.push(entry.path());
            } else if metadata.is_file() {
                total += metadata.len();
            }
        }
    }
    Ok(total)
}

fn collect_files(root: &Path, dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(Error::Config(format!(
                "plugin package contains unsupported symlink: {}",
                path.display()
            )));
        }
        if metadata.is_dir() {
            collect_files(root, &path, files)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|err| Error::Message(format!("failed to relativize package path: {err}")))?
                .to_path_buf();
            files.push(relative);
        }
    }
    Ok(())
}
