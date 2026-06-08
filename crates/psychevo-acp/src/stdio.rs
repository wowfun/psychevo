#[allow(unused_imports)]
pub(crate) use super::*;

#[derive(Debug, Clone)]
pub struct AcpOptions {
    pub home: PathBuf,
    pub db_path: PathBuf,
    pub config_path: Option<PathBuf>,
    pub inherited_env: BTreeMap<String, String>,
}

impl AcpOptions {
    pub fn from_env() -> Self {
        let inherited_env = std::env::vars().collect::<BTreeMap<_, _>>();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let home = env_path_or_default(&inherited_env, "PSYCHEVO_HOME", "~/.psychevo", &cwd);
        let db_path = env_path_or_default(
            &inherited_env,
            "PSYCHEVO_DB",
            &home.join("state.db").to_string_lossy(),
            &cwd,
        );
        let config_path = inherited_env
            .get("PSYCHEVO_CONFIG")
            .filter(|value| !value.trim().is_empty())
            .map(|value| resolve_path(value, &inherited_env, &cwd));
        Self {
            home,
            db_path,
            config_path,
            inherited_env,
        }
    }
}

pub async fn run_stdio(options: AcpOptions) -> std::io::Result<()> {
    let _ = std::fs::create_dir_all(&options.home);
    let agent = Arc::new(
        PsychevoAcpAgent::new(options)
            .map_err(|err| std::io::Error::other(format!("state DB error: {err}")))?,
    );
    let stdin = tokio::io::stdin().compat();
    let stdout = tokio::io::stdout().compat_write();
    agent
        .serve(ByteStreams::new(stdout, stdin))
        .await
        .map_err(|err| std::io::Error::other(format!("ACP error: {err}")))
}

pub(crate) struct PsychevoAcpAgent {
    pub(crate) options: AcpOptions,
    pub(crate) state: StateRuntime,
    pub(crate) gateway: Gateway,
    pub(crate) sessions: Arc<Mutex<HashMap<String, AcpSession>>>,
    pub(crate) client_terminal_auth: Arc<Mutex<bool>>,
    pub(crate) client_terminal_output: Arc<Mutex<bool>>,
}

#[derive(Debug, Clone)]
pub(crate) struct AcpSession {
    pub(crate) cwd: PathBuf,
    pub(crate) runtime_session_id: Option<String>,
    pub(crate) mode: RunMode,
    pub(crate) permission_mode: Option<PermissionMode>,
    pub(crate) model: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) mcp_servers: Vec<McpServerInput>,
    pub(crate) control: Option<RunControlHandle>,
    pub(crate) queued_prompts: VecDeque<String>,
    pub(crate) pending_steers: Vec<psychevo_runtime::PendingInputId>,
    pub(crate) last_session_list: Vec<SessionSummary>,
}

impl AcpSession {
    pub(crate) fn new(
        cwd: PathBuf,
        runtime_session_id: Option<String>,
        mcp_servers: Vec<McpServerInput>,
    ) -> Self {
        Self {
            cwd,
            runtime_session_id,
            mode: RunMode::Default,
            permission_mode: None,
            model: None,
            reasoning_effort: None,
            mcp_servers,
            control: None,
            queued_prompts: VecDeque::new(),
            pending_steers: Vec::new(),
            last_session_list: Vec::new(),
        }
    }
}

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
            .builder()
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
                    async move |request: AuthenticateRequest, responder, _cx| {
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
                        let setup = result
                            .as_ref()
                            .ok()
                            .map(|response| (response.session_id.clone(), RunMode::Default));
                        let response = responder.respond_with_result(result);
                        if response.is_ok()
                            && let Some((session_id, mode)) = setup
                        {
                            let commands = agent.available_commands_for_session(&session_id);
                            send_session_setup_updates(&cx, session_id, mode, commands);
                        }
                        response
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let agent = Arc::clone(&agent);
                    async move |request: LoadSessionRequest, responder, cx: ConnectionTo<Client>| {
                        let session_id = request.session_id.clone();
                        let result = agent.load_session(request).await;
                        let should_send_setup = result.is_ok();
                        let response = responder.respond_with_result(result);
                        if response.is_ok() && should_send_setup {
                            let commands = agent.available_commands_for_session(&session_id);
                            send_session_setup_updates(&cx, session_id, RunMode::Default, commands);
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
                            let _ = responder
                                .respond_with_result(agent.prompt(request, prompt_cx).await);
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
                    async move |notification: CancelNotification, _cx| {
                        agent.cancel(notification).await;
                        Ok(())
                    }
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .on_receive_request(
                {
                    let agent = Arc::clone(&agent);
                    async move |request: SetSessionModeRequest,
                                responder,
                                cx: ConnectionTo<Client>| {
                        responder.respond_with_result(agent.set_session_mode(request, cx).await)
                    }
                },
                agent_client_protocol::on_receive_request!(),
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
        let terminal_auth = request.client_capabilities.auth.terminal;
        let terminal_output = self.client_terminal_output_enabled(&request.client_capabilities);
        if let Ok(mut value) = self.client_terminal_auth.lock() {
            *value = terminal_auth;
        }
        if let Ok(mut value) = self.client_terminal_output.lock() {
            *value = terminal_output;
        }
        let auth_methods = self.auth_methods(terminal_auth);
        let mut capabilities = AgentCapabilities::new()
            .load_session(true)
            .prompt_capabilities(PromptCapabilities::new().embedded_context(true).image(true))
            .mcp_capabilities(McpCapabilities::new().http(true))
            .auth(AgentAuthCapabilities::new());
        capabilities.session_capabilities = SessionCapabilities::new()
            .close(SessionCloseCapabilities::new())
            .list(SessionListCapabilities::new());
        Ok(InitializeResponse::new(ProtocolVersion::V1)
            .agent_capabilities(capabilities)
            .agent_info(
                Implementation::new("psychevo-acp", env!("CARGO_PKG_VERSION")).title("Psychevo"),
            )
            .auth_methods(auth_methods))
    }

    pub(crate) async fn authenticate(
        &self,
        request: AuthenticateRequest,
    ) -> Result<AuthenticateResponse, Error> {
        let method = request.method_id.to_string();
        let ready = self.ready_auth_provider();
        if ready
            .as_ref()
            .is_some_and(|provider| provider.eq_ignore_ascii_case(&method))
            || (method == TERMINAL_SETUP_AUTH_METHOD_ID && ready.is_some())
        {
            return Ok(AuthenticateResponse::new());
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
        self.sessions
            .lock()
            .expect("acp session lock poisoned")
            .insert(session_id.to_string(), session);
        Ok(NewSessionResponse::new(session_id).modes(mode_state(RunMode::Default)))
    }

    pub(crate) async fn load_session(
        &self,
        request: LoadSessionRequest,
    ) -> Result<LoadSessionResponse, Error> {
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
        self.sessions
            .lock()
            .expect("acp session lock poisoned")
            .insert(request.session_id.to_string(), session);
        Ok(LoadSessionResponse::new().modes(mode_state(RunMode::Default)))
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
            .list_sessions_for_workdir_with_sources(&cwd, &[])
            .map_err(acp_internal_error)?
            .into_iter()
            .map(|summary| {
                SessionInfo::new(summary.id, PathBuf::from(summary.workdir))
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
                SlashPromptAction::Handled(response) => return Ok(response),
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
        let usage = usage.lock().expect("acp usage lock poisoned").clone();
        let mut response = PromptResponse::new(reason);
        if let Some(metrics) = usage.to_usage() {
            response = response.usage(metrics);
        }
        if let Some(meta) = usage.response_meta() {
            response = response.meta(meta);
        }
        Ok(response)
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
        let options = self.run_options(&session, prompt, image_inputs, Some(approval_handler));
        let source = self.gateway_source(&session_id, &session);
        let result = self
            .gateway
            .send_turn(SendTurnRequest {
                thread_id: session.runtime_session_id.clone(),
                source: Some(source),
                bind_source: None,
                reset_source_binding: false,
                input: Vec::new(),
                options,
                runtime_source: Some("acp".to_string()),
                continue_sources: vec!["acp".to_string(), "run".to_string(), "tui".to_string()],
                stream: Some(stream),
                event_sink: Some(event_sink),
                control_handle: Some(handle),
                control: Some(control),
                lineage: None,
            })
            .await
            .map(|turn| turn.result);
        match result {
            Ok(result) => {
                if !result.final_answer.trim().is_empty() {
                    send_session_update(
                        &cx,
                        session_id.clone(),
                        SessionUpdate::AgentMessageChunk(ContentChunk::new(
                            result.final_answer.into(),
                        )),
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
                    result.context_snapshot.as_ref(),
                    &usage,
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

    pub(crate) async fn cancel(&self, notification: CancelNotification) {
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

    pub(crate) async fn set_session_mode(
        &self,
        request: SetSessionModeRequest,
        cx: ConnectionTo<Client>,
    ) -> Result<SetSessionModeResponse, Error> {
        let mode = RunMode::parse(&request.mode_id.to_string())
            .ok_or_else(|| Error::invalid_params().data("unsupported mode"))?;
        let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
        let Some(session) = sessions.get_mut(&request.session_id.to_string()) else {
            return Err(Error::resource_not_found(Some(
                request.session_id.to_string(),
            )));
        };
        session.mode = mode;
        if mode == RunMode::Plan {
            session.permission_mode = None;
        }
        drop(sessions);
        send_session_update(
            &cx,
            request.session_id,
            SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(mode.as_str())),
        );
        Ok(SetSessionModeResponse::new())
    }

    pub(crate) async fn set_session_config_option(
        &self,
        request: SetSessionConfigOptionRequest,
        cx: ConnectionTo<Client>,
    ) -> Result<SetSessionConfigOptionResponse, Error> {
        if request.config_id.to_string() == "mode" {
            let value = request
                .value
                .as_value_id()
                .map(ToString::to_string)
                .unwrap_or_default();
            let mode = RunMode::parse(&value)
                .ok_or_else(|| Error::invalid_params().data("unsupported mode"))?;
            let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
            let Some(session) = sessions.get_mut(&request.session_id.to_string()) else {
                return Err(Error::resource_not_found(Some(
                    request.session_id.to_string(),
                )));
            };
            session.mode = mode;
            if mode == RunMode::Plan {
                session.permission_mode = None;
            }
            drop(sessions);
            send_session_update(
                &cx,
                request.session_id.clone(),
                SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(mode.as_str())),
            );
        }
        let mode = self
            .sessions
            .lock()
            .expect("acp session lock poisoned")
            .get(&request.session_id.to_string())
            .map(|session| session.mode)
            .unwrap_or(RunMode::Default);
        let options = session_config_options(mode);
        send_session_update(
            &cx,
            request.session_id,
            SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(options.clone())),
        );
        Ok(SetSessionConfigOptionResponse::new(options))
    }

    pub(crate) fn run_options(
        &self,
        session: &AcpSession,
        prompt: String,
        image_inputs: Vec<ImageInput>,
        approval_handler: Option<Arc<dyn ApprovalHandler>>,
    ) -> RunOptions {
        RunOptions {
            state: self.state.clone(),
            workdir: session.cwd.clone(),
            snapshot_root: Some(self.options.home.join("snapshots")),
            session: session.runtime_session_id.clone(),
            continue_latest: false,
            prompt,
            image_inputs,
            extract_prompt_image_sources: false,
            prompt_display: None,
            max_context_messages: None,
            config_path: self.options.config_path.clone(),
            project_context_override: None,
            model: session.model.clone(),
            reasoning_effort: session.reasoning_effort.clone(),
            include_reasoning: true,
            mode: session.mode,
            permission_mode: session.permission_mode,
            approval_mode: Some(ApprovalMode::Manual),
            approval_handler,
            clarify_enabled: false,
            inherited_env: Some(self.options.inherited_env.clone()),
            agent: None,
            no_agents: false,
            no_skills: false,
            skill_inputs: Vec::new(),
            mcp_servers: session.mcp_servers.clone(),
        }
    }

    pub(crate) fn probe_run_options(&self, cwd: PathBuf, model: Option<String>) -> RunOptions {
        RunOptions {
            state: self.state.clone(),
            workdir: cwd,
            snapshot_root: None,
            session: None,
            continue_latest: false,
            prompt: String::new(),
            image_inputs: Vec::new(),
            extract_prompt_image_sources: false,
            prompt_display: None,
            max_context_messages: None,
            config_path: self.options.config_path.clone(),
            project_context_override: None,
            model,
            reasoning_effort: None,
            include_reasoning: false,
            mode: RunMode::Default,
            permission_mode: None,
            approval_mode: None,
            approval_handler: None,
            clarify_enabled: false,
            inherited_env: Some(self.options.inherited_env.clone()),
            agent: None,
            no_agents: false,
            no_skills: false,
            skill_inputs: Vec::new(),
            mcp_servers: Vec::new(),
        }
    }

    pub(crate) fn ready_auth_provider(&self) -> Option<String> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let options = self.probe_run_options(cwd, None);
        let selected = selected_configured_model(&options).ok().flatten()?;
        model_catalog_providers(&options)
            .ok()?
            .into_iter()
            .find(|provider| provider.provider == selected.provider && provider.fetchable())
            .map(|provider| provider.provider)
    }

    pub(crate) fn terminal_auth_available(&self) -> bool {
        self.client_terminal_auth
            .lock()
            .map(|value| *value)
            .unwrap_or(false)
    }

    pub(crate) fn terminal_output_available(&self) -> bool {
        self.client_terminal_output
            .lock()
            .map(|value| *value)
            .unwrap_or(false)
    }

    fn client_terminal_output_enabled(&self, capabilities: &ClientCapabilities) -> bool {
        self.options
            .inherited_env
            .get("PSYCHEVO_ACP_TERMINAL_OUTPUT")
            .is_some_and(|value| env_flag_enabled(value))
            && capabilities.meta.as_ref().is_some_and(|meta| {
                meta.get("terminal_output")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            })
    }

    fn send_usage_update_from_context(
        &self,
        cx: &ConnectionTo<Client>,
        session_id: SessionId,
        snapshot: Option<&ContextSnapshot>,
        usage: &Arc<Mutex<AcpUsageAccumulator>>,
    ) {
        let Some(snapshot) = snapshot else {
            return;
        };
        let Some(size) = snapshot.context_limit else {
            return;
        };
        let mut update = UsageUpdate::new(snapshot.total.estimated_tokens, size);
        if let Ok(usage) = usage.lock()
            && let Some(cost) = usage.cumulative_cost_usd()
        {
            update = update.cost(Cost::new(cost, "USD"));
        }
        let mut psychevo = serde_json::Map::new();
        psychevo.insert(
            "source".to_string(),
            Value::String("runtime_context_snapshot".to_string()),
        );
        psychevo.insert(
            "provider".to_string(),
            Value::String(snapshot.provider.clone()),
        );
        psychevo.insert("model".to_string(), Value::String(snapshot.model.clone()));
        let mut meta = serde_json::Map::new();
        meta.insert("psychevo".to_string(), Value::Object(psychevo));
        update = update.meta(meta);
        send_session_update(cx, session_id, SessionUpdate::UsageUpdate(update));
    }

    pub(crate) async fn handle_slash_prompt(
        &self,
        session_id: &SessionId,
        session: &AcpSession,
        prompt: &str,
        cx: &ConnectionTo<Client>,
    ) -> Result<SlashPromptAction, Error> {
        use psychevo_runtime::command_registry::{SlashCommandParse, SlashCommandSurface};

        let dynamic = self.dynamic_slash_commands(session);
        let effect_and_action =
            match psychevo_runtime::command_registry::parse_slash_command_line(prompt) {
                SlashCommandParse::NotSlash => return Ok(SlashPromptAction::NotSlashOrPassThrough),
                SlashCommandParse::Unknown {
                    command,
                    args,
                    original: _,
                } => {
                    if let Some(effect) =
                        psychevo_runtime::command_registry::dynamic_slash_command_effect(
                            &command, &args, &dynamic,
                        )
                    {
                        (effect, None)
                    } else {
                        return Ok(SlashPromptAction::NotSlashOrPassThrough);
                    }
                }
                SlashCommandParse::Known(invocation) => {
                    let active_turn = session.control.is_some();
                    let effect = psychevo_runtime::command_registry::slash_invocation_effect(
                        &invocation,
                        acp_command_capabilities(),
                        SlashCommandSurface::Acp,
                        active_turn,
                    )
                    .map_err(|message| Error::invalid_params().data(message))?;
                    (effect, Some(invocation.spec.action))
                }
            };

        self.apply_slash_effect(
            session_id,
            session,
            effect_and_action.0,
            effect_and_action.1,
            cx,
        )
        .await
    }
}
