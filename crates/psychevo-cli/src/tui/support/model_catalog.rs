struct TuiSessionDisplaySummary {
    summary: SessionSummary,
    visible_message_count: usize,
}

type ClipboardSink = Arc<dyn Fn(&str) -> io::Result<()> + Send + Sync>;

#[derive(Default)]
struct ModelCatalogCache {
    providers: BTreeMap<String, ModelProviderCatalogState>,
    tasks: BTreeMap<String, JoinHandle<ModelCatalogFetchResult>>,
}

struct ModelProviderCatalogState {
    provider: ModelCatalogProvider,
    status: ModelCatalogStatus,
    fetched: Vec<ModelCatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ModelCatalogStatus {
    NotFetched,
    Fetching,
    Fetched,
    Failed(String),
}

struct ModelCatalogFetchResult {
    provider: String,
    result: std::result::Result<Vec<ModelCatalogEntry>, String>,
}

impl ModelCatalogCache {
    fn any_fetching(&self) -> bool {
        self.providers
            .values()
            .any(|state| matches!(state.status, ModelCatalogStatus::Fetching))
    }

    fn abort_unfinished(&mut self) {
        for (_, task) in std::mem::take(&mut self.tasks) {
            task.abort();
        }
        for state in self.providers.values_mut() {
            if matches!(state.status, ModelCatalogStatus::Fetching) {
                state.status = if state.fetched.is_empty() {
                    ModelCatalogStatus::NotFetched
                } else {
                    ModelCatalogStatus::Fetched
                };
            }
        }
    }
}

