use serde_json::Value;

use super::OUTPUT_LIMIT;
use super::types::{HookEventName, HookPermissionDecision, HookRunEntry};

pub(crate) fn parse_output_entries(
    event: HookEventName,
    stdout: &str,
    diagnostics: &mut Vec<String>,
) -> Vec<HookRunEntry> {
    let Some(value) = parse_hook_output(stdout) else {
        let text = stdout.trim();
        if text.is_empty() {
            return Vec::new();
        }
        let kind = match event {
            HookEventName::PreCompact => "compaction_guidance",
            HookEventName::PostToolUse => "feedback",
            _ => "feedback",
        };
        return vec![HookRunEntry {
            kind: kind.to_string(),
            message: text.to_string(),
        }];
    };
    let mut entries = Vec::new();
    for (field, kind) in [
        ("systemMessage", "context"),
        ("context", "context"),
        ("feedback", "feedback"),
        ("compactionGuidance", "compaction_guidance"),
        ("modelContent", "model_content"),
        ("model_content", "model_content"),
        ("stopReason", "stop"),
    ] {
        if let Some(entry) = value.get(field) {
            push_entry_values(&mut entries, kind, entry);
        }
    }
    if let Some(object) = value.as_object() {
        for key in object.keys() {
            if !matches!(
                key.as_str(),
                "continue"
                    | "stopReason"
                    | "systemMessage"
                    | "suppressOutput"
                    | "updatedInput"
                    | "decision"
                    | "context"
                    | "feedback"
                    | "compactionGuidance"
                    | "modelContent"
                    | "model_content"
            ) {
                diagnostics.push(format!("unsupported hook output field `{key}`"));
            }
        }
    }
    entries
}

fn push_entry_values(entries: &mut Vec<HookRunEntry>, kind: &str, value: &Value) {
    match value {
        Value::String(message) => entries.push(HookRunEntry {
            kind: kind.to_string(),
            message: message.clone(),
        }),
        Value::Array(items) => {
            for item in items {
                push_entry_values(entries, kind, item);
            }
        }
        other => entries.push(HookRunEntry {
            kind: kind.to_string(),
            message: other.to_string(),
        }),
    }
}

pub(crate) fn parse_hook_output(stdout: &str) -> Option<Value> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str(trimmed).ok()
}

pub(crate) fn parse_permission_decision(stdout: &str) -> Option<HookPermissionDecision> {
    let value = parse_hook_output(stdout)?;
    match value.get("decision").and_then(Value::as_str) {
        Some("allow") | Some("Allow") => Some(HookPermissionDecision::Allow),
        Some("deny") | Some("Deny") => Some(HookPermissionDecision::Deny),
        _ => None,
    }
}

pub(crate) fn parse_updated_input(stdout: &str) -> Option<Value> {
    parse_hook_output(stdout)?.get("updatedInput").cloned()
}

pub(crate) fn block_reason(stdout: &str, stderr: &str) -> String {
    if let Some(value) = parse_hook_output(stdout)
        && let Some(reason) = value.get("stopReason").and_then(Value::as_str)
        && !reason.trim().is_empty()
    {
        return reason.trim().to_string();
    }
    if !stderr.trim().is_empty() {
        stderr.trim().to_string()
    } else if !stdout.trim().is_empty() {
        stdout.trim().to_string()
    } else {
        "hook blocked the current event".to_string()
    }
}

pub(crate) fn bounded_output(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    if text.len() <= OUTPUT_LIMIT {
        text.trim().to_string()
    } else {
        let boundary = floor_char_boundary(&text, OUTPUT_LIMIT);
        format!("{}...[truncated]", &text[..boundary])
    }
}

fn floor_char_boundary(text: &str, limit: usize) -> usize {
    if limit >= text.len() {
        return text.len();
    }
    if text.is_char_boundary(limit) {
        return limit;
    }
    text.char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index < limit)
        .last()
        .unwrap_or(0)
}
