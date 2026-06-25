use super::*;

const SIDE_CONVERSATION_NO_SESSION_MESSAGE: &str = "'/btw' is unavailable until the current conversation has started. Send a message first, then try /btw again.";
type GatewaySlashConfig = SharedSlashConfig;
type GatewaySlashAlias = SharedSlashAlias;
type GatewaySlashKeybind = SharedSlashKeybind;

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
    let slash_config = effective_slash_config(state, scope)?;
    let expanded = slash_config.expand_alias_line(&raw);
    let parse_line = expanded.as_deref().unwrap_or(&raw);
    let active_turn = thread_id
        .as_deref()
        .map(|thread_id| state.activity(&scope.source, Some(thread_id)).running)
        .unwrap_or_else(|| state.activity(&scope.source, None).running);
    let dynamic = dynamic_slash_commands(state, scope)?;
    let result = match parse_slash_command_line(parse_line) {
        SlashCommandParse::Known(invocation) => {
            let action = invocation.spec.action;
            let has_session = thread_id.is_some();
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
            } else if matches!(action, SlashCommandAction::Btw) && thread_id.is_none() {
                command_unsupported(
                    &raw,
                    action,
                    SIDE_CONVERSATION_NO_SESSION_MESSAGE.to_string(),
                )
            } else {
                match slash_invocation_effect(
                    &invocation,
                    &gateway_command_capabilities(has_session),
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
            if expanded.is_some() {
                return Ok(serde_json::to_value(command_rejected_known(
                    &raw,
                    Some(format!(
                        "slash alias expands to unsupported command: {original}"
                    )),
                ))?);
            }
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
        SlashCommandEffect::Export { args } => Ok(command_download_action(
            raw,
            action,
            SessionArtifactKind::Export,
            args,
            thread_id,
        )),
        SlashCommandEffect::Share { args } => Ok(command_download_action(
            raw,
            action,
            SessionArtifactKind::Share,
            args,
            thread_id,
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
        SlashCommandEffect::Btw { prompt } => {
            command_side_conversation_start(state, scope, raw, action, thread_id, prompt)
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

fn command_download_action(
    raw: &str,
    action: SlashCommandAction,
    artifact_kind: SessionArtifactKind,
    args: Option<String>,
    thread_id: Option<String>,
) -> wire::CommandExecuteResult {
    let usage = match action {
        SlashCommandAction::Export => {
            psychevo_runtime::command_registry::slash_command_spec("/export")
                .map(|spec| spec.usage)
                .unwrap_or("/export [path] [-f|--format markdown|json] [-i|--include list]")
        }
        SlashCommandAction::Share => {
            psychevo_runtime::command_registry::slash_command_spec("/share")
                .map(|spec| spec.usage)
                .unwrap_or("/share [path] [-i|--include list]")
        }
        _ => unreachable!("download action is only used for export/share"),
    };
    let parsed = match parse_session_export_command_args(
        args.as_deref().unwrap_or(""),
        artifact_kind,
        usage,
    ) {
        Ok(parsed) => parsed,
        Err(err) => return command_unsupported(raw, action, err.to_string()),
    };
    let mut payload = json!({
        "type": "downloadSession",
        "kind": artifact_kind.as_str(),
        "threadId": thread_id,
        "format": parsed.format.as_str(),
        "include": parsed.include.tokens(),
    });
    if let Some(filename) = parsed
        .path
        .as_deref()
        .and_then(|path| sanitize_download_filename_hint(path, parsed.format))
    {
        payload["filename"] = json!(filename);
    }
    command_action(raw, action, payload)
}

fn sanitize_download_filename_hint(path: &str, format: SessionExportFormat) -> Option<String> {
    let basename = path.rsplit(['/', '\\']).next().unwrap_or(path).trim();
    if basename.is_empty() || basename == "." || basename == ".." {
        return None;
    }
    let sanitized = basename
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches(['.', '_', '-'])
        .chars()
        .take(180)
        .collect::<String>();
    if sanitized.is_empty() {
        return None;
    }
    Some(filename_with_format_extension(&sanitized, format))
}

fn filename_with_format_extension(filename: &str, format: SessionExportFormat) -> String {
    let extension = format.extension();
    let lower = filename.to_ascii_lowercase();
    let stem = if let Some(stripped) = lower
        .ends_with(".json")
        .then(|| filename.strip_suffix(&filename[filename.len() - 5..]))
        .flatten()
    {
        stripped
    } else if let Some(stripped) = lower
        .ends_with(".markdown")
        .then(|| filename.strip_suffix(&filename[filename.len() - 9..]))
        .flatten()
    {
        stripped
    } else if let Some(stripped) = lower
        .ends_with(".md")
        .then(|| filename.strip_suffix(&filename[filename.len() - 3..]))
        .flatten()
    {
        stripped
    } else {
        filename
    };
    format!("{stem}.{extension}")
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

fn command_rejected_known(raw: &str, message: Option<String>) -> wire::CommandExecuteResult {
    wire::CommandExecuteResult {
        accepted: false,
        command: raw.to_string(),
        known: Some(true),
        presentation_kind: None,
        feedback_anchor: Some("composer".to_string()),
        alternate_action: None,
        message,
        action: None,
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
        SlashCommandAction::Btw => SIDE_CONVERSATION_NO_SESSION_MESSAGE.to_string(),
        _ => format!("{command} is not available in Web/Desktop."),
    }
}

pub(super) fn compact_prompt_text(instructions: Option<String>) -> String {
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

fn command_side_conversation_start(
    state: &WebState,
    scope: &ResolvedScope,
    raw: &str,
    action: SlashCommandAction,
    parent_thread_id: Option<String>,
    prompt: Option<String>,
) -> psychevo_runtime::Result<wire::CommandExecuteResult> {
    let Some(parent_thread_id) = parent_thread_id else {
        return Ok(command_unsupported(
            raw,
            action,
            SIDE_CONVERSATION_NO_SESSION_MESSAGE.to_string(),
        ));
    };
    let summary = state
        .inner
        .state
        .store()
        .session_summary(&parent_thread_id)?
        .ok_or_else(|| Error::Message(format!("session not found: {parent_thread_id}")))?;
    if Path::new(&summary.workdir) != scope.workdir.as_path() {
        return Ok(command_unsupported(
            raw,
            action,
            format!(
                "session {parent_thread_id} does not belong to {}",
                scope.workdir.display()
            ),
        ));
    }

    let options = state.run_options(scope.workdir.clone(), Some(parent_thread_id.clone()));
    let side_thread_id = state
        .inner
        .state
        .store()
        .create_child_session_from_parent_snapshot(ChildSessionSnapshotInput {
            parent_session_id: &parent_thread_id,
            workdir: &scope.workdir,
            source: WEB_SIDE_CONVERSATION_SESSION_SOURCE,
            model: &summary.model,
            provider: &summary.provider,
            metadata: Some(json!({
                SIDE_CONVERSATION_METADATA_KEY: {
                    "ephemeral": true,
                    "parent_session_id": parent_thread_id.clone(),
                },
                "provider_label": summary.provider.clone(),
            })),
            max_context_messages: options.max_context_messages,
            inherited_message_metadata: json!({
                SIDE_INHERITED_METADATA_KEY: {
                    "hidden": true,
                    "parent_session_id": parent_thread_id.clone(),
                }
            }),
            boundary_text: side_conversation_boundary_prompt(),
        })?;
    Ok(command_action(
        raw,
        action,
        json!({
            "type": "sideConversationStart",
            "threadId": side_thread_id,
            "parentThreadId": parent_thread_id,
            "title": "Side chat",
            "prompt": prompt,
        }),
    ))
}

pub(super) fn command_list_value(
    state: &WebState,
    scope: &ResolvedScope,
    active_turn: bool,
    has_session: bool,
) -> psychevo_runtime::Result<Value> {
    Ok(serde_json::to_value(command_list_result(
        state,
        scope,
        active_turn,
        has_session,
        256,
    )?)?)
}

pub(super) fn command_list_result(
    state: &WebState,
    scope: &ResolvedScope,
    active_turn: bool,
    has_session: bool,
    cap: usize,
) -> psychevo_runtime::Result<wire::CommandListResult> {
    let dynamic = dynamic_slash_commands(state, scope)?;
    let dynamic_names = dynamic
        .iter()
        .map(|command| command.name.trim_start_matches('/').to_string())
        .collect::<std::collections::BTreeSet<_>>();
    let slash_config = effective_slash_config(state, scope)?;
    let available = available_slash_commands_for_surface(
        &gateway_command_capabilities(has_session),
        active_turn,
        &dynamic,
        cap,
    );
    let mut commands = available
        .commands
        .iter()
        .filter(|command| web_desktop_command_visible(command))
        .map(|command| command_value(command, &dynamic_names))
        .collect::<Vec<_>>();
    commands.extend(custom_alias_command_values(
        &slash_config,
        &gateway_command_capabilities(has_session),
        active_turn,
    ));
    Ok(wire::CommandListResult {
        commands,
        hidden_dynamic: available.hidden_dynamic,
    })
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
        expands_to: None,
        presentation_kind: Some(presentation.kind.as_str().to_string()),
        destination: Some(presentation.destination.as_str().to_string()),
        feedback_anchor: Some(presentation.feedback_anchor.as_str().to_string()),
        alternate_action: command_alternate_action(presentation),
    }
}

fn custom_alias_command_values(
    slash_config: &GatewaySlashConfig,
    capabilities: &[CommandCapability],
    active_turn: bool,
) -> Vec<wire::CommandListItem> {
    slash_config
        .aliases
        .iter()
        .filter_map(|alias| custom_alias_command_value(alias, capabilities, active_turn))
        .collect()
}

fn custom_alias_command_value(
    alias: &GatewaySlashAlias,
    capabilities: &[CommandCapability],
    active_turn: bool,
) -> Option<wire::CommandListItem> {
    let invocation = match parse_slash_command_line(&alias.target) {
        SlashCommandParse::Known(invocation) => invocation,
        _ => return None,
    };
    let spec = invocation.spec;
    if !web_desktop_action_visible(spec.action)
        || !psychevo_runtime::command_registry::supported_by_capabilities(spec, capabilities)
        || (active_turn && !spec.available_during_active_turn())
    {
        return None;
    }
    let presentation = spec.presentation();
    Some(wire::CommandListItem {
        name: alias.alias.trim_start_matches('/').to_string(),
        slash: alias.alias.clone(),
        usage: format!("{} [args]", alias.alias),
        summary: format!("alias for {}", alias.target),
        aliases: Vec::new(),
        argument_kind: command_argument_kind(spec.argument_kind).to_string(),
        source: "custom".to_string(),
        expands_to: Some(alias.target.clone()),
        presentation_kind: Some(presentation.kind.as_str().to_string()),
        destination: Some(presentation.destination.as_str().to_string()),
        feedback_anchor: Some(presentation.feedback_anchor.as_str().to_string()),
        alternate_action: command_alternate_action(presentation),
    })
}

pub(super) fn slash_settings_read_value(
    state: &WebState,
    scope: &ResolvedScope,
    workdir: &Path,
) -> psychevo_runtime::Result<Value> {
    Ok(serde_json::to_value(slash_settings_result(
        state,
        scope,
        workdir,
        profile_slash_config(state, scope)?,
        Vec::new(),
    )?)?)
}

pub(super) fn slash_settings_update_value(
    state: &WebState,
    scope: &ResolvedScope,
    workdir: &Path,
    params: wire::SlashSettingsUpdateParams,
) -> psychevo_runtime::Result<Value> {
    if params.scope != wire::ModelSettingsScope::Global {
        return Err(Error::Config(
            "slash settings writes support only global scope".to_string(),
        ));
    }
    let config = slash_config_from_update(params)?;
    let config_dir = active_profile_config_dir(state, scope);
    let aliases = slash_aliases_config_value(&config.aliases);
    let keybinds = slash_keybinds_config_value(&config.keybinds);
    set_config_value(
        config_dir.clone(),
        "tui.leader_key",
        json!(config.leader_key),
    )?;
    set_config_value(
        config_dir.clone(),
        "tui.leader_timeout_ms",
        json!(config.leader_timeout_ms),
    )?;
    set_config_value(config_dir.clone(), "tui.slash_aliases", aliases)?;
    set_config_value(config_dir, "tui.slash_keybinds", keybinds)?;
    Ok(serde_json::to_value(slash_settings_result(
        state,
        scope,
        workdir,
        profile_slash_config(state, scope)?,
        Vec::new(),
    )?)?)
}

fn slash_settings_result(
    _state: &WebState,
    _scope: &ResolvedScope,
    workdir: &Path,
    config: GatewaySlashConfig,
    diagnostics: Vec<String>,
) -> psychevo_runtime::Result<wire::SlashSettingsResult> {
    Ok(wire::SlashSettingsResult {
        scope: wire::ModelSettingsScope::Global,
        workdir: workdir.display().to_string(),
        leader_key: config.leader_key,
        leader_timeout_ms: config.leader_timeout_ms,
        aliases: config
            .aliases
            .into_iter()
            .map(|entry| wire::SlashAliasSetting {
                target_summary: slash_target_summary(&entry.target),
                alias: entry.alias,
                target: entry.target,
            })
            .collect(),
        keybinds: config
            .keybinds
            .into_iter()
            .map(|entry| wire::SlashKeybindSetting {
                target_summary: slash_target_summary(&entry.target),
                shortcut: entry.shortcut,
                target: entry.target,
            })
            .collect(),
        diagnostics,
    })
}

fn effective_slash_config(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<GatewaySlashConfig> {
    let options = state.run_options(scope.workdir.clone(), None);
    let document = match config_show_value(&options, ConfigScope::Effective) {
        Ok(document) => document,
        Err(Error::Config(message)) if message.contains("home is not initialized") => {
            return Ok(default_gateway_slash_config());
        }
        Err(err) => return Err(err),
    };
    parse_gateway_slash_config(document.get("value").unwrap_or(&Value::Null))
}

fn profile_slash_config(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<GatewaySlashConfig> {
    let path = active_profile_config_dir(state, scope).join("config.toml");
    let value = read_toml_config_value(&path)?;
    parse_gateway_slash_config(&value)
}

fn read_toml_config_value(path: &Path) -> psychevo_runtime::Result<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let text = std::fs::read_to_string(path)?;
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    let parsed: toml::Value =
        toml::from_str(&text).map_err(|err| Error::Config(format!("{}: {err}", path.display())))?;
    Ok(serde_json::to_value(parsed)?)
}

fn parse_gateway_slash_config(root: &Value) -> psychevo_runtime::Result<GatewaySlashConfig> {
    parse_shared_slash_config(root)
}

fn default_gateway_slash_config() -> GatewaySlashConfig {
    GatewaySlashConfig::default()
}

fn slash_config_from_update(
    params: wire::SlashSettingsUpdateParams,
) -> psychevo_runtime::Result<GatewaySlashConfig> {
    let leader_key = match params.leader_key {
        Some(value) => parse_key_chord_display(&value, "leaderKey")?,
        None => default_gateway_slash_config().leader_key,
    };
    let leader_timeout_ms = params
        .leader_timeout_ms
        .unwrap_or_else(|| default_gateway_slash_config().leader_timeout_ms);
    if leader_timeout_ms == 0 {
        return Err(Error::Config(
            "leaderTimeoutMs must be a positive integer".to_string(),
        ));
    }
    let aliases = params
        .aliases
        .into_iter()
        .map(|entry| {
            Ok(GatewaySlashAlias {
                alias: validate_configured_alias(&entry.alias, "aliases[].alias")?,
                target: validate_configured_slash_target(&entry.target, "aliases[].target")?,
            })
        })
        .collect::<psychevo_runtime::Result<Vec<_>>>()?;
    let keybinds = params
        .keybinds
        .into_iter()
        .flat_map(|entry| {
            split_key_sequence_list(&entry.shortcut)
                .into_iter()
                .map(move |shortcut| (shortcut, entry.target.clone()))
        })
        .filter(|(shortcut, _)| !shortcut.eq_ignore_ascii_case("none"))
        .map(|(shortcut, target)| {
            Ok(GatewaySlashKeybind {
                shortcut: parse_key_sequence_display(&shortcut, "keybinds[].shortcut")?,
                target: validate_configured_slash_target(&target, "keybinds[].target")?,
            })
        })
        .collect::<psychevo_runtime::Result<Vec<_>>>()?;
    let config = GatewaySlashConfig {
        leader_key,
        leader_timeout_ms,
        aliases,
        keybinds,
    };
    validate_shared_slash_config(&config)?;
    Ok(config)
}

fn slash_aliases_config_value(aliases: &[GatewaySlashAlias]) -> Value {
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for alias in aliases {
        grouped
            .entry(alias.target.clone())
            .or_default()
            .push(alias.alias.clone());
    }
    json!(grouped)
}

fn slash_keybinds_config_value(keybinds: &[GatewaySlashKeybind]) -> Value {
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for keybind in keybinds {
        grouped
            .entry(keybind.target.clone())
            .or_default()
            .push(keybind.shortcut.clone());
    }
    json!(grouped)
}

fn slash_target_summary(target: &str) -> Option<String> {
    let (command, _) = split_slash_command_token(target);
    psychevo_runtime::command_registry::slash_command_spec(command)
        .map(|spec| spec.summary.to_string())
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
            | SlashCommandAction::Btw
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
            | SlashCommandAction::Btw
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

pub(super) fn command_item_matches(command: &wire::CommandListItem, query: &str) -> bool {
    let query = query.to_ascii_lowercase();
    query.is_empty()
        || command.name.to_ascii_lowercase().contains(&query)
        || command
            .slash
            .trim_start_matches('/')
            .to_ascii_lowercase()
            .contains(&query)
        || command
            .aliases
            .iter()
            .any(|alias| alias.to_ascii_lowercase().contains(&query))
        || command.summary.to_ascii_lowercase().contains(&query)
        || command
            .expands_to
            .as_deref()
            .is_some_and(|target| target.to_ascii_lowercase().contains(&query))
}

pub(super) fn command_item_completion_detail(command: &wire::CommandListItem) -> String {
    let destination = match command.destination.as_deref().unwrap_or("none") {
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

pub(super) fn gateway_command_capabilities(has_session: bool) -> Vec<CommandCapability> {
    let mut capabilities = vec![
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
    ];
    if has_session {
        capabilities.push(CommandCapability::SideConversation);
    }
    capabilities
}
