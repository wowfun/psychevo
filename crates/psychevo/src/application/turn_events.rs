use std::fmt;
use std::sync::Arc;

use futures::future::BoxFuture;
use serde_json::Value;

use super::{
    InteractionResponse, InteractionResponseReceipt, ItemStage, TurnControl, TurnEvent,
    TurnEventSender, TurnEventStream,
};
use crate::Result;
use crate::types::{RunStreamEvent, SessionEventPayload};

impl TurnEventSender {
    pub fn emit(&self, event: TurnEvent) {
        if matches!(
            event,
            TurnEvent::InteractionRequested { .. } | TurnEvent::InteractionResolved { .. }
        ) {
            self.interactions.observe(event);
        } else {
            self.log.push(event);
        }
    }

    pub fn emit_agent_event(&self, event: RunStreamEvent) {
        if let Some(event) = TurnEvent::from_run_stream(event) {
            self.emit(event);
        }
    }
}

impl fmt::Debug for TurnEventSender {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TurnEventSender(..)")
    }
}

impl TurnControl {
    pub(super) fn take_runtime_control(&self) -> Result<crate::types::RunControl> {
        self.runtime
            .lock()
            .expect("Turn control owner poisoned")
            .take()
            .ok_or_else(|| crate::Error::Message("Turn control owner was already consumed".into()))
    }

    pub fn is_interrupted(&self) -> bool {
        self.handle.inner.is_aborted()
    }

    pub async fn respond(
        &self,
        interaction_id: &str,
        response: InteractionResponse,
    ) -> Result<InteractionResponseReceipt> {
        self.interactions.respond(interaction_id, response).await
    }

    pub fn wait_for_interrupt(&self) -> BoxFuture<'static, ()> {
        let mut abort = self.abort.clone();
        Box::pin(async move {
            abort.wait_for_abort().await;
        })
    }

    pub async fn request_clarification(
        &self,
        request: crate::types::ClarifyRequestEvent,
    ) -> crate::types::ClarifyInteractionOutcome {
        let events = self.events.clone();
        let stream: crate::types::RunStreamSink = Arc::new(move |event| {
            events.emit_agent_event(event);
        });
        self.handle
            .request_clarification(request, stream, Some(self.abort.clone()))
            .await
    }
}

impl fmt::Debug for TurnControl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TurnControl(..)")
    }
}

impl TurnEvent {
    pub(super) fn from_run_stream(event: RunStreamEvent) -> Option<Self> {
        match event {
            RunStreamEvent::Event(event) => {
                let runtime_data = event.as_value().clone();
                match event.payload {
                    SessionEventPayload::MessageStarted { message } => Some(Self::Message {
                        stage: ItemStage::Started,
                        message,
                        usage: None,
                        metadata: None,
                        accounting: None,
                    }),
                    SessionEventPayload::MessageUpdated { message } => Some(Self::Message {
                        stage: ItemStage::Updated,
                        message,
                        usage: None,
                        metadata: None,
                        accounting: None,
                    }),
                    SessionEventPayload::MessageCompleted {
                        message,
                        usage,
                        metadata,
                        accounting,
                    } => Some(Self::Message {
                        stage: ItemStage::Completed,
                        message,
                        usage,
                        metadata,
                        accounting,
                    }),
                    SessionEventPayload::ReasoningDelta { text } => {
                        Some(Self::ReasoningDelta { text })
                    }
                    SessionEventPayload::ReasoningCompleted { text } => {
                        Some(Self::ReasoningCompleted { text })
                    }
                    SessionEventPayload::ToolCallPending { data }
                    | SessionEventPayload::ToolExecutionStarted { data } => Some(Self::Tool {
                        stage: ItemStage::Started,
                        data,
                    }),
                    SessionEventPayload::ToolExecutionUpdated { data } => Some(Self::Tool {
                        stage: ItemStage::Updated,
                        data,
                    }),
                    SessionEventPayload::ToolExecutionCompleted { data } => Some(Self::Tool {
                        stage: ItemStage::Completed,
                        data,
                    }),
                    SessionEventPayload::BlockingActionRequested {
                        action_id,
                        kind,
                        payload,
                    }
                    | SessionEventPayload::BlockingActionUpdated {
                        action_id,
                        kind,
                        payload,
                    } => Some(Self::InteractionRequested {
                        interaction_id: action_id,
                        kind: blocking_action_kind_label(kind).to_string(),
                        payload,
                    }),
                    SessionEventPayload::BlockingActionResolved {
                        action_id,
                        kind,
                        reason,
                    }
                    | SessionEventPayload::BlockingActionCancelled {
                        action_id,
                        kind,
                        reason,
                    } => Some(Self::InteractionResolved {
                        interaction_id: action_id,
                        kind: blocking_action_kind_label(kind).to_string(),
                        reason,
                    }),
                    SessionEventPayload::Warning { data } => Some(Self::Warning { data }),
                    SessionEventPayload::SessionConfigured { .. }
                    | SessionEventPayload::TurnStarted { .. }
                    | SessionEventPayload::TurnCompleted { .. }
                    | SessionEventPayload::AgentSessionStarted { .. }
                    | SessionEventPayload::ContextSnapshot { .. }
                    | SessionEventPayload::DeliveryDiagnostic { .. }
                    | SessionEventPayload::Diagnostic { .. } => {
                        Some(Self::Runtime { data: runtime_data })
                    }
                }
            }
            RunStreamEvent::AssistantTextDelta { text } => Some(Self::MessageDelta { text }),
            RunStreamEvent::ReasoningDelta { text } => Some(Self::ReasoningDelta { text }),
            RunStreamEvent::ReasoningEnd => Some(Self::ReasoningCompleted { text: None }),
            RunStreamEvent::ClarifyRequest(request) => {
                let interaction_id = request.call_id.clone();
                Some(Self::InteractionRequested {
                    interaction_id,
                    kind: "clarify".to_string(),
                    payload: serde_json::to_value(request).unwrap_or(Value::Null),
                })
            }
            RunStreamEvent::ClarifyResolved(resolved) => Some(Self::InteractionResolved {
                interaction_id: resolved.call_id,
                kind: "clarify".to_string(),
                reason: format!("{:?}", resolved.reason).to_lowercase(),
            }),
            RunStreamEvent::Scoped {
                session_id,
                turn_id,
                event,
            } => Some(Self::Scoped {
                thread_id: session_id,
                turn_id: turn_id?,
                event: Box::new(Self::from_run_stream(*event)?),
            }),
        }
    }
}

fn blocking_action_kind_label(kind: crate::types::BlockingActionKind) -> &'static str {
    match kind {
        crate::types::BlockingActionKind::Permission => "permission",
        crate::types::BlockingActionKind::Clarify => "clarify",
        crate::types::BlockingActionKind::CustomTool => "custom_tool",
        crate::types::BlockingActionKind::UserInput => "user_input",
    }
}

impl fmt::Debug for TurnEventStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TurnEventStream")
            .field("cursor", &self.cursor)
            .finish_non_exhaustive()
    }
}

impl TurnEventStream {
    pub async fn next(&mut self) -> Option<TurnEvent> {
        self.log.next(&mut self.cursor).await
    }
}
