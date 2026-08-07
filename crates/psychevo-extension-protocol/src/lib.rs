use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: &str = "psychevo-extension/1";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostCapabilities {
    #[serde(default)]
    pub structured_displays: bool,
    #[serde(default)]
    pub mcp_apps: bool,
    #[serde(default)]
    pub channels: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InitializeParams {
    pub protocol: String,
    pub extension_id: String,
    pub extension_version: String,
    pub scope: String,
    pub package_root: PathBuf,
    pub data_root: PathBuf,
    #[serde(default)]
    pub capabilities: HostCapabilities,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InitializeResult {
    pub protocol: String,
    pub extension_id: String,
    #[serde(default)]
    pub capabilities: ContributionDescriptors,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContributionDescriptors {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<CommandDescriptor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<ChannelDescriptor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub displays: Vec<DisplayDescriptor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_apps: Vec<McpAppDescriptor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ExecutableDescriptor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hooks: Vec<ExecutableDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandDescriptor {
    pub name: String,
    pub usage: String,
    pub summary: String,
    #[serde(default)]
    pub argument_kind: CommandArgumentKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surfaces: Vec<ExtensionSurface>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_capabilities: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandArgumentKind {
    None,
    RequiredValue,
    OptionalValue,
    FixedEnum,
    #[default]
    TrailingArgs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionSurface {
    Cli,
    Tui,
    Web,
    Desktop,
    Gateway,
    Acp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChannelDescriptor {
    pub channel: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub delivery_capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChannelStartParams {
    pub connection_id: String,
    pub channel: String,
    #[serde(default)]
    pub configuration: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChannelConnectionParams {
    pub connection_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChannelPollResult {
    #[serde(default)]
    pub messages: Vec<ChannelInboundMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum WechatQrPollResult {
    #[serde(rename = "wait")]
    Waiting { message: String, base_url: String },
    #[serde(rename = "scaned")]
    Scanned { message: String, base_url: String },
    #[serde(rename = "scaned_but_redirect")]
    ScannedRedirect { message: String, base_url: String },
    #[serde(rename = "expired")]
    Expired { message: String },
    #[serde(rename = "confirmed")]
    Confirmed {
        account_id: String,
        token: String,
        base_url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user_id: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChannelSendParams {
    pub connection_id: String,
    pub message: ChannelOutboundMessage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChannelIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    pub platform: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_type: Option<String>,
    pub chat_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChannelInboundMessage {
    pub identity: ChannelIdentity,
    pub message_id: String,
    pub text: String,
    #[serde(default)]
    pub attachments: Vec<ChannelAttachment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChannelOutboundMessage {
    pub identity: ChannelIdentity,
    pub thread_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ChannelAttachment {
    Image {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
    },
    File {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size_bytes: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    MediaMetadata {
        media_kind: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size_bytes: Option<u64>,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisplayDescriptor {
    pub id: String,
    pub schema: Value,
    pub fallback: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpAppDescriptor {
    pub id: String,
    pub resource_uri: String,
    pub fallback: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connect_domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutableDescriptor {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandRunParams {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub surface: ExtensionSurface,
    pub interactive: bool,
    pub terminal: bool,
    #[serde(default)]
    pub host_capabilities: HostCapabilities,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandEffect {
    BoundedText {
        text: String,
    },
    StructuredDisplay {
        schema: String,
        value: Value,
        fallback: String,
    },
    Artifact {
        name: String,
        media_type: String,
        content_base64: String,
    },
    PromptSubmission {
        text: String,
    },
    HostRequest {
        action: HostAction,
        #[serde(default)]
        payload: Value,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostAction {
    OpenResource,
    RequestApproval,
    StartChannel,
    StopChannel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}
