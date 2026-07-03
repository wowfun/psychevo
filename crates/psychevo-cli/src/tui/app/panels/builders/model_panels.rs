impl TuiApp {
    pub(crate) fn model_selection_panel(&mut self) -> Result<BottomSelectionPanel> {
        self.sync_model_catalog_providers()?;
        let current = self.model_display_value();
        let local_models = configured_models(&self.run_options(String::new()))?;
        let mut local_by_provider: BTreeMap<String, Vec<ConfiguredModel>> = BTreeMap::new();
        let mut known_specs = BTreeMap::new();
        for model in local_models {
            known_specs.insert(format_model_spec(&model), ModelRowSource::Local);
            local_by_provider
                .entry(model.provider.clone())
                .or_default()
                .push(model);
        }

        let mut rows = Vec::new();
        rows.push(BottomSelectionRow {
            label: "Add provider".to_string(),
            description: Some("add built-in or custom OpenAI-compatible provider".to_string()),
            detail: None,
            group: None,
            search_text: "add provider built in custom openai compatible base url api key"
                .to_string(),
            is_current: false,
            is_default: false,
            style: BottomRowStyle::Action,
            footer: Some("Enter add  Esc close  Type search".to_string()),
            value: BottomSelectionValue::AddProvider,
        });
        let all_fetchable = self.model_catalog.providers.values().any(|state| {
            state.provider.fetchable() && !matches!(state.status, ModelCatalogStatus::Fetching)
        });
        rows.push(BottomSelectionRow {
            label: "All providers".to_string(),
            description: Some(self.all_providers_status()),
            detail: None,
            group: None,
            search_text: "all providers fetch models".to_string(),
            is_current: false,
            is_default: false,
            style: BottomRowStyle::Action,
            footer: Some("Enter fetch  Esc close  Type search".to_string()),
            value: if all_fetchable {
                BottomSelectionValue::FetchAllModels
            } else {
                BottomSelectionValue::ProviderInfo("all".to_string())
            },
        });

        let mut first_model_key = None;
        let mut first_local_key = None;
        let mut current_key = None;
        for provider_id in self.model_catalog_provider_order() {
            let Some(state) = self.model_catalog.providers.get(&provider_id) else {
                continue;
            };
            rows.push(BottomSelectionRow {
                label: state.provider.display_label.clone(),
                description: Some(self.provider_status_text(state)),
                detail: None,
                group: None,
                search_text: format!(
                    "{} {}",
                    state.provider.provider, state.provider.display_label
                ),
                is_current: false,
                is_default: false,
                style: BottomRowStyle::Action,
                footer: Some("Enter fetch  Esc close  Type search".to_string()),
                value: if state.provider.fetchable() {
                    BottomSelectionValue::FetchProvider(state.provider.provider.clone())
                } else {
                    BottomSelectionValue::ProviderInfo(state.provider.provider.clone())
                },
            });

            if let Some(models) = local_by_provider.get_mut(&provider_id) {
                models.sort_by(|left, right| left.model.cmp(&right.model));
                for model in models.iter().cloned() {
                    let key = format!("model:{}", format_model_spec(&model));
                    first_model_key.get_or_insert_with(|| key.clone());
                    first_local_key.get_or_insert_with(|| key.clone());
                    if format_model_spec(&model) == current {
                        current_key = Some(key.clone());
                    }
                    rows.push(self.model_row(model, ModelRowSource::Local, &current));
                }
            }

            for entry in &state.fetched {
                let spec = format!("{}/{}", state.provider.provider, entry.id);
                if known_specs.contains_key(&spec) {
                    continue;
                }
                let model = ConfiguredModel {
                    provider: state.provider.provider.clone(),
                    provider_label: state.provider.display_label.clone(),
                    model: entry.id.clone(),
                    model_name: None,
                    reasoning_effort: None,
                    context_limit: entry.context_limit,
                    metadata: entry.metadata.clone(),
                };
                let key = format!("model:{spec}");
                first_model_key.get_or_insert_with(|| key.clone());
                if spec == current {
                    current_key = Some(key.clone());
                }
                rows.push(self.model_row(model, ModelRowSource::Fetched, &current));
                known_specs.insert(spec, ModelRowSource::Fetched);
            }
        }

        if current != "config"
            && !known_specs.contains_key(&current)
            && let Some((provider, model)) = current.split_once('/')
        {
            let provider_label = self
                .model_catalog
                .providers
                .get(provider)
                .map(|state| state.provider.display_label.clone())
                .unwrap_or_else(|| provider.to_string());
            let model = ConfiguredModel {
                provider: provider.to_string(),
                provider_label,
                model: model.to_string(),
                model_name: None,
                reasoning_effort: None,
                context_limit: None,
                metadata: Default::default(),
            };
            let key = format!("model:{current}");
            current_key = Some(key.clone());
            first_model_key.get_or_insert(key);
            rows.push(self.model_row(model, ModelRowSource::CurrentOnly, &current));
        }

        let mut panel = BottomSelectionPanel::new("Select Model", "", "No models", rows);
        let initial_key = current_key
            .or(first_local_key)
            .or(first_model_key)
            .unwrap_or_else(|| "fetch:all".to_string());
        panel.select_value_key(&initial_key);
        Ok(panel)
    }

    pub(crate) fn provider_preset_panel(&self) -> BottomSelectionPanel {
        let rows = provider_setup_presets()
            .iter()
            .map(|preset| {
                let provider_id = preset.provider_id.unwrap_or("custom");
                BottomSelectionRow {
                    label: preset.label.to_string(),
                    description: Some(provider_id.to_string()),
                    detail: preset
                        .base_urls
                        .first()
                        .map(|base_url| base_url.url.to_string()),
                    group: None,
                    search_text: format!(
                        "{} {} {} add provider preset",
                        preset.label, provider_id, preset.default_model
                    ),
                    is_current: false,
                    is_default: false,
                    style: BottomRowStyle::Action,
                    footer: Some("Enter choose  Esc back  Type search".to_string()),
                    value: BottomSelectionValue::ProviderPreset(preset.id),
                }
            })
            .collect::<Vec<_>>();
        BottomSelectionPanel::new("Add Provider", "", "No provider presets", rows)
    }

    pub(crate) fn provider_base_url_panel(
        &self,
        preset: ProviderSetupPresetId,
    ) -> BottomSelectionPanel {
        let definition = provider_setup_preset(preset);
        let mut rows = definition
            .base_urls
            .iter()
            .enumerate()
            .map(|(index, base_url)| BottomSelectionRow {
                label: base_url.label.to_string(),
                description: Some(base_url.url.to_string()),
                detail: None,
                group: None,
                search_text: format!("{} {} {}", definition.label, base_url.label, base_url.url),
                is_current: false,
                is_default: index == 0,
                style: BottomRowStyle::Action,
                footer: Some("Enter choose  Esc back  Type search".to_string()),
                value: BottomSelectionValue::ProviderBaseUrl {
                    preset,
                    index: Some(index),
                },
            })
            .collect::<Vec<_>>();
        rows.push(BottomSelectionRow {
            label: "Custom base URL".to_string(),
            description: definition
                .base_urls
                .first()
                .map(|base_url| base_url.url.to_string()),
            detail: None,
            group: None,
            search_text: format!("{} custom base url", definition.label),
            is_current: false,
            is_default: false,
            style: BottomRowStyle::Action,
            footer: Some("Enter edit  Esc back  Type search".to_string()),
            value: BottomSelectionValue::ProviderBaseUrl {
                preset,
                index: None,
            },
        });
        BottomSelectionPanel::new("Base URL", "", "No base URLs", rows)
    }

    pub(crate) fn variant_panel(
        &self,
        model: ConfiguredModel,
        source: ModelRowSource,
        models: ModelPanel,
    ) -> BottomPanel {
        let model_spec = format_model_spec(&model);
        let current_model = self.model_display_value();
        let is_current_model = current_model == model_spec;
        let configured = model
            .reasoning_effort
            .as_deref()
            .map(|variant| format!("configured default: {variant}"))
            .unwrap_or_else(|| match source {
                ModelRowSource::Local => "use provider configuration".to_string(),
                ModelRowSource::Fetched | ModelRowSource::CurrentOnly => {
                    "use provider default".to_string()
                }
            });
        let mut rows = vec![BottomSelectionRow {
            label: "Config default".to_string(),
            description: Some(configured),
            detail: None,
            group: None,
            search_text: "config default provider configuration".to_string(),
            is_current: is_current_model && self.current_variant.is_none(),
            is_default: true,
            style: BottomRowStyle::Normal,
            footer: None,
            value: BottomSelectionValue::Variant {
                model: model_spec.clone(),
                variant: None,
                reasoning_effort: model.reasoning_effort.clone(),
            },
        }];
        rows.extend(VARIANTS.iter().map(|variant| BottomSelectionRow {
            label: (*variant).to_string(),
            description: Some(variant_description(variant).to_string()),
            detail: None,
            group: None,
            search_text: format!("{variant} {}", variant_description(variant)),
            is_current: is_current_model && self.current_variant.as_deref() == Some(*variant),
            is_default: false,
            style: BottomRowStyle::Normal,
            footer: None,
            value: BottomSelectionValue::Variant {
                model: model_spec.clone(),
                variant: Some((*variant).to_string()),
                reasoning_effort: Some((*variant).to_string()),
            },
        }));
        let mut panel = BottomSelectionPanel::new(
            &format!("Select Variant for {model_spec}"),
            "Use config default or persist an explicit variant override.",
            "No variants",
            rows,
        );
        panel.footer = "Enter apply  Esc back  Type search".to_string();
        if is_current_model
            && let Some(current_variant) = self.current_variant.as_deref()
            && let Some(index) = panel
                .rows
                .iter()
                .position(|row| row.label == current_variant)
        {
            panel.set_selected(index);
        }
        BottomPanel::Variants {
            models: Box::new(models),
            panel,
        }
    }

    pub(crate) fn sync_model_catalog_providers(&mut self) -> Result<()> {
        let providers = model_catalog_providers(&self.run_options(String::new()))?;
        let active = providers
            .iter()
            .map(|provider| provider.provider.clone())
            .collect::<Vec<_>>();
        for provider in providers {
            let cached = read_cached_model_catalog(&self.home, &provider);
            self.model_catalog
                .providers
                .entry(provider.provider.clone())
                .and_modify(|state| {
                    state.provider = provider.clone();
                    if !matches!(state.status, ModelCatalogStatus::Fetching)
                        && state.fetched.is_empty()
                        && let Some(models) = cached.clone()
                    {
                        state.fetched = models;
                        state.status = ModelCatalogStatus::Fetched;
                    }
                })
                .or_insert_with(|| {
                    let (status, fetched) = match cached {
                        Some(models) => (ModelCatalogStatus::Fetched, models),
                        None => (ModelCatalogStatus::NotFetched, Vec::new()),
                    };
                    ModelProviderCatalogState {
                        provider,
                        status,
                        fetched,
                    }
                });
        }
        self.model_catalog
            .providers
            .retain(|provider, _| active.contains(provider));
        Ok(())
    }

    pub(crate) fn model_catalog_provider_order(&self) -> Vec<String> {
        let mut providers = self
            .model_catalog
            .providers
            .values()
            .map(|state| {
                (
                    state.provider.display_label.clone(),
                    state.provider.provider.clone(),
                )
            })
            .collect::<Vec<_>>();
        providers.sort();
        providers
            .into_iter()
            .map(|(_, provider)| provider)
            .collect()
    }

    pub(crate) fn all_providers_status(&self) -> String {
        if self.model_catalog.providers.is_empty() {
            return "no providers".to_string();
        }
        if self.model_catalog.any_fetching() {
            return "fetching".to_string();
        }
        let mut fetchable = 0usize;
        let mut failed = 0usize;
        let mut fetched = 0usize;
        let mut models = 0usize;
        let mut missing = 0usize;
        for state in self.model_catalog.providers.values() {
            if !state.provider.fetchable() {
                missing += 1;
                continue;
            }
            fetchable += 1;
            match &state.status {
                ModelCatalogStatus::Failed(_) => failed += 1,
                ModelCatalogStatus::Fetched => {
                    fetched += 1;
                    models += state.fetched.len();
                }
                ModelCatalogStatus::Fetching | ModelCatalogStatus::NotFetched => {}
            }
        }
        if fetchable == 0 && missing > 0 {
            return "missing credentials".to_string();
        }
        if failed > 0 && fetched > 0 {
            return "partial failed".to_string();
        }
        if failed > 0 {
            return "failed".to_string();
        }
        if fetched > 0 {
            if models == 0 {
                "no models".to_string()
            } else {
                format!("fetched {models} models")
            }
        } else {
            "not fetched".to_string()
        }
    }

    pub(crate) fn provider_status_text(&self, state: &ModelProviderCatalogState) -> String {
        if let Some(missing) = &state.provider.missing_credentials {
            return format!("missing {missing}");
        }
        if let Some(reason) = &state.provider.unavailable_reason {
            return format!("failed: {}", short_fetch_error(reason));
        }
        match &state.status {
            ModelCatalogStatus::NotFetched => "not fetched".to_string(),
            ModelCatalogStatus::Fetching => "fetching".to_string(),
            ModelCatalogStatus::Fetched if state.fetched.is_empty() => "no models".to_string(),
            ModelCatalogStatus::Fetched => format!("fetched {} models", state.fetched.len()),
            ModelCatalogStatus::Failed(error) => format!("failed: {error}"),
        }
    }

    pub(crate) fn model_row(
        &self,
        model: ConfiguredModel,
        source: ModelRowSource,
        current: &str,
    ) -> BottomSelectionRow {
        let model_spec = format_model_spec(&model);
        let mut details = Vec::new();
        if source == ModelRowSource::Fetched {
            details.push("fetched".to_string());
        }
        if source == ModelRowSource::Local
            && let Some(variant) = &model.reasoning_effort
        {
            details.push(format!("default {variant}"));
        }
        if let Some(limit) = model.context_limit {
            details.push(format!("context {}", format_count(limit)));
        }
        if let Some(limit) = model.metadata.limits.output {
            details.push(format!("output {}", format_count(limit)));
        }
        details.extend(model_capability_tags(&model));
        if let Some(price) = model_pricing_label(&model) {
            details.push(price);
        }
        let description = if details.is_empty() {
            Some(model.provider_label.clone())
        } else {
            Some(format!("{}  {}", model.provider_label, details.join("  ")))
        };
        let search_text = format!(
            "{} {} {} {} {} {} {}",
            model_spec,
            model.provider_label,
            model.reasoning_effort.clone().unwrap_or_default(),
            model.context_limit.unwrap_or_default(),
            model.metadata.limits.output.unwrap_or_default(),
            model_pricing_label(&model).unwrap_or_default(),
            if source == ModelRowSource::Fetched {
                "fetched"
            } else {
                ""
            }
        );
        BottomSelectionRow {
            label: model_spec.clone(),
            description,
            detail: None,
            group: None,
            search_text,
            is_current: model_spec == current,
            is_default: self.current_model.is_none() && model_spec == current,
            style: BottomRowStyle::Normal,
            footer: Some("Enter choose model  Esc close  Type search".to_string()),
            value: BottomSelectionValue::Model {
                model: Box::new(model),
                source,
            },
        }
    }

    pub(crate) fn model_lines(&self) -> Result<Vec<String>> {
        let mut lines = vec![format!("model: {}", self.model_display_value())];
        let recent_models = self.model_state.recent_model_values();
        if !recent_models.is_empty() {
            lines.push(format!("recent: {}", recent_models.join(", ")));
        }
        lines.push("configured models:".to_string());
        lines.extend(self.configured_model_lines()?);
        Ok(lines)
    }

    pub(crate) fn configured_model_lines(&self) -> Result<Vec<String>> {
        let models = configured_models(&self.run_options(String::new()))?;
        if models.is_empty() {
            return Ok(vec!["no configured models".to_string()]);
        }
        Ok(models.iter().map(format_configured_model).collect())
    }
}
