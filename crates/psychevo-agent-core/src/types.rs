#[allow(unused_imports)]
pub(crate) use super::*;
#[derive(Debug, Error)]
pub enum Error {
    #[error("provider failed: {0}")]
    Provider(#[from] psychevo_ai::Error),
    #[error("event sink failed: {0}")]
    EventSink(String),
    #[error("agent failed: {0}")]
    Agent(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TerminalReason {
    MaxTurnsExceeded { max_turns: usize },
}

impl TerminalReason {
    pub fn message(self) -> String {
        match self {
            Self::MaxTurnsExceeded { max_turns } => format!(
                "reached model-turn limit ({max_turns}) before final answer; resume this session to continue."
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Message {
    User {
        content: Vec<UserContentBlock>,
        timestamp_ms: i64,
    },
    Assistant {
        content: Vec<AssistantBlock>,
        timestamp_ms: i64,
        finish_reason: Option<String>,
        outcome: Outcome,
        model: Option<String>,
        provider: Option<String>,
    },
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        content: String,
        is_error: bool,
        timestamp_ms: i64,
    },
}

impl Message {
    pub fn role(&self) -> &'static str {
        match self {
            Self::User { .. } => "user",
            Self::Assistant { .. } => "assistant",
            Self::ToolResult { .. } => "tool_result",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextualUserBlock {
    pub kind: String,
    pub source_name: Option<String>,
    pub source_path: Option<String>,
    pub text: String,
    pub hidden: bool,
}

impl ContextualUserBlock {
    pub fn new(
        kind: impl Into<String>,
        source_name: Option<String>,
        source_path: Option<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            kind: kind.into(),
            source_name,
            source_path,
            text: text.into(),
            hidden: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextualUserMessage {
    pub provider_group: String,
    pub context_category: String,
    pub blocks: Vec<ContextualUserBlock>,
    pub hidden: bool,
    pub timestamp_ms: i64,
}

impl ContextualUserMessage {
    pub fn new(provider_group: impl Into<String>, blocks: Vec<ContextualUserBlock>) -> Self {
        Self::new_with_category(provider_group, "turn_context", blocks)
    }

    pub fn new_with_category(
        provider_group: impl Into<String>,
        context_category: impl Into<String>,
        blocks: Vec<ContextualUserBlock>,
    ) -> Self {
        Self {
            provider_group: provider_group.into(),
            context_category: context_category.into(),
            blocks,
            hidden: true,
            timestamp_ms: now_ms(),
        }
    }

    pub fn to_provider_value(&self) -> Value {
        json!({
            "role": "user",
            "content": self.blocks.iter().map(|block| {
                json!({
                    "type": "contextual_text",
                    "text": block.text,
                    "context_kind": block.kind,
                    "source_name": block.source_name,
                    "source_path": block.source_path,
                    "hidden": block.hidden,
                })
            }).collect::<Vec<_>>(),
            "timestamp_ms": self.timestamp_ms,
            "metadata": {
                "contextual_user": true,
                "provider_group": self.provider_group,
                "context_category": self.context_category,
                "hidden": self.hidden,
            },
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextBlock {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum UserContentBlock {
    Text(TextBlock),
    LocalImage(LocalImageBlock),
    ImageUrl(ImageUrlBlock),
}

impl UserContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(TextBlock { text: text.into() })
    }

    pub fn local_image(path: impl Into<std::path::PathBuf>) -> Self {
        Self::LocalImage(LocalImageBlock {
            kind: LocalImageBlockKind::LocalImage,
            path: path.into(),
        })
    }

    pub fn image_url(url: impl Into<String>) -> Self {
        Self::ImageUrl(ImageUrlBlock {
            kind: ImageUrlBlockKind::ImageUrl,
            url: url.into(),
        })
    }

    pub fn text_value(&self) -> Option<&str> {
        match self {
            Self::Text(block) => Some(block.text.as_str()),
            Self::LocalImage(_) => None,
            Self::ImageUrl(_) => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalImageBlock {
    #[serde(rename = "type")]
    pub kind: LocalImageBlockKind,
    pub path: std::path::PathBuf,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalImageBlockKind {
    LocalImage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageUrlBlock {
    #[serde(rename = "type")]
    pub kind: ImageUrlBlockKind,
    pub url: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImageUrlBlockKind {
    ImageUrl,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssistantBlock {
    Text {
        text: String,
    },
    Reasoning {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_evidence: Option<Value>,
    },
    ToolCall(ToolCallBlock),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCallBlock {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    pub arguments_json: String,
    pub arguments_error: Option<String>,
    pub content_index: usize,
    pub call_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolExecutionMode {
    Parallel,
    Sequential,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolExposure {
    Direct,
    Deferred,
    Hidden,
}

impl ToolExposure {
    pub fn is_model_visible(self) -> bool {
        matches!(self, Self::Direct)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolDisplayCategory {
    Explore,
    Run,
    Update,
    Status,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolDisplayBodyPolicy {
    Summary,
    Body,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolDisplaySpec {
    pub category: ToolDisplayCategory,
    pub title_arg_keys: Vec<String>,
    pub title_result_keys: Vec<String>,
    pub summary_keys: Vec<String>,
    pub body_keys: Vec<String>,
    pub body_policy: ToolDisplayBodyPolicy,
}

impl ToolDisplaySpec {
    pub fn for_name(name: &str) -> Self {
        match name {
            "read" => Self::explore(),
            "exec_command" | "write_stdin" => Self::run(),
            "write" | "edit" => Self::update(),
            "clarify" => Self::status(),
            "web_fetch" => Self::web_fetch(),
            _ => Self::generic(),
        }
    }

    pub fn generic() -> Self {
        Self {
            category: ToolDisplayCategory::Run,
            title_arg_keys: string_keys(["path", "cmd", "url", "query", "name", "session_id"]),
            title_result_keys: string_keys(["path", "url", "final_url", "name", "session_id"]),
            summary_keys: string_keys([
                "error",
                "summary",
                "status",
                "path",
                "files_modified",
                "bytes_written",
                "exit_code",
                "truncated",
                "url",
                "final_url",
                "content_type",
                "output_bytes",
                "original_bytes",
            ]),
            body_keys: string_keys(["content", "output", "diff"]),
            body_policy: ToolDisplayBodyPolicy::Summary,
        }
    }

    pub fn explore() -> Self {
        Self {
            category: ToolDisplayCategory::Explore,
            body_policy: ToolDisplayBodyPolicy::Body,
            body_keys: string_keys(["content", "output", "error"]),
            ..Self::generic()
        }
    }

    pub fn run() -> Self {
        Self {
            category: ToolDisplayCategory::Run,
            title_arg_keys: string_keys(["cmd", "session_id", "path", "url", "query", "name"]),
            body_policy: ToolDisplayBodyPolicy::Body,
            body_keys: string_keys(["output", "content", "error"]),
            ..Self::generic()
        }
    }

    pub fn update() -> Self {
        Self {
            category: ToolDisplayCategory::Update,
            body_policy: ToolDisplayBodyPolicy::Body,
            body_keys: string_keys(["diff", "output", "error"]),
            ..Self::generic()
        }
    }

    pub fn status() -> Self {
        Self {
            category: ToolDisplayCategory::Status,
            ..Self::generic()
        }
    }

    pub fn web_fetch() -> Self {
        Self {
            category: ToolDisplayCategory::Explore,
            title_arg_keys: string_keys(["url"]),
            title_result_keys: string_keys(["final_url", "url"]),
            summary_keys: string_keys([
                "error",
                "status",
                "final_url",
                "content_type",
                "output_bytes",
                "original_bytes",
                "truncated",
            ]),
            body_keys: string_keys(["content"]),
            body_policy: ToolDisplayBodyPolicy::Summary,
        }
    }
}

pub(crate) fn string_keys(keys: impl IntoIterator<Item = &'static str>) -> Vec<String> {
    keys.into_iter().map(str::to_string).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolOutput {
    pub json: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<ToolAttachment>,
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolAttachment {
    ImageUrl {
        url: String,
        mime_type: String,
        source_url: Option<String>,
    },
}

impl ToolOutput {
    pub fn ok(json: Value) -> Self {
        Self {
            json,
            model_content: None,
            attachments: Vec::new(),
            is_error: false,
        }
    }

    pub fn ok_with_model_content(json: Value, model_content: impl Into<String>) -> Self {
        Self {
            json,
            model_content: Some(model_content.into()),
            attachments: Vec::new(),
            is_error: false,
        }
    }

    pub fn with_model_content(mut self, model_content: impl Into<String>) -> Self {
        self.model_content = Some(model_content.into());
        self
    }

    pub fn with_attachment(mut self, attachment: ToolAttachment) -> Self {
        self.attachments.push(attachment);
        self
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            json: json!({ "error": message.into() }),
            model_content: None,
            attachments: Vec::new(),
            is_error: true,
        }
    }

    pub fn model_content(&self) -> String {
        self.model_content.clone().unwrap_or_else(|| {
            serde_json::to_string(&self.json)
                .unwrap_or_else(|_| "{\"error\":\"invalid result\"}".to_string())
        })
    }
}

pub trait ToolBinding: Send + Sync {
    fn name(&self) -> &str;
    fn canonical_tool_name(&self) -> psychevo_ai::ToolName {
        psychevo_ai::ToolName::plain(self.name())
    }
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;
    fn search_metadata(&self) -> Vec<String> {
        Vec::new()
    }
    fn exposure(&self) -> ToolExposure {
        ToolExposure::Direct
    }
    fn execution_mode(&self) -> ToolExecutionMode;
    fn display_spec(&self) -> ToolDisplaySpec {
        ToolDisplaySpec::for_name(self.name())
    }
    fn execute(
        &self,
        tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput>;
}
