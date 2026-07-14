impl AgentPanel {
    pub(crate) fn new(running: BottomSelectionPanel, available: BottomSelectionPanel) -> Self {
        Self {
            running,
            available,
            tab: AgentTab::Running,
        }
    }

    pub(crate) fn move_tab(&mut self, direction: isize) {
        let current = self.tab_index() as isize;
        let next = (current + direction).rem_euclid(Self::tabs().len() as isize) as usize;
        self.tab = Self::tabs()[next];
    }

    pub(crate) fn tab_index(&self) -> usize {
        Self::tabs()
            .iter()
            .position(|tab| *tab == self.tab)
            .unwrap_or(0)
    }

    pub(crate) fn tabs() -> &'static [AgentTab] {
        &[AgentTab::Running, AgentTab::Available]
    }

    pub(crate) fn selection(&self) -> &BottomSelectionPanel {
        match self.tab {
            AgentTab::Running => &self.running,
            AgentTab::Available => &self.available,
        }
    }

    pub(crate) fn selection_mut(&mut self) -> &mut BottomSelectionPanel {
        match self.tab {
            AgentTab::Running => &mut self.running,
            AgentTab::Available => &mut self.available,
        }
    }
}

impl AgentTab {
    pub(crate) fn label(self) -> &'static str {
        match self {
            AgentTab::Running => "Running",
            AgentTab::Available => "Available",
        }
    }
}

impl AgentAction {
    pub(crate) fn label(self) -> &'static str {
        match self {
            AgentAction::UseAsMain => "Use as main",
            AgentAction::Run => "Run",
            AgentAction::View => "View",
            AgentAction::Update => "Update",
            AgentAction::Delete => "Delete",
        }
    }
}

impl AgentRunPromptPanel {
    pub(crate) fn new(agent_name: String) -> Self {
        Self {
            agent_name,
            prompt: String::new(),
            notice: None,
        }
    }
}

impl AgentEditorPanel {
    pub(crate) fn create() -> Self {
        Self {
            mode: AgentEditorMode::Create,
            name: String::new(),
            description: String::new(),
            instructions: String::new(),
            model: String::new(),
            tools: String::new(),
            permission_mode: String::new(),
            background: false,
            max_spawn_depth: "0".to_string(),
            active_field: AgentEditorField::Name,
            notice: None,
        }
    }

    pub(crate) fn move_field(&mut self, direction: isize) {
        let fields = AgentEditorField::fields();
        let current = fields
            .iter()
            .position(|field| *field == self.active_field)
            .unwrap_or(0) as isize;
        self.active_field =
            fields[(current + direction).rem_euclid(fields.len() as isize) as usize];
        self.notice = None;
    }

    pub(crate) fn insert_char(&mut self, ch: char) {
        match self.active_field {
            AgentEditorField::Name => self.name.push(ch),
            AgentEditorField::Description => self.description.push(ch),
            AgentEditorField::Instructions => self.instructions.push(ch),
            AgentEditorField::Model => self.model.push(ch),
            AgentEditorField::Tools => self.tools.push(ch),
            AgentEditorField::PermissionMode => self.permission_mode.push(ch),
            AgentEditorField::MaxSpawnDepth => self.max_spawn_depth.push(ch),
            AgentEditorField::Background => {
                if matches!(ch, 'y' | 'Y' | 't' | 'T' | '1') {
                    self.background = true;
                } else if matches!(ch, 'n' | 'N' | 'f' | 'F' | '0') {
                    self.background = false;
                }
            }
        }
        self.notice = None;
    }

    pub(crate) fn backspace(&mut self) {
        match self.active_field {
            AgentEditorField::Name => {
                self.name.pop();
            }
            AgentEditorField::Description => {
                self.description.pop();
            }
            AgentEditorField::Instructions => {
                self.instructions.pop();
            }
            AgentEditorField::Model => {
                self.model.pop();
            }
            AgentEditorField::Tools => {
                self.tools.pop();
            }
            AgentEditorField::PermissionMode => {
                self.permission_mode.pop();
            }
            AgentEditorField::MaxSpawnDepth => {
                self.max_spawn_depth.pop();
            }
            AgentEditorField::Background => self.background = false,
        }
        self.notice = None;
    }
}

impl AgentEditorField {
    pub(crate) fn fields() -> &'static [AgentEditorField] {
        &[
            AgentEditorField::Name,
            AgentEditorField::Description,
            AgentEditorField::Instructions,
            AgentEditorField::Model,
            AgentEditorField::Tools,
            AgentEditorField::PermissionMode,
            AgentEditorField::Background,
            AgentEditorField::MaxSpawnDepth,
        ]
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            AgentEditorField::Name => "Name",
            AgentEditorField::Description => "Description",
            AgentEditorField::Instructions => "Instructions",
            AgentEditorField::Model => "Model",
            AgentEditorField::Tools => "Tools",
            AgentEditorField::PermissionMode => "Permission",
            AgentEditorField::Background => "Background",
            AgentEditorField::MaxSpawnDepth => "Max spawn depth",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BottomSelectionRow {
    pub(crate) label: String,
    pub(crate) description: Option<String>,
    pub(crate) detail: Option<String>,
    pub(crate) group: Option<String>,
    pub(crate) search_text: String,
    pub(crate) is_current: bool,
    pub(crate) is_default: bool,
    pub(crate) style: BottomRowStyle,
    pub(crate) footer: Option<String>,
    pub(crate) value: BottomSelectionValue,
}

#[derive(Debug, Clone)]
pub(crate) enum BottomSelectionValue {
    HistoryMessageAction {
        message_id: String,
        action: HistoryMessageAction,
    },
    Session(String),
    LoadOlderSessions(String),
    AgentRunning {
        id: String,
        child_session_id: String,
    },
    AgentAvailable {
        name: String,
        source: AgentSource,
        path: Option<PathBuf>,
        entrypoints: BTreeSet<AgentEntrypoint>,
        shadowed: bool,
    },
    AgentAction {
        name: String,
        source: AgentSource,
        path: Option<PathBuf>,
        shadowed: bool,
        action: AgentAction,
    },
    AgentMainDefault,
    AgentCreate,
    AgentSpawningToggle,
    AgentDiagnostic(String),
    AddProvider,
    ProviderPreset(ProviderSetupPresetId),
    ProviderBaseUrl {
        preset: ProviderSetupPresetId,
        index: Option<usize>,
    },
    FetchAllModels,
    FetchProvider(String),
    ProviderInfo(String),
    StatsRow(String),
    Toolset {
        name: String,
        enabled: bool,
    },
    Model {
        model: Box<ConfiguredModel>,
        source: ModelRowSource,
    },
    Variant {
        model: String,
        variant: Option<String>,
        reasoning_effort: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BottomRowStyle {
    Normal,
    Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModelRowSource {
    Local,
    Fetched,
    CurrentOnly,
}

#[derive(Debug, Clone)]
pub(crate) enum BottomPanel {
    Help(HelpPanel),
    Sessions(BottomSelectionPanel),
    Agents(AgentPanel),
    AgentActions(BottomSelectionPanel),
    AgentRunPrompt(AgentRunPromptPanel),
    AgentEditor(AgentEditorPanel),
    Models(ModelPanel),
    Stats(BottomSelectionPanel),
    Tools(BottomSelectionPanel),
    ProviderPresets(BottomSelectionPanel),
    ProviderBaseUrls(BottomSelectionPanel),
    ProviderWizard(ProviderWizardPanel),
    PermissionApproval(PermissionApprovalPanel),
    Clarify(ClarifyPanel),
    Variants {
        models: Box<ModelPanel>,
        panel: BottomSelectionPanel,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct HelpPanel {
    pub(crate) sections: SlashHelpSections,
    pub(crate) tab: HelpTab,
    pub(crate) scroll: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HelpTab {
    General,
    Commands,
    CustomCommands,
}

#[derive(Debug, Clone)]
pub(crate) struct ModelPanel {
    pub(crate) models: BottomSelectionPanel,
    pub(crate) tab: ModelTab,
    pub(crate) info_scroll: u16,
    pub(crate) global: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModelTab {
    Models,
    Info,
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderWizardPanel {
    pub(crate) preset: ProviderSetupPresetId,
    pub(crate) label: String,
    pub(crate) provider_id: String,
    pub(crate) base_url: String,
    pub(crate) api_key_env: String,
    pub(crate) api_key: String,
    pub(crate) provider_id_touched: bool,
    pub(crate) api_key_env_touched: bool,
    pub(crate) api_key_env_present: bool,
    pub(crate) active_field: ProviderWizardField,
    pub(crate) notice: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ClarifyPanel {
    pub(crate) request: ClarifyRequestEvent,
    pub(crate) question_index: usize,
    pub(crate) states: Vec<ClarifyQuestionState>,
    pub(crate) answers: Vec<Option<ClarifyAnswer>>,
    pub(crate) previous_panel: Option<Box<BottomPanel>>,
    pub(crate) notice: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct PermissionApprovalPanel {
    pub(crate) session_id: Option<String>,
    pub(crate) request: PermissionApprovalRequest,
    pub(crate) selected: usize,
    pub(crate) scroll: u16,
    pub(crate) previous_panel: Option<Box<BottomPanel>>,
    pub(crate) notice: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ClarifyQuestionState {
    pub(crate) selected: usize,
    pub(crate) mode: ClarifyInputMode,
    pub(crate) other_draft: String,
    pub(crate) other_cursor: usize,
    pub(crate) note_drafts: BTreeMap<usize, String>,
    pub(crate) note_cursors: BTreeMap<usize, usize>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ClarifyInputMode {
    #[default]
    Options,
    Other,
    Note,
}
