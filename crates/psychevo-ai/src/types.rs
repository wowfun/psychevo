#[allow(unused_imports)]
pub(crate) use super::*;
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
    pub tools: Vec<GenerationTool>,
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostedWebSearchTool {
    pub config: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GenerationTool {
    Function { declaration: ToolDeclaration },
    WebSearch(HostedWebSearchTool),
}

impl From<ToolDeclaration> for GenerationTool {
    fn from(declaration: ToolDeclaration) -> Self {
        Self::Function { declaration }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UrlCitationSource {
    pub url: String,
    pub title: String,
    pub start_index: Option<usize>,
    pub end_index: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageSearchSource {
    pub image_url: String,
    pub thumbnail_url: Option<String>,
    pub source_website_url: String,
    pub caption: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssistantSource {
    UrlCitation(UrlCitationSource),
    Image(ImageSearchSource),
    Provider { kind: String, data: Value },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ToolName {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    pub name: String,
}

impl ToolName {
    pub fn plain(name: impl Into<String>) -> Self {
        Self {
            namespace: None,
            name: name.into(),
        }
    }

    pub fn namespaced(namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            namespace: Some(namespace.into()),
            name: name.into(),
        }
    }

    pub fn provider_fallback_name(&self) -> String {
        match &self.namespace {
            Some(namespace) if !namespace.is_empty() => {
                format!("{namespace}__{}", self.name)
            }
            _ => self.name.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolDeclaration {
    /// Provider-visible fallback name for adapter families that do not support
    /// tool namespaces.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_name: Option<String>,
    pub description: String,
    pub parameters: Value,
}

impl ToolDeclaration {
    pub fn new(name: impl Into<String>, description: impl Into<String>, parameters: Value) -> Self {
        Self {
            name: name.into(),
            namespace: None,
            canonical_name: None,
            description: description.into(),
            parameters,
        }
    }

    pub fn with_canonical_name(mut self, canonical: ToolName) -> Self {
        self.namespace = canonical.namespace;
        self.canonical_name = Some(canonical.name);
        self
    }

    pub fn canonical_tool_name(&self) -> ToolName {
        ToolName {
            namespace: self.namespace.clone(),
            name: self
                .canonical_name
                .clone()
                .unwrap_or_else(|| self.name.clone()),
        }
    }
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
    ProviderToolStart {
        id: String,
        name: String,
        action: Option<Value>,
    },
    ProviderToolEnd {
        id: String,
        name: String,
        action: Option<Value>,
        status: String,
    },
    Source {
        source: AssistantSource,
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
