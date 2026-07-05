#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) const TUI_ROLE_ACCENT: Color = Color::Cyan;
pub(crate) const TUI_ROLE_IDENTITY: Color = Color::Magenta;
pub(crate) const TUI_ROLE_DANGER: Color = Color::Red;
pub(crate) const TUI_ROLE_DIM: Color = Color::DarkGray;
pub(crate) const TUI_ROLE_THINKING: Color = Color::Rgb(216, 205, 184);
pub(crate) const TUI_ROLE_SURFACE_BG: Color = Color::Rgb(38, 38, 38);
pub(crate) const TUI_ROLE_SELECTION_BG: Color = Color::Rgb(62, 88, 105);

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
