use std::collections::BTreeMap;
use std::path::Path;

use psychevo::{
    agents::AgentDefinition, agents::AgentDiscoveryOptions, agents::AgentEntrypoint,
    agents::discover_agents, agents::resolve_agent_definition, config::RuntimeProfileConfig,
    config::RuntimeProfileKind, config::load_agent_backend_configs,
    config::load_runtime_profile_configs, skills::resolve_skills_home,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::agent_session::{
    AgentErrorStage, agent_session_configuration_error, agent_session_error,
};
use super::peer_runtime::ResolvedPeerTurn;

#[derive(Debug, Clone)]
pub(crate) struct GatewayAgentBindingSnapshot {
    pub(super) agent_ref: Option<String>,
    pub(super) fingerprint: String,
    pub(super) definition_json: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedGatewayAgentTurn {
    pub(crate) profile: RuntimeProfileConfig,
    pub(crate) profile_revision: u64,
    pub(crate) profile_fingerprint: String,
    pub(crate) peer: Option<ResolvedPeerTurn>,
    pub(crate) agent: GatewayAgentBindingSnapshot,
}

pub(crate) struct GatewayAgentTurnPreparation<'a> {
    pub(crate) thread: &'a psychevo::ThreadExecutionContext,
    pub(crate) binding: Option<&'a psychevo::AgentBindingSnapshot>,
    pub(crate) target: &'a psychevo::AgentTargetSelection,
    pub(crate) inherited_env: &'a BTreeMap<String, String>,
    pub(crate) purpose: psychevo::AgentTurnPurpose,
}

impl PreparedGatewayAgentTurn {
    pub(crate) fn initial_binding(
        &self,
        thread_id: &str,
    ) -> psychevo::Result<psychevo::InitialAgentBinding> {
        let backend_kind = match self.profile.runtime {
            RuntimeProfileKind::Native => "native",
            RuntimeProfileKind::Acp => "acp",
        };
        Ok(psychevo::InitialAgentBinding {
            agent_ref: self.agent.agent_ref.clone(),
            agent_fingerprint: self.agent.fingerprint.clone(),
            agent_definition_json: self.agent.definition_json.clone(),
            runtime_ref: self.profile.id.clone(),
            backend_kind: backend_kind.to_string(),
            native_kind: self.profile.runtime.as_str().to_string(),
            native_session_id: (self.profile.runtime == RuntimeProfileKind::Native)
                .then(|| thread_id.to_string()),
            profile_fingerprint: self.profile_fingerprint.clone(),
            profile_revision: self.profile_revision.to_string(),
            profile_config_json: serde_json::to_string(&self.profile)?,
            adapter_kind: self.profile.runtime.as_str().to_string(),
            adapter_revision: env!("CARGO_PKG_VERSION").to_string(),
        })
    }
}

pub(crate) fn prepare_framework_gateway_agent_turn(
    request: GatewayAgentTurnPreparation<'_>,
) -> psychevo::Result<(PreparedGatewayAgentTurn, bool)> {
    if let Some(binding) = request.binding {
        if let Some(requested_runtime_ref) = request
            .target
            .runtime_profile_ref
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            && requested_runtime_ref != binding.runtime_ref
        {
            return Err(agent_session_error(
                "immutable_binding",
                AgentErrorStage::Binding,
                "user_action",
                "not_delivered",
                format!(
                    "Thread `{}` is bound to Runtime Profile `{}`; start a new thread to use `{requested_runtime_ref}`.",
                    binding.thread_id, binding.runtime_ref,
                ),
                Some(format!("agent-binding:{}", binding.thread_id)),
            ));
        }
        let requested_agent_ref = request
            .target
            .agent_ref
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if requested_agent_ref != binding.agent_ref.as_deref() && requested_agent_ref.is_some() {
            return Err(agent_session_error(
                "immutable_binding",
                AgentErrorStage::Binding,
                "user_action",
                "not_delivered",
                format!(
                    "Thread `{}` is bound to Agent target `{}`; start a new Thread to use `{}`.",
                    binding.thread_id,
                    binding.agent_ref.as_deref().unwrap_or("Default Agent"),
                    requested_agent_ref.unwrap_or("Default Agent"),
                ),
                Some(format!("agent-binding:{}", binding.thread_id)),
            ));
        }
        validate_bound_child_target_against_current_profile(
            &request.thread.id,
            Path::new(&request.thread.cwd),
            request.inherited_env,
            request.purpose,
            request.target,
            &binding.runtime_ref,
        )?;
        let profile: RuntimeProfileConfig = serde_json::from_str(&binding.profile_config_json)
            .map_err(|error| {
                agent_session_error(
                    "bound_profile_snapshot_invalid",
                    AgentErrorStage::Binding,
                    "never",
                    "not_delivered",
                    format!("Bound Runtime Profile snapshot could not be decoded: {error}"),
                    Some(format!("agent-binding:{}", binding.thread_id)),
                )
            })?;
        if profile.id != binding.runtime_ref
            || runtime_profile_config_fingerprint(&profile) != binding.profile_fingerprint
        {
            return Err(agent_session_error(
                "bound_profile_snapshot_mismatch",
                AgentErrorStage::Binding,
                "never",
                "not_delivered",
                "Bound Runtime Profile snapshot does not match its immutable identity.",
                Some(format!("agent-binding:{}", binding.thread_id)),
            ));
        }
        let revision = binding
            .profile_revision
            .parse::<u64>()
            .unwrap_or_else(|_| runtime_profile_config_revision(&binding.profile_fingerprint));
        validate_captured_child_target(
            &request.thread.id,
            request.purpose,
            request.target,
            &profile,
            revision,
        )?;
        let peer = resolve_captured_agent_peer_at(CapturedAgentPeerInput {
            cwd: Path::new(&request.thread.cwd),
            env: request.inherited_env,
            thread_id: &binding.thread_id,
            agent_ref: binding.agent_ref.as_deref(),
            encoded: &binding.agent_definition_json,
            fingerprint: &binding.agent_fingerprint,
            profile: &profile,
            profile_fingerprint: &binding.profile_fingerprint,
        })?;
        return Ok((
            PreparedGatewayAgentTurn {
                profile,
                profile_revision: revision,
                profile_fingerprint: binding.profile_fingerprint.clone(),
                peer,
                agent: GatewayAgentBindingSnapshot {
                    agent_ref: binding.agent_ref.clone(),
                    fingerprint: binding.agent_fingerprint.clone(),
                    definition_json: binding.agent_definition_json.clone(),
                },
            },
            false,
        ));
    }

    let cwd = Path::new(&request.thread.cwd);
    let (profile, profile_revision, profile_fingerprint) = resolve_gateway_runtime_profile_at(
        cwd,
        request.inherited_env,
        request.target.runtime_profile_ref.as_deref(),
    )?;
    validate_captured_child_target(
        &request.thread.id,
        request.purpose,
        request.target,
        &profile,
        profile_revision,
    )?;
    let agent = resolve_gateway_agent_binding_snapshot_at(
        cwd,
        request.inherited_env,
        request.target.agent_ref.as_deref(),
        &profile,
        match request.purpose {
            psychevo::AgentTurnPurpose::Peer => AgentEntrypoint::Peer,
            psychevo::AgentTurnPurpose::Child => AgentEntrypoint::Subagent,
        },
    )?;
    let peer = resolve_captured_agent_peer_at(CapturedAgentPeerInput {
        cwd,
        env: request.inherited_env,
        thread_id: &request.thread.id,
        agent_ref: agent.agent_ref.as_deref(),
        encoded: &agent.definition_json,
        fingerprint: &agent.fingerprint,
        profile: &profile,
        profile_fingerprint: &profile_fingerprint,
    })?;
    Ok((
        PreparedGatewayAgentTurn {
            profile,
            profile_revision,
            profile_fingerprint,
            peer,
            agent,
        },
        true,
    ))
}

fn validate_bound_child_target_against_current_profile(
    thread_id: &str,
    cwd: &Path,
    inherited_env: &BTreeMap<String, String>,
    purpose: psychevo::AgentTurnPurpose,
    target: &psychevo::AgentTargetSelection,
    bound_runtime_ref: &str,
) -> psychevo::Result<()> {
    if purpose != psychevo::AgentTurnPurpose::Child
        || (target.expected_profile_revision.is_none() && target.expected_backend_ref.is_none())
    {
        return Ok(());
    }
    let (current_profile, current_revision, _) =
        resolve_gateway_runtime_profile_at(cwd, inherited_env, Some(bound_runtime_ref))?;
    validate_captured_child_target(
        thread_id,
        purpose,
        target,
        &current_profile,
        current_revision,
    )
}

fn validate_captured_child_target(
    thread_id: &str,
    purpose: psychevo::AgentTurnPurpose,
    target: &psychevo::AgentTargetSelection,
    profile: &RuntimeProfileConfig,
    profile_revision: u64,
) -> psychevo::Result<()> {
    if target
        .expected_profile_revision
        .is_some_and(|expected| expected != profile_revision)
    {
        return Err(agent_session_error(
            "stale_profile_revision",
            AgentErrorStage::Binding,
            "user_action",
            "not_delivered",
            format!(
                "Team member `{}` captured Runtime Profile `{}` revision {}, but the current revision is {}. Re-save or reactivate the Team before execution.",
                target.agent_ref.as_deref().unwrap_or("Default Agent"),
                profile.id,
                target.expected_profile_revision.unwrap_or_default(),
                profile_revision,
            ),
            Some(format!("agent-binding:{thread_id}")),
        ));
    }
    if purpose == psychevo::AgentTurnPurpose::Child
        && target.expected_backend_ref.is_some()
        && target.expected_backend_ref.as_deref() != profile.backend_ref.as_deref()
    {
        return Err(agent_session_error(
            "captured_backend_mismatch",
            AgentErrorStage::Binding,
            "user_action",
            "not_delivered",
            format!(
                "Team member `{}` captured backend `{}`, but Runtime Profile `{}` resolves to backend `{}`. Re-save or reactivate the Team before execution.",
                target.agent_ref.as_deref().unwrap_or("Default Agent"),
                target.expected_backend_ref.as_deref().unwrap_or("none"),
                profile.id,
                profile.backend_ref.as_deref().unwrap_or("none"),
            ),
            Some(format!("agent-binding:{thread_id}")),
        ));
    }
    Ok(())
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

pub(crate) fn resolve_gateway_agent_binding_snapshot_at(
    cwd: &Path,
    env: &BTreeMap<String, String>,
    requested_agent_ref: Option<&str>,
    profile: &RuntimeProfileConfig,
    entrypoint: AgentEntrypoint,
) -> psychevo::Result<GatewayAgentBindingSnapshot> {
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
    let home = resolve_skills_home(env, cwd)?;
    let catalog = discover_agents(&AgentDiscoveryOptions {
        home,
        cwd: cwd.to_path_buf(),
        env: env.clone(),
        explicit_inputs: vec![agent_ref.to_string()],
        no_agents: false,
    })?;
    let agent = resolve_agent_definition(&catalog, agent_ref, cwd, env)?;
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

pub(crate) struct CapturedAgentPeerInput<'a> {
    pub(crate) cwd: &'a Path,
    pub(crate) env: &'a BTreeMap<String, String>,
    pub(crate) thread_id: &'a str,
    pub(crate) agent_ref: Option<&'a str>,
    pub(crate) encoded: &'a str,
    pub(crate) fingerprint: &'a str,
    pub(crate) profile: &'a RuntimeProfileConfig,
    pub(crate) profile_fingerprint: &'a str,
}

pub(crate) fn resolve_captured_agent_peer_at(
    input: CapturedAgentPeerInput<'_>,
) -> psychevo::Result<Option<ResolvedPeerTurn>> {
    let CapturedAgentPeerInput {
        cwd,
        env,
        thread_id,
        agent_ref,
        encoded,
        fingerprint,
        profile,
        profile_fingerprint,
    } = input;
    if profile.runtime == RuntimeProfileKind::Native {
        return Ok(None);
    }
    let agent_ref = agent_ref.ok_or_else(|| {
        agent_session_error(
            "bound_agent_snapshot_missing",
            AgentErrorStage::Binding,
            "never",
            "not_delivered",
            "A bound ACP Thread is missing its captured Agent identity.",
            Some(format!("agent-binding:{thread_id}")),
        )
    })?;
    if gateway_agent_definition_fingerprint(encoded) != fingerprint {
        return Err(agent_session_error(
            "bound_agent_snapshot_mismatch",
            AgentErrorStage::Binding,
            "never",
            "not_delivered",
            "Bound Agent Definition snapshot does not match its immutable fingerprint.",
            Some(format!("agent-binding:{thread_id}")),
        ));
    }
    let agent: AgentDefinition = serde_json::from_str(encoded).map_err(|error| {
        agent_session_error(
            "bound_agent_snapshot_invalid",
            AgentErrorStage::Binding,
            "never",
            "not_delivered",
            format!("Bound Agent Definition snapshot could not be decoded: {error}"),
            Some(format!("agent-binding:{thread_id}")),
        )
    })?;
    if agent.name != agent_ref {
        return Err(agent_session_error(
            "bound_agent_snapshot_mismatch",
            AgentErrorStage::Binding,
            "never",
            "not_delivered",
            "Bound Agent Definition snapshot does not match its immutable identity.",
            Some(format!("agent-binding:{thread_id}")),
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
            Some(format!("agent-binding:{thread_id}")),
        ));
    }
    let home = resolve_skills_home(env, cwd)?;
    let backends = load_agent_backend_configs(&home, cwd, env)?;
    let backend = backends.get(backend_ref).cloned().ok_or_else(|| {
        agent_session_error(
            "runtime_unavailable",
            AgentErrorStage::Configuration,
            "user_action",
            "not_delivered",
            format!("Captured ACP backend `{backend_ref}` is unavailable."),
            Some(format!("agent-binding:{thread_id}")),
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
            Some(format!("agent-binding:{thread_id}")),
        ));
    }
    Ok(Some(ResolvedPeerTurn {
        agent,
        backend,
        env: env.clone(),
        process_scope_fingerprint: Some(profile_fingerprint.to_string()),
    }))
}

pub(crate) fn resolve_gateway_runtime_profile_at(
    cwd: &Path,
    env: &BTreeMap<String, String>,
    requested_runtime_ref: Option<&str>,
) -> psychevo::Result<(RuntimeProfileConfig, u64, String)> {
    let runtime_ref = requested_runtime_ref
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("native");
    let home = resolve_skills_home(env, cwd)?;
    let mut profiles = load_runtime_profile_configs(&home, cwd, env)?;
    for profile in generated_gateway_runtime_profiles() {
        profiles.entry(profile.id.clone()).or_insert(profile);
    }
    if !profiles.contains_key(runtime_ref) && runtime_ref != "native" {
        let backend_id = runtime_ref.strip_prefix("acp:").unwrap_or(runtime_ref);
        let backends = load_agent_backend_configs(&home, cwd, env)?;
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
        let backends = load_agent_backend_configs(&home, cwd, env)?;
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

pub(crate) fn runtime_session_handle(
    runtime_ref: &str,
    cwd: &Path,
    native_session_id: &str,
) -> String {
    let canonical = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let digest = Sha256::digest(
        format!(
            "agent-session-v1\0{runtime_ref}\0{}\0{native_session_id}",
            psychevo::host_paths::normalized_native_path(&canonical).display()
        )
        .as_bytes(),
    );
    format!("ags_{}", &format!("{digest:x}")[..24])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acp_profile(backend_ref: &str) -> RuntimeProfileConfig {
        RuntimeProfileConfig {
            id: "team-profile".to_string(),
            runtime: RuntimeProfileKind::Acp,
            enabled: true,
            label: "Team Profile".to_string(),
            backend_ref: Some(backend_ref.to_string()),
            default_model: None,
            default_mode: None,
            default_agent: None,
            sandbox: None,
            workspace_roots: Vec::new(),
            options: Value::Null,
        }
    }

    fn child_target() -> psychevo::AgentTargetSelection {
        psychevo::AgentTargetSelection {
            agent_ref: Some("researcher".to_string()),
            runtime_profile_ref: Some("team-profile".to_string()),
            runtime_options: BTreeMap::new(),
            preparation: None,
            expected_profile_revision: Some(41),
            expected_backend_ref: Some("captured-backend".to_string()),
        }
    }

    #[test]
    fn child_profile_revision_is_revalidated_before_delivery() {
        let error = validate_captured_child_target(
            "child-thread",
            psychevo::AgentTurnPurpose::Child,
            &child_target(),
            &acp_profile("captured-backend"),
            42,
        )
        .expect_err("stale captured revision must fail before Adapter preparation");
        let data = error.structured_data().expect("structured binding error");
        assert_eq!(data["code"], "stale_profile_revision");
        assert_eq!(data["delivery"], "not_delivered");
        assert_eq!(data["diagnosticRef"], "agent-binding:child-thread");
    }

    #[test]
    fn child_backend_is_revalidated_before_delivery() {
        let error = validate_captured_child_target(
            "child-thread",
            psychevo::AgentTurnPurpose::Child,
            &child_target(),
            &acp_profile("current-backend"),
            41,
        )
        .expect_err("changed captured backend must fail before Adapter preparation");
        let data = error.structured_data().expect("structured binding error");
        assert_eq!(data["code"], "captured_backend_mismatch");
        assert_eq!(data["delivery"], "not_delivered");
    }

    #[test]
    fn ordinary_peer_without_child_backend_guard_is_unchanged() {
        let mut target = child_target();
        target.expected_profile_revision = None;
        target.expected_backend_ref = None;
        validate_captured_child_target(
            "peer-thread",
            psychevo::AgentTurnPurpose::Peer,
            &target,
            &acp_profile("current-backend"),
            99,
        )
        .expect("ordinary peer selection has no Team capture guard");
    }

    #[test]
    fn ordinary_child_without_team_backend_capture_is_unchanged() {
        let mut target = child_target();
        target.expected_profile_revision = None;
        target.expected_backend_ref = None;
        validate_captured_child_target(
            "ordinary-child",
            psychevo::AgentTurnPurpose::Child,
            &target,
            &acp_profile("current-backend"),
            99,
        )
        .expect("ordinary child selection has no Team capture guard");
    }

    #[test]
    fn bound_team_child_revalidates_the_current_profile_configuration() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let cwd = temp.path().join("workspace");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let env = BTreeMap::from([("PSYCHEVO_HOME".to_string(), home.display().to_string())]);
        let write_profile = |backend_ref: &str| {
            std::fs::write(
                home.join("config.toml"),
                format!(
                    r#"[agents.backends.{backend_ref}]
kind = "acp"
command = "/bin/true"
entrypoints = ["peer", "subagent"]

[runtime_profiles.team-profile]
runtime = "acp"
enabled = true
label = "Team Profile"
backend_ref = "{backend_ref}"
"#,
                ),
            )
            .expect("profile config");
        };
        write_profile("captured-backend");
        let (captured_profile, captured_revision, captured_fingerprint) =
            resolve_gateway_runtime_profile_at(&cwd, &env, Some("team-profile"))
                .expect("captured profile");
        let mut target = child_target();
        target.expected_profile_revision = Some(captured_revision);
        let thread = psychevo::ThreadExecutionContext {
            id: "bound-child".to_string(),
            cwd: cwd.display().to_string(),
            source: "agent".to_string(),
            source_key: None,
        };
        let definition_json = r#"{"name":"researcher"}"#;
        let binding = psychevo::AgentBindingSnapshot {
            thread_id: thread.id.clone(),
            agent_ref: Some("researcher".to_string()),
            agent_fingerprint: gateway_agent_definition_fingerprint(definition_json),
            agent_definition_json: definition_json.to_string(),
            runtime_ref: "team-profile".to_string(),
            backend_kind: "acp".to_string(),
            native_kind: "acp".to_string(),
            native_session_id: None,
            cwd: thread.cwd.clone(),
            profile_fingerprint: captured_fingerprint,
            profile_revision: captured_revision.to_string(),
            profile_config_json: serde_json::to_string(&captured_profile)
                .expect("captured profile JSON"),
            adapter_kind: "acp".to_string(),
            adapter_revision: "test".to_string(),
            binding_revision: 1,
            control_revision: 1,
        };

        write_profile("current-backend");
        let (_, current_revision, _) =
            resolve_gateway_runtime_profile_at(&cwd, &env, Some("team-profile"))
                .expect("current profile");
        assert_ne!(captured_revision, current_revision);
        let stale = prepare_framework_gateway_agent_turn(GatewayAgentTurnPreparation {
            thread: &thread,
            binding: Some(&binding),
            target: &target,
            inherited_env: &env,
            purpose: psychevo::AgentTurnPurpose::Child,
        })
        .expect_err("bound Team child cannot validate against its stale snapshot");
        assert_eq!(
            stale.structured_data().expect("structured stale error")["code"],
            "stale_profile_revision"
        );

        target.expected_profile_revision = Some(current_revision);
        let backend = prepare_framework_gateway_agent_turn(GatewayAgentTurnPreparation {
            thread: &thread,
            binding: Some(&binding),
            target: &target,
            inherited_env: &env,
            purpose: psychevo::AgentTurnPurpose::Child,
        })
        .expect_err("bound Team child must compare the captured backend to current config");
        assert_eq!(
            backend.structured_data().expect("structured backend error")["code"],
            "captured_backend_mismatch"
        );
    }
}
