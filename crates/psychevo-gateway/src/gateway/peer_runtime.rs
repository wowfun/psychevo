#[derive(Debug)]
pub(crate) struct ResolvedPeerTurn {
    pub(crate) agent: psychevo_runtime::AgentDefinition,
    pub(crate) backend: psychevo_runtime::AgentBackendConfig,
    pub(crate) env: BTreeMap<String, String>,
}

pub(crate) fn resolve_peer_turn(
    options: &RunOptions,
) -> psychevo_runtime::Result<Option<ResolvedPeerTurn>> {
    if options.no_agents {
        return Ok(None);
    }
    let native_runtime_requested = options
        .runtime_ref
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| value == "native");
    let runtime_ref = options
        .runtime_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "native");
    let agent_input = options.agent.as_ref();
    if runtime_ref.is_none() && agent_input.is_none() {
        return Ok(None);
    }
    let env = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| std::env::vars().collect());
    let agents_home = resolve_skills_home(&env, &options.cwd)?;
    let explicit_inputs = match (agent_input, runtime_ref) {
        (Some(agent), Some(runtime)) if agent != runtime => {
            vec![agent.clone(), runtime.to_string()]
        }
        (Some(agent), _) => vec![agent.clone()],
        (None, Some(runtime)) => vec![runtime.to_string()],
        (None, None) => Vec::new(),
    };
    let catalog = discover_agents(&AgentDiscoveryOptions {
        home: agents_home.clone(),
        cwd: options.cwd.clone(),
        env: env.clone(),
        explicit_inputs,
        no_agents: false,
    })?;
    let agent = match (agent_input, runtime_ref) {
        (Some(agent_input), _) => {
            resolve_agent_definition(&catalog, agent_input, &options.cwd, &env)?
        }
        (None, Some(runtime)) => {
            resolve_agent_definition(&catalog, runtime, &options.cwd, &env)?
        }
        (None, None) => return Ok(None),
    };
    let Some(backend_ref) = agent.backend.as_ref() else {
        if let Some(runtime) = runtime_ref {
            return Err(Error::Message(format!(
                "agent `{}` cannot run on runtime `{runtime}`; ACP peer runtimes run their own modes, not Psychevo agent definitions",
                agent.name
            )));
        }
        return Ok(None);
    };
    if native_runtime_requested {
        return Err(Error::Message(format!(
            "agent `{}` is backed by ACP runtime `{}` and cannot run on native runtime",
            agent.name, backend_ref.name
        )));
    }
    if let Some(runtime) = runtime_ref
        && backend_ref.name != runtime
    {
        return Err(Error::Message(format!(
            "agent `{}` uses backend `{}` and cannot run on runtime `{runtime}`",
            agent.name, backend_ref.name
        )));
    }
    if !agent.supports_entrypoint(AgentEntrypoint::Peer) {
        return Err(Error::Message(format!(
            "agent `{}` references backend `{}` but does not support the peer entrypoint",
            agent.name, backend_ref.name
        )));
    }
    let backends = load_agent_backend_configs(&agents_home, &options.cwd, &env)?;
    let backend = backends
        .get(&backend_ref.name)
        .cloned()
        .ok_or_else(|| Error::Message(format!("unknown agent backend: {}", backend_ref.name)))?;
    if !backend.enabled {
        return Err(Error::Message(format!(
            "agent backend `{}` is disabled",
            backend.id
        )));
    }
    if backend
        .command
        .as_deref()
        .is_none_or(|command| command.trim().is_empty())
    {
        return Err(Error::Message(format!(
            "agent backend `{}` is missing command",
            backend.id
        )));
    }
    Ok(Some(ResolvedPeerTurn {
        agent,
        backend,
        env,
    }))
}

fn clear_acp_peer_usage_update(
    state: &StateRuntime,
    session_id: &str,
) -> psychevo_runtime::Result<()> {
    let Some(metadata) = state.store().session_metadata(session_id)? else {
        return Ok(());
    };
    let Some(peer) = metadata.get(ACP_PEER_METADATA_KEY) else {
        return Ok(());
    };
    let Some(mut peer) = peer.as_object().cloned() else {
        return Ok(());
    };
    if peer.remove("usageUpdate").is_none() {
        return Ok(());
    }
    let value = (!peer.is_empty()).then_some(Value::Object(peer));
    state
        .store()
        .set_session_metadata_field(session_id, ACP_PEER_METADATA_KEY, value)
}
