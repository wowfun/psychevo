use super::commands::{ChannelCommandAction, route_channel_command};
use super::events::{channel_event_sink, channel_reply_thread_id};
use super::*;
use futures::future::BoxFuture;
use sha2::{Digest, Sha256};

pub(super) async fn run_channel_loop(
    state: WebState,
    runtime: ChannelRuntimeState,
    connection: ChannelRuntimeConnection,
    channel_gateway: ChannelGateway,
    cancel: CancellationToken,
) {
    runtime.mark_running(&connection.id);
    eprintln!(
        "channel runner started: id={} channel={}",
        connection.id, connection.channel
    );
    if let Err(err) =
        retry_unacknowledged_channel_outbox(&state, &runtime, &connection, &channel_gateway).await
    {
        runtime.mark_error(&connection.id, &err);
        eprintln!(
            "channel outbox retry failed: id={} channel={} error={}",
            connection.id,
            connection.channel,
            redact_channel_error(&err.to_string())
        );
    }
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                runtime.mark_stopped(&connection.id);
                eprintln!(
                    "channel runner stopped: id={} channel={}",
                    connection.id, connection.channel
                );
                break;
            }
            result = channel_gateway.poll_once() => {
                match result {
                    Ok(messages) => {
                        let poll_reason = if messages.is_empty() {
                            Some("polling_empty")
                        } else {
                            Some("running")
                        };
                        runtime.mark_poll(&connection.id, poll_reason);
                        for message in messages {
                            runtime.mark_inbound(&connection.id);
                            if let Err(err) = handle_channel_message(
                                &state,
                                &runtime,
                                &connection,
                                &channel_gateway,
                                message,
                            )
                            .await
                            {
                                runtime.mark_error(&connection.id, &err);
                                eprintln!(
                                    "channel message failed: id={} channel={} error={}",
                                    connection.id,
                                    connection.channel,
                                    redact_channel_error(&err.to_string())
                                );
                            }
                        }
                        tokio::time::sleep(Duration::from_millis(CHANNEL_IDLE_SLEEP_MS)).await;
                    }
                    Err(err) => {
                        let message = err.to_string();
                        if connection.channel == "wechat"
                            && is_wechat_ilink_session_expired_error(&message)
                        {
                            if runtime.wechat_login_grace_active(&connection.id) {
                                runtime.mark_wechat_qr_login_pending(
                                    &connection.id,
                                    wechat_ilink_error_code_from_message(&message),
                                );
                                eprintln!(
                                    "channel runner waiting: id={} channel={} reason=qr_login_pending error={}",
                                    connection.id,
                                    connection.channel,
                                    redact_channel_error(&message)
                                );
                                tokio::time::sleep(Duration::from_millis(CHANNEL_POLL_BACKOFF_MS)).await;
                                continue;
                            }
                            runtime.deactivate(&connection.id);
                            runtime.mark_blocked_with_reason(
                                &connection.id,
                                Some("needs_qr_login"),
                                message.clone(),
                                wechat_ilink_error_code_from_message(&message),
                            );
                            eprintln!(
                                "channel runner blocked: id={} channel={} reason=needs_qr_login error={}",
                                connection.id,
                                connection.channel,
                                redact_channel_error(&message)
                            );
                            break;
                        }
                        runtime.mark_error(&connection.id, &err);
                        eprintln!(
                            "channel poll failed: id={} channel={} error={}",
                            connection.id,
                            connection.channel,
                            redact_channel_error(&err.to_string())
                        );
                        tokio::time::sleep(Duration::from_millis(CHANNEL_POLL_BACKOFF_MS)).await;
                    }
                }
            }
        }
    }
}

pub(super) async fn retry_unacknowledged_channel_outbox(
    state: &WebState,
    runtime: &ChannelRuntimeState,
    connection: &ChannelRuntimeConnection,
    channel_gateway: &ChannelGateway,
) -> psychevo_runtime::Result<usize> {
    let records = state
        .inner
        .state
        .retryable_gateway_channel_outbox(&connection.id)?;
    let mut delivered = 0;
    let mut first_error = None;
    for record in records {
        let delivery_id = record.delivery_id.clone();
        match deliver_channel_outbox_record(state, runtime, channel_gateway, record).await {
            Ok(()) => delivered += 1,
            Err(err) => {
                let _ = state.inner.state.fail_gateway_channel_outbox(&delivery_id);
                first_error.get_or_insert(err);
            }
        }
    }
    if let Some(err) = first_error {
        Err(err)
    } else {
        Ok(delivered)
    }
}

async fn deliver_channel_outbox_record(
    state: &WebState,
    runtime: &ChannelRuntimeState,
    channel_gateway: &ChannelGateway,
    record: psychevo_runtime::state::GatewayChannelOutboxRecord,
) -> psychevo_runtime::Result<()> {
    let payload = record.payload_text.ok_or_else(|| {
        psychevo_runtime::Error::Message(format!(
            "channel outbox `{}` has no retry payload",
            record.delivery_id
        ))
    })?;
    let actual_hash = format!("{:x}", Sha256::digest(payload.as_bytes()));
    if actual_hash != record.payload_hash {
        return Err(psychevo_runtime::Error::Message(format!(
            "channel outbox `{}` payload hash mismatch",
            record.delivery_id
        )));
    }
    let lane = state
        .inner
        .state
        .gateway_source_lane(&record.source_key)?
        .ok_or_else(|| {
            psychevo_runtime::Error::Message(format!(
                "channel outbox `{}` source lane is unavailable",
                record.delivery_id
            ))
        })?;
    let identity = channel_outbox_identity(&lane.raw_identity, &record.connection_id)?;
    channel_gateway
        .send(ImOutboundMessage {
            identity,
            thread_id: record.thread_id,
            text: payload,
        })
        .await?;
    state
        .inner
        .state
        .acknowledge_gateway_channel_outbox(&record.delivery_id)?;
    runtime.mark_outbound(&record.connection_id);
    Ok(())
}

fn channel_outbox_identity(
    raw: &Value,
    connection_id: &str,
) -> psychevo_runtime::Result<ImIdentity> {
    let required = |field: &str| {
        raw.get(field)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                psychevo_runtime::Error::Message(format!(
                    "channel outbox source identity is missing `{field}`"
                ))
            })
    };
    let optional = |field: &str| {
        raw.get(field)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    Ok(ImIdentity {
        connection_id: Some(connection_id.to_string()),
        platform: required("platform")?,
        domain: optional("domain"),
        workspace_id: optional("workspaceId"),
        chat_type: optional("chatType"),
        chat_id: required("chatId")?,
        thread_id: optional("threadId"),
        user_id: optional("userId"),
        operator_id: optional("operatorId"),
        reply_to: optional("replyTo"),
    })
}

pub(super) async fn handle_channel_message(
    state: &WebState,
    runtime: &ChannelRuntimeState,
    connection: &ChannelRuntimeConnection,
    channel_gateway: &ChannelGateway,
    mut message: ImInboundMessage,
) -> psychevo_runtime::Result<()> {
    let source = gateway_source_for_im(&message);
    let mut requested_thread_id = None;
    if let Some(action) =
        route_channel_command(state, runtime, connection, &message, &source).await?
    {
        match action {
            ChannelCommandAction::Reply(reply) => {
                channel_gateway
                    .send(ImOutboundMessage {
                        identity: message.identity,
                        thread_id: channel_reply_thread_id(state, &source),
                        text: reply,
                    })
                    .await?;
                runtime.mark_outbound(&connection.id);
                return Ok(());
            }
            ChannelCommandAction::SubmitPrompt { text, thread_id } => {
                message.text = text;
                requested_thread_id = thread_id;
            }
            ChannelCommandAction::Compact { instructions } => {
                let pending =
                    enqueue_channel_compaction(state, runtime, connection, &source, instructions)?;
                let reply_runtime = runtime.clone();
                let reply_connection = connection.clone();
                let reply_gateway = channel_gateway.clone();
                let reply_identity = message.identity;
                let fallback_thread_id = channel_reply_thread_id(state, &source);
                let _handle = tokio::spawn(async move {
                    let (thread_id, text) = match pending.await {
                        Ok(wire::ThreadActionRunResult::Compact { thread_id, result }) => {
                            (thread_id, channel_compaction_reply(&result))
                        }
                        Ok(_) => (
                            fallback_thread_id,
                            "Context compaction returned an unexpected action result.".to_string(),
                        ),
                        Err(err) => (
                            fallback_thread_id,
                            format!("Context compaction failed: {err}"),
                        ),
                    };
                    if let Err(err) = reply_gateway
                        .send(ImOutboundMessage {
                            identity: reply_identity,
                            thread_id,
                            text,
                        })
                        .await
                    {
                        reply_runtime.mark_error(&reply_connection.id, &err);
                    } else {
                        reply_runtime.mark_outbound(&reply_connection.id);
                    }
                });
                return Ok(());
            }
        }
    }
    let turn_state = state.clone();
    let turn_runtime = runtime.clone();
    let turn_connection = connection.clone();
    let turn_gateway = channel_gateway.clone();
    let _handle = tokio::spawn(async move {
        if let Err(err) = run_channel_inbound_turn(
            turn_state,
            turn_runtime.clone(),
            turn_connection.clone(),
            turn_gateway,
            message,
            source,
            requested_thread_id,
        )
        .await
        {
            turn_runtime.mark_error(&turn_connection.id, &err);
            eprintln!(
                "channel turn failed: id={} channel={} error={}",
                turn_connection.id,
                turn_connection.channel,
                redact_channel_error(&err.to_string())
            );
        }
    });
    Ok(())
}

fn enqueue_channel_compaction(
    state: &WebState,
    runtime: &ChannelRuntimeState,
    connection: &ChannelRuntimeConnection,
    source: &GatewaySource,
    instructions: Option<String>,
) -> psychevo_runtime::Result<
    BoxFuture<'static, psychevo_runtime::Result<wire::ThreadActionRunResult>>,
> {
    let thread_id = state
        .inner
        .gateway
        .resolve_source_thread(source)?
        .or_else(|| {
            state
                .activity(source, None)
                .running
                .then(|| runtime.observed_source_thread(&connection.id, &source.source_key()))
                .flatten()
        });
    let Some(thread_id) = thread_id else {
        return Err(Error::Message(
            "Open a channel Thread before compacting context.".to_string(),
        ));
    };
    let scope = super::commands::channel_resolved_scope(state, connection, source)?;
    let state = state.clone();
    let (out_tx, _out_rx) = mpsc::unbounded_channel();
    Ok(Box::pin(async move {
        run_routed_thread_action(
            &state,
            &scope,
            wire::ThreadActionRunParams {
                scope: scope.to_wire_scope(),
                thread_id,
                action: wire::ThreadActionInput::Compact { instructions },
            },
            out_tx.into(),
        )
        .await
    }))
}

fn channel_compaction_reply(result: &wire::ThreadCompactionResult) -> String {
    if result.compacted {
        if let (Some(before), Some(after)) = (result.tokens_before, result.tokens_after) {
            return format!("Session compacted ({before} -> {after} tokens).");
        }
        return "Session compacted.".to_string();
    }
    result.message.clone()
}

async fn run_channel_inbound_turn(
    state: WebState,
    runtime: ChannelRuntimeState,
    connection: ChannelRuntimeConnection,
    channel_gateway: ChannelGateway,
    message: ImInboundMessage,
    source: GatewaySource,
    thread_id: Option<String>,
) -> psychevo_runtime::Result<()> {
    let scope = super::commands::channel_resolved_scope(&state, &connection, &source)?;
    let default_runtime_ref = connection
        .runtime_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("native");
    let target = runnable_target_for_source(&state, &scope, &source, default_runtime_ref)?;
    let runtime_ref = target.runtime_profile_ref.clone();
    let turn_context = thread_context_read_result_for_target_id(
        &state,
        &scope,
        thread_id.clone(),
        &target.target_id,
    )
    .await?;
    let mut turn_overrides = BTreeMap::new();
    if let Some(model) = connection
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        turn_overrides.insert("model".to_string(), Value::String(model.to_string()));
    }
    if let Some(permission_mode) = connection
        .permission_mode
        .as_deref()
        .and_then(PermissionMode::parse)
    {
        turn_overrides.insert(
            "permissionMode".to_string(),
            Value::String(permission_mode.as_str().to_string()),
        );
    }
    let event_sink = channel_event_sink(
        runtime.clone(),
        connection.id.clone(),
        channel_gateway.clone(),
        message.identity.clone(),
        source.source_key(),
    );
    let input = gateway_input_parts_for_im(&message)?;
    let mut control_values = BTreeMap::new();
    apply_thread_control_precedence(&state, &scope, thread_id.as_deref(), &mut control_values)?;
    let initial_thread_preferences = source_draft_control_values(&turn_context)?;
    control_values.extend(initial_thread_preferences.clone());
    let result = run_routed_thread_turn(
        &state,
        &scope,
        RoutedThreadTurn {
            thread_id,
            context: turn_context,
            control_values,
            initial_thread_preferences,
            input,
            mentions: Vec::new(),
            turn_overrides,
            runtime_source: format!("channel/{}", connection.channel),
            continue_sources: vec![format!("channel/{}", connection.channel)],
            event_sink: Some(event_sink),
            workspace_mutations: None,
            lineage: Some(json!({
                "channel": connection.channel,
                "connectionId": connection.id,
                "messageId": message.message_id,
                "runtimeRef": runtime_ref,
            })),
            source: Some(source.clone()),
            bind_source: Some(source.clone()),
            turn_id: None,
        },
    )
    .await?;
    let answer = result.result.final_answer.trim().to_string();
    if answer.is_empty() {
        return Ok(());
    }
    let voice_policy = voice_policy_for_source(&state, &source);
    if voice_policy != wire::VoicePolicyMode::Off {
        eprintln!(
            "channel voice delivery fallback: id={} mode={:?} reason=native_voice_delivery_unavailable",
            connection.id, voice_policy
        );
    }
    let digest = Sha256::digest(
        format!(
            "channel-outbox\0{}\0{}\0{}\0{}",
            result.thread.id,
            result.turn.id,
            connection.id,
            source.source_key().0
        )
        .as_bytes(),
    );
    let delivery_id = format!("out_{digest:x}");
    let payload_hash = format!("{:x}", Sha256::digest(answer.as_bytes()));
    let record = state.inner.state.upsert_gateway_channel_outbox(
        psychevo_runtime::state::GatewayChannelOutboxInput {
            delivery_id: &delivery_id,
            thread_id: &result.thread.id,
            turn_id: &result.turn.id,
            connection_id: &connection.id,
            source_key: &source.source_key().0,
            payload_text: &answer,
            payload_hash: &payload_hash,
        },
    )?;
    if record.status == "acknowledged" {
        return Ok(());
    }
    let send_result = channel_gateway
        .send(ImOutboundMessage {
            identity: message.identity,
            thread_id: result.thread.id,
            text: answer,
        })
        .await;
    match send_result {
        Ok(()) => {
            state
                .inner
                .state
                .acknowledge_gateway_channel_outbox(&delivery_id)?;
        }
        Err(err) => {
            let _ = state.inner.state.fail_gateway_channel_outbox(&delivery_id);
            return Err(err);
        }
    }
    runtime.mark_outbound(&connection.id);
    Ok(())
}
