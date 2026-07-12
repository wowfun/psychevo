use super::paths::channel_cwd;
use super::state::ChannelInteractionKind;
use super::*;
use crate::server::commands::record_gateway_mission_metadata_for_parent;

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
            match run_channel_thread_action(
                state,
                runtime,
                connection,
                source,
                wire::ThreadActionInput::Interrupt,
            )
            .await
            {
                Ok(wire::ThreadActionRunResult::Interrupt {
                    interrupted,
                    cleared,
                    ..
                }) if interrupted || cleared > 0 => {
                    "Stop requested for this channel thread.".to_string()
                }
                _ => "No active turn is running for this channel thread.".to_string(),
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
                PermissionDecision::AllowOnce,
                "approve",
            )?
        }
        "deny" => {
            let token = args.split_whitespace().next().unwrap_or("");
            channel_permission_reply(
                state,
                runtime,
                connection,
                source,
                token,
                PermissionDecision::Deny,
                "deny",
            )?
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
            } else if let Some(route) = runtime.interaction_route(
                &connection.id,
                &source.source_key(),
                ChannelInteractionKind::Clarify,
                token,
            ) {
                match submit_channel_interaction(
                    state,
                    source,
                    &route,
                    GatewayActionKind::Clarify,
                    wire::ThreadInteractionResponse::Clarify {
                        answers: vec![vec![answer.to_string()]],
                    },
                ) {
                    Ok(true) => {
                        runtime.consume_interaction_token(
                            &connection.id,
                            &source.source_key(),
                            ChannelInteractionKind::Clarify,
                            token,
                            &route.action_id,
                        );
                        format!("Answered request {token}.")
                    }
                    Ok(false) => format!("Request {token} was not accepted."),
                    Err(error) => format!("Request {token} was not accepted: {error}"),
                }
            } else {
                "No matching Ask request token.".to_string()
            }
        }
        "cancel" => {
            let token = args.split_whitespace().next().unwrap_or("");
            if token.is_empty() {
                "Usage: /cancel <token>".to_string()
            } else if let Some(route) = runtime.interaction_route(
                &connection.id,
                &source.source_key(),
                ChannelInteractionKind::Clarify,
                token,
            ) {
                match submit_channel_interaction(
                    state,
                    source,
                    &route,
                    GatewayActionKind::Clarify,
                    wire::ThreadInteractionResponse::CancelClarify,
                ) {
                    Ok(true) => {
                        runtime.consume_interaction_token(
                            &connection.id,
                            &source.source_key(),
                            ChannelInteractionKind::Clarify,
                            token,
                            &route.action_id,
                        );
                        format!("Cancelled request {token}.")
                    }
                    Ok(false) => format!("Request {token} was not accepted."),
                    Err(error) => format!("Request {token} was not accepted: {error}"),
                }
            } else {
                "No matching Ask request token.".to_string()
            }
        }
        "agent" => channel_agent_reply(state, connection, source, args).await?,
        "profile" => channel_profile_reply(state, connection, source, args).await?,
        "reset" => reset_channel_source_reply(state, source)?,
        "" => return Ok(None),
        _ => {
            return route_shared_channel_command(state, runtime, connection, source, text).await;
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
    decision: PermissionDecision,
    command: &str,
) -> psychevo_runtime::Result<String> {
    if token.is_empty() {
        return Ok(format!("Usage: /{command} <token>"));
    }
    let Some(route) = runtime.interaction_route(
        &connection.id,
        &source.source_key(),
        ChannelInteractionKind::Permission,
        token,
    ) else {
        return Ok("No matching permission request token.".to_string());
    };
    let response = wire::ThreadInteractionResponse::Permission { decision };
    match submit_channel_interaction(
        state,
        source,
        &route,
        GatewayActionKind::Permission,
        response,
    ) {
        Ok(true) => {
            runtime.consume_interaction_token(
                &connection.id,
                &source.source_key(),
                ChannelInteractionKind::Permission,
                token,
                &route.action_id,
            );
            if command == "deny" {
                Ok(format!("Denied request {token}."))
            } else {
                Ok(format!("Approved request {token}."))
            }
        }
        Ok(false) => Ok(format!("Request {token} was not accepted.")),
        Err(error) => Ok(format!("Request {token} was not accepted: {error}")),
    }
}

fn channel_interaction_thread_id(
    state: &WebState,
    route: &super::state::ChannelInteractionRoute,
    source: &GatewaySource,
) -> psychevo_runtime::Result<String> {
    route
        .thread_id
        .clone()
        .or(state.inner.gateway.resolve_source_thread(source)?)
        .ok_or_else(|| {
            Error::Message("The interaction is not bound to a public Thread.".to_string())
        })
}

fn submit_channel_interaction(
    state: &WebState,
    source: &GatewaySource,
    route: &super::state::ChannelInteractionRoute,
    expected_kind: GatewayActionKind,
    response: wire::ThreadInteractionResponse,
) -> psychevo_runtime::Result<bool> {
    channel_interaction_thread_id(state, route, source)?;
    thread_routed_interaction_respond_for_selector(
        state,
        GatewayThreadSelector::source(source.source_key()),
        &route.action_id,
        expected_kind,
        response,
    )
    .map(|result| result.accepted)
}

async fn route_shared_channel_command(
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
                Ok(effect) => channel_command_action_from_effect(&context, action, effect).await?,
                Err(message) => ChannelCommandAction::Reply(message),
            }
        }
        SlashCommandParse::Unknown { command, args, .. } => {
            if let Some(effect) = dynamic_slash_command_effect(&command, &args, &dynamic) {
                channel_command_action_from_effect(
                    &context,
                    SlashCommandAction::SkillInvoke,
                    effect,
                )
                .await?
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

async fn channel_command_action_from_effect(
    context: &ChannelCommandContext<'_>,
    action: SlashCommandAction,
    effect: SlashCommandEffect,
) -> psychevo_runtime::Result<ChannelCommandAction> {
    let action = match effect {
        SlashCommandEffect::LocalText => match action {
            SlashCommandAction::Help => {
                ChannelCommandAction::Reply(channel_help_text(context).await?)
            }
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
            let expected_turn_id = context
                .state
                .activity(
                    context.source,
                    context
                        .state
                        .inner
                        .gateway
                        .resolve_source_thread(context.source)?
                        .as_deref(),
                )
                .active_turn_id;
            let accepted = if let Some(expected_turn_id) = expected_turn_id {
                matches!(
                    run_channel_thread_action(
                        context.state,
                        context.runtime,
                        context.connection,
                        context.source,
                        wire::ThreadActionInput::Steer {
                            expected_turn_id,
                            text,
                        },
                    )
                    .await,
                    Ok(wire::ThreadActionRunResult::Steer { accepted: true, .. })
                )
            } else {
                false
            };
            ChannelCommandAction::Reply(if accepted {
                "Steer message sent to the active channel turn.".to_string()
            } else {
                "No active channel turn accepted the steer message.".to_string()
            })
        }
        SlashCommandEffect::PendingCancel => {
            let (interrupted, cleared) = match run_channel_thread_action(
                context.state,
                context.runtime,
                context.connection,
                context.source,
                wire::ThreadActionInput::Interrupt,
            )
            .await
            {
                Ok(wire::ThreadActionRunResult::Interrupt {
                    interrupted,
                    cleared,
                    ..
                }) => (interrupted, cleared),
                _ => (false, 0),
            };
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
        SlashCommandEffect::ShowModel => ChannelCommandAction::Reply(
            channel_runtime_control_reply(context, wire::ThreadControlSurfaceRoleView::Model, None)
                .await?,
        ),
        SlashCommandEffect::SetModel { model, variant } => {
            let mut reply = channel_runtime_control_reply(
                context,
                wire::ThreadControlSurfaceRoleView::Model,
                Some(&model),
            )
            .await?;
            if let Some(variant) = variant {
                let variant_reply = channel_runtime_control_reply(
                    context,
                    wire::ThreadControlSurfaceRoleView::Reasoning,
                    Some(&variant),
                )
                .await?;
                reply.push('\n');
                reply.push_str(&variant_reply);
            }
            ChannelCommandAction::Reply(reply)
        }
        SlashCommandEffect::SetVariant(variant) => ChannelCommandAction::Reply(
            channel_runtime_control_reply(
                context,
                wire::ThreadControlSurfaceRoleView::Reasoning,
                Some(&variant),
            )
            .await?,
        ),
        SlashCommandEffect::SetMode(mode) => ChannelCommandAction::Reply(
            channel_runtime_control_reply(
                context,
                wire::ThreadControlSurfaceRoleView::Mode,
                Some(&mode),
            )
            .await?,
        ),
        SlashCommandEffect::Unsupported(message) => ChannelCommandAction::Reply(message),
        SlashCommandEffect::Diff
        | SlashCommandEffect::SessionsList
        | SlashCommandEffect::ResumeSession { .. }
        | SlashCommandEffect::Btw { .. }
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

async fn channel_runtime_control_reply(
    context: &ChannelCommandContext<'_>,
    role: wire::ThreadControlSurfaceRoleView,
    requested: Option<&str>,
) -> psychevo_runtime::Result<String> {
    let (runtime_ref, runtime_context) =
        channel_runtime_context(context).await.map_err(|error| {
            Error::Message(format!(
                "Runtime context is unavailable for {}: {}",
                channel_runtime_control_command_role_label(role),
                redact_channel_error(&error.to_string())
            ))
        })?;
    let Some(control) = runtime_context.controls.into_iter().find(|control| {
        control.surface_role == role
            && control.stability == wire::RuntimeStabilityView::Stable
            && control.channel_safe
    }) else {
        return Ok(format!(
            "Runtime Profile `{runtime_ref}` does not expose a stable, channel-safe {} control.",
            channel_runtime_control_command_role_label(role)
        ));
    };

    let current = control
        .effective_value
        .as_ref()
        .and_then(Value::as_str)
        .unwrap_or("runtime default");
    let choices = control
        .choices
        .iter()
        .filter_map(|choice| choice.value.as_str())
        .collect::<Vec<_>>();
    let Some(requested) = requested.map(str::trim).filter(|value| !value.is_empty()) else {
        let choices = if choices.is_empty() {
            String::new()
        } else {
            format!(" Choices: {}.", choices.join(", "))
        };
        return Ok(format!(
            "{} is `{current}` for Runtime Profile `{runtime_ref}`.{choices}",
            control.label
        ));
    };

    if control.mutability != wire::ThreadControlMutabilityView::Selectable {
        return Ok(format!(
            "{} is read-only for this runtime session. Send /new, then set it before the next prompt.",
            control.label
        ));
    }
    let Some(choice) = control
        .choices
        .iter()
        .find(|choice| choice.value.as_str() == Some(requested))
    else {
        return Ok(if choices.is_empty() {
            format!("{} does not advertise selectable values.", control.label)
        } else {
            format!(
                "Unknown {} `{requested}`. Choose one of: {}.",
                channel_runtime_control_role_label(role),
                choices.join(", ")
            )
        });
    };

    let value = choice
        .value
        .as_str()
        .expect("channel runtime controls only expose string choices");
    let binding = runtime_context.binding;
    if binding.is_some() {
        if control.apply_scope != wire::ThreadControlApplyScopeView::Session {
            return Ok(format!(
                "{} applies when a thread starts. Send /new, set it, then send the next prompt.",
                control.label
            ));
        }
    } else if control.apply_scope != wire::ThreadControlApplyScopeView::TurnDraft {
        return Ok(format!(
            "{} requires a bound runtime session; send a prompt first, then retry.",
            control.label
        ));
    }
    let result = thread_control_set_result(
        context.state,
        context.scope,
        wire::ThreadControlSetParams {
            thread_id: binding.as_ref().map(|binding| binding.thread_id.clone()),
            target_id: runtime_context.target_id.clone(),
            control_id: control.id.clone(),
            value: Value::String(value.to_string()),
            expected_capability_revision: control.capability_revision.clone(),
            expected_binding_revision: binding
                .as_ref()
                .map(|binding| binding.binding_revision)
                .unwrap_or_default(),
            expected_context_revision: runtime_context.context_revision,
            expected_control_revision: runtime_context.control_revision,
            scope: Some(context.scope.to_wire_scope()),
        },
    )
    .await?;
    Ok(if binding.is_some() {
        if result.changed {
            format!("{} is now `{value}`.", control.label)
        } else {
            format!("{} is already `{value}`.", control.label)
        }
    } else {
        format!(
            "{} `{value}` is saved for the next channel thread.",
            control.label
        )
    })
}

async fn channel_runtime_context(
    context: &ChannelCommandContext<'_>,
) -> psychevo_runtime::Result<(String, wire::ThreadContextReadResult)> {
    let default_runtime_ref = context
        .connection
        .runtime_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("native");
    let target = runnable_target_for_source(
        context.state,
        context.scope,
        context.source,
        default_runtime_ref,
    )?;
    let runtime_ref = target.runtime_profile_ref.clone();
    let thread_id = context
        .state
        .inner
        .gateway
        .resolve_source_thread(context.source)?;
    let runtime_context = thread_context_read_result_for_target_id(
        context.state,
        context.scope,
        thread_id,
        &target.target_id,
    )
    .await?;
    Ok((runtime_ref, runtime_context))
}

pub(super) async fn run_channel_thread_action(
    state: &WebState,
    runtime: &ChannelRuntimeState,
    connection: &ChannelRuntimeConnection,
    source: &GatewaySource,
    action: wire::ThreadActionInput,
) -> psychevo_runtime::Result<wire::ThreadActionRunResult> {
    let thread_id = state
        .inner
        .gateway
        .resolve_source_thread(source)?
        .or_else(|| {
            state
                .activity(source, None)
                .running
                .then(|| runtime.observed_source_thread(&connection.id, &source.source_key()))
                .flatten()
        });
    let Some(thread_id) = thread_id else {
        return Err(Error::Message(
            "No public Thread is bound to this channel source.".to_string(),
        ));
    };
    let scope = channel_resolved_scope(state, connection, source)?;
    let (out_tx, _out_rx) = mpsc::unbounded_channel();
    run_routed_thread_action(
        state,
        &scope,
        wire::ThreadActionRunParams {
            scope: scope.to_wire_scope(),
            thread_id,
            action,
        },
        out_tx,
    )
    .await
}

fn channel_runtime_control_role_label(role: wire::ThreadControlSurfaceRoleView) -> &'static str {
    match role {
        wire::ThreadControlSurfaceRoleView::Mode => "mode",
        wire::ThreadControlSurfaceRoleView::Model => "model",
        wire::ThreadControlSurfaceRoleView::Reasoning => "reasoning variant",
        wire::ThreadControlSurfaceRoleView::Advanced => "advanced",
    }
}

fn channel_runtime_control_command_role_label(
    role: wire::ThreadControlSurfaceRoleView,
) -> &'static str {
    match role {
        wire::ThreadControlSurfaceRoleView::Mode => "mode",
        wire::ThreadControlSurfaceRoleView::Model => "model",
        wire::ThreadControlSurfaceRoleView::Reasoning => "variant",
        wire::ThreadControlSurfaceRoleView::Advanced => "advanced",
    }
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
            | SlashCommandAction::ModelShow
            | SlashCommandAction::VariantSet
            | SlashCommandAction::ModeSet
            | SlashCommandAction::SkillInvoke
    )
}

async fn channel_help_text(
    context: &ChannelCommandContext<'_>,
) -> psychevo_runtime::Result<String> {
    let thread_id = context
        .state
        .inner
        .gateway
        .resolve_source_thread(context.source)?;
    let active_turn = context
        .state
        .activity(context.source, thread_id.as_deref())
        .running;
    let dynamic = dynamic_slash_commands(context.state, context.scope)?;
    let available = available_slash_commands_for_surface(
        &channel_command_capabilities(),
        active_turn,
        &dynamic,
        32,
    );
    let (_, runtime_context) = channel_runtime_context(context).await?;
    let stable_channel_roles = runtime_context
        .controls
        .into_iter()
        .filter(|control| {
            control.stability == wire::RuntimeStabilityView::Stable
                && control.channel_safe
                && matches!(
                    control.surface_role,
                    wire::ThreadControlSurfaceRoleView::Model
                        | wire::ThreadControlSurfaceRoleView::Reasoning
                        | wire::ThreadControlSurfaceRoleView::Mode
                )
        })
        .map(|control| control.surface_role)
        .collect::<Vec<_>>();
    let mut lines = vec![format!(
        "Channel {} commands:",
        context.connection.label.trim()
    )];
    for command in available
        .commands
        .iter()
        .filter(|command| channel_help_action_visible(command.action, &stable_channel_roles))
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
    let mut controls = Vec::new();
    if stable_channel_roles.contains(&wire::ThreadControlSurfaceRoleView::Model) {
        controls.push("/model");
    }
    if stable_channel_roles.contains(&wire::ThreadControlSurfaceRoleView::Reasoning) {
        controls.push("/variant <value>");
    }
    if stable_channel_roles.contains(&wire::ThreadControlSurfaceRoleView::Mode) {
        controls.push("/mode <value>");
    }
    controls.extend([
        "/stop",
        "/reset",
        "/profile",
        "/approve <token>",
        "/deny <token>",
        "/answer <token> <text>",
        "/cancel <token>",
    ]);
    lines.push(format!("Controls: {}.", controls.join(", ")));
    lines.push(channel_status_text(
        context.state,
        context.runtime,
        context.connection,
        context.source,
    )?);
    Ok(lines.join("\n"))
}

fn channel_help_action_visible(
    action: SlashCommandAction,
    stable_channel_roles: &[wire::ThreadControlSurfaceRoleView],
) -> bool {
    if !channel_action_visible(action) {
        return false;
    }
    let required_role = match action {
        SlashCommandAction::ModelShow => Some(wire::ThreadControlSurfaceRoleView::Model),
        SlashCommandAction::VariantSet => Some(wire::ThreadControlSurfaceRoleView::Reasoning),
        SlashCommandAction::ModeSet => Some(wire::ThreadControlSurfaceRoleView::Mode),
        _ => None,
    };
    required_role.is_none_or(|role| stable_channel_roles.contains(&role))
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
    let thread = state.inner.gateway.resolve_source_thread(source)?;
    let profile_ref = channel_effective_profile_ref(state, connection, source)?;
    let history = channel_history_status(state, thread.as_deref())?;
    Ok(format!(
        "Channel {} is {}{}; config {}; profile {}; thread {}; history {}.",
        connection.label,
        runner.state,
        runner
            .reason
            .as_deref()
            .map(|reason| format!(" ({reason})"))
            .unwrap_or_default(),
        connection.config_status,
        profile_ref,
        thread.as_deref().unwrap_or("none"),
        history,
    ))
}

fn channel_history_status(
    _state: &WebState,
    thread_id: Option<&str>,
) -> psychevo_runtime::Result<String> {
    let Some(_thread_id) = thread_id else {
        return Ok("unavailable".to_string());
    };
    Ok("psychevo/full".to_string())
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
            let target =
                match runnable_target_for_source_profile(state, &scope, source, Some(requested)) {
                    Ok(target) => target,
                    Err(_) if !matches!(profile.health.status.as_str(), "ready" | "unchecked") => {
                        return Ok(profile.health.summary.clone());
                    }
                    Err(error) => return Err(error),
                };
            if !target.ready {
                return Ok(target.unavailable_reason.unwrap_or_else(|| {
                    format!("Agent target `{}` is unavailable.", target.label)
                }));
            }
            match channel_bind_target_draft(state, source, &target)? {
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
            let target =
                runnable_target_for_source_profile(state, &scope, source, Some(runtime_ref))?;
            if !target.ready {
                return Ok(target.unavailable_reason.unwrap_or_else(|| {
                    format!("Agent target `{}` is unavailable.", target.label)
                }));
            }
            match channel_bind_target_draft(state, source, &target)? {
                Some(thread_id) => Ok(format!(
                    "Started a new channel thread ({thread_id}) with the default Runtime Profile `{runtime_ref}`. The previous thread is unchanged."
                )),
                None => Ok(format!(
                    "Default Runtime Profile `{runtime_ref}` is saved for the next channel thread."
                )),
            }
        }
        _ => Ok("Usage: /profile [list|status|use <id>|reset]".to_string()),
    }
}

async fn channel_agent_reply(
    state: &WebState,
    connection: &ChannelRuntimeConnection,
    source: &GatewaySource,
    args: &str,
) -> psychevo_runtime::Result<String> {
    let scope = channel_resolved_scope(state, connection, source)?;
    let default_runtime_ref = connection
        .runtime_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("native");
    let selected_target = runnable_target_for_source(state, &scope, source, default_runtime_ref)?;
    let runtime_ref = selected_target.runtime_profile_ref.clone();
    let thread_id = state.inner.gateway.resolve_source_thread(source)?;
    let context = thread_context_read_result_for_target_id(
        state,
        &scope,
        thread_id,
        &selected_target.target_id,
    )
    .await?;
    let candidates = context
        .compatible_targets
        .iter()
        .filter(|target| target.runtime_profile_ref == runtime_ref)
        .collect::<Vec<_>>();
    let requested = args.trim();
    if requested.is_empty() || requested == "list" || requested == "status" {
        let selected = context
            .binding
            .as_ref()
            .and_then(|binding| binding.agent_ref.clone())
            .or(channel_draft_agent_ref(state, source)?)
            .unwrap_or_else(|| "default".to_string());
        let mut lines = vec![format!(
            "Top-level Agent for Runtime Profile `{runtime_ref}`: `{selected}`."
        )];
        for target in candidates
            .iter()
            .filter(|target| target.agent_ref.is_some())
        {
            let status = if target.ready { "ready" } else { "unavailable" };
            lines.push(format!(
                "{} - {} ({status}){}",
                target.agent_ref.as_deref().unwrap_or("default"),
                target.label,
                target
                    .unavailable_reason
                    .as_deref()
                    .map(|reason| format!(": {reason}"))
                    .unwrap_or_default()
            ));
        }
        lines.push(
            "Use /agent <name> or /agent reset. /agents still lists callable subagents."
                .to_string(),
        );
        return Ok(lines.join("\n"));
    }
    let requested = requested.strip_prefix("use ").unwrap_or(requested).trim();
    let requested_agent = (!matches!(requested, "reset" | "default" | "none")).then_some(requested);
    let target = candidates
        .iter()
        .find(|target| target.agent_ref.as_deref() == requested_agent);
    let Some(target) = target else {
        return Ok(format!(
            "Agent `{}` is not compatible with Runtime Profile `{runtime_ref}`. Send /agent to list compatible targets.",
            requested_agent.unwrap_or("default")
        ));
    };
    if !target.ready {
        return Ok(target
            .unavailable_reason
            .clone()
            .unwrap_or_else(|| format!("{} is unavailable.", target.label)));
    }
    let new_thread = channel_bind_target_draft(state, source, target)?;
    Ok(match new_thread {
        Some(thread_id) => format!(
            "Started a new channel thread ({thread_id}) with top-level Agent `{}` and Runtime Profile `{runtime_ref}`. The previous thread is unchanged.",
            target.agent_ref.as_deref().unwrap_or("default")
        ),
        None => format!(
            "Top-level Agent `{}` is saved for the next channel thread with Runtime Profile `{runtime_ref}`.",
            target.agent_ref.as_deref().unwrap_or("default")
        ),
    })
}

fn channel_profile_status_text(
    state: &WebState,
    connection: &ChannelRuntimeConnection,
    source: &GatewaySource,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<String> {
    let profile_ref = channel_effective_profile_ref(state, connection, source)?;
    let profiles = runtime_profile_list_result(state, scope)?.profiles;
    let Some(profile) = profiles.iter().find(|profile| profile.id == profile_ref) else {
        return Ok(format!(
            "Runtime Profile `{profile_ref}` is not configured."
        ));
    };
    Ok(format!(
        "Runtime Profile `{}`: {} ({}) - {}.",
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

pub(super) fn channel_resolved_scope(
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
