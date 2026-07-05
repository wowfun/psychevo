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
    pub(crate) cancel: Option<Arc<AtomicBool>>,
    pub(crate) tx: mpsc::UnboundedSender<FileSearchResult>,
    pub(crate) rx: mpsc::UnboundedReceiver<FileSearchResult>,
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
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            generation: 0,
            popup: None,
            dismissed_query: None,
            cancel: None,
            tx,
            rx,
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
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(Arc::clone(&cancel));
        let tx = self.tx.clone();
        let root = root.to_path_buf();
        std::thread::spawn(move || {
            let matches = search_cwd_files(&root, &query, &cancel);
            if !cancel.load(Ordering::Relaxed) {
                let _ = tx.send(FileSearchResult {
                    generation,
                    query,
                    matches,
                });
            }
        });
    }

    pub(crate) fn drain_results(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.rx.try_recv() {
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
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
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
}
