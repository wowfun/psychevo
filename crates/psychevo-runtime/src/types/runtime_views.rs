
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

#[derive(Debug, Clone, PartialEq)]
pub enum RunStreamEvent {
    Event(Value),
    ReasoningDelta {
        text: String,
    },
    ReasoningEnd,
    ClarifyRequest(ClarifyRequestEvent),
    ClarifyResolved(ClarifyResolvedEvent),
    Scoped {
        session_id: String,
        event: Box<RunStreamEvent>,
    },
}

impl RunStreamEvent {
    pub fn scoped(session_id: impl Into<String>, event: RunStreamEvent) -> Self {
        Self::Scoped {
            session_id: session_id.into(),
            event: Box::new(event),
        }
    }
}

pub type RunStreamSink = Arc<dyn Fn(RunStreamEvent) + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarifyQuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarifyQuestion {
    pub question: String,
    pub options: Vec<ClarifyQuestionOption>,
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
    pub fn abort_signal(&self) -> AbortSignal {
        self.receivers.abort_signal()
    }
}

pub fn run_control() -> (RunControlHandle, RunControl) {
    let (inner, receivers) = ControlHandle::new();
    let clarify = Arc::new(ClarifyControl::default());
    let handle = RunControlHandle { inner, clarify };
    (handle.clone(), RunControl { handle, receivers })
}
