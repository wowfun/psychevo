#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) struct AssistantBuildState<'a> {
    pub(crate) text: &'a str,
    pub(crate) reasoning: &'a str,
    pub(crate) reasoning_provider_evidence: Option<Value>,
    pub(crate) tool_builders: &'a BTreeMap<(usize, usize), ToolCallBuilder>,
    pub(crate) timestamp_ms: i64,
    pub(crate) finish_reason: Option<String>,
    pub(crate) outcome: Outcome,
}

pub(crate) fn build_assistant_message(
    state: AssistantBuildState<'_>,
    request: &AgentLoopRequest,
) -> Message {
    let mut content = Vec::new();
    if !state.reasoning.is_empty() || state.reasoning_provider_evidence.is_some() {
        content.push(AssistantBlock::Reasoning {
            text: state.reasoning.to_string(),
            provider_evidence: state.reasoning_provider_evidence,
        });
    }
    if !state.text.is_empty() {
        content.push(AssistantBlock::Text {
            text: state.text.to_string(),
        });
    }
    for builder in state.tool_builders.values() {
        let parsed = serde_json::from_str::<Value>(&builder.arguments_json);
        let (arguments, arguments_error) = match parsed {
            Ok(value) => (value, None),
            Err(err) => (Value::Null, Some(err.to_string())),
        };
        content.push(AssistantBlock::ToolCall(ToolCallBlock {
            id: builder.id.clone(),
            name: builder.name.clone(),
            arguments,
            arguments_json: builder.arguments_json.clone(),
            arguments_error,
            content_index: builder.content_index,
            call_index: builder.call_index,
        }));
    }
    Message::Assistant {
        content,
        timestamp_ms: state.timestamp_ms,
        finish_reason: state.finish_reason,
        outcome: state.outcome,
        model: Some(request.model.clone()),
        provider: Some(request.model_provider.clone()),
    }
}

pub(crate) fn split_inline_think_blocks(input: &str, streaming: bool) -> (String, String) {
    let mut visible = String::new();
    let mut reasoning = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative_start) = input[cursor..].find("<think>") {
        let start = cursor + relative_start;
        visible.push_str(&input[cursor..start]);
        let content_start = start + "<think>".len();
        if let Some(relative_end) = input[content_start..].find("</think>") {
            let end = content_start + relative_end;
            let thought = input[content_start..end].trim();
            if !thought.is_empty() {
                reasoning.push(thought.to_string());
            }
            cursor = end + "</think>".len();
        } else {
            if !streaming {
                visible.push_str(&input[start..]);
            }
            return (visible, reasoning.join("\n\n"));
        }
    }
    visible.push_str(&input[cursor..]);
    (visible, reasoning.join("\n\n"))
}

pub(crate) fn combine_reasoning(provider_reasoning: &str, inline_reasoning: &str) -> String {
    match (
        provider_reasoning.trim().is_empty(),
        inline_reasoning.trim().is_empty(),
    ) {
        (true, true) => String::new(),
        (false, true) => provider_reasoning.to_string(),
        (true, false) => inline_reasoning.to_string(),
        (false, false) => format!("{provider_reasoning}\n\n{inline_reasoning}"),
    }
}

pub(crate) fn collect_reasoning_details(details: &mut Vec<Value>, value: Value) {
    match value {
        Value::Array(values) => details.extend(values),
        other => details.push(other),
    }
}

pub(crate) fn merge_object(target: &mut Option<Value>, value: Option<Value>) {
    let Some(Value::Object(next)) = value else {
        return;
    };
    match target {
        Some(Value::Object(existing)) => {
            existing.extend(next);
        }
        _ => *target = Some(Value::Object(next)),
    }
}

pub(crate) fn reasoning_provider_evidence(details: &[Value]) -> Option<Value> {
    (!details.is_empty()).then(|| json!({ "reasoning_details": details }))
}

pub(crate) fn visible_assistant_changed(previous: &Message, current: &Message) -> bool {
    visible_assistant_blocks(previous) != visible_assistant_blocks(current)
}

pub(crate) fn visible_assistant_blocks(message: &Message) -> Vec<AssistantBlock> {
    let Message::Assistant { content, .. } = message else {
        return Vec::new();
    };
    content
        .iter()
        .filter(|block| !matches!(block, AssistantBlock::Reasoning { .. }))
        .cloned()
        .collect()
}
