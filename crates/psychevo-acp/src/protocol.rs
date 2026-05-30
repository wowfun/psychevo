#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn send_runtime_event_update(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    value: Value,
) {
    let Some(update) = runtime_event_session_update(&value) else {
        return;
    };
    send_session_update(cx, session_id.clone(), update);
}

pub(crate) fn runtime_event_session_update(value: &Value) -> Option<SessionUpdate> {
    let event_type = value.get("type").and_then(Value::as_str)?;
    let update = match event_type {
        "tool_call_pending" => {
            let call_id = value
                .get("tool_call_id")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let tool_name = value
                .get("tool_name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                call_id.to_string(),
                ToolCallUpdateFields::new()
                    .title(tool_title(tool_name))
                    .kind(tool_kind(tool_name))
                    .status(ToolCallStatus::Pending)
                    .raw_input(tool_call_pending_raw_input(value)),
            ))
        }
        "tool_execution_start" => {
            let call_id = value
                .get("tool_call_id")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let tool_name = value
                .get("tool_name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let args = value.get("args").cloned();
            let mut tool_call = ToolCall::new(call_id.to_string(), tool_title(tool_name))
                .kind(tool_kind(tool_name))
                .status(ToolCallStatus::InProgress)
                .raw_input(args);
            if let Some(meta) = tool_timing_meta("startedAtMs", value.get("started_at_ms").cloned())
            {
                tool_call = tool_call.meta(meta);
            }
            SessionUpdate::ToolCall(tool_call)
        }
        "tool_execution_end" => {
            let call_id = value
                .get("tool_call_id")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let tool_name = value
                .get("tool_name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let result = value.get("result").cloned();
            let failed = value
                .get("outcome")
                .and_then(Value::as_str)
                .is_some_and(|outcome| outcome != "normal");
            let content = result
                .as_ref()
                .map(compact_tool_result_text)
                .filter(|text| !text.is_empty())
                .map(|text| vec![ToolCallContent::from(text)])
                .unwrap_or_default();
            let mut update = ToolCallUpdate::new(
                call_id.to_string(),
                ToolCallUpdateFields::new()
                    .title(tool_title(tool_name))
                    .status(if failed {
                        ToolCallStatus::Failed
                    } else {
                        ToolCallStatus::Completed
                    })
                    .content(content)
                    .raw_output(result),
            );
            if let Some(meta) = tool_timing_meta("elapsedMs", value.get("elapsed_ms").cloned()) {
                update = update.meta(meta);
            }
            SessionUpdate::ToolCallUpdate(update)
        }
        _ => return None,
    };
    Some(update)
}

pub(crate) fn tool_timing_meta(
    field_name: &str,
    field_value: Option<Value>,
) -> Option<serde_json::Map<String, Value>> {
    let field_value = field_value?;
    let mut timing = serde_json::Map::new();
    timing.insert(
        "source".to_string(),
        Value::String("psychevo_runtime".to_string()),
    );
    timing.insert(field_name.to_string(), field_value);
    let mut psychevo = serde_json::Map::new();
    psychevo.insert("toolTiming".to_string(), Value::Object(timing));
    let mut meta = serde_json::Map::new();
    meta.insert("psychevo".to_string(), Value::Object(psychevo));
    Some(meta)
}

pub(crate) fn tool_call_pending_raw_input(value: &Value) -> Value {
    let arguments_json = value
        .get("arguments_json")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match serde_json::from_str::<Value>(arguments_json) {
        Ok(parsed) => parsed,
        Err(_) => json!({
            "arguments_json": arguments_json,
            "partial": true,
        }),
    }
}

pub(crate) fn single_text_prompt(prompt: &[ContentBlock]) -> Option<&str> {
    match prompt {
        [ContentBlock::Text(content)] => Some(content.text.as_str()),
        _ => None,
    }
}

pub(crate) fn prompt_parts(prompt: Vec<ContentBlock>, cwd: &Path) -> (String, Vec<ImageInput>) {
    let mut text = Vec::new();
    let mut images = Vec::new();
    for block in prompt {
        match block {
            ContentBlock::Text(content) => text.push(content.text),
            ContentBlock::Image(content) => {
                if let Some(uri) = content.uri.filter(|uri| !uri.trim().is_empty()) {
                    images.push(ImageInput::ImageUrl(uri));
                } else if !content.data.trim().is_empty() {
                    images.push(ImageInput::ImageUrl(format!(
                        "data:{};base64,{}",
                        content.mime_type, content.data
                    )));
                } else {
                    text.push("[image omitted: empty ACP image block]".to_string());
                }
            }
            ContentBlock::ResourceLink(link) => {
                process_resource_link(link, cwd, &mut text, &mut images)
            }
            ContentBlock::Resource(resource) => {
                process_embedded_resource(resource, &mut text, &mut images)
            }
            ContentBlock::Audio(_) => {
                text.push("[audio omitted: Psychevo ACP does not support audio input]".to_string());
            }
            other => {
                if let Ok(serialized) = serde_json::to_string(&other) {
                    text.push(serialized);
                }
            }
        }
    }
    (text.join("\n\n"), images)
}

fn process_embedded_resource(
    resource: agent_client_protocol::schema::EmbeddedResource,
    text: &mut Vec<String>,
    images: &mut Vec<ImageInput>,
) {
    match resource.resource {
        agent_client_protocol::schema::EmbeddedResourceResource::TextResourceContents(resource) => {
            text.push(format!(
                "[resource: {}]\n{}",
                resource.uri,
                capped_text_resource(resource.text)
            ));
        }
        agent_client_protocol::schema::EmbeddedResourceResource::BlobResourceContents(resource) => {
            let mime_type = resource
                .mime_type
                .as_deref()
                .unwrap_or("application/octet-stream");
            if mime_type.starts_with("image/") && !resource.blob.trim().is_empty() {
                images.push(ImageInput::ImageUrl(format!(
                    "data:{mime_type};base64,{}",
                    resource.blob
                )));
            } else {
                text.push(format!(
                    "[resource omitted: embedded blob {} has unsupported MIME type {}]",
                    resource.uri, mime_type
                ));
            }
        }
        _ => {
            text.push("[resource omitted: unsupported embedded ACP resource]".to_string());
        }
    }
}

fn process_resource_link(
    link: agent_client_protocol::schema::ResourceLink,
    cwd: &Path,
    text: &mut Vec<String>,
    images: &mut Vec<ImageInput>,
) {
    if is_remote_uri(&link.uri) {
        text.push(format!(
            "[resource omitted: remote ResourceLink was not fetched: {}]",
            link.uri
        ));
        return;
    }
    let Some(path) = local_resource_path(&link.uri, cwd) else {
        text.push(format!(
            "[resource omitted: unsupported ResourceLink URI: {}]",
            link.uri
        ));
        return;
    };
    let mime_type = link.mime_type.as_deref().unwrap_or_default();
    if mime_type.starts_with("image/") {
        images.push(ImageInput::LocalPath(path));
        return;
    }
    match read_capped_text_resource(&path) {
        Ok(contents) => text.push(format!("[resource: {}]\n{}", link.uri, contents)),
        Err(err) => text.push(format!(
            "[resource omitted: failed to read {}: {err}]",
            link.uri
        )),
    }
}

const ACP_TEXT_RESOURCE_MAX_BYTES: usize = 512 * 1024;
const ACP_TEXT_RESOURCE_MAX_LINES: usize = 2_000;

fn capped_text_resource(value: String) -> String {
    truncate_text_resource(&value)
}

fn read_capped_text_resource(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    let truncated_by_bytes = bytes.len() > ACP_TEXT_RESOURCE_MAX_BYTES;
    let slice = &bytes[..bytes.len().min(ACP_TEXT_RESOURCE_MAX_BYTES)];
    let mut text = String::from_utf8_lossy(slice).into_owned();
    if truncated_by_bytes {
        text.push_str("\n[truncated: ACP text resource byte cap reached]");
    }
    Ok(truncate_text_resource(&text))
}

fn truncate_text_resource(value: &str) -> String {
    let mut lines = value
        .lines()
        .take(ACP_TEXT_RESOURCE_MAX_LINES)
        .collect::<Vec<_>>();
    let truncated_by_lines = value.lines().count() > ACP_TEXT_RESOURCE_MAX_LINES;
    let mut text = lines.join("\n");
    if truncated_by_lines {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str("[truncated: ACP text resource line cap reached]");
    }
    lines.clear();
    text
}

fn is_remote_uri(uri: &str) -> bool {
    uri.starts_with("http://") || uri.starts_with("https://")
}

fn local_resource_path(uri: &str, cwd: &Path) -> Option<PathBuf> {
    let value = uri.trim();
    if value.is_empty() {
        return None;
    }
    let path = if let Some(rest) = value.strip_prefix("file://") {
        PathBuf::from(rest)
    } else if value.contains("://") {
        return None;
    } else {
        PathBuf::from(value)
    };
    Some(if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    })
}

#[derive(Debug, Default, Clone)]
pub(crate) struct AcpUsageAccumulator {
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
    thought_tokens: u64,
    cached_read_tokens: u64,
    cached_write_tokens: u64,
    has_usage: bool,
    turns: u64,
    accounting: AcpAccountingAccumulator,
    warnings: Vec<String>,
}

#[derive(Debug, Default, Clone)]
struct AcpAccountingAccumulator {
    context_input_tokens: Option<u64>,
    billable_input_tokens: Option<u64>,
    billable_output_tokens: Option<u64>,
    reasoning_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    cache_write_tokens: Option<u64>,
    reported_total_tokens: Option<u64>,
    estimated_cost_nanodollars: Option<i64>,
    pricing_source: Option<String>,
    pricing_tier: Option<String>,
}

impl AcpUsageAccumulator {
    pub(crate) fn record_stream_event(&mut self, event: &RunStreamEvent) {
        match event {
            RunStreamEvent::Event(value) => self.record_runtime_value(value),
            RunStreamEvent::Scoped { event, .. } => self.record_stream_event(event),
            RunStreamEvent::ReasoningDelta { .. }
            | RunStreamEvent::ReasoningEnd
            | RunStreamEvent::ClarifyRequest(_)
            | RunStreamEvent::ClarifyResolved(_) => {}
        }
    }

    pub(crate) fn add_warning(&mut self, warning: impl Into<String>) {
        let warning = warning.into();
        if !warning.trim().is_empty() && !self.warnings.contains(&warning) {
            self.warnings.push(warning);
        }
    }

    pub(crate) fn to_usage(&self) -> Option<Usage> {
        if self.has_usage {
            let mut usage = Usage::new(self.total_tokens, self.input_tokens, self.output_tokens);
            if self.thought_tokens > 0 {
                usage = usage.thought_tokens(self.thought_tokens);
            }
            if self.cached_read_tokens > 0 {
                usage = usage.cached_read_tokens(self.cached_read_tokens);
            }
            if self.cached_write_tokens > 0 {
                usage = usage.cached_write_tokens(self.cached_write_tokens);
            }
            Some(usage)
        } else {
            self.accounting.synthesized_usage()
        }
    }

    pub(crate) fn response_meta(&self) -> Option<serde_json::Map<String, Value>> {
        let mut psychevo = serde_json::Map::new();
        if self.turns > 0 {
            psychevo.insert("turns".to_string(), Value::from(self.turns));
        }
        if let Some(accounting) = self.accounting.public_json() {
            psychevo.insert("accounting".to_string(), accounting);
        }
        if !self.warnings.is_empty() {
            psychevo.insert("warnings".to_string(), json!(self.warnings));
        }
        if psychevo.is_empty() {
            None
        } else {
            let mut meta = serde_json::Map::new();
            meta.insert("psychevo".to_string(), Value::Object(psychevo));
            Some(meta)
        }
    }

    pub(crate) fn cumulative_cost_usd(&self) -> Option<f64> {
        self.accounting
            .estimated_cost_nanodollars
            .map(|value| value as f64 / 1_000_000_000.0)
    }

    fn record_runtime_value(&mut self, value: &Value) {
        let Some(event_type) = value.get("type").and_then(Value::as_str) else {
            return;
        };
        match event_type {
            "turn_start" => {
                self.turns = self.turns.saturating_add(1);
            }
            "warning" => {
                if let Some(message) = value.get("message").and_then(Value::as_str) {
                    self.add_warning(message);
                }
            }
            "message_end" => {
                self.record_usage(value.get("usage"));
                self.record_accounting(value.get("accounting"));
            }
            _ => {}
        }
    }

    fn record_usage(&mut self, usage: Option<&Value>) {
        let Some(usage) = usage else {
            return;
        };
        self.has_usage = true;
        let input = usage_u64(
            usage,
            &["input_tokens", "prompt_tokens", "context_input_tokens"],
        )
        .unwrap_or(0);
        let output = usage_u64(usage, &["output_tokens", "completion_tokens"]).unwrap_or(0);
        let total = usage_u64(usage, &["total_tokens", "reported_total_tokens"])
            .unwrap_or_else(|| input.saturating_add(output));
        self.input_tokens = self.input_tokens.saturating_add(input);
        self.output_tokens = self.output_tokens.saturating_add(output);
        self.total_tokens = self.total_tokens.saturating_add(total);
        self.thought_tokens = self
            .thought_tokens
            .saturating_add(usage_u64(usage, &["thought_tokens", "reasoning_tokens"]).unwrap_or(0));
        self.cached_read_tokens = self.cached_read_tokens.saturating_add(
            usage_u64(
                usage,
                &[
                    "cached_read_tokens",
                    "cache_read_tokens",
                    "cached_tokens",
                    "cached_input_tokens",
                ],
            )
            .unwrap_or(0),
        );
        self.cached_write_tokens = self.cached_write_tokens.saturating_add(
            usage_u64(
                usage,
                &[
                    "cached_write_tokens",
                    "cache_write_tokens",
                    "cache_creation_input_tokens",
                    "cache_written_tokens",
                ],
            )
            .unwrap_or(0),
        );
    }

    fn record_accounting(&mut self, accounting: Option<&Value>) {
        let Some(accounting) = accounting else {
            return;
        };
        self.accounting.record(accounting);
    }
}

impl AcpAccountingAccumulator {
    fn record(&mut self, value: &Value) {
        add_optional_u64(
            &mut self.context_input_tokens,
            value_u64(value, "context_input_tokens"),
        );
        add_optional_u64(
            &mut self.billable_input_tokens,
            value_u64(value, "billable_input_tokens"),
        );
        add_optional_u64(
            &mut self.billable_output_tokens,
            value_u64(value, "billable_output_tokens"),
        );
        add_optional_u64(
            &mut self.reasoning_tokens,
            value_u64(value, "reasoning_tokens"),
        );
        add_optional_u64(
            &mut self.cache_read_tokens,
            value_u64(value, "cache_read_tokens"),
        );
        add_optional_u64(
            &mut self.cache_write_tokens,
            value_u64(value, "cache_write_tokens"),
        );
        add_optional_u64(
            &mut self.reported_total_tokens,
            value_u64(value, "reported_total_tokens"),
        );
        add_optional_i64(
            &mut self.estimated_cost_nanodollars,
            value_i64(value, "estimated_cost_nanodollars"),
        );
        merge_optional_string(
            &mut self.pricing_source,
            value.get("pricing_source").and_then(Value::as_str),
        );
        merge_optional_string(
            &mut self.pricing_tier,
            value.get("pricing_tier").and_then(Value::as_str),
        );
    }

    fn public_json(&self) -> Option<Value> {
        let mut object = serde_json::Map::new();
        insert_optional(
            &mut object,
            "context_input_tokens",
            self.context_input_tokens,
        );
        insert_optional(
            &mut object,
            "billable_input_tokens",
            self.billable_input_tokens,
        );
        insert_optional(
            &mut object,
            "billable_output_tokens",
            self.billable_output_tokens,
        );
        insert_optional(&mut object, "reasoning_tokens", self.reasoning_tokens);
        insert_optional(&mut object, "cache_read_tokens", self.cache_read_tokens);
        insert_optional(&mut object, "cache_write_tokens", self.cache_write_tokens);
        insert_optional(
            &mut object,
            "reported_total_tokens",
            self.reported_total_tokens,
        );
        if let Some(value) = self.estimated_cost_nanodollars {
            object.insert("estimated_cost_nanodollars".to_string(), Value::from(value));
        }
        if let Some(value) = &self.pricing_source {
            object.insert("pricing_source".to_string(), Value::String(value.clone()));
        }
        if let Some(value) = &self.pricing_tier {
            object.insert("pricing_tier".to_string(), Value::String(value.clone()));
        }
        (!object.is_empty()).then_some(Value::Object(object))
    }

    fn synthesized_usage(&self) -> Option<Usage> {
        if !self.has_token_data() {
            return None;
        }
        let cache_read = self.cache_read_tokens.unwrap_or(0);
        let cache_write = self.cache_write_tokens.unwrap_or(0);
        let reasoning = self.reasoning_tokens.unwrap_or(0);
        let input = self
            .context_input_tokens
            .or_else(|| {
                self.billable_input_tokens
                    .map(|value| value.saturating_add(cache_read).saturating_add(cache_write))
            })
            .unwrap_or(0);
        let output = self
            .billable_output_tokens
            .unwrap_or(0)
            .saturating_add(reasoning);
        let total = self
            .reported_total_tokens
            .unwrap_or_else(|| input.saturating_add(output));
        let mut usage = Usage::new(total, input, output);
        if reasoning > 0 {
            usage = usage.thought_tokens(reasoning);
        }
        if cache_read > 0 {
            usage = usage.cached_read_tokens(cache_read);
        }
        if cache_write > 0 {
            usage = usage.cached_write_tokens(cache_write);
        }
        Some(usage)
    }

    fn has_token_data(&self) -> bool {
        self.context_input_tokens.is_some()
            || self.billable_input_tokens.is_some()
            || self.billable_output_tokens.is_some()
            || self.reasoning_tokens.is_some()
            || self.cache_read_tokens.is_some()
            || self.cache_write_tokens.is_some()
            || self.reported_total_tokens.is_some()
    }
}

fn usage_u64(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| value_u64(value, key))
}

fn value_u64(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_i64().and_then(|value| u64::try_from(value).ok()))
            .or_else(|| {
                value
                    .as_str()
                    .and_then(|value| value.trim().parse::<u64>().ok())
            })
    })
}

fn value_i64(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| {
                value
                    .as_str()
                    .and_then(|value| value.trim().parse::<i64>().ok())
            })
    })
}

fn add_optional_u64(field: &mut Option<u64>, value: Option<u64>) {
    if let Some(value) = value {
        *field = Some(field.unwrap_or(0).saturating_add(value));
    }
}

fn add_optional_i64(field: &mut Option<i64>, value: Option<i64>) {
    if let Some(value) = value {
        *field = Some(field.unwrap_or(0).saturating_add(value));
    }
}

fn merge_optional_string(field: &mut Option<String>, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    match field {
        None => *field = Some(value.to_string()),
        Some(current) if current == value || current == "mixed" => {}
        Some(current) => *current = "mixed".to_string(),
    }
}

fn insert_optional(object: &mut serde_json::Map<String, Value>, key: &str, value: Option<u64>) {
    if let Some(value) = value {
        object.insert(key.to_string(), Value::from(value));
    }
}

pub(crate) fn acp_mcp_servers(servers: Vec<McpServer>) -> Vec<McpServerInput> {
    servers
        .into_iter()
        .map(|server| match server {
            McpServer::Http(McpServerHttp {
                name, url, headers, ..
            }) => McpServerInput {
                name,
                transport: McpTransportInput::StreamableHttp {
                    url,
                    headers: headers
                        .into_iter()
                        .map(|header| (header.name, header.value))
                        .collect(),
                },
            },
            McpServer::Stdio(McpServerStdio {
                name,
                command,
                args,
                env,
                ..
            }) => McpServerInput {
                name,
                transport: McpTransportInput::Stdio {
                    command,
                    args,
                    env: env_variable_map(env),
                },
            },
            McpServer::Sse(server) => McpServerInput {
                name: server.name,
                transport: McpTransportInput::Unsupported {
                    kind: "sse".to_string(),
                },
            },
            McpServer::Acp(server) => McpServerInput {
                name: server.name,
                transport: McpTransportInput::Unsupported {
                    kind: "acp".to_string(),
                },
            },
            _ => McpServerInput {
                name: "unknown".to_string(),
                transport: McpTransportInput::Unsupported {
                    kind: "unknown".to_string(),
                },
            },
        })
        .collect()
}

pub(crate) fn env_variable_map(vars: Vec<EnvVariable>) -> BTreeMap<String, String> {
    vars.into_iter().map(|var| (var.name, var.value)).collect()
}

pub(crate) fn mode_state(mode: RunMode) -> SessionModeState {
    SessionModeState::new(
        mode.as_str(),
        vec![
            SessionMode::new("default", "Default").description("Run tools and edit code"),
            SessionMode::new("plan", "Plan").description("Discuss and inspect without edits"),
        ],
    )
}

pub(crate) fn session_config_options(
    mode: RunMode,
) -> Vec<agent_client_protocol::schema::SessionConfigOption> {
    vec![agent_client_protocol::schema::SessionConfigOption::select(
        "mode",
        "Mode",
        mode.as_str(),
        vec![
            SessionConfigSelectOption::new("default", "Default"),
            SessionConfigSelectOption::new("plan", "Plan"),
        ],
    )]
}

pub(crate) fn tool_title(tool_name: &str) -> String {
    if let Some(rest) = tool_name.strip_prefix("mcp__")
        && let Some((server, tool)) = rest.split_once("__")
    {
        return format!("Tool: {server}/{tool}");
    }
    format!("Tool: {tool_name}")
}

pub(crate) fn tool_kind(tool_name: &str) -> ToolKind {
    match tool_name {
        "read" => ToolKind::Read,
        "write" | "edit" => ToolKind::Edit,
        "exec_command" | "write_stdin" => ToolKind::Execute,
        "web_fetch" => ToolKind::Fetch,
        _ => ToolKind::Other,
    }
}

pub(crate) fn compact_tool_result_text(value: &Value) -> String {
    value
        .get("model_content")
        .and_then(Value::as_str)
        .or_else(|| value.get("error").and_then(Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| serde_json::to_string(value).unwrap_or_default())
}

pub(crate) fn stop_reason(outcome: psychevo_ai::Outcome) -> StopReason {
    match outcome {
        psychevo_ai::Outcome::Normal => StopReason::EndTurn,
        psychevo_ai::Outcome::Aborted => StopReason::Cancelled,
        psychevo_ai::Outcome::Stopped => StopReason::EndTurn,
        psychevo_ai::Outcome::Failed => StopReason::Refusal,
    }
}

pub(crate) fn acp_internal_error(err: impl std::fmt::Display) -> Error {
    Error::internal_error().data(err.to_string())
}

pub(crate) fn env_path_or_default(
    env: &BTreeMap<String, String>,
    name: &str,
    default: &str,
    cwd: &Path,
) -> PathBuf {
    env.get(name)
        .filter(|value| !value.trim().is_empty())
        .map(String::as_str)
        .unwrap_or(default)
        .pipe(|value| resolve_path(value, env, cwd))
}

pub(crate) fn resolve_path(value: &str, env: &BTreeMap<String, String>, cwd: &Path) -> PathBuf {
    let path = if value == "~" {
        env.get("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| cwd.to_path_buf())
    } else if let Some(rest) = value.strip_prefix("~/") {
        env.get("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| cwd.to_path_buf())
            .join(rest)
    } else {
        PathBuf::from(value)
    };
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

pub(crate) trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::*;

    #[test]
    fn converts_acp_mcp_servers_to_runtime_inputs() {
        let servers = vec![McpServer::Stdio(
            McpServerStdio::new("repo tools", "server")
                .args(vec!["--stdio".to_string()])
                .env(vec![EnvVariable::new("A", "B")]),
        )];
        let converted = acp_mcp_servers(servers);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].name, "repo tools");
        match &converted[0].transport {
            McpTransportInput::Stdio { args, env, .. } => {
                assert_eq!(args, &vec!["--stdio".to_string()]);
                assert_eq!(env.get("A").map(String::as_str), Some("B"));
            }
            other => panic!("unexpected transport: {other:?}"),
        }
    }

    #[test]
    fn converts_prompt_text_and_http_images() {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let (text, images) = prompt_parts(
            vec![
                ContentBlock::Text(agent_client_protocol::schema::TextContent::new("hello")),
                ContentBlock::Image(
                    agent_client_protocol::schema::ImageContent::new("", "image/png")
                        .uri("https://example.com/a.png"),
                ),
            ],
            &cwd,
        );
        assert_eq!(text, "hello");
        assert_eq!(
            images,
            vec![ImageInput::ImageUrl(
                "https://example.com/a.png".to_string()
            )]
        );
    }

    #[test]
    fn synthesizes_usage_from_runtime_accounting() {
        let mut usage = AcpUsageAccumulator::default();
        usage.record_stream_event(&RunStreamEvent::Event(json!({
            "type": "message_end",
            "accounting": {
                "billable_input_tokens": 8,
                "billable_output_tokens": 5,
                "cache_read_tokens": 2,
                "reasoning_tokens": 1,
                "reported_total_tokens": 16,
            },
        })));

        let usage = usage.to_usage().expect("usage");
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 6);
        assert_eq!(usage.cached_read_tokens, Some(2));
        assert_eq!(usage.thought_tokens, Some(1));
        assert_eq!(usage.total_tokens, 16);
    }

    #[test]
    fn tool_call_pending_raw_input_preserves_partial_arguments() {
        assert_eq!(
            tool_call_pending_raw_input(&json!({
                "arguments_json": "{\"path\":\"add.py\"",
            })),
            json!({
                "arguments_json": "{\"path\":\"add.py\"",
                "partial": true,
            })
        );
        assert_eq!(
            tool_call_pending_raw_input(&json!({
                "arguments_json": "{\"path\":\"add.py\"}",
            })),
            json!({ "path": "add.py" })
        );
    }

    #[test]
    fn runtime_tool_execution_start_includes_timing_meta() {
        let update = runtime_event_session_update(&json!({
            "type": "tool_execution_start",
            "tool_call_id": "call-1",
            "tool_name": "edit",
            "args": { "path": "add.py" },
            "started_at_ms": 1_234,
        }))
        .expect("session update");

        let SessionUpdate::ToolCall(tool_call) = update else {
            panic!("expected tool call update");
        };
        assert_eq!(
            tool_call.meta.as_ref().expect("meta")["psychevo"]["toolTiming"],
            json!({
                "source": "psychevo_runtime",
                "startedAtMs": 1_234,
            })
        );
    }

    #[test]
    fn runtime_tool_execution_end_includes_timing_meta() {
        let update = runtime_event_session_update(&json!({
            "type": "tool_execution_end",
            "tool_call_id": "call-1",
            "tool_name": "edit",
            "result": { "success": true },
            "outcome": "normal",
            "elapsed_ms": 321,
        }))
        .expect("session update");

        let SessionUpdate::ToolCallUpdate(update) = update else {
            panic!("expected tool call update");
        };
        assert_eq!(
            update.meta.as_ref().expect("meta")["psychevo"]["toolTiming"],
            json!({
                "source": "psychevo_runtime",
                "elapsedMs": 321,
            })
        );
    }

    #[test]
    fn advertises_tools_slash_command() {
        let commands = available_command_lines_from(available_commands_from(
            psychevo_runtime::command_registry::available_slash_commands_for_surface(
                acp_command_capabilities(),
                false,
                &[],
                ACP_COMMAND_ADVERTISEMENT_LIMIT,
            ),
        ))
        .join("\n");
        assert!(
            commands.contains("/tools [list|enable|disable <toolset>] - toolsets"),
            "{commands}"
        );
    }

    #[test]
    fn parses_slash_prompt_command_and_args() {
        use psychevo_runtime::command_registry::{
            SlashCommandAction, SlashCommandParse, parse_slash_command_line,
        };

        let SlashCommandParse::Known(invocation) = parse_slash_command_line(" /tools ") else {
            panic!("expected known command");
        };
        assert_eq!(invocation.spec.action, SlashCommandAction::Tools);
        assert!(invocation.args.is_empty());

        let SlashCommandParse::Known(invocation) = parse_slash_command_line("/mode plan") else {
            panic!("expected known command");
        };
        assert_eq!(invocation.spec.action, SlashCommandAction::ModeSet);
        assert_eq!(invocation.args, "plan");

        assert!(matches!(
            parse_slash_command_line("hello /tools"),
            SlashCommandParse::NotSlash
        ));
    }

    #[test]
    fn handles_status_slash_command_locally() {
        let agent = PsychevoAcpAgent::new(AcpOptions {
            home: std::env::temp_dir().join("psychevo-acp-test-home"),
            db_path: PathBuf::from(":memory:"),
            config_path: None,
            inherited_env: BTreeMap::new(),
        })
        .expect("agent");
        let session_id = SessionId::new("acp-test");
        let session = AcpSession::new(
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            None,
            Vec::new(),
        );
        let text = agent.status_command_text(&session_id, &session);
        assert!(text.contains("ACP session: acp-test"), "{text}");
        assert!(text.contains("commands: "), "{text}");
    }
}
