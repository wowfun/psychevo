use std::time::Duration;

use psychevo_agent_core::{AssistantBlock, Message};
use serde_json::{Value, json};

pub(crate) fn assistant_text(message: &Message) -> Option<String> {
    let Message::Assistant { content, .. } = message else {
        return None;
    };
    let text = content
        .iter()
        .filter_map(|block| match block {
            AssistantBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() { None } else { Some(text) }
}

pub(crate) fn sanitize_message_for_output(message: &Message) -> Message {
    match message {
        Message::Assistant {
            content,
            timestamp_ms,
            finish_reason,
            outcome,
            model,
            provider,
            ..
        } => Message::Assistant {
            content: content
                .iter()
                .filter(|block| !matches!(block, AssistantBlock::Reasoning { .. }))
                .cloned()
                .collect(),
            timestamp_ms: *timestamp_ms,
            finish_reason: finish_reason.clone(),
            outcome: *outcome,
            model: model.clone(),
            provider: provider.clone(),
        },
        other => other.clone(),
    }
}

pub(crate) fn sanitize_message_for_tui_history(message: &Message) -> Message {
    match message {
        Message::Assistant {
            content,
            timestamp_ms,
            finish_reason,
            outcome,
            model,
            provider,
            ..
        } => Message::Assistant {
            content: content
                .iter()
                .filter_map(|block| match block {
                    AssistantBlock::Reasoning { text, .. } if !text.trim().is_empty() => {
                        Some(AssistantBlock::Reasoning {
                            text: text.clone(),
                            provider_evidence: None,
                        })
                    }
                    AssistantBlock::Reasoning { .. } => None,
                    other => Some(other.clone()),
                })
                .collect(),
            timestamp_ms: *timestamp_ms,
            finish_reason: finish_reason.clone(),
            outcome: *outcome,
            model: model.clone(),
            provider: provider.clone(),
        },
        other => other.clone(),
    }
}

pub(crate) fn add_elapsed_metadata(metadata: Option<Value>, elapsed: Duration) -> Option<Value> {
    add_elapsed_ms_metadata(
        metadata,
        elapsed.as_millis().min(u128::from(u64::MAX)) as u64,
    )
}

pub(crate) fn add_assistant_metadata(
    metadata: Option<Value>,
    elapsed: Duration,
    reasoning_effort: Option<&str>,
) -> Option<Value> {
    let mut object = match add_elapsed_metadata(metadata, elapsed) {
        Some(Value::Object(object)) => object,
        Some(other) => {
            let mut object = serde_json::Map::new();
            object.insert("provider_metadata".to_string(), other);
            object
        }
        None => serde_json::Map::new(),
    };
    if let Some(reasoning_effort) = reasoning_effort
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "none")
    {
        object.insert(
            "reasoning_effort".to_string(),
            json!(reasoning_effort.to_string()),
        );
    }
    Some(Value::Object(object))
}

pub(crate) fn add_elapsed_ms_metadata(metadata: Option<Value>, elapsed_ms: u64) -> Option<Value> {
    let mut object = match metadata {
        Some(Value::Object(object)) => object,
        Some(other) => {
            let mut object = serde_json::Map::new();
            object.insert("provider_metadata".to_string(), other);
            object
        }
        None => serde_json::Map::new(),
    };
    object.insert("elapsed_ms".to_string(), json!(elapsed_ms));
    Some(Value::Object(object))
}
