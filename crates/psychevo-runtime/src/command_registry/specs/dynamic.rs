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
    Btw {
        prompt: Option<String>,
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
