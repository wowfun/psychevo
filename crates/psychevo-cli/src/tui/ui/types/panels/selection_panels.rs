#[allow(unused_imports)]
pub(crate) use super::*;

impl ClarifyPanel {
    pub(crate) fn new(request: ClarifyRequestEvent, previous_panel: Option<BottomPanel>) -> Self {
        Self {
            states: vec![ClarifyQuestionState::default(); request.questions.len()],
            answers: vec![None; request.questions.len()],
            request,
            question_index: 0,
            previous_panel: previous_panel.map(Box::new),
            notice: None,
        }
    }

    pub(crate) fn current_question(&self) -> Option<&ClarifyQuestion> {
        self.request.questions.get(self.question_index)
    }

    pub(crate) fn option_count(&self) -> usize {
        self.current_question()
            .map(|question| question.options.len())
            .unwrap_or_default()
    }

    pub(crate) fn current_state(&self) -> Option<&ClarifyQuestionState> {
        self.states.get(self.question_index)
    }

    pub(crate) fn current_state_mut(&mut self) -> Option<&mut ClarifyQuestionState> {
        self.states.get_mut(self.question_index)
    }

    pub(crate) fn selected(&self) -> usize {
        self.current_state()
            .map(|state| state.selected)
            .unwrap_or_default()
    }

    pub(crate) fn mode(&self) -> ClarifyInputMode {
        self.current_state()
            .map(|state| state.mode)
            .unwrap_or_default()
    }

    pub(crate) fn total_choices(&self) -> usize {
        self.option_count().saturating_add(1)
    }

    pub(crate) fn selected_is_other(&self) -> bool {
        self.selected() >= self.option_count()
    }

    pub(crate) fn other_draft(&self) -> &str {
        self.current_state()
            .map(|state| state.other_draft.as_str())
            .unwrap_or_default()
    }

    pub(crate) fn other_cursor(&self) -> usize {
        self.current_state()
            .map(|state| state.other_cursor.min(char_count(&state.other_draft)))
            .unwrap_or_default()
    }

    pub(crate) fn note_draft(&self, option_index: usize) -> &str {
        self.current_state()
            .and_then(|state| state.note_drafts.get(&option_index))
            .map(String::as_str)
            .unwrap_or_default()
    }

    pub(crate) fn note_cursor(&self, option_index: usize) -> usize {
        self.current_state()
            .map(|state| {
                let draft = state
                    .note_drafts
                    .get(&option_index)
                    .map(String::as_str)
                    .unwrap_or_default();
                state
                    .note_cursors
                    .get(&option_index)
                    .copied()
                    .unwrap_or_else(|| char_count(draft))
                    .min(char_count(draft))
            })
            .unwrap_or_default()
    }

    pub(crate) fn desired_height(&self) -> u16 {
        let option_rows = self.option_count().saturating_add(1) as u16;
        let notice_rows = u16::from(self.notice.is_some());
        let inner_rows = 1 // progress
            + 1 // question
            + 1 // spacer before options
            + option_rows
            + 1 // spacer before footer
            + notice_rows
            + 1; // footer
        inner_rows.saturating_add(2).max(8)
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let total = self.total_choices();
        if total == 0 {
            if let Some(state) = self.current_state_mut() {
                state.selected = 0;
            }
            return;
        }
        let current = self.selected().min(total.saturating_sub(1)) as isize;
        let selected = (current + delta).clamp(0, total.saturating_sub(1) as isize) as usize;
        if let Some(state) = self.current_state_mut() {
            state.selected = selected;
            state.mode = ClarifyInputMode::Options;
        }
    }

    pub(crate) fn select_index(&mut self, index: usize) {
        let selected = index.min(self.total_choices().saturating_sub(1));
        if let Some(state) = self.current_state_mut() {
            state.selected = selected;
            state.mode = ClarifyInputMode::Options;
        }
    }

    pub(crate) fn move_question(&mut self, delta: isize) {
        let total = self.request.questions.len();
        if total == 0 {
            self.question_index = 0;
            return;
        }
        let current = self.question_index.min(total.saturating_sub(1)) as isize;
        self.question_index = (current + delta).clamp(0, total.saturating_sub(1) as isize) as usize;
        self.notice = None;
    }

    pub(crate) fn set_mode(&mut self, mode: ClarifyInputMode) {
        let selected = self.selected();
        if let Some(state) = self.current_state_mut() {
            state.mode = mode;
            match mode {
                ClarifyInputMode::Other => {
                    state.other_cursor = state.other_cursor.min(char_count(&state.other_draft));
                }
                ClarifyInputMode::Note => {
                    let len = state
                        .note_drafts
                        .get(&selected)
                        .map(|draft| char_count(draft))
                        .unwrap_or_default();
                    let cursor = state.note_cursors.entry(selected).or_insert(len);
                    *cursor = (*cursor).min(len);
                }
                ClarifyInputMode::Options => {}
            }
        }
    }

    pub(crate) fn pop_input_char(&mut self) {
        let selected = self.selected();
        let Some(state) = self.current_state_mut() else {
            return;
        };
        match state.mode {
            ClarifyInputMode::Other => {
                remove_previous_char(&mut state.other_draft, &mut state.other_cursor);
            }
            ClarifyInputMode::Note => {
                let len = state
                    .note_drafts
                    .get(&selected)
                    .map(|draft| char_count(draft))
                    .unwrap_or_default();
                let cursor = state.note_cursors.entry(selected).or_insert(len);
                let note = state.note_drafts.entry(selected).or_default();
                remove_previous_char(note, cursor);
            }
            ClarifyInputMode::Options => {}
        }
    }

    pub(crate) fn delete_input_char(&mut self) {
        let selected = self.selected();
        let Some(state) = self.current_state_mut() else {
            return;
        };
        match state.mode {
            ClarifyInputMode::Other => {
                remove_next_char(&mut state.other_draft, &mut state.other_cursor);
            }
            ClarifyInputMode::Note => {
                let len = state
                    .note_drafts
                    .get(&selected)
                    .map(|draft| char_count(draft))
                    .unwrap_or_default();
                let cursor = state.note_cursors.entry(selected).or_insert(len);
                let note = state.note_drafts.entry(selected).or_default();
                remove_next_char(note, cursor);
            }
            ClarifyInputMode::Options => {}
        }
    }

    pub(crate) fn push_input_char(&mut self, c: char) {
        let selected = self.selected();
        let Some(state) = self.current_state_mut() else {
            return;
        };
        match state.mode {
            ClarifyInputMode::Other => {
                insert_char(&mut state.other_draft, &mut state.other_cursor, c);
            }
            ClarifyInputMode::Note => {
                let len = state
                    .note_drafts
                    .get(&selected)
                    .map(|draft| char_count(draft))
                    .unwrap_or_default();
                let cursor = state.note_cursors.entry(selected).or_insert(len);
                let note = state.note_drafts.entry(selected).or_default();
                insert_char(note, cursor, c);
            }
            ClarifyInputMode::Options => {}
        }
    }

    pub(crate) fn move_input_cursor(&mut self, delta: isize) {
        let selected = self.selected();
        let Some(state) = self.current_state_mut() else {
            return;
        };
        match state.mode {
            ClarifyInputMode::Other => {
                move_cursor(&state.other_draft, &mut state.other_cursor, delta);
            }
            ClarifyInputMode::Note => {
                let len = state
                    .note_drafts
                    .get(&selected)
                    .map(|draft| char_count(draft))
                    .unwrap_or_default();
                let cursor = state.note_cursors.entry(selected).or_insert(len);
                let note = state
                    .note_drafts
                    .get(&selected)
                    .map(String::as_str)
                    .unwrap_or_default();
                move_cursor(note, cursor, delta);
            }
            ClarifyInputMode::Options => {}
        }
    }

    pub(crate) fn move_input_cursor_to_start(&mut self) {
        self.set_input_cursor(0);
    }

    pub(crate) fn move_input_cursor_to_end(&mut self) {
        let len = match self.mode() {
            ClarifyInputMode::Other => char_count(self.other_draft()),
            ClarifyInputMode::Note => char_count(self.note_draft(self.selected())),
            ClarifyInputMode::Options => 0,
        };
        self.set_input_cursor(len);
    }

    pub(crate) fn set_input_cursor(&mut self, cursor: usize) {
        let selected = self.selected();
        let Some(state) = self.current_state_mut() else {
            return;
        };
        match state.mode {
            ClarifyInputMode::Other => {
                state.other_cursor = cursor.min(char_count(&state.other_draft));
            }
            ClarifyInputMode::Note => {
                let len = state
                    .note_drafts
                    .get(&selected)
                    .map(|draft| char_count(draft))
                    .unwrap_or_default();
                state.note_cursors.insert(selected, cursor.min(len));
            }
            ClarifyInputMode::Options => {}
        }
    }

    pub(crate) fn move_to_next_unanswered(&mut self) {
        let total = self.request.questions.len();
        if total == 0 {
            return;
        }
        let current = self.question_index.min(total.saturating_sub(1));
        for offset in 1..=total {
            let index = (current + offset) % total;
            if self.answers.get(index).is_some_and(Option::is_none) {
                self.question_index = index;
                return;
            }
        }
    }

    pub(crate) fn question_progress(&self) -> String {
        let total = self.request.questions.len();
        let current = self.question_index.saturating_add(1);
        let answered = self
            .answers
            .iter()
            .filter(|answer| answer.is_some())
            .count();
        let unanswered = total.saturating_sub(answered);
        format!("Question {current}/{total} ({unanswered} unanswered)")
    }

    pub(crate) fn restore_panel(&mut self) -> Option<BottomPanel> {
        self.previous_panel.take().map(|panel| *panel)
    }
}

impl PermissionApprovalPanel {
    pub(crate) fn new(
        session_id: Option<String>,
        request: PermissionApprovalRequest,
        previous_panel: Option<BottomPanel>,
    ) -> Self {
        Self {
            session_id,
            request,
            selected: 0,
            scope_expanded: false,
            scroll: 0,
            ensure_selected_visible: false,
            previous_panel: previous_panel.map(Box::new),
            notice: None,
        }
    }

    pub(crate) fn options(&self) -> Vec<(PermissionApprovalChoice, String, String)> {
        let mut options = vec![(
            PermissionApprovalChoice::Decision(PermissionApprovalDecision::allow_once()),
            "Allow once".to_string(),
            "Run this exact operation one time".to_string(),
        )];
        if let Some(filesystem) = &self.request.filesystem {
            if self.scope_expanded {
                for directory in &filesystem.scope_candidates {
                    options.push((
                        PermissionApprovalChoice::Decision(
                            PermissionApprovalDecision::allow_filesystem_turn(directory.clone()),
                        ),
                        format!("Allow turn · {directory}"),
                        "Allow writes below this directory for the current turn".to_string(),
                    ));
                    options.push((
                        PermissionApprovalChoice::Decision(
                            PermissionApprovalDecision::allow_filesystem_session(
                                directory.clone(),
                            ),
                        ),
                        format!("Allow session · {directory}"),
                        "Allow writes below this directory for the current session".to_string(),
                    ));
                }
                options.push((
                    PermissionApprovalChoice::CollapseScopes,
                    "Hide directory scopes".to_string(),
                    "Return to the compact approval choices".to_string(),
                ));
            } else if !filesystem.scope_candidates.is_empty() {
                options.push((
                    PermissionApprovalChoice::ExpandScopes,
                    "Choose directory scope…".to_string(),
                    "Review canonical turn and session directory grants".to_string(),
                ));
            }
        } else {
            options.push((
                PermissionApprovalChoice::Decision(PermissionApprovalDecision::allow_session()),
                "Allow session".to_string(),
                "Remember this operation for the current session".to_string(),
            ));
        }
        if self.request.allow_always {
            options.push((
                PermissionApprovalChoice::Decision(PermissionApprovalDecision::allow_always()),
                "Allow permanent".to_string(),
                "Write a project-local permission grant".to_string(),
            ));
        }
        options.push((
            PermissionApprovalChoice::Decision(PermissionApprovalDecision::deny()),
            "Deny".to_string(),
            "Reject this operation".to_string(),
        ));
        options
    }

    pub(crate) fn desired_height(&self, width: u16) -> u16 {
        let inner_width = width.saturating_sub(4).max(1);
        let mut rows = vec![
            format!(
                "Permission required · {} · source: {}",
                self.request.tool_name,
                self.session_id.as_deref().unwrap_or("current")
            ),
            self.request.reason.clone(),
        ];
        if let Some(filesystem) = &self.request.filesystem {
            for target in &filesystem.targets {
                rows.push(format!("requested: {}", target.requested_path));
                if target.requested_path != target.resolved_path {
                    rows.push(format!("resolved:  {}", target.resolved_path));
                }
            }
            rows.push(
                "↑/↓ or j/k select | enter confirm | y once | a directory scopes | d/esc deny"
                    .to_string(),
            );
        } else {
            rows.push(format!("action: {}", self.request.summary));
            rows.push(
                self.request
                    .matched_rule
                    .as_ref()
                    .map(|rule| format!("matched: {rule}"))
                    .unwrap_or_default(),
            );
            rows.push(
                self.request
                    .suggested_rule
                    .as_ref()
                    .map(|rule| format!("grant: {rule}"))
                    .unwrap_or_default(),
            );
            rows.push(
                "↑/↓ or j/k select | enter confirm | y once | a session | p permanent | d/esc deny"
                    .to_string(),
            );
        }
        rows.push(self.notice.clone().unwrap_or_default());
        let wrapped_rows = rows
        .into_iter()
        .filter(|line| !line.is_empty())
        .map(|line| wrapped_height(&line, inner_width))
        .sum::<u16>();
        (self.options().len() as u16 + wrapped_rows + 4).max(10)
    }

    pub(crate) fn scroll_by(&mut self, amount: isize) {
        self.ensure_selected_visible = false;
        if amount.is_negative() {
            self.scroll = self.scroll.saturating_sub(amount.unsigned_abs() as u16);
        } else {
            self.scroll = self.scroll.saturating_add(amount as u16);
        }
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let max = self.options().len().saturating_sub(1) as isize;
        let selected = (self.selected as isize + delta).clamp(0, max) as usize;
        if selected != self.selected {
            self.selected = selected;
            self.ensure_selected_visible = true;
        }
    }

    pub(crate) fn set_scope_expanded(&mut self, expanded: bool) {
        self.scope_expanded = expanded;
        self.selected = 0;
        self.scroll = 0;
        self.ensure_selected_visible = true;
    }

    pub(crate) fn selected_choice(&self) -> PermissionApprovalChoice {
        self.options()
            .get(self.selected)
            .map(|(choice, _, _)| choice.clone())
            .unwrap_or_else(|| {
                PermissionApprovalChoice::Decision(PermissionApprovalDecision::deny())
            })
    }

    pub(crate) fn restore_panel(self) -> Option<BottomPanel> {
        self.previous_panel.map(|panel| *panel)
    }
}

#[derive(Debug, Clone)]
pub(crate) enum PermissionApprovalChoice {
    Decision(PermissionApprovalDecision),
    ExpandScopes,
    CollapseScopes,
}

pub(crate) fn wrapped_height(text: &str, width: u16) -> u16 {
    let width = usize::from(width.max(1));
    let display_width = UnicodeWidthStr::width(text);
    display_width.div_ceil(width).max(1) as u16
}

pub(crate) fn char_count(value: &str) -> usize {
    value.chars().count()
}

pub(crate) fn byte_index_for_char(value: &str, char_index: usize) -> usize {
    value
        .char_indices()
        .map(|(index, _)| index)
        .nth(char_index)
        .unwrap_or(value.len())
}

pub(crate) fn insert_char(value: &mut String, cursor: &mut usize, ch: char) {
    let len = char_count(value);
    *cursor = (*cursor).min(len);
    let byte_index = byte_index_for_char(value, *cursor);
    value.insert(byte_index, ch);
    *cursor += 1;
}

pub(crate) fn remove_previous_char(value: &mut String, cursor: &mut usize) {
    if *cursor == 0 {
        return;
    }
    let len = char_count(value);
    *cursor = (*cursor).min(len);
    let start = byte_index_for_char(value, (*cursor).saturating_sub(1));
    let end = byte_index_for_char(value, *cursor);
    value.replace_range(start..end, "");
    *cursor = (*cursor).saturating_sub(1);
}

pub(crate) fn remove_next_char(value: &mut String, cursor: &mut usize) {
    let len = char_count(value);
    *cursor = (*cursor).min(len);
    if *cursor >= len {
        return;
    }
    let start = byte_index_for_char(value, *cursor);
    let end = byte_index_for_char(value, (*cursor).saturating_add(1));
    value.replace_range(start..end, "");
}

pub(crate) fn move_cursor(value: &str, cursor: &mut usize, delta: isize) {
    let len = char_count(value);
    let current = (*cursor).min(len) as isize;
    *cursor = (current + delta).clamp(0, len as isize) as usize;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderWizardField {
    Label,
    ProviderId,
    BaseUrl,
    ApiKeyEnv,
    ApiKey,
}

impl BottomSelectionPanel {
    pub(crate) fn new(
        title: &str,
        _subtitle: &str,
        empty_label: &str,
        rows: Vec<BottomSelectionRow>,
    ) -> Self {
        Self {
            title: title.to_string(),
            empty_label: empty_label.to_string(),
            footer: "Enter select  Esc close  Type search".to_string(),
            notice: None,
            session_view: None,
            action_armed: false,
            delete_confirm: None,
            running_session_ids: BTreeSet::new(),
            rows,
            query: String::new(),
            selected: 0,
            scroll: 0,
        }
    }

    pub(crate) fn new_sessions(view: SessionListView, rows: Vec<BottomSelectionRow>) -> Self {
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

    pub(crate) fn new_agent_actions(agent_name: &str, rows: Vec<BottomSelectionRow>) -> Self {
        let mut panel = Self::new(
            &format!("Agent {agent_name}"),
            "",
            "No actions available",
            rows,
        );
        panel.footer = "Enter select  Esc back".to_string();
        panel
    }

    pub(crate) fn filtered_indices(&self) -> Vec<usize> {
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

    pub(crate) fn filtered_model_indices(&self, query: &str) -> Vec<usize> {
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
                BottomSelectionValue::ProviderPreset(_)
                | BottomSelectionValue::ProviderBaseUrl { .. } => {
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
                BottomSelectionValue::Model { model, .. }
                    if row.search_text.to_lowercase().contains(query)
                        || row.label.to_lowercase().contains(query) =>
                {
                    include.insert(index, ());
                    if let Some(provider_index) = provider_rows.get(&model.provider) {
                        include.insert(*provider_index, ());
                    }
                }
                _ => {}
            }
        }
        include.into_keys().collect()
    }

    pub(crate) fn selected_value(&self) -> Option<BottomSelectionValue> {
        self.filtered_indices()
            .get(self.selected)
            .and_then(|index| self.rows.get(*index))
            .map(|row| row.value.clone())
    }

    pub(crate) fn selected_row(&self) -> Option<&BottomSelectionRow> {
        self.filtered_indices()
            .get(self.selected)
            .and_then(|index| self.rows.get(*index))
    }

    pub(crate) fn row_has_running_activity(&self, row: &BottomSelectionRow) -> bool {
        if row.is_current {
            return false;
        }
        matches!(
            &row.value,
            BottomSelectionValue::Session(session_id)
                if self.running_session_ids.contains(session_id)
        )
    }

    pub(crate) fn selected_key(&self) -> String {
        self.selected_value()
            .map(|value| value.key())
            .unwrap_or_else(|| "fetch:all".to_string())
    }

    pub(crate) fn select_value_key(&mut self, key: &str) {
        let filtered = self.filtered_indices();
        if let Some(index) = filtered
            .iter()
            .position(|row_index| self.rows[*row_index].value.key() == key)
        {
            self.selected = index;
            self.ensure_selected_visible(8);
        }
    }

    pub(crate) fn footer_text(&self) -> String {
        if self.delete_confirm.is_some() {
            return "Ctrl+K D confirm delete  Esc cancel".to_string();
        }
        if self.action_armed {
            return match self.session_view.unwrap_or(SessionListView::Active) {
                SessionListView::Active => "F fork  A archive  D delete  Esc cancel".to_string(),
                SessionListView::Archived => "R restore  D delete  Esc cancel".to_string(),
            };
        }
        self.filtered_indices()
            .get(self.selected)
            .and_then(|index| self.rows.get(*index))
            .and_then(|row| row.footer.clone())
            .unwrap_or_else(|| self.footer.clone())
    }

    pub(crate) fn set_query_char(&mut self, c: char) {
        self.query.push(c);
        self.selected = 0;
        self.scroll = 0;
        self.clear_transient_action_state();
    }

    pub(crate) fn backspace_query(&mut self) {
        self.query.pop();
        self.selected = 0;
        self.scroll = 0;
        self.clear_transient_action_state();
    }

    pub(crate) fn move_selection(&mut self, direction: isize) {
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

    pub(crate) fn move_to(&mut self, index: usize) {
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

    pub(crate) fn ensure_selected_visible(&mut self, visible_rows: u16) {
        let selected = self.selected as u16;
        if selected < self.scroll {
            self.scroll = selected;
        }
        if selected >= self.scroll.saturating_add(visible_rows) {
            self.scroll = selected.saturating_sub(visible_rows.saturating_sub(1));
        }
        self.clamp_scroll(visible_rows);
    }

    pub(crate) fn clamp_scroll(&mut self, visible_rows: u16) {
        let len = self.filtered_indices().len() as u16;
        let max = len.saturating_sub(visible_rows);
        self.scroll = self.scroll.min(max);
    }

    pub(crate) fn set_selected(&mut self, index: usize) {
        self.selected = index.min(self.filtered_indices().len().saturating_sub(1));
        self.scroll = 0;
        self.clear_transient_action_state();
    }

    pub(crate) fn arm_action_mode(&mut self) {
        if self.session_view.is_none() {
            return;
        }
        self.action_armed = true;
        if self.delete_confirm.is_none() {
            self.notice = Some(match self.session_view.unwrap_or(SessionListView::Active) {
                SessionListView::Active => "action: F fork  A archive  D delete".to_string(),
                SessionListView::Archived => "action: R restore  D delete".to_string(),
            });
        }
    }

    pub(crate) fn cancel_transient_action(&mut self) -> bool {
        let had_transient = self.action_armed || self.delete_confirm.is_some();
        if had_transient {
            self.clear_transient_action_state();
        }
        had_transient
    }

    pub(crate) fn clear_transient_action_state(&mut self) {
        self.action_armed = false;
        self.delete_confirm = None;
        self.notice = None;
    }
}

impl BottomSelectionValue {
    pub(crate) fn key(&self) -> String {
        match self {
            BottomSelectionValue::HistoryMessageAction { message_id, action } => {
                format!("history-message:{message_id}:{action:?}")
            }
            BottomSelectionValue::Session(id) => format!("session:{id}"),
            BottomSelectionValue::LoadOlderSessions(cwd) => {
                format!("sessions:load-older:{cwd}")
            }
            BottomSelectionValue::AgentRunning {
                child_session_id, ..
            } => {
                format!("agent:running:{child_session_id}")
            }
            BottomSelectionValue::AgentAvailable {
                name,
                source,
                path,
                entrypoints: _,
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
            BottomSelectionValue::ProviderPreset(preset) => {
                format!("provider:preset:{}", preset.key())
            }
            BottomSelectionValue::ProviderBaseUrl { preset, index } => {
                format!(
                    "provider:base-url:{}:{}",
                    preset.key(),
                    index
                        .map(|index| index.to_string())
                        .unwrap_or_else(|| "custom".to_string())
                )
            }
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
            BottomSelectionValue::Toolset { name, .. } => format!("toolset:{name}"),
            BottomSelectionValue::Model { model, .. } => {
                format!("model:{}", format_model_spec(model))
            }
            BottomSelectionValue::Variant { model, variant, .. } => {
                format!(
                    "variant:{model}:{}",
                    variant.as_deref().unwrap_or("default")
                )
            }
        }
    }
}
