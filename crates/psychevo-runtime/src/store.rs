use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use psychevo_agent_core::{AssistantBlock, Message, now_ms};
use psychevo_ai::Outcome;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::messages::{sanitize_message_for_output, sanitize_message_for_tui_history};
use crate::run::normalize_session_title;
use crate::types::{SanitizedMessageSummary, SessionSummary, TuiMessageSummary};

const SQLITE_SCHEMA_VERSION: i64 = 3;
const SESSION_REVERT_METADATA_KEY: &str = "revert";
const MESSAGE_UNDO_METADATA_KEY: &str = "undo";
const MESSAGE_PRE_SNAPSHOT_KEY: &str = "pre_snapshot";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRevertState {
    pub start_seq: i64,
    pub original_snapshot: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoTarget {
    pub seq: i64,
    pub prompt: String,
    pub snapshot: Option<String>,
}

#[derive(Clone)]
pub struct SqliteStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStore {
    pub fn open(path: &Path) -> Result<Self> {
        if path != Path::new(":memory:")
            && let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_millis(250))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let user_version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let has_schema =
            sqlite_table_exists(&conn, "sessions")? || sqlite_table_exists(&conn, "messages")?;
        if user_version != 0 && user_version != SQLITE_SCHEMA_VERSION {
            return Err(Error::Config(format!(
                "state database schema version {user_version} is not supported; run `pevo init --reset-state` or set PSYCHEVO_DB to a new state database"
            )));
        }
        if user_version == 0 && has_schema {
            return Err(Error::Config(
                "state database has an unknown schema version; run `pevo init --reset-state` or set PSYCHEVO_DB to a new state database".to_string(),
            ));
        }
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                parent_session_id TEXT,
                workdir TEXT NOT NULL,
                model TEXT NOT NULL,
                provider TEXT NOT NULL,
                started_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                ended_at_ms INTEGER,
                end_reason TEXT,
                message_count INTEGER NOT NULL DEFAULT 0,
                tool_call_count INTEGER NOT NULL DEFAULT 0,
                title TEXT,
                metadata_json TEXT
            );

            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id),
                session_seq INTEGER NOT NULL,
                role TEXT NOT NULL,
                timestamp_ms INTEGER NOT NULL,
                message_json TEXT NOT NULL,
                content_text TEXT,
                tool_call_id TEXT,
                tool_name TEXT,
                tool_calls_json TEXT,
                finish_reason TEXT,
                outcome TEXT,
                model TEXT,
                provider TEXT,
                usage_json TEXT,
                metadata_json TEXT,
                UNIQUE(session_id, session_seq)
            );

            CREATE INDEX IF NOT EXISTS idx_messages_session_seq
                ON messages(session_id, session_seq);
            "#,
        )?;
        conn.pragma_update(None, "user_version", SQLITE_SCHEMA_VERSION)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn create_session(&self, workdir: &Path) -> Result<String> {
        self.create_session_with_metadata(workdir, "smoke", "fake-coding-model", "fake", None)
    }

    pub fn create_session_with_metadata(
        &self,
        workdir: &Path,
        source: &str,
        model: &str,
        provider: &str,
        metadata: Option<Value>,
    ) -> Result<String> {
        let id = Uuid::now_v7().to_string();
        let now = now_ms();
        let workdir = workdir.to_string_lossy().to_string();
        let metadata_json = metadata
            .map(|value| serde_json::to_string(&value))
            .transpose()?;
        self.write_retry(|conn| {
            conn.execute(
                r#"
                INSERT INTO sessions (
                    id, source, parent_session_id, workdir, model, provider,
                    started_at_ms, updated_at_ms, ended_at_ms, end_reason,
                    message_count, tool_call_count, title, metadata_json
                ) VALUES (?1, ?2, NULL, ?3, ?4, ?5,
                    ?6, ?6, NULL, NULL, 0, 0, NULL, ?7)
                "#,
                params![&id, source, &workdir, model, provider, now, &metadata_json],
            )?;
            Ok(())
        })?;
        Ok(id)
    }

    pub fn latest_run_session_for_workdir(&self, workdir: &Path) -> Result<Option<String>> {
        self.latest_session_for_workdir_with_sources(workdir, &["run"])
    }

    pub fn latest_session_for_workdir_with_sources(
        &self,
        workdir: &Path,
        sources: &[&str],
    ) -> Result<Option<String>> {
        Ok(self
            .list_sessions_for_workdir_with_sources(workdir, sources)?
            .into_iter()
            .next()
            .map(|session| session.id))
    }

    pub fn list_sessions_for_workdir_with_sources(
        &self,
        workdir: &Path,
        sources: &[&str],
    ) -> Result<Vec<SessionSummary>> {
        let workdir = workdir.to_string_lossy().to_string();
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT id, source, workdir, model, provider, started_at_ms,
                   updated_at_ms, ended_at_ms, end_reason, message_count,
                   tool_call_count, title
            FROM sessions
            WHERE workdir = ?1
            ORDER BY updated_at_ms DESC, started_at_ms DESC
            "#,
        )?;
        let rows = stmt.query_map(params![workdir], session_summary_from_row)?;
        let mut summaries = Vec::new();
        for row in rows {
            let summary = row?;
            if sources.is_empty() || sources.iter().any(|source| *source == summary.source) {
                summaries.push(summary);
            }
        }
        Ok(summaries)
    }

    pub fn session_summary(&self, session_id: &str) -> Result<Option<SessionSummary>> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        Ok(conn
            .query_row(
                r#"
                SELECT id, source, workdir, model, provider, started_at_ms,
                       updated_at_ms, ended_at_ms, end_reason, message_count,
                       tool_call_count, title
                FROM sessions
                WHERE id = ?1
                "#,
                params![session_id],
                session_summary_from_row,
            )
            .optional()?)
    }

    pub fn set_session_title(&self, session_id: &str, title: &str) -> Result<String> {
        let title = normalize_session_title(title)
            .ok_or_else(|| Error::Message("session title is empty".to_string()))?;
        let changed = self.write_retry(|conn| {
            conn.execute(
                "UPDATE sessions SET title = ?1 WHERE id = ?2",
                params![&title, session_id],
            )
        })?;
        if changed == 0 {
            return Err(Error::Message(format!("session not found: {session_id}")));
        }
        Ok(title)
    }

    pub fn session_revert_state(&self, session_id: &str) -> Result<Option<SessionRevertState>> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        let metadata_json = conn
            .query_row(
                "SELECT metadata_json FROM sessions WHERE id = ?1",
                params![session_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .ok_or_else(|| Error::Message(format!("session not found: {session_id}")))?;
        parse_session_revert(metadata_json.as_deref())
    }

    pub fn set_session_revert_state(
        &self,
        session_id: &str,
        revert: SessionRevertState,
    ) -> Result<()> {
        let changed = self.write_retry(|conn| {
            let metadata_json = session_metadata_json(conn, session_id)?;
            let mut metadata = metadata_object_sql(metadata_json.as_deref())?;
            metadata.insert(
                SESSION_REVERT_METADATA_KEY.to_string(),
                json!({
                    "start_seq": revert.start_seq,
                    "original_snapshot": revert.original_snapshot,
                }),
            );
            let metadata_json =
                serde_json::to_string(&Value::Object(metadata)).map_err(json_to_sql)?;
            conn.execute(
                "UPDATE sessions SET metadata_json = ?1, updated_at_ms = ?2 WHERE id = ?3",
                params![metadata_json, now_ms(), session_id],
            )
        })?;
        if changed == 0 {
            return Err(Error::Message(format!("session not found: {session_id}")));
        }
        Ok(())
    }

    pub fn clear_session_revert_state(&self, session_id: &str) -> Result<()> {
        let changed = self.write_retry(|conn| {
            let metadata_json = session_metadata_json(conn, session_id)?;
            let mut metadata = metadata_object_sql(metadata_json.as_deref())?;
            metadata.remove(SESSION_REVERT_METADATA_KEY);
            let metadata_json = metadata_json_sql(metadata)?;
            conn.execute(
                "UPDATE sessions SET metadata_json = ?1, updated_at_ms = ?2 WHERE id = ?3",
                params![metadata_json, now_ms(), session_id],
            )
        })?;
        if changed == 0 {
            return Err(Error::Message(format!("session not found: {session_id}")));
        }
        Ok(())
    }

    pub fn latest_undo_target(&self, session_id: &str) -> Result<Option<UndoTarget>> {
        let boundary = self
            .session_revert_state(session_id)?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        self.user_target_before(session_id, boundary)
    }

    pub fn next_redo_target(&self, session_id: &str) -> Result<Option<UndoTarget>> {
        let Some(revert) = self.session_revert_state(session_id)? else {
            return Ok(None);
        };
        self.user_target_after(session_id, revert.start_seq)
    }

    pub fn messages_from_count(&self, session_id: &str, start_seq: i64) -> Result<usize> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE session_id = ?1 AND session_seq >= ?2",
            params![session_id, start_seq],
            |row| row.get(0),
        )?;
        Ok(count.max(0) as usize)
    }

    pub fn cleanup_reverted_messages(&self, session_id: &str) -> Result<usize> {
        self.write_retry(|conn| {
            let metadata_json = session_metadata_json(conn, session_id)?;
            let Some(revert) = parse_session_revert_sql(metadata_json.as_deref())? else {
                return Ok(0);
            };
            let removed = conn.execute(
                "DELETE FROM messages WHERE session_id = ?1 AND session_seq >= ?2",
                params![session_id, revert.start_seq],
            )?;
            let mut metadata = metadata_object_sql(metadata_json.as_deref())?;
            metadata.remove(SESSION_REVERT_METADATA_KEY);
            let metadata_json = metadata_json_sql(metadata)?;
            let message_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )?;
            let tool_call_count = session_tool_call_count(conn, session_id)?;
            conn.execute(
                r#"
                UPDATE sessions
                SET metadata_json = ?1,
                    message_count = ?2,
                    tool_call_count = ?3,
                    updated_at_ms = ?4
                WHERE id = ?5
                "#,
                params![
                    metadata_json,
                    message_count,
                    tool_call_count,
                    now_ms(),
                    session_id
                ],
            )?;
            Ok(removed)
        })
    }

    pub fn resume_session(&self, session_id: &str) -> Result<()> {
        let now = now_ms();
        let changed = self.write_retry(|conn| {
            conn.execute(
                "UPDATE sessions SET updated_at_ms = ?1, ended_at_ms = NULL, end_reason = NULL WHERE id = ?2",
                params![now, session_id],
            )
        })?;
        if changed == 0 {
            return Err(Error::Message(format!("session not found: {session_id}")));
        }
        Ok(())
    }

    pub fn load_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            "SELECT message_json FROM messages WHERE session_id = ?1 ORDER BY session_seq ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| row.get::<_, String>(0))?;
        let mut messages = Vec::new();
        for row in rows {
            messages.push(serde_json::from_str(&row?)?);
        }
        Ok(messages)
    }

    pub fn load_sanitized_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        Ok(self
            .load_messages(session_id)?
            .iter()
            .map(sanitize_message_for_output)
            .collect())
    }

    pub fn load_sanitized_message_summaries(
        &self,
        session_id: &str,
    ) -> Result<Vec<SanitizedMessageSummary>> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT message_json, usage_json, metadata_json
            FROM messages
            WHERE session_id = ?1
            ORDER BY session_seq ASC
            "#,
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut messages = Vec::new();
        for row in rows {
            let (message_json, usage_json, metadata_json) = row?;
            let message = serde_json::from_str::<Message>(&message_json)?;
            let usage = parse_optional_json(usage_json)?;
            let metadata = parse_optional_json(metadata_json)?;
            messages.push(SanitizedMessageSummary {
                message: sanitize_message_for_output(&message),
                usage,
                metadata,
            });
        }
        Ok(messages)
    }

    pub fn load_tui_message_summaries(&self, session_id: &str) -> Result<Vec<TuiMessageSummary>> {
        let boundary = self
            .session_revert_state(session_id)?
            .map(|revert| revert.start_seq)
            .unwrap_or(i64::MAX);
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT message_json, usage_json, metadata_json
            FROM messages
            WHERE session_id = ?1 AND session_seq < ?2
            ORDER BY session_seq ASC
            "#,
        )?;
        let rows = stmt.query_map(params![session_id, boundary], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut messages = Vec::new();
        for row in rows {
            let (message_json, usage_json, metadata_json) = row?;
            let message = serde_json::from_str::<Message>(&message_json)?;
            let usage = parse_optional_json(usage_json)?;
            let metadata = parse_optional_json(metadata_json)?;
            messages.push(TuiMessageSummary {
                message: sanitize_message_for_tui_history(&message),
                usage,
                metadata,
            });
        }
        Ok(messages)
    }

    pub fn append_message(&self, session_id: &str, message: &Message) -> Result<()> {
        self.append_message_with_metrics(session_id, message, None, None)
    }

    pub fn append_message_with_undo_snapshot(
        &self,
        session_id: &str,
        message: &Message,
        snapshot: Option<String>,
    ) -> Result<()> {
        let metadata = snapshot.map(|snapshot| {
            json!({
                MESSAGE_UNDO_METADATA_KEY: {
                    MESSAGE_PRE_SNAPSHOT_KEY: snapshot
                }
            })
        });
        self.append_message_with_metrics(session_id, message, None, metadata)
    }

    pub fn append_message_with_metrics(
        &self,
        session_id: &str,
        message: &Message,
        usage: Option<Value>,
        metadata: Option<Value>,
    ) -> Result<()> {
        let fields = message_fields(message)?;
        let message_json = serde_json::to_string(message)?;
        let usage_json = optional_json_string(&usage)?;
        let metadata_json = optional_json_string(&metadata)?;
        let now = now_ms();
        self.write_retry(|conn| {
            let seq = next_session_seq(conn, session_id)?;
            conn.execute(
                r#"
                INSERT INTO messages (
                    session_id, session_seq, role, timestamp_ms, message_json,
                    content_text, tool_call_id, tool_name, tool_calls_json,
                    finish_reason, outcome, model, provider, usage_json, metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                "#,
                params![
                    session_id,
                    seq,
                    fields.role,
                    fields.timestamp_ms,
                    message_json,
                    fields.content_text,
                    fields.tool_call_id,
                    fields.tool_name,
                    fields.tool_calls_json,
                    fields.finish_reason,
                    fields.outcome,
                    fields.model,
                    fields.provider,
                    usage_json,
                    metadata_json
                ],
            )?;
            conn.execute(
                r#"
                UPDATE sessions
                SET updated_at_ms = ?1,
                    message_count = message_count + 1,
                    tool_call_count = tool_call_count + ?2
                WHERE id = ?3
                "#,
                params![now, fields.tool_call_count, session_id],
            )?;
            Ok(())
        })
    }

    pub fn touch_session(&self, session_id: &str) -> Result<()> {
        let now = now_ms();
        self.write_retry(|conn| {
            conn.execute(
                "UPDATE sessions SET updated_at_ms = ?1 WHERE id = ?2",
                params![now, session_id],
            )?;
            Ok(())
        })
    }

    pub fn finish_session(&self, session_id: &str, outcome: Outcome) -> Result<()> {
        let now = now_ms();
        self.write_retry(|conn| {
            conn.execute(
                "UPDATE sessions SET updated_at_ms = ?1, ended_at_ms = ?1, end_reason = ?2 WHERE id = ?3",
                params![now, outcome.as_str(), session_id],
            )?;
            Ok(())
        })
    }

    fn user_target_before(&self, session_id: &str, boundary: i64) -> Result<Option<UndoTarget>> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT session_seq, message_json, content_text, metadata_json
            FROM messages
            WHERE session_id = ?1 AND role = 'user' AND session_seq < ?2
            ORDER BY session_seq DESC
            LIMIT 1
            "#,
        )?;
        stmt.query_row(params![session_id, boundary], undo_target_from_row)
            .optional()
            .map_err(Into::into)
    }

    fn user_target_after(&self, session_id: &str, boundary: i64) -> Result<Option<UndoTarget>> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT session_seq, message_json, content_text, metadata_json
            FROM messages
            WHERE session_id = ?1 AND role = 'user' AND session_seq > ?2
            ORDER BY session_seq ASC
            LIMIT 1
            "#,
        )?;
        stmt.query_row(params![session_id, boundary], undo_target_from_row)
            .optional()
            .map_err(Into::into)
    }

    fn write_retry<T>(&self, mut f: impl FnMut(&Connection) -> rusqlite::Result<T>) -> Result<T> {
        let mut last = None;
        for attempt in 0..8 {
            let conn = self.conn.lock().expect("sqlite lock poisoned");
            let tx_result = (|| {
                conn.execute_batch("BEGIN IMMEDIATE")?;
                match f(&conn) {
                    Ok(value) => {
                        conn.execute_batch("COMMIT")?;
                        Ok(value)
                    }
                    Err(err) => {
                        let _ = conn.execute_batch("ROLLBACK");
                        Err(err)
                    }
                }
            })();
            drop(conn);
            match tx_result {
                Ok(value) => {
                    if attempt % 4 == 0
                        && let Ok(conn) = self.conn.lock()
                    {
                        let _ = conn.pragma_update(None, "wal_checkpoint", "PASSIVE");
                    }
                    return Ok(value);
                }
                Err(err) if is_busy(&err) && attempt < 7 => {
                    last = Some(err);
                    thread::sleep(Duration::from_millis(20 + (attempt as u64 * 17)));
                }
                Err(err) => return Err(err.into()),
            }
        }
        Err(last
            .unwrap_or(rusqlite::Error::ExecuteReturnedResults)
            .into())
    }
}

fn sqlite_table_exists(conn: &Connection, table: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
        params![table],
        |_| Ok(()),
    )
    .optional()
    .map(|value| value.is_some())
}

fn is_busy(err: &rusqlite::Error) -> bool {
    let msg = err.to_string().to_lowercase();
    msg.contains("busy") || msg.contains("locked")
}

fn session_summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionSummary> {
    Ok(SessionSummary {
        id: row.get(0)?,
        source: row.get(1)?,
        workdir: row.get(2)?,
        model: row.get(3)?,
        provider: row.get(4)?,
        started_at_ms: row.get(5)?,
        updated_at_ms: row.get(6)?,
        ended_at_ms: row.get(7)?,
        end_reason: row.get(8)?,
        message_count: row.get(9)?,
        tool_call_count: row.get(10)?,
        title: row.get(11)?,
    })
}

fn next_session_seq(conn: &Connection, session_id: &str) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COALESCE(MAX(session_seq), 0) + 1 FROM messages WHERE session_id = ?1",
        params![session_id],
        |row| row.get(0),
    )
}

#[derive(Debug)]
struct MessageFields {
    role: String,
    timestamp_ms: i64,
    content_text: Option<String>,
    tool_call_id: Option<String>,
    tool_name: Option<String>,
    tool_calls_json: Option<String>,
    finish_reason: Option<String>,
    outcome: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    tool_call_count: i64,
}

fn message_fields(message: &Message) -> Result<MessageFields> {
    match message {
        Message::User {
            content,
            timestamp_ms,
        } => Ok(MessageFields {
            role: "user".to_string(),
            timestamp_ms: *timestamp_ms,
            content_text: Some(
                content
                    .iter()
                    .map(|block| block.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            tool_call_id: None,
            tool_name: None,
            tool_calls_json: None,
            finish_reason: None,
            outcome: None,
            model: None,
            provider: None,
            tool_call_count: 0,
        }),
        Message::Assistant {
            content,
            timestamp_ms,
            finish_reason,
            outcome,
            model,
            provider,
        } => {
            let text = content
                .iter()
                .filter_map(|block| match block {
                    AssistantBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            let tool_calls = content
                .iter()
                .filter_map(|block| match block {
                    AssistantBlock::ToolCall(call) => Some(call),
                    _ => None,
                })
                .collect::<Vec<_>>();
            Ok(MessageFields {
                role: "assistant".to_string(),
                timestamp_ms: *timestamp_ms,
                content_text: if text.is_empty() { None } else { Some(text) },
                tool_call_id: None,
                tool_name: None,
                tool_calls_json: if tool_calls.is_empty() {
                    None
                } else {
                    Some(serde_json::to_string(&tool_calls)?)
                },
                finish_reason: finish_reason.clone(),
                outcome: Some(outcome.as_str().to_string()),
                model: model.clone(),
                provider: provider.clone(),
                tool_call_count: tool_calls.len() as i64,
            })
        }
        Message::ToolResult {
            tool_call_id,
            tool_name,
            content,
            is_error,
            timestamp_ms,
        } => Ok(MessageFields {
            role: "tool_result".to_string(),
            timestamp_ms: *timestamp_ms,
            content_text: Some(content.clone()),
            tool_call_id: Some(tool_call_id.clone()),
            tool_name: Some(tool_name.clone()),
            tool_calls_json: None,
            finish_reason: None,
            outcome: Some(if *is_error { "failed" } else { "normal" }.to_string()),
            model: None,
            provider: None,
            tool_call_count: 0,
        }),
    }
}

fn optional_json_string(value: &Option<Value>) -> Result<Option<String>> {
    value
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(Into::into)
}

fn parse_optional_json(value: Option<String>) -> Result<Option<Value>> {
    value
        .map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(Into::into)
}

fn session_metadata_json(conn: &Connection, session_id: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT metadata_json FROM sessions WHERE id = ?1",
        params![session_id],
        |row| row.get::<_, Option<String>>(0),
    )
}

fn metadata_object(value: Option<&str>) -> Result<Map<String, Value>> {
    let Some(value) = value else {
        return Ok(Map::new());
    };
    let parsed = serde_json::from_str::<Value>(value)?;
    Ok(parsed.as_object().cloned().unwrap_or_default())
}

fn metadata_object_sql(value: Option<&str>) -> rusqlite::Result<Map<String, Value>> {
    let Some(value) = value else {
        return Ok(Map::new());
    };
    let parsed = serde_json::from_str::<Value>(value).map_err(json_to_sql)?;
    Ok(parsed.as_object().cloned().unwrap_or_default())
}

fn metadata_json_sql(metadata: Map<String, Value>) -> rusqlite::Result<Option<String>> {
    (!metadata.is_empty())
        .then(|| serde_json::to_string(&Value::Object(metadata)).map_err(json_to_sql))
        .transpose()
}

fn json_to_sql(err: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(err))
}

fn parse_session_revert(value: Option<&str>) -> Result<Option<SessionRevertState>> {
    let metadata = metadata_object(value)?;
    parse_session_revert_from_metadata(&metadata).map_err(Into::into)
}

fn parse_session_revert_sql(value: Option<&str>) -> rusqlite::Result<Option<SessionRevertState>> {
    let metadata = metadata_object_sql(value)?;
    parse_session_revert_from_metadata(&metadata)
}

fn parse_session_revert_from_metadata(
    metadata: &Map<String, Value>,
) -> rusqlite::Result<Option<SessionRevertState>> {
    let Some(revert) = metadata
        .get(SESSION_REVERT_METADATA_KEY)
        .and_then(Value::as_object)
    else {
        return Ok(None);
    };
    let Some(start_seq) = revert.get("start_seq").and_then(Value::as_i64) else {
        return Ok(None);
    };
    let Some(original_snapshot) = revert
        .get("original_snapshot")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };
    Ok(Some(SessionRevertState {
        start_seq,
        original_snapshot,
    }))
}

fn undo_target_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UndoTarget> {
    let seq = row.get::<_, i64>(0)?;
    let message_json = row.get::<_, String>(1)?;
    let content_text = row.get::<_, Option<String>>(2)?;
    let metadata_json = row.get::<_, Option<String>>(3)?;
    let prompt = content_text
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| user_prompt_from_message_json(&message_json).unwrap_or_default());
    Ok(UndoTarget {
        seq,
        prompt,
        snapshot: undo_snapshot_from_metadata(metadata_json.as_deref()),
    })
}

fn undo_snapshot_from_metadata(value: Option<&str>) -> Option<String> {
    let metadata = metadata_object(value).ok()?;
    metadata
        .get(MESSAGE_UNDO_METADATA_KEY)
        .and_then(Value::as_object)
        .and_then(|undo| undo.get(MESSAGE_PRE_SNAPSHOT_KEY))
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
}

fn user_prompt_from_message_json(value: &str) -> Option<String> {
    let message = serde_json::from_str::<Message>(value).ok()?;
    let Message::User { content, .. } = message else {
        return None;
    };
    Some(
        content
            .iter()
            .map(|block| block.text.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn session_tool_call_count(conn: &Connection, session_id: &str) -> rusqlite::Result<i64> {
    let mut stmt =
        conn.prepare("SELECT tool_calls_json FROM messages WHERE session_id = ?1 AND tool_calls_json IS NOT NULL")?;
    let rows = stmt.query_map(params![session_id], |row| row.get::<_, String>(0))?;
    let mut count = 0i64;
    for row in rows {
        let value = row?;
        if let Ok(tool_calls) = serde_json::from_str::<Vec<Value>>(&value) {
            count += tool_calls.len() as i64;
        }
    }
    Ok(count)
}
