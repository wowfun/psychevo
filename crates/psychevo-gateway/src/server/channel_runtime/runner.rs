use super::commands::{ChannelCommandAction, route_channel_command};
use super::events::{channel_event_sink, channel_reply_thread_id};
use super::paths::channel_cwd;
use super::*;
use futures::future::BoxFuture;

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
                let pending = enqueue_channel_compaction(
                    state,
                    runtime,
                    connection,
                    channel_gateway,
                    &message,
                    &source,
                    instructions,
                )?;
                let reply_runtime = runtime.clone();
                let reply_connection = connection.clone();
                let reply_gateway = channel_gateway.clone();
                let reply_identity = message.identity;
                let fallback_thread_id = channel_reply_thread_id(state, &source);
                let _handle = tokio::spawn(async move {
                    let (thread_id, text) = match pending.await {
                        Ok(result) => {
                            (result.session_id.clone(), channel_compaction_reply(&result))
                        }
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
    channel_gateway: &ChannelGateway,
    message: &ImInboundMessage,
    source: &GatewaySource,
    instructions: Option<String>,
) -> psychevo_runtime::Result<
    BoxFuture<'static, psychevo_runtime::Result<psychevo_runtime::CompactionResult>>,
> {
    let cwd = channel_cwd(&state.inner.cwd, connection);
    let mut options = state.run_options(cwd.clone(), None);
    let runtime_ref = channel_effective_runtime_ref(state, connection, source)?;
    options.model = channel_model_for_runtime(&runtime_ref, connection.model.as_deref());
    let event_sink = channel_event_sink(
        runtime.clone(),
        connection.id.clone(),
        channel_gateway.clone(),
        message.identity.clone(),
        source.source_key(),
    );
    state
        .inner
        .gateway
        .enqueue_compact_session(crate::SendCompactRequest {
            thread_id: None,
            source: Some(source.clone()),
            runtime_ref: Some(runtime_ref),
            cwd,
            config_path: options.config_path,
            model: options.model,
            reasoning_effort: options.reasoning_effort,
            instructions,
            force: true,
            reason: psychevo_runtime::CompactionReason::Manual,
            inherited_env: options.inherited_env,
            event_sink: Some(event_sink),
        })
}

fn channel_compaction_reply(result: &psychevo_runtime::CompactionResult) -> String {
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
    let cwd = channel_cwd(&state.inner.cwd, &connection);
    let mut options = state.run_options(cwd, thread_id.clone());
    let runtime_ref = channel_effective_runtime_ref(&state, &connection, &source)?;
    options.runtime_ref = Some(runtime_ref.clone());
    options.model = channel_model_for_runtime(&runtime_ref, connection.model.as_deref());
    options.permission_mode = connection
        .permission_mode
        .as_deref()
        .and_then(PermissionMode::parse)
        .or(options.permission_mode);
    let event_sink = channel_event_sink(
        runtime.clone(),
        connection.id.clone(),
        channel_gateway.clone(),
        message.identity.clone(),
        source.source_key(),
    );
    let result = state
        .inner
        .gateway
        .send_turn(crate::SendTurnRequest {
            thread_id,
            source: Some(source.clone()),
            bind_source: Some(source.clone()),
            reset_source_binding: false,
            input: gateway_input_parts_for_im(&message),
            options,
            runtime_source: Some(format!("channel/{}", connection.channel)),
            continue_sources: vec![format!("channel/{}", connection.channel)],
            stream: None,
            event_sink: Some(event_sink),
            control_handle: None,
            control: None,
            lineage: Some(json!({
                "channel": connection.channel,
                "connectionId": connection.id,
                "messageId": message.message_id,
                "runtimeRef": runtime_ref,
            })),
        })
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
    channel_gateway
        .send(ImOutboundMessage {
            identity: message.identity,
            thread_id: result.thread.id,
            text: answer,
        })
        .await?;
    runtime.mark_outbound(&connection.id);
    Ok(())
}

fn channel_model_for_runtime(runtime_ref: &str, configured: Option<&str>) -> Option<String> {
    (runtime_ref == "native")
        .then(|| {
            configured
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .flatten()
}

#[cfg(test)]
mod runtime_control_safety_tests {
    use super::channel_model_for_runtime;

    #[test]
    fn channel_model_override_is_native_only_until_adapter_declares_safe_control() {
        assert_eq!(
            channel_model_for_runtime("native", Some("provider/model")),
            Some("provider/model".to_string())
        );
        for runtime_ref in ["codex", "opencode", "acp:review"] {
            assert_eq!(
                channel_model_for_runtime(runtime_ref, Some("must-not-leak")),
                None
            );
        }
    }
}
