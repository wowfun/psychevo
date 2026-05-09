#[derive(Debug, Error)]
pub enum Error {
    #[error("fake provider script exhausted")]
    ScriptExhausted,
    #[error("provider failed: {0}")]
    Provider(String),
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON failed: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Normal,
    Stopped,
    Failed,
    Aborted,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
            Self::Aborted => "aborted",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelTarget {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationRequest {
    pub model: ModelTarget,
    pub messages: Vec<Value>,
    pub tools: Vec<ToolDeclaration>,
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDeclaration {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    TextDelta {
        text: String,
    },
    ReasoningDelta {
        text: String,
        reasoning_content: Option<String>,
    },
    ReasoningDetails {
        details: Value,
    },
    ToolCallStart {
        content_index: usize,
        call_index: usize,
        id: String,
        name: String,
    },
    ToolCallDelta {
        content_index: usize,
        call_index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    },
    ToolCallEnd {
        content_index: usize,
        call_index: usize,
    },
    Usage {
        usage: Value,
    },
    Metadata {
        metadata: Value,
    },
    Done {
        outcome: Outcome,
        finish_reason: Option<String>,
    },
}

