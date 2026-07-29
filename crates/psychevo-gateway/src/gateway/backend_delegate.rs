#[derive(Debug)]
pub struct GatewayTurnResult {
    pub thread: GatewayThread,
    pub turn: GatewayTurn,
    pub result: RunResult,
    pub committed_entries: Vec<TranscriptEntry>,
}

#[derive(Debug)]
pub struct GatewayShellResult {
    pub thread: GatewayThread,
    pub result: UserShellResult,
    pub committed_entries: Vec<TranscriptEntry>,
}

pub struct BackendTurnRequest {
    pub options: RunOptions,
    pub input: Vec<GatewayInputPart>,
    pub runtime_source: String,
    pub continue_sources: Vec<String>,
    pub stream: Option<RunStreamSink>,
    pub control: Option<RunControl>,
}

pub trait GatewayBackend: Send + Sync + fmt::Debug {
    fn kind(&self) -> BackendKind;
    fn run_turn(
        &self,
        request: BackendTurnRequest,
    ) -> BoxFuture<'static, psychevo::Result<RunResult>>;
}

#[derive(Clone)]
struct GatewayExternalAgentDelegate {
    gateway: Gateway,
    base_options: RunOptions,
    stream: Option<RunStreamSink>,
}

impl fmt::Debug for GatewayExternalAgentDelegate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayExternalAgentDelegate")
            .field("cwd", &self.base_options.cwd)
            .field("has_stream", &self.stream.is_some())
            .finish_non_exhaustive()
    }
}

impl ExternalAgentDelegate for GatewayExternalAgentDelegate {
    fn run(
        &self,
        request: ExternalAgentDelegateRequest,
    ) -> BoxFuture<'static, psychevo::Result<ExternalAgentDelegateResult>> {
        let delegate = self.clone();
        Box::pin(async move { delegate.run_inner(request).await })
    }
}

impl GatewayExternalAgentDelegate {
    async fn run_inner(
        self,
        request: ExternalAgentDelegateRequest,
    ) -> psychevo::Result<ExternalAgentDelegateResult> {
        let child_session_id = request.child_session_id.clone();
        let child_turn_id = request.run_id.clone();
        let mut options = self.base_options.clone();
        options.session = Some(child_session_id.clone());
        options.continue_latest = false;
        options.prompt = request.prompt.clone();
        options.image_inputs = Vec::new();
        options.prompt_display = None;
        options.model = request.model.clone();
        options.runtime_ref = Some(request.runtime_ref.clone());
        options.runtime_session_id = None;
        options.runtime_options = request.runtime_options.clone();
        options.agent = None;
        options.external_agent_delegate = None;
        let child = options
            .state

            .session_summary(&child_session_id)
            .await?
            .ok_or_else(|| Error::Message(format!("session not found: {child_session_id}")))?;
        if child.parent_session_id.as_deref() != Some(request.parent_session_id.as_str()) {
            return Err(agent_session_configuration_error(format!(
                "Runtime-backed child `{child_session_id}` is not owned by parent `{}`.",
                request.parent_session_id
            )));
        }
        let stream = self.stream.map(|stream| {
            let child_session_id = child_session_id.clone();
            let child_turn_id = child_turn_id.clone();
            Arc::new(move |event| {
                stream(RunStreamEvent::scoped_turn(
                    child_session_id.clone(),
                    child_turn_id.clone(),
                    event,
                ));
            }) as RunStreamSink
        });
        let gateway = self.gateway.clone();
        let result = async {
            let (profile_config, profile_revision, profile_fingerprint) =
                resolve_gateway_runtime_profile(&options).await?;
            if request
                .expected_runtime_profile_revision
                .is_some_and(|expected| expected != profile_revision)
            {
                return Err(agent_session_error(
                    "stale_profile_revision",
                    AgentErrorStage::Binding,
                    "user_action",
                    "not_delivered",
                    format!(
                        "Team member `{}` captured Runtime Profile `{}` revision {}, but the current revision is {}. Re-save or reactivate the Team before execution.",
                        request.agent_name,
                        profile_config.id,
                        request.expected_runtime_profile_revision.unwrap_or_default(),
                        profile_revision,
                    ),
                    Some(format!("agent-binding:{child_session_id}")),
                ));
            }
            if profile_config.runtime == RuntimeProfileKind::Native {
                return Err(agent_session_configuration_error(format!(
                    "Runtime Profile `{}` is native and cannot be executed by the external Team delegate.",
                    profile_config.id
                )));
            }
            let expected_backend = profile_config.backend_ref.as_deref().ok_or_else(|| {
                agent_session_configuration_error(format!(
                    "ACP Runtime Profile `{}` is missing backendRef.",
                    profile_config.id
                ))
            })?;
            if request.backend_ref.as_deref() != Some(expected_backend) {
                return Err(agent_session_configuration_error(format!(
                    "Agent Definition `{}` uses ACP backend `{}`, but Runtime Profile `{}` resolves to backend `{expected_backend}`.",
                    request.agent_name,
                    request.backend_ref.as_deref().unwrap_or("none"),
                    profile_config.id,
                )));
            }
            let _ = profile_fingerprint;
            options.agent = Some(request.agent_name.clone());
            options.runtime_ref = Some(profile_config.id);
            let mut turn_request =
                psychevo::TurnRequest::__from_run_options(options, "agent", stream);
            turn_request.__set_turn_id(child_turn_id.clone());
            turn_request.__set_agent_entrypoint(AgentEntrypoint::Subagent);
            let thread = gateway
                .framework_client()?
                .resume_thread(&child_session_id)
                .await?;
            let handle = thread.start_turn(turn_request).await?;
            let mut abort = request.abort.clone();
            let completed = tokio::select! {
                completed = handle.wait() => completed,
                _ = abort.wait_for_abort() => {
                    handle.interrupt();
                    handle.wait().await
                }
            };
            completed.map(|turn| ExternalAgentDelegateResult {
                child_session_id: child_session_id.clone(),
                final_answer: turn.final_answer,
                outcome: match turn.outcome {
                    psychevo::TurnOutcome::Completed => Outcome::Normal,
                    psychevo::TurnOutcome::Stopped => Outcome::Stopped,
                    psychevo::TurnOutcome::Failed => Outcome::Failed,
                    psychevo::TurnOutcome::Interrupted => Outcome::Aborted,
                },
            })
        }
        .await;
        gateway
            .state
            .set_agent_edge_status(&child_session_id, psychevo::__product::persistence::AgentEdgeStatus::Closed)
            .await?;
        result
    }
}

#[cfg(test)]
fn resolve_peer_delegate(
    options: &RunOptions,
    request: &ExternalAgentDelegateRequest,
    profile_fingerprint: &str,
) -> psychevo::Result<ResolvedPeerTurn> {
    if options.no_agents {
        return Err(Error::Message("agent delegation is disabled".to_string()));
    }
    let env = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| std::env::vars().collect());
    let agents_home = resolve_skills_home(&env, &options.cwd)?;
    let catalog = discover_agents(&AgentDiscoveryOptions {
        home: agents_home.clone(),
        cwd: options.cwd.clone(),
        env: env.clone(),
        explicit_inputs: vec![request.agent_name.clone()],
        no_agents: false,
    })?;
    let agent = resolve_agent_definition(&catalog, &request.agent_name, &options.cwd, &env)?;
    let Some(backend_ref) = agent.backend.as_ref() else {
        return Err(Error::Message(format!(
            "agent `{}` is not backed by an ACP backend",
            agent.name
        )));
    };
    let requested_backend = request.backend_ref.as_deref().ok_or_else(|| {
        Error::Message(format!(
            "ACP Runtime Profile `{}` has no Agent Definition backend identity",
            request.runtime_ref
        ))
    })?;
    if backend_ref.name != requested_backend {
        return Err(Error::Message(format!(
            "agent `{}` uses backend `{}` and cannot delegate to backend `{}`",
            agent.name, backend_ref.name, requested_backend
        )));
    }
    if !agent.supports_entrypoint(AgentEntrypoint::Subagent) {
        return Err(Error::Message(format!(
            "agent `{}` references backend `{}` but does not support the subagent entrypoint",
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
    Ok(ResolvedPeerTurn {
        agent,
        backend,
        env,
        process_scope_fingerprint: Some(profile_fingerprint.to_string()),
    })
}

#[derive(Debug)]
pub struct PsychevoRuntimeBackend;

impl GatewayBackend for PsychevoRuntimeBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Native
    }

    fn run_turn(
        &self,
        request: BackendTurnRequest,
    ) -> BoxFuture<'static, psychevo::Result<RunResult>> {
        Box::pin(async move {
            let continue_sources = request
                .continue_sources
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            match (request.stream, request.control) {
                (Some(stream), Some(control)) => {
                    run_live_streaming_controlled(
                        request.options,
                        &request.runtime_source,
                        &continue_sources,
                        stream,
                        control,
                    )
                    .await
                }
                (Some(stream), None) => {
                    run_live_streaming(
                        request.options,
                        &request.runtime_source,
                        &continue_sources,
                        stream,
                    )
                    .await
                }
                (None, Some(control)) => {
                    let stream: RunStreamSink = Arc::new(|_| {});
                    run_live_streaming_controlled(
                        request.options,
                        &request.runtime_source,
                        &continue_sources,
                        stream,
                        control,
                    )
                    .await
                }
                (None, None)
                    if request.runtime_source == "run"
                        && continue_sources.len() == 1
                        && continue_sources[0] == "run" =>
                {
                    run_live(request.options).await
                }
                (None, None) => {
                    let stream: RunStreamSink = Arc::new(|_| {});
                    run_live_streaming(
                        request.options,
                        &request.runtime_source,
                        &continue_sources,
                        stream,
                    )
                    .await
                }
            }
        })
    }
}
