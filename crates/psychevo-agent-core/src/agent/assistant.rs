#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) struct AssistantBuildState<'a> {
    pub(crate) text: &'a str,
    pub(crate) reasoning: &'a str,
    pub(crate) reasoning_provider_evidence: Option<Value>,
    pub(crate) tool_builders: &'a BTreeMap<(usize, usize), ToolCallBuilder>,
    pub(crate) provider_tools: &'a BTreeMap<String, ProviderToolBlock>,
    pub(crate) sources: &'a [AssistantSource],
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
    content.extend(
        state
            .provider_tools
            .values()
            .cloned()
            .map(AssistantBlock::ProviderTool),
    );
    content.extend(state.sources.iter().cloned().map(AssistantBlock::Source));
    Message::Assistant {
        content,
        timestamp_ms: state.timestamp_ms,
        finish_reason: state.finish_reason,
        outcome: state.outcome,
        model: Some(request.model.clone()),
        provider: Some(request.model_provider.clone()),
    }
}

pub(crate) struct InlineThinkParser {
    pending: String,
    open_thought: String,
    visible: String,
    reasoning: String,
    inside_thought: bool,
}

impl InlineThinkParser {
    pub(crate) fn new() -> Self {
        Self {
            pending: String::new(),
            open_thought: String::new(),
            visible: String::new(),
            reasoning: String::new(),
            inside_thought: false,
        }
    }

    pub(crate) fn push(&mut self, delta: &str) -> (String, String) {
        const OPEN: &str = "<think>";
        const CLOSE: &str = "</think>";
        self.pending.push_str(delta);
        let mut visible_delta = String::new();
        let mut reasoning_delta = String::new();
        loop {
            if self.inside_thought {
                if let Some(end) = self.pending.find(CLOSE) {
                    self.open_thought.push_str(&self.pending[..end]);
                    self.pending.drain(..end + CLOSE.len());
                    let thought = self.open_thought.trim();
                    if !thought.is_empty() {
                        if !self.reasoning.is_empty() {
                            self.reasoning.push_str("\n\n");
                            reasoning_delta.push_str("\n\n");
                        }
                        self.reasoning.push_str(thought);
                        reasoning_delta.push_str(thought);
                    }
                    self.open_thought.clear();
                    self.inside_thought = false;
                    continue;
                }
                let retained = trailing_tag_prefix_len(&self.pending, CLOSE);
                let safe_len = self.pending.len() - retained;
                self.open_thought.push_str(&self.pending[..safe_len]);
                self.pending.drain(..safe_len);
                break;
            }

            if let Some(start) = self.pending.find(OPEN) {
                let text = self.pending[..start].to_string();
                self.visible.push_str(&text);
                visible_delta.push_str(&text);
                self.pending.drain(..start + OPEN.len());
                self.inside_thought = true;
                continue;
            }
            let retained = trailing_tag_prefix_len(&self.pending, OPEN);
            let safe_len = self.pending.len() - retained;
            let text = self.pending[..safe_len].to_string();
            self.visible.push_str(&text);
            visible_delta.push_str(&text);
            self.pending.drain(..safe_len);
            break;
        }
        (visible_delta, reasoning_delta)
    }

    pub(crate) fn finish(&mut self) -> (String, String) {
        let visible_delta = if self.inside_thought {
            let text = format!("<think>{}{}", self.open_thought, self.pending);
            self.visible.push_str(&text);
            text
        } else {
            let text = std::mem::take(&mut self.pending);
            self.visible.push_str(&text);
            text
        };
        self.pending.clear();
        self.open_thought.clear();
        self.inside_thought = false;
        (visible_delta, String::new())
    }

    pub(crate) fn visible(&self) -> &str {
        &self.visible
    }

    pub(crate) fn reasoning(&self) -> &str {
        &self.reasoning
    }
}

fn trailing_tag_prefix_len(value: &str, tag: &str) -> usize {
    (1..tag.len())
        .rev()
        .find(|length| value.ends_with(&tag[..*length]))
        .unwrap_or(0)
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
