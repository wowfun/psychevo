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
    ) -> BoxFuture<'static, psychevo_runtime::Result<RunResult>>;
}

#[derive(Clone)]
struct GatewayExternalAgentDelegate {
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
    ) -> BoxFuture<'static, psychevo_runtime::Result<ExternalAgentDelegateResult>> {
        let delegate = self.clone();
        Box::pin(async move { delegate.run_inner(request).await })
    }
}

impl GatewayExternalAgentDelegate {
    async fn run_inner(
        self,
        request: ExternalAgentDelegateRequest,
    ) -> psychevo_runtime::Result<ExternalAgentDelegateResult> {
        let peer = resolve_peer_delegate(&self.base_options, &request)?;
        let child_session_id = request.child_session_id.clone();
        let mut options = self.base_options.clone();
        options.session = Some(child_session_id.clone());
        options.continue_latest = false;
        options.prompt = request.prompt.clone();
        options.image_inputs = Vec::new();
        options.prompt_display = None;
        options.model = request.model.clone().or(options.model);
        options.runtime_ref = Some(request.backend_ref.clone());
        options.runtime_session_id = None;
        options.runtime_options = request.runtime_options.clone();
        options.agent = Some(request.agent_name.clone());
        options.external_agent_delegate = None;
        let (control_handle, control) = run_control();
        let abort_bridge =
            spawn_external_delegate_abort_bridge(request.abort.clone(), control_handle);
        let stream = self.stream.map(|stream| {
            let child_session_id = child_session_id.clone();
            Arc::new(move |event| {
                stream(RunStreamEvent::scoped(child_session_id.clone(), event));
            }) as RunStreamSink
        });
        let result = acp_peer::run_acp_peer_turn(
            peer,
            BackendTurnRequest {
                options,
                runtime_source: "agent".to_string(),
                continue_sources: vec!["agent".to_string()],
                stream,
                control: Some(control),
            },
            request.run_id,
        )
        .await;
        abort_bridge.abort();
        let result = result?;
        Ok(ExternalAgentDelegateResult {
            child_session_id,
            final_answer: result.run.final_answer,
            outcome: result.run.outcome,
        })
    }
}

fn spawn_external_delegate_abort_bridge(
    mut abort: AbortSignal,
    control: RunControlHandle,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        abort.wait_for_abort().await;
        control.abort();
    })
}

fn resolve_peer_delegate(
    options: &RunOptions,
    request: &ExternalAgentDelegateRequest,
) -> psychevo_runtime::Result<ResolvedPeerTurn> {
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
    if backend_ref.name != request.backend_ref {
        return Err(Error::Message(format!(
            "agent `{}` uses backend `{}` and cannot delegate to backend `{}`",
            agent.name, backend_ref.name, request.backend_ref
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
    })
}

#[derive(Debug)]
pub struct PsychevoRuntimeBackend;

impl GatewayBackend for PsychevoRuntimeBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Psychevo
    }

    fn run_turn(
        &self,
        request: BackendTurnRequest,
    ) -> BoxFuture<'static, psychevo_runtime::Result<RunResult>> {
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
