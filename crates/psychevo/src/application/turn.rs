use super::thread::NewThreadAdmission;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;
use uuid::Uuid;

use super::event_log::EventLog;
use super::interaction_broker::{
    FrameworkApprovalHandler, FrameworkInteractionControl, InteractionBroker,
};
use super::runtime::ApplicationRuntime;
use super::{
    AgentBindingSnapshot, AgentCapabilitySelection, AgentChildTurnDispatcher, AgentTurnInvocation,
    AgentTurnPreparation, AgentTurnPurpose, Client, FrameworkAgentTurnPersistence,
    FrameworkTurnTerminalEvidence, FrameworkTurnTerminalStatus, HistoryReader, PendingTerminal,
    ResolvedTurnPlan, Thread, ThreadExecutionContext, TurnAdmissionCancellation, TurnCompletion,
    TurnControl, TurnEvent, TurnEventSender, TurnHandle, TurnReceipt, TurnRequest, TurnResult,
};
#[cfg(test)]
use crate::state::GatewayTurnTerminalInput;
use crate::state::{
    ExistingFrameworkThreadTurnInput, GatewayRuntimeBindingInput, GatewayRuntimeBindingOwnership,
    GatewayTurnDeliveryInput, StateRuntime,
};
use crate::types::run_control;
use crate::{Error, Result};

#[cfg(test)]
use super::{
    AgentSessionAdapter, Application, FrameworkTurnTerminalOutcome, PreparedAgentTurn,
    StartThreadRequest, ThreadActivitySnapshot,
};

struct TurnTaskGuard {
    runtime: Arc<ApplicationRuntime>,
    receipt: TurnReceipt,
    interactions: FrameworkInteractionControl,
    interaction_broker: InteractionBroker,
    events: Arc<EventLog>,
    completion: Arc<TurnCompletion>,
    accepted: bool,
    pending_terminal: Option<PendingTerminal>,
    boundary_session_seq: Arc<AtomicI64>,
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
            boundary_session_seq: Arc::new(AtomicI64::new(-1)),
            armed: true,
        }
    }

    fn mark_accepted(&mut self) {
        self.accepted = true;
    }

    fn stage_terminal(&mut self, mut terminal: PendingTerminal) {
        let boundary = self.boundary_session_seq.load(Ordering::Acquire);
        if boundary >= 0 {
            terminal.boundary_session_seq = Some(boundary);
        }
        self.pending_terminal = Some(terminal);
    }

    fn set_boundary_session_seq(&self, boundary_session_seq: i64) {
        self.boundary_session_seq
            .store(boundary_session_seq, Ordering::Release);
    }

    async fn finalize_terminal(&mut self, state: &StateRuntime) {
        let mut terminal = self
            .pending_terminal
            .take()
            .expect("Turn terminal must be staged before finalization");
        let finalization = terminal.persist(state).await;

        self.interactions.cancel_permissions();
        self.interaction_broker.finish();

        let pending_terminal = finalization.as_ref().err().map(|error| {
            let mut terminal = terminal.clone();
            terminal.last_error = error.to_string();
            terminal
        });
        let activity = self.runtime.settle_turn(
            &self.receipt.thread_id,
            &self.receipt.turn_id,
            pending_terminal,
        );

        let completion = match finalization {
            Ok(()) => {
                if let Some(activity) = activity {
                    self.events.push(TurnEvent::ActivityChanged {
                        thread_id: self.receipt.thread_id.clone(),
                        activity,
                    });
                }
                self.events.push(terminal.terminal_event.clone());
                terminal.completion.clone()
            }
            Err(error) => {
                if let Some(activity) = activity {
                    self.events.push(TurnEvent::ActivityChanged {
                        thread_id: self.receipt.thread_id.clone(),
                        activity,
                    });
                }
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
        self.armed = false;
    }

    fn reject(&mut self, message: Arc<str>) {
        self.interactions.cancel_permissions();
        self.interaction_broker.finish();
        if let Some(activity) =
            self.runtime
                .settle_turn(&self.receipt.thread_id, &self.receipt.turn_id, None)
        {
            self.events.push(TurnEvent::ActivityChanged {
                thread_id: self.receipt.thread_id.clone(),
                activity,
            });
        }
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

        let message: Arc<str> = Arc::from("Framework Turn actor panicked");
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
        let activity = self.runtime.settle_turn(
            &self.receipt.thread_id,
            &self.receipt.turn_id,
            pending_terminal,
        );
        if let Some(activity) = activity {
            self.events.push(TurnEvent::ActivityChanged {
                thread_id: self.receipt.thread_id.clone(),
                activity,
            });
        }
        self.events.push(TurnEvent::Warning {
            data: serde_json::json!({
                "kind": "framework_turn_actor_panic",
                "message": message.as_ref(),
                "turnId": self.receipt.turn_id,
            }),
        });
        self.events.close();
        self.completion.settle(Err(message));
    }
}

impl Client {
    /// Read only the durable facts needed to fence a retained-live Framework terminal.
    pub async fn framework_turn_terminal_evidence(
        &self,
        turn_id: &str,
    ) -> Result<Option<FrameworkTurnTerminalEvidence>> {
        self.ensure_open()?;
        let Some(terminal) = self.inner.state.gateway_turn_terminal(turn_id).await? else {
            return Ok(None);
        };
        let outcome = match terminal.outcome {
            Some(outcome) => outcome,
            None => {
                return Err(Error::Message(format!(
                    "Framework Turn terminal `{turn_id}` has no durable outcome"
                )));
            }
        };
        Ok(Some(FrameworkTurnTerminalEvidence {
            turn_id: terminal.turn_id,
            thread_id: terminal.thread_id,
            status: terminal.status,
            outcome,
            completed_at_ms: terminal.completed_at_ms,
            boundary_session_seq: terminal.boundary_session_seq,
        }))
    }

    pub async fn resume_turn(&self, id: impl Into<String>) -> Result<TurnHandle> {
        self.ensure_open()?;
        let id = id.into();
        if let Some(mut pending) = self.inner.runtime.pending_terminal(&id) {
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
            _ if terminal.status == FrameworkTurnTerminalStatus::Failed => Ok(TurnHandle::failed(
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

impl AgentChildTurnDispatcher {
    pub(super) fn start_child_turn(
        &self,
        parent_thread_id: impl Into<String>,
        thread_id: impl Into<String>,
        plan: ResolvedTurnPlan,
    ) -> BoxFuture<'static, Result<TurnHandle>> {
        let parent_thread_id = parent_thread_id.into();
        let thread_id = thread_id.into();
        let inner = self.inner.clone();
        let approval_handler = self.approval_handler.clone();
        Box::pin(async move {
            let inner = inner.upgrade().ok_or_else(|| {
                Error::Message("Psychevo Application is shutting down".to_string())
            })?;
            let client = Client { inner };
            let child = client
                .inner
                .state
                .session_summary(&thread_id)
                .await?
                .ok_or_else(|| Error::Message(format!("thread not found: {thread_id}")))?;
            if child.parent_session_id.as_deref() != Some(parent_thread_id.as_str()) {
                return Err(Error::Message(format!(
                    "Runtime-backed child `{thread_id}` is not owned by parent `{parent_thread_id}`"
                )));
            }
            let thread = client.resume_thread(thread_id).await?;
            let mut plan = plan;
            plan.execution.approval_handler = approval_handler;
            thread
                .start_resolved_turn_inner(plan, None, AgentTurnPurpose::Child)
                .await
        })
    }

    pub(super) fn close_child_relationship(
        &self,
        thread_id: impl Into<String>,
    ) -> BoxFuture<'static, Result<()>> {
        let thread_id = thread_id.into();
        let inner = self.inner.clone();
        Box::pin(async move {
            let inner = inner.upgrade().ok_or_else(|| {
                Error::Message("Psychevo Application is shutting down".to_string())
            })?;
            inner
                .state
                .set_agent_edge_status(&thread_id, crate::state::AgentEdgeStatus::Closed)
                .await
        })
    }
}

async fn await_turn_acceptance<F>(
    mut acceptance_rx: oneshot::Receiver<Result<()>>,
    cancellation: Option<TurnAdmissionCancellation>,
    interrupt: F,
) -> std::result::Result<Result<()>, oneshot::error::RecvError>
where
    F: Fn(),
{
    let Some(cancellation) = cancellation else {
        return acceptance_rx.await;
    };
    let mut interrupted = false;
    let acceptance = tokio::select! {
        biased;
        acceptance = &mut acceptance_rx => acceptance,
        _ = cancellation.cancelled() => {
            interrupt();
            interrupted = true;
            acceptance_rx.await
        }
    };
    if !interrupted && cancellation.is_cancelled() {
        interrupt();
    }
    acceptance
}

impl Thread {
    pub async fn start_turn(&self, request: TurnRequest) -> Result<TurnHandle> {
        self.start_turn_inner(request, None, AgentTurnPurpose::Peer)
            .await
    }

    pub async fn start_child_turn(
        &self,
        child_thread_id: impl Into<String>,
        request: TurnRequest,
    ) -> Result<TurnHandle> {
        let child_thread_id = child_thread_id.into();
        let child = self
            .client
            .inner
            .state
            .session_summary(&child_thread_id)
            .await?
            .ok_or_else(|| Error::Message(format!("thread not found: {child_thread_id}")))?;
        if child.parent_session_id.as_deref() != Some(self.id.as_str()) {
            return Err(Error::Message(format!(
                "child Thread `{child_thread_id}` is not owned by parent `{}`",
                self.id
            )));
        }
        let thread = self.client.resume_thread(child_thread_id.clone()).await?;
        match thread
            .start_turn_inner(request, None, AgentTurnPurpose::Child)
            .await
        {
            Ok(handle) => Ok(handle),
            Err(error) => {
                let _ = self
                    .client
                    .inner
                    .state
                    .set_agent_edge_status(&child_thread_id, crate::state::AgentEdgeStatus::Closed)
                    .await;
                Err(error)
            }
        }
    }

    pub(super) async fn start_turn_inner(
        &self,
        mut request: TurnRequest,
        new_thread: Option<NewThreadAdmission>,
        purpose: AgentTurnPurpose,
    ) -> Result<TurnHandle> {
        let inherited_env = self
            .client
            .application_environment(request.inherited_env.take());
        let plan = request.resolve(inherited_env, self.client.inner.config_path.clone());
        self.start_resolved_turn_inner(plan, new_thread, purpose)
            .await
    }

    async fn start_resolved_turn_inner(
        &self,
        mut plan: ResolvedTurnPlan,
        new_thread: Option<NewThreadAdmission>,
        purpose: AgentTurnPurpose,
    ) -> Result<TurnHandle> {
        let admission_cancellation = plan.admission_cancellation.take();
        let admission_guard = if let Some(cancellation) = admission_cancellation.as_ref() {
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => {
                    return Err(Error::Message(
                        "Turn admission cancelled before acceptance".to_string(),
                    ));
                }
                guard = self.client.inner.runtime.begin_admission() => guard?,
            }
        } else {
            self.client.inner.runtime.begin_admission().await?
        };
        let client_turn_id = match plan.client_turn_id.as_deref() {
            Some(client_turn_id) if client_turn_id.trim().is_empty() => {
                return Err(Error::Message(
                    "client Turn id must contain a non-whitespace character".to_string(),
                ));
            }
            Some(client_turn_id) => Some(client_turn_id.to_string()),
            None => None,
        };
        let receipt = TurnReceipt {
            accepted: true,
            thread_id: self.id.clone(),
            turn_id: plan
                .requested_turn_id
                .take()
                .unwrap_or_else(|| Uuid::now_v7().to_string()),
            client_turn_id: client_turn_id.clone(),
        };
        let requested_runtime_ref = plan.target.runtime_profile_ref.clone();
        let events = Arc::new(EventLog::new(self.client.inner.event_capacity));
        let (control_handle, mut control) = run_control();
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
        let thread_context = match new_thread.as_ref() {
            Some(new_thread) => new_thread.execution_context(&thread_id),
            None => ThreadExecutionContext::from_summary(
                state
                    .session_summary(&thread_id)
                    .await?
                    .ok_or_else(|| Error::Message(format!("thread not found: {thread_id}")))?,
            ),
        };
        let binding_cwd = thread_context.cwd.clone();
        let binding = state
            .gateway_runtime_binding(&thread_id)
            .await?
            .map(AgentBindingSnapshot::try_from)
            .transpose()?;
        let binding_exists = binding.is_some();
        let existing_runtime_ref = binding.as_ref().map(|binding| binding.runtime_ref.clone());
        if let (Some(requested_runtime_ref), Some(existing_runtime_ref)) = (
            requested_runtime_ref.as_deref(),
            existing_runtime_ref.as_deref(),
        ) && requested_runtime_ref != existing_runtime_ref
        {
            return Err(Error::Message(format!(
                "runtime target `{requested_runtime_ref}` conflicts with the immutable binding runtime `{existing_runtime_ref}`"
            )));
        }
        let preparation = Arc::clone(&agent_sessions).prepare_turn(AgentTurnPreparation {
            thread: thread_context,
            binding,
            target: plan.target.clone(),
            inherited_env: plan.environment.inherited_env.clone(),
            purpose,
            native_backend: client.inner.native_backend.clone(),
        });
        let prepared = if let Some(cancellation) = admission_cancellation.as_ref() {
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => {
                    return Err(Error::Message(
                        "Turn admission cancelled before acceptance".to_string(),
                    ));
                }
                prepared = preparation => prepared?,
            }
        } else {
            preparation.await?
        };
        if admission_cancellation
            .as_ref()
            .is_some_and(TurnAdmissionCancellation::is_cancelled)
        {
            return Err(Error::Message(
                "Turn admission cancelled before acceptance".to_string(),
            ));
        }
        let admission_facts = prepared.admission();
        let runtime_ref = existing_runtime_ref
            .as_deref()
            .or_else(|| {
                admission_facts
                    .initial_binding
                    .as_ref()
                    .map(|binding| binding.runtime_ref.as_str())
            })
            .or(requested_runtime_ref.as_deref())
            .unwrap_or("native")
            .to_string();
        if let Some(requested_runtime_ref) = requested_runtime_ref.as_deref()
            && requested_runtime_ref != runtime_ref
        {
            return Err(Error::Message(format!(
                "runtime target `{requested_runtime_ref}` conflicts with the immutable binding runtime `{runtime_ref}`"
            )));
        }
        let durable_input = serde_json::to_string(&serde_json::json!({
            "prompt": plan.input.prompt,
            "imageCount": plan.input.image_inputs.len(),
            "clientTurnId": plan.client_turn_id,
            "source": plan.execution.source,
            "model": plan.model.model,
            "reasoningEffort": plan.model.reasoning_effort,
            "runtimeRef": runtime_ref,
        }))?;
        let durable_input_hash = format!("{:x}", Sha256::digest(durable_input.as_bytes()));
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
        let (lane, queue_position) =
            client
                .inner
                .runtime
                .register_turn(&thread_id, &turn_id, handle.clone())?;

        {
            let spawned_turn_id = turn_id.clone();
            let actor = format!("framework_turn:{spawned_turn_id}");
            let task = client.inner.runtime.spawn_named(actor, async move {
                let mut finalizer = TurnTaskGuard::new(
                    task_client.inner.runtime.clone(),
                    task_receipt.clone(),
                    task_interactions.clone(),
                    task_interaction_broker.clone(),
                    Arc::clone(&task_events),
                    task_completion.clone(),
                );
                let delivery = GatewayTurnDeliveryInput {
                    turn_id: &turn_id,
                    thread_id: &thread_id,
                    runtime_ref: &runtime_ref,
                    input_json: &durable_input,
                    input_hash: &durable_input_hash,
                };
                let admission_mission = plan.admission_mission.take();
                let acceptance = match new_thread {
                    Some(new_thread) => {
                        new_thread
                            .accept(
                                &state,
                                delivery,
                                client_turn_id.as_deref(),
                                admission_facts.initial_binding,
                                &plan.initial_thread_preferences,
                                admission_mission,
                            )
                            .await
                    }
                    None => {
                        let initial_binding = if binding_exists {
                            None
                        } else {
                            admission_facts.initial_binding.as_ref()
                        };
                        let initial_thread_preferences = if initial_binding.is_some() {
                            plan.initial_thread_preferences
                                .iter()
                                .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                                .collect::<BTreeMap<_, _>>()
                        } else {
                            BTreeMap::new()
                        };
                        let runtime_binding =
                            initial_binding.map(|binding| GatewayRuntimeBindingInput {
                                thread_id: &thread_id,
                                agent_ref: binding.agent_ref.as_deref(),
                                agent_fingerprint: &binding.agent_fingerprint,
                                agent_definition_json: &binding.agent_definition_json,
                                runtime_ref: &binding.runtime_ref,
                                backend_kind: &binding.backend_kind,
                                native_kind: &binding.native_kind,
                                native_session_id: binding.native_session_id.as_deref(),
                                cwd: &binding_cwd,
                                profile_fingerprint: &binding.profile_fingerprint,
                                profile_revision: &binding.profile_revision,
                                profile_config_json: &binding.profile_config_json,
                                adapter_kind: &binding.adapter_kind,
                                adapter_revision: &binding.adapter_revision,
                                ownership: GatewayRuntimeBindingOwnership::ReadWrite,
                                parent_thread_id: None,
                            });
                        state
                            .accept_framework_turn(ExistingFrameworkThreadTurnInput {
                                delivery,
                                client_turn_id: client_turn_id.as_deref(),
                                runtime_binding,
                                initial_thread_preferences: &initial_thread_preferences,
                                mission: admission_mission,
                            })
                            .await
                    }
                };
                if let Err(error) = acceptance {
                    let message: Arc<str> = Arc::from(error.to_string());
                    finalizer.reject(message);
                    let _ = acceptance_tx.send(Err(error));
                    return;
                }
                let activity = match task_client
                    .inner
                    .runtime
                    .mark_turn_accepted(&thread_id, &turn_id)
                {
                    Ok(activity) => activity,
                    Err(error) => {
                        let message: Arc<str> = Arc::from(error.to_string());
                        finalizer.reject(message);
                        let _ = acceptance_tx.send(Err(error));
                        return;
                    }
                };
                finalizer.mark_accepted();
                task_events.push(TurnEvent::Accepted {
                    receipt: task_receipt.clone(),
                    queue_position: (queue_position > 0).then_some(queue_position),
                });
                task_events.push(TurnEvent::ActivityChanged {
                    thread_id: thread_id.clone(),
                    activity,
                });
                let _ = acceptance_tx.send(Ok(()));
                drop(admission_guard);
                if lane.await.is_err() {
                    let message: Arc<str> = Arc::from("Thread operation reservation was cancelled");
                    finalizer
                        .stage_terminal(PendingTerminal::failed(task_receipt.clone(), message));
                    if purpose == AgentTurnPurpose::Child {
                        let _ = state
                            .set_agent_edge_status(
                                &thread_id,
                                crate::state::AgentEdgeStatus::Closed,
                            )
                            .await;
                    }
                    finalizer.finalize_terminal(&state).await;
                    return;
                }
                let boundary_session_seq = match state.latest_message_session_seq(&thread_id).await
                {
                    Ok(boundary_session_seq) => boundary_session_seq,
                    Err(error) => {
                        let message: Arc<str> = Arc::from(error.to_string());
                        finalizer
                            .stage_terminal(PendingTerminal::failed(task_receipt.clone(), message));
                        finalizer.finalize_terminal(&state).await;
                        return;
                    }
                };
                finalizer.set_boundary_session_seq(boundary_session_seq);
                task_events.push(TurnEvent::Started {
                    thread_id: thread_id.clone(),
                    turn_id: turn_id.clone(),
                });
                let result = async {
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
                    let child_approval_handler = plan.execution.approval_handler.clone();
                    plan.execution.approval_handler = Some(Arc::new(FrameworkApprovalHandler {
                        delegate: plan.execution.approval_handler.take(),
                        interactions: task_interactions.clone(),
                        broker: task_interaction_broker.clone(),
                    }));
                    let mcp_resolver = super::agent_session::AgentMcpServerResolver::for_turn(
                        &thread,
                        task_client.inner.home.clone(),
                        Arc::clone(&task_client.inner.mcp_oauth_credentials),
                        plan.execution.config_path.clone(),
                        plan.environment.inherited_env.clone(),
                        plan.capabilities.selected_capability_roots.clone(),
                        plan.capabilities.mcp_servers.clone(),
                    );
                    let capabilities = AgentCapabilitySelection {
                        no_agents: plan.capabilities.no_agents,
                        no_skills: plan.capabilities.no_skills,
                        selected_capability_roots: plan.capabilities.selected_capability_roots,
                        skill_inputs: plan.capabilities.skill_inputs,
                        mcp_servers: plan.capabilities.mcp_servers,
                        tools: plan.capabilities.tools,
                        mcp_runtime: task_client.inner.runtime.mcp_runtime(&thread_id),
                    };
                    let binding = state
                        .gateway_runtime_binding(&thread_id)
                        .await?
                        .map(AgentBindingSnapshot::try_from)
                        .transpose()?;
                    prepared
                        .invoke(AgentTurnInvocation {
                            thread,
                            history,
                            receipt: task_receipt.clone(),
                            binding,
                            target: plan.target,
                            input: plan.input,
                            model: plan.model,
                            execution: plan.execution,
                            capabilities,
                            environment: plan.environment,
                            persistence: Arc::new(FrameworkAgentTurnPersistence {
                                state: state.clone(),
                                thread_id: thread_id.clone(),
                                turn_id: turn_id.clone(),
                                boundary_session_seq: Arc::clone(&finalizer.boundary_session_seq),
                            }),
                            events: event_sender.clone(),
                            control: TurnControl {
                                handle: task_control_handle,
                                abort: control.abort_signal(),
                                interactions: task_interaction_broker.clone(),
                                events: event_sender,
                                runtime: Arc::new(Mutex::new(Some(control))),
                            },
                            child_turns: AgentChildTurnDispatcher {
                                inner: Arc::downgrade(&task_client.inner),
                                approval_handler: child_approval_handler,
                            },
                            mcp_resolver,
                        })
                        .await
                }
                .await;
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
                    boundary_session_seq: None,
                    last_error: String::new(),
                };
                finalizer.stage_terminal(terminal);
                if purpose == AgentTurnPurpose::Child {
                    let _ = state
                        .set_agent_edge_status(&thread_id, crate::state::AgentEdgeStatus::Closed)
                        .await;
                }
                finalizer.finalize_terminal(&state).await;
            });
            client.inner.runtime.set_turn_abort(&spawned_turn_id, task);
        }

        let acceptance =
            await_turn_acceptance(acceptance_rx, admission_cancellation, || handle.interrupt())
                .await;
        acceptance.map_err(|_| {
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

    #[derive(Debug)]
    struct PendingPreparationAdapter {
        entered: Arc<tokio::sync::Notify>,
    }

    impl AgentSessionAdapter for PendingPreparationAdapter {
        fn prepare_turn(
            self: Arc<Self>,
            _request: AgentTurnPreparation,
        ) -> BoxFuture<'static, Result<Box<dyn PreparedAgentTurn>>> {
            Box::pin(async move {
                self.entered.notify_one();
                std::future::pending().await
            })
        }
    }

    #[tokio::test]
    async fn explicit_admission_cancellation_releases_pending_adapter_preparation() {
        let home = tempdir().expect("tempdir");
        let entered = Arc::new(tokio::sync::Notify::new());
        let application = Application::builder()
            .home(home.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(PendingPreparationAdapter {
                entered: entered.clone(),
            }))
            .build()
            .await
            .expect("application");
        let client = application.client();
        let cancellation = TurnAdmissionCancellation::new();
        let task_cancellation = cancellation.clone();
        let cwd = home.path().to_path_buf();
        let admission = tokio::spawn(async move {
            client
                .start_thread_with_turn(
                    crate::application::StartThreadRequest::new(cwd),
                    TurnRequest::new("pending preparation")
                        .with_admission_cancellation(task_cancellation),
                )
                .await
        });

        entered.notified().await;
        cancellation.cancel();
        let error = admission
            .await
            .expect("admission task")
            .expect_err("cancelled admission");
        assert!(error.to_string().contains("cancelled before acceptance"));
        assert!(
            application
                .client()
                .list_threads(crate::application::ThreadListQuery::default())
                .await
                .expect("threads")
                .threads
                .is_empty()
        );
        application
            .shutdown()
            .await
            .expect("shutdown")
            .require_clean()
            .expect("clean shutdown");
    }

    #[tokio::test]
    async fn ready_acceptance_does_not_hide_ready_explicit_cancellation() {
        let cancellation = TurnAdmissionCancellation::new();
        let (acceptance_tx, acceptance_rx) = oneshot::channel();
        acceptance_tx.send(Ok(())).expect("acceptance receipt");
        cancellation.cancel();
        let interrupted = std::sync::atomic::AtomicBool::new(false);

        await_turn_acceptance(acceptance_rx, Some(cancellation), || {
            interrupted.store(true, std::sync::atomic::Ordering::Relaxed);
        })
        .await
        .expect("acceptance channel")
        .expect("accepted Turn");

        assert!(interrupted.load(std::sync::atomic::Ordering::Relaxed));
    }

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
        runtime
            .mark_turn_accepted(&receipt.thread_id, &receipt.turn_id)
            .expect("accept turn");
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
    async fn actor_panic_retains_the_staged_terminal_and_releases_waiters() {
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
        assert_eq!(waiter.to_string(), "Framework Turn actor panicked");
        let mut cursor = 0;
        assert!(matches!(
            events.next(&mut cursor).await,
            Some(TurnEvent::ActivityChanged {
                activity: ThreadActivitySnapshot {
                    running: false,
                    active_turn_id: None,
                    queued_turns: 0,
                    ..
                },
                ..
            })
        ));
        assert!(matches!(
            events.next(&mut cursor).await,
            Some(TurnEvent::Warning { data })
                if data["kind"] == "framework_turn_actor_panic"
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

    #[tokio::test]
    async fn framework_terminal_evidence_preserves_the_durable_fence_facts() {
        let home = tempdir().expect("tempdir").keep();
        let application = Application::builder()
            .home(&home)
            .database_path(":memory:")
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(&home))
            .await
            .expect("Thread");
        let state = &application.inner.state;
        state
            .insert_gateway_turn_delivery(GatewayTurnDeliveryInput {
                turn_id: "terminal-turn",
                thread_id: thread.id(),
                runtime_ref: "terminal-test-runtime",
                input_json: "[]",
                input_hash: "terminal-test-input",
            })
            .await
            .expect("accepted delivery");
        application
            .inner
            .state
            .finalize_framework_turn(
                GatewayTurnTerminalInput {
                    turn_id: "terminal-turn",
                    thread_id: thread.id(),
                    status: FrameworkTurnTerminalStatus::Interrupted,
                    outcome: Some(FrameworkTurnTerminalOutcome::Stopped),
                    error_message: None,
                    started_at_ms: Some(17),
                    completed_at_ms: 42,
                    boundary_session_seq: None,
                    metadata: None,
                },
                "turn_finished",
            )
            .await
            .expect("terminal");

        let evidence = application
            .client()
            .framework_turn_terminal_evidence("terminal-turn")
            .await
            .expect("evidence")
            .expect("terminal evidence");
        assert_eq!(evidence.turn_id, "terminal-turn");
        assert_eq!(evidence.thread_id, thread.id());
        assert_eq!(evidence.status, FrameworkTurnTerminalStatus::Interrupted);
        assert_eq!(evidence.outcome, FrameworkTurnTerminalOutcome::Stopped);
        assert_eq!(evidence.completed_at_ms, 42);
        assert_eq!(evidence.boundary_session_seq, 0);
        assert_eq!(
            state
                .gateway_turn_delivery("terminal-turn")
                .await
                .expect("delivery read")
                .expect("durable delivery")
                .status,
            crate::state::GatewayTurnDeliveryStatus::Terminal
        );

        application.shutdown().await.expect("shutdown");
    }
}
