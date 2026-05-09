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
    Changed,
    Meta,
    Status,
    Error,
}

#[derive(Debug, Clone)]
struct TranscriptRow {
    kind: TranscriptKind,
    title: String,
    text: String,
    full_text: Option<String>,
    expanded: bool,
    failed: bool,
    tool_call_id: Option<String>,
    tool_started: Option<Instant>,
    tool_elapsed: Option<Duration>,
}

#[derive(Debug, Clone, Default)]
struct TranscriptLayoutCache {
    width: u16,
    thinking_visible: bool,
    rows: Vec<TranscriptLayoutRow>,
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
struct TranscriptLayoutRow {
    key: TranscriptLayoutRowKey,
    start: usize,
    height: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptLayoutRowKey {
    visible: bool,
    compact_trailing: bool,
    selected: bool,
    kind: TranscriptKind,
    failed: bool,
    expanded: bool,
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
        Self {
            kind,
            title: title.into(),
            text: text.into(),
            full_text: None,
            expanded: false,
            failed: false,
            tool_call_id: None,
            tool_started: None,
            tool_elapsed: None,
        }
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
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusMode {
    Composer,
    Transcript,
}

struct FullscreenUi<'a> {
    textarea: TextArea<'a>,
    transcript: Vec<TranscriptRow>,
    assistant_row: Option<usize>,
    reasoning_row: Option<usize>,
    meta_row: Option<usize>,
    tool_rows: BTreeMap<String, usize>,
    streaming_tool_message_seq: u64,
    streaming_tool_message_open: bool,
    history_tool_titles: BTreeMap<String, String>,
    turn_started: Option<Instant>,
    turn_provider: String,
    turn_model: String,
    turn_mode: String,
    turn_context_limit: Option<u64>,
    turn_usage: Option<Value>,
    turn_metadata: Option<Value>,
    turn_failures: usize,
    turn_outcome: Option<Outcome>,
    history_prompt_started_ms: Option<i64>,
    thinking_visible: bool,
    running: Option<RunningTurn>,
    auxiliary_agent_tasks: Vec<JoinHandle<psychevo_runtime::Result<psychevo_runtime::RunResult>>>,
    running_started: Option<Instant>,
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
    last_entry_areas: Vec<(usize, Rect)>,
    sidebar_forced: bool,
    sidebar_hidden: bool,
    last_sidebar_visible: bool,
    sidebar: SidebarSnapshot,
    sidebar_tokens: Option<u64>,
    sidebar_context_limit: Option<u64>,
    history: Vec<String>,
    history_kinds: Vec<ComposerHistoryKind>,
    history_index: Option<usize>,
    history_draft: Option<String>,
    queued_inputs: VecDeque<QueuedInput>,
    history_search: bool,
    history_query: String,
    slash_menu_selected: usize,
    slash_menu_dismissed_input: Option<String>,
    last_slash_menu_areas: Vec<(usize, Rect)>,
    file_search: FileSearchState,
    last_file_popup_areas: Vec<(usize, Rect)>,
    skill_search: SkillSearchState,
    last_skill_popup_areas: Vec<(usize, Rect)>,
    last_bottom_panel_areas: Vec<(usize, Rect)>,
    bottom_panel: Option<BottomPanel>,
    screen_lines: Vec<ScreenLine>,
    selection: SelectionState,
    terminal_clear_requested: bool,
    quit_requested: bool,
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
    workdir: String,
    branch: String,
    tokens: Option<u64>,
    context_percent: Option<f64>,
    message_count: usize,
    tool_count: usize,
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
    AddProvider,
    FetchAllModels,
    FetchProvider(String),
    ProviderInfo(String),
    Model {
        model: ConfiguredModel,
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
    Sessions(BottomSelectionPanel),
    Models(BottomSelectionPanel),
    ProviderWizard(ProviderWizardPanel),
    Variants {
        models: Box<BottomSelectionPanel>,
        panel: BottomSelectionPanel,
    },
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
                "Enter select  Tab archived  Ctrl+K actions  Esc close  Type search",
            ),
            SessionListView::Archived => (
                "Archived Sessions",
                "No archived sessions",
                "Enter restore  Tab active  Ctrl+K actions  Esc close  Type search",
            ),
        };
        let mut panel = Self::new(title, "", empty_label, rows);
        panel.session_view = Some(view);
        panel.footer = footer.to_string();
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
            BottomPanel::Sessions(panel) | BottomPanel::Models(panel) => panel,
            BottomPanel::ProviderWizard(_) => {
                panic!("provider wizard does not expose a selection panel")
            }
            BottomPanel::Variants { panel, .. } => panel,
        }
    }

    fn selection_mut(&mut self) -> &mut BottomSelectionPanel {
        match self {
            BottomPanel::Sessions(panel) | BottomPanel::Models(panel) => panel,
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
            BottomPanel::Models(_)
            | BottomPanel::ProviderWizard(_)
            | BottomPanel::Variants { .. } => None,
        }
    }

    fn move_selection(&mut self, direction: isize) {
        self.selection_mut().move_selection(direction);
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
        self.active_field = fields[(current + direction).rem_euclid(fields.len() as isize) as usize];
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
        } else if matches!(ch, '-' | '_' | ' ' | '.' | '/' | ':') && !previous_sep && !slug.is_empty()
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
