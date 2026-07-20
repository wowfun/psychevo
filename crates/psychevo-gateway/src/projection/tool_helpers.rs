fn live_tool_entry(
    turn_id: &str,
    value: &Value,
    status: TranscriptBlockStatus,
    body: Option<String>,
) -> TranscriptEntry {
    let tool_name = value
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("tool");
    let tool_call_id = value
        .get("tool_call_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|tool_call_id| !tool_call_id.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            temporary_tool_call_id_for_value(tool_name, 0, value, crate::gateway_now_ms() as u64)
        });
    let mut metadata = tool_value_metadata(value);
    set_metadata_field(&mut metadata, "tool_call_id", json!(tool_call_id.clone()));
    let title = live_tool_title(tool_name, &metadata);
    live_entry(
        turn_id,
        &format!("tool:{}", tool_call_id),
        TranscriptEntryRole::Assistant,
        tool_kind(tool_name),
        status,
        Some(title),
        body,
        Some(metadata),
    )
}

fn live_tool_title(tool_name: &str, metadata: &Value) -> String {
    if let Some(display) = metadata
        .get("display")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|display| !display.is_empty())
    {
        return display.to_string();
    }
    if tool_name == "web_search" {
        return metadata
            .get("args")
            .and_then(|args| args.get("query"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|query| !query.is_empty())
            .map(|query| compact_text(&format!("Searching the web {query}"), 180))
            .unwrap_or_else(|| "Searching the web".to_string());
    }
    if tool_name == "exec_command"
        && let Some(command) = metadata
            .get("args")
            .and_then(|args| args.get("cmd"))
            .and_then(Value::as_str)
            .and_then(first_shell_command_line)
    {
        return format!("exec_command {command}");
    }
    if tool_name == "write"
        && let Some(path) = metadata
            .get("write_argument_preview")
            .and_then(|preview| preview.get("path"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|path| !path.is_empty())
    {
        return format!("write {path}");
    }
    if tool_name == "spawn_agent"
        && let Some(title) = live_agent_tool_title(metadata)
    {
        return title;
    }
    tool_name.to_string()
}

fn live_agent_tool_title(metadata: &Value) -> Option<String> {
    let agent = agent_metadata_string(metadata, "agent_name")
        .or_else(|| agent_metadata_string(metadata, "agent_type"))
        .or_else(|| agent_metadata_string(metadata, "name"))?;
    let task_name = agent_metadata_string(metadata, "task_name")
        .filter(|task_name| !generated_agent_task_name(agent, task_name));
    let detail = task_name
        .or_else(|| agent_metadata_string(metadata, "agent_description"))
        .or_else(|| agent_metadata_string(metadata, "message"))
        .or_else(|| agent_metadata_string(metadata, "task"))
        .or_else(|| agent_metadata_string(metadata, "prompt"));
    Some(match detail {
        Some(detail) => format!("{agent}({})", compact_text(detail, 96)),
        None => agent.to_string(),
    })
}

fn generated_agent_task_name(agent: &str, task_name: &str) -> bool {
    let Some(suffix) = task_name
        .strip_prefix(agent)
        .and_then(|rest| rest.strip_prefix('_'))
    else {
        return false;
    };
    suffix.len() >= 8 && suffix.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn first_shell_command_line(text: &str) -> Option<&str> {
    let mut first_non_empty = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        first_non_empty.get_or_insert(line);
        if !line.starts_with('#') {
            return Some(line);
        }
    }
    first_non_empty
}

#[allow(clippy::too_many_arguments)]
fn live_entry(
    turn_id: &str,
    id_suffix: &str,
    role: TranscriptEntryRole,
    kind: TranscriptBlockKind,
    status: TranscriptBlockStatus,
    title: Option<String>,
    body: Option<String>,
    metadata: Option<Value>,
) -> TranscriptEntry {
    let now = crate::gateway_now_ms();
    let id = format!("live:{turn_id}:{id_suffix}");
    TranscriptEntry {
        id: id.clone(),
        thread_id: String::new(),
        turn_id: Some(turn_id.to_string()),
        message_seq: None,
        role,
        status,
        source: "runtime.stream".to_string(),
        blocks: vec![TranscriptBlock {
            id,
            kind,
            status,
            order: 0,
            phase_ordinal: None,
            source: "runtime.stream".to_string(),
            title,
            preview: body.as_deref().map(|text| compact_text(text, 240)),
            detail: body.clone(),
            body,
            artifact_ids: Vec::new(),
            metadata,
            result: None,
            created_at_ms: now,
            updated_at_ms: now,
        }],
        metadata: None,
        usage: None,
        accounting: None,
        created_at_ms: now,
        updated_at_ms: now,
    }
}

fn assistant_message_metadata(value: &Value) -> Value {
    let mut metadata = serde_json::json!({
        "usage": value.get("usage").cloned().unwrap_or(Value::Null),
        "metadata": value.get("metadata").cloned().unwrap_or(Value::Null),
        "accounting": value.get("accounting").cloned().unwrap_or(Value::Null),
    });
    if let Some(object) = metadata.as_object_mut()
        && let Some(message) = value.get("message")
    {
        for key in ["provider", "model", "finish_reason", "outcome"] {
            if let Some(field) = message.get(key).filter(|field| !field.is_null()) {
                object.insert(key.to_string(), field.clone());
            }
        }
    }
    metadata
}

fn assistant_phase_metadata(value: &Value) -> Value {
    let mut metadata = assistant_message_metadata(value);
    if let Some(object) = metadata.as_object_mut() {
        object.insert(
            "projection".to_string(),
            Value::String("assistant_phase".to_string()),
        );
    }
    metadata
}

fn assistant_message_is_tool_call_turn(message: Option<&Value>) -> bool {
    let Some(message) = message else {
        return false;
    };
    if message
        .get("finish_reason")
        .and_then(Value::as_str)
        .is_some_and(|finish_reason| finish_reason == "tool_calls")
    {
        return true;
    }
    if message
        .get("tool_calls")
        .is_some_and(|value| !value.is_null())
    {
        return true;
    }
    message
        .get("content")
        .and_then(Value::as_array)
        .is_some_and(|blocks| {
            blocks.iter().any(|block| {
                matches!(
                    block.get("type").and_then(Value::as_str),
                    Some("tool_call" | "tool_calls" | "tool_use")
                )
            })
        })
}

fn tool_value_metadata(value: &Value) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("projection".to_string(), Value::String("tool".to_string()));
    for key in [
        "type",
        "tool_name",
        "tool_call_id",
        "outcome",
        "source",
        "display",
        "result",
        "metadata",
    ] {
        if let Some(field) = value.get(key) {
            object.insert(key.to_string(), field.clone());
        }
    }
    if let Some(args) = value
        .get("args")
        .or_else(|| value.get("arguments"))
        .cloned()
        .or_else(|| {
            value
                .get("arguments_json")
                .and_then(Value::as_str)
                .and_then(|raw| serde_json::from_str(raw).ok())
        })
    {
        object.insert("args".to_string(), args);
    }
    if !object.contains_key("outcome") {
        object.insert("outcome".to_string(), Value::String("normal".to_string()));
    }
    Value::Object(object)
}

fn tool_name_from_value(value: &Value) -> &str {
    value
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("tool")
}

fn explicit_tool_call_id_from_value(value: &Value) -> Option<&str> {
    value
        .get("tool_call_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|tool_call_id| !tool_call_id.is_empty())
}

fn tool_args_from_value(value: &Value) -> Option<Value> {
    value
        .get("args")
        .or_else(|| value.get("arguments"))
        .cloned()
        .or_else(|| {
            value
                .get("arguments_json")
                .and_then(Value::as_str)
                .and_then(|raw| serde_json::from_str(raw).ok())
        })
}

fn exec_session_id_from_args_value(value: &Value) -> Option<u64> {
    value.get("session_id").and_then(Value::as_u64)
}

fn exec_session_id_from_result_value(value: &Value) -> Option<u64> {
    value
        .get("result")
        .and_then(|result| result.get("session_id"))
        .and_then(Value::as_u64)
}

fn exec_result_running_value(value: &Value) -> bool {
    exec_session_id_from_result_value(value).is_some()
        && value
            .get("result")
            .and_then(|result| result.get("exit_code"))
            .is_none_or(Value::is_null)
}

fn exec_result_completed_value(value: &Value) -> bool {
    value
        .get("result")
        .and_then(|result| result.get("exit_code"))
        .is_some_and(|exit_code| !exit_code.is_null())
}

fn tool_event_failed(value: &Value) -> bool {
    value
        .get("outcome")
        .and_then(Value::as_str)
        .is_some_and(|outcome| outcome != "normal")
}

fn background_running_agent_result_value(tool_name: &str, value: &Value) -> bool {
    tool_name == "spawn_agent"
        && value.get("type").and_then(Value::as_str) != Some("agent_session_start")
        && value
            .get("result")
            .and_then(|result| result.get("status"))
            .and_then(Value::as_str)
            == Some("running")
}

fn tool_result_output_runtime(value: &Value) -> String {
    value
        .get("result")
        .and_then(|result| result.get("output"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn tool_result_output_value(value: &Value) -> String {
    value
        .get("result")
        .and_then(|result| result.get("output"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn merge_output(existing: &mut String, next: &str) {
    if next.is_empty() || existing.ends_with(next) {
        return;
    }
    if next.starts_with(existing.as_str()) {
        *existing = next.to_string();
    } else {
        existing.push_str(next);
    }
}

fn set_metadata_field(metadata: &mut Value, key: &str, value: Value) {
    if let Some(object) = metadata.as_object_mut() {
        object.insert(key.to_string(), value);
    }
}

fn set_metadata_result_field(metadata: &mut Value, key: &str, value: Value) {
    let Some(object) = metadata.as_object_mut() else {
        return;
    };
    let result = object
        .entry("result".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !result.is_object() {
        *result = Value::Object(serde_json::Map::new());
    }
    if let Some(result) = result.as_object_mut() {
        result.insert(key.to_string(), value);
    }
}

fn result_body_from_metadata(metadata: &Value) -> Option<String> {
    metadata
        .get("result")
        .and_then(|result| result.get("output"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            metadata
                .get("result")
                .and_then(|result| serde_json::to_string(result).ok())
        })
}

fn runtime_value_metadata(value: &Value) -> Value {
    let mut object = value.as_object().cloned().unwrap_or_default();
    object.insert(
        "projection".to_string(),
        Value::String("runtimeValue".to_string()),
    );
    Value::Object(object)
}

fn selected_skills_from_value(value: &Value) -> Vec<GatewaySelectedSkill> {
    value
        .get("selected_skills")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|skill| {
            Some(GatewaySelectedSkill {
                name: skill.get("name")?.as_str()?.to_string(),
                path: skill
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect()
}

fn tool_kind(tool_name: &str) -> TranscriptBlockKind {
    match tool_name {
        "exec_command" | "write_stdin" => TranscriptBlockKind::Shell,
        "read" | "write" | "edit" | "apply_patch" => TranscriptBlockKind::File,
        "web_fetch" | "web_search" => TranscriptBlockKind::Web,
        "mcp" | "mcp_call" => TranscriptBlockKind::Mcp,
        "clarify" => TranscriptBlockKind::Clarify,
        "spawn_agent" => TranscriptBlockKind::Agent,
        "image_generate" | "image_generation.generate" | "image_generation__generate" => {
            TranscriptBlockKind::Artifact
        }
        _ => TranscriptBlockKind::ToolCall,
    }
}

fn generated_image_artifact_id(metadata: &Value) -> Option<String> {
    let result = metadata.get("result").unwrap_or(metadata);
    let media_kind = result
        .get("mediaKind")
        .or_else(|| result.get("media_kind"))
        .and_then(Value::as_str)?;
    if media_kind != "generated_image" {
        return None;
    }
    result
        .get("artifactId")
        .or_else(|| result.get("artifact_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|artifact_id| !artifact_id.is_empty())
        .map(str::to_string)
}

fn generated_image_body_from_metadata(metadata: &Value) -> Option<String> {
    let result = metadata.get("result").unwrap_or(metadata);
    generated_image_artifact_id(metadata)?;
    let mut lines = vec!["Generated image".to_string()];
    if let Some(prompt) = result
        .get("prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
    {
        lines.push(format!("Prompt: {}", compact_text(prompt, 180)));
    }
    if let Some(saved_path) = result
        .get("savedPath")
        .or_else(|| result.get("saved_path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        lines.push(format!("Saved: {saved_path}"));
    }
    Some(lines.join("\n"))
}

fn json_preview(value: &Value) -> Option<String> {
    serde_json::to_string(value).ok()
}

fn message_text(message: Option<&Value>) -> Option<String> {
    let text = message?
        .get("content")?
        .as_array()?
        .iter()
        .filter_map(|block| {
            (block.get("type").and_then(Value::as_str) == Some("text"))
                .then(|| block.get("text").and_then(Value::as_str))
                .flatten()
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

fn compact_text(text: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index >= max_chars {
            output.push_str("...");
            return output;
        }
        output.push(ch);
    }
    output
}
