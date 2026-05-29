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
    pub(crate) sessions: Arc<Mutex<HashMap<String, AcpSession>>>,
    pub(crate) client_terminal_auth: Arc<Mutex<bool>>,
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
        Ok(Self {
            options,
            state,
            sessions: Arc::default(),
            client_terminal_auth: Arc::new(Mutex::new(false)),
        })
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
                    async move |request: SetSessionModelRequest, responder, _cx| {
                        responder.respond_with_result(agent.set_session_model(request).await)
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
        if let Ok(mut value) = self.client_terminal_auth.lock() {
            *value = terminal_auth;
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
        Ok(NewSessionResponse::new(session_id)
            .modes(mode_state(RunMode::Default))
            .models(self.model_state(None)))
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
        Ok(LoadSessionResponse::new()
            .modes(mode_state(RunMode::Default))
            .models(self.model_state(None)))
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
        if let Some(session) = self
            .sessions
            .lock()
            .expect("acp session lock poisoned")
            .remove(&request.session_id.to_string())
            && let Some(control) = session.control
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
        let stream_session_id = session_id.clone();
        let stream_cx = cx.clone();
        let stream_usage = Arc::clone(&usage);
        let stream = Arc::new(move |event| {
            if let Ok(mut usage) = stream_usage.lock() {
                usage.record_stream_event(&event);
            }
            send_run_stream_update(&stream_cx, &stream_session_id, event);
        });
        let options = self.run_options(&session, prompt, image_inputs, Some(approval_handler));
        let result =
            run_live_streaming_controlled(options, "acp", &["acp", "run", "tui"], stream, control)
                .await;
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
        if let Some(control) = control {
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

    pub(crate) async fn set_session_model(
        &self,
        request: SetSessionModelRequest,
    ) -> Result<SetSessionModelResponse, Error> {
        let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
        let Some(session) = sessions.get_mut(&request.session_id.to_string()) else {
            return Err(Error::resource_not_found(Some(
                request.session_id.to_string(),
            )));
        };
        let model = self.normalize_session_model(session, &request.model_id.to_string())?;
        session.model = Some(model);
        Ok(SetSessionModelResponse::new())
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

    pub(crate) fn model_state(&self, session: Option<&AcpSession>) -> SessionModelState {
        let cwd = session
            .map(|session| session.cwd.clone())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let model_override = session.and_then(|session| session.model.clone());
        let options = self.probe_run_options(cwd, model_override.clone());
        let selected = selected_configured_model(&options).ok().flatten();
        let current_id = selected
            .as_ref()
            .map(configured_model_id)
            .or(model_override)
            .unwrap_or_else(|| "auto/default".to_string());
        let mut models = configured_models(&options)
            .unwrap_or_default()
            .into_iter()
            .map(|model| {
                ModelInfo::new(
                    configured_model_id(&model),
                    format!("{} ({})", model.model, model.provider_label),
                )
            })
            .collect::<Vec<_>>();
        if !models
            .iter()
            .any(|model| model.model_id.to_string() == current_id)
        {
            let name = selected
                .as_ref()
                .map(|model| format!("{} ({})", model.model, model.provider_label))
                .unwrap_or_else(|| current_id.clone());
            models.push(ModelInfo::new(current_id.clone(), name));
        }
        models.sort_by(|left, right| left.name.cmp(&right.name));
        SessionModelState::new(current_id, models)
    }

    pub(crate) fn normalize_session_model(
        &self,
        session: &AcpSession,
        requested: &str,
    ) -> Result<String, Error> {
        let requested = requested.trim();
        if requested.is_empty() {
            return Err(Error::invalid_params().data("model id must not be empty"));
        }
        let requested = if requested.contains('/') {
            let Some((provider, model)) = requested.split_once('/') else {
                return Err(Error::invalid_params().data("model id must use provider/model"));
            };
            if provider.trim().is_empty() || model.trim().is_empty() {
                return Err(Error::invalid_params().data("model id must use provider/model"));
            }
            format!("{}/{}", provider.trim(), model.trim())
        } else {
            self.unique_bare_model_id(session, requested)?
        };
        let options = self.probe_run_options(session.cwd.clone(), Some(requested.clone()));
        if selected_configured_model(&options)
            .map_err(acp_internal_error)?
            .is_none()
        {
            return Err(
                Error::invalid_params().data(format!("unknown model selection: {requested}"))
            );
        }
        Ok(requested)
    }

    fn unique_bare_model_id(&self, session: &AcpSession, requested: &str) -> Result<String, Error> {
        let options = self.probe_run_options(session.cwd.clone(), None);
        let matches = configured_models(&options)
            .map_err(acp_internal_error)?
            .into_iter()
            .filter(|model| model.model == requested)
            .map(|model| configured_model_id(&model))
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [model] => Ok(model.clone()),
            [] => Err(Error::invalid_params().data(format!("unknown bare model id: {requested}"))),
            _ => Err(Error::invalid_params().data(format!(
                "ambiguous bare model id: {requested}; use provider/model"
            ))),
        }
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

fn configured_model_id(model: &psychevo_runtime::ConfiguredModel) -> String {
    format!("{}/{}", model.provider, model.model)
}
