#[allow(unused_imports)]
pub(crate) use super::*;

use tempfile::NamedTempFile;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FileVersion {
    pub(crate) mtime: SystemTime,
}

#[derive(Debug)]
pub(crate) struct FileSnapshot {
    pub(crate) bytes: Vec<u8>,
    pub(crate) version: FileVersion,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum MutationConflict {
    #[error("{} already exists; no changes were applied", path.display())]
    TargetExists { path: PathBuf },
    #[error("{} no longer exists; no changes were applied", path.display())]
    TargetMissing { path: PathBuf },
    #[error(
        "{} already exists and has not been fully read by this agent. Read the complete existing file, then retry the replacement or deletion; no changes were applied",
        path.display()
    )]
    NotRead { path: PathBuf },
    #[error(
        "{} was last read through a partial or truncated view. Read the complete file before replacing or deleting it; no changes were applied",
        path.display()
    )]
    PartialRead { path: PathBuf },
    #[error(
        "{} was modified by sibling agent {writer:?} after this agent last read it. Read the file again; no changes were applied",
        path.display()
    )]
    SiblingWrite { path: PathBuf, writer: String },
    #[error(
        "{} changed on disk since the expected version was read. Read the file again; no changes were applied",
        path.display()
    )]
    Modified { path: PathBuf },
    #[error(
        "{} does not expose a usable modification time, so a safe conditional mutation cannot be performed",
        path.display()
    )]
    VersionUnavailable { path: PathBuf },
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum MutationError {
    #[error(transparent)]
    Conflict(#[from] MutationConflict),
    #[error("file mutation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Message(String),
}

impl From<MutationError> for Error {
    fn from(error: MutationError) -> Self {
        match error {
            MutationError::Io(error) => Error::Io(error),
            MutationError::Conflict(error) => Error::Message(error.to_string()),
            MutationError::Message(message) => Error::Message(message),
        }
    }
}

impl From<MutationConflict> for Error {
    fn from(error: MutationConflict) -> Self {
        Error::Message(error.to_string())
    }
}

pub(crate) type MutationResult<T> = std::result::Result<T, MutationError>;

pub(crate) trait FileMutationBackend {
    fn snapshot(&self, path: &Path) -> MutationResult<FileSnapshot>;
    fn create(&self, task_id: &str, path: &Path, content: &[u8]) -> MutationResult<()>;
    fn replace(
        &self,
        task_id: &str,
        path: &Path,
        expected: FileVersion,
        content: &[u8],
    ) -> MutationResult<()>;
    fn delete(&self, task_id: &str, path: &Path, expected: FileVersion) -> MutationResult<()>;
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LocalFileMutation;

pub(crate) const LOCAL_FILE_MUTATION: LocalFileMutation = LocalFileMutation;

impl FileMutationBackend for LocalFileMutation {
    fn snapshot(&self, path: &Path) -> MutationResult<FileSnapshot> {
        let before = current_file_version(path)?;
        let bytes = fs::read(path)?;
        let after = current_file_version(path)?;
        if before != after {
            return Err(MutationConflict::Modified {
                path: path.to_path_buf(),
            }
            .into());
        }
        Ok(FileSnapshot {
            bytes,
            version: after,
        })
    }

    fn create(&self, task_id: &str, path: &Path, content: &[u8]) -> MutationResult<()> {
        if path.exists() {
            return Err(MutationConflict::TargetExists {
                path: path.to_path_buf(),
            }
            .into());
        }
        let parent = mutation_parent(path)?;
        fs::create_dir_all(parent)?;
        let temp = prepared_temp_file(parent, content, None)?;
        temp.persist_noclobber(path)
            .map_err(|error| map_persist_noclobber_error(path, error.error))?;
        note_file_write(task_id, path);
        Ok(())
    }

    fn replace(
        &self,
        task_id: &str,
        path: &Path,
        expected: FileVersion,
        content: &[u8],
    ) -> MutationResult<()> {
        ensure_file_version(path, expected)?;
        let metadata = fs::metadata(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                MutationError::Conflict(MutationConflict::TargetMissing {
                    path: path.to_path_buf(),
                })
            } else {
                MutationError::Io(error)
            }
        })?;
        let parent = mutation_parent(path)?;
        let temp = prepared_temp_file(parent, content, Some(metadata.permissions()))?;
        ensure_file_version(path, expected)?;
        temp.persist(path)
            .map_err(|error| MutationError::Io(error.error))?;
        note_file_write(task_id, path);
        Ok(())
    }

    fn delete(&self, task_id: &str, path: &Path, expected: FileVersion) -> MutationResult<()> {
        ensure_file_version(path, expected)?;
        fs::remove_file(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                MutationError::Conflict(MutationConflict::TargetMissing {
                    path: path.to_path_buf(),
                })
            } else {
                MutationError::Io(error)
            }
        })?;
        note_file_write(task_id, path);
        Ok(())
    }
}

fn mutation_parent(path: &Path) -> MutationResult<&Path> {
    path.parent().ok_or_else(|| {
        MutationError::Message(format!("mutation target has no parent: {}", path.display()))
    })
}

fn prepared_temp_file(
    parent: &Path,
    content: &[u8],
    permissions: Option<fs::Permissions>,
) -> MutationResult<NamedTempFile> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(".psychevo-write-");
    if let Some(permissions) = permissions.as_ref() {
        builder.permissions(permissions.clone());
    }
    #[cfg(unix)]
    if permissions.is_none() {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o666));
    }
    let mut temp = builder.tempfile_in(parent)?;
    temp.write_all(content)?;
    temp.flush()?;
    let verify = fs::read(temp.path())?;
    if verify != content {
        return Err(MutationError::Message(format!(
            "temporary-file verification failed for {}",
            parent.display()
        )));
    }
    Ok(temp)
}

fn map_persist_noclobber_error(path: &Path, error: std::io::Error) -> MutationError {
    if error.kind() == std::io::ErrorKind::AlreadyExists || path.exists() {
        MutationConflict::TargetExists {
            path: path.to_path_buf(),
        }
        .into()
    } else {
        MutationError::Io(error)
    }
}

pub(crate) fn current_file_version(
    path: &Path,
) -> std::result::Result<FileVersion, MutationConflict> {
    let metadata = fs::metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            MutationConflict::TargetMissing {
                path: path.to_path_buf(),
            }
        } else {
            MutationConflict::VersionUnavailable {
                path: path.to_path_buf(),
            }
        }
    })?;
    let mtime = metadata
        .modified()
        .map_err(|_| MutationConflict::VersionUnavailable {
            path: path.to_path_buf(),
        })?;
    Ok(FileVersion { mtime })
}

pub(crate) fn ensure_file_version(
    path: &Path,
    expected: FileVersion,
) -> std::result::Result<(), MutationConflict> {
    if current_file_version(path)? == expected {
        Ok(())
    } else {
        Err(MutationConflict::Modified {
            path: path.to_path_buf(),
        })
    }
}

#[derive(Clone)]
pub(crate) struct ReadStamp {
    pub(crate) mtime: Option<SystemTime>,
    pub(crate) seq: u64,
    pub(crate) partial: bool,
}

pub(crate) struct FileState {
    pub(crate) reads: HashMap<String, HashMap<PathBuf, ReadStamp>>,
    pub(crate) last_writer: HashMap<PathBuf, (String, u64)>,
    pub(crate) seq: u64,
}

impl FileState {
    pub(crate) fn next_seq(&mut self) -> u64 {
        self.seq = self.seq.saturating_add(1);
        self.seq
    }
}

pub(crate) static FILE_STATE: LazyLock<Mutex<FileState>> = LazyLock::new(|| {
    Mutex::new(FileState {
        reads: HashMap::new(),
        last_writer: HashMap::new(),
        seq: 0,
    })
});

pub(crate) static PATH_LOCKS: LazyLock<(Mutex<HashSet<PathBuf>>, Condvar)> =
    LazyLock::new(|| (Mutex::new(HashSet::new()), Condvar::new()));

pub(crate) struct FilePathLocks {
    pub(crate) paths: Vec<PathBuf>,
}

impl Drop for FilePathLocks {
    fn drop(&mut self) {
        let (locked, wake) = &*PATH_LOCKS;
        let mut locked = locked.lock().expect("path lock state");
        for path in &self.paths {
            locked.remove(path);
        }
        wake.notify_all();
    }
}

pub(crate) fn acquire_path_locks(paths: &[PathBuf]) -> FilePathLocks {
    let mut paths = paths.to_vec();
    paths.sort();
    paths.dedup();
    let (locked, wake) = &*PATH_LOCKS;
    let mut state = locked.lock().expect("path lock state");
    loop {
        if paths.iter().all(|path| !state.contains(path)) {
            for path in &paths {
                state.insert(path.clone());
            }
            return FilePathLocks { paths };
        }
        state = wake.wait(state).expect("path lock state");
    }
}

pub(crate) fn record_file_read(task_id: &str, path: &Path, version: FileVersion, partial: bool) {
    let mut state = FILE_STATE.lock().expect("file state");
    let seq = state.next_seq();
    let reads = state.reads.entry(task_id.to_string()).or_default();
    reads.insert(
        path.to_path_buf(),
        ReadStamp {
            mtime: Some(version.mtime),
            seq,
            partial,
        },
    );
    cap_map(reads, 4096);
}

pub(crate) fn note_file_write(task_id: &str, path: &Path) {
    let mtime = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok();
    let mut state = FILE_STATE.lock().expect("file state");
    let seq = state.next_seq();
    state
        .last_writer
        .insert(path.to_path_buf(), (task_id.to_string(), seq));
    if state.last_writer.len() > 4096
        && let Some(key) = state.last_writer.keys().next().cloned()
    {
        state.last_writer.remove(&key);
    }
    let reads = state.reads.entry(task_id.to_string()).or_default();
    reads.insert(
        path.to_path_buf(),
        ReadStamp {
            mtime,
            seq,
            partial: false,
        },
    );
    cap_map(reads, 4096);
}

pub(crate) fn require_fresh_read(
    task_id: &str,
    path: &Path,
) -> std::result::Result<FileVersion, MutationConflict> {
    let path = path.to_path_buf();
    let (stamp, last_writer) = {
        let state = FILE_STATE.lock().expect("file state");
        (
            state
                .reads
                .get(task_id)
                .and_then(|reads| reads.get(&path))
                .cloned(),
            state.last_writer.get(&path).cloned(),
        )
    };
    let Some(stamp) = stamp else {
        return Err(MutationConflict::NotRead { path });
    };
    if stamp.partial {
        return Err(MutationConflict::PartialRead { path });
    }
    if let Some((writer, writer_seq)) = last_writer
        && writer != task_id
        && writer_seq > stamp.seq
    {
        return Err(MutationConflict::SiblingWrite { path, writer });
    }
    let Some(mtime) = stamp.mtime else {
        return Err(MutationConflict::VersionUnavailable { path });
    };
    let version = FileVersion { mtime };
    ensure_file_version(&path, version)?;
    Ok(version)
}

pub(crate) fn cap_map<V>(map: &mut HashMap<PathBuf, V>, max_len: usize) {
    while map.len() > max_len {
        let Some(key) = map.keys().next().cloned() else {
            break;
        };
        map.remove(&key);
    }
}

#[cfg(test)]
pub(crate) mod file_mutation_tests {
    use super::*;

    #[test]
    fn local_mutation_create_is_no_clobber_and_replace_is_atomic_visible() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("note.txt");
        LOCAL_FILE_MUTATION
            .create("creator", &path, b"one\n")
            .expect("create");
        let duplicate = LOCAL_FILE_MUTATION
            .create("creator", &path, b"two\n")
            .expect_err("no clobber");
        assert!(matches!(
            duplicate,
            MutationError::Conflict(MutationConflict::TargetExists { .. })
        ));
        assert_eq!(fs::read(&path).expect("created"), b"one\n");

        let snapshot = LOCAL_FILE_MUTATION.snapshot(&path).expect("snapshot");
        LOCAL_FILE_MUTATION
            .replace("creator", &path, snapshot.version, b"three\n")
            .expect("replace");
        assert_eq!(fs::read(&path).expect("replaced"), b"three\n");
        assert!(fs::read_dir(temp.path()).expect("dir").all(|entry| {
            !entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".psychevo-write-")
        }));
    }

    #[test]
    fn local_mutation_rejects_changed_mtime_without_replacing_external_content() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("note.txt");
        fs::write(&path, "one\n").expect("seed");
        let snapshot = LOCAL_FILE_MUTATION.snapshot(&path).expect("snapshot");
        fs::write(&path, "external\n").expect("external write");
        let changed = SystemTime::now() + Duration::from_secs(2);
        fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_times(fs::FileTimes::new().set_modified(changed)))
            .expect("change mtime");

        let error = LOCAL_FILE_MUTATION
            .replace("agent", &path, snapshot.version, b"agent\n")
            .expect_err("mtime conflict");
        assert!(matches!(
            error,
            MutationError::Conflict(MutationConflict::Modified { .. })
        ));
        let delete_error = LOCAL_FILE_MUTATION
            .delete("agent", &path, snapshot.version)
            .expect_err("delete mtime conflict");
        assert!(matches!(
            delete_error,
            MutationError::Conflict(MutationConflict::Modified { .. })
        ));
        assert_eq!(
            fs::read_to_string(path).expect("external content"),
            "external\n"
        );
    }
}
