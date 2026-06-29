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
