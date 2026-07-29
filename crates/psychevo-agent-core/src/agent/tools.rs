#[allow(unused_imports)]
pub(crate) use super::*;
#[derive(Debug, Clone)]
pub(crate) struct ToolCallBuilder {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) arguments_json: String,
    pub(crate) argument_error: Option<psychevo_ai::ToolArgumentError>,
    pub(crate) content_index: usize,
    pub(crate) call_index: usize,
}

type IndexedToolExecution = BoxFuture<'static, (usize, Result<(ToolCallBlock, ToolOutput)>)>;

pub(crate) fn assistant_outcome(message: &Message) -> Outcome {
    match message {
        Message::Assistant { outcome, .. } => *outcome,
        _ => Outcome::Failed,
    }
}

pub(crate) fn assistant_tool_calls(message: &Message) -> Vec<ToolCallBlock> {
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

pub(crate) async fn execute_tool_batch(
    router: &mut ToolRouter,
    tool_calls: &[ToolCallBlock],
    sink: Arc<dyn EventSink>,
    abort: AbortSignal,
) -> Result<Vec<Message>> {
    const MAX_PARALLEL_TOOLS: usize = 8;
    let mut indexed_outputs = Vec::with_capacity(tool_calls.len());
    let mut index = 0usize;
    while index < tool_calls.len() {
        if router.execution_mode_for_call(&tool_calls[index]) == ToolExecutionMode::Sequential {
            let output = execute_one_tool_mut(
                router,
                tool_calls[index].clone(),
                Arc::clone(&sink),
                abort.clone(),
            )
            .await?;
            indexed_outputs.push((index, output));
            index += 1;
            continue;
        }

        let segment_start = index;
        while index < tool_calls.len()
            && router.execution_mode_for_call(&tool_calls[index]) == ToolExecutionMode::Parallel
        {
            index += 1;
        }
        let segment_end = index;
        let router_snapshot = router.clone();
        let mut running: FuturesUnordered<IndexedToolExecution> = FuturesUnordered::new();
        let mut next = segment_start;
        while next < segment_end && running.len() < MAX_PARALLEL_TOOLS {
            let call = tool_calls[next].clone();
            let sink = Arc::clone(&sink);
            let abort = abort.clone();
            let router = router_snapshot.clone();
            running.push(Box::pin(async move {
                (next, execute_one_tool(&router, call, sink, abort).await)
            }));
            next += 1;
        }
        let mut fatal = None;
        while let Some((source_index, result)) = running.next().await {
            match result {
                Ok(output) => indexed_outputs.push((source_index, output)),
                Err(error) => {
                    fatal.get_or_insert(error);
                }
            }
            if fatal.is_none() && next < segment_end {
                let call = tool_calls[next].clone();
                let sink = Arc::clone(&sink);
                let abort = abort.clone();
                let router = router_snapshot.clone();
                running.push(Box::pin(async move {
                    (next, execute_one_tool(&router, call, sink, abort).await)
                }));
                next += 1;
            }
        }
        if let Some(error) = fatal {
            return Err(error);
        }
    }

    let now = now_ms();
    let mut result_messages = Vec::new();
    let mut attachment_messages = Vec::new();
    indexed_outputs.sort_by_key(|(source_index, _)| *source_index);
    for (_, (call, output)) in indexed_outputs {
        attachment_messages.extend(tool_attachment_messages(&call, &output, now));
        result_messages.push(tool_result_message(call, output));
    }
    result_messages.extend(attachment_messages);
    Ok(result_messages)
}

pub(crate) async fn execute_one_tool_mut(
    router: &mut ToolRouter,
    call: ToolCallBlock,
    sink: Arc<dyn EventSink>,
    abort: AbortSignal,
) -> Result<(ToolCallBlock, ToolOutput)> {
    if router.is_tool_search_call(&call.name) {
        return execute_tool_search(router, call, sink).await;
    }
    execute_one_tool(router, call, sink, abort).await
}

async fn execute_tool_search(
    router: &mut ToolRouter,
    call: ToolCallBlock,
    sink: Arc<dyn EventSink>,
) -> Result<(ToolCallBlock, ToolOutput)> {
    let started_at_ms = now_ms();
    let started = Instant::now();
    let display = router.display_spec(&call.name);
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
        ToolOutput::error(format!("invalid tool arguments: {}", err.message))
    } else {
        router.execute_tool_search(&call.arguments)
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

pub(crate) async fn execute_one_tool(
    router: &ToolRouter,
    call: ToolCallBlock,
    sink: Arc<dyn EventSink>,
    abort: AbortSignal,
) -> Result<(ToolCallBlock, ToolOutput)> {
    let started_at_ms = now_ms();
    let started = Instant::now();
    let tool = router
        .effective_exposure(&call.name)
        .is_some_and(ToolExposure::is_model_visible)
        .then(|| router.tool(&call.name))
        .flatten();
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
        ToolOutput::error(format!("invalid tool arguments: {}", err.message))
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

pub(crate) fn tool_result_message(call: ToolCallBlock, output: ToolOutput) -> Message {
    Message::ToolResult {
        tool_call_id: call.id,
        tool_name: call.name,
        content: output.model_content(),
        is_error: output.is_error,
        timestamp_ms: now_ms(),
    }
}

pub(crate) fn tool_attachment_messages(
    call: &ToolCallBlock,
    output: &ToolOutput,
    timestamp_ms: i64,
) -> Vec<Message> {
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

#[cfg(test)]
mod provider_tool_tests {
    use super::*;

    #[test]
    fn provider_executed_blocks_never_become_router_calls() {
        let message = Message::Assistant {
            content: vec![AssistantBlock::ProviderTool(ProviderToolBlock {
                id: "ws_1".into(),
                name: "web_search".into(),
                action: Some(json!({"type":"search","query":"rust"})),
                status: "completed".into(),
            })],
            timestamp_ms: 0,
            finish_reason: Some("completed".into()),
            outcome: Outcome::Normal,
            model: Some("gpt-5".into()),
            provider: Some("openai".into()),
        };
        assert!(assistant_tool_calls(&message).is_empty());
    }
}
