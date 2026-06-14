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
    if usage.estimated_cost_nanodollars == 0
        && let Some(cost) = acp_peer_usage_cost_nanodollars(peer_usage)
    {
        usage.estimated_cost_nanodollars = cost;
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
        unknown_pricing_count: 0,
        cache_read_percent: None,
    }
}

fn settings_read_value(
    state: &WebState,
    workdir: &Path,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<Value> {
    let controls = workbench_controls_value(state, workdir, thread_id)?;
    let project = workbench_project_value(workdir);
    Ok(json!({
        "workdir": workdir,
        "project": project,
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
    let selected = selected_configured_model(&options).ok().flatten();
    let configured = configured_models(&options).unwrap_or_default();
    Ok(wire::WorkbenchControlsView {
        permission_mode: PermissionMode::Default.as_str().to_string(),
        mode: RunMode::Default.as_str().to_string(),
        runtime_ref: "native".to_string(),
        agent,
        model: selected
            .as_ref()
            .map(|model| format!("{}/{}", model.provider, model.model)),
        variant: selected
            .as_ref()
            .and_then(|model| model.reasoning_effort.clone())
            .or_else(|| Some("none".to_string())),
        permission_mode_options: ["default", "acceptEdits", "dontAsk", "bypassPermissions"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        mode_options: ["default", "plan"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        model_options: configured
            .into_iter()
            .map(|model| format!("{}/{}", model.provider, model.model))
            .collect(),
        variant_options: ["none", "minimal", "low", "medium", "high", "xhigh", "max"]
            .into_iter()
            .map(str::to_string)
            .collect(),
    })
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
