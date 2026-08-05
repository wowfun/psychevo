use psychevo_agent_core::{Message, UserContentBlock, now_ms};
use serde_json::{Value, json};
use sqlx::Row;

use crate::error::{Error, Result};
use crate::types::{
    EDITABLE_INPUT_METADATA_KEY, StoredEditableInputEnvelope, StoredEditableInputPart,
};

use super::store_metadata::{
    metadata_object, parse_session_revert, parse_session_revert_from_metadata,
};
use super::{
    ConversationDraftPart, SESSION_REVERT_METADATA_KEY, SessionRevertKind, SessionRevertState,
    StateRuntime, UndoTarget,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConversationEditDraftFidelity {
    Exact,
    BestEffort,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConversationEditDraftRead {
    pub(crate) session_seq: i64,
    pub(crate) draft: Vec<ConversationDraftPart>,
    pub(crate) fidelity: ConversationEditDraftFidelity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConversationEditDraftUnavailable {
    SessionNotFound,
    MessageNotFound,
    NotUserMessage,
    CorruptMessage,
    CorruptMetadata,
    CorruptEditableInput,
    EditableInputMismatch,
    NoEditableInput,
    EmptyReplacement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConversationEditDraftReadOutcome {
    Available(ConversationEditDraftRead),
    Unavailable(ConversationEditDraftUnavailable),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HistoryEditingRevertFacts {
    WorkspaceUndo {
        boundary_seq: i64,
        hidden_entry_count: usize,
    },
    ConversationEdit {
        boundary_seq: i64,
        hidden_entry_count: usize,
        draft: Vec<ConversationDraftPart>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryEditingFacts {
    pub(crate) eligibility: HistoryEditingEligibility,
    pub(crate) staged: Option<HistoryEditingRevertFacts>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoryEditingEligibility {
    Eligible,
    Unavailable(HistoryEditingUnavailable),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoryEditingUnavailable {
    SessionNotFound,
    UnsupportedSource,
    ChildThread,
    AgentChildThread,
    SideConversation,
    RuntimeBindingMissing,
    RuntimeBindingUnresolved,
    RuntimeBindingNotNative,
    RuntimeBindingReadOnly,
    CorruptSessionMetadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConversationEditUnavailable {
    HistoryEditing(HistoryEditingUnavailable),
    Draft(ConversationEditDraftUnavailable),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConversationEditConflict {
    WorkspaceUndoStaged,
    ConversationEditStaged,
    ConcurrentMetadataChange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConversationEditStageOutcome {
    Staged,
    AlreadyStaged,
    Unchanged,
    Unavailable(ConversationEditUnavailable),
    Conflict(ConversationEditConflict),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConversationEditRestoreOutcome {
    Restored(Vec<ConversationDraftPart>),
    SessionNotFound,
    NotStaged,
    Conflict(ConversationEditConflict),
}

impl StateRuntime {
    pub(crate) async fn history_editing_facts(
        &self,
        session_id: &str,
    ) -> Result<HistoryEditingFacts> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            Ok(history_editing_snapshot(&mut conn, session_id).await?.facts)
        })
        .await
    }

    pub(crate) async fn conversation_editable_draft(
        &self,
        session_id: &str,
        session_seq: i64,
    ) -> Result<ConversationEditDraftReadOutcome> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            if !session_exists(&mut conn, session_id).await? {
                return Ok(ConversationEditDraftReadOutcome::Unavailable(
                    ConversationEditDraftUnavailable::SessionNotFound,
                ));
            }
            read_conversation_editable_draft(&mut conn, session_id, session_seq).await
        })
        .await
    }

    pub(crate) async fn stage_conversation_edit_atomic(
        &self,
        session_id: &str,
        boundary_seq: i64,
        replacement_draft: Vec<ConversationDraftPart>,
    ) -> Result<ConversationEditStageOutcome> {
        if !draft_is_non_empty(&replacement_draft) {
            return Ok(ConversationEditStageOutcome::Unavailable(
                ConversationEditUnavailable::Draft(
                    ConversationEditDraftUnavailable::EmptyReplacement,
                ),
            ));
        }
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let snapshot = history_editing_snapshot(&mut tx, session_id).await?;
            match snapshot.facts.eligibility {
                HistoryEditingEligibility::Eligible => {}
                HistoryEditingEligibility::Unavailable(reason) => {
                    return Ok(ConversationEditStageOutcome::Unavailable(
                        ConversationEditUnavailable::HistoryEditing(reason),
                    ));
                }
            }
            let expected_metadata_json = snapshot.metadata_json;
            if let Some(revert) = parse_session_revert(expected_metadata_json.as_deref())? {
                return Ok(match revert.kind {
                    SessionRevertKind::WorkspaceUndo { .. } => {
                        ConversationEditStageOutcome::Conflict(
                            ConversationEditConflict::WorkspaceUndoStaged,
                        )
                    }
                    SessionRevertKind::ConversationEdit {
                        boundary_message_id,
                        draft,
                    } if revert.start_seq == boundary_seq
                        && boundary_message_id == format!("message:{boundary_seq}")
                        && draft == replacement_draft =>
                    {
                        ConversationEditStageOutcome::AlreadyStaged
                    }
                    SessionRevertKind::ConversationEdit { .. } => {
                        ConversationEditStageOutcome::Conflict(
                            ConversationEditConflict::ConversationEditStaged,
                        )
                    }
                });
            }
            let current =
                read_conversation_editable_draft(&mut tx, session_id, boundary_seq).await?;
            let current = match current {
                ConversationEditDraftReadOutcome::Available(current) => current,
                ConversationEditDraftReadOutcome::Unavailable(reason) => {
                    return Ok(ConversationEditStageOutcome::Unavailable(
                        ConversationEditUnavailable::Draft(reason),
                    ));
                }
            };
            if current.draft == replacement_draft {
                return Ok(ConversationEditStageOutcome::Unchanged);
            }

            let mut metadata = metadata_object(expected_metadata_json.as_deref())?;
            metadata.insert(
                SESSION_REVERT_METADATA_KEY.to_string(),
                conversation_edit_revert_value(boundary_seq, &replacement_draft),
            );
            let next_metadata_json = encode_metadata(metadata)?;
            let changed = sqlx::query(
                r#"
                UPDATE sessions
                SET metadata_json = ?1, updated_at_ms = ?2
                WHERE id = ?3 AND metadata_json IS ?4
                "#,
            )
            .bind(next_metadata_json)
            .bind(now_ms())
            .bind(session_id)
            .bind(expected_metadata_json)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if changed != 1 {
                return Ok(ConversationEditStageOutcome::Conflict(
                    ConversationEditConflict::ConcurrentMetadataChange,
                ));
            }
            tx.commit().await?;
            Ok(ConversationEditStageOutcome::Staged)
        })
        .await
    }

    pub(crate) async fn restore_conversation_edit_atomic(
        &self,
        session_id: &str,
    ) -> Result<ConversationEditRestoreOutcome> {
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let expected_metadata_json = sqlx::query_scalar::<_, Option<String>>(
                "SELECT metadata_json FROM sessions WHERE id = ?1",
            )
            .bind(session_id)
            .fetch_optional(&mut *tx)
            .await?;
            let Some(expected_metadata_json) = expected_metadata_json else {
                return Ok(ConversationEditRestoreOutcome::SessionNotFound);
            };
            let Some(revert) = parse_session_revert(expected_metadata_json.as_deref())? else {
                return Ok(ConversationEditRestoreOutcome::NotStaged);
            };
            let draft = match revert.kind {
                SessionRevertKind::WorkspaceUndo { .. } => {
                    return Ok(ConversationEditRestoreOutcome::Conflict(
                        ConversationEditConflict::WorkspaceUndoStaged,
                    ));
                }
                SessionRevertKind::ConversationEdit { draft, .. } => draft,
            };
            let mut metadata = metadata_object(expected_metadata_json.as_deref())?;
            metadata.remove(SESSION_REVERT_METADATA_KEY);
            let next_metadata_json = encode_metadata(metadata)?;
            let changed = sqlx::query(
                r#"
                UPDATE sessions
                SET metadata_json = ?1, updated_at_ms = ?2
                WHERE id = ?3 AND metadata_json IS ?4
                "#,
            )
            .bind(next_metadata_json)
            .bind(now_ms())
            .bind(session_id)
            .bind(expected_metadata_json)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if changed != 1 {
                return Ok(ConversationEditRestoreOutcome::Conflict(
                    ConversationEditConflict::ConcurrentMetadataChange,
                ));
            }
            tx.commit().await?;
            Ok(ConversationEditRestoreOutcome::Restored(draft))
        })
        .await
    }

    pub async fn session_revert_state(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionRevertState>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let metadata_json = sqlx::query_scalar::<_, Option<String>>(
                "SELECT metadata_json FROM sessions WHERE id = ?1",
            )
            .bind(session_id)
            .fetch_optional(&mut *conn)
            .await?
            .ok_or_else(|| Error::Message(format!("session not found: {session_id}")))?;
            parse_session_revert(metadata_json.as_deref())
        })
        .await
    }

    pub async fn set_session_revert_state(
        &self,
        session_id: &str,
        revert: SessionRevertState,
    ) -> Result<()> {
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let metadata_json = session_metadata_json(&mut tx, session_id).await?;
            let mut metadata = metadata_object(metadata_json.as_deref())?;
            let value = match &revert.kind {
                SessionRevertKind::WorkspaceUndo { original_snapshot } => json!({
                    "kind": "workspaceUndo",
                    "start_seq": revert.start_seq,
                    "original_snapshot": original_snapshot,
                }),
                SessionRevertKind::ConversationEdit {
                    boundary_message_id,
                    draft,
                } => json!({
                    "kind": "conversationEdit",
                    "start_seq": revert.start_seq,
                    "boundary_message_id": boundary_message_id,
                    "draft": draft,
                }),
            };
            metadata.insert(SESSION_REVERT_METADATA_KEY.to_string(), value);
            let metadata_json = serde_json::to_string(&Value::Object(metadata))?;
            let changed = sqlx::query(
                "UPDATE sessions SET metadata_json = ?1, updated_at_ms = ?2 WHERE id = ?3",
            )
            .bind(metadata_json)
            .bind(now_ms())
            .bind(session_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if changed == 0 {
                return Err(Error::Message(format!("session not found: {session_id}")));
            }
            tx.commit().await?;
            Ok(())
        })
        .await
    }

    pub async fn clear_session_revert_state(&self, session_id: &str) -> Result<()> {
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let metadata_json = session_metadata_json(&mut tx, session_id).await?;
            let mut metadata = metadata_object(metadata_json.as_deref())?;
            metadata.remove(SESSION_REVERT_METADATA_KEY);
            let metadata_json = encode_metadata(metadata)?;
            let changed = sqlx::query(
                "UPDATE sessions SET metadata_json = ?1, updated_at_ms = ?2 WHERE id = ?3",
            )
            .bind(metadata_json)
            .bind(now_ms())
            .bind(session_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if changed == 0 {
                return Err(Error::Message(format!("session not found: {session_id}")));
            }
            tx.commit().await?;
            Ok(())
        })
        .await
    }

    pub async fn latest_undo_target(&self, session_id: &str) -> Result<Option<UndoTarget>> {
        let boundary = self
            .session_revert_state(session_id)
            .await?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        self.user_target_before(session_id, boundary).await
    }

    pub async fn next_redo_target(&self, session_id: &str) -> Result<Option<UndoTarget>> {
        let Some(revert) = self.session_revert_state(session_id).await? else {
            return Ok(None);
        };
        self.user_target_after(session_id, revert.start_seq).await
    }

    pub async fn messages_from_count(&self, session_id: &str, start_seq: i64) -> Result<usize> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let count = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM messages WHERE session_id = ?1 AND session_seq >= ?2",
            )
            .bind(session_id)
            .bind(start_seq)
            .fetch_one(&mut *conn)
            .await?;
            Ok(count.max(0) as usize)
        })
        .await
    }

    pub async fn cleanup_reverted_messages(&self, session_id: &str) -> Result<usize> {
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let metadata_json = session_metadata_json(&mut tx, session_id).await?;
            let Some(revert) = parse_session_revert(metadata_json.as_deref())? else {
                return Ok(0);
            };
            let removed =
                sqlx::query("DELETE FROM messages WHERE session_id = ?1 AND session_seq >= ?2")
                    .bind(session_id)
                    .bind(revert.start_seq)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
            let mut metadata = metadata_object(metadata_json.as_deref())?;
            metadata.remove(SESSION_REVERT_METADATA_KEY);
            let metadata_json = encode_metadata(metadata)?;
            let message_count =
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM messages WHERE session_id = ?1")
                    .bind(session_id)
                    .fetch_one(&mut *tx)
                    .await?;
            let tool_call_count = session_tool_call_count(&mut tx, session_id).await?;
            sqlx::query(
                r#"
                UPDATE sessions
                SET metadata_json = ?1,
                    message_count = ?2,
                    tool_call_count = ?3,
                    updated_at_ms = ?4
                WHERE id = ?5
                "#,
            )
            .bind(metadata_json)
            .bind(message_count)
            .bind(tool_call_count)
            .bind(now_ms())
            .bind(session_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(removed as usize)
        })
        .await
    }
}

struct HistoryEditingSnapshot {
    metadata_json: Option<String>,
    facts: HistoryEditingFacts,
}

async fn history_editing_snapshot(
    conn: &mut sqlx::SqliteConnection,
    session_id: &str,
) -> Result<HistoryEditingSnapshot> {
    let row = sqlx::query(
        r#"
        SELECT s.source,
               s.parent_session_id,
               s.metadata_json,
               b.resolution_status,
               b.backend_kind,
               b.ownership,
               EXISTS(
                   SELECT 1
                   FROM agent_edges e
                   WHERE e.child_session_id = s.id
               ) AS has_agent_edge,
               COALESCE((
                   SELECT COUNT(*)
                   FROM messages m
                   WHERE m.session_id = s.id
                     AND m.session_seq >= CAST(
                         CASE
                             WHEN json_valid(s.metadata_json)
                             THEN json_extract(s.metadata_json, '$.revert.start_seq')
                             ELSE NULL
                         END AS INTEGER
                     )
               ), 0) AS hidden_entry_count
        FROM sessions s
        LEFT JOIN gateway_runtime_bindings b ON b.thread_id = s.id
        WHERE s.id = ?1
        "#,
    )
    .bind(session_id)
    .fetch_optional(conn)
    .await?;
    let Some(row) = row else {
        return Ok(HistoryEditingSnapshot {
            metadata_json: None,
            facts: HistoryEditingFacts {
                eligibility: HistoryEditingEligibility::Unavailable(
                    HistoryEditingUnavailable::SessionNotFound,
                ),
                staged: None,
            },
        });
    };
    let source: String = row.try_get(0)?;
    let parent_session_id: Option<String> = row.try_get(1)?;
    let metadata_json: Option<String> = row.try_get(2)?;
    let resolution_status: Option<String> = row.try_get(3)?;
    let backend_kind: Option<String> = row.try_get(4)?;
    let ownership: Option<String> = row.try_get(5)?;
    let has_agent_edge: i64 = row.try_get(6)?;
    let hidden_entry_count: i64 = row.try_get(7)?;

    let metadata = match metadata_json.as_deref() {
        Some(raw_metadata_json) => match serde_json::from_str::<Value>(raw_metadata_json) {
            Ok(Value::Object(metadata)) => metadata,
            Ok(_) => serde_json::Map::new(),
            Err(_) => {
                return Ok(HistoryEditingSnapshot {
                    metadata_json: metadata_json.clone(),
                    facts: HistoryEditingFacts {
                        eligibility: HistoryEditingEligibility::Unavailable(
                            HistoryEditingUnavailable::CorruptSessionMetadata,
                        ),
                        staged: None,
                    },
                });
            }
        },
        None => serde_json::Map::new(),
    };
    let staged = parse_session_revert_from_metadata(&metadata)?.map(|revert| {
        let hidden_entry_count = hidden_entry_count.max(0) as usize;
        match revert.kind {
            SessionRevertKind::WorkspaceUndo { .. } => HistoryEditingRevertFacts::WorkspaceUndo {
                boundary_seq: revert.start_seq,
                hidden_entry_count,
            },
            SessionRevertKind::ConversationEdit { draft, .. } => {
                HistoryEditingRevertFacts::ConversationEdit {
                    boundary_seq: revert.start_seq,
                    hidden_entry_count,
                    draft,
                }
            }
        }
    });
    let unavailable = if !matches!(source.as_str(), "web" | "tui") {
        Some(HistoryEditingUnavailable::UnsupportedSource)
    } else if parent_session_id.is_some() {
        Some(HistoryEditingUnavailable::ChildThread)
    } else if has_agent_edge != 0 {
        Some(HistoryEditingUnavailable::AgentChildThread)
    } else if metadata
        .get(crate::thread_lineage::SIDE_CONVERSATION_METADATA_KEY)
        .and_then(Value::as_bool)
        == Some(true)
    {
        Some(HistoryEditingUnavailable::SideConversation)
    } else if resolution_status.is_none() {
        Some(HistoryEditingUnavailable::RuntimeBindingMissing)
    } else if resolution_status.as_deref() != Some("resolved") {
        Some(HistoryEditingUnavailable::RuntimeBindingUnresolved)
    } else if backend_kind.as_deref() != Some("native") {
        Some(HistoryEditingUnavailable::RuntimeBindingNotNative)
    } else if ownership.as_deref() != Some("read_write") {
        Some(HistoryEditingUnavailable::RuntimeBindingReadOnly)
    } else {
        None
    };
    Ok(HistoryEditingSnapshot {
        metadata_json,
        facts: HistoryEditingFacts {
            eligibility: unavailable.map_or(
                HistoryEditingEligibility::Eligible,
                HistoryEditingEligibility::Unavailable,
            ),
            staged,
        },
    })
}

pub(super) async fn ensure_native_history_fork_eligible(
    conn: &mut sqlx::SqliteConnection,
    session_id: &str,
) -> Result<()> {
    let snapshot = history_editing_snapshot(conn, session_id).await?;
    if let HistoryEditingEligibility::Unavailable(reason) = snapshot.facts.eligibility {
        return Err(native_history_fork_unavailable(
            session_id,
            history_editing_unavailable_code(reason),
        ));
    }
    if let Some(staged) = snapshot.facts.staged {
        let reason = match staged {
            HistoryEditingRevertFacts::WorkspaceUndo { .. } => "workspace_undo_staged",
            HistoryEditingRevertFacts::ConversationEdit { .. } => "conversation_edit_staged",
        };
        return Err(native_history_fork_unavailable(session_id, reason));
    }
    Ok(())
}

pub(super) async fn ensure_native_history_fork_boundary(
    conn: &mut sqlx::SqliteConnection,
    session_id: &str,
    session_seq: i64,
) -> Result<()> {
    match read_conversation_editable_draft(conn, session_id, session_seq).await? {
        ConversationEditDraftReadOutcome::Available(_) => Ok(()),
        ConversationEditDraftReadOutcome::Unavailable(reason) => Err(
            native_history_fork_unavailable(session_id, editable_draft_unavailable_code(reason)),
        ),
    }
}

fn history_editing_unavailable_code(reason: HistoryEditingUnavailable) -> &'static str {
    match reason {
        HistoryEditingUnavailable::SessionNotFound => "thread_not_found",
        HistoryEditingUnavailable::UnsupportedSource => "unsupported_source",
        HistoryEditingUnavailable::ChildThread => "child_thread",
        HistoryEditingUnavailable::AgentChildThread => "agent_child_thread",
        HistoryEditingUnavailable::SideConversation => "side_conversation",
        HistoryEditingUnavailable::RuntimeBindingMissing => "runtime_binding_missing",
        HistoryEditingUnavailable::RuntimeBindingUnresolved => "runtime_binding_unresolved",
        HistoryEditingUnavailable::RuntimeBindingNotNative => "runtime_binding_not_native",
        HistoryEditingUnavailable::RuntimeBindingReadOnly => "runtime_binding_read_only",
        HistoryEditingUnavailable::CorruptSessionMetadata => "corrupt_thread_metadata",
    }
}

fn editable_draft_unavailable_code(reason: ConversationEditDraftUnavailable) -> &'static str {
    match reason {
        ConversationEditDraftUnavailable::SessionNotFound => "thread_not_found",
        ConversationEditDraftUnavailable::MessageNotFound => "message_not_found",
        ConversationEditDraftUnavailable::NotUserMessage => "not_user_message",
        ConversationEditDraftUnavailable::CorruptMessage => "corrupt_message",
        ConversationEditDraftUnavailable::CorruptMetadata => "corrupt_message_metadata",
        ConversationEditDraftUnavailable::CorruptEditableInput => "corrupt_editable_input",
        ConversationEditDraftUnavailable::EditableInputMismatch => "editable_input_mismatch",
        ConversationEditDraftUnavailable::NoEditableInput => "no_editable_input",
        ConversationEditDraftUnavailable::EmptyReplacement => "empty_replacement",
    }
}

fn native_history_fork_unavailable(session_id: &str, reason: &str) -> Error {
    Error::structured(
        format!("Thread `{session_id}` cannot fork Native history: {reason}"),
        json!({
            "kind": "native_history_unavailable",
            "threadId": session_id,
            "reason": reason,
        }),
    )
}

async fn session_exists(conn: &mut sqlx::SqliteConnection, session_id: &str) -> Result<bool> {
    Ok(
        sqlx::query_scalar::<_, i64>("SELECT 1 FROM sessions WHERE id = ?1")
            .bind(session_id)
            .fetch_optional(conn)
            .await?
            .is_some(),
    )
}

async fn read_conversation_editable_draft(
    conn: &mut sqlx::SqliteConnection,
    session_id: &str,
    session_seq: i64,
) -> Result<ConversationEditDraftReadOutcome> {
    let row = sqlx::query_as::<_, (String, String, Option<String>)>(
        r#"
        SELECT role, message_json, metadata_json
        FROM messages
        WHERE session_id = ?1 AND session_seq = ?2
        "#,
    )
    .bind(session_id)
    .bind(session_seq)
    .fetch_optional(conn)
    .await?;
    let Some((role, message_json, metadata_json)) = row else {
        return Ok(ConversationEditDraftReadOutcome::Unavailable(
            ConversationEditDraftUnavailable::MessageNotFound,
        ));
    };
    if role != "user" {
        return Ok(ConversationEditDraftReadOutcome::Unavailable(
            ConversationEditDraftUnavailable::NotUserMessage,
        ));
    }
    let message = match serde_json::from_str::<Message>(&message_json) {
        Ok(message) => message,
        Err(_) => {
            return Ok(ConversationEditDraftReadOutcome::Unavailable(
                ConversationEditDraftUnavailable::CorruptMessage,
            ));
        }
    };
    let Message::User { content, .. } = message else {
        return Ok(ConversationEditDraftReadOutcome::Unavailable(
            ConversationEditDraftUnavailable::NotUserMessage,
        ));
    };
    let metadata = match metadata_json {
        Some(metadata_json) => match serde_json::from_str::<Value>(&metadata_json) {
            Ok(metadata) => Some(metadata),
            Err(_) => {
                return Ok(ConversationEditDraftReadOutcome::Unavailable(
                    ConversationEditDraftUnavailable::CorruptMetadata,
                ));
            }
        },
        None => None,
    };
    let envelope = metadata
        .as_ref()
        .and_then(|metadata| metadata.get(EDITABLE_INPUT_METADATA_KEY));
    let (draft, fidelity) = match envelope {
        Some(envelope) => {
            let envelope =
                match serde_json::from_value::<StoredEditableInputEnvelope>(envelope.clone()) {
                    Ok(envelope) => envelope,
                    Err(_) => {
                        return Ok(ConversationEditDraftReadOutcome::Unavailable(
                            ConversationEditDraftUnavailable::CorruptEditableInput,
                        ));
                    }
                };
            if envelope.version == 1 {
                let Some(draft) = draft_from_envelope(&envelope, &content) else {
                    return Ok(ConversationEditDraftReadOutcome::Unavailable(
                        ConversationEditDraftUnavailable::EditableInputMismatch,
                    ));
                };
                (draft, ConversationEditDraftFidelity::Exact)
            } else {
                (
                    draft_from_legacy_message(&content),
                    ConversationEditDraftFidelity::BestEffort,
                )
            }
        }
        None => (
            draft_from_legacy_message(&content),
            ConversationEditDraftFidelity::BestEffort,
        ),
    };
    if !draft_is_non_empty(&draft) {
        return Ok(ConversationEditDraftReadOutcome::Unavailable(
            ConversationEditDraftUnavailable::NoEditableInput,
        ));
    }
    Ok(ConversationEditDraftReadOutcome::Available(
        ConversationEditDraftRead {
            session_seq,
            draft,
            fidelity,
        },
    ))
}

fn draft_from_envelope(
    envelope: &StoredEditableInputEnvelope,
    content: &[UserContentBlock],
) -> Option<Vec<ConversationDraftPart>> {
    let images = content
        .iter()
        .filter(|block| !matches!(block, UserContentBlock::Text(_)))
        .collect::<Vec<_>>();
    envelope
        .parts
        .iter()
        .map(|part| match part {
            StoredEditableInputPart::Text { text } => {
                Some(ConversationDraftPart::Text { text: text.clone() })
            }
            StoredEditableInputPart::Image { image_block_index } => images
                .get(*image_block_index)
                .and_then(|block| draft_image_part(block)),
        })
        .collect()
}

fn draft_from_legacy_message(content: &[UserContentBlock]) -> Vec<ConversationDraftPart> {
    content
        .iter()
        .filter_map(|block| match block {
            UserContentBlock::Text(block) => Some(ConversationDraftPart::Text {
                text: block.text.clone(),
            }),
            block => draft_image_part(block),
        })
        .collect()
}

fn draft_image_part(block: &UserContentBlock) -> Option<ConversationDraftPart> {
    match block {
        UserContentBlock::Text(_) => None,
        UserContentBlock::LocalImage(block) => Some(ConversationDraftPart::LocalImage {
            path: block.path.display().to_string(),
        }),
        UserContentBlock::ImageUrl(block) => Some(ConversationDraftPart::ImageUrl {
            url: block.url.clone(),
        }),
    }
}

fn draft_is_non_empty(draft: &[ConversationDraftPart]) -> bool {
    draft.iter().any(|part| match part {
        ConversationDraftPart::Text { text } => !text.trim().is_empty(),
        ConversationDraftPart::LocalImage { .. } | ConversationDraftPart::ImageUrl { .. } => true,
    })
}

fn conversation_edit_revert_value(boundary_seq: i64, draft: &[ConversationDraftPart]) -> Value {
    json!({
        "kind": "conversationEdit",
        "start_seq": boundary_seq,
        "boundary_message_id": format!("message:{boundary_seq}"),
        "draft": draft,
    })
}

async fn session_metadata_json(
    tx: &mut sqlx::Transaction<'static, sqlx::Sqlite>,
    session_id: &str,
) -> Result<Option<String>> {
    sqlx::query_scalar("SELECT metadata_json FROM sessions WHERE id = ?1")
        .bind(session_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(Into::into)
}

fn encode_metadata(metadata: serde_json::Map<String, Value>) -> Result<Option<String>> {
    (!metadata.is_empty())
        .then(|| serde_json::to_string(&Value::Object(metadata)))
        .transpose()
        .map_err(Into::into)
}

async fn session_tool_call_count(
    tx: &mut sqlx::Transaction<'static, sqlx::Sqlite>,
    session_id: &str,
) -> Result<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COALESCE(SUM(json_array_length(tool_calls_json)), 0)
        FROM messages
        WHERE session_id = ?1 AND tool_calls_json IS NOT NULL
        "#,
    )
    .bind(session_id)
    .fetch_one(&mut **tx)
    .await?)
}
