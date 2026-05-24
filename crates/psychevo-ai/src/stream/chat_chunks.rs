#[allow(unused_imports)]
pub(crate) use super::*;
#[derive(Debug, Deserialize)]
pub(crate) struct ChatCompletionChunk {
    pub(crate) id: Option<String>,
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) choices: Vec<ChatChoice>,
    pub(crate) usage: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatChoice {
    pub(crate) delta: ChatDelta,
    pub(crate) finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatDelta {
    pub(crate) content: Option<String>,
    pub(crate) reasoning: Option<String>,
    pub(crate) reasoning_content: Option<String>,
    pub(crate) reasoning_details: Option<Value>,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub(crate) tool_calls: Vec<ChatDeltaToolCall>,
}

pub(crate) fn null_as_empty_vec<'de, D, T>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatDeltaToolCall {
    pub(crate) index: usize,
    pub(crate) id: Option<String>,
    pub(crate) function: Option<ChatDeltaFunction>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatDeltaFunction {
    pub(crate) name: Option<String>,
    pub(crate) arguments: Option<String>,
}

#[derive(Debug)]
pub(crate) struct ChatChunkNormalizer {
    pub(crate) model: String,
    pub(crate) tool_calls: BTreeMap<usize, NormalizedToolCallState>,
    pub(crate) finish_reason: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct NormalizedToolCallState {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) started: bool,
    pub(crate) ended: bool,
}

impl ChatChunkNormalizer {
    pub(crate) fn new(model: String) -> Self {
        Self {
            model,
            tool_calls: BTreeMap::new(),
            finish_reason: None,
        }
    }

    pub(crate) fn ingest(&mut self, chunk: ChatCompletionChunk) -> Result<Vec<StreamEvent>> {
        let mut output = Vec::new();
        if let Some(usage) = chunk.usage {
            output.push(StreamEvent::Usage { usage });
        }
        if let Some(id) = chunk.id {
            output.push(StreamEvent::Metadata {
                metadata: json!({ "provider_response_id": id, "model": chunk.model.unwrap_or_else(|| self.model.clone()) }),
            });
        }

        for choice in chunk.choices {
            if let Some(reasoning_content) = choice
                .delta
                .reasoning_content
                .filter(|value| !value.is_empty())
            {
                output.push(StreamEvent::ReasoningDelta {
                    text: reasoning_content.clone(),
                    reasoning_content: Some(reasoning_content),
                });
            }
            if let Some(reasoning) = choice.delta.reasoning.filter(|value| !value.is_empty()) {
                output.push(StreamEvent::ReasoningDelta {
                    text: reasoning,
                    reasoning_content: None,
                });
            }
            if let Some(details) = choice
                .delta
                .reasoning_details
                .filter(|value| !value.is_null())
            {
                output.push(StreamEvent::ReasoningDetails { details });
            }
            if let Some(text) = choice.delta.content.filter(|value| !value.is_empty()) {
                output.push(StreamEvent::TextDelta { text });
            }
            for call in choice.delta.tool_calls {
                let state = self.tool_calls.entry(call.index).or_default();
                if let Some(id) = call.id.filter(|value| !value.is_empty()) {
                    state.id = id;
                }
                if let Some(function) = call.function {
                    if let Some(name) = function.name.filter(|value| !value.is_empty()) {
                        state.name = name;
                    }
                    if !state.started && !state.id.is_empty() && !state.name.is_empty() {
                        state.started = true;
                        output.push(StreamEvent::ToolCallStart {
                            content_index: call.index,
                            call_index: call.index,
                            id: state.id.clone(),
                            name: state.name.clone(),
                        });
                    }
                    if let Some(arguments_delta) =
                        function.arguments.filter(|value| !value.is_empty())
                    {
                        output.push(StreamEvent::ToolCallDelta {
                            content_index: call.index,
                            call_index: call.index,
                            id: (!state.id.is_empty()).then_some(state.id.clone()),
                            name: (!state.name.is_empty()).then_some(state.name.clone()),
                            arguments_delta,
                        });
                    }
                }
            }
            if let Some(reason) = choice.finish_reason {
                if reason == "tool_calls" {
                    output.extend(self.end_started_tool_calls());
                }
                self.finish_reason = Some(reason);
            }
        }
        Ok(output)
    }

    pub(crate) fn finish(&mut self) -> Vec<StreamEvent> {
        let mut output = self.end_started_tool_calls();
        output.push(StreamEvent::Done {
            outcome: Outcome::Normal,
            finish_reason: self.finish_reason.clone(),
        });
        output
    }

    pub(crate) fn end_started_tool_calls(&mut self) -> Vec<StreamEvent> {
        let mut output = Vec::new();
        for (index, state) in &mut self.tool_calls {
            if state.started && !state.ended {
                state.ended = true;
                output.push(StreamEvent::ToolCallEnd {
                    content_index: *index,
                    call_index: *index,
                });
            }
        }
        output
    }
}
