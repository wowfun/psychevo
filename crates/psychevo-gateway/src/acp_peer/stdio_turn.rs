#[derive(Clone)]
struct AcpPeerTurnContext {
    cwd: PathBuf,
    home: PathBuf,
    local_session_id: String,
    native_session_id: Option<String>,
    native_session_slot: Arc<std::sync::Mutex<Option<String>>>,
    input: Vec<wire::GatewayInputPart>,
    prompt: String,
    images: Vec<ImageInput>,
    instructions: Option<String>,
    peer_model: Option<String>,
    peer_reasoning_effort: Option<String>,
    peer_runtime_options: BTreeMap<String, String>,
    mcp_servers: Vec<psychevo_runtime::ResolvedMcpServerInput>,
    stream: Option<RunStreamSink>,
    workspace_mutations: Option<WorkspaceMutationSink>,
    approval_handler: Option<Arc<dyn psychevo_runtime::ApprovalHandler>>,
    clarify_control: Option<RunControlHandle>,
    abort: Option<AbortSignal>,
    before_prompt: AcpBeforePromptCallback,
    delivery_observer: crate::AgentDeliveryObserver,
}

type AcpBeforePromptCallback = Arc<
    dyn Fn(&AcpHistoryReplayProjection) -> psychevo_runtime::Result<()> + Send + Sync,
>;

pub(crate) fn resolve_peer_mcp_server_handoffs(
    peer: &ResolvedPeerTurn,
    options: &psychevo_runtime::RunOptions,
) -> psychevo_runtime::Result<Vec<psychevo_runtime::ResolvedMcpServerInput>> {
    let names = mcp_handoff::requested_peer_mcp_server_names(peer)?;
    resolve_mcp_server_handoffs(options, &names).map_err(|error| {
        crate::agent_session_error(
            "acp_mcp_configuration_invalid",
            crate::AgentErrorStage::Binding,
            "user_action",
            "not_delivered",
            error.to_string(),
            Some(format!("acp-mcp:{}", peer.backend.id)),
        )
    })
}

async fn run_acp_stdio_turn(
    pool: &AcpProcessPool,
    peer: &ResolvedPeerTurn,
    context: &AcpPeerTurnContext,
    session_ready: AcpSessionReadyCallback,
) -> psychevo_runtime::Result<AcpTurnOutput> {
    pool.run_turn(peer.clone(), context.clone(), session_ready)
        .await
}

async fn wait_for_optional_abort(abort: Option<AbortSignal>) {
    if let Some(mut abort) = abort {
        abort.wait_for_abort().await;
    } else {
        std::future::pending::<()>().await;
    }
}

fn is_acp_peer_abort_error(err: &Error) -> bool {
    err.to_string().contains(ACP_PEER_ABORT_MESSAGE)
}

#[allow(clippy::too_many_arguments)]
async fn ensure_resident_acp_session(
    cx: &ConnectionTo<Agent>,
    initialized: &InitializeResponse,
    peer: &ResolvedPeerTurn,
    contexts: &Arc<Mutex<BTreeMap<String, Arc<AcpClientContext>>>>,
    sessions: &AcpResidentSessions,
    notification_ingress: &AcpNotificationIngress,
    notification_rx: &mut AcpNotificationSubscription,
    next_session_epoch: &AtomicU64,
    generation: u64,
    local_session_id: &str,
    requested_native_session_id: Option<&str>,
    cwd: &Path,
    resolved_mcp_servers: &[psychevo_runtime::ResolvedMcpServerInput],
    approval_handler: Option<Arc<dyn psychevo_runtime::ApprovalHandler>>,
    clarify_control: Option<RunControlHandle>,
    stream: Option<RunStreamSink>,
    abort: Option<AbortSignal>,
    mut active_state: Option<&mut AcpPeerStreamState>,
) -> psychevo_runtime::Result<AcpResidentSession> {
    let mcp_servers = mcp_handoff::acp_mcp_server_declarations(
        peer,
        resolved_mcp_servers,
        &initialized.agent_capabilities,
    )
    .map_err(|error| {
        acp_not_delivered_error("acp_mcp_configuration_invalid", error.to_string())
    })?;
    let mcp_declaration_fingerprint = mcp_declaration_fingerprint(&mcp_servers)?;
    let client_context = Arc::new(AcpClientContext {
        cwd: cwd.to_path_buf(),
        fs_read: peer_allows_fs_read(peer),
        fs_write: peer_allows_fs_write(peer),
        approval_handler,
        clarify_control,
        terminal: peer_allows_terminal(peer),
        terminal_env: acp_backend_effective_env(peer),
        stream: stream.clone(),
        abort,
    });
    let existing_session = sessions.lock().await.get(local_session_id).cloned();
    if let Some(session) = existing_session {
        if requested_native_session_id
            .is_some_and(|requested| requested != session.native_session_id)
        {
            return Err(crate::agent_session_error(
                "acp_session_identity_mismatch",
                crate::AgentErrorStage::Binding,
                "never",
                "not_delivered",
                "The resident ACP process owns a different native session for this thread.",
                Some(format!("acp-session:{local_session_id}")),
            ));
        }
        if session.mcp_servers != mcp_servers {
            return Err(crate::agent_session_error(
                "acp_mcp_binding_changed",
                crate::AgentErrorStage::Binding,
                "never",
                "not_delivered",
                "The resident ACP session was created with a different MCP declaration set; create a new Thread.",
                Some(format!("acp-mcp-session:{local_session_id}")),
            ));
        }
        contexts
            .lock()
            .map_err(|_| Error::Message("ACP session context lock poisoned".to_string()))?
            .insert(session.native_session_id.clone(), client_context);
        notification_rx.set_native_session_id(session.native_session_id.clone())?;
        let barrier = notification_ingress.barrier()?;
        reduce_acp_notifications_through_barrier(
            notification_rx,
            sessions,
            generation,
            barrier,
            None,
            Some(&session.native_session_id),
            active_state.as_deref_mut(),
        )
        .await?;
        return sessions
            .lock()
            .await
            .get(local_session_id)
            .cloned()
            .ok_or_else(|| {
                Error::Message("resident ACP session disappeared during inspection".to_string())
            });
    }

    let loaded_from_agent = requested_native_session_id.is_some();
    let (session, response_barrier) = if let Some(native_session_id) = requested_native_session_id {
        if !initialized.agent_capabilities.load_session {
            return Err(crate::agent_session_error(
                "acp_session_not_resumable",
                crate::AgentErrorStage::History,
                "user_action",
                "not_delivered",
                format!(
                    "ACP peer `{}` does not advertise session/load; this process-ephemeral thread cannot be resumed after process restart.",
                    peer.backend.id
                ),
                Some(format!("acp-session:{local_session_id}")),
            ));
        }
        contexts
            .lock()
            .map_err(|_| Error::Message("ACP session context lock poisoned".to_string()))?
            .insert(native_session_id.to_string(), Arc::clone(&client_context));
        let loaded = acp_session_response_with_legacy_models::<LoadSessionResponse, _>(
            cx,
            "session/load",
            LoadSessionRequest::new(native_session_id.to_string(), cwd)
                .mcp_servers(mcp_servers.clone()),
            notification_ingress,
        )
        .await;
        let (loaded, legacy_models, response_barrier) = match loaded {
            Ok(loaded) => loaded,
            Err(error) => {
                let _ = remove_acp_context(contexts, native_session_id);
                return Err(acp_agent_not_delivered_error(
                    "acp_session_load_failed",
                    "session/load",
                    &error,
                ));
            }
        };
        let modes = loaded.modes;
        let config_options = loaded.config_options.unwrap_or_default();
        (
            new_acp_resident_session(
                initialized,
                AcpResidentSessionInput {
                    native_session_id: native_session_id.to_string(),
                    modes,
                    config_options,
                    legacy_models,
                    session_epoch: next_acp_session_epoch(next_session_epoch)?,
                    loaded_from_agent: true,
                    mcp_servers: mcp_servers.clone(),
                    mcp_declaration_fingerprint: mcp_declaration_fingerprint.clone(),
                },
            ),
            response_barrier,
        )
    } else {
        let (created, legacy_models, response_barrier) =
            acp_session_response_with_legacy_models::<NewSessionResponse, _>(
            cx,
            "session/new",
            NewSessionRequest::new(cwd).mcp_servers(mcp_servers.clone()),
            notification_ingress,
        )
        .await
        .map_err(|error| {
            acp_agent_not_delivered_error("acp_session_create_failed", "session/new", &error)
        })?;
        let native_session_id = created.session_id.to_string();
        let modes = created.modes;
        let config_options = created.config_options.unwrap_or_default();
        contexts
            .lock()
            .map_err(|_| Error::Message("ACP session context lock poisoned".to_string()))?
            .insert(native_session_id.clone(), client_context);
        (
            new_acp_resident_session(
                initialized,
                AcpResidentSessionInput {
                    native_session_id,
                    modes,
                    config_options,
                    legacy_models,
                    session_epoch: next_acp_session_epoch(next_session_epoch)?,
                    loaded_from_agent: false,
                    mcp_servers,
                    mcp_declaration_fingerprint,
                },
            ),
            response_barrier,
        )
    };
    let native_session_id = session.native_session_id.clone();
    emit_runtime_event(
        &stream,
        json!({
            "type": "acp_peer_mcp_configured",
            "session_id": local_session_id,
            "source": "acp_peer",
            "protocol_version": "1",
            "server_names": resolved_mcp_servers
                .iter()
                .map(|resolved| resolved.server.name.clone())
                .collect::<Vec<_>>(),
        }),
    );
    notification_rx.set_native_session_id(native_session_id.clone())?;
    sessions
        .lock()
        .await
        .insert(local_session_id.to_string(), session.clone());
    reduce_acp_notifications_through_barrier(
        notification_rx,
        sessions,
        generation,
        response_barrier,
        loaded_from_agent.then_some(native_session_id.as_str()),
        Some(&native_session_id),
        active_state.as_deref_mut(),
    )
    .await?;
    let replay_complete = active_state
        .as_deref()
        .is_none_or(|state| state.history_replay.is_complete());
    if loaded_from_agent && let Some(session) = sessions.lock().await.get_mut(local_session_id) {
        session.history.replay_complete = replay_complete;
    }
    sessions
        .lock()
        .await
        .get(local_session_id)
        .cloned()
        .ok_or_else(|| {
            Error::Message("resident ACP session disappeared after attachment".to_string())
        })
}

#[allow(clippy::too_many_arguments)]
async fn execute_resident_acp_turn(
    cx: &ConnectionTo<Agent>,
    initialized: &InitializeResponse,
    peer: &ResolvedPeerTurn,
    contexts: &Arc<Mutex<BTreeMap<String, Arc<AcpClientContext>>>>,
    sessions: &AcpResidentSessions,
    notification_ingress: &AcpNotificationIngress,
    notification_rx: &mut AcpNotificationSubscription,
    force_rx: &mut watch::Receiver<bool>,
    next_session_epoch: &AtomicU64,
    generation: u64,
    turn: AcpPeerTurnContext,
    session_ready: AcpSessionReadyCallback,
    delivery: AcpDeliveryMarker,
) -> psychevo_runtime::Result<AcpTurnOutput> {
    emit_runtime_event(
        &turn.stream,
        json!({
            "type": "acp_peer_protocol_negotiated",
            "session_id": turn.local_session_id,
            "source": "acp_peer",
            "protocol_version": "1",
            "process_generation": generation,
        }),
    );
    let mut state = AcpPeerStreamState::new(
        turn.stream.clone(),
        turn.workspace_mutations.clone(),
        turn.local_session_id.clone(),
    );
    let mut session = ensure_resident_acp_session(
        cx,
        initialized,
        peer,
        contexts,
        sessions,
        notification_ingress,
        notification_rx,
        next_session_epoch,
        generation,
        &turn.local_session_id,
        turn.native_session_id.as_deref(),
        &turn.cwd,
        &turn.mcp_servers,
        turn.approval_handler.clone(),
        turn.clarify_control.clone(),
        turn.stream.clone(),
        turn.abort.clone(),
        Some(&mut state),
    )
    .await?;
    let native_session_id = session.native_session_id.clone();
    if let Ok(mut slot) = turn.native_session_slot.lock() {
        *slot = Some(native_session_id.clone());
    }
    session_ready(&native_session_id).map_err(|error| {
        crate::agent_session_error(
            "acp_session_binding_failed",
            crate::AgentErrorStage::Binding,
            "never",
            "not_delivered",
            format!("Failed to persist ACP native session identity before prompt: {error}"),
            Some(format!("acp-session:{}", turn.local_session_id)),
        )
    })?;
    (turn.before_prompt)(&state.history_replay).map_err(|error| {
        acp_not_delivered_error(
            "acp_before_prompt_commit_failed",
            format!(
                "Failed to commit ACP history replay and current user input before prompt delivery: {error}"
            ),
        )
    })?;

    session_controls::apply_acp_v1_config_options(
        cx,
        notification_ingress,
        session_controls::AcpSessionControlState {
            config_options: &mut session.config_options,
            legacy_models: &mut session.legacy_models,
        },
        &native_session_id,
        &turn.local_session_id,
        &turn.stream,
        session_controls::requested_acp_config_selections(&turn),
    )
    .await?;
    sessions
        .lock()
        .await
        .insert(turn.local_session_id.clone(), session);
    let config_barrier = notification_ingress.barrier()?;
    reduce_acp_notifications_through_barrier(
        notification_rx,
        sessions,
        generation,
        config_barrier,
        None,
        Some(&native_session_id),
        Some(&mut state),
    )
    .await?;

    let prompt = prompt_input::acp_prompt_blocks(peer, &turn, &initialized.agent_capabilities)
        .await
        .map_err(|error| acp_not_delivered_error("acp_input_rejected", error.to_string()))?;

    turn.delivery_observer.mark_unknown().map_err(|error| {
        acp_not_delivered_error(
            "delivery_intent_persistence_failed",
            format!("Failed to persist ACP delivery intent before dispatch: {error}"),
        )
    })?;
    state.begin_prompt();
    let sent = cx.send_request(PromptRequest::new(native_session_id.clone(), prompt));
    delivery.mark_sent();
    let request_id: Option<agent_client_protocol::schema::v1::RequestId> =
        serde_json::from_value(sent.id()).ok();
    let mut prompt_result = Box::pin(acp_response_with_projection_barrier(
        sent,
        notification_ingress,
    ));
    let mut abort = Box::pin(wait_for_optional_abort(turn.abort.clone()));
    let mut observed_response_barriers = std::collections::BTreeSet::new();
    let prompt_response = loop {
        tokio::select! {
            biased;
            forced = force_rx.changed() => {
                if forced.is_err() || *force_rx.borrow() {
                    let _ = cx.send_notification(CancelNotification::new(native_session_id.clone()));
                    if let Some(request_id) = request_id {
                        let _ = cx.send_cancel_request(request_id);
                    }
                    state.finish();
                    return Err(acp_unknown_delivery_error(ACP_PROCESS_FORCE_SHUTDOWN_MESSAGE));
                }
            }
            _ = &mut abort => {
                let _ = cx.send_notification(CancelNotification::new(native_session_id.clone()));
                if let Some(request_id) = request_id {
                    let _ = cx.send_cancel_request(request_id);
                }
                let _ = tokio::time::timeout(Duration::from_secs(2), &mut prompt_result).await;
                state.finish();
                return Err(Error::Message(ACP_PEER_ABORT_MESSAGE.to_string()));
            }
            response = &mut prompt_result => {
                let (response, barrier) = response.map_err(|error| acp_unknown_delivery_error(format!(
                        "ACP prompt delivery is unknown after a connection error: {}",
                        safe_acp_error(&error)
                    )))?;
                turn.delivery_observer.confirm().map_err(|error| {
                    acp_unknown_delivery_error(format!(
                        "ACP prompt response was observed but delivery confirmation could not be persisted: {error}"
                    ))
                })?;
                if !observed_response_barriers.contains(&barrier) {
                    reduce_acp_notifications_through_barrier(
                        notification_rx,
                        sessions,
                        generation,
                        barrier,
                        None,
                        Some(&native_session_id),
                        Some(&mut state),
                    )
                    .await?;
                }
                break response;
            }
            notification = notification_rx.recv() => {
                if let Some(notification) = notification {
                    if !acp_notification_is_for_session_or_barrier(
                        &notification,
                        Some(&native_session_id),
                    ) {
                        continue;
                    }
                    let reduction = {
                        let mut sessions = sessions.lock().await;
                        reduce_acp_inbound_notification(
                            &mut sessions,
                            generation,
                            notification,
                            None,
                            Some(&native_session_id),
                            Some(&mut state),
                        )
                    };
                        if reduction.active_session_observed {
                            turn.delivery_observer.confirm().map_err(|error| {
                                acp_unknown_delivery_error(format!(
                                    "ACP delivery was observed but could not be persisted: {error}"
                                ))
                            })?;
                        }
                        if let Some(barrier) = reduction.barrier {
                            observed_response_barriers.insert(barrier);
                        }
                }
            }
        }
    };
    let codex_prompt_quota = project_codex_prompt_quota(initialized, prompt_response.meta.as_ref());
    if let Some(usage) = prompt_response.usage {
        state.handle_prompt_usage(serde_json::to_value(usage).unwrap_or(Value::Null));
    }
    match codex_prompt_quota {
        Ok(Some(quota)) => state.handle_codex_prompt_quota(quota),
        Err(rejection) => state.handle_codex_prompt_quota_rejection(rejection),
        Ok(None) => {}
    }
    state.finish();
    let final_answer = state.final_answer.clone();
    let final_content = state.final_message_content();
    let content_slots = state.content_slots.clone();
    let latest_plan = state.latest_plan.clone();
    let session_title = state.session_title.clone();
    let prompt_usage = state.prompt_usage.clone();
    let usage_update = state.usage_update.clone();
    let tools = state
        .tools
        .iter()
        .map(|(tool_call_id, state)| (tool_call_id.clone(), state.value.clone()))
        .collect();
    let session_snapshot = sessions
        .lock()
        .await
        .get(&turn.local_session_id)
        .map(|session| acp_session_snapshot(session, generation))
        .ok_or_else(|| {
            Error::Message("resident ACP session disappeared after prompt completion".to_string())
        })?;
    Ok(AcpTurnOutput {
        native_session_id,
        final_answer,
        final_content,
        content_slots,
        latest_plan,
        session_title,
        tools,
        prompt_usage,
        usage_update,
        events: state.events,
        session_snapshot,
    })
}

#[allow(clippy::too_many_arguments)]
async fn inspect_resident_acp_session(
    cx: &ConnectionTo<Agent>,
    initialized: &InitializeResponse,
    peer: &ResolvedPeerTurn,
    contexts: &Arc<Mutex<BTreeMap<String, Arc<AcpClientContext>>>>,
    sessions: &AcpResidentSessions,
    notification_ingress: &AcpNotificationIngress,
    notification_rx: &mut AcpNotificationSubscription,
    next_session_epoch: &AtomicU64,
    generation: u64,
    local_session_id: String,
    native_session_id: String,
    cwd: PathBuf,
    mcp_servers: Vec<psychevo_runtime::ResolvedMcpServerInput>,
) -> psychevo_runtime::Result<AcpSessionSnapshot> {
    let session = ensure_resident_acp_session(
        cx,
        initialized,
        peer,
        contexts,
        sessions,
        notification_ingress,
        notification_rx,
        next_session_epoch,
        generation,
        &local_session_id,
        Some(&native_session_id),
        &cwd,
        &mcp_servers,
        None,
        None,
        None,
        None,
        None,
    )
    .await?;
    Ok(acp_session_snapshot(&session, generation))
}

#[allow(clippy::too_many_arguments)]
async fn load_resident_acp_session(
    cx: &ConnectionTo<Agent>,
    initialized: &InitializeResponse,
    peer: &ResolvedPeerTurn,
    contexts: &Arc<Mutex<BTreeMap<String, Arc<AcpClientContext>>>>,
    sessions: &AcpResidentSessions,
    notification_ingress: &AcpNotificationIngress,
    notification_rx: &mut AcpNotificationSubscription,
    next_session_epoch: &AtomicU64,
    generation: u64,
    local_session_id: String,
    native_session_id: String,
    cwd: PathBuf,
    mcp_servers: Vec<psychevo_runtime::ResolvedMcpServerInput>,
) -> psychevo_runtime::Result<AcpSessionLoadOutput> {
    let mut state = AcpPeerStreamState::new(None, None, local_session_id.clone());
    let session = ensure_resident_acp_session(
        cx,
        initialized,
        peer,
        contexts,
        sessions,
        notification_ingress,
        notification_rx,
        next_session_epoch,
        generation,
        &local_session_id,
        Some(&native_session_id),
        &cwd,
        &mcp_servers,
        None,
        None,
        None,
        None,
        Some(&mut state),
    )
    .await?;
    state.finish();
    Ok(AcpSessionLoadOutput {
        snapshot: acp_session_snapshot(&session, generation),
        replay: state.history_replay,
    })
}

#[allow(clippy::too_many_arguments)]
async fn prepare_resident_acp_session(
    cx: &ConnectionTo<Agent>,
    initialized: &InitializeResponse,
    peer: &ResolvedPeerTurn,
    contexts: &Arc<Mutex<BTreeMap<String, Arc<AcpClientContext>>>>,
    sessions: &AcpResidentSessions,
    notification_ingress: &AcpNotificationIngress,
    notification_rx: &mut AcpNotificationSubscription,
    next_session_epoch: &AtomicU64,
    generation: u64,
    local_session_id: String,
    cwd: PathBuf,
    mcp_servers: Vec<psychevo_runtime::ResolvedMcpServerInput>,
) -> psychevo_runtime::Result<AcpSessionSnapshot> {
    let session = ensure_resident_acp_session(
        cx,
        initialized,
        peer,
        contexts,
        sessions,
        notification_ingress,
        notification_rx,
        next_session_epoch,
        generation,
        &local_session_id,
        None,
        &cwd,
        &mcp_servers,
        None,
        None,
        None,
        None,
        None,
    )
    .await?;
    Ok(acp_session_snapshot(&session, generation))
}

#[allow(clippy::too_many_arguments)]
async fn set_resident_acp_control(
    cx: &ConnectionTo<Agent>,
    initialized: &InitializeResponse,
    peer: &ResolvedPeerTurn,
    contexts: &Arc<Mutex<BTreeMap<String, Arc<AcpClientContext>>>>,
    sessions: &AcpResidentSessions,
    notification_ingress: &AcpNotificationIngress,
    notification_rx: &mut AcpNotificationSubscription,
    next_session_epoch: &AtomicU64,
    generation: u64,
    local_session_id: String,
    native_session_id: String,
    cwd: PathBuf,
    mcp_servers: Vec<psychevo_runtime::ResolvedMcpServerInput>,
    control_id: String,
    value: Value,
) -> psychevo_runtime::Result<AcpSessionSnapshot> {
    let mut session = ensure_resident_acp_session(
        cx,
        initialized,
        peer,
        contexts,
        sessions,
        notification_ingress,
        notification_rx,
        next_session_epoch,
        generation,
        &local_session_id,
        Some(&native_session_id),
        &cwd,
        &mcp_servers,
        None,
        None,
        None,
        None,
        None,
    )
    .await?;
    let option = session
        .config_options
        .iter()
        .find(|option| option.id.to_string() == control_id)
        .cloned();
    if option.is_none()
        && control_id == "model"
        && effective_legacy_models(&session.config_options, session.legacy_models.as_ref()).is_some()
    {
        let requested_model = value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                acp_not_delivered_error(
                    "acp_control_invalid",
                    "ACP legacy model selector requires a non-empty string value",
                )
            })?;
        let response_barrier = session_controls::apply_legacy_model_selection(
            cx,
            notification_ingress,
            &session.config_options,
            &mut session.legacy_models,
            &native_session_id,
            requested_model,
        )
        .await?;
        sessions
            .lock()
            .await
            .insert(local_session_id.clone(), session);
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
        let session = sessions
            .lock()
            .await
            .get(&local_session_id)
            .cloned()
            .ok_or_else(|| {
                Error::Message("resident ACP session disappeared after model update".to_string())
            })?;
        return Ok(acp_session_snapshot(&session, generation));
    }
    if option.is_none() && control_id == "mode" {
        let requested_mode = value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                acp_not_delivered_error(
                    "acp_control_invalid",
                    "ACP session mode requires a non-empty string value",
                )
            })?;
        if !session
            .available_modes
            .iter()
            .any(|mode| mode.id == requested_mode)
        {
            return Err(acp_not_delivered_error(
                "acp_control_invalid",
                format!("ACP session does not expose mode `{requested_mode}`"),
            ));
        }
        let (_, response_barrier) = acp_response_with_projection_barrier(
            cx.send_request(SetSessionModeRequest::new(
                native_session_id.clone(),
                requested_mode.to_string(),
            )),
            notification_ingress,
        )
        .await
        .map_err(|error| {
            acp_agent_not_delivered_error("acp_control_rejected", "session/set_mode", &error)
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
        let session = sessions
            .lock()
            .await
            .get(&local_session_id)
            .cloned()
            .ok_or_else(|| {
                Error::Message("resident ACP session disappeared after mode update".to_string())
            })?;
        return Ok(acp_session_snapshot(&session, generation));
    }
    let option = option.ok_or_else(|| {
        acp_not_delivered_error(
            "acp_control_not_found",
            format!("ACP session does not expose control `{control_id}`"),
        )
    })?;
    let value = session_controls::acp_config_option_json_value(&option, value)?;
    let (response, response_barrier) = acp_response_with_projection_barrier(
        cx.send_request(SetSessionConfigOptionRequest::new(
            native_session_id.clone(),
            control_id,
            value,
        )),
        notification_ingress,
    )
    .await
    .map_err(|error| {
        acp_agent_not_delivered_error("acp_control_rejected", "session/set_config_option", &error)
    })?;
    session.config_options = response.config_options;
    sessions
        .lock()
        .await
        .insert(local_session_id.clone(), session);
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
    let session = sessions
        .lock()
        .await
        .get(&local_session_id)
        .cloned()
        .ok_or_else(|| {
            Error::Message("resident ACP session disappeared after control update".to_string())
        })?;
    Ok(acp_session_snapshot(&session, generation))
}

fn acp_not_delivered_error(code: &str, message: impl Into<String>) -> Error {
    crate::agent_session_error(
        code,
        crate::AgentErrorStage::Delivery,
        "user_action",
        "not_delivered",
        message,
        Some("acp-process".to_string()),
    )
}
