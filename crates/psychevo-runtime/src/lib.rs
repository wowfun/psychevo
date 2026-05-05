use std::collections::{BTreeMap, HashSet, VecDeque};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use futures::future::BoxFuture;
use psychevo_agent_core::{
    AgentEvent, AgentLoopRequest, AssistantBlock, ControlHandle, ControlReceivers, EventSink,
    Message, Result as CoreResult, ToolBinding, ToolExecutionMode, ToolOutput, now_ms,
    run_agent_loop, user_text_message,
};
use psychevo_ai::{AbortSignal, FakeProvider, OpenAiChatProvider, Outcome, RawStreamEvent};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use similar::TextDiff;
use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time;
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, Error>;

const SMOKE_DIR: &str = ".psychevo-smoke";
const SMOKE_SUBJECT: &str = ".psychevo-smoke/subject.txt";
const SMOKE_GENERATED: &str = ".psychevo-smoke/generated.txt";
const SMOKE_MANIFEST: &str = ".psychevo-smoke/manifest.json";
const READ_MAX_BYTES: usize = 50 * 1024;
const READ_MAX_LINES: usize = 2000;
const BASH_DEFAULT_TIMEOUT_SECS: u64 = 120;
const BASH_MAX_TIMEOUT_SECS: u64 = 300;
const SQLITE_SCHEMA_VERSION: i64 = 3;
const REASONING_EFFORT_VALUES: &[&str] =
    &["none", "minimal", "low", "medium", "high", "xhigh", "max"];

#[derive(Debug, Error)]
pub enum Error {
    #[error("sqlite failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("json failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("agent failed: {0}")]
    Agent(#[from] psychevo_agent_core::Error),
    #[error("config failed: {0}")]
    Config(String),
    #[error("{0}")]
    Message(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SmokeControl {
    #[default]
    None,
    StopAfterTurn,
    AbortOnAgentStart,
}

#[derive(Debug, Clone)]
pub struct SmokeOptions {
    pub db_path: PathBuf,
    pub workdir: PathBuf,
    pub session: Option<String>,
    pub prompt: Option<String>,
    pub max_context_messages: Option<usize>,
    pub control: SmokeControl,
    pub reset: bool,
}

#[derive(Debug, Clone)]
pub struct SmokeResult {
    pub session_id: String,
    pub outcome: Outcome,
    pub final_answer: String,
    pub db_path: PathBuf,
    pub workdir: PathBuf,
    pub tool_failures: usize,
    pub expected_control_outcome: Option<Outcome>,
}

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub db_path: PathBuf,
    pub workdir: PathBuf,
    pub session: Option<String>,
    pub continue_latest: bool,
    pub prompt: String,
    pub max_context_messages: Option<usize>,
    pub config_path: Option<PathBuf>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub include_reasoning: bool,
    pub mode: RunMode,
    pub inherited_env: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RunMode {
    Plan,
    #[default]
    Build,
}

impl RunMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Build => "build",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "plan" => Some(Self::Plan),
            "build" => Some(Self::Build),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunResult {
    pub session_id: String,
    pub outcome: Outcome,
    pub final_answer: String,
    pub db_path: PathBuf,
    pub workdir: PathBuf,
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub api_key_env: Option<String>,
    pub reasoning_effort: Option<String>,
    pub context_limit: Option<u64>,
    pub tool_failures: usize,
    pub events: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub id: String,
    pub source: String,
    pub workdir: String,
    pub model: String,
    pub provider: String,
    pub started_at_ms: i64,
    pub updated_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub end_reason: Option<String>,
    pub message_count: i64,
    pub tool_call_count: i64,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredModel {
    pub provider: String,
    pub provider_label: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub context_limit: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SanitizedMessageSummary {
    pub message: Message,
    pub usage: Option<Value>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RunStreamEvent {
    Event(Value),
    ReasoningDelta { text: String },
    ReasoningEnd,
}

pub type RunStreamSink = Arc<dyn Fn(RunStreamEvent) + Send + Sync>;

#[derive(Clone)]
pub struct RunControlHandle {
    inner: ControlHandle,
}

impl RunControlHandle {
    pub fn stop(&self) {
        self.inner.stop();
    }

    pub fn abort(&self) {
        self.inner.abort();
    }
}

pub struct RunControl {
    handle: RunControlHandle,
    receivers: ControlReceivers,
}

pub fn run_control() -> (RunControlHandle, RunControl) {
    let (inner, receivers) = ControlHandle::new();
    let handle = RunControlHandle { inner };
    (handle.clone(), RunControl { handle, receivers })
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

    pub fn append_message(&self, session_id: &str, message: &Message) -> Result<()> {
        self.append_message_with_metrics(session_id, message, None, None)
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

#[derive(Debug, Clone, Default)]
struct RunConfig {
    model: ModelSelection,
    provider: BTreeMap<String, ConfigProviderEntry>,
}

#[derive(Debug, Clone, Default)]
struct ModelSelection {
    id: Option<String>,
    provider: Option<String>,
    reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ConfigProviderEntry {
    options: ConfigProviderOptions,
    models: BTreeMap<String, ConfigModelEntry>,
}

#[derive(Debug, Clone, Default)]
struct ConfigProviderOptions {
    base_url: Option<String>,
    api_key_env: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ConfigModelEntry {
    reasoning_effort: Option<String>,
    context_limit: Option<u64>,
}

#[derive(Debug, Clone)]
struct BuiltInProvider {
    id: &'static str,
    label: &'static str,
    base_url: Option<&'static str>,
    api_key_envs: &'static [&'static str],
    base_url_env: Option<&'static str>,
    allow_no_auth: bool,
}

#[derive(Debug, Clone)]
struct ResolvedRunProvider {
    provider: String,
    display_label: String,
    model: String,
    base_url: String,
    api_key_env: Option<String>,
    api_key: String,
    reasoning_effort: Option<String>,
    context_limit: Option<u64>,
}

#[derive(Debug, Clone)]
struct LoadedRunConfig {
    config: RunConfig,
    env: BTreeMap<String, String>,
}

const AUTO_PROVIDER_ORDER: &[&str] = &[
    "openrouter",
    "openai",
    "xai",
    "zai",
    "deepseek",
    "dashscope",
    "xiaomi",
    "lmstudio",
    "custom",
];

const BUILT_IN_PROVIDERS: &[BuiltInProvider] = &[
    BuiltInProvider {
        id: "openrouter",
        label: "OpenRouter",
        base_url: Some("https://openrouter.ai/api/v1"),
        api_key_envs: &["OPENROUTER_API_KEY", "OPENAI_API_KEY"],
        base_url_env: Some("OPENROUTER_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "openai",
        label: "OpenAI",
        base_url: Some("https://api.openai.com/v1"),
        api_key_envs: &["OPENAI_API_KEY"],
        base_url_env: Some("OPENAI_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "xai",
        label: "xAI",
        base_url: Some("https://api.x.ai/v1"),
        api_key_envs: &["XAI_API_KEY"],
        base_url_env: Some("XAI_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "zai",
        label: "Z.AI / GLM",
        base_url: Some("https://api.z.ai/api/paas/v4"),
        api_key_envs: &["GLM_API_KEY", "ZAI_API_KEY", "Z_AI_API_KEY"],
        base_url_env: Some("GLM_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "deepseek",
        label: "DeepSeek",
        base_url: Some("https://api.deepseek.com/v1"),
        api_key_envs: &["DEEPSEEK_API_KEY"],
        base_url_env: Some("DEEPSEEK_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "dashscope",
        label: "Alibaba Cloud DashScope",
        base_url: Some("https://dashscope-intl.aliyuncs.com/compatible-mode/v1"),
        api_key_envs: &["DASHSCOPE_API_KEY"],
        base_url_env: Some("DASHSCOPE_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "xiaomi",
        label: "Xiaomi MiMo",
        base_url: Some("https://api.xiaomimimo.com/v1"),
        api_key_envs: &["XIAOMI_API_KEY"],
        base_url_env: Some("XIAOMI_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "lmstudio",
        label: "LM Studio",
        base_url: Some("http://127.0.0.1:1234/v1"),
        api_key_envs: &["LM_API_KEY"],
        base_url_env: Some("LM_BASE_URL"),
        allow_no_auth: true,
    },
    BuiltInProvider {
        id: "custom",
        label: "Custom",
        base_url: None,
        api_key_envs: &[],
        base_url_env: None,
        allow_no_auth: false,
    },
];

pub async fn run_live(options: RunOptions) -> Result<RunResult> {
    run_live_internal(options, "run", &["run"], None, None).await
}

pub async fn run_live_streaming(
    options: RunOptions,
    source: &str,
    continue_sources: &[&str],
    stream: RunStreamSink,
) -> Result<RunResult> {
    run_live_internal(options, source, continue_sources, Some(stream), None).await
}

pub async fn run_live_streaming_controlled(
    options: RunOptions,
    source: &str,
    continue_sources: &[&str],
    stream: RunStreamSink,
    control: RunControl,
) -> Result<RunResult> {
    run_live_internal(
        options,
        source,
        continue_sources,
        Some(stream),
        Some(control),
    )
    .await
}

async fn run_live_internal(
    options: RunOptions,
    source: &str,
    continue_sources: &[&str],
    stream_events: Option<RunStreamSink>,
    control: Option<RunControl>,
) -> Result<RunResult> {
    let workdir = canonical_workdir(&options.workdir)?;
    if options.prompt.trim().is_empty() {
        return Err(Error::Message("prompt is empty".to_string()));
    }

    let loaded = load_run_config(&options, &workdir)?;
    let resolved = resolve_run_provider(&options, &loaded)?;
    let store = SqliteStore::open(&options.db_path)?;
    let session_id = if let Some(session_id) = options.session.clone() {
        store.resume_session(&session_id)?;
        session_id
    } else if options.continue_latest {
        if let Some(session_id) =
            store.latest_session_for_workdir_with_sources(&workdir, continue_sources)?
        {
            store.resume_session(&session_id)?;
            session_id
        } else {
            store.create_session_with_metadata(
                &workdir,
                source,
                &resolved.model,
                &resolved.provider,
                Some(json!({
                    "provider_label": resolved.display_label.clone(),
                    "base_url": resolved.base_url.clone(),
                    "api_key_env": resolved.api_key_env.clone(),
                    "reasoning_effort": resolved.reasoning_effort.clone(),
                    "context_limit": resolved.context_limit,
                    "mode": options.mode.as_str(),
                })),
            )?
        }
    } else {
        store.create_session_with_metadata(
            &workdir,
            source,
            &resolved.model,
            &resolved.provider,
            Some(json!({
                "provider_label": resolved.display_label.clone(),
                "base_url": resolved.base_url.clone(),
                "api_key_env": resolved.api_key_env.clone(),
                "reasoning_effort": resolved.reasoning_effort.clone(),
                "context_limit": resolved.context_limit,
                "mode": options.mode.as_str(),
            })),
        )?
    };

    let run_start = json!({
        "type": "run_start",
        "source": source,
        "session_id": session_id.clone(),
        "provider": resolved.provider.clone(),
        "model": resolved.model.clone(),
        "db": options.db_path.clone(),
        "workdir": workdir.clone(),
        "base_url": resolved.base_url.clone(),
        "api_key_env": resolved.api_key_env.clone(),
        "reasoning_effort": resolved.reasoning_effort.clone(),
        "context_limit": resolved.context_limit,
        "mode": options.mode.as_str(),
    });
    if let Some(stream) = &stream_events {
        stream(RunStreamEvent::Event(run_start.clone()));
    }
    let events = Arc::new(Mutex::new(vec![run_start]));

    let previous_messages = prune_context(
        store.load_messages(&session_id)?,
        options.max_context_messages,
    );
    let provider = Arc::new(OpenAiChatProvider::new(
        resolved.base_url.clone(),
        resolved.api_key.clone(),
        resolved.provider.clone(),
    ));
    let (control_handle, control_receivers) = match control {
        Some(control) => (control.handle.inner.clone(), control.receivers),
        None => ControlHandle::new(),
    };
    let sink = Arc::new(PersistenceSink {
        store: store.clone(),
        session_id: session_id.clone(),
        control: SmokeControl::None,
        control_handle: Some(control_handle),
        events: Some(Arc::clone(&events)),
        stream_events,
        include_reasoning: options.include_reasoning,
    });
    let generation_metadata = resolved
        .reasoning_effort
        .as_ref()
        .map(|effort| json!({ "reasoning_effort": effort }))
        .unwrap_or_else(|| json!({}));
    let request = AgentLoopRequest {
        model_provider: resolved.provider.clone(),
        model: resolved.model.clone(),
        generation_metadata,
        system_instructions: vec![mode_instruction(options.mode).to_string()],
        previous_messages,
        prompt_messages: vec![user_text_message(options.prompt.clone())],
        tools: coding_core_tools_for_mode(&workdir, options.mode),
        max_turns: 8,
    };
    let completion = run_agent_loop(provider, request, sink, control_receivers).await?;
    let final_answer = completion
        .messages
        .iter()
        .rev()
        .find_map(assistant_text)
        .unwrap_or_default();
    let tool_failures = completion
        .messages
        .iter()
        .filter(|message| matches!(message, Message::ToolResult { is_error: true, .. }))
        .count();

    let events = events.lock().expect("event lock poisoned").clone();
    Ok(RunResult {
        session_id,
        outcome: completion.outcome,
        final_answer,
        db_path: options.db_path,
        workdir,
        provider: resolved.provider,
        model: resolved.model,
        base_url: resolved.base_url,
        api_key_env: resolved.api_key_env,
        reasoning_effort: resolved.reasoning_effort,
        context_limit: resolved.context_limit,
        tool_failures,
        events,
    })
}

fn load_run_config(options: &RunOptions, workdir: &Path) -> Result<LoadedRunConfig> {
    let mut env_map = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| env::vars().collect());
    let project_dir = workdir.join(".psychevo");
    let mut value = json!({});

    if let Some(config_path) = resolve_config_path(options, &env_map)? {
        let loaded = load_jsonc_config_file(&config_path, true)?;
        deep_merge(&mut value, loaded);
        if let Some(parent) = config_path.parent() {
            load_dotenv_file(&parent.join(".env"), &mut env_map)?;
        }
    } else {
        let home = resolve_psychevo_home(&env_map)?;
        let home_config = home.join("config.jsonc");
        if !home_config.exists() {
            return Err(Error::Config(format!(
                "Psychevo home is not initialized; run `pevo init` to create {}",
                home_config.display()
            )));
        }
        let loaded = load_jsonc_config_file(&home_config, true)?;
        deep_merge(&mut value, loaded);
        load_dotenv_file(&home.join(".env"), &mut env_map)?;
        let loaded = load_jsonc_config_file(&project_dir.join("config.jsonc"), false)?;
        deep_merge(&mut value, loaded);
    }

    load_dotenv_file(&project_dir.join(".env"), &mut env_map)?;
    Ok(LoadedRunConfig {
        config: parse_run_config(value)?,
        env: env_map,
    })
}

fn resolve_config_path(
    options: &RunOptions,
    env_map: &BTreeMap<String, String>,
) -> Result<Option<PathBuf>> {
    if let Some(path) = &options.config_path {
        return Ok(Some(resolve_explicit_path(path, env_map)?));
    }
    env_map
        .get("PSYCHEVO_CONFIG")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| resolve_explicit_path(Path::new(value), env_map))
        .transpose()
}

fn resolve_psychevo_home(env_map: &BTreeMap<String, String>) -> Result<PathBuf> {
    if let Some(value) = env_map
        .get("PSYCHEVO_HOME")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        resolve_explicit_path(Path::new(value), env_map)
    } else {
        resolve_explicit_path(Path::new("~/.psychevo"), env_map)
    }
}

fn resolve_explicit_path(path: &Path, env_map: &BTreeMap<String, String>) -> Result<PathBuf> {
    let expanded = expand_tilde(path, env_map)?;
    if expanded.is_absolute() {
        Ok(expanded)
    } else {
        Ok(env::current_dir()?.join(expanded))
    }
}

fn expand_tilde(path: &Path, env_map: &BTreeMap<String, String>) -> Result<PathBuf> {
    let raw = path.to_string_lossy();
    if raw == "~" {
        return home_path(env_map);
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return Ok(home_path(env_map)?.join(rest));
    }
    Ok(path.to_path_buf())
}

fn home_path(env_map: &BTreeMap<String, String>) -> Result<PathBuf> {
    env_map
        .get("HOME")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| Error::Config("HOME is required to expand ~".to_string()))
}

fn load_jsonc_config_file(path: &Path, required: bool) -> Result<Value> {
    if !path.exists() {
        if required {
            return Err(Error::Config(format!(
                "config file not found: {}",
                path.display()
            )));
        }
        return Ok(json!({}));
    }
    let text = fs::read_to_string(path)?;
    let parsed: Option<Value> = jsonc_parser::parse_to_serde_value(&text, &Default::default())
        .map_err(|err| Error::Config(format!("{}: {err}", path.display())))?;
    let value = parsed.unwrap_or_else(|| json!({}));
    if !value.is_object() {
        return Err(Error::Config(format!(
            "{} must contain a JSON object",
            path.display()
        )));
    }
    Ok(value)
}

fn deep_merge(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                if let Some(existing) = base.get_mut(&key) {
                    deep_merge(existing, value);
                } else {
                    base.insert(key, value);
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

fn load_dotenv_file(path: &Path, env_map: &mut BTreeMap<String, String>) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let text = fs::read_to_string(path)?;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if !valid_env_name(name) {
            continue;
        }
        env_map.insert(name.to_string(), strip_env_quotes(value.trim()).to_string());
    }
    Ok(())
}

fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('_' | 'A'..='Z' | 'a'..='z'))
        && chars.all(|ch| matches!(ch, '_' | 'A'..='Z' | 'a'..='z' | '0'..='9'))
}

fn strip_env_quotes(value: &str) -> &str {
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn parse_run_config(value: Value) -> Result<RunConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("config root must be an object".to_string()))?;
    let mut config = RunConfig::default();
    let configured_keys = object
        .get("provider")
        .and_then(Value::as_object)
        .map(|providers| {
            providers
                .keys()
                .map(|key| normalize_provider_id(key))
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();

    if let Some(model) = object.get("model") {
        config.model = parse_model_selection(model, &configured_keys)?;
    }
    if let Some(providers) = object.get("provider") {
        let providers = providers
            .as_object()
            .ok_or_else(|| Error::Config("provider must be an object".to_string()))?;
        for (key, entry) in providers {
            let provider_id = normalize_provider_id(key);
            config
                .provider
                .insert(provider_id, parse_config_provider_entry(key, entry)?);
        }
    }
    Ok(config)
}

fn parse_model_selection(
    value: &Value,
    configured_keys: &HashSet<String>,
) -> Result<ModelSelection> {
    match value {
        Value::String(raw) => Ok(model_selection_from_raw(raw, configured_keys, None, None)),
        Value::Object(object) => {
            let id = optional_string_field(object, "id")?;
            let provider = optional_string_field(object, "provider")?
                .map(|provider| normalize_provider_id(&provider));
            let reasoning_effort =
                validate_reasoning_effort(optional_string_field(object, "reasoning_effort")?)?;
            if let Some(id) = id {
                Ok(model_selection_from_raw(
                    &id,
                    configured_keys,
                    provider,
                    reasoning_effort,
                ))
            } else {
                Err(Error::Config("model object requires id".to_string()))
            }
        }
        _ => Err(Error::Config(
            "model must be a string or object".to_string(),
        )),
    }
}

fn parse_config_provider_entry(name: &str, value: &Value) -> Result<ConfigProviderEntry> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config(format!("provider.{name} must be an object")))?;
    let mut entry = ConfigProviderEntry::default();
    if let Some(options) = object.get("options") {
        let options = options
            .as_object()
            .ok_or_else(|| Error::Config(format!("provider.{name}.options must be an object")))?;
        if options.contains_key("api_key") || options.contains_key("apiKey") {
            return Err(Error::Config(format!(
                "provider.{name}.options must not contain raw API keys"
            )));
        }
        entry.options.base_url = optional_string_field(options, "base_url")?;
        entry.options.api_key_env = optional_string_field(options, "api_key_env")?;
    }
    if let Some(models) = object.get("models") {
        let models = models
            .as_object()
            .ok_or_else(|| Error::Config(format!("provider.{name}.models must be an object")))?;
        for (model_id, model_value) in models {
            entry.models.insert(
                model_id.clone(),
                parse_config_model_entry(name, model_id, model_value)?,
            );
        }
    }
    Ok(entry)
}

fn parse_config_model_entry(
    provider_name: &str,
    model_id: &str,
    value: &Value,
) -> Result<ConfigModelEntry> {
    if value.is_null() {
        return Ok(ConfigModelEntry::default());
    }
    let object = value.as_object().ok_or_else(|| {
        Error::Config(format!(
            "provider.{provider_name}.models.{model_id} must be an object"
        ))
    })?;
    Ok(ConfigModelEntry {
        reasoning_effort: validate_reasoning_effort(optional_string_field(
            object,
            "reasoning_effort",
        )?)?,
        context_limit: optional_u64_field(object, "context_limit")?,
    })
}

fn optional_string_field(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<String>> {
    object
        .get(key)
        .map(|value| {
            value
                .as_str()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| Error::Config(format!("{key} must be a non-empty string")))
        })
        .transpose()
}

fn optional_u64_field(object: &serde_json::Map<String, Value>, key: &str) -> Result<Option<u64>> {
    object
        .get(key)
        .map(|value| {
            value
                .as_u64()
                .filter(|value| *value > 0)
                .ok_or_else(|| Error::Config(format!("{key} must be a positive integer")))
        })
        .transpose()
}

fn validate_reasoning_effort(value: Option<String>) -> Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if REASONING_EFFORT_VALUES.contains(&value.as_str()) {
        Ok(Some(value))
    } else {
        Err(Error::Config(format!(
            "reasoning_effort must be one of {}",
            REASONING_EFFORT_VALUES.join(", ")
        )))
    }
}

fn enabled_reasoning_effort(value: Option<String>) -> Result<Option<String>> {
    match validate_reasoning_effort(value)? {
        Some(value) if value == "none" => Ok(None),
        value => Ok(value),
    }
}

fn model_selection_from_raw(
    raw: &str,
    configured_keys: &HashSet<String>,
    provider_override: Option<String>,
    reasoning_effort: Option<String>,
) -> ModelSelection {
    let raw = raw.trim();
    let mut selection = ModelSelection {
        id: (!raw.is_empty()).then_some(raw.to_string()),
        provider: provider_override,
        reasoning_effort,
    };
    if selection.provider.is_none()
        && let Some((provider, model)) = raw.split_once('/')
    {
        let normalized = normalize_provider_id(provider);
        if configured_keys.contains(&normalized) || built_in_provider(&normalized).is_some() {
            selection.provider = Some(normalized);
            selection.id = (!model.trim().is_empty()).then_some(model.trim().to_string());
        }
    }
    selection
}

fn parse_model_override(raw: Option<&String>) -> Result<ModelSelection> {
    let Some(raw) = raw else {
        return Ok(ModelSelection::default());
    };
    let raw = raw.trim();
    let Some((provider, model)) = raw.split_once('/') else {
        return Err(Error::Config(
            "model override must use provider/model form".to_string(),
        ));
    };
    let provider = provider.trim();
    let model = model.trim();
    if provider.is_empty() || model.is_empty() {
        return Err(Error::Config(
            "model override must use provider/model form".to_string(),
        ));
    }
    Ok(ModelSelection {
        id: Some(model.to_string()),
        provider: Some(normalize_provider_id(provider)),
        reasoning_effort: None,
    })
}

fn resolve_run_provider(
    options: &RunOptions,
    loaded: &LoadedRunConfig,
) -> Result<ResolvedRunProvider> {
    let cli_model = parse_model_override(options.model.as_ref())?;
    let env_model = loaded
        .env
        .get("PSYCHEVO_INFERENCE_MODEL")
        .map(|value| {
            parse_model_selection(
                &Value::String(value.clone()),
                &loaded.config.provider.keys().cloned().collect(),
            )
        })
        .transpose()?
        .unwrap_or_default();

    let inferred_config_provider = loaded
        .config
        .model
        .id
        .as_deref()
        .and_then(|model| infer_provider_for_model(&loaded.config, model));
    let inferred_env_provider = env_model
        .id
        .as_deref()
        .and_then(|model| infer_provider_for_model(&loaded.config, model));
    let provider = first_string([
        cli_model.provider.clone(),
        loaded.config.model.provider.clone(),
        inferred_config_provider,
        loaded
            .env
            .get("PSYCHEVO_INFERENCE_PROVIDER")
            .map(|value| normalize_provider_id(value)),
        env_model.provider.clone(),
        inferred_env_provider,
    ])
    .unwrap_or_else(|| "auto".to_string());

    if provider == "auto" {
        for candidate in AUTO_PROVIDER_ORDER {
            let (model, reasoning_effort) = model_for_provider(
                candidate,
                &cli_model,
                &loaded.config.model,
                &env_model,
                loaded.config.provider.get(*candidate),
            );
            if let Ok(resolved) =
                resolve_one_provider(candidate, model, reasoning_effort, options, loaded, true)
            {
                return Ok(resolved);
            }
        }
        return Err(Error::Config(
            "auto provider could not find usable credentials and model".to_string(),
        ));
    }

    let (model, reasoning_effort) = model_for_provider(
        &provider,
        &cli_model,
        &loaded.config.model,
        &env_model,
        loaded.config.provider.get(&provider),
    );
    resolve_one_provider(&provider, model, reasoning_effort, options, loaded, false)
}

fn model_for_provider(
    provider: &str,
    cli_model: &ModelSelection,
    config_model: &ModelSelection,
    env_model: &ModelSelection,
    config_entry: Option<&ConfigProviderEntry>,
) -> (Option<String>, Option<String>) {
    for selection in [cli_model, config_model, env_model] {
        if let Some(id) = &selection.id
            && selection
                .provider
                .as_deref()
                .is_none_or(|selected_provider| selected_provider == provider)
        {
            let reasoning_effort = selection.reasoning_effort.clone().or_else(|| {
                config_model_entry(config_entry, id)
                    .and_then(|entry| entry.reasoning_effort.clone())
            });
            return (Some(id.clone()), reasoning_effort);
        }
    }
    let model = unique_config_model(config_entry);
    let reasoning_effort = model
        .as_deref()
        .and_then(|model| config_model_entry(config_entry, model))
        .and_then(|entry| entry.reasoning_effort.clone());
    (model, reasoning_effort)
}

fn resolve_one_provider(
    provider: &str,
    explicit_model: Option<String>,
    explicit_reasoning_effort: Option<String>,
    options: &RunOptions,
    loaded: &LoadedRunConfig,
    skip_missing: bool,
) -> Result<ResolvedRunProvider> {
    let provider = normalize_provider_id(provider);
    let config_entry = loaded.config.provider.get(&provider);
    let built_in = built_in_provider(&provider);
    if built_in.is_none() && config_entry.is_none() {
        return Err(Error::Config(format!("unknown provider: {provider}")));
    }
    let model = explicit_model
        .or_else(|| unique_config_model(config_entry))
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Error::Config(format!("provider {provider} requires a model")))?;
    let reasoning_effort = enabled_reasoning_effort(first_string([
        options.reasoning_effort.clone(),
        explicit_reasoning_effort,
        config_model_entry(config_entry, &model).and_then(|entry| entry.reasoning_effort.clone()),
    ]))?;
    let context_limit = config_model_entry(config_entry, &model)
        .and_then(|entry| entry.context_limit)
        .or_else(|| built_in_context_limit(&provider, &model));
    let base_url = first_string([
        config_entry.and_then(|entry| entry.options.base_url.clone()),
        built_in
            .and_then(|provider| provider.base_url_env)
            .and_then(|key| loaded.env.get(key).cloned())
            .filter(|value| !value.trim().is_empty()),
        built_in.and_then(|provider| provider.base_url.map(str::to_string)),
    ])
    .ok_or_else(|| Error::Config(format!("provider {provider} requires a base_url")))?;

    let api_key_env = first_string([
        config_entry.and_then(|entry| entry.options.api_key_env.clone()),
        built_in.and_then(|provider| {
            provider
                .api_key_envs
                .iter()
                .find(|key| env_value(&loaded.env, key).is_some())
                .or_else(|| provider.api_key_envs.first())
                .map(|key| (*key).to_string())
        }),
    ]);
    let api_key = api_key_env
        .as_deref()
        .and_then(|key| env_value(&loaded.env, key))
        .or_else(|| {
            let allow_no_auth = built_in.is_some_and(|provider| provider.allow_no_auth)
                || is_loopback_base_url(&base_url);
            allow_no_auth.then(|| "not-needed".to_string())
        });
    let Some(api_key) = api_key else {
        if skip_missing {
            return Err(Error::Config("missing credentials".to_string()));
        }
        return Err(Error::Config(format!(
            "provider {provider} requires credentials{}",
            api_key_env
                .as_ref()
                .map(|key| format!(" in {key}"))
                .unwrap_or_default()
        )));
    };

    Ok(ResolvedRunProvider {
        provider: provider.clone(),
        display_label: built_in
            .map(|provider| provider.label.to_string())
            .unwrap_or_else(|| provider.clone()),
        model,
        base_url,
        api_key_env,
        api_key,
        reasoning_effort,
        context_limit,
    })
}

fn built_in_context_limit(provider: &str, model: &str) -> Option<u64> {
    let model = model.to_lowercase();
    match normalize_provider_id(provider).as_str() {
        "deepseek" if model.contains("deepseek") => Some(64_000),
        "openai" if model.contains("gpt-4.1") || model.contains("gpt-4o") => Some(128_000),
        _ => None,
    }
}

fn built_in_provider(provider: &str) -> Option<&'static BuiltInProvider> {
    BUILT_IN_PROVIDERS
        .iter()
        .find(|entry| entry.id == normalize_provider_id(provider))
}

fn normalize_provider_id(provider: &str) -> String {
    let key = provider.trim().to_lowercase();
    match key.as_str() {
        "z.ai" | "z-ai" | "glm" => "zai".to_string(),
        "alibaba" | "qwen" => "dashscope".to_string(),
        "mimo" => "xiaomi".to_string(),
        "x-ai" | "x.ai" | "grok" => "xai".to_string(),
        "lm-studio" | "lm_studio" => "lmstudio".to_string(),
        other => other.to_string(),
    }
}

fn first_string(values: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    values
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

fn infer_provider_for_model(config: &RunConfig, model: &str) -> Option<String> {
    let matches = config
        .provider
        .iter()
        .filter_map(|(provider, entry)| {
            entry.models.contains_key(model).then_some(provider.clone())
        })
        .collect::<Vec<_>>();
    (matches.len() == 1).then(|| matches[0].clone())
}

fn unique_config_model(entry: Option<&ConfigProviderEntry>) -> Option<String> {
    let entry = entry?;
    (entry.models.len() == 1).then(|| entry.models.keys().next().expect("one model").clone())
}

fn config_model_entry<'a>(
    entry: Option<&'a ConfigProviderEntry>,
    model: &str,
) -> Option<&'a ConfigModelEntry> {
    entry.and_then(|entry| entry.models.get(model))
}

pub fn configured_models(options: &RunOptions) -> Result<Vec<ConfiguredModel>> {
    let workdir = canonical_workdir(&options.workdir)?;
    let loaded = load_run_config(options, &workdir)?;
    let cli_model = parse_model_override(options.model.as_ref())?;
    let env_model = loaded
        .env
        .get("PSYCHEVO_INFERENCE_MODEL")
        .map(|value| {
            parse_model_selection(
                &Value::String(value.clone()),
                &loaded.config.provider.keys().cloned().collect(),
            )
        })
        .transpose()?
        .unwrap_or_default();

    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    let mut push_model = |provider: &str,
                          model: &str,
                          reasoning_effort: Option<String>,
                          context_limit: Option<u64>,
                          rows: &mut Vec<ConfiguredModel>| {
        let provider = normalize_provider_id(provider);
        let model = model.trim().to_string();
        if provider.is_empty() || model.is_empty() || !seen.insert(format!("{provider}/{model}")) {
            return;
        }
        let context_limit = context_limit.or_else(|| built_in_context_limit(&provider, &model));
        rows.push(ConfiguredModel {
            provider: provider.clone(),
            provider_label: provider_label(&provider),
            model,
            reasoning_effort,
            context_limit,
        });
    };

    for (provider, entry) in &loaded.config.provider {
        for (model, config) in &entry.models {
            push_model(
                provider,
                model,
                config.reasoning_effort.clone(),
                config.context_limit,
                &mut rows,
            );
        }
    }

    for selection in [&cli_model, &loaded.config.model, &env_model] {
        if let (Some(provider), Some(model)) = (&selection.provider, &selection.id) {
            let reasoning_effort = loaded
                .config
                .provider
                .get(provider)
                .and_then(|entry| config_model_entry(Some(entry), model))
                .and_then(|entry| entry.reasoning_effort.clone())
                .or_else(|| selection.reasoning_effort.clone());
            let context_limit = loaded
                .config
                .provider
                .get(provider)
                .and_then(|entry| config_model_entry(Some(entry), model))
                .and_then(|entry| entry.context_limit);
            push_model(provider, model, reasoning_effort, context_limit, &mut rows);
        }
    }

    rows.sort_by(|left, right| {
        left.provider
            .cmp(&right.provider)
            .then_with(|| left.model.cmp(&right.model))
    });
    Ok(rows)
}

fn provider_label(provider: &str) -> String {
    built_in_provider(provider)
        .map(|entry| entry.label.to_string())
        .unwrap_or_else(|| provider.to_string())
}

fn env_value(env_map: &BTreeMap<String, String>, key: &str) -> Option<String> {
    env_map
        .get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn is_loopback_base_url(base_url: &str) -> bool {
    let value = base_url.to_lowercase();
    value.contains("://localhost")
        || value.contains("://127.0.0.1")
        || value.contains("://0.0.0.0")
        || value.contains("://[::1]")
}

pub async fn run_smoke(options: SmokeOptions) -> Result<SmokeResult> {
    let workdir = canonical_workdir(&options.workdir)?;
    if options.reset {
        reset_smoke(&workdir)?;
    }
    prepare_smoke_files(&workdir)?;

    let store = SqliteStore::open(&options.db_path)?;
    let session_id = if let Some(session_id) = options.session.clone() {
        store.resume_session(&session_id)?;
        session_id
    } else {
        store.create_session(&workdir)?
    };

    let prompt = options
        .prompt
        .clone()
        .unwrap_or_else(|| "smoke".to_string());
    let previous_messages = prune_context(
        store.load_messages(&session_id)?,
        options.max_context_messages,
    );
    let scripts = fake_scripts_for_prompt(&prompt);
    let provider = Arc::new(FakeProvider::new(scripts));
    let tools = coding_core_tools(&workdir);

    let (control_handle, control_receivers) = ControlHandle::new();
    let expected_control_outcome = match options.control {
        SmokeControl::None => None,
        SmokeControl::StopAfterTurn => Some(Outcome::Stopped),
        SmokeControl::AbortOnAgentStart => Some(Outcome::Aborted),
    };
    let sink = Arc::new(PersistenceSink {
        store: store.clone(),
        session_id: session_id.clone(),
        control: options.control,
        control_handle: Some(control_handle),
        events: None,
        stream_events: None,
        include_reasoning: false,
    });
    let request = AgentLoopRequest {
        model_provider: "fake".to_string(),
        model: "fake-coding-model".to_string(),
        generation_metadata: json!({}),
        system_instructions: Vec::new(),
        previous_messages,
        prompt_messages: vec![user_text_message(prompt)],
        tools,
        max_turns: 8,
    };
    let completion = run_agent_loop(provider, request, sink, control_receivers).await?;
    store.touch_session(&session_id)?;
    write_smoke_manifest(&workdir)?;

    let final_answer = completion
        .messages
        .iter()
        .rev()
        .find_map(assistant_text)
        .unwrap_or_default();
    let tool_failures = completion
        .messages
        .iter()
        .filter(|message| matches!(message, Message::ToolResult { is_error: true, .. }))
        .count();

    Ok(SmokeResult {
        session_id,
        outcome: completion.outcome,
        final_answer,
        db_path: options.db_path,
        workdir,
        tool_failures,
        expected_control_outcome,
    })
}

struct PersistenceSink {
    store: SqliteStore,
    session_id: String,
    control: SmokeControl,
    control_handle: Option<ControlHandle>,
    events: Option<Arc<Mutex<Vec<Value>>>>,
    stream_events: Option<RunStreamSink>,
    include_reasoning: bool,
}

impl EventSink for PersistenceSink {
    fn emit(&self, event: AgentEvent) -> BoxFuture<'static, CoreResult<()>> {
        let store = self.store.clone();
        let session_id = self.session_id.clone();
        let control = self.control;
        let control_handle = self.control_handle.clone();
        let events = self.events.clone();
        let stream_events = self.stream_events.clone();
        let include_reasoning = self.include_reasoning;
        Box::pin(async move {
            if let Some(events) = events
                && let Some(value) = project_agent_event(&event, include_reasoning)
            {
                events.lock().expect("event lock poisoned").push(value);
            }
            if let Some(stream_events) = stream_events
                && let Some(value) = project_run_stream_event(&event)
            {
                stream_events(value);
            }
            match event {
                AgentEvent::AgentStart => match control {
                    SmokeControl::None => {}
                    SmokeControl::StopAfterTurn => {
                        if let Some(handle) = control_handle {
                            handle.stop();
                        }
                    }
                    SmokeControl::AbortOnAgentStart => {
                        if let Some(handle) = control_handle {
                            handle.abort();
                        }
                    }
                },
                AgentEvent::MessageEnd {
                    message,
                    usage,
                    metadata,
                } => store
                    .append_message_with_metrics(&session_id, &message, usage, metadata)
                    .map_err(|err| psychevo_agent_core::Error::EventSink(err.to_string()))?,
                AgentEvent::AgentEnd { outcome, .. } => store
                    .finish_session(&session_id, outcome)
                    .map_err(|err| psychevo_agent_core::Error::EventSink(err.to_string()))?,
                _ => {}
            }
            Ok(())
        })
    }
}

fn project_agent_event(event: &AgentEvent, include_reasoning: bool) -> Option<Value> {
    let projected = match event {
        AgentEvent::ReasoningDelta { text } => {
            return include_reasoning.then(|| json!({ "type": "reasoning_delta", "text": text }));
        }
        AgentEvent::ReasoningEnd { text } => {
            return include_reasoning.then(|| json!({ "type": "reasoning_end", "text": text }));
        }
        AgentEvent::AgentEnd { outcome, messages } => AgentEvent::AgentEnd {
            outcome: *outcome,
            messages: messages.iter().map(sanitize_message_for_output).collect(),
        },
        AgentEvent::MessageStart { message } => AgentEvent::MessageStart {
            message: sanitize_message_for_output(message),
        },
        AgentEvent::MessageUpdate { message } => AgentEvent::MessageUpdate {
            message: sanitize_message_for_output(message),
        },
        AgentEvent::MessageEnd { message, .. } => AgentEvent::MessageEnd {
            message: sanitize_message_for_output(message),
            usage: None,
            metadata: None,
        },
        other => other.clone(),
    };
    serde_json::to_value(projected).ok()
}

fn project_run_stream_event(event: &AgentEvent) -> Option<RunStreamEvent> {
    match event {
        AgentEvent::ReasoningDelta { text } => {
            Some(RunStreamEvent::ReasoningDelta { text: text.clone() })
        }
        AgentEvent::ReasoningEnd { .. } => Some(RunStreamEvent::ReasoningEnd),
        AgentEvent::MessageEnd {
            message,
            usage,
            metadata,
        } => {
            let mut value = json!({
                "type": "message_end",
                "message": sanitize_message_for_output(message),
            });
            if let Some(usage) = usage
                && let Some(object) = value.as_object_mut()
            {
                object.insert("usage".to_string(), usage.clone());
            }
            if let Some(metadata) = metadata
                && let Some(object) = value.as_object_mut()
            {
                object.insert("metadata".to_string(), metadata.clone());
            }
            Some(RunStreamEvent::Event(value))
        }
        _ => project_agent_event(event, false).map(RunStreamEvent::Event),
    }
}

fn sanitize_message_for_output(message: &Message) -> Message {
    match message {
        Message::Assistant {
            content,
            timestamp_ms,
            finish_reason,
            outcome,
            model,
            provider,
            ..
        } => Message::Assistant {
            content: content
                .iter()
                .filter(|block| !matches!(block, AssistantBlock::Reasoning { .. }))
                .cloned()
                .collect(),
            timestamp_ms: *timestamp_ms,
            finish_reason: finish_reason.clone(),
            outcome: *outcome,
            model: model.clone(),
            provider: provider.clone(),
        },
        other => other.clone(),
    }
}

fn canonical_workdir(path: &Path) -> Result<PathBuf> {
    fs::create_dir_all(path)?;
    Ok(path.canonicalize()?)
}

pub fn canonicalize_workdir(path: &Path) -> Result<PathBuf> {
    canonical_workdir(path)
}

fn coding_core_tools(workdir: &Path) -> Vec<Arc<dyn ToolBinding>> {
    coding_core_tools_for_mode(workdir, RunMode::Build)
}

fn coding_core_tools_for_mode(workdir: &Path, mode: RunMode) -> Vec<Arc<dyn ToolBinding>> {
    match mode {
        RunMode::Plan => read_only_plan_tools(workdir),
        RunMode::Build => full_build_tools(workdir),
    }
}

fn full_build_tools(workdir: &Path) -> Vec<Arc<dyn ToolBinding>> {
    vec![
        Arc::new(ReadTool::new(workdir.to_path_buf())),
        Arc::new(WriteTool::new(workdir.to_path_buf())),
        Arc::new(EditTool::new(workdir.to_path_buf())),
        Arc::new(BashTool::new(workdir.to_path_buf())),
    ]
}

fn read_only_plan_tools(workdir: &Path) -> Vec<Arc<dyn ToolBinding>> {
    vec![
        Arc::new(ReadTool::new(workdir.to_path_buf())),
        Arc::new(ListTool::new(workdir.to_path_buf())),
        Arc::new(SearchTool::new(workdir.to_path_buf())),
    ]
}

pub fn tool_names_for_mode(mode: RunMode) -> Vec<&'static str> {
    match mode {
        RunMode::Plan => vec!["read", "list", "search"],
        RunMode::Build => vec!["read", "write", "edit", "bash"],
    }
}

fn mode_instruction(mode: RunMode) -> &'static str {
    match mode {
        RunMode::Build => {
            "Runtime mode: build. You may use the available coding tools to read, edit, write, and run commands under the selected workdir."
        }
        RunMode::Plan => {
            "Runtime mode: plan. This turn is hard read-only. Use only the available read, list, and search tools to inspect the workdir. Do not write files, edit files, run shell commands, or claim to have modified the workspace."
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct SmokeManifest {
    files: Vec<String>,
}

fn reset_smoke(workdir: &Path) -> Result<()> {
    let manifest_path = workdir.join(SMOKE_MANIFEST);
    if !manifest_path.exists() {
        return Ok(());
    }
    let manifest: SmokeManifest = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    for file in manifest.files {
        let path = workdir.join(file);
        if path.is_file() {
            fs::remove_file(path)?;
        }
    }
    if manifest_path.is_file() {
        fs::remove_file(manifest_path)?;
    }
    Ok(())
}

fn prepare_smoke_files(workdir: &Path) -> Result<()> {
    let dir = workdir.join(SMOKE_DIR);
    fs::create_dir_all(&dir)?;
    fs::write(
        workdir.join(SMOKE_SUBJECT),
        "original psychevo smoke\nsecond line\n",
    )?;
    Ok(())
}

fn write_smoke_manifest(workdir: &Path) -> Result<()> {
    let manifest = SmokeManifest {
        files: vec![
            SMOKE_SUBJECT.to_string(),
            SMOKE_GENERATED.to_string(),
            SMOKE_MANIFEST.to_string(),
        ],
    };
    fs::write(
        workdir.join(SMOKE_MANIFEST),
        serde_json::to_string_pretty(&manifest)?,
    )?;
    Ok(())
}

fn fake_scripts_for_prompt(prompt: &str) -> Vec<Vec<RawStreamEvent>> {
    let tools = selected_tools(prompt);
    if tools.is_empty() {
        return vec![vec![
            RawStreamEvent::Text(format!("smoke text: {prompt}")),
            RawStreamEvent::Done(Outcome::Normal),
        ]];
    }

    let mut first = Vec::new();
    let mut call_index = 0usize;
    for tool in tools {
        if tool == "read" {
            push_tool_call(
                &mut first,
                call_index,
                "read-1",
                "read",
                json!({ "path": SMOKE_SUBJECT, "offset": 1, "limit": 20 }),
            );
            call_index += 1;
            push_tool_call(
                &mut first,
                call_index,
                "read-2",
                "read",
                json!({ "path": SMOKE_SUBJECT, "offset": 2, "limit": 20 }),
            );
            call_index += 1;
        } else if tool == "write" {
            push_tool_call(
                &mut first,
                call_index,
                "write-1",
                "write",
                json!({ "path": SMOKE_GENERATED, "content": "written by psychevo smoke\n" }),
            );
            call_index += 1;
        } else if tool == "edit" {
            push_tool_call(
                &mut first,
                call_index,
                "edit-1",
                "edit",
                json!({
                    "mode": "replace",
                    "path": SMOKE_SUBJECT,
                    "edits": [{ "oldText": "original", "newText": "edited" }]
                }),
            );
            call_index += 1;
        } else if tool == "bash" {
            push_tool_call(
                &mut first,
                call_index,
                "bash-1",
                "bash",
                json!({ "command": "printf 'bash smoke\\n'", "timeout": 5 }),
            );
            call_index += 1;
        }
    }
    first.push(RawStreamEvent::Done(Outcome::Normal));
    vec![
        first,
        vec![
            RawStreamEvent::Text("smoke tools complete".to_string()),
            RawStreamEvent::Done(Outcome::Normal),
        ],
    ]
}

fn push_tool_call(
    events: &mut Vec<RawStreamEvent>,
    call_index: usize,
    id: &str,
    name: &str,
    args: Value,
) {
    events.push(RawStreamEvent::ToolStart {
        content_index: call_index,
        call_index,
        id: id.to_string(),
        name: name.to_string(),
    });
    let args = serde_json::to_string(&args).expect("smoke tool args serializable");
    let split = args.len() / 2;
    events.push(RawStreamEvent::ToolArgs {
        content_index: call_index,
        call_index,
        delta: args[..split].to_string(),
    });
    events.push(RawStreamEvent::ToolArgs {
        content_index: call_index,
        call_index,
        delta: args[split..].to_string(),
    });
    events.push(RawStreamEvent::ToolEnd {
        content_index: call_index,
        call_index,
    });
}

fn selected_tools(prompt: &str) -> Vec<&'static str> {
    let lower = prompt.to_lowercase();
    let mut found = ["read", "write", "edit", "bash"]
        .into_iter()
        .filter_map(|name| lower.find(name).map(|idx| (idx, name)))
        .collect::<Vec<_>>();
    found.sort_by_key(|(idx, _)| *idx);
    found.into_iter().map(|(_, name)| name).collect()
}

fn assistant_text(message: &Message) -> Option<String> {
    let Message::Assistant { content, .. } = message else {
        return None;
    };
    let text = content
        .iter()
        .filter_map(|block| match block {
            AssistantBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() { None } else { Some(text) }
}

pub fn prune_context(messages: Vec<Message>, max_context_messages: Option<usize>) -> Vec<Message> {
    let Some(max) = max_context_messages else {
        return messages;
    };
    if messages.len() <= max {
        return messages;
    }
    let mut start = messages.len().saturating_sub(max);
    loop {
        let retained = &messages[start..];
        let missing = missing_tool_call_ids(retained);
        if missing.is_empty() || start == 0 {
            break;
        }
        let mut new_start = start;
        for idx in (0..start).rev() {
            if assistant_contains_any_tool_id(&messages[idx], &missing) {
                new_start = idx;
            }
        }
        if new_start == start {
            break;
        }
        start = new_start;
    }
    messages[start..].to_vec()
}

fn missing_tool_call_ids(messages: &[Message]) -> HashSet<String> {
    let mut calls = HashSet::new();
    let mut results = HashSet::new();
    for message in messages {
        match message {
            Message::Assistant { content, .. } => {
                for block in content {
                    if let AssistantBlock::ToolCall(call) = block {
                        calls.insert(call.id.clone());
                    }
                }
            }
            Message::ToolResult { tool_call_id, .. } => {
                results.insert(tool_call_id.clone());
            }
            Message::User { .. } => {}
        }
    }
    results.difference(&calls).cloned().collect()
}

fn assistant_contains_any_tool_id(message: &Message, ids: &HashSet<String>) -> bool {
    let Message::Assistant { content, .. } = message else {
        return false;
    };
    content.iter().any(|block| match block {
        AssistantBlock::ToolCall(call) => ids.contains(&call.id),
        _ => false,
    })
}

#[derive(Clone)]
struct WorkdirTool {
    workdir: PathBuf,
}

impl WorkdirTool {
    fn new(workdir: PathBuf) -> Self {
        Self { workdir }
    }

    fn resolve_existing(&self, raw: &str) -> Result<PathBuf> {
        let target = self.resolve_raw(raw);
        let canonical = target.canonicalize()?;
        self.ensure_contained(&canonical)?;
        Ok(canonical)
    }

    fn resolve_write_target(&self, raw: &str) -> Result<(PathBuf, bool)> {
        let target = self.resolve_raw(raw);
        if target.exists() {
            let canonical = target.canonicalize()?;
            self.ensure_contained(&canonical)?;
            return Ok((canonical, false));
        }
        let parent = target
            .parent()
            .ok_or_else(|| Error::Message("target has no parent".to_string()))?
            .to_path_buf();
        let mut existing = parent.as_path();
        while !existing.exists() {
            existing = existing
                .parent()
                .ok_or_else(|| Error::Message("no existing parent under workdir".to_string()))?;
        }
        let canonical_parent = existing.canonicalize()?;
        self.ensure_contained(&canonical_parent)?;
        let dirs_created = !parent.exists();
        Ok((target, dirs_created))
    }

    fn resolve_raw(&self, raw: &str) -> PathBuf {
        let path = Path::new(raw);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workdir.join(path)
        }
    }

    fn ensure_contained(&self, path: &Path) -> Result<()> {
        if path == self.workdir || path.starts_with(&self.workdir) {
            Ok(())
        } else {
            Err(Error::Message(format!(
                "path escapes workdir: {}",
                path.display()
            )))
        }
    }

    fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.workdir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }
}

struct ReadTool(WorkdirTool);

impl ReadTool {
    fn new(workdir: PathBuf) -> Self {
        Self(WorkdirTool::new(workdir))
    }
}

impl ToolBinding for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read UTF-8 text from the working directory."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","required":["path"],"properties":{"path":{"type":"string"},"offset":{"type":"integer"},"limit":{"type":"integer"}}})
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
        let tool = self.0.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match read_tool_impl(tool, args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

fn read_tool_impl(tool: WorkdirTool, args: Value) -> Result<Value> {
    let path = required_string(&args, "path")?;
    let offset = optional_i64(&args, "offset")?.unwrap_or(1);
    let limit = optional_i64(&args, "limit")?;
    if offset < 1 {
        return Err(Error::Message("offset must be >= 1".to_string()));
    }
    if let Some(limit) = limit
        && limit < 1
    {
        return Err(Error::Message("limit must be >= 1".to_string()));
    }
    let target = tool.resolve_existing(path)?;
    let bytes = fs::read(&target)?;
    if bytes.contains(&0) {
        return Err(Error::Message("binary files are not supported".to_string()));
    }
    let content =
        String::from_utf8(bytes).map_err(|_| Error::Message("invalid UTF-8".to_string()))?;
    let file_size = content.len();
    let lines = content.split('\n').collect::<Vec<_>>();
    let total_lines = lines.len();
    let start = (offset as usize).saturating_sub(1);
    if start >= total_lines {
        return Err(Error::Message(format!(
            "offset {offset} is beyond end of file ({total_lines} lines)"
        )));
    }
    let end = limit
        .map(|limit| start.saturating_add(limit as usize).min(total_lines))
        .unwrap_or(total_lines);
    let selected = lines[start..end].join("\n");
    let truncated = truncate_head(&selected, READ_MAX_BYTES, READ_MAX_LINES);
    let mut hint = Value::Null;
    if truncated.truncated || end < total_lines {
        let next = start + truncated.lines + 1;
        hint = json!(format!("Use offset={next} to continue."));
    }
    Ok(json!({
        "path": tool.relative(&target),
        "content": truncated.content,
        "total_lines": total_lines,
        "file_size": file_size,
        "truncated": truncated.truncated || end < total_lines,
        "hint": hint,
        "error": null,
        "similar_files": []
    }))
}

struct ListTool(WorkdirTool);

impl ListTool {
    fn new(workdir: PathBuf) -> Self {
        Self(WorkdirTool::new(workdir))
    }
}

impl ToolBinding for ListTool {
    fn name(&self) -> &str {
        "list"
    }

    fn description(&self) -> &str {
        "List files and directories under the working directory."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"},"limit":{"type":"integer"}}})
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
        let tool = self.0.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match list_tool_impl(tool, args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

fn list_tool_impl(tool: WorkdirTool, args: Value) -> Result<Value> {
    let path = optional_string(&args, "path")?.unwrap_or(".");
    let limit = bounded_limit(optional_i64(&args, "limit")?, 200, 1000)?;
    let target = tool.resolve_existing(path)?;
    let mut entries = Vec::new();
    if target.is_file() {
        entries.push(json!({
            "path": tool.relative(&target),
            "type": "file",
        }));
        return Ok(json!({
            "path": tool.relative(&target),
            "entries": entries,
            "truncated": false,
            "error": null
        }));
    }

    let mut raw_entries = fs::read_dir(&target)?.collect::<std::result::Result<Vec<_>, _>>()?;
    raw_entries.sort_by_key(|entry| entry.path());
    let truncated = raw_entries.len() > limit;
    for entry in raw_entries.into_iter().take(limit) {
        let file_type = entry.file_type()?;
        let kind = if file_type.is_dir() {
            "dir"
        } else if file_type.is_file() {
            "file"
        } else if file_type.is_symlink() {
            "symlink"
        } else {
            "other"
        };
        entries.push(json!({
            "path": tool.relative(&entry.path()),
            "type": kind,
        }));
    }

    Ok(json!({
        "path": tool.relative(&target),
        "entries": entries,
        "truncated": truncated,
        "error": null
    }))
}

struct SearchTool(WorkdirTool);

impl SearchTool {
    fn new(workdir: PathBuf) -> Self {
        Self(WorkdirTool::new(workdir))
    }
}

impl ToolBinding for SearchTool {
    fn name(&self) -> &str {
        "search"
    }

    fn description(&self) -> &str {
        "Search UTF-8 text files under the working directory for a literal string."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","required":["query"],"properties":{"query":{"type":"string"},"path":{"type":"string"},"limit":{"type":"integer"}}})
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
        let tool = self.0.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match search_tool_impl(tool, args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

fn search_tool_impl(tool: WorkdirTool, args: Value) -> Result<Value> {
    let query = required_string(&args, "query")?;
    if query.is_empty() {
        return Err(Error::Message("query must not be empty".to_string()));
    }
    let path = optional_string(&args, "path")?.unwrap_or(".");
    let limit = bounded_limit(optional_i64(&args, "limit")?, 100, 1000)?;
    let target = tool.resolve_existing(path)?;
    let mut queue = VecDeque::from([target.clone()]);
    let mut matches = Vec::new();
    let mut skipped_files = 0usize;
    let mut truncated = false;

    while let Some(path) = queue.pop_front() {
        if matches.len() >= limit {
            truncated = true;
            break;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            skipped_files += 1;
            continue;
        }
        if metadata.is_dir() {
            let mut entries = fs::read_dir(&path)?.collect::<std::result::Result<Vec<_>, _>>()?;
            entries.sort_by_key(|entry| entry.path());
            for entry in entries {
                let name = entry.file_name();
                if name == ".git" {
                    continue;
                }
                queue.push_back(entry.path());
            }
            continue;
        }
        if !metadata.is_file() {
            skipped_files += 1;
            continue;
        }
        let bytes = fs::read(&path)?;
        if bytes.contains(&0) {
            skipped_files += 1;
            continue;
        }
        let Ok(content) = String::from_utf8(bytes) else {
            skipped_files += 1;
            continue;
        };
        for (idx, line) in content.lines().enumerate() {
            if line.contains(query) {
                matches.push(json!({
                    "path": tool.relative(&path),
                    "line_number": idx + 1,
                    "line": truncate_match_line(line),
                }));
                if matches.len() >= limit {
                    truncated = true;
                    break;
                }
            }
        }
    }

    Ok(json!({
        "path": tool.relative(&target),
        "query": query,
        "matches": matches,
        "truncated": truncated,
        "skipped_files": skipped_files,
        "error": null
    }))
}

struct WriteTool(WorkdirTool);

impl WriteTool {
    fn new(workdir: PathBuf) -> Self {
        Self(WorkdirTool::new(workdir))
    }
}

impl ToolBinding for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Create or completely replace a UTF-8 text file."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","required":["path","content"],"properties":{"path":{"type":"string"},"content":{"type":"string"}}})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let tool = self.0.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match write_tool_impl(tool, args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

fn write_tool_impl(tool: WorkdirTool, args: Value) -> Result<Value> {
    let path = required_string(&args, "path")?;
    let content = required_string(&args, "content")?;
    let (target, dirs_created) = tool.resolve_write_target(path)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&target, content)?;
    Ok(json!({
        "path": tool.relative(&target),
        "bytes_written": content.len(),
        "dirs_created": dirs_created,
        "error": null
    }))
}

struct EditTool(WorkdirTool);

impl EditTool {
    fn new(workdir: PathBuf) -> Self {
        Self(WorkdirTool::new(workdir))
    }
}

impl ToolBinding for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Apply targeted replacements or a unified diff to existing text files."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"mode":{"type":"string"},"path":{"type":"string"},"edits":{"type":"array"},"patch":{"type":"string"}}})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let tool = self.0.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match edit_tool_impl(tool, args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

fn edit_tool_impl(tool: WorkdirTool, args: Value) -> Result<Value> {
    let mode = args
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("replace");
    match mode {
        "replace" => edit_replace(tool, args),
        "patch" => edit_patch(tool, args),
        _ => Err(Error::Message(format!("unsupported edit mode: {mode}"))),
    }
}

#[derive(Debug, Deserialize)]
struct ReplaceEdit {
    #[serde(rename = "oldText")]
    old_text: String,
    #[serde(rename = "newText")]
    new_text: String,
}

fn edit_replace(tool: WorkdirTool, args: Value) -> Result<Value> {
    let path = required_string(&args, "path")?;
    let edits_value = args
        .get("edits")
        .ok_or_else(|| Error::Message("edits is required".to_string()))?;
    let edits: Vec<ReplaceEdit> = serde_json::from_value(edits_value.clone())?;
    if edits.is_empty() {
        return Err(Error::Message("edits must not be empty".to_string()));
    }
    let target = tool.resolve_existing(path)?;
    let original_bytes = fs::read(&target)?;
    let original_text = String::from_utf8(original_bytes)
        .map_err(|_| Error::Message("invalid UTF-8".to_string()))?;
    let bom = original_text.starts_with('\u{feff}');
    let body = original_text.trim_start_matches('\u{feff}');
    let line_ending = dominant_line_ending(body);
    let normalized = normalize_lf(body);
    let mut ranges = Vec::new();
    for edit in &edits {
        if edit.old_text == edit.new_text {
            return Err(Error::Message("no-change edit".to_string()));
        }
        let old = normalize_lf(edit.old_text.trim_start_matches('\u{feff}'));
        let matches = normalized.match_indices(&old).collect::<Vec<_>>();
        if matches.is_empty() {
            return Err(Error::Message(format!(
                "oldText not found: {}",
                edit.old_text
            )));
        }
        if matches.len() > 1 {
            return Err(Error::Message(format!(
                "oldText is ambiguous: {}",
                edit.old_text
            )));
        }
        let start = matches[0].0;
        let end = start + old.len();
        ranges.push((start, end, normalize_lf(&edit.new_text)));
    }
    ranges.sort_by_key(|(start, _, _)| *start);
    for pair in ranges.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err(Error::Message("edits overlap".to_string()));
        }
    }
    let mut updated = String::new();
    let mut cursor = 0usize;
    for (start, end, replacement) in ranges {
        updated.push_str(&normalized[cursor..start]);
        updated.push_str(&replacement);
        cursor = end;
    }
    updated.push_str(&normalized[cursor..]);
    let diff = unified_diff(&tool.relative(&target), &normalized, &updated);
    let restored = restore_line_endings(&updated, line_ending);
    fs::write(
        &target,
        if bom {
            format!("\u{feff}{restored}")
        } else {
            restored
        },
    )?;
    Ok(json!({
        "success": true,
        "diff": diff,
        "files_modified": [tool.relative(&target)],
        "error": null
    }))
}

fn edit_patch(tool: WorkdirTool, args: Value) -> Result<Value> {
    let patch = required_string(&args, "patch")?;
    let files = parse_unified_patch(patch)?;
    if files.is_empty() {
        return Err(Error::Message("patch contains no file updates".to_string()));
    }
    let mut diffs = Vec::new();
    let mut modified = Vec::new();
    for file in files {
        let target = tool.resolve_existing(&file.path)?;
        let original_text = String::from_utf8(fs::read(&target)?)
            .map_err(|_| Error::Message("invalid UTF-8".to_string()))?;
        let bom = original_text.starts_with('\u{feff}');
        let body = original_text.trim_start_matches('\u{feff}');
        let line_ending = dominant_line_ending(body);
        let mut lines = normalize_lf(body)
            .split('\n')
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        for hunk in file.hunks {
            let idx = find_unique_subslice(&lines, &hunk.old_lines).ok_or_else(|| {
                Error::Message(format!("patch hunk did not match uniquely: {}", file.path))
            })?;
            lines.splice(idx..idx + hunk.old_lines.len(), hunk.new_lines);
        }
        let updated = lines.join("\n");
        let original_norm = normalize_lf(body);
        let rel = tool.relative(&target);
        diffs.push(unified_diff(&rel, &original_norm, &updated));
        let restored = restore_line_endings(&updated, line_ending);
        fs::write(
            &target,
            if bom {
                format!("\u{feff}{restored}")
            } else {
                restored
            },
        )?;
        modified.push(rel);
    }
    Ok(json!({
        "success": true,
        "diff": diffs.join("\n"),
        "files_modified": modified,
        "error": null
    }))
}

#[derive(Debug)]
struct PatchFile {
    path: String,
    hunks: Vec<PatchHunk>,
}

#[derive(Debug)]
struct PatchHunk {
    old_lines: Vec<String>,
    new_lines: Vec<String>,
}

fn parse_unified_patch(patch: &str) -> Result<Vec<PatchFile>> {
    let mut files = Vec::new();
    let mut lines = patch.lines().peekable();
    while let Some(line) = lines.next() {
        if !line.starts_with("--- ") {
            continue;
        }
        let old = line.trim_start_matches("--- ").trim();
        let new = lines
            .next()
            .ok_or_else(|| Error::Message("patch missing +++ header".to_string()))?;
        if !new.starts_with("+++ ") {
            return Err(Error::Message("patch missing +++ header".to_string()));
        }
        let new = new.trim_start_matches("+++ ").trim();
        if old == "/dev/null" || new == "/dev/null" {
            return Err(Error::Message(
                "patch add/delete is not supported".to_string(),
            ));
        }
        let path = strip_diff_prefix(new);
        let mut hunks = Vec::new();
        while let Some(next) = lines.peek().copied() {
            if next.starts_with("--- ") {
                break;
            }
            if !next.starts_with("@@") {
                let _ = lines.next();
                continue;
            }
            let _ = lines.next();
            let mut old_lines = Vec::new();
            let mut new_lines = Vec::new();
            while let Some(hunk_line) = lines.peek().copied() {
                if hunk_line.starts_with("@@") || hunk_line.starts_with("--- ") {
                    break;
                }
                let hunk_line = lines.next().expect("peeked line exists");
                if let Some(rest) = hunk_line.strip_prefix(' ') {
                    old_lines.push(rest.to_string());
                    new_lines.push(rest.to_string());
                } else if let Some(rest) = hunk_line.strip_prefix('-') {
                    old_lines.push(rest.to_string());
                } else if let Some(rest) = hunk_line.strip_prefix('+') {
                    new_lines.push(rest.to_string());
                } else if hunk_line.starts_with("\\ No newline") {
                } else {
                    return Err(Error::Message(format!(
                        "unsupported patch line: {hunk_line}"
                    )));
                }
            }
            if old_lines.is_empty() {
                return Err(Error::Message(
                    "empty patch hunks are not supported".to_string(),
                ));
            }
            hunks.push(PatchHunk {
                old_lines,
                new_lines,
            });
        }
        files.push(PatchFile { path, hunks });
    }
    Ok(files)
}

fn strip_diff_prefix(path: &str) -> String {
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path)
        .to_string()
}

fn find_unique_subslice(lines: &[String], needle: &[String]) -> Option<usize> {
    let mut found = None;
    for idx in 0..=lines.len().saturating_sub(needle.len()) {
        if lines[idx..idx + needle.len()] == *needle {
            if found.is_some() {
                return None;
            }
            found = Some(idx);
        }
    }
    found
}

struct BashTool(WorkdirTool);

impl BashTool {
    fn new(workdir: PathBuf) -> Self {
        Self(WorkdirTool::new(workdir))
    }
}

impl ToolBinding for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Run a bounded foreground bash command in the working directory."
    }

    fn parameters(&self) -> Value {
        json!({"type":"object","required":["command"],"properties":{"command":{"type":"string"},"timeout":{"type":"number"}}})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let workdir = self.0.workdir.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match bash_tool_impl(workdir, args, abort).await {
                Ok((value, is_error)) => ToolOutput {
                    json: value,
                    is_error,
                },
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

async fn bash_tool_impl(
    workdir: PathBuf,
    args: Value,
    abort: AbortSignal,
) -> Result<(Value, bool)> {
    let command = required_string(&args, "command")?.to_string();
    let timeout_secs = optional_u64(&args, "timeout")?
        .unwrap_or(BASH_DEFAULT_TIMEOUT_SECS)
        .min(BASH_MAX_TIMEOUT_SECS);
    let mut child = Command::new("bash")
        .arg("-lc")
        .arg(&command)
        .current_dir(&workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdout = child.stdout.take().expect("stdout piped");
    let mut stderr = child.stderr.take().expect("stderr piped");
    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf).await;
        buf
    });
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf).await;
        buf
    });
    let status = time::timeout(Duration::from_secs(timeout_secs), child.wait()).await;
    let (exit_code, mut error) = match status {
        Ok(Ok(status)) => (status.code(), None),
        Ok(Err(err)) => return Err(err.into()),
        Err(_) => {
            let _ = child.kill().await;
            (
                None,
                Some(format!("command timed out after {timeout_secs} seconds")),
            )
        }
    };
    if abort.aborted() {
        let _ = child.kill().await;
        error = Some("aborted".to_string());
    }
    let mut output = stdout_task.await.unwrap_or_default();
    output.extend(stderr_task.await.unwrap_or_default());
    let output = String::from_utf8_lossy(&output).to_string();
    let truncated = truncate_tail(&output, READ_MAX_BYTES, READ_MAX_LINES);
    if exit_code.is_some_and(|code| code != 0) && error.is_none() {
        error = Some(format!(
            "command exited with code {}",
            exit_code.unwrap_or_default()
        ));
    }
    let meaning = exit_code.and_then(|code| exit_code_meaning(&command, code));
    let is_error = error.is_some() || exit_code.is_some_and(|code| code != 0);
    let output_text = if truncated.content.is_empty() {
        "(no output)".to_string()
    } else {
        truncated.content
    };
    Ok((
        json!({
            "output": output_text,
            "exit_code": exit_code,
            "error": error,
            "exit_code_meaning": meaning,
            "truncated": truncated.truncated
        }),
        is_error,
    ))
}

fn exit_code_meaning(command: &str, code: i32) -> Option<String> {
    if code != 1 {
        return None;
    }
    let first = command.split_whitespace().next().unwrap_or_default();
    match first {
        "grep" | "rg" | "ag" | "ack" => Some("no matches found".to_string()),
        "diff" => Some("files differ".to_string()),
        "test" | "[" => Some("condition evaluated false".to_string()),
        _ => None,
    }
}

fn required_string<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Message(format!("{key} must be a string")))
}

fn optional_string<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>> {
    args.get(key)
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| Error::Message(format!("{key} must be a string")))
        })
        .transpose()
}

fn optional_i64(args: &Value, key: &str) -> Result<Option<i64>> {
    args.get(key)
        .map(|value| {
            value
                .as_i64()
                .ok_or_else(|| Error::Message(format!("{key} must be an integer")))
        })
        .transpose()
}

fn bounded_limit(value: Option<i64>, default: usize, max: usize) -> Result<usize> {
    let limit = value.unwrap_or(default as i64);
    if limit < 1 {
        return Err(Error::Message("limit must be >= 1".to_string()));
    }
    Ok((limit as usize).min(max))
}

fn truncate_match_line(line: &str) -> String {
    const MAX_LINE_CHARS: usize = 240;
    if line.chars().count() <= MAX_LINE_CHARS {
        return line.to_string();
    }
    let mut value = line.chars().take(MAX_LINE_CHARS).collect::<String>();
    value.push_str("...");
    value
}

fn optional_u64(args: &Value, key: &str) -> Result<Option<u64>> {
    args.get(key)
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| Error::Message(format!("{key} must be an integer")))
        })
        .transpose()
}

#[derive(Debug)]
struct Truncated {
    content: String,
    truncated: bool,
    lines: usize,
}

fn truncate_head(input: &str, max_bytes: usize, max_lines: usize) -> Truncated {
    let mut out = String::new();
    let mut lines = 0usize;
    let mut bytes = 0usize;
    let mut truncated = false;
    for (idx, line) in input.split('\n').enumerate() {
        let addition = if idx == 0 {
            line.to_string()
        } else {
            format!("\n{line}")
        };
        if lines >= max_lines || bytes + addition.len() > max_bytes {
            truncated = true;
            break;
        }
        bytes += addition.len();
        out.push_str(&addition);
        lines += 1;
    }
    Truncated {
        content: out,
        truncated,
        lines,
    }
}

fn truncate_tail(input: &str, max_bytes: usize, max_lines: usize) -> Truncated {
    let all = input.split('\n').collect::<Vec<_>>();
    let mut selected = Vec::new();
    let mut bytes = 0usize;
    for line in all.iter().rev() {
        let addition = line.len() + usize::from(!selected.is_empty());
        if selected.len() >= max_lines || bytes + addition > max_bytes {
            break;
        }
        bytes += addition;
        selected.push(*line);
    }
    selected.reverse();
    Truncated {
        content: selected.join("\n"),
        truncated: selected.len() < all.len(),
        lines: selected.len(),
    }
}

fn dominant_line_ending(text: &str) -> &'static str {
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count();
    if crlf > 0 && crlf >= lf.saturating_sub(crlf) {
        "\r\n"
    } else {
        "\n"
    }
}

fn normalize_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn restore_line_endings(text: &str, line_ending: &str) -> String {
    if line_ending == "\n" {
        text.to_string()
    } else {
        text.replace('\n', line_ending)
    }
}

fn unified_diff(path: &str, old: &str, new: &str) -> String {
    TextDiff::from_lines(old, new)
        .unified_diff()
        .header(&format!("a/{path}"), &format!("b/{path}"))
        .to_string()
}

pub fn session_exists(db_path: &Path, session_id: &str) -> Result<bool> {
    let conn = Connection::open(db_path)?;
    let found = conn
        .query_row(
            "SELECT 1 FROM sessions WHERE id = ?1",
            params![session_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    Ok(found)
}

pub fn latest_run_session_for_workdir(db_path: &Path, workdir: &Path) -> Result<Option<String>> {
    SqliteStore::open(db_path)?.latest_run_session_for_workdir(workdir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    fn base_options(temp: &tempfile::TempDir) -> RunOptions {
        RunOptions {
            db_path: temp.path().join("state.db"),
            workdir: temp.path().join("work"),
            session: None,
            continue_latest: false,
            prompt: "hello".to_string(),
            max_context_messages: None,
            config_path: None,
            model: None,
            reasoning_effort: None,
            include_reasoning: false,
            mode: RunMode::Build,
            inherited_env: Some(BTreeMap::from([(
                "HOME".to_string(),
                temp.path().to_string_lossy().to_string(),
            )])),
        }
    }

    fn home_dir(temp: &tempfile::TempDir) -> PathBuf {
        temp.path().join(".psychevo")
    }

    #[test]
    fn run_mode_tool_names_enforce_plan_read_only_surface() {
        assert_eq!(
            tool_names_for_mode(RunMode::Plan),
            vec!["read", "list", "search"]
        );
        assert_eq!(
            tool_names_for_mode(RunMode::Build),
            vec!["read", "write", "edit", "bash"]
        );
    }

    #[test]
    fn plan_list_and_search_tools_are_read_only_and_bounded() {
        let temp = tempdir().expect("temp");
        let workdir = temp.path().join("work");
        fs::create_dir_all(workdir.join("src")).expect("dirs");
        fs::write(workdir.join("src/lib.rs"), "alpha\nneedle one\n").expect("file");
        fs::write(workdir.join("README.md"), "needle two\n").expect("file");
        let tool = WorkdirTool::new(workdir.canonicalize().expect("canonical"));

        let listed = list_tool_impl(tool.clone(), json!({"path":".","limit":1})).expect("list");
        assert_eq!(listed["entries"].as_array().expect("entries").len(), 1);
        assert_eq!(listed["truncated"], true);

        let searched = search_tool_impl(tool, json!({"query":"needle","path":".","limit":10}))
            .expect("search");
        let matches = searched["matches"].as_array().expect("matches");
        assert_eq!(matches.len(), 2);
        assert!(
            matches
                .iter()
                .all(|entry| entry["line"].as_str().unwrap().contains("needle"))
        );
    }

    #[test]
    fn default_global_config_uses_home_psychevo_config_jsonc() {
        let temp = tempdir().expect("temp");
        let options = base_options(&temp);
        let global_dir = home_dir(&temp);
        fs::create_dir_all(&global_dir).expect("global dir");
        fs::write(
            global_dir.join("config.jsonc"),
            r#"
            {
              "model": "deepseek/deepseek-chat",
              "provider": {
                "deepseek": {
                  "options": {
                    "base_url": "http://home.example/v1",
                    "api_key_env": "DEEPSEEK_API_KEY"
                  },
                  "models": { "deepseek-chat": {} }
                }
              }
            }
            "#,
        )
        .expect("global config");
        fs::write(global_dir.join(".env"), "DEEPSEEK_API_KEY=home-key\n").expect("global env");

        let workdir = canonical_workdir(&options.workdir).expect("workdir");
        let loaded = load_run_config(&options, &workdir).expect("config");
        let resolved = resolve_run_provider(&options, &loaded).expect("provider");
        assert_eq!(resolved.provider, "deepseek");
        assert_eq!(resolved.model, "deepseek-chat");
        assert_eq!(resolved.base_url, "http://home.example/v1");
        assert_eq!(resolved.api_key, "home-key");
    }

    #[test]
    fn psychevo_home_overrides_default_home() {
        let temp = tempdir().expect("temp");
        let custom_home = temp.path().join("custom-home");
        let mut options = base_options(&temp);
        options.inherited_env = Some(BTreeMap::from([
            (
                "HOME".to_string(),
                temp.path().join("ignored").to_string_lossy().to_string(),
            ),
            (
                "PSYCHEVO_HOME".to_string(),
                custom_home.to_string_lossy().to_string(),
            ),
        ]));
        fs::create_dir_all(&custom_home).expect("home");
        fs::write(
            custom_home.join("config.jsonc"),
            r#"
            {
              "model": "deepseek/deepseek-chat",
              "provider": {
                "deepseek": {
                  "options": {
                    "base_url": "http://custom-home.example/v1",
                    "api_key_env": "DEEPSEEK_API_KEY"
                  },
                  "models": { "deepseek-chat": {} }
                }
              }
            }
            "#,
        )
        .expect("config");
        fs::write(custom_home.join(".env"), "DEEPSEEK_API_KEY=custom-key\n").expect("env");

        let workdir = canonical_workdir(&options.workdir).expect("workdir");
        let loaded = load_run_config(&options, &workdir).expect("config");
        let resolved = resolve_run_provider(&options, &loaded).expect("provider");
        assert_eq!(resolved.base_url, "http://custom-home.example/v1");
        assert_eq!(resolved.api_key, "custom-key");
    }

    #[test]
    fn config_merge_dotenv_precedence_and_provider_qualified_model() {
        let temp = tempdir().expect("temp");
        let options = base_options(&temp);
        let config_dir = home_dir(&temp);
        let project_dir = options.workdir.join(".psychevo");
        fs::create_dir_all(&config_dir).expect("config dir");
        fs::create_dir_all(&project_dir).expect("project dir");
        fs::write(
            config_dir.join("config.jsonc"),
            r#"
            {
              // global default
              "model": "deepseek/deepseek-chat",
              "provider": {
                "deepseek": {
                  "options": {
                    "base_url": "http://global.example/v1",
                    "api_key_env": "DEEPSEEK_API_KEY"
                  },
                  "models": {
                    "deepseek-chat": { "reasoning_effort": "low" }
                  }
                }
              }
            }
            "#,
        )
        .expect("global config");
        fs::write(config_dir.join(".env"), "DEEPSEEK_API_KEY=global-key\n").expect("global env");
        fs::write(
            project_dir.join("config.jsonc"),
            r#"
            {
              "provider": {
                "deepseek": {
                  "options": { "base_url": "http://project.example/v1" },
                  "models": {
                    "deepseek-chat": { "reasoning_effort": "high" }
                  }
                }
              }
            }
            "#,
        )
        .expect("project config");
        fs::write(project_dir.join(".env"), "DEEPSEEK_API_KEY='project-key'\n")
            .expect("project env");

        let workdir = canonical_workdir(&options.workdir).expect("workdir");
        let loaded = load_run_config(&options, &workdir).expect("config");
        let resolved = resolve_run_provider(&options, &loaded).expect("provider");
        assert_eq!(resolved.provider, "deepseek");
        assert_eq!(resolved.model, "deepseek-chat");
        assert_eq!(resolved.base_url, "http://project.example/v1");
        assert_eq!(resolved.api_key, "project-key");
        assert_eq!(resolved.reasoning_effort.as_deref(), Some("high"));
    }

    #[test]
    fn model_object_provider_and_reasoning_effort_are_resolved() {
        let temp = tempdir().expect("temp");
        let options = base_options(&temp);
        let config_dir = home_dir(&temp);
        fs::create_dir_all(&config_dir).expect("config dir");
        fs::write(
            config_dir.join("config.jsonc"),
            r#"
            {
              "model": {
                "provider": "mimo",
                "id": "mimo-v2.5",
                "reasoning_effort": "medium"
              },
              "provider": {
                "xiaomi": {
                  "models": { "mimo-v2.5": {} }
                }
              }
            }
            "#,
        )
        .expect("config");
        fs::write(config_dir.join(".env"), "XIAOMI_API_KEY=xiaomi-key\n").expect("env");

        let workdir = canonical_workdir(&options.workdir).expect("workdir");
        let loaded = load_run_config(&options, &workdir).expect("config");
        let resolved = resolve_run_provider(&options, &loaded).expect("provider");
        assert_eq!(resolved.provider, "xiaomi");
        assert_eq!(resolved.model, "mimo-v2.5");
        assert_eq!(resolved.api_key_env.as_deref(), Some("XIAOMI_API_KEY"));
        assert_eq!(resolved.reasoning_effort.as_deref(), Some("medium"));
    }

    #[test]
    fn raw_api_keys_are_rejected() {
        let temp = tempdir().expect("temp");
        let options = base_options(&temp);
        let config_dir = home_dir(&temp);
        fs::create_dir_all(&config_dir).expect("config dir");
        fs::write(
            config_dir.join("config.jsonc"),
            r#"
            {
              "provider": {
                "custom": {
                  "options": {
                    "base_url": "http://127.0.0.1:1234/v1",
                    "api_key": "secret"
                  },
                  "models": { "local": {} }
                }
              }
            }
            "#,
        )
        .expect("config");

        let workdir = canonical_workdir(&options.workdir).expect("workdir");
        let err = load_run_config(&options, &workdir).expect_err("raw key");
        assert!(err.to_string().contains("raw API keys"));
    }

    #[test]
    fn unique_model_default_and_multiple_model_rejection() {
        let temp = tempdir().expect("temp");
        let options = base_options(&temp);
        let config_dir = home_dir(&temp);
        fs::create_dir_all(&config_dir).expect("config dir");
        fs::write(
            config_dir.join("config.jsonc"),
            r#"
            {
              "provider": {
                "xiaomi": {
                  "models": { "mimo-v2.5": {} }
                }
              }
            }
            "#,
        )
        .expect("config");
        fs::write(config_dir.join(".env"), "XIAOMI_API_KEY=xiaomi-key\n").expect("env");

        let workdir = canonical_workdir(&options.workdir).expect("workdir");
        let loaded = load_run_config(&options, &workdir).expect("config");
        let resolved = resolve_run_provider(&options, &loaded).expect("provider");
        assert_eq!(resolved.model, "mimo-v2.5");

        let mut explicit_options = options.clone();
        explicit_options.model = Some("xiaomi/mimo-v2.5".to_string());
        fs::write(
            config_dir.join("config.jsonc"),
            r#"
            {
              "provider": {
                "xiaomi": {
                  "models": { "one": {}, "two": {} }
                }
              }
            }
            "#,
        )
        .expect("config");
        let loaded = load_run_config(&explicit_options, &workdir).expect("config");
        let resolved = resolve_run_provider(&explicit_options, &loaded).expect("provider");
        assert_eq!(resolved.model, "mimo-v2.5");
    }

    #[test]
    fn cli_provider_qualified_model_selects_provider() {
        let temp = tempdir().expect("temp");
        let mut options = base_options(&temp);
        options.model = Some("deepseek/deepseek-chat".to_string());
        let config_dir = home_dir(&temp);
        fs::create_dir_all(&config_dir).expect("config dir");
        fs::write(
            config_dir.join("config.jsonc"),
            r#"
            {
              "model": "xiaomi/mimo-v2.5",
              "provider": {
                "deepseek": {
                  "models": { "deepseek-chat": {} }
                },
                "xiaomi": {
                  "models": { "mimo-v2.5": {} }
                }
              }
            }
            "#,
        )
        .expect("config");
        fs::write(
            config_dir.join(".env"),
            "DEEPSEEK_API_KEY=deepseek-key\nXIAOMI_API_KEY=xiaomi-key\n",
        )
        .expect("env");

        let workdir = canonical_workdir(&options.workdir).expect("workdir");
        let loaded = load_run_config(&options, &workdir).expect("config");
        let resolved = resolve_run_provider(&options, &loaded).expect("provider");
        assert_eq!(resolved.provider, "deepseek");
        assert_eq!(resolved.model, "deepseek-chat");
    }

    #[test]
    fn aliases_and_auto_resolution_use_local_env_map() {
        let temp = tempdir().expect("temp");
        let mut options = base_options(&temp);
        options.model = Some("qwen/qwen-test".to_string());
        let config_dir = home_dir(&temp);
        fs::create_dir_all(&config_dir).expect("config dir");
        fs::write(
            config_dir.join("config.jsonc"),
            r#"
            {
              "provider": {
                "dashscope": {
                  "models": { "qwen-test": {} }
                }
              }
            }
            "#,
        )
        .expect("config");
        fs::write(config_dir.join(".env"), "DASHSCOPE_API_KEY=qwen-key\n").expect("env");

        let workdir = canonical_workdir(&options.workdir).expect("workdir");
        let loaded = load_run_config(&options, &workdir).expect("config");
        let resolved = resolve_run_provider(&options, &loaded).expect("provider");
        assert_eq!(resolved.provider, "dashscope");
        assert_eq!(resolved.api_key_env.as_deref(), Some("DASHSCOPE_API_KEY"));

        options.model = None;
        options.inherited_env = Some(BTreeMap::from([
            (
                "HOME".to_string(),
                temp.path().to_string_lossy().to_string(),
            ),
            (
                "PSYCHEVO_INFERENCE_MODEL".to_string(),
                "auto-model".to_string(),
            ),
        ]));
        fs::write(
            config_dir.join("config.jsonc"),
            r#"
            {
              "provider": {
                "openrouter": { "models": { "auto-model": {} } },
                "deepseek": { "models": { "auto-model": {} } }
              }
            }
            "#,
        )
        .expect("config");
        fs::write(
            config_dir.join(".env"),
            "DEEPSEEK_API_KEY=deepseek-key\nOPENAI_API_KEY=openai-key\n",
        )
        .expect("env");
        let loaded = load_run_config(&options, &workdir).expect("config");
        let resolved = resolve_run_provider(&options, &loaded).expect("auto");
        assert_eq!(resolved.provider, "openrouter");
    }

    #[test]
    fn explicit_config_replaces_home_and_project_config_but_loads_project_env() {
        let temp = tempdir().expect("temp");
        let mut options = base_options(&temp);
        let explicit_dir = temp.path().join("explicit");
        let project_dir = options.workdir.join(".psychevo");
        fs::create_dir_all(&explicit_dir).expect("explicit dir");
        fs::create_dir_all(&project_dir).expect("project dir");
        fs::create_dir_all(home_dir(&temp)).expect("home dir");
        fs::write(
            home_dir(&temp).join("config.jsonc"),
            r#"{ "model": "deepseek/ignored", "provider": { "deepseek": { "models": { "ignored": {} } } } }"#,
        )
        .expect("home config");
        fs::write(
            project_dir.join("config.jsonc"),
            r#"{ "model": "deepseek/project-ignored" }"#,
        )
        .expect("project config");
        let explicit = explicit_dir.join("config.jsonc");
        fs::write(
            &explicit,
            r#"
            {
              "model": "custom/local",
              "provider": {
                "custom": {
                  "options": {
                    "base_url": "http://127.0.0.1:1234/v1",
                    "api_key_env": "CUSTOM_KEY"
                  },
                  "models": { "local": {} }
                }
              }
            }
            "#,
        )
        .expect("explicit config");
        fs::write(explicit_dir.join(".env"), "CUSTOM_KEY=explicit-key\n").expect("explicit env");
        fs::write(project_dir.join(".env"), "CUSTOM_KEY=project-key\n").expect("project env");
        options.config_path = Some(explicit);

        let workdir = canonical_workdir(&options.workdir).expect("workdir");
        let loaded = load_run_config(&options, &workdir).expect("config");
        let resolved = resolve_run_provider(&options, &loaded).expect("provider");
        assert_eq!(resolved.provider, "custom");
        assert_eq!(resolved.model, "local");
        assert_eq!(resolved.api_key, "project-key");
    }

    #[test]
    fn psychevo_config_env_is_supported_and_config_dir_is_ignored() {
        let temp = tempdir().expect("temp");
        let mut options = base_options(&temp);
        let old_dir = temp.path().join("old-config-dir");
        let explicit_dir = temp.path().join("explicit");
        fs::create_dir_all(&old_dir).expect("old dir");
        fs::create_dir_all(&explicit_dir).expect("explicit dir");
        fs::write(
            old_dir.join("config.jsonc"),
            r#"{ "model": "deepseek/old", "provider": { "deepseek": { "models": { "old": {} } } } }"#,
        )
        .expect("old config");
        let explicit = explicit_dir.join("config.jsonc");
        fs::write(
            &explicit,
            r#"
            {
              "model": "custom/local",
              "provider": {
                "custom": {
                  "options": { "base_url": "http://127.0.0.1:1234/v1" },
                  "models": { "local": {} }
                }
              }
            }
            "#,
        )
        .expect("explicit config");
        options.inherited_env = Some(BTreeMap::from([
            (
                "HOME".to_string(),
                temp.path().to_string_lossy().to_string(),
            ),
            (
                "PSYCHEVO_CONFIG".to_string(),
                explicit.to_string_lossy().to_string(),
            ),
            (
                "PSYCHEVO_CONFIG_DIR".to_string(),
                old_dir.to_string_lossy().to_string(),
            ),
        ]));

        let workdir = canonical_workdir(&options.workdir).expect("workdir");
        let loaded = load_run_config(&options, &workdir).expect("config");
        let resolved = resolve_run_provider(&options, &loaded).expect("provider");
        assert_eq!(resolved.provider, "custom");
        assert_eq!(resolved.model, "local");
    }

    #[test]
    fn missing_home_config_rejects_before_agent_start() {
        let temp = tempdir().expect("temp");
        let options = base_options(&temp);
        let workdir = canonical_workdir(&options.workdir).expect("workdir");
        let err = load_run_config(&options, &workdir).expect_err("missing home");
        assert!(err.to_string().contains("pevo init"));
    }

    #[test]
    fn reasoning_effort_values_are_validated_and_none_disables() {
        let temp = tempdir().expect("temp");
        let mut options = base_options(&temp);
        let config_dir = home_dir(&temp);
        fs::create_dir_all(&config_dir).expect("config dir");
        fs::write(
            config_dir.join("config.jsonc"),
            r#"
            {
              "model": "custom/local",
              "provider": {
                "custom": {
                  "options": { "base_url": "http://127.0.0.1:1234/v1" },
                  "models": { "local": { "reasoning_effort": "high" } }
                }
              }
            }
            "#,
        )
        .expect("config");

        let workdir = canonical_workdir(&options.workdir).expect("workdir");
        let loaded = load_run_config(&options, &workdir).expect("config");
        let resolved = resolve_run_provider(&options, &loaded).expect("provider");
        assert_eq!(resolved.reasoning_effort.as_deref(), Some("high"));

        options.reasoning_effort = Some("none".to_string());
        let resolved = resolve_run_provider(&options, &loaded).expect("provider");
        assert_eq!(resolved.reasoning_effort, None);

        options.reasoning_effort = Some("turbo".to_string());
        let err = resolve_run_provider(&options, &loaded).expect_err("invalid");
        assert!(err.to_string().contains("reasoning_effort"));
    }

    #[test]
    fn latest_run_session_filters_source_and_workdir() {
        let temp = tempdir().expect("temp");
        let db = temp.path().join("state.db");
        let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
        let other_workdir = canonical_workdir(&temp.path().join("other")).expect("other");
        let store = SqliteStore::open(&db).expect("store");
        let smoke = store.create_session(&workdir).expect("smoke");
        let other = store
            .create_session_with_metadata(&other_workdir, "run", "model", "provider", None)
            .expect("other");
        let first = store
            .create_session_with_metadata(&workdir, "run", "model", "provider", None)
            .expect("first");
        let second = store
            .create_session_with_metadata(&workdir, "run", "model", "provider", None)
            .expect("second");
        thread::sleep(Duration::from_millis(2));
        store.touch_session(&first).expect("touch");

        let latest = latest_run_session_for_workdir(&db, &workdir)
            .expect("latest")
            .expect("session");
        assert_eq!(latest, first);
        assert_ne!(latest, second);
        assert_ne!(latest, smoke);
        assert_ne!(latest, other);
    }

    #[test]
    fn sqlite_schema_v3_rejects_old_state_databases() {
        let temp = tempdir().expect("temp");
        let db = temp.path().join("old.db");
        {
            let conn = Connection::open(&db).expect("db");
            conn.pragma_update(None, "user_version", 1)
                .expect("version");
            conn.execute_batch("CREATE TABLE sessions (id TEXT);")
                .expect("schema");
        }

        let err = match SqliteStore::open(&db) {
            Ok(_) => panic!("old db opened successfully"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("schema version 1"));
        assert!(err.to_string().contains("--reset-state"));

        let v2_db = temp.path().join("v2.db");
        {
            let conn = Connection::open(&v2_db).expect("db");
            conn.pragma_update(None, "user_version", 2)
                .expect("version");
            conn.execute_batch("CREATE TABLE sessions (id TEXT);")
                .expect("schema");
        }
        let err = match SqliteStore::open(&v2_db) {
            Ok(_) => panic!("v2 db opened successfully"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("schema version 2"));
        assert!(err.to_string().contains("--reset-state"));
    }

    #[test]
    fn sqlite_schema_v3_stores_reasoning_only_in_message_json_and_metrics_separately() {
        let temp = tempdir().expect("temp");
        let db = temp.path().join("state.db");
        let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
        let store = SqliteStore::open(&db).expect("store");
        let session_id = store
            .create_session_with_metadata(&workdir, "run", "model", "provider", None)
            .expect("session");
        store
            .append_message_with_metrics(
                &session_id,
                &Message::Assistant {
                    content: vec![
                        AssistantBlock::Reasoning {
                            text: "folded".to_string(),
                            provider_evidence: Some(json!({
                                "reasoning_details": [{ "type": "thinking", "text": "opaque" }]
                            })),
                        },
                        AssistantBlock::Text {
                            text: "visible".to_string(),
                        },
                    ],
                    timestamp_ms: 1,
                    finish_reason: Some("stop".to_string()),
                    outcome: Outcome::Normal,
                    model: Some("model".to_string()),
                    provider: Some("provider".to_string()),
                },
                Some(json!({"total_tokens": 12, "input_tokens": 5, "output_tokens": 7})),
                Some(json!({"provider_response_id": "resp_1", "model": "model"})),
            )
            .expect("append");

        let conn = Connection::open(&db).expect("db");
        let columns = conn
            .prepare("PRAGMA table_info(messages)")
            .expect("schema stmt")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("schema rows")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("columns");
        assert!(!columns.iter().any(|name| name == "reasoning_json"));
        assert!(!columns.iter().any(|name| name == "reasoning_content"));
        assert!(!columns.iter().any(|name| name == "reasoning_details_json"));

        let (message_json, usage_json, metadata_json): (String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT message_json, usage_json, metadata_json FROM messages WHERE session_id = ?1",
                [&session_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("message row");
        let message: Value = serde_json::from_str(&message_json).expect("message");
        assert_eq!(message["content"][0]["type"], "reasoning");
        assert_eq!(message["content"][0]["text"], "folded");
        assert_eq!(
            message["content"][0]["provider_evidence"]["reasoning_details"][0]["type"],
            "thinking"
        );
        assert!(message.get("reasoning_content").is_none());
        assert!(message.get("reasoning_details").is_none());
        assert!(message.get("usage").is_none());
        assert!(message.get("metadata").is_none());

        let usage: Value = serde_json::from_str(&usage_json.expect("usage")).expect("usage json");
        let metadata: Value =
            serde_json::from_str(&metadata_json.expect("metadata")).expect("metadata json");
        assert_eq!(usage["total_tokens"], 12);
        assert_eq!(metadata["provider_response_id"], "resp_1");

        let summaries = store
            .load_sanitized_message_summaries(&session_id)
            .expect("summaries");
        assert_eq!(summaries[0].usage.as_ref().unwrap()["total_tokens"], 12);
        assert_eq!(
            summaries[0].metadata.as_ref().unwrap()["provider_response_id"],
            "resp_1"
        );
        let sanitized = serde_json::to_string(&summaries[0].message).expect("sanitized");
        assert!(!sanitized.contains("folded"));
    }

    #[test]
    fn json_projection_hides_reasoning_unless_included() {
        let message = Message::Assistant {
            content: vec![
                AssistantBlock::Reasoning {
                    text: "private".to_string(),
                    provider_evidence: Some(json!({
                        "reasoning_details": [{ "type": "thinking" }]
                    })),
                },
                AssistantBlock::Text {
                    text: "visible".to_string(),
                },
            ],
            timestamp_ms: 1,
            finish_reason: Some("stop".to_string()),
            outcome: Outcome::Normal,
            model: Some("model".to_string()),
            provider: Some("provider".to_string()),
        };
        let event = AgentEvent::MessageEnd {
            message: message.clone(),
            usage: Some(json!({"total_tokens": 2})),
            metadata: Some(json!({"provider_response_id": "resp"})),
        };
        let hidden = project_agent_event(&event, false).expect("hidden");
        let hidden_text = serde_json::to_string(&hidden).expect("hidden json");
        assert!(hidden_text.contains("visible"));
        assert!(!hidden_text.contains("private"));
        assert!(!hidden_text.contains("reasoning_content"));
        assert!(!hidden_text.contains("total_tokens"));

        assert!(
            project_agent_event(&AgentEvent::ReasoningDelta { text: "x".into() }, false).is_none()
        );
        let shown = project_agent_event(&AgentEvent::ReasoningDelta { text: "x".into() }, true)
            .expect("shown");
        assert_eq!(shown, json!({"type":"reasoning_delta","text":"x"}));

        let stream = project_run_stream_event(&AgentEvent::ReasoningDelta { text: "x".into() })
            .expect("stream");
        assert_eq!(
            stream,
            RunStreamEvent::ReasoningDelta {
                text: "x".to_string()
            }
        );
        let metrics = project_run_stream_event(&event).expect("metrics");
        match metrics {
            RunStreamEvent::Event(value) => {
                assert_eq!(value["usage"]["total_tokens"], 2);
                assert_eq!(value["metadata"]["provider_response_id"], "resp");
                assert!(!serde_json::to_string(&value).unwrap().contains("private"));
            }
            other => panic!("unexpected stream event: {other:?}"),
        }
    }
}
