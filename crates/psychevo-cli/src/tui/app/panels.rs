impl TuiApp {
    fn session_selection_panel(&self, view: SessionListView) -> Result<BottomSelectionPanel> {
        let current_session = self.current_session.as_deref();
        let rows = self
            .tui_sessions_for_workdir(view)?
            .into_iter()
            .map(|session| {
                let summary = session.summary;
                let title = summary
                    .title
                    .clone()
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| short_session(&summary.id).to_string());
                let provider_model = format!("{}/{}", summary.provider, summary.model);
                let description = Some(format!(
                    "{}  messages={}",
                    provider_model, session.visible_message_count
                ));
                let search_text = format!(
                    "{} {} {} {} {}",
                    summary.id, title, summary.provider, summary.model, summary.source
                );
                BottomSelectionRow {
                    label: title,
                    description,
                    detail: Some(format_session_time(summary.updated_at_ms)),
                    group: Some(format_session_date(summary.updated_at_ms)),
                    search_text,
                    is_current: current_session.is_some_and(|id| id == summary.id),
                    is_default: false,
                    style: BottomRowStyle::Normal,
                    footer: None,
                    value: BottomSelectionValue::Session(summary.id),
                }
            })
            .collect();
        Ok(BottomSelectionPanel::new_sessions(view, rows))
    }

    fn agent_panel(&self) -> AgentPanel {
        AgentPanel::new(self.agent_running_panel(), self.agent_available_panel())
    }

    fn agent_running_panel(&self) -> BottomSelectionPanel {
        let paused = agent_spawn_paused();
        let mut rows = vec![BottomSelectionRow {
            label: if paused {
                "Resume spawning".to_string()
            } else {
                "Pause spawning".to_string()
            },
            description: Some(if paused {
                "New Agent calls are blocked; running children continue".to_string()
            } else {
                "New Agent calls are allowed".to_string()
            }),
            detail: Some(format!(
                "depth cap {}  concurrency unbounded",
                MAX_AGENT_SPAWN_DEPTH_CAP
            )),
            group: Some("Controls".to_string()),
            search_text: "pause resume spawning depth cap concurrency".to_string(),
            is_current: paused,
            is_default: false,
            style: BottomRowStyle::Action,
            footer: Some("Enter toggle  P toggle  Esc close  Tab available".to_string()),
            value: BottomSelectionValue::AgentSpawningToggle,
        }];
        let mut live_count = 0usize;
        if let Some(parent) = self.current_session.as_deref()
            && let Ok(store) = SqliteStore::open(&self.db_path)
        {
            let value = agent_status_value(Some(&store), Some(parent), false);
            if let Some(agents) = value.get("agents").and_then(Value::as_array) {
                for agent in agents {
                    let status = agent
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if !matches!(status, "pending_init" | "running") {
                        continue;
                    }
                    let child_session_id = agent
                        .get("child_session_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    if child_session_id.is_empty() {
                        continue;
                    }
                    live_count = live_count.saturating_add(1);
                    let id = agent
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or(child_session_id.as_str())
                        .to_string();
                    let name = agent
                        .get("agent_name")
                        .and_then(Value::as_str)
                        .unwrap_or("agent");
                    let task = agent
                        .get("task")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let task_name = agent.get("task_name").and_then(Value::as_str);
                    rows.push(BottomSelectionRow {
                        label: name.to_string(),
                        description: Some(truncate_chars(task, 80)),
                        detail: Some(task_name.unwrap_or(status).to_string()),
                        group: Some("Live child agents".to_string()),
                        search_text: format!("{id} {child_session_id} {name} {task} {status}"),
                        is_current: self.current_session.as_deref()
                            == Some(child_session_id.as_str()),
                        is_default: false,
                        style: BottomRowStyle::Normal,
                        footer: Some(
                            "Enter open  S stop subtree  P pause/resume  Esc close  Tab available"
                                .to_string(),
                        ),
                        value: BottomSelectionValue::AgentRunning {
                            id,
                            child_session_id,
                        },
                    });
                }
            }
        }
        if live_count == 0 {
            rows.push(BottomSelectionRow {
                label: "No running subagents".to_string(),
                description: Some("Completed children stay reachable from Agent rows".to_string()),
                detail: None,
                group: Some("Live child agents".to_string()),
                search_text: "no running subagents completed reachable agent rows".to_string(),
                is_current: false,
                is_default: false,
                style: BottomRowStyle::Normal,
                footer: Some("P pause/resume  Esc close  Tab available".to_string()),
                value: BottomSelectionValue::AgentDiagnostic("no running subagents".to_string()),
            });
        }
        let mut panel = BottomSelectionPanel::new("Agents", "", "No running subagents", rows);
        panel.footer = if live_count == 0 {
            "P pause/resume  Esc close  Tab available".to_string()
        } else {
            "Enter open  S stop subtree  P pause/resume  Esc close  Tab available  Type search"
                .to_string()
        };
        panel
    }

    fn agent_available_panel(&self) -> BottomSelectionPanel {
        let Some(catalog) = self.current_agent_catalog() else {
            let mut panel = BottomSelectionPanel::new("Agents", "", "Agents disabled", Vec::new());
            panel.footer = "Esc close".to_string();
            return panel;
        };
        let mut rows = vec![
            BottomSelectionRow {
                label: "Default main agent".to_string(),
                description: Some("Use the normal session identity for future turns".to_string()),
                detail: self
                    .current_agent
                    .is_none()
                    .then(|| "Current main".to_string()),
                group: Some("Main session".to_string()),
                search_text: "default main agent normal session identity".to_string(),
                is_current: self.current_agent.is_none(),
                is_default: self.current_agent.is_none(),
                style: BottomRowStyle::Normal,
                footer: Some("Enter use default  Esc close  Tab running".to_string()),
                value: BottomSelectionValue::AgentMainDefault,
            },
            BottomSelectionRow {
                label: "Create agent".to_string(),
                description: Some("project .psychevo/agents".to_string()),
                detail: None,
                group: Some("Actions".to_string()),
                search_text: "create new agent project .psychevo".to_string(),
                is_current: false,
                is_default: false,
                style: BottomRowStyle::Action,
                footer: Some("Enter create  Esc close  Tab running".to_string()),
                value: BottomSelectionValue::AgentCreate,
            },
        ];
        rows.extend(
            catalog
                .agents
                .into_iter()
                .map(|agent| agent_definition_row(agent, false, self.current_agent.as_deref())),
        );
        rows.extend(
            catalog
                .shadowed_agents
                .into_iter()
                .map(|agent| agent_definition_row(agent, true, self.current_agent.as_deref())),
        );
        rows.extend(catalog.diagnostics.into_iter().map(agent_diagnostic_row));
        let mut panel = BottomSelectionPanel::new("Agents", "", "No agents available", rows);
        panel.footer = "Enter actions  Esc close  Tab running  Type search".to_string();
        panel
    }

    fn agent_action_panel(
        &self,
        name: String,
        source: AgentSource,
        path: Option<PathBuf>,
        shadowed: bool,
    ) -> BottomSelectionPanel {
        let mut rows = Vec::new();
        if !shadowed {
            rows.push(agent_action_row(
                &name,
                source,
                path.clone(),
                shadowed,
                AgentAction::UseAsMain,
            ));
        }
        for action in [AgentAction::Run, AgentAction::View] {
            rows.push(agent_action_row(
                &name,
                source,
                path.clone(),
                shadowed,
                action,
            ));
        }
        if agent_definition_editable(source, path.as_ref()) {
            rows.push(agent_action_row(
                &name,
                source,
                path.clone(),
                shadowed,
                AgentAction::Update,
            ));
            rows.push(agent_action_row(
                &name,
                source,
                path,
                shadowed,
                AgentAction::Delete,
            ));
        }
        BottomSelectionPanel::new_agent_actions(&name, rows)
    }

    fn stats_panel(&self) -> Result<BottomSelectionPanel> {
        let report = usage_stats(StatsOptions {
            db_path: self.db_path.clone(),
            workdir: self.workdir.clone(),
            all: false,
            days: None,
            limit: 8,
        })?;
        let totals = report.get("totals").unwrap_or(&Value::Null);
        let mut rows = vec![
            stats_row(
                "totals",
                "Totals",
                format!(
                    "{} sessions  {} messages",
                    json_i64(totals, "sessions"),
                    json_i64(totals, "messages")
                ),
                Some(format!(
                    "{} tokens  {}",
                    json_i64(totals, "reported_total_tokens"),
                    format_nanodollars(json_i64(totals, "estimated_cost_nanodollars"))
                )),
                None,
            ),
            stats_row(
                "breakdown",
                "Token breakdown",
                format!(
                    "{} input  {} output  {} context",
                    json_i64(totals, "billable_input_tokens"),
                    json_i64(totals, "billable_output_tokens"),
                    json_i64(totals, "context_input_tokens")
                ),
                Some(format!(
                    "{} reported total",
                    json_i64(totals, "reported_total_tokens")
                )),
                None,
            ),
            stats_row(
                "cache-reasoning",
                "Cache and reasoning",
                format!(
                    "{} reasoning  {} cache read  {} cache write",
                    json_i64(totals, "reasoning_tokens"),
                    json_i64(totals, "cache_read_tokens"),
                    json_i64(totals, "cache_write_tokens")
                ),
                None,
                None,
            ),
        ];
        let unknown_priced_messages = json_i64(totals, "unknown_priced_messages");
        if unknown_priced_messages > 0 {
            rows.push(stats_row(
                "unknown-pricing",
                "Unknown pricing",
                format!("{unknown_priced_messages} assistant messages had usage without pricing"),
                Some(format!(
                    "estimated cost excludes {}",
                    pluralize_count(unknown_priced_messages, "message")
                )),
                None,
            ));
        }
        if let Some(models) = report.get("provider_models").and_then(Value::as_array) {
            for (index, model) in models.iter().enumerate() {
                rows.push(stats_row(
                    format!("model-{index}"),
                    format!(
                        "{}/{}",
                        model.get("provider").and_then(Value::as_str).unwrap_or("-"),
                        model.get("model").and_then(Value::as_str).unwrap_or("-")
                    ),
                    format!(
                        "{} messages  {} tokens",
                        json_i64(model, "messages"),
                        json_i64(model, "reported_total_tokens")
                    ),
                    Some(format_nanodollars(json_i64(
                        model,
                        "estimated_cost_nanodollars",
                    ))),
                    Some("Provider / model".to_string()),
                ));
            }
        }
        if let Some(tools) = report.get("top_tools").and_then(Value::as_array)
            && !tools.is_empty()
        {
            for (index, tool) in tools.iter().enumerate() {
                rows.push(stats_row(
                    format!("tool-{index}"),
                    tool.get("tool").and_then(Value::as_str).unwrap_or("-"),
                    pluralize_count(json_i64(tool, "calls"), "call"),
                    None,
                    Some("Top tools".to_string()),
                ));
            }
        }
        if let Some(sessions) = report.get("top_sessions").and_then(Value::as_array)
            && !sessions.is_empty()
        {
            for (index, session) in sessions.iter().enumerate() {
                let session_id = session
                    .get("session")
                    .and_then(Value::as_str)
                    .unwrap_or("-");
                let title = session
                    .get("title")
                    .and_then(Value::as_str)
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| short_session(session_id));
                rows.push(stats_row(
                    format!("session-{index}"),
                    title,
                    format!(
                        "{}/{}  {} tokens",
                        session
                            .get("provider")
                            .and_then(Value::as_str)
                            .unwrap_or("-"),
                        session.get("model").and_then(Value::as_str).unwrap_or("-"),
                        json_i64(session, "reported_total_tokens")
                    ),
                    Some(format!(
                        "{}  {}",
                        format_nanodollars(json_i64(session, "estimated_cost_nanodollars")),
                        format_session_time(json_i64(session, "updated_at_ms"))
                    )),
                    Some("Top sessions".to_string()),
                ));
            }
        }
        Ok(BottomSelectionPanel::new("Usage", "", "No usage yet", rows))
    }

    fn model_selection_panel(&mut self) -> Result<BottomSelectionPanel> {
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
            description: Some("configure global OpenAI-compatible provider".to_string()),
            detail: None,
            group: None,
            search_text: "add provider custom openai compatible base url api key".to_string(),
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

    fn variant_panel(
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

    fn sync_model_catalog_providers(&mut self) -> Result<()> {
        let providers = model_catalog_providers(&self.run_options(String::new()))?;
        let active = providers
            .iter()
            .map(|provider| provider.provider.clone())
            .collect::<Vec<_>>();
        for provider in providers {
            self.model_catalog
                .providers
                .entry(provider.provider.clone())
                .and_modify(|state| state.provider = provider.clone())
                .or_insert_with(|| ModelProviderCatalogState {
                    provider,
                    status: ModelCatalogStatus::NotFetched,
                    fetched: Vec::new(),
                });
        }
        self.model_catalog
            .providers
            .retain(|provider, _| active.contains(provider));
        Ok(())
    }

    fn model_catalog_provider_order(&self) -> Vec<String> {
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

    fn all_providers_status(&self) -> String {
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

    fn provider_status_text(&self, state: &ModelProviderCatalogState) -> String {
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

    fn model_row(
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

    fn model_lines(&self) -> Result<Vec<String>> {
        let mut lines = vec![format!("model: {}", self.model_display_value())];
        if !self.state.recent_models.is_empty() {
            lines.push(format!("recent: {}", self.state.recent_models.join(", ")));
        }
        lines.push("configured models:".to_string());
        lines.extend(self.configured_model_lines()?);
        Ok(lines)
    }

    fn configured_model_lines(&self) -> Result<Vec<String>> {
        let models = configured_models(&self.run_options(String::new()))?;
        if models.is_empty() {
            return Ok(vec!["no configured models".to_string()]);
        }
        Ok(models.iter().map(format_configured_model).collect())
    }

    fn variant_line(&self) -> String {
        format!("variant: {}", self.variant_display_value())
    }

    fn model_display_value(&self) -> String {
        self.current_model
            .clone()
            .or_else(|| {
                self.selected_model
                    .as_ref()
                    .map(|model| format!("{}/{}", model.provider, model.model))
            })
            .unwrap_or_else(|| "config".to_string())
    }

    fn variant_display_value(&self) -> String {
        self.current_variant
            .clone()
            .or_else(|| {
                self.selected_model
                    .as_ref()
                    .and_then(|model| model.reasoning_effort.clone())
            })
            .unwrap_or_else(|| "default".to_string())
    }
}

fn model_capability_tags(model: &ConfiguredModel) -> Vec<String> {
    let caps = &model.metadata.capabilities;
    let mut tags = Vec::new();
    match caps.reasoning {
        Some(true) => tags.push("reasoning".to_string()),
        Some(false) => tags.push("no reasoning".to_string()),
        None => {}
    }
    match caps.tool_call {
        Some(true) => tags.push("tools".to_string()),
        Some(false) => tags.push("no tools".to_string()),
        None => {}
    }
    match caps.developer_role {
        Some(true) => tags.push("developer".to_string()),
        Some(false) => tags.push("no developer".to_string()),
        None => {}
    }
    if caps.attachment == Some(true) || caps.input_modalities.iter().any(|value| value != "text") {
        tags.push("multi-modal".to_string());
    }
    if caps.structured_output == Some(true) {
        tags.push("structured".to_string());
    }
    tags
}

fn model_pricing_label(model: &ConfiguredModel) -> Option<String> {
    let cost = model.metadata.cost.as_ref()?;
    let input = cost.input?;
    let output = cost.output?;
    if input == 0.0 && output == 0.0 {
        return Some("free".to_string());
    }
    Some(format!("${input:.3}/${output:.3} /1M"))
}

fn stats_row(
    key: impl Into<String>,
    label: impl Into<String>,
    description: impl Into<String>,
    detail: Option<String>,
    group: Option<String>,
) -> BottomSelectionRow {
    let label = label.into();
    let description = description.into();
    BottomSelectionRow {
        label: label.clone(),
        description: Some(description.clone()),
        detail,
        group,
        search_text: format!("{label} {description}"),
        is_current: false,
        is_default: false,
        style: BottomRowStyle::Normal,
        footer: None,
        value: BottomSelectionValue::StatsRow(key.into()),
    }
}

fn agent_definition_row(
    agent: psychevo_runtime::AgentDefinition,
    shadowed: bool,
    current_agent: Option<&str>,
) -> BottomSelectionRow {
    let source = agent.source;
    let path = agent.file_path.clone();
    let state = if shadowed { "Shadowed" } else { "Active" };
    let editable = agent_definition_editable(source, path.as_ref());
    let source_label = source.as_str().replace('_', "-");
    let current_main = current_agent.is_some_and(|current| {
        current == agent.name.as_str()
            || agent
                .file_path
                .as_ref()
                .is_some_and(|path| current == path.display().to_string())
    });
    let definition_detail = if editable {
        format!(
            "{state} {source_label} editable  depth {}",
            agent.max_spawn_depth
        )
    } else {
        format!(
            "{state} {source_label} read-only  depth {}",
            agent.max_spawn_depth
        )
    };
    let detail = if current_main && !shadowed {
        format!("Current main  {definition_detail}")
    } else {
        definition_detail
    };
    BottomSelectionRow {
        label: agent.name.clone(),
        description: Some(agent.description.clone()),
        detail: Some(detail),
        group: Some(if shadowed {
            "Shadowed duplicates".to_string()
        } else {
            "Available definitions".to_string()
        }),
        search_text: format!(
            "{} {} {} {} {} {}",
            agent.name,
            agent.description,
            source.as_str(),
            path.as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            state,
            agent.max_spawn_depth
        ),
        is_current: current_main && !shadowed,
        is_default: false,
        style: BottomRowStyle::Normal,
        footer: Some("Enter actions  R run  V view  Esc close".to_string()),
        value: BottomSelectionValue::AgentAvailable {
            name: agent.name,
            source,
            path,
            shadowed,
        },
    }
}

fn agent_diagnostic_row(diagnostic: psychevo_runtime::AgentDiagnostic) -> BottomSelectionRow {
    let path = diagnostic
        .path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    BottomSelectionRow {
        label: "Definition error".to_string(),
        description: Some(diagnostic.message.clone()),
        detail: (!path.is_empty()).then_some(path.clone()),
        group: Some("Diagnostics".to_string()),
        search_text: format!("{} {} {}", diagnostic.kind, diagnostic.message, path),
        is_current: false,
        is_default: false,
        style: BottomRowStyle::Normal,
        footer: Some("Read-only diagnostic  Esc close  Tab running".to_string()),
        value: BottomSelectionValue::AgentDiagnostic(diagnostic.message),
    }
}

fn agent_action_row(
    name: &str,
    source: AgentSource,
    path: Option<PathBuf>,
    shadowed: bool,
    action: AgentAction,
) -> BottomSelectionRow {
    let description = match action {
        AgentAction::UseAsMain => "Use this definition for future turns in the current session",
        AgentAction::Run => "Start a background fresh-context child run",
        AgentAction::View => "Show definition details",
        AgentAction::Update => "Edit the .psychevo Markdown definition",
        AgentAction::Delete => "Delete the .psychevo Markdown definition",
    };
    BottomSelectionRow {
        label: action.label().to_string(),
        description: Some(description.to_string()),
        detail: None,
        group: None,
        search_text: format!("{name} {} {description}", action.label()),
        is_current: false,
        is_default: false,
        style: if matches!(action, AgentAction::UseAsMain | AgentAction::Run) {
            BottomRowStyle::Action
        } else {
            BottomRowStyle::Normal
        },
        footer: Some("Enter select  Esc back".to_string()),
        value: BottomSelectionValue::AgentAction {
            name: name.to_string(),
            source,
            path,
            shadowed,
            action,
        },
    }
}

fn agent_definition_editable(source: AgentSource, path: Option<&PathBuf>) -> bool {
    matches!(source, AgentSource::Project | AgentSource::Global) && path.is_some()
}

fn json_i64(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn pluralize_count(count: i64, singular: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {singular}s")
    }
}
