use super::*;

struct TurnTaskGuard {
    runtime: Arc<ApplicationRuntime>,
    receipt: TurnReceipt,
    interactions: FrameworkInteractionControl,
    interaction_broker: InteractionBroker,
    events: Arc<EventLog>,
    completion: Arc<TurnCompletion>,
    accepted: bool,
    pending_terminal: Option<PendingTerminal>,
    armed: bool,
}

impl TurnTaskGuard {
    fn new(
        runtime: Arc<ApplicationRuntime>,
        receipt: TurnReceipt,
        interactions: FrameworkInteractionControl,
        interaction_broker: InteractionBroker,
        events: Arc<EventLog>,
        completion: Arc<TurnCompletion>,
    ) -> Self {
        Self {
            runtime,
            receipt,
            interactions,
            interaction_broker,
            events,
            completion,
            accepted: false,
            pending_terminal: None,
            armed: true,
        }
    }

    fn mark_accepted(&mut self) {
        self.accepted = true;
    }

    fn stage_terminal(&mut self, terminal: PendingTerminal) {
        self.pending_terminal = Some(terminal);
    }

    async fn finalize_terminal(&mut self, state: &StateRuntime) {
        let terminal = self
            .pending_terminal
            .as_ref()
            .expect("Turn terminal must be staged before finalization")
            .clone();
        let finalization = terminal.persist(state).await;

        self.interactions.cancel_permissions();
        self.interaction_broker.finish();

        let pending_terminal = finalization.as_ref().err().map(|error| {
            let mut terminal = terminal.clone();
            terminal.last_error = error.to_string();
            terminal
        });
        self.runtime.settle_turn(
            &self.receipt.thread_id,
            &self.receipt.turn_id,
            pending_terminal,
        );

        let completion = match finalization {
            Ok(()) => {
                self.events.push(terminal.terminal_event.clone());
                terminal.completion.clone()
            }
            Err(error) => {
                let message: Arc<str> = Arc::from(format!(
                    "failed to persist Framework Turn terminal: {error}"
                ));
                self.events.push(TurnEvent::Warning {
                    data: serde_json::json!({
                        "kind": "framework_terminal_persistence",
                        "message": message.as_ref(),
                        "turnId": self.receipt.turn_id,
                    }),
                });
                Err(message)
            }
        };
        self.events.close();
        self.completion.settle(completion);
        self.pending_terminal = None;
        self.armed = false;
    }

    fn reject(&mut self, message: Arc<str>) {
        self.interactions.cancel_permissions();
        self.interaction_broker.finish();
        self.runtime
            .settle_turn(&self.receipt.thread_id, &self.receipt.turn_id, None);
        self.events.close();
        self.completion.settle(Err(message));
        self.armed = false;
    }
}

impl Drop for TurnTaskGuard {
    fn drop(&mut self) {
        if !self.armed || !std::thread::panicking() {
            return;
        }

        let message: Arc<str> = Arc::from("Framework Turn task panicked during finalization");
        let pending_terminal = self.accepted.then(|| {
            let mut terminal = self
                .pending_terminal
                .clone()
                .unwrap_or_else(|| PendingTerminal::failed(self.receipt.clone(), message.clone()));
            terminal.last_error = message.to_string();
            terminal
        });
        self.interactions.cancel_permissions();
        self.interaction_broker.finish();
        self.runtime.settle_turn(
            &self.receipt.thread_id,
            &self.receipt.turn_id,
            pending_terminal,
        );
        self.events.push(TurnEvent::Warning {
            data: serde_json::json!({
                "kind": "framework_turn_finalization_panic",
                "message": message.as_ref(),
                "turnId": self.receipt.turn_id,
            }),
        });
        self.events.close();
        self.completion.settle(Err(message));
    }
}

impl Client {
    pub async fn resume_turn(&self, id: impl Into<String>) -> Result<TurnHandle> {
        self.ensure_open()?;
        let id = id.into();
        if let Some(pending) = self.inner.runtime.pending_terminal(&id) {
            pending.persist(&self.inner.state).await.map_err(|error| {
                Error::TerminalPersistence {
                    turn_id: id.clone(),
                    message: error.to_string(),
                }
            })?;
            self.inner.runtime.remove_pending_terminal(&id);
            return Ok(pending.completed_handle());
        }
        if let Some(handle) = self.inner.runtime.turn_handle(&id) {
            return Ok(handle);
        }
        let Some(terminal) = self.inner.state.gateway_turn_terminal(&id).await? else {
            return if self.inner.state.gateway_turn_delivery(&id).await?.is_some() {
                Err(Error::OutcomeIndeterminate { turn_id: id })
            } else {
                Err(Error::Message(format!("turn not found: {id}")))
            };
        };
        let metadata = terminal.metadata.unwrap_or(Value::Null);
        let receipt = serde_json::from_value::<TurnReceipt>(
            metadata
                .get("frameworkReceipt")
                .cloned()
                .ok_or_else(|| Error::Message(format!("turn is not a Framework turn: {id}")))?,
        )?;
        match metadata.get("frameworkResult").cloned() {
            Some(result) if !result.is_null() => Ok(TurnHandle::completed(
                receipt,
                serde_json::from_value::<TurnResult>(result)?,
            )),
            _ if terminal.status == "failed" => Ok(TurnHandle::failed(
                receipt,
                terminal
                    .error_message
                    .unwrap_or_else(|| "Framework Turn failed".to_string()),
            )),
            _ => Err(Error::Message(format!(
                "Framework turn has no durable result: {id}"
            ))),
        }
    }
}

impl Thread {
    pub async fn start_turn(&self, mut request: TurnRequest) -> Result<TurnHandle> {
        let admission_guard = self.client.inner.runtime.begin_admission().await?;
        request.inherited_env = Some(
            self.client
                .application_environment(request.inherited_env.take()),
        );
        request.adapter_options.mcp_runtime = Some(self.client.inner.runtime.mcp_runtime(&self.id));
        let receipt = TurnReceipt {
            accepted: true,
            thread_id: self.id.clone(),
            turn_id: request
                .requested_turn_id
                .take()
                .unwrap_or_else(|| Uuid::now_v7().to_string()),
            client_turn_id: request.client_turn_id.clone(),
        };
        let durable_input = serde_json::to_string(&serde_json::json!({
            "prompt": request.prompt,
            "imageCount": request.image_inputs.len(),
            "clientTurnId": request.client_turn_id,
            "source": request.source,
            "model": request.model,
            "reasoningEffort": request.reasoning_effort,
            "runtimeRef": request.runtime_ref,
        }))?;
        let durable_input_hash = format!("{:x}", Sha256::digest(durable_input.as_bytes()));
        let runtime_ref = request
            .runtime_ref
            .as_deref()
            .unwrap_or("native")
            .to_string();
        let client_turn_id = request
            .client_turn_id
            .as_deref()
            .map(str::trim)
            .filter(|client_turn_id| !client_turn_id.is_empty())
            .map(ToOwned::to_owned);
        let event_observer = request.adapter_options.turn_event_observer.take();
        let events = Arc::new(EventLog::new(self.client.inner.event_capacity));
        let (control_handle, mut control) = request
            .prepared_control
            .take()
            .map(|prepared| (prepared.handle, prepared.control))
            .unwrap_or_else(run_control);
        control.agent_supervisor = self.client.inner.runtime.agent_supervisor.clone();
        let interactions = FrameworkInteractionControl::default();
        let completion = TurnCompletion::pending();
        let task_completion = completion.clone();
        let client = self.client.clone();
        let task_client = client.clone();
        let thread_id = self.id.clone();
        let turn_id = receipt.turn_id.clone();
        let task_receipt = receipt.clone();
        let task_events = Arc::clone(&events);
        let task_control_handle = control_handle.clone();
        let task_interactions = interactions.clone();
        let agent_sessions = Arc::clone(&client.inner.agent_sessions);
        let state = client.inner.state.clone();
        let interaction_broker = InteractionBroker::new(
            state.clone(),
            client.inner.runtime.clone(),
            Arc::clone(&events),
            interactions.clone(),
            control_handle.clone(),
            thread_id.clone(),
            turn_id.clone(),
        );
        let task_interaction_broker = interaction_broker.clone();
        let (acceptance_tx, acceptance_rx) = oneshot::channel();
        let handle = TurnHandle {
            receipt: receipt.clone(),
            events,
            completion,
            control: control_handle,
            interaction_broker: Some(interaction_broker),
        };
        let lane = client
            .inner
            .runtime
            .register_turn(&thread_id, &turn_id, handle.clone())?;

        if let Some(observer) = event_observer {
            let mut stream = handle.events();
            client.inner.runtime.spawn(async move {
                while let Some(event) = stream.next().await {
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observer(event)))
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }
        {
            let spawned_turn_id = turn_id.clone();
            let task = client.inner.runtime.spawn(async move {
                let mut finalizer = TurnTaskGuard::new(
                    task_client.inner.runtime.clone(),
                    task_receipt.clone(),
                    task_interactions.clone(),
                    task_interaction_broker.clone(),
                    Arc::clone(&task_events),
                    task_completion.clone(),
                );
                let acceptance = state
                    .accept_gateway_turn(
                        GatewayTurnDeliveryInput {
                            turn_id: &turn_id,
                            thread_id: &thread_id,
                            runtime_ref: &runtime_ref,
                            input_json: &durable_input,
                            input_hash: &durable_input_hash,
                        },
                        client_turn_id.as_deref(),
                    )
                    .await;
                if let Err(error) = acceptance {
                    let message: Arc<str> = Arc::from(error.to_string());
                    finalizer.reject(message);
                    let _ = acceptance_tx.send(Err(error));
                    return;
                }
                finalizer.mark_accepted();
                task_events.push(TurnEvent::Accepted {
                    receipt: task_receipt.clone(),
                });
                let _ = acceptance_tx.send(Ok(()));
                drop(admission_guard);
                if lane.await.is_err() {
                    let message: Arc<str> = Arc::from("Thread operation reservation was cancelled");
                    finalizer
                        .stage_terminal(PendingTerminal::failed(task_receipt.clone(), message));
                    finalizer.finalize_terminal(&state).await;
                    return;
                }
                task_events.push(TurnEvent::Started {
                    thread_id: thread_id.clone(),
                    turn_id: turn_id.clone(),
                });
                let result = std::panic::AssertUnwindSafe(async {
                    let summary = state
                        .session_summary(&thread_id)
                        .await?
                        .ok_or_else(|| Error::Message(format!("thread not found: {thread_id}")))?;
                    let thread = ThreadExecutionContext::from_summary(summary);
                    let history = HistoryReader::new(state.clone(), thread_id.clone());
                    let event_sender = TurnEventSender {
                        log: Arc::clone(&task_events),
                        interactions: task_interaction_broker.clone(),
                    };
                    request.approval_handler = Some(Arc::new(FrameworkApprovalHandler {
                        delegate: request.approval_handler.take(),
                        interactions: task_interactions.clone(),
                        broker: task_interaction_broker.clone(),
                    }));
                    agent_sessions
                        .run_turn(AgentTurnRequest {
                            thread,
                            history,
                            receipt: task_receipt.clone(),
                            input: request,
                            events: event_sender,
                            control: TurnControl {
                                handle: task_control_handle,
                                interactions: task_interaction_broker.clone(),
                            },
                            native_control: Some(control),
                        })
                        .await
                })
                .catch_unwind()
                .await
                .unwrap_or_else(|_| {
                    Err(Error::Message(
                        "Agent Session Adapter panicked while running the Turn".to_string(),
                    ))
                });
                let (shared, terminal_event) = match result {
                    Ok(result) => {
                        let event = TurnEvent::Completed {
                            thread_id: thread_id.clone(),
                            turn_id: turn_id.clone(),
                            outcome: result.outcome,
                        };
                        (Ok(Arc::new(result)), event)
                    }
                    Err(error) => {
                        let message: Arc<str> = Arc::from(error.to_string());
                        let event = TurnEvent::Failed {
                            thread_id: thread_id.clone(),
                            turn_id: turn_id.clone(),
                            message: message.to_string(),
                        };
                        (Err(message), event)
                    }
                };
                let completed_at_ms = psychevo_agent_core::now_ms();
                let terminal = PendingTerminal {
                    receipt: task_receipt.clone(),
                    completion: shared.clone(),
                    terminal_event: terminal_event.clone(),
                    completed_at_ms,
                    last_error: String::new(),
                };
                finalizer.stage_terminal(terminal);
                finalizer.finalize_terminal(&state).await;
            });
            client.inner.runtime.set_turn_abort(&spawned_turn_id, task);
        }

        acceptance_rx.await.map_err(|_| {
            Error::Message("accepted Turn admission task ended without a receipt".to_string())
        })??;
        Ok(handle)
    }
}

#[cfg(test)]
mod tests {
    use std::panic::AssertUnwindSafe;
    use std::time::Duration;

    use futures::FutureExt;
    use tempfile::tempdir;

    use super::*;

    async fn guard_fixture() -> (
        Application,
        Arc<ApplicationRuntime>,
        TurnReceipt,
        FrameworkInteractionControl,
        InteractionBroker,
        Arc<EventLog>,
        Arc<TurnCompletion>,
        TurnHandle,
    ) {
        let home = tempdir().expect("tempdir").keep();
        let application = Application::builder()
            .home(&home)
            .database_path(":memory:")
            .build()
            .await
            .expect("application");
        let runtime = application.inner.runtime.clone();
        let receipt = TurnReceipt {
            accepted: true,
            thread_id: "guard-thread".to_string(),
            turn_id: Uuid::now_v7().to_string(),
            client_turn_id: None,
        };
        let events = Arc::new(EventLog::new(8));
        let completion = TurnCompletion::pending();
        let interactions = FrameworkInteractionControl::default();
        let (control, _) = run_control();
        let broker = InteractionBroker::new(
            application.inner.state.clone(),
            runtime.clone(),
            events.clone(),
            interactions.clone(),
            control.clone(),
            receipt.thread_id.clone(),
            receipt.turn_id.clone(),
        );
        let handle = TurnHandle {
            receipt: receipt.clone(),
            events: events.clone(),
            completion: completion.clone(),
            control,
            interaction_broker: Some(broker.clone()),
        };
        runtime
            .register_turn(&receipt.thread_id, &receipt.turn_id, handle.clone())
            .expect("register turn");
        (
            application,
            runtime,
            receipt,
            interactions,
            broker,
            events,
            completion,
            handle,
        )
    }

    #[tokio::test]
    async fn finalization_panic_retains_the_staged_terminal_and_releases_waiters() {
        let (application, runtime, receipt, interactions, broker, events, completion, handle) =
            guard_fixture().await;
        let staged_message: Arc<str> = Arc::from("staged terminal");
        let staged = PendingTerminal::failed(receipt.clone(), staged_message.clone());
        let result = AssertUnwindSafe(async {
            let mut guard = TurnTaskGuard::new(
                runtime.clone(),
                receipt.clone(),
                interactions,
                broker.clone(),
                events.clone(),
                completion.clone(),
            );
            guard.mark_accepted();
            guard.stage_terminal(staged);
            panic!("injected finalization panic");
        })
        .catch_unwind()
        .await;
        assert!(result.is_err());

        assert!(!runtime.thread_activity(&receipt.thread_id).0);
        let pending = runtime
            .pending_terminal(&receipt.turn_id)
            .expect("staged terminal retained");
        assert_eq!(
            pending.completion.expect_err("failed terminal"),
            staged_message
        );
        let waiter = tokio::time::timeout(Duration::from_secs(1), handle.wait())
            .await
            .expect("completion waiter released")
            .expect_err("panic is a lifecycle failure");
        assert!(waiter.to_string().contains("panicked during finalization"));
        let mut cursor = 0;
        assert!(matches!(
            events.next(&mut cursor).await,
            Some(TurnEvent::Warning { data })
                if data["kind"] == "framework_turn_finalization_panic"
        ));
        assert_eq!(events.next(&mut cursor).await, None);

        runtime.take_turn_slots();
        broker.finish();
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn non_panicking_guard_drop_leaves_the_active_slot_for_forced_shutdown() {
        let (application, runtime, receipt, interactions, broker, events, completion, _) =
            guard_fixture().await;
        {
            let mut guard = TurnTaskGuard::new(
                runtime.clone(),
                receipt.clone(),
                interactions,
                broker.clone(),
                events,
                completion,
            );
            guard.mark_accepted();
        }

        assert!(runtime.thread_activity(&receipt.thread_id).0);
        assert!(runtime.pending_terminal(&receipt.turn_id).is_none());

        runtime.take_turn_slots();
        broker.finish();
        application.shutdown().await.expect("shutdown");
    }
}
