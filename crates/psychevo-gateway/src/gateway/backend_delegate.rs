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
    ) -> BoxFuture<'static, psychevo_runtime::Result<RunResult>>;
}

#[derive(Clone)]
struct GatewayExternalAgentDelegate {
    gateway: Gateway,
    base_options: RunOptions,
    stream: Option<RunStreamSink>,
    event_sink: Option<GatewayEventSink>,
}

impl fmt::Debug for GatewayExternalAgentDelegate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayExternalAgentDelegate")
            .field("cwd", &self.base_options.cwd)
            .field("has_stream", &self.stream.is_some())
            .field("has_event_sink", &self.event_sink.is_some())
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
        let child_session_id = request.child_session_id.clone();
        let terminal_child_session_id = child_session_id.clone();
        let child_turn_id = request.run_id.clone();
        let terminal_gateway = self.gateway.clone();
        let terminal_event_sink = self.event_sink.clone();
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
            .store()
            .session_summary(&child_session_id)?
            .ok_or_else(|| Error::Message(format!("session not found: {child_session_id}")))?;
        if child.parent_session_id.as_deref() != Some(request.parent_session_id.as_str()) {
            return Err(agent_session_configuration_error(format!(
                "Runtime-backed child `{child_session_id}` is not owned by parent `{}`.",
                request.parent_session_id
            )));
        }
        let (control_handle, control) = run_control();
        let child_activity = self.gateway.claim_durable_gateway_activity(
            DurableGatewayActivityClaim {
                activity_id: &child_turn_id,
                thread_id: Some(&child_session_id),
                source_key: None,
                turn_id: Some(&child_turn_id),
                kind: "turn",
                owner_surface: Some("agent"),
                queued_turns: 0,
                intent: Some(json!({
                    "kind": "delegated_agent_turn",
                    "threadId": child_session_id,
                    "parentThreadId": request.parent_session_id,
                })),
            },
        )?;
        let child_heartbeat = self
            .gateway
            .spawn_durable_activity_heartbeat(child_activity.clone());
        self.gateway.register_active(
            &thread_key(&child_session_id),
            child_turn_id.clone(),
            Some(control_handle.clone()),
            ActiveActivityKind::Turn,
        );
        let abort_bridge =
            spawn_external_delegate_abort_bridge(request.abort.clone(), control_handle);
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
        let result = async move {
            let (profile_config, profile_revision, profile_fingerprint) =
                resolve_gateway_runtime_profile(&options)?;
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
            match profile_config.runtime {
                RuntimeProfileKind::Acp => {
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
                    options.agent = Some(request.agent_name.clone());
                    let existing_binding = options
                        .state
                        .store()
                        .gateway_runtime_binding(&child_session_id)?;
                    let agent_binding = resolve_gateway_agent_binding_snapshot(
                        &options,
                        &profile_config,
                        existing_binding.as_ref(),
                        AgentEntrypoint::Subagent,
                    )?;
                    let binding = ensure_gateway_runtime_binding(
                        &options.state,
                        &child_session_id,
                        &agent_binding,
                        &profile_config,
                        profile_revision,
                        &profile_fingerprint,
                    )?;
                    options.runtime_ref = Some(expected_backend.to_string());
                    let peer = resolve_peer_delegate(&options, &request, &profile_fingerprint)?;
                    let captured_binding = binding.clone();
                    let session_ready =
                        acp_session_ready_for_binding(options.state.clone(), binding);
                    self.gateway
                        .run_internal_agent_turn(
                            Some(captured_binding),
                            profile_config,
                            Some(peer),
                        BackendTurnRequest {
                            options,
                            input: Vec::new(),
                            runtime_source: "agent".to_string(),
                            continue_sources: vec!["agent".to_string()],
                            stream,
                            control: Some(control),
                        },
                        request.run_id,
                        Some(session_ready),
                    )
                    .await
                    .map(|run| ExternalAgentDelegateResult {
                        child_session_id,
                        final_answer: run.final_answer,
                        outcome: run.outcome,
                    })
                }
                RuntimeProfileKind::Native => Err(agent_session_configuration_error(format!(
                    "Runtime Profile `{}` is native and cannot be executed by the external Team delegate.",
                    profile_config.id
                ))),
            }
        }
        .await;
        abort_bridge.abort();
        let _ = child_heartbeat.send(());
        terminal_gateway
            .state
            .store()
            .set_agent_edge_status(
                &terminal_child_session_id,
                psychevo_runtime::AgentEdgeStatus::Closed,
            )?;
        match &result {
            Ok(result) => terminal_gateway.record_external_delegate_terminal(
                &terminal_child_session_id,
                &child_turn_id,
                result,
                Some(&child_activity),
                terminal_event_sink.as_ref(),
            )?,
            Err(error) => terminal_gateway.ensure_failed_terminal_after_turn_error(
                &child_turn_id,
                Some(&terminal_child_session_id),
                terminal_event_sink.as_ref(),
                error,
            )?,
        }
        result
    }
}

impl Gateway {
    fn record_external_delegate_terminal(
        &self,
        child_session_id: &str,
        turn_id: &str,
        result: &ExternalAgentDelegateResult,
        durable_activity: Option<&DurableGatewayActivity>,
        event_sink: Option<&GatewayEventSink>,
    ) -> psychevo_runtime::Result<()> {
        if let Some(terminal) = self.state.store().gateway_turn_terminal(turn_id)? {
            self.mark_active_turn_terminal(turn_id);
            self.finish_durable_gateway_activity(durable_activity, &terminal.status);
            return Ok(());
        }
        let status = gateway_turn_status_for_outcome(result.outcome);
        let error_message = match result.outcome {
            Outcome::Normal => None,
            Outcome::Failed => Some("The delegated runtime turn failed."),
            Outcome::Stopped | Outcome::Aborted => {
                Some("The delegated runtime turn was interrupted.")
            }
        };
        let turn = self.record_and_project_terminal_turn(TerminalTurnInput {
            thread_id: Some(child_session_id),
            turn_id,
            status,
            outcome: Some(result.outcome.as_str()),
            error_message,
            error_data: None,
            classified_error: None,
            first_committed_seq: None,
            last_committed_seq: None,
            durable_activity,
        })?;
        let committed_entries = self.project_terminal_entry_for_turn(turn_id);
        self.finish_durable_gateway_activity(
            durable_activity,
            durable_activity_status_for_turn(status),
        );
        if let Some(event_sink) = event_sink {
            event_sink(GatewayEvent::TurnCompleted {
                thread_id: Some(child_session_id.to_string()),
                turn_id: turn_id.to_string(),
                turn,
                committed_entries,
            });
        }
        Ok(())
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
    profile_fingerprint: &str,
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
