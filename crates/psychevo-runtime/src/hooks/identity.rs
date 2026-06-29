use std::collections::BTreeMap;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::types::{HookEventName, HookHandler, HookHandlerType};

pub(crate) fn hook_definition_hash(
    event: HookEventName,
    matcher: Option<&str>,
    handler: &HookHandler,
) -> String {
    sha256_hex(&json!({
        "event": event.as_str(),
        "matcher": matcher.unwrap_or("*"),
        "handler": normalized_handler_identity(handler),
    }))
}

pub(crate) fn hook_key(
    source_id: &str,
    event: HookEventName,
    matcher: Option<&str>,
    handler_type: HookHandlerType,
    declaration_index: usize,
) -> String {
    let hash = sha256_hex(&json!({
        "source": source_id,
        "event": event.as_str(),
        "matcher": matcher.unwrap_or("*"),
        "handler_type": handler_type.as_str(),
        "declaration_index": declaration_index,
    }));
    format!("hk_{}", &hash[..24])
}

fn normalized_handler_identity(handler: &HookHandler) -> Value {
    match handler.handler_type {
        HookHandlerType::Command => json!({
            "type": "command",
            "command": handler.command.as_deref().unwrap_or(""),
            "timeout": handler.timeout_secs,
        }),
        HookHandlerType::Worker => json!({
            "type": "worker",
            "raw": handler.raw,
            "timeout": handler.timeout_secs,
        }),
        HookHandlerType::Prompt => json!({
            "type": "prompt",
            "prompt": handler.prompt.as_deref().unwrap_or(""),
            "timeout": handler.timeout_secs,
        }),
        HookHandlerType::Agent => json!({
            "type": "agent",
            "agent": handler.agent.as_deref().unwrap_or(""),
            "timeout": handler.timeout_secs,
        }),
        HookHandlerType::Unsupported => json!({
            "type": "unsupported",
            "raw": handler.raw,
        }),
    }
}

fn sha256_hex(value: &Value) -> String {
    let canonical = canonicalize_value(value);
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub(crate) fn canonicalize_value(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonicalize_value).collect()),
        Value::Object(map) => {
            let mut sorted = BTreeMap::new();
            for (key, value) in map {
                sorted.insert(key.clone(), canonicalize_value(value));
            }
            Value::Object(sorted.into_iter().collect())
        }
        other => other.clone(),
    }
}
