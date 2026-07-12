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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    pub(crate) fn public_json(&self) -> Value {
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
    pub request: Option<f64>,
    pub context_over_200k: Option<ModelCostTier>,
    pub source: Option<String>,
    pub version: Option<String>,
}

impl ModelCost {
    pub(crate) fn public_json(&self) -> Value {
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
        if let Some(value) = self.request {
            object.insert("request".to_string(), Value::from(value));
        }
        if let Some(tier) = &self.context_over_200k {
            object.insert("context_over_200k".to_string(), tier.public_json());
        }
        if let Some(source) = &self.source {
            object.insert("source".to_string(), Value::String(source.clone()));
        }
        if let Some(version) = &self.version {
            object.insert("version".to_string(), Value::String(version.clone()));
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
    pub(crate) fn public_json(&self) -> Value {
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
    pub developer_role: Option<bool>,
    pub temperature: Option<bool>,
    pub attachment: Option<bool>,
    pub structured_output: Option<bool>,
    pub interleaved: Option<Value>,
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
}

impl ModelCapabilities {
    pub(crate) fn public_json(&self) -> Value {
        let mut object = serde_json::Map::new();
        if let Some(value) = self.reasoning {
            object.insert("reasoning".to_string(), Value::Bool(value));
        }
        if let Some(value) = self.tool_call {
            object.insert("tool_call".to_string(), Value::Bool(value));
        }
        if let Some(value) = self.developer_role {
            object.insert("developer_role".to_string(), Value::Bool(value));
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostStatus {
    Estimated,
    Free,
    Included,
    Unknown,
}

impl CostStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Estimated => "estimated",
            Self::Free => "free",
            Self::Included => "included",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "estimated" => Some(Self::Estimated),
            "free" => Some(Self::Free),
            "included" => Some(Self::Included),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
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
    pub cost_status: Option<CostStatus>,
    pub pricing_missing_reason: Option<String>,
    pub pricing_version: Option<String>,
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
        if let Some(value) = self.cost_status {
            object.insert(
                "cost_status".to_string(),
                Value::String(value.as_str().to_string()),
            );
        }
        if let Some(value) = &self.pricing_missing_reason {
            object.insert(
                "pricing_missing_reason".to_string(),
                Value::String(value.clone()),
            );
        }
        if let Some(value) = &self.pricing_version {
            object.insert("pricing_version".to_string(), Value::String(value.clone()));
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
pub struct SessionExportMessageSummary {
    pub session_seq: i64,
    pub message: Message,
    pub usage: Option<Value>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TuiMessageSummary {
    pub session_seq: i64,
    pub message: Message,
    pub usage: Option<Value>,
    pub metadata: Option<Value>,
    pub accounting: Option<Value>,
}

static SESSION_EVENT_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq)]
pub struct SessionEvent {
    pub event_id: String,
    pub session_id: Option<String>,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub sequence: Option<i64>,
    pub payload: SessionEventPayload,
    value: Value,
}

impl SessionEvent {
    pub fn from_legacy_value(value: Value) -> Self {
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("runtime_event")
            .to_string();
        let generated_sequence = next_session_event_sequence();
        let session_id = value
            .get("session_id")
            .or_else(|| value.get("sessionId"))
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let thread_id = value
            .get("thread_id")
            .or_else(|| value.get("threadId"))
            .or_else(|| value.get("session_id"))
            .or_else(|| value.get("sessionId"))
            .or_else(|| value.get("parent_thread_id"))
            .or_else(|| value.get("child_thread_id"))
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let turn_id = value
            .get("turn_id")
            .or_else(|| value.get("turnId"))
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let sequence = value
            .get("sequence")
            .or_else(|| value.get("seq"))
            .and_then(Value::as_i64)
            .or_else(|| i64::try_from(generated_sequence).ok());
        let event_id = value
            .get("event_id")
            .or_else(|| value.get("eventId"))
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_else(|| session_event_id(&kind, generated_sequence));
        let payload = SessionEventPayload::from_legacy_value(&kind, &value);
        Self {
            event_id,
            session_id,
            thread_id,
            turn_id,
            sequence,
            payload,
            value,
        }
    }

    pub fn new(payload: SessionEventPayload) -> Self {
        let value = payload.to_legacy_value();
        let mut event = Self::from_legacy_value(value);
        event.payload = payload;
        event
    }

    pub fn kind(&self) -> &'static str {
        self.payload.kind()
    }

    pub fn as_value(&self) -> &Value {
        &self.value
    }

    pub fn into_value(self) -> Value {
        self.value
    }
}

impl std::ops::Deref for SessionEvent {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        self.as_value()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionEventPayload {
    SessionConfigured { data: Value },
    TurnStarted { data: Value },
    TurnCompleted { data: Value },
    MessageStarted { message: Value },
    MessageUpdated { message: Value },
    MessageCompleted {
        message: Value,
        usage: Option<Value>,
        metadata: Option<Value>,
        accounting: Option<Value>,
    },
    ReasoningDelta { text: String },
    ReasoningCompleted { text: Option<String> },
    ToolCallPending { data: Value },
    ToolExecutionStarted { data: Value },
    ToolExecutionUpdated { data: Value },
    ToolExecutionCompleted { data: Value },
    AgentSessionStarted { data: Value },
    ContextSnapshot { data: Value },
    Warning { data: Value },
    BlockingActionRequested {
        action_id: String,
        kind: BlockingActionKind,
        payload: Value,
    },
    BlockingActionUpdated {
        action_id: String,
        kind: BlockingActionKind,
        payload: Value,
    },
    BlockingActionResolved {
        action_id: String,
        kind: BlockingActionKind,
        reason: String,
    },
    BlockingActionCancelled {
        action_id: String,
        kind: BlockingActionKind,
        reason: String,
    },
    DeliveryDiagnostic {
        status: DeliveryDiagnosticStatus,
        data: Value,
    },
    Diagnostic { kind: String, data: Value },
}

impl SessionEventPayload {
    pub fn from_legacy_value(kind: &str, value: &Value) -> Self {
        match kind {
            "run_start" => Self::SessionConfigured {
                data: value.clone(),
            },
            "agent_start" | "task_started" | "turn_started" => Self::TurnStarted {
                data: value.clone(),
            },
            "task_complete" | "turn_complete" | "agent_end" | "run_end" => {
                Self::TurnCompleted {
                    data: value.clone(),
                }
            }
            "message_start" => Self::MessageStarted {
                message: value.get("message").cloned().unwrap_or(Value::Null),
            },
            "message_update" => Self::MessageUpdated {
                message: value.get("message").cloned().unwrap_or(Value::Null),
            },
            "message_end" => Self::MessageCompleted {
                message: value.get("message").cloned().unwrap_or(Value::Null),
                usage: value.get("usage").cloned(),
                metadata: value.get("metadata").cloned(),
                accounting: value.get("accounting").cloned(),
            },
            "reasoning_delta" => Self::ReasoningDelta {
                text: value
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            },
            "reasoning_end" => Self::ReasoningCompleted {
                text: value
                    .get("text")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            },
            "tool_call_pending" => Self::ToolCallPending {
                data: value.clone(),
            },
            "tool_execution_start" => Self::ToolExecutionStarted {
                data: value.clone(),
            },
            "tool_execution_update" => Self::ToolExecutionUpdated {
                data: value.clone(),
            },
            "tool_execution_end" => Self::ToolExecutionCompleted {
                data: value.clone(),
            },
            "agent_session_start" => Self::AgentSessionStarted {
                data: value.clone(),
            },
            "context_snapshot" => Self::ContextSnapshot {
                data: value.clone(),
            },
            "warning" => Self::Warning {
                data: value.clone(),
            },
            "action_requested" => Self::BlockingActionRequested {
                action_id: action_id_from_value(value),
                kind: blocking_action_kind_from_value(value),
                payload: value.get("payload").cloned().unwrap_or(Value::Null),
            },
            "action_updated" => Self::BlockingActionUpdated {
                action_id: action_id_from_value(value),
                kind: blocking_action_kind_from_value(value),
                payload: value.get("payload").cloned().unwrap_or(Value::Null),
            },
            "action_resolved" => Self::BlockingActionResolved {
                action_id: action_id_from_value(value),
                kind: blocking_action_kind_from_value(value),
                reason: value
                    .get("reason")
                    .and_then(Value::as_str)
                    .or_else(|| value.get("outcome").and_then(Value::as_str))
                    .unwrap_or_default()
                    .to_string(),
            },
            "action_cancelled" => Self::BlockingActionCancelled {
                action_id: action_id_from_value(value),
                kind: blocking_action_kind_from_value(value),
                reason: value
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            },
            "delivery_started" => Self::DeliveryDiagnostic {
                status: DeliveryDiagnosticStatus::Started,
                data: value.clone(),
            },
            "delivery_updated" => Self::DeliveryDiagnostic {
                status: DeliveryDiagnosticStatus::Updated,
                data: value.clone(),
            },
            "delivery_completed" => Self::DeliveryDiagnostic {
                status: DeliveryDiagnosticStatus::Completed,
                data: value.clone(),
            },
            "delivery_failed" => Self::DeliveryDiagnostic {
                status: DeliveryDiagnosticStatus::Failed,
                data: value.clone(),
            },
            _ => Self::Diagnostic {
                kind: kind.to_string(),
                data: value.clone(),
            },
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::SessionConfigured { .. } => "run_start",
            Self::TurnStarted { .. } => "turn_started",
            Self::TurnCompleted { .. } => "turn_complete",
            Self::MessageStarted { .. } => "message_start",
            Self::MessageUpdated { .. } => "message_update",
            Self::MessageCompleted { .. } => "message_end",
            Self::ReasoningDelta { .. } => "reasoning_delta",
            Self::ReasoningCompleted { .. } => "reasoning_end",
            Self::ToolCallPending { .. } => "tool_call_pending",
            Self::ToolExecutionStarted { .. } => "tool_execution_start",
            Self::ToolExecutionUpdated { .. } => "tool_execution_update",
            Self::ToolExecutionCompleted { .. } => "tool_execution_end",
            Self::AgentSessionStarted { .. } => "agent_session_start",
            Self::ContextSnapshot { .. } => "context_snapshot",
            Self::Warning { .. } => "warning",
            Self::BlockingActionRequested { .. } => "action_requested",
            Self::BlockingActionUpdated { .. } => "action_updated",
            Self::BlockingActionResolved { .. } => "action_resolved",
            Self::BlockingActionCancelled { .. } => "action_cancelled",
            Self::DeliveryDiagnostic { status, .. } => status.as_event_type(),
            Self::Diagnostic { .. } => "runtime_event",
        }
    }

    pub fn to_legacy_value(&self) -> Value {
        match self {
            Self::SessionConfigured { data }
            | Self::TurnStarted { data }
            | Self::TurnCompleted { data }
            | Self::ToolCallPending { data }
            | Self::ToolExecutionStarted { data }
            | Self::ToolExecutionUpdated { data }
            | Self::ToolExecutionCompleted { data }
            | Self::AgentSessionStarted { data }
            | Self::ContextSnapshot { data }
            | Self::Warning { data }
            | Self::DeliveryDiagnostic { data, .. }
            | Self::Diagnostic { data, .. } => data.clone(),
            Self::MessageStarted { message } => json!({
                "type": "message_start",
                "message": message,
            }),
            Self::MessageUpdated { message } => json!({
                "type": "message_update",
                "message": message,
            }),
            Self::MessageCompleted {
                message,
                usage,
                metadata,
                accounting,
            } => {
                let mut value = json!({
                    "type": "message_end",
                    "message": message,
                });
                if let Some(object) = value.as_object_mut() {
                    if let Some(usage) = usage {
                        object.insert("usage".to_string(), usage.clone());
                    }
                    if let Some(metadata) = metadata {
                        object.insert("metadata".to_string(), metadata.clone());
                    }
                    if let Some(accounting) = accounting {
                        object.insert("accounting".to_string(), accounting.clone());
                    }
                }
                value
            }
            Self::ReasoningDelta { text } => json!({
                "type": "reasoning_delta",
                "text": text,
            }),
            Self::ReasoningCompleted { text } => {
                let mut value = json!({ "type": "reasoning_end" });
                if let Some(text) = text
                    && let Some(object) = value.as_object_mut()
                {
                    object.insert("text".to_string(), Value::String(text.clone()));
                }
                value
            }
            Self::BlockingActionRequested {
                action_id,
                kind,
                payload,
            } => json!({
                "type": "action_requested",
                "action_id": action_id,
                "kind": kind,
                "payload": payload,
            }),
            Self::BlockingActionUpdated {
                action_id,
                kind,
                payload,
            } => json!({
                "type": "action_updated",
                "action_id": action_id,
                "kind": kind,
                "payload": payload,
            }),
            Self::BlockingActionResolved {
                action_id,
                kind,
                reason,
            } => json!({
                "type": "action_resolved",
                "action_id": action_id,
                "kind": kind,
                "reason": reason,
            }),
            Self::BlockingActionCancelled {
                action_id,
                kind,
                reason,
            } => json!({
                "type": "action_cancelled",
                "action_id": action_id,
                "kind": kind,
                "reason": reason,
            }),
        }
    }
}

fn action_id_from_value(value: &Value) -> String {
    value
        .get("action_id")
        .or_else(|| value.get("actionId"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn blocking_action_kind_from_value(value: &Value) -> BlockingActionKind {
    match value
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "permission" => BlockingActionKind::Permission,
        "custom_tool" | "customTool" => BlockingActionKind::CustomTool,
        "user_input" | "userInput" => BlockingActionKind::UserInput,
        _ => BlockingActionKind::Clarify,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockingActionKind {
    Permission,
    Clarify,
    CustomTool,
    UserInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryDiagnosticStatus {
    Started,
    Updated,
    Completed,
    Failed,
}

impl DeliveryDiagnosticStatus {
    fn as_event_type(self) -> &'static str {
        match self {
            Self::Started => "delivery_started",
            Self::Updated => "delivery_updated",
            Self::Completed => "delivery_completed",
            Self::Failed => "delivery_failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RunStreamEvent {
    Event(Box<SessionEvent>),
    ReasoningDelta {
        text: String,
    },
    ReasoningEnd,
    ClarifyRequest(ClarifyRequestEvent),
    ClarifyResolved(ClarifyResolvedEvent),
    Scoped {
        session_id: String,
        turn_id: Option<String>,
        event: Box<RunStreamEvent>,
    },
}

impl RunStreamEvent {
    pub fn value(value: Value) -> Self {
        Self::Event(Box::new(SessionEvent::from_legacy_value(value)))
    }

    pub fn session(event: SessionEvent) -> Self {
        Self::Event(Box::new(event))
    }

    pub fn scoped(session_id: impl Into<String>, event: RunStreamEvent) -> Self {
        Self::Scoped {
            session_id: session_id.into(),
            turn_id: None,
            event: Box::new(event),
        }
    }

    pub fn scoped_turn(
        session_id: impl Into<String>,
        turn_id: impl Into<String>,
        event: RunStreamEvent,
    ) -> Self {
        Self::Scoped {
            session_id: session_id.into(),
            turn_id: Some(turn_id.into()),
            event: Box::new(event),
        }
    }

    pub fn legacy_value(&self) -> Option<&Value> {
        match self {
            Self::Event(event) => Some(event.as_value()),
            Self::Scoped { event, .. } => event.legacy_value(),
            _ => None,
        }
    }
}

fn next_session_event_sequence() -> u64 {
    SESSION_EVENT_COUNTER.fetch_add(1, Ordering::Relaxed)
}

fn session_event_id(kind: &str, sequence: u64) -> String {
    format!("sevt_{kind}_{sequence}")
}

pub type RunStreamSink = Arc<dyn Fn(RunStreamEvent) + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarifyQuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarifyQuestion {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub header: String,
    pub question: String,
    pub options: Vec<ClarifyQuestionOption>,
    #[serde(default, skip_serializing_if = "clarify_question_false")]
    pub multiple: bool,
    #[serde(
        default = "clarify_question_custom_default",
        skip_serializing_if = "clarify_question_custom_default_value"
    )]
    pub custom: bool,
    #[serde(default, skip_serializing_if = "clarify_question_false")]
    pub secret: bool,
}

fn clarify_question_custom_default() -> bool {
    true
}

fn clarify_question_custom_default_value(value: &bool) -> bool {
    *value
}

fn clarify_question_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarifyRequestEvent {
    pub call_id: String,
    pub questions: Vec<ClarifyQuestion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarifyAnswer {
    pub answers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarifyResponse {
    pub answers: Vec<ClarifyAnswer>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClarifyResult {
    Answered(ClarifyResponse),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClarifyInteractionOutcome {
    Answered(ClarifyResponse),
    Cancelled,
    TimedOut,
    TurnFinished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClarifyResolvedReason {
    Answered,
    Cancelled,
    TimedOut,
    TurnFinished,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarifyResolvedEvent {
    pub call_id: String,
    pub reason: ClarifyResolvedReason,
}

#[derive(Debug, Default)]
pub(crate) struct ClarifyControl {
    pub(crate) pending: Mutex<HashMap<String, oneshot::Sender<ClarifyResult>>>,
}

impl ClarifyControl {
    pub(crate) fn register(&self, call_id: String) -> oneshot::Receiver<ClarifyResult> {
        let (tx, rx) = oneshot::channel();
        let mut pending = self.pending.lock().expect("clarify pending map poisoned");
        pending.insert(call_id, tx);
        rx
    }

    pub(crate) fn submit(&self, call_id: &str, result: ClarifyResult) -> bool {
        let sender = self
            .pending
            .lock()
            .expect("clarify pending map poisoned")
            .remove(call_id);
        sender.is_some_and(|sender| sender.send(result).is_ok())
    }

    pub(crate) fn remove(&self, call_id: &str) -> bool {
        self.pending
            .lock()
            .expect("clarify pending map poisoned")
            .remove(call_id)
            .is_some()
    }
}

#[derive(Clone)]
pub struct RunControlHandle {
    pub(crate) inner: ControlHandle,
    pub(crate) clarify: Arc<ClarifyControl>,
}

impl RunControlHandle {
    pub fn stop(&self) {
        self.inner.stop();
    }

    pub fn abort(&self) {
        self.inner.abort();
    }

    pub fn inject_user_message(&self, message: Message) -> bool {
        self.inner.inject_user_message(message)
    }

    pub fn steer_user_message(&self, message: Message) -> Option<PendingInputId> {
        self.inner.steer_user_message(message)
    }

    pub fn update_pending_user_message(&self, id: PendingInputId, message: Message) -> bool {
        self.inner.update_pending_user_message(id, message)
    }

    pub fn cancel_pending_user_message(&self, id: PendingInputId) -> bool {
        self.inner.cancel_pending_user_message(id)
    }

    pub fn submit_clarify_result(&self, call_id: &str, result: ClarifyResult) -> bool {
        self.clarify.submit(call_id, result)
    }

    /// Runs a product clarification through the same pending-interaction broker
    /// used by Native tools. Adapters use this instead of owning a second
    /// response registry or emitting transport-specific interaction events.
    pub async fn request_clarification(
        &self,
        request: ClarifyRequestEvent,
        stream: RunStreamSink,
        abort: Option<AbortSignal>,
    ) -> ClarifyInteractionOutcome {
        let call_id = request.call_id.clone();
        let receiver = self.clarify.register(call_id.clone());
        stream(RunStreamEvent::session(SessionEvent::new(
            SessionEventPayload::BlockingActionRequested {
                action_id: call_id.clone(),
                kind: BlockingActionKind::Clarify,
                payload: serde_json::to_value(request).unwrap_or(Value::Null),
            },
        )));
        let timeout = tokio::time::sleep(std::time::Duration::from_secs(600));
        tokio::pin!(timeout);
        let abort = wait_for_optional_clarify_abort(abort);
        tokio::pin!(abort);

        let (outcome, reason) = tokio::select! {
            result = receiver => match result {
                Ok(ClarifyResult::Answered(response)) => (
                    ClarifyInteractionOutcome::Answered(response),
                    ClarifyResolvedReason::Answered,
                ),
                Ok(ClarifyResult::Cancelled) => (
                    ClarifyInteractionOutcome::Cancelled,
                    ClarifyResolvedReason::Cancelled,
                ),
                Err(_) => (
                    ClarifyInteractionOutcome::TurnFinished,
                    ClarifyResolvedReason::TurnFinished,
                ),
            },
            _ = &mut timeout => {
                self.clarify.remove(&call_id);
                (
                    ClarifyInteractionOutcome::TimedOut,
                    ClarifyResolvedReason::TimedOut,
                )
            }
            _ = &mut abort => {
                self.clarify.remove(&call_id);
                (
                    ClarifyInteractionOutcome::TurnFinished,
                    ClarifyResolvedReason::TurnFinished,
                )
            }
        };
        stream(RunStreamEvent::session(SessionEvent::new(
            SessionEventPayload::BlockingActionResolved {
                action_id: call_id,
                kind: BlockingActionKind::Clarify,
                reason: match reason {
                    ClarifyResolvedReason::Answered => "answered",
                    ClarifyResolvedReason::Cancelled => "cancelled",
                    ClarifyResolvedReason::TimedOut => "timed_out",
                    ClarifyResolvedReason::TurnFinished => "turn_finished",
                }
                .to_string(),
            },
        )));
        outcome
    }
}

async fn wait_for_optional_clarify_abort(abort: Option<AbortSignal>) {
    if let Some(mut abort) = abort {
        abort.wait_for_abort().await;
    } else {
        std::future::pending::<()>().await;
    }
}

impl fmt::Debug for RunControlHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RunControlHandle")
            .finish_non_exhaustive()
    }
}

pub struct RunControl {
    pub(crate) handle: RunControlHandle,
    pub(crate) receivers: ControlReceivers,
}

impl RunControl {
    pub fn handle(&self) -> RunControlHandle {
        self.handle.clone()
    }

    pub fn abort_signal(&self) -> AbortSignal {
        self.receivers.abort_signal()
    }

    /// Consumes steer inputs that have not yet been accepted by a backend.
    pub fn drain_pending_user_messages(&mut self) -> Vec<(PendingInputId, Message)> {
        self.receivers.drain_pending_user_messages()
    }
}

pub fn run_control() -> (RunControlHandle, RunControl) {
    let (inner, receivers) = ControlHandle::new();
    let clarify = Arc::new(ClarifyControl::default());
    let handle = RunControlHandle { inner, clarify };
    (handle.clone(), RunControl { handle, receivers })
}
