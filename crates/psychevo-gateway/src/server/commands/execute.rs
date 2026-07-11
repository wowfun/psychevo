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
        SlashCommandEffect::Mission { prompt, team, goal } => {
            let mission_thread_id =
                record_gateway_mission_metadata(state, scope, thread_id.clone(), team.as_deref(), &goal)?;
            Ok(command_action(
                raw,
                action,
                json!({
                    "type": "submitPrompt",
                    "text": prompt,
                    "displayText": raw,
                    "threadId": mission_thread_id,
                }),
            ))
        }
        SlashCommandEffect::Compact { instructions } => Ok(command_action(
            raw,
            action,
            json!({"type": "threadCompactStart", "instructions": instructions}),
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
            let options = state.run_options(scope.cwd.clone(), thread_id.clone());
            let status = psychevo_runtime::sandbox_status_text(&options, RunMode::Default)?;
            Ok(command_accepted_message(raw, action, Some(status)))
        }
        SlashCommandEffect::Voice(mode) => Ok(command_voice_result(
            state,
            scope,
            raw,
            action,
            &mode,
        )),
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

fn record_gateway_mission_metadata(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<String>,
    team: Option<&str>,
    goal: &str,
) -> psychevo_runtime::Result<String> {
    let parent_thread_id = ensure_turn_start_thread(state, scope, thread_id)?.ok_or_else(|| {
        Error::Message("mission requires a thread context".to_string())
    })?;
    record_gateway_mission_metadata_for_parent(
        state,
        scope,
        &parent_thread_id,
        team,
        goal,
        "web:/mission",
    )?;
    Ok(parent_thread_id)
}

pub(crate) fn record_gateway_mission_metadata_for_parent(
    state: &WebState,
    scope: &ResolvedScope,
    parent_thread_id: &str,
    team: Option<&str>,
    goal: &str,
    source: &str,
) -> psychevo_runtime::Result<()> {
    let mission_id = Uuid::now_v7().to_string();
    let metadata = Some(json!({"source": source}));
    if let Some(team_name) = team.map(str::trim).filter(|team| !team.is_empty()) {
        let options = AgentDiscoveryOptions {
            home: state.inner.home.clone(),
            cwd: scope.cwd.clone(),
            env: state.inner.inherited_env.clone(),
            explicit_inputs: Vec::new(),
            no_agents: false,
        };
        let agents = discover_agents(&options)?;
        let teams = discover_agent_teams_with_catalog(&options, &agents)?;
        let team = resolve_agent_team_definition(&teams, team_name)?;
        let team_id = Uuid::now_v7().to_string();
        let members = validate_and_capture_team_runtime_members(
            state,
            scope,
            &agents,
            &team.members,
        )?;
        let members = serde_json::to_value(&members)?;
        let source_path = team
            .file_path
            .as_ref()
            .map(|path| path.display().to_string());
        state
            .inner
            .state
            .store()
            .create_agent_team_run(AgentTeamRunInput {
                id: &team_id,
                parent_session_id: parent_thread_id,
                mission_run_id: Some(&mission_id),
                team_name: &team.name,
                description: Some(&team.description),
                source_path: source_path.as_deref(),
                leader_agent_name: &team.leader,
                members,
                max_parallel_agents: team.max_parallel_agents,
                status: "running",
                metadata: metadata.clone(),
            })?;
        state
            .inner
            .state
            .store()
            .create_agent_mission_run(AgentMissionRunInput {
                id: &mission_id,
                parent_session_id: parent_thread_id,
                team_run_id: Some(&team_id),
                team_name: Some(&team.name),
                goal,
                lead_agent_name: &team.leader,
                status: "running",
                metadata,
            })?;
    } else {
        let lead_agent = session_control_agent(state, Some(parent_thread_id))?
            .unwrap_or_else(|| "general".to_string());
        state
            .inner
            .state
            .store()
            .create_agent_mission_run(AgentMissionRunInput {
                id: &mission_id,
                parent_session_id: parent_thread_id,
                team_run_id: None,
                team_name: None,
                goal,
                lead_agent_name: &lead_agent,
                status: "running",
                metadata,
            })?;
    }
    Ok(())
}

fn command_voice_result(
    state: &WebState,
    scope: &ResolvedScope,
    raw: &str,
    action: SlashCommandAction,
    mode: &str,
) -> wire::CommandExecuteResult {
    let policy = match mode {
        "status" => voice_policy_for_source(state, &scope.source),
        "on" => {
            update_voice_policy_for_source(state, &scope.source, wire::VoicePolicyMode::VoiceOnly);
            wire::VoicePolicyMode::VoiceOnly
        }
        "tts" => {
            update_voice_policy_for_source(state, &scope.source, wire::VoicePolicyMode::All);
            wire::VoicePolicyMode::All
        }
        "off" => {
            update_voice_policy_for_source(state, &scope.source, wire::VoicePolicyMode::Off);
            wire::VoicePolicyMode::Off
        }
        _ => return command_unsupported(raw, action, "usage: /voice <on|tts|off|status>".to_string()),
    };
    command_accepted_message(raw, action, Some(voice_policy_message(policy)))
}

fn voice_policy_message(mode: wire::VoicePolicyMode) -> String {
    match mode {
        wire::VoicePolicyMode::Off => "Voice replies are off.".to_string(),
        wire::VoicePolicyMode::VoiceOnly => {
            "Voice replies will follow voice inputs. Text fallback remains active.".to_string()
        }
        wire::VoicePolicyMode::All => {
            "Voice replies are on for all replies. Text fallback remains active.".to_string()
        }
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
    if Path::new(&summary.cwd) != scope.cwd.as_path() {
        return Err(format!(
            "session {thread_id} does not belong to {}",
            scope.cwd.display()
        ));
    }
    Ok(SessionUndoOptions {
        state: state.inner.state.clone(),
        cwd: scope.cwd.clone(),
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
    if Path::new(&summary.cwd) != scope.cwd.as_path() {
        return Ok(command_unsupported(
            raw,
            action,
            format!(
                "session {parent_thread_id} does not belong to {}",
                scope.cwd.display()
            ),
        ));
    }

    let options = state.run_options(scope.cwd.clone(), Some(parent_thread_id.clone()));
    let side_thread_id = state
        .inner
        .state
        .store()
        .create_child_session_from_parent_snapshot(ChildSessionSnapshotInput {
            parent_session_id: &parent_thread_id,
            cwd: &scope.cwd,
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
