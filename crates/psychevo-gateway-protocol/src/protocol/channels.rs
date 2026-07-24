#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ChannelListParams {
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ChannelIdParams {
    pub id: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ChannelEnableParams {
    pub id: String,
    pub enabled: bool,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ChannelUpdateParams {
    pub id: String,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    #[serde(rename = "runtimeRef")]
    pub runtime_ref: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub require_mention: Option<bool>,
    #[serde(default)]
    pub allow_users: Option<Vec<String>>,
    #[serde(default)]
    pub allow_groups: Option<Vec<String>>,
    #[serde(default)]
    pub credential_env: Option<String>,
    #[serde(default)]
    pub account_env: Option<String>,
    #[serde(default)]
    pub base_url_env: Option<String>,
    #[serde(default)]
    pub app_id_env: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ChannelDoctorParams {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub live: Option<bool>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ChannelWechatQrStartParams {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub ilink_base_url: Option<String>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ChannelWechatQrStartResult {
    pub session_id: String,
    pub qr_url: String,
    #[serde(default)]
    pub qr_image: Option<String>,
    #[serde(default)]
    pub qr_svg: Option<String>,
    pub status: String,
    pub message: String,
    #[serde(serialize_with = "json_safe_u64::serialize", deserialize_with = "json_safe_u64::deserialize")]
    #[schemars(with = "JsonSafeU64")]
    #[ts(type = "number")]
    pub interval_ms: u64,
    #[serde(serialize_with = "json_safe_i64::serialize", deserialize_with = "json_safe_i64::deserialize")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ChannelWechatQrPollParams {
    pub session_id: String,
    #[serde(default)]
    pub enable: Option<bool>,
    #[serde(default)]
    pub scope: Option<GatewayRequestScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ChannelWechatQrPollResult {
    pub done: bool,
    pub status: String,
    pub message: String,
    #[serde(default)]
    pub channel: Option<ChannelConfigView>,
    #[serde(default)]
    #[serde(serialize_with = "option_json_safe_i64::serialize", deserialize_with = "option_json_safe_i64::deserialize")]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub expires_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ChannelListResult {
    pub channels: Vec<ChannelConfigView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ChannelEnableResult {
    pub channel: ChannelConfigView,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ChannelSourceListResult {
    pub sources: Vec<ChannelSourceBindingView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ChannelSourceBindingView {
    pub source_key: String,
    pub connection_id: String,
    pub platform: String,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub chat_type: Option<String>,
    #[serde(default)]
    pub chat_label: Option<String>,
    #[serde(default)]
    pub user_label: Option<String>,
    #[serde(default)]
    pub visible_name: Option<String>,
    pub thread_id: String,
    #[serde(default)]
    pub thread_title: Option<String>,
    pub cwd: String,
    pub activity_status: String,
    #[serde(serialize_with = "json_safe_usize::serialize", deserialize_with = "json_safe_usize::deserialize")]
    #[schemars(with = "JsonSafeU64")]
    #[ts(type = "number")]
    pub queued_turns: usize,
    #[serde(serialize_with = "json_safe_i64::serialize", deserialize_with = "json_safe_i64::deserialize")]
    #[schemars(with = "JsonSafeI64")]
    #[ts(type = "number")]
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ChannelConfigView {
    pub id: String,
    pub channel: String,
    #[serde(default)]
    pub domain: Option<String>,
    pub enabled: bool,
    pub label: String,
    pub transport: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    #[serde(rename = "runtimeRef")]
    pub runtime_ref: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default, rename = "permissionMode")]
    pub permission_mode: Option<String>,
    #[serde(rename = "requireMention")]
    pub require_mention: bool,
    pub credential: ChannelCredentialView,
    #[serde(default)]
    pub account: Option<ChannelCredentialView>,
    #[serde(default)]
    pub base_url: Option<ChannelCredentialView>,
    #[serde(default)]
    pub app_id: Option<ChannelCredentialView>,
    pub allowlist: ChannelAllowlistView,
    #[serde(rename = "runtimeStatus")]
    pub runtime_status: String,
    pub runner: ChannelRunnerView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ChannelRunnerView {
    pub state: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    #[serde(serialize_with = "option_json_safe_i64::serialize", deserialize_with = "option_json_safe_i64::deserialize")]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub last_poll_at_ms: Option<i64>,
    #[serde(default)]
    #[serde(serialize_with = "option_json_safe_i64::serialize", deserialize_with = "option_json_safe_i64::deserialize")]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub last_healthy_poll_at_ms: Option<i64>,
    #[serde(default)]
    #[serde(serialize_with = "option_json_safe_i64::serialize", deserialize_with = "option_json_safe_i64::deserialize")]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub last_inbound_at_ms: Option<i64>,
    #[serde(default)]
    #[serde(serialize_with = "option_json_safe_i64::serialize", deserialize_with = "option_json_safe_i64::deserialize")]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub last_outbound_at_ms: Option<i64>,
    #[serde(default)]
    #[serde(serialize_with = "option_json_safe_i64::serialize", deserialize_with = "option_json_safe_i64::deserialize")]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub last_ilink_errcode: Option<i64>,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ChannelCredentialView {
    #[serde(default)]
    pub env: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ChannelAllowlistView {
    #[serde(default)]
    pub users: Vec<String>,
    #[serde(default)]
    pub groups: Vec<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ChannelDoctorCheck {
    pub name: String,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ChannelDoctorChannelView {
    pub id: String,
    pub channel: String,
    pub enabled: bool,
    #[serde(rename = "runtimeStatus")]
    pub runtime_status: String,
    pub runner: ChannelRunnerView,
    pub checks: Vec<ChannelDoctorCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ChannelDoctorResult {
    pub live: bool,
    pub channels: Vec<ChannelDoctorChannelView>,
}
