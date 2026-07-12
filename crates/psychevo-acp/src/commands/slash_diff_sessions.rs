pub(crate) fn acp_command_capabilities()
-> &'static [psychevo_runtime::command_registry::CommandCapability] {
    use psychevo_runtime::command_registry::CommandCapability;
    &[
        CommandCapability::ActiveTurnControl,
        CommandCapability::Queue,
        CommandCapability::SessionSwitch,
        CommandCapability::SessionRevert,
        CommandCapability::ArtifactWrite,
        CommandCapability::WorkspaceDiff,
        CommandCapability::ConfigWrite,
        CommandCapability::PolicyWrite,
        CommandCapability::SkillStateWrite,
    ]
}

pub(crate) fn send_slash_text(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    text: impl Into<String>,
) -> SlashPromptAction {
    send_session_update(
        cx,
        session_id.clone(),
        agent_message_update(session_id, text),
    );
    SlashPromptAction::Handled(PromptResponse::new())
}

pub(crate) fn agent_message_update(
    session_id: &SessionId,
    text: impl Into<String>,
) -> SessionUpdate {
    SessionUpdate::AgentMessageChunk(text_chunk(session_id, "agent", text))
}

pub(crate) fn agent_thought_update(
    session_id: &SessionId,
    text: impl Into<String>,
) -> SessionUpdate {
    SessionUpdate::AgentThoughtChunk(text_chunk(session_id, "thought", text))
}

fn text_chunk(session_id: &SessionId, stream: &str, text: impl Into<String>) -> ContentChunk {
    ContentChunk::new(
        ContentBlock::Text(TextContent::new(text)),
        MessageId::new(format!("{session_id}:{stream}")),
    )
}

pub(crate) fn send_diff_tool_call(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    diff: &WorkspaceDiff,
) -> SlashPromptAction {
    let call_id = format!("slash_diff_{}", Uuid::now_v7());
    let (start, completed) = diff_tool_call_updates(call_id, diff);
    send_session_update(cx, session_id.clone(), start);
    send_session_update(cx, session_id.clone(), completed);
    SlashPromptAction::Handled(PromptResponse::new())
}

fn diff_tool_call_updates(
    call_id: impl Into<String>,
    diff: &WorkspaceDiff,
) -> (SessionUpdate, SessionUpdate) {
    let call_id = call_id.into();
    (
        SessionUpdate::ToolCallUpdate(
            ToolCallUpdate::new(call_id.clone())
                .title("Workspace diff")
                .kind(ToolKind::Read)
                .status(ToolCallStatus::InProgress)
                .raw_input(json!({ "command": "/diff" })),
        ),
        SessionUpdate::ToolCallUpdate(
            ToolCallUpdate::new(call_id)
                .title("Workspace diff")
                .kind(ToolKind::Read)
                .status(ToolCallStatus::Completed)
                .content(acp_diff_content(diff))
                .raw_output(diff_raw_output(diff)),
        ),
    )
}

fn acp_diff_content(diff: &WorkspaceDiff) -> Vec<ToolCallContent> {
    let changes = diff
        .files
        .iter()
        .map(|file| {
            let path = PathBuf::from(&file.path);
            match (file.old_text.is_some(), file.new_text.is_some()) {
                (false, true) => DiffChange::add(path),
                (true, false) => DiffChange::delete(path),
                _ => DiffChange::modify(path),
            }
        })
        .collect();
    let acp_diff = if diff.unified_diff.trim().is_empty() {
        AcpDiff::new(changes)
    } else {
        AcpDiff::patch(diff.unified_diff.clone(), changes)
    };
    vec![ToolCallContent::Diff(acp_diff)]
}

fn diff_raw_output(diff: &WorkspaceDiff) -> Value {
    let status = if !diff.is_git_repo {
        "not_git_repo"
    } else if diff.is_empty() {
        "empty"
    } else {
        "ok"
    };
    json!({
        "status": status,
        "file_count": diff.files.len(),
        "truncation": diff.truncation,
        "files": diff.files.iter().map(diff_file_summary).collect::<Vec<_>>(),
    })
}

fn diff_file_summary(file: &WorkspaceDiffFile) -> Value {
    json!({
        "path": file.path,
        "status": file.status,
        "binary": file.binary,
        "unreadable": file.unreadable,
        "placeholder": file.placeholder,
    })
}

pub(crate) fn user_text_message(text: &str) -> Message {
    Message::User {
        content: vec![UserContentBlock::text(text)],
        timestamp_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64,
    }
}

pub(crate) fn resolve_session_reference(
    reference: &str,
    sessions: &[SessionSummary],
) -> Option<SessionSummary> {
    if sessions.is_empty() {
        return None;
    }
    if reference.is_empty() || reference == "latest" {
        return sessions.first().cloned();
    }
    if let Ok(index) = reference.parse::<usize>()
        && index > 0
    {
        return sessions.get(index - 1).cloned();
    }
    let id_matches = sessions
        .iter()
        .filter(|summary| summary.id.starts_with(reference))
        .cloned()
        .collect::<Vec<_>>();
    if id_matches.len() == 1 {
        return id_matches.into_iter().next();
    }
    let title_matches = sessions
        .iter()
        .filter(|summary| summary.title.as_deref() == Some(reference))
        .cloned()
        .collect::<Vec<_>>();
    (title_matches.len() == 1)
        .then(|| title_matches.into_iter().next())
        .flatten()
}

pub(crate) fn ambiguous_session_matches(
    reference: &str,
    sessions: &[SessionSummary],
) -> Vec<SessionSummary> {
    if reference.is_empty() || reference == "latest" {
        return Vec::new();
    }
    let id_matches = sessions
        .iter()
        .filter(|summary| summary.id.starts_with(reference))
        .cloned()
        .collect::<Vec<_>>();
    if id_matches.len() > 1 {
        return id_matches;
    }
    let title_matches = sessions
        .iter()
        .filter(|summary| summary.title.as_deref() == Some(reference))
        .cloned()
        .collect::<Vec<_>>();
    if title_matches.len() > 1 {
        title_matches
    } else {
        Vec::new()
    }
}

pub(crate) fn reasoning_effort_value(value: &str) -> Option<String> {
    (value != "none").then(|| value.to_string())
}

pub(crate) fn available_commands_from(
    available: psychevo_runtime::command_registry::AvailableSlashCommands,
) -> Vec<AvailableCommand> {
    available
        .commands
        .into_iter()
        .map(|command| {
            let description = if command.aliases.is_empty() {
                command.summary
            } else {
                format!(
                    "{} (aliases: {})",
                    command.summary,
                    command.aliases.join(", ")
                )
            };
            let input = match command.argument_kind {
                psychevo_runtime::command_registry::CommandArgumentKind::None => None,
                _ => Some(AvailableCommandInput::Text(TextCommandInput::new(
                    command.usage,
                ))),
            };
            AvailableCommand::new(command.name, description).input(input)
        })
        .collect()
}

pub(crate) fn available_command_lines_from(commands: Vec<AvailableCommand>) -> Vec<String> {
    commands
        .into_iter()
        .map(|command| {
            let input_hint = command
                .input
                .as_ref()
                .map(|input| match input {
                    AvailableCommandInput::Text(input) => input.hint.clone(),
                    _ => String::new(),
                })
                .unwrap_or_default();
            let display = if input_hint.starts_with('/') {
                input_hint
            } else if input_hint.is_empty() {
                format!("/{}", command.name)
            } else {
                format!("/{} {}", command.name, input_hint)
            };
            format!("- {display} - {}", command.description)
        })
        .collect()
}

pub(crate) struct ParsedArtifactArgs {
    pub(crate) path: Option<PathBuf>,
    pub(crate) format: Option<SessionExportFormat>,
    pub(crate) include: Option<SessionExportIncludeSet>,
}

pub(crate) fn parse_artifact_args(
    args: &str,
    artifact_kind: SessionArtifactKind,
) -> std::result::Result<ParsedArtifactArgs, String> {
    let tokens = args.split_whitespace().collect::<Vec<_>>();
    let mut path = None;
    let mut format = None;
    let mut include = None;
    let mut index = 0usize;
    while index < tokens.len() {
        match tokens[index] {
            "--format" | "-f" if artifact_kind == SessionArtifactKind::Export => {
                index += 1;
                let Some(value) = tokens.get(index) else {
                    return Err(
                        "usage: /export [path] [-f|--format markdown|json] [-i|--include list]"
                            .to_string(),
                    );
                };
                format = Some(parse_export_format(value)?);
            }
            value
                if artifact_kind == SessionArtifactKind::Export
                    && value.starts_with("--format=") =>
            {
                format = Some(parse_export_format(value.trim_start_matches("--format="))?);
            }
            "--include" | "-i" => {
                index += 1;
                let Some(value) = tokens.get(index) else {
                    return Err("usage: /export|/share [path] [-i|--include list]".to_string());
                };
                include = Some(
                    SessionExportIncludeSet::parse(value, artifact_kind)
                        .map_err(|err| err.to_string())?,
                );
            }
            value if value.starts_with("--include=") => {
                include = Some(
                    SessionExportIncludeSet::parse(
                        value.trim_start_matches("--include="),
                        artifact_kind,
                    )
                    .map_err(|err| err.to_string())?,
                );
            }
            value if value.starts_with('-') => {
                return Err(format!("unsupported option: {value}"));
            }
            value => {
                if path.is_some() {
                    return Err("only one output path is supported".to_string());
                }
                path = Some(PathBuf::from(value));
            }
        }
        index += 1;
    }
    Ok(ParsedArtifactArgs {
        path,
        format,
        include,
    })
}

pub(crate) fn parse_export_format(value: &str) -> std::result::Result<SessionExportFormat, String> {
    match value {
        "markdown" | "md" => Ok(SessionExportFormat::Markdown),
        "json" => Ok(SessionExportFormat::Json),
        _ => Err("format must be markdown or json".to_string()),
    }
}

pub(crate) fn skill_scope_from_args(args: &[&str]) -> SkillTarget {
    match skill_option_value(args, "--scope") {
        Some("project") | Some("local") => SkillTarget::Project,
        _ => SkillTarget::Global,
    }
}

pub(crate) fn skill_option_value<'a>(args: &'a [&str], option: &str) -> Option<&'a str> {
    args.windows(2)
        .find_map(|window| (window[0] == option).then_some(window[1]))
}

pub(crate) fn skill_args_without_scope<'a>(args: &'a [&str]) -> Vec<&'a str> {
    let mut filtered = Vec::new();
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if *arg == "--scope" {
            skip_next = true;
            continue;
        }
        filtered.push(*arg);
    }
    filtered
}

#[derive(Clone)]
pub(crate) struct AcpApprovalHandler {
    pub(crate) session_id: SessionId,
    pub(crate) cx: ConnectionTo<Client>,
}

impl fmt::Debug for AcpApprovalHandler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AcpApprovalHandler")
            .field("session_id", &self.session_id)
            .finish_non_exhaustive()
    }
}

impl ApprovalHandler for AcpApprovalHandler {
    fn request_permission(
        &self,
        request: PermissionApprovalRequest,
    ) -> BoxFuture<'static, PermissionApprovalDecision> {
        let session_id = self.session_id.clone();
        let cx = self.cx.clone();
        Box::pin(async move {
            let title = format!("Permission: {}", request.tool_name);
            let tool_call = ToolCallUpdate::new(request.tool_call_id.clone())
                .title(title.clone())
                .status(ToolCallStatus::Pending)
                .raw_input(json!({
                    "summary": request.summary,
                    "reason": request.reason,
                    "matched_rule": request.matched_rule,
                    "suggested_rule": request.suggested_rule,
                }));
            let mut options = vec![
                PermissionOption::new("allow_once", "Allow once", PermissionOptionKind::AllowOnce),
                PermissionOption::new(
                    "allow_session",
                    "Allow for session",
                    PermissionOptionKind::AllowAlways,
                ),
                PermissionOption::new("deny", "Deny", PermissionOptionKind::RejectOnce),
            ];
            if request.allow_always {
                options.insert(
                    2,
                    PermissionOption::new(
                        "allow_always",
                        "Allow always",
                        PermissionOptionKind::AllowAlways,
                    ),
                );
            }
            match cx
                .send_request(
                    RequestPermissionRequest::new(session_id, title, options)
                        .description(request.reason)
                        .subject(RequestPermissionSubject::from(tool_call)),
                )
                .block_task()
                .await
            {
                Ok(response) => match response.outcome {
                    RequestPermissionOutcome::Cancelled => PermissionApprovalDecision::deny(),
                    RequestPermissionOutcome::Selected(selected) => {
                        match selected.option_id.to_string().as_str() {
                            "allow_once" => PermissionApprovalDecision::allow_once(),
                            "allow_session" => PermissionApprovalDecision::allow_session(),
                            "allow_always" => PermissionApprovalDecision::allow_always(),
                            _ => PermissionApprovalDecision::deny(),
                        }
                    }
                    _ => PermissionApprovalDecision::deny(),
                },
                Err(_) => PermissionApprovalDecision::deny(),
            }
        })
    }
}

pub(crate) fn send_session_setup_updates(
    cx: &ConnectionTo<Client>,
    session_id: SessionId,
    config_options: Vec<SessionConfigOption>,
    commands: Vec<AvailableCommand>,
) {
    send_session_update(
        cx,
        session_id.clone(),
        SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(config_options)),
    );
    send_session_update(
        cx,
        session_id,
        SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(commands)),
    );
}

pub(crate) fn send_session_update(
    cx: &ConnectionTo<Client>,
    session_id: SessionId,
    update: SessionUpdate,
) {
    let _ = cx.send_notification(UpdateSessionNotification::new(session_id, update));
}

#[derive(Debug, Default)]
pub(crate) struct AcpLiveProjection {
    reasoning_offsets: HashMap<String, usize>,
    terminal_output: bool,
    terminal_offsets: HashMap<String, usize>,
}
