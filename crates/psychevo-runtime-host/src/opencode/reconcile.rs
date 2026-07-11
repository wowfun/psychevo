use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use futures::future::try_join_all;
use serde_json::Value;

use crate::{
    HistoryFidelity, RuntimeDiffUpdate, RuntimeError, RuntimeHistoryMessage, RuntimePlanStep,
    RuntimePlanStepStatus, RuntimePlanUpdate, RuntimeSession, SessionOwnership,
};

use super::process::Generation;
use super::types::{
    AgentInfo, FileDiffInfo, MessageInfo, MessageWithParts, NativeEvent, PermissionRequest,
    QuestionRequest, SessionInfo, StatusMap, TodoInfo,
};

#[derive(Debug)]
pub(crate) struct HydratedInstance {
    pub(crate) children: Vec<SessionInfo>,
    pub(crate) permissions: Vec<PermissionRequest>,
    pub(crate) questions: Vec<QuestionRequest>,
    pub(crate) statuses: StatusMap,
    pub(crate) agents: Vec<AgentInfo>,
    pub(crate) todos: BTreeMap<String, Vec<TodoInfo>>,
    pub(crate) diffs: BTreeMap<String, Vec<FileDiffInfo>>,
}

pub(crate) async fn hydrate(
    generation: &Generation,
    cwd: &Path,
    session_id: &str,
) -> Result<HydratedInstance, RuntimeError> {
    let (root_history, children, permissions, questions, statuses, agents, _mcp) = tokio::try_join!(
        generation.http.messages(cwd, session_id, None),
        generation.http.children(cwd, session_id),
        generation.http.permissions(cwd),
        generation.http.questions(cwd),
        generation.http.statuses(cwd),
        generation.http.agents(cwd),
        generation.http.mcp_status(cwd),
    )?;
    let root_session_id = session_id.to_string();
    let root_messages = root_history.0;
    let timelines = try_join_all(
        std::iter::once(root_session_id.clone())
            .chain(children.iter().map(|child| child.id.clone()))
            .map(|timeline_session_id| {
                let hydrated_messages =
                    (timeline_session_id == root_session_id).then(|| root_messages.clone());
                async move {
                    let messages = match hydrated_messages {
                        Some(messages) => messages,
                        None => {
                            generation
                                .http
                                .messages(cwd, &timeline_session_id, None)
                                .await?
                                .0
                        }
                    };
                    let message_id = messages
                        .iter()
                        .rev()
                        .find(|message| message.info.role == "user")
                        .map(|message| message.info.id.as_str());
                    let (todos, diff) = tokio::try_join!(
                        generation.http.todos(cwd, &timeline_session_id),
                        generation.http.diff(cwd, &timeline_session_id, message_id),
                    )?;
                    Ok::<_, RuntimeError>((timeline_session_id, todos, diff))
                }
            }),
    )
    .await?;
    let mut todos = BTreeMap::new();
    let mut diffs = BTreeMap::new();
    for (timeline_session_id, session_todos, session_diff) in timelines {
        todos.insert(timeline_session_id.clone(), session_todos);
        diffs.insert(timeline_session_id, session_diff);
    }
    generation.set_agents(cwd, agents.clone()).await;
    generation.observe_sessions(cwd, &children).await;
    generation.mark_timeline_http_hydrated(cwd).await;
    Ok(HydratedInstance {
        children,
        permissions,
        questions,
        statuses,
        agents,
        todos,
        diffs,
    })
}

pub(crate) fn event_matches(
    event: &NativeEvent,
    cwd: &Path,
    known_sessions: impl IntoIterator<Item = String>,
) -> bool {
    if let Some(directory) = event.directory.as_deref() {
        return same_directory(directory, cwd);
    }
    let Some(session_id) = event.session_id() else {
        return false;
    };
    known_sessions.into_iter().any(|known| known == session_id)
}

pub(crate) fn is_disposed(event: &NativeEvent, cwd: &Path) -> bool {
    if event.event_type != "server.instance.disposed" {
        return false;
    }
    event
        .directory
        .as_deref()
        .is_some_and(|directory| same_directory(directory, cwd))
        || event
            .properties
            .get("directory")
            .and_then(Value::as_str)
            .is_some_and(|directory| same_directory(directory, cwd))
}

pub(crate) fn session_from_event(event: &NativeEvent) -> Option<SessionInfo> {
    if !matches!(
        event.event_type.as_str(),
        "session.created" | "session.updated"
    ) {
        return None;
    }
    serde_json::from_value(event.properties.get("info")?.clone()).ok()
}

pub(crate) fn message_from_event(event: &NativeEvent) -> Option<MessageInfo> {
    if event.event_type != "message.updated" {
        return None;
    }
    serde_json::from_value(event.properties.get("info")?.clone()).ok()
}

pub(crate) fn permission_from_event(event: &NativeEvent) -> Option<PermissionRequest> {
    (event.event_type == "permission.asked")
        .then(|| serde_json::from_value(event.properties.clone()).ok())
        .flatten()
}

pub(crate) fn question_from_event(event: &NativeEvent) -> Option<QuestionRequest> {
    (event.event_type == "question.asked")
        .then(|| serde_json::from_value(event.properties.clone()).ok())
        .flatten()
}

pub(crate) fn todos_from_event(event: &NativeEvent) -> Option<(&str, Vec<TodoInfo>)> {
    if event.event_type != "todo.updated" {
        return None;
    }
    let session_id = event.properties.get("sessionID")?.as_str()?;
    let todos = serde_json::from_value(event.properties.get("todos")?.clone()).ok()?;
    Some((session_id, todos))
}

pub(crate) fn diff_from_event(event: &NativeEvent) -> Option<(&str, Vec<FileDiffInfo>)> {
    if event.event_type != "session.diff" {
        return None;
    }
    let session_id = event.properties.get("sessionID")?.as_str()?;
    let diff = serde_json::from_value(event.properties.get("diff")?.clone()).ok()?;
    Some((session_id, diff))
}

pub(crate) fn runtime_plan_update(
    runtime_ref: &str,
    thread_id: &str,
    turn_id: &str,
    todos: &[TodoInfo],
) -> Option<RuntimePlanUpdate> {
    let steps = todos
        .iter()
        .map(|todo| {
            let status = match todo.status.as_str() {
                "pending" => RuntimePlanStepStatus::Pending,
                "in_progress" => RuntimePlanStepStatus::InProgress,
                "completed" => RuntimePlanStepStatus::Completed,
                "cancelled" => RuntimePlanStepStatus::Cancelled,
                _ => return None,
            };
            Some(RuntimePlanStep {
                step: todo.content.clone(),
                status,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(RuntimePlanUpdate {
        runtime_ref: runtime_ref.to_string(),
        thread_id: thread_id.to_string(),
        turn_id: turn_id.to_string(),
        explanation: None,
        steps,
    })
}

pub(crate) fn runtime_diff_update(
    runtime_ref: &str,
    thread_id: &str,
    turn_id: &str,
    files: &[FileDiffInfo],
) -> RuntimeDiffUpdate {
    let diff = files
        .iter()
        .map(|entry| {
            entry
                .patch
                .as_deref()
                .filter(|patch| !patch.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| {
                    let file = entry.file.as_deref().unwrap_or("Changed file");
                    let status = entry.status.as_deref().unwrap_or("modified");
                    format!(
                        "{file} ({status}): +{} -{}",
                        entry.additions, entry.deletions
                    )
                })
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    RuntimeDiffUpdate {
        runtime_ref: runtime_ref.to_string(),
        thread_id: thread_id.to_string(),
        turn_id: turn_id.to_string(),
        diff,
    }
}

pub(crate) fn resolved_interaction_id(event: &NativeEvent) -> Option<&str> {
    matches!(
        event.event_type.as_str(),
        "permission.replied" | "question.replied" | "question.rejected"
    )
    .then(|| event.properties.get("requestID")?.as_str())
    .flatten()
}

pub(crate) fn status_is_idle(event: &NativeEvent, session_id: &str) -> bool {
    event.event_type == "session.status"
        && event.session_id() == Some(session_id)
        && event
            .properties
            .get("status")
            .and_then(|status| status.get("type"))
            .and_then(Value::as_str)
            == Some("idle")
}

pub(crate) fn text_delta<'a>(
    event: &'a NativeEvent,
    assistant_ids: &std::collections::HashSet<String>,
) -> Option<&'a str> {
    if event.event_type != "message.part.delta" {
        return None;
    }
    let message_id = event.properties.get("messageID")?.as_str()?;
    if !assistant_ids.contains(message_id) || event.properties.get("field")?.as_str()? != "text" {
        return None;
    }
    event.properties.get("delta")?.as_str()
}

pub(crate) fn tool_observation(
    event: &NativeEvent,
    assistant_ids: &std::collections::HashSet<String>,
) -> Option<ToolEvent> {
    if event.event_type != "message.part.updated" {
        return None;
    }
    let part = event.properties.get("part")?;
    if part.get("type")?.as_str()? != "tool" {
        return None;
    }
    let message_id = part.get("messageID")?.as_str()?;
    if !assistant_ids.contains(message_id) {
        return None;
    }
    Some(ToolEvent {
        id: part.get("id")?.as_str()?.to_string(),
        name: part.get("tool")?.as_str()?.to_string(),
        status: part
            .get("state")
            .and_then(|state| state.get("status"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        detail: part.get("state").cloned(),
    })
}

#[derive(Debug)]
pub(crate) struct ToolEvent {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) status: String,
    pub(crate) detail: Option<Value>,
}

pub(crate) fn matching_assistants<'a>(
    messages: &'a [MessageWithParts],
    user_message_id: &str,
) -> Vec<&'a MessageWithParts> {
    messages
        .iter()
        .filter(|message| {
            message.info.role == "assistant"
                && message.info.parent_id.as_deref() == Some(user_message_id)
        })
        .collect()
}

pub(crate) fn final_answer(messages: &[&MessageWithParts]) -> String {
    messages
        .iter()
        .map(|message| message.text())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn runtime_session(
    info: &SessionInfo,
    messages: Vec<MessageWithParts>,
    cursor: Option<String>,
    ownership: SessionOwnership,
) -> RuntimeSession {
    RuntimeSession {
        native_session_id: info.id.clone(),
        thread_id: None,
        parent_native_session_id: info.parent_id.clone(),
        title: info.title.clone(),
        cwd: info.directory.as_ref().map(Into::into),
        archived: info.time.archived.is_some(),
        updated_at_ms: info.time.updated.or(info.time.created),
        cursor: cursor.clone(),
        native_dedup_key: format!("opencode:{}", info.id),
        fidelity: HistoryFidelity::Partial,
        ownership,
        actions: session_actions(info, ownership),
        messages: messages.into_iter().map(runtime_history_message).collect(),
    }
}

fn runtime_history_message(message: MessageWithParts) -> RuntimeHistoryMessage {
    let text = message.text();
    RuntimeHistoryMessage {
        dedup_key: format!("opencode:{}", message.info.id),
        role: message.info.role,
        text,
        created_at_ms: message.info.time.created,
        metadata: Some(serde_json::json!({
            "nativeMessageId": message.info.id,
            "parentId": message.info.parent_id,
            "providerId": message.info.provider_id,
            "modelId": message.info.model_id,
        })),
    }
}

fn session_actions(info: &SessionInfo, ownership: SessionOwnership) -> Vec<String> {
    let mut actions = vec!["read".to_string(), "fork".to_string()];
    if matches!(ownership, SessionOwnership::Active) || info.parent_id.is_some() {
        return actions;
    }
    if ownership == SessionOwnership::ReadOnly && info.time.archived.is_none() {
        actions.push("resume".to_string());
    }
    actions.extend([
        "rename".to_string(),
        "revert".to_string(),
        "unrevert".to_string(),
        "delete".to_string(),
    ]);
    if info.time.archived.is_none() {
        actions.push("archive".to_string());
    }
    actions
}

pub(crate) fn child_map(children: &[SessionInfo]) -> HashMap<String, String> {
    children
        .iter()
        .filter_map(|child| {
            child
                .parent_id
                .as_ref()
                .map(|parent| (child.id.clone(), parent.clone()))
        })
        .collect()
}

pub(crate) fn status_ownership(
    statuses: &BTreeMap<String, super::types::StatusInfo>,
    session_id: &str,
) -> SessionOwnership {
    match statuses.get(session_id).map(|status| status.kind.as_str()) {
        Some("busy" | "retry") => SessionOwnership::Active,
        _ => SessionOwnership::ReadOnly,
    }
}

fn same_directory(value: &str, cwd: &Path) -> bool {
    let left = std::fs::canonicalize(value).unwrap_or_else(|_| value.into());
    let right = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    left == right
}
