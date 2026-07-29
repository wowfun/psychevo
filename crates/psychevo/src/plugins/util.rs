use std::fs;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use super::materialization::{BoundedEntryKind, bounded_tree};
use crate::error::Result;

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
    let tree = bounded_tree(root)?;
    let mut hasher = Sha256::new();
    for entry in tree
        .entries
        .into_iter()
        .filter(|entry| entry.kind == BoundedEntryKind::File)
    {
        hasher.update(entry.relative.to_string_lossy().as_bytes());
        hasher.update([0]);
        let mut file = fs::File::open(root.join(&entry.relative))?;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        hasher.update([0]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn external_plugin_fingerprint(
    package_root: Option<&Path>,
    identity: &str,
    version: Option<&str>,
) -> Result<String> {
    if let Some(root) = package_root {
        return directory_fingerprint(root);
    }
    let mut hasher = Sha256::new();
    hasher.update(identity.as_bytes());
    hasher.update([0]);
    hasher.update(version.unwrap_or("unknown").as_bytes());
    Ok(format!("metadata:{:x}", hasher.finalize()))
}
