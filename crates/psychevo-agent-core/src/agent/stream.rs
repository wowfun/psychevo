#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) async fn emit(sink: &Arc<dyn EventSink>, event: AgentEvent) -> Result<()> {
    sink.emit(event)
        .await
        .map_err(|err| Error::EventSink(err.to_string()))
}

pub(crate) fn display_spec_for_tool(router: &ToolRouter, name: &str) -> ToolDisplaySpec {
    router.display_spec(name)
}

pub(crate) async fn stream_assistant(
    model: LanguageModel,
    request: &AgentLoopRequest,
    tool_router: &ToolRouter,
    context: &[Message],
    sink: Arc<dyn EventSink>,
    mut abort: AbortSignal,
) -> Result<Message> {
    let generation_request = build_language_request(request, tool_router, context).await?;
    let generation_started_at_ms = now_ms();
    let generation_started = Instant::now();
    let generation_id = format!("generation:{generation_started_at_ms}:{}", context.len());
    emit(
        &sink,
        AgentEvent::GenerationStart {
            generation_id: generation_id.clone(),
            provider: request.model_provider.clone(),
            model: request.model.clone(),
            message_count: generation_request.messages.len(),
            tool_count: generation_request.tools.len(),
            started_at_ms: generation_started_at_ms,
        },
    )
    .await?;

    let mut generation = model.stream(generation_request);
    let mut caller_aborted = false;
    let mut inline_think = InlineThinkParser::new();
    let mut provider_reasoning = String::new();
    let mut reasoning_details = Vec::new();
    let mut usage = None;
    let mut metadata = None;
    let mut warnings = Vec::new();
    let mut tool_builders: BTreeMap<(usize, usize), ToolCallBuilder> = BTreeMap::new();
    let mut provider_tools: BTreeMap<String, ProviderToolBlock> = BTreeMap::new();
    let mut sources: Vec<AssistantSource> = Vec::new();
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

    let mut saw_finish = false;
    while !saw_finish {
        let next = tokio::select! {
            biased;
            _ = abort.wait_for_abort() => {
                caller_aborted = true;
                generation.abort();
                break;
            }
            event = generation.next_event() => event,
        };
        let Some(event) = next else {
            break;
        };
        let event = match event {
            Ok(event) => event,
            Err(err) => {
                emit_generation_failure(
                    &sink,
                    generation_id,
                    request,
                    generation_started,
                    usage,
                    metadata,
                    err.to_string(),
                )
                .await?;
                return Err(err.into());
            }
        };
        let mut visible_changed = false;
        match event {
            GenerationEvent::Started { .. }
            | GenerationEvent::TextStart { .. }
            | GenerationEvent::TextEnd { .. }
            | GenerationEvent::ReasoningStart { .. }
            | GenerationEvent::ReasoningEnd { .. } => {}
            GenerationEvent::TextDelta { delta, .. } => {
                let (visible_delta, reasoning_delta) = inline_think.push(&delta);
                if !visible_delta.is_empty() {
                    emit(
                        &sink,
                        AgentEvent::AssistantTextDelta {
                            text: visible_delta,
                        },
                    )
                    .await?;
                }
                if !reasoning_delta.is_empty() {
                    emit(
                        &sink,
                        AgentEvent::ReasoningDelta {
                            text: reasoning_delta,
                        },
                    )
                    .await?;
                }
            }
            GenerationEvent::ReasoningDelta {
                delta,
                provider_evidence,
                ..
            } => {
                provider_reasoning.push_str(&delta);
                if let Some(evidence) = provider_evidence {
                    collect_reasoning_details(&mut reasoning_details, evidence);
                }
                if !delta.is_empty() {
                    emit(&sink, AgentEvent::ReasoningDelta { text: delta }).await?;
                }
            }
            GenerationEvent::ToolCallStart {
                content_index,
                id,
                name,
            } => {
                let call_index = content_index;
                tool_builders.insert(
                    (content_index, call_index),
                    ToolCallBuilder {
                        id: id.clone(),
                        name: name.clone(),
                        arguments_json: String::new(),
                        argument_error: None,
                        content_index,
                        call_index,
                    },
                );
                emit(
                    &sink,
                    AgentEvent::ToolCallPending {
                        tool_call_id: id,
                        tool_name: name.clone(),
                        arguments_json: String::new(),
                        content_index,
                        call_index,
                        display: Some(display_spec_for_tool(tool_router, &name)),
                    },
                )
                .await?;
                visible_changed = true;
            }
            GenerationEvent::ToolCallArgumentsDelta {
                content_index,
                delta,
            } => {
                let call_index = content_index;
                let builder = tool_builders
                    .get_mut(&(content_index, call_index))
                    .ok_or_else(|| {
                        Error::Agent(format!(
                            "tool-call delta arrived before start at content index {content_index}"
                        ))
                    })?;
                builder.arguments_json.push_str(&delta);
                emit(
                    &sink,
                    AgentEvent::ToolCallPending {
                        tool_call_id: builder.id.clone(),
                        tool_name: builder.name.clone(),
                        arguments_json: builder.arguments_json.clone(),
                        content_index,
                        call_index,
                        display: Some(display_spec_for_tool(tool_router, &builder.name)),
                    },
                )
                .await?;
                visible_changed = true;
            }
            GenerationEvent::ToolCallEnd {
                content_index,
                arguments_raw,
                argument_error,
            } => {
                let call_index = content_index;
                let builder = tool_builders
                    .get_mut(&(content_index, call_index))
                    .ok_or_else(|| {
                        Error::Agent(format!(
                            "tool-call end arrived before start at content index {content_index}"
                        ))
                    })?;
                builder.arguments_json = arguments_raw;
                builder.argument_error = argument_error;
                emit(
                    &sink,
                    AgentEvent::ToolCallPending {
                        tool_call_id: builder.id.clone(),
                        tool_name: builder.name.clone(),
                        arguments_json: builder.arguments_json.clone(),
                        content_index,
                        call_index,
                        display: Some(display_spec_for_tool(tool_router, &builder.name)),
                    },
                )
                .await?;
                visible_changed = true;
            }
            GenerationEvent::ProviderToolStart { tool, .. }
            | GenerationEvent::ProviderToolEnd { tool, .. } => {
                provider_tools.insert(
                    tool.id.clone(),
                    ProviderToolBlock {
                        id: tool.id,
                        name: tool.name,
                        action: tool.action,
                        status: tool.status,
                    },
                );
                visible_changed = true;
            }
            GenerationEvent::Source { source, .. } => {
                if !sources.contains(&source) {
                    sources.push(source);
                }
                visible_changed = true;
            }
            GenerationEvent::Usage { usage: reported } => {
                usage = serde_json::to_value(reported).ok();
            }
            GenerationEvent::Metadata { metadata: reported } => {
                merge_object(
                    &mut metadata,
                    Some(Value::Object(reported.into_iter().collect())),
                );
            }
            GenerationEvent::Warning { warning } => {
                warnings.push(warning);
            }
            GenerationEvent::Resync {
                snapshot,
                dropped_events: _,
            } => {
                inline_think = InlineThinkParser::new();
                provider_reasoning.clear();
                reasoning_details.clear();
                tool_builders.clear();
                provider_tools.clear();
                sources.clear();
                for (content_index, content) in snapshot.assistant.content.iter().enumerate() {
                    match content {
                        psychevo_ai::AssistantContent::Text(text) => {
                            inline_think.push(&text.text);
                        }
                        psychevo_ai::AssistantContent::Reasoning {
                            text,
                            provider_evidence,
                        } => {
                            if !provider_reasoning.is_empty() && !text.is_empty() {
                                provider_reasoning.push_str("\n\n");
                            }
                            provider_reasoning.push_str(text);
                            if let Some(evidence) = provider_evidence.clone() {
                                collect_reasoning_details(&mut reasoning_details, evidence);
                            }
                        }
                        psychevo_ai::AssistantContent::ToolCall(call) => {
                            tool_builders.insert(
                                (content_index, content_index),
                                ToolCallBuilder {
                                    id: call.id.clone(),
                                    name: call.name.clone(),
                                    arguments_json: call.arguments_raw.clone(),
                                    argument_error: call.argument_error.clone(),
                                    content_index,
                                    call_index: content_index,
                                },
                            );
                        }
                        psychevo_ai::AssistantContent::ProviderTool(tool) => {
                            provider_tools.insert(
                                tool.id.clone(),
                                ProviderToolBlock {
                                    id: tool.id.clone(),
                                    name: tool.name.clone(),
                                    action: tool.action.clone(),
                                    status: tool.status.clone(),
                                },
                            );
                        }
                        psychevo_ai::AssistantContent::Source { source } => {
                            if !sources.contains(source) {
                                sources.push(source.clone());
                            }
                        }
                        psychevo_ai::AssistantContent::Extension { .. } => {}
                    }
                }
                usage = snapshot
                    .usage
                    .as_ref()
                    .and_then(|usage| serde_json::to_value(usage).ok());
                metadata = (!snapshot.provider_metadata.is_empty()).then(|| {
                    Value::Object(snapshot.provider_metadata.clone().into_iter().collect())
                });
                warnings = snapshot.warnings.clone();
                assistant = build_assistant_message_from_snapshot(
                    &snapshot.assistant,
                    request,
                    timestamp_ms,
                    finish_reason.clone(),
                    outcome,
                );
                if visible_assistant_changed(&last_visible_assistant, &assistant) {
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
            GenerationEvent::Finish {
                outcome: done_outcome,
                finish_reason: done_reason,
            } => {
                outcome = map_generation_outcome(done_outcome, done_reason.as_ref());
                finish_reason = done_reason
                    .as_ref()
                    .and_then(|reason| reason.raw.clone())
                    .or_else(|| done_reason.as_ref().map(normalized_finish_reason));
                saw_finish = true;
            }
        }
        if visible_changed {
            let reasoning = combine_reasoning(&provider_reasoning, inline_think.reasoning());
            assistant = build_assistant_message(
                AssistantBuildState {
                    text: inline_think.visible(),
                    reasoning: &reasoning,
                    reasoning_provider_evidence: reasoning_provider_evidence(&reasoning_details),
                    tool_builders: &tool_builders,
                    provider_tools: &provider_tools,
                    sources: &sources,
                    timestamp_ms,
                    finish_reason: finish_reason.clone(),
                    outcome,
                },
                request,
            );
            if visible_assistant_changed(&last_visible_assistant, &assistant) {
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
    }

    let output = match generation.finish().await {
        Ok(output) => output,
        Err(err) => {
            emit_generation_failure(
                &sink,
                generation_id,
                request,
                generation_started,
                usage,
                metadata,
                err.to_string(),
            )
            .await?;
            return Err(err.into());
        }
    };
    outcome = if caller_aborted {
        Outcome::Aborted
    } else {
        map_generation_outcome(output.outcome, output.finish_reason.as_ref())
    };
    finish_reason = if caller_aborted {
        Some("aborted".to_string())
    } else {
        output
            .finish_reason
            .as_ref()
            .and_then(|reason| reason.raw.clone())
            .or_else(|| output.finish_reason.as_ref().map(normalized_finish_reason))
    };
    usage = output
        .snapshot
        .usage
        .as_ref()
        .and_then(|usage| serde_json::to_value(usage).ok())
        .or(usage);
    if !output.snapshot.provider_metadata.is_empty() {
        merge_object(
            &mut metadata,
            Some(Value::Object(
                output.snapshot.provider_metadata.into_iter().collect(),
            )),
        );
    }
    warnings.extend(output.snapshot.warnings);
    if !warnings.is_empty()
        && let Ok(warnings) = serde_json::to_value(&warnings)
    {
        let mut warning_metadata = serde_json::Map::new();
        warning_metadata.insert("warnings".to_string(), warnings);
        merge_object(&mut metadata, Some(Value::Object(warning_metadata)));
    }

    let (visible_delta, reasoning_delta) = inline_think.finish();
    if !visible_delta.is_empty() {
        emit(
            &sink,
            AgentEvent::AssistantTextDelta {
                text: visible_delta,
            },
        )
        .await?;
    }
    if !reasoning_delta.is_empty() {
        emit(
            &sink,
            AgentEvent::ReasoningDelta {
                text: reasoning_delta,
            },
        )
        .await?;
    }
    let reasoning = combine_reasoning(&provider_reasoning, inline_think.reasoning());
    assistant = build_assistant_message_from_snapshot(
        &output.snapshot.assistant,
        request,
        timestamp_ms,
        finish_reason,
        outcome,
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
    emit(
        &sink,
        AgentEvent::GenerationEnd {
            generation_id,
            provider: request.model_provider.clone(),
            model: request.model.clone(),
            outcome,
            elapsed_ms: duration_ms_u64(generation_started.elapsed()),
            usage: usage.clone(),
            metadata: metadata.clone(),
            error: None,
        },
    )
    .await?;
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

async fn build_language_request(
    request: &AgentLoopRequest,
    tool_router: &ToolRouter,
    context: &[Message],
) -> Result<LanguageRequest> {
    let mut messages = request
        .prompt_instructions
        .iter()
        .filter(|instruction| !instruction.content.trim().is_empty())
        .map(prompt_instruction_to_ai)
        .collect::<Vec<_>>();
    messages.extend(
        request
            .prefix_contextual_user_messages
            .iter()
            .filter(|message| !message.blocks.is_empty())
            .map(contextual_user_message_to_ai),
    );
    let contextual_insert_index = request
        .previous_messages
        .len()
        .saturating_add(request.context_messages.len())
        .min(context.len());
    for message in &context[..contextual_insert_index] {
        messages.push(message_to_ai(message).await);
    }
    messages.extend(
        request
            .turn_prompt_instructions
            .iter()
            .filter(|instruction| !instruction.content.trim().is_empty())
            .map(prompt_instruction_to_ai),
    );
    messages.extend(
        request
            .turn_contextual_user_messages
            .iter()
            .filter(|message| !message.blocks.is_empty())
            .map(contextual_user_message_to_ai),
    );
    for message in &context[contextual_insert_index..] {
        messages.push(message_to_ai(message).await);
    }

    let mut tools = tool_router
        .declarations()
        .into_iter()
        .map(LanguageTool::from)
        .collect::<Vec<_>>();
    if let Some(config) = request.generation_metadata.get("hosted_web_search") {
        tools.push(LanguageTool::WebSearch(WebSearchTool {
            extensions: BTreeMap::from([("openai".to_string(), config.clone())]),
            ..WebSearchTool::default()
        }));
    }
    let settings = LanguageSettings {
        reasoning_effort: request
            .generation_metadata
            .get("reasoning_effort")
            .and_then(Value::as_str)
            .map(str::to_string),
        ..LanguageSettings::default()
    };
    Ok(LanguageRequest {
        messages,
        tools,
        settings,
        headers: BTreeMap::new(),
        extensions: BTreeMap::from([("psychevo".to_string(), request.generation_metadata.clone())]),
    })
}

pub fn prompt_instruction_to_ai(instruction: &PromptInstruction) -> psychevo_ai::Message {
    let metadata = json!({
        "prompt_slot": instruction.slot,
        "prompt_slot_tier": instruction.tier,
        "prompt_semantic_role": instruction.semantic_role,
        "prompt_content_hash": instruction.content_hash,
        "prompt_order": instruction.order,
        "source_kind": instruction.source_kind,
        "source_name": instruction.source_name,
        "source_path": instruction.source_path,
    });
    let extensions = BTreeMap::from([("psychevo".to_string(), metadata)]);
    let content = vec![psychevo_ai::TextContent::new(&instruction.content)];
    if instruction.provider_role == "developer" {
        psychevo_ai::Message::Developer {
            content,
            extensions,
        }
    } else {
        psychevo_ai::Message::System {
            content,
            extensions,
        }
    }
}

pub fn contextual_user_message_to_ai(message: &ContextualUserMessage) -> psychevo_ai::Message {
    let content = message
        .blocks
        .iter()
        .map(|block| {
            let mut text = psychevo_ai::TextContent::new(&block.text);
            text.extensions.insert(
                "psychevo".to_string(),
                json!({
                    "type": "contextual_text",
                    "context_kind": block.kind,
                    "source_name": block.source_name,
                    "source_path": block.source_path,
                    "hidden": block.hidden,
                }),
            );
            psychevo_ai::UserContent::Text(text)
        })
        .collect();
    psychevo_ai::Message::User {
        content,
        extensions: BTreeMap::from([(
            "psychevo".to_string(),
            json!({
                "contextual_user": true,
                "provider_group": message.provider_group,
                "context_category": message.context_category,
                "hidden": message.hidden,
                "timestamp_ms": message.timestamp_ms,
            }),
        )]),
    }
}

pub async fn message_to_ai(message: &Message) -> psychevo_ai::Message {
    match message {
        Message::User {
            content,
            timestamp_ms,
        } => {
            let mut output = Vec::new();
            for block in content {
                match block {
                    UserContentBlock::Text(block) => {
                        output.push(psychevo_ai::UserContent::Text(
                            psychevo_ai::TextContent::new(&block.text),
                        ));
                    }
                    UserContentBlock::LocalImage(block) => {
                        let mime_type = image_mime_type(&block.path);
                        match psychevo_ai::Media::from_file(&block.path, mime_type).await {
                            Ok(media) => output.push(psychevo_ai::UserContent::Image(
                                psychevo_ai::ImageContent {
                                    source: psychevo_ai::MediaInput::Inline { media },
                                    detail: None,
                                    extensions: BTreeMap::new(),
                                },
                            )),
                            Err(error) => output.push(psychevo_ai::UserContent::Text(
                                psychevo_ai::TextContent::new(format!(
                                    "Image at `{}` could not be attached: {error}",
                                    block.path.display()
                                )),
                            )),
                        }
                    }
                    UserContentBlock::ImageUrl(block) => {
                        output.push(psychevo_ai::UserContent::Image(psychevo_ai::ImageContent {
                            source: psychevo_ai::MediaInput::Url {
                                url: block.url.clone(),
                                mime_type: None,
                            },
                            detail: None,
                            extensions: BTreeMap::new(),
                        }));
                    }
                }
            }
            psychevo_ai::Message::User {
                content: output,
                extensions: BTreeMap::from([(
                    "psychevo".to_string(),
                    json!({"timestamp_ms": timestamp_ms}),
                )]),
            }
        }
        Message::Assistant {
            content,
            timestamp_ms,
            finish_reason,
            outcome,
            model,
            provider,
        } => {
            let content = content
                .iter()
                .map(|block| match block {
                    AssistantBlock::Text { text } => {
                        psychevo_ai::AssistantContent::Text(psychevo_ai::TextContent::new(text))
                    }
                    AssistantBlock::Reasoning {
                        text,
                        provider_evidence,
                    } => psychevo_ai::AssistantContent::Reasoning {
                        text: text.clone(),
                        provider_evidence: provider_evidence.clone(),
                    },
                    AssistantBlock::ToolCall(call) => {
                        psychevo_ai::AssistantContent::ToolCall(psychevo_ai::ToolCall {
                            id: call.id.clone(),
                            name: call.name.clone(),
                            arguments_raw: call.arguments_json.clone(),
                            arguments: call.arguments.is_object().then(|| call.arguments.clone()),
                            argument_error: call.arguments_error.clone(),
                            extensions: BTreeMap::new(),
                        })
                    }
                    AssistantBlock::ProviderTool(tool) => {
                        psychevo_ai::AssistantContent::ProviderTool(psychevo_ai::ProviderTool {
                            id: tool.id.clone(),
                            name: tool.name.clone(),
                            action: tool.action.clone(),
                            status: tool.status.clone(),
                            extensions: BTreeMap::new(),
                        })
                    }
                    AssistantBlock::Source(source) => psychevo_ai::AssistantContent::Source {
                        source: source.clone(),
                    },
                })
                .collect();
            psychevo_ai::Message::Assistant {
                message: psychevo_ai::AssistantMessage {
                    content,
                    extensions: BTreeMap::from([(
                        "psychevo".to_string(),
                        json!({
                            "timestamp_ms": timestamp_ms,
                            "finish_reason": finish_reason,
                            "outcome": outcome,
                            "model": model,
                            "provider": provider,
                        }),
                    )]),
                },
            }
        }
        Message::ToolResult {
            tool_call_id,
            tool_name,
            content,
            is_error,
            timestamp_ms,
        } => psychevo_ai::Message::Tool {
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            content: vec![psychevo_ai::TextContent::new(content)],
            is_error: *is_error,
            extensions: BTreeMap::from([(
                "psychevo".to_string(),
                json!({"timestamp_ms": timestamp_ms}),
            )]),
        },
    }
}

fn image_mime_type(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        _ => "image/png",
    }
}

fn map_generation_outcome(
    outcome: GenerationOutcome,
    reason: Option<&psychevo_ai::FinishReason>,
) -> Outcome {
    match outcome {
        GenerationOutcome::Aborted => Outcome::Aborted,
        GenerationOutcome::Completed
            if reason.is_some_and(|reason| {
                matches!(
                    reason.kind,
                    FinishReasonKind::Length | FinishReasonKind::ContentFilter
                )
            }) =>
        {
            Outcome::Stopped
        }
        GenerationOutcome::Completed => Outcome::Normal,
    }
}

fn normalized_finish_reason(reason: &psychevo_ai::FinishReason) -> String {
    match reason.kind {
        FinishReasonKind::Stop => "stop",
        FinishReasonKind::Length => "length",
        FinishReasonKind::ToolCalls => "tool_calls",
        FinishReasonKind::ContentFilter => "content_filter",
        FinishReasonKind::Other => "other",
    }
    .to_string()
}

async fn emit_generation_failure(
    sink: &Arc<dyn EventSink>,
    generation_id: String,
    request: &AgentLoopRequest,
    started: Instant,
    usage: Option<Value>,
    metadata: Option<Value>,
    error: String,
) -> Result<()> {
    emit(
        sink,
        AgentEvent::GenerationEnd {
            generation_id,
            provider: request.model_provider.clone(),
            model: request.model.clone(),
            outcome: Outcome::Failed,
            elapsed_ms: duration_ms_u64(started.elapsed()),
            usage,
            metadata,
            error: Some(error),
        },
    )
    .await
}
