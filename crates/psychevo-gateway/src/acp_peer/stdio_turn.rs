use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    CancelNotification, LoadSessionRequest, LoadSessionResponse, NewSessionRequest,
    NewSessionResponse, PromptRequest, SetSessionConfigOptionRequest, SetSessionModeRequest,
};
use psychevo::{
    Error,
    application::{ImageInput, ResolvedMcpServerInput, RunStreamSink, WorkspaceMutationSink},
};
use serde_json::{Value, json};
use tokio::sync::watch;

use crate::gateway::agent_session::{AgentErrorStage, agent_session_error};
use crate::gateway::peer_runtime::ResolvedPeerTurn;
use psychevo_gateway_protocol as wire;

use super::acp_backend_effective_env;
use super::capability_packs::project_codex_prompt_quota;
use super::lifecycle::{
    acp_agent_not_delivered_error, mcp_declaration_fingerprint, remove_acp_context, safe_acp_error,
};
use super::mcp_handoff;
use super::metadata_permissions::{
    emit_runtime_event, peer_allows_fs_read, peer_allows_fs_write, peer_allows_terminal,
};
use super::process_pool::{
    ACP_PROCESS_FORCE_SHUTDOWN_MESSAGE, AcpDeliveryMarker, AcpProcessGeneration, AcpProcessPool,
    AcpSessionReadyCallback, acp_unknown_delivery_error,
};
use super::prompt_input;
use super::session_controls;
use super::session_projection::{
    AcpBarrierProjection, AcpNotificationSubscription, AcpResidentSession, AcpResidentSessionInput,
    AcpSessionSnapshot, acp_notification_is_for_session_or_barrier,
    acp_response_with_projection_barrier, acp_session_response_with_legacy_models,
    acp_session_snapshot, effective_legacy_models, new_acp_resident_session,
    next_acp_session_epoch, reduce_acp_inbound_notification,
    reduce_acp_notifications_through_barrier,
};
use super::stream_state::{
    AcpHistoryReplayProjection, AcpPeerStreamState, AcpSessionLoadOutput, AcpTurnOutput,
};
use super::turn::{ACP_PEER_ABORT_MESSAGE, AcpClientContext};

#[derive(Clone)]
pub(super) struct AcpPeerTurnContext {
    pub(super) cwd: PathBuf,
    pub(super) home: PathBuf,
    pub(super) local_session_id: String,
    pub(super) native_session_id: Option<String>,
    pub(super) native_session_slot: Arc<std::sync::Mutex<Option<String>>>,
    pub(super) input: Vec<wire::source::GatewayInputPart>,
    pub(super) prompt: String,
    pub(super) images: Vec<ImageInput>,
    pub(super) instructions: Option<String>,
    pub(super) peer_model: Option<String>,
    pub(super) peer_reasoning_effort: Option<String>,
    pub(super) peer_runtime_options: BTreeMap<String, String>,
    pub(super) mcp_servers: Vec<ResolvedMcpServerInput>,
    pub(super) stream: Option<RunStreamSink>,
    pub(super) workspace_mutations: Option<WorkspaceMutationSink>,
    pub(super) approval_handler: Option<Arc<dyn psychevo::ApprovalHandler>>,
    pub(super) turn_control: psychevo::TurnControl,
    pub(super) before_prompt: AcpBeforePromptCallback,
    pub(super) persistence: Arc<dyn psychevo::AgentTurnPersistence>,
}

pub(super) type AcpBeforePromptCallback = Arc<
    dyn Fn(AcpHistoryReplayProjection) -> futures::future::BoxFuture<'static, psychevo::Result<()>>
        + Send
        + Sync,
>;

pub(super) struct AcpResidentTurnInput {
    pub(super) peer: ResolvedPeerTurn,
    pub(super) turn: AcpPeerTurnContext,
    pub(super) session_ready: AcpSessionReadyCallback,
    pub(super) delivery: AcpDeliveryMarker,
}

pub(super) struct AcpSessionLoadInput {
    pub(super) local_session_id: String,
    pub(super) native_session_id: String,
    pub(super) cwd: PathBuf,
    pub(super) mcp_servers: Vec<ResolvedMcpServerInput>,
}

pub(super) struct AcpSessionPrepareInput {
    pub(super) local_session_id: String,
    pub(super) cwd: PathBuf,
    pub(super) mcp_servers: Vec<ResolvedMcpServerInput>,
}

pub(super) struct AcpResidentControlInput {
    pub(super) session: AcpSessionLoadInput,
    pub(super) control_id: String,
    pub(super) value: Value,
}

struct AcpSessionAttachment<'a> {
    local_session_id: &'a str,
    native_session_id: Option<&'a str>,
    cwd: &'a Path,
    mcp_servers: &'a [ResolvedMcpServerInput],
}

struct AcpEnsureSessionInput<'a> {
    peer: &'a ResolvedPeerTurn,
    attachment: AcpSessionAttachment<'a>,
    approval_handler: Option<Arc<dyn psychevo::ApprovalHandler>>,
    turn_control: Option<psychevo::TurnControl>,
    stream: Option<RunStreamSink>,
    active_state: Option<&'a mut AcpPeerStreamState>,
}

pub(crate) async fn resolve_peer_mcp_server_handoffs(
    peer: &ResolvedPeerTurn,
    configuration: &psychevo::Configuration,
) -> psychevo::Result<Vec<ResolvedMcpServerInput>> {
    let names = requested_peer_mcp_server_names(peer)?;
    configuration
        .resolve_mcp_server_handoffs(&names)
        .await
        .map_err(|error| {
            agent_session_error(
                "acp_mcp_configuration_invalid",
                AgentErrorStage::Binding,
                "user_action",
                "not_delivered",
                error.to_string(),
                Some(format!("acp-mcp:{}", peer.backend.id)),
            )
        })
}

pub(crate) fn requested_peer_mcp_server_names(
    peer: &ResolvedPeerTurn,
) -> psychevo::Result<BTreeSet<String>> {
    mcp_handoff::requested_peer_mcp_server_names(peer)
}

pub(super) async fn run_acp_stdio_turn(
    pool: &AcpProcessPool,
    peer: &ResolvedPeerTurn,
    context: &AcpPeerTurnContext,
    session_ready: AcpSessionReadyCallback,
) -> psychevo::Result<AcpTurnOutput> {
    pool.run_turn(peer.clone(), context.clone(), session_ready)
        .await
}

async fn wait_for_optional_abort(control: Option<psychevo::TurnControl>) {
    if let Some(control) = control {
        control.wait_for_interrupt().await;
    } else {
        std::future::pending::<()>().await;
    }
}

pub(super) fn is_acp_peer_abort_error(err: &Error) -> bool {
    err.to_string().contains(ACP_PEER_ABORT_MESSAGE)
}

async fn ensure_resident_acp_session(
    process: &AcpProcessGeneration,
    notification_rx: &mut AcpNotificationSubscription,
    input: AcpEnsureSessionInput<'_>,
) -> psychevo::Result<AcpResidentSession> {
    let cx = &process.cx;
    let initialized = process.initialized.as_ref();
    let contexts = &process.contexts;
    let sessions = &process.sessions;
    let notification_ingress = &process.notification_ingress;
    let next_session_epoch = process.next_session_epoch.as_ref();
    let generation = process.generation;
    let AcpEnsureSessionInput {
        peer,
        attachment:
            AcpSessionAttachment {
                local_session_id,
                native_session_id: requested_native_session_id,
                cwd,
                mcp_servers: resolved_mcp_servers,
            },
        approval_handler,
        turn_control,
        stream,
        mut active_state,
    } = input;
    let mcp_servers = mcp_handoff::acp_mcp_server_declarations(
        peer,
        resolved_mcp_servers,
        &initialized.agent_capabilities,
    )
    .map_err(|error| acp_not_delivered_error("acp_mcp_configuration_invalid", error.to_string()))?;
    let mcp_declaration_fingerprint = mcp_declaration_fingerprint(&mcp_servers)?;
    let client_context = Arc::new(AcpClientContext {
        cwd: cwd.to_path_buf(),
        fs_read: peer_allows_fs_read(peer),
        fs_write: peer_allows_fs_write(peer),
        approval_handler,
        turn_control,
        terminal: peer_allows_terminal(peer),
        terminal_env: acp_backend_effective_env(peer),
    });
    let existing_session = sessions.lock().await.get(local_session_id).cloned();
    if let Some(session) = existing_session {
        if requested_native_session_id
            .is_some_and(|requested| requested != session.native_session_id)
        {
            return Err(agent_session_error(
                "acp_session_identity_mismatch",
                AgentErrorStage::Binding,
                "never",
                "not_delivered",
                "The resident ACP process owns a different native session for this thread.",
                Some(format!("acp-session:{local_session_id}")),
            ));
        }
        if session.mcp_servers != mcp_servers {
            return Err(agent_session_error(
                "acp_mcp_binding_changed",
                AgentErrorStage::Binding,
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
            AcpBarrierProjection {
                sessions,
                generation,
                barrier_sequence: barrier,
                replay_native_session_id: None,
                active_native_session_id: Some(&session.native_session_id),
                active_state: active_state.as_deref_mut(),
            },
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
            return Err(agent_session_error(
                "acp_session_not_resumable",
                AgentErrorStage::History,
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
        AcpBarrierProjection {
            sessions,
            generation,
            barrier_sequence: response_barrier,
            replay_native_session_id: loaded_from_agent.then_some(native_session_id.as_str()),
            active_native_session_id: Some(&native_session_id),
            active_state: active_state.as_deref_mut(),
        },
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

pub(super) async fn execute_resident_acp_turn(
    process: &AcpProcessGeneration,
    notification_rx: &mut AcpNotificationSubscription,
    force_rx: &mut watch::Receiver<bool>,
    input: AcpResidentTurnInput,
) -> psychevo::Result<AcpTurnOutput> {
    let cx = &process.cx;
    let initialized = process.initialized.as_ref();
    let sessions = &process.sessions;
    let notification_ingress = &process.notification_ingress;
    let generation = process.generation;
    let AcpResidentTurnInput {
        peer,
        turn,
        session_ready,
        delivery,
    } = input;
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
        process,
        notification_rx,
        AcpEnsureSessionInput {
            peer: &peer,
            attachment: AcpSessionAttachment {
                local_session_id: &turn.local_session_id,
                native_session_id: turn.native_session_id.as_deref(),
                cwd: &turn.cwd,
                mcp_servers: &turn.mcp_servers,
            },
            approval_handler: turn.approval_handler.clone(),
            turn_control: Some(turn.turn_control.clone()),
            stream: turn.stream.clone(),
            active_state: Some(&mut state),
        },
    )
    .await?;
    let native_session_id = session.native_session_id.clone();
    if let Ok(mut slot) = turn.native_session_slot.lock() {
        *slot = Some(native_session_id.clone());
    }
    session_ready(native_session_id.clone())
        .await
        .map_err(|error| {
            agent_session_error(
                "acp_session_binding_failed",
                AgentErrorStage::Binding,
                "never",
                "not_delivered",
                format!("Failed to persist ACP native session identity before prompt: {error}"),
                Some(format!("acp-session:{}", turn.local_session_id)),
            )
        })?;
    (turn.before_prompt)(state.history_replay.clone())
        .await
        .map_err(|error| {
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
        AcpBarrierProjection {
            sessions,
            generation,
            barrier_sequence: config_barrier,
            replay_native_session_id: None,
            active_native_session_id: Some(&native_session_id),
            active_state: Some(&mut state),
        },
    )
    .await?;

    let prompt = prompt_input::acp_prompt_blocks(&peer, &turn, &initialized.agent_capabilities)
        .await
        .map_err(|error| acp_not_delivered_error("acp_input_rejected", error.to_string()))?;

    turn.persistence
        .mark_delivery_unknown()
        .await
        .map_err(|error| {
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
    let mut abort = Box::pin(wait_for_optional_abort(Some(turn.turn_control.clone())));
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
                turn.persistence.confirm_delivery().await.map_err(|error| {
                    acp_unknown_delivery_error(format!(
                        "ACP prompt response was observed but delivery confirmation could not be persisted: {error}"
                    ))
                })?;
                if !observed_response_barriers.contains(&barrier) {
                    reduce_acp_notifications_through_barrier(
                        notification_rx,
                        AcpBarrierProjection {
                            sessions,
                            generation,
                            barrier_sequence: barrier,
                            replay_native_session_id: None,
                            active_native_session_id: Some(&native_session_id),
                            active_state: Some(&mut state),
                        },
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
                            turn.persistence.confirm_delivery().await.map_err(|error| {
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
        session_snapshot,
    })
}

#[cfg(test)]
pub(super) async fn inspect_resident_acp_session(
    process: &AcpProcessGeneration,
    notification_rx: &mut AcpNotificationSubscription,
    input: AcpSessionLoadInput,
) -> psychevo::Result<AcpSessionSnapshot> {
    let AcpSessionLoadInput {
        local_session_id,
        native_session_id,
        cwd,
        mcp_servers,
    } = input;
    let session = ensure_resident_acp_session(
        process,
        notification_rx,
        AcpEnsureSessionInput {
            peer: &process.peer,
            attachment: AcpSessionAttachment {
                local_session_id: &local_session_id,
                native_session_id: Some(&native_session_id),
                cwd: &cwd,
                mcp_servers: &mcp_servers,
            },
            approval_handler: None,
            turn_control: None,
            stream: None,
            active_state: None,
        },
    )
    .await?;
    Ok(acp_session_snapshot(&session, process.generation))
}

pub(super) async fn load_resident_acp_session(
    process: &AcpProcessGeneration,
    notification_rx: &mut AcpNotificationSubscription,
    input: AcpSessionLoadInput,
) -> psychevo::Result<AcpSessionLoadOutput> {
    let AcpSessionLoadInput {
        local_session_id,
        native_session_id,
        cwd,
        mcp_servers,
    } = input;
    let mut state = AcpPeerStreamState::new(None, None, local_session_id.clone());
    let session = ensure_resident_acp_session(
        process,
        notification_rx,
        AcpEnsureSessionInput {
            peer: &process.peer,
            attachment: AcpSessionAttachment {
                local_session_id: &local_session_id,
                native_session_id: Some(&native_session_id),
                cwd: &cwd,
                mcp_servers: &mcp_servers,
            },
            approval_handler: None,
            turn_control: None,
            stream: None,
            active_state: Some(&mut state),
        },
    )
    .await?;
    state.finish();
    Ok(AcpSessionLoadOutput {
        snapshot: acp_session_snapshot(&session, process.generation),
        replay: state.history_replay,
    })
}

pub(super) async fn prepare_resident_acp_session(
    process: &AcpProcessGeneration,
    notification_rx: &mut AcpNotificationSubscription,
    input: AcpSessionPrepareInput,
) -> psychevo::Result<AcpSessionSnapshot> {
    let AcpSessionPrepareInput {
        local_session_id,
        cwd,
        mcp_servers,
    } = input;
    let session = ensure_resident_acp_session(
        process,
        notification_rx,
        AcpEnsureSessionInput {
            peer: &process.peer,
            attachment: AcpSessionAttachment {
                local_session_id: &local_session_id,
                native_session_id: None,
                cwd: &cwd,
                mcp_servers: &mcp_servers,
            },
            approval_handler: None,
            turn_control: None,
            stream: None,
            active_state: None,
        },
    )
    .await?;
    Ok(acp_session_snapshot(&session, process.generation))
}

pub(super) async fn set_resident_acp_control(
    process: &AcpProcessGeneration,
    notification_rx: &mut AcpNotificationSubscription,
    input: AcpResidentControlInput,
) -> psychevo::Result<AcpSessionSnapshot> {
    let cx = &process.cx;
    let sessions = &process.sessions;
    let notification_ingress = &process.notification_ingress;
    let generation = process.generation;
    let AcpResidentControlInput {
        session:
            AcpSessionLoadInput {
                local_session_id,
                native_session_id,
                cwd,
                mcp_servers,
            },
        control_id,
        value,
    } = input;
    let mut session = ensure_resident_acp_session(
        process,
        notification_rx,
        AcpEnsureSessionInput {
            peer: &process.peer,
            attachment: AcpSessionAttachment {
                local_session_id: &local_session_id,
                native_session_id: Some(&native_session_id),
                cwd: &cwd,
                mcp_servers: &mcp_servers,
            },
            approval_handler: None,
            turn_control: None,
            stream: None,
            active_state: None,
        },
    )
    .await?;
    let option = session
        .config_options
        .iter()
        .find(|option| option.id.to_string() == control_id)
        .cloned();
    if option.is_none()
        && control_id == "model"
        && effective_legacy_models(&session.config_options, session.legacy_models.as_ref())
            .is_some()
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
            AcpBarrierProjection {
                sessions,
                generation,
                barrier_sequence: response_barrier,
                replay_native_session_id: None,
                active_native_session_id: Some(&native_session_id),
                active_state: None,
            },
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
            AcpBarrierProjection {
                sessions,
                generation,
                barrier_sequence: response_barrier,
                replay_native_session_id: None,
                active_native_session_id: Some(&native_session_id),
                active_state: None,
            },
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
        AcpBarrierProjection {
            sessions,
            generation,
            barrier_sequence: response_barrier,
            replay_native_session_id: None,
            active_native_session_id: Some(&native_session_id),
            active_state: None,
        },
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

pub(super) fn acp_not_delivered_error(code: &str, message: impl Into<String>) -> Error {
    agent_session_error(
        code,
        AgentErrorStage::Delivery,
        "user_action",
        "not_delivered",
        message,
        Some("acp-process".to_string()),
    )
}
