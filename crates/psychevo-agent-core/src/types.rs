#[derive(Debug, Error)]
pub enum Error {
    #[error("provider failed: {0}")]
    Provider(#[from] psychevo_ai::Error),
    #[error("event sink failed: {0}")]
    EventSink(String),
    #[error("agent failed: {0}")]
    Agent(String),
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolOutput {
    pub json: Value,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn ok(json: Value) -> Self {
        Self {
            json,
            is_error: false,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            json: json!({ "error": message.into() }),
            is_error: true,
        }
    }
}

pub trait ToolBinding: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;
    fn execution_mode(&self) -> ToolExecutionMode;
    fn execute(
        &self,
        tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput>;
}
