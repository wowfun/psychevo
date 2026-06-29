#[allow(unused_imports)]
pub(crate) use super::*;

impl TuiApp {
    pub(crate) fn start_missing_model_metadata_cache_warmup(&mut self) {
        if self.home.join("models_dev_cache.json").is_file() {
            return;
        }
        self.start_model_metadata_refresh_task(false);
    }

    pub(crate) fn start_model_metadata_refresh(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        user_initiated: bool,
    ) {
        if self.model_catalog.metadata_refreshing() {
            if user_initiated {
                ui.set_bottom_panel_notice("refreshing metadata");
            }
            return;
        }
        self.start_model_metadata_refresh_task(user_initiated);
        if user_initiated {
            ui.set_bottom_panel_notice("refreshing metadata");
        }
    }

    pub(crate) fn start_model_metadata_refresh_task(&mut self, user_initiated: bool) {
        if self.model_catalog.metadata_refreshing() {
            return;
        }
        let targets = self.model_metadata_cache_targets();
        if targets.is_empty() {
            return;
        }
        let home = self.home.clone();
        let env_map = self.env_map.clone();
        let task = tokio::spawn(async move {
            refresh_model_metadata_cache(home, env_map, targets)
                .await
                .map_err(|err| short_fetch_error(&err.to_string()))
        });
        self.model_catalog.metadata_refresh = Some(ModelMetadataRefreshTask {
            user_initiated,
            task,
        });
    }

    pub(crate) fn model_metadata_cache_targets(&mut self) -> Vec<ModelMetadataCacheTarget> {
        let _ = self.sync_model_catalog_providers();
        let mut targets = Vec::new();
        let mut seen = BTreeMap::new();
        if let Some(model) = selected_configured_model(&self.run_options(String::new()))
            .ok()
            .flatten()
        {
            push_model_metadata_target(&mut targets, &mut seen, &model, &self.model_catalog);
        }
        if let Some((provider, model)) = self
            .current_model
            .as_deref()
            .and_then(|value| value.split_once('/'))
        {
            push_raw_model_metadata_target(
                &mut targets,
                &mut seen,
                provider,
                model,
                &self.model_catalog,
            );
        }
        if let Ok(models) = configured_models(&self.run_options(String::new())) {
            let mut by_spec = BTreeMap::new();
            for model in &models {
                by_spec.insert(format_model_spec(model), model);
            }
            for recent in self.model_state.recent_model_values() {
                if let Some(model) = by_spec.get(&recent) {
                    push_model_metadata_target(&mut targets, &mut seen, model, &self.model_catalog);
                } else if let Some((provider, model)) = recent.split_once('/') {
                    push_raw_model_metadata_target(
                        &mut targets,
                        &mut seen,
                        provider,
                        model,
                        &self.model_catalog,
                    );
                }
            }
            for model in &models {
                push_model_metadata_target(&mut targets, &mut seen, model, &self.model_catalog);
            }
        }
        targets
    }

    pub(crate) async fn drain_model_metadata_refresh(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<bool> {
        let Some(refresh) = self.model_catalog.metadata_refresh.as_ref() else {
            return Ok(false);
        };
        if !refresh.task.is_finished() {
            return Ok(false);
        }
        let refresh = self
            .model_catalog
            .metadata_refresh
            .take()
            .expect("checked refresh");
        let user_initiated = refresh.user_initiated;
        let result = match refresh.task.await {
            Ok(result) => result,
            Err(err) if err.is_cancelled() => return Ok(true),
            Err(err) => Err(short_fetch_error(&err.to_string())),
        };

        match result {
            Ok(()) => {
                self.refresh_selected_model();
                if matches!(ui.bottom_panel, Some(BottomPanel::Models(_))) {
                    let selected_key = ui
                        .bottom_panel
                        .as_ref()
                        .map(|panel| panel.selection().selected_key());
                    self.rebuild_model_panel(ui, selected_key)?;
                    if user_initiated {
                        ui.set_bottom_panel_notice("metadata refreshed");
                    }
                }
            }
            Err(error) => {
                if user_initiated {
                    ui.set_bottom_panel_notice(format!("metadata refresh failed: {error}"));
                } else if self.debug {
                    ui.push_status(format!("warning: metadata warmup failed: {error}"));
                }
            }
        }
        Ok(true)
    }

    pub(crate) fn start_model_catalog_fetch_all(
        &mut self,
        ui: &mut FullscreenUi<'_>,
    ) -> Result<()> {
        if self.model_catalog.any_fetching() {
            ui.set_bottom_panel_notice("already fetching");
            return Ok(());
        }
        let providers = self
            .model_catalog_provider_order()
            .into_iter()
            .filter(|provider| {
                self.model_catalog
                    .providers
                    .get(provider)
                    .is_some_and(|state| state.provider.fetchable())
            })
            .collect::<Vec<_>>();
        if providers.is_empty() {
            ui.set_bottom_panel_notice(if self.model_catalog.providers.is_empty() {
                "no configured providers"
            } else {
                "no fetchable providers"
            });
            return Ok(());
        }
        for provider in providers {
            self.start_model_catalog_fetch_task(&provider);
        }
        self.rebuild_model_panel(ui, Some("fetch:all".to_string()))?;
        Ok(())
    }

    pub(crate) fn start_model_catalog_fetch_provider(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        provider: &str,
    ) -> Result<()> {
        let Some(state) = self.model_catalog.providers.get(provider) else {
            ui.set_bottom_panel_notice("provider unavailable");
            return Ok(());
        };
        if matches!(state.status, ModelCatalogStatus::Fetching) {
            ui.set_bottom_panel_notice("already fetching");
            return Ok(());
        }
        if !state.provider.fetchable() {
            ui.set_bottom_panel_notice(self.provider_status_text(state));
            return Ok(());
        }
        let key = format!("fetch:provider:{provider}");
        self.start_model_catalog_fetch_task(provider);
        self.rebuild_model_panel(ui, Some(key))?;
        Ok(())
    }

    pub(crate) fn start_model_catalog_fetch_task(&mut self, provider: &str) {
        if self.model_catalog.tasks.contains_key(provider) {
            return;
        }
        let Some(state) = self.model_catalog.providers.get_mut(provider) else {
            return;
        };
        if !state.provider.fetchable() {
            return;
        }
        state.status = ModelCatalogStatus::Fetching;
        let provider_config = state.provider.clone();
        let provider_id = provider_config.provider.clone();
        let home = self.home.clone();
        let task = tokio::spawn(async move {
            let result = fetch_and_cache_model_catalog(&home, &provider_config)
                .await
                .map_err(|err| short_fetch_error(&err.to_string()));
            ModelCatalogFetchResult {
                provider: provider_id,
                result,
            }
        });
        self.model_catalog.tasks.insert(provider.to_string(), task);
    }
}
