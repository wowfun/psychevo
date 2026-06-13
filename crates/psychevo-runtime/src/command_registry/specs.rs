#[allow(unused_imports)]
pub(crate) use super::*;
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
    OverlayPager,
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
    SessionRevert,
    ArtifactWrite,
    WorkspaceDiff,
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
    Diff,
    Refresh,
    Btw,
    Steer,
    Queue,
    Pending,
    ModelShow,
    VariantSet,
    ModeSet,
    Permissions,
    Sandbox,
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
    WebDesktop,
    Acp,
    Messaging,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandPresentationKind {
    Navigate,
    Inspect,
    Control,
    Submit,
    Export,
    Extension,
}

impl CommandPresentationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Navigate => "navigate",
            Self::Inspect => "inspect",
            Self::Control => "control",
            Self::Submit => "submit",
            Self::Export => "export",
            Self::Extension => "extension",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandDestination {
    Commands,
    History,
    Agents,
    Status,
    Preview,
    Composer,
    Download,
    None,
}

impl CommandDestination {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Commands => "commands",
            Self::History => "history",
            Self::Agents => "agents",
            Self::Status => "status",
            Self::Preview => "preview",
            Self::Composer => "composer",
            Self::Download => "download",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandFeedbackAnchor {
    Trigger,
    CommandsPanel,
    Composer,
    Status,
}

impl CommandFeedbackAnchor {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Trigger => "trigger",
            Self::CommandsPanel => "commandsPanel",
            Self::Composer => "composer",
            Self::Status => "status",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandAlternateActionType {
    OpenPanel,
    OpenComposerControl,
}

impl CommandAlternateActionType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenPanel => "openPanel",
            Self::OpenComposerControl => "openComposerControl",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandAlternateAction {
    pub action_type: CommandAlternateActionType,
    pub target: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandPresentation {
    pub kind: CommandPresentationKind,
    pub destination: CommandDestination,
    pub feedback_anchor: CommandFeedbackAnchor,
    pub alternate_action: Option<CommandAlternateAction>,
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
            SlashCommandAction::Undo | SlashCommandAction::Redo => {
                &[CommandCapability::SessionRevert]
            }
            SlashCommandAction::Tools => &[],
            SlashCommandAction::Permissions => &[],
            SlashCommandAction::Sandbox => &[],
            SlashCommandAction::Export | SlashCommandAction::Share => {
                &[CommandCapability::ArtifactWrite]
            }
            SlashCommandAction::Diff => &[CommandCapability::WorkspaceDiff],
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
                    SlashCommandSurface::WebDesktop => "Web/Desktop shell",
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

    pub fn presentation(self) -> CommandPresentation {
        command_presentation(self.action)
    }

    pub fn available_during_active_turn(self) -> bool {
        matches!(
            self.action,
            SlashCommandAction::Help
                | SlashCommandAction::Status
                | SlashCommandAction::Usage
                | SlashCommandAction::Context
                | SlashCommandAction::Diff
                | SlashCommandAction::Tools
                | SlashCommandAction::Sandbox
                | SlashCommandAction::Agents
                | SlashCommandAction::Steer
                | SlashCommandAction::Queue
                | SlashCommandAction::Pending
        )
    }
}

pub fn command_presentation(action: SlashCommandAction) -> CommandPresentation {
    use CommandDestination as Destination;
    use CommandFeedbackAnchor as Anchor;
    use CommandPresentationKind as Kind;
    match action {
        SlashCommandAction::Help => {
            presentation(Kind::Navigate, Destination::Commands, Anchor::CommandsPanel)
        }
        SlashCommandAction::Status
        | SlashCommandAction::Usage
        | SlashCommandAction::Context
        | SlashCommandAction::Sandbox => {
            presentation(Kind::Inspect, Destination::Status, Anchor::Status)
        }
        SlashCommandAction::New => {
            presentation(Kind::Navigate, Destination::Composer, Anchor::Composer)
        }
        SlashCommandAction::Sessions | SlashCommandAction::Resume => {
            presentation(Kind::Navigate, Destination::History, Anchor::CommandsPanel)
        }
        SlashCommandAction::Diff => {
            presentation(Kind::Inspect, Destination::Preview, Anchor::Trigger)
        }
        SlashCommandAction::Steer | SlashCommandAction::Queue | SlashCommandAction::Pending => {
            presentation(Kind::Control, Destination::Composer, Anchor::Composer)
        }
        SlashCommandAction::Agents => {
            presentation(Kind::Navigate, Destination::Agents, Anchor::CommandsPanel)
        }
        SlashCommandAction::Fork | SlashCommandAction::Compact => {
            presentation(Kind::Submit, Destination::Composer, Anchor::Composer)
        }
        SlashCommandAction::Export | SlashCommandAction::Share => {
            presentation(Kind::Export, Destination::Download, Anchor::Trigger)
        }
        SlashCommandAction::SkillInvoke => {
            presentation(Kind::Extension, Destination::Composer, Anchor::Composer)
        }
        SlashCommandAction::ModelShow
        | SlashCommandAction::VariantSet
        | SlashCommandAction::ModeSet => presentation_with_alternate(
            Kind::Control,
            Destination::Composer,
            Anchor::Composer,
            CommandAlternateAction {
                action_type: CommandAlternateActionType::OpenComposerControl,
                target: "model",
                label: "Open model controls",
            },
        ),
        SlashCommandAction::Image => presentation_with_alternate(
            Kind::Submit,
            Destination::Composer,
            Anchor::Composer,
            CommandAlternateAction {
                action_type: CommandAlternateActionType::OpenComposerControl,
                target: "attachments",
                label: "Add attachment",
            },
        ),
        SlashCommandAction::Permissions => presentation_with_alternate(
            Kind::Control,
            Destination::Status,
            Anchor::Status,
            CommandAlternateAction {
                action_type: CommandAlternateActionType::OpenPanel,
                target: "status",
                label: "Open status",
            },
        ),
        SlashCommandAction::Tools
        | SlashCommandAction::Skills
        | SlashCommandAction::Bundles
        | SlashCommandAction::Curator => presentation_with_alternate(
            Kind::Control,
            Destination::Commands,
            Anchor::CommandsPanel,
            CommandAlternateAction {
                action_type: CommandAlternateActionType::OpenPanel,
                target: "commands",
                label: "Open commands",
            },
        ),
        SlashCommandAction::Quit
        | SlashCommandAction::Btw
        | SlashCommandAction::Thinking
        | SlashCommandAction::Raw
        | SlashCommandAction::Copy
        | SlashCommandAction::Refresh
        | SlashCommandAction::Rename
        | SlashCommandAction::Undo
        | SlashCommandAction::Redo => {
            presentation(Kind::Control, Destination::None, Anchor::Composer)
        }
    }
}

fn presentation(
    kind: CommandPresentationKind,
    destination: CommandDestination,
    feedback_anchor: CommandFeedbackAnchor,
) -> CommandPresentation {
    CommandPresentation {
        kind,
        destination,
        feedback_anchor,
        alternate_action: None,
    }
}

fn presentation_with_alternate(
    kind: CommandPresentationKind,
    destination: CommandDestination,
    feedback_anchor: CommandFeedbackAnchor,
    alternate_action: CommandAlternateAction,
) -> CommandPresentation {
    CommandPresentation {
        kind,
        destination,
        feedback_anchor,
        alternate_action: Some(alternate_action),
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

pub(crate) const TUI_SLASH: CommandSurface = CommandSurface::TuiSlash;
pub(crate) const PEVO_CLI: CommandSurface = CommandSurface::PevoCli;
pub(crate) const COMMANDS: CommandGroup = CommandGroup::Commands;

pub const SLASH_COMMANDS: &[SlashCommandSpec] = &[
    SlashCommandSpec {
        canonical: "/help",
        aliases: &["/commands"],
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
        aliases: &["/history"],
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
        canonical: "/diff",
        aliases: &[],
        usage: "/diff",
        summary: "show workspace diff",
        help_detail: Some(
            "Shows a bounded local git diff snapshot for tracked unstaged changes and untracked files; it does not add a model message.",
        ),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::OverlayPager,
        status: CommandStatus::Active,
        action: SlashCommandAction::Diff,
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
        usage: "/model [--local|-g|--global]",
        summary: "configure model",
        help_detail: Some(
            "Opens the model picker to write a local default model, or a global default with -g/--global.",
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
        usage: "/mode <plan|default>",
        summary: "set collaboration mode",
        help_detail: Some(
            "Changes collaboration mode for future turns. Permission policy is managed with /permissions and pevo config.",
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
        canonical: "/sandbox",
        aliases: &[],
        usage: "/sandbox",
        summary: "show sandbox status",
        help_detail: Some(
            "Shows the effective sandbox mode, platform backend, and confined roots.",
        ),
        surface: TUI_SLASH,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::TranscriptStatusBlock,
        status: CommandStatus::Active,
        action: SlashCommandAction::Sandbox,
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
            "Writes a local artifact; last-provider-request can expose hidden prompts and tool data; last-provider-response is normalized from persisted data.",
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
    SandboxShow,
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
    Diff,
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableSlashCommand {
    pub name: String,
    pub usage: String,
    pub summary: String,
    pub aliases: Vec<String>,
    pub argument_kind: CommandArgumentKind,
    pub action: SlashCommandAction,
    pub presentation: CommandPresentation,
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
