use super::*;

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

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __emit_run_stream(&self, event: RunStreamEvent) {
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
}

impl fmt::Debug for TurnControl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TurnControl(..)")
    }
}

impl TurnEvent {
    pub(super) fn from_run_stream(event: RunStreamEvent) -> Option<Self> {
        match event {
            RunStreamEvent::Event(event) => match event.payload {
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
                SessionEventPayload::ReasoningDelta { text } => Some(Self::ReasoningDelta { text }),
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
                    kind: format!("{kind:?}").to_lowercase(),
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
                    kind: format!("{kind:?}").to_lowercase(),
                    reason,
                }),
                SessionEventPayload::Warning { data } => Some(Self::Warning { data }),
                SessionEventPayload::SessionConfigured { .. }
                | SessionEventPayload::TurnStarted { .. }
                | SessionEventPayload::TurnCompleted { .. }
                | SessionEventPayload::AgentSessionStarted { .. }
                | SessionEventPayload::ContextSnapshot { .. }
                | SessionEventPayload::DeliveryDiagnostic { .. }
                | SessionEventPayload::Diagnostic { .. } => None,
            },
            RunStreamEvent::AssistantTextDelta { text } => Some(Self::MessageDelta { text }),
            RunStreamEvent::ReasoningDelta { text } => Some(Self::ReasoningDelta { text }),
            RunStreamEvent::ReasoningEnd => Some(Self::ReasoningCompleted { text: None }),
            RunStreamEvent::ClarifyRequest(request) => Some(Self::InteractionRequested {
                interaction_id: request.call_id,
                kind: "clarify".to_string(),
                payload: serde_json::to_value(request.questions).unwrap_or(Value::Null),
            }),
            RunStreamEvent::ClarifyResolved(resolved) => Some(Self::InteractionResolved {
                interaction_id: resolved.call_id,
                kind: "clarify".to_string(),
                reason: format!("{:?}", resolved.reason).to_lowercase(),
            }),
            RunStreamEvent::Scoped { event, .. } => Self::from_run_stream(*event),
        }
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
