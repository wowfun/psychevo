pub const JSONRPC_VERSION: &str = "2.0";

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, TS)]
#[serde(transparent)]
#[ts(type = "string")]
pub struct SourceKey(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum GatewaySourceLifetime {
    Invocation,
    Process,
    Persistent,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewaySource {
    pub kind: String,
    pub raw_id: String,
    pub lifetime: GatewaySourceLifetime,
    #[serde(default)]
    #[ts(type = "unknown | null")]
    pub raw_identity: Option<Value>,
    #[serde(default)]
    pub visible_name: Option<String>,
}

impl GatewaySource {
    pub fn new(kind: impl Into<String>, raw_id: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            raw_id: raw_id.into(),
            lifetime: GatewaySourceLifetime::Invocation,
            raw_identity: None,
            visible_name: None,
        }
    }

    pub fn invocation(mut self) -> Self {
        self.lifetime = GatewaySourceLifetime::Invocation;
        self
    }

    pub fn process(mut self) -> Self {
        self.lifetime = GatewaySourceLifetime::Process;
        self
    }

    pub fn persistent(mut self) -> Self {
        self.lifetime = GatewaySourceLifetime::Persistent;
        self
    }

    pub fn with_raw_identity(mut self, raw_identity: Value) -> Self {
        self.raw_identity = Some(raw_identity);
        self
    }

    pub fn with_visible_name(mut self, visible_name: impl Into<String>) -> Self {
        self.visible_name = Some(visible_name.into());
        self
    }

    pub fn source_key(&self) -> SourceKey {
        SourceKey(format!("{}:{}", self.kind, self.raw_id))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewaySourceInput {
    pub kind: String,
    #[serde(default)]
    pub raw_id: Option<String>,
    #[serde(default)]
    pub lifetime: Option<GatewaySourceLifetime>,
    #[serde(default)]
    #[ts(type = "unknown | null")]
    pub raw_identity: Option<Value>,
    #[serde(default)]
    pub visible_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayRequestScope {
    pub cwd: String,
    pub source: GatewaySourceInput,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GatewayThreadSelector {
    ThreadId {
        #[serde(rename = "threadId")]
        thread_id: String,
    },
    Source {
        #[serde(rename = "sourceKey")]
        source_key: SourceKey,
    },
}

impl GatewayThreadSelector {
    pub fn thread_id(thread_id: impl Into<String>) -> Self {
        Self::ThreadId {
            thread_id: thread_id.into(),
        }
    }

    pub fn source(source_key: SourceKey) -> Self {
        Self::Source { source_key }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum BackendKind {
    Native,
    Acp,
}

impl BackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Acp => "acp",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayBackendInfo {
    pub kind: BackendKind,
    #[serde(
        default,
        rename = "runtimeRef",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub runtime_ref: Option<String>,
    #[serde(default, rename = "sessionHandle")]
    pub native_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayThread {
    pub id: String,
    pub backend: GatewayBackendInfo,
    #[serde(default)]
    pub source_key: Option<SourceKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub forked_from_thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayTurn {
    pub id: String,
    #[serde(default)]
    pub thread_id: Option<String>,
    pub status: GatewayTurnStatus,
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default)]
    pub error: Option<AgentErrorView>,
    #[serde(
        rename = "startedAtMs",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub started_at_ms: Option<i64>,
    #[serde(
        rename = "completedAtMs",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub completed_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum AgentDeliveryStatusView {
    NotDelivered,
    #[default]
    Unknown,
    Delivered,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentErrorView {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(
        default,
        rename = "retryClass",
        skip_serializing_if = "Option::is_none"
    )]
    pub retry_class: Option<String>,
    #[serde(default)]
    pub delivery: AgentDeliveryStatusView,
    #[serde(
        default,
        rename = "recoveryAction",
        skip_serializing_if = "Option::is_none"
    )]
    pub recovery_action: Option<String>,
    #[serde(
        default,
        rename = "diagnosticRef",
        skip_serializing_if = "Option::is_none"
    )]
    pub diagnostic_ref: Option<String>,
}

/// Compatibility name for code that models the terminal error attached to a
/// `GatewayTurn`. The public wire shape is the shared `AgentErrorView`.
pub type GatewayTurnError = AgentErrorView;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum GatewayTurnStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GatewayInputPart {
    Text {
        text: String,
    },
    Image {
        input: GatewayImageInput,
    },
    Context {
        label: String,
        text: String,
        #[serde(rename = "visibleToModel")]
        visible_to_model: bool,
    },
    Resource {
        uri: String,
        #[serde(default, rename = "mimeType")]
        mime_type: Option<String>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        blob: Option<String>,
    },
    ResourceLink {
        name: String,
        uri: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default, rename = "mimeType")]
        mime_type: Option<String>,
        #[serde(default)]
        size: Option<i64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayMentionRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GatewayMentionTarget {
    Skill {
        name: String,
        #[serde(default)]
        path: Option<String>,
    },
    Agent {
        name: String,
        #[serde(default)]
        source: Option<String>,
        #[serde(default)]
        entrypoints: Vec<String>,
        #[serde(default)]
        backend_ref: Option<String>,
    },
    File {
        path: String,
        relative_path: String,
    },
    Capability {
        id: String,
        label: String,
        target_kind: String,
        #[serde(default)]
        uri: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayMention {
    pub visible_text: String,
    pub range: GatewayMentionRange,
    pub target: GatewayMentionTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GatewayImageInput {
    LocalPath { path: String },
    Url { url: String },
}
