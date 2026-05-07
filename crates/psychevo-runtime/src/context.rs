use std::collections::HashSet;

use psychevo_agent_core::{AssistantBlock, Message};

pub fn prune_context(messages: Vec<Message>, max_context_messages: Option<usize>) -> Vec<Message> {
    let Some(max) = max_context_messages else {
        return messages;
    };
    if messages.len() <= max {
        return messages;
    }
    let mut start = messages.len().saturating_sub(max);
    loop {
        let retained = &messages[start..];
        let missing = missing_tool_call_ids(retained);
        if missing.is_empty() || start == 0 {
            break;
        }
        let mut new_start = start;
        for idx in (0..start).rev() {
            if assistant_contains_any_tool_id(&messages[idx], &missing) {
                new_start = idx;
            }
        }
        if new_start == start {
            break;
        }
        start = new_start;
    }
    messages[start..].to_vec()
}

fn missing_tool_call_ids(messages: &[Message]) -> HashSet<String> {
    let mut calls = HashSet::new();
    let mut results = HashSet::new();
    for message in messages {
        match message {
            Message::Assistant { content, .. } => {
                for block in content {
                    if let AssistantBlock::ToolCall(call) = block {
                        calls.insert(call.id.clone());
                    }
                }
            }
            Message::ToolResult { tool_call_id, .. } => {
                results.insert(tool_call_id.clone());
            }
            Message::User { .. } => {}
        }
    }
    results.difference(&calls).cloned().collect()
}

fn assistant_contains_any_tool_id(message: &Message, ids: &HashSet<String>) -> bool {
    let Message::Assistant { content, .. } = message else {
        return false;
    };
    content.iter().any(|block| match block {
        AssistantBlock::ToolCall(call) => ids.contains(&call.id),
        _ => false,
    })
}
