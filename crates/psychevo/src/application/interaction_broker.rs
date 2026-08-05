use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::state::FrameworkInteractionStatus;
use crate::types::{
    ApprovalHandler, BlockingActionKind, ClarifyAnswer, ClarifyResponse, ClarifyResult,
    PermissionApprovalDecision, PermissionApprovalOutcome, PermissionApprovalRequest,
};

use super::{
    ApplicationRuntime, Error, EventLog, InteractionResponse, InteractionResponseReceipt, Result,
    StateRuntime, TurnEvent,
};

const INTERACTION_COMMAND_CAPACITY: usize = 32;

#[derive(Clone)]
pub(super) struct InteractionBroker {
    sender: mpsc::Sender<InteractionCommand>,
    finished: CancellationToken,
    waiters: FrameworkInteractionControl,
    control: crate::types::RunControlHandle,
    log: Arc<EventLog>,
}

enum InteractionCommand {
    Request {
        event: Box<TurnEvent>,
        receipt: Option<oneshot::Sender<std::result::Result<(), String>>>,
    },
    Respond {
        interaction_id: String,
        response: InteractionResponse,
        receipt: oneshot::Sender<std::result::Result<InteractionResponseReceipt, String>>,
    },
    ObservedResolution {
        interaction_id: String,
        kind: String,
        reason: String,
        receipt: Option<oneshot::Sender<std::result::Result<(), String>>>,
    },
}

impl InteractionBroker {
    pub(super) fn new(
        state: StateRuntime,
        runtime: Arc<ApplicationRuntime>,
        log: Arc<EventLog>,
        waiters: FrameworkInteractionControl,
        control: crate::types::RunControlHandle,
        thread_id: String,
        turn_id: String,
    ) -> Self {
        let (sender, mut receiver) = mpsc::channel(INTERACTION_COMMAND_CAPACITY);
        let finished = CancellationToken::new();
        let broker = Self {
            sender,
            finished: finished.clone(),
            waiters: waiters.clone(),
            control: control.clone(),
            log: log.clone(),
        };
        runtime.spawn(async move {
            loop {
                let command = tokio::select! {
                    _ = finished.cancelled() => break,
                    command = receiver.recv() => {
                        let Some(command) = command else {
                            break;
                        };
                        command
                    }
                };
                match command {
                    InteractionCommand::Request { event, receipt } => {
                        let TurnEvent::InteractionRequested {
                            interaction_id,
                            kind,
                            payload,
                        } = &*event
                        else {
                            continue;
                        };
                        let result = match BlockingActionKind::parse_persisted(kind) {
                            Some(kind) => state
                                .request_framework_interaction(
                                    interaction_id,
                                    &thread_id,
                                    &turn_id,
                                    kind,
                                    payload.clone(),
                                )
                                .await
                                .map_err(|error| error.to_string())
                                .and_then(|inserted| {
                                    inserted.then_some(()).ok_or_else(|| {
                                        "interaction is no longer pending".to_string()
                                    })
                                }),
                            None => Err(format!("unsupported Framework interaction kind `{kind}`")),
                        };
                        match &result {
                            Ok(()) => log.push(*event),
                            Err(_) => {
                                cancel_interaction_waiter(&waiters, &control, interaction_id, kind)
                            }
                        }
                        if let Some(receipt) = receipt {
                            let _ = receipt.send(result);
                        }
                    }
                    InteractionCommand::Respond {
                        interaction_id,
                        response,
                        receipt,
                    } => {
                        let kind = match state
                            .pending_framework_interaction_kind(
                                &interaction_id,
                                &thread_id,
                                &turn_id,
                            )
                            .await
                        {
                            Ok(Some(kind)) => kind,
                            Ok(None) => {
                                let _ = receipt.send(Ok(InteractionResponseReceipt {
                                    accepted: false,
                                }));
                                continue;
                            }
                            Err(error) => {
                                let _ = receipt.send(Err(error.to_string()));
                                continue;
                            }
                        };
                        let Some(waiter) = adopt_interaction_waiter(
                            &control,
                            &waiters,
                            &interaction_id,
                            kind.as_str(),
                            &response,
                        ) else {
                            let _ = receipt.send(Ok(InteractionResponseReceipt {
                                accepted: false,
                            }));
                            continue;
                        };
                        let resolution = match serde_json::to_value(&response) {
                            Ok(resolution) => resolution,
                            Err(error) => {
                                waiter.restore(&control, &waiters, interaction_id.clone());
                                let _ = receipt.send(Err(error.to_string()));
                                continue;
                            }
                        };
                        let status = if matches!(&response, InteractionResponse::Cancel) {
                            FrameworkInteractionStatus::Cancelled
                        } else {
                            FrameworkInteractionStatus::Resolved
                        };
                        let result = state
                            .resolve_framework_interaction(
                                &interaction_id,
                                &thread_id,
                                &turn_id,
                                kind,
                                status,
                                resolution,
                            )
                            .await
                            .map_err(|error| error.to_string());
                        let result = match result {
                            Err(error) => {
                                waiter.restore(&control, &waiters, interaction_id.clone());
                                Err(error)
                            }
                            Ok(false) => {
                                waiter.restore(&control, &waiters, interaction_id.clone());
                                Ok(InteractionResponseReceipt { accepted: false })
                            }
                            Ok(true) => {
                                let reason = interaction_response_reason(&response);
                                let delivered = waiter.deliver(response);
                                log.push(TurnEvent::InteractionResolved {
                                    interaction_id: interaction_id.clone(),
                                    kind: kind.as_str().to_string(),
                                    reason: reason.to_string(),
                                });
                                if delivered {
                                    Ok(InteractionResponseReceipt { accepted: true })
                                } else {
                                    Err(format!(
                                        "interaction {interaction_id} committed after its waiter closed"
                                    ))
                                }
                            }
                        };
                        let _ = receipt.send(result);
                    }
                    InteractionCommand::ObservedResolution {
                        interaction_id,
                        kind,
                        reason,
                        receipt,
                    } => {
                        let status = if matches!(
                            reason.as_str(),
                            "cancelled" | "timed_out" | "turn_finished"
                        ) {
                            FrameworkInteractionStatus::Cancelled
                        } else {
                            FrameworkInteractionStatus::Resolved
                        };
                        let resolution = serde_json::json!({
                            "kind": "observed",
                            "interactionKind": kind,
                            "reason": reason,
                        });
                        let result = match BlockingActionKind::parse_persisted(&kind) {
                            Some(durable_kind) => state
                                .resolve_framework_interaction(
                                    &interaction_id,
                                    &thread_id,
                                    &turn_id,
                                    durable_kind,
                                    status,
                                    resolution,
                                )
                                .await
                                .map_err(|error| error.to_string()),
                            None => Err(format!("unsupported Framework interaction kind `{kind}`")),
                        };
                        if matches!(result, Ok(true)) {
                            log.push(TurnEvent::InteractionResolved {
                                interaction_id: interaction_id.clone(),
                                kind,
                                reason,
                            });
                        }
                        if let Some(receipt) = receipt {
                            let _ = receipt.send(result.map(|_| ()));
                        }
                    }
                }
            }
        });
        broker
    }

    pub(super) async fn request(&self, event: TurnEvent) -> Result<()> {
        let (receipt_tx, receipt_rx) = oneshot::channel();
        self.sender
            .send(InteractionCommand::Request {
                event: Box::new(event),
                receipt: Some(receipt_tx),
            })
            .await
            .map_err(|_| Error::Message("interaction broker is closed".to_string()))?;
        receipt_rx
            .await
            .map_err(|_| Error::Message("interaction broker request was cancelled".to_string()))?
            .map_err(Error::Message)
    }

    pub(super) fn observe(&self, event: TurnEvent) {
        let command = match event {
            event @ TurnEvent::InteractionRequested { .. } => InteractionCommand::Request {
                event: Box::new(event),
                receipt: None,
            },
            TurnEvent::InteractionResolved {
                interaction_id,
                kind,
                reason,
            } => InteractionCommand::ObservedResolution {
                interaction_id,
                kind,
                reason,
                receipt: None,
            },
            _ => return,
        };
        if let Err(error) = self.sender.try_send(command) {
            let event = match error.into_inner() {
                InteractionCommand::Request { event, .. } => Some(event),
                _ => None,
            };
            let pending = event.as_deref().and_then(|event| match event {
                TurnEvent::InteractionRequested {
                    interaction_id,
                    kind,
                    ..
                } => Some((interaction_id, kind)),
                _ => None,
            });
            if let Some((interaction_id, kind)) = pending {
                cancel_interaction_waiter(&self.waiters, &self.control, interaction_id, kind);
            }
            self.log.push(TurnEvent::Warning {
                data: serde_json::json!({
                    "kind": "framework_interaction_overload",
                    "message": "interaction broker queue is unavailable",
                }),
            });
        }
    }

    pub(super) async fn respond(
        &self,
        interaction_id: &str,
        response: InteractionResponse,
    ) -> Result<InteractionResponseReceipt> {
        let (receipt_tx, receipt_rx) = oneshot::channel();
        self.sender
            .send(InteractionCommand::Respond {
                interaction_id: interaction_id.to_string(),
                response,
                receipt: receipt_tx,
            })
            .await
            .map_err(|_| Error::Message("interaction broker is closed".to_string()))?;
        receipt_rx
            .await
            .map_err(|_| Error::Message("interaction response was cancelled".to_string()))?
            .map_err(Error::Message)
    }

    async fn cancel(&self, interaction_id: &str, kind: &str, reason: &str) -> Result<()> {
        let (receipt_tx, receipt_rx) = oneshot::channel();
        self.sender
            .send(InteractionCommand::ObservedResolution {
                interaction_id: interaction_id.to_string(),
                kind: kind.to_string(),
                reason: reason.to_string(),
                receipt: Some(receipt_tx),
            })
            .await
            .map_err(|_| Error::Message("interaction broker is closed".to_string()))?;
        receipt_rx
            .await
            .map_err(|_| Error::Message("interaction cancellation was cancelled".to_string()))?
            .map_err(Error::Message)
    }

    pub(super) fn finish(&self) {
        self.finished.cancel();
    }
}

fn cancel_interaction_waiter(
    waiters: &FrameworkInteractionControl,
    control: &crate::types::RunControlHandle,
    interaction_id: &str,
    kind: &str,
) {
    if kind == "permission" {
        let _ = waiters.submit_permission(interaction_id, PermissionApprovalDecision::deny());
    } else if kind == "clarify" {
        let _ = control.submit_clarify_result(interaction_id, ClarifyResult::Cancelled);
    }
}

#[derive(Clone, Default)]
pub(super) struct FrameworkInteractionControl {
    permissions: Arc<Mutex<HashMap<String, oneshot::Sender<PermissionApprovalDecision>>>>,
}

impl FrameworkInteractionControl {
    fn register_permission(
        &self,
        interaction_id: String,
    ) -> oneshot::Receiver<PermissionApprovalDecision> {
        let (sender, receiver) = oneshot::channel();
        self.permissions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(interaction_id, sender);
        receiver
    }

    fn submit_permission(
        &self,
        interaction_id: &str,
        decision: PermissionApprovalDecision,
    ) -> bool {
        self.permissions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(interaction_id)
            .and_then(|sender| sender.send(decision).ok())
            .is_some()
    }

    fn take_permission(
        &self,
        interaction_id: &str,
    ) -> Option<oneshot::Sender<PermissionApprovalDecision>> {
        self.permissions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(interaction_id)
    }

    fn restore_permission(
        &self,
        interaction_id: String,
        sender: oneshot::Sender<PermissionApprovalDecision>,
    ) -> bool {
        if sender.is_closed() {
            return false;
        }
        let mut permissions = self
            .permissions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if permissions.contains_key(&interaction_id) {
            return false;
        }
        permissions.insert(interaction_id, sender);
        true
    }

    fn remove_permission(&self, interaction_id: &str) {
        self.permissions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(interaction_id);
    }

    pub(super) fn cancel_permissions(&self) {
        self.permissions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
}

#[derive(Clone)]
pub(super) struct FrameworkApprovalHandler {
    pub(super) delegate: Option<Arc<dyn ApprovalHandler>>,
    pub(super) interactions: FrameworkInteractionControl,
    pub(super) broker: InteractionBroker,
}

impl fmt::Debug for FrameworkApprovalHandler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FrameworkApprovalHandler")
            .field("has_delegate", &self.delegate.is_some())
            .finish_non_exhaustive()
    }
}

impl ApprovalHandler for FrameworkApprovalHandler {
    fn timeout_secs(&self) -> u64 {
        self.delegate
            .as_ref()
            .map_or(300, |delegate| delegate.timeout_secs())
    }

    fn request_permission(
        &self,
        mut request: PermissionApprovalRequest,
    ) -> BoxFuture<'static, PermissionApprovalDecision> {
        let interaction_id = if request.tool_call_id.trim().is_empty() {
            Uuid::now_v7().to_string()
        } else {
            request.tool_call_id.clone()
        };
        request.tool_call_id = interaction_id.clone();
        let receiver = self
            .interactions
            .register_permission(interaction_id.clone());
        let request_event = TurnEvent::InteractionRequested {
            interaction_id: interaction_id.clone(),
            kind: "permission".to_string(),
            payload: framework_permission_payload(&request),
        };
        let delegate = self.delegate.clone();
        let interactions = self.interactions.clone();
        let broker = self.broker.clone();
        Box::pin(async move {
            if broker.request(request_event).await.is_err() {
                interactions.remove_permission(&interaction_id);
                return PermissionApprovalDecision::deny();
            }
            let mut receiver = receiver;
            let decision = match delegate {
                Some(delegate) => {
                    let mut delegate_response = delegate.request_permission(request);
                    tokio::select! {
                        decision = &mut delegate_response => {
                            match broker
                                .respond(
                                    &interaction_id,
                                    InteractionResponse::Permission(decision.clone()),
                                )
                                .await
                            {
                                Ok(receipt) if receipt.accepted => decision,
                                _ => receiver.await.unwrap_or_else(|_| PermissionApprovalDecision::deny()),
                            }
                        },
                        decision = &mut receiver => decision.unwrap_or_else(|_| PermissionApprovalDecision::deny()),
                    }
                }
                None => receiver
                    .await
                    .unwrap_or_else(|_| PermissionApprovalDecision::deny()),
            };
            interactions.remove_permission(&interaction_id);
            decision
        })
    }

    fn cancel_permission(&self, tool_call_id: &str) -> BoxFuture<'static, ()> {
        self.cancel_permission_with_reason(tool_call_id, "timed_out")
    }

    fn cancel_permission_with_reason(
        &self,
        tool_call_id: &str,
        reason: &str,
    ) -> BoxFuture<'static, ()> {
        let interaction_id = tool_call_id.to_string();
        let reason = reason.to_string();
        let interactions = self.interactions.clone();
        let broker = self.broker.clone();
        let delegate = self.delegate.clone();
        Box::pin(async move {
            interactions.remove_permission(&interaction_id);
            if let Some(delegate) = delegate {
                let (_, broker_result) = tokio::join!(
                    delegate.cancel_permission_with_reason(&interaction_id, &reason),
                    broker.cancel(&interaction_id, "permission", &reason),
                );
                let _ = broker_result;
            } else {
                let _ = broker.cancel(&interaction_id, "permission", &reason).await;
            }
        })
    }
}

fn framework_permission_payload(request: &PermissionApprovalRequest) -> serde_json::Value {
    serde_json::json!({
        "toolName": request.tool_name,
        "summary": request.summary,
        "reason": request.reason,
        "matchedRule": request.matched_rule,
        "suggestedRule": request.suggested_rule,
        "allowSession": request.mcp_startup.is_none(),
        "allowAlways": request.mcp_startup.is_none() && request.allow_always,
        "filesystem": request.filesystem,
        "mcpStartup": request.mcp_startup,
        "timeoutSecs": request.timeout_secs,
    })
}

enum AdoptedInteractionWaiter {
    Permission(oneshot::Sender<PermissionApprovalDecision>),
    Clarify(oneshot::Sender<ClarifyResult>),
}

impl AdoptedInteractionWaiter {
    fn restore(
        self,
        control: &crate::types::RunControlHandle,
        interactions: &FrameworkInteractionControl,
        interaction_id: String,
    ) {
        match self {
            Self::Permission(sender) => {
                interactions.restore_permission(interaction_id, sender);
            }
            Self::Clarify(sender) => {
                control.restore_clarify_waiter(interaction_id, sender);
            }
        }
    }

    fn deliver(self, response: InteractionResponse) -> bool {
        match (self, response) {
            (Self::Permission(sender), InteractionResponse::Permission(decision)) => {
                sender.send(decision).is_ok()
            }
            (Self::Permission(sender), InteractionResponse::Cancel) => {
                sender.send(PermissionApprovalDecision::deny()).is_ok()
            }
            (Self::Clarify(sender), InteractionResponse::Clarify(answers)) => sender
                .send(ClarifyResult::Answered(ClarifyResponse {
                    answers: answers
                        .into_iter()
                        .map(|answers| ClarifyAnswer { answers })
                        .collect(),
                }))
                .is_ok(),
            (Self::Clarify(sender), InteractionResponse::Cancel) => {
                sender.send(ClarifyResult::Cancelled).is_ok()
            }
            _ => false,
        }
    }
}

fn adopt_interaction_waiter(
    control: &crate::types::RunControlHandle,
    interactions: &FrameworkInteractionControl,
    interaction_id: &str,
    durable_kind: &str,
    response: &InteractionResponse,
) -> Option<AdoptedInteractionWaiter> {
    match (durable_kind, response) {
        ("permission", InteractionResponse::Permission(_) | InteractionResponse::Cancel) => {
            interactions
                .take_permission(interaction_id)
                .filter(|sender| !sender.is_closed())
                .map(AdoptedInteractionWaiter::Permission)
        }
        ("clarify", InteractionResponse::Clarify(_) | InteractionResponse::Cancel) => control
            .take_clarify_waiter(interaction_id)
            .filter(|sender| !sender.is_closed())
            .map(AdoptedInteractionWaiter::Clarify),
        _ => None,
    }
}

fn permission_approval_reason(outcome: PermissionApprovalOutcome) -> &'static str {
    match outcome {
        PermissionApprovalOutcome::AllowOnce => "allow_once",
        PermissionApprovalOutcome::AllowTurn => "allow_turn",
        PermissionApprovalOutcome::AllowSession => "allow_session",
        PermissionApprovalOutcome::AllowAlways => "allow_always",
        PermissionApprovalOutcome::Deny => "deny",
    }
}

fn interaction_response_reason(response: &InteractionResponse) -> &'static str {
    match response {
        InteractionResponse::Permission(decision) => permission_approval_reason(decision.outcome),
        InteractionResponse::Clarify(_) => "answered",
        InteractionResponse::Cancel => "cancelled",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framework_mcp_startup_payload_is_typed_and_one_shot_only() {
        let payload = framework_permission_payload(&PermissionApprovalRequest {
            tool_call_id: "call-1".to_string(),
            tool_name: "mcp_startup".to_string(),
            summary: "Start docs".to_string(),
            reason: "review startup".to_string(),
            matched_rule: None,
            suggested_rule: None,
            allow_always: true,
            filesystem: None,
            mcp_startup: Some(crate::types::McpStartupApprovalRequest {
                server: "docs".to_string(),
                source: "profile:mcp:docs".to_string(),
                target: crate::types::McpStartupApprovalTarget::Stdio {
                    command: "/usr/bin/docs-mcp".to_string(),
                    args: vec!["--serve".to_string()],
                    cwd: "/workspace".to_string(),
                    env_names: vec!["DOCS_TOKEN".to_string()],
                },
            }),
            timeout_secs: 30,
        });

        assert_eq!(payload["allowSession"], false);
        assert_eq!(payload["allowAlways"], false);
        assert_eq!(payload["mcpStartup"]["server"], "docs");
        assert_eq!(payload["mcpStartup"]["source"], "profile:mcp:docs");
        assert_eq!(payload["mcpStartup"]["target"]["kind"], "stdio");
        assert_eq!(
            payload["mcpStartup"]["target"]["command"],
            "/usr/bin/docs-mcp"
        );
        assert_eq!(payload["mcpStartup"]["target"]["envNames"][0], "DOCS_TOKEN");
    }
}
