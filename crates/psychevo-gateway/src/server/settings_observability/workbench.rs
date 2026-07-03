fn settings_read_value(
    state: &WebState,
    cwd: &Path,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<Value> {
    let normalized_cwd = psychevo_runtime::normalized_native_path(cwd);
    let cwd = normalized_cwd.as_path();
    let controls = workbench_controls_value(state, cwd, thread_id)?;
    let project = workbench_project_value(cwd);
    let channels = channel_list_result_for_cwd(state, cwd).unwrap_or_default();
    Ok(json!({
        "cwd": cwd.display().to_string(),
        "project": project,
        "channels": channels,
        "memoryResources": {"mode": "status_only", "available": true},
        "secrets": {"frontendPersistence": "disabled"},
        "controls": controls
    }))
}

fn workbench_controls_value(
    state: &WebState,
    cwd: &Path,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<wire::WorkbenchControlsView> {
    let options = state.run_options(cwd.to_path_buf(), None);
    let agent = session_control_agent(state, thread_id)?;
    let model_state = ModelState::load(&ModelState::path_for_home(&state.inner.home))?;
    let cwd_key = cwd.to_string_lossy().to_string();
    let session_selection = thread_id
        .map(|thread_id| session_model_state_selection(state, thread_id))
        .transpose()?
        .flatten();
    let state_model = model_state.model_for(&cwd_key);
    let state_reasoning_effort = model_state.reasoning_effort_for(&cwd_key);
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
    let model_details = model_options_with_cached_catalog(state, &options, &configured);
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
    if Path::new(&summary.cwd) != scope.cwd.as_path() {
        return Err(Error::Message(format!(
            "session {thread_id} does not belong to {}",
            scope.cwd.display()
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
        resolve_agent_definition(&catalog, input, &scope.cwd, &state.inner.inherited_env)?;
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

fn workbench_project_value(cwd: &Path) -> wire::WorkbenchProjectView {
    let cwd = psychevo_runtime::normalized_native_path(cwd);
    wire::WorkbenchProjectView {
        path: cwd.display().to_string(),
        display_path: display_cwd(&cwd),
        branch: current_git_branch(&cwd),
    }
}

fn display_cwd(cwd: &Path) -> String {
    let cwd_display = psychevo_runtime::display_path_for_native_path(cwd);
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
        && let Some(display) = display_relative_to_home(
            &cwd_display,
            &psychevo_runtime::display_path_for_native_path(&home),
        )
    {
        return display;
    }
    cwd_display
}

fn display_relative_to_home(cwd_display: &str, home_display: &str) -> Option<String> {
    let home = if home_display == "/" {
        home_display
    } else {
        home_display.trim_end_matches('/')
    };
    if home.is_empty() {
        return None;
    }
    if cwd_display == home {
        return Some("~".to_string());
    }
    cwd_display
        .strip_prefix(&format!("{home}/"))
        .map(|relative| format!("~/{relative}"))
}

fn current_git_branch(cwd: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!branch.is_empty()).then_some(branch)
}
