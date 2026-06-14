use super::*;

pub(super) fn write_project_agent_definition(
    workdir: &Path,
    params: wire::AgentWriteParams,
) -> psychevo_runtime::Result<Value> {
    if !valid_agent_name(&params.name) {
        return Err(Error::Message(format!(
            "invalid agent name: {}",
            params.name
        )));
    }
    let description = params.description.trim();
    if description.is_empty() {
        return Err(Error::Message(
            "agent description must be non-empty".to_string(),
        ));
    }
    if let Some(backend) = &params.backend
        && !valid_agent_name(&backend.name)
    {
        return Err(Error::Message(format!(
            "invalid backend ref: {}",
            backend.name
        )));
    }
    let mut entrypoints = Vec::new();
    for entrypoint in &params.entrypoints {
        let parsed = AgentEntrypoint::parse(entrypoint).ok_or_else(|| {
            Error::Message(format!(
                "agent entrypoint `{entrypoint}` must be peer or subagent"
            ))
        })?;
        entrypoints.push(parsed.as_str().to_string());
    }
    let path = project_agent_definition_path(workdir, &params.name);
    let mut frontmatter = serde_yaml::Mapping::new();
    frontmatter.insert(
        serde_yaml::Value::String("name".to_string()),
        serde_yaml::Value::String(params.name.clone()),
    );
    frontmatter.insert(
        serde_yaml::Value::String("description".to_string()),
        serde_yaml::Value::String(description.to_string()),
    );
    if let Some(backend) = params.backend {
        let mut backend_value = serde_yaml::Mapping::new();
        backend_value.insert(
            serde_yaml::Value::String("ref".to_string()),
            serde_yaml::Value::String(backend.name),
        );
        frontmatter.insert(
            serde_yaml::Value::String("backend".to_string()),
            serde_yaml::Value::Mapping(backend_value),
        );
    }
    if !entrypoints.is_empty() {
        frontmatter.insert(
            serde_yaml::Value::String("entrypoints".to_string()),
            serde_yaml::Value::Sequence(
                entrypoints
                    .into_iter()
                    .map(serde_yaml::Value::String)
                    .collect(),
            ),
        );
    }
    if !params.tools.is_empty() {
        frontmatter.insert(
            serde_yaml::Value::String("tools".to_string()),
            serde_yaml::Value::Sequence(
                params
                    .tools
                    .into_iter()
                    .filter(|tool| !tool.trim().is_empty())
                    .map(|tool| serde_yaml::Value::String(tool.trim().to_string()))
                    .collect(),
            ),
        );
    }
    if !params.mcp_servers.is_empty() {
        frontmatter.insert(
            serde_yaml::Value::String("mcpServers".to_string()),
            serde_yaml::Value::Sequence(
                params
                    .mcp_servers
                    .into_iter()
                    .filter(|server| !server.trim().is_empty())
                    .map(|server| serde_yaml::Value::String(server.trim().to_string()))
                    .collect(),
            ),
        );
    }
    let frontmatter = serde_yaml::to_string(&frontmatter)?;
    let body = params.instructions.trim();
    let text = if body.is_empty() {
        format!("---\n{frontmatter}---\n")
    } else {
        format!("---\n{frontmatter}---\n{body}\n")
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, text)?;
    Ok(serde_json::to_value(wire::AgentWriteResult {
        written: true,
        name: params.name,
        path: path.display().to_string(),
    })?)
}

pub(super) fn delete_project_agent_definition(
    workdir: &Path,
    name: &str,
) -> psychevo_runtime::Result<Value> {
    if !valid_agent_name(name) {
        return Err(Error::Message(format!("invalid agent name: {name}")));
    }
    let path = project_agent_definition_path(workdir, name);
    let deleted = match std::fs::remove_file(&path) {
        Ok(()) => true,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => return Err(err.into()),
    };
    Ok(serde_json::to_value(wire::AgentDeleteResult {
        deleted,
        name: name.to_string(),
        path: path.display().to_string(),
    })?)
}

fn project_agent_definition_path(workdir: &Path, name: &str) -> PathBuf {
    workdir
        .join(".psychevo")
        .join("agents")
        .join(format!("{name}.md"))
}

pub(super) fn agent_list_result(catalog: &AgentCatalog) -> wire::AgentListResult {
    wire::AgentListResult {
        agents: catalog.agents.iter().map(agent_definition_view).collect(),
        shadowed_agents: catalog
            .shadowed_agents
            .iter()
            .map(agent_definition_view)
            .collect(),
        diagnostics: catalog
            .diagnostics
            .iter()
            .map(agent_diagnostic_view)
            .collect(),
    }
}

pub(super) fn agent_read_result(agent: &AgentDefinition) -> wire::AgentReadResult {
    wire::AgentReadResult {
        agent: agent_definition_view(agent),
        instructions: agent.instructions.clone(),
    }
}

fn agent_definition_view(agent: &AgentDefinition) -> wire::AgentDefinitionView {
    wire::AgentDefinitionView {
        name: agent.name.clone(),
        description: agent.description.clone(),
        source: agent.source.as_str().to_string(),
        generated: matches!(agent.source, psychevo_runtime::AgentSource::Generated),
        path: agent
            .file_path
            .as_ref()
            .map(|path| path.display().to_string()),
        backend: agent
            .backend
            .as_ref()
            .map(|backend| wire::AgentBackendRefView {
                name: backend.name.clone(),
            }),
        entrypoints: agent
            .entrypoints
            .iter()
            .map(|entrypoint| entrypoint.as_str().to_string())
            .collect(),
        diagnostics: agent
            .diagnostics
            .iter()
            .map(agent_diagnostic_view)
            .collect(),
    }
}

fn agent_diagnostic_view(diagnostic: &AgentDiagnostic) -> wire::AgentDiagnosticView {
    wire::AgentDiagnosticView {
        kind: diagnostic.kind.clone(),
        message: diagnostic.message.clone(),
        path: diagnostic
            .path
            .as_ref()
            .map(|path| path.display().to_string()),
    }
}

pub(super) fn agent_status_result(
    store: Option<&psychevo_runtime::SqliteStore>,
    parent_session_id: Option<&str>,
    all: bool,
) -> wire::AgentStatusResult {
    wire::AgentStatusResult {
        agents: agent_status_records(store, parent_session_id, all)
            .iter()
            .map(agent_run_view)
            .collect(),
        control: wire::AgentStatusControlView {
            spawning_paused: agent_spawn_paused(),
            max_spawn_depth_cap: MAX_AGENT_SPAWN_DEPTH_CAP,
            concurrency_cap: None,
        },
    }
}

fn agent_run_view(record: &AgentRunRecord) -> wire::AgentRunView {
    wire::AgentRunView {
        id: record.id.clone(),
        task_name: record.task_name.clone(),
        agent_name: record.agent_name.clone(),
        task: record.task.clone(),
        parent_session_id: record.parent_session_id.clone(),
        child_session_id: record.child_session_id.clone(),
        role: record.role.as_str().to_string(),
        background: record.background,
        status: record.status.as_str().to_string(),
        edge_status: record.edge_status.map(|status| status.as_str().to_string()),
        started_at_ms: record.started_at_ms,
        ended_at_ms: record.ended_at_ms,
        outcome: record.outcome.clone(),
        final_answer: record.final_answer.clone(),
        error: record.error.clone(),
        effective_max_spawn_depth: record.effective_max_spawn_depth,
    }
}

fn backend_value_with_sources(
    backend: &AgentBackendConfig,
    source_targets: Vec<wire::BackendConfigTarget>,
) -> wire::BackendConfigView {
    wire::BackendConfigView {
        id: backend.id.clone(),
        kind: backend.kind.as_str().to_string(),
        enabled: backend.enabled,
        label: backend.label.clone(),
        description: backend.description.clone(),
        command: backend.command.clone(),
        args: backend.args.clone(),
        cwd: backend.cwd.clone(),
        entrypoints: backend
            .entrypoints
            .iter()
            .map(|entrypoint| entrypoint.as_str().to_string())
            .collect(),
        client_capabilities: backend.client_capabilities.iter().cloned().collect(),
        mcp_servers: backend.mcp_servers.iter().cloned().collect(),
        env_keys: backend.env.keys().cloned().collect(),
        source_targets,
        diagnostics: backend_diagnostics(backend),
    }
}

pub(super) fn backend_values_for_scope(
    state: &WebState,
    scope: &ResolvedScope,
    backends: &BTreeMap<String, AgentBackendConfig>,
) -> psychevo_runtime::Result<Vec<wire::BackendConfigView>> {
    backends
        .values()
        .map(|backend| {
            Ok(backend_value_with_sources(
                backend,
                backend_source_targets(state, scope, &backend.id)?,
            ))
        })
        .collect()
}

pub(super) fn write_backend_config(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::BackendWriteParams,
) -> psychevo_runtime::Result<Value> {
    if !valid_agent_name(&params.id) {
        return Err(Error::Message(format!("invalid backend id: {}", params.id)));
    }
    ensure_profile_config_for_backend_write(state, scope, params.target)?;
    let existing_backends = load_agent_backend_configs(
        &state.inner.home,
        &scope.workdir,
        &state.inner.inherited_env,
    )?;
    let value = backend_config_json(&params, existing_backends.get(&params.id))?;
    let target = params.target;
    let config_dir = backend_config_dir(state, scope, target)?;
    let result = set_config_value(config_dir, &format!("agents.backends.{}", params.id), value)?;
    let backends = load_agent_backend_configs(
        &state.inner.home,
        &scope.workdir,
        &state.inner.inherited_env,
    )?;
    let backend = backends
        .get(&params.id)
        .ok_or_else(|| Error::Message(format!("backend write did not reload: {}", params.id)))?;
    Ok(serde_json::to_value(wire::BackendWriteResult {
        written: true,
        changed: result.changed,
        path: result.path.display().to_string(),
        target,
        backend: backend_value_with_sources(
            backend,
            backend_source_targets(state, scope, &backend.id)?,
        ),
    })?)
}

pub(super) fn delete_backend_config(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::BackendDeleteParams,
) -> psychevo_runtime::Result<Value> {
    if !valid_agent_name(&params.id) {
        return Err(Error::Message(format!("invalid backend id: {}", params.id)));
    }
    let target = params.target;
    let config_dir = backend_config_dir(state, scope, target)?;
    let result = remove_config_value(config_dir, &format!("agents.backends.{}", params.id))?;
    Ok(serde_json::to_value(wire::BackendDeleteResult {
        deleted: result.changed,
        changed: result.changed,
        id: params.id,
        path: result.path.display().to_string(),
        target,
    })?)
}

fn backend_config_dir(
    state: &WebState,
    scope: &ResolvedScope,
    target: wire::BackendConfigTarget,
) -> psychevo_runtime::Result<PathBuf> {
    match target {
        wire::BackendConfigTarget::Project => Ok(scope.workdir.join(".psychevo")),
        wire::BackendConfigTarget::Profile => Ok(active_profile_config_dir(state, scope)),
    }
}

fn ensure_profile_config_for_backend_write(
    state: &WebState,
    scope: &ResolvedScope,
    target: wire::BackendConfigTarget,
) -> psychevo_runtime::Result<()> {
    if target != wire::BackendConfigTarget::Profile
        || !state
            .inner
            .inherited_env
            .get("PSYCHEVO_CONFIG")
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    {
        return Ok(());
    }
    let config_path = active_profile_config_dir(state, scope).join("config.toml");
    if config_path.exists() {
        return Ok(());
    }
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(config_path, "")?;
    Ok(())
}

fn active_profile_config_dir(state: &WebState, scope: &ResolvedScope) -> PathBuf {
    state
        .inner
        .inherited_env
        .get("PSYCHEVO_CONFIG")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .and_then(|value| {
            let path = resolve_gateway_env_path(value, state, scope);
            path.parent().map(Path::to_path_buf)
        })
        .unwrap_or_else(|| state.inner.home.clone())
}

fn resolve_gateway_env_path(value: &str, state: &WebState, scope: &ResolvedScope) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/") {
        let home = state
            .inner
            .inherited_env
            .get("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| state.inner.home.clone());
        return home.join(rest);
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        scope.workdir.join(path)
    }
}

fn backend_config_json(
    params: &wire::BackendWriteParams,
    existing: Option<&AgentBackendConfig>,
) -> psychevo_runtime::Result<Value> {
    let entrypoints = if params.entrypoints.is_empty() {
        vec!["peer".to_string(), "subagent".to_string()]
    } else {
        validate_backend_entrypoints(&params.entrypoints)?
    };
    let client_capabilities = if params.client_capabilities.is_empty() {
        vec![
            "fs.read".to_string(),
            "fs.write".to_string(),
            "terminal".to_string(),
        ]
    } else {
        validate_backend_client_capabilities(&params.client_capabilities)?
    };
    let args = trimmed_string_list(&params.args);
    let mcp_servers = trimmed_string_list(&params.mcp_servers);
    let env = if params.env.is_empty() {
        existing
            .map(|backend| backend.env.clone())
            .unwrap_or_default()
    } else {
        params
            .env
            .iter()
            .filter_map(|(key, value)| {
                let key = key.trim();
                if key.is_empty() {
                    None
                } else {
                    Some((key.to_string(), value.to_string()))
                }
            })
            .collect::<BTreeMap<_, _>>()
    };
    let label = params
        .label
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let cwd = params
        .cwd
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("invocation");
    let mut object = serde_json::Map::new();
    object.insert("kind".to_string(), json!("acp"));
    object.insert("enabled".to_string(), json!(params.enabled.unwrap_or(true)));
    if let Some(label) = label {
        object.insert("label".to_string(), json!(label));
    }
    if let Some(description) = params
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert("description".to_string(), json!(description));
    }
    if let Some(command) = params
        .command
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert("command".to_string(), json!(command));
    }
    object.insert("args".to_string(), json!(args));
    object.insert("env".to_string(), json!(env));
    object.insert("cwd".to_string(), json!(cwd));
    object.insert("entrypoints".to_string(), json!(entrypoints));
    object.insert(
        "client_capabilities".to_string(),
        json!(client_capabilities),
    );
    object.insert("mcp_servers".to_string(), json!(mcp_servers));
    Ok(Value::Object(object))
}

fn validate_backend_entrypoints(values: &[String]) -> psychevo_runtime::Result<Vec<String>> {
    let mut entrypoints = Vec::new();
    for value in values {
        let value = value.trim();
        let entrypoint = AgentEntrypoint::parse(value).ok_or_else(|| {
            Error::Message(format!(
                "backend entrypoint `{value}` must be peer or subagent"
            ))
        })?;
        let entrypoint = entrypoint.as_str().to_string();
        if !entrypoints.contains(&entrypoint) {
            entrypoints.push(entrypoint);
        }
    }
    if entrypoints.is_empty() {
        return Err(Error::Message(
            "backend entrypoints must include peer or subagent".to_string(),
        ));
    }
    Ok(entrypoints)
}

fn validate_backend_client_capabilities(
    values: &[String],
) -> psychevo_runtime::Result<Vec<String>> {
    let mut capabilities = Vec::new();
    for value in values {
        let value = value.trim();
        if !matches!(value, "fs.read" | "fs.write" | "terminal") {
            return Err(Error::Message(format!(
                "backend client capability `{value}` must be fs.read, fs.write, or terminal"
            )));
        }
        if !capabilities.iter().any(|capability| capability == value) {
            capabilities.push(value.to_string());
        }
    }
    Ok(capabilities)
}

fn trimmed_string_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn backend_source_targets(
    state: &WebState,
    scope: &ResolvedScope,
    id: &str,
) -> psychevo_runtime::Result<Vec<wire::BackendConfigTarget>> {
    let mut targets = Vec::new();
    if backend_exists_in_scope(state, scope, id, ConfigScope::Global)? {
        targets.push(wire::BackendConfigTarget::Profile);
    }
    if backend_exists_in_scope(state, scope, id, ConfigScope::Local)? {
        targets.push(wire::BackendConfigTarget::Project);
    }
    Ok(targets)
}

fn backend_exists_in_scope(
    state: &WebState,
    scope: &ResolvedScope,
    id: &str,
    config_scope: ConfigScope,
) -> psychevo_runtime::Result<bool> {
    let config_dir = match config_scope {
        ConfigScope::Global => active_profile_config_dir(state, scope),
        ConfigScope::Local => scope.workdir.join(".psychevo"),
        ConfigScope::Effective => {
            return Err(Error::Config(
                "backend source target checks require a concrete config scope".to_string(),
            ));
        }
    };
    backend_exists_in_config_dir(&config_dir, id)
}

fn backend_exists_in_config_dir(config_dir: &Path, id: &str) -> psychevo_runtime::Result<bool> {
    let config_path = config_dir.join("config.toml");
    if !config_path.exists() {
        return Ok(false);
    }
    let text = std::fs::read_to_string(&config_path)?;
    let parsed: toml::Value = toml::from_str(&text)
        .map_err(|err| Error::Config(format!("{}: {err}", config_path.display())))?;
    Ok(parsed
        .get("agents")
        .and_then(|value| value.get("backends"))
        .and_then(|value| value.get(id))
        .is_some())
}

fn backend_diagnostics(backend: &AgentBackendConfig) -> Vec<wire::BackendDiagnosticView> {
    let mut diagnostics = Vec::new();
    if !backend.enabled {
        diagnostics.push(wire::BackendDiagnosticView {
            kind: "disabled".to_string(),
            message: "backend is disabled".to_string(),
        });
    }
    if backend.command.is_none() {
        diagnostics.push(wire::BackendDiagnosticView {
            kind: "missing_command".to_string(),
            message: "backend command is required for execution".to_string(),
        });
    }
    diagnostics
}

pub(super) fn backend_doctor_value(
    backend: &AgentBackendConfig,
    env: &BTreeMap<String, String>,
) -> psychevo_runtime::Result<wire::BackendDoctorResult> {
    let mut checks = Vec::new();
    checks.push(wire::BackendDoctorCheck {
        name: "enabled".to_string(),
        ok: backend.enabled,
        message: if backend.enabled {
            "backend enabled"
        } else {
            "backend disabled"
        }
        .to_string(),
        path: None,
    });
    checks.push(wire::BackendDoctorCheck {
        name: "description".to_string(),
        ok: true,
        message: if backend
            .description
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            "description configured"
        } else {
            "description optional; using backend label"
        }
        .to_string(),
        path: None,
    });
    let command_check = match backend.command.as_deref() {
        Some(command) => match resolve_command_path(command, env) {
            Some(path) => wire::BackendDoctorCheck {
                name: "command".to_string(),
                ok: true,
                message: "command resolved".to_string(),
                path: Some(path.display().to_string()),
            },
            None => wire::BackendDoctorCheck {
                name: "command".to_string(),
                ok: false,
                message: "command was not found on PATH or as a configured path".to_string(),
                path: None,
            },
        },
        None => wire::BackendDoctorCheck {
            name: "command".to_string(),
            ok: false,
            message: "command missing".to_string(),
            path: None,
        },
    };
    checks.push(command_check);
    let ok = checks.iter().all(|check| check.ok);
    Ok(wire::BackendDoctorResult {
        id: backend.id.clone(),
        kind: backend.kind.as_str().to_string(),
        ok,
        checks,
    })
}

fn resolve_command_path(command: &str, env: &BTreeMap<String, String>) -> Option<PathBuf> {
    let command_path = PathBuf::from(command);
    if command_path.components().count() > 1 {
        return command_path.is_file().then_some(command_path);
    }
    let path_var = env
        .get("PATH")
        .cloned()
        .or_else(|| std::env::var("PATH").ok())?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(command))
        .find(|path| path.is_file())
}
