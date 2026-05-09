#[derive(Debug, Deserialize)]
struct ChatCompletionChunk {
    id: Option<String>,
    model: Option<String>,
    #[serde(default)]
    choices: Vec<ChatChoice>,
    usage: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    delta: ChatDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatDelta {
    content: Option<String>,
    reasoning: Option<String>,
    reasoning_content: Option<String>,
    reasoning_details: Option<Value>,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    tool_calls: Vec<ChatDeltaToolCall>,
}

fn null_as_empty_vec<'de, D, T>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Deserialize)]
struct ChatDeltaToolCall {
    index: usize,
    id: Option<String>,
    function: Option<ChatDeltaFunction>,
}

#[derive(Debug, Deserialize)]
struct ChatDeltaFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Debug)]
struct ChatChunkNormalizer {
    model: String,
    tool_calls: BTreeMap<usize, NormalizedToolCallState>,
    finish_reason: Option<String>,
}

#[derive(Debug, Default)]
struct NormalizedToolCallState {
    id: String,
    name: String,
    started: bool,
    ended: bool,
}

impl ChatChunkNormalizer {
    fn new(model: String) -> Self {
        Self {
            model,
            tool_calls: BTreeMap::new(),
            finish_reason: None,
        }
    }

    fn ingest(&mut self, chunk: ChatCompletionChunk) -> Result<Vec<StreamEvent>> {
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

    fn finish(&mut self) -> Vec<StreamEvent> {
        let mut output = self.end_started_tool_calls();
        output.push(StreamEvent::Done {
            outcome: Outcome::Normal,
            finish_reason: self.finish_reason.clone(),
        });
        output
    }

    fn end_started_tool_calls(&mut self) -> Vec<StreamEvent> {
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

