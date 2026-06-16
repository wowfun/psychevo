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
                    "{} opens a side chat and is unavailable until this surface has an active session.",
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
                | SlashCommandAction::Btw
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
