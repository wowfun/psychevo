use std::path::Path;

use psychevo_runtime::{
    AgentDefinition, AgentDiscoveryOptions, AgentEntrypoint, Error, GatewayRuntimeBindingInput,
    GatewayRuntimeBindingOwnership, GatewayRuntimeBindingRecord, GatewayRuntimeBindingStatus,
    RunOptions, RuntimeProfileConfig, RuntimeProfileKind, StateRuntime, discover_agents,
    load_agent_backend_configs, load_runtime_profile_configs, resolve_agent_definition,
    resolve_skills_home,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    AgentErrorStage, ResolvedPeerTurn, agent_session_configuration_error, agent_session_error,
};

pub(super) struct BoundGatewayRuntimeProfile {
    pub(super) profile: RuntimeProfileConfig,
    pub(super) revision: u64,
    pub(super) fingerprint: String,
}

pub(crate) struct BoundGatewayAgentTarget {
    pub(super) binding: GatewayRuntimeBindingRecord,
    pub(super) profile: RuntimeProfileConfig,
    pub(super) revision: u64,
    pub(super) fingerprint: String,
    pub(super) peer: Option<ResolvedPeerTurn>,
}

pub(super) struct GatewayAgentBindingSnapshot {
    pub(super) agent_ref: Option<String>,
    fingerprint: String,
    definition_json: String,
}

pub(crate) fn gateway_agent_definition_fingerprint(definition_json: &str) -> String {
    format!("{:x}", Sha256::digest(definition_json.as_bytes()))
}

pub(crate) fn agent_definition_matches_runtime_profile(
    agent: &AgentDefinition,
    profile: &RuntimeProfileConfig,
) -> bool {
    agent_definition_matches_runtime_profile_at(agent, profile, AgentEntrypoint::Peer)
}

fn agent_definition_matches_runtime_profile_at(
    agent: &AgentDefinition,
    profile: &RuntimeProfileConfig,
    entrypoint: AgentEntrypoint,
) -> bool {
    match profile.runtime {
        RuntimeProfileKind::Native => agent.backend.is_none(),
        RuntimeProfileKind::Acp => {
            agent.supports_entrypoint(entrypoint)
                && profile
                    .backend_ref
                    .as_deref()
                    .zip(agent.backend.as_ref().map(|backend| backend.name.as_str()))
                    .is_some_and(|(profile_backend, agent_backend)| {
                        profile_backend == agent_backend
                    })
        }
    }
}

pub(super) fn resolve_gateway_agent_binding_snapshot(
    options: &RunOptions,
    profile: &RuntimeProfileConfig,
    existing: Option<&GatewayRuntimeBindingRecord>,
    entrypoint: AgentEntrypoint,
) -> psychevo_runtime::Result<GatewayAgentBindingSnapshot> {
    let requested_agent_ref = options
        .agent
        .as_deref()
        .map(str::trim)
        .filter(|agent_ref| !agent_ref.is_empty())
        .or_else(|| existing.and_then(|binding| binding.agent_ref.as_deref()));
    if let Some(existing) = existing {
        if requested_agent_ref != existing.agent_ref.as_deref() {
            return Err(agent_session_error(
                "immutable_binding",
                AgentErrorStage::Binding,
                "user_action",
                "not_delivered",
                format!(
                    "Thread `{}` is bound to Agent target `{}`; start a new Thread to use `{}`.",
                    existing.thread_id,
                    existing.agent_ref.as_deref().unwrap_or("Default Agent"),
                    requested_agent_ref.unwrap_or("Default Agent"),
                ),
                Some(format!("agent-binding:{}", existing.thread_id)),
            ));
        }
        return Ok(GatewayAgentBindingSnapshot {
            agent_ref: existing.agent_ref.clone(),
            fingerprint: existing.agent_fingerprint.clone().ok_or_else(|| {
                agent_session_configuration_error(
                    "Bound Agent Definition snapshot is missing its fingerprint.",
                )
            })?,
            definition_json: existing.agent_definition_json.clone().ok_or_else(|| {
                agent_session_configuration_error(
                    "Bound Agent Definition snapshot is missing its captured definition.",
                )
            })?,
        });
    }
    let Some(agent_ref) = requested_agent_ref else {
        if profile.runtime != RuntimeProfileKind::Native {
            return Err(agent_session_configuration_error(format!(
                "ACP Runtime Profile `{}` requires an explicit compatible Agent Definition target.",
                profile.id
            )));
        }
        let definition_json = json!({
            "kind": "psychevo.default-agent",
            "version": 1,
            "agentRef": Value::Null,
        })
        .to_string();
        return Ok(GatewayAgentBindingSnapshot {
            agent_ref: None,
            fingerprint: gateway_agent_definition_fingerprint(&definition_json),
            definition_json,
        });
    };
    let env = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| std::env::vars().collect());
    let home = resolve_skills_home(&env, &options.cwd)?;
    let catalog = discover_agents(&AgentDiscoveryOptions {
        home,
        cwd: options.cwd.clone(),
        env: env.clone(),
        explicit_inputs: vec![agent_ref.to_string()],
        no_agents: false,
    })?;
    let agent = resolve_agent_definition(&catalog, agent_ref, &options.cwd, &env)?;
    if !agent_definition_matches_runtime_profile_at(&agent, profile, entrypoint) {
        return Err(agent_session_configuration_error(format!(
            "Agent Definition `{agent_ref}` is not compatible with Runtime Profile `{}`.",
            profile.id
        )));
    }
    let definition_json = serde_json::to_string(&agent)?;
    Ok(GatewayAgentBindingSnapshot {
        agent_ref: Some(agent.name),
        fingerprint: gateway_agent_definition_fingerprint(&definition_json),
        definition_json,
    })
}

pub(crate) fn resolve_bound_gateway_agent_target(
    options: &RunOptions,
    requested_runtime_ref: Option<&str>,
) -> psychevo_runtime::Result<Option<BoundGatewayAgentTarget>> {
    let Some(thread_id) = options.session.as_deref() else {
        return Ok(None);
    };
    let Some(binding) = options.state.store().gateway_runtime_binding(thread_id)? else {
        return Ok(None);
    };
    let bound =
        resolve_bound_gateway_runtime_profile(&options.state, thread_id, requested_runtime_ref)?
            .ok_or_else(|| {
                agent_session_error(
                    "bound_profile_snapshot_missing",
                    AgentErrorStage::Binding,
                    "never",
                    "not_delivered",
                    "The immutable Runtime Profile snapshot is missing; reset pre-release state.",
                    Some(format!("agent-binding:{thread_id}")),
                )
            })?;
    let peer = resolve_captured_bound_peer(options, &binding, &bound.profile, &bound.fingerprint)?;
    Ok(Some(BoundGatewayAgentTarget {
        binding,
        profile: bound.profile,
        revision: bound.revision,
        fingerprint: bound.fingerprint,
        peer,
    }))
}

fn resolve_captured_bound_peer(
    options: &RunOptions,
    binding: &GatewayRuntimeBindingRecord,
    profile: &RuntimeProfileConfig,
    profile_fingerprint: &str,
) -> psychevo_runtime::Result<Option<ResolvedPeerTurn>> {
    if profile.runtime == RuntimeProfileKind::Native {
        return Ok(None);
    }
    let agent_ref = binding.agent_ref.as_deref().ok_or_else(|| {
        agent_session_error(
            "bound_agent_snapshot_missing",
            AgentErrorStage::Binding,
            "never",
            "not_delivered",
            "A bound ACP Thread is missing its captured Agent identity.",
            Some(format!("agent-binding:{}", binding.thread_id)),
        )
    })?;
    let encoded = binding.agent_definition_json.as_deref().ok_or_else(|| {
        agent_session_error(
            "bound_agent_snapshot_missing",
            AgentErrorStage::Binding,
            "never",
            "not_delivered",
            "The immutable Agent Definition snapshot is missing; reset pre-release state.",
            Some(format!("agent-binding:{}", binding.thread_id)),
        )
    })?;
    let fingerprint = binding.agent_fingerprint.as_deref().ok_or_else(|| {
        agent_session_error(
            "bound_agent_snapshot_missing",
            AgentErrorStage::Binding,
            "never",
            "not_delivered",
            "The immutable Agent Definition fingerprint is missing; reset pre-release state.",
            Some(format!("agent-binding:{}", binding.thread_id)),
        )
    })?;
    if gateway_agent_definition_fingerprint(encoded) != fingerprint {
        return Err(agent_session_error(
            "bound_agent_snapshot_mismatch",
            AgentErrorStage::Binding,
            "never",
            "not_delivered",
            "Bound Agent Definition snapshot does not match its immutable fingerprint.",
            Some(format!("agent-binding:{}", binding.thread_id)),
        ));
    }
    let agent: AgentDefinition = serde_json::from_str(encoded).map_err(|error| {
        agent_session_error(
            "bound_agent_snapshot_invalid",
            AgentErrorStage::Binding,
            "never",
            "not_delivered",
            format!("Bound Agent Definition snapshot could not be decoded: {error}"),
            Some(format!("agent-binding:{}", binding.thread_id)),
        )
    })?;
    if agent.name != agent_ref {
        return Err(agent_session_error(
            "bound_agent_snapshot_mismatch",
            AgentErrorStage::Binding,
            "never",
            "not_delivered",
            "Bound Agent Definition snapshot does not match its immutable identity.",
            Some(format!("agent-binding:{}", binding.thread_id)),
        ));
    }
    let backend_ref = profile.backend_ref.as_deref().ok_or_else(|| {
        agent_session_configuration_error(format!(
            "ACP Runtime Profile `{}` is missing backendRef.",
            profile.id
        ))
    })?;
    if agent.backend.as_ref().map(|backend| backend.name.as_str()) != Some(backend_ref) {
        return Err(agent_session_error(
            "bound_target_mismatch",
            AgentErrorStage::Binding,
            "never",
            "not_delivered",
            format!(
                "Captured Agent `{agent_ref}` does not use the captured Runtime Profile backend `{backend_ref}`."
            ),
            Some(format!("agent-binding:{}", binding.thread_id)),
        ));
    }
    let env = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| std::env::vars().collect());
    let home = resolve_skills_home(&env, &options.cwd)?;
    let backends = load_agent_backend_configs(&home, &options.cwd, &env)?;
    let backend = backends.get(backend_ref).cloned().ok_or_else(|| {
        agent_session_error(
            "runtime_unavailable",
            AgentErrorStage::Configuration,
            "user_action",
            "not_delivered",
            format!("Captured ACP backend `{backend_ref}` is unavailable."),
            Some(format!("agent-binding:{}", binding.thread_id)),
        )
    })?;
    if !backend.enabled
        || backend
            .command
            .as_deref()
            .is_none_or(|command| command.trim().is_empty())
    {
        return Err(agent_session_error(
            "runtime_unavailable",
            AgentErrorStage::Configuration,
            "user_action",
            "not_delivered",
            format!("Captured ACP backend `{backend_ref}` is disabled or unlaunchable."),
            Some(format!("agent-binding:{}", binding.thread_id)),
        ));
    }
    Ok(Some(ResolvedPeerTurn {
        agent,
        backend,
        env,
        process_scope_fingerprint: Some(profile_fingerprint.to_string()),
    }))
}

pub(super) fn resolve_gateway_runtime_profile(
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
    if !profiles.contains_key(runtime_ref) && runtime_ref != "native" {
        let backend_id = runtime_ref.strip_prefix("acp:").unwrap_or(runtime_ref);
        let backends = load_agent_backend_configs(&home, &options.cwd, &env)?;
        if let Some(backend) = backends.get(backend_id).filter(|backend| backend.enabled) {
            profiles.insert(
                runtime_ref.to_string(),
                RuntimeProfileConfig {
                    id: runtime_ref.to_string(),
                    runtime: RuntimeProfileKind::Acp,
                    enabled: true,
                    label: if runtime_ref == backend.id {
                        backend.label.clone()
                    } else {
                        format!("{} (ACP)", backend.label.trim_end_matches("(ACP)").trim())
                    },
                    backend_ref: Some(backend.id.clone()),
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
    let profile = profiles.remove(runtime_ref).ok_or_else(|| {
        agent_session_configuration_error(format!("unknown Runtime Profile: {runtime_ref}"))
    })?;
    if !profile.enabled {
        return Err(agent_session_configuration_error(format!(
            "Runtime Profile `{runtime_ref}` is disabled."
        )));
    }
    if profile.runtime == RuntimeProfileKind::Acp {
        let backend_ref = profile.backend_ref.as_deref().ok_or_else(|| {
            agent_session_configuration_error(format!(
                "ACP Runtime Profile `{runtime_ref}` is missing backendRef."
            ))
        })?;
        let backends = load_agent_backend_configs(&home, &options.cwd, &env)?;
        let backend = backends.get(backend_ref).ok_or_else(|| {
            agent_session_configuration_error(format!(
                "ACP Runtime Profile `{runtime_ref}` references unknown backend `{backend_ref}`."
            ))
        })?;
        if !backend.enabled {
            return Err(agent_session_configuration_error(format!(
                "ACP backend `{backend_ref}` is disabled."
            )));
        }
    }
    let fingerprint = runtime_profile_config_fingerprint(&profile);
    let revision = runtime_profile_config_revision(&fingerprint);
    Ok((profile, revision, fingerprint))
}

pub(super) fn resolve_bound_gateway_runtime_profile(
    state: &StateRuntime,
    thread_id: &str,
    requested_runtime_ref: Option<&str>,
) -> psychevo_runtime::Result<Option<BoundGatewayRuntimeProfile>> {
    let Some(binding) = state.store().gateway_runtime_binding(thread_id)? else {
        return Ok(None);
    };
    if binding.status != GatewayRuntimeBindingStatus::Resolved {
        return Err(agent_session_error(
            "unresolved_binding",
            AgentErrorStage::Binding,
            "user_action",
            "not_delivered",
            binding
                .unresolved_reason
                .clone()
                .unwrap_or_else(|| "This thread has an unresolved runtime binding.".to_string()),
            Some(format!("agent-binding:{thread_id}")),
        ));
    }
    if let Some(requested) = requested_runtime_ref
        .map(str::trim)
        .filter(|value| !value.is_empty())
        && binding.runtime_ref.as_deref() != Some(requested)
    {
        return Err(agent_session_error(
            "immutable_binding",
            AgentErrorStage::Binding,
            "user_action",
            "not_delivered",
            format!(
                "Thread `{thread_id}` is bound to Runtime Profile `{}`; start a new thread to use `{requested}`.",
                binding.runtime_ref.as_deref().unwrap_or("unresolved")
            ),
            Some(format!("agent-binding:{thread_id}")),
        ));
    }
    let snapshot = binding.profile_config_json.as_deref().ok_or_else(|| {
        agent_session_error(
            "bound_profile_snapshot_missing",
            AgentErrorStage::Binding,
            "never",
            "not_delivered",
            "The immutable Runtime Profile snapshot is missing; reset pre-release state.",
            Some(format!("agent-binding:{thread_id}")),
        )
    })?;
    let profile: RuntimeProfileConfig = serde_json::from_str(snapshot).map_err(|error| {
        agent_session_error(
            "bound_profile_snapshot_invalid",
            AgentErrorStage::Binding,
            "never",
            "not_delivered",
            format!("Bound Runtime Profile snapshot could not be decoded: {error}"),
            Some(format!("agent-binding:{thread_id}")),
        )
    })?;
    if binding.runtime_ref.as_deref() != Some(profile.id.as_str()) {
        return Err(agent_session_error(
            "bound_profile_snapshot_mismatch",
            AgentErrorStage::Binding,
            "never",
            "not_delivered",
            "Bound Runtime Profile snapshot does not match its immutable identity.",
            Some(format!("agent-binding:{thread_id}")),
        ));
    }
    let fingerprint = runtime_profile_config_fingerprint(&profile);
    if binding.profile_fingerprint.as_deref() != Some(fingerprint.as_str()) {
        return Err(agent_session_error(
            "bound_profile_snapshot_mismatch",
            AgentErrorStage::Binding,
            "never",
            "not_delivered",
            "Bound Runtime Profile snapshot does not match its fingerprint.",
            Some(format!("agent-binding:{thread_id}")),
        ));
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
    }))
}

pub(crate) fn generated_gateway_runtime_profiles() -> Vec<RuntimeProfileConfig> {
    vec![RuntimeProfileConfig {
        id: "native".to_string(),
        runtime: RuntimeProfileKind::Native,
        enabled: true,
        label: "Psychevo (Native)".to_string(),
        backend_ref: None,
        default_model: None,
        default_mode: None,
        default_agent: None,
        approval_mode: None,
        sandbox: None,
        workspace_roots: Vec::new(),
        options: Value::Null,
    }]
}

pub(crate) fn runtime_profile_config_fingerprint(profile: &RuntimeProfileConfig) -> String {
    let encoded = serde_json::to_vec(profile).expect("runtime profile fingerprint serializes");
    format!("{:x}", Sha256::digest(encoded))
}

pub(crate) fn runtime_profile_config_revision(fingerprint: &str) -> u64 {
    let mut bytes = [0u8; 8];
    for (index, pair) in fingerprint.as_bytes().chunks_exact(2).take(8).enumerate() {
        let text = std::str::from_utf8(pair).expect("fingerprint is ASCII");
        bytes[index] = u8::from_str_radix(text, 16).expect("fingerprint is hexadecimal");
    }
    u64::from_be_bytes(bytes)
}

pub(super) fn ensure_gateway_runtime_binding(
    state: &StateRuntime,
    thread_id: &str,
    agent: &GatewayAgentBindingSnapshot,
    profile: &RuntimeProfileConfig,
    revision: u64,
    fingerprint: &str,
) -> psychevo_runtime::Result<GatewayRuntimeBindingRecord> {
    if let Some(existing) = state.store().gateway_runtime_binding(thread_id)? {
        if existing.status != GatewayRuntimeBindingStatus::Resolved {
            return Err(agent_session_configuration_error(
                existing.unresolved_reason.unwrap_or_else(|| {
                    "This thread has an unresolved runtime binding.".to_string()
                }),
            ));
        }
        if existing.agent_ref != agent.agent_ref
            || existing.agent_fingerprint.as_deref() != Some(agent.fingerprint.as_str())
            || existing.agent_definition_json.as_deref() != Some(agent.definition_json.as_str())
            || existing.runtime_ref.as_deref() != Some(profile.id.as_str())
            || existing.profile_fingerprint.as_deref() != Some(fingerprint)
        {
            return Err(agent_session_error(
                "immutable_binding",
                AgentErrorStage::Binding,
                "user_action",
                "not_delivered",
                "The thread's immutable Agent/Runtime binding differs; start a new thread.",
                Some(format!("agent-binding:{thread_id}")),
            ));
        }
        if existing.ownership == GatewayRuntimeBindingOwnership::ReadOnly {
            return Err(agent_session_error(
                "read_only_session",
                AgentErrorStage::Binding,
                "user_action",
                "not_delivered",
                "This Agent session is read-only.",
                Some(format!("agent-binding:{thread_id}")),
            ));
        }
        return Ok(existing);
    }
    let summary = state
        .store()
        .session_summary(thread_id)?
        .ok_or_else(|| Error::Message(format!("session not found: {thread_id}")))?;
    let backend_kind = match profile.runtime {
        RuntimeProfileKind::Native => "native",
        RuntimeProfileKind::Acp => "acp",
    };
    let native_session_id = (profile.runtime == RuntimeProfileKind::Native).then_some(thread_id);
    let profile_config_json = serde_json::to_string(profile)?;
    state
        .store()
        .create_gateway_runtime_binding(GatewayRuntimeBindingInput {
            thread_id,
            agent_ref: agent.agent_ref.as_deref(),
            agent_fingerprint: &agent.fingerprint,
            agent_definition_json: &agent.definition_json,
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

pub(crate) fn runtime_session_handle(
    runtime_ref: &str,
    cwd: &Path,
    native_session_id: &str,
) -> String {
    let canonical = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let digest = Sha256::digest(
        format!(
            "agent-session-v1\0{runtime_ref}\0{}\0{native_session_id}",
            psychevo_runtime::normalized_native_path(&canonical).display()
        )
        .as_bytes(),
    );
    format!("ags_{}", &format!("{digest:x}")[..24])
}
