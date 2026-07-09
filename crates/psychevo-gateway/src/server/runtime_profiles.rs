use super::*;

#[derive(Clone)]
struct RuntimeProfileRecord {
    config: RuntimeProfileConfig,
    generated: bool,
}

pub(super) fn runtime_profile_list_result(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<wire::RuntimeProfileListResult> {
    let profiles = runtime_profile_records(state, scope)?;
    Ok(wire::RuntimeProfileListResult {
        profiles: profiles
            .values()
            .map(|record| runtime_profile_view(state, scope, record, None))
            .collect::<psychevo_runtime::Result<Vec<_>>>()?,
    })
}

pub(super) fn runtime_profile_read_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeProfileReadParams,
) -> psychevo_runtime::Result<Value> {
    let profiles = runtime_profile_records(state, scope)?;
    let record = profiles
        .get(&params.id)
        .ok_or_else(|| Error::Message(format!("unknown runtime profile: {}", params.id)))?;
    Ok(serde_json::to_value(wire::RuntimeProfileReadResult {
        profile: runtime_profile_view(state, scope, record, None)?,
        options: Some(record.config.options.clone()),
    })?)
}

pub(super) fn runtime_snapshot_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeSnapshotParams,
) -> psychevo_runtime::Result<wire::RuntimeSnapshotResult> {
    let profiles = runtime_profile_records(state, scope)?;
    let mut selected = Vec::new();
    for record in profiles.values() {
        if let Some(runtime_ref) = params.runtime_ref.as_deref()
            && record.config.id != runtime_ref
        {
            continue;
        }
        selected.push(runtime_profile_view(state, scope, record, None)?);
    }
    Ok(wire::RuntimeSnapshotResult {
        agents: runtime_snapshot_agents(&selected),
        profiles: selected,
    })
}

pub(super) fn runtime_profile_options(
    state: &WebState,
    scope: &ResolvedScope,
    runtime_ref: &str,
) -> psychevo_runtime::Result<Option<Vec<wire::RuntimeConfigOptionView>>> {
    let profiles = runtime_profile_records(state, scope)?;
    let Some(record) = profiles.get(runtime_ref) else {
        return Ok(None);
    };
    let profile = runtime_profile_view(state, scope, record, None)?;
    let options = match profile.runtime.as_str() {
        "native" => vec![native_runtime_mode_option()],
        "codex" => codex_runtime_config_options(&profile),
        "opencode" => opencode_runtime_config_options(&profile),
        "acp" => return Ok(None),
        _ => Vec::new(),
    };
    Ok(Some(options))
}

pub(super) fn resolve_runtime_ref_peer_turn(
    state: &WebState,
    scope: &ResolvedScope,
    runtime_ref: &str,
) -> psychevo_runtime::Result<Option<crate::ResolvedPeerTurn>> {
    let runtime_ref = runtime_ref.trim();
    if runtime_ref.is_empty() || runtime_ref == "native" {
        return Ok(None);
    }
    let mut options = state.run_options(scope.cwd.clone(), None);
    options.runtime_ref = Some(runtime_ref.to_string());
    crate::resolve_peer_turn(&options)
}

pub(super) fn ensure_turn_runtime_profile_supported(
    state: &WebState,
    scope: &ResolvedScope,
    runtime_ref: Option<&str>,
) -> psychevo_runtime::Result<()> {
    let runtime_ref = runtime_ref
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("native");
    if runtime_ref == "native" {
        return Ok(());
    }
    let peer_resolution = resolve_runtime_ref_peer_turn(state, scope, runtime_ref);
    if matches!(peer_resolution, Ok(Some(_))) {
        return Ok(());
    }
    let profiles = runtime_profile_records(state, scope)?;
    let Some(record) = profiles.get(runtime_ref) else {
        peer_resolution?;
        return Err(Error::Message(format!(
            "unknown runtime profile: {runtime_ref}"
        )));
    };
    if !record.config.enabled {
        return Err(Error::Message(format!(
            "runtime profile `{runtime_ref}` is disabled"
        )));
    }
    match record.config.runtime {
        RuntimeProfileKind::Native => Ok(()),
        RuntimeProfileKind::Codex | RuntimeProfileKind::OpenCode => Err(Error::Message(format!(
            "runtime profile `{runtime_ref}` uses a direct {} runtime, but direct turn execution is not enabled yet",
            record.config.runtime.as_str()
        ))),
        RuntimeProfileKind::Acp => Err(Error::Message(format!(
            "runtime profile `{runtime_ref}` uses ACP compatibility, but runtimeRef turn execution is not enabled for ACP profiles"
        ))),
    }
}

pub(super) fn runtime_health_check_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeHealthCheckParams,
) -> psychevo_runtime::Result<Value> {
    let profiles = runtime_profile_records(state, scope)?;
    let record = profiles.get(&params.runtime_ref).ok_or_else(|| {
        Error::Message(format!("unknown runtime profile: {}", params.runtime_ref))
    })?;
    Ok(serde_json::to_value(runtime_profile_view(
        state,
        scope,
        record,
        Some(gateway_now_ms()),
    )?)?)
}

pub(super) fn write_runtime_profile(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeProfileWriteParams,
) -> psychevo_runtime::Result<Value> {
    if !valid_agent_name(&params.id) {
        return Err(Error::Message(format!(
            "invalid runtime profile id: {}",
            params.id
        )));
    }
    validate_runtime_profile_kind(&params.runtime)?;
    ensure_profile_config_for_runtime_profile_write(state, scope, params.target)?;
    let value = runtime_profile_config_json(&params, None)?;
    let target = params.target;
    let config_dir = runtime_profile_config_dir(state, scope, target);
    let result = set_config_value(
        config_dir,
        &format!("runtime_profiles.{}", params.id),
        value,
    )?;
    let profiles = runtime_profile_records(state, scope)?;
    let record = profiles.get(&params.id).ok_or_else(|| {
        Error::Message(format!(
            "runtime profile write did not reload: {}",
            params.id
        ))
    })?;
    Ok(serde_json::to_value(wire::RuntimeProfileWriteResult {
        written: true,
        changed: result.changed,
        path: result.path.display().to_string(),
        target,
        profile: runtime_profile_view(state, scope, record, None)?,
    })?)
}

pub(super) fn set_runtime_profile_enabled(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeProfileSetEnabledParams,
) -> psychevo_runtime::Result<Value> {
    let profiles = runtime_profile_records(state, scope)?;
    let existing = profiles
        .get(&params.id)
        .ok_or_else(|| Error::Message(format!("unknown runtime profile: {}", params.id)))?;
    let write = wire::RuntimeProfileWriteParams {
        id: existing.config.id.clone(),
        target: params.target,
        runtime: existing.config.runtime.as_str().to_string(),
        enabled: Some(params.enabled),
        label: Some(existing.config.label.clone()),
        command: existing.config.command.clone(),
        args: existing.config.args.clone(),
        env: existing.config.env.clone(),
        default_model: existing.config.default_model.clone(),
        default_mode: existing.config.default_mode.clone(),
        default_agent: existing.config.default_agent.clone(),
        approval_mode: existing.config.approval_mode.clone(),
        sandbox: existing.config.sandbox.clone(),
        workspace_roots: existing.config.workspace_roots.clone(),
        options: Some(existing.config.options.clone()),
        scope: Some(scope.to_wire_scope()),
    };
    write_runtime_profile(state, scope, write)
}

pub(super) fn delete_runtime_profile(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeProfileDeleteParams,
) -> psychevo_runtime::Result<Value> {
    if !valid_agent_name(&params.id) {
        return Err(Error::Message(format!(
            "invalid runtime profile id: {}",
            params.id
        )));
    }
    let target = params.target;
    let config_dir = runtime_profile_config_dir(state, scope, target);
    let result = remove_config_value(config_dir, &format!("runtime_profiles.{}", params.id))?;
    Ok(serde_json::to_value(wire::RuntimeProfileDeleteResult {
        deleted: result.changed,
        changed: result.changed,
        id: params.id,
        path: result.path.display().to_string(),
        target,
    })?)
}

pub(super) fn runtime_session_list_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeSessionListParams,
) -> psychevo_runtime::Result<wire::RuntimeSessionListResult> {
    let runtime_ref = params.runtime_ref.unwrap_or_else(|| "native".to_string());
    if runtime_ref != "native" {
        return Ok(wire::RuntimeSessionListResult {
            runtime_ref,
            supported: false,
            sessions: Vec::new(),
        });
    }
    let sessions = state
        .inner
        .state
        .store()
        .list_sessions_for_cwd_with_sources(&scope.cwd, &[])?
        .into_iter()
        .map(native_runtime_session_view)
        .collect();
    Ok(wire::RuntimeSessionListResult {
        runtime_ref,
        supported: true,
        sessions,
    })
}

pub(super) fn runtime_session_read_result(
    state: &WebState,
    params: wire::RuntimeSessionParams,
) -> psychevo_runtime::Result<wire::RuntimeSessionMutationResult> {
    let Some(summary) = state
        .inner
        .state
        .store()
        .session_summary(&params.native_session_id)?
    else {
        return Err(Error::Message(format!(
            "runtime session not found: {}",
            params.native_session_id
        )));
    };
    Ok(wire::RuntimeSessionMutationResult {
        runtime_ref: params.runtime_ref,
        native_session_id: params.native_session_id,
        supported: true,
        changed: false,
        session: Some(native_runtime_session_view(summary)),
        message: None,
    })
}

pub(super) fn runtime_session_resume_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::RuntimeSessionParams,
) -> psychevo_runtime::Result<wire::RuntimeSessionMutationResult> {
    if params.runtime_ref != "native" {
        return unsupported_runtime_session_mutation(params.runtime_ref, params.native_session_id);
    }
    let backend = GatewayBackendInfo {
        kind: BackendKind::Psychevo,
        runtime_ref: Some("native".to_string()),
        native_id: Some(params.native_session_id.clone()),
    };
    state.inner.gateway.bind_source_thread(
        &scope.source,
        &params.native_session_id,
        &backend,
        None,
    )?;
    runtime_session_read_result(state, params).map(|mut result| {
        result.changed = true;
        result
    })
}

pub(super) fn runtime_session_archive_result(
    state: &WebState,
    params: wire::RuntimeSessionParams,
    archived: bool,
) -> psychevo_runtime::Result<wire::RuntimeSessionMutationResult> {
    if params.runtime_ref != "native" {
        return unsupported_runtime_session_mutation(params.runtime_ref, params.native_session_id);
    }
    if archived {
        state
            .inner
            .state
            .store()
            .archive_session(&params.native_session_id)?;
    } else {
        state
            .inner
            .state
            .store()
            .restore_session(&params.native_session_id)?;
    }
    runtime_session_read_result(state, params).map(|mut result| {
        result.changed = true;
        result
    })
}

pub(super) fn runtime_session_delete_result(
    state: &WebState,
    params: wire::RuntimeSessionParams,
) -> psychevo_runtime::Result<wire::RuntimeSessionMutationResult> {
    if params.runtime_ref != "native" {
        return unsupported_runtime_session_mutation(params.runtime_ref, params.native_session_id);
    }
    state
        .inner
        .state
        .delete_session(&params.native_session_id)?;
    Ok(wire::RuntimeSessionMutationResult {
        runtime_ref: params.runtime_ref,
        native_session_id: params.native_session_id,
        supported: true,
        changed: true,
        session: None,
        message: None,
    })
}

pub(super) fn runtime_session_rename_result(
    state: &WebState,
    params: wire::RuntimeSessionRenameParams,
) -> psychevo_runtime::Result<wire::RuntimeSessionMutationResult> {
    if params.runtime_ref != "native" {
        return unsupported_runtime_session_mutation(params.runtime_ref, params.native_session_id);
    }
    state
        .inner
        .state
        .store()
        .set_session_title(&params.native_session_id, &params.title)?;
    runtime_session_read_result(
        state,
        wire::RuntimeSessionParams {
            runtime_ref: params.runtime_ref,
            native_session_id: params.native_session_id,
            scope: params.scope,
        },
    )
    .map(|mut result| {
        result.changed = true;
        result
    })
}

pub(super) fn runtime_session_rollback_result(
    params: wire::RuntimeSessionRollbackParams,
) -> psychevo_runtime::Result<wire::RuntimeSessionMutationResult> {
    unsupported_runtime_session_mutation(params.runtime_ref, params.native_session_id)
}

fn runtime_profile_records(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<BTreeMap<String, RuntimeProfileRecord>> {
    let configured =
        load_runtime_profile_configs(&state.inner.home, &scope.cwd, &state.inner.inherited_env)?;
    let mut records = generated_runtime_profiles()
        .into_iter()
        .map(|config| {
            (
                config.id.clone(),
                RuntimeProfileRecord {
                    config,
                    generated: true,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    for config in configured.into_values() {
        records.insert(
            config.id.clone(),
            RuntimeProfileRecord {
                config,
                generated: false,
            },
        );
    }
    Ok(records)
}

fn generated_runtime_profiles() -> Vec<RuntimeProfileConfig> {
    vec![
        RuntimeProfileConfig {
            id: "native".to_string(),
            runtime: RuntimeProfileKind::Native,
            enabled: true,
            label: "Native".to_string(),
            command: None,
            args: Vec::new(),
            env: BTreeMap::new(),
            default_model: None,
            default_mode: None,
            default_agent: None,
            approval_mode: None,
            sandbox: None,
            workspace_roots: Vec::new(),
            options: Value::Null,
        },
        RuntimeProfileConfig {
            id: "codex".to_string(),
            runtime: RuntimeProfileKind::Codex,
            enabled: true,
            label: "Codex".to_string(),
            command: Some("codex".to_string()),
            args: vec!["app-server".to_string(), "--stdio".to_string()],
            env: BTreeMap::new(),
            default_model: None,
            default_mode: None,
            default_agent: None,
            approval_mode: None,
            sandbox: None,
            workspace_roots: Vec::new(),
            options: Value::Null,
        },
        RuntimeProfileConfig {
            id: "opencode".to_string(),
            runtime: RuntimeProfileKind::OpenCode,
            enabled: true,
            label: "OpenCode".to_string(),
            command: Some("opencode".to_string()),
            args: vec!["serve".to_string()],
            env: BTreeMap::new(),
            default_model: None,
            default_mode: Some("build".to_string()),
            default_agent: Some("build".to_string()),
            approval_mode: None,
            sandbox: None,
            workspace_roots: Vec::new(),
            options: Value::Null,
        },
    ]
}

fn runtime_profile_view(
    state: &WebState,
    scope: &ResolvedScope,
    record: &RuntimeProfileRecord,
    checked_at_ms: Option<i64>,
) -> psychevo_runtime::Result<wire::RuntimeProfileView> {
    let config = &record.config;
    Ok(wire::RuntimeProfileView {
        id: config.id.clone(),
        runtime: config.runtime.as_str().to_string(),
        enabled: config.enabled,
        label: config.label.clone(),
        generated: record.generated,
        configured: !record.generated,
        command: config.command.clone(),
        args: config.args.clone(),
        default_model: config.default_model.clone(),
        default_mode: config.default_mode.clone(),
        default_agent: config.default_agent.clone(),
        approval_mode: config.approval_mode.clone(),
        sandbox: config.sandbox.clone(),
        workspace_roots: config.workspace_roots.clone(),
        env_keys: config.env.keys().cloned().collect(),
        option_keys: runtime_profile_option_keys(&config.options),
        source_targets: runtime_profile_source_targets(state, scope, &config.id)?,
        health: runtime_profile_health(state, scope, config, checked_at_ms),
        diagnostics: runtime_profile_diagnostics(config),
    })
}

fn runtime_profile_health(
    state: &WebState,
    scope: &ResolvedScope,
    config: &RuntimeProfileConfig,
    checked_at_ms: Option<i64>,
) -> wire::RuntimeHealthView {
    if !config.enabled {
        return wire::RuntimeHealthView {
            status: "disabled".to_string(),
            summary: "runtime profile disabled".to_string(),
            command_path: None,
            checked_at_ms,
        };
    }
    if matches!(config.runtime, RuntimeProfileKind::Native) {
        return wire::RuntimeHealthView {
            status: "ready".to_string(),
            summary: "native runtime available".to_string(),
            command_path: None,
            checked_at_ms,
        };
    }
    let Some(command) = config
        .command
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    else {
        return wire::RuntimeHealthView {
            status: "missing".to_string(),
            summary: "runtime command missing".to_string(),
            command_path: None,
            checked_at_ms,
        };
    };
    let mut env = state.inner.inherited_env.clone();
    env.extend(config.env.clone());
    match resolve_executable_path(
        command,
        &scope.cwd,
        &ExecutableResolveOptions {
            platform: HostPlatform::current(),
            env: &env,
        },
    ) {
        Some(path) => wire::RuntimeHealthView {
            status: "ready".to_string(),
            summary: "runtime command resolved".to_string(),
            command_path: Some(path.display().to_string()),
            checked_at_ms,
        },
        None => wire::RuntimeHealthView {
            status: "missing".to_string(),
            summary: "runtime command was not found on PATH or as a configured path".to_string(),
            command_path: None,
            checked_at_ms,
        },
    }
}

fn runtime_profile_diagnostics(config: &RuntimeProfileConfig) -> Vec<wire::BackendDiagnosticView> {
    let mut diagnostics = Vec::new();
    if !config.enabled {
        diagnostics.push(wire::BackendDiagnosticView {
            kind: "disabled".to_string(),
            message: "runtime profile is disabled".to_string(),
        });
    }
    if !matches!(config.runtime, RuntimeProfileKind::Native) && config.command.is_none() {
        diagnostics.push(wire::BackendDiagnosticView {
            kind: "missing_command".to_string(),
            message: "runtime command is required for execution".to_string(),
        });
    }
    if matches!(
        config.runtime,
        RuntimeProfileKind::Codex | RuntimeProfileKind::OpenCode
    ) {
        diagnostics.push(wire::BackendDiagnosticView {
            kind: "direct_adapter_limited".to_string(),
            message: "direct runtime health is local-only; turn execution is not enabled without the adapter worker".to_string(),
        });
    }
    diagnostics
}

fn runtime_profile_option_keys(options: &Value) -> Vec<String> {
    options
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}

fn runtime_snapshot_agents(
    profiles: &[wire::RuntimeProfileView],
) -> Vec<wire::RuntimeSnapshotAgentView> {
    profiles
        .iter()
        .filter(|profile| profile.enabled)
        .filter_map(|profile| match profile.runtime.as_str() {
            "opencode" => Some(wire::RuntimeSnapshotAgentView {
                name: format!(
                    "{}-{}",
                    profile.id,
                    profile.default_agent.as_deref().unwrap_or("build")
                ),
                label: format!(
                    "{} {}",
                    profile.label,
                    profile.default_agent.as_deref().unwrap_or("build")
                ),
                runtime_ref: profile.id.clone(),
                native_id: profile.default_agent.clone(),
                mode: profile.default_mode.clone(),
            }),
            "codex" => Some(wire::RuntimeSnapshotAgentView {
                name: profile.id.clone(),
                label: profile.label.clone(),
                runtime_ref: profile.id.clone(),
                native_id: None,
                mode: profile.default_mode.clone(),
            }),
            _ => None,
        })
        .collect()
}

fn codex_runtime_config_options(
    profile: &wire::RuntimeProfileView,
) -> Vec<wire::RuntimeConfigOptionView> {
    vec![wire::RuntimeConfigOptionView {
        id: "mode".to_string(),
        name: format!("{} mode", profile.label),
        description: None,
        category: Some("mode".to_string()),
        option_type: "select".to_string(),
        current_value: profile
            .default_mode
            .clone()
            .or_else(|| Some("default".to_string())),
        values: ["default", "auto-review", "full-access"]
            .into_iter()
            .map(|mode| wire::RuntimeConfigOptionValueView {
                value: mode.to_string(),
                name: mode.to_string(),
                description: None,
                group: None,
            })
            .collect(),
    }]
}

fn opencode_runtime_config_options(
    profile: &wire::RuntimeProfileView,
) -> Vec<wire::RuntimeConfigOptionView> {
    let default_mode = profile
        .default_mode
        .clone()
        .unwrap_or_else(|| "build".to_string());
    let mut values = vec![default_mode.clone()];
    if !values.iter().any(|value| value == "plan") {
        values.push("plan".to_string());
    }
    vec![wire::RuntimeConfigOptionView {
        id: "mode".to_string(),
        name: format!("{} mode", profile.label),
        description: None,
        category: Some("mode".to_string()),
        option_type: "select".to_string(),
        current_value: Some(default_mode),
        values: values
            .into_iter()
            .map(|mode| wire::RuntimeConfigOptionValueView {
                value: mode.clone(),
                name: mode,
                description: None,
                group: None,
            })
            .collect(),
    }]
}

fn runtime_profile_config_json(
    params: &wire::RuntimeProfileWriteParams,
    existing: Option<&RuntimeProfileConfig>,
) -> psychevo_runtime::Result<Value> {
    validate_runtime_profile_kind(&params.runtime)?;
    let mut object = serde_json::Map::new();
    object.insert("runtime".to_string(), json!(params.runtime.trim()));
    object.insert("enabled".to_string(), json!(params.enabled.unwrap_or(true)));
    if let Some(label) = params
        .label
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert("label".to_string(), json!(label));
    }
    if let Some(command) = params
        .command
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert("command".to_string(), json!(command));
    }
    object.insert("args".to_string(), json!(trimmed_string_list(&params.args)));
    let env = if params.env.is_empty() {
        existing
            .map(|profile| profile.env.clone())
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
    object.insert("env".to_string(), json!(env));
    insert_optional_string(
        &mut object,
        "default_model",
        params.default_model.as_deref(),
    );
    insert_optional_string(&mut object, "default_mode", params.default_mode.as_deref());
    insert_optional_string(
        &mut object,
        "default_agent",
        params.default_agent.as_deref(),
    );
    insert_optional_string(
        &mut object,
        "approval_mode",
        params.approval_mode.as_deref(),
    );
    insert_optional_string(&mut object, "sandbox", params.sandbox.as_deref());
    object.insert(
        "workspace_roots".to_string(),
        json!(trimmed_string_list(&params.workspace_roots)),
    );
    if let Some(options) = params.options.clone().filter(|value| !value.is_null()) {
        object.insert("options".to_string(), options);
    }
    Ok(Value::Object(object))
}

fn insert_optional_string(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<&str>,
) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        object.insert(key.to_string(), json!(value));
    }
}

fn validate_runtime_profile_kind(value: &str) -> psychevo_runtime::Result<()> {
    if matches!(value.trim(), "native" | "codex" | "opencode" | "acp") {
        Ok(())
    } else {
        Err(Error::Message(format!(
            "runtime profile kind `{value}` must be native, codex, opencode, or acp"
        )))
    }
}

fn runtime_profile_config_dir(
    state: &WebState,
    scope: &ResolvedScope,
    target: wire::BackendConfigTarget,
) -> PathBuf {
    match target {
        wire::BackendConfigTarget::Project => scope.cwd.join(".psychevo"),
        wire::BackendConfigTarget::Profile => active_profile_config_dir(state, scope),
    }
}

fn ensure_profile_config_for_runtime_profile_write(
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

fn runtime_profile_source_targets(
    state: &WebState,
    scope: &ResolvedScope,
    id: &str,
) -> psychevo_runtime::Result<Vec<wire::BackendConfigTarget>> {
    let mut targets = Vec::new();
    if runtime_profile_exists_in_config_dir(&active_profile_config_dir(state, scope), id)? {
        targets.push(wire::BackendConfigTarget::Profile);
    }
    if runtime_profile_exists_in_config_dir(&scope.cwd.join(".psychevo"), id)? {
        targets.push(wire::BackendConfigTarget::Project);
    }
    Ok(targets)
}

fn runtime_profile_exists_in_config_dir(
    config_dir: &Path,
    id: &str,
) -> psychevo_runtime::Result<bool> {
    let config_path = config_dir.join("config.toml");
    if !config_path.exists() {
        return Ok(false);
    }
    let text = std::fs::read_to_string(&config_path)?;
    let parsed: toml::Value = toml::from_str(&text)
        .map_err(|err| Error::Config(format!("{}: {err}", config_path.display())))?;
    Ok(parsed
        .get("runtime_profiles")
        .or_else(|| parsed.get("runtimeProfiles"))
        .and_then(|value| value.get(id))
        .is_some())
}

fn native_runtime_session_view(summary: SessionSummary) -> wire::RuntimeSessionView {
    wire::RuntimeSessionView {
        native_session_id: summary.id.clone(),
        thread_id: Some(summary.id),
        title: summary.title,
        archived: summary.archived_at_ms.is_some(),
        updated_at_ms: Some(summary.updated_at_ms),
    }
}

fn unsupported_runtime_session_mutation<T: Into<String>>(
    runtime_ref: T,
    native_session_id: T,
) -> psychevo_runtime::Result<wire::RuntimeSessionMutationResult> {
    Ok(wire::RuntimeSessionMutationResult {
        runtime_ref: runtime_ref.into(),
        native_session_id: native_session_id.into(),
        supported: false,
        changed: false,
        session: None,
        message: Some(
            "runtime session operation is not supported by this adapter slice".to_string(),
        ),
    })
}

fn trimmed_string_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}
