async fn emit(sink: &Arc<dyn EventSink>, event: AgentEvent) -> Result<()> {
    sink.emit(event)
        .await
        .map_err(|err| Error::EventSink(err.to_string()))
}

async fn stream_assistant(
    provider: Arc<dyn GenerationProvider>,
    request: &AgentLoopRequest,
    context: &[Message],
    sink: Arc<dyn EventSink>,
    abort: AbortSignal,
) -> Result<Message> {
    let mut messages = request
        .system_instructions
        .iter()
        .filter(|instruction| !instruction.trim().is_empty())
        .map(|instruction| json!({ "role": "system", "content": instruction }))
        .collect::<Vec<_>>();
    let contextual_insert_index = request
        .previous_messages
        .len()
        .saturating_add(request.context_messages.len())
        .min(context.len());
    messages.extend(
        context[..contextual_insert_index]
            .iter()
            .map(serde_json::to_value)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|err| Error::Agent(err.to_string()))?,
    );
    messages.extend(
        request
            .contextual_user_messages
            .iter()
            .filter(|message| !message.blocks.is_empty())
            .map(ContextualUserMessage::to_provider_value),
    );
    messages.extend(
        context[contextual_insert_index..]
            .iter()
            .map(serde_json::to_value)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|err| Error::Agent(err.to_string()))?,
    );
    let generation_request = GenerationRequest {
        model: psychevo_ai::ModelTarget {
            provider: request.model_provider.clone(),
            model: request.model.clone(),
        },
        messages,
        tools: request
            .tools
            .iter()
            .map(|tool| ToolDeclaration {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters(),
            })
            .collect(),
        metadata: request.generation_metadata.clone(),
    };

    let mut stream = provider.stream(generation_request, abort).await?;
    let mut raw_text = String::new();
    let mut provider_reasoning = String::new();
    let mut reasoning_details = Vec::new();
    let mut usage = None;
    let mut metadata = None;
    let mut emitted_inline_reasoning_len = 0usize;
    let mut tool_builders: BTreeMap<(usize, usize), ToolCallBuilder> = BTreeMap::new();
    let mut finish_reason = None;
    let mut outcome = Outcome::Normal;
    let timestamp_ms = now_ms();
    let mut assistant = Message::Assistant {
        content: Vec::new(),
        timestamp_ms,
        finish_reason: None,
        outcome,
        model: Some(request.model.clone()),
        provider: Some(request.model_provider.clone()),
    };
    let mut last_visible_assistant = assistant.clone();
    emit(
        &sink,
        AgentEvent::MessageStart {
            message: assistant.clone(),
        },
    )
    .await?;

    while let Some(event) = stream.next().await {
        let mut visible_changed = false;
        match event? {
            StreamEvent::TextDelta { text: delta } => {
                raw_text.push_str(&delta);
                let (_, inline_reasoning) = split_inline_think_blocks(&raw_text, true);
                if inline_reasoning.len() > emitted_inline_reasoning_len {
                    let delta = inline_reasoning[emitted_inline_reasoning_len..].to_string();
                    emitted_inline_reasoning_len = inline_reasoning.len();
                    if !delta.is_empty() {
                        emit(&sink, AgentEvent::ReasoningDelta { text: delta }).await?;
                    }
                }
                visible_changed = true;
            }
            StreamEvent::ReasoningDelta {
                text: delta,
                reasoning_content: _,
            } => {
                provider_reasoning.push_str(&delta);
                emit(&sink, AgentEvent::ReasoningDelta { text: delta }).await?;
            }
            StreamEvent::ReasoningDetails { details } => {
                collect_reasoning_details(&mut reasoning_details, details);
            }
            StreamEvent::ToolCallStart {
                content_index,
                call_index,
                id,
                name,
            } => {
                let pending_id = id.clone();
                let pending_name = name.clone();
                tool_builders.insert(
                    (content_index, call_index),
                    ToolCallBuilder {
                        id,
                        name,
                        arguments_json: String::new(),
                        content_index,
                        call_index,
                    },
                );
                emit(
                    &sink,
                    AgentEvent::ToolCallPending {
                        tool_call_id: pending_id,
                        tool_name: pending_name,
                        arguments_json: String::new(),
                        content_index,
                        call_index,
                    },
                )
                .await?;
                visible_changed = true;
            }
            StreamEvent::ToolCallDelta {
                content_index,
                call_index,
                id,
                name,
                arguments_delta,
            } => {
                let builder = tool_builders
                    .entry((content_index, call_index))
                    .or_insert_with(|| ToolCallBuilder {
                        id: String::new(),
                        name: String::new(),
                        arguments_json: String::new(),
                        content_index,
                        call_index,
                    });
                if let Some(id) = id {
                    builder.id = id;
                }
                if let Some(name) = name {
                    builder.name = name;
                }
                builder.arguments_json.push_str(&arguments_delta);
                if !builder.name.is_empty() {
                    emit(
                        &sink,
                        AgentEvent::ToolCallPending {
                            tool_call_id: builder.id.clone(),
                            tool_name: builder.name.clone(),
                            arguments_json: builder.arguments_json.clone(),
                            content_index: builder.content_index,
                            call_index: builder.call_index,
                        },
                    )
                    .await?;
                }
                visible_changed = true;
            }
            StreamEvent::ToolCallEnd { .. } => {}
            StreamEvent::Usage { usage: reported } => {
                merge_object(&mut usage, normalize_usage(&reported));
            }
            StreamEvent::Metadata { metadata: reported } => {
                merge_object(&mut metadata, allowlisted_provider_metadata(&reported));
            }
            StreamEvent::Done {
                outcome: done_outcome,
                finish_reason: done_reason,
            } => {
                outcome = done_outcome;
                finish_reason = done_reason;
                break;
            }
        }
        let (visible_text, inline_reasoning) = split_inline_think_blocks(&raw_text, true);
        let reasoning = combine_reasoning(&provider_reasoning, &inline_reasoning);
        assistant = build_assistant_message(
            AssistantBuildState {
                text: &visible_text,
                reasoning: &reasoning,
                reasoning_provider_evidence: reasoning_provider_evidence(&reasoning_details),
                tool_builders: &tool_builders,
                timestamp_ms,
                finish_reason: finish_reason.clone(),
                outcome,
            },
            request,
        );
        if visible_changed && visible_assistant_changed(&last_visible_assistant, &assistant) {
            last_visible_assistant = assistant.clone();
            emit(
                &sink,
                AgentEvent::MessageUpdate {
                    message: assistant.clone(),
                },
            )
            .await?;
        }
    }

    let (visible_text, inline_reasoning) = split_inline_think_blocks(&raw_text, false);
    if inline_reasoning.len() > emitted_inline_reasoning_len {
        let delta = inline_reasoning[emitted_inline_reasoning_len..].to_string();
        if !delta.is_empty() {
            emit(&sink, AgentEvent::ReasoningDelta { text: delta }).await?;
        }
    }
    let reasoning = combine_reasoning(&provider_reasoning, &inline_reasoning);
    assistant = build_assistant_message(
        AssistantBuildState {
            text: &visible_text,
            reasoning: &reasoning,
            reasoning_provider_evidence: reasoning_provider_evidence(&reasoning_details),
            tool_builders: &tool_builders,
            timestamp_ms,
            finish_reason,
            outcome,
        },
        request,
    );
    if visible_assistant_changed(&last_visible_assistant, &assistant) {
        emit(
            &sink,
            AgentEvent::MessageUpdate {
                message: assistant.clone(),
            },
        )
        .await?;
    }
    if !reasoning.is_empty() {
        emit(&sink, AgentEvent::ReasoningEnd { text: reasoning }).await?;
    }
    emit(
        &sink,
        AgentEvent::MessageEnd {
            message: assistant.clone(),
            usage,
            metadata,
        },
    )
    .await?;
    Ok(assistant)
}
