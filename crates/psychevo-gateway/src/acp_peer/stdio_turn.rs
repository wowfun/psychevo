#[derive(Clone)]
struct AcpPeerTurnContext {
    cwd: PathBuf,
    local_session_id: String,
    native_session_id: Option<String>,
    prompt: String,
    peer_model: Option<String>,
    peer_reasoning_effort: Option<String>,
    peer_runtime_mode: Option<String>,
    stream: Option<RunStreamSink>,
    approval_handler: Option<Arc<dyn psychevo_runtime::ApprovalHandler>>,
    abort: Option<AbortSignal>,
}

async fn run_acp_stdio_turn(
    peer: &ResolvedPeerTurn,
    context: &AcpPeerTurnContext,
) -> psychevo_runtime::Result<AcpTurnOutput> {
    match run_acp_stdio_turn_v2(peer, context).await {
        Ok(output) => return Ok(output),
        Err(err) if err.fallback_safe => {
            emit_runtime_event(
                &context.stream,
                json!({
                    "type": "acp_peer_protocol_fallback",
                    "session_id": context.local_session_id,
                    "source": "acp_peer",
                    "from": "2",
                    "to": "1",
                    "error": err.error.to_string(),
                }),
            );
        }
        Err(err) => return Err(err.error),
    }

    run_acp_stdio_turn_v1(peer, context).await
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

async fn drain_pending_acp_v2_notifications(
    notification_rx: &mut mpsc::UnboundedReceiver<acp_v2::SessionNotification>,
    _native_session_id: &str,
) {
    let drain_until = tokio::time::sleep(std::time::Duration::from_millis(50));
    tokio::pin!(drain_until);
    loop {
        tokio::select! {
            notification = notification_rx.next() => {
                if notification.is_none() {
                    break;
                }
            }
            _ = &mut drain_until => break,
        }
    }
}

struct AcpProtocolAttemptError {
    fallback_safe: bool,
    error: Error,
}

struct AcpPeerConfigSelection<'a> {
    config_id: &'static str,
    category: acp_v2::SessionConfigOptionCategory,
    requested: &'a str,
}

async fn apply_acp_v2_config_option(
    cx: &ConnectionTo<Agent>,
    config_options: &mut Vec<acp_v2::SessionConfigOption>,
    native_session_id: &str,
    local_session_id: &str,
    stream: &Option<RunStreamSink>,
    selection: AcpPeerConfigSelection<'_>,
) {
    let Some(value) = acp_v2_matching_select_value(config_options, &selection) else {
        emit_runtime_event(
            stream,
            json!({
                "type": "acp_peer_config_option_unmatched",
                "session_id": local_session_id,
                "source": "acp_peer",
                "protocol_version": "2",
                "config_id": selection.config_id,
                "requested": selection.requested,
            }),
        );
        return;
    };
    match cx
        .send_request(acp_v2::SetSessionConfigOptionRequest::new(
            native_session_id.to_string(),
            selection.config_id,
            value.as_str(),
        ))
        .block_task()
        .await
    {
        Ok(response) => {
            *config_options = response.config_options;
            emit_runtime_event(
                stream,
                json!({
                    "type": "acp_peer_config_option_set",
                    "session_id": local_session_id,
                    "source": "acp_peer",
                    "protocol_version": "2",
                    "config_id": selection.config_id,
                    "value": value,
                }),
            );
        }
        Err(err) => emit_runtime_event(
            stream,
            json!({
                "type": "acp_peer_config_option_failed",
                "session_id": local_session_id,
                "source": "acp_peer",
                "protocol_version": "2",
                "config_id": selection.config_id,
                "requested": selection.requested,
                "error": err.to_string(),
            }),
        ),
    }
}

fn acp_v2_matching_select_value(
    config_options: &[acp_v2::SessionConfigOption],
    selection: &AcpPeerConfigSelection<'_>,
) -> Option<String> {
    config_options
        .iter()
        .filter(|option| option.id.to_string() == selection.config_id)
        .find_map(|option| acp_v2_select_value(option, selection.requested))
        .or_else(|| {
            config_options
                .iter()
                .filter(|option| option.category.as_ref() == Some(&selection.category))
                .find_map(|option| acp_v2_select_value(option, selection.requested))
        })
}

fn acp_v2_select_value(option: &acp_v2::SessionConfigOption, requested: &str) -> Option<String> {
    let acp_v2::SessionConfigKind::Select(select) = &option.kind else {
        return None;
    };
    match &select.options {
        acp_v2::SessionConfigSelectOptions::Ungrouped(options) => options
            .iter()
            .find(|option| option.value.to_string() == requested)
            .map(|option| option.value.to_string()),
        acp_v2::SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter())
            .find(|option| option.value.to_string() == requested)
            .map(|option| option.value.to_string()),
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

async fn run_acp_stdio_turn_v2(
    peer: &ResolvedPeerTurn,
    turn: &AcpPeerTurnContext,
) -> Result<AcpTurnOutput, AcpProtocolAttemptError> {
    let (mut child, _launch_cwd) = acp_backend_attempt_command(peer, &turn.cwd)?;
    let mut child = child.spawn().map_err(|err| AcpProtocolAttemptError {
        fallback_safe: false,
        error: Error::Message(format!(
            "failed to spawn ACP backend `{}` ({}): {err}",
            peer.backend.id,
            acp_backend_command_text(peer).unwrap_or("<missing>")
        )),
    })?;
    let stdin = child.stdin.take().ok_or_else(|| AcpProtocolAttemptError {
        fallback_safe: false,
        error: Error::Message(format!(
            "ACP backend `{}` did not provide stdin",
            peer.backend.id
        )),
    })?;
    let stdout = child.stdout.take().ok_or_else(|| AcpProtocolAttemptError {
        fallback_safe: false,
        error: Error::Message(format!(
            "ACP backend `{}` did not provide stdout",
            peer.backend.id
        )),
    })?;
    let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());
    let client_context = Arc::new(AcpClientContext {
        cwd: turn.cwd.clone(),
        fs_read: peer_allows_fs_read(peer),
        fs_write: peer_allows_fs_write(peer),
        approval_handler: turn.approval_handler.clone(),
    });
    let cwd = turn.cwd.clone();
    let prompt_sent = Arc::new(AtomicBool::new(false));
    let prompt_sent_for_result = Arc::clone(&prompt_sent);
    let (notification_tx, notification_rx) = mpsc::unbounded::<acp_v2::SessionNotification>();

    emit_runtime_event(
        &turn.stream,
        json!({
            "type": "acp_peer_protocol_attempt",
            "session_id": turn.local_session_id,
            "source": "acp_peer",
            "protocol_version": "2",
        }),
    );

    let turn_stream = turn.stream.clone();
    let turn_local_session_id = turn.local_session_id.clone();
    let turn_native_session_id = turn.native_session_id.clone();
    let turn_prompt = turn.prompt.clone();
    let turn_peer_model = turn.peer_model.clone();
    let turn_peer_reasoning_effort = turn.peer_reasoning_effort.clone();
    let turn_peer_runtime_mode = turn.peer_runtime_mode.clone();
    let turn_abort = turn.abort.clone();

    let result = Client
        .v2()
        .name("psychevo-gateway-acp-peer")
        .on_receive_notification(
            {
                let notification_tx = notification_tx.clone();
                async move |notification: acp_v2::SessionNotification, _cx| {
                    let _ = notification_tx.unbounded_send(notification);
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            {
                let context = Arc::clone(&client_context);
                async move |request: acp_v2::RequestPermissionRequest, responder, _cx| {
                    let context = Arc::clone(&context);
                    responder.respond_with_result(request_permission_v2(context, request).await)
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(transport, async move |cx| {
            cx.send_request(
                acp_v2::InitializeRequest::new(ProtocolVersion::V2)
                    .capabilities(client_capabilities_v2())
                    .client_info(
                        acp_v2::Implementation::new(
                            "psychevo-gateway",
                            env!("CARGO_PKG_VERSION"),
                        )
                        .title("Psychevo Gateway"),
                    ),
            )
            .block_task()
            .await?;
            emit_runtime_event(
                &turn_stream,
                json!({
                    "type": "acp_peer_protocol_negotiated",
                    "session_id": turn_local_session_id,
                    "source": "acp_peer",
                    "protocol_version": "2",
                }),
            );

            let (native_session_id, mut config_options) = if let Some(native_session_id) = turn_native_session_id {
                let loaded = cx.send_request(acp_v2::LoadSessionRequest::new(
                    native_session_id.clone(),
                    &cwd,
                ))
                .block_task()
                .await?;
                (native_session_id, loaded.config_options.unwrap_or_default())
            } else {
                let created = cx.send_request(acp_v2::NewSessionRequest::new(&cwd))
                    .block_task()
                    .await?;
                (
                    created.session_id.to_string(),
                    created.config_options.unwrap_or_default(),
                )
            };

            if let Some(model) = turn_peer_model.as_deref() {
                apply_acp_v2_config_option(
                    &cx,
                    &mut config_options,
                    &native_session_id,
                    &turn_local_session_id,
                    &turn_stream,
                    AcpPeerConfigSelection {
                        config_id: "model",
                        category: acp_v2::SessionConfigOptionCategory::Model,
                        requested: model,
                    },
                )
                .await;
            }

            if let Some(effort) = turn_peer_reasoning_effort.as_deref() {
                apply_acp_v2_config_option(
                    &cx,
                    &mut config_options,
                    &native_session_id,
                    &turn_local_session_id,
                    &turn_stream,
                    AcpPeerConfigSelection {
                        config_id: "effort",
                        category: acp_v2::SessionConfigOptionCategory::ThoughtLevel,
                        requested: effort,
                    },
                )
                .await;
            }

            if let Some(mode) = turn_peer_runtime_mode.as_deref() {
                apply_acp_v2_config_option(
                    &cx,
                    &mut config_options,
                    &native_session_id,
                    &turn_local_session_id,
                    &turn_stream,
                    AcpPeerConfigSelection {
                        config_id: "mode",
                        category: acp_v2::SessionConfigOptionCategory::Mode,
                        requested: mode,
                    },
                )
                .await;
            }

            let prompt_request = acp_v2::PromptRequest::new(
                native_session_id.clone(),
                vec![acp_v2::ContentBlock::Text(acp_v2::TextContent::new(
                    turn_prompt,
                ))],
            );
            let mut notification_rx = notification_rx;
            drain_pending_acp_v2_notifications(&mut notification_rx, &native_session_id).await;
            let mut state = AcpPeerStreamState::new(turn_stream, turn_local_session_id);
            let mut notification_rx = notification_rx.fuse();
            let (done_tx, done_rx) =
                oneshot::channel::<Result<acp_v2::PromptResponse, agent_client_protocol::Error>>();
            prompt_sent.store(true, Ordering::SeqCst);
            cx.spawn({
                let cx = cx.clone();
                async move {
                    let result = cx.send_request(prompt_request).block_task().await;
                    let _ = done_tx.send(result);
                    Ok(())
                }
            })?;
            let mut done_rx = done_rx.fuse();
            let abort = wait_for_optional_abort(turn_abort).fuse();
            futures::pin_mut!(abort);

            loop {
                futures::select! {
                    notification = notification_rx.next() => {
                        if let Some(notification) = notification {
                            if notification.session_id.to_string() == native_session_id {
                                state.handle_notification_v2(notification);
                            }
                        } else {
                            let response = done_rx
                                .await
                                .map_err(|_| agent_client_protocol::Error::internal_error().data("prompt response channel cancelled"))?;
                            response?;
                            break;
                        }
                    }
                    response = done_rx => {
                        let response = response
                            .map_err(|_| agent_client_protocol::Error::internal_error().data("prompt response channel cancelled"))?;
                        response?;
                        break;
                    }
                    _ = abort => {
                        state.finish();
                        return Err(agent_client_protocol::Error::internal_error().data(ACP_PEER_ABORT_MESSAGE));
                    }
                }
            }

            while let Some(notification) = notification_rx.next().now_or_never().flatten() {
                if notification.session_id.to_string() == native_session_id {
                    state.handle_notification_v2(notification);
                }
            }

            state.finish();
            let final_answer = state.final_answer.clone();
            let final_content = state.final_message_content();
            let content_slots = state.content_slots.clone();
            let session_title = state.session_title.clone();
            let usage_update = state.usage_update.clone();
            let tools = state
                .tools
                .iter()
                .map(|(tool_call_id, state)| (tool_call_id.clone(), state.value.clone()))
                .collect();
            Ok(AcpTurnOutput {
                native_session_id,
                final_answer,
                final_content,
                content_slots,
                session_title,
                tools,
                usage_update,
                events: state.events,
            })
        })
        .await;

    let _ = child.kill().await;
    let _ = child.wait().await;

    result.map_err(|err| AcpProtocolAttemptError {
        fallback_safe: !prompt_sent_for_result.load(Ordering::SeqCst),
        error: Error::Message(format!("ACP peer `{}` v2 failed: {err}", peer.backend.id)),
    })
}

async fn run_acp_stdio_turn_v1(
    peer: &ResolvedPeerTurn,
    turn: &AcpPeerTurnContext,
) -> psychevo_runtime::Result<AcpTurnOutput> {
    let (mut child, _launch_cwd) = acp_backend_command(peer, &turn.cwd)?;
    let mut child = child.spawn().map_err(|err| {
        Error::Message(format!(
            "failed to spawn ACP backend `{}` ({}): {err}",
            peer.backend.id,
            acp_backend_command_text(peer).unwrap_or("<missing>")
        ))
    })?;
    let stdin = child.stdin.take().ok_or_else(|| {
        Error::Message(format!(
            "ACP backend `{}` did not provide stdin",
            peer.backend.id
        ))
    })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        Error::Message(format!(
            "ACP backend `{}` did not provide stdout",
            peer.backend.id
        ))
    })?;
    let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());
    let context = Arc::new(AcpClientContext {
        cwd: turn.cwd.clone(),
        fs_read: peer_allows_fs_read(peer),
        fs_write: peer_allows_fs_write(peer),
        approval_handler: turn.approval_handler.clone(),
    });
    let cwd = turn.cwd.clone();
    let turn_stream = turn.stream.clone();
    let turn_local_session_id = turn.local_session_id.clone();
    let turn_native_session_id = turn.native_session_id.clone();
    let turn_prompt = turn.prompt.clone();
    let turn_abort = turn.abort.clone();

    let result = Client
        .builder()
        .name("psychevo-gateway-acp-peer")
        .on_receive_request(
            {
                let context = Arc::clone(&context);
                async move |request: ReadTextFileRequest, responder, _cx| {
                    let context = Arc::clone(&context);
                    responder.respond_with_result(read_text_file(context, request).await)
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let context = Arc::clone(&context);
                async move |request: WriteTextFileRequest, responder, _cx| {
                    let context = Arc::clone(&context);
                    responder.respond_with_result(write_text_file(context, request).await)
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let context = Arc::clone(&context);
                async move |request: RequestPermissionRequest, responder, _cx| {
                    let context = Arc::clone(&context);
                    responder.respond_with_result(request_permission(context, request).await)
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(transport, async move |cx| {
            let capabilities = client_capabilities(peer);
            cx.send_request(
                InitializeRequest::new(ProtocolVersion::V1)
                    .client_capabilities(capabilities)
                    .client_info(
                        Implementation::new("psychevo-gateway", env!("CARGO_PKG_VERSION"))
                            .title("Psychevo Gateway"),
                    ),
            )
            .block_task()
            .await?;
            emit_runtime_event(
                &turn_stream,
                json!({
                    "type": "acp_peer_protocol_negotiated",
                    "session_id": turn_local_session_id,
                    "source": "acp_peer",
                    "protocol_version": "1",
                }),
            );

            let mut session = if let Some(native_session_id) = turn_native_session_id {
                let loaded = cx
                    .send_request(LoadSessionRequest::new(native_session_id.clone(), &cwd))
                    .block_task()
                    .await?;
                cx.attach_session(
                    NewSessionResponse::new(native_session_id)
                        .modes(loaded.modes)
                        .meta(loaded.meta),
                    Vec::new(),
                )?
            } else {
                cx.build_session(&cwd)
                    .block_task()
                    .start_session()
                    .await?
            };
            let drain_until = tokio::time::sleep(std::time::Duration::from_millis(50));
            tokio::pin!(drain_until);
            loop {
                tokio::select! {
                    update = session.read_update() => {
                        if let SessionMessage::StopReason(_) = update? {
                            break;
                        }
                    }
                    _ = &mut drain_until => break,
                }
            }
            session.send_prompt(turn_prompt)?;
            let mut state = AcpPeerStreamState::new(turn_stream, turn_local_session_id);
            let abort = wait_for_optional_abort(turn_abort);
            tokio::pin!(abort);
            loop {
                let update = tokio::select! {
                    update = session.read_update() => update?,
                    _ = &mut abort => {
                        state.finish();
                        return Err(agent_client_protocol::Error::internal_error().data(ACP_PEER_ABORT_MESSAGE));
                    }
                };
                match update {
                    SessionMessage::SessionMessage(dispatch) => {
                        MatchDispatch::new(dispatch)
                            .if_notification(async |notif: SessionNotification| {
                                state.handle_notification(notif);
                                Ok(())
                            })
                            .await
                            .otherwise_ignore()?;
                    }
                    SessionMessage::StopReason(_stop_reason) => break,
                    _ => {}
                }
            }
            state.finish();
            let final_answer = state.final_answer.clone();
            let final_content = state.final_message_content();
            let content_slots = state.content_slots.clone();
            let session_title = state.session_title.clone();
            let usage_update = state.usage_update.clone();
            let tools = state
                .tools
                .iter()
                .map(|(tool_call_id, state)| (tool_call_id.clone(), state.value.clone()))
                .collect();
            Ok(AcpTurnOutput {
                native_session_id: session.session_id().to_string(),
                final_answer,
                final_content,
                content_slots,
                session_title,
                tools,
                usage_update,
                events: state.events,
            })
        })
        .await
        .map_err(|err| Error::Message(format!("ACP peer `{}` failed: {err}", peer.backend.id)));

    let _ = child.kill().await;
    let _ = child.wait().await;
    result
}
