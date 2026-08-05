use std::path::PathBuf;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::{
    AgentRemoteDeleteState, Client, DEFAULT_THREAD_LIST_LIMIT, MAX_THREAD_LIST_LIMIT, Thread,
    ThreadItem, ThreadSummary,
};
use crate::paths::canonicalize_cwd;
use crate::state::{SessionBrowserRequest, SessionListCursor, SessionListProjection};
use crate::types::{PromptDisplayMetadata, TUI_DISPLAY_METADATA_KEY, USER_SHELL_METADATA_KEY};
use crate::{Error, Result};

const MAX_HUMAN_THREAD_BROWSER_LIMIT: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadPresentationBackend {
    Unbound,
    Native,
    Acp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadLifecycleActionPresentation {
    pub enabled: bool,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadLifecyclePresentation {
    pub target_label: Option<String>,
    pub fork: ThreadLifecycleActionPresentation,
    pub delete: ThreadLifecycleActionPresentation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanThreadSummary {
    pub summary: ThreadSummary,
    pub display_title: String,
    pub backend: ThreadPresentationBackend,
    pub lifecycle: ThreadLifecyclePresentation,
}

#[derive(Debug, Clone)]
pub struct HumanThreadListQuery {
    pub cwd: Option<PathBuf>,
    pub archived: bool,
    pub cursor: Option<String>,
    pub limit: usize,
}

impl Default for HumanThreadListQuery {
    fn default() -> Self {
        Self {
            cwd: None,
            archived: false,
            cursor: None,
            limit: DEFAULT_THREAD_LIST_LIMIT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanThreadListPage {
    pub threads: Vec<HumanThreadSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HumanThreadBrowserQuery {
    pub cwd: Option<PathBuf>,
    pub archived: bool,
    pub cursor_cwd: Option<String>,
    pub cursor_offset: usize,
    pub limit: usize,
    pub recent_since_ms: i64,
    pub include_thread_ids: Vec<String>,
    pub active_thread_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanThreadBrowserWorkspace {
    pub cwd: String,
    pub threads: Vec<HumanThreadSummary>,
    pub hidden_count: usize,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadTurnStartReceipt {
    pub client_turn_id: String,
    pub turn_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HumanThreadListCursor {
    cwd: Option<String>,
    archived: bool,
    position: SessionListCursor,
}

impl Client {
    pub async fn human_thread_summary(
        &self,
        thread_id: &str,
    ) -> Result<Option<HumanThreadSummary>> {
        self.ensure_open()?;
        let projection = self.inner.state.session_list_projection(thread_id).await?;
        let activity = self.activity_snapshot();
        projection
            .map(|projection| human_thread_summary(projection, &activity))
            .transpose()
    }

    pub async fn list_human_threads(
        &self,
        query: HumanThreadListQuery,
    ) -> Result<HumanThreadListPage> {
        self.ensure_open()?;
        let cwd = query
            .cwd
            .as_deref()
            .map(canonicalize_cwd)
            .transpose()?
            .map(|cwd| cwd.to_string_lossy().into_owned());
        let cursor = query
            .cursor
            .as_deref()
            .map(|cursor| decode_human_thread_list_cursor(cursor, cwd.as_deref(), query.archived))
            .transpose()?;
        let page = self
            .inner
            .state
            .list_human_session_projections(
                cwd.as_deref(),
                query.archived,
                cursor.as_ref(),
                query.limit.clamp(1, MAX_THREAD_LIST_LIMIT),
            )
            .await?;
        let activity = self.activity_snapshot();
        let threads = page
            .sessions
            .into_iter()
            .map(|projection| human_thread_summary(projection, &activity))
            .collect::<Result<Vec<_>>>()?;
        let next_cursor = page
            .next_cursor
            .map(|position| encode_human_thread_list_cursor(cwd, query.archived, position))
            .transpose()?;
        Ok(HumanThreadListPage {
            threads,
            next_cursor,
        })
    }

    pub async fn browse_human_threads(
        &self,
        query: HumanThreadBrowserQuery,
    ) -> Result<Vec<HumanThreadBrowserWorkspace>> {
        self.ensure_open()?;
        let cwd = query
            .cwd
            .as_deref()
            .map(canonicalize_cwd)
            .transpose()?
            .map(|cwd| cwd.to_string_lossy().into_owned());
        let workspaces = self
            .inner
            .state
            .browse_human_sessions(SessionBrowserRequest {
                cwd: cwd.as_deref(),
                archived: query.archived,
                cursor_cwd: query.cursor_cwd.as_deref(),
                cursor_offset: query.cursor_offset,
                limit: query.limit.clamp(1, MAX_HUMAN_THREAD_BROWSER_LIMIT),
                recent_since_ms: query.recent_since_ms,
                include_session_ids: &query.include_thread_ids,
                active_session_ids: &query.active_thread_ids,
            })
            .await?;
        let activity = self.activity_snapshot();
        let mut workspaces = workspaces
            .into_iter()
            .map(|workspace| {
                Ok(HumanThreadBrowserWorkspace {
                    cwd: workspace.cwd,
                    threads: workspace
                        .sessions
                        .into_iter()
                        .map(|projection| human_thread_summary(projection, &activity))
                        .collect::<Result<Vec<_>>>()?,
                    hidden_count: workspace.hidden_count,
                    next_offset: workspace.next_offset,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        workspaces.sort_by(|left, right| {
            let left_latest = workspace_latest_at(left);
            let right_latest = workspace_latest_at(right);
            right_latest
                .cmp(&left_latest)
                .then_with(|| left.cwd.cmp(&right.cwd))
        });
        Ok(workspaces)
    }
}

impl Thread {
    pub async fn presentation_backend(&self) -> Result<ThreadPresentationBackend> {
        self.client.ensure_open()?;
        let binding = self
            .client
            .inner
            .state
            .gateway_runtime_binding(&self.id)
            .await?;
        thread_presentation_backend(
            binding
                .as_ref()
                .and_then(|binding| binding.backend_kind.as_deref()),
            &self.id,
        )
    }

    pub async fn turn_start_receipts(&self) -> Result<Vec<ThreadTurnStartReceipt>> {
        self.client.ensure_open()?;
        Ok(self
            .client
            .inner
            .state
            .gateway_turn_start_receipts(&self.id)
            .await?
            .into_iter()
            .map(|receipt| ThreadTurnStartReceipt {
                client_turn_id: receipt.client_turn_id,
                turn_id: receipt.turn_id,
            })
            .collect())
    }
}

fn human_thread_summary(
    projection: SessionListProjection,
    activity: &super::ApplicationActivitySnapshot,
) -> Result<HumanThreadSummary> {
    let thread_id = projection.summary.id.clone();
    let metadata = presentation_metadata(&projection)?;
    let backend =
        thread_presentation_backend(projection.runtime_backend_kind.as_deref(), &thread_id)?;
    let lifecycle = thread_lifecycle_presentation(&projection, metadata, backend)?;
    let display_title = projection
        .summary
        .title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .or_else(|| {
            projection
                .first_user_text
                .as_deref()
                .map(compact_display_text)
        })
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| short_thread_id(&thread_id));
    let active_turn_id = activity
        .threads
        .get(&thread_id)
        .and_then(|activity| activity.active_turn_id.clone());
    Ok(HumanThreadSummary {
        summary: ThreadSummary::from_summary(projection.summary, active_turn_id),
        display_title,
        backend,
        lifecycle,
    })
}

fn presentation_metadata(
    projection: &SessionListProjection,
) -> Result<Option<&Map<String, Value>>> {
    match projection.metadata.as_ref() {
        None => Ok(None),
        Some(Value::Object(metadata)) => Ok(Some(metadata)),
        Some(_) => Err(corrupt_thread_presentation(
            &projection.summary.id,
            "metadata",
        )),
    }
}

fn thread_presentation_backend(
    backend_kind: Option<&str>,
    thread_id: &str,
) -> Result<ThreadPresentationBackend> {
    match backend_kind {
        None => Ok(ThreadPresentationBackend::Unbound),
        Some("native") => Ok(ThreadPresentationBackend::Native),
        Some("acp") => Ok(ThreadPresentationBackend::Acp),
        Some(_) => Err(corrupt_thread_presentation(thread_id, "backendKind")),
    }
}

fn thread_lifecycle_presentation(
    projection: &SessionListProjection,
    metadata: Option<&Map<String, Value>>,
    backend: ThreadPresentationBackend,
) -> Result<ThreadLifecyclePresentation> {
    match backend {
        ThreadPresentationBackend::Native => native_thread_lifecycle(projection, metadata),
        ThreadPresentationBackend::Unbound => Ok(ThreadLifecyclePresentation {
            target_label: Some("Psychevo".to_string()),
            fork: unavailable_action("Fork requires a resolved Native or ACP binding."),
            delete: available_action(),
        }),
        ThreadPresentationBackend::Acp => acp_thread_lifecycle(projection, metadata),
    }
}

fn native_thread_lifecycle(
    projection: &SessionListProjection,
    metadata: Option<&Map<String, Value>>,
) -> Result<ThreadLifecyclePresentation> {
    let staged = match metadata.and_then(|metadata| metadata.get("revert")) {
        None => false,
        Some(Value::Object(_)) => true,
        Some(_) => {
            return Err(corrupt_thread_presentation(
                &projection.summary.id,
                "revert",
            ));
        }
    };
    let side = match metadata
        .and_then(|metadata| metadata.get(crate::thread_lineage::SIDE_CONVERSATION_METADATA_KEY))
    {
        None => false,
        Some(Value::Bool(side)) => *side,
        Some(Value::Object(_)) => true,
        Some(_) => {
            return Err(corrupt_thread_presentation(
                &projection.summary.id,
                "sideConversation",
            ));
        }
    };
    let eligible = projection.summary.parent_session_id.is_none()
        && matches!(projection.summary.source.as_str(), "web" | "tui")
        && !side
        && !staged;
    let fork = if eligible {
        available_action()
    } else if staged {
        unavailable_action("Run, restore, or redo the staged history state before forking.")
    } else {
        unavailable_action("Only root Workbench and TUI Native Threads can be forked.")
    };
    Ok(ThreadLifecyclePresentation {
        target_label: Some("Psychevo (Native)".to_string()),
        fork,
        delete: available_action(),
    })
}

fn acp_thread_lifecycle(
    projection: &SessionListProjection,
    metadata: Option<&Map<String, Value>>,
) -> Result<ThreadLifecyclePresentation> {
    let lifecycle = super::thread::decode_agent_thread_lifecycle(projection.metadata.as_ref())
        .map_err(|_| corrupt_thread_presentation(&projection.summary.id, "agentLifecycle"))?;
    let fallback = acp_lifecycle_fallback(metadata, &projection.summary.id)?;
    let target_label = lifecycle
        .projection
        .as_ref()
        .map(|lifecycle| lifecycle.target_label.clone())
        .or(fallback.target_label)
        .or_else(|| projection.runtime_ref.clone());
    let fork = lifecycle
        .projection
        .as_ref()
        .map(|lifecycle| lifecycle.fork)
        .unwrap_or(fallback.fork);
    let pending_delete = !matches!(
        lifecycle.remote_delete,
        AgentRemoteDeleteState::NotRequested
    );
    let delete = lifecycle
        .projection
        .as_ref()
        .map(|lifecycle| lifecycle.delete)
        .unwrap_or(fallback.delete)
        && !pending_delete;
    Ok(ThreadLifecyclePresentation {
        target_label,
        fork: if fork {
            available_action()
        } else {
            unavailable_action("This ACP Agent did not advertise session fork.")
        },
        delete: if delete {
            available_action()
        } else if pending_delete {
            unavailable_action("Remote deletion is pending reconciliation.")
        } else {
            unavailable_action("This ACP Agent did not advertise persistent session deletion.")
        },
    })
}

#[derive(Debug, Default)]
struct AcpLifecycleFallback {
    target_label: Option<String>,
    fork: bool,
    delete: bool,
}

fn acp_lifecycle_fallback(
    metadata: Option<&Map<String, Value>>,
    thread_id: &str,
) -> Result<AcpLifecycleFallback> {
    let Some(peer) = metadata
        .and_then(|metadata| metadata.get(super::agent_session::AGENT_SESSION_METADATA_KEY))
    else {
        return Ok(AcpLifecycleFallback::default());
    };
    let peer = required_object(peer, thread_id, "peerAgent")?;
    let Some(session) = peer.get("sessionProjection") else {
        return Ok(AcpLifecycleFallback::default());
    };
    let session = required_object(session, thread_id, "agentSessionProjection")?;
    let target_label = match session.get("agent") {
        None | Some(Value::Null) => None,
        Some(agent) => {
            let agent = required_object(agent, thread_id, "agentSessionProjection.agent")?;
            match agent.get("title") {
                Some(Value::String(title)) => Some(title.clone()),
                Some(Value::Null) => None,
                Some(_) => {
                    return Err(corrupt_thread_presentation(
                        thread_id,
                        "agentSessionProjection.agent.title",
                    ));
                }
                None => optional_string(agent, "name", thread_id)?.map(str::to_string),
            }
        }
    };
    let session_capabilities = match session.get("capabilities") {
        None => None,
        Some(capabilities) => {
            let capabilities = required_object(
                capabilities,
                thread_id,
                "agentSessionProjection.capabilities",
            )?;
            match capabilities.get("session") {
                None => None,
                Some(session) => Some(required_object(
                    session,
                    thread_id,
                    "agentSessionProjection.capabilities.session",
                )?),
            }
        }
    };
    Ok(AcpLifecycleFallback {
        target_label,
        fork: optional_bool(session_capabilities, "fork", thread_id)?.unwrap_or(false),
        delete: optional_bool(session_capabilities, "delete", thread_id)?.unwrap_or(false),
    })
}

fn required_object<'a>(
    value: &'a Value,
    thread_id: &str,
    field: &'static str,
) -> Result<&'a Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| corrupt_thread_presentation(thread_id, field))
}

fn optional_string<'a>(
    object: &'a Map<String, Value>,
    field: &'static str,
    thread_id: &str,
) -> Result<Option<&'a str>> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(corrupt_thread_presentation(thread_id, field)),
    }
}

fn optional_bool(
    object: Option<&Map<String, Value>>,
    field: &'static str,
    thread_id: &str,
) -> Result<Option<bool>> {
    match object.and_then(|object| object.get(field)) {
        None => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(corrupt_thread_presentation(thread_id, field)),
    }
}

fn available_action() -> ThreadLifecycleActionPresentation {
    ThreadLifecycleActionPresentation {
        enabled: true,
        unavailable_reason: None,
    }
}

fn unavailable_action(reason: &str) -> ThreadLifecycleActionPresentation {
    ThreadLifecycleActionPresentation {
        enabled: false,
        unavailable_reason: Some(reason.to_string()),
    }
}

fn workspace_latest_at(workspace: &HumanThreadBrowserWorkspace) -> i64 {
    workspace
        .threads
        .iter()
        .map(|thread| thread.summary.updated_at_ms)
        .max()
        .unwrap_or_default()
}

fn encode_human_thread_list_cursor(
    cwd: Option<String>,
    archived: bool,
    position: SessionListCursor,
) -> Result<String> {
    Ok(
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&HumanThreadListCursor {
            cwd,
            archived,
            position,
        })?),
    )
}

fn decode_human_thread_list_cursor(
    encoded: &str,
    cwd: Option<&str>,
    archived: bool,
) -> Result<SessionListCursor> {
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| Error::Message("invalid thread list cursor".to_string()))?;
    let cursor = serde_json::from_slice::<HumanThreadListCursor>(&bytes)
        .map_err(|_| Error::Message("invalid thread list cursor".to_string()))?;
    if cursor.cwd.as_deref() != cwd || cursor.archived != archived {
        return Err(Error::Message(
            "thread list cursor does not match the current filters".to_string(),
        ));
    }
    Ok(cursor.position)
}

fn compact_display_text(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_CHARS: usize = 120;
    if collapsed.chars().count() <= MAX_CHARS {
        return collapsed;
    }
    let mut out = collapsed.chars().take(MAX_CHARS - 1).collect::<String>();
    out.push('…');
    out
}

fn short_thread_id(id: &str) -> String {
    id.chars().take(8).collect()
}

fn corrupt_thread_presentation(thread_id: &str, field: &'static str) -> Error {
    Error::structured(
        "Persisted Thread presentation data is invalid.",
        json!({
            "kind": "corrupt_thread_presentation",
            "threadId": thread_id,
            "field": field,
        }),
    )
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserShellDisplay {
    pub command: String,
    pub result: Value,
    pub outcome: String,
}

impl ThreadItem {
    pub fn prompt_display(&self) -> Option<PromptDisplayMetadata> {
        self.metadata
            .as_ref()
            .and_then(|metadata| metadata.get(TUI_DISPLAY_METADATA_KEY))
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
    }

    pub fn user_shell_display(&self) -> Option<UserShellDisplay> {
        if let Some(display) = self
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get(USER_SHELL_METADATA_KEY))
            .and_then(user_shell_display_from_metadata)
        {
            return Some(display);
        }
        user_shell_display_from_xml(&legacy_user_text(&self.message)?)
    }

    pub fn is_side_inherited(&self) -> bool {
        crate::thread_lineage::side_inherited_metadata_hidden(self.metadata.as_ref())
    }
}

fn user_shell_display_from_metadata(metadata: &Value) -> Option<UserShellDisplay> {
    Some(UserShellDisplay {
        command: metadata.get("command")?.as_str()?.to_string(),
        result: metadata
            .get("result")
            .cloned()
            .unwrap_or_else(|| json!({"output": "(no output)", "truncated": false})),
        outcome: metadata
            .get("outcome")
            .and_then(Value::as_str)
            .unwrap_or("normal")
            .to_string(),
    })
}

fn legacy_user_text(message: &psychevo_agent_core::Message) -> Option<String> {
    let message = serde_json::to_value(message).ok()?;
    let text = message
        .get("content")?
        .as_array()?
        .iter()
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

fn user_shell_display_from_xml(text: &str) -> Option<UserShellDisplay> {
    let trimmed = text.trim_start();
    if !trimmed.starts_with("<user_shell_command>") {
        return None;
    }
    let command = unescape_xml_text(extract_xml_tag(trimmed, "command")?);
    let result_text = unescape_xml_text(extract_xml_tag(trimmed, "result").unwrap_or_default());
    let outcome = if result_text.contains("Exit code: 0") {
        "normal"
    } else {
        "failed"
    };
    Some(UserShellDisplay {
        command,
        result: json!({
            "output": result_text,
            "truncated": false,
        }),
        outcome: outcome.to_string(),
    })
}

fn extract_xml_tag<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let start_tag = format!("<{tag}>");
    let end_tag = format!("</{tag}>");
    let start = text.find(&start_tag)? + start_tag.len();
    let end = text[start..].find(&end_tag)? + start;
    Some(&text[start..end])
}

fn unescape_xml_text(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::application::{
        Application, ApplicationActivitySnapshot, StartThreadRequest, user_text_message,
    };
    use crate::types::SessionSummary;

    fn item(message: psychevo_agent_core::Message, metadata: Option<Value>) -> ThreadItem {
        ThreadItem {
            session_seq: 1,
            message,
            usage: None,
            metadata,
            accounting: None,
        }
    }

    fn session_projection(
        backend_kind: Option<&str>,
        metadata: Option<Value>,
    ) -> SessionListProjection {
        SessionListProjection {
            summary: SessionSummary {
                id: "0123456789abcdef".to_string(),
                source: "web".to_string(),
                parent_session_id: None,
                cwd: "/workspace".to_string(),
                model: "model".to_string(),
                provider: "provider".to_string(),
                started_at_ms: 1,
                updated_at_ms: 2,
                ended_at_ms: None,
                end_reason: None,
                archived_at_ms: None,
                message_count: 1,
                tool_call_count: 0,
                title: None,
                forked_from_thread_id: Some("source-thread".to_string()),
            },
            first_user_text: Some(format!("{}   tail", "word ".repeat(30))),
            metadata,
            runtime_backend_kind: backend_kind.map(str::to_string),
            runtime_ref: Some("runtime".to_string()),
        }
    }

    fn idle_activity() -> ApplicationActivitySnapshot {
        ApplicationActivitySnapshot {
            revision: 1,
            threads: BTreeMap::new(),
        }
    }

    #[test]
    fn human_thread_projection_owns_title_fork_and_native_lifecycle_semantics() {
        let summary =
            human_thread_summary(session_projection(Some("native"), None), &idle_activity())
                .expect("Native presentation");

        assert_eq!(
            summary.summary.forked_from_thread_id.as_deref(),
            Some("source-thread")
        );
        assert_eq!(summary.display_title.chars().count(), 120);
        assert!(summary.display_title.ends_with('…'));
        assert_eq!(summary.backend, ThreadPresentationBackend::Native);
        assert_eq!(
            summary.lifecycle.target_label.as_deref(),
            Some("Psychevo (Native)")
        );
        assert!(summary.lifecycle.fork.enabled);
        assert!(summary.lifecycle.delete.enabled);
    }

    #[test]
    fn acp_lifecycle_prefers_typed_projection_and_gates_pending_remote_delete() {
        let summary = human_thread_summary(
            session_projection(
                Some("acp"),
                Some(json!({
                    "agentSessionLifecycle": {
                        "targetLabel": "Imported ACP Agent",
                        "fork": true,
                        "delete": true,
                        "close": false,
                        "resume": true,
                    },
                    "agentSessionDeleteIntent": {
                        "state": "prepared",
                        "createdAtMs": 10,
                    },
                    "peer_agent": {
                        "sessionProjection": {
                            "agent": {"name": "fallback"},
                            "capabilities": {"session": {"fork": false, "delete": false}},
                        }
                    }
                })),
            ),
            &idle_activity(),
        )
        .expect("ACP presentation");

        assert_eq!(
            summary.lifecycle.target_label.as_deref(),
            Some("Imported ACP Agent")
        );
        assert!(summary.lifecycle.fork.enabled);
        assert!(!summary.lifecycle.delete.enabled);
        assert_eq!(
            summary.lifecycle.delete.unavailable_reason.as_deref(),
            Some("Remote deletion is pending reconciliation.")
        );
    }

    #[test]
    fn acp_lifecycle_preserves_resident_session_capability_fallback() {
        let summary = human_thread_summary(
            session_projection(
                Some("acp"),
                Some(json!({
                    "peer_agent": {
                        "sessionProjection": {
                            "agent": {"name": "agent", "title": "Resident ACP Agent"},
                            "capabilities": {"session": {"fork": true, "delete": true}},
                        }
                    }
                })),
            ),
            &idle_activity(),
        )
        .expect("resident ACP presentation");

        assert_eq!(
            summary.lifecycle.target_label.as_deref(),
            Some("Resident ACP Agent")
        );
        assert!(summary.lifecycle.fork.enabled);
        assert!(summary.lifecycle.delete.enabled);
    }

    #[test]
    fn malformed_presentation_metadata_is_a_bounded_structured_error() {
        let error = human_thread_summary(
            session_projection(
                Some("acp"),
                Some(Value::String("not-an-object".to_string())),
            ),
            &idle_activity(),
        )
        .expect_err("corrupt metadata must fail");

        assert_eq!(
            error.structured_data().and_then(|data| data.get("kind")),
            Some(&json!("corrupt_thread_presentation"))
        );
        assert_eq!(
            error.structured_data().and_then(|data| data.get("field")),
            Some(&json!("metadata"))
        );
        assert!(!error.to_string().contains("not-an-object"));
    }

    #[test]
    fn human_thread_cursor_is_opaque_and_filter_scoped() {
        let position = SessionListCursor {
            updated_at_ms: 42,
            id: "thread".to_string(),
        };
        let encoded = encode_human_thread_list_cursor(
            Some("/workspace".to_string()),
            false,
            position.clone(),
        )
        .expect("cursor");

        assert_eq!(
            decode_human_thread_list_cursor(&encoded, Some("/workspace"), false)
                .expect("matching cursor"),
            position
        );
        assert!(
            decode_human_thread_list_cursor(&encoded, Some("/workspace"), true)
                .expect_err("archive mismatch")
                .to_string()
                .contains("does not match the current filters")
        );
    }

    #[tokio::test]
    async fn thread_receipt_projection_rejects_corrupt_durable_entries() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .build()
            .await
            .expect("Application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("Thread");
        application
            .inner
            .state
            .set_session_metadata_field(
                thread.id(),
                "gatewayTurnStartReceipts",
                Some(json!([{"clientTurnId": "client-only"}])),
            )
            .await
            .expect("corrupt fixture");

        let error = thread
            .turn_start_receipts()
            .await
            .expect_err("corrupt receipt must fail");
        assert_eq!(
            error.structured_data().and_then(|data| data.get("kind")),
            Some(&json!("corrupt_thread_turn_start_receipts"))
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn human_list_and_browser_each_use_one_page_bounded_storage_operation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .build()
            .await
            .expect("Application");
        let client = application.client();
        for _ in 0..3 {
            client
                .start_thread(StartThreadRequest::new(temp.path()))
                .await
                .expect("Thread");
        }

        let before_list = application
            .operational_snapshot()
            .storage
            .completed_operations;
        let page = client
            .list_human_threads(HumanThreadListQuery {
                cwd: Some(temp.path().to_path_buf()),
                ..HumanThreadListQuery::default()
            })
            .await
            .expect("human Thread list");
        let after_list = application
            .operational_snapshot()
            .storage
            .completed_operations;
        assert_eq!(page.threads.len(), 3);
        assert_eq!(after_list - before_list, 1);

        let before_browser = after_list;
        let workspaces = client
            .browse_human_threads(HumanThreadBrowserQuery {
                cwd: Some(temp.path().to_path_buf()),
                archived: false,
                cursor_cwd: None,
                cursor_offset: 0,
                limit: 20,
                recent_since_ms: 0,
                include_thread_ids: Vec::new(),
                active_thread_ids: Vec::new(),
            })
            .await
            .expect("human Thread browser");
        let after_browser = application
            .operational_snapshot()
            .storage
            .completed_operations;
        assert_eq!(workspaces.len(), 1);
        assert_eq!(workspaces[0].threads.len(), 3);
        assert_eq!(after_browser - before_browser, 1);
        application.shutdown().await.expect("shutdown");
    }

    #[test]
    fn thread_item_decodes_owned_prompt_and_shell_presentations() {
        let prompt = item(
            user_text_message("provider context"),
            Some(json!({
                TUI_DISPLAY_METADATA_KEY: {
                    "content_text": "visible prompt",
                    "attachments": [],
                }
            })),
        );
        assert_eq!(
            prompt
                .prompt_display()
                .expect("prompt display")
                .content_text,
            "visible prompt"
        );

        let shell = item(
            user_text_message("provider context"),
            Some(json!({
                USER_SHELL_METADATA_KEY: {
                    "command": "pwd",
                    "result": {"output": "/tmp", "truncated": false},
                    "outcome": "normal",
                }
            })),
        );
        let display = shell.user_shell_display().expect("shell display");
        assert_eq!(display.command, "pwd");
        assert_eq!(display.result["output"], "/tmp");

        let inherited = item(
            user_text_message("hidden"),
            Some(json!({"side_inherited": {"hidden": true}})),
        );
        assert!(inherited.is_side_inherited());
    }

    #[test]
    fn shell_display_preserves_undecorated_xml_context_projection() {
        let shell = item(
            user_text_message(
                "<user_shell_command><command>pwd</command><result>Exit code: 0&amp;ok</result></user_shell_command>",
            ),
            None,
        );
        let display = shell.user_shell_display().expect("legacy shell display");
        assert_eq!(display.command, "pwd");
        assert_eq!(display.outcome, "normal");
        assert_eq!(display.result["output"], "Exit code: 0&ok");
    }
}
