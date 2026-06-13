#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) const TUI_CYAN: Color = Color::Cyan;
pub(crate) const TUI_MAGENTA: Color = Color::Magenta;
pub(crate) const TUI_RED: Color = Color::Red;
pub(crate) const TUI_DIM: Color = Color::DarkGray;
pub(crate) const TUI_PAPER: Color = Color::Rgb(216, 205, 184);
pub(crate) const TUI_SURFACE_BG: Color = Color::Rgb(38, 38, 38);
pub(crate) const TUI_SELECTION_BG: Color = Color::Rgb(62, 88, 105);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TranscriptKind {
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
pub(crate) struct TranscriptRowId(u64);

pub(crate) static NEXT_TRANSCRIPT_ROW_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum TranscriptHitTarget {
    Row(TranscriptRowId),
    AgentOpen(TranscriptRowId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PendingInputRef {
    Steer(PendingInputId),
    Queue(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PendingInputKind {
    Steer,
    Queue,
}

impl PendingInputKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Steer => "steer",
            Self::Queue => "queue",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingInputEntry {
    pub(crate) target: PendingInputRef,
    pub(crate) kind: PendingInputKind,
    pub(crate) text: String,
    pub(crate) images: Vec<PendingImageAttachment>,
    pub(crate) sequence: u64,
}

pub(crate) struct PendingInputEdit<'a> {
    pub(crate) target: PendingInputRef,
    pub(crate) kind: PendingInputKind,
    pub(crate) textarea: TextArea<'a>,
    pub(crate) images: Vec<PendingImageAttachment>,
    pub(crate) cursor_top_row: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PendingInputAction {
    Edit,
    Undo,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TranscriptRenderBlock {
    pub(crate) index: usize,
    pub(crate) target: TranscriptHitTarget,
    pub(crate) kind: TranscriptKind,
}

#[derive(Debug, Clone)]
pub(crate) struct TranscriptRow {
    pub(crate) id: TranscriptRowId,
    pub(crate) kind: TranscriptKind,
    pub(crate) title: String,
    pub(crate) text: String,
    pub(crate) full_text: Option<String>,
    pub(crate) expanded: bool,
    pub(crate) details_collapsed: bool,
    pub(crate) failed: bool,
    pub(crate) interrupted: bool,
    pub(crate) user_shell: bool,
    pub(crate) tool_call_id: Option<String>,
    pub(crate) tool_name: Option<String>,
    pub(crate) agent_target: Option<String>,
    pub(crate) agent_child_tool_uses: i64,
    pub(crate) agent_child_latest_tokens: Option<u64>,
    pub(crate) agent_child_live_text: String,
    pub(crate) tool_started: Option<Instant>,
    pub(crate) tool_elapsed: Option<Duration>,
    pub(crate) transcript_turn_id: Option<String>,
    pub(crate) transcript_source: Option<String>,
    pub(crate) transcript_entry_id: Option<String>,
    pub(crate) transcript_block_id: Option<String>,
    pub(crate) transcript_message_seq: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TranscriptLayoutCache {
    pub(crate) width: u16,
    pub(crate) thinking_visible: bool,
    pub(crate) raw_visible: bool,
    pub(crate) blocks: Vec<TranscriptLayoutBlock>,
    pub(crate) total_height: usize,
    #[cfg(test)]
    pub(crate) recomputed_rows: usize,
}

impl TranscriptLayoutCache {
    pub(crate) fn max_scroll(&self, viewport_height: u16) -> u16 {
        let total = self.total_height.min(usize::from(u16::MAX)) as u16;
        total.saturating_sub(viewport_height)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TranscriptLayoutBlock {
    pub(crate) key: TranscriptLayoutBlockKey,
    pub(crate) target: TranscriptHitTarget,
    pub(crate) start: usize,
    pub(crate) height: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranscriptLayoutBlockKey {
    pub(crate) target: TranscriptHitTarget,
    pub(crate) compact_trailing: bool,
    pub(crate) selected: bool,
    pub(crate) row_key: TranscriptLayoutRowKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranscriptLayoutRowKey {
    pub(crate) visible: bool,
    pub(crate) compact_trailing: bool,
    pub(crate) selected: bool,
    pub(crate) kind: TranscriptKind,
    pub(crate) failed: bool,
    pub(crate) interrupted: bool,
    pub(crate) user_shell: bool,
    pub(crate) agent_tool: bool,
    pub(crate) agent_open: bool,
    pub(crate) expanded: bool,
    pub(crate) details_collapsed: bool,
    pub(crate) expandable: bool,
    pub(crate) tool_elapsed_hash: u64,
    pub(crate) active_tool_marker_hash: u64,
    pub(crate) title_hash: u64,
    pub(crate) text_hash: u64,
}

impl TranscriptRow {
    pub(crate) fn simple(kind: TranscriptKind, text: impl Into<String>) -> Self {
        let title = default_title(kind).to_string();
        Self::with_title(kind, title, text)
    }

    pub(crate) fn with_title(
        kind: TranscriptKind,
        title: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
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
            transcript_turn_id: None,
            transcript_source: None,
            transcript_entry_id: None,
            transcript_block_id: None,
            transcript_message_seq: None,
        };
        row.apply_default_evidence_collapse();
        row
    }

    pub(crate) fn expandable_text(&self) -> &str {
        if self.expanded {
            self.full_text.as_deref().unwrap_or(&self.text)
        } else {
            &self.text
        }
    }

    pub(crate) fn is_expandable(&self) -> bool {
        self.full_text
            .as_ref()
            .is_some_and(|full| full != &self.text)
            || foldable_evidence_body(self)
            || foldable_tool_title(self)
    }

    pub(crate) fn apply_default_evidence_collapse(&mut self) {
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
        self.set_evidence_body_text(source);
    }

    pub(crate) fn set_evidence_body_text(&mut self, full: impl Into<String>) {
        let full = full.into();
        if full.is_empty() {
            self.text.clear();
            self.full_text = None;
            self.expanded = false;
            self.details_collapsed = false;
            return;
        }

        let keep_expanded = self.expanded && !self.details_collapsed;
        let keep_details_collapsed = self.details_collapsed;
        let collapsed = ledger_body_collapse_policy().collapse(&full);
        self.text = if collapsed.preview.is_empty() {
            full.clone()
        } else {
            collapsed.preview
        };
        self.full_text = collapsed.full_text;
        self.expanded = self.full_text.is_some() && keep_expanded;
        self.details_collapsed = keep_details_collapsed && self.is_expandable();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FocusMode {
    Composer,
    Transcript,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MouseWheelTarget {
    Transcript,
    BottomPanel,
}

pub(crate) struct FullscreenUi<'a> {
    pub(crate) textarea: TextArea<'a>,
    pub(crate) workdir: PathBuf,
    pub(crate) transcript: Vec<TranscriptRow>,
    pub(crate) assistant_row: Option<usize>,
    pub(crate) assistant_preamble_row: Option<usize>,
    pub(crate) reasoning_row: Option<usize>,
    pub(crate) meta_row: Option<usize>,
    pub(crate) gateway_item_rows: BTreeMap<String, usize>,
    pub(crate) tool_rows: BTreeMap<String, usize>,
    pub(crate) streaming_tool_message_seq: u64,
    pub(crate) streaming_tool_message_open: bool,
    pub(crate) deferred_stream_events: VecDeque<TuiLiveEvent>,
    pub(crate) history_tool_titles: BTreeMap<String, String>,
    pub(crate) history_tool_args: BTreeMap<String, Value>,
    pub(crate) live_tool_args: BTreeMap<String, Value>,
    pub(crate) clarify_tool_args: BTreeMap<String, Value>,
    pub(crate) exec_session_rows: BTreeMap<u64, usize>,
    pub(crate) exec_session_elapsed: BTreeMap<u64, Duration>,
    pub(crate) shell_mode: bool,
    pub(crate) turn_started: Option<Instant>,
    pub(crate) turn_provider: String,
    pub(crate) turn_model: String,
    pub(crate) turn_mode: String,
    pub(crate) turn_context_limit: Option<u64>,
    pub(crate) turn_usage: Option<Value>,
    pub(crate) turn_metadata: Option<Value>,
    pub(crate) turn_accounting: Option<Value>,
    pub(crate) turn_session_id: Option<String>,
    pub(crate) active_event_session_id: Option<String>,
    pub(crate) turn_failures: usize,
    pub(crate) turn_interrupted: bool,
    pub(crate) turn_outcome: Option<Outcome>,
    pub(crate) turn_terminal_message: Option<String>,
    pub(crate) turn_had_reasoning: bool,
    pub(crate) turn_terminal_visible_answer: bool,
    pub(crate) history_prompt_started_ms: Option<i64>,
    pub(crate) loaded_session_message_count: usize,
    pub(crate) thinking_visible: bool,
    pub(crate) raw_visible: bool,
    pub(crate) running: Option<RunningTurn>,
    pub(crate) auxiliary_agent_tasks: Vec<AuxiliaryAgentTask>,
    pub(crate) agent_child_event_backlog: BTreeMap<String, Vec<RunStreamEvent>>,
    pub(crate) session_live_event_backlog: BTreeMap<String, Vec<RunStreamEvent>>,
    pub(crate) auxiliary_shell_tasks: Vec<AuxiliaryShellTask>,
    pub(crate) pending_auxiliary_shell_commands: VecDeque<String>,
    pub(crate) approval_rx: Option<mpsc::UnboundedReceiver<TuiApprovalRequest>>,
    pub(crate) pending_permission_approvals: VecDeque<TuiApprovalRequest>,
    pub(crate) active_permission_approval: Option<oneshot::Sender<PermissionApprovalDecision>>,
    pub(crate) visible_turn_started: Option<Instant>,
    pub(crate) motion_started: Instant,
    #[cfg(test)]
    pub(crate) running_elapsed_override: Option<Duration>,
    pub(crate) interrupt_requested: bool,
    pub(crate) scroll: u16,
    pub(crate) last_transcript_height: u16,
    pub(crate) last_transcript_width: u16,
    pub(crate) transcript_layout: TranscriptLayoutCache,
    pub(crate) auto_follow_transcript: bool,
    pub(crate) pending_scroll_to_bottom: bool,
    pub(crate) focus: FocusMode,
    pub(crate) selected_row: Option<usize>,
    pub(crate) selected_target: Option<TranscriptHitTarget>,
    pub(crate) last_transcript_area: Option<Rect>,
    pub(crate) last_composer_area: Option<Rect>,
    pub(crate) last_composer_input_area: Option<Rect>,
    pub(crate) composer_cursor_top_row: u16,
    pub(crate) last_status_area: Option<Rect>,
    pub(crate) last_bottom_panel_area: Option<Rect>,
    pub(crate) last_entry_areas: Vec<(TranscriptHitTarget, Rect)>,
    pub(crate) mouse_down_target: Option<TranscriptHitTarget>,
    pub(crate) mouse_dragged: bool,
    pub(crate) composer_mouse_selecting: bool,
    pub(crate) sidebar_forced: bool,
    pub(crate) sidebar_hidden: bool,
    pub(crate) last_sidebar_visible: bool,
    pub(crate) sidebar: SidebarSnapshot,
    pub(crate) sidebar_tokens: Option<u64>,
    pub(crate) sidebar_context_limit: Option<u64>,
    pub(crate) last_context_snapshot: Option<ContextSnapshot>,
    pub(crate) sidebar_cost_nanodollars: Option<i64>,
    pub(crate) session_usage_summary: Option<SessionUsageSummary>,
    pub(crate) history: Vec<String>,
    pub(crate) history_kinds: Vec<ComposerHistoryKind>,
    pub(crate) history_index: Option<usize>,
    pub(crate) history_draft: Option<String>,
    pub(crate) queued_inputs: VecDeque<QueuedInput>,
    pub(crate) pending_steers: VecDeque<PendingSteerInput>,
    pub(crate) pending_input_edit: Option<PendingInputEdit<'a>>,
    pub(crate) pending_input_sequence: u64,
    pub(crate) pending_images: Vec<PendingImageAttachment>,
    pub(crate) history_search: bool,
    pub(crate) history_query: String,
    pub(crate) slash_menu_selected: usize,
    pub(crate) slash_menu_dismissed_input: Option<String>,
    pub(crate) pending_leader_started: Option<Instant>,
    pub(crate) last_slash_menu_areas: Vec<(usize, Rect)>,
    pub(crate) last_pending_input_action_areas: Vec<(PendingInputRef, PendingInputAction, Rect)>,
    pub(crate) last_pending_input_edit_area: Option<Rect>,
    pub(crate) file_search: FileSearchState,
    pub(crate) last_file_popup_areas: Vec<(usize, Rect)>,
    pub(crate) agent_search: AgentSearchState,
    pub(crate) last_agent_popup_areas: Vec<(usize, Rect)>,
    pub(crate) skill_search: SkillSearchState,
    pub(crate) last_skill_popup_areas: Vec<(usize, Rect)>,
    pub(crate) last_bottom_panel_areas: Vec<(usize, Rect)>,
    pub(crate) bottom_panel: Option<BottomPanel>,
    pub(crate) diff_overlay: Option<DiffOverlay>,
    pub(crate) last_diff_overlay_area: Option<Rect>,
    pub(crate) ephemeral_status: Option<UiEphemeralStatus>,
    pub(crate) screen_lines: Vec<ScreenLine>,
    pub(crate) selection: SelectionState,
    pub(crate) terminal_clear_requested: bool,
    pub(crate) quit_requested: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct DiffOverlay {
    pub(crate) title: String,
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) scroll: u16,
}

impl DiffOverlay {
    pub(crate) fn computing() -> Self {
        Self::from_lines(vec![Line::from("computing diff")])
    }

    pub(crate) fn error(message: impl Into<String>) -> Self {
        Self::from_lines(vec![Line::from(format!("error: {}", message.into()))])
    }

    pub(crate) fn from_lines(lines: Vec<Line<'static>>) -> Self {
        Self {
            title: "D I F F".to_string(),
            lines,
            scroll: 0,
        }
    }

    pub(crate) fn scroll_by(&mut self, amount: isize, viewport_height: u16) {
        let max_scroll = self.max_scroll(viewport_height);
        if amount.is_negative() {
            self.scroll = self.scroll.saturating_sub(amount.unsigned_abs() as u16);
        } else {
            self.scroll = self.scroll.saturating_add(amount as u16).min(max_scroll);
        }
    }

    pub(crate) fn scroll_to_top(&mut self) {
        self.scroll = 0;
    }

    pub(crate) fn scroll_to_bottom(&mut self, viewport_height: u16) {
        self.scroll = self.max_scroll(viewport_height);
    }

    pub(crate) fn max_scroll(&self, viewport_height: u16) -> u16 {
        let visible = viewport_height.saturating_sub(2);
        (self.lines.len() as u16).saturating_sub(visible)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UiEphemeralStatus {
    pub(crate) text: String,
    pub(crate) failed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComposerHistoryKind {
    SessionPrompt,
    ProcessCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScreenLine {
    pub(crate) region: SelectableRegion,
    pub(crate) y: u16,
    pub(crate) cells: Vec<ScreenCell>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectableRegion {
    Transcript,
    Sidebar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScreenCell {
    pub(crate) x: u16,
    pub(crate) width: u16,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SelectionState {
    pub(crate) anchor: Option<(u16, u16)>,
    pub(crate) focus: Option<(u16, u16)>,
    pub(crate) region: Option<SelectableRegion>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SidebarSnapshot {
    pub(crate) title: String,
    pub(crate) session: String,
    pub(crate) branch: String,
    pub(crate) changed_files: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct BottomSelectionPanel {
    pub(crate) title: String,
    pub(crate) empty_label: String,
    pub(crate) footer: String,
    pub(crate) notice: Option<String>,
    pub(crate) session_view: Option<SessionListView>,
    pub(crate) action_armed: bool,
    pub(crate) delete_confirm: Option<String>,
    pub(crate) running_session_ids: BTreeSet<String>,
    pub(crate) rows: Vec<BottomSelectionRow>,
    pub(crate) query: String,
    pub(crate) selected: usize,
    pub(crate) scroll: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionListView {
    Active,
    Archived,
}

#[derive(Debug, Clone)]
pub(crate) struct AgentPanel {
    pub(crate) running: BottomSelectionPanel,
    pub(crate) available: BottomSelectionPanel,
    pub(crate) tab: AgentTab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentTab {
    Running,
    Available,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentAction {
    UseAsMain,
    Run,
    View,
    Update,
    Delete,
}

#[derive(Debug, Clone)]
pub(crate) struct AgentRunPromptPanel {
    pub(crate) agent_name: String,
    pub(crate) prompt: String,
    pub(crate) notice: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct AgentEditorPanel {
    pub(crate) mode: AgentEditorMode,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) instructions: String,
    pub(crate) model: String,
    pub(crate) tools: String,
    pub(crate) permission_mode: String,
    pub(crate) background: bool,
    pub(crate) max_spawn_depth: String,
    pub(crate) active_field: AgentEditorField,
    pub(crate) notice: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum AgentEditorMode {
    Create,
    Update { path: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentEditorField {
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
    Session(String),
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
    pub(crate) label: String,
    pub(crate) provider_id: String,
    pub(crate) base_url: String,
    pub(crate) api_key: String,
    pub(crate) provider_id_touched: bool,
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
