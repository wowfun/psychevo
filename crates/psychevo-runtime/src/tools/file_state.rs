#[derive(Clone)]
struct ReadStamp {
    mtime: Option<SystemTime>,
    seq: u64,
    partial: bool,
}

struct FileState {
    reads: HashMap<String, HashMap<PathBuf, ReadStamp>>,
    last_writer: HashMap<PathBuf, (String, u64)>,
    seq: u64,
}

impl FileState {
    fn next_seq(&mut self) -> u64 {
        self.seq = self.seq.saturating_add(1);
        self.seq
    }
}

static FILE_STATE: LazyLock<Mutex<FileState>> = LazyLock::new(|| {
    Mutex::new(FileState {
        reads: HashMap::new(),
        last_writer: HashMap::new(),
        seq: 0,
    })
});

static PATH_LOCKS: LazyLock<(Mutex<HashSet<PathBuf>>, Condvar)> =
    LazyLock::new(|| (Mutex::new(HashSet::new()), Condvar::new()));

pub(crate) struct FilePathLocks {
    paths: Vec<PathBuf>,
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

pub(crate) fn record_file_read(task_id: &str, path: &Path, partial: bool) {
    let mtime = fs_mtime(path);
    let mut state = FILE_STATE.lock().expect("file state");
    let seq = state.next_seq();
    let reads = state.reads.entry(task_id.to_string()).or_default();
    reads.insert(
        path.to_path_buf(),
        ReadStamp {
            mtime,
            seq,
            partial,
        },
    );
    cap_map(reads, 4096);
}

pub(crate) fn note_file_write(task_id: &str, path: &Path) {
    let mtime = fs_mtime(path);
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

pub(crate) fn stale_file_warning(task_id: &str, path: &Path) -> Option<String> {
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

    if stamp.is_none() && last_writer.is_none() {
        return None;
    }

    let current_mtime = fs_mtime(&path);

    if let Some((writer, writer_seq)) = last_writer
        && writer != task_id
    {
        match &stamp {
            Some(stamp) if writer_seq > stamp.seq => {
                return Some(format!(
                    "{} was modified by sibling agent {writer:?} after this agent last read it. Re-read before writing to avoid overwriting those changes.",
                    path.display()
                ));
            }
            None => {
                return Some(format!(
                    "{} was modified by sibling agent {writer:?}, but this agent has not read it. Read it before writing.",
                    path.display()
                ));
            }
            _ => {}
        }
    }

    if let Some(stamp) = stamp {
        if current_mtime != stamp.mtime {
            return Some(format!(
                "{} was modified on disk since this agent last read it. Re-read before writing.",
                path.display()
            ));
        }
        if stamp.partial {
            return Some(format!(
                "{} was last read with a partial offset/limit view. Re-read the whole file before overwriting it.",
                path.display()
            ));
        }
        return None;
    }

    Some(format!(
        "{} was not read by this agent. Read the file first so the edit is based on current content.",
        path.display()
    ))
}

fn fs_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|metadata| metadata.modified()).ok()
}

fn cap_map<V>(map: &mut HashMap<PathBuf, V>, max_len: usize) {
    while map.len() > max_len {
        let Some(key) = map.keys().next().cloned() else {
            break;
        };
        map.remove(&key);
    }
}
