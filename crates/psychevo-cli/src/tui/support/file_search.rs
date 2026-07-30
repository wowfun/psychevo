#[allow(unused_imports)]
pub(crate) use super::*;
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileSearchMatch {
    pub(crate) path: String,
    pub(crate) kind: FileSearchMatchKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileSearchMatchKind {
    File,
    Directory,
}

#[derive(Debug, Clone)]
pub(crate) struct FileSearchResult {
    pub(crate) generation: u64,
    pub(crate) query: String,
    pub(crate) matches: Vec<FileSearchMatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileToken {
    pub(crate) row: usize,
    pub(crate) start_col: usize,
    pub(crate) end_col: usize,
    pub(crate) query: String,
}

pub(crate) struct FileSearchState {
    pub(crate) generation: u64,
    pub(crate) popup: Option<FileSearchPopupState>,
    pub(crate) dismissed_query: Option<String>,
    worker: FileSearchWorker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileSearchPopupState {
    pub(crate) query: String,
    pub(crate) matches: Vec<FileSearchMatch>,
    pub(crate) selected: usize,
    pub(crate) waiting: bool,
}

impl FileSearchState {
    pub(crate) fn new() -> Self {
        Self {
            generation: 0,
            popup: None,
            dismissed_query: None,
            worker: FileSearchWorker::new(),
        }
    }

    pub(crate) fn sync(&mut self, root: &Path, token: Option<&FileToken>) {
        let Some(token) = token else {
            self.close();
            self.dismissed_query = None;
            return;
        };
        if self.dismissed_query.as_deref() == Some(token.query.as_str()) {
            self.cancel_current();
            self.popup = None;
            return;
        }
        if self
            .popup
            .as_ref()
            .is_some_and(|popup| popup.query == token.query)
        {
            return;
        }
        self.dismissed_query = None;
        self.start_search(root, token.query.clone());
    }

    pub(crate) fn start_search(&mut self, root: &Path, query: String) {
        self.cancel_current();
        self.generation = self.generation.wrapping_add(1);
        self.popup = Some(FileSearchPopupState {
            query: query.clone(),
            matches: Vec::new(),
            selected: 0,
            waiting: true,
        });
        let generation = self.generation;
        self.worker.submit(FileSearchRequest {
            generation,
            root: root.to_path_buf(),
            query,
        });
    }

    pub(crate) fn drain_results(&mut self) -> bool {
        let mut changed = false;
        while let Some(result) = self.worker.take_result() {
            if result.generation != self.generation {
                continue;
            }
            let Some(popup) = &mut self.popup else {
                continue;
            };
            if popup.query != result.query {
                continue;
            }
            popup.matches = result.matches;
            popup.waiting = false;
            popup.selected = popup.selected.min(popup.matches.len().saturating_sub(1));
            changed = true;
        }
        changed
    }

    pub(crate) fn close(&mut self) {
        self.cancel_current();
        self.popup = None;
    }

    pub(crate) fn dismiss(&mut self, query: Option<String>) {
        self.dismissed_query = query;
        self.close();
    }

    pub(crate) fn cancel_current(&mut self) {
        self.worker.cancel();
    }

    pub(crate) fn selected_path(&self) -> Option<String> {
        self.popup
            .as_ref()
            .and_then(|popup| popup.matches.get(popup.selected))
            .map(|entry| entry.path.clone())
    }

    pub(crate) fn set_selection(&mut self, index: usize) {
        let Some(popup) = &mut self.popup else {
            return;
        };
        let len = popup.matches.len();
        popup.selected = if len == 0 {
            0
        } else {
            index.min(len.saturating_sub(1))
        };
    }

    #[cfg(test)]
    pub(crate) fn worker_probe(&self) -> (Arc<std::sync::atomic::AtomicUsize>, Arc<AtomicBool>) {
        (
            Arc::clone(&self.worker.thread_starts),
            Arc::clone(&self.worker.thread_alive),
        )
    }

    #[cfg(test)]
    pub(crate) fn inject_result(&self, result: FileSearchResult) {
        self.worker.store_result(result);
    }
}

struct FileSearchRequest {
    generation: u64,
    root: PathBuf,
    query: String,
}

#[derive(Default)]
struct FileSearchWorkerState {
    latest: Option<FileSearchRequest>,
    active_generation: Option<u64>,
    result: Option<FileSearchResult>,
    stopped: bool,
}

struct FileSearchWorker {
    shared: Arc<(Mutex<FileSearchWorkerState>, std::sync::Condvar)>,
    handle: Option<std::thread::JoinHandle<()>>,
    #[cfg(test)]
    thread_starts: Arc<std::sync::atomic::AtomicUsize>,
    #[cfg(test)]
    thread_alive: Arc<AtomicBool>,
}

impl FileSearchWorker {
    fn new() -> Self {
        let shared = Arc::new((
            Mutex::new(FileSearchWorkerState::default()),
            std::sync::Condvar::new(),
        ));
        let worker_shared = Arc::clone(&shared);
        #[cfg(test)]
        let thread_starts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        #[cfg(test)]
        let worker_thread_starts = Arc::clone(&thread_starts);
        #[cfg(test)]
        let thread_alive = Arc::new(AtomicBool::new(false));
        #[cfg(test)]
        let worker_thread_alive = Arc::clone(&thread_alive);
        let handle = std::thread::spawn(move || {
            #[cfg(test)]
            {
                worker_thread_starts.fetch_add(1, Ordering::Relaxed);
                worker_thread_alive.store(true, Ordering::Release);
            }
            loop {
                let request = {
                    let (lock, wake) = &*worker_shared;
                    let mut state = lock.lock().expect("file search worker");
                    while state.latest.is_none() && !state.stopped {
                        state = wake.wait(state).expect("file search worker");
                    }
                    if state.stopped {
                        break;
                    }
                    state.latest.take().expect("latest file search")
                };
                let generation = request.generation;
                let cancelled = || {
                    let state = worker_shared.0.lock().expect("file search worker");
                    state.stopped || state.active_generation != Some(generation)
                };
                let matches = search_cwd_files_while(&request.root, &request.query, cancelled);
                let mut state = worker_shared.0.lock().expect("file search worker");
                if !state.stopped && state.active_generation == Some(generation) {
                    state.result = Some(FileSearchResult {
                        generation,
                        query: request.query,
                        matches,
                    });
                }
            }
            #[cfg(test)]
            worker_thread_alive.store(false, Ordering::Release);
        });
        Self {
            shared,
            handle: Some(handle),
            #[cfg(test)]
            thread_starts,
            #[cfg(test)]
            thread_alive,
        }
    }

    fn submit(&self, request: FileSearchRequest) {
        let (lock, wake) = &*self.shared;
        let mut state = lock.lock().expect("file search worker");
        state.active_generation = Some(request.generation);
        state.latest = Some(request);
        state.result = None;
        wake.notify_one();
    }

    fn cancel(&self) {
        let (lock, _) = &*self.shared;
        let mut state = lock.lock().expect("file search worker");
        state.active_generation = None;
        state.latest = None;
        state.result = None;
    }

    fn take_result(&self) -> Option<FileSearchResult> {
        self.shared
            .0
            .lock()
            .expect("file search worker")
            .result
            .take()
    }

    #[cfg(test)]
    fn store_result(&self, result: FileSearchResult) {
        self.shared.0.lock().expect("file search worker").result = Some(result);
    }

    fn stop(&mut self) {
        let (lock, wake) = &*self.shared;
        {
            let mut state = lock.lock().expect("file search worker");
            state.stopped = true;
            state.latest = None;
            state.active_generation = None;
            state.result = None;
        }
        wake.notify_one();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for FileSearchWorker {
    fn drop(&mut self) {
        self.stop();
    }
}
