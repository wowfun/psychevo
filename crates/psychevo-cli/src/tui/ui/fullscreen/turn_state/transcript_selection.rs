#[allow(unused_imports)]
pub(crate) use super::*;

impl<'a> FullscreenUi<'a> {
    pub(crate) fn ensure_selection(&mut self) {
        if self
            .selected_target
            .is_some_and(|target| self.target_visible(target))
        {
            return;
        }
        if let Some(index) = self.selected_row
            && let Some(row) = self.transcript.get(index)
            && self.target_visible(TranscriptHitTarget::Row(row.id))
        {
            self.selected_target = Some(TranscriptHitTarget::Row(row.id));
            return;
        }
        let targets = self.visible_transcript_targets();
        self.selected_target = targets
            .iter()
            .copied()
            .find(|target| self.target_toggleable(*target))
            .or_else(|| targets.last().copied());
        self.selected_row = self
            .selected_target
            .and_then(|target| self.target_row_index(target));
    }

    pub(crate) fn move_selection(&mut self, direction: isize) {
        self.auto_follow_transcript = false;
        self.ensure_selection();
        let visible = self.visible_transcript_targets();
        if visible.is_empty() {
            self.selected_row = None;
            self.selected_target = None;
            return;
        }
        let current_position = self
            .selected_target
            .and_then(|current| visible.iter().position(|target| *target == current))
            .unwrap_or(0);
        let next_position = if direction < 0 {
            current_position.saturating_sub(direction.unsigned_abs())
        } else {
            current_position
                .saturating_add(direction as usize)
                .min(visible.len().saturating_sub(1))
        };
        self.set_selected_target(visible.get(next_position).copied());
        self.scroll_selected_target_into_view();
    }

    pub(crate) fn scroll_selected_target_into_view(&mut self) {
        let Some(selected) = self.selected_target else {
            return;
        };
        if !self.transcript_layout_matches_viewport() && self.last_transcript_width > 0 {
            refresh_transcript_layout(self, self.last_transcript_width);
        }
        let Some(block) = self
            .transcript_layout
            .blocks
            .iter()
            .find(|block| block.target == selected)
        else {
            return;
        };
        if block.height == 0 || self.last_transcript_height == 0 {
            return;
        }
        let viewport_start = usize::from(self.scroll);
        let viewport_end = viewport_start.saturating_add(usize::from(self.last_transcript_height));
        let row_start = block.start;
        let row_end = block.start.saturating_add(block.height);
        if row_start < viewport_start {
            self.scroll = row_start.min(usize::from(u16::MAX)) as u16;
        } else if row_end > viewport_end {
            let next = row_end.saturating_sub(usize::from(self.last_transcript_height));
            self.scroll = next.min(usize::from(u16::MAX)) as u16;
        }
        self.clamp_transcript_scroll();
    }

    pub(crate) fn toggle_selected(&mut self) {
        self.auto_follow_transcript = false;
        if self.selected_target.is_none() {
            self.ensure_selection();
        }
        if let Some(target) = self.selected_target {
            self.toggle_target(target);
        }
    }

    pub(crate) fn visible_transcript_targets(&self) -> Vec<TranscriptHitTarget> {
        transcript_render_blocks(self)
            .iter()
            .map(|block| block.target)
            .collect()
    }

    pub(crate) fn target_visible(&self, target: TranscriptHitTarget) -> bool {
        self.visible_transcript_targets()
            .into_iter()
            .any(|visible| visible == target)
    }

    pub(crate) fn target_row_index(&self, target: TranscriptHitTarget) -> Option<usize> {
        match target {
            TranscriptHitTarget::Row(row_id) => {
                self.transcript.iter().position(|row| row.id == row_id)
            }
            TranscriptHitTarget::AgentOpen(row_id) => {
                self.transcript.iter().position(|row| row.id == row_id)
            }
        }
    }

    pub(crate) fn agent_target_for_target(&self, target: TranscriptHitTarget) -> Option<String> {
        match target {
            TranscriptHitTarget::AgentOpen(row_id) => self
                .transcript
                .iter()
                .find(|row| row.id == row_id)
                .and_then(|row| row.agent_target.clone()),
            TranscriptHitTarget::Row(_) => None,
        }
    }

    pub(crate) fn selected_agent_target(&self) -> Option<String> {
        let target = self.selected_target?;
        let index = self.target_row_index(target)?;
        self.transcript
            .get(index)
            .filter(|row| row.agent_target.is_some())
            .and_then(|row| row.agent_target.clone())
    }

    pub(crate) fn visible_agent_target(&self) -> Option<String> {
        self.visible_transcript_targets()
            .into_iter()
            .rev()
            .find_map(|target| {
                self.target_row_index(target)
                    .and_then(|index| self.transcript.get(index))
                    .and_then(|row| row.agent_target.clone())
            })
            .or_else(|| {
                self.transcript
                    .iter()
                    .rev()
                    .find_map(|row| row.agent_target.clone())
            })
    }

    pub(crate) fn ensure_agent_open_selection(&mut self) {
        if self.selected_agent_target().is_some() {
            return;
        }
        let target = self
            .visible_transcript_targets()
            .into_iter()
            .rev()
            .find(|target| {
                self.target_row_index(*target)
                    .and_then(|index| self.transcript.get(index))
                    .is_some_and(|row| row.agent_target.is_some())
            })
            .or_else(|| {
                self.transcript
                    .iter()
                    .rev()
                    .find(|row| row.agent_target.is_some())
                    .map(|row| TranscriptHitTarget::Row(row.id))
            });
        if target.is_some() {
            self.set_selected_target(target);
        } else {
            self.ensure_selection();
        }
    }

    pub(crate) fn set_selected_target(&mut self, target: Option<TranscriptHitTarget>) {
        self.selected_target = target;
        self.selected_row = target.and_then(|target| self.target_row_index(target));
    }

    pub(crate) fn target_toggleable(&self, target: TranscriptHitTarget) -> bool {
        match target {
            TranscriptHitTarget::Row(row_id) => self
                .transcript
                .iter()
                .find(|row| row.id == row_id)
                .is_some_and(TranscriptRow::is_expandable),
            TranscriptHitTarget::AgentOpen(_) => false,
        }
    }

    pub(crate) fn toggle_target(&mut self, target: TranscriptHitTarget) {
        match target {
            TranscriptHitTarget::Row(row_id) | TranscriptHitTarget::AgentOpen(row_id) => {
                if let Some(row) = self.transcript.iter_mut().find(|row| row.id == row_id)
                    && row_visible(row, self.thinking_visible)
                    && row.is_expandable()
                {
                    toggle_transcript_row_details(row);
                }
            }
        }
        self.set_selected_target(Some(target));
        self.clamp_transcript_scroll();
    }

    pub(crate) fn transcript_hit(&self, column: u16, row: u16) -> Option<TranscriptHitTarget> {
        let mut first_hit = None;
        for (target, area) in &self.last_entry_areas {
            if !rect_contains(*area, column, row) {
                continue;
            }
            if matches!(target, TranscriptHitTarget::AgentOpen(_)) {
                return Some(*target);
            }
            first_hit.get_or_insert(*target);
        }
        first_hit
    }
}
