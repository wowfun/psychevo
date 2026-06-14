use super::*;

pub(super) fn command_execute_value(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::CommandExecuteParams,
) -> psychevo_runtime::Result<Value> {
    let raw = params.command.trim().to_string();
    let thread_id = params.thread_id.clone();
    if raw.is_empty() {
        return Ok(serde_json::to_value(command_rejected_unknown(
            &raw,
            Some("empty command".to_string()),
            None,
        ))?);
    }
    let active_turn = thread_id
        .as_deref()
        .map(|thread_id| state.activity(&scope.source, Some(thread_id)).running)
        .unwrap_or_else(|| state.activity(&scope.source, None).running);
    let dynamic = dynamic_slash_commands(state, scope)?;
    let result = match parse_slash_command_line(&raw) {
        SlashCommandParse::Known(invocation) => {
            let action = invocation.spec.action;
            if !web_desktop_action_visible(action) {
                command_unsupported(
                    &raw,
                    action,
                    web_desktop_unavailable_message(invocation.spec.canonical, action),
                )
            } else if active_turn
                && matches!(action, SlashCommandAction::Undo | SlashCommandAction::Redo)
            {
                let command_name = invocation.spec.canonical;
                command_known_result(
                    &raw,
                    action,
                    true,
                    Some(format!(
                        "interrupt requested; run {command_name} again after the turn settles"
                    )),
                    Some(json!({"type": "turnInterrupt", "threadId": thread_id})),
                )
            } else {
                match slash_invocation_effect(
                    &invocation,
                    &gateway_command_capabilities(),
                    SlashCommandSurface::WebDesktop,
                    active_turn,
                ) {
                    Ok(effect) => {
                        command_result_from_effect(state, scope, &raw, action, effect, thread_id)?
                    }
                    Err(message) => command_unsupported(&raw, action, message),
                }
            }
        }
        SlashCommandParse::Unknown {
            original,
            command,
            args,
        } => {
            if let Some(effect) = dynamic_slash_command_effect(&command, &args, &dynamic) {
                command_result_from_effect(
                    state,
                    scope,
                    &raw,
                    SlashCommandAction::SkillInvoke,
                    effect,
                    thread_id,
                )?
            } else {
                command_rejected_unknown(
                    &command,
                    None,
                    Some(json!({"type": "passThroughPrompt", "text": original})),
                )
            }
        }
        SlashCommandParse::NotSlash => command_rejected_unknown(
            &raw,
            None,
            Some(json!({"type": "passThroughPrompt", "text": raw})),
        ),
    };
    Ok(serde_json::to_value(result)?)
}

fn command_result_from_effect(
    state: &WebState,
    scope: &ResolvedScope,
    raw: &str,
    action: SlashCommandAction,
    effect: SlashCommandEffect,
    thread_id: Option<String>,
) -> psychevo_runtime::Result<wire::CommandExecuteResult> {
    match effect {
        SlashCommandEffect::LocalText => match action {
            SlashCommandAction::Help => Ok(command_action(
                raw,
                action,
                json!({"type": "showPanel", "panel": "commands"}),
            )),
            SlashCommandAction::Status
            | SlashCommandAction::Usage
            | SlashCommandAction::Context => Ok(command_action(
                raw,
                action,
                json!({"type": "showPanel", "panel": "status"}),
            )),
            _ => Ok(command_accepted_message(raw, action, None)),
        },
        SlashCommandEffect::PassThroughPrompt(text) => Ok(command_action(
            raw,
            action,
            json!({"type": "passThroughPrompt", "text": text}),
        )),
        SlashCommandEffect::SubmitPrompt(text) => Ok(command_action(
            raw,
            action,
            json!({"type": "submitPrompt", "text": text, "displayText": raw}),
        )),
        SlashCommandEffect::Steer(text) => Ok(command_action(
            raw,
            action,
            json!({"type": "steerPrompt", "text": text}),
        )),
        SlashCommandEffect::Queue(text) => Ok(command_action(
            raw,
            action,
            json!({"type": "queuePrompt", "text": text, "displayText": raw}),
        )),
        SlashCommandEffect::PendingCancel => Ok(command_action(
            raw,
            action,
            json!({"type": "turnInterrupt", "threadId": thread_id}),
        )),
        SlashCommandEffect::NewSession => {
            Ok(command_action(raw, action, json!({"type": "threadStart"})))
        }
        SlashCommandEffect::SessionsList => Ok(command_action(
            raw,
            action,
            json!({"type": "showPanel", "panel": "history"}),
        )),
        SlashCommandEffect::ResumeSession { .. } => Ok(command_action(
            raw,
            action,
            json!({"type": "showPanel", "panel": "history"}),
        )),
        SlashCommandEffect::Agents => Ok(command_action(
            raw,
            action,
            json!({"type": "showPanel", "panel": "agents"}),
        )),
        SlashCommandEffect::Export { .. } => Ok(command_action(
            raw,
            action,
            json!({"type": "downloadSession", "kind": "export", "threadId": thread_id}),
        )),
        SlashCommandEffect::Share { .. } => Ok(command_action(
            raw,
            action,
            json!({"type": "downloadSession", "kind": "share", "threadId": thread_id}),
        )),
        SlashCommandEffect::Fork(prompt) => Ok(command_action(
            raw,
            action,
            json!({"type": "submitPrompt", "text": prompt, "displayText": raw}),
        )),
        SlashCommandEffect::Compact { instructions } => Ok(command_action(
            raw,
            action,
            json!({"type": "submitPrompt", "text": compact_prompt_text(instructions), "displayText": raw}),
        )),
        SlashCommandEffect::Diff => {
            let diff = workspace_diff_result(scope, None)?;
            Ok(command_action(
                raw,
                action,
                json!({"type": "workspaceDiff", "diff": diff}),
            ))
        }
        SlashCommandEffect::SandboxShow => {
            let options = state.run_options(scope.workdir.clone(), thread_id.clone());
            let status = psychevo_runtime::sandbox_status_text(&options, RunMode::Default)?;
            Ok(command_accepted_message(raw, action, Some(status)))
        }
        SlashCommandEffect::Undo => Ok(command_session_undo(state, scope, raw, action, thread_id)),
        SlashCommandEffect::Redo => Ok(command_session_redo(state, scope, raw, action, thread_id)),
        SlashCommandEffect::Unsupported(message) => Ok(command_unsupported(raw, action, message)),
        SlashCommandEffect::ShowModel
        | SlashCommandEffect::SetModel { .. }
        | SlashCommandEffect::SetVariant(_)
        | SlashCommandEffect::SetMode(_)
        | SlashCommandEffect::PermissionsShow
        | SlashCommandEffect::PermissionAdd { .. }
        | SlashCommandEffect::PermissionRemove { .. }
        | SlashCommandEffect::ToolsShow
        | SlashCommandEffect::ToolsetSet { .. }
        | SlashCommandEffect::Rename(_)
        | SlashCommandEffect::Skills { .. }
        | SlashCommandEffect::Bundles { .. }
        | SlashCommandEffect::Curator { .. } => Ok(command_unsupported(
            raw,
            action,
            web_desktop_unavailable_message(raw.split_whitespace().next().unwrap_or(raw), action),
        )),
    }
}

fn command_session_undo(
    state: &WebState,
    scope: &ResolvedScope,
    raw: &str,
    action: SlashCommandAction,
    thread_id: Option<String>,
) -> wire::CommandExecuteResult {
    let options = match command_session_undo_options(state, scope, thread_id, "undo") {
        Ok(options) => options,
        Err(message) => return command_unsupported(raw, action, message),
    };
    match undo_session(options) {
        Ok(result) => command_known_result(
            raw,
            action,
            true,
            Some(format!(
                "undone {} messages; prompt restored",
                result.reverted_messages
            )),
            Some(json!({
                "type": "sessionUndo",
                "threadId": result.session_id,
                "prompt": result.prompt,
                "revertedMessages": result.reverted_messages
            })),
        ),
        Err(err) => command_unsupported(raw, action, err.to_string()),
    }
}

fn command_session_redo(
    state: &WebState,
    scope: &ResolvedScope,
    raw: &str,
    action: SlashCommandAction,
    thread_id: Option<String>,
) -> wire::CommandExecuteResult {
    let options = match command_session_undo_options(state, scope, thread_id, "redo") {
        Ok(options) => options,
        Err(message) => return command_unsupported(raw, action, message),
    };
    match redo_session(options) {
        Ok(result) => {
            let suffix = if result.complete {
                "complete"
            } else {
                "partial"
            };
            command_known_result(
                raw,
                action,
                true,
                Some(format!(
                    "redone {} messages; {suffix}",
                    result.restored_messages
                )),
                Some(json!({
                    "type": "sessionRedo",
                    "threadId": result.session_id,
                    "restoredMessages": result.restored_messages,
                    "complete": result.complete
                })),
            )
        }
        Err(err) => command_unsupported(raw, action, err.to_string()),
    }
}

fn command_session_undo_options(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<String>,
    verb: &str,
) -> std::result::Result<SessionUndoOptions, String> {
    let Some(thread_id) = thread_id else {
        return Err(format!("no current session to {verb}"));
    };
    let summary = state
        .inner
        .state
        .store()
        .session_summary(&thread_id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("session not found: {thread_id}"))?;
    if Path::new(&summary.workdir) != scope.workdir.as_path() {
        return Err(format!(
            "session {thread_id} does not belong to {}",
            scope.workdir.display()
        ));
    }
    Ok(SessionUndoOptions {
        state: state.inner.state.clone(),
        workdir: scope.workdir.clone(),
        snapshot_root: state.inner.home.join("snapshots"),
        session_id: thread_id,
    })
}

fn command_action(
    raw: &str,
    slash_action: SlashCommandAction,
    action: Value,
) -> wire::CommandExecuteResult {
    command_known_result(raw, slash_action, true, None, Some(action))
}

fn command_accepted_message(
    raw: &str,
    slash_action: SlashCommandAction,
    message: Option<String>,
) -> wire::CommandExecuteResult {
    command_known_result(raw, slash_action, true, message, None)
}

fn command_unsupported(
    raw: &str,
    slash_action: SlashCommandAction,
    message: String,
) -> wire::CommandExecuteResult {
    command_known_result(raw, slash_action, false, Some(message), None)
}

fn command_known_result(
    raw: &str,
    slash_action: SlashCommandAction,
    accepted: bool,
    message: Option<String>,
    action: Option<Value>,
) -> wire::CommandExecuteResult {
    let presentation = command_presentation(slash_action);
    wire::CommandExecuteResult {
        accepted,
        command: raw.to_string(),
        known: Some(true),
        presentation_kind: Some(presentation.kind.as_str().to_string()),
        feedback_anchor: Some(presentation.feedback_anchor.as_str().to_string()),
        alternate_action: command_alternate_action(presentation),
        message,
        action,
    }
}

fn command_rejected_unknown(
    raw: &str,
    message: Option<String>,
    action: Option<Value>,
) -> wire::CommandExecuteResult {
    wire::CommandExecuteResult {
        accepted: false,
        command: raw.to_string(),
        known: Some(false),
        presentation_kind: None,
        feedback_anchor: None,
        alternate_action: None,
        message,
        action,
    }
}

fn web_desktop_unavailable_message(command: &str, action: SlashCommandAction) -> String {
    let command = command.split_whitespace().next().unwrap_or(command);
    match action {
        SlashCommandAction::ModelShow
        | SlashCommandAction::VariantSet
        | SlashCommandAction::ModeSet => {
            format!("{command} is managed by the Workbench model controls.")
        }
        SlashCommandAction::Image => {
            format!("{command} is managed by the Workbench attachment control.")
        }
        SlashCommandAction::Permissions => {
            format!("{command} is managed by Workbench status controls.")
        }
        SlashCommandAction::Agents => {
            format!("{command} is managed by the Workbench agent selector and Settings Agents.")
        }
        SlashCommandAction::Sessions | SlashCommandAction::Resume => {
            format!("{command} is managed by Workbench history.")
        }
        SlashCommandAction::Tools
        | SlashCommandAction::Skills
        | SlashCommandAction::Bundles
        | SlashCommandAction::Curator => {
            format!("{command} is managed by Workbench panels.")
        }
        _ => format!("{command} is not available in Web/Desktop."),
    }
}

fn compact_prompt_text(instructions: Option<String>) -> String {
    match instructions {
        Some(instructions) if !instructions.trim().is_empty() => {
            format!(
                "Compact this session with these instructions:\n\n{}",
                instructions.trim()
            )
        }
        _ => "Compact this session.".to_string(),
    }
}

pub(super) fn command_list_value(
    state: &WebState,
    scope: &ResolvedScope,
    active_turn: bool,
) -> psychevo_runtime::Result<Value> {
    let dynamic = dynamic_slash_commands(state, scope)?;
    let dynamic_names = dynamic
        .iter()
        .map(|command| command.name.trim_start_matches('/').to_string())
        .collect::<std::collections::BTreeSet<_>>();
    let available = available_slash_commands_for_surface(
        &gateway_command_capabilities(),
        active_turn,
        &dynamic,
        256,
    );
    Ok(serde_json::to_value(wire::CommandListResult {
        commands: available
            .commands
            .iter()
            .filter(|command| web_desktop_command_visible(command))
            .map(|command| command_value(command, &dynamic_names))
            .collect(),
        hidden_dynamic: available.hidden_dynamic,
    })?)
}

fn command_value(
    command: &AvailableSlashCommand,
    dynamic_names: &std::collections::BTreeSet<String>,
) -> wire::CommandListItem {
    let presentation = command.presentation;
    wire::CommandListItem {
        name: command.name.clone(),
        slash: format!("/{}", command.name),
        usage: command.usage.clone(),
        summary: command.summary.clone(),
        aliases: command
            .aliases
            .iter()
            .map(|alias| alias.trim_start_matches('/').to_string())
            .collect(),
        argument_kind: command_argument_kind(command.argument_kind).to_string(),
        source: if dynamic_names.contains(&command.name) {
            "dynamic".to_string()
        } else {
            "core".to_string()
        },
        presentation_kind: Some(presentation.kind.as_str().to_string()),
        destination: Some(presentation.destination.as_str().to_string()),
        feedback_anchor: Some(presentation.feedback_anchor.as_str().to_string()),
        alternate_action: command_alternate_action(presentation),
    }
}

pub(super) fn web_desktop_command_visible(command: &AvailableSlashCommand) -> bool {
    matches!(
        command.action,
        SlashCommandAction::Help
            | SlashCommandAction::Status
            | SlashCommandAction::New
            | SlashCommandAction::Sessions
            | SlashCommandAction::Resume
            | SlashCommandAction::Usage
            | SlashCommandAction::Context
            | SlashCommandAction::Diff
            | SlashCommandAction::Steer
            | SlashCommandAction::Queue
            | SlashCommandAction::Pending
            | SlashCommandAction::Sandbox
            | SlashCommandAction::Fork
            | SlashCommandAction::Compact
            | SlashCommandAction::Export
            | SlashCommandAction::Share
            | SlashCommandAction::Undo
            | SlashCommandAction::Redo
            | SlashCommandAction::SkillInvoke
    )
}

fn web_desktop_action_visible(action: SlashCommandAction) -> bool {
    matches!(
        action,
        SlashCommandAction::Help
            | SlashCommandAction::Status
            | SlashCommandAction::New
            | SlashCommandAction::Sessions
            | SlashCommandAction::Resume
            | SlashCommandAction::Usage
            | SlashCommandAction::Context
            | SlashCommandAction::Diff
            | SlashCommandAction::Steer
            | SlashCommandAction::Queue
            | SlashCommandAction::Pending
            | SlashCommandAction::Sandbox
            | SlashCommandAction::Fork
            | SlashCommandAction::Compact
            | SlashCommandAction::Export
            | SlashCommandAction::Share
            | SlashCommandAction::Undo
            | SlashCommandAction::Redo
            | SlashCommandAction::SkillInvoke
    )
}

pub(super) fn command_completion_detail(command: &AvailableSlashCommand) -> String {
    let destination = match command.presentation.destination.as_str() {
        "commands" => "Panel",
        "history" => "History",
        "agents" => "Agents",
        "status" => "Status",
        "preview" => "Preview",
        "composer" => "Prompt",
        "download" => "Download",
        _ => "Command",
    };
    format!("{destination} - {}", command.summary)
}

fn command_alternate_action(
    presentation: CommandPresentation,
) -> Option<wire::CommandAlternateAction> {
    presentation
        .alternate_action
        .map(|action| wire::CommandAlternateAction {
            action_type: action.action_type.as_str().to_string(),
            target: action.target.to_string(),
            label: action.label.to_string(),
        })
}

fn command_argument_kind(kind: CommandArgumentKind) -> &'static str {
    match kind {
        CommandArgumentKind::None => "none",
        CommandArgumentKind::RequiredValue => "required_value",
        CommandArgumentKind::OptionalValue => "optional_value",
        CommandArgumentKind::FixedEnumValue => "fixed_enum_value",
        CommandArgumentKind::FreeFormTrailingText => "free_form_trailing_text",
        CommandArgumentKind::DynamicSuffixOptionalText => "dynamic_suffix_optional_text",
    }
}

pub(super) fn gateway_command_capabilities() -> Vec<CommandCapability> {
    vec![
        CommandCapability::Picker,
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
