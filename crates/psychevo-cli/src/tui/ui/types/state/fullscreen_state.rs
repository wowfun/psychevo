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
