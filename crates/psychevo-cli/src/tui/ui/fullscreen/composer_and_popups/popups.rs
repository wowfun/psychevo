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

    pub(crate) fn file_popup_height(&self) -> u16 {
        self.file_search.height()
    }

    pub(crate) fn close_file_popup(&mut self) {
        self.file_search.close();
        self.last_file_popup_areas.clear();
    }

    pub(crate) fn dismiss_file_popup(&mut self) {
        let query = self.current_file_token().map(|token| token.query);
        self.file_search.dismiss(query);
        self.last_file_popup_areas.clear();
    }

    pub(crate) fn selected_file_path(&self) -> Option<String> {
        self.file_search.selected_path()
    }

    pub(crate) fn move_file_popup_selection(&mut self, direction: isize) {
        self.file_search.move_selection(direction);
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

    pub(crate) fn agent_popup_height(&self) -> u16 {
        self.agent_search.height()
    }

    pub(crate) fn close_agent_popup(&mut self) {
        self.agent_search.close();
        self.last_agent_popup_areas.clear();
    }

    pub(crate) fn dismiss_agent_popup(&mut self) {
        let query = self.current_agent_token().map(|token| token.query);
        self.agent_search.dismiss(query);
        self.last_agent_popup_areas.clear();
    }

    pub(crate) fn selected_agent_name(&self) -> Option<String> {
        self.agent_search.selected_name()
    }

    pub(crate) fn move_agent_popup_selection(&mut self, direction: isize) {
        self.agent_search.move_selection(direction);
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

    pub(crate) fn skill_popup_height(&self) -> u16 {
        self.skill_search.height()
    }

    pub(crate) fn close_skill_popup(&mut self) {
        self.skill_search.close();
        self.last_skill_popup_areas.clear();
    }

    pub(crate) fn dismiss_skill_popup(&mut self) {
        let query = self.current_skill_token().map(|token| token.query);
        self.skill_search.dismiss(query);
        self.last_skill_popup_areas.clear();
    }

    pub(crate) fn selected_skill_name(&self) -> Option<String> {
        self.skill_search.selected_name()
    }

    pub(crate) fn move_skill_popup_selection(&mut self, direction: isize) {
        self.skill_search.move_selection(direction);
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
