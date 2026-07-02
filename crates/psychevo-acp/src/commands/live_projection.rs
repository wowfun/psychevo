impl AcpLiveProjection {
    pub(crate) fn new(terminal_output: bool) -> Self {
        Self {
            terminal_output,
            ..Self::default()
        }
    }
}

pub(crate) fn send_gateway_event_update(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    event: GatewayEvent,
    projection: &mut AcpLiveProjection,
) {
    match event {
        GatewayEvent::EntryStarted { entry, .. }
        | GatewayEvent::EntryUpdated { entry, .. }
        | GatewayEvent::EntryCompleted { entry, .. } => {
            for update in transcript_entry_session_updates(&entry, session_id, projection, true) {
                send_session_update(cx, session_id.clone(), update);
            }
        }
        GatewayEvent::Warning { message, .. } => send_session_update(
            cx,
            session_id.clone(),
            agent_message_update(session_id, format!("warning: {message}")),
        ),
        GatewayEvent::TurnCompleted {
            committed_entries, ..
        } => {
            for entry in committed_entries {
                for update in
                    transcript_entry_session_updates(&entry, session_id, projection, false)
                {
                    send_session_update(cx, session_id.clone(), update);
                }
            }
        }
        GatewayEvent::TurnStarted { .. }
        | GatewayEvent::TurnQueued { .. }
        | GatewayEvent::ActionRequested { .. }
        | GatewayEvent::ActionUpdated { .. }
        | GatewayEvent::ActionResolved { .. }
        | GatewayEvent::ActionCancelled { .. }
        | GatewayEvent::ActivityChanged { .. }
        | GatewayEvent::TitleChanged { .. } => {}
    }
}

fn transcript_entry_session_updates(
    entry: &TranscriptEntry,
    session_id: &SessionId,
    projection: &mut AcpLiveProjection,
    include_reasoning: bool,
) -> Vec<SessionUpdate> {
    let mut updates = Vec::new();
    for block in &entry.blocks {
        if include_reasoning
            && block.kind == TranscriptBlockKind::Reasoning
            && let Some(delta) = reasoning_block_delta(block, projection)
        {
            updates.push(agent_thought_update(session_id, delta));
        }
        if let Some(update) = transcript_block_session_update(block, projection, include_reasoning)
        {
            updates.push(update);
        }
    }
    updates
}

fn transcript_block_session_update(
    block: &TranscriptBlock,
    projection: &mut AcpLiveProjection,
    live_presentation: bool,
) -> Option<SessionUpdate> {
    if !matches!(
        block.kind,
        TranscriptBlockKind::Tool
            | TranscriptBlockKind::ToolCall
            | TranscriptBlockKind::ToolResult
            | TranscriptBlockKind::Shell
            | TranscriptBlockKind::File
            | TranscriptBlockKind::Web
            | TranscriptBlockKind::Mcp
            | TranscriptBlockKind::Clarify
            | TranscriptBlockKind::Diff
            | TranscriptBlockKind::Artifact
    ) {
        return None;
    }
    let call_id = transcript_tool_call_id(block);
    let tool_name = block
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("tool_name"))
        .and_then(Value::as_str)
        .or(block.title.as_deref())
        .unwrap_or("tool");
    let use_terminal_output =
        live_presentation && projection.terminal_output && tool_name == "exec_command";
    let content = transcript_tool_content(block, tool_name, &call_id, use_terminal_output);
    let mut update = ToolCallUpdate::new(
        call_id,
        ToolCallUpdateFields::new()
            .title(transcript_tool_title(block, tool_name))
            .kind(tool_kind(tool_name))
            .status(transcript_tool_status(block.status))
            .content(content)
            .raw_input(block.metadata.clone()),
    );
    if use_terminal_output
        && let Some(meta) = terminal_output_meta(block, update.tool_call_id.0.as_ref(), projection)
    {
        update = update.meta(meta);
    }
    Some(SessionUpdate::ToolCallUpdate(update))
}

fn reasoning_block_delta(
    block: &TranscriptBlock,
    projection: &mut AcpLiveProjection,
) -> Option<String> {
    let text = transcript_block_text(block)?.to_string();
    if text.trim().is_empty() {
        return None;
    }
    let offset = projection
        .reasoning_offsets
        .entry(block.id.clone())
        .or_insert(0);
    if *offset > text.len() {
        *offset = 0;
    }
    let delta = text.get(*offset..)?.to_string();
    *offset = text.len();
    if delta.is_empty() { None } else { Some(delta) }
}

fn transcript_tool_title(block: &TranscriptBlock, tool_name: &str) -> String {
    if let Some(title) = block
        .title
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return title.to_string();
    }
    if tool_name == "exec_command"
        && let Some(command) =
            exec_command_arg(block.metadata.as_ref()).and_then(first_shell_command_line)
    {
        return format!("exec_command {command}");
    }
    tool_title(tool_name)
}

fn transcript_tool_content(
    block: &TranscriptBlock,
    tool_name: &str,
    _call_id: &str,
    use_terminal_output: bool,
) -> Vec<ToolCallContent> {
    if tool_name == "exec_command"
        && let Some(command) = exec_command_arg(block.metadata.as_ref())
    {
        let command_text = format!("$ {command}");
        if use_terminal_output {
            return vec![ToolCallContent::from(command_text)];
        }
        let mut text = command_text;
        if let Some(output) = transcript_block_text(block).filter(|value| !value.trim().is_empty())
        {
            text.push_str("\n\n");
            text.push_str(output);
        }
        return vec![ToolCallContent::from(text)];
    }
    transcript_block_text(block)
        .filter(|text| !text.trim().is_empty())
        .map(|text| vec![ToolCallContent::from(text.to_string())])
        .unwrap_or_default()
}

fn transcript_block_text(block: &TranscriptBlock) -> Option<&str> {
    block
        .result
        .as_ref()
        .map(|result| result.content.as_str())
        .or(block
            .detail
            .as_deref()
            .or(block.body.as_deref())
            .or(block.preview.as_deref()))
}

fn exec_command_arg(metadata: Option<&Value>) -> Option<&str> {
    metadata?
        .get("args")
        .and_then(|args| args.get("cmd"))
        .and_then(Value::as_str)
}

fn first_shell_command_line(text: &str) -> Option<&str> {
    let mut first_non_empty = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        first_non_empty.get_or_insert(line);
        if !line.starts_with('#') {
            return Some(line);
        }
    }
    first_non_empty
}

fn terminal_output_meta(
    block: &TranscriptBlock,
    call_id: &str,
    projection: &mut AcpLiveProjection,
) -> Option<Meta> {
    let command = exec_command_arg(block.metadata.as_ref())?;
    let first_update = !projection.terminal_offsets.contains_key(call_id);
    let mut meta = Meta::new();
    if first_update {
        meta.insert(
            "terminal_info".to_string(),
            json!({
                "terminal_id": call_id,
                "command": command,
            }),
        );
    }

    let output = transcript_block_text(block).unwrap_or_default();
    let offset = projection
        .terminal_offsets
        .entry(call_id.to_string())
        .or_insert(0);
    if *offset > output.len() {
        *offset = 0;
    }
    let mut data = String::new();
    if first_update {
        data.push_str("$ ");
        data.push_str(command);
        data.push('\n');
    }
    if let Some(delta) = output.get(*offset..) {
        data.push_str(delta);
    }
    *offset = output.len();
    if !data.is_empty() {
        meta.insert(
            "terminal_output".to_string(),
            json!({
                "terminal_id": call_id,
                "data": data,
            }),
        );
    }
    if matches!(
        block.status,
        TranscriptBlockStatus::Completed
            | TranscriptBlockStatus::Failed
            | TranscriptBlockStatus::Cancelled
    ) {
        meta.insert(
            "terminal_exit".to_string(),
            json!({
                "terminal_id": call_id,
                "exit_code": exec_exit_code(block.metadata.as_ref()),
                "signal": null,
            }),
        );
    }
    if meta.is_empty() { None } else { Some(meta) }
}

fn exec_exit_code(metadata: Option<&Value>) -> Option<i64> {
    metadata?
        .get("result")
        .and_then(|result| result.get("exit_code"))
        .and_then(Value::as_i64)
}

fn transcript_tool_call_id(block: &TranscriptBlock) -> String {
    block
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("tool_call_id"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            block
                .id
                .rsplit_once("tool:")
                .map(|(_, id)| id)
                .unwrap_or(block.id.as_str())
                .to_string()
        })
}

fn transcript_tool_status(status: TranscriptBlockStatus) -> ToolCallStatus {
    match status {
        TranscriptBlockStatus::Pending => ToolCallStatus::Pending,
        TranscriptBlockStatus::Running => ToolCallStatus::InProgress,
        TranscriptBlockStatus::Completed | TranscriptBlockStatus::Info => ToolCallStatus::Completed,
        TranscriptBlockStatus::Failed | TranscriptBlockStatus::Cancelled => ToolCallStatus::Failed,
        TranscriptBlockStatus::NeedsInput => ToolCallStatus::Pending,
    }
}
