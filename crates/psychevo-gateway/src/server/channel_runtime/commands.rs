use super::paths::channel_cwd;
use super::state::ChannelInteractionKind;
use super::*;
use crate::server::commands::record_gateway_mission_metadata_for_parent;
use sha2::{Digest, Sha256};

pub(super) enum ChannelCommandAction {
    Reply(String),
    SubmitPrompt {
        text: String,
        thread_id: Option<String>,
    },
    Compact {
        instructions: Option<String>,
    },
}

struct ChannelCommandContext<'a> {
    state: &'a WebState,
    runtime: &'a ChannelRuntimeState,
    connection: &'a ChannelRuntimeConnection,
    source: &'a GatewaySource,
    scope: &'a ResolvedScope,
    raw: &'a str,
}

pub(super) async fn route_channel_command(
    state: &WebState,
    runtime: &ChannelRuntimeState,
    connection: &ChannelRuntimeConnection,
    message: &ImInboundMessage,
    source: &GatewaySource,
) -> psychevo_runtime::Result<Option<ChannelCommandAction>> {
    let text = message.text.trim();
    let Some((command, args)) = parse_channel_command(text) else {
        return Ok(None);
    };
    let reply = match command.as_str() {
        "stop" => {
            let interrupted = state
                .inner
                .gateway
                .interrupt_turn(GatewayThreadSelector::source(source.source_key()));
            if interrupted {
                "Stop requested for this channel thread.".to_string()
            } else {
                "No active turn is running for this channel thread.".to_string()
            }
        }
        "approve" | "allow" => {
            let token = args.split_whitespace().next().unwrap_or("");
            channel_permission_reply(
                state,
                runtime,
                connection,
                source,
                token,
                PermissionApprovalDecision::allow_once(),
                "approve",
            )
        }
        "deny" => {
            let token = args.split_whitespace().next().unwrap_or("");
            channel_permission_reply(
                state,
                runtime,
                connection,
                source,
                token,
                PermissionApprovalDecision::deny(),
                "deny",
            )
        }
        "answer" => {
            let (token, answer) = split_first_arg(args);
            if token.is_empty() || answer.is_empty() {
                "Usage: /answer <token> <answer>".to_string()
            } else if runtime
                .clarify_question_count(&connection.id, &source.source_key(), token)
                .is_some_and(|question_count| question_count > 1)
            {
                channel_multi_question_guidance(token)
            } else if let Some(route) = runtime.take_interaction_token(
                &connection.id,
                &source.source_key(),
                ChannelInteractionKind::Clarify,
                token,
            ) && submit_channel_clarify(
                state,
                &route,
                source,
                ClarifyResult::Answered(ClarifyResponse {
                    answers: vec![ClarifyAnswer {
                        answers: vec![answer.to_string()],
                    }],
                }),
            ) {
                format!("Answered request {token}.")
            } else {
                "No matching Ask request token.".to_string()
            }
        }
        "cancel" => {
            let token = args.split_whitespace().next().unwrap_or("");
            if token.is_empty() {
                "Usage: /cancel <token>".to_string()
            } else if let Some(route) = runtime.take_interaction_token(
                &connection.id,
                &source.source_key(),
                ChannelInteractionKind::Clarify,
                token,
            ) && submit_channel_clarify(state, &route, source, ClarifyResult::Cancelled)
            {
                format!("Cancelled request {token}.")
            } else {
                "No matching Ask request token.".to_string()
            }
        }
        "profile" => channel_profile_reply(state, connection, source, args).await?,
        "reset" => reset_channel_source_reply(state, source)?,
        "" => return Ok(None),
        _ => {
            return route_shared_channel_command(state, runtime, connection, source, text);
        }
    };
    Ok(Some(ChannelCommandAction::Reply(reply)))
}

fn channel_permission_reply(
    state: &WebState,
    runtime: &ChannelRuntimeState,
    connection: &ChannelRuntimeConnection,
    source: &GatewaySource,
    token: &str,
    decision: PermissionApprovalDecision,
    command: &str,
) -> String {
    if token.is_empty() {
        return format!("Usage: /{command} <token>");
    }
    let Some(route) = runtime.take_interaction_token(
        &connection.id,
        &source.source_key(),
        ChannelInteractionKind::Permission,
        token,
    ) else {
        return "No matching permission request token.".to_string();
    };
    if submit_channel_permission(state, &route, source, decision) {
        if command == "deny" {
            format!("Denied request {token}.")
        } else {
            format!("Approved request {token}.")
        }
    } else {
        "No matching permission request token.".to_string()
    }
}

fn channel_interaction_selector(
    route: &super::state::ChannelInteractionRoute,
    source: &GatewaySource,
) -> GatewayThreadSelector {
    route
        .thread_id
        .clone()
        .map(GatewayThreadSelector::thread_id)
        .unwrap_or_else(|| GatewayThreadSelector::source(source.source_key()))
}

fn submit_channel_permission(
    state: &WebState,
    route: &super::state::ChannelInteractionRoute,
    source: &GatewaySource,
    decision: PermissionApprovalDecision,
) -> bool {
    let primary = channel_interaction_selector(route, source);
    state
        .inner
        .gateway
        .submit_permission(primary, &route.action_id, decision.clone())
        || (route.thread_id.is_some()
            && state.inner.gateway.submit_permission(
                GatewayThreadSelector::source(source.source_key()),
                &route.action_id,
                decision,
            ))
}

fn submit_channel_clarify(
    state: &WebState,
    route: &super::state::ChannelInteractionRoute,
    source: &GatewaySource,
    result: ClarifyResult,
) -> bool {
    let primary = channel_interaction_selector(route, source);
    state
        .inner
        .gateway
        .submit_clarify(primary, &route.action_id, result.clone())
        || (route.thread_id.is_some()
            && state.inner.gateway.submit_clarify(
                GatewayThreadSelector::source(source.source_key()),
                &route.action_id,
                result,
            ))
}

fn route_shared_channel_command(
    state: &WebState,
    runtime: &ChannelRuntimeState,
    connection: &ChannelRuntimeConnection,
    source: &GatewaySource,
    text: &str,
) -> psychevo_runtime::Result<Option<ChannelCommandAction>> {
    let scope = channel_resolved_scope(state, connection, source)?;
    let context = ChannelCommandContext {
        state,
        runtime,
        connection,
        source,
        scope: &scope,
        raw: text,
    };
    let thread_id = state.inner.gateway.resolve_source_thread(source)?;
    let active_turn = state.activity(source, thread_id.as_deref()).running;
    let dynamic = dynamic_slash_commands(state, &scope)?;
    let action = match parse_slash_command_line(text) {
        SlashCommandParse::Known(invocation) => {
            let action = invocation.spec.action;
            if !channel_action_visible(action) {
                return Ok(Some(ChannelCommandAction::Reply(
                    invocation
                        .spec
                        .unavailable_guidance(SlashCommandSurface::Messaging),
                )));
            }
            match slash_invocation_effect(
                &invocation,
                &channel_command_capabilities(),
                SlashCommandSurface::Messaging,
                active_turn,
            ) {
                Ok(effect) => channel_command_action_from_effect(&context, action, effect)?,
                Err(message) => ChannelCommandAction::Reply(message),
            }
        }
        SlashCommandParse::Unknown { command, args, .. } => {
            if let Some(effect) = dynamic_slash_command_effect(&command, &args, &dynamic) {
                channel_command_action_from_effect(
                    &context,
                    SlashCommandAction::SkillInvoke,
                    effect,
                )?
            } else {
                ChannelCommandAction::Reply(format!(
                    "Unsupported channel command /{}. Send /help for available commands.",
                    command
                ))
            }
        }
        SlashCommandParse::NotSlash => return Ok(None),
    };
    Ok(Some(action))
}

fn channel_command_action_from_effect(
    context: &ChannelCommandContext<'_>,
    action: SlashCommandAction,
    effect: SlashCommandEffect,
) -> psychevo_runtime::Result<ChannelCommandAction> {
    let action = match effect {
        SlashCommandEffect::LocalText => match action {
            SlashCommandAction::Help => ChannelCommandAction::Reply(channel_help_text(
                context.state,
                context.scope,
                context.source,
                context.connection,
                context.runtime,
            )?),
            SlashCommandAction::Status => ChannelCommandAction::Reply(channel_status_text(
                context.state,
                context.runtime,
                context.connection,
                context.source,
            )?),
            _ => ChannelCommandAction::Reply(format!(
                "{} is not available as channel text output yet.",
                context.raw.split_whitespace().next().unwrap_or(context.raw)
            )),
        },
        SlashCommandEffect::NewSession => {
            ChannelCommandAction::Reply(reset_channel_source_reply(context.state, context.source)?)
        }
        SlashCommandEffect::PassThroughPrompt(text)
        | SlashCommandEffect::SubmitPrompt(text)
        | SlashCommandEffect::Queue(text)
        | SlashCommandEffect::Fork(text) => ChannelCommandAction::SubmitPrompt {
            text,
            thread_id: None,
        },
        SlashCommandEffect::Mission { prompt, team, goal } => {
            let thread_id = ensure_channel_mission_thread(context)?;
            record_gateway_mission_metadata_for_parent(
                context.state,
                context.scope,
                &thread_id,
                team.as_deref(),
                &goal,
                "channel:/mission",
            )?;
            ChannelCommandAction::SubmitPrompt {
                text: prompt,
                thread_id: Some(thread_id),
            }
        }
        SlashCommandEffect::Compact { instructions } => {
            ChannelCommandAction::Compact { instructions }
        }
        SlashCommandEffect::Steer(text) => {
            let message = RuntimeMessage::User {
                content: vec![UserContentBlock::text(text)],
                timestamp_ms: gateway_now_ms(),
            };
            let accepted = context.state.inner.gateway.steer_foreign_turn(
                GatewayThreadSelector::source(context.source.source_key()),
                None,
                message,
            );
            ChannelCommandAction::Reply(if accepted {
                "Steer message sent to the active channel turn.".to_string()
            } else {
                "No active channel turn accepted the steer message.".to_string()
            })
        }
        SlashCommandEffect::PendingCancel => {
            let selector = GatewayThreadSelector::source(context.source.source_key());
            let cleared = context.state.inner.gateway.clear_queue(selector.clone());
            let interrupted = context.state.inner.gateway.interrupt_turn(selector);
            ChannelCommandAction::Reply(format!(
                "Pending work updated: interrupted={}, cleared queued turns={}.",
                interrupted, cleared
            ))
        }
        SlashCommandEffect::Voice(mode) => {
            ChannelCommandAction::Reply(channel_voice_reply(context.state, context.source, &mode)?)
        }
        SlashCommandEffect::SandboxShow => {
            let thread_id = context
                .state
                .inner
                .gateway
                .resolve_source_thread(context.source)?;
            let options = context
                .state
                .run_options(context.scope.cwd.clone(), thread_id);
            ChannelCommandAction::Reply(psychevo_runtime::sandbox_status_text(
                &options,
                RunMode::Default,
            )?)
        }
        SlashCommandEffect::Skills { .. } => {
            ChannelCommandAction::Reply(channel_skills_text(context.state, context.scope)?)
        }
        SlashCommandEffect::Agents => {
            ChannelCommandAction::Reply(channel_agents_text(context.state, context.scope)?)
        }
        SlashCommandEffect::Unsupported(message) => ChannelCommandAction::Reply(message),
        SlashCommandEffect::Diff
        | SlashCommandEffect::SessionsList
        | SlashCommandEffect::ResumeSession { .. }
        | SlashCommandEffect::Btw { .. }
        | SlashCommandEffect::ShowModel
        | SlashCommandEffect::SetModel { .. }
        | SlashCommandEffect::SetVariant(_)
        | SlashCommandEffect::SetMode(_)
        | SlashCommandEffect::PermissionsShow
        | SlashCommandEffect::PermissionAdd { .. }
        | SlashCommandEffect::PermissionRemove { .. }
        | SlashCommandEffect::ToolsShow
        | SlashCommandEffect::ToolsetSet { .. }
        | SlashCommandEffect::Rename(_)
        | SlashCommandEffect::Undo
        | SlashCommandEffect::Redo
        | SlashCommandEffect::Bundles { .. }
        | SlashCommandEffect::Curator { .. }
        | SlashCommandEffect::Export { .. }
        | SlashCommandEffect::Share { .. } => ChannelCommandAction::Reply(format!(
            "{} is not available on messaging channels yet.",
            context.raw.split_whitespace().next().unwrap_or(context.raw)
        )),
    };
    Ok(action)
}

fn ensure_channel_mission_thread(
    context: &ChannelCommandContext<'_>,
) -> psychevo_runtime::Result<String> {
    if let Some(thread_id) = context
        .state
        .inner
        .gateway
        .resolve_source_thread(context.source)?
    {
        return Ok(thread_id);
    }
    let thread_id = context
        .state
        .inner
        .state
        .store()
        .create_session_with_metadata(&context.scope.cwd, "channel", "pending", "pending", None)?;
    bind_source_to_thread(context.state, context.scope, &thread_id)?;
    Ok(thread_id)
}

fn channel_command_capabilities() -> Vec<CommandCapability> {
    vec![
        CommandCapability::ActiveTurnControl,
        CommandCapability::Queue,
    ]
}

fn channel_action_visible(action: SlashCommandAction) -> bool {
    matches!(
        action,
        SlashCommandAction::Help
            | SlashCommandAction::Status
            | SlashCommandAction::New
            | SlashCommandAction::Steer
            | SlashCommandAction::Queue
            | SlashCommandAction::Pending
            | SlashCommandAction::Sandbox
            | SlashCommandAction::Skills
            | SlashCommandAction::Agents
            | SlashCommandAction::Mission
            | SlashCommandAction::Compact
            | SlashCommandAction::Voice
            | SlashCommandAction::SkillInvoke
    )
}

fn channel_help_text(
    state: &WebState,
    scope: &ResolvedScope,
    source: &GatewaySource,
    connection: &ChannelRuntimeConnection,
    runtime: &ChannelRuntimeState,
) -> psychevo_runtime::Result<String> {
    let thread_id = state.inner.gateway.resolve_source_thread(source)?;
    let active_turn = state.activity(source, thread_id.as_deref()).running;
    let dynamic = dynamic_slash_commands(state, scope)?;
    let available = available_slash_commands_for_surface(
        &channel_command_capabilities(),
        active_turn,
        &dynamic,
        32,
    );
    let mut lines = vec![format!("Channel {} commands:", connection.label.trim())];
    for command in available
        .commands
        .iter()
        .filter(|command| channel_action_visible(command.action))
        .take(16)
    {
        lines.push(format!("/{} - {}", command.name, command.summary));
    }
    if available.hidden_dynamic > 0 {
        lines.push(format!(
            "...and {} more skill commands.",
            available.hidden_dynamic
        ));
    }
    lines.push(
        "Controls: /stop, /reset, /profile, /approve <token>, /deny <token>, /answer <token> <text>, /cancel <token>."
            .to_string(),
    );
    lines.push(channel_status_text(state, runtime, connection, source)?);
    Ok(lines.join("\n"))
}

fn channel_voice_reply(
    state: &WebState,
    source: &GatewaySource,
    mode: &str,
) -> psychevo_runtime::Result<String> {
    let mode = match mode {
        "status" => voice_policy_for_source(state, source),
        "on" => {
            update_voice_policy_for_source(state, source, wire::VoicePolicyMode::VoiceOnly);
            wire::VoicePolicyMode::VoiceOnly
        }
        "tts" => {
            update_voice_policy_for_source(state, source, wire::VoicePolicyMode::All);
            wire::VoicePolicyMode::All
        }
        "off" => {
            update_voice_policy_for_source(state, source, wire::VoicePolicyMode::Off);
            wire::VoicePolicyMode::Off
        }
        _ => {
            return Err(Error::Message(
                "usage: /voice <on|tts|off|status>".to_string(),
            ));
        }
    };
    Ok(match mode {
        wire::VoicePolicyMode::Off => "Voice replies are off.".to_string(),
        wire::VoicePolicyMode::VoiceOnly => {
            "Voice replies will follow voice inputs. Text fallback remains active.".to_string()
        }
        wire::VoicePolicyMode::All => {
            "Voice replies are on for all replies. Text fallback remains active.".to_string()
        }
    })
}

fn channel_status_text(
    state: &WebState,
    runtime: &ChannelRuntimeState,
    connection: &ChannelRuntimeConnection,
    source: &GatewaySource,
) -> psychevo_runtime::Result<String> {
    let runner = runtime.runner_view(&connection.id);
    let thread = state
        .inner
        .gateway
        .resolve_source_thread(source)?
        .unwrap_or_else(|| "none".to_string());
    let runtime_ref = channel_effective_runtime_ref(state, connection, source)?;
    Ok(format!(
        "Channel {} is {}{}; config {}; runtime {}; thread {}.",
        connection.label,
        runner.state,
        runner
            .reason
            .as_deref()
            .map(|reason| format!(" ({reason})"))
            .unwrap_or_default(),
        connection.config_status,
        runtime_ref,
        thread
    ))
}

async fn channel_profile_reply(
    state: &WebState,
    connection: &ChannelRuntimeConnection,
    source: &GatewaySource,
    args: &str,
) -> psychevo_runtime::Result<String> {
    let (subcommand, rest) = split_first_arg(args);
    let scope = channel_resolved_scope(state, connection, source)?;
    match subcommand {
        "" | "status" => channel_profile_status_text(state, connection, source, &scope),
        "list" => channel_profile_list_text(state, &scope),
        "use" => {
            let requested = rest.split_whitespace().next().unwrap_or("");
            if requested.is_empty() {
                return Ok("Usage: /profile use <id>".to_string());
            }
            let profiles = runtime_profile_list_result(state, &scope)?.profiles;
            let Some(profile) = profiles.iter().find(|profile| profile.id == requested) else {
                return Ok(format!("Unknown Runtime Profile `{requested}`."));
            };
            if !profile.enabled {
                return Ok(format!("Runtime Profile `{requested}` is disabled."));
            }
            match channel_bind_runtime_ref(state, source, requested)? {
                Some(thread_id) => Ok(format!(
                    "Started a new channel thread ({thread_id}) with Runtime Profile `{requested}`. The previous thread is unchanged."
                )),
                None => Ok(format!(
                    "Runtime Profile `{requested}` is saved for the next channel thread."
                )),
            }
        }
        "reset" => {
            let runtime_ref = connection
                .runtime_ref
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("native");
            match channel_bind_runtime_ref(state, source, runtime_ref)? {
                Some(thread_id) => Ok(format!(
                    "Started a new channel thread ({thread_id}) with the default Runtime Profile `{runtime_ref}`. The previous thread is unchanged."
                )),
                None => Ok(format!(
                    "Default Runtime Profile `{runtime_ref}` is saved for the next channel thread."
                )),
            }
        }
        "sessions" => channel_profile_sessions_text(state, connection, source, &scope).await,
        "resume" => {
            let short_handle = rest.split_whitespace().next().unwrap_or("");
            if short_handle.is_empty() {
                return Ok("Usage: /profile resume <short-handle>".to_string());
            }
            let runtime_ref = channel_effective_runtime_ref(state, connection, source)?;
            let sessions = runtime_session_list_result_live(
                state,
                &scope,
                wire::RuntimeSessionListParams {
                    runtime_ref: Some(runtime_ref.clone()),
                    cursor: None,
                    scope: Some(scope.to_wire_scope()),
                },
            )
            .await?;
            if !sessions.supported {
                return Ok(format!(
                    "Runtime Profile `{runtime_ref}` does not expose resumable sessions here."
                ));
            }
            let matches = sessions
                .sessions
                .iter()
                .filter(|session| {
                    runtime_session_short_handle(&runtime_ref, session) == short_handle
                })
                .collect::<Vec<_>>();
            let [session] = matches.as_slice() else {
                return Ok(if matches.is_empty() {
                    format!(
                        "Unknown session handle `{short_handle}` for Runtime Profile `{runtime_ref}`."
                    )
                } else {
                    format!(
                        "Session handle `{short_handle}` is ambiguous; open the GUI to choose a session."
                    )
                });
            };
            if session.ownership == wire::RuntimeSessionOwnershipView::Active {
                return Ok(format!(
                    "Session `{short_handle}` is active and cannot be taken over from a Channel. Open it in the GUI or Fork it."
                ));
            }
            let native_session_id = session.native_session_id.clone();
            let result = runtime_session_resume_result_live(
                state,
                &scope,
                wire::RuntimeSessionParams {
                    runtime_ref: runtime_ref.clone(),
                    native_session_id,
                    scope: Some(scope.to_wire_scope()),
                },
            )
            .await?;
            if result.supported && result.changed {
                Ok(format!(
                    "Resumed `{short_handle}` on Runtime Profile `{runtime_ref}`."
                ))
            } else {
                Ok(result.message.unwrap_or_else(|| {
                    format!("Runtime Profile `{runtime_ref}` cannot resume native sessions here.")
                }))
            }
        }
        _ => Ok(
            "Usage: /profile [list|status|sessions|use <id>|resume <short-handle>|reset]"
                .to_string(),
        ),
    }
}

async fn channel_profile_sessions_text(
    state: &WebState,
    connection: &ChannelRuntimeConnection,
    source: &GatewaySource,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<String> {
    let runtime_ref = channel_effective_runtime_ref(state, connection, source)?;
    let sessions = runtime_session_list_result_live(
        state,
        scope,
        wire::RuntimeSessionListParams {
            runtime_ref: Some(runtime_ref.clone()),
            cursor: None,
            scope: Some(scope.to_wire_scope()),
        },
    )
    .await?;
    if !sessions.supported {
        return Ok(format!(
            "Runtime Profile `{runtime_ref}` does not expose resumable sessions here."
        ));
    }
    if sessions.sessions.is_empty() {
        return Ok(format!(
            "No sessions are available for Runtime Profile `{runtime_ref}`."
        ));
    }
    let mut lines = vec![format!("Sessions for Runtime Profile `{runtime_ref}`:")];
    for session in sessions.sessions.iter().take(20) {
        let handle = runtime_session_short_handle(&runtime_ref, session);
        let title = channel_runtime_session_title(session);
        let ownership = match session.ownership {
            wire::RuntimeSessionOwnershipView::ReadWrite => "resumable",
            wire::RuntimeSessionOwnershipView::ReadOnly => "read-only",
            wire::RuntimeSessionOwnershipView::Active => "active; GUI or Fork only",
        };
        let fidelity = match session.fidelity {
            wire::RuntimeHistoryFidelityView::Full => "full history",
            wire::RuntimeHistoryFidelityView::Summary => "summary history",
            wire::RuntimeHistoryFidelityView::Partial => "partial history",
        };
        let archived = if session.archived { " · archived" } else { "" };
        lines.push(format!(
            "{handle}: {title} · {ownership} · {fidelity}{archived}"
        ));
    }
    if sessions.sessions.len() > 20 || sessions.next_cursor.is_some() {
        lines.push("More sessions are available in Workbench.".to_string());
    }
    lines.push("Resume with /profile resume <short-handle>.".to_string());
    Ok(lines.join("\n"))
}

fn channel_runtime_session_title(session: &wire::RuntimeSessionView) -> String {
    let title = session.title.as_deref().unwrap_or_default().trim();
    if title.is_empty()
        || (!session.native_session_id.is_empty() && title.contains(&session.native_session_id))
        || (!session.native_dedup_key.is_empty() && title.contains(&session.native_dedup_key))
    {
        return "Untitled session".to_string();
    }
    let single_line = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = single_line.chars();
    let bounded = chars.by_ref().take(80).collect::<String>();
    if chars.next().is_some() {
        format!("{bounded}…")
    } else {
        bounded
    }
}

fn runtime_session_short_handle(runtime_ref: &str, session: &wire::RuntimeSessionView) -> String {
    let digest = Sha256::digest(
        format!(
            "channel-session\0{runtime_ref}\0{}",
            session.native_dedup_key
        )
        .as_bytes(),
    );
    format!("rs_{:x}", digest).chars().take(15).collect()
}

fn channel_profile_status_text(
    state: &WebState,
    connection: &ChannelRuntimeConnection,
    source: &GatewaySource,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<String> {
    let runtime_ref = channel_effective_runtime_ref(state, connection, source)?;
    let profiles = runtime_profile_list_result(state, scope)?.profiles;
    let Some(profile) = profiles.iter().find(|profile| profile.id == runtime_ref) else {
        return Ok(format!(
            "Runtime Profile `{runtime_ref}` is not configured."
        ));
    };
    Ok(format!(
        "Runtime Profile `{}`: {} ({}) - {}. Use /profile sessions to list resumable handles.",
        profile.id, profile.label, profile.runtime, profile.health.summary
    ))
}

fn channel_profile_list_text(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<String> {
    let profiles = runtime_profile_list_result(state, scope)?.profiles;
    let enabled_count = profiles.iter().filter(|profile| profile.enabled).count();
    let mut lines = vec!["Runtime Profiles:".to_string()];
    for profile in profiles.iter().filter(|profile| profile.enabled).take(20) {
        lines.push(format!(
            "{} - {} ({}, {})",
            profile.id, profile.label, profile.runtime, profile.health.status
        ));
    }
    if enabled_count > 20 {
        lines.push("...and more.".to_string());
    }
    Ok(lines.join("\n"))
}

fn channel_skills_text(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<String> {
    let dynamic = dynamic_slash_commands(state, scope)?;
    if dynamic.is_empty() {
        return Ok("No channel-available skills found for this workspace.".to_string());
    }
    let mut lines = vec!["Channel-available skills:".to_string()];
    for command in dynamic.iter().take(20) {
        lines.push(format!("/{} - {}", command.name, command.summary));
    }
    if dynamic.len() > 20 {
        lines.push(format!("...and {} more.", dynamic.len() - 20));
    }
    Ok(lines.join("\n"))
}

fn channel_agents_text(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<String> {
    let catalog = discover_gateway_agents(state, scope)?;
    let mut callable = Vec::new();
    let mut peer_only = Vec::new();
    for agent in catalog.agents {
        if agent.supports_entrypoint(AgentEntrypoint::Subagent) {
            callable.push(agent);
        } else if agent.supports_entrypoint(AgentEntrypoint::Peer) {
            peer_only.push(agent);
        }
    }
    if callable.is_empty() && peer_only.is_empty() {
        return Ok("No channel-callable agents found for this workspace.".to_string());
    }

    let mut lines = Vec::new();
    if callable.is_empty() {
        lines.push("No channel-callable agents found for this workspace.".to_string());
        lines.push("Add a project agent with entrypoints: [subagent] to call it here.".to_string());
    } else {
        lines.push("Callable agents:".to_string());
    }
    for agent in callable.iter().take(20) {
        lines.push(format!("@{} - {}", agent.name, agent.description));
    }
    if callable.len() > 20 {
        lines.push(format!("...and {} more.", callable.len() - 20));
    }
    if !callable.is_empty() {
        lines.push("Use @agent-name followed by a task.".to_string());
    }

    if !peer_only.is_empty() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push("Peer runtimes:".to_string());
        for agent in peer_only.iter().take(20) {
            lines.push(format!("@{} - {}", agent.name, agent.description));
        }
        if peer_only.len() > 20 {
            lines.push(format!("...and {} more.", peer_only.len() - 20));
        }
        lines.push(
            "Peer runtimes are listed for visibility; use callable agents for @agent delegation in channels."
                .to_string(),
        );
    }
    Ok(lines.join("\n"))
}

fn reset_channel_source_reply(
    state: &WebState,
    source: &GatewaySource,
) -> psychevo_runtime::Result<String> {
    let previous = state.inner.gateway.reset_source_to_empty(source)?;
    Ok(if previous.is_some() {
        "Started a new channel thread. The next message will use this channel's current default workspace.".to_string()
    } else {
        "No channel thread was active. The next message will start a new channel thread."
            .to_string()
    })
}

fn channel_resolved_scope(
    state: &WebState,
    connection: &ChannelRuntimeConnection,
    source: &GatewaySource,
) -> psychevo_runtime::Result<ResolvedScope> {
    let cwd = match state.inner.gateway.resolve_source_thread(source)? {
        Some(thread_id) => state
            .inner
            .state
            .store()
            .session_summary(&thread_id)?
            .map(|summary| PathBuf::from(summary.cwd))
            .unwrap_or_else(|| channel_cwd(&state.inner.cwd, connection)),
        None => channel_cwd(&state.inner.cwd, connection),
    };
    Ok(ResolvedScope {
        cwd,
        source: source.clone(),
    })
}

fn parse_channel_command(text: &str) -> Option<(String, &str)> {
    let text = text.trim();
    let command_line = text.strip_prefix('/')?;
    let split_at = command_line
        .find(char::is_whitespace)
        .unwrap_or(command_line.len());
    let (token, args) = command_line.split_at(split_at);
    let command = token.split('@').next().unwrap_or("").to_ascii_lowercase();
    Some((command, args.trim()))
}

fn split_first_arg(value: &str) -> (&str, &str) {
    let value = value.trim();
    let split_at = value.find(char::is_whitespace).unwrap_or(value.len());
    let (first, rest) = value.split_at(split_at);
    (first, rest.trim())
}

#[cfg(test)]
mod runtime_session_handle_tests {
    use super::*;

    fn session(native_dedup_key: &str) -> wire::RuntimeSessionView {
        wire::RuntimeSessionView {
            native_session_id: "raw-native-id".to_string(),
            thread_id: None,
            title: None,
            archived: false,
            updated_at_ms: None,
            parent_thread_id: None,
            status: None,
            native_dedup_key: native_dedup_key.to_string(),
            fidelity: wire::RuntimeHistoryFidelityView::Full,
            ownership: wire::RuntimeSessionOwnershipView::ReadWrite,
            actions: Vec::new(),
        }
    }

    #[test]
    fn channel_session_handle_is_opaque_stable_and_runtime_scoped() {
        let first = runtime_session_short_handle("codex", &session("native-session-secret"));
        assert_eq!(
            first,
            runtime_session_short_handle("codex", &session("native-session-secret"))
        );
        assert_ne!(
            first,
            runtime_session_short_handle("opencode", &session("native-session-secret"))
        );
        assert!(first.starts_with("rs_"));
        assert!(!first.contains("native"));
        assert_eq!(first.len(), 15);
    }

    #[test]
    fn channel_session_title_never_echoes_native_identity() {
        let mut raw = session("native-session-secret");
        raw.native_session_id = "native-session-secret".to_string();
        raw.title = Some("native-session-secret".to_string());
        assert_eq!(channel_runtime_session_title(&raw), "Untitled session");

        raw.title = Some("Review\nthis workspace".to_string());
        assert_eq!(channel_runtime_session_title(&raw), "Review this workspace");
    }
}
