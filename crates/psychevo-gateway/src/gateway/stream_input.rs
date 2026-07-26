fn thread_key(thread_id: &str) -> String {
    format!("thread:{thread_id}")
}

fn source_key_key(source_key: &SourceKey) -> String {
    format!("source:{}", source_key.0)
}

fn wrap_stream(
    stream: Option<RunStreamSink>,
    event_sink: Option<GatewayEventEmitter>,
    turn_id: String,
    thread_id: Option<String>,
) -> Option<RunStreamSink> {
    match (stream, event_sink) {
        (None, None) => None,
        (stream, event_sink) => {
            let projector = Arc::new(Mutex::new(GatewayLiveProjector::new(thread_id)));
            Some(Arc::new(move |event: RunStreamEvent| {
                if let Some(event_sink) = &event_sink
                    && let Some(event) = projector
                        .lock()
                        .expect("gateway live projector poisoned")
                        .project(&turn_id, &event)
                    // Adapter stream terminals are useful as an internal
                    // projection fence, but the Framework Application owns the one
                    // authoritative public terminal after persistence and
                    // delivery classification have completed.
                    && !matches!(
                        event,
                        GatewayEvent::TurnStarted { .. } | GatewayEvent::TurnCompleted { .. }
                    )
                {
                    let fields = journey_profile::gateway_profile_event_fields(&event);
                    gateway_profile_mark(
                        "gateway_event_emitted",
                        Some(&turn_id),
                        match &event {
                            GatewayEvent::EntryStarted { entry, .. }
                            | GatewayEvent::EntryUpdated { entry, .. }
                            | GatewayEvent::EntryCompleted { entry, .. } => {
                                Some(entry.thread_id.as_str())
                            }
                            _ => None,
                        },
                        fields,
                    );
                    let _ = event_sink.emit(event);
                }
                if let Some(stream) = &stream {
                    stream(event);
                }
            }))
        }
    }
}

fn framework_input_parts(
    input: &[GatewayInputPart],
) -> psychevo::Result<(String, Vec<ImageInput>, PromptDisplayMetadata, bool)> {
    let mut prompt_parts = Vec::new();
    let mut image_inputs = Vec::new();
    let mut editable_parts = Vec::new();
    let mut editable_text_parts = Vec::new();
    let mut has_structured_input = false;
    for part in input {
        match part {
            GatewayInputPart::Text { text } => {
                prompt_parts.push(text.clone());
                editable_text_parts.push(text.clone());
                editable_parts.push(StoredEditableInputPart::Text { text: text.clone() });
            }
            GatewayInputPart::Context {
                text,
                visible_to_model,
                ..
            } if *visible_to_model => prompt_parts.push(text.clone()),
            GatewayInputPart::Context { .. } => {}
            GatewayInputPart::Image { input } => {
                let image_block_index = image_inputs.len();
                image_inputs.push(gateway_image_input_into_runtime(input.clone()));
                editable_parts.push(StoredEditableInputPart::Image { image_block_index });
            }
            GatewayInputPart::Resource { text, blob, .. } => {
                if text.is_some() == blob.is_some() {
                    return Err(agent_session_error(
                        "invalid_input",
                        AgentErrorStage::Delivery,
                        "user_action",
                        "not_delivered",
                        "A resource input must contain exactly one of `text` or `blob`.",
                        None,
                    ));
                }
                has_structured_input = true;
            }
            GatewayInputPart::ResourceLink { name, uri, .. } => {
                if name.trim().is_empty() || uri.trim().is_empty() {
                    return Err(agent_session_error(
                        "invalid_input",
                        AgentErrorStage::Delivery,
                        "user_action",
                        "not_delivered",
                        "A resource link requires non-empty `name` and `uri`.",
                        None,
                    ));
                }
                has_structured_input = true;
            }
        }
    }
    let prompt = prompt_parts.join("\n");
    let prompt_display = PromptDisplayMetadata {
        content_text: editable_text_parts.join("\n"),
        attachments: Vec::new(),
        editable_input: Some(StoredEditableInputEnvelope {
            version: 1,
            parts: editable_parts,
        }),
    };
    Ok((prompt, image_inputs, prompt_display, has_structured_input))
}

fn gateway_image_input_into_runtime(input: GatewayImageInput) -> ImageInput {
    match input {
        GatewayImageInput::LocalPath { path } => ImageInput::LocalPath(path.into()),
        GatewayImageInput::Url { url } => ImageInput::ImageUrl(url),
    }
}
