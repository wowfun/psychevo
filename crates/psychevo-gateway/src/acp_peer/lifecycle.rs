const ACP_MAX_LISTED_SESSIONS: usize = 512;
const ACP_MAX_LIFECYCLE_CURSOR_CHARS: usize = 16_384;
const ACP_MAX_SAFE_ERROR_MESSAGE_CHARS: usize = 1_024;

fn safe_acp_error(error: &agent_client_protocol::Error) -> String {
    let message = error
        .message
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(ACP_MAX_SAFE_ERROR_MESSAGE_CHARS)
        .collect::<String>();
    format!("ACP error {}: {message}", i32::from(error.code))
}

fn acp_agent_not_delivered_error(
    default_code: &str,
    operation: &str,
    error: &agent_client_protocol::Error,
) -> Error {
    let safe = safe_acp_error(error);
    let message = format!("ACP Agent rejected {operation}: {safe}");
    if error.code == agent_client_protocol::ErrorCode::AuthRequired {
        return Error::structured(
            message.clone(),
            json!({
                "code": "acp_auth_required",
                "stage": "configuration",
                "retryClass": "user_action",
                "delivery": "not_delivered",
                "message": message,
                "recoveryAction": "backend/doctor",
                "diagnosticRef": "acp-auth",
            }),
        );
    }
    acp_lifecycle_error(default_code, message)
}

/// Product-safe page returned by the outbound ACP lifecycle adapter. Raw ACP
/// metadata deliberately does not cross this interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AcpSessionListPage {
    pub(crate) sessions: Vec<AcpListedSession>,
    pub(crate) next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AcpListedSession {
    pub(crate) native_session_id: String,
    pub(crate) cwd: PathBuf,
    pub(crate) additional_directories: Vec<PathBuf>,
    pub(crate) title: Option<String>,
    pub(crate) updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AcpResidentSessionRef {
    pub(crate) local_session_id: String,
    pub(crate) native_session_id: String,
}

#[derive(Debug, Clone, Copy)]
enum AcpLifecycleCapability {
    List,
    Resume,
    Fork,
    Close,
    Delete,
}

impl AcpLifecycleCapability {
    fn operation(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Resume => "resume",
            Self::Fork => "fork",
            Self::Close => "close",
            Self::Delete => "delete",
        }
    }

    fn advertised(self, initialized: &InitializeResponse) -> bool {
        let session = &initialized.agent_capabilities.session_capabilities;
        match self {
            Self::List => session.list.is_some(),
            Self::Resume => session.resume.is_some(),
            Self::Fork => session.fork.is_some(),
            Self::Close => session.close.is_some(),
            Self::Delete => session.delete.is_some(),
        }
    }
}

fn require_acp_lifecycle_capability(
    initialized: &InitializeResponse,
    capability: AcpLifecycleCapability,
) -> psychevo_runtime::Result<()> {
    if capability.advertised(initialized) {
        return Ok(());
    }
    Err(acp_lifecycle_error(
        "acp_lifecycle_unsupported",
        format!(
            "ACP Agent did not advertise session/{} support; no request was sent.",
            capability.operation()
        ),
    ))
}

fn acp_lifecycle_error(code: &str, message: impl Into<String>) -> Error {
    crate::agent_session_error(
        code,
        crate::AgentErrorStage::History,
        "user_action",
        "not_delivered",
        message,
        Some("acp-lifecycle".to_string()),
    )
}

fn lifecycle_client_context(peer: &ResolvedPeerTurn, cwd: PathBuf) -> Arc<AcpClientContext> {
    Arc::new(AcpClientContext {
        cwd,
        fs_read: peer_allows_fs_read(peer),
        fs_write: peer_allows_fs_write(peer),
        approval_handler: None,
        clarify_control: None,
        terminal: peer_allows_terminal(peer),
        terminal_env: acp_backend_effective_env(peer),
        stream: None,
        abort: None,
    })
}

fn mcp_declaration_fingerprint(mcp_servers: &[McpServer]) -> psychevo_runtime::Result<String> {
    Ok(format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(mcp_servers)?)
    ))
}

fn insert_acp_context(
    contexts: &Arc<Mutex<BTreeMap<String, Arc<AcpClientContext>>>>,
    native_session_id: &str,
    context: Arc<AcpClientContext>,
) -> psychevo_runtime::Result<()> {
    contexts
        .lock()
        .map_err(|_| Error::Message("ACP session context lock poisoned".to_string()))?
        .insert(native_session_id.to_string(), context);
    Ok(())
}

fn remove_acp_context(
    contexts: &Arc<Mutex<BTreeMap<String, Arc<AcpClientContext>>>>,
    native_session_id: &str,
) -> psychevo_runtime::Result<()> {
    contexts
        .lock()
        .map_err(|_| Error::Message("ACP session context lock poisoned".to_string()))?
        .remove(native_session_id);
    Ok(())
}

fn validate_lifecycle_session_identity(
    session: &AcpResidentSession,
    expected: &AcpResidentSessionRef,
) -> psychevo_runtime::Result<()> {
    if session.native_session_id == expected.native_session_id {
        return Ok(());
    }
    Err(acp_lifecycle_error(
        "acp_session_identity_mismatch",
        format!(
            "Resident ACP session `{}` owns native id `{}`, not `{}`.",
            expected.local_session_id, session.native_session_id, expected.native_session_id
        ),
    ))
}

async fn validate_resident_session_ref(
    sessions: &AcpResidentSessions,
    session_ref: &AcpResidentSessionRef,
) -> psychevo_runtime::Result<()> {
    let sessions = sessions.lock().await;
    let session = sessions.get(&session_ref.local_session_id).ok_or_else(|| {
        acp_lifecycle_error(
            "acp_session_not_resident",
            "ACP lifecycle target is not attached to this process generation.",
        )
    })?;
    validate_lifecycle_session_identity(session, session_ref)
}

async fn validate_delete_session_ref(
    sessions: &AcpResidentSessions,
    native_session_id: &str,
    resident: Option<&AcpResidentSessionRef>,
) -> psychevo_runtime::Result<()> {
    if let Some(resident) = resident {
        if resident.native_session_id != native_session_id {
            return Err(acp_lifecycle_error(
                "acp_session_identity_mismatch",
                "ACP session/delete native id does not match its resident session reference.",
            ));
        }
        return validate_resident_session_ref(sessions, resident).await;
    }
    if sessions
        .lock()
        .await
        .values()
        .any(|session| session.native_session_id == native_session_id)
    {
        return Err(acp_lifecycle_error(
            "acp_session_resident_reference_required",
            "ACP session/delete requires the owning public Thread for a resident session.",
        ));
    }
    Ok(())
}

struct AcpListSessionsInput {
    cwd: Option<PathBuf>,
    cursor: Option<String>,
}

async fn listed_acp_sessions(
    cx: &ConnectionTo<Agent>,
    initialized: &InitializeResponse,
    sessions: &AcpResidentSessions,
    notification_ingress: &AcpNotificationIngress,
    notification_rx: &mut AcpNotificationSubscription,
    generation: u64,
    input: AcpListSessionsInput,
) -> psychevo_runtime::Result<AcpSessionListPage> {
    let AcpListSessionsInput { cwd, cursor } = input;
    require_acp_lifecycle_capability(initialized, AcpLifecycleCapability::List)?;
    if cwd.as_ref().is_some_and(|cwd| !cwd.is_absolute()) {
        return Err(acp_lifecycle_error(
            "acp_lifecycle_invalid_cwd",
            "ACP session/list cwd must be absolute.",
        ));
    }
    if cursor
        .as_ref()
        .is_some_and(|cursor| cursor.chars().count() > ACP_MAX_LIFECYCLE_CURSOR_CHARS)
    {
        return Err(acp_lifecycle_error(
            "acp_lifecycle_cursor_oversized",
            "ACP session/list cursor exceeds the product bound.",
        ));
    }
    let request = ListSessionsRequest::new().cwd(cwd).cursor(cursor);
    let (response, response_barrier) = acp_response_with_projection_barrier(
        cx.send_request(request),
        notification_ingress,
    )
    .await
    .map_err(|error| {
        acp_agent_not_delivered_error("acp_session_list_failed", "session/list", &error)
    })?;
    reduce_acp_notifications_through_barrier(
        notification_rx,
        sessions,
        generation,
        response_barrier,
        None,
        None,
        None,
    )
    .await?;
    if response.sessions.len() > ACP_MAX_LISTED_SESSIONS {
        return Err(acp_lifecycle_error(
            "acp_session_list_oversized",
            format!(
                "ACP Agent returned {} sessions, exceeding the product bound of {ACP_MAX_LISTED_SESSIONS}.",
                response.sessions.len()
            ),
        ));
    }
    let next_cursor = response
        .next_cursor
        .map(|cursor| {
            if cursor.chars().count() > ACP_MAX_LIFECYCLE_CURSOR_CHARS {
                Err(acp_lifecycle_error(
                    "acp_lifecycle_cursor_oversized",
                    "ACP Agent returned an oversized session/list cursor.",
                ))
            } else {
                Ok(cursor)
            }
        })
        .transpose()?;
    let sessions = response
        .sessions
        .into_iter()
        .map(|session| {
            if !session.cwd.is_absolute()
                || session
                    .additional_directories
                    .iter()
                    .any(|directory| !directory.is_absolute())
            {
                return Err(acp_lifecycle_error(
                    "acp_session_list_invalid_path",
                    "ACP Agent returned a non-absolute session workspace path.",
                ));
            }
            Ok(AcpListedSession {
                native_session_id: bounded_acp_text(
                    &session.session_id.to_string(),
                    ACP_MAX_AGENT_NAME_CHARS,
                ),
                cwd: session.cwd,
                additional_directories: session.additional_directories,
                title: session
                    .title
                    .as_deref()
                    .map(|title| bounded_acp_text(title, ACP_MAX_SESSION_TITLE_CHARS)),
                updated_at: session
                    .updated_at
                    .as_deref()
                    .map(|updated_at| bounded_acp_text(updated_at, ACP_MAX_UPDATED_AT_CHARS)),
            })
        })
        .collect::<psychevo_runtime::Result<Vec<_>>>()?;
    Ok(AcpSessionListPage {
        sessions,
        next_cursor,
    })
}

#[allow(clippy::too_many_arguments)]
async fn resume_resident_acp_session(
    cx: &ConnectionTo<Agent>,
    initialized: &InitializeResponse,
    peer: &ResolvedPeerTurn,
    contexts: &Arc<Mutex<BTreeMap<String, Arc<AcpClientContext>>>>,
    sessions: &AcpResidentSessions,
    notification_ingress: &AcpNotificationIngress,
    notification_rx: &mut AcpNotificationSubscription,
    next_session_epoch: &AtomicU64,
    generation: u64,
    session_ref: AcpResidentSessionRef,
    cwd: PathBuf,
    resolved_mcp_servers: Vec<psychevo_runtime::ResolvedMcpServerInput>,
) -> psychevo_runtime::Result<AcpSessionSnapshot> {
    require_acp_lifecycle_capability(initialized, AcpLifecycleCapability::Resume)?;
    if !cwd.is_absolute() {
        return Err(acp_lifecycle_error(
            "acp_lifecycle_invalid_cwd",
            "ACP session/resume cwd must be absolute.",
        ));
    }
    {
        let sessions = sessions.lock().await;
        if sessions.contains_key(&session_ref.local_session_id)
            || sessions.values().any(|session| {
                session.native_session_id == session_ref.native_session_id
            })
        {
            return Err(acp_lifecycle_error(
                "acp_session_already_resident",
                "ACP session/resume cannot replace an attached resident session.",
            ));
        }
    }
    let mcp_servers = mcp_handoff::acp_mcp_server_declarations(
        peer,
        &resolved_mcp_servers,
        &initialized.agent_capabilities,
    )
    .map_err(|error| {
        acp_lifecycle_error("acp_mcp_configuration_invalid", error.to_string())
    })?;
    let mcp_declaration_fingerprint = mcp_declaration_fingerprint(&mcp_servers)?;
    let session_epoch = next_acp_session_epoch(next_session_epoch)?;
    insert_acp_context(
        contexts,
        &session_ref.native_session_id,
        lifecycle_client_context(peer, cwd.clone()),
    )?;
    let request = ResumeSessionRequest::new(session_ref.native_session_id.clone(), &cwd)
        .mcp_servers(mcp_servers.clone());
    let response = acp_session_response_with_legacy_models::<ResumeSessionResponse, _>(
        cx,
        "session/resume",
        request,
        notification_ingress,
    )
    .await
    .map_err(|error| {
        acp_agent_not_delivered_error("acp_session_resume_failed", "session/resume", &error)
    });
    let (response, legacy_models, response_barrier) = match response {
        Ok(response) => response,
        Err(error) => {
            let _ = remove_acp_context(contexts, &session_ref.native_session_id);
            return Err(error);
        }
    };
    let mut session = new_acp_resident_session(
        initialized,
        AcpResidentSessionInput {
            native_session_id: session_ref.native_session_id.clone(),
            modes: response.modes,
            config_options: response.config_options.unwrap_or_default(),
            legacy_models,
            session_epoch,
            loaded_from_agent: true,
            mcp_servers,
            mcp_declaration_fingerprint,
        },
    );
    // session/resume explicitly does not replay prior messages.
    session.history.replay_complete = true;
    sessions
        .lock()
        .await
        .insert(session_ref.local_session_id.clone(), session);
    if let Err(error) = reduce_acp_notifications_through_barrier(
        notification_rx,
        sessions,
        generation,
        response_barrier,
        None,
        Some(&session_ref.native_session_id),
        None,
    )
    .await
    {
        sessions.lock().await.remove(&session_ref.local_session_id);
        let _ = remove_acp_context(contexts, &session_ref.native_session_id);
        return Err(error);
    }
    let session = sessions
        .lock()
        .await
        .get(&session_ref.local_session_id)
        .cloned()
        .ok_or_else(|| {
            acp_lifecycle_error(
                "acp_session_disappeared",
                "Resident ACP session disappeared after session/resume.",
            )
        })?;
    Ok(acp_session_snapshot(&session, generation))
}

#[allow(clippy::too_many_arguments)]
async fn fork_resident_acp_session(
    cx: &ConnectionTo<Agent>,
    initialized: &InitializeResponse,
    peer: &ResolvedPeerTurn,
    contexts: &Arc<Mutex<BTreeMap<String, Arc<AcpClientContext>>>>,
    sessions: &AcpResidentSessions,
    notification_ingress: &AcpNotificationIngress,
    notification_rx: &mut AcpNotificationSubscription,
    next_session_epoch: &AtomicU64,
    generation: u64,
    source: AcpResidentSessionRef,
    fork_local_session_id: String,
    cwd: PathBuf,
) -> psychevo_runtime::Result<AcpSessionSnapshot> {
    require_acp_lifecycle_capability(initialized, AcpLifecycleCapability::Fork)?;
    if !cwd.is_absolute() {
        return Err(acp_lifecycle_error(
            "acp_lifecycle_invalid_cwd",
            "ACP session/fork cwd must be absolute.",
        ));
    }
    if source.local_session_id == fork_local_session_id {
        return Err(acp_lifecycle_error(
            "acp_session_fork_identity_conflict",
            "ACP session/fork requires a distinct destination public Thread.",
        ));
    }
    let source_session = {
        let sessions = sessions.lock().await;
        if sessions.contains_key(&fork_local_session_id) {
            return Err(acp_lifecycle_error(
                "acp_session_already_resident",
                "ACP session/fork destination is already resident.",
            ));
        }
        let session = sessions.get(&source.local_session_id).cloned().ok_or_else(|| {
            acp_lifecycle_error(
                "acp_session_not_resident",
                "ACP session/fork source is not attached to this process generation.",
            )
        })?;
        validate_lifecycle_session_identity(&session, &source)?;
        session
    };
    let request = ForkSessionRequest::new(source.native_session_id.clone(), &cwd)
        .mcp_servers(source_session.mcp_servers.clone());
    let (response, legacy_models, response_barrier) =
        acp_session_response_with_legacy_models::<ForkSessionResponse, _>(
        cx,
        "session/fork",
        request,
        notification_ingress,
    )
    .await
    .map_err(|error| {
        acp_agent_not_delivered_error("acp_session_fork_failed", "session/fork", &error)
    })?;
    let native_session_id = response.session_id.to_string();
    {
        let sessions = sessions.lock().await;
        if sessions
            .values()
            .any(|session| session.native_session_id == native_session_id)
        {
            return Err(acp_lifecycle_error(
                "acp_session_identity_conflict",
                "ACP Agent returned an already-resident native id for session/fork.",
            ));
        }
    }
    let session_epoch = next_acp_session_epoch(next_session_epoch)?;
    insert_acp_context(
        contexts,
        &native_session_id,
        lifecycle_client_context(peer, cwd),
    )?;
    notification_rx.set_native_session_id(native_session_id.clone())?;
    let mut forked = new_acp_resident_session(
        initialized,
        AcpResidentSessionInput {
            native_session_id: native_session_id.clone(),
            modes: response.modes,
            config_options: response.config_options.unwrap_or_default(),
            legacy_models,
            session_epoch,
            loaded_from_agent: true,
            mcp_servers: source_session.mcp_servers,
            mcp_declaration_fingerprint: source_session.mcp_declaration_fingerprint,
        },
    );
    forked.history.replay_complete = false;
    sessions
        .lock()
        .await
        .insert(fork_local_session_id.clone(), forked);
    if let Err(error) = reduce_acp_notifications_through_barrier(
        notification_rx,
        sessions,
        generation,
        response_barrier,
        Some(&native_session_id),
        Some(&native_session_id),
        None,
    )
    .await
    {
        sessions.lock().await.remove(&fork_local_session_id);
        let _ = remove_acp_context(contexts, &native_session_id);
        return Err(error);
    }
    let session = {
        let mut sessions = sessions.lock().await;
        let session = sessions.get_mut(&fork_local_session_id).ok_or_else(|| {
            acp_lifecycle_error(
                "acp_session_disappeared",
                "Resident ACP session disappeared after session/fork.",
            )
        })?;
        session.history.replay_complete = true;
        session.clone()
    };
    Ok(acp_session_snapshot(&session, generation))
}

fn cooperative_acp_session_cancel(
    cx: &ConnectionTo<Agent>,
    native_session_id: &str,
) -> psychevo_runtime::Result<()> {
    cx.send_notification(CancelNotification::new(native_session_id.to_string()))
        .map_err(|error| {
            acp_lifecycle_error(
                "acp_session_cancel_failed",
                format!(
                    "ACP session cancellation could not be sent: {}",
                    safe_acp_error(&error)
                ),
            )
        })
}

async fn remove_resident_session_resources(
    contexts: &Arc<Mutex<BTreeMap<String, Arc<AcpClientContext>>>>,
    sessions: &AcpResidentSessions,
    terminals: &AcpTerminalRegistry,
    session_ref: &AcpResidentSessionRef,
) -> psychevo_runtime::Result<()> {
    let removed = {
        let mut sessions = sessions.lock().await;
        let session = sessions.get(&session_ref.local_session_id).ok_or_else(|| {
            acp_lifecycle_error(
                "acp_session_not_resident",
                "ACP lifecycle operation completed for a non-resident public Thread.",
            )
        })?;
        validate_lifecycle_session_identity(session, session_ref)?;
        sessions
            .remove(&session_ref.local_session_id)
            .expect("validated resident ACP session exists")
    };
    debug_assert_eq!(removed.native_session_id, session_ref.native_session_id);
    remove_acp_context(contexts, &session_ref.native_session_id)?;
    terminals.terminate_session(&session_ref.native_session_id)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn close_resident_acp_session(
    cx: &ConnectionTo<Agent>,
    initialized: &InitializeResponse,
    contexts: &Arc<Mutex<BTreeMap<String, Arc<AcpClientContext>>>>,
    sessions: &AcpResidentSessions,
    terminals: &AcpTerminalRegistry,
    notification_ingress: &AcpNotificationIngress,
    notification_rx: &mut AcpNotificationSubscription,
    generation: u64,
    session_ref: AcpResidentSessionRef,
) -> psychevo_runtime::Result<()> {
    require_acp_lifecycle_capability(initialized, AcpLifecycleCapability::Close)?;
    {
        let sessions = sessions.lock().await;
        let resident = sessions.get(&session_ref.local_session_id).ok_or_else(|| {
            acp_lifecycle_error(
                "acp_session_not_resident",
                "ACP session/close target is not resident.",
            )
        })?;
        validate_lifecycle_session_identity(resident, &session_ref)?;
    }
    let (_, response_barrier) = acp_response_with_projection_barrier(
        cx.send_request(CloseSessionRequest::new(
            session_ref.native_session_id.clone(),
        )),
        notification_ingress,
    )
    .await
    .map_err(|error| {
        acp_agent_not_delivered_error("acp_session_close_failed", "session/close", &error)
    })?;
    reduce_acp_notifications_through_barrier(
        notification_rx,
        sessions,
        generation,
        response_barrier,
        None,
        Some(&session_ref.native_session_id),
        None,
    )
    .await?;
    remove_resident_session_resources(contexts, sessions, terminals, &session_ref).await
}

#[allow(clippy::too_many_arguments)]
async fn delete_acp_session(
    cx: &ConnectionTo<Agent>,
    initialized: &InitializeResponse,
    contexts: &Arc<Mutex<BTreeMap<String, Arc<AcpClientContext>>>>,
    sessions: &AcpResidentSessions,
    terminals: &AcpTerminalRegistry,
    notification_ingress: &AcpNotificationIngress,
    notification_rx: &mut AcpNotificationSubscription,
    generation: u64,
    native_session_id: String,
    resident: Option<AcpResidentSessionRef>,
) -> psychevo_runtime::Result<()> {
    require_acp_lifecycle_capability(initialized, AcpLifecycleCapability::Delete)?;
    validate_delete_session_ref(sessions, &native_session_id, resident.as_ref()).await?;
    let (_, response_barrier) = acp_response_with_projection_barrier(
        cx.send_request(DeleteSessionRequest::new(native_session_id.clone())),
        notification_ingress,
    )
    .await
    .map_err(|error| {
        acp_agent_not_delivered_error("acp_session_delete_failed", "session/delete", &error)
    })?;
    reduce_acp_notifications_through_barrier(
        notification_rx,
        sessions,
        generation,
        response_barrier,
        None,
        Some(&native_session_id),
        None,
    )
    .await?;
    if let Some(resident) = resident {
        remove_resident_session_resources(contexts, sessions, terminals, &resident).await?;
    } else {
        remove_acp_context(contexts, &native_session_id)?;
        terminals.terminate_session(&native_session_id)?;
    }
    Ok(())
}
