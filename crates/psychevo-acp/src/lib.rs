use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_client_protocol::schema::{
    AgentAuthCapabilities, AgentCapabilities, AuthEnvVar, AuthMethod, AuthMethodEnvVar,
    AuthenticateRequest, AuthenticateResponse, AvailableCommand, AvailableCommandInput,
    AvailableCommandsUpdate, CancelNotification, CloseSessionRequest, CloseSessionResponse,
    ConfigOptionUpdate, ContentBlock, ContentChunk, CurrentModeUpdate, EnvVariable, Implementation,
    InitializeRequest, InitializeResponse, ListSessionsRequest, ListSessionsResponse,
    LoadSessionRequest, LoadSessionResponse, LogoutCapabilities, McpCapabilities, McpServer,
    McpServerHttp, McpServerStdio, NewSessionRequest, NewSessionResponse, PermissionOption,
    PermissionOptionKind, PromptCapabilities, PromptRequest, PromptResponse,
    RequestPermissionOutcome, RequestPermissionRequest, SessionCapabilities,
    SessionCloseCapabilities, SessionConfigSelectOption, SessionId, SessionInfo,
    SessionListCapabilities, SessionMode, SessionModeState, SessionNotification, SessionUpdate,
    SetSessionConfigOptionRequest, SetSessionConfigOptionResponse, SetSessionModeRequest,
    SetSessionModeResponse, SetSessionModelRequest, SetSessionModelResponse, StopReason, ToolCall,
    ToolCallContent, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
    UnstructuredCommandInput,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectTo, ConnectionTo, Error};
use futures::future::BoxFuture;
use psychevo_runtime::{
    AgentDiscoveryOptions, ApprovalHandler, ApprovalMode, CompactSessionOptions, CompactionReason,
    ConfigScope, ContextFormatOptions, ContextOptions, ImageInput, InstallOptions, McpServerInput,
    McpTransportInput, Message, PermissionApprovalDecision, PermissionApprovalRequest,
    PermissionMode, RunControlHandle, RunMode, RunOptions, RunStreamEvent, SessionArtifactKind,
    SessionExportFormat, SessionExportIncludeSet, SessionExportOptions, SessionSummary,
    SessionUndoOptions, SkillDiscoveryOptions, SkillTarget, SqliteStore, UserContentBlock,
    append_local_permission_rule, canonicalize_workdir, compact_session, configured_models,
    context_snapshot, default_session_export_filename, discover_agents, discover_skills,
    format_context_snapshot_text_with_options, install_skill, list_agents_value,
    list_skill_bundles, model_catalog_providers, permission_rules_value, redo_session,
    remove_local_permission_rule, remove_skill, run_control, run_live_streaming_controlled,
    scan_skill_path, set_local_toolset_enabled, set_skill_config_value, set_skill_enabled,
    toolsets_value, undo_session, usage_stats,
};
use serde_json::{Value, json};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use uuid::Uuid;

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
    let agent = Arc::new(PsychevoAcpAgent::new(options));
    let stdin = tokio::io::stdin().compat();
    let stdout = tokio::io::stdout().compat_write();
    agent
        .serve(ByteStreams::new(stdout, stdin))
        .await
        .map_err(|err| std::io::Error::other(format!("ACP error: {err}")))
}

struct PsychevoAcpAgent {
    options: AcpOptions,
    sessions: Arc<Mutex<HashMap<String, AcpSession>>>,
}

#[derive(Debug, Clone)]
struct AcpSession {
    cwd: PathBuf,
    runtime_session_id: Option<String>,
    mode: RunMode,
    permission_mode: Option<PermissionMode>,
    model: Option<String>,
    reasoning_effort: Option<String>,
    mcp_servers: Vec<McpServerInput>,
    control: Option<RunControlHandle>,
    queued_prompts: VecDeque<String>,
    pending_steers: Vec<psychevo_runtime::PendingInputId>,
    last_session_list: Vec<SessionSummary>,
}

impl AcpSession {
    fn new(
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
    fn new(options: AcpOptions) -> Self {
        Self {
            options,
            sessions: Arc::default(),
        }
    }

    async fn serve(
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

    async fn initialize(&self, request: InitializeRequest) -> Result<InitializeResponse, Error> {
        let auth_methods = self.auth_methods();
        let mut capabilities = AgentCapabilities::new()
            .load_session(true)
            .prompt_capabilities(PromptCapabilities::new().embedded_context(true).image(true))
            .mcp_capabilities(McpCapabilities::new().http(true))
            .auth(AgentAuthCapabilities::new().logout(LogoutCapabilities::new()));
        capabilities.session_capabilities = SessionCapabilities::new()
            .close(SessionCloseCapabilities::new())
            .list(SessionListCapabilities::new());
        Ok(InitializeResponse::new(request.protocol_version)
            .agent_capabilities(capabilities)
            .agent_info(
                Implementation::new("psychevo-acp", env!("CARGO_PKG_VERSION")).title("Psychevo"),
            )
            .auth_methods(auth_methods))
    }

    async fn authenticate(
        &self,
        request: AuthenticateRequest,
    ) -> Result<AuthenticateResponse, Error> {
        let method = request.method_id.to_string();
        if let Some(env_name) = method.strip_prefix("env:") {
            if self
                .options
                .inherited_env
                .get(env_name)
                .is_some_and(|value| !value.trim().is_empty())
            {
                return Ok(AuthenticateResponse::new());
            }
            return Err(Error::auth_required().data(format!("{env_name} is not set")));
        }
        Err(Error::invalid_params().data(format!("unsupported auth method: {method}")))
    }

    async fn new_session(&self, request: NewSessionRequest) -> Result<NewSessionResponse, Error> {
        let session_id = SessionId::new(format!("acp-{}", Uuid::now_v7()));
        let mcp_servers = acp_mcp_servers(request.mcp_servers);
        let session = AcpSession::new(request.cwd, None, mcp_servers);
        self.sessions
            .lock()
            .expect("acp session lock poisoned")
            .insert(session_id.to_string(), session);
        Ok(NewSessionResponse::new(session_id).modes(mode_state(RunMode::Default)))
    }

    async fn load_session(
        &self,
        request: LoadSessionRequest,
    ) -> Result<LoadSessionResponse, Error> {
        let runtime_session_id = request.session_id.to_string();
        let store = SqliteStore::open(&self.options.db_path).map_err(acp_internal_error)?;
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

    async fn list_sessions(
        &self,
        request: ListSessionsRequest,
    ) -> Result<ListSessionsResponse, Error> {
        let cwd = request
            .cwd
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let store = SqliteStore::open(&self.options.db_path).map_err(acp_internal_error)?;
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

    async fn close_session(
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

    async fn prompt(
        &self,
        request: PromptRequest,
        cx: ConnectionTo<Client>,
    ) -> Result<PromptResponse, Error> {
        let session_id = request.session_id.clone();
        let session_key = session_id.to_string();
        let (prompt, image_inputs) = prompt_parts(request.prompt);
        let session = {
            let sessions = self.sessions.lock().expect("acp session lock poisoned");
            let Some(session) = sessions.get(&session_key) else {
                return Err(Error::resource_not_found(Some(session_key)));
            };
            session.clone()
        };

        match self
            .handle_slash_prompt(&session_id, &session, &prompt, &cx)
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

        self.run_prompt_and_drain(session_id, prompt, image_inputs, cx)
            .await
    }

    async fn run_prompt_and_drain(
        &self,
        session_id: SessionId,
        prompt: String,
        image_inputs: Vec<ImageInput>,
        cx: ConnectionTo<Client>,
    ) -> Result<PromptResponse, Error> {
        let response = self
            .run_prompt_once(session_id.clone(), prompt, image_inputs, cx.clone())
            .await?;
        while let Some(prompt) = self.pop_queued_prompt(&session_id) {
            self.run_prompt_once(session_id.clone(), prompt, Vec::new(), cx.clone())
                .await?;
        }
        Ok(response)
    }

    async fn run_prompt_once(
        &self,
        session_id: SessionId,
        prompt: String,
        image_inputs: Vec<ImageInput>,
        cx: ConnectionTo<Client>,
    ) -> Result<PromptResponse, Error> {
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
        let stream = Arc::new(move |event| {
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
                Ok(PromptResponse::new(stop_reason(result.outcome)))
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

    async fn cancel(&self, notification: CancelNotification) {
        if let Some(control) = self
            .sessions
            .lock()
            .expect("acp session lock poisoned")
            .get(&notification.session_id.to_string())
            .and_then(|session| session.control.clone())
        {
            control.abort();
        }
    }

    async fn set_session_mode(
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

    async fn set_session_model(
        &self,
        request: SetSessionModelRequest,
    ) -> Result<SetSessionModelResponse, Error> {
        let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
        let Some(session) = sessions.get_mut(&request.session_id.to_string()) else {
            return Err(Error::resource_not_found(Some(
                request.session_id.to_string(),
            )));
        };
        session.model = Some(request.model_id.to_string());
        Ok(SetSessionModelResponse::new())
    }

    async fn set_session_config_option(
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
        let options = session_config_options(RunMode::Default);
        send_session_update(
            &cx,
            request.session_id,
            SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(options.clone())),
        );
        Ok(SetSessionConfigOptionResponse::new(options))
    }

    fn run_options(
        &self,
        session: &AcpSession,
        prompt: String,
        image_inputs: Vec<ImageInput>,
        approval_handler: Option<Arc<dyn ApprovalHandler>>,
    ) -> RunOptions {
        RunOptions {
            db_path: self.options.db_path.clone(),
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

    async fn handle_slash_prompt(
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

    async fn apply_slash_effect(
        &self,
        session_id: &SessionId,
        session: &AcpSession,
        effect: psychevo_runtime::command_registry::SlashCommandEffect,
        action: Option<psychevo_runtime::command_registry::SlashCommandAction>,
        cx: &ConnectionTo<Client>,
    ) -> Result<SlashPromptAction, Error> {
        use psychevo_runtime::command_registry::{SlashCommandAction, SlashCommandEffect};

        match effect {
            SlashCommandEffect::LocalText => {
                let text = match action {
                    Some(SlashCommandAction::Help) => self.help_command_text(session),
                    Some(SlashCommandAction::Status) => {
                        self.status_command_text(session_id, session)
                    }
                    Some(SlashCommandAction::Usage) => self.usage_command_text(session)?,
                    Some(SlashCommandAction::Context) => self.context_command_text(session)?,
                    Some(SlashCommandAction::Refresh) => self.refresh_command_text(session)?,
                    _ => "Command completed.".to_string(),
                };
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::PassThroughPrompt(prompt)
            | SlashCommandEffect::SubmitPrompt(prompt) => Ok(SlashPromptAction::RunPrompt(prompt)),
            SlashCommandEffect::Steer(message) => self.apply_steer_effect(session_id, &message, cx),
            SlashCommandEffect::Queue(message) => {
                self.queue_prompt(session_id, message.clone())?;
                Ok(send_slash_text(
                    cx,
                    session_id,
                    format!("queued prompt: {message}"),
                ))
            }
            SlashCommandEffect::PendingCancel => {
                let total = self.cancel_pending_inputs(session_id);
                let text = if total == 0 {
                    "no pending input".to_string()
                } else {
                    format!("pending input canceled: {total}")
                };
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::NewSession => {
                let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
                let Some(session) = sessions.get_mut(&session_id.to_string()) else {
                    return Err(Error::resource_not_found(Some(session_id.to_string())));
                };
                session.runtime_session_id = None;
                session.queued_prompts.clear();
                session.pending_steers.clear();
                session.last_session_list.clear();
                Ok(send_slash_text(
                    cx,
                    session_id,
                    "New runtime session will be used for the next prompt.",
                ))
            }
            SlashCommandEffect::SessionsList => {
                let text = self.sessions_list_text(session_id, session)?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::ResumeSession { reference } => {
                let text = self.resume_session_text(session_id, session, reference.as_deref())?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::ShowModel => {
                let text = self.model_command_text(session)?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::SetModel { model, variant } => {
                let text = self.set_model_text(session_id, &model, variant.as_deref())?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::SetVariant(variant) => {
                let text = self.set_variant_text(session_id, &variant)?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::SetMode(mode) => {
                let text = self.set_mode_text(session_id, &mode, cx)?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::PermissionsShow => Ok(send_slash_text(
                cx,
                session_id,
                self.permissions_status_text(session)?,
            )),
            SlashCommandEffect::PermissionAdd { kind, rule } => {
                if !self
                    .request_command_approval(
                        session_id,
                        cx,
                        "/permissions",
                        "change local permission policy",
                    )
                    .await
                {
                    return Ok(send_slash_text(cx, session_id, "permission denied"));
                }
                let result =
                    append_local_permission_rule(self.local_config_dir(session)?, &kind, &rule)
                        .map_err(acp_internal_error)?;
                Ok(send_slash_text(
                    cx,
                    session_id,
                    format!(
                        "{} {} rule: {}",
                        if result.changed {
                            "added"
                        } else {
                            "already had"
                        },
                        result.kind,
                        result.rule
                    ),
                ))
            }
            SlashCommandEffect::PermissionRemove { kind, rule } => {
                if !self
                    .request_command_approval(
                        session_id,
                        cx,
                        "/permissions",
                        "change local permission policy",
                    )
                    .await
                {
                    return Ok(send_slash_text(cx, session_id, "permission denied"));
                }
                let result =
                    remove_local_permission_rule(self.local_config_dir(session)?, &kind, &rule)
                        .map_err(acp_internal_error)?;
                Ok(send_slash_text(
                    cx,
                    session_id,
                    format!(
                        "{} {} rule: {}",
                        if result.changed {
                            "removed"
                        } else {
                            "no matching"
                        },
                        result.kind,
                        result.rule
                    ),
                ))
            }
            SlashCommandEffect::ToolsShow => Ok(send_slash_text(
                cx,
                session_id,
                self.toolsets_status_text(session)
                    .map_err(acp_internal_error)?,
            )),
            SlashCommandEffect::ToolsetSet { name, enabled } => {
                if !self
                    .request_command_approval(
                        session_id,
                        cx,
                        "/tools",
                        "change local toolset configuration",
                    )
                    .await
                {
                    return Ok(send_slash_text(cx, session_id, "permission denied"));
                }
                let result = set_local_toolset_enabled(
                    self.local_config_dir(session)?,
                    session.mode,
                    &name,
                    enabled,
                )
                .map_err(acp_internal_error)?;
                Ok(send_slash_text(
                    cx,
                    session_id,
                    format!(
                        "{} toolset `{}` for {} mode",
                        if enabled { "enabled" } else { "disabled" },
                        result.name,
                        session.mode.as_str()
                    ),
                ))
            }
            SlashCommandEffect::Rename(title) => {
                let Some(runtime_session_id) = session.runtime_session_id.as_deref() else {
                    return Ok(send_slash_text(cx, session_id, "no runtime session yet"));
                };
                let title = SqliteStore::open(&self.options.db_path)
                    .and_then(|store| store.set_session_title(runtime_session_id, &title))
                    .map_err(acp_internal_error)?;
                Ok(send_slash_text(
                    cx,
                    session_id,
                    format!("session renamed: {title}"),
                ))
            }
            SlashCommandEffect::Undo => {
                let result =
                    undo_session(self.undo_options(session)?).map_err(acp_internal_error)?;
                Ok(send_slash_text(
                    cx,
                    session_id,
                    format!(
                        "undone {} messages; prompt restored",
                        result.reverted_messages
                    ),
                ))
            }
            SlashCommandEffect::Redo => {
                let result =
                    redo_session(self.undo_options(session)?).map_err(acp_internal_error)?;
                Ok(send_slash_text(
                    cx,
                    session_id,
                    format!(
                        "redone {} messages ({})",
                        result.restored_messages,
                        if result.complete {
                            "complete"
                        } else {
                            "partial"
                        }
                    ),
                ))
            }
            SlashCommandEffect::Skills { args } => {
                let text = self
                    .skills_command_text(session_id, session, args.as_deref(), cx)
                    .await?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::Bundles { args } => {
                let text = self.bundles_command_text(session, args.as_deref())?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::Curator { args } => Ok(send_slash_text(
                cx,
                session_id,
                self.curator_command_text(args.as_deref()),
            )),
            SlashCommandEffect::Agents => Ok(send_slash_text(
                cx,
                session_id,
                self.agents_status_text(session)
                    .map_err(acp_internal_error)?,
            )),
            SlashCommandEffect::Fork(prompt) => Ok(SlashPromptAction::RunPrompt(prompt)),
            SlashCommandEffect::Compact { instructions } => {
                let text = self.compact_command_text(session, instructions).await?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::Export { args } => {
                if !self
                    .request_command_approval(
                        session_id,
                        cx,
                        "/export",
                        "write a local session export artifact",
                    )
                    .await
                {
                    return Ok(send_slash_text(cx, session_id, "permission denied"));
                }
                let text = self.write_artifact_text(
                    session,
                    SessionArtifactKind::Export,
                    args.as_deref(),
                )?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::Share { args } => {
                if !self
                    .request_command_approval(
                        session_id,
                        cx,
                        "/share",
                        "write a local share artifact",
                    )
                    .await
                {
                    return Ok(send_slash_text(cx, session_id, "permission denied"));
                }
                let text =
                    self.write_artifact_text(session, SessionArtifactKind::Share, args.as_deref())?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::Unsupported(text) => Ok(send_slash_text(cx, session_id, text)),
        }
    }

    fn status_command_text(&self, session_id: &SessionId, session: &AcpSession) -> String {
        let model = session.model.as_deref().unwrap_or("(configured default)");
        let runtime_session = session.runtime_session_id.as_deref().unwrap_or("(new)");
        format!(
            "ACP session: {session_id}\nruntime session: {runtime_session}\nworkdir: {}\nmode: {}\nmodel: {model}\ncommands: {}",
            session.cwd.display(),
            session.mode.as_str(),
            self.available_commands_for_session_state(session, session.control.is_some())
                .commands
                .len()
        )
    }

    fn toolsets_status_text(
        &self,
        session: &AcpSession,
    ) -> Result<String, psychevo_runtime::Error> {
        let options = self.run_options(session, String::new(), Vec::new(), None);
        let value = toolsets_value(&options, ConfigScope::Effective)?;
        let mode_key = session.mode.as_str();
        let tools = value["modes"][mode_key]["effective_tools"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let mut lines = vec![format!("mode: {mode_key}"), format!("tools: {tools}")];
        for row in value["toolsets"].as_array().cloned().unwrap_or_default() {
            lines.push(format!(
                "- {} ({}) {}",
                row["name"].as_str().unwrap_or("-"),
                row["source"].as_str().unwrap_or("-"),
                row["description"].as_str().unwrap_or("")
            ));
        }
        Ok(lines.join("\n"))
    }

    fn agents_status_text(&self, session: &AcpSession) -> Result<String, psychevo_runtime::Error> {
        let catalog = discover_agents(&AgentDiscoveryOptions {
            home: self.options.home.clone(),
            workdir: session.cwd.clone(),
            env: self.options.inherited_env.clone(),
            explicit_inputs: Vec::new(),
            no_agents: false,
        })?;
        let value = list_agents_value(&catalog);
        let Some(agents) = value["agents"].as_array() else {
            return Ok("No agents discovered.".to_string());
        };
        if agents.is_empty() {
            return Ok("No agents discovered.".to_string());
        }
        let mut lines = Vec::from(["Available agents:".to_string()]);
        for agent in agents {
            lines.push(format!(
                "- {} ({}) {}",
                agent["name"].as_str().unwrap_or("-"),
                agent["source"].as_str().unwrap_or("-"),
                agent["description"].as_str().unwrap_or("")
            ));
        }
        Ok(lines.join("\n"))
    }

    fn help_command_text(&self, session: &AcpSession) -> String {
        let available =
            self.available_commands_for_session_state(session, session.control.is_some());
        let hidden_dynamic = available.hidden_dynamic;
        let mut lines = vec!["Available commands:".to_string()];
        lines.extend(available_command_lines_from(available_commands_from(
            available,
        )));
        if hidden_dynamic > 0 {
            lines.push(format!(
                "{} dynamic skill or bundle commands hidden; type /skills or /bundles to list them.",
                hidden_dynamic
            ));
        }
        lines.join("\n")
    }

    fn usage_command_text(&self, session: &AcpSession) -> Result<String, Error> {
        let value = usage_stats(psychevo_runtime::StatsOptions {
            db_path: self.options.db_path.clone(),
            workdir: session.cwd.clone(),
            all: false,
            days: None,
            limit: 20,
        })
        .map_err(acp_internal_error)?;
        serde_json::to_string_pretty(&value).map_err(acp_internal_error)
    }

    fn context_command_text(&self, session: &AcpSession) -> Result<String, Error> {
        let Some(runtime_session_id) = session.runtime_session_id.clone() else {
            return Ok("no runtime session yet".to_string());
        };
        let snapshot = context_snapshot(ContextOptions {
            db_path: self.options.db_path.clone(),
            workdir: session.cwd.clone(),
            session: runtime_session_id,
            config_path: self.options.config_path.clone(),
            inherited_env: Some(self.options.inherited_env.clone()),
        })
        .map_err(acp_internal_error)?;
        Ok(format_context_snapshot_text_with_options(
            &snapshot,
            ContextFormatOptions {
                heading: true,
                bar_width: None,
            },
        ))
    }

    fn refresh_command_text(&self, session: &AcpSession) -> Result<String, Error> {
        let Some(runtime_session_id) = session.runtime_session_id.clone() else {
            return Ok("no runtime session yet".to_string());
        };
        let result =
            psychevo_runtime::reload_session_context(psychevo_runtime::ReloadContextOptions {
                db_path: self.options.db_path.clone(),
                session: runtime_session_id,
                config_path: self.options.config_path.clone(),
                mode: Some(session.mode),
                inherited_env: Some(self.options.inherited_env.clone()),
                agent: None,
                no_agents: false,
                no_skills: false,
                invalidation_reason: "manual_reload".to_string(),
                notice: None,
            })
            .map_err(acp_internal_error)?;
        Ok(format!(
            "reloaded context: {} v{}",
            result.prefix_hash, result.version
        ))
    }

    fn sessions_list_text(
        &self,
        session_id: &SessionId,
        session: &AcpSession,
    ) -> Result<String, Error> {
        let summaries = self.session_summaries_for(session)?;
        let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
        if let Some(current) = sessions.get_mut(&session_id.to_string()) {
            current.last_session_list = summaries.clone();
        }
        drop(sessions);
        if summaries.is_empty() {
            return Ok("No sessions found.".to_string());
        }
        let mut lines = vec!["Sessions:".to_string()];
        for (idx, summary) in summaries.iter().enumerate() {
            lines.push(format!(
                "{}. {}  {}  updated:{}  messages:{}",
                idx + 1,
                summary.title.as_deref().unwrap_or("(untitled)"),
                summary.id,
                summary.updated_at_ms,
                summary.message_count
            ));
        }
        lines.push("Use /resume <number|latest|id-prefix|title>.".to_string());
        Ok(lines.join("\n"))
    }

    fn resume_session_text(
        &self,
        session_id: &SessionId,
        session: &AcpSession,
        reference: Option<&str>,
    ) -> Result<String, Error> {
        let reference = reference.unwrap_or("latest").trim();
        let summaries = if session.last_session_list.is_empty() {
            self.session_summaries_for(session)?
        } else {
            session.last_session_list.clone()
        };
        let Some(target) = resolve_session_reference(reference, &summaries) else {
            let ambiguous = ambiguous_session_matches(reference, &summaries);
            if !ambiguous.is_empty() {
                let mut lines = vec![format!("Ambiguous session reference `{reference}`:")];
                for (idx, summary) in ambiguous.iter().enumerate() {
                    lines.push(format!(
                        "{}. {}  {}  updated:{}",
                        idx + 1,
                        summary.title.as_deref().unwrap_or("(untitled)"),
                        summary.id,
                        summary.updated_at_ms
                    ));
                }
                return Ok(lines.join("\n"));
            }
            return Ok(format!("No session matched `{reference}`."));
        };
        let store = SqliteStore::open(&self.options.db_path).map_err(acp_internal_error)?;
        store
            .resume_session(&target.id)
            .map_err(|_| Error::resource_not_found(Some(target.id.clone())))?;
        let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
        let Some(current) = sessions.get_mut(&session_id.to_string()) else {
            return Err(Error::resource_not_found(Some(session_id.to_string())));
        };
        current.runtime_session_id = Some(target.id.clone());
        current.queued_prompts.clear();
        current.pending_steers.clear();
        Ok(format!(
            "resumed session: {} {}",
            target.id,
            target.title.unwrap_or_default()
        ))
    }

    fn session_summaries_for(&self, session: &AcpSession) -> Result<Vec<SessionSummary>, Error> {
        let store = SqliteStore::open(&self.options.db_path).map_err(acp_internal_error)?;
        store
            .list_sessions_for_workdir_with_sources(&session.cwd, &[])
            .map_err(acp_internal_error)
    }

    fn model_command_text(&self, session: &AcpSession) -> Result<String, Error> {
        let options = self.run_options(session, String::new(), Vec::new(), None);
        let configured = configured_models(&options).map_err(acp_internal_error)?;
        let mut lines = vec![
            format!(
                "model: {}",
                session.model.as_deref().unwrap_or("(configured default)")
            ),
            format!(
                "variant: {}",
                session
                    .reasoning_effort
                    .as_deref()
                    .unwrap_or("(configured default)")
            ),
        ];
        if configured.is_empty() {
            lines.push("No locally configured models.".to_string());
        } else {
            lines.push("Configured models:".to_string());
            for model in configured {
                let id = if model.provider.is_empty() {
                    model.model
                } else {
                    format!("{}/{}", model.provider, model.model)
                };
                lines.push(format!("- {id} ({})", model.provider_label));
            }
        }
        Ok(lines.join("\n"))
    }

    fn set_model_text(
        &self,
        session_id: &SessionId,
        model: &str,
        variant: Option<&str>,
    ) -> Result<String, Error> {
        let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
        let Some(session) = sessions.get_mut(&session_id.to_string()) else {
            return Err(Error::resource_not_found(Some(session_id.to_string())));
        };
        session.model = Some(model.to_string());
        if let Some(variant) = variant {
            session.reasoning_effort = reasoning_effort_value(variant);
        }
        Ok(format!(
            "model: {model}{}",
            variant
                .map(|value| format!("\nvariant: {value}"))
                .unwrap_or_default()
        ))
    }

    fn set_variant_text(&self, session_id: &SessionId, variant: &str) -> Result<String, Error> {
        let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
        let Some(session) = sessions.get_mut(&session_id.to_string()) else {
            return Err(Error::resource_not_found(Some(session_id.to_string())));
        };
        session.reasoning_effort = reasoning_effort_value(variant);
        Ok(format!("variant: {variant}"))
    }

    fn set_mode_text(
        &self,
        session_id: &SessionId,
        value: &str,
        cx: &ConnectionTo<Client>,
    ) -> Result<String, Error> {
        let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
        let Some(session) = sessions.get_mut(&session_id.to_string()) else {
            return Err(Error::resource_not_found(Some(session_id.to_string())));
        };
        if let Some(mode) = RunMode::parse(value) {
            session.mode = mode;
            if mode == RunMode::Plan {
                session.permission_mode = None;
            }
            send_session_update(
                cx,
                session_id.clone(),
                SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(mode.as_str())),
            );
            return Ok(format!("mode: {}", mode.as_str()));
        }
        let Some(permission_mode) = PermissionMode::parse(value) else {
            return Err(Error::invalid_params().data(format!("unsupported mode: {value}")));
        };
        session.mode = RunMode::Default;
        session.permission_mode = Some(permission_mode);
        send_session_update(
            cx,
            session_id.clone(),
            SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(RunMode::Default.as_str())),
        );
        Ok(format!("mode: {}", permission_mode.as_str()))
    }

    fn permissions_status_text(&self, session: &AcpSession) -> Result<String, Error> {
        let options = self.run_options(session, String::new(), Vec::new(), None);
        let value =
            permission_rules_value(&options, ConfigScope::Local).map_err(acp_internal_error)?;
        let permissions = &value["permissions"];
        let mut lines = vec![
            format!("mode: {}", session.mode.as_str()),
            format!(
                "permission_mode: {}",
                session
                    .permission_mode
                    .map(PermissionMode::as_str)
                    .unwrap_or("default")
            ),
            format!(
                "approval_mode: {}",
                permissions["approval_mode"].as_str().unwrap_or("manual")
            ),
            format!(
                "path: {}",
                value["path"].as_str().unwrap_or(".psychevo/config.toml")
            ),
        ];
        for kind in ["allow", "ask", "deny"] {
            lines.push(format!("{kind}:"));
            let rules = permissions[kind].as_array().cloned().unwrap_or_default();
            if rules.is_empty() {
                lines.push("  (none)".to_string());
            } else {
                for rule in rules {
                    lines.push(format!("  {}", rule.as_str().unwrap_or("-")));
                }
            }
        }
        Ok(lines.join("\n"))
    }

    async fn compact_command_text(
        &self,
        session: &AcpSession,
        instructions: Option<String>,
    ) -> Result<String, Error> {
        let Some(runtime_session_id) = session.runtime_session_id.clone() else {
            return Ok("no runtime session yet".to_string());
        };
        let result = compact_session(CompactSessionOptions {
            db_path: self.options.db_path.clone(),
            workdir: session.cwd.clone(),
            session: runtime_session_id,
            config_path: self.options.config_path.clone(),
            model: session.model.clone(),
            reasoning_effort: session.reasoning_effort.clone(),
            inherited_env: Some(self.options.inherited_env.clone()),
            reason: CompactionReason::Manual,
            instructions,
            force: true,
        })
        .await
        .map_err(acp_internal_error)?;
        Ok(format!(
            "{}\ncompacted: {}",
            result.message, result.compacted
        ))
    }

    fn undo_options(&self, session: &AcpSession) -> Result<SessionUndoOptions, Error> {
        let Some(runtime_session_id) = session.runtime_session_id.clone() else {
            return Err(Error::invalid_params().data("no runtime session yet"));
        };
        Ok(SessionUndoOptions {
            db_path: self.options.db_path.clone(),
            workdir: session.cwd.clone(),
            snapshot_root: self.options.home.join("snapshots"),
            session_id: runtime_session_id,
        })
    }

    fn local_config_dir(&self, session: &AcpSession) -> Result<PathBuf, Error> {
        if self.options.config_path.is_some() {
            return Err(Error::invalid_params()
                .data("cannot change project-local config while PSYCHEVO_CONFIG is active"));
        }
        canonicalize_workdir(&session.cwd)
            .map(|path| path.join(".psychevo"))
            .map_err(acp_internal_error)
    }

    fn available_commands_for_session(&self, session_id: &SessionId) -> Vec<AvailableCommand> {
        let session = self
            .sessions
            .lock()
            .expect("acp session lock poisoned")
            .get(&session_id.to_string())
            .cloned();
        let Some(session) = session else {
            return available_commands_from(
                psychevo_runtime::command_registry::AvailableSlashCommands {
                    commands: Vec::new(),
                    hidden_dynamic: 0,
                },
            );
        };
        let active_turn = session.control.is_some();
        available_commands_from(self.available_commands_for_session_state(&session, active_turn))
    }

    fn available_commands_for_session_state(
        &self,
        session: &AcpSession,
        active_turn: bool,
    ) -> psychevo_runtime::command_registry::AvailableSlashCommands {
        psychevo_runtime::command_registry::available_slash_commands_for_surface(
            acp_command_capabilities(),
            active_turn,
            &self.dynamic_slash_commands(session),
            ACP_COMMAND_ADVERTISEMENT_LIMIT,
        )
    }

    fn dynamic_slash_commands(
        &self,
        session: &AcpSession,
    ) -> Vec<psychevo_runtime::command_registry::DynamicSlashCommand> {
        let mut commands = Vec::new();
        if let Ok(bundles) = list_skill_bundles(&self.options.home, &session.cwd) {
            for bundle in bundles {
                commands.push(psychevo_runtime::command_registry::DynamicSlashCommand {
                    name: bundle.slug.clone(),
                    summary: bundle.description,
                    prompt: psychevo_runtime::command_registry::skill_prompt_marker(
                        &bundle.slug,
                        "",
                    ),
                });
            }
        }
        if let Ok(catalog) = discover_skills(&SkillDiscoveryOptions {
            home: self.options.home.clone(),
            workdir: session.cwd.clone(),
            config_path: self.options.config_path.clone(),
            env: self.options.inherited_env.clone(),
            explicit_inputs: Vec::new(),
            no_skills: false,
        }) {
            for skill in catalog.skills {
                commands.push(psychevo_runtime::command_registry::DynamicSlashCommand {
                    name: skill.name.clone(),
                    summary: skill.description,
                    prompt: psychevo_runtime::command_registry::skill_prompt_marker(
                        &skill.name,
                        "",
                    ),
                });
            }
        }
        commands.sort_by(|left, right| left.name.cmp(&right.name));
        commands
    }

    fn queue_prompt(&self, session_id: &SessionId, prompt: String) -> Result<(), Error> {
        let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
        let Some(session) = sessions.get_mut(&session_id.to_string()) else {
            return Err(Error::resource_not_found(Some(session_id.to_string())));
        };
        session.queued_prompts.push_back(prompt);
        Ok(())
    }

    fn pop_queued_prompt(&self, session_id: &SessionId) -> Option<String> {
        self.sessions
            .lock()
            .expect("acp session lock poisoned")
            .get_mut(&session_id.to_string())
            .and_then(|session| session.queued_prompts.pop_front())
    }

    fn cancel_pending_inputs(&self, session_id: &SessionId) -> usize {
        let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
        let Some(session) = sessions.get_mut(&session_id.to_string()) else {
            return 0;
        };
        let control = session.control.clone();
        let mut canceled = 0usize;
        for id in session.pending_steers.drain(..) {
            if control
                .as_ref()
                .is_some_and(|control| control.cancel_pending_user_message(id))
            {
                canceled += 1;
            }
        }
        let queued = session.queued_prompts.len();
        session.queued_prompts.clear();
        canceled + queued
    }

    fn apply_steer_effect(
        &self,
        session_id: &SessionId,
        message: &str,
        cx: &ConnectionTo<Client>,
    ) -> Result<SlashPromptAction, Error> {
        let prompt = message.trim().to_string();
        let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
        let Some(session) = sessions.get_mut(&session_id.to_string()) else {
            return Err(Error::resource_not_found(Some(session_id.to_string())));
        };
        let Some(control) = session.control.clone() else {
            return Ok(SlashPromptAction::RunPrompt(prompt));
        };
        let Some(id) = control.steer_user_message(user_text_message(&prompt)) else {
            session.queued_prompts.push_back(prompt.clone());
            return Ok(send_slash_text(
                cx,
                session_id,
                format!("turn is not ready for steering; queued prompt: {prompt}"),
            ));
        };
        session.pending_steers.push(id);
        Ok(send_slash_text(
            cx,
            session_id,
            format!("steer queued: {prompt}"),
        ))
    }

    async fn request_command_approval(
        &self,
        session_id: &SessionId,
        cx: &ConnectionTo<Client>,
        command: &str,
        reason: &str,
    ) -> bool {
        let tool_call = ToolCallUpdate::new(
            format!("slash_command_{}", Uuid::now_v7()),
            ToolCallUpdateFields::new()
                .title(format!("Command: {command}"))
                .status(ToolCallStatus::Pending)
                .raw_input(json!({
                    "command": command,
                    "reason": reason,
                })),
        );
        let options = vec![
            PermissionOption::new("allow_once", "Allow once", PermissionOptionKind::AllowOnce),
            PermissionOption::new("deny", "Deny", PermissionOptionKind::RejectOnce),
        ];
        match cx
            .send_request(RequestPermissionRequest::new(
                session_id.clone(),
                tool_call,
                options,
            ))
            .block_task()
            .await
        {
            Ok(response) => matches!(
                response.outcome,
                RequestPermissionOutcome::Selected(selected)
                    if selected.option_id.to_string() == "allow_once"
            ),
            Err(_) => false,
        }
    }

    async fn skills_command_text(
        &self,
        session_id: &SessionId,
        session: &AcpSession,
        args: Option<&str>,
        cx: &ConnectionTo<Client>,
    ) -> Result<String, Error> {
        let Some(args) = args.map(str::trim).filter(|value| !value.is_empty()) else {
            return self.skills_dashboard_text(session);
        };
        let mut parts = args.split_whitespace().collect::<Vec<_>>();
        let action = parts.remove(0).to_ascii_lowercase();
        match action.as_str() {
            "help" | "--help" | "-h" => self.skills_dashboard_text(session),
            "list" => self.skills_list_text(session, None),
            "browse" | "search" => self.skills_list_text(session, Some(&parts.join(" "))),
            "inspect" => self.skills_inspect_text(session, parts.first().copied()),
            "check" => Ok(self.skills_check_text(session)),
            "audit" => self.skills_audit_text(session, &parts),
            "reload" => Ok(self.skills_reload_text(session)),
            "install" | "uninstall" | "config" => {
                if !self
                    .request_command_approval(session_id, cx, "/skills", "change local skill state")
                    .await
                {
                    return Ok("permission denied".to_string());
                }
                self.skills_mutation_text(session, action.as_str(), &parts)
            }
            _ => Ok(format!(
                "unknown /skills action: {action}\nSupported: list, browse, search, inspect, check, audit, reload"
            )),
        }
    }

    fn skills_dashboard_text(&self, session: &AcpSession) -> Result<String, Error> {
        let catalog = self.skill_catalog(session)?;
        let bundles = list_skill_bundles(&self.options.home, &session.cwd).unwrap_or_default();
        Ok([
            "Skills hub".to_string(),
            format!(
                "installed: {} skills, {} bundles",
                catalog.skills.len(),
                bundles.len()
            ),
            "/skills list - list installed skills".to_string(),
            "/skills search <query> - search installed skills".to_string(),
            "/skills inspect <name> - show local skill metadata".to_string(),
            "/skills check - check configured hub updates".to_string(),
            "/skills audit [name] - scan local skills".to_string(),
            "/skills reload - refresh skill context".to_string(),
            "/skills install <identifier-or-path> [--scope global|project] [--name <name>]"
                .to_string(),
            "/skills uninstall <name>".to_string(),
            "/skills config enable|disable <name> [--scope global|project]".to_string(),
        ]
        .join("\n"))
    }

    fn skills_list_text(&self, session: &AcpSession, query: Option<&str>) -> Result<String, Error> {
        let catalog = self.skill_catalog(session)?;
        let query = query.map(str::trim).filter(|value| !value.is_empty());
        let mut rows = catalog
            .skills
            .iter()
            .filter(|skill| {
                query.is_none_or(|query| {
                    let query = query.to_ascii_lowercase();
                    skill.name.to_ascii_lowercase().contains(&query)
                        || skill.description.to_ascii_lowercase().contains(&query)
                })
            })
            .map(|skill| format!("{}: {}", skill.name, skill.description))
            .collect::<Vec<_>>();
        rows.sort();
        if rows.is_empty() {
            Ok("No skills found.".to_string())
        } else {
            Ok(rows.join("\n"))
        }
    }

    fn skills_inspect_text(
        &self,
        session: &AcpSession,
        name: Option<&str>,
    ) -> Result<String, Error> {
        let Some(name) = name else {
            return Ok("usage: /skills inspect <name>".to_string());
        };
        let catalog = self.skill_catalog(session)?;
        let Some(skill) = catalog.skills.iter().find(|skill| {
            skill.name == name
                || psychevo_runtime::command_registry::normalize_dynamic_skill_name(&skill.name)
                    == psychevo_runtime::command_registry::normalize_dynamic_skill_name(name)
        }) else {
            return Ok(format!("skill not found: {name}"));
        };
        Ok(format!(
            "{}\n{}\npath: {}",
            skill.name,
            skill.description,
            skill.file_path.display()
        ))
    }

    fn skills_check_text(&self, session: &AcpSession) -> String {
        let skill_count = self
            .skill_catalog(session)
            .map(|catalog| catalog.skills.len())
            .unwrap_or(0);
        let bundle_count = list_skill_bundles(&self.options.home, &session.cwd)
            .map(|bundles| bundles.len())
            .unwrap_or(0);
        format!(
            "no hub update source configured\ninstalled: {skill_count} skills, {bundle_count} bundles"
        )
    }

    fn skills_audit_text(&self, session: &AcpSession, args: &[&str]) -> Result<String, Error> {
        let catalog = self.skill_catalog(session)?;
        if let Some(name) = args.first() {
            let normalized = psychevo_runtime::command_registry::normalize_dynamic_skill_name(name);
            let Some(skill) = catalog.skills.iter().find(|skill| {
                skill.name == *name
                    || psychevo_runtime::command_registry::normalize_dynamic_skill_name(&skill.name)
                        == normalized
            }) else {
                return Ok(format!("unknown skill: {name}"));
            };
            return scan_skill_path(&skill.base_dir)
                .map(|scan| {
                    format!(
                        "{}: {:?} ({} findings)",
                        skill.name,
                        scan.verdict,
                        scan.findings.len()
                    )
                })
                .map_err(acp_internal_error);
        }
        if catalog.skills.is_empty() {
            return Ok("No skills found.".to_string());
        }
        Ok(catalog
            .skills
            .iter()
            .map(|skill| match scan_skill_path(&skill.base_dir) {
                Ok(scan) => format!(
                    "{}: {:?} ({} findings)",
                    skill.name,
                    scan.verdict,
                    scan.findings.len()
                ),
                Err(err) => format!("{}: error: {err:#}", skill.name),
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn skills_reload_text(&self, session: &AcpSession) -> String {
        let skill_count = self
            .skill_catalog(session)
            .map(|catalog| catalog.skills.len())
            .unwrap_or(0);
        let bundle_count = list_skill_bundles(&self.options.home, &session.cwd)
            .map(|bundles| bundles.len())
            .unwrap_or(0);
        format!("reloaded skills: {skill_count} skills, {bundle_count} bundles")
    }

    fn skills_mutation_text(
        &self,
        session: &AcpSession,
        action: &str,
        args: &[&str],
    ) -> Result<String, Error> {
        match action {
            "install" => {
                let Some(source) = args.first() else {
                    return Ok("usage: /skills install <identifier-or-path> [--scope global|project] [--name <name>]".to_string());
                };
                let value = install_skill(
                    &self.options.home,
                    &session.cwd,
                    InstallOptions {
                        source: (*source).to_string(),
                        target: skill_scope_from_args(args),
                        name: skill_option_value(args, "--name").map(ToOwned::to_owned),
                        all: args.contains(&"--all"),
                        force: args.contains(&"--force"),
                    },
                )
                .map_err(acp_internal_error)?;
                serde_json::to_string_pretty(&value).map_err(acp_internal_error)
            }
            "uninstall" => {
                let Some(name) = args.first() else {
                    return Ok("usage: /skills uninstall <name>".to_string());
                };
                let catalog = self.skill_catalog(session)?;
                let value = remove_skill(&catalog, &self.options.home, &session.cwd, name)
                    .map_err(acp_internal_error)?;
                serde_json::to_string_pretty(&value).map_err(acp_internal_error)
            }
            "config" => self.skills_config_mutation_text(session, args),
            _ => Ok("unsupported skill mutation".to_string()),
        }
    }

    fn skills_config_mutation_text(
        &self,
        session: &AcpSession,
        args: &[&str],
    ) -> Result<String, Error> {
        let Some(action) = args.first() else {
            return Ok("usage: /skills config enable|disable|set ...".to_string());
        };
        match *action {
            "enable" | "disable" => {
                let Some(name) = args.get(1) else {
                    return Ok(format!(
                        "usage: /skills config {action} <name> [--scope global|project]"
                    ));
                };
                let value = set_skill_enabled(
                    &self.options.home,
                    &session.cwd,
                    skill_scope_from_args(args),
                    name,
                    *action == "enable",
                )
                .map_err(acp_internal_error)?;
                serde_json::to_string_pretty(&value).map_err(acp_internal_error)
            }
            "set" => {
                let filtered = skill_args_without_scope(args);
                if filtered.len() < 3 {
                    return Ok("usage: /skills config set skills.config.<key> <value> [--scope global|project]".to_string());
                }
                let value = serde_json::from_str::<Value>(filtered[2])
                    .unwrap_or_else(|_| Value::String(filtered[2].to_string()));
                let value = set_skill_config_value(
                    &self.options.home,
                    &session.cwd,
                    skill_scope_from_args(args),
                    filtered[1],
                    value,
                )
                .map_err(acp_internal_error)?;
                serde_json::to_string_pretty(&value).map_err(acp_internal_error)
            }
            other => Ok(format!("unknown /skills config action: {other}")),
        }
    }

    fn skill_catalog(&self, session: &AcpSession) -> Result<psychevo_runtime::SkillCatalog, Error> {
        discover_skills(&SkillDiscoveryOptions {
            home: self.options.home.clone(),
            workdir: session.cwd.clone(),
            config_path: self.options.config_path.clone(),
            env: self.options.inherited_env.clone(),
            explicit_inputs: Vec::new(),
            no_skills: false,
        })
        .map_err(acp_internal_error)
    }

    fn bundles_command_text(
        &self,
        session: &AcpSession,
        args: Option<&str>,
    ) -> Result<String, Error> {
        match args.map(str::trim).filter(|value| !value.is_empty()) {
            None | Some("list") => {
                let bundles = list_skill_bundles(&self.options.home, &session.cwd)
                    .map_err(acp_internal_error)?;
                if bundles.is_empty() {
                    return Ok("No skill bundles found.".to_string());
                }
                Ok(bundles
                    .into_iter()
                    .map(|bundle| {
                        format!(
                            "{}: {} [{}]",
                            bundle.slug,
                            bundle.description,
                            bundle.skills.join(", ")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            Some(_) => Ok("Supported bundle commands: /bundles, /bundles list".to_string()),
        }
    }

    fn curator_command_text(&self, args: Option<&str>) -> String {
        match args.map(str::trim).filter(|value| !value.is_empty()) {
            None | Some("status") => [
                "Skill curator",
                "status: enabled",
                "scope: global",
                "automatic destructive actions: disabled",
            ]
            .join("\n"),
            Some(_) => "Supported curator commands: /curator, /curator status".to_string(),
        }
    }

    fn write_artifact_text(
        &self,
        session: &AcpSession,
        artifact_kind: SessionArtifactKind,
        args: Option<&str>,
    ) -> Result<String, Error> {
        let Some(runtime_session_id) = session.runtime_session_id.as_deref() else {
            return Ok("no runtime session yet".to_string());
        };
        let parsed = parse_artifact_args(args.unwrap_or(""), artifact_kind)
            .map_err(|message| Error::invalid_params().data(message))?;
        let format = parsed.format.unwrap_or(SessionExportFormat::Markdown);
        let include = parsed
            .include
            .unwrap_or_else(|| SessionExportIncludeSet::default_for(artifact_kind));
        let path = parsed.path.unwrap_or_else(|| {
            session.cwd.join(default_session_export_filename(
                runtime_session_id,
                format,
                artifact_kind,
            ))
        });
        let path = if path.is_absolute() {
            path
        } else {
            session.cwd.join(path)
        };
        let store = SqliteStore::open(&self.options.db_path).map_err(acp_internal_error)?;
        let result = psychevo_runtime::write_session_export(
            &store,
            runtime_session_id,
            &path,
            SessionExportOptions {
                format,
                include,
                artifact_kind,
            },
        )
        .map_err(acp_internal_error)?;
        Ok(format!(
            "{}: {} ({} bytes)",
            artifact_kind.as_str(),
            result.path.display(),
            result.bytes
        ))
    }

    fn auth_methods(&self) -> Vec<AuthMethod> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let options = RunOptions {
            db_path: self.options.db_path.clone(),
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
            model: None,
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
        };
        model_catalog_providers(&options)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|provider| provider.api_key_env)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|env_name| {
                AuthMethod::EnvVar(AuthMethodEnvVar::new(
                    format!("env:{env_name}"),
                    env_name.clone(),
                    vec![AuthEnvVar::new(env_name.clone()).label(env_name)],
                ))
            })
            .collect()
    }
}

enum SlashPromptAction {
    NotSlashOrPassThrough,
    Handled(PromptResponse),
    RunPrompt(String),
}

const ACP_COMMAND_ADVERTISEMENT_LIMIT: usize = 100;

fn acp_command_capabilities() -> &'static [psychevo_runtime::command_registry::CommandCapability] {
    use psychevo_runtime::command_registry::CommandCapability;
    &[
        CommandCapability::ActiveTurnControl,
        CommandCapability::Queue,
        CommandCapability::SessionSwitch,
        CommandCapability::ArtifactWrite,
        CommandCapability::ConfigWrite,
        CommandCapability::PolicyWrite,
        CommandCapability::SkillStateWrite,
    ]
}

fn send_slash_text(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    text: impl Into<String>,
) -> SlashPromptAction {
    send_session_update(
        cx,
        session_id.clone(),
        SessionUpdate::AgentMessageChunk(ContentChunk::new(text.into().into())),
    );
    SlashPromptAction::Handled(PromptResponse::new(StopReason::EndTurn))
}

fn user_text_message(text: &str) -> Message {
    Message::User {
        content: vec![UserContentBlock::text(text)],
        timestamp_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64,
    }
}

fn resolve_session_reference(
    reference: &str,
    sessions: &[SessionSummary],
) -> Option<SessionSummary> {
    if sessions.is_empty() {
        return None;
    }
    if reference.is_empty() || reference == "latest" {
        return sessions.first().cloned();
    }
    if let Ok(index) = reference.parse::<usize>()
        && index > 0
    {
        return sessions.get(index - 1).cloned();
    }
    let id_matches = sessions
        .iter()
        .filter(|summary| summary.id.starts_with(reference))
        .cloned()
        .collect::<Vec<_>>();
    if id_matches.len() == 1 {
        return id_matches.into_iter().next();
    }
    let title_matches = sessions
        .iter()
        .filter(|summary| summary.title.as_deref() == Some(reference))
        .cloned()
        .collect::<Vec<_>>();
    (title_matches.len() == 1)
        .then(|| title_matches.into_iter().next())
        .flatten()
}

fn ambiguous_session_matches(reference: &str, sessions: &[SessionSummary]) -> Vec<SessionSummary> {
    if reference.is_empty() || reference == "latest" {
        return Vec::new();
    }
    let id_matches = sessions
        .iter()
        .filter(|summary| summary.id.starts_with(reference))
        .cloned()
        .collect::<Vec<_>>();
    if id_matches.len() > 1 {
        return id_matches;
    }
    let title_matches = sessions
        .iter()
        .filter(|summary| summary.title.as_deref() == Some(reference))
        .cloned()
        .collect::<Vec<_>>();
    if title_matches.len() > 1 {
        title_matches
    } else {
        Vec::new()
    }
}

fn reasoning_effort_value(value: &str) -> Option<String> {
    (value != "none").then(|| value.to_string())
}

fn available_commands_from(
    available: psychevo_runtime::command_registry::AvailableSlashCommands,
) -> Vec<AvailableCommand> {
    available
        .commands
        .into_iter()
        .map(|command| {
            let description = if command.aliases.is_empty() {
                command.summary
            } else {
                format!(
                    "{} (aliases: {})",
                    command.summary,
                    command.aliases.join(", ")
                )
            };
            let input = match command.argument_kind {
                psychevo_runtime::command_registry::CommandArgumentKind::None => None,
                _ => Some(AvailableCommandInput::Unstructured(
                    UnstructuredCommandInput::new(command.usage),
                )),
            };
            AvailableCommand::new(command.name, description).input(input)
        })
        .collect()
}

fn available_command_lines_from(commands: Vec<AvailableCommand>) -> Vec<String> {
    commands
        .into_iter()
        .map(|command| {
            let input_hint = command
                .input
                .as_ref()
                .map(|input| match input {
                    AvailableCommandInput::Unstructured(input) => input.hint.clone(),
                    _ => String::new(),
                })
                .unwrap_or_default();
            let display = if input_hint.starts_with('/') {
                input_hint
            } else if input_hint.is_empty() {
                format!("/{}", command.name)
            } else {
                format!("/{} {}", command.name, input_hint)
            };
            format!("- {display} - {}", command.description)
        })
        .collect()
}

struct ParsedArtifactArgs {
    path: Option<PathBuf>,
    format: Option<SessionExportFormat>,
    include: Option<SessionExportIncludeSet>,
}

fn parse_artifact_args(
    args: &str,
    artifact_kind: SessionArtifactKind,
) -> std::result::Result<ParsedArtifactArgs, String> {
    let tokens = args.split_whitespace().collect::<Vec<_>>();
    let mut path = None;
    let mut format = None;
    let mut include = None;
    let mut index = 0usize;
    while index < tokens.len() {
        match tokens[index] {
            "--format" | "-f" if artifact_kind == SessionArtifactKind::Export => {
                index += 1;
                let Some(value) = tokens.get(index) else {
                    return Err(
                        "usage: /export [path] [-f|--format markdown|json] [-i|--include list]"
                            .to_string(),
                    );
                };
                format = Some(parse_export_format(value)?);
            }
            value
                if artifact_kind == SessionArtifactKind::Export
                    && value.starts_with("--format=") =>
            {
                format = Some(parse_export_format(value.trim_start_matches("--format="))?);
            }
            "--include" | "-i" => {
                index += 1;
                let Some(value) = tokens.get(index) else {
                    return Err("usage: /export|/share [path] [-i|--include list]".to_string());
                };
                include = Some(
                    SessionExportIncludeSet::parse(value, artifact_kind)
                        .map_err(|err| err.to_string())?,
                );
            }
            value if value.starts_with("--include=") => {
                include = Some(
                    SessionExportIncludeSet::parse(
                        value.trim_start_matches("--include="),
                        artifact_kind,
                    )
                    .map_err(|err| err.to_string())?,
                );
            }
            value if value.starts_with('-') => {
                return Err(format!("unsupported option: {value}"));
            }
            value => {
                if path.is_some() {
                    return Err("only one output path is supported".to_string());
                }
                path = Some(PathBuf::from(value));
            }
        }
        index += 1;
    }
    Ok(ParsedArtifactArgs {
        path,
        format,
        include,
    })
}

fn parse_export_format(value: &str) -> std::result::Result<SessionExportFormat, String> {
    match value {
        "markdown" | "md" => Ok(SessionExportFormat::Markdown),
        "json" => Ok(SessionExportFormat::Json),
        _ => Err("format must be markdown or json".to_string()),
    }
}

fn skill_scope_from_args(args: &[&str]) -> SkillTarget {
    match skill_option_value(args, "--scope") {
        Some("project") | Some("local") => SkillTarget::Project,
        _ => SkillTarget::Global,
    }
}

fn skill_option_value<'a>(args: &'a [&str], option: &str) -> Option<&'a str> {
    args.windows(2)
        .find_map(|window| (window[0] == option).then_some(window[1]))
}

fn skill_args_without_scope<'a>(args: &'a [&str]) -> Vec<&'a str> {
    let mut filtered = Vec::new();
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if *arg == "--scope" {
            skip_next = true;
            continue;
        }
        filtered.push(*arg);
    }
    filtered
}

#[derive(Clone)]
struct AcpApprovalHandler {
    session_id: SessionId,
    cx: ConnectionTo<Client>,
}

impl fmt::Debug for AcpApprovalHandler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AcpApprovalHandler")
            .field("session_id", &self.session_id)
            .finish_non_exhaustive()
    }
}

impl ApprovalHandler for AcpApprovalHandler {
    fn request_permission(
        &self,
        request: PermissionApprovalRequest,
    ) -> BoxFuture<'static, PermissionApprovalDecision> {
        let session_id = self.session_id.clone();
        let cx = self.cx.clone();
        Box::pin(async move {
            let tool_call = ToolCallUpdate::new(
                request.tool_call_id.clone(),
                ToolCallUpdateFields::new()
                    .title(format!("Permission: {}", request.tool_name))
                    .status(ToolCallStatus::Pending)
                    .raw_input(json!({
                        "summary": request.summary,
                        "reason": request.reason,
                        "matched_rule": request.matched_rule,
                        "suggested_rule": request.suggested_rule,
                    })),
            );
            let mut options = vec![
                PermissionOption::new("allow_once", "Allow once", PermissionOptionKind::AllowOnce),
                PermissionOption::new(
                    "allow_session",
                    "Allow for session",
                    PermissionOptionKind::AllowAlways,
                ),
                PermissionOption::new("deny", "Deny", PermissionOptionKind::RejectOnce),
            ];
            if request.allow_always {
                options.insert(
                    2,
                    PermissionOption::new(
                        "allow_always",
                        "Allow always",
                        PermissionOptionKind::AllowAlways,
                    ),
                );
            }
            match cx
                .send_request(RequestPermissionRequest::new(
                    session_id, tool_call, options,
                ))
                .block_task()
                .await
            {
                Ok(response) => match response.outcome {
                    RequestPermissionOutcome::Cancelled => PermissionApprovalDecision::deny(),
                    RequestPermissionOutcome::Selected(selected) => {
                        match selected.option_id.to_string().as_str() {
                            "allow_once" => PermissionApprovalDecision::allow_once(),
                            "allow_session" => PermissionApprovalDecision::allow_session(),
                            "allow_always" => PermissionApprovalDecision::allow_always(),
                            _ => PermissionApprovalDecision::deny(),
                        }
                    }
                    _ => PermissionApprovalDecision::deny(),
                },
                Err(_) => PermissionApprovalDecision::deny(),
            }
        })
    }
}

fn send_session_setup_updates(
    cx: &ConnectionTo<Client>,
    session_id: SessionId,
    mode: RunMode,
    commands: Vec<AvailableCommand>,
) {
    send_session_update(
        cx,
        session_id.clone(),
        SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(mode.as_str())),
    );
    send_session_update(
        cx,
        session_id.clone(),
        SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(session_config_options(mode))),
    );
    send_session_update(
        cx,
        session_id,
        SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(commands)),
    );
}

fn send_session_update(cx: &ConnectionTo<Client>, session_id: SessionId, update: SessionUpdate) {
    let _ = cx.send_notification(SessionNotification::new(session_id, update));
}

fn send_run_stream_update(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    event: RunStreamEvent,
) {
    match event {
        RunStreamEvent::ReasoningDelta { text } => send_session_update(
            cx,
            session_id.clone(),
            SessionUpdate::AgentThoughtChunk(ContentChunk::new(text.into())),
        ),
        RunStreamEvent::Event(value) => send_runtime_event_update(cx, session_id, value),
        RunStreamEvent::Scoped { event, .. } => send_run_stream_update(cx, session_id, *event),
        RunStreamEvent::ReasoningEnd
        | RunStreamEvent::ClarifyRequest(_)
        | RunStreamEvent::ClarifyResolved(_) => {}
    }
}

fn send_runtime_event_update(cx: &ConnectionTo<Client>, session_id: &SessionId, value: Value) {
    let Some(event_type) = value.get("type").and_then(Value::as_str) else {
        return;
    };
    match event_type {
        "tool_execution_start" => {
            let call_id = value
                .get("tool_call_id")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let tool_name = value
                .get("tool_name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let args = value.get("args").cloned();
            send_session_update(
                cx,
                session_id.clone(),
                SessionUpdate::ToolCall(
                    ToolCall::new(call_id.to_string(), tool_title(tool_name))
                        .kind(tool_kind(tool_name))
                        .status(ToolCallStatus::InProgress)
                        .raw_input(args),
                ),
            );
        }
        "tool_execution_end" => {
            let call_id = value
                .get("tool_call_id")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let tool_name = value
                .get("tool_name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let result = value.get("result").cloned();
            let failed = value
                .get("outcome")
                .and_then(Value::as_str)
                .is_some_and(|outcome| outcome != "normal");
            let content = result
                .as_ref()
                .map(compact_tool_result_text)
                .filter(|text| !text.is_empty())
                .map(|text| vec![ToolCallContent::from(text)])
                .unwrap_or_default();
            send_session_update(
                cx,
                session_id.clone(),
                SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                    call_id.to_string(),
                    ToolCallUpdateFields::new()
                        .title(tool_title(tool_name))
                        .status(if failed {
                            ToolCallStatus::Failed
                        } else {
                            ToolCallStatus::Completed
                        })
                        .content(content)
                        .raw_output(result),
                )),
            );
        }
        _ => {}
    }
}

fn prompt_parts(prompt: Vec<ContentBlock>) -> (String, Vec<ImageInput>) {
    let mut text = Vec::new();
    let mut images = Vec::new();
    for block in prompt {
        match block {
            ContentBlock::Text(content) => text.push(content.text),
            ContentBlock::Image(content) => {
                if let Some(uri) = content.uri {
                    if uri.starts_with("http://") || uri.starts_with("https://") {
                        images.push(ImageInput::ImageUrl(uri));
                    } else {
                        text.push(format!("[image: {uri}]"));
                    }
                } else {
                    text.push("[embedded image omitted]".to_string());
                }
            }
            other => {
                if let Ok(serialized) = serde_json::to_string(&other) {
                    text.push(serialized);
                }
            }
        }
    }
    (text.join("\n\n"), images)
}

fn acp_mcp_servers(servers: Vec<McpServer>) -> Vec<McpServerInput> {
    servers
        .into_iter()
        .map(|server| match server {
            McpServer::Http(McpServerHttp {
                name, url, headers, ..
            }) => McpServerInput {
                name,
                transport: McpTransportInput::StreamableHttp {
                    url,
                    headers: headers
                        .into_iter()
                        .map(|header| (header.name, header.value))
                        .collect(),
                },
            },
            McpServer::Stdio(McpServerStdio {
                name,
                command,
                args,
                env,
                ..
            }) => McpServerInput {
                name,
                transport: McpTransportInput::Stdio {
                    command,
                    args,
                    env: env_variable_map(env),
                },
            },
            McpServer::Sse(server) => McpServerInput {
                name: server.name,
                transport: McpTransportInput::Unsupported {
                    kind: "sse".to_string(),
                },
            },
            McpServer::Acp(server) => McpServerInput {
                name: server.name,
                transport: McpTransportInput::Unsupported {
                    kind: "acp".to_string(),
                },
            },
            _ => McpServerInput {
                name: "unknown".to_string(),
                transport: McpTransportInput::Unsupported {
                    kind: "unknown".to_string(),
                },
            },
        })
        .collect()
}

fn env_variable_map(vars: Vec<EnvVariable>) -> BTreeMap<String, String> {
    vars.into_iter().map(|var| (var.name, var.value)).collect()
}

fn mode_state(mode: RunMode) -> SessionModeState {
    SessionModeState::new(
        mode.as_str(),
        vec![
            SessionMode::new("default", "Default").description("Run tools and edit code"),
            SessionMode::new("plan", "Plan").description("Discuss and inspect without edits"),
        ],
    )
}

fn session_config_options(
    mode: RunMode,
) -> Vec<agent_client_protocol::schema::SessionConfigOption> {
    vec![agent_client_protocol::schema::SessionConfigOption::select(
        "mode",
        "Mode",
        mode.as_str(),
        vec![
            SessionConfigSelectOption::new("default", "Default"),
            SessionConfigSelectOption::new("plan", "Plan"),
        ],
    )]
}

fn tool_title(tool_name: &str) -> String {
    if let Some(rest) = tool_name.strip_prefix("mcp__")
        && let Some((server, tool)) = rest.split_once("__")
    {
        return format!("Tool: {server}/{tool}");
    }
    format!("Tool: {tool_name}")
}

fn tool_kind(tool_name: &str) -> ToolKind {
    match tool_name {
        "read" => ToolKind::Read,
        "write" | "edit" => ToolKind::Edit,
        "exec_command" | "write_stdin" => ToolKind::Execute,
        "web_fetch" => ToolKind::Fetch,
        _ => ToolKind::Other,
    }
}

fn compact_tool_result_text(value: &Value) -> String {
    value
        .get("model_content")
        .and_then(Value::as_str)
        .or_else(|| value.get("error").and_then(Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| serde_json::to_string(value).unwrap_or_default())
}

fn stop_reason(outcome: psychevo_ai::Outcome) -> StopReason {
    match outcome {
        psychevo_ai::Outcome::Normal => StopReason::EndTurn,
        psychevo_ai::Outcome::Aborted => StopReason::Cancelled,
        psychevo_ai::Outcome::Stopped => StopReason::EndTurn,
        psychevo_ai::Outcome::Failed => StopReason::Refusal,
    }
}

fn acp_internal_error(err: impl std::fmt::Display) -> Error {
    Error::internal_error().data(err.to_string())
}

fn env_path_or_default(
    env: &BTreeMap<String, String>,
    name: &str,
    default: &str,
    cwd: &Path,
) -> PathBuf {
    env.get(name)
        .filter(|value| !value.trim().is_empty())
        .map(String::as_str)
        .unwrap_or(default)
        .pipe(|value| resolve_path(value, env, cwd))
}

fn resolve_path(value: &str, env: &BTreeMap<String, String>, cwd: &Path) -> PathBuf {
    let path = if value == "~" {
        env.get("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| cwd.to_path_buf())
    } else if let Some(rest) = value.strip_prefix("~/") {
        env.get("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| cwd.to_path_buf())
            .join(rest)
    } else {
        PathBuf::from(value)
    };
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_acp_mcp_servers_to_runtime_inputs() {
        let servers = vec![McpServer::Stdio(
            McpServerStdio::new("repo tools", "server")
                .args(vec!["--stdio".to_string()])
                .env(vec![EnvVariable::new("A", "B")]),
        )];
        let converted = acp_mcp_servers(servers);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].name, "repo tools");
        match &converted[0].transport {
            McpTransportInput::Stdio { args, env, .. } => {
                assert_eq!(args, &vec!["--stdio".to_string()]);
                assert_eq!(env.get("A").map(String::as_str), Some("B"));
            }
            other => panic!("unexpected transport: {other:?}"),
        }
    }

    #[test]
    fn converts_prompt_text_and_http_images() {
        let (text, images) = prompt_parts(vec![
            ContentBlock::Text(agent_client_protocol::schema::TextContent::new("hello")),
            ContentBlock::Image(
                agent_client_protocol::schema::ImageContent::new("", "image/png")
                    .uri("https://example.com/a.png"),
            ),
        ]);
        assert_eq!(text, "hello");
        assert_eq!(
            images,
            vec![ImageInput::ImageUrl(
                "https://example.com/a.png".to_string()
            )]
        );
    }

    #[test]
    fn advertises_tools_slash_command() {
        let commands = available_command_lines_from(available_commands_from(
            psychevo_runtime::command_registry::available_slash_commands_for_surface(
                acp_command_capabilities(),
                false,
                &[],
                ACP_COMMAND_ADVERTISEMENT_LIMIT,
            ),
        ))
        .join("\n");
        assert!(
            commands.contains("/tools [list|enable|disable <toolset>] - toolsets"),
            "{commands}"
        );
    }

    #[test]
    fn parses_slash_prompt_command_and_args() {
        use psychevo_runtime::command_registry::{
            SlashCommandAction, SlashCommandParse, parse_slash_command_line,
        };

        let SlashCommandParse::Known(invocation) = parse_slash_command_line(" /tools ") else {
            panic!("expected known command");
        };
        assert_eq!(invocation.spec.action, SlashCommandAction::Tools);
        assert!(invocation.args.is_empty());

        let SlashCommandParse::Known(invocation) = parse_slash_command_line("/mode plan") else {
            panic!("expected known command");
        };
        assert_eq!(invocation.spec.action, SlashCommandAction::ModeSet);
        assert_eq!(invocation.args, "plan");

        assert!(matches!(
            parse_slash_command_line("hello /tools"),
            SlashCommandParse::NotSlash
        ));
    }

    #[test]
    fn handles_status_slash_command_locally() {
        let agent = PsychevoAcpAgent::new(AcpOptions {
            home: std::env::temp_dir().join("psychevo-acp-test-home"),
            db_path: PathBuf::from(":memory:"),
            config_path: None,
            inherited_env: BTreeMap::new(),
        });
        let session_id = SessionId::new("acp-test");
        let session = AcpSession::new(
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            None,
            Vec::new(),
        );
        let text = agent.status_command_text(&session_id, &session);
        assert!(text.contains("ACP session: acp-test"), "{text}");
        assert!(text.contains("commands: "), "{text}");
    }
}
