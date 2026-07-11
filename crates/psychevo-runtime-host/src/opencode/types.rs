use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HealthResponse {
    pub healthy: bool,
    pub version: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionInfo {
    pub id: String,
    #[serde(default, rename = "parentID")]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub directory: Option<String>,
    #[serde(default)]
    pub time: SessionTime,
    #[serde(default)]
    pub model: Option<SessionModel>,
    #[serde(default)]
    pub agent: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionTime {
    #[serde(default)]
    pub created: Option<i64>,
    #[serde(default)]
    pub updated: Option<i64>,
    #[serde(default)]
    pub archived: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionModel {
    pub id: String,
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(default)]
    pub variant: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct MessageWithParts {
    pub info: MessageInfo,
    #[serde(default)]
    pub parts: Vec<Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageInfo {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub role: String,
    #[serde(default, rename = "parentID")]
    pub parent_id: Option<String>,
    #[serde(default, rename = "providerID")]
    pub provider_id: Option<String>,
    #[serde(default, rename = "modelID")]
    pub model_id: Option<String>,
    #[serde(default)]
    pub error: Option<Value>,
    #[serde(default)]
    pub time: MessageTime,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageTime {
    #[serde(default)]
    pub created: Option<i64>,
    #[serde(default)]
    pub completed: Option<i64>,
}

impl MessageWithParts {
    pub(crate) fn text(&self) -> String {
        self.parts
            .iter()
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("")
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PermissionRequest {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub permission: String,
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub always: Vec<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuestionRequest {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(default)]
    pub questions: Vec<QuestionInfo>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuestionInfo {
    pub question: String,
    pub header: String,
    #[serde(default)]
    pub options: Vec<QuestionOption>,
    #[serde(default)]
    pub multiple: bool,
    #[serde(default)]
    pub custom: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct QuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct TodoInfo {
    pub content: String,
    pub status: String,
    pub priority: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct FileDiffInfo {
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub patch: Option<String>,
    pub additions: i64,
    pub deletions: i64,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct AgentInfo {
    pub name: String,
    pub mode: String,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct StatusInfo {
    #[serde(rename = "type")]
    pub kind: String,
}

pub(crate) type StatusMap = BTreeMap<String, StatusInfo>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionCreateBody {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<PromptModel>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PromptModel {
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(rename = "modelID")]
    pub model_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PromptBody {
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<PromptModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub parts: Vec<PromptPart>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PromptPart {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub text: String,
}

#[derive(Debug, Clone)]
pub(crate) struct NativeEvent {
    pub id: Option<String>,
    pub directory: Option<String>,
    pub event_type: String,
    pub properties: Value,
}

impl NativeEvent {
    pub(crate) fn session_id(&self) -> Option<&str> {
        self.properties
            .get("sessionID")
            .and_then(Value::as_str)
            .or_else(|| {
                self.properties
                    .get("info")
                    .and_then(|info| info.get("sessionID"))
                    .and_then(Value::as_str)
            })
            .or_else(|| {
                self.properties
                    .get("part")
                    .and_then(|part| part.get("sessionID"))
                    .and_then(Value::as_str)
            })
    }
}

pub(crate) fn parse_model(value: Option<&str>) -> Option<PromptModel> {
    let (provider_id, model_id) = value?.split_once('/')?;
    if provider_id.is_empty() || model_id.is_empty() {
        return None;
    }
    Some(PromptModel {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
    })
}
