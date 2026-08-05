use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v2::{
    AgentAuthCapabilities, AgentCapabilities, AvailableCommandsUpdate, CancelSessionNotification,
    CloseSessionRequest, CloseSessionResponse, ConfigOptionUpdate, IdleStateUpdate, Implementation,
    InitializeRequest, InitializeResponse, ListSessionsRequest, ListSessionsResponse,
    LoginAuthRequest, LoginAuthResponse, McpCapabilities, McpHttpCapabilities, NewSessionRequest,
    NewSessionResponse, PromptCapabilities, PromptEmbeddedContextCapabilities,
    PromptImageCapabilities, PromptRequest, PromptResponse, ReplayFrom, ResumeSessionRequest,
    ResumeSessionResponse, RunningStateUpdate, SessionCapabilities, SessionConfigOption, SessionId,
    SessionInfo, SessionUpdate, SetSessionConfigOptionRequest, SetSessionConfigOptionResponse,
    StateUpdate, StopReason,
};
use agent_client_protocol::{Agent, Client, ConnectTo, ConnectionTo, Error};
use psychevo::{Application, ImageInput, RunMode, StartThreadRequest, ThreadListQuery};

use crate::commands::{
    AcpApprovalHandler, AcpTurnProjection, SlashPromptAction, TERMINAL_SETUP_AUTH_METHOD_ID,
    agent_message_update, reasoning_effort_value, send_session_setup_updates, send_session_update,
    send_turn_event_update,
};
use crate::protocol::{
    AcpUsageAccumulator, REASONING_EFFORT_VALUES, acp_internal_error, acp_mcp_servers,
    prompt_parts, replay_thread_history, session_config_options, single_text_prompt, stop_reason,
};
use crate::stdio::{AcpOptions, AcpSession, PsychevoAcpAgent};

use super::options_and_tests::AcpUsageUpdateContext;

impl PsychevoAcpAgent {
    pub(crate) async fn new(options: AcpOptions) -> psychevo::Result<Self> {
        let mut builder = Application::builder()
            .home(&options.home)
            .database_path(&options.db_path);
        if let Some(config_path) = options.config_path.as_ref() {
            builder = builder.config_path(config_path);
        }
        let application = builder.build().await?;
        let framework = application.client();
        Ok(Self {
            options,
            application,
            framework,
            sessions: Arc::default(),
            client_terminal_auth: Arc::new(Mutex::new(false)),
            client_terminal_output: Arc::new(Mutex::new(false)),
        })
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
                        let result = agent.resume_session(request, &cx).await;
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
        let session_id = SessionId::new(uuid::Uuid::now_v7().to_string());
        let mcp_servers = acp_mcp_servers(request.mcp_servers);
        let session = AcpSession::new(request.cwd, mcp_servers);
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
        cx: &ConnectionTo<Client>,
    ) -> Result<ResumeSessionResponse, Error> {
        let replay_from_start = match request.replay_from.as_ref() {
            None => false,
            Some(ReplayFrom::Start(_)) => true,
            Some(ReplayFrom::Other(cursor)) => {
                return Err(Error::invalid_params()
                    .data(format!("unsupported replay cursor: {}", cursor.type_)));
            }
            Some(_) => {
                return Err(Error::invalid_params().data("unsupported replay cursor"));
            }
        };
        let runtime_session_id = request.session_id.to_string();
        let thread = self
            .framework
            .resume_thread(runtime_session_id.clone())
            .await
            .map_err(|_| Error::resource_not_found(Some(runtime_session_id.clone())))?;
        let session = AcpSession::loaded(
            request.cwd,
            thread.clone(),
            acp_mcp_servers(request.mcp_servers),
        );
        let config_options = self.session_config_options_for_session(&session);
        self.sessions
            .lock()
            .expect("acp session lock poisoned")
            .insert(request.session_id.to_string(), session);
        let replay_meta = if replay_from_start {
            match replay_thread_history(&thread, &request.session_id, cx).await {
                Ok(meta) => meta,
                Err(error) => {
                    self.sessions
                        .lock()
                        .expect("acp session lock poisoned")
                        .remove(&request.session_id.to_string());
                    return Err(error);
                }
            }
        } else {
            None
        };
        Ok(ResumeSessionResponse::new()
            .config_options(config_options)
            .meta(replay_meta))
    }

    pub(crate) async fn list_sessions(
        &self,
        request: ListSessionsRequest,
    ) -> Result<ListSessionsResponse, Error> {
        let cwd = request
            .cwd
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let page = self
            .framework
            .list_threads(ThreadListQuery {
                cwd: Some(cwd),
                cursor: request.cursor,
                ..Default::default()
            })
            .await
            .map_err(acp_internal_error)?;
        let sessions = page
            .threads
            .into_iter()
            .map(|summary| {
                SessionInfo::new(summary.id, PathBuf::from(summary.cwd))
                    .title(summary.title)
                    .updated_at(Some(summary.updated_at_ms.to_string()))
            })
            .collect();
        Ok(ListSessionsResponse::new(sessions).next_cursor(page.next_cursor))
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
            && let Some(turn) = session.turn
        {
            turn.interrupt();
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
        let (prompt, image_inputs) = prompt_parts(prompt_blocks, &session.cwd)?;
        send_session_update(
            &cx,
            session_id.clone(),
            SessionUpdate::StateUpdate(StateUpdate::Running(RunningStateUpdate::new())),
        );

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
        let session = {
            let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
            let Some(session) = sessions.get_mut(&session_key) else {
                return Err(Error::resource_not_found(Some(session_key)));
            };
            if session.starting_turn || session.turn.is_some() {
                return Err(Error::invalid_params().data("session already has an active prompt"));
            }
            session.starting_turn = true;
            session.cancel_starting_turn = false;
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
        let request = self.turn_request(&session, prompt, image_inputs, Some(approval_handler));
        let started = if let Some(thread) = session.thread.as_ref() {
            thread
                .start_turn(request)
                .await
                .map(|handle| (handle, None))
        } else {
            let mut start = StartThreadRequest::new(&session.cwd);
            start.source = "acp".to_string();
            match self.framework.start_thread_with_turn(start, request).await {
                Ok(handle) => {
                    let thread = self
                        .framework
                        .resume_thread(handle.receipt().thread_id.clone())
                        .await;
                    match thread {
                        Ok(thread) => Ok((handle, Some(thread))),
                        Err(error) => {
                            handle.interrupt();
                            Err(error)
                        }
                    }
                }
                Err(error) => Err(error),
            }
        };
        let (handle, new_thread) = match started {
            Ok(started) => started,
            Err(error) => {
                if let Ok(mut sessions) = self.sessions.lock()
                    && let Some(session) = sessions.get_mut(&session_key)
                {
                    session.starting_turn = false;
                    session.cancel_starting_turn = false;
                    session.clear_pending_admission_steers();
                }
                return Err(acp_internal_error(error));
            }
        };
        let publication_error = {
            let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
            let Some(session) = sessions.get_mut(&session_key) else {
                handle.interrupt();
                return Err(Error::resource_not_found(Some(session_key)));
            };
            session.starting_turn = false;
            if let Some(thread) = new_thread {
                session.thread = Some(thread);
            }
            let canceled = std::mem::take(&mut session.cancel_starting_turn);
            let publication_error = session
                .take_pending_admission_steers()
                .into_iter()
                .find_map(|prompt| handle.queue_steer(prompt).err());
            if canceled || publication_error.is_some() {
                handle.interrupt();
            }
            if publication_error.is_none() {
                session.turn = Some(handle.clone());
            }
            publication_error
        };
        if let Some(error) = publication_error {
            return Err(acp_internal_error(format!(
                "failed to publish retained ACP steer: {error}"
            )));
        }
        let mut events = handle.events();
        let event_session_id = session_id.clone();
        let event_cx = cx.clone();
        let event_usage = Arc::clone(&usage);
        let terminal_output = self.terminal_output_available();
        let event_task = async move {
            let mut projection = AcpTurnProjection::new(terminal_output);
            while let Some(event) = events.next().await {
                send_turn_event_update(
                    &event_cx,
                    &event_session_id,
                    event,
                    &event_usage,
                    &mut projection,
                );
            }
            projection
        };
        let (result, projection) = tokio::join!(handle.wait(), event_task);
        match result {
            Ok(result) => {
                if let Some(final_text) = projection.remaining_final_text(&result.final_answer) {
                    send_session_update(
                        &cx,
                        session_id.clone(),
                        agent_message_update(&session_id, final_text),
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
                    session.turn = None;
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
                    session.turn = None;
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
        let turn = self
            .sessions
            .lock()
            .expect("acp session lock poisoned")
            .get_mut(&notification.session_id.to_string())
            .and_then(|session| {
                session.queued_prompts.clear();
                session.clear_pending_admission_steers();
                if session.starting_turn {
                    session.cancel_starting_turn = true;
                }
                session.turn.clone()
            });
        if let Some(turn) = turn {
            turn.interrupt();
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
        let configured = self
            .configuration_for_session(session)
            .and_then(|configuration| configuration.configured_models())
            .unwrap_or_default();
        session_config_options(
            session.mode,
            session.model.as_deref(),
            session.reasoning_effort.as_deref(),
            &configured,
        )
    }
}
