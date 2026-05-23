#[derive(Debug, Clone)]
struct ToolCallBuilder {
    id: String,
    name: String,
    arguments_json: String,
    content_index: usize,
    call_index: usize,
}

fn assistant_outcome(message: &Message) -> Outcome {
    match message {
        Message::Assistant { outcome, .. } => *outcome,
        _ => Outcome::Failed,
    }
}

fn assistant_tool_calls(message: &Message) -> Vec<ToolCallBlock> {
    let Message::Assistant { content, .. } = message else {
        return Vec::new();
    };
    content
        .iter()
        .filter_map(|block| match block {
            AssistantBlock::ToolCall(call) => Some(call.clone()),
            _ => None,
        })
        .collect()
}

async fn execute_tool_batch(
    tools: &[Arc<dyn ToolBinding>],
    tool_calls: &[ToolCallBlock],
    sink: Arc<dyn EventSink>,
    abort: AbortSignal,
) -> Result<Vec<Message>> {
    let has_sequential = tool_calls.iter().any(|call| {
        tools
            .iter()
            .find(|tool| tool.name() == call.name)
            .is_none_or(|tool| tool.execution_mode() == ToolExecutionMode::Sequential)
    });

    let outputs = if has_sequential {
        let mut outputs = Vec::new();
        for call in tool_calls {
            let output =
                execute_one_tool(tools, call.clone(), Arc::clone(&sink), abort.clone()).await?;
            outputs.push(output);
        }
        outputs
    } else {
        let futures = tool_calls
            .iter()
            .cloned()
            .map(|call| execute_one_tool(tools, call, Arc::clone(&sink), abort.clone()));
        let joined = join_all(futures).await;
        let mut outputs = Vec::new();
        for output in joined {
            outputs.push(output?);
        }
        outputs
    };

    let now = now_ms();
    let mut result_messages = Vec::new();
    let mut attachment_messages = Vec::new();
    for (call, output) in outputs {
        attachment_messages.extend(tool_attachment_messages(&call, &output, now));
        result_messages.push(tool_result_message(call, output));
    }
    result_messages.extend(attachment_messages);
    Ok(result_messages)
}

async fn execute_one_tool(
    tools: &[Arc<dyn ToolBinding>],
    call: ToolCallBlock,
    sink: Arc<dyn EventSink>,
    abort: AbortSignal,
) -> Result<(ToolCallBlock, ToolOutput)> {
    let started_at_ms = now_ms();
    let started = Instant::now();
    let tool = tools.iter().find(|tool| tool.name() == call.name).cloned();
    let display = tool
        .as_ref()
        .map(|tool| tool.display_spec())
        .unwrap_or_else(|| ToolDisplaySpec::for_name(&call.name));
    emit(
        &sink,
        AgentEvent::ToolExecutionStart {
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            args: call.arguments.clone(),
            started_at_ms,
            display: Some(display.clone()),
        },
    )
    .await?;
    let output = if let Some(err) = &call.arguments_error {
        ToolOutput::error(format!("invalid tool arguments JSON: {err}"))
    } else if let Some(tool) = tool {
        tool.execute(call.id.clone(), call.arguments.clone(), abort)
            .await
    } else {
        ToolOutput::error(format!("tool not found: {}", call.name))
    };
    let outcome = if output.is_error {
        Outcome::Failed
    } else {
        Outcome::Normal
    };
    emit(
        &sink,
        AgentEvent::ToolExecutionEnd {
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            result: output.json.clone(),
            outcome,
            elapsed_ms: duration_ms_u64(started.elapsed()),
            display: Some(display),
        },
    )
    .await?;
    Ok((call, output))
}

fn tool_result_message(call: ToolCallBlock, output: ToolOutput) -> Message {
    Message::ToolResult {
        tool_call_id: call.id,
        tool_name: call.name,
        content: output.model_content(),
        is_error: output.is_error,
        timestamp_ms: now_ms(),
    }
}

fn tool_attachment_messages(call: &ToolCallBlock, output: &ToolOutput, timestamp_ms: i64) -> Vec<Message> {
    output
        .attachments
        .iter()
        .map(|attachment| match attachment {
            ToolAttachment::ImageUrl {
                url,
                mime_type,
                source_url,
            } => Message::User {
                content: vec![
                    UserContentBlock::text(format!(
                        "Image attachment from tool `{}`{} ({mime_type}):",
                        call.name,
                        source_url
                            .as_deref()
                            .map(|url| format!(" at {url}"))
                            .unwrap_or_default()
                    )),
                    UserContentBlock::image_url(url.clone()),
                ],
                timestamp_ms,
            },
        })
        .collect()
}
