impl PsychevoAcpAgent {
    pub(crate) fn new(options: AcpOptions) -> psychevo_runtime::Result<Self> {
        let state = StateRuntime::open(&options.db_path)?;
        let gateway = Gateway::new(state.clone());
        Ok(Self {
            options,
            state,
            gateway,
            sessions: Arc::default(),
            client_terminal_auth: Arc::new(Mutex::new(false)),
            client_terminal_output: Arc::new(Mutex::new(false)),
        })
    }

    fn gateway_source(&self, session_id: &SessionId, session: &AcpSession) -> GatewaySource {
        GatewaySource::new("acp", session_id.to_string())
            .persistent()
            .with_visible_name(format!("ACP {session_id}"))
            .with_raw_identity(json!({
                "kind": "acp",
                "session_id": session_id.to_string(),
                "cwd": session.cwd.display().to_string(),
            }))
    }

    fn gateway_selector(&self, session_id: &SessionId) -> GatewayThreadSelector {
        GatewayThreadSelector::source(
            GatewaySource::new("acp", session_id.to_string())
                .persistent()
                .source_key(),
        )
    }

    pub(crate) async fn serve(
        self: Arc<Self>,
        transport: impl ConnectTo<Agent> + 'static,
    ) -> Result<(), Error> {
        let agent = self;
        Agent
            .v2()
            .name("psychevo-acp")
            .on_receive_request(
                {
                    let agent = Arc::clone(&agent);
                    async move |request: InitializeRequest, responder, _cx| {
                        responder.respond_with_result(agent.initialize(request).await)
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let agent = Arc::clone(&agent);
                    async move |request: LoginAuthRequest, responder, _cx| {
                        responder.respond_with_result(agent.authenticate(request).await)
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let agent = Arc::clone(&agent);
                    async move |request: NewSessionRequest, responder, cx: ConnectionTo<Client>| {
                        let result = agent.new_session(request).await;
                        let setup = result.as_ref().ok().map(|response| {
                            (response.session_id.clone(), response.config_options.clone())
                        });
                        let response = responder.respond_with_result(result);
                        if response.is_ok()
                            && let Some((session_id, config_options)) = setup
                        {
                            let commands = agent.available_commands_for_session(&session_id);
                            send_session_setup_updates(&cx, session_id, config_options, commands);
                        }
                        response
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let agent = Arc::clone(&agent);
                    async move |request: ResumeSessionRequest,
                                responder,
                                cx: ConnectionTo<Client>| {
                        let session_id = request.session_id.clone();
                        let result = agent.resume_session(request).await;
                        let config_options = result
                            .as_ref()
                            .ok()
                            .map(|response| response.config_options.clone())
                            .unwrap_or_default();
                        let response = responder.respond_with_result(result);
                        if response.is_ok() {
                            let commands = agent.available_commands_for_session(&session_id);
                            send_session_setup_updates(&cx, session_id, config_options, commands);
                        }
                        response
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let agent = Arc::clone(&agent);
                    async move |request: ListSessionsRequest, responder, _cx| {
                        responder.respond_with_result(agent.list_sessions(request).await)
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let agent = Arc::clone(&agent);
                    async move |request: CloseSessionRequest, responder, _cx| {
                        responder.respond_with_result(agent.close_session(request).await)
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let agent = Arc::clone(&agent);
                    async move |request: PromptRequest, responder, cx: ConnectionTo<Client>| {
                        let agent = Arc::clone(&agent);
                        let prompt_cx = cx.clone();
                        cx.spawn(async move {
                            if let Err(err) = responder
                                .respond_with_result(agent.prompt(request, prompt_cx).await)
                            {
                                eprintln!("psychevo-acp prompt response failed: {err}");
                            }
                            Ok(())
                        })?;
                        Ok(())
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_notification(
                {
                    let agent = Arc::clone(&agent);
                    async move |notification: CancelSessionNotification, _cx| {
                        agent.cancel(notification).await;
                        Ok(())
                    }
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .on_receive_request(
                {
                    let agent = Arc::clone(&agent);
                    async move |request: SetSessionConfigOptionRequest,
                                responder,
                                cx: ConnectionTo<Client>| {
                        responder
                            .respond_with_result(agent.set_session_config_option(request, cx).await)
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .connect_to(transport)
            .await
    }

    pub(crate) async fn initialize(
        &self,
        request: InitializeRequest,
    ) -> Result<InitializeResponse, Error> {
        let terminal_auth = request
            .capabilities
            .auth
            .as_ref()
            .and_then(|auth| auth.terminal.as_ref())
            .is_some();
        let terminal_output = self.client_terminal_output_enabled(&request.capabilities);
        if let Ok(mut value) = self.client_terminal_auth.lock() {
            *value = terminal_auth;
        }
        if let Ok(mut value) = self.client_terminal_output.lock() {
            *value = terminal_output;
        }
        let auth_methods = self.auth_methods(terminal_auth);
        let capabilities = AgentCapabilities::new()
            .session(
                SessionCapabilities::new()
                    .prompt(
                        PromptCapabilities::new()
                            .embedded_context(PromptEmbeddedContextCapabilities::new())
                            .image(PromptImageCapabilities::new()),
                    )
                    .mcp(McpCapabilities::new().http(McpHttpCapabilities::new())),
            )
            .auth(AgentAuthCapabilities::new());
        Ok(InitializeResponse::new(
            ProtocolVersion::V2,
            Implementation::new("psychevo-acp", env!("CARGO_PKG_VERSION")).title("Psychevo"),
        )
        .capabilities(capabilities)
        .auth_methods(auth_methods))
    }

    pub(crate) async fn authenticate(
        &self,
        request: LoginAuthRequest,
    ) -> Result<LoginAuthResponse, Error> {
        let method = request.method_id.to_string();
        let ready = self.ready_auth_provider();
        if ready
            .as_ref()
            .is_some_and(|provider| provider.eq_ignore_ascii_case(&method))
            || (method == TERMINAL_SETUP_AUTH_METHOD_ID && ready.is_some())
        {
            return Ok(LoginAuthResponse::new());
        }
        Err(Error::invalid_params().data(format!("unsupported auth method: {method}")))
    }

    pub(crate) async fn new_session(
        &self,
        request: NewSessionRequest,
    ) -> Result<NewSessionResponse, Error> {
        if self.ready_auth_provider().is_none() && !self.terminal_auth_available() {
            return Err(Error::auth_required().data("provider credentials are not configured"));
        }
        let session_id = SessionId::new(format!("acp-{}", Uuid::now_v7()));
        let mcp_servers = acp_mcp_servers(request.mcp_servers);
        let session = AcpSession::new(request.cwd, None, mcp_servers);
        let config_options = self.session_config_options_for_session(&session);
        self.sessions
            .lock()
            .expect("acp session lock poisoned")
            .insert(session_id.to_string(), session);
        Ok(NewSessionResponse::new(session_id).config_options(config_options))
    }

    pub(crate) async fn resume_session(
        &self,
        request: ResumeSessionRequest,
    ) -> Result<ResumeSessionResponse, Error> {
        let runtime_session_id = request.session_id.to_string();
        let store = self.state.store().clone();
        store
            .resume_session(&runtime_session_id)
            .map_err(|_| Error::resource_not_found(Some(runtime_session_id.clone())))?;
        let session = AcpSession::new(
            request.cwd,
            Some(runtime_session_id),
            acp_mcp_servers(request.mcp_servers),
        );
        let config_options = self.session_config_options_for_session(&session);
        self.sessions
            .lock()
            .expect("acp session lock poisoned")
            .insert(request.session_id.to_string(), session);
        Ok(ResumeSessionResponse::new().config_options(config_options))
    }

    pub(crate) async fn list_sessions(
        &self,
        request: ListSessionsRequest,
    ) -> Result<ListSessionsResponse, Error> {
        let cwd = request
            .cwd
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let store = self.state.store().clone();
        let sessions = store
            .list_sessions_for_cwd_with_sources(&cwd, &[])
            .map_err(acp_internal_error)?
            .into_iter()
            .map(|summary| {
                SessionInfo::new(summary.id, PathBuf::from(summary.cwd))
                    .title(summary.title)
                    .updated_at(Some(summary.updated_at_ms.to_string()))
            })
            .collect();
        Ok(ListSessionsResponse::new(sessions))
    }

    pub(crate) async fn close_session(
        &self,
        request: CloseSessionRequest,
    ) -> Result<CloseSessionResponse, Error> {
        let selector = self.gateway_selector(&request.session_id);
        let interrupted = self.gateway.interrupt_turn(selector.clone());
        self.gateway.clear_queue(selector);
        if let Some(session) = self
            .sessions
            .lock()
            .expect("acp session lock poisoned")
            .remove(&request.session_id.to_string())
            && let Some(control) = session.control
            && !interrupted
        {
            control.abort();
        }
        Ok(CloseSessionResponse::new())
    }

    pub(crate) async fn prompt(
        &self,
        request: PromptRequest,
        cx: ConnectionTo<Client>,
    ) -> Result<PromptResponse, Error> {
        let session_id = request.session_id.clone();
        send_session_update(
            &cx,
            session_id.clone(),
            SessionUpdate::StateUpdate(StateUpdate::Running(RunningStateUpdate::new())),
        );
        let session_key = session_id.to_string();
        let prompt_blocks = request.prompt;
        let slash_prompt = single_text_prompt(&prompt_blocks).map(str::to_string);
        let session = {
            let sessions = self.sessions.lock().expect("acp session lock poisoned");
            let Some(session) = sessions.get(&session_key) else {
                return Err(Error::resource_not_found(Some(session_key)));
            };
            session.clone()
        };
        let (prompt, image_inputs) = prompt_parts(prompt_blocks, &session.cwd);

        if let Some(slash_prompt) = slash_prompt {
            match self
                .handle_slash_prompt(&session_id, &session, &slash_prompt, &cx)
                .await?
            {
                SlashPromptAction::Handled(response) => {
                    send_session_update(
                        &cx,
                        session_id,
                        SessionUpdate::StateUpdate(StateUpdate::Idle(
                            IdleStateUpdate::new().stop_reason(StopReason::EndTurn),
                        )),
                    );
                    return Ok(response);
                }
                SlashPromptAction::RunPrompt(prompt) => {
                    return self
                        .run_prompt_and_drain(session_id, prompt, Vec::new(), cx)
                        .await;
                }
                SlashPromptAction::NotSlashOrPassThrough => {}
            }
        }

        self.run_prompt_and_drain(session_id, prompt, image_inputs, cx)
            .await
    }

    pub(crate) async fn run_prompt_and_drain(
        &self,
        session_id: SessionId,
        prompt: String,
        image_inputs: Vec<ImageInput>,
        cx: ConnectionTo<Client>,
    ) -> Result<PromptResponse, Error> {
        let usage = Arc::new(Mutex::new(AcpUsageAccumulator::default()));
        let mut reason = self
            .run_prompt_once(
                session_id.clone(),
                prompt,
                image_inputs,
                cx.clone(),
                Arc::clone(&usage),
            )
            .await?;
        while let Some(prompt) = self.pop_queued_prompt(&session_id) {
            reason = self
                .run_prompt_once(
                    session_id.clone(),
                    prompt,
                    Vec::new(),
                    cx.clone(),
                    Arc::clone(&usage),
                )
                .await?;
        }
        let mut idle = IdleStateUpdate::new().stop_reason(reason);
        // Accounting is diagnostic metadata; it must not block the required
        // idle state update after the runtime turn has completed.
        if let Ok(usage) = usage.try_lock() {
            let usage = usage.clone();
            if let Some(metrics) = usage.to_usage() {
                idle = idle.usage(metrics);
            }
            if let Some(meta) = usage.response_meta() {
                idle = idle.meta(meta);
            }
        }
        send_session_update(
            &cx,
            session_id,
            SessionUpdate::StateUpdate(StateUpdate::Idle(idle)),
        );
        Ok(PromptResponse::new())
    }

    pub(crate) async fn run_prompt_once(
        &self,
        session_id: SessionId,
        prompt: String,
        image_inputs: Vec<ImageInput>,
        cx: ConnectionTo<Client>,
        usage: Arc<Mutex<AcpUsageAccumulator>>,
    ) -> Result<StopReason, Error> {
        let session_key = session_id.to_string();
        let (handle, control) = run_control();
        let session = {
            let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
            let Some(session) = sessions.get_mut(&session_key) else {
                return Err(Error::resource_not_found(Some(session_key)));
            };
            if session.control.is_some() {
                return Err(Error::invalid_params().data("session already has an active prompt"));
            }
            session.control = Some(handle.clone());
            session.clone()
        };
        send_session_update(
            &cx,
            session_id.clone(),
            SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(
                self.available_commands_for_session(&session_id),
            )),
        );
        let approval_handler = Arc::new(AcpApprovalHandler {
            session_id: session_id.clone(),
            cx: cx.clone(),
        });
        let stream_usage = Arc::clone(&usage);
        let stream = Arc::new(move |event| {
            if let Ok(mut usage) = stream_usage.lock() {
                usage.record_stream_event(&event);
            }
        });
        let event_session_id = session_id.clone();
        let event_cx = cx.clone();
        let event_projection = Arc::new(Mutex::new(AcpLiveProjection::new(
            self.terminal_output_available(),
        )));
        let event_sink = Arc::new(move |event| {
            if let Ok(mut projection) = event_projection.lock() {
                send_gateway_event_update(&event_cx, &event_session_id, event, &mut projection);
            }
        });
        let mut request =
            self.thread_turn_request(&session, prompt, image_inputs, Some(approval_handler));
        let source = self.gateway_source(&session_id, &session);
        request.source = Some(source);
        request.runtime_source = Some("acp".to_string());
        request.continue_sources = vec!["acp".to_string(), "run".to_string(), "tui".to_string()];
        request.stream = Some(stream);
        request.event_sink = Some(event_sink);
        request.control_handle = Some(handle);
        request.control = Some(control);
        let result = self.gateway.run_turn(request).await.map(|turn| turn.result);
        match result {
            Ok(result) => {
                if !result.final_answer.trim().is_empty() {
                    send_session_update(
                        &cx,
                        session_id.clone(),
                        agent_message_update(&session_id, result.final_answer),
                    );
                }
                if let Ok(mut usage) = usage.lock() {
                    for warning in &result.warnings {
                        usage.add_warning(warning.message.clone());
                    }
                }
                self.send_usage_update_from_context(
                    &cx,
                    session_id.clone(),
                    AcpUsageUpdateContext {
                        snapshot: result.context_snapshot.as_ref(),
                        context_limit: result.context_limit,
                        provider: &result.provider,
                        model: &result.model,
                        usage: &usage,
                    },
                );
                if let Ok(mut sessions) = self.sessions.lock()
                    && let Some(session) = sessions.get_mut(&session_id.to_string())
                {
                    session.runtime_session_id = Some(result.session_id);
                    session.control = None;
                }
                send_session_update(
                    &cx,
                    session_id.clone(),
                    SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(
                        self.available_commands_for_session(&session_id),
                    )),
                );
                Ok(stop_reason(result.outcome))
            }
            Err(err) => {
                if let Ok(mut sessions) = self.sessions.lock()
                    && let Some(session) = sessions.get_mut(&session_id.to_string())
                {
                    session.control = None;
                }
                send_session_update(
                    &cx,
                    session_id.clone(),
                    SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(
                        self.available_commands_for_session(&session_id),
                    )),
                );
                Err(acp_internal_error(err))
            }
        }
    }

    pub(crate) async fn cancel(&self, notification: CancelSessionNotification) {
        let selector = self.gateway_selector(&notification.session_id);
        let interrupted = self.gateway.interrupt_turn(selector.clone());
        self.gateway.clear_queue(selector);
        let control = self
            .sessions
            .lock()
            .expect("acp session lock poisoned")
            .get_mut(&notification.session_id.to_string())
            .and_then(|session| {
                session.queued_prompts.clear();
                session.pending_steers.clear();
                session.control.clone()
            });
        if !interrupted && let Some(control) = control {
            control.abort();
        }
    }

    pub(crate) async fn set_session_config_option(
        &self,
        request: SetSessionConfigOptionRequest,
        cx: ConnectionTo<Client>,
    ) -> Result<SetSessionConfigOptionResponse, Error> {
        let value = request
            .value
            .as_id()
            .map(ToString::to_string)
            .or_else(|| request.value.as_bool().map(|value| value.to_string()))
            .unwrap_or_default();
        let updated_session = {
            let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
            let Some(session) = sessions.get_mut(&request.session_id.to_string()) else {
                return Err(Error::resource_not_found(Some(
                    request.session_id.to_string(),
                )));
            };
            match request.config_id.to_string().as_str() {
                "mode" => {
                    let mode = RunMode::parse(&value)
                        .ok_or_else(|| Error::invalid_params().data("unsupported mode"))?;
                    session.mode = mode;
                    if mode == RunMode::Plan {
                        session.permission_mode = None;
                    }
                }
                "model" => {
                    if !value.trim().is_empty() {
                        session.model = Some(value);
                    }
                }
                "effort" => {
                    if REASONING_EFFORT_VALUES.contains(&value.as_str()) {
                        session.reasoning_effort = reasoning_effort_value(&value);
                    } else {
                        return Err(Error::invalid_params().data("unsupported reasoning effort"));
                    }
                }
                id => return Err(Error::invalid_params().data(format!("unsupported config: {id}"))),
            }
            session.clone()
        };
        let options = self.session_config_options_for_session(&updated_session);
        send_session_update(
            &cx,
            request.session_id,
            SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(options.clone())),
        );
        Ok(SetSessionConfigOptionResponse::new(options))
    }

    pub(crate) fn session_config_options_for_session(
        &self,
        session: &AcpSession,
    ) -> Vec<SessionConfigOption> {
        let options = self.run_options(session, String::new(), Vec::new(), None);
        let configured = configured_models(&options).unwrap_or_default();
        session_config_options(
            session.mode,
            session.model.as_deref(),
            session.reasoning_effort.as_deref(),
            &configured,
        )
    }
}
