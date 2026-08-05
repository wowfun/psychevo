use std::collections::BTreeMap;
use std::path::Path;

use psychevo::agents::{
    AgentDiscoveryOptions, discover_agent_teams_with_catalog, discover_agents,
    resolve_agent_team_definition,
};
use psychevo::application::{
    AgentMissionRegistration, AgentTeamRegistration, SideConversationAgentBindingSnapshot,
    SideConversationSurface, StartSideConversationRequest, ThreadAgentBinding,
    ThreadModelSelection,
};
use psychevo::command_registry::{
    SlashCommandAction, SlashCommandEffect, SlashCommandParse, SlashCommandSurface,
    command_presentation, dynamic_slash_command_effect, parse_session_export_command_args,
    parse_slash_command_line, slash_command_spec, slash_invocation_effect,
};
use psychevo::session_export::{SessionArtifactKind, SessionExportFormat};
use psychevo::{Error, PermissionMode, RunMode};
use psychevo_gateway_protocol as wire;
use serde_json::{Value, json};
use uuid::Uuid;

use super::super::binding::WebState;
use super::super::runtime_profiles::{
    thread_context_read_result_live, validate_and_capture_team_runtime_members,
};
use super::super::scope_session::{ResolvedScope, ensure_turn_start_thread};
use super::super::settings_observability::{dynamic_slash_commands, session_control_agent};
use super::super::voice::{update_voice_policy_for_source, voice_policy_for_source};
use super::super::workspace::workspace_diff_result;
use super::presentation::{
    command_alternate_action, gateway_command_capabilities, web_desktop_action_visible,
};
use super::settings::effective_slash_config;
use super::{SIDE_CONVERSATION_NO_SESSION_MESSAGE, SIDE_CONVERSATION_NO_TARGET_MESSAGE};

pub(in super::super) async fn command_execute_value(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::thread_command_turn::CommandExecuteParams,
) -> psychevo::Result<Value> {
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
    let active_turn = state
        .activity(&scope.source, thread_id.as_deref())
        .await
        .running;
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
                        command_result_from_effect(state, scope, &raw, action, effect, thread_id)
                            .await?
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
                )
                .await?
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

async fn command_result_from_effect(
    state: &WebState,
    scope: &ResolvedScope,
    raw: &str,
    action: SlashCommandAction,
    effect: SlashCommandEffect,
    thread_id: Option<String>,
) -> psychevo::Result<wire::thread_command_turn::CommandExecuteResult> {
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
            Ok(command_action(raw, action, json!({"type": "newSession"})))
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
            let mission_thread_id = record_gateway_mission_metadata(
                state,
                scope,
                thread_id.clone(),
                team.as_deref(),
                &goal,
            )
            .await?;
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
            command_side_conversation_start(state, scope, raw, action, thread_id, prompt).await
        }
        SlashCommandEffect::SandboxShow => {
            let mut query = psychevo::ConfigurationQuery::new(&scope.cwd);
            query.inherited_env = Some(state.inner.inherited_env.clone());
            let status = state
                .inner
                .framework
                .configuration(query)?
                .sandbox_status_text(RunMode::Default)?;
            Ok(command_accepted_message(raw, action, Some(status)))
        }
        SlashCommandEffect::Voice(mode) => {
            Ok(command_voice_result(state, scope, raw, action, &mode))
        }
        SlashCommandEffect::Undo => {
            Ok(command_session_undo(state, scope, raw, action, thread_id).await)
        }
        SlashCommandEffect::Redo => {
            Ok(command_session_redo(state, scope, raw, action, thread_id).await)
        }
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

async fn record_gateway_mission_metadata(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<String>,
    team: Option<&str>,
    goal: &str,
) -> psychevo::Result<String> {
    let parent_thread_id = ensure_turn_start_thread(state, scope, thread_id)
        .await?
        .0
        .ok_or_else(|| Error::Message("mission requires a thread context".to_string()))?;
    record_gateway_mission_metadata_for_parent(
        state,
        scope,
        &parent_thread_id,
        team,
        goal,
        "web:/mission",
    )
    .await?;
    Ok(parent_thread_id)
}

pub(crate) async fn record_gateway_mission_metadata_for_parent(
    state: &WebState,
    scope: &ResolvedScope,
    parent_thread_id: &str,
    team: Option<&str>,
    goal: &str,
    source: &str,
) -> psychevo::Result<()> {
    let mission_id = Uuid::now_v7().to_string();
    let metadata = Some(json!({"source": source}));
    let (team, lead_agent_name) =
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
            let members =
                validate_and_capture_team_runtime_members(state, scope, &agents, &team.members)?;
            let members = serde_json::to_value(&members)?;
            let source_path = team
                .file_path
                .as_ref()
                .map(|path| path.display().to_string());
            let lead_agent_name = team.leader.clone();
            (
                Some(AgentTeamRegistration {
                    id: team_id,
                    name: team.name,
                    description: Some(team.description),
                    source_path,
                    leader_agent_name: lead_agent_name.clone(),
                    members,
                    max_parallel_agents: team.max_parallel_agents,
                }),
                lead_agent_name,
            )
        } else {
            let lead_agent = session_control_agent(state, Some(parent_thread_id))
                .await?
                .unwrap_or_else(|| "general".to_string());
            (None, lead_agent)
        };
    state
        .inner
        .framework
        .resume_thread(parent_thread_id.to_string())
        .await?
        .register_agent_mission(AgentMissionRegistration {
            id: mission_id,
            goal: goal.to_string(),
            lead_agent_name,
            team,
            metadata,
        })
        .await
}

fn command_voice_result(
    state: &WebState,
    scope: &ResolvedScope,
    raw: &str,
    action: SlashCommandAction,
    mode: &str,
) -> wire::thread_command_turn::CommandExecuteResult {
    let policy = match mode {
        "status" => voice_policy_for_source(state, &scope.source),
        "on" => {
            update_voice_policy_for_source(
                state,
                &scope.source,
                wire::voice::VoicePolicyMode::VoiceOnly,
            );
            wire::voice::VoicePolicyMode::VoiceOnly
        }
        "tts" => {
            update_voice_policy_for_source(state, &scope.source, wire::voice::VoicePolicyMode::All);
            wire::voice::VoicePolicyMode::All
        }
        "off" => {
            update_voice_policy_for_source(state, &scope.source, wire::voice::VoicePolicyMode::Off);
            wire::voice::VoicePolicyMode::Off
        }
        _ => {
            return command_unsupported(
                raw,
                action,
                "usage: /voice <on|tts|off|status>".to_string(),
            );
        }
    };
    command_accepted_message(raw, action, Some(voice_policy_message(policy)))
}

fn voice_policy_message(mode: wire::voice::VoicePolicyMode) -> String {
    match mode {
        wire::voice::VoicePolicyMode::Off => "Voice replies are off.".to_string(),
        wire::voice::VoicePolicyMode::VoiceOnly => {
            "Voice replies will follow voice inputs. Text fallback remains active.".to_string()
        }
        wire::voice::VoicePolicyMode::All => {
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
) -> wire::thread_command_turn::CommandExecuteResult {
    let usage = match action {
        SlashCommandAction::Export => slash_command_spec("/export")
            .map(|spec| spec.usage)
            .unwrap_or("/export [path] [-f|--format markdown|json] [-i|--include list]"),
        SlashCommandAction::Share => slash_command_spec("/share")
            .map(|spec| spec.usage)
            .unwrap_or("/share [path] [-i|--include list]"),
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

async fn command_session_undo(
    state: &WebState,
    scope: &ResolvedScope,
    raw: &str,
    action: SlashCommandAction,
    thread_id: Option<String>,
) -> wire::thread_command_turn::CommandExecuteResult {
    let thread = match command_session_thread(state, scope, thread_id, "undo").await {
        Ok(thread) => thread,
        Err(message) => return command_unsupported(raw, action, message),
    };
    match thread.undo().await {
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

async fn command_session_redo(
    state: &WebState,
    scope: &ResolvedScope,
    raw: &str,
    action: SlashCommandAction,
    thread_id: Option<String>,
) -> wire::thread_command_turn::CommandExecuteResult {
    let thread = match command_session_thread(state, scope, thread_id, "redo").await {
        Ok(thread) => thread,
        Err(message) => return command_unsupported(raw, action, message),
    };
    match thread.redo().await {
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

async fn command_session_thread(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<String>,
    verb: &str,
) -> std::result::Result<psychevo::Thread, String> {
    let Some(thread_id) = thread_id else {
        return Err(format!("no current session to {verb}"));
    };
    let thread = state
        .inner
        .framework
        .resume_thread(&thread_id)
        .await
        .map_err(|error| error.to_string())?;
    let summary = thread.summary().await.map_err(|error| error.to_string())?;
    if Path::new(&summary.cwd) != scope.cwd.as_path() {
        return Err(format!(
            "session {thread_id} does not belong to {}",
            scope.cwd.display()
        ));
    }
    Ok(thread)
}
fn command_action(
    raw: &str,
    slash_action: SlashCommandAction,
    action: Value,
) -> wire::thread_command_turn::CommandExecuteResult {
    command_known_result(raw, slash_action, true, None, Some(action))
}

fn command_accepted_message(
    raw: &str,
    slash_action: SlashCommandAction,
    message: Option<String>,
) -> wire::thread_command_turn::CommandExecuteResult {
    command_known_result(raw, slash_action, true, message, None)
}

fn command_unsupported(
    raw: &str,
    slash_action: SlashCommandAction,
    message: String,
) -> wire::thread_command_turn::CommandExecuteResult {
    command_known_result(raw, slash_action, false, Some(message), None)
}

fn command_known_result(
    raw: &str,
    slash_action: SlashCommandAction,
    accepted: bool,
    message: Option<String>,
    action: Option<Value>,
) -> wire::thread_command_turn::CommandExecuteResult {
    let presentation = command_presentation(slash_action);
    wire::thread_command_turn::CommandExecuteResult {
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
) -> wire::thread_command_turn::CommandExecuteResult {
    wire::thread_command_turn::CommandExecuteResult {
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

fn command_rejected_known(
    raw: &str,
    message: Option<String>,
) -> wire::thread_command_turn::CommandExecuteResult {
    wire::thread_command_turn::CommandExecuteResult {
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

async fn command_side_conversation_start(
    state: &WebState,
    scope: &ResolvedScope,
    raw: &str,
    action: SlashCommandAction,
    parent_thread_id: Option<String>,
    prompt: Option<String>,
) -> psychevo::Result<wire::thread_command_turn::CommandExecuteResult> {
    let Some(parent_thread_id) = parent_thread_id else {
        return Ok(command_unsupported(
            raw,
            action,
            SIDE_CONVERSATION_NO_SESSION_MESSAGE.to_string(),
        ));
    };
    let parent_thread = state
        .inner
        .framework
        .resume_thread(&parent_thread_id)
        .await?;
    let summary = parent_thread.summary().await?;
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
    let Some(ThreadAgentBinding::Resolved {
        binding: parent_binding,
        writable: true,
        ..
    }) = parent_thread.agent_binding().await?
    else {
        return Ok(command_unsupported(
            raw,
            action,
            SIDE_CONVERSATION_NO_TARGET_MESSAGE.to_string(),
        ));
    };
    let parent_context = thread_context_read_result_live(
        state,
        scope,
        wire::agents_backend_rpc::ThreadContextReadParams {
            thread_id: Some(parent_thread_id.clone()),
            target: None,
            scope: Some(scope.to_wire_scope()),
        },
    )
    .await?;
    let effective_controls = parent_context
        .controls
        .into_iter()
        .filter_map(|control| control.effective_value.map(|value| (control.id, value)))
        .collect::<BTreeMap<_, _>>();
    let agent_binding =
        SideConversationAgentBindingSnapshot::new(&parent_binding, effective_controls);
    let side_thread = parent_thread
        .start_side_conversation(StartSideConversationRequest {
            surface: SideConversationSurface::Web,
            model: ThreadModelSelection {
                provider: summary.provider,
                model: summary.model,
                reasoning_effort: None,
            },
            mode: RunMode::Default,
            permission_mode: PermissionMode::Default,
            selected_agent: None,
            agent_binding: Some(agent_binding),
        })
        .await?;
    let side_thread_id = side_thread.id().to_string();
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
