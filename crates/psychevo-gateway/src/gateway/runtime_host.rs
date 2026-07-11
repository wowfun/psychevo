use psychevo_runtime_host::{
    ExecuteRequest, ExecuteResult, HistoryFidelity, RetryClass, RuntimeAuthOperation,
    RuntimeAuthRequest, RuntimeAuthResult, RuntimeCompactionRequest,
    RuntimeControl as HostRuntimeControl, RuntimeControlSetRequest, RuntimeControlSetResult,
    RuntimeError, RuntimeErrorStage, RuntimeExtensionRequest, RuntimeGoalStatus,
    RuntimeHistoryMessage, RuntimeIntent, RuntimeInteractionExposure, RuntimeInteractionKind,
    RuntimeModule, RuntimeObservation, RuntimeObserver, RuntimePlanStepStatus, RuntimeProfile,
    RuntimeSession, RuntimeSessionOperation, RuntimeSessionRequest, RuntimeSessionResult,
    RuntimeTurnOutcome, RuntimeTurnRequest, RuntimeUsageUpdate,
};
use sha2::{Digest, Sha256};

fn gateway_runtime_snapshot_cache_key(query: &SnapshotQuery) -> GatewayRuntimeSnapshotCacheKey {
    GatewayRuntimeSnapshotCacheKey {
        runtime_ref: query.profile.id.clone(),
        profile_fingerprint: query.profile.fingerprint.clone(),
        scope: match &query.scope {
            SnapshotScope::Profile => GatewayRuntimeSnapshotScopeKey::Profile,
            SnapshotScope::Workspace { cwd } => {
                GatewayRuntimeSnapshotScopeKey::Workspace { cwd: cwd.clone() }
            }
            SnapshotScope::Session {
                cwd,
                thread_id,
                native_session_id,
            } => GatewayRuntimeSnapshotScopeKey::Session {
                cwd: cwd.clone(),
                thread_id: thread_id.clone(),
                native_session_id: native_session_id.clone(),
            },
        },
    }
}

fn resolve_gateway_runtime_profile(
    options: &RunOptions,
) -> psychevo_runtime::Result<(RuntimeProfileConfig, u64, String)> {
    if let Some(thread_id) = options.session.as_deref()
        && let Some(bound) = resolve_bound_gateway_runtime_profile(
            &options.state,
            thread_id,
            options.runtime_ref.as_deref(),
        )?
    {
        return Ok((bound.profile, bound.revision, bound.fingerprint));
    }
    let runtime_ref = options
        .runtime_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("native");
    let env = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| std::env::vars().collect());
    let home = resolve_skills_home(&env, &options.cwd)?;
    let mut profiles = load_runtime_profile_configs(&home, &options.cwd, &env)?;
    for profile in generated_gateway_runtime_profiles() {
        profiles.entry(profile.id.clone()).or_insert(profile);
    }
    if let Some(backend_id) = runtime_ref.strip_prefix("acp:") {
        let referenced = profiles
            .values()
            .filter_map(|profile| profile.backend_ref.as_deref())
            .any(|backend_ref| backend_ref == backend_id);
        if !referenced {
            let backends = load_agent_backend_configs(&home, &options.cwd, &env)?;
            if let Some(backend) = backends.get(backend_id)
                && backend.enabled
            {
                profiles.insert(
                    runtime_ref.to_string(),
                    RuntimeProfileConfig {
                        id: runtime_ref.to_string(),
                        runtime: RuntimeProfileKind::Acp,
                        enabled: true,
                        label: format!("{} (ACP)", backend.label.trim_end_matches("(ACP)").trim()),
                        backend_ref: Some(backend.id.clone()),
                        command: backend.command.clone(),
                        args: backend.args.clone(),
                        env: backend.env.clone(),
                        default_model: None,
                        default_mode: None,
                        default_agent: None,
                        approval_mode: None,
                        sandbox: None,
                        workspace_roots: Vec::new(),
                        options: Value::Null,
                    },
                );
            }
        }
    }
    let mut profile = profiles.remove(runtime_ref).ok_or_else(|| {
        runtime_host_configuration_error(format!("unknown Runtime Profile: {runtime_ref}"))
    })?;
    if !profile.enabled {
        return Err(runtime_host_configuration_error(format!(
            "Runtime Profile `{runtime_ref}` is disabled."
        )));
    }
    if profile.runtime == RuntimeProfileKind::Acp {
        let backend_ref = profile.backend_ref.as_deref().ok_or_else(|| {
            runtime_host_configuration_error(format!(
                "ACP Runtime Profile `{runtime_ref}` is missing backendRef."
            ))
        })?;
        let backends = load_agent_backend_configs(&home, &options.cwd, &env)?;
        let backend = backends.get(backend_ref).ok_or_else(|| {
            runtime_host_configuration_error(format!(
                "ACP Runtime Profile `{runtime_ref}` references unknown backend `{backend_ref}`."
            ))
        })?;
        if profile.command.is_none() {
            profile.command.clone_from(&backend.command);
        }
        if profile.args.is_empty() {
            profile.args.clone_from(&backend.args);
        }
        for (key, value) in &backend.env {
            profile
                .env
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }
    let fingerprint = runtime_profile_config_fingerprint(&profile);
    let revision = runtime_profile_config_revision(&fingerprint);
    Ok((profile, revision, fingerprint))
}

struct BoundGatewayRuntimeProfile {
    profile: RuntimeProfileConfig,
    revision: u64,
    fingerprint: String,
    binding: GatewayRuntimeBindingRecord,
}

fn resolve_bound_gateway_runtime_profile(
    state: &StateRuntime,
    thread_id: &str,
    requested_runtime_ref: Option<&str>,
) -> psychevo_runtime::Result<Option<BoundGatewayRuntimeProfile>> {
    let Some(binding) = state.store().gateway_runtime_binding(thread_id)? else {
        return Ok(None);
    };
    if binding.status != GatewayRuntimeBindingStatus::Resolved {
        return Err(runtime_host_error(RuntimeError::new(
            "unresolved_binding",
            RuntimeErrorStage::Binding,
            RetryClass::UserAction,
            format!(
                "Thread `{thread_id}` has unresolved runtime identity: {}.",
                binding
                    .unresolved_reason
                    .as_deref()
                    .unwrap_or("an immutable Profile snapshot is required")
            ),
        )));
    }
    if let Some(requested) = requested_runtime_ref
        .map(str::trim)
        .filter(|value| !value.is_empty())
        && binding.runtime_ref.as_deref() != Some(requested)
    {
        return Err(runtime_host_error(RuntimeError::new(
            "immutable_binding",
            RuntimeErrorStage::Binding,
            RetryClass::UserAction,
            format!(
                "Thread `{thread_id}` is bound to Runtime Profile `{}`; start a new thread to use `{requested}`.",
                binding.runtime_ref.as_deref().unwrap_or("unresolved"),
            ),
        )));
    }
    let snapshot = binding.profile_config_json.as_deref().ok_or_else(|| {
        runtime_host_error(RuntimeError::new(
            "bound_profile_snapshot_missing",
            RuntimeErrorStage::Binding,
            RetryClass::Never,
            format!(
                "Thread `{thread_id}` is missing its immutable effective Runtime Profile snapshot; it cannot safely continue."
            ),
        ))
    })?;
    let profile: RuntimeProfileConfig = serde_json::from_str(snapshot).map_err(|error| {
        runtime_host_error(RuntimeError::new(
            "bound_profile_snapshot_invalid",
            RuntimeErrorStage::Binding,
            RetryClass::Never,
            format!("Bound Runtime Profile snapshot could not be decoded: {error}"),
        ))
    })?;
    let fingerprint = runtime_profile_config_fingerprint(&profile);
    if binding.profile_fingerprint.as_deref() != Some(fingerprint.as_str()) {
        return Err(runtime_host_error(RuntimeError::new(
            "bound_profile_snapshot_mismatch",
            RuntimeErrorStage::Binding,
            RetryClass::Never,
            "Bound Runtime Profile snapshot does not match its immutable fingerprint",
        )));
    }
    let revision = binding
        .profile_revision
        .as_deref()
        .and_then(|revision| revision.parse::<u64>().ok())
        .unwrap_or_else(|| runtime_profile_config_revision(&fingerprint));
    Ok(Some(BoundGatewayRuntimeProfile {
        profile,
        revision,
        fingerprint,
        binding,
    }))
}

fn generated_gateway_runtime_profiles() -> Vec<RuntimeProfileConfig> {
    vec![
        RuntimeProfileConfig {
            id: "native".to_string(),
            runtime: RuntimeProfileKind::Native,
            enabled: true,
            label: "Native".to_string(),
            backend_ref: None,
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
            backend_ref: None,
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
            backend_ref: None,
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

fn runtime_profile_config_fingerprint(profile: &RuntimeProfileConfig) -> String {
    let encoded = serde_json::to_vec(&json!({
        "id": profile.id,
        "runtime": profile.runtime.as_str(),
        "enabled": profile.enabled,
        "label": profile.label,
        "backendRef": profile.backend_ref,
        "command": profile.command,
        "args": profile.args,
        "env": profile.env,
        "defaultModel": profile.default_model,
        "defaultMode": profile.default_mode,
        "defaultAgent": profile.default_agent,
        "approvalMode": profile.approval_mode,
        "sandbox": profile.sandbox,
        "workspaceRoots": profile.workspace_roots,
        "options": profile.options,
    }))
    .expect("runtime profile fingerprint serializes");
    format!("{:x}", Sha256::digest(encoded))
}

fn hex_prefix_bytes(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .take(8)
        .map(|pair| {
            let text = std::str::from_utf8(pair).expect("fingerprint is ASCII");
            u8::from_str_radix(text, 16).expect("fingerprint is hexadecimal")
        })
        .collect()
}

fn runtime_profile_config_revision(fingerprint: &str) -> u64 {
    u64::from_be_bytes(
        hex_prefix_bytes(fingerprint)
            .try_into()
            .expect("runtime fingerprint prefix has eight bytes"),
    )
}

fn runtime_kind_from_profile(kind: RuntimeProfileKind) -> RuntimeKind {
    match kind {
        RuntimeProfileKind::Native => RuntimeKind::Native,
        RuntimeProfileKind::Acp => RuntimeKind::Acp,
        RuntimeProfileKind::Codex => RuntimeKind::Codex,
        RuntimeProfileKind::OpenCode => RuntimeKind::OpenCode,
    }
}

fn gateway_runtime_profile(
    config: RuntimeProfileConfig,
    revision: u64,
    fingerprint: String,
) -> RuntimeProfile {
    RuntimeProfile {
        id: config.id,
        label: config.label,
        kind: runtime_kind_from_profile(config.runtime),
        enabled: config.enabled,
        command: config.command,
        args: config.args,
        env: config.env,
        backend_ref: config.backend_ref,
        default_model: config.default_model,
        default_mode: config.default_mode,
        default_agent: config.default_agent,
        approval_mode: config.approval_mode,
        sandbox: config.sandbox,
        workspace_roots: config
            .workspace_roots
            .into_iter()
            .map(PathBuf::from)
            .collect(),
        options: config.options,
        revision,
        fingerprint,
    }
}

fn ensure_gateway_runtime_binding(
    state: &StateRuntime,
    thread_id: &str,
    profile: &RuntimeProfileConfig,
    revision: u64,
    fingerprint: &str,
) -> psychevo_runtime::Result<GatewayRuntimeBindingRecord> {
    if let Some(existing) = state.store().gateway_runtime_binding(thread_id)? {
        if existing.status != GatewayRuntimeBindingStatus::Resolved {
            return Err(runtime_host_configuration_error(
                existing.unresolved_reason.unwrap_or_else(|| {
                    "This thread has an unresolved runtime binding.".to_string()
                }),
            ));
        }
        if existing.runtime_ref.as_deref() != Some(profile.id.as_str()) {
            return Err(runtime_host_error(RuntimeError::new(
                "immutable_binding",
                RuntimeErrorStage::Binding,
                RetryClass::UserAction,
                format!(
                    "Thread `{thread_id}` is bound to Runtime Profile `{}`; start a new thread to use `{}`.",
                    existing.runtime_ref.as_deref().unwrap_or("unresolved"),
                    profile.id,
                ),
            )));
        }
        if existing.profile_fingerprint.as_deref() != Some(fingerprint) {
            return Err(runtime_host_error(RuntimeError::new(
                "stale_revision",
                RuntimeErrorStage::Binding,
                RetryClass::UserAction,
                format!(
                    "Runtime Profile `{}` changed after this thread was bound; start a new thread.",
                    profile.id
                ),
            )));
        }
        if existing.ownership == GatewayRuntimeBindingOwnership::ReadOnly {
            return Err(runtime_host_error(RuntimeError::new(
                "read_only_session",
                RuntimeErrorStage::Binding,
                RetryClass::UserAction,
                format!(
                    "Thread `{thread_id}` is a read-only runtime-native child; open its history from the parent thread."
                ),
            )));
        }
        return Ok(existing);
    }
    let summary = state
        .store()
        .session_summary(thread_id)?
        .ok_or_else(|| Error::Message(format!("session not found: {thread_id}")))?;
    let backend_kind = match profile.runtime {
        RuntimeProfileKind::Native => "psychevo",
        RuntimeProfileKind::Acp => "peer_agent",
        RuntimeProfileKind::Codex | RuntimeProfileKind::OpenCode => "runtime",
    };
    let native_session_id = (profile.runtime == RuntimeProfileKind::Native).then_some(thread_id);
    let profile_config_json = serde_json::to_string(profile)?;
    state
        .store()
        .create_gateway_runtime_binding(GatewayRuntimeBindingInput {
            thread_id,
            runtime_ref: &profile.id,
            backend_kind,
            native_kind: profile.runtime.as_str(),
            native_session_id,
            cwd: &summary.cwd,
            profile_fingerprint: fingerprint,
            profile_revision: &revision.to_string(),
            profile_config_json: &profile_config_json,
            adapter_kind: profile.runtime.as_str(),
            adapter_revision: env!("CARGO_PKG_VERSION"),
            ownership: GatewayRuntimeBindingOwnership::ReadWrite,
            parent_thread_id: summary.parent_session_id.as_deref(),
        })
}

fn runtime_host_error(error: RuntimeError) -> Error {
    Error::structured(
        error.message.clone(),
        json!({
            "code": error.code,
            "stage": runtime_error_stage_name(error.stage),
            "retryClass": runtime_retry_class_name(error.retry_class),
            "message": error.message,
            "diagnosticRef": error.diagnostic_ref,
        }),
    )
}

fn runtime_error_stage_name(stage: RuntimeErrorStage) -> &'static str {
    match stage {
        RuntimeErrorStage::Configuration => "configuration",
        RuntimeErrorStage::Discovery => "discovery",
        RuntimeErrorStage::Launch => "launch",
        RuntimeErrorStage::Transport => "transport",
        RuntimeErrorStage::Handshake => "handshake",
        RuntimeErrorStage::Authentication => "authentication",
        RuntimeErrorStage::Hydration => "hydration",
        RuntimeErrorStage::Binding => "binding",
        RuntimeErrorStage::Prompt => "prompt",
        RuntimeErrorStage::Interaction => "interaction",
        RuntimeErrorStage::Control => "control",
        RuntimeErrorStage::History => "history",
        RuntimeErrorStage::Shutdown => "shutdown",
    }
}

fn runtime_retry_class_name(retry_class: RetryClass) -> &'static str {
    match retry_class {
        RetryClass::Never => "never",
        RetryClass::UserAction => "user_action",
        RetryClass::SafeRetry => "safe_retry",
        RetryClass::Reconnect => "reconnect",
        RetryClass::UnknownDelivery => "unknown_delivery",
    }
}

fn direct_runtime_terminal_error(
    outcome: RuntimeTurnOutcome,
    terminal_error: Option<psychevo_runtime_host::RuntimeTerminalError>,
    runtime_kind: &str,
    process_epoch: u64,
) -> Option<psychevo_runtime::RunTerminalError> {
    if outcome != RuntimeTurnOutcome::Failed {
        return None;
    }
    Some(match terminal_error {
        Some(error) => psychevo_runtime::RunTerminalError {
            code: error.code,
            stage: runtime_error_stage_name(error.stage).to_string(),
            retry_class: runtime_retry_class_name(error.retry_class).to_string(),
            message: error.message,
            diagnostic_ref: error.diagnostic_ref,
        },
        None => psychevo_runtime::RunTerminalError {
            code: "adapter_contract_violation".to_string(),
            stage: "prompt".to_string(),
            retry_class: "never".to_string(),
            message: "The direct runtime failed without a classified terminal error."
                .to_string(),
            diagnostic_ref: format!(
                "{runtime_kind}-process-{process_epoch}-terminal-contract"
            ),
        },
    })
}

fn runtime_host_configuration_error(message: impl Into<String>) -> Error {
    runtime_host_error(RuntimeError::new(
        "configuration",
        RuntimeErrorStage::Configuration,
        RetryClass::UserAction,
        message,
    ))
}

fn runtime_history_message_to_session_message(
    message: &RuntimeHistoryMessage,
) -> Option<psychevo_agent_core::Message> {
    match message.role.as_str() {
        "user" => Some(psychevo_agent_core::user_text_message(&message.text)),
        "assistant" => Some(psychevo_agent_core::Message::Assistant {
            content: vec![psychevo_agent_core::AssistantBlock::Text {
                text: message.text.clone(),
            }],
            timestamp_ms: message.created_at_ms.unwrap_or_else(gateway_now_ms),
            finish_reason: Some("stop".to_string()),
            outcome: Outcome::Normal,
            model: None,
            provider: None,
        }),
        _ => None,
    }
}

fn import_runtime_session_history(
    state: &StateRuntime,
    thread_id: &str,
    runtime_ref: &str,
    session: &RuntimeSession,
) -> psychevo_runtime::Result<usize> {
    let mut imported = state
        .store()
        .load_sanitized_message_summaries(thread_id)?
        .into_iter()
        .filter_map(|summary| {
            summary
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("runtimeHistoryDedupKey"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect::<HashSet<_>>();
    let fidelity = runtime_session_fidelity(session);
    let mut appended = 0;
    for (index, history) in session.messages.iter().enumerate() {
        let Some(message) = runtime_history_message_to_session_message(history) else {
            continue;
        };
        let native_key = if history.dedup_key.trim().is_empty() {
            format!(
                "{}:{index}:{}:{}",
                session.native_dedup_key, history.role, history.text
            )
        } else {
            format!("{}:{}", session.native_dedup_key, history.dedup_key)
        };
        let dedup_key = runtime_public_dedup_key(runtime_ref, &native_key);
        if !imported.insert(dedup_key.clone()) {
            continue;
        }
        state.store().append_message_with_metrics(
            thread_id,
            &message,
            None,
            Some(json!({
                "runtimeRef": runtime_ref,
                "runtimeHistoryDedupKey": dedup_key,
                "historyFidelity": fidelity,
            })),
        )?;
        appended += 1;
    }
    Ok(appended)
}

fn runtime_session_fidelity(session: &RuntimeSession) -> &'static str {
    match session.fidelity {
        HistoryFidelity::Full => "full",
        HistoryFidelity::Summary => "summary",
        HistoryFidelity::Partial => "partial",
    }
}

async fn execute_gateway_runtime_session(
    gateway: &Gateway,
    mut options: RunOptions,
    runtime_ref: &str,
    operation: RuntimeSessionOperation,
    native_session_id: Option<String>,
    cursor: Option<String>,
    argument: Option<Value>,
) -> psychevo_runtime::Result<(RuntimeProfileConfig, RuntimeSessionResult)> {
    options.runtime_ref = Some(runtime_ref.to_string());
    let (profile_config, profile_revision, profile_fingerprint) =
        resolve_gateway_runtime_profile(&options)?;
    if !matches!(
        profile_config.runtime,
        RuntimeProfileKind::Codex | RuntimeProfileKind::OpenCode
    ) {
        return Err(runtime_host_configuration_error(format!(
            "Runtime Profile `{runtime_ref}` is not a direct runtime."
        )));
    }
    let binding_revision = options
        .session
        .as_deref()
        .map(|thread_id| gateway.state.store().gateway_runtime_binding(thread_id))
        .transpose()?
        .flatten()
        .map(|binding| u64::try_from(binding.binding_revision).unwrap_or_default());
    let profile = gateway_runtime_profile(
        profile_config.clone(),
        profile_revision,
        profile_fingerprint,
    );
    let result = gateway
        .runtime_host
        .execute(
            ExecuteRequest {
                profile,
                expected_profile_revision: profile_revision,
                expected_capability_revision: None,
                expected_binding_revision: binding_revision,
                intent: RuntimeIntent::Session(RuntimeSessionRequest {
                    operation,
                    thread_id: options.session.clone(),
                    native_session_id,
                    cwd: options.cwd,
                    cursor,
                    argument,
                }),
            },
            RuntimeObserver::default(),
            HostRuntimeControl::default(),
        )
        .await
        .map_err(runtime_host_error)?;
    let ExecuteResult::Session(result) = result else {
        return Err(runtime_host_configuration_error(
            "runtime adapter returned a non-session result for session execution",
        ));
    };
    Ok((profile_config, result))
}

async fn execute_gateway_runtime_control(
    gateway: &Gateway,
    mut options: RunOptions,
    runtime_ref: &str,
    control_id: String,
    value: Value,
    expected_capability_revision: u64,
    expected_binding_revision: u64,
) -> psychevo_runtime::Result<RuntimeControlSetResult> {
    options.runtime_ref = Some(runtime_ref.to_string());
    let thread_id = options.session.clone().ok_or_else(|| {
        runtime_host_configuration_error("Runtime control mutation requires a bound public thread.")
    })?;
    let (profile_config, profile_revision, profile_fingerprint) =
        resolve_gateway_runtime_profile(&options)?;
    if !matches!(
        profile_config.runtime,
        RuntimeProfileKind::Codex | RuntimeProfileKind::OpenCode
    ) {
        return Err(runtime_host_configuration_error(format!(
            "Runtime Profile `{runtime_ref}` does not expose direct observed control mutation."
        )));
    }
    let binding = gateway
        .state
        .store()
        .gateway_runtime_binding(&thread_id)?
        .ok_or_else(|| runtime_host_configuration_error("Runtime binding is missing."))?;
    let binding_revision = u64::try_from(binding.binding_revision).unwrap_or_default();
    if binding_revision != expected_binding_revision {
        return Err(runtime_host_error(RuntimeError::new(
            "stale_revision",
            RuntimeErrorStage::Control,
            RetryClass::SafeRetry,
            format!(
                "Runtime binding changed: expected {expected_binding_revision}, current {binding_revision}."
            ),
        )));
    }
    let native_session_id = binding.native_session_id.ok_or_else(|| {
        runtime_host_error(RuntimeError::new(
            "runtime_session_unbound",
            RuntimeErrorStage::Binding,
            RetryClass::UserAction,
            "The runtime has not attached a native session yet.",
        ))
    })?;
    let profile = gateway_runtime_profile(profile_config, profile_revision, profile_fingerprint);
    let result = gateway
        .runtime_host
        .execute(
            ExecuteRequest {
                profile,
                expected_profile_revision: profile_revision,
                expected_capability_revision: Some(expected_capability_revision),
                expected_binding_revision: Some(expected_binding_revision),
                intent: RuntimeIntent::Control(RuntimeControlSetRequest {
                    thread_id,
                    native_session_id,
                    cwd: options.cwd,
                    control_id,
                    value,
                }),
            },
            RuntimeObserver::default(),
            HostRuntimeControl::default(),
        )
        .await
        .map_err(runtime_host_error)?;
    let ExecuteResult::Control(result) = result else {
        return Err(runtime_host_configuration_error(
            "runtime adapter returned a non-control result for control execution",
        ));
    };
    Ok(result)
}

async fn execute_gateway_runtime_auth(
    gateway: &Gateway,
    mut options: RunOptions,
    runtime_ref: &str,
    operation: RuntimeAuthOperation,
) -> psychevo_runtime::Result<RuntimeAuthResult> {
    options.runtime_ref = Some(runtime_ref.to_string());
    let (profile_config, profile_revision, profile_fingerprint) =
        resolve_gateway_runtime_profile(&options)?;
    if !matches!(
        profile_config.runtime,
        RuntimeProfileKind::Codex | RuntimeProfileKind::OpenCode
    ) {
        return Err(runtime_host_configuration_error(format!(
            "Runtime Profile `{runtime_ref}` does not expose managed authentication."
        )));
    }
    let profile = gateway_runtime_profile(profile_config, profile_revision, profile_fingerprint);
    let result = match gateway
        .runtime_host
        .execute(
            ExecuteRequest {
                profile,
                expected_profile_revision: profile_revision,
                expected_capability_revision: None,
                expected_binding_revision: None,
                intent: RuntimeIntent::Auth(RuntimeAuthRequest {
                    operation,
                    cwd: options.cwd,
                }),
            },
            RuntimeObserver::default(),
            HostRuntimeControl::default(),
        )
        .await
    {
        Ok(result) => result,
        Err(error) if error.code == "opencode_auth_cli_required" => {
            return Ok(RuntimeAuthResult {
                accepted: false,
                status: "cli_required".to_string(),
                message: error.message,
                output: Value::Null,
            });
        }
        Err(error) => return Err(runtime_host_error(error)),
    };
    let ExecuteResult::Auth(result) = result else {
        return Err(runtime_host_configuration_error(
            "runtime adapter returned a non-auth result for auth execution",
        ));
    };
    Ok(result)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GatewayGoalTokenBudgetUpdate {
    Unchanged,
    Clear,
    Set(i64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GatewayCodexExtension {
    GoalRead {
        thread_id: String,
        native_session_id: String,
        cwd: PathBuf,
    },
    GoalSet {
        thread_id: String,
        native_session_id: String,
        cwd: PathBuf,
        objective: Option<String>,
        status: Option<RuntimeGoalStatus>,
        token_budget: GatewayGoalTokenBudgetUpdate,
    },
    GoalClear {
        thread_id: String,
        native_session_id: String,
        cwd: PathBuf,
    },
    AccountRateLimitsRead {
        cwd: PathBuf,
    },
}

impl GatewayCodexExtension {
    fn into_runtime_request(self) -> RuntimeExtensionRequest {
        match self {
            Self::GoalRead {
                thread_id,
                native_session_id,
                cwd,
            } => RuntimeExtensionRequest {
                namespace: "codex.goal".to_string(),
                operation: "read".to_string(),
                argument: Some(json!({
                    "threadId": thread_id,
                    "nativeSessionId": native_session_id,
                    "cwd": cwd,
                })),
            },
            Self::GoalSet {
                thread_id,
                native_session_id,
                cwd,
                objective,
                status,
                token_budget,
            } => {
                let mut argument = serde_json::Map::from_iter([
                    ("threadId".to_string(), Value::String(thread_id)),
                    (
                        "nativeSessionId".to_string(),
                        Value::String(native_session_id),
                    ),
                    ("cwd".to_string(), json!(cwd)),
                ]);
                if let Some(objective) = objective {
                    argument.insert("objective".to_string(), Value::String(objective));
                }
                if let Some(status) = status {
                    argument.insert("status".to_string(), json!(status));
                }
                match token_budget {
                    GatewayGoalTokenBudgetUpdate::Unchanged => {}
                    GatewayGoalTokenBudgetUpdate::Clear => {
                        argument.insert("tokenBudget".to_string(), Value::Null);
                    }
                    GatewayGoalTokenBudgetUpdate::Set(token_budget) => {
                        argument.insert("tokenBudget".to_string(), Value::from(token_budget));
                    }
                }
                RuntimeExtensionRequest {
                    namespace: "codex.goal".to_string(),
                    operation: "set".to_string(),
                    argument: Some(Value::Object(argument)),
                }
            }
            Self::GoalClear {
                thread_id,
                native_session_id,
                cwd,
            } => RuntimeExtensionRequest {
                namespace: "codex.goal".to_string(),
                operation: "clear".to_string(),
                argument: Some(json!({
                    "threadId": thread_id,
                    "nativeSessionId": native_session_id,
                    "cwd": cwd,
                })),
            },
            Self::AccountRateLimitsRead { cwd } => RuntimeExtensionRequest {
                namespace: "codex.account".to_string(),
                operation: "rateLimits/read".to_string(),
                argument: Some(json!({ "cwd": cwd })),
            },
        }
    }
}

async fn execute_gateway_codex_extension(
    gateway: &Gateway,
    mut options: RunOptions,
    runtime_ref: &str,
    expected_binding_revision: Option<u64>,
    extension: GatewayCodexExtension,
) -> psychevo_runtime::Result<Value> {
    options.runtime_ref = Some(runtime_ref.to_string());
    if let Some(expected_binding_revision) = expected_binding_revision {
        let thread_id = options.session.as_deref().ok_or_else(|| {
            runtime_host_configuration_error(
                "A binding revision was supplied without a bound public thread.",
            )
        })?;
        let binding = gateway
            .state
            .store()
            .gateway_runtime_binding(thread_id)?
            .ok_or_else(|| runtime_host_configuration_error("Runtime binding is missing."))?;
        let current_binding_revision = u64::try_from(binding.binding_revision).unwrap_or_default();
        if current_binding_revision != expected_binding_revision {
            return Err(runtime_host_error(RuntimeError::new(
                "stale_revision",
                RuntimeErrorStage::Binding,
                RetryClass::SafeRetry,
                format!(
                    "Runtime binding changed: expected {expected_binding_revision}, current {current_binding_revision}."
                ),
            )));
        }
    }
    let (profile_config, profile_revision, profile_fingerprint) =
        resolve_gateway_runtime_profile(&options)?;
    if profile_config.runtime != RuntimeProfileKind::Codex {
        return Err(runtime_host_error(RuntimeError::new(
            "codex_extension_unsupported",
            RuntimeErrorStage::Configuration,
            RetryClass::UserAction,
            format!(
                "Runtime Profile `{runtime_ref}` does not expose stable Codex goal or account metadata operations."
            ),
        )));
    }
    let profile = gateway_runtime_profile(profile_config, profile_revision, profile_fingerprint);
    let result = gateway
        .runtime_host
        .execute(
            ExecuteRequest {
                profile,
                expected_profile_revision: profile_revision,
                expected_capability_revision: None,
                expected_binding_revision,
                intent: RuntimeIntent::Extension(extension.into_runtime_request()),
            },
            RuntimeObserver::default(),
            HostRuntimeControl::default(),
        )
        .await
        .map_err(runtime_host_error)?;
    let ExecuteResult::Extension(result) = result else {
        return Err(runtime_host_configuration_error(
            "runtime adapter returned a non-extension result for a typed Codex operation",
        ));
    };
    Ok(result)
}

async fn execute_gateway_runtime_compaction(
    gateway: &Gateway,
    bound: BoundGatewayRuntimeProfile,
    thread_id: &str,
    cwd: PathBuf,
    reason: psychevo_runtime::CompactionReason,
    instructions: Option<String>,
) -> psychevo_runtime::Result<psychevo_runtime::CompactionResult> {
    if instructions
        .as_deref()
        .is_some_and(|instructions| !instructions.trim().is_empty())
    {
        return Err(runtime_host_error(RuntimeError::new(
            "codex_compaction_instructions_unsupported",
            RuntimeErrorStage::Control,
            RetryClass::UserAction,
            "Codex native compaction does not accept custom instructions; remove them and retry.",
        )));
    }
    if bound.profile.runtime != RuntimeProfileKind::Codex {
        return Err(runtime_host_configuration_error(format!(
            "Runtime Profile `{}` does not own stable native compaction.",
            bound.profile.id
        )));
    }
    let native_session_id = bound.binding.native_session_id.clone().ok_or_else(|| {
        runtime_host_error(RuntimeError::new(
            "runtime_native_session_missing",
            RuntimeErrorStage::Binding,
            RetryClass::UserAction,
            "Codex native compaction requires an attached native session.",
        ))
    })?;
    let expected_binding_revision =
        u64::try_from(bound.binding.binding_revision).unwrap_or_default();
    let runtime_ref = bound.profile.id.clone();
    let profile = gateway_runtime_profile(bound.profile, bound.revision, bound.fingerprint);
    let result = gateway
        .runtime_host
        .execute(
            ExecuteRequest {
                profile,
                expected_profile_revision: bound.revision,
                expected_capability_revision: None,
                expected_binding_revision: Some(expected_binding_revision),
                intent: RuntimeIntent::Compaction(RuntimeCompactionRequest {
                    thread_id: thread_id.to_string(),
                    native_session_id: native_session_id.clone(),
                    cwd,
                    instructions: None,
                }),
            },
            RuntimeObserver::default(),
            HostRuntimeControl::default(),
        )
        .await
        .map_err(runtime_host_error)?;
    let ExecuteResult::Compaction(result) = result else {
        return Err(runtime_host_configuration_error(
            "runtime adapter returned a non-compaction result for compaction execution",
        ));
    };
    if result.thread_id != thread_id
        || result.native_session_id != native_session_id
        || !result.compacted
    {
        return Err(runtime_host_error(RuntimeError::new(
            "runtime_compaction_result_mismatch",
            RuntimeErrorStage::Binding,
            RetryClass::Never,
            "Codex returned compaction completion for different native provenance.",
        )));
    }

    let records = gateway.state.store().load_message_records(thread_id)?;
    let created_after_session_seq = records
        .last()
        .map(|record| record.session_seq)
        .unwrap_or_default();
    let first_kept_session_seq = created_after_session_seq.saturating_add(1);
    let tokens_before = gateway
        .state
        .store()
        .load_sanitized_message_summaries(thread_id)?
        .into_iter()
        .rev()
        .filter_map(|message| message.usage)
        .find_map(|usage| {
            usage
                .get("input_tokens")
                .and_then(Value::as_u64)
                .or_else(|| usage.get("context_input_tokens").and_then(Value::as_u64))
        });
    let summary = "Codex compacted its native conversation context.".to_string();
    let checkpoint = gateway.state.store().append_session_compaction(
        psychevo_runtime::SessionCompactionInput {
            session_id: thread_id.to_string(),
            reason: reason.as_str().to_string(),
            summary_text: summary.clone(),
            first_kept_session_seq,
            created_after_session_seq,
            tokens_before,
            tokens_after: None,
            summary_provider: "codex".to_string(),
            summary_model: "runtime-owned".to_string(),
            instructions: None,
            metadata: Some(json!({
                "projection_only": true,
                "runtimeRef": runtime_ref,
                "nativeOwned": true,
            })),
        },
    )?;
    Ok(psychevo_runtime::CompactionResult {
        session_id: thread_id.to_string(),
        compacted: true,
        reason: reason.as_str().to_string(),
        message: summary.clone(),
        checkpoint_id: Some(checkpoint.id),
        first_kept_session_seq: Some(first_kept_session_seq),
        tokens_before,
        tokens_after: None,
        summary: Some(summary),
        summary_provider: Some("codex".to_string()),
        summary_model: Some("runtime-owned".to_string()),
    })
}

struct DirectRuntimeTurn {
    run: RunResult,
    native_session_id: String,
}

struct DirectRuntimeTurnInput {
    profile_config: RuntimeProfileConfig,
    profile_revision: u64,
    profile_fingerprint: String,
    binding: GatewayRuntimeBindingRecord,
    request: BackendTurnRequest,
    turn_id: String,
    event_sink: Option<GatewayEventSink>,
    instructions: Option<String>,
}

async fn run_direct_runtime_turn(
    gateway: &Gateway,
    input: DirectRuntimeTurnInput,
) -> psychevo_runtime::Result<DirectRuntimeTurn> {
    let DirectRuntimeTurnInput {
        profile_config,
        profile_revision,
        profile_fingerprint,
        binding,
        request,
        turn_id,
        event_sink,
        instructions,
    } = input;
    let thread_id = binding.thread_id.clone();
    let runtime_ref = profile_config.id.clone();
    let runtime_kind = profile_config.runtime.as_str().to_string();
    let prompt = request.options.prompt.clone();
    let cwd = request.options.cwd.clone();
    let stream = request.stream.clone();
    // No current product caller supplies a real GUI Advanced turn context.
    // Keep this policy derived inside Gateway rather than accepting a
    // self-asserted runtime option from the client.
    let interaction_exposure = RuntimeInteractionExposure::Standard;
    let profile = gateway_runtime_profile(
        profile_config.clone(),
        profile_revision,
        profile_fingerprint,
    );
    let transcript_text = Arc::new(Mutex::new(String::new()));
    let reasoning_text = Arc::new(Mutex::new(String::new()));
    let runtime_usage = Arc::new(Mutex::new(None::<RuntimeUsageUpdate>));
    let runtime_plan = Arc::new(Mutex::new(
        None::<psychevo_runtime_host::RuntimePlanUpdate>,
    ));
    let runtime_diff = Arc::new(Mutex::new(
        None::<psychevo_runtime_host::RuntimeDiffUpdate>,
    ));
    let interaction_dispatch_handle = tokio::runtime::Handle::current();
    let observer = RuntimeObserver::new({
        let runtime_ref = runtime_ref.clone();
        let turn_id = turn_id.clone();
        let thread_id = thread_id.clone();
        let stream = stream.clone();
        let event_sink = event_sink.clone();
        let transcript_text = Arc::clone(&transcript_text);
        let reasoning_text = Arc::clone(&reasoning_text);
        let runtime_usage = Arc::clone(&runtime_usage);
        let runtime_plan = Arc::clone(&runtime_plan);
        let runtime_diff = Arc::clone(&runtime_diff);
        let pending_runtime_interactions = Arc::clone(&gateway.pending_runtime_interactions);
        let interaction_gateway = gateway.clone();
        let interaction_dispatch_handle = interaction_dispatch_handle.clone();
        let state = gateway.state.clone();
        let profile = profile.clone();
        move |observation| match observation {
            RuntimeObservation::TextDelta { text, .. } => {
                let cumulative = {
                    let mut current = transcript_text
                        .lock()
                        .expect("direct runtime transcript accumulator poisoned");
                    current.push_str(&text);
                    current.clone()
                };
                if let Some(stream) = &stream {
                    stream(RunStreamEvent::value(json!({
                        "type": "message_update",
                        "message": direct_runtime_assistant_message(
                            &cumulative,
                            "streaming",
                            &runtime_ref,
                        ),
                    })));
                }
            }
            RuntimeObservation::ReasoningDelta { text, .. } => {
                let cumulative = {
                    let mut current = reasoning_text
                        .lock()
                        .expect("direct runtime reasoning accumulator poisoned");
                    current.push_str(&text);
                    current.clone()
                };
                if let Some(stream) = &stream {
                    stream(RunStreamEvent::ReasoningDelta { text: cumulative });
                }
            }
            RuntimeObservation::Tool {
                item_id,
                name,
                status,
                detail,
                ..
            } => {
                if let Some(stream) = &stream {
                    let event_type = if status == "completed" || status == "failed" {
                        "tool_execution_end"
                    } else {
                        "tool_execution_start"
                    };
                    let public_item_id = runtime_public_dedup_key(
                        &runtime_ref,
                        &format!("tool:{turn_id}:{item_id}"),
                    );
                    let public_detail = public_runtime_tool_detail(detail.as_ref());
                    stream(RunStreamEvent::value(json!({
                        "type": event_type,
                        "tool_call_id": public_item_id,
                        "tool_name": name,
                        "status": status,
                        "metadata": {
                            "runtimeRef": runtime_ref,
                            "detail": public_detail,
                        },
                    })));
                }
            }
            RuntimeObservation::PlanUpdated(update) => {
                if update.runtime_ref != runtime_ref
                    || update.thread_id != thread_id
                    || update.turn_id != turn_id
                {
                    return;
                }
                *runtime_plan
                    .lock()
                    .expect("direct runtime plan accumulator poisoned") = Some(update.clone());
                if let Some(stream) = &stream {
                    stream(RunStreamEvent::value(json!({
                        "type": "runtime_plan",
                        "body": direct_runtime_plan_body(&update),
                        "plan": update,
                        "metadata": {
                            "runtimeRef": runtime_ref,
                        },
                    })));
                }
            }
            RuntimeObservation::DiffUpdated(update) => {
                if update.runtime_ref != runtime_ref
                    || update.thread_id != thread_id
                    || update.turn_id != turn_id
                {
                    return;
                }
                *runtime_diff
                    .lock()
                    .expect("direct runtime diff accumulator poisoned") = Some(update.clone());
                if let Some(stream) = &stream {
                    stream(RunStreamEvent::value(json!({
                        "type": "runtime_diff",
                        "diff": update.diff,
                        "metadata": {
                            "runtimeRef": runtime_ref,
                        },
                    })));
                }
            }
            RuntimeObservation::UsageUpdated(update) => {
                if update.runtime_ref == runtime_ref
                    && update.thread_id == thread_id
                    && update.turn_id == turn_id
                {
                    *runtime_usage
                        .lock()
                        .expect("direct runtime usage accumulator poisoned") = Some(update);
                }
            }
            RuntimeObservation::GoalChanged(update) => {
                if update.runtime_ref != runtime_ref || update.thread_id != thread_id {
                    return;
                }
                let value = update
                    .goal
                    .as_ref()
                    .and_then(|goal| serde_json::to_value(goal).ok());
                if let Err(error) =
                    state
                        .store()
                        .set_session_metadata_field(&thread_id, "runtimeGoal", value)
                    && let Some(event_sink) = &event_sink
                {
                    event_sink(GatewayEvent::Warning {
                        kind: "runtime_goal_persistence_failed".to_string(),
                        message: format!("Runtime goal state could not be persisted: {error}"),
                        source_path: None,
                        suggestion: None,
                    });
                }
            }
            RuntimeObservation::CompactionChanged(update) => {
                if update.runtime_ref != runtime_ref || update.thread_id != thread_id {
                    return;
                }
                if let Some(stream) = &stream {
                    stream(RunStreamEvent::value(json!({
                        "type": "runtime_compaction",
                        "status": update.status,
                        "metadata": {
                            "runtimeRef": runtime_ref,
                        },
                    })));
                }
            }
            RuntimeObservation::AccountRateLimitsUpdated(update) => {
                if update.runtime_ref != runtime_ref {
                    return;
                }
                let value = serde_json::to_value(&update.rate_limits).ok();
                if let Err(error) = state.store().set_session_metadata_field(
                    &thread_id,
                    "runtimeAccountRateLimits",
                    value,
                ) && let Some(event_sink) = &event_sink
                {
                    event_sink(GatewayEvent::Warning {
                        kind: "runtime_rate_limit_persistence_failed".to_string(),
                        message: format!(
                            "Runtime account rate-limit state could not be persisted: {error}"
                        ),
                        source_path: None,
                        suggestion: None,
                    });
                }
            }
            RuntimeObservation::StateChanged {
                process_epoch,
                instance_epoch,
                state,
                detail,
                ..
            } => {
                if let Some(event_sink) = &event_sink {
                    event_sink(GatewayEvent::RuntimeStateChanged {
                        runtime_ref: runtime_ref.clone(),
                        thread_id: Some(thread_id.clone()),
                        state,
                        detail,
                        process_epoch,
                        instance_epoch,
                    });
                }
            }
            RuntimeObservation::ChildChanged {
                parent_native_session_id,
                native_session_id,
                status,
                read_only,
                ..
            } => {
                let projected = resolve_runtime_native_parent_thread(
                    &state,
                    &profile,
                    &thread_id,
                    &parent_native_session_id,
                )
                .and_then(|parent_thread_id| {
                    project_runtime_native_child(
                        &state,
                        &profile,
                        &parent_thread_id,
                        &native_session_id,
                    )
                    .map(|child_thread_id| (parent_thread_id, child_thread_id))
                });
                let (parent_thread_id, child_thread_id) = match projected {
                    Ok((parent_thread_id, child_thread_id)) => {
                        if let Err(error) = state.store().set_session_metadata_field(
                            &child_thread_id,
                            "runtimeStatus",
                            Some(Value::String(status.clone())),
                        ) && let Some(event_sink) = &event_sink
                        {
                            event_sink(GatewayEvent::Warning {
                                kind: "runtime_child_status_persistence_failed".to_string(),
                                message: format!(
                                    "Runtime child status could not be persisted: {error}"
                                ),
                                source_path: None,
                                suggestion: None,
                            });
                        }
                        (parent_thread_id, Some(child_thread_id))
                    }
                    Err(error) => {
                        if let Some(event_sink) = &event_sink {
                            event_sink(GatewayEvent::Warning {
                                kind: "runtime_child_parent_unresolved".to_string(),
                                message: format!(
                                    "Runtime child topology could not be projected: {error}"
                                ),
                                source_path: None,
                                suggestion: None,
                            });
                        }
                        (thread_id.clone(), None)
                    }
                };
                if let Some(event_sink) = &event_sink {
                    event_sink(GatewayEvent::RuntimeChildChanged {
                        runtime_ref: runtime_ref.clone(),
                        parent_thread_id,
                        thread_id: child_thread_id,
                        native_dedup_key: runtime_native_dedup_key(
                            &runtime_ref,
                            &native_session_id,
                        ),
                        status,
                        read_only,
                    });
                }
            }
            RuntimeObservation::Interaction(interaction) => {
                let interaction = *interaction;
                let action_id = runtime_interaction_action_id(&runtime_ref, &interaction.id);
                let action_kind = runtime_interaction_action_kind(&interaction);
                let pending = PendingRuntimeInteraction {
                    interaction: interaction.clone(),
                    profile: profile.clone(),
                    event_sink: event_sink.clone(),
                };
                if !interaction_exposure.allows(interaction.policy.exposure) {
                    let reason = "This runtime interaction requires GUI Advanced mode and was declined on this surface.".to_string();
                    if let Some(event_sink) = &event_sink {
                        event_sink(GatewayEvent::ActionCancelled {
                            action_id: action_id.clone(),
                            kind: action_kind,
                            reason: reason.clone(),
                        });
                        event_sink(GatewayEvent::Warning {
                            kind: "runtime_interaction_exposure_blocked".to_string(),
                            message: reason,
                            source_path: None,
                            suggestion: None,
                        });
                    }
                    interaction_gateway.dispatch_runtime_interaction_on(
                        interaction_dispatch_handle.clone(),
                        action_id,
                        pending,
                        runtime_interaction_decline_response(&interaction),
                    );
                    return;
                }
                let child_thread_id = interaction
                    .child_native_session_id
                    .as_deref()
                    .and_then(|native_session_id| {
                        project_runtime_native_child(
                            &state,
                            &profile,
                            &thread_id,
                            native_session_id,
                        )
                        .ok()
                    });
                let is_permission = interaction.policy.kind == RuntimeInteractionKind::Permission;
                let allow_session = is_permission
                    && runtime_permission_choice(
                        &interaction,
                        PermissionApprovalOutcome::AllowSession,
                    )
                    .is_some();
                let allow_always = is_permission
                    && runtime_permission_choice(
                        &interaction,
                        PermissionApprovalOutcome::AllowAlways,
                    )
                    .is_some();
                pending_runtime_interactions
                    .lock()
                    .expect("gateway runtime interaction map poisoned")
                    .insert(
                        action_id.clone(),
                        pending,
                    );
                if let Some(event_sink) = &event_sink {
                    event_sink(GatewayEvent::ActionRequested {
                        action: PendingActionView {
                            action_id,
                            kind: action_kind,
                            title: Some(interaction.kind.clone()),
                            summary: Some(interaction.prompt.clone()),
                            payload: json!({
                                "runtimeRef": runtime_ref,
                                "runtimeKind": runtime_kind_name(profile.kind),
                                "profileLabel": profile.label.clone(),
                                "toolName": interaction.kind.clone(),
                                "summary": interaction.prompt.clone(),
                                "reason": interaction.prompt.clone(),
                                "allowSession": allow_session,
                                "allowAlways": allow_always,
                                "choices": if is_permission {
                                    interaction.choices.clone()
                                } else {
                                    Vec::new()
                                },
                                "authorizationLifetime": interaction.authorization_lifetime.clone(),
                                "alwaysAuthorizationLifetime": allow_always.then_some("permanent"),
                                "interactionExpiresAtMs": interaction.expires_at_ms,
                                "origin": {
                                    "parentThreadId": thread_id,
                                    "childThreadId": child_thread_id,
                                },
                                "raw": {
                                    "questions": interaction.questions.clone(),
                                },
                            }),
                            thread_id: Some(thread_id.clone()),
                            turn_id: Some(turn_id.clone()),
                            activity_id: Some(turn_id.clone()),
                            source_key: None,
                            owner_id: None,
                            lease_expires_at_ms: interaction.expires_at_ms,
                        },
                    });
                }
            }
            RuntimeObservation::Warning {
                code,
                message,
                diagnostic_ref,
            } => {
                if let Some(event_sink) = &event_sink {
                    event_sink(GatewayEvent::Warning {
                        kind: code,
                        message,
                        source_path: None,
                        suggestion: diagnostic_ref,
                    });
                }
            }
        }
    })
    .with_session_binder({
        let state = gateway.state.clone();
        let expected_runtime_ref = runtime_ref.clone();
        let expected_thread_id = thread_id.clone();
        let expected_cwd = cwd.clone();
        let expected_binding_revision = binding.binding_revision;
        move |native_binding| {
            let state = state.clone();
            let expected_runtime_ref = expected_runtime_ref.clone();
            let expected_thread_id = expected_thread_id.clone();
            let expected_cwd = expected_cwd.clone();
            async move {
                let expected_binding_epoch =
                    u64::try_from(expected_binding_revision).unwrap_or_default();
                if native_binding.runtime_ref != expected_runtime_ref
                    || native_binding.thread_id != expected_thread_id
                    || native_binding.cwd != expected_cwd
                    || native_binding.binding_epoch != expected_binding_epoch
                {
                    return Err(RuntimeError::new(
                        "runtime_native_binding_mismatch",
                        RuntimeErrorStage::Binding,
                        RetryClass::Never,
                        "The runtime adapter returned native session identity for different Gateway provenance",
                    ));
                }
                state
                    .store()
                    .attach_gateway_runtime_native_session(
                        &expected_thread_id,
                        expected_binding_revision,
                        &native_binding.native_session_id,
                    )
                    .map(|_| ())
                    .map_err(|error| {
                        RuntimeError::new(
                            "runtime_native_binding_failed",
                            RuntimeErrorStage::Binding,
                            RetryClass::Never,
                            format!("Failed to persist runtime native session identity: {error}"),
                        )
                    })
            }
        }
    });
    let host_control = HostRuntimeControl::default();
    let control_bridge = request
        .control
        .map(|control| spawn_direct_runtime_control_bridge(control, host_control.clone()));
    let mode = request
        .options
        .runtime_options
        .get("mode")
        .cloned()
        .or_else(|| profile_config.default_mode.clone());
    let features = request
        .options
        .runtime_options
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "mode" | "model" | "agent"))
        .map(|(key, value)| (key.clone(), Value::String(value.clone())))
        .collect();
    if let Some(event_sink) = &event_sink {
        event_sink(GatewayEvent::TurnStarted {
            thread_id: Some(thread_id.clone()),
            turn_id: turn_id.clone(),
            selected_skills: Vec::new(),
        });
    }
    let execute_result = gateway
        .runtime_host
        .execute(
            ExecuteRequest {
                profile: profile.clone(),
                expected_profile_revision: profile_revision,
                expected_capability_revision: None,
                expected_binding_revision: Some(
                    u64::try_from(binding.binding_revision).unwrap_or_default(),
                ),
                intent: RuntimeIntent::Turn(RuntimeTurnRequest {
                    turn_id: turn_id.clone(),
                    thread_id: thread_id.clone(),
                    native_session_id: binding.native_session_id.clone(),
                    cwd: cwd.clone(),
                    prompt: prompt.clone(),
                    instructions,
                    model: request
                        .options
                        .runtime_options
                        .get("model")
                        .cloned(),
                    mode: mode.clone(),
                    agent: request
                        .options
                        .agent
                        .clone()
                        .or_else(|| profile_config.default_agent.clone()),
                    features,
                    interaction_exposure,
                    binding_epoch: u64::try_from(binding.binding_revision).unwrap_or_default(),
                }),
            },
            observer,
            host_control,
        )
        .await
        .map_err(runtime_host_error);
    if let Some(control_bridge) = control_bridge {
        control_bridge.abort();
    }
    let expired_actions = {
        let mut pending = gateway
            .pending_runtime_interactions
            .lock()
            .expect("gateway runtime interaction map poisoned");
        let actions = pending
            .iter()
            .filter(|(_, pending)| pending.interaction.thread_id == thread_id)
            .map(|(action_id, pending)| {
                let kind = runtime_interaction_action_kind(&pending.interaction);
                (action_id.clone(), kind)
            })
            .collect::<Vec<_>>();
        for (action_id, _) in &actions {
            pending.remove(action_id);
        }
        actions
    };
    if let Some(event_sink) = &event_sink {
        for (action_id, kind) in expired_actions {
            event_sink(GatewayEvent::ActionResolved {
                action_id,
                kind,
                outcome: crate::GatewayActionOutcome::Cancelled,
                payload: json!({"reason": "runtime_turn_ended"}),
            });
        }
    }
    let ExecuteResult::Turn(result) = execute_result? else {
        return Err(runtime_host_configuration_error(
            "runtime adapter returned a non-turn result for turn execution",
        ));
    };
    let binding = gateway
        .state
        .store()
        .attach_gateway_runtime_native_session(
            &thread_id,
            binding.binding_revision,
            &result.native_session_id,
        )?;
    // This is a cache-only adapter read after the worker already completed the
    // turn. It cannot spawn, and it gives bound Runtime Context an exact-session
    // observed control snapshot instead of reusing workspace defaults.
    let _ = gateway
        .observe_runtime_snapshot(SnapshotQuery {
            profile: profile.clone(),
            scope: SnapshotScope::Session {
                cwd: cwd.clone(),
                thread_id: thread_id.clone(),
                native_session_id: Some(result.native_session_id.clone()),
            },
            mode: psychevo_runtime_host::SnapshotMode::Cached,
        })
        .await;
    let _ = gateway
        .observe_runtime_snapshot(SnapshotQuery {
            profile: profile.clone(),
            scope: SnapshotScope::Workspace { cwd: cwd.clone() },
            mode: psychevo_runtime_host::SnapshotMode::Cached,
        })
        .await;
    let outcome = match result.outcome {
        RuntimeTurnOutcome::Completed => Outcome::Normal,
        RuntimeTurnOutcome::Interrupted => Outcome::Aborted,
        RuntimeTurnOutcome::Failed => Outcome::Failed,
    };
    let terminal_error = direct_runtime_terminal_error(
        result.outcome,
        result.terminal_error.clone(),
        &runtime_kind,
        result.process_epoch,
    );
    let user_message = psychevo_agent_core::user_text_message(prompt);
    gateway.state.store().append_message_with_metrics(
        &thread_id,
        &user_message,
        None,
        Some(json!({
            "runtimeRef": runtime_ref,
            "runtimeDedupKey": runtime_native_dedup_key(
                &runtime_ref,
                &result.native_session_id,
            ),
        })),
    )?;
    let assistant_message = psychevo_agent_core::Message::Assistant {
        content: vec![psychevo_agent_core::AssistantBlock::Text {
            text: result.final_answer.clone(),
        }],
        timestamp_ms: gateway_now_ms(),
        finish_reason: Some("stop".to_string()),
        outcome,
        model: Some(result.model.clone()),
        provider: Some(result.provider.clone()),
    };
    let runtime_usage = runtime_usage
        .lock()
        .expect("direct runtime usage accumulator poisoned")
        .clone();
    let message_usage = runtime_usage.as_ref().map(direct_runtime_message_usage);
    let context_snapshot = runtime_usage.as_ref().map(|update| {
        direct_runtime_context_snapshot(
            &thread_id,
            &result.provider,
            &result.model,
            mode.clone(),
            update,
        )
    });
    let runtime_plan = runtime_plan
        .lock()
        .expect("direct runtime plan accumulator poisoned")
        .clone();
    let runtime_diff = runtime_diff
        .lock()
        .expect("direct runtime diff accumulator poisoned")
        .clone();
    let mut assistant_metadata = json!({
        "runtimeRef": runtime_ref,
        "runtimeKind": runtime_kind,
        "historyFidelity": format!("{:?}", result.history_fidelity).to_ascii_lowercase(),
        "processEpoch": result.process_epoch,
        "instanceEpoch": result.instance_epoch,
    });
    if let Some(object) = assistant_metadata.as_object_mut() {
        if let Some(plan) = runtime_plan {
            object.insert(
                "runtimePlan".to_string(),
                serde_json::to_value(plan).expect("typed runtime plan serializes"),
            );
        }
        if let Some(diff) = runtime_diff {
            object.insert(
                "runtimeDiff".to_string(),
                serde_json::to_value(diff).expect("typed runtime diff serializes"),
            );
        }
    }
    gateway.state.store().append_message_with_metrics(
        &thread_id,
        &assistant_message,
        message_usage,
        Some(assistant_metadata),
    )?;
    gateway
        .state
        .store()
        .finish_session(&thread_id, outcome, None)?;
    if let Some(stream) = stream {
        stream(RunStreamEvent::value(json!({
            "type": "message_end",
            "message": assistant_message,
            "metadata": {
                "runtimeRef": runtime_ref,
                "historyFidelity": format!("{:?}", result.history_fidelity).to_ascii_lowercase(),
            },
        })));
    }
    Ok(DirectRuntimeTurn {
        native_session_id: binding
            .native_session_id
            .unwrap_or(result.native_session_id),
        run: RunResult {
            session_id: thread_id,
            outcome,
            terminal_reason: None,
            final_answer: result.final_answer,
            db_path: gateway.state.db_path().to_path_buf(),
            cwd,
            provider: result.provider,
            model: result.model,
            base_url: String::new(),
            api_key_env: None,
            reasoning_effort: None,
            context_limit: None,
            tool_failures: 0,
            selected_agent: request
                .options
                .agent
                .map(|name| psychevo_runtime::SelectedAgent {
                    name,
                    source: "runtime_profile".to_string(),
                    path: None,
                }),
            selected_skills: Vec::new(),
            context_snapshot,
            terminal_error,
            events: Vec::new(),
            warnings: Vec::new(),
        },
    })
}

fn spawn_direct_runtime_control_bridge(
    mut control: psychevo_runtime::RunControl,
    host_control: HostRuntimeControl,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut abort_signal = control.abort_signal();
        let mut steer_tick = tokio::time::interval(std::time::Duration::from_millis(25));
        steer_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = abort_signal.wait_for_abort() => {
                    host_control.abort();
                    break;
                }
                _ = steer_tick.tick() => {
                    for (_, message) in control.drain_pending_user_messages() {
                        if let Some(text) = direct_runtime_steer_text(message) {
                            host_control.steer(text);
                        }
                    }
                }
            }
        }
    })
}

fn direct_runtime_steer_text(message: psychevo_agent_core::Message) -> Option<String> {
    let psychevo_agent_core::Message::User { content, .. } = message else {
        return None;
    };
    let text = content
        .iter()
        .filter_map(psychevo_agent_core::UserContentBlock::text_value)
        .collect::<Vec<_>>()
        .join("\n");
    (!text.trim().is_empty()).then_some(text)
}

#[cfg(test)]
mod direct_runtime_control_tests {
    use super::*;

    #[test]
    fn typed_runtime_terminal_error_maps_without_adapter_metadata() {
        let error = direct_runtime_terminal_error(
            RuntimeTurnOutcome::Failed,
            Some(psychevo_runtime_host::RuntimeTerminalError {
                code: "event_gap".to_string(),
                stage: RuntimeErrorStage::Transport,
                retry_class: RetryClass::UnknownDelivery,
                message: "OpenCode event continuity was lost.".to_string(),
                diagnostic_ref: "opencode-process-9-event_gap".to_string(),
            }),
            "opencode",
            9,
        )
        .expect("failed turn classification");
        assert_eq!(error.code, "event_gap");
        assert_eq!(error.stage, "transport");
        assert_eq!(error.retry_class, "unknown_delivery");
        assert_eq!(error.message, "OpenCode event continuity was lost.");
        assert_eq!(error.diagnostic_ref, "opencode-process-9-event_gap");
    }

    #[test]
    fn unclassified_failed_runtime_turn_gets_safe_contract_error() {
        assert_eq!(
            direct_runtime_terminal_error(RuntimeTurnOutcome::Failed, None, "codex", 7),
            Some(psychevo_runtime::RunTerminalError {
                code: "adapter_contract_violation".to_string(),
                stage: "prompt".to_string(),
                retry_class: "never".to_string(),
                message: "The direct runtime failed without a classified terminal error."
                    .to_string(),
                diagnostic_ref: "codex-process-7-terminal-contract".to_string(),
            })
        );
        assert_eq!(
            direct_runtime_terminal_error(
                RuntimeTurnOutcome::Completed,
                Some(psychevo_runtime_host::RuntimeTerminalError {
                    code: "must_not_project".to_string(),
                    stage: RuntimeErrorStage::Prompt,
                    retry_class: RetryClass::Never,
                    message: "must not project".to_string(),
                    diagnostic_ref: "must-not-project".to_string(),
                }),
                "codex",
                7,
            ),
            None,
            "a successful turn must not expose a terminal error"
        );
    }

    #[tokio::test]
    async fn public_pending_steer_reaches_direct_runtime_control_once() {
        let (handle, control) = psychevo_runtime::run_control();
        let pending = handle
            .steer_user_message(psychevo_agent_core::user_text_message("original"))
            .expect("text steer should queue");
        assert!(handle.update_pending_user_message(
            pending,
            psychevo_agent_core::user_text_message("updated steer")
        ));
        let cancelled = handle
            .steer_user_message(psychevo_agent_core::user_text_message("cancelled"))
            .expect("second text steer should queue");
        assert!(handle.cancel_pending_user_message(cancelled));

        let host_control = HostRuntimeControl::default();
        let bridge = spawn_direct_runtime_control_bridge(control, host_control.clone());
        let forwarded = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                let messages = host_control.take_steer();
                if !messages.is_empty() {
                    break messages;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("steer bridge should make progress");

        assert_eq!(forwarded, vec!["updated steer"]);
        assert!(!handle.update_pending_user_message(
            pending,
            psychevo_agent_core::user_text_message("too late")
        ));
        handle.abort();
        tokio::time::timeout(std::time::Duration::from_secs(1), bridge)
            .await
            .expect("abort should stop the bridge")
            .expect("bridge task should finish cleanly");
        assert!(host_control.is_aborted());
    }
}

fn direct_runtime_plan_body(update: &psychevo_runtime_host::RuntimePlanUpdate) -> String {
    let mut lines = update
        .explanation
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| vec![value.to_string()])
        .unwrap_or_default();
    lines.extend(update.steps.iter().map(|step| {
        let marker = match step.status {
            RuntimePlanStepStatus::Pending => " ",
            RuntimePlanStepStatus::InProgress => "~",
            RuntimePlanStepStatus::Completed => "x",
            RuntimePlanStepStatus::Cancelled => "-",
        };
        format!("- [{marker}] {}", step.step)
    }));
    if lines.is_empty() {
        "No plan entries.".to_string()
    } else {
        lines.join("\n")
    }
}

fn nonnegative_runtime_tokens(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

fn direct_runtime_message_usage(update: &RuntimeUsageUpdate) -> Value {
    let usage = &update.usage.last;
    json!({
        "input_tokens": nonnegative_runtime_tokens(usage.input_tokens),
        "cached_input_tokens": nonnegative_runtime_tokens(usage.cached_input_tokens),
        "output_tokens": nonnegative_runtime_tokens(usage.output_tokens),
        "reasoning_tokens": nonnegative_runtime_tokens(usage.reasoning_output_tokens),
        "total_tokens": nonnegative_runtime_tokens(usage.total_tokens),
    })
}

fn direct_runtime_context_snapshot(
    thread_id: &str,
    provider: &str,
    model: &str,
    mode: Option<String>,
    update: &RuntimeUsageUpdate,
) -> psychevo_runtime::ContextSnapshot {
    let input_tokens = nonnegative_runtime_tokens(update.usage.last.input_tokens);
    let context_limit = update
        .usage
        .model_context_window
        .and_then(|value| u64::try_from(value).ok());
    let percent = context_limit
        .filter(|limit| *limit > 0)
        .map(|limit| (input_tokens as f64 / limit as f64) * 100.0);
    let mut categories = std::collections::BTreeMap::new();
    categories.insert(
        "runtime_input".to_string(),
        psychevo_runtime::ContextCategory {
            label: "Runtime input".to_string(),
            tokens: input_tokens,
            estimated: false,
            status: "runtime_usage".to_string(),
            percent,
            details: json!({
                "cachedInputTokens": nonnegative_runtime_tokens(
                    update.usage.last.cached_input_tokens,
                ),
                "outputTokens": nonnegative_runtime_tokens(update.usage.last.output_tokens),
                "reasoningOutputTokens": nonnegative_runtime_tokens(
                    update.usage.last.reasoning_output_tokens,
                ),
            }),
        },
    );
    psychevo_runtime::ContextSnapshot {
        event_type: "context_snapshot".to_string(),
        scope: psychevo_runtime::ContextScope::LastProviderRequest,
        status: "runtime_usage".to_string(),
        session_id: Some(thread_id.to_string()),
        provider: provider.to_string(),
        model: model.to_string(),
        mode,
        context_limit,
        tokenizer: psychevo_runtime::ContextTokenizer {
            encoding: "runtime-native".to_string(),
            source: "runtime_usage".to_string(),
            fallback: false,
        },
        total: psychevo_runtime::ContextTotal {
            tokens: input_tokens,
            estimated_tokens: input_tokens,
            estimated: false,
            source: "runtime_usage".to_string(),
            percent,
        },
        categories,
        advice: Vec::new(),
    }
}

fn direct_runtime_assistant_message(
    text: &str,
    finish_reason: &str,
    runtime_ref: &str,
) -> psychevo_agent_core::Message {
    psychevo_agent_core::Message::Assistant {
        content: vec![psychevo_agent_core::AssistantBlock::Text {
            text: text.to_string(),
        }],
        timestamp_ms: gateway_now_ms(),
        finish_reason: Some(finish_reason.to_string()),
        outcome: Outcome::Normal,
        model: None,
        provider: Some(runtime_ref.to_string()),
    }
}

fn public_runtime_tool_detail(detail: Option<&Value>) -> Option<Value> {
    const ALLOWED_FIELDS: &[&str] = &[
        "aggregatedOutput",
        "arguments",
        "changes",
        "command",
        "cwd",
        "diff",
        "durationMs",
        "error",
        "exitCode",
        "files",
        "input",
        "output",
        "path",
        "result",
        "server",
        "status",
        "title",
        "tool",
    ];
    let detail = detail?.as_object()?;
    let identities = runtime_tool_native_identities(detail);
    let projected = detail
        .iter()
        .filter(|(key, _)| ALLOWED_FIELDS.contains(&key.as_str()))
        .filter_map(|(key, value)| {
            sanitize_runtime_tool_value(value, &identities).map(|value| (key.clone(), value))
        })
        .collect::<serde_json::Map<_, _>>();
    (!projected.is_empty()).then_some(Value::Object(projected))
}

fn runtime_tool_native_identities(detail: &serde_json::Map<String, Value>) -> HashSet<String> {
    let mut identities = HashSet::new();
    collect_runtime_tool_native_identities(&Value::Object(detail.clone()), &mut identities);
    identities
}

fn collect_runtime_tool_native_identities(value: &Value, identities: &mut HashSet<String>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if runtime_tool_identity_key(key) {
                    if let Some(identity) = value.as_str().filter(|value| !value.is_empty()) {
                        identities.insert(identity.to_string());
                    }
                } else {
                    collect_runtime_tool_native_identities(value, identities);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_runtime_tool_native_identities(value, identities);
            }
        }
        _ => {}
    }
}

fn sanitize_runtime_tool_value(value: &Value, identities: &HashSet<String>) -> Option<Value> {
    match value {
        Value::Object(object) => Some(Value::Object(
            object
                .iter()
                .filter(|(key, _)| !runtime_tool_identity_key(key))
                .filter_map(|(key, value)| {
                    sanitize_runtime_tool_value(value, identities).map(|value| (key.clone(), value))
                })
                .collect(),
        )),
        Value::Array(values) => Some(Value::Array(
            values
                .iter()
                .filter_map(|value| sanitize_runtime_tool_value(value, identities))
                .collect(),
        )),
        Value::String(value) if identities.contains(value) => None,
        _ => Some(value.clone()),
    }
}

fn runtime_tool_identity_key(key: &str) -> bool {
    matches!(
        key.chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
            .as_str(),
        "id" | "callid"
            | "clientid"
            | "eventid"
            | "itemid"
            | "messageid"
            | "nativeid"
            | "parentid"
            | "partid"
            | "requestid"
            | "sessionid"
            | "threadid"
            | "turnid"
    )
}

fn runtime_native_dedup_key(runtime_ref: &str, native_id: &str) -> String {
    let digest = Sha256::digest(format!("{runtime_ref}\0{native_id}").as_bytes());
    format!("rt_{}", &format!("{digest:x}")[..20])
}

fn runtime_session_handle(runtime_ref: &str, cwd: &Path, native_session_id: &str) -> String {
    let canonical = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let digest = Sha256::digest(
        format!(
            "runtime-session-v1\0{runtime_ref}\0{}\0{native_session_id}",
            psychevo_runtime::normalized_native_path(&canonical).display()
        )
        .as_bytes(),
    );
    format!("rts_{}", &format!("{digest:x}")[..24])
}

fn runtime_public_dedup_key(runtime_ref: &str, native_dedup_key: &str) -> String {
    let digest =
        Sha256::digest(format!("runtime-dedup-v1\0{runtime_ref}\0{native_dedup_key}").as_bytes());
    format!("rtd_{}", &format!("{digest:x}")[..24])
}

fn runtime_interaction_action_id(runtime_ref: &str, native_id: &str) -> String {
    let digest = Sha256::digest(format!("interaction\0{runtime_ref}\0{native_id}").as_bytes());
    format!("rt_{}", &format!("{digest:x}")[..10])
}

fn project_runtime_native_child(
    state: &StateRuntime,
    profile: &RuntimeProfile,
    parent_thread_id: &str,
    native_session_id: &str,
) -> psychevo_runtime::Result<String> {
    if let Some(binding) = state
        .store()
        .gateway_runtime_binding_by_native_session(&profile.id, native_session_id)?
    {
        if binding.parent_thread_id.as_deref() != Some(parent_thread_id) {
            return Err(Error::Message(
                "native child topology conflicts with its immutable public binding".to_string(),
            ));
        }
        return Ok(binding.thread_id);
    }
    let parent = state
        .store()
        .session_summary(parent_thread_id)?
        .ok_or_else(|| Error::Message(format!("session not found: {parent_thread_id}")))?;
    let thread_id = state.store().create_child_session_with_metadata(
        parent_thread_id,
        Path::new(&parent.cwd),
        "runtime_child",
        "pending",
        &profile.id,
        Some(json!({
            "runtimeRef": profile.id,
            "runtimeDedupKey": runtime_native_dedup_key(&profile.id, native_session_id),
            "ownership": "read_only",
        })),
    )?;
    let native_kind = match profile.kind {
        RuntimeKind::Native => "native",
        RuntimeKind::Acp => "acp",
        RuntimeKind::Codex => "codex",
        RuntimeKind::OpenCode => "opencode",
    };
    let profile_config = runtime_profile_config_from_host(profile);
    let profile_config_json = serde_json::to_string(&profile_config)?;
    state
        .store()
        .create_gateway_runtime_binding(GatewayRuntimeBindingInput {
            thread_id: &thread_id,
            runtime_ref: &profile.id,
            backend_kind: "runtime",
            native_kind,
            native_session_id: Some(native_session_id),
            cwd: &parent.cwd,
            profile_fingerprint: &profile.fingerprint,
            profile_revision: &profile.revision.to_string(),
            profile_config_json: &profile_config_json,
            adapter_kind: native_kind,
            adapter_revision: env!("CARGO_PKG_VERSION"),
            ownership: GatewayRuntimeBindingOwnership::ReadOnly,
            parent_thread_id: Some(parent_thread_id),
        })?;
    Ok(thread_id)
}

fn resolve_runtime_native_parent_thread(
    state: &StateRuntime,
    profile: &RuntimeProfile,
    root_thread_id: &str,
    parent_native_session_id: &str,
) -> psychevo_runtime::Result<String> {
    if let Some(binding) = state
        .store()
        .gateway_runtime_binding_by_native_session(&profile.id, parent_native_session_id)?
    {
        return Ok(binding.thread_id);
    }
    let root = state
        .store()
        .gateway_runtime_binding(root_thread_id)?
        .ok_or_else(|| Error::Message(format!("runtime binding not found: {root_thread_id}")))?;
    if root.runtime_ref.as_deref() == Some(profile.id.as_str())
        && root.native_session_id.as_deref() == Some(parent_native_session_id)
    {
        return Ok(root_thread_id.to_string());
    }
    Err(Error::Message(
        "native child parent has no public runtime binding".to_string(),
    ))
}

fn runtime_profile_config_from_host(profile: &RuntimeProfile) -> RuntimeProfileConfig {
    RuntimeProfileConfig {
        id: profile.id.clone(),
        runtime: match profile.kind {
            RuntimeKind::Native => RuntimeProfileKind::Native,
            RuntimeKind::Acp => RuntimeProfileKind::Acp,
            RuntimeKind::Codex => RuntimeProfileKind::Codex,
            RuntimeKind::OpenCode => RuntimeProfileKind::OpenCode,
        },
        enabled: profile.enabled,
        label: profile.label.clone(),
        backend_ref: profile.backend_ref.clone(),
        command: profile.command.clone(),
        args: profile.args.clone(),
        env: profile.env.clone(),
        default_model: profile.default_model.clone(),
        default_mode: profile.default_mode.clone(),
        default_agent: profile.default_agent.clone(),
        approval_mode: profile.approval_mode.clone(),
        sandbox: profile.sandbox.clone(),
        workspace_roots: profile
            .workspace_roots
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect(),
        options: profile.options.clone(),
    }
}

impl Gateway {
    fn submit_runtime_permission(
        &self,
        selector: &GatewayThreadSelector,
        action_id: &str,
        decision: &PermissionApprovalDecision,
    ) -> bool {
        let Some(pending) = self.take_runtime_interaction(selector, action_id) else {
            return false;
        };
        let Some(response) = runtime_permission_response(&pending.interaction, decision) else {
            self.pending_runtime_interactions
                .lock()
                .expect("gateway runtime interaction map poisoned")
                .insert(action_id.to_string(), pending);
            return false;
        };
        self.dispatch_runtime_interaction(action_id.to_string(), pending, response)
    }

    fn submit_runtime_clarify(
        &self,
        selector: &GatewayThreadSelector,
        action_id: &str,
        result: &ClarifyResult,
    ) -> bool {
        let Some(pending) = self.take_runtime_interaction(selector, action_id) else {
            return false;
        };
        if pending.interaction.policy.kind != RuntimeInteractionKind::Question {
            self.pending_runtime_interactions
                .lock()
                .expect("gateway runtime interaction map poisoned")
                .insert(action_id.to_string(), pending);
            return false;
        }
        let response = match result {
            ClarifyResult::Answered(response) => json!({
                "answers": response
                    .answers
                    .iter()
                    .map(|answer| answer.answers.clone())
                    .collect::<Vec<_>>(),
            }),
            ClarifyResult::Cancelled => json!({"reject": true, "decision": "cancel"}),
        };
        self.dispatch_runtime_interaction(action_id.to_string(), pending, response)
    }

    fn take_runtime_interaction(
        &self,
        selector: &GatewayThreadSelector,
        action_id: &str,
    ) -> Option<PendingRuntimeInteraction> {
        let pending = self
            .pending_runtime_interactions
            .lock()
            .expect("gateway runtime interaction map poisoned")
            .get(action_id)
            .cloned()?;
        let selector_keys = self.selector_keys_with_active_aliases(selector);
        if !selector_keys
            .iter()
            .any(|key| key == &thread_key(&pending.interaction.thread_id))
        {
            return None;
        }
        self.pending_runtime_interactions
            .lock()
            .expect("gateway runtime interaction map poisoned")
            .remove(action_id)
    }

    fn dispatch_runtime_interaction(
        &self,
        action_id: String,
        pending: PendingRuntimeInteraction,
        response: Value,
    ) -> bool {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            self.pending_runtime_interactions
                .lock()
                .expect("gateway runtime interaction map poisoned")
                .insert(action_id, pending);
            return false;
        };
        self.dispatch_runtime_interaction_on(handle, action_id, pending, response)
    }

    fn dispatch_runtime_interaction_on(
        &self,
        handle: tokio::runtime::Handle,
        action_id: String,
        pending: PendingRuntimeInteraction,
        response: Value,
    ) -> bool {
        let host = self.runtime_host.clone();
        let binding_revision = self
            .state
            .store()
            .gateway_runtime_binding(&pending.interaction.thread_id)
            .ok()
            .flatten()
            .map(|binding| u64::try_from(binding.binding_revision).unwrap_or_default());
        handle.spawn(async move {
            let kind = runtime_interaction_action_kind(&pending.interaction);
            let result = host
                .execute(
                    ExecuteRequest {
                        expected_profile_revision: pending.profile.revision,
                        expected_capability_revision: None,
                        expected_binding_revision: binding_revision,
                        profile: pending.profile,
                        intent: RuntimeIntent::Interaction(
                            psychevo_runtime_host::RuntimeInteractionResponse {
                                interaction_id: pending.interaction.id,
                                process_epoch: pending.interaction.process_epoch,
                                instance_epoch: pending.interaction.instance_epoch,
                                response,
                            },
                        ),
                    },
                    RuntimeObserver::default(),
                    HostRuntimeControl::default(),
                )
                .await;
            if let Some(event_sink) = pending.event_sink {
                let (outcome, payload) = match result {
                    Ok(ExecuteResult::Interaction(result)) if result.accepted => (
                        crate::GatewayActionOutcome::Accepted,
                        json!({"accepted": true}),
                    ),
                    Ok(ExecuteResult::Interaction(result)) if result.expired => (
                        crate::GatewayActionOutcome::TimedOut,
                        json!({"accepted": false, "message": result.message}),
                    ),
                    Ok(ExecuteResult::Interaction(result)) => (
                        crate::GatewayActionOutcome::Rejected,
                        json!({"accepted": false, "message": result.message}),
                    ),
                    Ok(_) => (
                        crate::GatewayActionOutcome::Rejected,
                        json!({"accepted": false, "message": "runtime returned the wrong result kind"}),
                    ),
                    Err(error) => (
                        crate::GatewayActionOutcome::Rejected,
                        json!({"accepted": false, "message": error.message}),
                    ),
                };
                event_sink(GatewayEvent::ActionResolved {
                    action_id,
                    kind,
                    outcome,
                    payload,
                });
            }
        });
        true
    }
}

fn runtime_permission_response(
    interaction: &psychevo_runtime_host::RuntimeInteraction,
    decision: &PermissionApprovalDecision,
) -> Option<Value> {
    let choice = runtime_permission_choice(interaction, decision.outcome)?;
    Some(json!({"decision": choice.decision}))
}

fn runtime_permission_choice(
    interaction: &psychevo_runtime_host::RuntimeInteraction,
    outcome: PermissionApprovalOutcome,
) -> Option<&psychevo_runtime_host::RuntimeInteractionChoice> {
    if interaction.policy.kind != RuntimeInteractionKind::Permission {
        return None;
    }
    interaction.choices.iter().find(|choice| {
        let id = choice.id.to_ascii_lowercase();
        let decision = choice.decision.to_ascii_lowercase();
        match outcome {
            PermissionApprovalOutcome::AllowOnce => {
                id.contains("once") || matches!(decision.as_str(), "once" | "accept")
            }
            PermissionApprovalOutcome::AllowSession => {
                interaction.authorization_lifetime.is_some()
                    && (id.contains("session")
                        || decision.contains("session")
                        || matches!(decision.as_str(), "always"))
            }
            PermissionApprovalOutcome::AllowAlways => {
                interaction.authorization_lifetime.as_deref() == Some("permanent")
                    && (id.contains("always") || decision.contains("always"))
            }
            PermissionApprovalOutcome::Deny => {
                id.contains("deny")
                    || id.contains("decline")
                    || id.contains("reject")
                    || decision.contains("deny")
                    || decision.contains("decline")
                    || decision.contains("reject")
            }
        }
    })
}

fn runtime_interaction_action_kind(
    interaction: &psychevo_runtime_host::RuntimeInteraction,
) -> GatewayActionKind {
    match interaction.policy.kind {
        RuntimeInteractionKind::Permission => GatewayActionKind::Permission,
        RuntimeInteractionKind::Question => GatewayActionKind::Clarify,
        RuntimeInteractionKind::UserInput => GatewayActionKind::UserInput,
    }
}

fn runtime_interaction_decline_response(
    interaction: &psychevo_runtime_host::RuntimeInteraction,
) -> Value {
    match interaction.policy.kind {
        RuntimeInteractionKind::Permission => json!({"decision": "deny"}),
        RuntimeInteractionKind::Question | RuntimeInteractionKind::UserInput => {
            json!({"reject": true, "decision": "cancel"})
        }
    }
}

fn runtime_kind_name(kind: RuntimeKind) -> &'static str {
    match kind {
        RuntimeKind::Native => "native",
        RuntimeKind::Acp => "acp",
        RuntimeKind::Codex => "codex",
        RuntimeKind::OpenCode => "opencode",
    }
}
