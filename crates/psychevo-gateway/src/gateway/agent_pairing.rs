const DIRECT_AGENT_BINDING_METADATA_KEY: &str = "runtimeAgentDefinition";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BoundDirectAgentSelection {
    Default,
    Named(String),
}

#[derive(Debug, Clone)]
struct DirectAgentPairing {
    agent_name: Option<String>,
    instructions: Option<String>,
    fingerprint: String,
}

fn resolve_direct_agent_pairing(
    options: &RunOptions,
) -> psychevo_runtime::Result<DirectAgentPairing> {
    let Some(agent_input) = options
        .agent
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(direct_agent_pairing(None, None));
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
        explicit_inputs: vec![agent_input.to_string()],
        no_agents: false,
    })?;
    let agent = resolve_agent_definition(&catalog, agent_input, &options.cwd, &env)?;
    if let Some(backend) = agent.backend.as_ref() {
        return Err(runtime_host_configuration_error(format!(
            "Agent Definition `{}` belongs to ACP backend `{}` and cannot be paired with a direct Runtime Profile.",
            agent.name, backend.name
        )));
    }

    let labels = direct_required_contribution_labels(&agent);
    if !labels.is_empty() {
        return Err(runtime_host_configuration_error(format!(
            "Agent Definition `{}` requires {} that the direct Runtime Profile cannot faithfully inject. Mark {} optional or choose Native.",
            agent.name,
            join_contribution_labels(&labels),
            join_contribution_labels(&labels),
        )));
    }

    let instructions =
        (!agent.instructions.trim().is_empty()).then(|| agent.instructions.trim().to_string());
    Ok(direct_agent_pairing(Some(agent.name), instructions))
}

fn resolve_direct_agent_pairing_for_thread(
    state: &StateRuntime,
    thread_id: &str,
    options: &RunOptions,
) -> psychevo_runtime::Result<DirectAgentPairing> {
    let metadata = state.store().session_metadata(thread_id)?;
    if let Some(existing) = metadata
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get(DIRECT_AGENT_BINDING_METADATA_KEY))
    {
        let stored_agent_name = existing
            .get("agentName")
            .and_then(Value::as_str)
            .map(str::to_string);
        let instructions = existing
            .get("instructions")
            .and_then(Value::as_str)
            .map(str::to_string);
        let stored_fingerprint = existing
            .get("fingerprint")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| bound_agent_snapshot_error(thread_id))?;
        let captured = direct_agent_pairing(stored_agent_name.clone(), instructions);
        if captured.fingerprint != stored_fingerprint {
            return Err(bound_agent_snapshot_error(thread_id));
        }
        let requested_agent_name = options
            .agent
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if requested_agent_name.is_some()
            && requested_agent_name != stored_agent_name.as_deref()
        {
            return Err(immutable_agent_binding_error(
                stored_agent_name.as_deref(),
                requested_agent_name,
            ));
        }
        return Ok(captured);
    }

    let pairing = resolve_direct_agent_pairing(options)?;
    ensure_direct_agent_pairing_binding(state, thread_id, &pairing)?;
    Ok(pairing)
}

pub(crate) fn bound_direct_agent_selection(
    thread_id: &str,
    metadata: Option<&Value>,
) -> psychevo_runtime::Result<Option<BoundDirectAgentSelection>> {
    let Some(existing) = metadata
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get(DIRECT_AGENT_BINDING_METADATA_KEY))
    else {
        return Ok(None);
    };
    let agent_name = existing
        .get("agentName")
        .and_then(Value::as_str)
        .map(str::to_string);
    let instructions = existing
        .get("instructions")
        .and_then(Value::as_str)
        .map(str::to_string);
    let fingerprint = existing
        .get("fingerprint")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| bound_agent_snapshot_error(thread_id))?;
    let pairing = direct_agent_pairing(agent_name.clone(), instructions);
    if pairing.fingerprint != fingerprint {
        return Err(bound_agent_snapshot_error(thread_id));
    }
    Ok(Some(match agent_name {
        Some(agent_name) => BoundDirectAgentSelection::Named(agent_name),
        None => BoundDirectAgentSelection::Default,
    }))
}

pub(crate) fn direct_required_contribution_labels(
    agent: &psychevo_runtime::AgentDefinition,
) -> Vec<&'static str> {
    direct_uninjectable_contributions(agent)
        .into_iter()
        .filter(|contribution| !agent.contribution_is_optional(*contribution))
        .map(direct_contribution_label)
        .collect()
}

fn direct_agent_pairing(
    agent_name: Option<String>,
    instructions: Option<String>,
) -> DirectAgentPairing {
    let encoded = serde_json::to_vec(&json!({
        "agentName": agent_name,
        "instructions": instructions,
    }))
    .expect("direct Agent Definition binding serializes");
    let fingerprint = format!("{:x}", Sha256::digest(encoded));
    DirectAgentPairing {
        agent_name,
        instructions,
        fingerprint,
    }
}

fn ensure_direct_agent_pairing_binding(
    state: &StateRuntime,
    thread_id: &str,
    pairing: &DirectAgentPairing,
) -> psychevo_runtime::Result<()> {
    let metadata = state.store().session_metadata(thread_id)?;
    if let Some(existing) = metadata
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get(DIRECT_AGENT_BINDING_METADATA_KEY))
    {
        let existing_fingerprint = existing
            .get("fingerprint")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if existing_fingerprint != pairing.fingerprint {
            return Err(immutable_agent_binding_error(
                existing.get("agentName").and_then(Value::as_str),
                pairing.agent_name.as_deref(),
            ));
        }
        return Ok(());
    }

    state.store().set_session_metadata_field(
        thread_id,
        DIRECT_AGENT_BINDING_METADATA_KEY,
        Some(json!({
            "agentName": pairing.agent_name,
            "instructions": pairing.instructions,
            "fingerprint": pairing.fingerprint,
        })),
    )
}

fn immutable_agent_binding_error(
    bound_agent: Option<&str>,
    requested_agent: Option<&str>,
) -> Error {
    let bound_agent = bound_agent.unwrap_or("Default Agent");
    let requested_agent = requested_agent.unwrap_or("Default Agent");
    runtime_host_error(RuntimeError::new(
        "immutable_agent_binding",
        RuntimeErrorStage::Binding,
        RetryClass::UserAction,
        format!(
            "This direct runtime thread is bound to Agent Definition `{bound_agent}`. Start a new thread to use `{requested_agent}`."
        ),
    ))
}

fn bound_agent_snapshot_error(thread_id: &str) -> Error {
    runtime_host_error(RuntimeError::new(
        "bound_agent_snapshot_invalid",
        RuntimeErrorStage::Binding,
        RetryClass::Never,
        format!(
            "Thread `{thread_id}` is missing a valid immutable Agent Definition snapshot; it cannot safely continue."
        ),
    ))
}

fn direct_uninjectable_contributions(
    agent: &psychevo_runtime::AgentDefinition,
) -> Vec<AgentContribution> {
    let mut contributions = Vec::new();
    if agent.tool_policy.allowed.is_some()
        || !agent.tool_policy.denied.is_empty()
        || agent.tool_policy.allowed_agents.is_some()
        || !agent.tool_policy.denied_agents.is_empty()
    {
        contributions.push(AgentContribution::Tools);
    }
    if !agent.tool_policy.mcp_servers.is_empty() {
        contributions.push(AgentContribution::Mcp);
    }
    if !agent.skills.is_empty() {
        contributions.push(AgentContribution::Skills);
    }
    contributions
}

fn direct_contribution_label(contribution: AgentContribution) -> &'static str {
    match contribution {
        AgentContribution::Instructions => "instructions",
        AgentContribution::Tools => "tool policy",
        AgentContribution::Mcp => "MCP servers",
        AgentContribution::Skills => "skills",
    }
}

fn join_contribution_labels(labels: &[&str]) -> String {
    match labels {
        [] => String::new(),
        [label] => (*label).to_string(),
        [head @ .., tail] => format!("{} and {tail}", head.join(", ")),
    }
}
