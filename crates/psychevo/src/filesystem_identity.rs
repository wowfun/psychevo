use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FilesystemIdentity {
    pub(crate) requested_absolute: PathBuf,
    pub(crate) resolved: PathBuf,
    pub(crate) uri: String,
}

pub(crate) fn resolve(raw: &str, cwd: &Path) -> Result<FilesystemIdentity> {
    let requested_absolute = crate::host_paths::resolve_input_path(raw, cwd)?;
    let resolved = canonicalize_deepest_existing(&requested_absolute)?;
    let uri = crate::host_paths::path_ref_for_native_path(&resolved).uri;
    Ok(FilesystemIdentity {
        requested_absolute,
        resolved,
        uri,
    })
}

pub(crate) fn canonicalize_deepest_existing(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(Error::Message("empty filesystem path".to_string()));
    }

    let mut current = crate::host_paths::normalized_native_path(path);
    let mut tail = PathBuf::new();
    loop {
        match std::fs::symlink_metadata(&current) {
            Ok(_) => {
                let mut resolved = current.canonicalize()?;
                if !tail.as_os_str().is_empty() {
                    resolved.push(tail);
                }
                return Ok(resolved);
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }

        let Some(name) = current.file_name().map(|name| name.to_os_string()) else {
            return Err(Error::Message(format!(
                "no existing ancestor for filesystem path {}",
                path.display()
            )));
        };
        let mut next_tail = PathBuf::from(name);
        if !tail.as_os_str().is_empty() {
            next_tail.push(tail);
        }
        tail = next_tail;
        if !current.pop() {
            return Err(Error::Message(format!(
                "no existing ancestor for filesystem path {}",
                path.display()
            )));
        }
    }
}

pub(crate) fn is_within(root: &Path, target: &Path) -> bool {
    target == root || target.starts_with(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn resolves_missing_descendant_through_directory_symlink() {
        let work = tempfile::tempdir().expect("work");
        let outside = tempfile::tempdir().expect("outside");
        std::os::unix::fs::symlink(outside.path(), work.path().join("linked")).expect("symlink");

        let identity = resolve("linked/new/file.txt", work.path()).expect("identity");

        assert_eq!(
            identity.requested_absolute,
            work.path().join("linked/new/file.txt")
        );
        assert_eq!(identity.resolved, outside.path().join("new/file.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_dangling_symlink_identity() {
        let work = tempfile::tempdir().expect("work");
        std::os::unix::fs::symlink(work.path().join("missing"), work.path().join("dangling"))
            .expect("symlink");

        let error = resolve("dangling/file.txt", work.path()).expect_err("dangling identity");

        assert!(error.to_string().contains("No such file"));
    }
}
