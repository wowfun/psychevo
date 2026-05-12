#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandSurface {
    TuiSlash,
    PevoCli,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandGroup {
    Commands,
    CustomCommands,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandArgumentKind {
    None,
    RequiredValue,
    OptionalValue,
    FixedEnumValue,
    FreeFormTrailingText,
    DynamicSuffixOptionalText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandOutputKind {
    TranscriptStatusBlock,
    BottomSelectionPane,
    BottomHelpPane,
    ImmediateStateChange,
    PromptSubmission,
    ProcessResult,
    BoundedFeedback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandStatus {
    Active,
    Upcoming,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlashCommandAction {
    Help,
    Quit,
    Status,
    New,
    Sessions,
    Usage,
    Context,
    ModelShow,
    VariantSet,
    ModeSet,
    Thinking,
    Raw,
    Copy,
    Image,
    Rename,
    Undo,
    Redo,
    Skills,
    SkillInvoke,
    Upcoming,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SlashCommandSpec {
    pub(crate) canonical: &'static str,
    pub(crate) aliases: &'static [&'static str],
    pub(crate) usage: &'static str,
    pub(crate) summary: &'static str,
    pub(crate) surface: CommandSurface,
    pub(crate) group: CommandGroup,
    pub(crate) argument_kind: CommandArgumentKind,
    pub(crate) output_kind: CommandOutputKind,
    pub(crate) status: CommandStatus,
    pub(crate) action: SlashCommandAction,
    pub(crate) common: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CliCommandSpec {
    pub(crate) canonical: &'static str,
    pub(crate) aliases: &'static [&'static str],
    pub(crate) usage: &'static str,
    pub(crate) summary: &'static str,
    pub(crate) surface: CommandSurface,
    pub(crate) group: CommandGroup,
    pub(crate) argument_kind: CommandArgumentKind,
    pub(crate) output_kind: CommandOutputKind,
    pub(crate) status: CommandStatus,
}

const TUI_SLASH: CommandSurface = CommandSurface::TuiSlash;
const PEVO_CLI: CommandSurface = CommandSurface::PevoCli;
const COMMANDS: CommandGroup = CommandGroup::Commands;

pub(crate) const SLASH_COMMANDS: &[SlashCommandSpec] = &[
    SlashCommandSpec {
        canonical: "/help",
        aliases: &[],
        usage: "/help",
        summary: "show commands and shortcuts",
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::BottomHelpPane,
        status: CommandStatus::Active,
        action: SlashCommandAction::Help,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/status",
        aliases: &[],
        usage: "/status",
        summary: "show local status",
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::TranscriptStatusBlock,
        status: CommandStatus::Active,
        action: SlashCommandAction::Status,
        common: true,
    },
    SlashCommandSpec {
        canonical: "/new",
        aliases: &["/clear"],
        usage: "/new",
        summary: "start a new session on next prompt",
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::ImmediateStateChange,
        status: CommandStatus::Active,
        action: SlashCommandAction::New,
        common: true,
    },
    SlashCommandSpec {
        canonical: "/sessions",
        aliases: &["/resume", "/continue"],
        usage: "/sessions",
        summary: "switch session",
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::BottomSelectionPane,
        status: CommandStatus::Active,
        action: SlashCommandAction::Sessions,
        common: true,
    },
    SlashCommandSpec {
        canonical: "/usage",
        aliases: &["/stats"],
        usage: "/usage",
        summary: "usage and cost summary",
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::BottomSelectionPane,
        status: CommandStatus::Active,
        action: SlashCommandAction::Usage,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/context",
        aliases: &[],
        usage: "/context",
        summary: "context usage",
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::TranscriptStatusBlock,
        status: CommandStatus::Active,
        action: SlashCommandAction::Context,
        common: true,
    },
    SlashCommandSpec {
        canonical: "/model",
        aliases: &[],
        usage: "/model",
        summary: "select/fetch model",
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::BottomSelectionPane,
        status: CommandStatus::Active,
        action: SlashCommandAction::ModelShow,
        common: true,
    },
    SlashCommandSpec {
        canonical: "/variant",
        aliases: &[],
        usage: "/variant <none|minimal|low|medium|high|xhigh|max>",
        summary: "set <value>",
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FixedEnumValue,
        output_kind: CommandOutputKind::ImmediateStateChange,
        status: CommandStatus::Active,
        action: SlashCommandAction::VariantSet,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/mode",
        aliases: &[],
        usage: "/mode <plan|default>",
        summary: "set <plan|default>",
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FixedEnumValue,
        output_kind: CommandOutputKind::ImmediateStateChange,
        status: CommandStatus::Active,
        action: SlashCommandAction::ModeSet,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/show-thinking",
        aliases: &[],
        usage: "/show-thinking [on|off]",
        summary: "toggle; set <on|off>",
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::OptionalValue,
        output_kind: CommandOutputKind::ImmediateStateChange,
        status: CommandStatus::Active,
        action: SlashCommandAction::Thinking,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/show-raw",
        aliases: &[],
        usage: "/show-raw [on|off]",
        summary: "toggle raw Markdown display",
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::OptionalValue,
        output_kind: CommandOutputKind::ImmediateStateChange,
        status: CommandStatus::Active,
        action: SlashCommandAction::Raw,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/copy",
        aliases: &[],
        usage: "/copy",
        summary: "copy latest answer as Markdown",
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::BoundedFeedback,
        status: CommandStatus::Active,
        action: SlashCommandAction::Copy,
        common: true,
    },
    SlashCommandSpec {
        canonical: "/image",
        aliases: &[],
        usage: "/image <source> [prompt]",
        summary: "attach image",
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FreeFormTrailingText,
        output_kind: CommandOutputKind::BoundedFeedback,
        status: CommandStatus::Active,
        action: SlashCommandAction::Image,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/rename",
        aliases: &[],
        usage: "/rename <title>",
        summary: "<title> rename current session",
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FreeFormTrailingText,
        output_kind: CommandOutputKind::ImmediateStateChange,
        status: CommandStatus::Active,
        action: SlashCommandAction::Rename,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/undo",
        aliases: &[],
        usage: "/undo",
        summary: "undo last message",
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::ImmediateStateChange,
        status: CommandStatus::Active,
        action: SlashCommandAction::Undo,
        common: true,
    },
    SlashCommandSpec {
        canonical: "/redo",
        aliases: &[],
        usage: "/redo",
        summary: "redo undone messages",
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::ImmediateStateChange,
        status: CommandStatus::Active,
        action: SlashCommandAction::Redo,
        common: true,
    },
    SlashCommandSpec {
        canonical: "/skills",
        aliases: &[],
        usage: "/skills",
        summary: "list skills",
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::TranscriptStatusBlock,
        status: CommandStatus::Active,
        action: SlashCommandAction::Skills,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/compact",
        aliases: &[],
        usage: "/compact",
        summary: "upcoming",
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::BoundedFeedback,
        status: CommandStatus::Upcoming,
        action: SlashCommandAction::Upcoming,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/export",
        aliases: &[],
        usage: "/export",
        summary: "upcoming",
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::BoundedFeedback,
        status: CommandStatus::Upcoming,
        action: SlashCommandAction::Upcoming,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/quit",
        aliases: &["/exit", "/q"],
        usage: "/quit",
        summary: "quit TUI",
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::ImmediateStateChange,
        status: CommandStatus::Active,
        action: SlashCommandAction::Quit,
        common: true,
    },
];

pub(crate) const CUSTOM_SKILL_COMMAND: SlashCommandSpec = SlashCommandSpec {
    canonical: "/skill:<name>",
    aliases: &[],
    usage: "/skill:<name> [args]",
    summary: "invoke a skill",
    surface: TUI_SLASH,
    group: CommandGroup::CustomCommands,
    argument_kind: CommandArgumentKind::DynamicSuffixOptionalText,
    output_kind: CommandOutputKind::PromptSubmission,
    status: CommandStatus::Active,
    action: SlashCommandAction::SkillInvoke,
    common: false,
};

pub(crate) const CLI_COMMANDS: &[CliCommandSpec] = &[
    CliCommandSpec {
        canonical: "init",
        aliases: &[],
        usage: "pevo init",
        summary: "initialize Psychevo home",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "run",
        aliases: &[],
        usage: "pevo run [message..]",
        summary: "run one coding-agent turn",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FreeFormTrailingText,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "smoke",
        aliases: &[],
        usage: "pevo smoke",
        summary: "run deterministic fake-provider smoke behavior",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::RequiredValue,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "tui",
        aliases: &[],
        usage: "pevo tui [message..]",
        summary: "open the interactive terminal UI",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FreeFormTrailingText,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "skills",
        aliases: &[],
        usage: "pevo skills <command>",
        summary: "manage local skills",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::RequiredValue,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "stats",
        aliases: &[],
        usage: "pevo stats",
        summary: "show local usage and estimated cost",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::OptionalValue,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "context",
        aliases: &[],
        usage: "pevo context --session <id|latest>",
        summary: "inspect context-window usage for a session",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::RequiredValue,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
];

pub(crate) fn slash_command_spec(name: &str) -> Option<&'static SlashCommandSpec> {
    SLASH_COMMANDS
        .iter()
        .find(|spec| spec.canonical == name || spec.aliases.contains(&name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_registry_keeps_aliases_hidden_from_canonical_rows() {
        assert!(SLASH_COMMANDS.iter().any(|spec| spec.canonical == "/help"));
        assert!(
            SLASH_COMMANDS
                .iter()
                .any(|spec| spec.canonical == "/usage" && spec.aliases == ["/stats"].as_slice())
        );
        assert!(!SLASH_COMMANDS.iter().any(|spec| spec.canonical == "/stats"));
        assert!(
            !SLASH_COMMANDS
                .iter()
                .any(|spec| spec.canonical == "/thinking")
        );
        assert!(!SLASH_COMMANDS.iter().any(|spec| spec.canonical == "/raw"));
    }

    #[test]
    fn cli_registry_records_process_commands() {
        let names = CLI_COMMANDS
            .iter()
            .map(|spec| spec.canonical)
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            ["init", "run", "smoke", "tui", "skills", "stats", "context"]
        );
        assert!(
            CLI_COMMANDS
                .iter()
                .all(|spec| spec.surface == CommandSurface::PevoCli)
        );
    }
}
