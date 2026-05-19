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
    ReloadContext,
    ModelShow,
    VariantSet,
    ModeSet,
    Permissions,
    Thinking,
    Raw,
    Copy,
    Export,
    Share,
    Image,
    Rename,
    Undo,
    Redo,
    Skills,
    Agents,
    Fork,
    SkillInvoke,
    Upcoming,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SlashCommandSpec {
    pub(crate) canonical: &'static str,
    pub(crate) aliases: &'static [&'static str],
    pub(crate) usage: &'static str,
    pub(crate) summary: &'static str,
    pub(crate) help_detail: Option<&'static str>,
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
        help_detail: Some(
            "Opens local help only; it does not add a session message or call a provider.",
        ),
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
        help_detail: Some(
            "Shows local paths, session, model, variant, mode, and debug state without provider calls.",
        ),
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
        help_detail: Some(
            "Clears the current transcript view and uses a new session for the next submitted prompt.",
        ),
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
        summary: "switch or manage sessions",
        help_detail: Some(
            "Opens the local session picker; archive, restore, and delete actions affect local state only.",
        ),
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
        summary: "local usage and cost",
        help_detail: Some(
            "Reads persisted SQLite accounting and cost estimates; it does not contact providers.",
        ),
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
        summary: "context window usage",
        help_detail: Some(
            "Shows the latest context snapshot or session estimate from local data without provider calls.",
        ),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::TranscriptStatusBlock,
        status: CommandStatus::Active,
        action: SlashCommandAction::Context,
        common: true,
    },
    SlashCommandSpec {
        canonical: "/reload-context",
        aliases: &[],
        usage: "/reload-context",
        summary: "rebuild session prompt prefix",
        help_detail: Some(
            "Re-reads current local agents, skills, and AGENTS context for future turns in this session.",
        ),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::TranscriptStatusBlock,
        status: CommandStatus::Active,
        action: SlashCommandAction::ReloadContext,
        common: true,
    },
    SlashCommandSpec {
        canonical: "/model",
        aliases: &[],
        usage: "/model",
        summary: "choose or fetch model",
        help_detail: Some(
            "Opens the model picker; explicit fetch rows may call configured provider /models endpoints.",
        ),
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
        summary: "set reasoning effort",
        help_detail: Some(
            "Persists a TUI-local reasoning effort override for this workdir and future turns.",
        ),
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
        usage: "/mode <plan|default|acceptEdits|dontAsk|bypassPermissions>",
        summary: "set runtime permission mode",
        help_detail: Some(
            "Changes mode for future turns; plan uses read-only tools and dangerous bypass modes never skip hard denies.",
        ),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FixedEnumValue,
        output_kind: CommandOutputKind::ImmediateStateChange,
        status: CommandStatus::Active,
        action: SlashCommandAction::ModeSet,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/permissions",
        aliases: &[],
        usage: "/permissions",
        summary: "show permission policy",
        help_detail: Some(
            "Shows effective permission mode and project-local allow/ask/deny rules.",
        ),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::TranscriptStatusBlock,
        status: CommandStatus::Active,
        action: SlashCommandAction::Permissions,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/show-thinking",
        aliases: &[],
        usage: "/show-thinking [on|off]",
        summary: "toggle; set <on|off>",
        help_detail: Some(
            "Changes local display of folded reasoning; stored transcript and provider payloads are unchanged.",
        ),
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
        help_detail: Some(
            "Changes local rendering of answers; /copy still copies the original Markdown source.",
        ),
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
        help_detail: Some(
            "Copies only the latest visible assistant answer, excluding Thinking, tools, and metadata.",
        ),
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
        summary: "attach one image",
        help_detail: Some(
            "Adds one pending image; local files are checked before submit, remote URLs are not downloaded.",
        ),
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
        summary: "rename current session",
        help_detail: Some("Updates the current local session title in SQLite."),
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
        help_detail: Some(
            "Restores the previous local session and file snapshot when undo state is available.",
        ),
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
        help_detail: Some(
            "Restores the local messages and file snapshot from the most recent undo when available.",
        ),
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
        summary: "list available skills",
        help_detail: Some(
            "Lists discovered skills and descriptions; it does not invoke a skill by itself.",
        ),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::TranscriptStatusBlock,
        status: CommandStatus::Active,
        action: SlashCommandAction::Skills,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/agents",
        aliases: &[],
        usage: "/agents",
        summary: "list available agents",
        help_detail: Some(
            "Lists discovered agent definitions and current-session child agent runs; it does not call a provider.",
        ),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::TranscriptStatusBlock,
        status: CommandStatus::Active,
        action: SlashCommandAction::Agents,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/fork",
        aliases: &[],
        usage: "/fork <prompt>",
        summary: "delegate a forked agent",
        help_detail: Some(
            "Submits a request for the main agent to call the Agent tool with fork_context=true in the background.",
        ),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FreeFormTrailingText,
        output_kind: CommandOutputKind::PromptSubmission,
        status: CommandStatus::Active,
        action: SlashCommandAction::Fork,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/compact",
        aliases: &[],
        usage: "/compact",
        summary: "upcoming compaction",
        help_detail: Some(
            "Placeholder only; it reports that compaction is upcoming and makes no session changes.",
        ),
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
        usage: "/export [path] [-f|--format markdown|json] [-i|--include list]",
        summary: "write session export",
        help_detail: Some(
            "Writes a local artifact; last-provider-request can expose hidden prompts and tool data.",
        ),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FreeFormTrailingText,
        output_kind: CommandOutputKind::TranscriptStatusBlock,
        status: CommandStatus::Active,
        action: SlashCommandAction::Export,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/share",
        aliases: &[],
        usage: "/share [path] [-i|--include list]",
        summary: "write share artifact",
        help_detail: Some(
            "Writes a local Markdown share file; it does not upload or include provider request bodies.",
        ),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FreeFormTrailingText,
        output_kind: CommandOutputKind::TranscriptStatusBlock,
        status: CommandStatus::Active,
        action: SlashCommandAction::Share,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/quit",
        aliases: &["/exit", "/q"],
        usage: "/quit",
        summary: "quit TUI",
        help_detail: Some("Leaves the fullscreen TUI without deleting local session state."),
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
    help_detail: Some(
        "Inserts an explicit skill invocation with optional args into the prompt flow.",
    ),
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
        canonical: "agent",
        aliases: &[],
        usage: "pevo agent <command>",
        summary: "manage agents",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::RequiredValue,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "skill",
        aliases: &[],
        usage: "pevo skill <command>",
        summary: "manage local skills",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::RequiredValue,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "session",
        aliases: &[],
        usage: "pevo session <command>",
        summary: "manage local sessions",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::RequiredValue,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "model",
        aliases: &[],
        usage: "pevo model <command>",
        summary: "inspect configured models",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::RequiredValue,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "config",
        aliases: &[],
        usage: "pevo config <command>",
        summary: "inspect and update Psychevo config",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::RequiredValue,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "auth",
        aliases: &[],
        usage: "pevo auth <command>",
        summary: "inspect and update provider credentials",
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
            [
                "init", "run", "smoke", "tui", "agent", "skill", "session", "model", "config",
                "auth", "stats", "context",
            ]
        );
        assert!(
            CLI_COMMANDS
                .iter()
                .all(|spec| spec.surface == CommandSurface::PevoCli)
        );
    }
}
