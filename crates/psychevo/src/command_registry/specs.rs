#[path = "specs/dynamic.rs"]
mod dynamic;
#[path = "specs/slash_catalog.rs"]
mod slash_catalog;
#[path = "specs/types_presentation.rs"]
mod types_presentation;

pub use dynamic::{
    AvailableSlashCommand, AvailableSlashCommands, DynamicSlashCommand, SlashCommandEffect,
    SlashCommandInvocation, SlashCommandParse,
};
pub use slash_catalog::SLASH_COMMANDS;
pub(crate) use types_presentation::{COMMANDS, PEVO_CLI, TUI_SLASH};
pub use types_presentation::{
    CliCommandSpec, CommandAlternateAction, CommandAlternateActionType, CommandArgumentKind,
    CommandCapability, CommandDestination, CommandFeedbackAnchor, CommandGroup, CommandOutputKind,
    CommandPresentation, CommandPresentationKind, CommandStatus, CommandSurface,
    SlashCommandAction, SlashCommandSpec, SlashCommandSurface, command_presentation,
};
