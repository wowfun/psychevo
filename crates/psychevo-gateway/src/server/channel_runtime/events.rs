use super::*;

pub(super) fn channel_reply_thread_id(state: &WebState, source: &GatewaySource) -> String {
    state
        .inner
        .gateway
        .resolve_source_thread(source)
        .ok()
        .flatten()
        .unwrap_or_else(|| source.source_key().0)
}

pub(super) fn channel_event_sink(
    runtime: ChannelRuntimeState,
    connection_id: String,
    channel_gateway: ChannelGateway,
    identity: ImIdentity,
    fallback_source_key: SourceKey,
) -> GatewayEventSink {
    Arc::new(move |event| {
        let Some(text) = channel_event_reply_text(&event) else {
            return;
        };
        let thread_id = channel_event_thread_id(&event, &fallback_source_key);
        let gateway = channel_gateway.clone();
        let runtime = runtime.clone();
        let connection_id = connection_id.clone();
        let identity = identity.clone();
        tokio::spawn(async move {
            let result = gateway
                .send(ImOutboundMessage {
                    identity,
                    thread_id,
                    text,
                })
                .await;
            match result {
                Ok(()) => runtime.mark_outbound(&connection_id),
                Err(err) => {
                    runtime.mark_error(&connection_id, &err);
                    eprintln!(
                        "channel event delivery failed: id={} error={}",
                        connection_id,
                        redact_channel_error(&err.to_string())
                    );
                }
            }
        });
    })
}

fn channel_event_reply_text(event: &GatewayEvent) -> Option<String> {
    match event {
        GatewayEvent::PermissionRequested {
            request_id,
            tool_name,
            summary,
            reason,
            ..
        } => {
            let detail = if !summary.trim().is_empty() {
                summary.trim()
            } else if !reason.trim().is_empty() {
                reason.trim()
            } else {
                "approval requested"
            };
            Some(format!(
                "Permission required for {tool_name}: {detail}. Reply /approve {request_id} to allow once or /deny {request_id} to deny."
            ))
        }
        GatewayEvent::ClarifyRequested {
            request_id, raw, ..
        } => {
            let question = raw
                .get("questions")
                .and_then(Value::as_array)
                .and_then(|questions| questions.first())
                .and_then(|question| question.get("question"))
                .and_then(Value::as_str)
                .filter(|question| !question.trim().is_empty())
                .unwrap_or("Please provide more information.");
            Some(format!(
                "Psychevo asks: {question}. Reply /answer {request_id} <answer> or /cancel {request_id}."
            ))
        }
        _ => None,
    }
}

fn channel_event_thread_id(event: &GatewayEvent, fallback_source_key: &SourceKey) -> String {
    match event {
        GatewayEvent::PermissionRequested {
            thread_id,
            source_key,
            ..
        }
        | GatewayEvent::ClarifyRequested {
            thread_id,
            source_key,
            ..
        } => thread_id
            .clone()
            .or_else(|| source_key.clone())
            .unwrap_or_else(|| fallback_source_key.0.clone()),
        _ => fallback_source_key.0.clone(),
    }
}
