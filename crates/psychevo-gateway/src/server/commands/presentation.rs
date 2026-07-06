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
            | SlashCommandAction::Voice
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
            | SlashCommandAction::Voice
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
