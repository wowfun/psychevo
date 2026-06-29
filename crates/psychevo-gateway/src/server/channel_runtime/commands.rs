use super::paths::channel_cwd;
use super::*;

pub(super) enum ChannelCommandAction {
    Reply(String),
    SubmitPrompt(String),
}

struct ChannelCommandContext<'a> {
    state: &'a WebState,
    runtime: &'a ChannelRuntimeState,
    connection: &'a ChannelRuntimeConnection,
    source: &'a GatewaySource,
    scope: &'a ResolvedScope,
    raw: &'a str,
}

pub(super) fn route_channel_command(
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
            let request_id = args.split_whitespace().next().unwrap_or("");
            if request_id.is_empty() {
                "Usage: /approve <request_id>".to_string()
            } else if state.inner.gateway.submit_permission(
                GatewayThreadSelector::source(source.source_key()),
                request_id,
                PermissionApprovalDecision::allow_once(),
            ) {
                format!("Approved request {request_id}.")
            } else {
                format!("No matching permission request for {request_id}.")
            }
        }
        "deny" => {
            let request_id = args.split_whitespace().next().unwrap_or("");
            if request_id.is_empty() {
                "Usage: /deny <request_id>".to_string()
            } else if state.inner.gateway.submit_permission(
                GatewayThreadSelector::source(source.source_key()),
                request_id,
                PermissionApprovalDecision::deny(),
            ) {
                format!("Denied request {request_id}.")
            } else {
                format!("No matching permission request for {request_id}.")
            }
        }
        "answer" => {
            let (request_id, answer) = split_first_arg(args);
            if request_id.is_empty() || answer.is_empty() {
                "Usage: /answer <request_id> <answer>".to_string()
            } else if state.inner.gateway.submit_clarify(
                GatewayThreadSelector::source(source.source_key()),
                request_id,
                ClarifyResult::Answered(ClarifyResponse {
                    answers: vec![ClarifyAnswer {
                        answers: vec![answer.to_string()],
                    }],
                }),
            ) {
                format!("Answered request {request_id}.")
            } else {
                format!("No matching Ask request for {request_id}.")
            }
        }
        "cancel" => {
            let request_id = args.split_whitespace().next().unwrap_or("");
            if request_id.is_empty() {
                "Usage: /cancel <request_id>".to_string()
            } else if state.inner.gateway.submit_clarify(
                GatewayThreadSelector::source(source.source_key()),
                request_id,
                ClarifyResult::Cancelled,
            ) {
                format!("Cancelled request {request_id}.")
            } else {
                format!("No matching Ask request for {request_id}.")
            }
        }
        "reset" => reset_channel_source_reply(state, source)?,
        "" => return Ok(None),
        _ => {
            return route_shared_channel_command(state, runtime, connection, source, text);
        }
    };
    Ok(Some(ChannelCommandAction::Reply(reply)))
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
        | SlashCommandEffect::Fork(text) => ChannelCommandAction::SubmitPrompt(text),
        SlashCommandEffect::Compact { instructions } => {
            ChannelCommandAction::SubmitPrompt(compact_prompt_text(instructions))
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
            | SlashCommandAction::Compact
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
        "Controls: /stop, /reset, /approve <id>, /deny <id>, /answer <id> <text>, /cancel <id>."
            .to_string(),
    );
    lines.push(channel_status_text(state, runtime, connection, source)?);
    Ok(lines.join("\n"))
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
    Ok(format!(
        "Channel {} is {}{}; config {}; thread {}.",
        connection.label,
        runner.state,
        runner
            .reason
            .as_deref()
            .map(|reason| format!(" ({reason})"))
            .unwrap_or_default(),
        connection.config_status,
        thread
    ))
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
