#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) struct TuiSessionDisplaySummary {
    pub(crate) summary: SessionSummary,
    pub(crate) project_label: String,
    pub(crate) project_display_path: String,
    pub(crate) visible_message_count: usize,
}

pub(crate) type ClipboardSink = Arc<dyn Fn(&str) -> io::Result<()> + Send + Sync>;

#[derive(Default)]
pub(crate) struct ModelCatalogCache {
    pub(crate) providers: BTreeMap<String, ModelProviderCatalogState>,
    pub(crate) tasks: BTreeMap<String, JoinHandle<ModelCatalogFetchResult>>,
    pub(crate) metadata_refresh: Option<ModelMetadataRefreshTask>,
}

pub(crate) struct ModelProviderCatalogState {
    pub(crate) provider: ModelCatalogProvider,
    pub(crate) status: ModelCatalogStatus,
    pub(crate) fetched: Vec<ModelCatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModelCatalogStatus {
    NotFetched,
    Fetching,
    Fetched,
    Failed(String),
}

pub(crate) struct ModelCatalogFetchResult {
    pub(crate) provider: String,
    pub(crate) result: std::result::Result<Vec<ModelCatalogEntry>, String>,
}

pub(crate) struct ModelMetadataRefreshTask {
    pub(crate) user_initiated: bool,
    pub(crate) task: JoinHandle<std::result::Result<(), String>>,
}

pub(crate) fn push_model_metadata_target(
    targets: &mut Vec<ModelMetadataCacheTarget>,
    seen: &mut BTreeMap<String, ()>,
    model: &ConfiguredModel,
    catalog: &ModelCatalogCache,
) {
    push_raw_model_metadata_target(targets, seen, &model.provider, &model.model, catalog);
}

pub(crate) fn push_raw_model_metadata_target(
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
    pub(crate) fn any_fetching(&self) -> bool {
        self.providers
            .values()
            .any(|state| matches!(state.status, ModelCatalogStatus::Fetching))
    }

    pub(crate) fn metadata_refreshing(&self) -> bool {
        self.metadata_refresh.is_some()
    }

    pub(crate) fn abort_unfinished(&mut self) {
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
