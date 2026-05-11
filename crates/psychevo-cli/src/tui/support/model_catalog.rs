struct TuiSessionDisplaySummary {
    summary: SessionSummary,
    visible_message_count: usize,
}

type ClipboardSink = Arc<dyn Fn(&str) -> io::Result<()> + Send + Sync>;

#[derive(Default)]
struct ModelCatalogCache {
    providers: BTreeMap<String, ModelProviderCatalogState>,
    tasks: BTreeMap<String, JoinHandle<ModelCatalogFetchResult>>,
    metadata_refresh: Option<ModelMetadataRefreshTask>,
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

struct ModelMetadataRefreshTask {
    user_initiated: bool,
    task: JoinHandle<std::result::Result<(), String>>,
}

fn push_model_metadata_target(
    targets: &mut Vec<ModelMetadataCacheTarget>,
    seen: &mut BTreeMap<String, ()>,
    model: &ConfiguredModel,
    catalog: &ModelCatalogCache,
) {
    push_raw_model_metadata_target(targets, seen, &model.provider, &model.model, catalog);
}

fn push_raw_model_metadata_target(
    targets: &mut Vec<ModelMetadataCacheTarget>,
    seen: &mut BTreeMap<String, ()>,
    provider: &str,
    model: &str,
    catalog: &ModelCatalogCache,
) {
    let provider = provider.trim();
    let model = model.trim();
    if provider.is_empty() || model.is_empty() {
        return;
    }
    let key = format!("{provider}/{model}");
    if seen.insert(key, ()).is_some() {
        return;
    }
    targets.push(ModelMetadataCacheTarget {
        provider: provider.to_string(),
        model: model.to_string(),
        base_url: catalog
            .providers
            .get(provider)
            .map(|state| state.provider.base_url.clone()),
    });
}

impl ModelCatalogCache {
    fn any_fetching(&self) -> bool {
        self.providers
            .values()
            .any(|state| matches!(state.status, ModelCatalogStatus::Fetching))
    }

    fn metadata_refreshing(&self) -> bool {
        self.metadata_refresh.is_some()
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
