use std::collections::BTreeSet;

use serde_json::Value;

use super::DEFAULT_COMMAND_TIMEOUT_SECS;
use super::identity::canonicalize_value;
use super::types::{HookEventName, HookHandler, HookHandlerType, HookMatcherGroup};

pub(crate) fn normalize_hook_declarations(
    value: &Value,
) -> Vec<(HookEventName, Vec<HookMatcherGroup>)> {
    let Some(map) = value.as_object() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (event_name, value) in map {
        let Some(event) = HookEventName::parse(event_name) else {
            continue;
        };
        let groups = parse_matcher_groups(value);
        if !groups.is_empty() {
            out.push((event, groups));
        }
    }
    out.sort_by_key(|(event, _)| event.as_str().to_string());
    out
}

fn parse_matcher_groups(value: &Value) -> Vec<HookMatcherGroup> {
    match value {
        Value::Array(items) => items.iter().flat_map(parse_matcher_groups).collect(),
        Value::Object(map) if map.contains_key("hooks") => {
            let hooks = map.get("hooks").map(parse_handlers).unwrap_or_default();
            if hooks.is_empty() {
                Vec::new()
            } else {
                vec![HookMatcherGroup {
                    matcher: map
                        .get("matcher")
                        .or_else(|| map.get("match"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    hooks,
                }]
            }
        }
        Value::String(_) | Value::Object(_) => {
            let hooks = parse_handlers(value);
            if hooks.is_empty() {
                Vec::new()
            } else {
                vec![HookMatcherGroup {
                    matcher: None,
                    hooks,
                }]
            }
        }
        _ => Vec::new(),
    }
}

fn parse_handlers(value: &Value) -> Vec<HookHandler> {
    match value {
        Value::String(command) => vec![command_handler(command, value)],
        Value::Array(items) => items.iter().flat_map(parse_handlers).collect(),
        Value::Object(map) => {
            let has_command = map.get("command").and_then(Value::as_str).is_some();
            let handler_type =
                HookHandlerType::parse(map.get("type").and_then(Value::as_str), has_command);
            let timeout_secs = map
                .get("timeout")
                .or_else(|| map.get("timeout_secs"))
                .or_else(|| map.get("timeoutSeconds"))
                .and_then(Value::as_u64)
                .unwrap_or(DEFAULT_COMMAND_TIMEOUT_SECS)
                .max(1);
            vec![HookHandler {
                handler_type,
                command: map
                    .get("command")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                timeout_secs,
                status_message: map
                    .get("statusMessage")
                    .or_else(|| map.get("status_message"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                prompt: map
                    .get("prompt")
                    .or_else(|| map.get("text"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                agent: map
                    .get("agent")
                    .or_else(|| map.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                raw: canonicalize_value(value),
            }]
        }
        _ => Vec::new(),
    }
}

fn command_handler(command: &str, raw: &Value) -> HookHandler {
    HookHandler {
        handler_type: HookHandlerType::Command,
        command: Some(command.to_string()),
        timeout_secs: DEFAULT_COMMAND_TIMEOUT_SECS,
        status_message: None,
        prompt: None,
        agent: None,
        raw: canonicalize_value(raw),
    }
}

pub(crate) fn matcher_matches(
    event: HookEventName,
    matcher: Option<&str>,
    payload: &Value,
) -> bool {
    let matcher = matcher.map(str::trim).filter(|value| !value.is_empty());
    let Some(matcher) = matcher else {
        return true;
    };
    if matcher == "*" {
        return true;
    }
    let Some(value) = event.matcher_value(payload) else {
        return true;
    };
    tool_match_values(&value).contains(matcher)
}

fn tool_match_values(value: &str) -> BTreeSet<&str> {
    let mut values = BTreeSet::new();
    values.insert(value);
    match value {
        "exec_command" | "bash" | "shell" => {
            values.insert("Bash");
            values.insert("Shell");
        }
        "write" => {
            values.insert("Write");
        }
        "edit" => {
            values.insert("Edit");
        }
        "apply_patch" => {
            values.insert("Edit");
            values.insert("Write");
        }
        _ => {}
    }
    values
}
