use super::*;

impl PendingTerminal {
    pub(super) fn failed(receipt: TurnReceipt, message: Arc<str>) -> Self {
        Self {
            terminal_event: TurnEvent::Failed {
                thread_id: receipt.thread_id.clone(),
                turn_id: receipt.turn_id.clone(),
                message: message.to_string(),
            },
            receipt,
            completion: Err(message),
            completed_at_ms: psychevo_agent_core::now_ms(),
            last_error: String::new(),
        }
    }

    pub(super) fn interrupted(receipt: TurnReceipt) -> Self {
        let result = TurnResult {
            thread_id: receipt.thread_id.clone(),
            outcome: TurnOutcome::Interrupted,
            final_answer: String::new(),
            provider: "application".to_string(),
            model: "forced-shutdown".to_string(),
            reasoning_effort: None,
            tool_failures: 0,
            context_limit: None,
            context_snapshot: None,
            warnings: Vec::new(),
            terminal_reason: None,
            terminal_error: None,
            selected_agent: None,
            selected_skills: Vec::new(),
        };
        Self {
            terminal_event: TurnEvent::Completed {
                thread_id: receipt.thread_id.clone(),
                turn_id: receipt.turn_id.clone(),
                outcome: TurnOutcome::Interrupted,
            },
            receipt,
            completion: Ok(Arc::new(result)),
            completed_at_ms: psychevo_agent_core::now_ms(),
            last_error: String::new(),
        }
    }

    pub(super) async fn persist(&self, state: &StateRuntime) -> Result<()> {
        match &self.completion {
            Ok(result) => {
                let framework_result = serde_json::to_value(result.as_ref())?;
                let (status, outcome) = gateway_terminal_facts(result.outcome);
                state
                    .finalize_framework_turn(
                        GatewayTurnTerminalInput {
                            turn_id: &self.receipt.turn_id,
                            thread_id: &self.receipt.thread_id,
                            status,
                            outcome: Some(outcome),
                            error_message: None,
                            started_at_ms: None,
                            completed_at_ms: self.completed_at_ms,
                            metadata: Some(serde_json::json!({
                                "source": "framework",
                                "frameworkReceipt": self.receipt,
                                "frameworkResult": framework_result,
                            })),
                        },
                        "turn_finished",
                    )
                    .await
            }
            Err(message) => {
                state
                    .finalize_framework_turn(
                        GatewayTurnTerminalInput {
                            turn_id: &self.receipt.turn_id,
                            thread_id: &self.receipt.thread_id,
                            status: "failed",
                            outcome: Some("failed"),
                            error_message: Some(message.as_ref()),
                            started_at_ms: None,
                            completed_at_ms: self.completed_at_ms,
                            metadata: Some(serde_json::json!({
                                "source": "framework",
                                "frameworkReceipt": self.receipt,
                                "frameworkResult": Value::Null,
                            })),
                        },
                        "turn_finished",
                    )
                    .await
            }
        }
    }

    pub(super) fn completed_handle(&self) -> TurnHandle {
        match &self.completion {
            Ok(result) => TurnHandle::completed(self.receipt.clone(), result.as_ref().clone()),
            Err(message) => TurnHandle::failed(self.receipt.clone(), message.to_string()),
        }
    }
}

impl TurnCompletion {
    pub(super) fn pending() -> Arc<Self> {
        Arc::new(Self {
            value: Mutex::new(None),
            notify: Notify::new(),
        })
    }

    fn ready(value: SharedTurnCompletion) -> Arc<Self> {
        Arc::new(Self {
            value: Mutex::new(Some(value)),
            notify: Notify::new(),
        })
    }

    pub(super) fn settle(&self, value: SharedTurnCompletion) -> bool {
        let mut current = self
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if current.is_some() {
            return false;
        }
        *current = Some(value);
        drop(current);
        self.notify.notify_waiters();
        true
    }

    async fn wait(&self) -> SharedTurnCompletion {
        loop {
            let notified = self.notify.notified();
            if let Some(value) = self
                .value
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
            {
                return value;
            }
            notified.await;
        }
    }
}

impl fmt::Debug for TurnHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TurnHandle")
            .field("receipt", &self.receipt)
            .finish_non_exhaustive()
    }
}

impl TurnHandle {
    pub(super) fn completed(receipt: TurnReceipt, result: TurnResult) -> Self {
        let events = Arc::new(EventLog::new(DEFAULT_EVENT_CAPACITY));
        events.push(TurnEvent::Accepted {
            receipt: receipt.clone(),
        });
        events.push(TurnEvent::Completed {
            thread_id: receipt.thread_id.clone(),
            turn_id: receipt.turn_id.clone(),
            outcome: result.outcome,
        });
        events.close();
        let result = Arc::new(result);
        let completion = TurnCompletion::ready(Ok(result));
        let (control, _) = run_control();
        Self {
            receipt,
            events,
            completion,
            control,
            interaction_broker: None,
        }
    }

    pub(super) fn failed(receipt: TurnReceipt, message: String) -> Self {
        let events = Arc::new(EventLog::new(DEFAULT_EVENT_CAPACITY));
        events.push(TurnEvent::Accepted {
            receipt: receipt.clone(),
        });
        events.push(TurnEvent::Failed {
            thread_id: receipt.thread_id.clone(),
            turn_id: receipt.turn_id.clone(),
            message: message.clone(),
        });
        events.close();
        let completion = TurnCompletion::ready(Err(Arc::from(message)));
        let (control, _) = run_control();
        Self {
            receipt,
            events,
            completion,
            control,
            interaction_broker: None,
        }
    }

    pub fn receipt(&self) -> &TurnReceipt {
        &self.receipt
    }

    pub fn events(&self) -> TurnEventStream {
        TurnEventStream {
            log: Arc::clone(&self.events),
            cursor: 0,
        }
    }

    pub async fn wait(&self) -> Result<TurnResult> {
        match self.completion.wait().await {
            Ok(result) => Ok((*result).clone()),
            Err(message) => Err(Error::Message(message.to_string())),
        }
    }

    pub fn steer(
        &self,
        input: impl Into<String>,
    ) -> std::result::Result<(), psychevo_agent_core::ControlInputError> {
        self.__steer(input).map(|_| ())
    }

    #[doc(hidden)]
    pub fn __steer(
        &self,
        input: impl Into<String>,
    ) -> std::result::Result<
        psychevo_agent_core::PendingInputId,
        psychevo_agent_core::ControlInputError,
    > {
        self.control
            .steer_user_message(psychevo_agent_core::user_text_message(input))
    }

    #[doc(hidden)]
    pub fn __cancel_steer(&self, id: psychevo_agent_core::PendingInputId) -> bool {
        self.control.cancel_pending_user_message(id)
    }

    pub fn interrupt(&self) {
        self.control.abort();
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __control_handle(&self) -> crate::types::RunControlHandle {
        self.control.clone()
    }

    pub async fn respond(
        &self,
        interaction_id: &str,
        response: InteractionResponse,
    ) -> Result<InteractionResponseReceipt> {
        match self.interaction_broker.as_ref() {
            Some(broker) => broker.respond(interaction_id, response).await,
            None => Ok(InteractionResponseReceipt { accepted: false }),
        }
    }
}

impl From<crate::types::RunResult> for TurnResult {
    fn from(result: crate::types::RunResult) -> Self {
        let outcome = match result.outcome {
            psychevo_ai::Outcome::Normal => TurnOutcome::Completed,
            psychevo_ai::Outcome::Stopped => TurnOutcome::Stopped,
            psychevo_ai::Outcome::Failed => TurnOutcome::Failed,
            psychevo_ai::Outcome::Aborted => TurnOutcome::Interrupted,
        };
        Self {
            thread_id: result.session_id,
            outcome,
            final_answer: result.final_answer,
            provider: result.provider,
            model: result.model,
            reasoning_effort: result.reasoning_effort,
            tool_failures: result.tool_failures,
            context_limit: result.context_limit,
            context_snapshot: result.context_snapshot,
            warnings: result.warnings,
            terminal_reason: result.terminal_reason,
            terminal_error: result.terminal_error,
            selected_agent: result.selected_agent,
            selected_skills: result.selected_skills,
        }
    }
}
