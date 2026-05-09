#[derive(Debug, Clone, PartialEq, Eq)]
struct FileSearchMatch {
    path: String,
    kind: FileSearchMatchKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileSearchMatchKind {
    File,
    Directory,
}

#[derive(Debug, Clone)]
struct FileSearchResult {
    generation: u64,
    query: String,
    matches: Vec<FileSearchMatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileToken {
    row: usize,
    start_col: usize,
    end_col: usize,
    query: String,
}

struct FileSearchState {
    generation: u64,
    popup: Option<FileSearchPopupState>,
    dismissed_query: Option<String>,
    cancel: Option<Arc<AtomicBool>>,
    tx: mpsc::UnboundedSender<FileSearchResult>,
    rx: mpsc::UnboundedReceiver<FileSearchResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileSearchPopupState {
    query: String,
    matches: Vec<FileSearchMatch>,
    selected: usize,
    waiting: bool,
}

impl FileSearchState {
    fn new() -> Self {
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

    fn sync(&mut self, root: &Path, token: Option<&FileToken>) {
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

    fn start_search(&mut self, root: &Path, query: String) {
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
            let matches = search_workdir_files(&root, &query, &cancel);
            if !cancel.load(Ordering::Relaxed) {
                let _ = tx.send(FileSearchResult {
                    generation,
                    query,
                    matches,
                });
            }
        });
    }

    fn drain_results(&mut self) -> bool {
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

    fn close(&mut self) {
        self.cancel_current();
        self.popup = None;
    }

    fn dismiss(&mut self, query: Option<String>) {
        self.dismissed_query = query;
        self.close();
    }

    fn cancel_current(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    fn height(&self) -> u16 {
        let Some(popup) = &self.popup else {
            return 0;
        };
        let rows = popup.matches.len().clamp(1, FILE_POPUP_MAX_ROWS);
        (rows as u16 + 2).min(FILE_POPUP_MAX_ROWS as u16 + 2)
    }

    fn selected_path(&self) -> Option<String> {
        self.popup
            .as_ref()
            .and_then(|popup| popup.matches.get(popup.selected))
            .map(|entry| entry.path.clone())
    }

    fn move_selection(&mut self, direction: isize) {
        let Some(popup) = &mut self.popup else {
            return;
        };
        let len = popup.matches.len();
        if len == 0 {
            popup.selected = 0;
            return;
        }
        let current = popup.selected.min(len.saturating_sub(1)) as isize;
        popup.selected = (current + direction).rem_euclid(len as isize) as usize;
    }

    fn set_selection(&mut self, index: usize) {
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
