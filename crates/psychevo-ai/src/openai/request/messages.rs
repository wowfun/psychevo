use serde_json::{Value, json};

use super::{capability_is_true, model_capabilities};
use crate::types::ModelTarget;

pub(crate) fn assistant_messages(
    message: &Value,
    target: &ModelTarget,
    metadata: &Value,
    base_url: &str,
) -> Vec<Value> {
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    let mut normalized_reasoning = Vec::new();
    if let Some(blocks) = message.get("content").and_then(Value::as_array) {
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(value) = block.get("text").and_then(Value::as_str) {
                        text.push_str(value);
                    }
                }
                Some("tool_call") => {
                    let id = block.get("id").and_then(Value::as_str).unwrap_or_default();
                    let name = block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let arguments = block
                        .get("arguments_json")
                        .and_then(Value::as_str)
                        .unwrap_or("{}");
                    if !id.is_empty() && !name.is_empty() {
                        tool_calls.push(json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": arguments,
                            }
                        }));
                    }
                }
                Some("reasoning") => {
                    if let Some(value) = block.get("text").and_then(Value::as_str)
                        && !value.is_empty()
                    {
                        normalized_reasoning.push(value.to_string());
                    }
                }
                _ => {}
            }
        }
    }
    if text.is_empty() && tool_calls.is_empty() {
        return Vec::new();
    }
    let has_text = !text.is_empty();
    let mut output = json!({
        "role": "assistant",
        "content": has_text.then_some(text),
    });
    if !tool_calls.is_empty() {
        output["tool_calls"] = Value::Array(tool_calls);
    }
    let has_tool_calls = output
        .get("tool_calls")
        .and_then(Value::as_array)
        .is_some_and(|calls| !calls.is_empty());
    apply_reasoning_content_for_api(
        &mut output,
        has_text,
        has_tool_calls,
        &normalized_reasoning.join("\n\n"),
        target,
        metadata,
        base_url,
    );
    vec![output]
}

pub(crate) fn merge_adjacent_user_messages(messages: Vec<Value>) -> Vec<Value> {
    let mut merged: Vec<Value> = Vec::new();
    for message in messages {
        let is_user = message.get("role").and_then(Value::as_str) == Some("user");
        if is_user
            && let Some(last) = merged.last_mut()
            && last.get("role").and_then(Value::as_str) == Some("user")
            && let Some(previous) = last.get("content").and_then(Value::as_str)
            && let Some(current) = message.get("content").and_then(Value::as_str)
        {
            let previous = previous.to_string();
            last["content"] = Value::String(format!("{previous}\n\n{current}"));
            continue;
        }
        merged.push(message);
    }
    merged
}

pub(crate) fn apply_reasoning_content_for_api(
    output: &mut Value,
    has_text: bool,
    has_tool_calls: bool,
    normalized_reasoning: &str,
    target: &ModelTarget,
    metadata: &Value,
    base_url: &str,
) {
    if !projects_reasoning_content(target, metadata, base_url) {
        return;
    }
    if !has_text && !has_tool_calls {
        return;
    }
    let value = if normalized_reasoning.trim().is_empty() {
        " ".to_string()
    } else {
        normalized_reasoning.to_string()
    };
    output["reasoning_content"] = Value::String(value);
}

pub(crate) fn projects_reasoning_content(
    target: &ModelTarget,
    metadata: &Value,
    base_url: &str,
) -> bool {
    if model_interleaved_is_false(metadata) {
        return false;
    }
    if let Some(field) = model_interleaved_field(metadata) {
        return field == "reasoning_content";
    }
    capability_is_true(metadata, "reasoning")
        || needs_thinking_reasoning_pad_fallback(target, base_url)
}

pub(crate) fn model_interleaved_field(metadata: &Value) -> Option<&str> {
    model_capabilities(metadata)
        .and_then(|capabilities| capabilities.get("interleaved"))
        .and_then(|interleaved| interleaved.get("field"))
        .and_then(Value::as_str)
}

pub(crate) fn model_interleaved_is_false(metadata: &Value) -> bool {
    model_capabilities(metadata)
        .and_then(|capabilities| capabilities.get("interleaved"))
        .and_then(Value::as_bool)
        == Some(false)
}

pub(crate) fn needs_thinking_reasoning_pad_fallback(target: &ModelTarget, base_url: &str) -> bool {
    let provider = target.provider.to_lowercase();
    let model = target.model.to_lowercase();
    provider == "deepseek"
        || model.contains("deepseek")
        || base_url_host_matches(base_url, "api.deepseek.com")
        || provider == "kimi-coding"
        || provider == "kimi-coding-cn"
        || base_url_host_matches(base_url, "api.kimi.com")
        || base_url_host_matches(base_url, "moonshot.ai")
        || base_url_host_matches(base_url, "moonshot.cn")
        || provider == "xiaomi"
        || provider == "xiaomi-token-plan"
        || provider == "xiaomi-token-plan-cn"
        || model.contains("mimo")
        || base_url_host_matches(base_url, "api.xiaomimimo.com")
}

pub(crate) fn base_url_host_matches(base_url: &str, needle: &str) -> bool {
    let lower = base_url.to_lowercase();
    lower
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(lower.as_str())
        .split('/')
        .next()
        .unwrap_or_default()
        .ends_with(needle)
}

pub(crate) fn tool_result_messages(message: &Value) -> Vec<Value> {
    let tool_call_id = message
        .get("tool_call_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if tool_call_id.is_empty() {
        return Vec::new();
    }
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    vec![json!({
        "role": "tool",
        "tool_call_id": tool_call_id,
        "content": content,
    })]
}
