#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSurface {
    TuiSlash,
    PevoCli,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandGroup {
    Commands,
    CustomCommands,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandArgumentKind {
    None,
    RequiredValue,
    OptionalValue,
    FixedEnumValue,
    FreeFormTrailingText,
    DynamicSuffixOptionalText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOutputKind {
    TranscriptStatusBlock,
    BottomSelectionPane,
    BottomHelpPane,
    ImmediateStateChange,
    PromptSubmission,
    ProcessResult,
    BoundedFeedback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStatus {
    Active,
    Upcoming,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CommandCapability {
    Picker,
    Clipboard,
    RendererToggle,
    ProcessExit,
    SideConversation,
    ImageAttachment,
    ActiveTurnControl,
    Queue,
    SessionSwitch,
    ArtifactWrite,
    ConfigWrite,
    PolicyWrite,
    SkillStateWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommandAction {
    Help,
    Quit,
    Status,
    New,
    Sessions,
    Resume,
    Usage,
    Context,
    Refresh,
    Btw,
    Steer,
    Queue,
    Pending,
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
    Tools,
    Bundles,
    Curator,
    Agents,
    Fork,
    Compact,
    SkillInvoke,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommandSurface {
    Tui,
    Acp,
    Messaging,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlashCommandSpec {
    pub canonical: &'static str,
    pub aliases: &'static [&'static str],
    pub usage: &'static str,
    pub summary: &'static str,
    pub help_detail: Option<&'static str>,
    pub surface: CommandSurface,
    pub group: CommandGroup,
    pub argument_kind: CommandArgumentKind,
    pub output_kind: CommandOutputKind,
    pub status: CommandStatus,
    pub action: SlashCommandAction,
    pub common: bool,
}

impl SlashCommandSpec {
    pub fn required_capabilities(self) -> &'static [CommandCapability] {
        match self.action {
            SlashCommandAction::Quit => &[CommandCapability::ProcessExit],
            SlashCommandAction::Btw => &[CommandCapability::SideConversation],
            SlashCommandAction::Steer => &[CommandCapability::ActiveTurnControl],
            SlashCommandAction::Queue | SlashCommandAction::Pending => &[CommandCapability::Queue],
            SlashCommandAction::Thinking | SlashCommandAction::Raw => {
                &[CommandCapability::RendererToggle]
            }
            SlashCommandAction::Copy => &[CommandCapability::Clipboard],
            SlashCommandAction::Image => &[CommandCapability::ImageAttachment],
            SlashCommandAction::Sessions | SlashCommandAction::Resume => {
                &[CommandCapability::SessionSwitch]
            }
            SlashCommandAction::Tools => &[],
            SlashCommandAction::Permissions => &[],
            SlashCommandAction::Export | SlashCommandAction::Share => {
                &[CommandCapability::ArtifactWrite]
            }
            SlashCommandAction::Skills => &[],
            _ => &[],
        }
    }

    pub fn unavailable_guidance(self, surface: SlashCommandSurface) -> String {
        match self.action {
            SlashCommandAction::Quit => {
                format!(
                    "{} is only meaningful in a local TUI process.",
                    self.canonical
                )
            }
            SlashCommandAction::Copy => {
                format!(
                    "{} needs local clipboard access; use the client copy action instead.",
                    self.canonical
                )
            }
            SlashCommandAction::Thinking | SlashCommandAction::Raw => {
                format!(
                    "{} controls TUI rendering only and is not available on this surface.",
                    self.canonical
                )
            }
            SlashCommandAction::Image => {
                let surface = match surface {
                    SlashCommandSurface::Acp => "ACP client",
                    SlashCommandSurface::Messaging => "messaging client",
                    SlashCommandSurface::Tui => "TUI",
                };
                format!(
                    "{} is not advertised here; attach images with the {surface}'s native attachment flow when available.",
                    self.canonical
                )
            }
            SlashCommandAction::Btw => {
                format!(
                    "{} opens a local TUI side conversation and is not available on this surface.",
                    self.canonical
                )
            }
            _ => format!("{} is not available on this surface.", self.canonical),
        }
    }

    pub fn available_during_active_turn(self) -> bool {
        matches!(
            self.action,
            SlashCommandAction::Help
                | SlashCommandAction::Status
                | SlashCommandAction::Usage
                | SlashCommandAction::Context
                | SlashCommandAction::Tools
                | SlashCommandAction::Agents
                | SlashCommandAction::Steer
                | SlashCommandAction::Queue
                | SlashCommandAction::Pending
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CliCommandSpec {
    pub canonical: &'static str,
    pub aliases: &'static [&'static str],
    pub usage: &'static str,
    pub summary: &'static str,
    pub surface: CommandSurface,
    pub group: CommandGroup,
    pub argument_kind: CommandArgumentKind,
    pub output_kind: CommandOutputKind,
    pub status: CommandStatus,
}

const TUI_SLASH: CommandSurface = CommandSurface::TuiSlash;
const PEVO_CLI: CommandSurface = CommandSurface::PevoCli;
const COMMANDS: CommandGroup = CommandGroup::Commands;

pub const SLASH_COMMANDS: &[SlashCommandSpec] = &[
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
        aliases: &[],
        usage: "/sessions",
        summary: "list sessions",
        help_detail: Some(
            "Opens the local session picker in TUI; text surfaces list numbered sessions.",
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
        canonical: "/resume",
        aliases: &[],
        usage: "/resume [ref]",
        summary: "resume a previous session",
        help_detail: Some(
            "Text surfaces accept latest, a listed number, an id prefix, or an exact title.",
        ),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::OptionalValue,
        output_kind: CommandOutputKind::ImmediateStateChange,
        status: CommandStatus::Active,
        action: SlashCommandAction::Resume,
        common: true,
    },
    SlashCommandSpec {
        canonical: "/continue",
        aliases: &[],
        usage: "/continue [ref]",
        summary: "continue a previous session",
        help_detail: Some("Alias-style resume command with the same text references as /resume."),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::OptionalValue,
        output_kind: CommandOutputKind::ImmediateStateChange,
        status: CommandStatus::Active,
        action: SlashCommandAction::Resume,
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
        canonical: "/refresh",
        aliases: &[],
        usage: "/refresh",
        summary: "reload context and clean side sessions",
        help_detail: Some(
            "Re-reads current local agents, skills, and AGENTS context, then cleans orphan /btw side sessions.",
        ),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::TranscriptStatusBlock,
        status: CommandStatus::Active,
        action: SlashCommandAction::Refresh,
        common: true,
    },
    SlashCommandSpec {
        canonical: "/btw",
        aliases: &[],
        usage: "/btw [prompt]",
        summary: "open a side conversation",
        help_detail: Some(
            "Starts a temporary side conversation; Ctrl+C returns and deletes the side transcript.",
        ),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::OptionalValue,
        output_kind: CommandOutputKind::ImmediateStateChange,
        status: CommandStatus::Active,
        action: SlashCommandAction::Btw,
        common: true,
    },
    SlashCommandSpec {
        canonical: "/steer",
        aliases: &[],
        usage: "/steer <message>",
        summary: "steer the running turn",
        help_detail: Some(
            "Adds a user message to the active agent turn at the next provider boundary.",
        ),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FreeFormTrailingText,
        output_kind: CommandOutputKind::PromptSubmission,
        status: CommandStatus::Active,
        action: SlashCommandAction::Steer,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/queue",
        aliases: &[],
        usage: "/queue <message>",
        summary: "queue the next prompt",
        help_detail: Some(
            "Adds a prompt to the next-turn FIFO queue; idle TUI drains it immediately.",
        ),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FreeFormTrailingText,
        output_kind: CommandOutputKind::PromptSubmission,
        status: CommandStatus::Active,
        action: SlashCommandAction::Queue,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/pending",
        aliases: &[],
        usage: "/pending cancel",
        summary: "cancel unsent input",
        help_detail: Some("Cancels pending steers and clears the next-turn queue."),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FixedEnumValue,
        output_kind: CommandOutputKind::ImmediateStateChange,
        status: CommandStatus::Active,
        action: SlashCommandAction::Pending,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/model",
        aliases: &[],
        usage: "/model [model] [variant]",
        summary: "choose model",
        help_detail: Some(
            "Shows the active model, or changes the model used for future turns on text surfaces.",
        ),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FreeFormTrailingText,
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
        usage: "/permissions [allow|ask|deny <rule>|remove <allow|ask|deny> <rule>]",
        summary: "view/edit permission policy",
        help_detail: Some("Shows local permission policy state and command forms."),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FreeFormTrailingText,
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
        usage: "/skills [list|browse|search|inspect|check|audit|reload]",
        summary: "skill hub",
        help_detail: Some(
            "Dispatches local skills hub commands without invoking a skill; /skills reload refreshes skill context.",
        ),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FreeFormTrailingText,
        output_kind: CommandOutputKind::TranscriptStatusBlock,
        status: CommandStatus::Active,
        action: SlashCommandAction::Skills,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/tools",
        aliases: &[],
        usage: "/tools [list|enable|disable <toolset>]",
        summary: "toolsets",
        help_detail: Some(
            "Shows toolset state and lets supported surfaces enable or disable toolsets.",
        ),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FreeFormTrailingText,
        output_kind: CommandOutputKind::BottomSelectionPane,
        status: CommandStatus::Active,
        action: SlashCommandAction::Tools,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/bundles",
        aliases: &[],
        usage: "/bundles [list]",
        summary: "skill bundles",
        help_detail: Some("Shows skill bundle status and help."),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FreeFormTrailingText,
        output_kind: CommandOutputKind::TranscriptStatusBlock,
        status: CommandStatus::Active,
        action: SlashCommandAction::Bundles,
        common: false,
    },
    SlashCommandSpec {
        canonical: "/curator",
        aliases: &[],
        usage: "/curator [status]",
        summary: "skill curator",
        help_detail: Some("Shows skill curator status and help."),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FreeFormTrailingText,
        output_kind: CommandOutputKind::TranscriptStatusBlock,
        status: CommandStatus::Active,
        action: SlashCommandAction::Curator,
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
        usage: "/compact [instructions]",
        summary: "compact context",
        help_detail: Some(
            "Summarizes older context for future turns; optional instructions guide the summary focus.",
        ),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FreeFormTrailingText,
        output_kind: CommandOutputKind::BoundedFeedback,
        status: CommandStatus::Active,
        action: SlashCommandAction::Compact,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommandParse {
    NotSlash,
    Known(SlashCommandInvocation),
    Unknown {
        original: String,
        command: String,
        args: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashCommandInvocation {
    pub original: String,
    pub command: String,
    pub args: String,
    pub spec: &'static SlashCommandSpec,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommandEffect {
    LocalText,
    PassThroughPrompt(String),
    SubmitPrompt(String),
    Steer(String),
    Queue(String),
    PendingCancel,
    NewSession,
    SessionsList,
    ResumeSession {
        reference: Option<String>,
    },
    ShowModel,
    SetModel {
        model: String,
        variant: Option<String>,
    },
    SetVariant(String),
    SetMode(String),
    PermissionsShow,
    PermissionAdd {
        kind: String,
        rule: String,
    },
    PermissionRemove {
        kind: String,
        rule: String,
    },
    ToolsShow,
    ToolsetSet {
        name: String,
        enabled: bool,
    },
    Rename(String),
    Undo,
    Redo,
    Skills {
        args: Option<String>,
    },
    Bundles {
        args: Option<String>,
    },
    Curator {
        args: Option<String>,
    },
    Agents,
    Fork(String),
    Compact {
        instructions: Option<String>,
    },
    Export {
        args: Option<String>,
    },
    Share {
        args: Option<String>,
    },
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableSlashCommand {
    pub name: String,
    pub usage: String,
    pub summary: String,
    pub aliases: Vec<String>,
    pub argument_kind: CommandArgumentKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicSlashCommand {
    pub name: String,
    pub summary: String,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableSlashCommands {
    pub commands: Vec<AvailableSlashCommand>,
    pub hidden_dynamic: usize,
}

pub const CUSTOM_SKILL_COMMAND: SlashCommandSpec = SlashCommandSpec {
    canonical: "/<skill-or-bundle>",
    aliases: &[],
    usage: "/<skill-or-bundle> [args]",
    summary: "submit a skill or bundle",
    help_detail: Some(
        "Submits the slash line while using the equivalent explicit skill or bundle marker for runtime expansion.",
    ),
    surface: TUI_SLASH,
    group: CommandGroup::CustomCommands,
    argument_kind: CommandArgumentKind::DynamicSuffixOptionalText,
    output_kind: CommandOutputKind::PromptSubmission,
    status: CommandStatus::Active,
    action: SlashCommandAction::SkillInvoke,
    common: false,
};

pub const CLI_COMMANDS: &[CliCommandSpec] = &[
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
        canonical: "acp",
        aliases: &[],
        usage: "pevo acp",
        summary: "run the ACP stdio server",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
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
        canonical: "tool",
        aliases: &["tools"],
        usage: "pevo tool <command>",
        summary: "manage local toolsets",
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

pub fn slash_command_spec(name: &str) -> Option<&'static SlashCommandSpec> {
    let name = normalize_slash_command_name(name)?;
    SLASH_COMMANDS
        .iter()
        .find(|spec| spec.canonical == name || spec.aliases.contains(&name))
}

pub fn parse_slash_command_line(input: &str) -> SlashCommandParse {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return SlashCommandParse::NotSlash;
    }
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let command = parts.next().unwrap_or_default();
    if command == "/" {
        return SlashCommandParse::Unknown {
            original: trimmed.to_string(),
            command: command.to_string(),
            args: String::new(),
        };
    }
    let args = parts.next().unwrap_or("").trim().to_string();
    match slash_command_spec(command) {
        Some(spec) => SlashCommandParse::Known(SlashCommandInvocation {
            original: trimmed.to_string(),
            command: command.to_string(),
            args,
            spec,
        }),
        None => SlashCommandParse::Unknown {
            original: trimmed.to_string(),
            command: command.to_string(),
            args,
        },
    }
}

pub fn supported_by_capabilities(
    spec: &SlashCommandSpec,
    capabilities: &[CommandCapability],
) -> bool {
    spec.required_capabilities()
        .iter()
        .all(|capability| capabilities.contains(capability))
}

pub fn available_slash_commands_for_surface(
    capabilities: &[CommandCapability],
    active_turn: bool,
    dynamic: &[DynamicSlashCommand],
    cap: usize,
) -> AvailableSlashCommands {
    let mut commands = SLASH_COMMANDS
        .iter()
        .filter(|spec| spec.status == CommandStatus::Active)
        .filter(|spec| supported_by_capabilities(spec, capabilities))
        .filter(|spec| !active_turn || spec.available_during_active_turn())
        .map(|spec| AvailableSlashCommand {
            name: spec.canonical.trim_start_matches('/').to_string(),
            usage: spec.usage.to_string(),
            summary: spec.summary.to_string(),
            aliases: spec
                .aliases
                .iter()
                .map(|alias| (*alias).to_string())
                .collect(),
            argument_kind: spec.argument_kind,
        })
        .collect::<Vec<_>>();
    let core_count = commands.len();
    if commands.len() > cap {
        commands.truncate(cap);
        return AvailableSlashCommands {
            commands,
            hidden_dynamic: dynamic.len(),
        };
    }
    if commands.len() < cap {
        let remaining = cap - commands.len();
        commands.extend(
            dynamic
                .iter()
                .take(remaining)
                .map(|command| AvailableSlashCommand {
                    name: command.name.trim_start_matches('/').to_string(),
                    usage: format!("/{} [args]", command.name.trim_start_matches('/')),
                    summary: command.summary.clone(),
                    aliases: Vec::new(),
                    argument_kind: CommandArgumentKind::DynamicSuffixOptionalText,
                }),
        );
    }
    AvailableSlashCommands {
        hidden_dynamic: dynamic.len().saturating_sub(cap.saturating_sub(core_count)),
        commands,
    }
}

pub fn slash_invocation_effect(
    invocation: &SlashCommandInvocation,
    capabilities: &[CommandCapability],
    surface: SlashCommandSurface,
    active_turn: bool,
) -> std::result::Result<SlashCommandEffect, String> {
    let spec = invocation.spec;
    if !supported_by_capabilities(spec, capabilities) {
        return Ok(SlashCommandEffect::Unsupported(
            spec.unavailable_guidance(surface),
        ));
    }
    if active_turn && !spec.available_during_active_turn() {
        return Ok(SlashCommandEffect::Unsupported(format!(
            "{} is not available while a turn is running; wait, cancel, or use /queue <message>.",
            spec.canonical
        )));
    }
    if spec.status == CommandStatus::Upcoming {
        if invocation.args.trim().is_empty() {
            return Ok(SlashCommandEffect::Unsupported(format!(
                "{} is not available yet.",
                spec.canonical
            )));
        }
        return Err(format!(
            "{} is upcoming and does not accept arguments",
            spec.canonical
        ));
    }

    match spec.action {
        SlashCommandAction::Help
        | SlashCommandAction::Status
        | SlashCommandAction::Usage
        | SlashCommandAction::Context
        | SlashCommandAction::Refresh => {
            no_args(spec, &invocation.args)?;
            Ok(SlashCommandEffect::LocalText)
        }
        SlashCommandAction::New => {
            no_args(spec, &invocation.args)?;
            Ok(SlashCommandEffect::NewSession)
        }
        SlashCommandAction::Sessions => {
            no_args(spec, &invocation.args)?;
            Ok(SlashCommandEffect::SessionsList)
        }
        SlashCommandAction::Resume => Ok(SlashCommandEffect::ResumeSession {
            reference: optional_text(&invocation.args),
        }),
        SlashCommandAction::Steer => Ok(SlashCommandEffect::Steer(required_text(
            spec,
            &invocation.args,
        )?)),
        SlashCommandAction::Queue => Ok(SlashCommandEffect::Queue(required_text(
            spec,
            &invocation.args,
        )?)),
        SlashCommandAction::Pending => match invocation.args.as_str() {
            "cancel" => Ok(SlashCommandEffect::PendingCancel),
            _ => Err(format!("usage: {}", spec.usage)),
        },
        SlashCommandAction::ModelShow => parse_model_effect(spec, &invocation.args),
        SlashCommandAction::VariantSet => Ok(SlashCommandEffect::SetVariant(parse_single_value(
            spec,
            &invocation.args,
            validate_variant,
        )?)),
        SlashCommandAction::ModeSet => Ok(SlashCommandEffect::SetMode(parse_single_value(
            spec,
            &invocation.args,
            validate_mode,
        )?)),
        SlashCommandAction::Permissions => parse_permissions_effect(&invocation.args),
        SlashCommandAction::Tools => parse_tools_effect(&invocation.args),
        SlashCommandAction::Rename => Ok(SlashCommandEffect::Rename(required_text(
            spec,
            &invocation.args,
        )?)),
        SlashCommandAction::Undo => {
            no_args(spec, &invocation.args)?;
            Ok(SlashCommandEffect::Undo)
        }
        SlashCommandAction::Redo => {
            no_args(spec, &invocation.args)?;
            Ok(SlashCommandEffect::Redo)
        }
        SlashCommandAction::Skills => Ok(SlashCommandEffect::Skills {
            args: optional_text(&invocation.args),
        }),
        SlashCommandAction::Bundles => Ok(SlashCommandEffect::Bundles {
            args: optional_text(&invocation.args),
        }),
        SlashCommandAction::Curator => Ok(SlashCommandEffect::Curator {
            args: optional_text(&invocation.args),
        }),
        SlashCommandAction::Agents => {
            no_args(spec, &invocation.args)?;
            Ok(SlashCommandEffect::Agents)
        }
        SlashCommandAction::Fork => Ok(SlashCommandEffect::Fork(fork_prompt_marker(
            &required_text(spec, &invocation.args)?,
        ))),
        SlashCommandAction::Compact => Ok(SlashCommandEffect::Compact {
            instructions: optional_text(&invocation.args),
        }),
        SlashCommandAction::Export => Ok(SlashCommandEffect::Export {
            args: optional_text(&invocation.args),
        }),
        SlashCommandAction::Share => Ok(SlashCommandEffect::Share {
            args: optional_text(&invocation.args),
        }),
        SlashCommandAction::Quit
        | SlashCommandAction::Btw
        | SlashCommandAction::Thinking
        | SlashCommandAction::Raw
        | SlashCommandAction::Copy
        | SlashCommandAction::Image => Ok(SlashCommandEffect::Unsupported(
            spec.unavailable_guidance(surface),
        )),
        SlashCommandAction::SkillInvoke => unreachable!("dynamic skills use dynamic effect"),
    }
}

pub fn dynamic_slash_command_effect(
    command: &str,
    args: &str,
    dynamic: &[DynamicSlashCommand],
) -> Option<SlashCommandEffect> {
    let command = normalize_dynamic_skill_name(command.trim_start_matches('/'));
    dynamic
        .iter()
        .find(|entry| normalize_dynamic_skill_name(&entry.name) == command)
        .map(|entry| {
            let mut prompt = entry.prompt.trim().to_string();
            if !args.trim().is_empty() {
                if !prompt.ends_with(' ') {
                    prompt.push(' ');
                }
                prompt.push_str(args.trim());
            }
            SlashCommandEffect::SubmitPrompt(prompt)
        })
}

pub fn skill_prompt_marker(name: &str, args: &str) -> String {
    if args.trim().is_empty() {
        format!("${} ", name.trim_start_matches('/'))
    } else {
        format!("${} {}", name.trim_start_matches('/'), args.trim())
    }
}

pub fn normalize_dynamic_skill_name(name: &str) -> String {
    name.chars()
        .flat_map(char::to_lowercase)
        .filter_map(|ch| {
            if ch.is_ascii_alphanumeric() {
                Some(ch)
            } else if ch == '-' || ch == '_' || ch.is_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn fork_prompt_marker(prompt: &str) -> String {
    format!(
        "Use the Agent tool with agent_type=\"general\", fork_context=true, and background=true for this task:\n\n{}",
        prompt.trim()
    )
}

fn normalize_slash_command_name(name: &str) -> Option<&str> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    name.starts_with('/').then_some(name)
}

fn no_args(spec: &SlashCommandSpec, args: &str) -> std::result::Result<(), String> {
    if args.trim().is_empty() {
        Ok(())
    } else {
        Err(format!("{} does not accept arguments", spec.canonical))
    }
}

fn required_text(spec: &SlashCommandSpec, args: &str) -> std::result::Result<String, String> {
    let args = args.trim();
    if args.is_empty() {
        Err(format!("usage: {}", spec.usage))
    } else {
        Ok(args.to_string())
    }
}

fn optional_text(args: &str) -> Option<String> {
    let args = args.trim();
    (!args.is_empty()).then(|| args.to_string())
}

fn parse_single_value(
    spec: &SlashCommandSpec,
    args: &str,
    validate: fn(&str) -> bool,
) -> std::result::Result<String, String> {
    let parts = args.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [value] if validate(value) => Ok((*value).to_string()),
        _ => Err(format!("usage: {}", spec.usage)),
    }
}

fn parse_model_effect(
    spec: &SlashCommandSpec,
    args: &str,
) -> std::result::Result<SlashCommandEffect, String> {
    let parts = args.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [] => Ok(SlashCommandEffect::ShowModel),
        [model] => Ok(SlashCommandEffect::SetModel {
            model: (*model).to_string(),
            variant: None,
        }),
        [model, variant] if validate_variant(variant) => Ok(SlashCommandEffect::SetModel {
            model: (*model).to_string(),
            variant: Some((*variant).to_string()),
        }),
        _ => Err(format!("usage: {}", spec.usage)),
    }
}

fn parse_tools_effect(args: &str) -> std::result::Result<SlashCommandEffect, String> {
    let parts = args.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [] | ["list"] => Ok(SlashCommandEffect::ToolsShow),
        ["enable", name] => Ok(SlashCommandEffect::ToolsetSet {
            name: (*name).to_string(),
            enabled: true,
        }),
        ["disable", name] => Ok(SlashCommandEffect::ToolsetSet {
            name: (*name).to_string(),
            enabled: false,
        }),
        _ => Err("usage: /tools [list|enable|disable <toolset>]".to_string()),
    }
}

fn parse_permissions_effect(args: &str) -> std::result::Result<SlashCommandEffect, String> {
    let args = args.trim();
    if args.is_empty() {
        return Ok(SlashCommandEffect::PermissionsShow);
    }
    let mut parts = args.split_whitespace();
    let Some(action) = parts.next() else {
        return Ok(SlashCommandEffect::PermissionsShow);
    };
    match action {
        "allow" | "ask" | "deny" => {
            let rule = parts.collect::<Vec<_>>().join(" ");
            if rule.trim().is_empty() {
                return Err("usage: /permissions allow|ask|deny <rule>".to_string());
            }
            Ok(SlashCommandEffect::PermissionAdd {
                kind: action.to_string(),
                rule,
            })
        }
        "remove" => {
            let Some(kind) = parts.next() else {
                return Err("usage: /permissions remove <allow|ask|deny> <rule>".to_string());
            };
            if !matches!(kind, "allow" | "ask" | "deny") {
                return Err("usage: /permissions remove <allow|ask|deny> <rule>".to_string());
            }
            let rule = parts.collect::<Vec<_>>().join(" ");
            if rule.trim().is_empty() {
                return Err("usage: /permissions remove <allow|ask|deny> <rule>".to_string());
            }
            Ok(SlashCommandEffect::PermissionRemove {
                kind: kind.to_string(),
                rule,
            })
        }
        _ => Err(
            "usage: /permissions [allow|ask|deny <rule>|remove <allow|ask|deny> <rule>]"
                .to_string(),
        ),
    }
}

fn validate_variant(value: &str) -> bool {
    matches!(
        value,
        "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
    )
}

fn validate_mode(value: &str) -> bool {
    matches!(
        value,
        "plan"
            | "default"
            | "acceptEdits"
            | "accept-edits"
            | "dontAsk"
            | "dont-ask"
            | "bypassPermissions"
            | "bypass-permissions"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_registry_keeps_aliases_hidden_from_canonical_rows() {
        assert!(SLASH_COMMANDS.iter().any(|spec| spec.canonical == "/help"));
        assert!(SLASH_COMMANDS.iter().any(|spec| spec.canonical == "/btw"));
        assert!(
            SLASH_COMMANDS
                .iter()
                .any(|spec| spec.canonical == "/refresh")
        );
        assert!(!SLASH_COMMANDS.iter().any(|spec| spec.canonical == "/side"));
        assert!(
            !SLASH_COMMANDS
                .iter()
                .any(|spec| spec.canonical == "/reload-context")
        );
        assert!(
            SLASH_COMMANDS
                .iter()
                .any(|spec| spec.canonical == "/usage" && spec.aliases == ["/stats"].as_slice())
        );
        assert!(!SLASH_COMMANDS.iter().any(|spec| spec.canonical == "/stats"));
        assert!(
            SLASH_COMMANDS
                .iter()
                .any(|spec| spec.canonical == "/resume")
        );
        assert!(
            SLASH_COMMANDS
                .iter()
                .any(|spec| spec.canonical == "/continue")
        );
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
                "init", "run", "smoke", "tui", "acp", "agent", "skill", "tool", "session", "model",
                "config", "auth", "stats", "context",
            ]
        );
        assert!(
            CLI_COMMANDS
                .iter()
                .all(|spec| spec.surface == CommandSurface::PevoCli)
        );
    }

    #[test]
    fn shared_parser_resolves_aliases_and_unknowns() {
        let SlashCommandParse::Known(invocation) = parse_slash_command_line(" /stats ") else {
            panic!("expected known command");
        };
        assert_eq!(invocation.spec.canonical, "/usage");
        assert_eq!(invocation.command, "/stats");

        let SlashCommandParse::Unknown { command, args, .. } =
            parse_slash_command_line("/made-up hello")
        else {
            panic!("expected unknown command");
        };
        assert_eq!(command, "/made-up");
        assert_eq!(args, "hello");
        assert_eq!(
            parse_slash_command_line("hello"),
            SlashCommandParse::NotSlash
        );
    }

    #[test]
    fn acp_capabilities_hide_tui_only_commands() {
        let available = available_slash_commands_for_surface(
            &[
                CommandCapability::ActiveTurnControl,
                CommandCapability::Queue,
                CommandCapability::SessionSwitch,
                CommandCapability::ArtifactWrite,
                CommandCapability::ConfigWrite,
                CommandCapability::PolicyWrite,
                CommandCapability::SkillStateWrite,
            ],
            false,
            &[],
            100,
        );
        let names = available
            .commands
            .iter()
            .map(|command| command.name.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"tools"));
        assert!(names.contains(&"resume"));
        assert!(!names.contains(&"copy"));
        assert!(!names.contains(&"image"));
        assert!(!names.contains(&"quit"));
    }

    #[test]
    fn active_turn_filters_disruptive_commands() {
        let available = available_slash_commands_for_surface(
            &[
                CommandCapability::ActiveTurnControl,
                CommandCapability::Queue,
                CommandCapability::SessionSwitch,
                CommandCapability::ArtifactWrite,
            ],
            true,
            &[],
            100,
        );
        let names = available
            .commands
            .iter()
            .map(|command| command.name.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"steer"));
        assert!(names.contains(&"queue"));
        assert!(!names.contains(&"resume"));
        assert!(!names.contains(&"compact"));
    }

    #[test]
    fn dynamic_commands_are_capped_after_core_commands() {
        let dynamic = (0..5)
            .map(|idx| DynamicSlashCommand {
                name: format!("skill-{idx}"),
                summary: "skill".to_string(),
                prompt: format!("$skill-{idx} "),
            })
            .collect::<Vec<_>>();
        let available = available_slash_commands_for_surface(&[], false, &dynamic, 3);
        assert_eq!(available.commands.len(), 3);
        assert_eq!(available.hidden_dynamic, 5);
    }

    #[test]
    fn shared_effect_parses_text_forms() {
        let SlashCommandParse::Known(invocation) = parse_slash_command_line("/tools enable web")
        else {
            panic!("expected known command");
        };
        let effect = slash_invocation_effect(
            &invocation,
            &[CommandCapability::ConfigWrite],
            SlashCommandSurface::Acp,
            false,
        )
        .expect("effect");
        assert_eq!(
            effect,
            SlashCommandEffect::ToolsetSet {
                name: "web".to_string(),
                enabled: true,
            }
        );
    }
}
