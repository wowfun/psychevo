use std::collections::BTreeMap;
use std::path::Path;

use psychevo::{
    Error,
    agents::{AgentDiscoveryOptions, AgentEntrypoint, discover_agents, resolve_agent_definition},
    config::load_agent_backend_configs,
    skills::resolve_skills_home,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct PeerResolutionContext<'a> {
    pub(crate) cwd: &'a Path,
    pub(crate) base_env: &'a BTreeMap<String, String>,
    pub(crate) runtime_ref: Option<&'a str>,
    pub(crate) agent_ref: Option<&'a str>,
    pub(crate) no_agents: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedPeerTurn {
    pub(crate) agent: psychevo::agents::AgentDefinition,
    pub(crate) backend: psychevo::agents::AgentBackendConfig,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) process_scope_fingerprint: Option<String>,
}

pub(crate) fn resolve_peer_turn(
    context: PeerResolutionContext<'_>,
) -> psychevo::Result<Option<ResolvedPeerTurn>> {
    if context.no_agents {
        return Ok(None);
    }
    let native_runtime_requested = context
        .runtime_ref
        .map(str::trim)
        .is_some_and(|value| value == "native");
    let runtime_ref = context
        .runtime_ref
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "native");
    let agent_input = context.agent_ref;
    if runtime_ref.is_none() && agent_input.is_none() {
        return Ok(None);
    }
    let env = context.base_env.clone();
    let agents_home = resolve_skills_home(&env, context.cwd)?;
    let explicit_inputs = match (agent_input, runtime_ref) {
        (Some(agent), Some(runtime)) if agent != runtime => {
            vec![agent.to_string(), runtime.to_string()]
        }
        (Some(agent), _) => vec![agent.to_string()],
        (None, Some(runtime)) => vec![runtime.to_string()],
        (None, None) => Vec::new(),
    };
    let catalog = discover_agents(&AgentDiscoveryOptions {
        home: agents_home.clone(),
        cwd: context.cwd.to_path_buf(),
        env: env.clone(),
        explicit_inputs,
        no_agents: false,
    })?;
    let agent = match (agent_input, runtime_ref) {
        (Some(agent_input), _) => {
            resolve_agent_definition(&catalog, agent_input, context.cwd, &env)?
        }
        (None, Some(runtime)) => resolve_agent_definition(&catalog, runtime, context.cwd, &env)?,
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
    let backends = load_agent_backend_configs(&agents_home, context.cwd, &env)?;
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
        process_scope_fingerprint: None,
    }))
}
