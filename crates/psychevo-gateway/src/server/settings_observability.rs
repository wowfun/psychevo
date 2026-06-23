fn discover_gateway_agents(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<psychevo_runtime::AgentCatalog> {
    discover_agents(&AgentDiscoveryOptions {
        home: state.inner.home.clone(),
        workdir: scope.workdir.clone(),
        env: state.inner.inherited_env.clone(),
        explicit_inputs: Vec::new(),
        no_agents: false,
    })
}

fn discover_gateway_skills(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<psychevo_runtime::SkillCatalog> {
    discover_skills(&SkillDiscoveryOptions {
        home: state.inner.home.clone(),
        workdir: scope.workdir.clone(),
        config_path: state.inner.config_path.clone(),
        env: state.inner.inherited_env.clone(),
        explicit_inputs: Vec::new(),
        no_skills: false,
    })
}

fn dynamic_slash_commands(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<Vec<DynamicSlashCommand>> {
    let mut commands = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for bundle in list_skill_bundles(&state.inner.home, &scope.workdir)? {
        if seen.insert(bundle.slug.clone()) {
            commands.push(DynamicSlashCommand {
                name: bundle.slug.clone(),
                summary: bundle.description,
                prompt: skill_prompt_marker(&bundle.slug, ""),
            });
        }
    }
    for skill in discover_gateway_skills(state, scope)?.skills {
        if skill.disable_model_invocation || !skill.supported_on_current_platform {
            continue;
        }
        if seen.insert(skill.name.clone()) {
            commands.push(DynamicSlashCommand {
                name: skill.name.clone(),
                summary: skill.description,
                prompt: skill_prompt_marker(&skill.name, ""),
            });
        }
    }
    Ok(commands)
}

fn context_read_value(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<Value> {
    Ok(serde_json::to_value(context_read_result(
        state, scope, thread_id,
    )?)?)
}

fn context_read_result(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<wire::ContextReadResult> {
    let thread_id = match thread_id {
        Some(thread_id) => Some(thread_id.to_string()),
        None => state.inner.gateway.resolve_source_thread(&scope.source)?,
    };
    let Some(thread_id) = thread_id else {
        return Ok(context_unavailable("No active session"));
    };
    let snapshot = match context_snapshot(ContextOptions {
        state: state.inner.state.clone(),
        workdir: scope.workdir.clone(),
        session: thread_id,
        config_path: state.inner.config_path.clone(),
        inherited_env: Some(state.inner.inherited_env.clone()),
    }) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            return Ok(context_unavailable(&err.to_string()));
        }
    };
    let categories = snapshot
        .categories
        .iter()
        .filter(|(id, _)| id.as_str() != "free_space")
        .map(|(id, category)| wire::ContextUsageCategoryView {
            id: id.clone(),
            label: category.label.clone(),
            tokens: category.tokens,
            estimated: category.estimated,
            status: category.status.clone(),
            percent: category.percent,
            details: Some(category.details.clone()),
        })
        .collect::<Vec<_>>();
    Ok(wire::ContextReadResult {
        available: true,
        label: format_context_total_value(&snapshot),
        status: snapshot.status,
        used_tokens: snapshot.total.tokens,
        context_limit: snapshot.context_limit,
        percent: snapshot.total.percent,
        categories,
        advice: snapshot
            .advice
            .into_iter()
            .map(|advice| advice.message)
            .collect(),
    })
}

fn observability_read_value(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<Value> {
    let resolved_thread_id = match thread_id {
        Some(thread_id) => Some(thread_id.to_string()),
        None => state.inner.gateway.resolve_source_thread(&scope.source)?,
    };
    let metadata = match resolved_thread_id.as_deref() {
        Some(session_id) => state.inner.state.store().session_metadata(session_id)?,
        None => None,
    };
    let peer_usage = metadata.as_ref().and_then(acp_peer_usage_update);
    let context = match peer_usage.and_then(acp_peer_context_read_result) {
        Some(context) => context,
        None => context_read_result(state, scope, resolved_thread_id.as_deref())?,
    };
    let usage = match resolved_thread_id {
        Some(session_id) => {
            let summary = session_usage_summary(SessionUsageOptions {
                state: state.inner.state.clone(),
                session_id,
            })?;
            let mut view = wire::SessionUsageSummaryView {
                available: true,
                session_id: Some(summary.session_id),
                provider: Some(summary.provider),
                model: Some(summary.model),
                message_count: summary.message_count,
                assistant_message_count: summary.assistant_message_count,
                context_input_tokens: summary.context_input_tokens,
                billable_input_tokens: summary.billable_input_tokens,
                billable_output_tokens: summary.billable_output_tokens,
                reasoning_tokens: summary.reasoning_tokens,
                cache_read_tokens: summary.cache_read_tokens,
                cache_write_tokens: summary.cache_write_tokens,
                reported_total_tokens: summary.reported_total_tokens,
                estimated_cost_nanodollars: summary.estimated_cost_nanodollars,
                cost_status: summary.cost_status,
                estimated_pricing_count: summary.estimated_pricing_count,
                free_pricing_count: summary.free_pricing_count,
                included_pricing_count: summary.included_pricing_count,
                unknown_pricing_count: summary.unknown_pricing_count,
                cache_read_percent: summary.cache_read_percent,
            };
            apply_acp_peer_usage_to_summary(&mut view, peer_usage);
            view
        }
        None => usage_unavailable(),
    };
    Ok(serde_json::to_value(wire::ObservabilityReadResult {
        context,
        usage,
    })?)
}

fn acp_peer_usage_update(metadata: &Value) -> Option<&Value> {
    metadata.get(ACP_PEER_METADATA_KEY)?.get("usageUpdate")
}

fn acp_peer_context_read_result(usage: &Value) -> Option<wire::ContextReadResult> {
    let used = usage_u64_field(usage, "used")?;
    let size = usage_u64_field(usage, "size")?;
    let percent = (size > 0).then(|| (used as f64 / size as f64) * 100.0);
    Some(wire::ContextReadResult {
        available: true,
        label: format_context_total_value_parts(used, false, Some(size), percent),
        status: "reported by ACP peer".to_string(),
        used_tokens: used,
        context_limit: Some(size),
        percent,
        categories: Vec::new(),
        advice: Vec::new(),
    })
}

fn apply_acp_peer_usage_to_summary(
    usage: &mut wire::SessionUsageSummaryView,
    peer_usage: Option<&Value>,
) {
    let Some(peer_usage) = peer_usage else {
        return;
    };
    if let Some(used) = usage_u64_field(peer_usage, "used") {
        if usage.reported_total_tokens == 0 {
            usage.reported_total_tokens = used;
        }
        if usage.context_input_tokens == 0 {
            usage.context_input_tokens = used;
        }
    }
    let has_persisted_pricing = usage.estimated_pricing_count
        + usage.free_pricing_count
        + usage.included_pricing_count
        > 0;
    if !has_persisted_pricing
        && let Some(cost) = acp_peer_usage_cost_nanodollars(peer_usage)
    {
        usage.estimated_cost_nanodollars = cost;
        usage.cost_status = if cost == 0 {
            "free".to_string()
        } else {
            "estimated".to_string()
        };
        usage.estimated_pricing_count = (cost > 0) as u64;
        usage.free_pricing_count = (cost == 0) as u64;
    }
}

fn usage_u64_field(value: &Value, field: &str) -> Option<u64> {
    value.get(field).and_then(|value| {
        value.as_u64().or_else(|| {
            value
                .as_f64()
                .filter(|number| *number >= 0.0)
                .map(|number| number as u64)
        })
    })
}

fn acp_peer_usage_cost_nanodollars(usage: &Value) -> Option<i64> {
    let cost = usage.get("cost")?;
    let amount = cost.get("amount").and_then(Value::as_f64)?;
    let currency = cost
        .get("currency")
        .and_then(Value::as_str)
        .unwrap_or("USD");
    if !currency.eq_ignore_ascii_case("USD") || amount < 0.0 {
        return None;
    }
    Some((amount * 1_000_000_000.0).round() as i64)
}

fn context_unavailable(label: &str) -> wire::ContextReadResult {
    wire::ContextReadResult {
        available: false,
        label: label.to_string(),
        status: "unavailable".to_string(),
        used_tokens: 0,
        context_limit: None,
        percent: None,
        categories: Vec::new(),
        advice: Vec::new(),
    }
}

fn usage_unavailable() -> wire::SessionUsageSummaryView {
    wire::SessionUsageSummaryView {
        available: false,
        session_id: None,
        provider: None,
        model: None,
        message_count: 0,
        assistant_message_count: 0,
        context_input_tokens: 0,
        billable_input_tokens: 0,
        billable_output_tokens: 0,
        reasoning_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reported_total_tokens: 0,
        estimated_cost_nanodollars: 0,
        cost_status: "unknown".to_string(),
        estimated_pricing_count: 0,
        free_pricing_count: 0,
        included_pricing_count: 0,
        unknown_pricing_count: 0,
        cache_read_percent: None,
    }
}

fn usage_read_value(
    state: &WebState,
    params: wire::UsageReadParams,
) -> psychevo_runtime::Result<Value> {
    let result = usage_read(UsageReadOptions {
        state: state.inner.state.clone(),
        activity_days: params.activity_days.unwrap_or(365) as usize,
    })?;
    Ok(serde_json::to_value(wire::UsageReadResult {
        generated_at_ms: result.generated_at_ms,
        windows: result
            .windows
            .into_iter()
            .map(|window| wire::UsageWindowSummaryView {
                id: window.id,
                label: window.label,
                since_ms: window.since_ms,
                session_count: window.session_count,
                message_count: window.message_count,
                assistant_message_count: window.assistant_message_count,
                context_input_tokens: window.context_input_tokens,
                billable_input_tokens: window.billable_input_tokens,
                billable_output_tokens: window.billable_output_tokens,
                reasoning_tokens: window.reasoning_tokens,
                cache_read_tokens: window.cache_read_tokens,
                cache_write_tokens: window.cache_write_tokens,
                reported_total_tokens: window.reported_total_tokens,
                estimated_cost_nanodollars: window.estimated_cost_nanodollars,
                cost_status: window.cost_status,
                estimated_pricing_count: window.estimated_pricing_count,
                free_pricing_count: window.free_pricing_count,
                included_pricing_count: window.included_pricing_count,
                unknown_pricing_count: window.unknown_pricing_count,
                cache_read_percent: window.cache_read_percent,
            })
            .collect(),
        activity: wire::UsageActivityView {
            start_date: result.activity.start_date,
            end_date: result.activity.end_date,
            days: result
                .activity
                .days
                .into_iter()
                .map(|day| wire::UsageActivityDayView {
                    date: day.date,
                    session_count: day.session_count,
                    message_count: day.message_count,
                    reported_total_tokens: day.reported_total_tokens,
                    context_input_tokens: day.context_input_tokens,
                    cache_read_tokens: day.cache_read_tokens,
                    cache_write_tokens: day.cache_write_tokens,
                    estimated_cost_nanodollars: day.estimated_cost_nanodollars,
                    cost_status: day.cost_status,
                    estimated_pricing_count: day.estimated_pricing_count,
                    free_pricing_count: day.free_pricing_count,
                    included_pricing_count: day.included_pricing_count,
                    unknown_pricing_count: day.unknown_pricing_count,
                })
                .collect(),
        },
    })?)
}

fn settings_read_value(
    state: &WebState,
    workdir: &Path,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<Value> {
    let controls = workbench_controls_value(state, workdir, thread_id)?;
    let project = workbench_project_value(workdir);
    let channels = channel_list_result_for_workdir(state, workdir).unwrap_or_default();
    Ok(json!({
        "workdir": workdir,
        "project": project,
        "channels": channels,
        "memoryResources": {"mode": "status_only", "available": true},
        "secrets": {"frontendPersistence": "disabled"},
        "controls": controls
    }))
}

fn workbench_controls_value(
    state: &WebState,
    workdir: &Path,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<wire::WorkbenchControlsView> {
    let options = state.run_options(workdir.to_path_buf(), None);
    let agent = session_control_agent(state, thread_id)?;
    let model_state = ModelState::load(&ModelState::path_for_home(&state.inner.home))?;
    let workdir_key = workdir.to_string_lossy().to_string();
    let session_selection = thread_id
        .map(|thread_id| session_model_state_selection(state, thread_id))
        .transpose()?
        .flatten();
    let state_model = model_state.model_for(&workdir_key);
    let state_reasoning_effort = model_state.reasoning_effort_for(&workdir_key);
    let selected = selected_configured_model(&options);
    let (config_model, config_status, config_error) = match selected {
        Ok(Some(model)) => (
            Some(format!("{}/{}", model.provider, model.model)),
            wire::WorkbenchModelStatus::Resolved,
            None,
        ),
        Ok(None) => (None, wire::WorkbenchModelStatus::Unconfigured, None),
        Err(error) if model_resolution_unconfigured_error(&error.to_string()) => {
            (None, wire::WorkbenchModelStatus::Unconfigured, None)
        }
        Err(error) => (
            None,
            wire::WorkbenchModelStatus::Error,
            Some(error.to_string()),
        ),
    };
    let model = session_selection
        .as_ref()
        .and_then(|selection| selection.model.clone())
        .or(state_model)
        .or(config_model);
    let model_status = if model.is_some() {
        wire::WorkbenchModelStatus::Resolved
    } else {
        config_status
    };
    let model_error = if model.is_some() { None } else { config_error };
    let variant = session_selection
        .as_ref()
        .and_then(|selection| selection.reasoning_effort.clone())
        .or(state_reasoning_effort)
        .or_else(|| options.reasoning_effort.clone())
        .or_else(|| Some("none".to_string()));
    let configured = configured_models(&options).unwrap_or_default();
    let model_details = model_options_with_cached_catalog(state, &configured);
    let model_options = model_details
        .iter()
        .map(|model| model.value.clone())
        .collect();
    Ok(wire::WorkbenchControlsView {
        permission_mode: PermissionMode::Default.as_str().to_string(),
        mode: RunMode::Default.as_str().to_string(),
        runtime_ref: "native".to_string(),
        agent,
        model,
        model_status,
        model_error,
        variant,
        permission_mode_options: ["default", "acceptEdits", "dontAsk", "bypassPermissions"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        mode_options: ["default", "plan"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        model_options,
        model_details,
        recent_models: model_state.recent_model_values(),
        variant_options: REASONING_EFFORT_VALUES
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
    })
}

#[derive(Debug, Clone)]
struct ComposerModelSelection {
    model: Option<String>,
    reasoning_effort: Option<String>,
}

fn session_model_state_selection(
    state: &WebState,
    thread_id: &str,
) -> psychevo_runtime::Result<Option<ComposerModelSelection>> {
    let Some(summary) = state.inner.state.store().session_summary(thread_id)? else {
        return Ok(None);
    };
    let metadata = state.inner.state.store().session_metadata(thread_id)?;
    let reasoning_effort = metadata
        .as_ref()
        .and_then(|metadata| metadata.get(SESSION_COMPOSER_MODEL_METADATA_KEY))
        .and_then(|metadata| metadata.get("reasoningEffort"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(Some(ComposerModelSelection {
        model: Some(format!("{}/{}", summary.provider, summary.model)),
        reasoning_effort: normalize_reasoning_effort(reasoning_effort),
    }))
}

fn model_resolution_unconfigured_error(message: &str) -> bool {
    message.contains("auto provider could not find usable credentials and model")
        || message.contains("Psychevo home is not initialized")
}

fn configured_model_option_view(model: &psychevo_runtime::ConfiguredModel) -> wire::ModelOptionView {
    let reasoning_supported = model.metadata.capabilities.reasoning;
    wire::ModelOptionView {
        value: format!("{}/{}", model.provider, model.model),
        provider: model.provider.clone(),
        id: model.model.clone(),
        label: None,
        provider_label: Some(model.provider_label.clone()),
        free: configured_model_is_free(model),
        context_limit: model.context_limit,
        reasoning_supported,
        reasoning_efforts: reasoning_efforts_for_model(reasoning_supported),
    }
}

fn model_options_with_cached_catalog(
    state: &WebState,
    configured: &[psychevo_runtime::ConfiguredModel],
) -> Vec<wire::ModelOptionView> {
    let mut seen = std::collections::BTreeSet::new();
    let mut options = Vec::new();
    for model in configured {
        let option = configured_model_option_view(model);
        if seen.insert(option.value.clone()) {
            options.push(option);
        }
    }
    let cache = state
        .inner
        .model_catalog_cache
        .lock()
        .expect("model catalog cache");
    for cached in cache.values() {
        for model in &cached.models {
            let option = catalog_model_option_view(&cached.provider, model.clone());
            if seen.insert(option.value.clone()) {
                options.push(option);
            }
        }
    }
    options.sort_by(|left, right| left.value.cmp(&right.value));
    options
}

fn reasoning_efforts_for_model(reasoning_supported: Option<bool>) -> Vec<String> {
    if reasoning_supported == Some(false) {
        return vec!["none".to_string()];
    }
    REASONING_EFFORT_VALUES
        .iter()
        .map(|value| (*value).to_string())
        .collect()
}

fn model_settings_value(
    state: &WebState,
    workdir: &Path,
) -> psychevo_runtime::Result<Value> {
    Ok(serde_json::to_value(model_settings_result(state, workdir)?)?)
}

fn model_settings_result(
    state: &WebState,
    workdir: &Path,
) -> psychevo_runtime::Result<wire::ModelSettingsResult> {
    let options = model_settings_global_options(state, workdir);
    let selected_model = selected_configured_model(&options)?;
    let default_reasoning_effort = selected_model
        .as_ref()
        .and_then(|model| model.reasoning_effort.clone());
    let default_model = selected_model.map(|model| format!("{}/{}", model.provider, model.model));
    let configured = configured_models(&options).unwrap_or_default();
    let effective_config = config_show_value(&options, ConfigScope::Effective)?;
    let effective = effective_config.get("value").unwrap_or(&Value::Null);
    let configured_provider_ids = effective
        .get("provider")
        .and_then(Value::as_object)
        .map(|providers| providers.keys().cloned().collect::<std::collections::BTreeSet<_>>())
        .unwrap_or_default();
    let auth = auth_status_value(&options, None)?;
    let mut providers = auth
        .get("providers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| model_provider_view(row, &configured_provider_ids))
        .collect::<Vec<_>>();
    if !providers.iter().any(|provider| provider.id == "custom") {
        providers.push(wire::ModelProviderView {
            id: "custom".to_string(),
            label: "Custom".to_string(),
            built_in: false,
            configured: false,
            base_url: None,
            api_key_env: None,
            credential_status: wire::ModelCredentialStatus::Missing,
            no_auth: false,
            can_fetch_models: false,
            unavailable_reason: Some("requires provider setup".to_string()),
        });
    }
    providers.sort_by(|left, right| {
        provider_sort_key(&left.id)
            .cmp(&provider_sort_key(&right.id))
            .then_with(|| left.label.cmp(&right.label))
    });
    let model_options = model_options_with_cached_catalog(state, &configured);
    Ok(wire::ModelSettingsResult {
        scope: wire::ModelSettingsScope::Global,
        workdir: workdir.display().to_string(),
        default_model,
        default_reasoning_effort,
        providers,
        auxiliary: vec![
            auxiliary_model_assignment_view(effective, "title_generation", "Title generation"),
            auxiliary_model_assignment_view(effective, "compression", "Context compression"),
        ],
        model_options,
    })
}

fn model_settings_global_options(state: &WebState, workdir: &Path) -> RunOptions {
    let mut options = state.run_options(workdir.to_path_buf(), None);
    options.config_path = Some(state.inner.home.join("config.toml"));
    options
}

fn model_provider_view(
    row: &Value,
    configured_provider_ids: &std::collections::BTreeSet<String>,
) -> Option<wire::ModelProviderView> {
    let id = row.get("provider").and_then(Value::as_str)?.to_string();
    let base_url = row
        .get("base_url")
        .and_then(Value::as_str)
        .map(str::to_string);
    let status = match row.get("status").and_then(Value::as_str).unwrap_or("missing") {
        "present" => wire::ModelCredentialStatus::Present,
        "not_required" => wire::ModelCredentialStatus::NotRequired,
        _ => wire::ModelCredentialStatus::Missing,
    };
    let can_fetch_models = base_url.is_some() && status != wire::ModelCredentialStatus::Missing;
    let unavailable_reason = (!can_fetch_models).then(|| {
        row.get("api_key_env")
            .and_then(Value::as_str)
            .map(|key| format!("missing {key}"))
            .unwrap_or_else(|| "requires provider setup".to_string())
    });
    Some(wire::ModelProviderView {
        id: id.clone(),
        label: row
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or(id.as_str())
            .to_string(),
        built_in: is_known_builtin_provider(&id),
        configured: configured_provider_ids.contains(&id),
        base_url,
        api_key_env: row
            .get("api_key_env")
            .and_then(Value::as_str)
            .map(str::to_string),
        credential_status: status,
        no_auth: row
            .get("no_auth")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        can_fetch_models,
        unavailable_reason,
    })
}

fn auxiliary_model_assignment_view(
    effective: &Value,
    task: &str,
    label: &str,
) -> wire::AuxiliaryModelAssignmentView {
    let task_value = effective
        .get("auxiliary")
        .and_then(|auxiliary| auxiliary.get(task));
    let provider = task_value
        .and_then(|value| value.get("provider"))
        .and_then(Value::as_str)
        .unwrap_or("auto")
        .to_string();
    let model = task_value
        .and_then(|value| value.get("model"))
        .and_then(config_model_value_string)
        .unwrap_or_default();
    let effective_model = if !model.trim().is_empty() && provider != "auto" {
        Some(format!("{provider}/{model}"))
    } else if task == "compression" {
        effective
            .get("compression")
            .and_then(|value| value.get("model"))
            .and_then(config_model_value_string)
    } else {
        None
    };
    let reasoning_effort = task_value
        .and_then(|value| value.get("model"))
        .and_then(config_model_reasoning_effort);
    wire::AuxiliaryModelAssignmentView {
        task: task.to_string(),
        label: label.to_string(),
        provider,
        model,
        reasoning_effort,
        effective_model,
    }
}

fn config_model_value_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) if !value.trim().is_empty() => Some(value.trim().to_string()),
        Value::Object(object) => object
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        _ => None,
    }
}

fn config_model_reasoning_effort(value: &Value) -> Option<String> {
    value
        .as_object()
        .and_then(|object| object.get("reasoning_effort"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "none")
        .map(str::to_string)
}

async fn model_provider_catalog_value(
    state: &WebState,
    workdir: &Path,
    params: wire::ModelProviderCatalogParams,
) -> psychevo_runtime::Result<Value> {
    let options = model_settings_global_options(state, workdir);
    let provider_id = normalize_provider_id(&params.provider_id);
    let provider = model_catalog_provider(&options, &provider_id)?
        .ok_or_else(|| Error::Config(format!("unknown provider: {provider_id}")))?;
    let models = fetch_model_catalog(&provider).await?;
    state
        .inner
        .model_catalog_cache
        .lock()
        .expect("model catalog cache")
        .insert(
            provider.provider.clone(),
            CachedModelCatalog {
                provider: provider.clone(),
                models: models.clone(),
            },
        );
    let models = models
        .into_iter()
        .map(|model| catalog_model_option_view(&provider, model))
        .collect();
    Ok(serde_json::to_value(wire::ModelProviderCatalogResult {
        provider_id: provider.provider,
        models,
    })?)
}

fn catalog_model_option_view(
    provider: &ModelCatalogProvider,
    model: ModelCatalogEntry,
) -> wire::ModelOptionView {
    let reasoning_supported = model.metadata.capabilities.reasoning;
    wire::ModelOptionView {
        provider: provider.provider.clone(),
        value: format!("{}/{}", provider.provider, model.id),
        free: model_catalog_entry_is_free(&provider.provider, &model),
        context_limit: model.context_limit,
        id: model.id,
        label: None,
        provider_label: Some(provider.display_label.clone()),
        reasoning_supported,
        reasoning_efforts: reasoning_efforts_for_model(reasoning_supported),
    }
}

fn model_state_read_value(
    state: &WebState,
    workdir: &Path,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<Value> {
    Ok(serde_json::to_value(model_state_result(
        state, workdir, thread_id,
    )?)?)
}

fn model_state_set_value(
    state: &WebState,
    workdir: &Path,
    thread_id: Option<&str>,
    params: wire::ModelStateSetParams,
) -> psychevo_runtime::Result<Value> {
    let (model_spec, provider, model_id) = normalize_provider_qualified_model(&params.model)?;
    let reasoning_effort = normalize_model_state_reasoning_effort(params.reasoning_effort)?;
    let path = ModelState::path_for_home(&state.inner.home);
    let mut model_state = ModelState::load(&path)?;
    let workdir_key = workdir.to_string_lossy().to_string();
    model_state.set_model(&workdir_key, model_spec.clone(), reasoning_effort.clone());
    model_state.save(&path)?;
    if let Some(thread_id) = thread_id {
        let store = state.inner.state.store();
        store.set_session_model(thread_id, &provider, &model_id)?;
        let mut metadata = serde_json::Map::new();
        metadata.insert("model".to_string(), Value::String(model_spec));
        if let Some(reasoning_effort) = reasoning_effort {
            metadata.insert(
                "reasoningEffort".to_string(),
                Value::String(reasoning_effort),
            );
        }
        store.set_session_metadata_field(
            thread_id,
            SESSION_COMPOSER_MODEL_METADATA_KEY,
            Some(Value::Object(metadata)),
        )?;
    }
    Ok(serde_json::to_value(model_state_result(
        state, workdir, thread_id,
    )?)?)
}

fn model_state_result(
    state: &WebState,
    workdir: &Path,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<wire::ModelStateResult> {
    let model_state = ModelState::load(&ModelState::path_for_home(&state.inner.home))?;
    let workdir_key = workdir.to_string_lossy().to_string();
    let session_selection = thread_id
        .map(|thread_id| session_model_state_selection(state, thread_id))
        .transpose()?
        .flatten();
    Ok(wire::ModelStateResult {
        workdir: workdir.display().to_string(),
        thread_id: thread_id.map(str::to_string),
        model: session_selection
            .as_ref()
            .and_then(|selection| selection.model.clone())
            .or_else(|| model_state.model_for(&workdir_key)),
        reasoning_effort: session_selection
            .as_ref()
            .and_then(|selection| selection.reasoning_effort.clone())
            .or_else(|| model_state.reasoning_effort_for(&workdir_key)),
        recent_models: model_state.recent_model_values(),
    })
}

fn normalize_provider_qualified_model(value: &str) -> psychevo_runtime::Result<(String, String, String)> {
    let value = value.trim();
    let Some((provider, model)) = value.split_once('/') else {
        return Err(Error::Config(
            "model must use provider/model format".to_string(),
        ));
    };
    let provider = normalize_provider_id(provider);
    validate_model_provider_id(&provider)?;
    let model = model.trim();
    if model.is_empty() {
        return Err(Error::Config("model id is required".to_string()));
    }
    Ok((format!("{provider}/{model}"), provider, model.to_string()))
}

fn normalize_model_state_reasoning_effort(
    value: Option<String>,
) -> psychevo_runtime::Result<Option<String>> {
    let reasoning_effort = normalize_reasoning_effort(value);
    if let Some(reasoning_effort) = reasoning_effort.as_deref()
        && !REASONING_EFFORT_VALUES.contains(&reasoning_effort)
    {
        return Err(Error::Config(format!(
            "reasoning_effort must be one of {}",
            REASONING_EFFORT_VALUES.join(", ")
        )));
    }
    Ok(reasoning_effort)
}

fn model_provider_save_value(
    state: &WebState,
    workdir: &Path,
    params: wire::ModelProviderSaveParams,
) -> psychevo_runtime::Result<Value> {
    let provider_id = normalize_provider_id(&params.provider_id);
    validate_model_provider_id(&provider_id)?;
    let label = params.label.trim();
    if label.is_empty() {
        return Err(Error::Config("provider label is required".to_string()));
    }
    let base_url = validate_model_base_url(&params.base_url)?;
    let config_dir = state.inner.home.clone();
    set_config_value(
        config_dir.clone(),
        &format!("provider.{provider_id}.label"),
        json!(label),
    )?;
    set_config_value(
        config_dir.clone(),
        &format!("provider.{provider_id}.options.base_url"),
        json!(base_url),
    )?;
    if params.no_auth {
        if params
            .api_key
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            return Err(Error::Config(
                "no_auth provider save must not include an API key".to_string(),
            ));
        }
        remove_config_value(
            config_dir.clone(),
            &format!("provider.{provider_id}.options.api_key_env"),
        )?;
        set_config_value(
            config_dir.clone(),
            &format!("provider.{provider_id}.options.no_auth"),
            json!(true),
        )?;
    } else {
        let api_key_env = params
            .api_key_env
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| custom_provider_api_key_env(&provider_id));
        validate_model_api_key_env(&api_key_env)?;
        set_config_value(
            config_dir.clone(),
            &format!("provider.{provider_id}.options.api_key_env"),
            json!(api_key_env),
        )?;
        remove_config_value(
            config_dir.clone(),
            &format!("provider.{provider_id}.options.no_auth"),
        )?;
        if let Some(api_key) = params
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let options = state.run_options(workdir.to_path_buf(), None);
            set_provider_api_key(&options, config_dir, &provider_id, api_key)?;
        }
    }
    model_settings_value(state, workdir)
}

fn model_assignment_set_value(
    state: &WebState,
    workdir: &Path,
    params: wire::ModelAssignmentSetParams,
) -> psychevo_runtime::Result<Value> {
    let provider = normalize_provider_id(&params.provider);
    validate_model_provider_id(&provider)?;
    let reasoning_effort = assignment_reasoning_effort(params.reasoning_effort.as_deref());
    match params.target {
        wire::ModelAssignmentTarget::Default => {
            let model_spec = format!("{provider}/{}", params.model.trim());
            set_default_model_with_reasoning(
                &state.inner.home,
                workdir,
                true,
                &model_spec,
                reasoning_effort,
            )?;
            Ok(serde_json::to_value(wire::ModelAssignmentSetResult {
                ok: true,
                target: wire::ModelAssignmentTarget::Default,
                task: None,
                provider,
                model: params.model.trim().to_string(),
                reasoning_effort: reasoning_effort.map(str::to_string),
            })?)
        }
        wire::ModelAssignmentTarget::Auxiliary => {
            let task = params
                .task
                .as_deref()
                .ok_or_else(|| Error::Config("auxiliary assignment requires task".to_string()))?;
            set_auxiliary_model_with_reasoning(
                &state.inner.home,
                workdir,
                true,
                task,
                &provider,
                params.model.trim(),
                reasoning_effort,
            )?;
            Ok(serde_json::to_value(wire::ModelAssignmentSetResult {
                ok: true,
                target: wire::ModelAssignmentTarget::Auxiliary,
                task: Some(task.to_string()),
                provider,
                model: params.model.trim().to_string(),
                reasoning_effort: reasoning_effort.map(str::to_string),
            })?)
        }
    }
}

fn assignment_reasoning_effort(value: Option<&str>) -> Option<&str> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "none")
}

fn validate_model_provider_id(provider_id: &str) -> psychevo_runtime::Result<()> {
    if provider_id == "custom" {
        return Err(Error::Config(
            "custom provider save requires a unique provider id".to_string(),
        ));
    }
    let mut chars = provider_id.chars();
    if matches!(chars.next(), Some('a'..='z' | '0'..='9'))
        && chars.all(|ch| matches!(ch, 'a'..='z' | '0'..='9' | '-' | '_'))
    {
        Ok(())
    } else {
        Err(Error::Config(
            "provider id must use lowercase letters, numbers, hyphens, or underscores".to_string(),
        ))
    }
}

fn validate_model_base_url(value: &str) -> psychevo_runtime::Result<String> {
    let value = value.trim().trim_end_matches('/').to_string();
    if value.starts_with("http://") || value.starts_with("https://") {
        Ok(value)
    } else {
        Err(Error::Config(
            "provider base_url must start with http:// or https://".to_string(),
        ))
    }
}

fn validate_model_api_key_env(value: &str) -> psychevo_runtime::Result<()> {
    let mut chars = value.chars();
    if matches!(chars.next(), Some('A'..='Z' | '_'))
        && chars.all(|ch| matches!(ch, 'A'..='Z' | '0'..='9' | '_'))
    {
        Ok(())
    } else {
        Err(Error::Config(
            "provider api_key_env must be a valid environment variable name".to_string(),
        ))
    }
}

fn configured_model_is_free(model: &psychevo_runtime::ConfiguredModel) -> bool {
    let Some(cost) = &model.metadata.cost else {
        return false;
    };
    let values = [
        cost.input,
        cost.output,
        cost.cache_read,
        cost.cache_write,
        cost.request,
    ];
    values.iter().flatten().any(|value| *value == 0.0)
        && values.iter().flatten().all(|value| *value == 0.0)
}

fn is_known_builtin_provider(provider_id: &str) -> bool {
    matches!(
        provider_id,
        "openrouter"
            | "openai"
            | "opencode-zen"
            | "xai"
            | "zai"
            | "deepseek"
            | "dashscope"
            | "xiaomi"
            | "xiaomi-token-plan"
            | "lmstudio"
    )
}

fn provider_sort_key(provider_id: &str) -> (u8, &str) {
    let index = match provider_id {
        "openrouter" => 0,
        "openai" => 1,
        "opencode-zen" => 2,
        "xai" => 3,
        "zai" => 4,
        "deepseek" => 5,
        "dashscope" => 6,
        "xiaomi" => 7,
        "xiaomi-token-plan" => 8,
        "lmstudio" => 9,
        _ => 100,
    };
    (index, provider_id)
}

fn native_runtime_mode_option() -> wire::RuntimeConfigOptionView {
    wire::RuntimeConfigOptionView {
        id: "mode".to_string(),
        name: "Psychevo mode".to_string(),
        description: None,
        category: Some("mode".to_string()),
        option_type: "select".to_string(),
        current_value: Some(RunMode::Default.as_str().to_string()),
        values: [RunMode::Default, RunMode::Plan]
            .into_iter()
            .map(|mode| wire::RuntimeConfigOptionValueView {
                value: mode.as_str().to_string(),
                name: mode.as_str().to_string(),
                description: None,
                group: None,
            })
            .collect(),
    }
}

fn session_control_agent(
    state: &WebState,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<Option<String>> {
    let Some(thread_id) = thread_id else {
        return Ok(None);
    };
    let metadata = state.inner.state.store().session_metadata(thread_id)?;
    Ok(match main_agent_from_session_metadata(metadata.as_ref()) {
        LoadedMainAgent::Agent(agent) => Some(agent),
        LoadedMainAgent::Default | LoadedMainAgent::Missing => None,
    })
}

fn update_session_agent_setting(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: &str,
    input: Option<&str>,
) -> psychevo_runtime::Result<()> {
    let summary = state
        .inner
        .state
        .store()
        .session_summary(thread_id)?
        .ok_or_else(|| Error::Message(format!("session not found: {thread_id}")))?;
    if Path::new(&summary.workdir) != scope.workdir.as_path() {
        return Err(Error::Message(format!(
            "session {thread_id} does not belong to {}",
            scope.workdir.display()
        )));
    }
    let Some(input) = input else {
        state.inner.state.store().set_session_metadata_field(
            thread_id,
            SESSION_MAIN_AGENT_METADATA_KEY,
            Some(main_agent_default_metadata()),
        )?;
        return Ok(());
    };
    let input = input.trim();
    if input.is_empty() {
        return Err(Error::Message(
            "settings/update agent must be null or a concrete agent".to_string(),
        ));
    }
    let catalog = discover_gateway_agents(state, scope)?;
    if catalog.shadowed_agents.iter().any(|agent| {
        agent
            .file_path
            .as_ref()
            .is_some_and(|path| path.to_string_lossy() == input)
    }) {
        return Err(Error::Message(format!(
            "shadowed agent definitions cannot be used as main: {input}"
        )));
    }
    let agent =
        resolve_agent_definition(&catalog, input, &scope.workdir, &state.inner.inherited_env)?;
    state.inner.state.store().set_session_metadata_field(
        thread_id,
        SESSION_MAIN_AGENT_METADATA_KEY,
        Some(main_agent_metadata(
            input,
            &agent.name,
            agent.source,
            agent.file_path.as_ref(),
        )),
    )?;
    Ok(())
}

fn workbench_project_value(workdir: &Path) -> wire::WorkbenchProjectView {
    wire::WorkbenchProjectView {
        path: workdir.display().to_string(),
        display_path: display_workdir(workdir),
        branch: current_git_branch(workdir),
    }
}

fn display_workdir(workdir: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(home) = home
        && let Ok(relative) = workdir.strip_prefix(&home)
    {
        let relative = relative.to_string_lossy();
        return if relative.is_empty() {
            "~".to_string()
        } else {
            format!("~/{}", relative.replace('\\', "/"))
        };
    }
    workdir.to_string_lossy().replace('\\', "/")
}

fn current_git_branch(workdir: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(workdir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!branch.is_empty()).then_some(branch)
}
