const TUI_CYAN: Color = Color::Cyan;
const TUI_MAGENTA: Color = Color::Magenta;
const TUI_RED: Color = Color::Red;
const TUI_DIM: Color = Color::DarkGray;
const TUI_PAPER: Color = Color::Rgb(216, 205, 184);
const TUI_SURFACE_BG: Color = Color::Rgb(38, 38, 38);
const TUI_SELECTION_BG: Color = Color::Rgb(62, 88, 105);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TranscriptKind {
    Prompt,
    Answer,
    Thinking,
    Explored,
    Ran,
    Updated,
    Meta,
    Command,
    Status,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct TranscriptRowId(u64);

static NEXT_TRANSCRIPT_ROW_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum TranscriptHitTarget {
    Row(TranscriptRowId),
    AgentOpen(TranscriptRowId),
}

#[derive(Debug, Clone, Copy)]
struct TranscriptRenderBlock {
    index: usize,
    target: TranscriptHitTarget,
    kind: TranscriptKind,
}

#[derive(Debug, Clone)]
struct TranscriptRow {
    id: TranscriptRowId,
    kind: TranscriptKind,
    title: String,
    text: String,
    full_text: Option<String>,
    expanded: bool,
    details_collapsed: bool,
    failed: bool,
    interrupted: bool,
    user_shell: bool,
    tool_call_id: Option<String>,
    tool_name: Option<String>,
    agent_target: Option<String>,
    agent_child_tool_uses: i64,
    agent_child_latest_tokens: Option<u64>,
    agent_child_live_text: String,
    tool_started: Option<Instant>,
    tool_elapsed: Option<Duration>,
}

#[derive(Debug, Clone, Default)]
struct TranscriptLayoutCache {
    width: u16,
    thinking_visible: bool,
    raw_visible: bool,
    blocks: Vec<TranscriptLayoutBlock>,
    total_height: usize,
    #[cfg(test)]
    recomputed_rows: usize,
}

impl TranscriptLayoutCache {
    fn max_scroll(&self, viewport_height: u16) -> u16 {
        let total = self.total_height.min(usize::from(u16::MAX)) as u16;
        total.saturating_sub(viewport_height)
    }
}

#[derive(Debug, Clone)]
struct TranscriptLayoutBlock {
    key: TranscriptLayoutBlockKey,
    target: TranscriptHitTarget,
    start: usize,
    height: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptLayoutBlockKey {
    target: TranscriptHitTarget,
    compact_trailing: bool,
    selected: bool,
    row_key: TranscriptLayoutRowKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptLayoutRowKey {
    visible: bool,
    compact_trailing: bool,
    selected: bool,
    kind: TranscriptKind,
    failed: bool,
    interrupted: bool,
    user_shell: bool,
    agent_tool: bool,
    agent_open: bool,
    expanded: bool,
    details_collapsed: bool,
    expandable: bool,
    tool_elapsed_hash: u64,
    active_tool_marker_hash: u64,
    title_hash: u64,
    text_hash: u64,
}

impl TranscriptRow {
    fn simple(kind: TranscriptKind, text: impl Into<String>) -> Self {
        let title = default_title(kind).to_string();
        Self::with_title(kind, title, text)
    }

    fn with_title(kind: TranscriptKind, title: impl Into<String>, text: impl Into<String>) -> Self {
        let mut row = Self {
            id: TranscriptRowId(NEXT_TRANSCRIPT_ROW_ID.fetch_add(1, Ordering::Relaxed)),
            kind,
            title: title.into(),
            text: text.into(),
            full_text: None,
            expanded: false,
            details_collapsed: false,
            failed: false,
            interrupted: false,
            user_shell: false,
            tool_call_id: None,
            tool_name: None,
            agent_target: None,
            agent_child_tool_uses: 0,
            agent_child_latest_tokens: None,
            agent_child_live_text: String::new(),
            tool_started: None,
            tool_elapsed: None,
        };
        row.apply_default_evidence_collapse();
        row
    }

    fn expandable_text(&self) -> &str {
        if self.expanded {
            self.full_text.as_deref().unwrap_or(&self.text)
        } else {
            &self.text
        }
    }

    fn is_expandable(&self) -> bool {
        self.full_text
            .as_ref()
            .is_some_and(|full| full != &self.text)
            || foldable_evidence_body(self)
            || foldable_tool_title(self)
    }

    fn apply_default_evidence_collapse(&mut self) {
        if !matches!(
            self.kind,
            TranscriptKind::Thinking
                | TranscriptKind::Explored
                | TranscriptKind::Ran
                | TranscriptKind::Updated
        ) || self.expanded
        {
            return;
        }
        let source = self
            .full_text
            .as_ref()
            .cloned()
            .unwrap_or_else(|| self.text.clone());
        if !ledger_body_collapse_policy().should_collapse(&source) {
            return;
        }
        let collapsed = ledger_body_collapse_policy().collapse(&source);
        self.text = collapsed.preview;
        self.full_text = collapsed.full_text;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusMode {
    Composer,
    Transcript,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MouseWheelTarget {
    Transcript,
    BottomPanel,
}

struct FullscreenUi<'a> {
    textarea: TextArea<'a>,
    workdir: PathBuf,
    transcript: Vec<TranscriptRow>,
    assistant_row: Option<usize>,
    reasoning_row: Option<usize>,
    meta_row: Option<usize>,
    tool_rows: BTreeMap<String, usize>,
    streaming_tool_message_seq: u64,
    streaming_tool_message_open: bool,
    deferred_stream_events: VecDeque<RunStreamEvent>,
    history_tool_titles: BTreeMap<String, String>,
    shell_mode: bool,
    turn_started: Option<Instant>,
    turn_provider: String,
    turn_model: String,
    turn_mode: String,
    turn_context_limit: Option<u64>,
    turn_usage: Option<Value>,
    turn_metadata: Option<Value>,
    turn_accounting: Option<Value>,
    turn_failures: usize,
    turn_interrupted: bool,
    turn_outcome: Option<Outcome>,
    turn_terminal_message: Option<String>,
    turn_had_reasoning: bool,
    history_prompt_started_ms: Option<i64>,
    loaded_session_message_count: usize,
    thinking_visible: bool,
    raw_visible: bool,
    running: Option<RunningTurn>,
    auxiliary_agent_tasks: Vec<AuxiliaryAgentTask>,
    agent_child_event_backlog: BTreeMap<String, Vec<RunStreamEvent>>,
    session_live_event_backlog: BTreeMap<String, Vec<RunStreamEvent>>,
    auxiliary_shell_tasks: Vec<AuxiliaryShellTask>,
    pending_auxiliary_shell_commands: VecDeque<String>,
    visible_turn_started: Option<Instant>,
    #[cfg(test)]
    running_elapsed_override: Option<Duration>,
    interrupt_requested: bool,
    scroll: u16,
    last_transcript_height: u16,
    last_transcript_width: u16,
    transcript_layout: TranscriptLayoutCache,
    auto_follow_transcript: bool,
    pending_scroll_to_bottom: bool,
    focus: FocusMode,
    selected_row: Option<usize>,
    selected_target: Option<TranscriptHitTarget>,
    last_transcript_area: Option<Rect>,
    last_composer_area: Option<Rect>,
    last_status_area: Option<Rect>,
    last_bottom_panel_area: Option<Rect>,
    last_entry_areas: Vec<(TranscriptHitTarget, Rect)>,
    mouse_down_target: Option<TranscriptHitTarget>,
    mouse_dragged: bool,
    sidebar_forced: bool,
    sidebar_hidden: bool,
    last_sidebar_visible: bool,
    sidebar: SidebarSnapshot,
    sidebar_tokens: Option<u64>,
    sidebar_context_limit: Option<u64>,
    last_context_snapshot: Option<ContextSnapshot>,
    sidebar_cost_nanodollars: Option<i64>,
    history: Vec<String>,
    history_kinds: Vec<ComposerHistoryKind>,
    history_index: Option<usize>,
    history_draft: Option<String>,
    queued_inputs: VecDeque<QueuedInput>,
    pending_images: Vec<PendingImageAttachment>,
    history_search: bool,
    history_query: String,
    slash_menu_selected: usize,
    slash_menu_dismissed_input: Option<String>,
    pending_leader_started: Option<Instant>,
    last_slash_menu_areas: Vec<(usize, Rect)>,
    file_search: FileSearchState,
    last_file_popup_areas: Vec<(usize, Rect)>,
    agent_search: AgentSearchState,
    last_agent_popup_areas: Vec<(usize, Rect)>,
    skill_search: SkillSearchState,
    last_skill_popup_areas: Vec<(usize, Rect)>,
    last_bottom_panel_areas: Vec<(usize, Rect)>,
    bottom_panel: Option<BottomPanel>,
    ephemeral_status: Option<UiEphemeralStatus>,
    screen_lines: Vec<ScreenLine>,
    selection: SelectionState,
    terminal_clear_requested: bool,
    quit_requested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UiEphemeralStatus {
    text: String,
    failed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComposerHistoryKind {
    SessionPrompt,
    ProcessCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScreenLine {
    region: SelectableRegion,
    y: u16,
    cells: Vec<ScreenCell>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectableRegion {
    Transcript,
    Sidebar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScreenCell {
    x: u16,
    width: u16,
    text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SelectionState {
    anchor: Option<(u16, u16)>,
    focus: Option<(u16, u16)>,
    region: Option<SelectableRegion>,
}

#[derive(Debug, Clone, Default)]
struct SidebarSnapshot {
    title: String,
    session: String,
    branch: String,
    changed_files: Vec<String>,
}

#[derive(Debug, Clone)]
struct BottomSelectionPanel {
    title: String,
    empty_label: String,
    footer: String,
    notice: Option<String>,
    session_view: Option<SessionListView>,
    action_armed: bool,
    delete_confirm: Option<String>,
    rows: Vec<BottomSelectionRow>,
    query: String,
    selected: usize,
    scroll: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionListView {
    Active,
    Archived,
}

#[derive(Debug, Clone)]
struct AgentPanel {
    running: BottomSelectionPanel,
    available: BottomSelectionPanel,
    tab: AgentTab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentTab {
    Running,
    Available,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentAction {
    UseAsMain,
    Run,
    View,
    Update,
    Delete,
}

#[derive(Debug, Clone)]
struct AgentRunPromptPanel {
    agent_name: String,
    prompt: String,
    notice: Option<String>,
}

#[derive(Debug, Clone)]
struct AgentEditorPanel {
    mode: AgentEditorMode,
    name: String,
    description: String,
    instructions: String,
    model: String,
    tools: String,
    permission_mode: String,
    background: bool,
    max_spawn_depth: String,
    active_field: AgentEditorField,
    notice: Option<String>,
}

#[derive(Debug, Clone)]
enum AgentEditorMode {
    Create,
    Update { path: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentEditorField {
    Name,
    Description,
    Instructions,
    Model,
    Tools,
    PermissionMode,
    Background,
    MaxSpawnDepth,
}

impl AgentPanel {
    fn new(running: BottomSelectionPanel, available: BottomSelectionPanel) -> Self {
        Self {
            running,
            available,
            tab: AgentTab::Running,
        }
    }

    fn move_tab(&mut self, direction: isize) {
        let current = self.tab_index() as isize;
        let next = (current + direction).rem_euclid(Self::tabs().len() as isize) as usize;
        self.tab = Self::tabs()[next];
    }

    fn tab_index(&self) -> usize {
        Self::tabs()
            .iter()
            .position(|tab| *tab == self.tab)
            .unwrap_or(0)
    }

    fn tabs() -> &'static [AgentTab] {
        &[AgentTab::Running, AgentTab::Available]
    }

    fn selection(&self) -> &BottomSelectionPanel {
        match self.tab {
            AgentTab::Running => &self.running,
            AgentTab::Available => &self.available,
        }
    }

    fn selection_mut(&mut self) -> &mut BottomSelectionPanel {
        match self.tab {
            AgentTab::Running => &mut self.running,
            AgentTab::Available => &mut self.available,
        }
    }
}

impl AgentTab {
    fn label(self) -> &'static str {
        match self {
            AgentTab::Running => "Running",
            AgentTab::Available => "Available",
        }
    }
}

impl AgentAction {
    fn label(self) -> &'static str {
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
    fn new(agent_name: String) -> Self {
        Self {
            agent_name,
            prompt: String::new(),
            notice: None,
        }
    }
}

impl AgentEditorPanel {
    fn create() -> Self {
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

    fn move_field(&mut self, direction: isize) {
        let fields = AgentEditorField::fields();
        let current = fields
            .iter()
            .position(|field| *field == self.active_field)
            .unwrap_or(0) as isize;
        self.active_field =
            fields[(current + direction).rem_euclid(fields.len() as isize) as usize];
        self.notice = None;
    }

    fn insert_char(&mut self, ch: char) {
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

    fn backspace(&mut self) {
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
    fn fields() -> &'static [AgentEditorField] {
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

    fn label(self) -> &'static str {
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
struct BottomSelectionRow {
    label: String,
    description: Option<String>,
    detail: Option<String>,
    group: Option<String>,
    search_text: String,
    is_current: bool,
    is_default: bool,
    style: BottomRowStyle,
    footer: Option<String>,
    value: BottomSelectionValue,
}

#[derive(Debug, Clone)]
enum BottomSelectionValue {
    Session(String),
    AgentRunning {
        id: String,
        child_session_id: String,
    },
    AgentAvailable {
        name: String,
        source: AgentSource,
        path: Option<PathBuf>,
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
    FetchAllModels,
    FetchProvider(String),
    ProviderInfo(String),
    StatsRow(String),
    Model {
        model: Box<ConfiguredModel>,
        source: ModelRowSource,
    },
    Variant {
        model: String,
        variant: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BottomRowStyle {
    Normal,
    Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelRowSource {
    Local,
    Fetched,
    CurrentOnly,
}

#[derive(Debug, Clone)]
enum BottomPanel {
    Help(HelpPanel),
    Sessions(BottomSelectionPanel),
    Agents(AgentPanel),
    AgentActions(BottomSelectionPanel),
    AgentRunPrompt(AgentRunPromptPanel),
    AgentEditor(AgentEditorPanel),
    Models(ModelPanel),
    Stats(BottomSelectionPanel),
    ProviderWizard(ProviderWizardPanel),
    Variants {
        models: Box<ModelPanel>,
        panel: BottomSelectionPanel,
    },
}

#[derive(Debug, Clone)]
struct HelpPanel {
    sections: SlashHelpSections,
    tab: HelpTab,
    scroll: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HelpTab {
    General,
    Commands,
    CustomCommands,
}

#[derive(Debug, Clone)]
struct ModelPanel {
    models: BottomSelectionPanel,
    tab: ModelTab,
    info_scroll: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelTab {
    Models,
    Info,
}

#[derive(Debug, Clone)]
struct ProviderWizardPanel {
    label: String,
    provider_id: String,
    base_url: String,
    api_key: String,
    provider_id_touched: bool,
    api_key_env_present: bool,
    active_field: ProviderWizardField,
    notice: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderWizardField {
    Label,
    ProviderId,
    BaseUrl,
    ApiKey,
}

impl BottomSelectionPanel {
    fn new(title: &str, _subtitle: &str, empty_label: &str, rows: Vec<BottomSelectionRow>) -> Self {
        Self {
            title: title.to_string(),
            empty_label: empty_label.to_string(),
            footer: "Enter select  Esc close  Type search".to_string(),
            notice: None,
            session_view: None,
            action_armed: false,
            delete_confirm: None,
            rows,
            query: String::new(),
            selected: 0,
            scroll: 0,
        }
    }

    fn new_sessions(view: SessionListView, rows: Vec<BottomSelectionRow>) -> Self {
        let (title, empty_label, footer) = match view {
            SessionListView::Active => (
                "Active Sessions",
                "No active sessions",
                "Enter switch  Tab archived  Ctrl+K manage  Esc close  Type search",
            ),
            SessionListView::Archived => (
                "Archived Sessions",
                "No archived sessions",
                "Enter restore  Tab active  Ctrl+K manage  Esc close  Type search",
            ),
        };
        let mut panel = Self::new(title, "", empty_label, rows);
        panel.session_view = Some(view);
        panel.footer = footer.to_string();
        panel
    }

    fn new_agent_actions(agent_name: &str, rows: Vec<BottomSelectionRow>) -> Self {
        let mut panel = Self::new(
            &format!("Agent {agent_name}"),
            "",
            "No actions available",
            rows,
        );
        panel.footer = "Enter select  Esc back".to_string();
        panel
    }

    fn filtered_indices(&self) -> Vec<usize> {
        let query = self.query.trim().to_lowercase();
        if self
            .rows
            .iter()
            .any(|row| matches!(row.value, BottomSelectionValue::FetchAllModels))
        {
            return self.filtered_model_indices(&query);
        }
        self.rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                if query.is_empty() || row.search_text.to_lowercase().contains(&query) {
                    Some(index)
                } else {
                    None
                }
            })
            .collect()
    }

    fn filtered_model_indices(&self, query: &str) -> Vec<usize> {
        if query.is_empty() {
            return (0..self.rows.len()).collect();
        }
        let mut include = BTreeMap::new();
        let mut provider_rows = BTreeMap::new();
        for (index, row) in self.rows.iter().enumerate() {
            match &row.value {
                BottomSelectionValue::AgentCreate => {
                    include.insert(index, ());
                }
                BottomSelectionValue::AddProvider => {
                    include.insert(index, ());
                }
                BottomSelectionValue::FetchAllModels => {
                    include.insert(index, ());
                }
                BottomSelectionValue::ProviderInfo(provider) if provider == "all" => {
                    include.insert(index, ());
                }
                BottomSelectionValue::FetchProvider(provider)
                | BottomSelectionValue::ProviderInfo(provider) => {
                    provider_rows.insert(provider.clone(), index);
                    if row.search_text.to_lowercase().contains(query)
                        || row.label.to_lowercase().contains(query)
                    {
                        include.insert(index, ());
                        for (model_index, model_row) in self.rows.iter().enumerate() {
                            if let BottomSelectionValue::Model { model, .. } = &model_row.value
                                && &model.provider == provider
                            {
                                include.insert(model_index, ());
                            }
                        }
                    }
                }
                BottomSelectionValue::Model { model, .. } => {
                    if row.search_text.to_lowercase().contains(query)
                        || row.label.to_lowercase().contains(query)
                    {
                        include.insert(index, ());
                        if let Some(provider_index) = provider_rows.get(&model.provider) {
                            include.insert(*provider_index, ());
                        }
                    }
                }
                _ => {}
            }
        }
        include.into_keys().collect()
    }

    fn selected_value(&self) -> Option<BottomSelectionValue> {
        self.filtered_indices()
            .get(self.selected)
            .and_then(|index| self.rows.get(*index))
            .map(|row| row.value.clone())
    }

    fn selected_row(&self) -> Option<&BottomSelectionRow> {
        self.filtered_indices()
            .get(self.selected)
            .and_then(|index| self.rows.get(*index))
    }

    fn selected_key(&self) -> String {
        self.selected_value()
            .map(|value| value.key())
            .unwrap_or_else(|| "fetch:all".to_string())
    }

    fn select_value_key(&mut self, key: &str) {
        let filtered = self.filtered_indices();
        if let Some(index) = filtered
            .iter()
            .position(|row_index| self.rows[*row_index].value.key() == key)
        {
            self.selected = index;
            self.ensure_selected_visible(8);
        }
    }

    fn footer_text(&self) -> String {
        if self.delete_confirm.is_some() {
            return "Ctrl+K D confirm delete  Esc cancel".to_string();
        }
        if self.action_armed {
            return match self.session_view.unwrap_or(SessionListView::Active) {
                SessionListView::Active => "A archive  D delete  Esc cancel".to_string(),
                SessionListView::Archived => "R restore  D delete  Esc cancel".to_string(),
            };
        }
        self.filtered_indices()
            .get(self.selected)
            .and_then(|index| self.rows.get(*index))
            .and_then(|row| row.footer.clone())
            .unwrap_or_else(|| self.footer.clone())
    }

    fn set_query_char(&mut self, c: char) {
        self.query.push(c);
        self.selected = 0;
        self.scroll = 0;
        self.clear_transient_action_state();
    }

    fn backspace_query(&mut self) {
        self.query.pop();
        self.selected = 0;
        self.scroll = 0;
        self.clear_transient_action_state();
    }

    fn move_selection(&mut self, direction: isize) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.selected = 0;
            self.clear_transient_action_state();
            return;
        }
        let current = self.selected.min(len.saturating_sub(1)) as isize;
        self.selected = (current + direction).rem_euclid(len as isize) as usize;
        self.clear_transient_action_state();
        self.ensure_selected_visible(8);
    }

    fn move_to(&mut self, index: usize) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.selected = 0;
            self.scroll = 0;
            self.clear_transient_action_state();
            return;
        }
        self.selected = index.min(len.saturating_sub(1));
        self.clear_transient_action_state();
        self.ensure_selected_visible(8);
    }

    fn ensure_selected_visible(&mut self, visible_rows: u16) {
        let selected = self.selected as u16;
        if selected < self.scroll {
            self.scroll = selected;
        }
        if selected >= self.scroll.saturating_add(visible_rows) {
            self.scroll = selected.saturating_sub(visible_rows.saturating_sub(1));
        }
        self.clamp_scroll(visible_rows);
    }

    fn clamp_scroll(&mut self, visible_rows: u16) {
        let len = self.filtered_indices().len() as u16;
        let max = len.saturating_sub(visible_rows);
        self.scroll = self.scroll.min(max);
    }

    fn set_selected(&mut self, index: usize) {
        self.selected = index.min(self.filtered_indices().len().saturating_sub(1));
        self.scroll = 0;
        self.clear_transient_action_state();
    }

    fn arm_action_mode(&mut self) {
        if self.session_view.is_none() {
            return;
        }
        self.action_armed = true;
        if self.delete_confirm.is_none() {
            self.notice = Some(match self.session_view.unwrap_or(SessionListView::Active) {
                SessionListView::Active => "action: A archive  D delete".to_string(),
                SessionListView::Archived => "action: R restore  D delete".to_string(),
            });
        }
    }

    fn cancel_transient_action(&mut self) -> bool {
        let had_transient = self.action_armed || self.delete_confirm.is_some();
        if had_transient {
            self.clear_transient_action_state();
        }
        had_transient
    }

    fn clear_transient_action_state(&mut self) {
        self.action_armed = false;
        self.delete_confirm = None;
        self.notice = None;
    }
}

impl BottomSelectionValue {
    fn key(&self) -> String {
        match self {
            BottomSelectionValue::Session(id) => format!("session:{id}"),
            BottomSelectionValue::AgentRunning {
                child_session_id, ..
            } => {
                format!("agent:running:{child_session_id}")
            }
            BottomSelectionValue::AgentAvailable {
                name,
                source,
                path,
                shadowed,
            } => format!(
                "agent:available:{name}:{}:{}:{}",
                source.as_str(),
                path.as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
                shadowed
            ),
            BottomSelectionValue::AgentAction {
                name, action, path, ..
            } => format!(
                "agent:action:{name}:{action:?}:{}",
                path.as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default()
            ),
            BottomSelectionValue::AgentMainDefault => "agent:main-default".to_string(),
            BottomSelectionValue::AgentCreate => "agent:create".to_string(),
            BottomSelectionValue::AgentSpawningToggle => "agent:spawning-toggle".to_string(),
            BottomSelectionValue::AgentDiagnostic(message) => {
                format!("agent:diagnostic:{message}")
            }
            BottomSelectionValue::AddProvider => "provider:add".to_string(),
            BottomSelectionValue::FetchAllModels => "fetch:all".to_string(),
            BottomSelectionValue::FetchProvider(provider) => {
                format!("fetch:provider:{provider}")
            }
            BottomSelectionValue::ProviderInfo(provider) => {
                if provider == "all" {
                    "fetch:all".to_string()
                } else {
                    format!("fetch:provider:{provider}")
                }
            }
            BottomSelectionValue::StatsRow(key) => format!("stats:{key}"),
            BottomSelectionValue::Model { model, .. } => {
                format!("model:{}", format_model_spec(model))
            }
            BottomSelectionValue::Variant { model, variant } => {
                format!(
                    "variant:{model}:{}",
                    variant.as_deref().unwrap_or("default")
                )
            }
        }
    }
}

impl BottomPanel {
    fn selection(&self) -> &BottomSelectionPanel {
        match self {
            BottomPanel::Sessions(panel) | BottomPanel::Stats(panel) => panel,
            BottomPanel::Agents(panel) => match panel.tab {
                AgentTab::Running => &panel.running,
                AgentTab::Available => &panel.available,
            },
            BottomPanel::AgentActions(panel) => panel,
            BottomPanel::Models(panel) => &panel.models,
            BottomPanel::Help(_) => {
                panic!("help panel does not expose a selection panel")
            }
            BottomPanel::AgentRunPrompt(_) | BottomPanel::AgentEditor(_) => {
                panic!("agent form panel does not expose a selection panel")
            }
            BottomPanel::ProviderWizard(_) => {
                panic!("provider wizard does not expose a selection panel")
            }
            BottomPanel::Variants { panel, .. } => panel,
        }
    }

    fn selection_mut(&mut self) -> &mut BottomSelectionPanel {
        match self {
            BottomPanel::Sessions(panel) | BottomPanel::Stats(panel) => panel,
            BottomPanel::Agents(panel) => match panel.tab {
                AgentTab::Running => &mut panel.running,
                AgentTab::Available => &mut panel.available,
            },
            BottomPanel::AgentActions(panel) => panel,
            BottomPanel::Models(panel) => &mut panel.models,
            BottomPanel::Help(_) => {
                panic!("help panel does not expose a selection panel")
            }
            BottomPanel::AgentRunPrompt(_) | BottomPanel::AgentEditor(_) => {
                panic!("agent form panel does not expose a selection panel")
            }
            BottomPanel::ProviderWizard(_) => {
                panic!("provider wizard does not expose a selection panel")
            }
            BottomPanel::Variants { panel, .. } => panel,
        }
    }

    fn selected_value(&self) -> Option<BottomSelectionValue> {
        self.selection().selected_value()
    }

    fn session_view(&self) -> Option<SessionListView> {
        match self {
            BottomPanel::Sessions(panel) => panel.session_view,
            BottomPanel::Help(_)
            | BottomPanel::Agents(_)
            | BottomPanel::AgentActions(_)
            | BottomPanel::AgentRunPrompt(_)
            | BottomPanel::AgentEditor(_)
            | BottomPanel::Models(_)
            | BottomPanel::Stats(_)
            | BottomPanel::ProviderWizard(_)
            | BottomPanel::Variants { .. } => None,
        }
    }

    fn move_selection(&mut self, direction: isize) {
        self.selection_mut().move_selection(direction);
    }
}

impl HelpPanel {
    fn new(sections: SlashHelpSections) -> Self {
        Self {
            sections,
            tab: HelpTab::General,
            scroll: 0,
        }
    }

    fn move_tab(&mut self, direction: isize) {
        let current = self.tab_index() as isize;
        let next = (current + direction).rem_euclid(Self::tabs().len() as isize) as usize;
        self.tab = Self::tabs()[next];
        self.scroll = 0;
    }

    fn set_tab(&mut self, tab: HelpTab) {
        self.tab = tab;
        self.scroll = 0;
    }

    fn scroll_by(&mut self, direction: isize) {
        if direction.is_negative() {
            self.scroll = self.scroll.saturating_sub(direction.unsigned_abs() as u16);
        } else {
            self.scroll = self.scroll.saturating_add(direction as u16);
        }
    }

    fn tab_index(&self) -> usize {
        Self::tabs()
            .iter()
            .position(|tab| *tab == self.tab)
            .unwrap_or(0)
    }

    fn tabs() -> &'static [HelpTab] {
        &[HelpTab::General, HelpTab::Commands, HelpTab::CustomCommands]
    }
}

impl HelpTab {
    fn label(self) -> &'static str {
        match self {
            HelpTab::General => "General",
            HelpTab::Commands => "Commands",
            HelpTab::CustomCommands => "Custom commands",
        }
    }
}

impl ModelPanel {
    fn new(models: BottomSelectionPanel) -> Self {
        Self {
            models,
            tab: ModelTab::Models,
            info_scroll: 0,
        }
    }

    fn move_tab(&mut self, direction: isize) {
        let current = self.tab_index() as isize;
        let next = (current + direction).rem_euclid(Self::tabs().len() as isize) as usize;
        self.tab = Self::tabs()[next];
    }

    fn scroll_info_by(&mut self, direction: isize) {
        if direction.is_negative() {
            self.info_scroll = self
                .info_scroll
                .saturating_sub(direction.unsigned_abs() as u16);
        } else {
            self.info_scroll = self.info_scroll.saturating_add(direction as u16);
        }
    }

    fn tab_index(&self) -> usize {
        Self::tabs()
            .iter()
            .position(|tab| *tab == self.tab)
            .unwrap_or(0)
    }

    fn tabs() -> &'static [ModelTab] {
        &[ModelTab::Models, ModelTab::Info]
    }
}

impl ModelTab {
    fn label(self) -> &'static str {
        match self {
            ModelTab::Models => "Models",
            ModelTab::Info => "Info",
        }
    }
}

impl ProviderWizardPanel {
    fn new() -> Self {
        Self {
            label: String::new(),
            provider_id: String::new(),
            base_url: String::new(),
            api_key: String::new(),
            provider_id_touched: false,
            api_key_env_present: false,
            active_field: ProviderWizardField::Label,
            notice: None,
        }
    }

    fn active_fields(&self) -> Vec<ProviderWizardField> {
        let mut fields = vec![
            ProviderWizardField::Label,
            ProviderWizardField::ProviderId,
            ProviderWizardField::BaseUrl,
        ];
        if !self.api_key_env_present {
            fields.push(ProviderWizardField::ApiKey);
        }
        fields
    }

    fn move_field(&mut self, direction: isize) {
        let fields = self.active_fields();
        let current = fields
            .iter()
            .position(|field| *field == self.active_field)
            .unwrap_or(0) as isize;
        self.active_field =
            fields[(current + direction).rem_euclid(fields.len() as isize) as usize];
        self.notice = None;
    }

    fn move_to_first_field(&mut self) {
        self.active_field = ProviderWizardField::Label;
        self.notice = None;
    }

    fn move_to_last_field(&mut self) {
        self.active_field = *self
            .active_fields()
            .last()
            .unwrap_or(&ProviderWizardField::BaseUrl);
        self.notice = None;
    }

    fn insert_char(&mut self, ch: char) {
        match self.active_field {
            ProviderWizardField::Label => {
                self.label.push(ch);
                if !self.provider_id_touched {
                    self.provider_id = provider_id_slug(&self.label);
                }
            }
            ProviderWizardField::ProviderId => {
                self.provider_id.push(ch);
                self.provider_id_touched = true;
            }
            ProviderWizardField::BaseUrl => self.base_url.push(ch),
            ProviderWizardField::ApiKey => self.api_key.push(ch),
        }
        self.notice = None;
    }

    fn backspace(&mut self) {
        match self.active_field {
            ProviderWizardField::Label => {
                self.label.pop();
                if !self.provider_id_touched {
                    self.provider_id = provider_id_slug(&self.label);
                }
            }
            ProviderWizardField::ProviderId => {
                self.provider_id.pop();
                self.provider_id_touched = true;
            }
            ProviderWizardField::BaseUrl => {
                self.base_url.pop();
            }
            ProviderWizardField::ApiKey => {
                self.api_key.pop();
            }
        }
        self.notice = None;
    }

    fn env_var(&self) -> Option<String> {
        (!self.provider_id.trim().is_empty())
            .then(|| custom_provider_api_key_env(self.provider_id.trim()))
    }

    fn is_last_field(&self) -> bool {
        self.active_fields()
            .last()
            .is_some_and(|field| *field == self.active_field)
    }
}

fn provider_id_slug(label: &str) -> String {
    let mut slug = String::new();
    let mut previous_sep = false;
    for ch in label.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            previous_sep = false;
        } else if matches!(ch, '-' | '_' | ' ' | '.' | '/' | ':')
            && !previous_sep
            && !slug.is_empty()
        {
            slug.push('-');
            previous_sep = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}
