#[allow(unused_imports)]
pub(crate) use super::*;

impl<'a> FullscreenUi<'a> {
    pub(crate) fn complete_slash_command(&mut self, items: &[SlashMenuItem]) {
        let input = textarea_text(&self.textarea);
        if let Some(completed) = slash_completion_with_items(&input, items) {
            self.textarea = textarea_with_text(&completed);
            self.slash_menu_selected = 0;
            self.clear_slash_menu_dismissal();
        }
    }

    pub(crate) fn current_file_token(&self) -> Option<FileToken> {
        current_file_token(&self.textarea)
    }

    pub(crate) fn sync_file_popup(&mut self, root: &Path) {
        let token = self.current_file_token();
        self.file_search.sync(root, token.as_ref());
    }

    pub(crate) fn drain_file_search_results(&mut self) -> bool {
        self.file_search.drain_results()
    }

    pub(crate) fn file_popup_visible(&self) -> bool {
        self.file_search.popup.is_some()
    }

    pub(crate) fn close_file_popup(&mut self) {
        self.file_search.close();
        self.last_file_popup_areas.clear();
        self.last_completion_popup_areas.clear();
    }

    pub(crate) fn dismiss_file_popup(&mut self) {
        let query = self.current_file_token().map(|token| token.query);
        self.file_search.dismiss(query);
        self.last_file_popup_areas.clear();
        self.last_completion_popup_areas.clear();
    }

    pub(crate) fn selected_file_path(&self) -> Option<String> {
        self.file_search.selected_path()
    }

    pub(crate) fn set_file_popup_selection(&mut self, index: usize) {
        self.file_search.set_selection(index);
    }

    pub(crate) fn insert_selected_file_path(&mut self) {
        let Some(path) = self.selected_file_path() else {
            return;
        };
        if replace_current_file_token(&mut self.textarea, &path) {
            self.file_search.close();
            self.file_search.dismissed_query = None;
            self.last_file_popup_areas.clear();
        }
    }

    pub(crate) fn file_popup_hit(&self, column: u16, row: u16) -> Option<usize> {
        self.last_file_popup_areas
            .iter()
            .find(|(_, area)| rect_contains(*area, column, row))
            .map(|(index, _)| *index)
    }

    pub(crate) fn current_skill_token(&self) -> Option<SkillToken> {
        current_skill_token(&self.textarea)
    }

    pub(crate) fn current_agent_token(&self) -> Option<AgentToken> {
        current_agent_token(&self.textarea)
    }

    pub(crate) fn sync_agent_popup(&mut self, matches: Vec<AgentSearchMatch>) {
        let token = self.current_agent_token();
        self.agent_search.sync(token.as_ref(), matches);
    }

    pub(crate) fn agent_popup_visible(&self) -> bool {
        self.agent_search.popup.is_some()
    }

    pub(crate) fn close_agent_popup(&mut self) {
        self.agent_search.close();
        self.last_agent_popup_areas.clear();
        self.last_completion_popup_areas.clear();
    }

    pub(crate) fn dismiss_agent_popup(&mut self) {
        let query = self.current_agent_token().map(|token| token.query);
        self.agent_search.dismiss(query);
        self.last_agent_popup_areas.clear();
        self.last_completion_popup_areas.clear();
    }

    pub(crate) fn selected_agent_name(&self) -> Option<String> {
        self.agent_search.selected_name()
    }

    pub(crate) fn set_agent_popup_selection(&mut self, index: usize) {
        self.agent_search.set_selection(index);
    }

    pub(crate) fn insert_selected_agent_marker(&mut self) {
        let Some(name) = self.selected_agent_name() else {
            return;
        };
        if replace_current_agent_token(&mut self.textarea, &name) {
            self.agent_search.close();
            self.agent_search.dismissed_query = None;
            self.last_agent_popup_areas.clear();
            self.clear_slash_menu_dismissal();
        }
    }

    pub(crate) fn agent_popup_hit(&self, column: u16, row: u16) -> Option<usize> {
        self.last_agent_popup_areas
            .iter()
            .find(|(_, area)| rect_contains(*area, column, row))
            .map(|(index, _)| *index)
    }

    pub(crate) fn sync_skill_popup(&mut self, matches: Vec<SkillSearchMatch>) {
        let token = self.current_skill_token();
        self.skill_search.sync(token.as_ref(), matches);
    }

    pub(crate) fn skill_popup_visible(&self) -> bool {
        self.skill_search.popup.is_some()
    }

    pub(crate) fn close_skill_popup(&mut self) {
        self.skill_search.close();
        self.last_skill_popup_areas.clear();
        self.last_completion_popup_areas.clear();
    }

    pub(crate) fn dismiss_skill_popup(&mut self) {
        let query = self.current_skill_token().map(|token| token.query);
        self.skill_search.dismiss(query);
        self.last_skill_popup_areas.clear();
        self.last_completion_popup_areas.clear();
    }

    pub(crate) fn selected_skill_name(&self) -> Option<String> {
        self.skill_search.selected_name()
    }

    pub(crate) fn set_skill_popup_selection(&mut self, index: usize) {
        self.skill_search.set_selection(index);
    }

    pub(crate) fn insert_selected_skill_marker(&mut self) {
        let Some(name) = self.selected_skill_name() else {
            return;
        };
        self.insert_skill_marker(&name);
    }

    pub(crate) fn insert_skill_marker(&mut self, name: &str) {
        if replace_current_skill_token(&mut self.textarea, name) {
            self.skill_search.close();
            self.skill_search.dismissed_query = None;
            self.last_skill_popup_areas.clear();
            self.clear_slash_menu_dismissal();
        } else {
            self.set_composer_text(&format!("${name} "));
            self.close_skill_popup();
            self.slash_menu_selected = 0;
            self.clear_slash_menu_dismissal();
        }
    }

    pub(crate) fn skill_popup_hit(&self, column: u16, row: u16) -> Option<usize> {
        self.last_skill_popup_areas
            .iter()
            .find(|(_, area)| rect_contains(*area, column, row))
            .map(|(index, _)| *index)
    }

    pub(crate) fn completion_popup_visible(&self) -> bool {
        self.agent_popup_visible() || self.file_popup_visible() || self.skill_popup_visible()
    }

    pub(crate) fn completion_popup_height(&self) -> u16 {
        let mut rows = 0usize;
        if let Some(popup) = &self.agent_search.popup
            && !popup.matches.is_empty()
        {
            rows = rows.saturating_add(1 + popup.matches.len().min(FILE_POPUP_MAX_ROWS));
        }
        if let Some(popup) = &self.file_search.popup {
            if popup.matches.is_empty() {
                rows = rows.saturating_add(2);
            } else {
                let directories = popup
                    .matches
                    .iter()
                    .filter(|item| item.kind == FileSearchMatchKind::Directory)
                    .count()
                    .min(FILE_POPUP_MAX_ROWS);
                let files = popup
                    .matches
                    .iter()
                    .filter(|item| item.kind == FileSearchMatchKind::File)
                    .count()
                    .min(FILE_POPUP_MAX_ROWS);
                if directories > 0 {
                    rows = rows.saturating_add(1 + directories);
                }
                if files > 0 {
                    rows = rows.saturating_add(1 + files);
                }
            }
        }
        if let Some(popup) = &self.skill_search.popup {
            rows = rows.saturating_add(1 + popup.matches.len().clamp(1, FILE_POPUP_MAX_ROWS));
        }
        rows.min(COMPLETION_POPUP_MAX_ROWS)
            .min(usize::from(u16::MAX)) as u16
    }

    pub(crate) fn completion_popup_selectable_count(&self) -> usize {
        self.completion_popup_targets().len()
    }

    pub(crate) fn clamp_completion_popup_selection(&mut self) {
        let count = self.completion_popup_selectable_count();
        if count == 0 {
            self.completion_popup_selected = 0;
            return;
        }
        self.completion_popup_selected =
            self.completion_popup_selected.min(count.saturating_sub(1));
        self.apply_completion_popup_selection();
    }

    pub(crate) fn move_completion_popup_selection(&mut self, direction: isize) {
        let count = self.completion_popup_selectable_count();
        if count == 0 {
            self.completion_popup_selected = 0;
            return;
        }
        let current = self.completion_popup_selected.min(count.saturating_sub(1)) as isize;
        self.completion_popup_selected = (current + direction).rem_euclid(count as isize) as usize;
        self.apply_completion_popup_selection();
    }

    pub(crate) fn set_completion_popup_selection(&mut self, index: usize) {
        let count = self.completion_popup_selectable_count();
        self.completion_popup_selected = if count == 0 {
            0
        } else {
            index.min(count.saturating_sub(1))
        };
        self.apply_completion_popup_selection();
    }

    pub(crate) fn selected_completion_popup_target(&self) -> Option<CompletionPopupTarget> {
        self.completion_popup_targets()
            .get(self.completion_popup_selected)
            .copied()
    }

    pub(crate) fn insert_selected_completion_popup_item(&mut self) -> bool {
        let Some(target) = self.selected_completion_popup_target() else {
            return false;
        };
        match target {
            CompletionPopupTarget::Agent(index) => {
                self.set_agent_popup_selection(index);
                self.insert_selected_agent_marker();
                self.close_file_popup();
                self.close_skill_popup();
            }
            CompletionPopupTarget::File(index) => {
                self.set_file_popup_selection(index);
                self.insert_selected_file_path();
                self.close_agent_popup();
                self.close_skill_popup();
            }
            CompletionPopupTarget::Skill(index) => {
                self.set_skill_popup_selection(index);
                self.insert_selected_skill_marker();
                self.close_agent_popup();
                self.close_file_popup();
            }
        }
        self.completion_popup_selected = 0;
        true
    }

    pub(crate) fn dismiss_completion_popup(&mut self) {
        if self.agent_popup_visible() {
            self.dismiss_agent_popup();
        }
        if self.file_popup_visible() {
            self.dismiss_file_popup();
        }
        if self.skill_popup_visible() {
            self.dismiss_skill_popup();
        }
        self.completion_popup_selected = 0;
        self.last_completion_popup_areas.clear();
    }

    pub(crate) fn completion_popup_hit(
        &self,
        column: u16,
        row: u16,
    ) -> Option<CompletionPopupTarget> {
        self.last_completion_popup_areas
            .iter()
            .find(|(_, area)| rect_contains(*area, column, row))
            .map(|(target, _)| *target)
    }

    pub(crate) fn set_completion_popup_target_selection(&mut self, target: CompletionPopupTarget) {
        if let Some(index) = self
            .completion_popup_targets()
            .iter()
            .position(|candidate| *candidate == target)
        {
            self.completion_popup_selected = index;
            self.apply_completion_popup_selection();
        }
    }

    fn apply_completion_popup_selection(&mut self) {
        match self.selected_completion_popup_target() {
            Some(CompletionPopupTarget::Agent(index)) => self.set_agent_popup_selection(index),
            Some(CompletionPopupTarget::File(index)) => self.set_file_popup_selection(index),
            Some(CompletionPopupTarget::Skill(index)) => self.set_skill_popup_selection(index),
            None => {}
        }
    }

    fn completion_popup_targets(&self) -> Vec<CompletionPopupTarget> {
        let mut targets = Vec::new();
        if let Some(popup) = &self.agent_search.popup {
            targets.extend(
                popup
                    .matches
                    .iter()
                    .take(FILE_POPUP_MAX_ROWS)
                    .enumerate()
                    .map(|(index, _)| CompletionPopupTarget::Agent(index)),
            );
        }
        if let Some(popup) = &self.file_search.popup {
            for kind in [FileSearchMatchKind::Directory, FileSearchMatchKind::File] {
                targets.extend(
                    popup
                        .matches
                        .iter()
                        .enumerate()
                        .filter_map(|(index, item)| {
                            (item.kind == kind).then_some(CompletionPopupTarget::File(index))
                        })
                        .take(FILE_POPUP_MAX_ROWS),
                );
            }
        }
        if let Some(popup) = &self.skill_search.popup {
            targets.extend(
                popup
                    .matches
                    .iter()
                    .take(FILE_POPUP_MAX_ROWS)
                    .enumerate()
                    .map(|(index, _)| CompletionPopupTarget::Skill(index)),
            );
        }
        targets
    }

    pub(crate) fn clamp_slash_menu_selection(&mut self, len: usize) {
        if len == 0 {
            self.slash_menu_selected = 0;
            self.last_slash_menu_areas.clear();
            return;
        }
        self.slash_menu_selected = self.slash_menu_selected.min(len.saturating_sub(1));
    }

    pub(crate) fn move_slash_menu_selection(&mut self, direction: isize, len: usize) {
        if len == 0 {
            self.slash_menu_selected = 0;
            return;
        }
        let current = self.slash_menu_selected.min(len.saturating_sub(1)) as isize;
        let next = (current + direction).rem_euclid(len as isize) as usize;
        self.slash_menu_selected = next;
    }

    pub(crate) fn set_slash_menu_selection(&mut self, index: usize, len: usize) {
        self.slash_menu_selected = if len == 0 {
            0
        } else {
            index.min(len.saturating_sub(1))
        };
    }

    pub(crate) fn slash_menu_hit(&self, column: u16, row: u16) -> Option<usize> {
        self.last_slash_menu_areas
            .iter()
            .find(|(_, area)| rect_contains(*area, column, row))
            .map(|(index, _)| *index)
    }

    pub(crate) fn slash_menu_dismissed(&self, input: &str) -> bool {
        self.slash_menu_dismissed_input.as_deref() == Some(input)
    }

    pub(crate) fn dismiss_slash_menu(&mut self) {
        self.slash_menu_dismissed_input = Some(textarea_text(&self.textarea));
        self.slash_menu_selected = 0;
        self.last_slash_menu_areas.clear();
    }

    pub(crate) fn clear_slash_menu_dismissal(&mut self) {
        self.slash_menu_dismissed_input = None;
    }
}
