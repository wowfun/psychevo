use super::{
    AbortSignal, AgentEdgeStatus, AgentMailboxEventInput, AgentMailboxEventRecord, AssistantBlock,
    BTreeMap, BoxFuture, Duration, Message, Outcome, Path, PathBuf, Result, SqliteStore,
    ToolBinding, ToolExecutionMode, ToolOutput, Value, json, resolve_skills_home,
    user_text_message,
};
use super::{
    catalog_surface::{
        AGENT_RUNS, AgentRunRecord, AgentRunStatus, AgentToolContext, agent_status_model_value,
        agent_status_value, close_agent_id, wait_agent_mailbox,
    },
    child_runs::{AGENT_NOTIFICATION_METADATA_KEY, sanitize_task_name},
    lifecycle::{
        agent_status_is_final, model_content_string, resume_agent_id,
        send_agent_message_with_context, subagent_summary_value,
    },
};

pub(crate) fn append_parent_agent_start_notification(
    store: &SqliteStore,
    parent_session_id: &str,
    record: &AgentRunRecord,
) -> Result<()> {
    let text = format!(
        "Agent `{}` started in the background.\n\n{}",
        record.agent_name, record.task
    );
    let message = user_text_message(text);
    store.append_message_with_metrics(
        parent_session_id,
        &message,
        None,
        Some(json!({
            AGENT_NOTIFICATION_METADATA_KEY: {
                "type": "agent_started",
                "agent_id": record.id,
                "task_name": record.task_name,
                "agent_name": record.agent_name,
                "child_session_id": record.child_session_id,
                "status": record.status,
                "summary": record.task,
                "effective_max_spawn_depth": record.effective_max_spawn_depth,
                "hidden": false
            }
        })),
    )
}

pub(crate) fn append_parent_agent_mailbox_event(
    store: &SqliteStore,
    parent_session_id: &str,
    record: &AgentRunRecord,
    outcome: &str,
    final_answer: &str,
) -> Result<()> {
    let summary = subagent_summary_value(Some(store), record, false);
    let content = subagent_notification_content(&summary);
    let payload = inter_agent_communication_payload(record, content.clone());
    let content_text = serde_json::to_string(&payload)?;
    store.append_agent_mailbox_event(AgentMailboxEventInput {
        parent_session_id: parent_session_id.to_string(),
        child_session_id: record.child_session_id.clone(),
        agent_id: record.id.clone(),
        task_name: record.task_name.clone(),
        agent_name: record.agent_name.clone(),
        content_text,
        payload,
        metadata: Some(json!({
            "type": "agent_completed",
            "agent_id": record.id,
            "task_name": record.task_name,
            "agent_name": record.agent_name,
            "child_session_id": record.child_session_id,
            "status": record.status,
            "outcome": outcome,
            "summary": final_answer,
            "model_visible_summary": summary,
            "background": record.background,
            "effective_max_spawn_depth": record.effective_max_spawn_depth
        })),
    })?;
    Ok(())
}

pub(crate) fn subagent_notification_content(summary: &Value) -> String {
    format!(
        "<subagent_notification>\n{}\n</subagent_notification>",
        summary
    )
}

pub(crate) fn inter_agent_communication_payload(record: &AgentRunRecord, content: String) -> Value {
    let author = record
        .task_name
        .as_deref()
        .map(sanitize_task_name)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| sanitize_task_name(&record.agent_name));
    json!({
        "author": format!("/root/{author}"),
        "recipient": "/root",
        "other_recipients": [],
        "content": content,
        "trigger_turn": false
    })
}

pub(crate) fn agent_mailbox_event_message(record: &AgentMailboxEventRecord) -> Message {
    Message::Assistant {
        content: vec![AssistantBlock::Text {
            text: record.content_text.clone(),
        }],
        timestamp_ms: record.delivered_at_ms.unwrap_or(record.created_at_ms),
        finish_reason: Some("inter_agent_communication".to_string()),
        outcome: Outcome::Normal,
        model: None,
        provider: None,
    }
}

pub(crate) fn fork_messages(
    snapshot: &[Message],
    fork_context: bool,
    fork_turns: Option<&str>,
) -> Vec<Message> {
    if !fork_context && fork_turns.unwrap_or("none") == "none" {
        return Vec::new();
    }
    match fork_turns.unwrap_or("all") {
        "none" => Vec::new(),
        "all" => snapshot.to_vec(),
        raw => match raw.parse::<usize>() {
            Ok(count) => snapshot
                .iter()
                .rev()
                .take(count)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect(),
            Err(_) => snapshot.to_vec(),
        },
    }
}

pub(crate) fn update_run_child_session(id: &str, child_session: &str) {
    let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
    if let Some(state) = runs.get_mut(id) {
        state.record.child_session_id = Some(child_session.to_string());
    }
}

pub(crate) fn update_run_completed(
    id: &str,
    outcome: Outcome,
    final_answer: String,
) -> AgentRunRecord {
    let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
    let state = runs.get_mut(id).expect("agent run exists");
    if agent_status_is_final(state.record.status) {
        return state.record.clone();
    }
    state.record.status = match outcome {
        Outcome::Normal => AgentRunStatus::Completed,
        Outcome::Stopped | Outcome::Aborted => AgentRunStatus::Interrupted,
        Outcome::Failed => AgentRunStatus::Errored,
    };
    state.record.ended_at_ms = Some(now_ms());
    state.record.outcome = Some(outcome.as_str().to_string());
    state.record.final_answer = Some(final_answer);
    state.record.edge_status = Some(AgentEdgeStatus::Closed);
    state.record.clone()
}

pub(crate) fn update_run_failed(id: &str, error: &str) {
    let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
    if let Some(state) = runs.get_mut(id) {
        if agent_status_is_final(state.record.status) {
            return;
        }
        state.record.status = AgentRunStatus::Errored;
        state.record.ended_at_ms = Some(now_ms());
        state.record.outcome = Some("failed".to_string());
        state.record.error = Some(error.to_string());
        state.record.edge_status = Some(AgentEdgeStatus::Closed);
    }
}

pub(crate) struct ListAgentsTool {
    pub(crate) context: AgentToolContext,
}

impl ListAgentsTool {
    pub(crate) fn new(context: AgentToolContext) -> Self {
        Self { context }
    }
}

impl ToolBinding for ListAgentsTool {
    fn name(&self) -> &str {
        "list_agents"
    }

    fn description(&self) -> &str {
        "List live and resumable child agents for this session."
    }

    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}, "additionalProperties": false})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn execute(
        &self,
        _tool_call_id: String,
        _args: Value,
        _abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let store = self.context.state.store().clone();
        let parent = self.context.parent_session_id.clone();
        Box::pin(async move {
            let system_value = agent_status_value(Some(&store), Some(&parent), false);
            let model_value = agent_status_model_value(Some(&store), Some(&parent), false);
            ToolOutput::ok_with_model_content(system_value, model_content_string(&model_value))
        })
    }
}

pub(crate) struct WaitAgentTool {
    pub(crate) context: AgentToolContext,
}

impl WaitAgentTool {
    pub(crate) fn new(context: AgentToolContext) -> Self {
        Self { context }
    }
}

impl ToolBinding for WaitAgentTool {
    fn name(&self) -> &str {
        "wait_agent"
    }

    fn description(&self) -> &str {
        "Wait for a background child-agent update. The result reports wait status; completed output arrives separately in the conversation context."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "timeout_ms": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Maximum time to wait for a pending or newly arriving background-agent update; defaults to 30000 milliseconds."
                }
            },
            "additionalProperties": false
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        tool_call_id: String,
        args: Value,
        _abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let store = self.context.state.store().clone();
        let parent_session_id = self.context.parent_session_id.clone();
        let control_handle = self.context.control_handle.clone();
        Box::pin(async move {
            let timeout_ms = args
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(30_000);
            let value = match wait_agent_mailbox(
                &parent_session_id,
                Duration::from_millis(timeout_ms),
                &store,
            )
            .await
            {
                Ok(value) => value,
                Err(err) => return ToolOutput::error(err.to_string()),
            };
            let timed_out = value
                .get("timed_out")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !timed_out {
                let delivered_after_seq = match store.next_message_seq(&parent_session_id) {
                    Ok(seq) => seq,
                    Err(err) => return ToolOutput::error(err.to_string()),
                };
                let delivered = match store.deliver_pending_agent_mailbox_events_for_tool(
                    &parent_session_id,
                    &tool_call_id,
                    delivered_after_seq,
                ) {
                    Ok(records) => records,
                    Err(err) => return ToolOutput::error(err.to_string()),
                };
                if let Some(handle) = control_handle {
                    for record in delivered.iter().filter(|record| {
                        record.delivered_tool_call_id.as_deref() == Some(tool_call_id.as_str())
                            && record.delivered_after_session_seq == Some(delivered_after_seq)
                    }) {
                        let _ = handle.inject_user_message(agent_mailbox_event_message(record));
                    }
                }
            }
            ToolOutput::ok(value)
        })
    }
}

pub(crate) struct SendMessageTool {
    pub(crate) context: AgentToolContext,
}

impl SendMessageTool {
    pub(crate) fn new(context: AgentToolContext) -> Self {
        Self { context }
    }
}

impl ToolBinding for SendMessageTool {
    fn name(&self) -> &str {
        "send_message"
    }

    fn description(&self) -> &str {
        "Send a message to an agent. Closed or completed agents are reopened for continuation."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "Agent id or unambiguous task label identifying the child agent to message."
                },
                "message": {
                    "type": "string",
                    "description": "Message text to send as the next user turn for the target agent."
                }
            },
            "required": ["target", "message"],
            "additionalProperties": false
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let context = self.context.clone();
        let store = context.state.store().clone();
        Box::pin(async move {
            let target = args
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let message = args
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match send_agent_message_with_context(context, target, message, abort).await {
                Ok(Some(record)) => {
                    let system_value = json!({ "agent": record.clone() });
                    let model_value =
                        json!({ "agent": subagent_summary_value(Some(&store), &record, true) });
                    ToolOutput::ok_with_model_content(
                        system_value,
                        model_content_string(&model_value),
                    )
                }
                Ok(None) => ToolOutput::error(format!("agent not found: {target}")),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

pub(crate) struct CloseAgentTool {
    pub(crate) context: AgentToolContext,
}

impl CloseAgentTool {
    pub(crate) fn new(context: AgentToolContext) -> Self {
        Self { context }
    }
}

impl ToolBinding for CloseAgentTool {
    fn name(&self) -> &str {
        "close_agent"
    }

    fn description(&self) -> &str {
        "Close an agent and its open descendants."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "Agent id or unambiguous task label identifying the child agent to close."
                }
            },
            "required": ["target"],
            "additionalProperties": false
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        _abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let store = self.context.state.store().clone();
        Box::pin(async move {
            let target = args
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match close_agent_id(target, Some(&store)) {
                Ok(Some(record)) => {
                    let system_value = json!({ "previous_status": record.clone() });
                    let model_value = json!({
                        "previous_status": subagent_summary_value(Some(&store), &record, true)
                    });
                    ToolOutput::ok_with_model_content(
                        system_value,
                        model_content_string(&model_value),
                    )
                }
                Ok(None) => ToolOutput::error(format!("agent not found: {target}")),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

pub(crate) struct ResumeAgentTool {
    pub(crate) context: AgentToolContext,
}

impl ResumeAgentTool {
    pub(crate) fn new(context: AgentToolContext) -> Self {
        Self { context }
    }
}

impl ToolBinding for ResumeAgentTool {
    fn name(&self) -> &str {
        "resume_agent"
    }

    fn description(&self) -> &str {
        "Reopen a closed agent."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Agent id or unambiguous task label identifying the closed agent to reopen."
                }
            },
            "required": ["id"],
            "additionalProperties": false
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        _abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let store = self.context.state.store().clone();
        Box::pin(async move {
            let id = args.get("id").and_then(Value::as_str).unwrap_or_default();
            match resume_agent_id(id, Some(&store)) {
                Ok(Some(record)) => {
                    let system_value = json!({ "agent": record.clone() });
                    let model_value =
                        json!({ "agent": subagent_summary_value(Some(&store), &record, true) });
                    ToolOutput::ok_with_model_content(
                        system_value,
                        model_content_string(&model_value),
                    )
                }
                Ok(None) => ToolOutput::error(format!("agent not found: {id}")),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

pub(crate) fn resolve_agents_home(env: &BTreeMap<String, String>, cwd: &Path) -> Result<PathBuf> {
    resolve_skills_home(env, cwd)
}

#[cfg(test)]
pub(crate) fn run_hook_commands(
    hooks: Option<&Value>,
    event: &str,
    cwd: &Path,
    payload: &Value,
) -> Option<String> {
    crate::hooks::run_hook_commands(hooks, event, cwd, payload)
}

pub(crate) fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
