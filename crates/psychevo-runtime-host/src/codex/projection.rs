use std::path::PathBuf;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::oneshot;

use crate::{
    HistoryFidelity, RuntimeDiffUpdate, RuntimeHistoryMessage, RuntimeInteraction,
    RuntimeObservation, RuntimeObserver, RuntimePlanStep, RuntimePlanStepStatus, RuntimePlanUpdate,
    RuntimeSession, RuntimeTerminalError, RuntimeTokenUsage, RuntimeTokenUsageBreakdown,
    RuntimeTurnOutcome, RuntimeUsageUpdate, SessionOwnership,
};

#[derive(Debug, Clone)]
pub(super) struct NativeTerminal {
    pub outcome: RuntimeTurnOutcome,
    pub terminal_error: Option<RuntimeTerminalError>,
    pub metadata: Option<Value>,
}

pub(super) struct ActiveTurn {
    pub gateway_turn_id: String,
    pub gateway_thread_id: String,
    pub native_session_id: String,
    pub runtime_ref: String,
    pub process_epoch: u64,
    observer: RuntimeObserver,
    native_turn_id: Mutex<Option<String>>,
    early_notifications: Mutex<Vec<(String, Value)>>,
    final_answer: Mutex<String>,
    latest_plan: Mutex<Option<RuntimePlanUpdate>>,
    latest_diff: Mutex<Option<RuntimeDiffUpdate>>,
    latest_usage: Mutex<Option<RuntimeUsageUpdate>>,
    last_error: Mutex<Option<RuntimeTerminalError>>,
    terminal_sent: AtomicBool,
    terminal: Mutex<Option<oneshot::Sender<NativeTerminal>>>,
}

impl std::fmt::Debug for ActiveTurn {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ActiveTurn")
            .field("gateway_turn_id", &self.gateway_turn_id)
            .field("gateway_thread_id", &self.gateway_thread_id)
            .field("native_session_id", &self.native_session_id)
            .field("runtime_ref", &self.runtime_ref)
            .field("process_epoch", &self.process_epoch)
            .finish_non_exhaustive()
    }
}

impl ActiveTurn {
    pub(super) fn new(
        gateway_turn_id: String,
        gateway_thread_id: String,
        native_session_id: String,
        runtime_ref: String,
        process_epoch: u64,
        observer: RuntimeObserver,
    ) -> (Self, oneshot::Receiver<NativeTerminal>) {
        let (terminal, receiver) = oneshot::channel();
        (
            Self {
                gateway_turn_id,
                gateway_thread_id,
                native_session_id,
                runtime_ref,
                process_epoch,
                observer,
                native_turn_id: Mutex::new(None),
                early_notifications: Mutex::new(Vec::new()),
                final_answer: Mutex::new(String::new()),
                latest_plan: Mutex::new(None),
                latest_diff: Mutex::new(None),
                latest_usage: Mutex::new(None),
                last_error: Mutex::new(None),
                terminal_sent: AtomicBool::new(false),
                terminal: Mutex::new(Some(terminal)),
            },
            receiver,
        )
    }

    pub(super) fn native_turn_id(&self) -> Option<String> {
        self.native_turn_id
            .lock()
            .expect("Codex active turn id poisoned")
            .clone()
    }

    pub(super) fn final_answer(&self) -> String {
        self.final_answer
            .lock()
            .expect("Codex final answer poisoned")
            .clone()
    }

    pub(super) fn emit_interaction(&self, interaction: RuntimeInteraction) {
        self.observer
            .emit(RuntimeObservation::Interaction(Box::new(interaction)));
    }

    pub(super) fn latest_plan(&self) -> Option<RuntimePlanUpdate> {
        self.latest_plan
            .lock()
            .expect("Codex plan cache poisoned")
            .clone()
    }

    pub(super) fn latest_diff(&self) -> Option<RuntimeDiffUpdate> {
        self.latest_diff
            .lock()
            .expect("Codex diff cache poisoned")
            .clone()
    }

    pub(super) fn latest_usage(&self) -> Option<RuntimeUsageUpdate> {
        self.latest_usage
            .lock()
            .expect("Codex usage cache poisoned")
            .clone()
    }

    pub(super) fn emit_observation(&self, observation: RuntimeObservation) {
        self.observer.emit(observation);
    }

    pub(super) fn emit_warning(&self, code: &str, message: impl Into<String>) {
        self.observer.emit(RuntimeObservation::Warning {
            code: code.to_string(),
            message: message.into(),
            diagnostic_ref: None,
        });
    }

    pub(super) fn activate(&self, native_turn_id: String) {
        {
            let mut current = self
                .native_turn_id
                .lock()
                .expect("Codex active turn id poisoned");
            if current.is_none() {
                *current = Some(native_turn_id.clone());
            }
        }
        let pending = std::mem::take(
            &mut *self
                .early_notifications
                .lock()
                .expect("Codex early notification buffer poisoned"),
        );
        for (method, params) in pending {
            if notification_turn_id(&params).as_deref() == Some(native_turn_id.as_str()) {
                self.project(&method, &params);
            }
        }
    }

    pub(super) fn handle_notification(&self, method: &str, params: &Value) {
        if let Some(thread_id) = params.get("threadId").and_then(Value::as_str)
            && thread_id != self.native_session_id
        {
            return;
        }
        let native_turn_id = self.native_turn_id();
        let event_turn_id = notification_turn_id(params);
        if native_turn_id.is_none() && event_turn_id.is_some() {
            self.early_notifications
                .lock()
                .expect("Codex early notification buffer poisoned")
                .push((method.to_string(), params.clone()));
            return;
        }
        if let (Some(expected), Some(observed)) =
            (native_turn_id.as_deref(), event_turn_id.as_deref())
            && expected != observed
        {
            return;
        }
        self.project(method, params);
    }

    fn project(&self, method: &str, params: &Value) {
        match method {
            "item/agentMessage/delta" => {
                if let Some(delta) = params.get("delta").and_then(Value::as_str) {
                    self.final_answer
                        .lock()
                        .expect("Codex final answer poisoned")
                        .push_str(delta);
                    self.observer.emit(RuntimeObservation::TextDelta {
                        turn_id: self.gateway_turn_id.clone(),
                        text: delta.to_string(),
                    });
                }
            }
            "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
                if let Some(delta) = params.get("delta").and_then(Value::as_str) {
                    self.observer.emit(RuntimeObservation::ReasoningDelta {
                        turn_id: self.gateway_turn_id.clone(),
                        text: delta.to_string(),
                    });
                }
            }
            "item/started" | "item/completed" => {
                if let Some(item) = params.get("item") {
                    self.project_item(method == "item/completed", item);
                }
            }
            "turn/plan/updated" => match native_plan_update(params) {
                Ok(native) => {
                    let update = RuntimePlanUpdate {
                        runtime_ref: self.runtime_ref.clone(),
                        thread_id: self.gateway_thread_id.clone(),
                        turn_id: self.gateway_turn_id.clone(),
                        explanation: native.explanation,
                        steps: native
                            .plan
                            .into_iter()
                            .map(|step| RuntimePlanStep {
                                step: step.step,
                                status: step.status.into(),
                            })
                            .collect(),
                    };
                    *self.latest_plan.lock().expect("Codex plan cache poisoned") =
                        Some(update.clone());
                    self.observer.emit(RuntimeObservation::PlanUpdated(update));
                }
                Err(message) => self.emit_warning("codex_invalid_plan_update", message),
            },
            "turn/diff/updated" => match native_diff_update(params) {
                Ok(native) => {
                    let update = RuntimeDiffUpdate {
                        runtime_ref: self.runtime_ref.clone(),
                        thread_id: self.gateway_thread_id.clone(),
                        turn_id: self.gateway_turn_id.clone(),
                        diff: native.diff,
                    };
                    *self.latest_diff.lock().expect("Codex diff cache poisoned") =
                        Some(update.clone());
                    self.observer.emit(RuntimeObservation::DiffUpdated(update));
                }
                Err(message) => self.emit_warning("codex_invalid_diff_update", message),
            },
            "thread/tokenUsage/updated" => match native_usage_update(params) {
                Ok(native) => {
                    let update = RuntimeUsageUpdate {
                        runtime_ref: self.runtime_ref.clone(),
                        thread_id: self.gateway_thread_id.clone(),
                        turn_id: self.gateway_turn_id.clone(),
                        usage: native.token_usage.into(),
                    };
                    *self
                        .latest_usage
                        .lock()
                        .expect("Codex usage cache poisoned") = Some(update.clone());
                    self.observer.emit(RuntimeObservation::UsageUpdated(update));
                }
                Err(message) => self.emit_warning("codex_invalid_usage_update", message),
            },
            "error" => {
                if !params
                    .get("willRetry")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    let error = codex_turn_terminal_error(params.get("error"), self.process_epoch);
                    *self.last_error.lock().expect("Codex turn error poisoned") = Some(error);
                }
            }
            "turn/completed" => {
                let turn = params.get("turn").unwrap_or(params);
                let status = turn
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("failed");
                let outcome = match status {
                    "completed" => RuntimeTurnOutcome::Completed,
                    "interrupted" => RuntimeTurnOutcome::Interrupted,
                    _ => RuntimeTurnOutcome::Failed,
                };
                let terminal_error = (outcome == RuntimeTurnOutcome::Failed).then(|| {
                    turn.get("error")
                        .map(|error| codex_turn_terminal_error(Some(error), self.process_epoch))
                        .or_else(|| {
                            self.last_error
                                .lock()
                                .expect("Codex turn error poisoned")
                                .clone()
                        })
                        .unwrap_or_else(|| codex_turn_terminal_error(None, self.process_epoch))
                });
                self.complete(NativeTerminal {
                    outcome,
                    terminal_error,
                    metadata: Some(json!({"nativeTurn": turn})),
                });
            }
            _ => {}
        }
    }

    fn project_item(&self, completed: bool, item: &Value) {
        let item_type = item
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let item_id = item
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        match item_type {
            "agentMessage" if completed => {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    *self
                        .final_answer
                        .lock()
                        .expect("Codex final answer poisoned") = text.to_string();
                }
            }
            "commandExecution" | "fileChange" | "mcpToolCall" | "dynamicToolCall" => {
                let name = match item_type {
                    "commandExecution" => item
                        .get("command")
                        .and_then(Value::as_str)
                        .unwrap_or("Command"),
                    "fileChange" => "File change",
                    "mcpToolCall" | "dynamicToolCall" => {
                        item.get("tool").and_then(Value::as_str).unwrap_or("Tool")
                    }
                    _ => "Tool",
                };
                let status = item
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or(if completed { "completed" } else { "inProgress" });
                self.observer.emit(RuntimeObservation::Tool {
                    turn_id: self.gateway_turn_id.clone(),
                    item_id,
                    name: name.to_string(),
                    status: status.to_string(),
                    detail: Some(item.clone()),
                });
            }
            "subAgentActivity" => {
                let Some(child_id) = item.get("agentThreadId").and_then(Value::as_str) else {
                    return;
                };
                let status = item
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or(if completed { "completed" } else { "started" });
                self.observer.emit(RuntimeObservation::ChildChanged {
                    runtime_ref: self.runtime_ref.clone(),
                    parent_native_session_id: self.native_session_id.clone(),
                    native_session_id: child_id.to_string(),
                    thread_id: None,
                    status: status.to_string(),
                    read_only: true,
                });
            }
            _ => {}
        }
    }

    pub(super) fn fail_process(
        &self,
        terminal_error: RuntimeTerminalError,
        metadata: Option<Value>,
    ) {
        self.complete(NativeTerminal {
            outcome: RuntimeTurnOutcome::Failed,
            terminal_error: Some(terminal_error),
            metadata,
        });
    }

    pub(super) fn interrupt_after_timeout(&self) {
        self.complete(NativeTerminal {
            outcome: RuntimeTurnOutcome::Interrupted,
            terminal_error: None,
            metadata: Some(json!({"terminalSource": "interrupt_timeout"})),
        });
    }

    fn complete(&self, terminal: NativeTerminal) {
        if self.terminal_sent.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(sender) = self
            .terminal
            .lock()
            .expect("Codex terminal sender poisoned")
            .take()
        {
            let _ = sender.send(terminal);
        }
    }
}

fn codex_turn_terminal_error(error: Option<&Value>, process_epoch: u64) -> RuntimeTerminalError {
    let kind = error
        .and_then(|error| error.get("codexErrorInfo"))
        .and_then(|info| {
            info.as_str().or_else(|| {
                info.as_object()
                    .and_then(|object| object.keys().next().map(String::as_str))
            })
        });
    let (code, stage, retry_class, message) = match kind {
        Some("contextWindowExceeded") => (
            "context_window_exceeded",
            crate::RuntimeErrorStage::Prompt,
            crate::RetryClass::UserAction,
            "The Codex context window was exceeded.",
        ),
        Some("sessionBudgetExceeded") => (
            "session_budget_exceeded",
            crate::RuntimeErrorStage::Prompt,
            crate::RetryClass::UserAction,
            "The Codex session budget was exhausted.",
        ),
        Some("usageLimitExceeded") => (
            "usage_limit_exceeded",
            crate::RuntimeErrorStage::Authentication,
            crate::RetryClass::UserAction,
            "The Codex account usage limit was reached.",
        ),
        Some("serverOverloaded") => (
            "server_overloaded",
            crate::RuntimeErrorStage::Transport,
            crate::RetryClass::SafeRetry,
            "Codex is temporarily overloaded.",
        ),
        Some("unauthorized") => (
            "auth_required",
            crate::RuntimeErrorStage::Authentication,
            crate::RetryClass::UserAction,
            "Codex authentication is required.",
        ),
        Some("httpConnectionFailed")
        | Some("responseStreamConnectionFailed")
        | Some("responseStreamDisconnected")
        | Some("responseTooManyFailedAttempts") => (
            "event_gap",
            crate::RuntimeErrorStage::Transport,
            crate::RetryClass::UnknownDelivery,
            "Codex lost the response stream before the turn completed.",
        ),
        Some("cyberPolicy") => (
            "policy_rejected",
            crate::RuntimeErrorStage::Prompt,
            crate::RetryClass::Never,
            "Codex rejected the turn under its safety policy.",
        ),
        Some("badRequest") => (
            "bad_request",
            crate::RuntimeErrorStage::Prompt,
            crate::RetryClass::Never,
            "Codex rejected the turn request.",
        ),
        Some("sandboxError") => (
            "sandbox_error",
            crate::RuntimeErrorStage::Prompt,
            crate::RetryClass::UserAction,
            "Codex could not apply the requested sandbox policy.",
        ),
        Some("internalServerError") => (
            "internal_server_error",
            crate::RuntimeErrorStage::Prompt,
            crate::RetryClass::UnknownDelivery,
            "Codex failed internally before the turn completed.",
        ),
        _ => (
            "codex_turn_failed",
            crate::RuntimeErrorStage::Prompt,
            crate::RetryClass::Never,
            "Codex failed the turn.",
        ),
    };
    RuntimeTerminalError {
        code: code.to_string(),
        stage,
        retry_class,
        message: message.to_string(),
        diagnostic_ref: format!("codex-process-{process_epoch}-{code}"),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativePlanUpdate {
    #[serde(rename = "threadId")]
    _thread_id: String,
    #[serde(rename = "turnId")]
    _turn_id: String,
    explanation: Option<String>,
    plan: Vec<NativePlanStep>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativePlanStep {
    step: String,
    status: NativePlanStepStatus,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum NativePlanStepStatus {
    Pending,
    InProgress,
    Completed,
}

impl From<NativePlanStepStatus> for RuntimePlanStepStatus {
    fn from(value: NativePlanStepStatus) -> Self {
        match value {
            NativePlanStepStatus::Pending => Self::Pending,
            NativePlanStepStatus::InProgress => Self::InProgress,
            NativePlanStepStatus::Completed => Self::Completed,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeDiffUpdate {
    #[serde(rename = "threadId")]
    _thread_id: String,
    #[serde(rename = "turnId")]
    _turn_id: String,
    diff: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeUsageUpdate {
    #[serde(rename = "threadId")]
    _thread_id: String,
    #[serde(rename = "turnId")]
    _turn_id: String,
    token_usage: NativeTokenUsage,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeTokenUsage {
    total: NativeTokenUsageBreakdown,
    last: NativeTokenUsageBreakdown,
    model_context_window: Option<i64>,
}

impl From<NativeTokenUsage> for RuntimeTokenUsage {
    fn from(value: NativeTokenUsage) -> Self {
        Self {
            total: value.total.into(),
            last: value.last.into(),
            model_context_window: value.model_context_window,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeTokenUsageBreakdown {
    total_tokens: i64,
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    reasoning_output_tokens: i64,
}

impl From<NativeTokenUsageBreakdown> for RuntimeTokenUsageBreakdown {
    fn from(value: NativeTokenUsageBreakdown) -> Self {
        Self {
            total_tokens: value.total_tokens,
            input_tokens: value.input_tokens,
            cached_input_tokens: value.cached_input_tokens,
            output_tokens: value.output_tokens,
            reasoning_output_tokens: value.reasoning_output_tokens,
        }
    }
}

fn native_plan_update(params: &Value) -> Result<NativePlanUpdate, String> {
    serde_json::from_value(params.clone())
        .map_err(|error| format!("Codex plan update did not match the stable schema: {error}"))
}

fn native_diff_update(params: &Value) -> Result<NativeDiffUpdate, String> {
    serde_json::from_value(params.clone())
        .map_err(|error| format!("Codex diff update did not match the stable schema: {error}"))
}

fn native_usage_update(params: &Value) -> Result<NativeUsageUpdate, String> {
    serde_json::from_value(params.clone())
        .map_err(|error| format!("Codex usage update did not match the stable schema: {error}"))
}

pub(super) fn notification_turn_id(params: &Value) -> Option<String> {
    params
        .get("turnId")
        .and_then(Value::as_str)
        .or_else(|| {
            params
                .get("turn")
                .and_then(|turn| turn.get("id"))
                .and_then(Value::as_str)
        })
        .map(str::to_string)
}

pub(super) fn session_from_thread(thread: &Value, archived: bool) -> Option<RuntimeSession> {
    let native_session_id = thread.get("id")?.as_str()?.to_string();
    let parent_native_session_id = thread
        .get("parentThreadId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let ownership = if parent_native_session_id.is_some() {
        SessionOwnership::ReadOnly
    } else if thread_status_active(thread.get("status")) {
        SessionOwnership::Active
    } else {
        SessionOwnership::ReadWrite
    };
    let mut actions = vec!["read".to_string()];
    if ownership == SessionOwnership::ReadWrite {
        let available = if archived {
            &["unarchive", "delete"][..]
        } else {
            &["resume", "fork", "rename", "archive", "delete"][..]
        };
        actions.extend(available.iter().map(|action| (*action).to_string()));
    } else if ownership == SessionOwnership::Active {
        actions.push("fork".to_string());
    }
    let title = thread
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| thread.get("preview").and_then(Value::as_str))
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let cwd = thread.get("cwd").and_then(Value::as_str).map(PathBuf::from);
    let updated_at_ms = thread
        .get("updatedAt")
        .and_then(Value::as_i64)
        .map(|seconds| seconds.saturating_mul(1000));
    Some(RuntimeSession {
        native_session_id: native_session_id.clone(),
        thread_id: None,
        parent_native_session_id,
        title,
        cwd,
        archived,
        updated_at_ms,
        cursor: None,
        native_dedup_key: format!("codex:thread:{native_session_id}"),
        fidelity: HistoryFidelity::Partial,
        ownership,
        actions,
        messages: history_messages(thread, &native_session_id),
    })
}

fn thread_status_active(status: Option<&Value>) -> bool {
    status.is_some_and(|status| {
        status.as_str() == Some("active")
            || status.get("type").and_then(Value::as_str) == Some("active")
    })
}

fn history_messages(thread: &Value, native_session_id: &str) -> Vec<RuntimeHistoryMessage> {
    let mut messages = Vec::new();
    let Some(turns) = thread.get("turns").and_then(Value::as_array) else {
        return messages;
    };
    for turn in turns {
        let turn_id = turn.get("id").and_then(Value::as_str).unwrap_or("unknown");
        let created_at_ms = turn
            .get("startedAt")
            .and_then(Value::as_i64)
            .map(|seconds| seconds.saturating_mul(1000));
        let items_view = turn
            .get("itemsView")
            .and_then(Value::as_str)
            .unwrap_or("full");
        let Some(items) = turn.get("items").and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let item_id = item.get("id").and_then(Value::as_str).unwrap_or("unknown");
            let item_type = item
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let (role, text) = match item_type {
                "userMessage" => (
                    "user",
                    item.get("content")
                        .and_then(Value::as_array)
                        .map(|content| {
                            content
                                .iter()
                                .filter_map(|part| part.get("text").and_then(Value::as_str))
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default(),
                ),
                "agentMessage" => (
                    "assistant",
                    item.get("text")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                ),
                _ => continue,
            };
            messages.push(RuntimeHistoryMessage {
                dedup_key: format!("codex:{native_session_id}:{turn_id}:{item_id}"),
                role: role.to_string(),
                text,
                created_at_ms,
                metadata: Some(json!({
                    "nativeTurnId": turn_id,
                    "nativeItemId": item_id,
                    "itemsView": items_view,
                })),
            });
        }
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_native_turn_error_maps_through_an_allowlisted_typed_classification() {
        let error = codex_turn_terminal_error(
            Some(&json!({
                "message": "native-message-secret failed",
                "codexErrorInfo": "serverOverloaded",
                "additionalDetails": "raw-native-diagnostics",
            })),
            17,
        );
        assert_eq!(error.code, "server_overloaded");
        assert_eq!(error.stage, crate::RuntimeErrorStage::Transport);
        assert_eq!(error.retry_class, crate::RetryClass::SafeRetry);
        assert_eq!(error.message, "Codex is temporarily overloaded.");
        assert_eq!(error.diagnostic_ref, "codex-process-17-server_overloaded");
        let public = serde_json::to_string(&error).expect("terminal error JSON");
        assert!(!public.contains("native-message-secret"));
        assert!(!public.contains("raw-native-diagnostics"));
    }

    #[test]
    fn child_sessions_are_always_read_only_and_history_is_partial() {
        let session = session_from_thread(
            &json!({
                "id": "child-1",
                "parentThreadId": "parent-1",
                "cwd": "/tmp/work",
                "turns": []
            }),
            false,
        )
        .expect("session");
        assert_eq!(session.ownership, SessionOwnership::ReadOnly);
        assert_eq!(session.fidelity, HistoryFidelity::Partial);
        assert_eq!(session.actions, vec!["read"]);
    }
}
