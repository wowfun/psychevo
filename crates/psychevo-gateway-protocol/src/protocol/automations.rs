#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AutomationScheduleInput {
    Interval {
        #[serde(rename = "everyMinutes")]
        every_minutes: u32,
    },
    Delay {
        #[serde(rename = "afterMinutes")]
        after_minutes: u32,
    },
    Once {
        at: String,
    },
    Daily {
        time: String,
    },
    Weekly {
        weekdays: Vec<u8>,
        time: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum AutomationTaskKind {
    Project,
    ThreadHeartbeat,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AutomationTargetInput {
    Project,
    ThreadHeartbeat {
        #[serde(rename = "threadId")]
        thread_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum AutomationExecutionPolicy {
    AutoSandbox,
    AskFirst,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationExecutionInput {
    pub policy: AutomationExecutionPolicy,
}

impl Default for AutomationExecutionInput {
    fn default() -> Self {
        Self {
            policy: AutomationExecutionPolicy::AutoSandbox,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationListParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationWriteParams {
    #[serde(default)]
    pub automation_id: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
    pub target: AutomationTargetInput,
    pub title: String,
    pub prompt: String,
    pub schedule: AutomationScheduleInput,
    #[serde(default)]
    pub execution: Option<AutomationExecutionInput>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationDraftParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
    pub request: String,
    #[serde(default, rename = "currentThreadId")]
    pub current_thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationDraftView {
    pub target: AutomationTargetInput,
    pub title: String,
    pub prompt: String,
    pub schedule: AutomationScheduleInput,
    #[serde(default)]
    pub execution: AutomationExecutionInput,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationIdParams {
    pub automation_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRunParams {
    pub automation_id: String,
    #[serde(default)]
    pub trigger: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationTaskView {
    pub id: String,
    pub workdir: String,
    pub kind: AutomationTaskKind,
    #[serde(default)]
    pub target_thread_id: Option<String>,
    pub title: String,
    pub prompt: String,
    pub schedule: AutomationScheduleInput,
    pub enabled: bool,
    pub execution: AutomationExecutionInput,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub source_key: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub last_run_at_ms: Option<i64>,
    #[serde(default)]
    pub next_run_at_ms: Option<i64>,
    #[serde(default)]
    pub last_status: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub runs: Vec<AutomationRunView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRunView {
    pub id: String,
    pub automation_id: String,
    pub trigger: String,
    pub status: String,
    pub started_at_ms: i64,
    #[serde(default)]
    pub completed_at_ms: Option<i64>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub source_key: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    #[ts(type = "Record<string, unknown> | null")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationListResult {
    pub automations: Vec<AutomationTaskView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationMutationResult {
    pub automation: AutomationTaskView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationDraftResult {
    pub draft: AutomationDraftView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationDeleteResult {
    pub deleted: bool,
    pub automation_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRunResult {
    pub accepted: bool,
    pub automation: AutomationTaskView,
    #[serde(default)]
    pub run: Option<AutomationRunView>,
}
