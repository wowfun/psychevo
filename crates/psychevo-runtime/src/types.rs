use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use psychevo_agent_core::{ControlHandle, ControlReceivers, Message};
use psychevo_ai::Outcome;
use serde::{Deserialize, Serialize};
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
    pub image_inputs: Vec<ImageInput>,
    pub extract_prompt_image_sources: bool,
    pub prompt_display: Option<PromptDisplayMetadata>,
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

pub const TUI_DISPLAY_METADATA_KEY: &str = "tui_display";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptDisplayMetadata {
    pub content_text: String,
    pub attachments: Vec<PromptAttachmentDisplay>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptAttachmentDisplay {
    pub kind: String,
    pub placeholder: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageInput {
    LocalPath(PathBuf),
    ImageUrl(String),
}

impl ImageInput {
    pub fn display_source(&self) -> String {
        match self {
            Self::LocalPath(path) => path.display().to_string(),
            Self::ImageUrl(url) => url.clone(),
        }
    }
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
    pub context_snapshot: Option<crate::context_usage::ContextSnapshot>,
    pub events: Vec<Value>,
}

#[derive(Debug, Clone)]
pub struct UserShellOptions {
    pub workdir: PathBuf,
    pub command: String,
}

#[derive(Debug, Clone)]
pub struct StatsOptions {
    pub db_path: PathBuf,
    pub workdir: PathBuf,
    pub all: bool,
    pub days: Option<u64>,
    pub limit: usize,
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

#[derive(Debug, Clone, PartialEq)]
pub struct ConfiguredModel {
    pub provider: String,
    pub provider_label: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub context_limit: Option<u64>,
    pub metadata: ModelMetadata,
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

#[derive(Debug, Clone, PartialEq)]
pub struct ModelCatalogEntry {
    pub id: String,
    pub context_limit: Option<u64>,
    pub metadata: ModelMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelMetadataCacheTarget {
    pub provider: String,
    pub model: String,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelMetadata {
    pub limits: ModelLimits,
    pub cost: Option<ModelCost>,
    pub capabilities: ModelCapabilities,
    pub source: Option<String>,
    pub raw: Option<Value>,
}

impl ModelMetadata {
    pub fn context_limit(&self) -> Option<u64> {
        self.limits.context
    }

    pub fn public_json(&self) -> Value {
        let mut object = serde_json::Map::new();
        let limits = self.limits.public_json();
        if !limits.as_object().is_none_or(|object| object.is_empty()) {
            object.insert("limit".to_string(), limits);
        }
        if let Some(cost) = &self.cost {
            object.insert("cost".to_string(), cost.public_json());
        }
        let capabilities = self.capabilities.public_json();
        if !capabilities
            .as_object()
            .is_none_or(|object| object.is_empty())
        {
            object.insert("capabilities".to_string(), capabilities);
        }
        if let Some(source) = &self.source {
            object.insert("source".to_string(), Value::String(source.clone()));
        }
        Value::Object(object)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelLimits {
    pub context: Option<u64>,
    pub input: Option<u64>,
    pub output: Option<u64>,
}

impl ModelLimits {
    fn public_json(&self) -> Value {
        let mut object = serde_json::Map::new();
        if let Some(value) = self.context {
            object.insert("context".to_string(), Value::from(value));
        }
        if let Some(value) = self.input {
            object.insert("input".to_string(), Value::from(value));
        }
        if let Some(value) = self.output {
            object.insert("output".to_string(), Value::from(value));
        }
        Value::Object(object)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelCost {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
    pub context_over_200k: Option<ModelCostTier>,
    pub source: Option<String>,
}

impl ModelCost {
    fn public_json(&self) -> Value {
        let mut object = serde_json::Map::new();
        if let Some(value) = self.input {
            object.insert("input".to_string(), Value::from(value));
        }
        if let Some(value) = self.output {
            object.insert("output".to_string(), Value::from(value));
        }
        if let Some(value) = self.cache_read {
            object.insert("cache_read".to_string(), Value::from(value));
        }
        if let Some(value) = self.cache_write {
            object.insert("cache_write".to_string(), Value::from(value));
        }
        if let Some(tier) = &self.context_over_200k {
            object.insert("context_over_200k".to_string(), tier.public_json());
        }
        if let Some(source) = &self.source {
            object.insert("source".to_string(), Value::String(source.clone()));
        }
        Value::Object(object)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelCostTier {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}

impl ModelCostTier {
    fn public_json(&self) -> Value {
        let mut object = serde_json::Map::new();
        if let Some(value) = self.input {
            object.insert("input".to_string(), Value::from(value));
        }
        if let Some(value) = self.output {
            object.insert("output".to_string(), Value::from(value));
        }
        if let Some(value) = self.cache_read {
            object.insert("cache_read".to_string(), Value::from(value));
        }
        if let Some(value) = self.cache_write {
            object.insert("cache_write".to_string(), Value::from(value));
        }
        Value::Object(object)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub reasoning: Option<bool>,
    pub tool_call: Option<bool>,
    pub temperature: Option<bool>,
    pub attachment: Option<bool>,
    pub structured_output: Option<bool>,
    pub interleaved: Option<Value>,
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
}

impl ModelCapabilities {
    fn public_json(&self) -> Value {
        let mut object = serde_json::Map::new();
        if let Some(value) = self.reasoning {
            object.insert("reasoning".to_string(), Value::Bool(value));
        }
        if let Some(value) = self.tool_call {
            object.insert("tool_call".to_string(), Value::Bool(value));
        }
        if let Some(value) = self.temperature {
            object.insert("temperature".to_string(), Value::Bool(value));
        }
        if let Some(value) = self.attachment {
            object.insert("attachment".to_string(), Value::Bool(value));
        }
        if let Some(value) = self.structured_output {
            object.insert("structured_output".to_string(), Value::Bool(value));
        }
        if let Some(value) = &self.interleaved {
            object.insert("interleaved".to_string(), value.clone());
        }
        if !self.input_modalities.is_empty() || !self.output_modalities.is_empty() {
            let mut modalities = serde_json::Map::new();
            if !self.input_modalities.is_empty() {
                modalities.insert(
                    "input".to_string(),
                    Value::Array(
                        self.input_modalities
                            .iter()
                            .map(|value| Value::String(value.clone()))
                            .collect(),
                    ),
                );
            }
            if !self.output_modalities.is_empty() {
                modalities.insert(
                    "output".to_string(),
                    Value::Array(
                        self.output_modalities
                            .iter()
                            .map(|value| Value::String(value.clone()))
                            .collect(),
                    ),
                );
            }
            object.insert("modalities".to_string(), Value::Object(modalities));
        }
        Value::Object(object)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageAccounting {
    pub context_input_tokens: Option<u64>,
    pub billable_input_tokens: Option<u64>,
    pub billable_output_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reported_total_tokens: Option<u64>,
    pub estimated_cost_nanodollars: Option<i64>,
    pub pricing_source: Option<String>,
    pub pricing_tier: Option<String>,
}

impl MessageAccounting {
    pub fn public_json(&self) -> Value {
        let mut object = serde_json::Map::new();
        if let Some(value) = self.context_input_tokens {
            object.insert("context_input_tokens".to_string(), Value::from(value));
        }
        if let Some(value) = self.billable_input_tokens {
            object.insert("billable_input_tokens".to_string(), Value::from(value));
        }
        if let Some(value) = self.billable_output_tokens {
            object.insert("billable_output_tokens".to_string(), Value::from(value));
        }
        if let Some(value) = self.reasoning_tokens {
            object.insert("reasoning_tokens".to_string(), Value::from(value));
        }
        if let Some(value) = self.cache_read_tokens {
            object.insert("cache_read_tokens".to_string(), Value::from(value));
        }
        if let Some(value) = self.cache_write_tokens {
            object.insert("cache_write_tokens".to_string(), Value::from(value));
        }
        if let Some(value) = self.reported_total_tokens {
            object.insert("reported_total_tokens".to_string(), Value::from(value));
        }
        if let Some(value) = self.estimated_cost_nanodollars {
            object.insert("estimated_cost_nanodollars".to_string(), Value::from(value));
        }
        if let Some(value) = &self.pricing_source {
            object.insert("pricing_source".to_string(), Value::String(value.clone()));
        }
        if let Some(value) = &self.pricing_tier {
            object.insert("pricing_tier".to_string(), Value::String(value.clone()));
        }
        Value::Object(object)
    }
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
    pub accounting: Option<Value>,
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
