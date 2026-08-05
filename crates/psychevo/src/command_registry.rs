#[path = "command_registry/specs.rs"]
mod specs;
pub use specs::{
    AvailableSlashCommand, AvailableSlashCommands, CliCommandSpec, CommandAlternateAction,
    CommandAlternateActionType, CommandArgumentKind, CommandCapability, CommandDestination,
    CommandFeedbackAnchor, CommandGroup, CommandOutputKind, CommandPresentation,
    CommandPresentationKind, CommandStatus, CommandSurface, DynamicSlashCommand, SLASH_COMMANDS,
    SlashCommandAction, SlashCommandEffect, SlashCommandInvocation, SlashCommandParse,
    SlashCommandSpec, SlashCommandSurface, command_presentation,
};
#[path = "command_registry/parsing.rs"]
mod parsing;
pub use parsing::{
    CLI_COMMANDS, CUSTOM_SKILL_COMMAND, available_slash_commands_for_surface,
    dynamic_slash_command_effect, fork_prompt_marker, mission_prompt_marker,
    normalize_dynamic_skill_name, parse_mission_args, parse_slash_command_line,
    skill_prompt_marker, slash_command_spec, slash_invocation_effect, supported_by_capabilities,
};
#[path = "command_registry/export_args.rs"]
mod export_args;
pub use export_args::{
    ParsedSessionExportCommand, parse_session_export_command_args, parse_session_export_format,
    parse_session_export_include, split_slash_argument_tokens,
};
#[path = "command_registry/slash_config.rs"]
mod slash_config;
pub use slash_config::{
    DEFAULT_SLASH_LEADER_KEY, DEFAULT_SLASH_LEADER_TIMEOUT_MS, OBSOLETE_SLASH_COMMAND_TOKENS,
    OLD_DYNAMIC_SKILL_PREFIX, SharedSlashAlias, SharedSlashConfig, SharedSlashKeybind,
    fixed_key_chord_displays, parse_configured_alias_entries, parse_configured_keybind_entries,
    parse_key_chord_display, parse_key_sequence_display, parse_shared_slash_config,
    split_key_sequence_list, split_slash_command_token, validate_configured_alias,
    validate_configured_slash_target, validate_shared_slash_config,
};
