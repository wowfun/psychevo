use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::safe_integer::{
    JsonSafeI64, JsonSafeU64, json_safe_i64, json_safe_u64, json_safe_usize, option_json_safe_i64,
    option_json_safe_u64, option_json_safe_usize,
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppProductInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
pub struct AppClientCapabilities {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppInitializeParams {
    pub client: AppProductInfo,
    pub protocol_min: u32,
    pub protocol_max: u32,
    #[serde(default)]
    pub capabilities: AppClientCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppServerCapabilities {
    pub threads: bool,
    pub turns: bool,
    pub event_replay: String,
    pub interactions: bool,
    pub custom_tools: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppInitializeResult {
    pub server: AppProductInfo,
    pub protocol_version: u32,
    pub protocol_min: u32,
    pub protocol_max: u32,
    pub capabilities: AppServerCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppThreadStartParams {
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "unknown")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppThreadIdParams {
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppThreadForkParams {
    pub thread_id: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "option_json_safe_i64::serialize",
        deserialize_with = "option_json_safe_i64::deserialize"
    )]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub before_session_seq: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppThreadCompactParams {
    pub thread_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppThreadCompactResult {
    pub thread_id: String,
    pub compacted: bool,
    pub reason: String,
    pub message: String,
    #[serde(
        serialize_with = "option_json_safe_i64::serialize",
        deserialize_with = "option_json_safe_i64::deserialize"
    )]
    #[schemars(required)]
    #[schemars(with = "AppNullable<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub checkpoint_id: Option<i64>,
    #[serde(
        serialize_with = "option_json_safe_i64::serialize",
        deserialize_with = "option_json_safe_i64::deserialize"
    )]
    #[schemars(required)]
    #[schemars(with = "AppNullable<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub first_kept_session_seq: Option<i64>,
    #[serde(
        serialize_with = "option_json_safe_u64::serialize",
        deserialize_with = "option_json_safe_u64::deserialize"
    )]
    #[schemars(required)]
    #[schemars(with = "AppNullable<JsonSafeU64>")]
    #[ts(type = "number | null")]
    pub tokens_before: Option<u64>,
    #[serde(
        serialize_with = "option_json_safe_u64::serialize",
        deserialize_with = "option_json_safe_u64::deserialize"
    )]
    #[schemars(required)]
    #[schemars(with = "AppNullable<JsonSafeU64>")]
    #[ts(type = "number | null")]
    pub tokens_after: Option<u64>,
    #[schemars(required)]
    #[schemars(with = "AppNullable<String>")]
    #[serde(deserialize_with = "required_nullable::deserialize")]
    pub summary: Option<String>,
    #[schemars(required)]
    #[schemars(with = "AppNullable<String>")]
    #[serde(deserialize_with = "required_nullable::deserialize")]
    pub summary_provider: Option<String>,
    #[schemars(required)]
    #[schemars(with = "AppNullable<String>")]
    #[serde(deserialize_with = "required_nullable::deserialize")]
    pub summary_model: Option<String>,
}

struct AppNullable<T>(std::marker::PhantomData<T>);

impl<T: JsonSchema> JsonSchema for AppNullable<T> {
    fn schema_name() -> String {
        format!("AppNullable{}", T::schema_name())
    }

    fn json_schema(generator: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
        <Option<T>>::json_schema(generator)
    }
}

mod required_nullable {
    use serde::{Deserialize, Deserializer};

    pub(super) fn deserialize<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        Option::<T>::deserialize(deserializer)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppThreadListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(
        serialize_with = "option_json_safe_usize::serialize",
        deserialize_with = "option_json_safe_usize::deserialize"
    )]
    #[schemars(with = "Option<JsonSafeU64>")]
    #[ts(type = "number | null")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppTurnStartParams {
    pub thread_id: String,
    pub turn_id: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub no_agents: bool,
    #[serde(default)]
    pub no_skills: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherited_env: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub use_registered_approval_handler: bool,
    #[serde(default)]
    pub use_registered_clarify_handler: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppTurnIdParams {
    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppTurnSteerParams {
    pub turn_id: String,
    pub input: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppInteractionRespondParams {
    pub turn_id: String,
    pub interaction_id: String,
    pub response: AppInteractionResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[ts(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AppInteractionResponse {
    Permission {
        outcome: AppApprovalOutcome,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(rename = "filesystemDirectory")]
        #[ts(optional)]
        filesystem_directory: Option<String>,
    },
    Clarify {
        answers: Vec<Vec<String>>,
    },
    Cancel,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename_all = "snake_case")]
pub enum AppToolExecutionMode {
    Parallel,
    Sequential,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppToolDefinition {
    pub name: String,
    pub description: String,
    #[ts(type = "unknown")]
    pub parameters: Value,
    pub execution_mode: AppToolExecutionMode,
    #[serde(
        default = "default_app_callback_timeout_ms",
        serialize_with = "json_safe_u64::serialize",
        deserialize_with = "json_safe_u64::deserialize"
    )]
    #[schemars(with = "JsonSafeU64")]
    #[ts(type = "number")]
    pub timeout_ms: u64,
}

const fn default_app_callback_timeout_ms() -> u64 {
    300_000
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppToolRegisterParams {
    #[serde(default)]
    pub tools: Vec<AppToolDefinition>,
    #[serde(default)]
    pub approval_handler: bool,
    #[serde(default)]
    pub clarify_handler: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppToolCallParams {
    pub call_id: String,
    pub tool_name: String,
    #[ts(type = "unknown")]
    pub arguments: Value,
    pub thread_id: String,
    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppToolCallResult {
    #[ts(type = "unknown")]
    pub result: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_content: Option<String>,
    #[serde(default)]
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppApprovalRequestParams {
    pub call_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub summary: String,
    pub reason: String,
    pub matched_rule: Option<String>,
    pub suggested_rule: Option<String>,
    pub allow_always: bool,
    pub filesystem: Option<AppFilesystemApprovalRequest>,
    #[serde(default, rename = "mcpStartup")]
    pub mcp_startup: Option<AppMcpStartupApprovalRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppFilesystemApprovalTarget {
    pub requested_path: String,
    pub resolved_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppFilesystemApprovalRequest {
    pub targets: Vec<AppFilesystemApprovalTarget>,
    pub scope_candidates: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppMcpStartupApprovalRequest {
    pub server: String,
    pub source: String,
    pub target: AppMcpStartupApprovalTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[ts(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AppMcpStartupApprovalTarget {
    Stdio {
        command: String,
        args: Vec<String>,
        cwd: String,
        #[schemars(rename = "envNames")]
        env_names: Vec<String>,
    },
    Http {
        url: String,
        #[schemars(rename = "headerNames")]
        header_names: Vec<String>,
        #[schemars(rename = "credentialNames")]
        credential_names: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename_all = "snake_case")]
pub enum AppApprovalOutcome {
    AllowOnce,
    AllowTurn,
    AllowSession,
    AllowAlways,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppApprovalResult {
    pub outcome: AppApprovalOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filesystem_directory: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppThreadSnapshot {
    pub id: String,
    pub source: String,
    pub cwd: String,
    pub title: Option<String>,
    #[serde(with = "json_safe_i64")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub started_at_ms: i64,
    #[serde(with = "json_safe_i64")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub updated_at_ms: i64,
    pub archived: bool,
    #[serde(with = "json_safe_i64")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub message_count: i64,
    #[serde(with = "json_safe_i64")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub tool_call_count: i64,
    pub active_turn_id: Option<String>,
    #[serde(default)]
    pub pending_interactions: Vec<AppPendingInteraction>,
    #[serde(default)]
    pub items: Vec<AppThreadItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppThreadItem {
    #[serde(with = "json_safe_i64")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub session_seq: i64,
    #[ts(type = "unknown")]
    pub message: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "unknown")]
    pub usage: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "unknown")]
    pub metadata: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "unknown")]
    pub accounting: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppThreadSummary {
    pub id: String,
    pub source: String,
    pub cwd: String,
    pub title: Option<String>,
    #[serde(with = "json_safe_i64")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub started_at_ms: i64,
    #[serde(with = "json_safe_i64")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub updated_at_ms: i64,
    pub archived: bool,
    #[serde(with = "json_safe_i64")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub message_count: i64,
    #[serde(with = "json_safe_i64")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub tool_call_count: i64,
    pub active_turn_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppPendingInteraction {
    pub interaction_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub kind: String,
    pub status: String,
    #[ts(type = "unknown")]
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "unknown")]
    pub resolution: Option<Value>,
    #[serde(with = "json_safe_i64")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub requested_at_ms: i64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "option_json_safe_i64::serialize",
        deserialize_with = "option_json_safe_i64::deserialize"
    )]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub resolved_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppThreadListResult {
    pub threads: Vec<AppThreadSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppTurnReceipt {
    pub accepted: bool,
    pub thread_id: String,
    pub turn_id: String,
    pub client_turn_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename_all = "snake_case")]
pub enum AppTurnOutcome {
    Completed,
    Stopped,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppTurnResult {
    pub thread_id: String,
    pub outcome: AppTurnOutcome,
    pub final_answer: String,
    pub provider: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
    #[serde(
        serialize_with = "json_safe_usize::serialize",
        deserialize_with = "json_safe_usize::deserialize"
    )]
    #[schemars(with = "JsonSafeU64")]
    #[ts(type = "number")]
    pub tool_failures: usize,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "option_json_safe_u64::serialize",
        deserialize_with = "option_json_safe_u64::deserialize"
    )]
    #[schemars(with = "Option<JsonSafeU64>")]
    #[ts(type = "number | null")]
    pub context_limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "unknown")]
    pub context_snapshot: Option<Value>,
    #[serde(default)]
    #[ts(type = "Array<unknown>")]
    pub warnings: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "unknown")]
    pub terminal_reason: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "unknown")]
    pub terminal_error: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "unknown")]
    pub selected_agent: Option<Value>,
    #[serde(default)]
    #[ts(type = "Array<unknown>")]
    pub selected_skills: Vec<Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename_all = "snake_case")]
pub enum AppItemStage {
    Started,
    Updated,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[ts(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AppTurnEvent {
    Accepted {
        receipt: AppTurnReceipt,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            rename = "queuePosition",
            serialize_with = "option_json_safe_usize::serialize",
            deserialize_with = "option_json_safe_usize::deserialize"
        )]
        #[schemars(with = "Option<JsonSafeU64>")]
        #[ts(type = "number | null")]
        #[ts(optional)]
        queue_position: Option<usize>,
    },
    Started {
        #[serde(rename = "threadId")]
        thread_id: String,
        #[serde(rename = "turnId")]
        turn_id: String,
    },
    Message {
        stage: AppItemStage,
        #[ts(type = "unknown")]
        message: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(type = "unknown | null")]
        #[ts(optional)]
        usage: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(type = "unknown | null")]
        #[ts(optional)]
        metadata: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(type = "unknown | null")]
        #[ts(optional)]
        accounting: Option<Value>,
    },
    MessageDelta {
        text: String,
    },
    ReasoningDelta {
        text: String,
    },
    ReasoningCompleted {
        #[serde(deserialize_with = "required_nullable::deserialize")]
        #[schemars(required)]
        #[schemars(with = "AppNullable<String>")]
        text: Option<String>,
    },
    Tool {
        stage: AppItemStage,
        #[ts(type = "unknown")]
        data: Value,
    },
    InteractionRequested {
        #[serde(rename = "interactionId")]
        interaction_id: String,
        kind: String,
        #[ts(type = "unknown")]
        payload: Value,
    },
    InteractionResolved {
        #[serde(rename = "interactionId")]
        interaction_id: String,
        reason: String,
    },
    Warning {
        #[ts(type = "unknown")]
        data: Value,
    },
    Completed {
        #[serde(rename = "threadId")]
        thread_id: String,
        #[serde(rename = "turnId")]
        turn_id: String,
        outcome: AppTurnOutcome,
    },
    Failed {
        #[serde(rename = "threadId")]
        thread_id: String,
        #[serde(rename = "turnId")]
        turn_id: String,
        message: String,
    },
    ResyncRequired {
        #[serde(with = "json_safe_u64")]
        #[schemars(with = "JsonSafeU64")]
        #[ts(type = "number")]
        missed: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AppTurnEventNotification {
    pub thread_id: String,
    pub turn_id: String,
    pub event: AppTurnEvent,
}

#[cfg(test)]
mod app_server_contract_tests {
    use serde::de::DeserializeOwned;

    use super::*;

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct PublicWireCorpus {
        schema_version: u32,
        decoders: BTreeMap<String, PublicWireCases>,
    }

    #[derive(Debug, Deserialize)]
    struct PublicWireCases {
        schema: String,
        valid: Vec<PublicWireCase>,
        invalid: Vec<PublicWireCase>,
    }

    #[derive(Debug, Deserialize)]
    struct PublicWireCase {
        name: String,
        value: Value,
    }

    fn public_wire_corpus() -> PublicWireCorpus {
        serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../packages/protocol/fixtures/app-python-wire.json"
        )))
        .expect("public Python wire corpus")
    }

    fn assert_public_wire_type<T>(corpus: &PublicWireCorpus, python_type: &str, schema: &str)
    where
        T: DeserializeOwned + Serialize,
    {
        let cases = corpus
            .decoders
            .get(python_type)
            .unwrap_or_else(|| panic!("missing fixture decoder {python_type}"));
        assert_eq!(cases.schema, schema, "schema name for {python_type}");
        for case in &cases.valid {
            let decoded = serde_json::from_value::<T>(case.value.clone()).unwrap_or_else(|error| {
                panic!("valid {python_type} fixture {} failed: {error}", case.name)
            });
            assert_eq!(
                serde_json::to_value(decoded).expect("serialize accepted public wire value"),
                case.value,
                "valid {python_type} fixture {} did not normalize canonically",
                case.name
            );
        }
        for case in &cases.invalid {
            assert!(
                serde_json::from_value::<T>(case.value.clone()).is_err(),
                "invalid {python_type} fixture {} was accepted",
                case.name
            );
        }
    }

    fn assert_serialized_variant_matches_schema<T>(tag: &str, value: T)
    where
        T: JsonSchema + Serialize,
    {
        let serialized = serde_json::to_value(value).expect("serialize fixture");
        let object = serialized.as_object().expect("serialized object");
        let tag_value = object
            .get(tag)
            .and_then(Value::as_str)
            .expect("serialized tag");
        let schema = serde_json::to_value(schemars::schema_for!(T)).expect("schema json");
        let branch = schema["oneOf"]
            .as_array()
            .expect("tagged enum branches")
            .iter()
            .find(|branch| {
                branch["properties"][tag]["enum"]
                    .as_array()
                    .is_some_and(|values| values.iter().any(|value| value == tag_value))
            })
            .expect("matching schema branch");
        let properties = branch["properties"].as_object().expect("branch properties");
        for key in object.keys() {
            assert!(
                properties.contains_key(key),
                "serialized key {key:?} is absent from the {tag_value:?} schema branch"
            );
        }
        for required in branch["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            assert!(
                object.contains_key(required),
                "schema-required key {required:?} is absent from serialized {tag_value:?}"
            );
        }
    }

    #[test]
    fn app_turn_event_schema_uses_the_serialized_camel_case_keys() {
        assert_serialized_variant_matches_schema(
            "type",
            AppTurnEvent::Accepted {
                receipt: AppTurnReceipt {
                    accepted: true,
                    thread_id: "thread-1".to_string(),
                    turn_id: "turn-2".to_string(),
                    client_turn_id: Some("client-turn-2".to_string()),
                },
                queue_position: Some(1),
            },
        );
        assert_serialized_variant_matches_schema(
            "type",
            AppTurnEvent::Started {
                thread_id: "thread-1".to_string(),
                turn_id: "turn-1".to_string(),
            },
        );
        assert_serialized_variant_matches_schema(
            "type",
            AppTurnEvent::Message {
                stage: AppItemStage::Completed,
                message: serde_json::json!({"role": "assistant"}),
                usage: Some(serde_json::json!({"inputTokens": 1})),
                metadata: Some(serde_json::json!({"provider": "fake"})),
                accounting: Some(serde_json::json!({"reportedTotalTokens": 1})),
            },
        );
        assert_serialized_variant_matches_schema(
            "type",
            AppTurnEvent::MessageDelta {
                text: "hello".to_string(),
            },
        );
        assert_serialized_variant_matches_schema(
            "type",
            AppTurnEvent::InteractionRequested {
                interaction_id: "call_1".to_string(),
                kind: "clarify".to_string(),
                payload: Value::Null,
            },
        );
        assert_serialized_variant_matches_schema(
            "type",
            AppTurnEvent::InteractionResolved {
                interaction_id: "call_1".to_string(),
                reason: "answered".to_string(),
            },
        );
        assert_serialized_variant_matches_schema(
            "type",
            AppTurnEvent::Completed {
                thread_id: "thread-1".to_string(),
                turn_id: "turn-1".to_string(),
                outcome: AppTurnOutcome::Completed,
            },
        );
        assert_serialized_variant_matches_schema(
            "type",
            AppTurnEvent::Failed {
                thread_id: "thread-1".to_string(),
                turn_id: "turn-1".to_string(),
                message: "failed".to_string(),
            },
        );
    }

    #[test]
    fn public_python_wire_types_match_the_shared_cross_language_corpus() {
        let corpus = public_wire_corpus();
        assert_eq!(corpus.schema_version, 1);
        assert_eq!(corpus.decoders.len(), 11);
        assert_public_wire_type::<AppPendingInteraction>(
            &corpus,
            "PendingInteraction",
            "AppPendingInteraction",
        );
        assert_public_wire_type::<AppThreadCompactResult>(
            &corpus,
            "CompactionResult",
            "AppThreadCompactResult",
        );
        assert_public_wire_type::<AppThreadItem>(&corpus, "ThreadItem", "AppThreadItem");
        assert_public_wire_type::<AppThreadSummary>(&corpus, "ThreadSummary", "AppThreadSummary");
        assert_public_wire_type::<AppThreadSnapshot>(
            &corpus,
            "ThreadSnapshot",
            "AppThreadSnapshot",
        );
        assert_public_wire_type::<AppTurnReceipt>(&corpus, "TurnReceipt", "AppTurnReceipt");
        assert_public_wire_type::<AppTurnResult>(&corpus, "TurnResult", "AppTurnResult");
        assert_public_wire_type::<AppTurnEvent>(&corpus, "TurnEvent", "AppTurnEvent");
        assert_public_wire_type::<AppFilesystemApprovalTarget>(
            &corpus,
            "FilesystemApprovalTarget",
            "AppFilesystemApprovalTarget",
        );
        assert_public_wire_type::<AppFilesystemApprovalRequest>(
            &corpus,
            "FilesystemApprovalRequest",
            "AppFilesystemApprovalRequest",
        );
        assert_public_wire_type::<AppMcpStartupApprovalRequest>(
            &corpus,
            "McpStartupApprovalRequest",
            "AppMcpStartupApprovalRequest",
        );
    }

    #[test]
    fn interaction_response_schema_uses_the_serialized_camel_case_keys() {
        assert_serialized_variant_matches_schema(
            "kind",
            AppInteractionResponse::Permission {
                outcome: AppApprovalOutcome::AllowTurn,
                filesystem_directory: Some("/workspace".to_string()),
            },
        );
    }
}
