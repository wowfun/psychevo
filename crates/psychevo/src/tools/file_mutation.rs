#[allow(unused_imports)]
pub(crate) use super::*;

use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

#[cfg(test)]
std::thread_local! {
    static SHA256_FILE_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FileVersion {
    pub(crate) len: u64,
    pub(crate) modified: SystemTime,
    pub(crate) sha256: [u8; 32],
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
        let before = current_file_metadata(path)?;
        let bytes = fs::read(path)?;
        let after = current_file_metadata(path)?;
        if before != after {
            return Err(MutationConflict::Modified {
                path: path.to_path_buf(),
            }
            .into());
        }
        Ok(FileSnapshot {
            version: FileVersion {
                len: after.len,
                modified: after.modified,
                sha256: sha256_bytes(&bytes),
            },
            bytes,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileMetadataVersion {
    len: u64,
    modified: SystemTime,
}

fn current_file_metadata(
    path: &Path,
) -> std::result::Result<FileMetadataVersion, MutationConflict> {
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
    let modified = metadata
        .modified()
        .map_err(|_| MutationConflict::VersionUnavailable {
            path: path.to_path_buf(),
        })?;
    Ok(FileMetadataVersion {
        len: metadata.len(),
        modified,
    })
}

fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn sha256_file(path: &Path) -> std::io::Result<[u8; 32]> {
    #[cfg(test)]
    SHA256_FILE_CALLS.with(|calls| calls.set(calls.get().saturating_add(1)));
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().into())
}

pub(crate) fn ensure_file_version(
    path: &Path,
    expected: FileVersion,
) -> std::result::Result<(), MutationConflict> {
    ensure_file_metadata_version(path, expected)?;
    if sha256_file(path).map_err(|_| MutationConflict::VersionUnavailable {
        path: path.to_path_buf(),
    })? != expected.sha256
    {
        return Err(MutationConflict::Modified {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

fn ensure_file_metadata_version(
    path: &Path,
    expected: FileVersion,
) -> std::result::Result<(), MutationConflict> {
    let metadata = current_file_metadata(path)?;
    if metadata.len == expected.len && metadata.modified == expected.modified {
        return Ok(());
    }
    Err(MutationConflict::Modified {
        path: path.to_path_buf(),
    })
}

#[derive(Clone, Debug)]
pub(crate) struct ReadStamp {
    pub(crate) version: FileVersion,
    pub(crate) tracker_seq: u64,
    pub(crate) observed_writer_seq: u64,
    pub(crate) partial: bool,
}

#[derive(Debug, Default)]
struct FileReadTrackerState {
    reads: HashMap<PathBuf, ReadStamp>,
    fifo: VecDeque<(PathBuf, u64)>,
    seq: u64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FileReadTracker {
    inner: Arc<Mutex<FileReadTrackerState>>,
}

impl FileReadTracker {
    fn record(&self, path: &Path, version: FileVersion, observed_writer_seq: u64, partial: bool) {
        let mut state = self.inner.lock().expect("file read tracker");
        state.seq = state.seq.saturating_add(1);
        let seq = state.seq;
        let path = path.to_path_buf();
        state.reads.insert(
            path.clone(),
            ReadStamp {
                version,
                tracker_seq: seq,
                observed_writer_seq,
                partial,
            },
        );
        state.fifo.push_back((path, seq));
        while state.fifo.len() > 4096 {
            let Some((path, evicted_seq)) = state.fifo.pop_front() else {
                break;
            };
            if state
                .reads
                .get(&path)
                .is_some_and(|stamp| stamp.tracker_seq == evicted_seq)
            {
                state.reads.remove(&path);
            }
        }
    }

    fn get(&self, path: &Path) -> Option<ReadStamp> {
        self.inner
            .lock()
            .expect("file read tracker")
            .reads
            .get(path)
            .cloned()
    }

    pub(crate) fn remove(&self, path: &Path) {
        self.inner
            .lock()
            .expect("file read tracker")
            .reads
            .remove(path);
    }
}

pub(crate) struct WorkspaceMutationCoordinatorState {
    pub(crate) active_paths: HashSet<PathBuf>,
    pub(crate) last_writer: HashMap<PathBuf, (String, u64)>,
    pub(crate) writer_fifo: VecDeque<(PathBuf, u64)>,
    pub(crate) seq: u64,
}

impl WorkspaceMutationCoordinatorState {
    pub(crate) fn next_seq(&mut self) -> u64 {
        self.seq = self.seq.saturating_add(1);
        self.seq
    }
}

pub(crate) struct WorkspaceMutationCoordinator {
    state: Mutex<WorkspaceMutationCoordinatorState>,
    wake: Condvar,
}

pub(crate) static WORKSPACE_MUTATIONS: LazyLock<WorkspaceMutationCoordinator> =
    LazyLock::new(|| WorkspaceMutationCoordinator {
        state: Mutex::new(WorkspaceMutationCoordinatorState {
            active_paths: HashSet::new(),
            last_writer: HashMap::new(),
            writer_fifo: VecDeque::new(),
            seq: 0,
        }),
        wake: Condvar::new(),
    });

pub(crate) struct FilePathLocks {
    pub(crate) paths: Vec<PathBuf>,
}

impl Drop for FilePathLocks {
    fn drop(&mut self) {
        let mut state = WORKSPACE_MUTATIONS.state.lock().expect("path lock state");
        for path in &self.paths {
            state.active_paths.remove(path);
        }
        WORKSPACE_MUTATIONS.wake.notify_all();
    }
}

pub(crate) fn acquire_path_locks(paths: &[PathBuf]) -> FilePathLocks {
    let mut paths = paths.to_vec();
    paths.sort();
    paths.dedup();
    let mut state = WORKSPACE_MUTATIONS.state.lock().expect("path lock state");
    loop {
        if paths.iter().all(|path| !state.active_paths.contains(path)) {
            for path in &paths {
                state.active_paths.insert(path.clone());
            }
            return FilePathLocks { paths };
        }
        state = WORKSPACE_MUTATIONS
            .wake
            .wait(state)
            .expect("path lock state");
    }
}

pub(crate) fn record_file_read(
    tracker: &FileReadTracker,
    path: &Path,
    version: FileVersion,
    partial: bool,
) {
    let observed_writer_seq = WORKSPACE_MUTATIONS
        .state
        .lock()
        .expect("file state")
        .last_writer
        .get(path)
        .map_or(0, |(_, seq)| *seq);
    tracker.record(path, version, observed_writer_seq, partial);
}

pub(crate) fn record_written_file(
    tracker: &FileReadTracker,
    path: &Path,
    content: &[u8],
) -> MutationResult<()> {
    let metadata = current_file_metadata(path)?;
    if metadata.len != content.len() as u64 {
        return Err(MutationConflict::Modified {
            path: path.to_path_buf(),
        }
        .into());
    }
    let observed_writer_seq = WORKSPACE_MUTATIONS
        .state
        .lock()
        .expect("file state")
        .last_writer
        .get(path)
        .map_or(0, |(_, seq)| *seq);
    tracker.record(
        path,
        FileVersion {
            len: metadata.len,
            modified: metadata.modified,
            sha256: sha256_bytes(content),
        },
        observed_writer_seq,
        false,
    );
    Ok(())
}

pub(crate) fn note_file_write(task_id: &str, path: &Path) {
    let mut state = WORKSPACE_MUTATIONS.state.lock().expect("file state");
    let seq = state.next_seq();
    state
        .last_writer
        .insert(path.to_path_buf(), (task_id.to_string(), seq));
    state.writer_fifo.push_back((path.to_path_buf(), seq));
    while state.writer_fifo.len() > 4096 {
        let Some((path, evicted_seq)) = state.writer_fifo.pop_front() else {
            break;
        };
        if state
            .last_writer
            .get(&path)
            .is_some_and(|(_, current_seq)| *current_seq == evicted_seq)
        {
            state.last_writer.remove(&path);
        }
    }
}

pub(crate) fn require_fresh_read(
    tracker: &FileReadTracker,
    task_id: &str,
    path: &Path,
) -> std::result::Result<FileVersion, MutationConflict> {
    let path = path.to_path_buf();
    let stamp = tracker.get(&path);
    let last_writer = WORKSPACE_MUTATIONS
        .state
        .lock()
        .expect("file state")
        .last_writer
        .get(&path)
        .cloned();
    let Some(stamp) = stamp else {
        return Err(MutationConflict::NotRead { path });
    };
    if stamp.partial {
        return Err(MutationConflict::PartialRead { path });
    }
    if let Some((writer, writer_seq)) = last_writer
        && writer != task_id
        && writer_seq > stamp.observed_writer_seq
    {
        return Err(MutationConflict::SiblingWrite { path, writer });
    }
    let version = stamp.version;
    Ok(version)
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

    #[test]
    fn local_mutation_rejects_same_size_content_with_restored_mtime() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("note.txt");
        fs::write(&path, "one\n").expect("seed");
        let snapshot = LOCAL_FILE_MUTATION.snapshot(&path).expect("snapshot");

        fs::write(&path, "two\n").expect("same-size external write");
        fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|file| {
                file.set_times(fs::FileTimes::new().set_modified(snapshot.version.modified))
            })
            .expect("restore mtime");

        let error = LOCAL_FILE_MUTATION
            .replace("agent", &path, snapshot.version, b"new\n")
            .expect_err("digest conflict");
        assert!(matches!(
            error,
            MutationError::Conflict(MutationConflict::Modified { .. })
        ));
        assert_eq!(fs::read_to_string(path).expect("external content"), "two\n");
    }

    #[test]
    fn local_replace_performs_one_authoritative_precommit_hash() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("note.txt");
        fs::write(&path, "old\n").expect("seed");
        let snapshot = LOCAL_FILE_MUTATION.snapshot(&path).expect("snapshot");
        SHA256_FILE_CALLS.with(|calls| calls.set(0));

        LOCAL_FILE_MUTATION
            .replace("agent", &path, snapshot.version, b"new\n")
            .expect("replace");

        assert_eq!(SHA256_FILE_CALLS.with(std::cell::Cell::get), 1);
        assert_eq!(fs::read_to_string(path).expect("replaced"), "new\n");
    }

    #[test]
    fn freshness_admission_uses_only_the_invocation_stamp_and_sibling_writer() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("note.txt");
        fs::write(&path, "old\n").expect("seed");
        let snapshot = LOCAL_FILE_MUTATION.snapshot(&path).expect("snapshot");
        let tracker = FileReadTracker::default();
        record_file_read(&tracker, &path, snapshot.version, false);
        fs::remove_file(&path).expect("remove after read");
        SHA256_FILE_CALLS.with(|calls| calls.set(0));

        assert_eq!(
            require_fresh_read(&tracker, "reader", &path).expect("stamp admission"),
            snapshot.version
        );
        assert_eq!(SHA256_FILE_CALLS.with(std::cell::Cell::get), 0);
    }

    #[test]
    fn read_tracker_is_per_invocation_bounded_and_dropped_with_its_owner() {
        let first = FileReadTracker::default();
        let shared_clone = first.clone();
        let unrelated = FileReadTracker::default();
        let version = FileVersion {
            len: 0,
            modified: SystemTime::UNIX_EPOCH,
            sha256: [0; 32],
        };
        for index in 0..=4096 {
            first.record(Path::new(&format!("/tracker/{index}")), version, 0, false);
        }
        assert!(first.get(Path::new("/tracker/0")).is_none());
        assert!(shared_clone.get(Path::new("/tracker/4096")).is_some());
        assert!(unrelated.get(Path::new("/tracker/4096")).is_none());

        let weak = Arc::downgrade(&first.inner);
        drop(first);
        assert!(
            weak.upgrade().is_some(),
            "a runtime clone still owns the read set"
        );
        drop(shared_clone);
        assert!(
            weak.upgrade().is_none(),
            "the invocation read set must be reclaimed"
        );
    }

    #[test]
    fn repeated_reads_keep_the_fifo_bounded_without_evicting_the_latest_stamp() {
        let tracker = FileReadTracker::default();
        let path = Path::new("/tracker/repeated");
        let version = FileVersion {
            len: 0,
            modified: SystemTime::UNIX_EPOCH,
            sha256: [0; 32],
        };
        for _ in 0..5000 {
            tracker.record(path, version, 0, false);
        }

        let state = tracker.inner.lock().expect("tracker");
        assert_eq!(state.reads.len(), 1);
        assert!(state.fifo.len() <= 4096);
        assert_eq!(
            state.reads.get(path).map(|stamp| stamp.tracker_seq),
            Some(state.seq)
        );
    }

    #[test]
    fn repeated_writes_to_one_path_keep_process_writer_diagnostics_bounded() {
        let path = PathBuf::from("/writer-fifo/repeated.txt");
        for index in 0..5000 {
            note_file_write(&format!("writer-{index}"), &path);
        }

        let state = WORKSPACE_MUTATIONS.state.lock().expect("file state");
        assert!(
            state.writer_fifo.len() <= 4096,
            "stale FIFO nodes grew to {} while the last-writer map held {} paths",
            state.writer_fifo.len(),
            state.last_writer.len()
        );
        assert_eq!(
            state.last_writer.get(&path).map(|(writer, _)| writer.as_str()),
            Some("writer-4999")
        );
    }

    #[test]
    fn workspace_path_lock_serializes_independent_runtime_owners() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("shared.txt");
        let first = acquire_path_locks(std::slice::from_ref(&path));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let second_path = path.clone();
        let second = thread::spawn(move || {
            let _second = acquire_path_locks(std::slice::from_ref(&second_path));
            entered_tx.send(()).expect("signal");
        });

        assert!(
            entered_rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "the second runtime owner entered while the first still held the path"
        );
        drop(first);
        entered_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("second runtime owner entered after release");
        second.join().expect("second owner");
    }
}
