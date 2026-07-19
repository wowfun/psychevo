use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentErrorStage {
    Configuration,
    Binding,
    Control,
    Delivery,
    History,
    Interaction,
}

impl AgentErrorStage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::Binding => "binding",
            Self::Control => "control",
            Self::Delivery => "delivery",
            Self::History => "history",
            Self::Interaction => "interaction",
        }
    }
}

fn agent_session_error(
    code: &str,
    stage: AgentErrorStage,
    retry_class: &str,
    delivery: &str,
    message: impl Into<String>,
    diagnostic_ref: Option<String>,
) -> Error {
    let message = message.into();
    Error::structured(
        message.clone(),
        json!({
            "code": code,
            "stage": stage.as_str(),
            "retryClass": retry_class,
            "delivery": delivery,
            "message": message,
            "diagnosticRef": diagnostic_ref,
        }),
    )
}

pub(crate) fn agent_error_view(message: impl Into<String>, data: Option<&Value>) -> AgentErrorView {
    let message = message.into();
    let nested_error = data.and_then(|value| value.get("error"));
    let field = |name: &str| {
        data.and_then(|value| value.get(name))
            .or_else(|| nested_error.and_then(|value| value.get(name)))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let delivery = match field("delivery").as_deref() {
        Some("not_delivered" | "notDelivered") => AgentDeliveryStatusView::NotDelivered,
        Some("delivered") => AgentDeliveryStatusView::Delivered,
        Some("unknown") | Some(_) | None => AgentDeliveryStatusView::Unknown,
    };
    AgentErrorView {
        message,
        code: field("code"),
        stage: field("stage"),
        retry_class: field("retryClass"),
        delivery,
        recovery_action: field("recoveryAction"),
        diagnostic_ref: field("diagnosticRef"),
    }
}

fn agent_session_configuration_error(message: impl Into<String>) -> Error {
    agent_session_error(
        "configuration",
        AgentErrorStage::Configuration,
        "user_action",
        "not_delivered",
        message,
        None,
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct BoundAgentSessionIdentity {
    thread_id: String,
    binding_revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BoundAgentSessionCapture {
    identity: BoundAgentSessionIdentity,
    runtime_ref: String,
    profile_fingerprint: String,
    agent_fingerprint: Option<String>,
    adapter_kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AgentSessionAttachmentIdentity {
    Bound(BoundAgentSessionCapture),
    Invocation { invocation_id: String },
}

struct CapturedAgentSessionTarget {
    identity: AgentSessionAttachmentIdentity,
    profile: RuntimeProfileConfig,
    peer: Option<ResolvedPeerTurn>,
}

impl CapturedAgentSessionTarget {
    fn bound(
        binding: &GatewayRuntimeBindingRecord,
        profile: RuntimeProfileConfig,
        peer: Option<ResolvedPeerTurn>,
    ) -> psychevo_runtime::Result<Self> {
        if binding.status != GatewayRuntimeBindingStatus::Resolved {
            return Err(agent_session_configuration_error(format!(
                "Cannot attach unresolved Agent binding for thread `{}`.",
                binding.thread_id
            )));
        }
        let runtime_ref = binding.runtime_ref.clone().ok_or_else(|| {
            agent_session_configuration_error(format!(
                "Agent binding for thread `{}` is missing its Runtime Profile identity.",
                binding.thread_id
            ))
        })?;
        if runtime_ref != profile.id {
            return Err(agent_session_configuration_error(format!(
                "Agent binding for thread `{}` captured Runtime Profile `{runtime_ref}`, not `{}`.",
                binding.thread_id, profile.id
            )));
        }
        let profile_fingerprint = binding.profile_fingerprint.clone().ok_or_else(|| {
            agent_session_configuration_error(format!(
                "Agent binding for thread `{}` is missing its Runtime Profile fingerprint.",
                binding.thread_id
            ))
        })?;
        let resolved_fingerprint = runtime_profile_config_fingerprint(&profile);
        if profile_fingerprint != resolved_fingerprint {
            return Err(agent_session_configuration_error(format!(
                "Agent binding for thread `{}` no longer matches its captured Runtime Profile.",
                binding.thread_id
            )));
        }
        Ok(Self {
            identity: AgentSessionAttachmentIdentity::Bound(BoundAgentSessionCapture {
                identity: BoundAgentSessionIdentity {
                    thread_id: binding.thread_id.clone(),
                    binding_revision: binding.binding_revision,
                },
                runtime_ref,
                profile_fingerprint,
                agent_fingerprint: binding.agent_fingerprint.clone(),
                adapter_kind: binding.adapter_kind.clone(),
            }),
            profile,
            peer,
        })
    }

    fn invocation(
        invocation_id: impl Into<String>,
        profile: RuntimeProfileConfig,
        peer: Option<ResolvedPeerTurn>,
    ) -> Self {
        Self {
            identity: AgentSessionAttachmentIdentity::Invocation {
                invocation_id: invocation_id.into(),
            },
            profile,
            peer,
        }
    }
}

enum AgentSessionTarget {
    Native {
        profile: RuntimeProfileConfig,
    },
    Acp {
        peer: Box<ResolvedPeerTurn>,
        profile: RuntimeProfileConfig,
    },
}

#[derive(Debug)]
struct AgentTurnOutput {
    run: RunResult,
    backend: GatewayBackendInfo,
}

#[derive(Clone)]
struct AgentSessionRef {
    cwd: PathBuf,
    local_session_id: String,
    native_session_id: String,
    mcp_servers: Vec<psychevo_runtime::ResolvedMcpServerInput>,
}

enum AgentSessionCommand {
    // Context projection currently uses the cache-only public read path; keep
    // inspection in the sealed host protocol for adapter conformance.
    #[allow(dead_code)]
    Inspect(AgentSessionRef),
    SetControl {
        session: AgentSessionRef,
        control_id: String,
        value: Value,
    },
    SubmitTurn {
        request: Box<BackendTurnRequest>,
        turn_id: String,
        session_ready: Option<acp_peer::AcpSessionReadyCallback>,
    },
    LoadSession(AgentSessionRef),
    ResumeSession(AgentSessionRef),
    ForkSession {
        source: AgentSessionRef,
        fork_local_session_id: String,
    },
    CloseSession(AgentSessionRef),
    DeleteSession(AgentSessionRef),
}

struct AgentSessionDiscoveryQuery {
    cwd_filter: Option<PathBuf>,
    cursor: Option<String>,
}

#[derive(Debug)]
enum AgentSessionSnapshot {
    Native { profile_id: String },
    Acp(Box<acp_peer::AcpSessionSnapshot>),
}

#[derive(Debug)]
enum AgentSessionResponse {
    #[allow(dead_code)]
    Inspected(AgentSessionSnapshot),
    ControlSet(AgentSessionSnapshot),
    TurnSubmitted(Box<AgentTurnOutput>),
    SessionLoaded(Box<acp_peer::AcpSessionLoadOutput>),
    SessionResumed(AgentSessionSnapshot),
    SessionForked(AgentSessionSnapshot),
    SessionClosed,
    SessionDeleted,
}

impl AgentSessionResponse {
    fn kind(&self) -> &'static str {
        match self {
            Self::Inspected(_) => "inspection",
            Self::ControlSet(_) => "control",
            Self::TurnSubmitted(_) => "turn",
            Self::SessionLoaded(_) => "session-loaded",
            Self::SessionResumed(_) => "session-resumed",
            Self::SessionForked(_) => "session-forked",
            Self::SessionClosed => "session-closed",
            Self::SessionDeleted => "session-deleted",
        }
    }

    fn mismatch(expected: &str, actual: &str) -> Error {
        agent_session_error(
            "agent_session_response_mismatch",
            AgentErrorStage::Configuration,
            "never",
            "unknown",
            format!(
                "Agent Session Adapter returned `{actual}` for a command expecting `{expected}`."
            ),
            Some("agent-session:response-kind".to_string()),
        )
    }

    #[allow(dead_code)]
    fn into_inspection(self) -> psychevo_runtime::Result<AgentSessionSnapshot> {
        match self {
            Self::Inspected(snapshot) => Ok(snapshot),
            response => Err(Self::mismatch("inspection", response.kind())),
        }
    }

    fn into_control(self) -> psychevo_runtime::Result<AgentSessionSnapshot> {
        match self {
            Self::ControlSet(snapshot) => Ok(snapshot),
            response => Err(Self::mismatch("control", response.kind())),
        }
    }

    fn into_turn(self) -> psychevo_runtime::Result<AgentTurnOutput> {
        match self {
            Self::TurnSubmitted(output) => Ok(*output),
            response => Err(Self::mismatch("turn", response.kind())),
        }
    }

    fn into_resumed(self) -> psychevo_runtime::Result<AgentSessionSnapshot> {
        match self {
            Self::SessionResumed(snapshot) => Ok(snapshot),
            response => Err(Self::mismatch("session-resumed", response.kind())),
        }
    }

    fn into_loaded(self) -> psychevo_runtime::Result<acp_peer::AcpSessionLoadOutput> {
        match self {
            Self::SessionLoaded(output) => Ok(*output),
            response => Err(Self::mismatch("session-loaded", response.kind())),
        }
    }

    fn into_forked(self) -> psychevo_runtime::Result<AgentSessionSnapshot> {
        match self {
            Self::SessionForked(snapshot) => Ok(snapshot),
            response => Err(Self::mismatch("session-forked", response.kind())),
        }
    }

    fn into_closed(self) -> psychevo_runtime::Result<()> {
        match self {
            Self::SessionClosed => Ok(()),
            response => Err(Self::mismatch("session-closed", response.kind())),
        }
    }

    fn into_deleted(self) -> psychevo_runtime::Result<()> {
        match self {
            Self::SessionDeleted => Ok(()),
            response => Err(Self::mismatch("session-deleted", response.kind())),
        }
    }
}

impl AgentSessionSnapshot {
    fn into_acp(self) -> psychevo_runtime::Result<acp_peer::AcpSessionSnapshot> {
        match self {
            Self::Acp(snapshot) => Ok(*snapshot),
            Self::Native { profile_id } => Err(agent_session_error(
                "agent_session_snapshot_mismatch",
                AgentErrorStage::Configuration,
                "never",
                "not_delivered",
                format!(
                    "Agent Session inspection for Native profile `{profile_id}` cannot be decoded as an ACP session."
                ),
                Some("agent-session:snapshot-kind".to_string()),
            )),
        }
    }
}

#[derive(Clone)]
struct AgentDeliveryObserver {
    state: StateRuntime,
    turn_id: String,
}

impl AgentDeliveryObserver {
    fn new(state: StateRuntime, turn_id: String) -> Self {
        Self { state, turn_id }
    }

    fn mark_unknown(&self) -> psychevo_runtime::Result<()> {
        self.state
            .store()
            .mark_gateway_turn_delivery_unknown(&self.turn_id)
            .map(|_| ())
    }

    fn confirm(&self) -> psychevo_runtime::Result<()> {
        self.state
            .store()
            .confirm_gateway_turn_delivery(&self.turn_id)
            .map(|_| ())
    }
}

#[derive(Clone)]
struct AgentSessionHost {
    native: Arc<dyn GatewayBackend>,
    acp: acp_peer::AcpProcessPool,
    /// Captured attachment identity, not a second command mailbox. Native
    /// ordering remains in ThreadApplication and ACP ordering remains in the
    /// resident process/session actor.
    bound_attachments: Arc<Mutex<HashMap<BoundAgentSessionIdentity, BoundAgentSessionCapture>>>,
    prepared_sessions: Arc<Mutex<HashMap<String, PreparedAgentSession>>>,
}

#[derive(Clone)]
struct PreparedAgentSession {
    target_id: String,
    agent_ref: Option<String>,
    runtime_ref: String,
    profile_fingerprint: String,
    cwd: PathBuf,
    local_session_id: String,
    native_session_id: String,
    mcp_servers: Vec<psychevo_runtime::ResolvedMcpServerInput>,
    peer: ResolvedPeerTurn,
}

impl fmt::Debug for AgentSessionHost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentSessionHost")
            .finish_non_exhaustive()
    }
}

impl AgentSessionHost {
    fn new(native: Arc<dyn GatewayBackend>) -> Self {
        Self {
            native,
            acp: acp_peer::AcpProcessPool::default(),
            bound_attachments: Arc::new(Mutex::new(HashMap::new())),
            prepared_sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn prepare(
        &self,
        captured: CapturedAgentSessionTarget,
        source_key: String,
        target_id: String,
        agent_ref: Option<String>,
        cwd: PathBuf,
        mcp_servers: Vec<psychevo_runtime::ResolvedMcpServerInput>,
    ) -> psychevo_runtime::Result<acp_peer::AcpSessionSnapshot> {
        let attached = self.attach(captured)?;
        let (peer, profile) = match &attached.target {
            AgentSessionTarget::Native { profile } => {
                return Err(agent_session_configuration_error(format!(
                    "Native profile `{}` does not create a prepared Agent session.",
                    profile.id
                )));
            }
            AgentSessionTarget::Acp { peer, profile } => {
                (peer.as_ref().clone(), profile.clone())
            }
        };
        let profile_fingerprint = runtime_profile_config_fingerprint(&profile);
        let existing = self
            .prepared_sessions
            .lock()
            .expect("prepared Agent Session registry poisoned")
            .get(&source_key)
            .filter(|prepared| {
                prepared.target_id == target_id
                    && prepared.cwd == cwd
                    && prepared.profile_fingerprint == profile_fingerprint
            })
            .cloned();
        if let Some(existing) = existing
            && let Some(snapshot) = self
                .acp
                .inspect_cached(
                    existing.local_session_id.clone(),
                    existing.native_session_id.clone(),
                )
                .await?
        {
            return Ok(snapshot);
        }
        self.release_prepared(&source_key).await?;

        let digest = Sha256::digest(
            format!("{source_key}\0{target_id}\0{}\0{profile_fingerprint}", cwd.display())
                .as_bytes(),
        );
        let local_session_id = format!("draft:{:x}", digest);
        let snapshot = self
            .acp
            .prepare_session(
                peer.clone(),
                cwd.clone(),
                local_session_id.clone(),
                mcp_servers.clone(),
            )
            .await?;
        self.prepared_sessions
            .lock()
            .expect("prepared Agent Session registry poisoned")
            .insert(
                source_key,
                PreparedAgentSession {
                    target_id,
                    agent_ref,
                    runtime_ref: profile.id,
                    profile_fingerprint,
                    cwd,
                    local_session_id,
                    native_session_id: snapshot.native_session_id.clone(),
                    mcp_servers,
                    peer,
                },
            );
        Ok(snapshot)
    }

    async fn release_prepared(&self, source_key: &str) -> psychevo_runtime::Result<bool> {
        let prepared = self
            .prepared_sessions
            .lock()
            .expect("prepared Agent Session registry poisoned")
            .remove(source_key);
        let Some(prepared) = prepared else {
            return Ok(false);
        };
        let session = acp_peer::AcpResidentSessionRef {
            local_session_id: prepared.local_session_id,
            native_session_id: prepared.native_session_id,
        };
        if self
            .acp
            .close_session(prepared.peer, prepared.cwd, session.clone())
            .await
            .is_err()
        {
            self.acp.release_session(session).await?;
        }
        Ok(true)
    }

    async fn inspect_prepared(
        &self,
        source_key: &str,
        target_id: &str,
    ) -> psychevo_runtime::Result<Option<acp_peer::AcpSessionSnapshot>> {
        let prepared = self
            .prepared_sessions
            .lock()
            .expect("prepared Agent Session registry poisoned")
            .get(source_key)
            .filter(|prepared| prepared.target_id == target_id)
            .cloned();
        let Some(prepared) = prepared else {
            return Ok(None);
        };
        self.acp
            .inspect_cached(prepared.local_session_id, prepared.native_session_id)
            .await
    }

    async fn set_prepared_control(
        &self,
        source_key: &str,
        target_id: &str,
        control_id: String,
        value: Value,
    ) -> psychevo_runtime::Result<Option<acp_peer::AcpSessionSnapshot>> {
        let prepared = self
            .prepared_sessions
            .lock()
            .expect("prepared Agent Session registry poisoned")
            .get(source_key)
            .filter(|prepared| prepared.target_id == target_id)
            .cloned();
        let Some(prepared) = prepared else {
            return Ok(None);
        };
        self.acp
            .set_control(acp_peer::AcpSetControlInput {
                peer: prepared.peer,
                cwd: prepared.cwd,
                local_session_id: prepared.local_session_id,
                native_session_id: prepared.native_session_id,
                mcp_servers: prepared.mcp_servers,
                control_id,
                value,
            })
            .await
            .map(Some)
    }

    async fn promote_prepared(
        &self,
        source_key: &str,
        agent_ref: Option<&str>,
        runtime_ref: &str,
        profile_fingerprint: &str,
        thread_id: &str,
    ) -> psychevo_runtime::Result<Option<String>> {
        let prepared = self
            .prepared_sessions
            .lock()
            .expect("prepared Agent Session registry poisoned")
            .get(source_key)
            .filter(|prepared| {
                prepared.agent_ref.as_deref() == agent_ref
                    && prepared.runtime_ref == runtime_ref
                    && prepared.profile_fingerprint == profile_fingerprint
            })
            .cloned();
        let Some(prepared) = prepared else {
            return Ok(None);
        };
        self.acp
            .promote_session(
                prepared.local_session_id,
                thread_id.to_string(),
                prepared.native_session_id.clone(),
            )
            .await?;
        self.prepared_sessions
            .lock()
            .expect("prepared Agent Session registry poisoned")
            .remove(source_key);
        Ok(Some(prepared.native_session_id))
    }

    fn attach(
        &self,
        captured: CapturedAgentSessionTarget,
    ) -> psychevo_runtime::Result<AttachedAgent> {
        let CapturedAgentSessionTarget {
            identity,
            profile,
            mut peer,
        } = captured;
        if let Some(peer) = peer.as_mut() {
            peer.process_scope_fingerprint = Some(runtime_profile_config_fingerprint(&profile));
        }
        let target = match profile.runtime {
            RuntimeProfileKind::Native => {
                if peer.is_some() {
                    return Err(agent_session_configuration_error(
                        "Native Runtime Profile resolved an ACP Agent backend.",
                    ));
                }
                AgentSessionTarget::Native { profile }
            }
            RuntimeProfileKind::Acp => AgentSessionTarget::Acp {
                peer: Box::new(peer.ok_or_else(|| {
                    agent_session_configuration_error(format!(
                        "ACP Runtime Profile `{}` references an unavailable backend.",
                        profile.id
                    ))
                })?),
                profile,
            },
        };
        if let AgentSessionAttachmentIdentity::Bound(capture) = &identity {
            let mut attachments = self
                .bound_attachments
                .lock()
                .expect("Agent Session attachment registry poisoned");
            match attachments.get(&capture.identity) {
                Some(existing) if existing != capture => {
                    return Err(agent_session_error(
                        "agent_session_attachment_conflict",
                        AgentErrorStage::Binding,
                        "never",
                        "not_delivered",
                        format!(
                            "Thread `{}` binding revision {} was attached with a different immutable Agent target.",
                            capture.identity.thread_id, capture.identity.binding_revision
                        ),
                        Some(format!(
                            "agent-binding:{}:{}",
                            capture.identity.thread_id, capture.identity.binding_revision
                        )),
                    ));
                }
                Some(_) => {}
                None => {
                    attachments.insert(capture.identity.clone(), capture.clone());
                }
            }
        }
        Ok(AttachedAgent {
            host: self.clone(),
            _identity: identity,
            target,
        })
    }

    async fn discover(
        &self,
        captured: CapturedAgentSessionTarget,
        query: AgentSessionDiscoveryQuery,
    ) -> psychevo_runtime::Result<acp_peer::AcpSessionListPage> {
        let invocation_cwd = query
            .cwd_filter
            .clone()
            .ok_or_else(|| agent_session_configuration_error("Agent session discovery requires a workspace cwd."))?;
        let attached = self.attach(captured)?;
        match &attached.target {
            AgentSessionTarget::Native { profile } => Err(agent_session_error(
                "agent_session_discovery_unsupported",
                AgentErrorStage::History,
                "user_action",
                "not_delivered",
                format!("Native profile `{}` does not own external Agent sessions.", profile.id),
                None,
            )),
            AgentSessionTarget::Acp { peer, .. } => {
                self.acp
                    .list_sessions(
                        peer.as_ref().clone(),
                        invocation_cwd,
                        query.cwd_filter,
                        query.cursor,
                    )
                    .await
            }
        }
    }

    async fn shutdown(&self, force: bool) -> psychevo_runtime::Result<()> {
        let result = self.acp.shutdown(force).await;
        self.bound_attachments
            .lock()
            .expect("Agent Session attachment registry poisoned")
            .clear();
        self.prepared_sessions
            .lock()
            .expect("prepared Agent Session registry poisoned")
            .clear();
        result
    }

    async fn inspect_cached_acp_session(
        &self,
        local_session_id: String,
        native_session_id: String,
    ) -> psychevo_runtime::Result<Option<acp_peer::AcpSessionSnapshot>> {
        self.acp
            .inspect_cached(local_session_id, native_session_id)
            .await
    }

    async fn release_acp_session(
        &self,
        local_session_id: String,
        native_session_id: String,
    ) -> psychevo_runtime::Result<()> {
        self.acp
            .release_session(acp_peer::AcpResidentSessionRef {
                local_session_id,
                native_session_id,
            })
            .await
    }

    // Protocol and authentication diagnosis belong to backend administration,
    // not to an attached public Thread session, so they deliberately stay
    // outside the sealed AgentSessionCommand family.
    async fn probe_acp_protocol_compatibility(
        &self,
        peer: ResolvedPeerTurn,
        cwd: PathBuf,
    ) -> psychevo_runtime::Result<acp_peer::AcpProtocolDoctorStatus> {
        self.acp.probe_protocol_compatibility(peer, cwd).await
    }

    async fn probe_acp_authentication(
        &self,
        peer: ResolvedPeerTurn,
        cwd: PathBuf,
    ) -> psychevo_runtime::Result<acp_peer::AcpAuthDoctorStatus> {
        self.acp.probe_authentication(peer, cwd).await
    }
}

struct AttachedAgent {
    host: AgentSessionHost,
    _identity: AgentSessionAttachmentIdentity,
    target: AgentSessionTarget,
}

fn lower_native_runtime_options(options: &mut RunOptions) -> psychevo_runtime::Result<()> {
    for (control_id, value) in std::mem::take(&mut options.runtime_options) {
        match control_id.as_str() {
            "model" => options.model = Some(value),
            "reasoning" | "effort" => options.reasoning_effort = Some(value),
            "mode" => {
                options.mode = RunMode::parse(&value).ok_or_else(|| {
                    agent_session_error(
                        "invalid_control",
                        AgentErrorStage::Control,
                        "user_action",
                        "not_delivered",
                        format!("Unknown Native mode `{value}`."),
                        None,
                    )
                })?;
            }
            "permission" | "permissionMode" => {
                options.permission_mode = Some(PermissionMode::parse(&value).ok_or_else(|| {
                    agent_session_error(
                        "invalid_control",
                        AgentErrorStage::Control,
                        "user_action",
                        "not_delivered",
                        format!("Unknown permission mode `{value}`."),
                        None,
                    )
                })?);
            }
            _ => {
                return Err(agent_session_error(
                    "unsupported_control",
                    AgentErrorStage::Control,
                    "user_action",
                    "not_delivered",
                    format!("Psychevo (Native) does not expose control `{control_id}`."),
                    None,
                ));
            }
        }
    }
    Ok(())
}

impl AttachedAgent {
    fn transact(
        &self,
        command: AgentSessionCommand,
    ) -> BoxFuture<'_, psychevo_runtime::Result<AgentSessionResponse>> {
        Box::pin(async move { match command {
            AgentSessionCommand::Inspect(session) => self.inspect(session).await,
            AgentSessionCommand::SetControl {
                session,
                control_id,
                value,
            } => self.set_control(session, control_id, value).await,
            AgentSessionCommand::SubmitTurn {
                request,
                turn_id,
                session_ready,
            } => self.run_turn(*request, turn_id, session_ready).await,
            AgentSessionCommand::LoadSession(session) => self.load_session(session).await,
            AgentSessionCommand::ResumeSession(session) => self.resume_session(session).await,
            AgentSessionCommand::ForkSession {
                source,
                fork_local_session_id,
            } => self.fork_session(source, fork_local_session_id).await,
            AgentSessionCommand::CloseSession(session) => self.close_session(session).await,
            AgentSessionCommand::DeleteSession(session) => self.delete_session(session).await,
        } })
    }

    fn unsupported_lifecycle(
        &self,
        profile: &RuntimeProfileConfig,
        operation: &str,
    ) -> psychevo_runtime::Result<AgentSessionResponse> {
        Err(agent_session_error(
            "agent_session_lifecycle_unsupported",
            AgentErrorStage::History,
            "user_action",
            "not_delivered",
            format!("Native profile `{}` does not expose Agent session/{operation}.", profile.id),
            None,
        ))
    }

    async fn resume_session(
        &self,
        session: AgentSessionRef,
    ) -> psychevo_runtime::Result<AgentSessionResponse> {
        match &self.target {
            AgentSessionTarget::Native { profile } => self.unsupported_lifecycle(profile, "resume"),
            AgentSessionTarget::Acp { peer, .. } => self
                .host
                .acp
                .resume_session(
                    peer.as_ref().clone(),
                    session.cwd,
                    acp_peer::AcpResidentSessionRef {
                        local_session_id: session.local_session_id,
                        native_session_id: session.native_session_id,
                    },
                    session.mcp_servers,
                )
                .await
                .map(|snapshot| {
                    AgentSessionResponse::SessionResumed(AgentSessionSnapshot::Acp(Box::new(snapshot)))
                }),
        }
    }

    async fn load_session(
        &self,
        session: AgentSessionRef,
    ) -> psychevo_runtime::Result<AgentSessionResponse> {
        match &self.target {
            AgentSessionTarget::Native { profile } => self.unsupported_lifecycle(profile, "load"),
            AgentSessionTarget::Acp { peer, .. } => self
                .host
                .acp
                .load_session(
                    peer.as_ref().clone(),
                    session.cwd,
                    session.local_session_id,
                    session.native_session_id,
                    session.mcp_servers,
                )
                .await
                .map(|output| AgentSessionResponse::SessionLoaded(Box::new(output))),
        }
    }

    async fn fork_session(
        &self,
        source: AgentSessionRef,
        fork_local_session_id: String,
    ) -> psychevo_runtime::Result<AgentSessionResponse> {
        match &self.target {
            AgentSessionTarget::Native { profile } => self.unsupported_lifecycle(profile, "fork"),
            AgentSessionTarget::Acp { peer, .. } => self
                .host
                .acp
                .fork_session(
                    peer.as_ref().clone(),
                    source.cwd,
                    acp_peer::AcpResidentSessionRef {
                        local_session_id: source.local_session_id,
                        native_session_id: source.native_session_id,
                    },
                    fork_local_session_id,
                )
                .await
                .map(|snapshot| {
                    AgentSessionResponse::SessionForked(AgentSessionSnapshot::Acp(Box::new(snapshot)))
                }),
        }
    }

    async fn close_session(
        &self,
        session: AgentSessionRef,
    ) -> psychevo_runtime::Result<AgentSessionResponse> {
        match &self.target {
            AgentSessionTarget::Native { profile } => self.unsupported_lifecycle(profile, "close"),
            AgentSessionTarget::Acp { peer, .. } => self
                .host
                .acp
                .close_session(
                    peer.as_ref().clone(),
                    session.cwd,
                    acp_peer::AcpResidentSessionRef {
                        local_session_id: session.local_session_id,
                        native_session_id: session.native_session_id,
                    },
                )
                .await
                .map(|()| AgentSessionResponse::SessionClosed),
        }
    }

    async fn delete_session(
        &self,
        session: AgentSessionRef,
    ) -> psychevo_runtime::Result<AgentSessionResponse> {
        match &self.target {
            AgentSessionTarget::Native { profile } => self.unsupported_lifecycle(profile, "delete"),
            AgentSessionTarget::Acp { peer, .. } => {
                let resident = self
                    .host
                    .acp
                    .inspect_cached(
                        session.local_session_id.clone(),
                        session.native_session_id.clone(),
                    )
                    .await?
                    .map(|_| acp_peer::AcpResidentSessionRef {
                        local_session_id: session.local_session_id,
                        native_session_id: session.native_session_id.clone(),
                    });
                self.host
                    .acp
                    .delete_session(
                        peer.as_ref().clone(),
                        session.cwd,
                        session.native_session_id,
                        resident,
                    )
                    .await
                    .map(|()| AgentSessionResponse::SessionDeleted)
            }
        }
    }

    async fn inspect(
        &self,
        session: AgentSessionRef,
    ) -> psychevo_runtime::Result<AgentSessionResponse> {
        match &self.target {
            AgentSessionTarget::Native { profile } => Ok(AgentSessionResponse::Inspected(
                AgentSessionSnapshot::Native {
                    profile_id: profile.id.clone(),
                },
            )),
            AgentSessionTarget::Acp { peer, .. } => {
                let snapshot = self
                    .host
                    .acp
                    .inspect(
                        peer.as_ref().clone(),
                        session.cwd,
                        session.local_session_id,
                        session.native_session_id,
                        session.mcp_servers,
                    )
                    .await?;
                Ok(AgentSessionResponse::Inspected(AgentSessionSnapshot::Acp(
                    Box::new(snapshot),
                )))
            }
        }
    }

    async fn set_control(
        &self,
        session: AgentSessionRef,
        control_id: String,
        value: Value,
    ) -> psychevo_runtime::Result<AgentSessionResponse> {
        match &self.target {
            AgentSessionTarget::Native { profile } => Err(agent_session_error(
                "unsupported_control",
                AgentErrorStage::Control,
                "user_action",
                "not_delivered",
                format!(
                    "Native profile `{}` applies controls when a turn is submitted and does not expose live Agent-session control mutation.",
                    profile.id
                ),
                Some(format!("agent-session:{}", session.local_session_id)),
            )),
            AgentSessionTarget::Acp { peer, .. } => {
                let snapshot = self
                    .host
                    .acp
                    .set_control(acp_peer::AcpSetControlInput {
                        peer: peer.as_ref().clone(),
                        cwd: session.cwd,
                        local_session_id: session.local_session_id,
                        native_session_id: session.native_session_id,
                        mcp_servers: session.mcp_servers,
                        control_id,
                        value,
                    })
                    .await?;
                Ok(AgentSessionResponse::ControlSet(AgentSessionSnapshot::Acp(
                    Box::new(snapshot),
                )))
            }
        }
    }

    async fn run_turn(
        &self,
        mut request: BackendTurnRequest,
        turn_id: String,
        session_ready: Option<acp_peer::AcpSessionReadyCallback>,
    ) -> psychevo_runtime::Result<AgentSessionResponse> {
        let delivery = AgentDeliveryObserver::new(request.options.state.clone(), turn_id.clone());
        match &self.target {
            AgentSessionTarget::Native { profile } => {
                lower_native_runtime_options(&mut request.options)?;
                if request.input.iter().any(|part| {
                    matches!(
                        part,
                        GatewayInputPart::Resource { .. } | GatewayInputPart::ResourceLink { .. }
                    )
                }) {
                    return Err(agent_session_error(
                        "unsupported_input",
                        AgentErrorStage::Delivery,
                        "user_action",
                        "not_delivered",
                        "Psychevo (Native) does not accept resource or resource-link input.",
                        Some("agent-input:native".to_string()),
                    ));
                }
                let runtime_ref = request.options.runtime_ref.clone();
                delivery.confirm()?;
                gateway_profile_mark(
                    "native_adapter_submitted",
                    Some(&turn_id),
                    request.options.session.as_deref(),
                    GatewayProfileFields {
                        adapter: Some("native"),
                        ..GatewayProfileFields::default()
                    },
                );
                let run = self.host.native.run_turn(request).await?;
                Ok(AgentSessionResponse::TurnSubmitted(Box::new(
                    AgentTurnOutput {
                        run,
                        backend: GatewayBackendInfo {
                            kind: self.host.native.kind(),
                            runtime_ref: runtime_ref.or_else(|| Some(profile.id.clone())),
                            native_id: None,
                        },
                    },
                )))
            }
            AgentSessionTarget::Acp { peer, profile } => {
                let session_ready = session_ready.ok_or_else(|| {
                    agent_session_error(
                        "acp_session_binder_missing",
                        AgentErrorStage::Binding,
                        "never",
                        "not_delivered",
                        "ACP turn is missing its durable native-session binder.",
                        None,
                    )
                })?;
                let cwd = request.options.cwd.clone();
                let result = acp_peer::run_acp_peer_turn(
                    &self.host.acp,
                    peer.as_ref().clone(),
                    profile,
                    request,
                    turn_id,
                    session_ready,
                    delivery,
                )
                .await?;
                let native_session_id = result.native_session_id;
                Ok(AgentSessionResponse::TurnSubmitted(Box::new(
                    AgentTurnOutput {
                        backend: GatewayBackendInfo {
                            kind: BackendKind::Acp,
                            runtime_ref: Some(profile.id.clone()),
                            native_id: Some(runtime_session_handle(
                                &profile.id,
                                &cwd,
                                &native_session_id,
                            )),
                        },
                        run: result.run,
                    },
                )))
            }
        }
    }
}

fn acp_session_ready_for_binding(
    state: StateRuntime,
    binding: GatewayRuntimeBindingRecord,
) -> acp_peer::AcpSessionReadyCallback {
    Arc::new(move |native_session_id| {
        state
            .store()
            .attach_gateway_runtime_native_session(
                &binding.thread_id,
                binding.binding_revision,
                native_session_id,
            )
            .map(|_| ())
    })
}
