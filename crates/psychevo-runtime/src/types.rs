use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use psychevo_agent_core::{ControlHandle, ControlReceivers, Message};
use psychevo_ai::Outcome;
use serde_json::Value;

use crate::skills::SelectedSkill;

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
    pub snapshot_root: Option<PathBuf>,
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
    pub no_skills: bool,
    pub skill_inputs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomProviderInput {
    pub home: PathBuf,
    pub provider_id: String,
    pub label: String,
    pub base_url: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomProviderResult {
    pub provider_id: String,
    pub label: String,
    pub base_url: String,
    pub api_key_env: String,
    pub wrote_api_key: bool,
    pub reused_existing_api_key: bool,
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
            Self::Build => "default",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "plan" => Some(Self::Plan),
            "default" => Some(Self::Build),
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
    pub selected_skills: Vec<SelectedSkill>,
    pub events: Vec<Value>,
}

#[derive(Debug, Clone)]
pub struct UserShellOptions {
    pub workdir: PathBuf,
    pub command: String,
}

#[derive(Debug, Clone)]
pub struct UserShellResult {
    pub command: String,
    pub workdir: PathBuf,
    pub outcome: Outcome,
    pub tool_failures: usize,
    pub result: Value,
}

#[derive(Debug, Clone)]
pub struct SessionUndoOptions {
    pub db_path: PathBuf,
    pub workdir: PathBuf,
    pub snapshot_root: PathBuf,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionUndoResult {
    pub session_id: String,
    pub prompt: String,
    pub reverted_messages: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRedoResult {
    pub session_id: String,
    pub restored_messages: usize,
    pub complete: bool,
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
    pub archived_at_ms: Option<i64>,
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

#[derive(Clone, PartialEq, Eq)]
pub struct ModelCatalogProvider {
    pub provider: String,
    pub display_label: String,
    pub base_url: String,
    pub api_key_env: Option<String>,
    pub missing_credentials: Option<String>,
    pub unavailable_reason: Option<String>,
    pub no_auth: bool,
    pub(crate) api_key: Option<String>,
}

impl ModelCatalogProvider {
    pub fn fetchable(&self) -> bool {
        self.missing_credentials.is_none() && self.unavailable_reason.is_none()
    }
}

impl fmt::Debug for ModelCatalogProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ModelCatalogProvider")
            .field("provider", &self.provider)
            .field("display_label", &self.display_label)
            .field("base_url", &self.base_url)
            .field("api_key_env", &self.api_key_env)
            .field("missing_credentials", &self.missing_credentials)
            .field("unavailable_reason", &self.unavailable_reason)
            .field("no_auth", &self.no_auth)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCatalogEntry {
    pub id: String,
    pub context_limit: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SanitizedMessageSummary {
    pub message: Message,
    pub usage: Option<Value>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TuiMessageSummary {
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
    pub(crate) inner: ControlHandle,
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
    pub(crate) handle: RunControlHandle,
    pub(crate) receivers: ControlReceivers,
}

pub fn run_control() -> (RunControlHandle, RunControl) {
    let (inner, receivers) = ControlHandle::new();
    let handle = RunControlHandle { inner };
    (handle.clone(), RunControl { handle, receivers })
}
