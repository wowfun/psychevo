use serde_json::Value;

use crate::gateway::Gateway;
use crate::im::{ChannelGateway, ImIdentity, ImOutboundMessage};
use crate::{GatewayEventEmitter, gateway_now_ms};
use psychevo_gateway_protocol::events_transcript::{
    GatewayActionKind, GatewayEvent, PendingActionView,
};
use psychevo_gateway_protocol::source::{GatewaySource, SourceKey};

#[cfg(test)]
use psychevo_gateway_protocol::events_transcript::{
    TranscriptBlock, TranscriptBlockKind, TranscriptBlockStatus, TranscriptEntry,
    TranscriptEntryRole,
};

use super::super::binding::WebState;
use super::ChannelRuntimeState;
use super::channel_multi_question_guidance;
use super::paths::redact_channel_error;
use super::state::{ChannelInteractionKind, ChannelInteractionTokenInput};

pub(super) async fn channel_reply_thread_id(state: &WebState, source: &GatewaySource) -> String {
    state
        .inner
        .gateway
        .resolve_source_thread(source)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| source.source_key().0)
}

pub(super) fn channel_event_sink(
    supervisor: Option<Gateway>,
    runtime: ChannelRuntimeState,
    connection_id: String,
    channel_gateway: ChannelGateway,
    identity: ImIdentity,
    fallback_source_key: SourceKey,
) -> GatewayEventEmitter {
    GatewayEventEmitter::new(move |event| {
        if let Some(thread_id) = channel_event_public_thread_id(&event) {
            runtime.observe_source_thread(&connection_id, &fallback_source_key, thread_id);
        }
        let token = match &event {
            GatewayEvent::ActionRequested { action }
                if action.kind == GatewayActionKind::Permission =>
            {
                runtime.issue_interaction_token(
                    &connection_id,
                    &fallback_source_key,
                    ChannelInteractionTokenInput {
                        kind: ChannelInteractionKind::Permission,
                        action_id: &action.action_id,
                        thread_id: action.thread_id.as_deref(),
                        clarify_question_count: 0,
                        action_expires_at_ms: channel_action_token_expiry(action),
                    },
                )
            }
            GatewayEvent::ActionRequested { action }
                if action.kind == GatewayActionKind::Clarify =>
            {
                runtime.issue_interaction_token(
                    &connection_id,
                    &fallback_source_key,
                    ChannelInteractionTokenInput {
                        kind: ChannelInteractionKind::Clarify,
                        action_id: &action.action_id,
                        thread_id: action.thread_id.as_deref(),
                        clarify_question_count: channel_clarify_questions(action).len().max(1),
                        action_expires_at_ms: channel_action_token_expiry(action),
                    },
                )
            }
            GatewayEvent::ActionResolved { action_id, .. }
            | GatewayEvent::ActionCancelled { action_id, .. } => {
                runtime.revoke_interaction_action(&connection_id, action_id);
                None
            }
            _ => None,
        };
        let Some(text) = channel_event_reply_text(&event, token.as_deref()) else {
            return;
        };
        let thread_id = channel_event_thread_id(&event, &fallback_source_key);
        let gateway = channel_gateway.clone();
        let runtime = runtime.clone();
        let connection_id = connection_id.clone();
        let identity = identity.clone();
        let delivery = async move {
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
        };
        if let Some(supervisor) = &supervisor {
            supervisor.spawn_background("channel-event-delivery", delivery);
        } else {
            #[cfg(test)]
            {
                tokio::spawn(delivery);
            }
        }
    })
}

fn channel_event_public_thread_id(event: &GatewayEvent) -> Option<&str> {
    match event {
        GatewayEvent::TurnStarted { thread_id, .. }
        | GatewayEvent::TurnQueued { thread_id, .. }
        | GatewayEvent::TurnCompleted { thread_id, .. }
        | GatewayEvent::ActivityChanged { thread_id, .. } => thread_id.as_deref(),
        GatewayEvent::EntryStarted { entry, .. }
        | GatewayEvent::EntryUpdated { entry, .. }
        | GatewayEvent::EntryCompleted { entry, .. } => {
            (!entry.thread_id.trim().is_empty()).then_some(entry.thread_id.as_str())
        }
        GatewayEvent::EntryBlockTextDelta { thread_id, .. } => thread_id.as_deref(),
        GatewayEvent::ActionRequested { action } | GatewayEvent::ActionUpdated { action } => {
            action.thread_id.as_deref()
        }
        GatewayEvent::TitleChanged { thread_id, .. } => Some(thread_id),
        _ => None,
    }
}

fn channel_action_token_expiry(action: &PendingActionView) -> Option<i64> {
    action
        .payload
        .get("interactionExpiresAtMs")
        .and_then(Value::as_i64)
        .or_else(|| {
            action
                .payload
                .get("timeoutSecs")
                .and_then(Value::as_u64)
                .filter(|seconds| *seconds > 0)
                .and_then(|seconds| i64::try_from(seconds).ok())
                .map(|seconds| gateway_now_ms().saturating_add(seconds.saturating_mul(1_000)))
        })
}

fn channel_event_reply_text(event: &GatewayEvent, token: Option<&str>) -> Option<String> {
    match event {
        GatewayEvent::ActionRequested { action }
            if action.kind == GatewayActionKind::Permission =>
        {
            let token = token?;
            let tool_name = action
                .payload
                .get("toolName")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let summary = action
                .payload
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let reason = action
                .payload
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let detail = if !summary.trim().is_empty() {
                summary.trim()
            } else if !reason.trim().is_empty() {
                reason.trim()
            } else {
                "approval requested"
            };
            Some(format!(
                "Permission required for {tool_name}: {detail}. Reply /approve {} to allow once or /deny {} to deny.",
                token, token
            ))
        }
        GatewayEvent::ActionRequested { action } if action.kind == GatewayActionKind::Clarify => {
            let token = token?;
            let questions = channel_clarify_questions(action);
            if questions.len() > 1 {
                return Some(channel_multi_question_guidance(token));
            }
            let question = questions
                .first()
                .and_then(|question| question.get("question"))
                .and_then(Value::as_str)
                .filter(|question| !question.trim().is_empty())
                .unwrap_or("Please provide more information.");
            let punctuation = if question
                .chars()
                .last()
                .is_some_and(|character| matches!(character, '.' | '?' | '!'))
            {
                ""
            } else {
                "."
            };
            Some(format!(
                "Psychevo asks: {question}{punctuation} Reply /answer {} <answer> or /cancel {}.",
                token, token
            ))
        }
        GatewayEvent::Warning { kind, .. }
            if kind == "runtime_interaction_exposure_blocked" =>
        {
            Some(
                "A runtime interaction requires GUI Advanced mode and was declined on this Channel."
                    .to_string(),
            )
        }
        _ => None,
    }
}

fn channel_clarify_questions(action: &PendingActionView) -> &[Value] {
    action
        .payload
        .get("raw")
        .unwrap_or(&action.payload)
        .get("questions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn channel_event_thread_id(event: &GatewayEvent, fallback_source_key: &SourceKey) -> String {
    match event {
        GatewayEvent::ActionRequested { action } | GatewayEvent::ActionUpdated { action } => action
            .thread_id
            .clone()
            .or_else(|| action.source_key.clone())
            .unwrap_or_else(|| fallback_source_key.0.clone()),
        _ => fallback_source_key.0.clone(),
    }
}

#[cfg(test)]
mod delivery_filter_tests {
    use super::*;

    #[test]
    fn transcript_thinking_events_never_become_channel_messages() {
        let reasoning = GatewayEvent::EntryUpdated {
            turn_id: "turn-1".to_string(),
            entry: TranscriptEntry {
                id: "entry-1".to_string(),
                thread_id: "thread-1".to_string(),
                turn_id: Some("turn-1".to_string()),
                message_seq: None,
                role: TranscriptEntryRole::Assistant,
                status: TranscriptBlockStatus::Running,
                source: "runtime.profile".to_string(),
                blocks: vec![TranscriptBlock {
                    id: "block-1".to_string(),
                    kind: TranscriptBlockKind::Reasoning,
                    status: TranscriptBlockStatus::Running,
                    order: 0,
                    phase_ordinal: Some(1),
                    source: "runtime.profile".to_string(),
                    title: Some("Thinking".to_string()),
                    preview: Some("private reasoning".to_string()),
                    body: Some("private reasoning".to_string()),
                    detail: Some("private reasoning".to_string()),
                    artifact_ids: Vec::new(),
                    metadata: None,
                    result: None,
                    created_at_ms: 1,
                    updated_at_ms: 1,
                }],
                metadata: None,
                usage: None,
                accounting: None,
                created_at_ms: 1,
                updated_at_ms: 1,
            },
        };
        assert_eq!(channel_event_reply_text(&reasoning, None), None);
    }
}
