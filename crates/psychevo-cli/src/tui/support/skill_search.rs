#[derive(Debug, Clone, PartialEq, Eq)]
struct SkillSearchMatch {
    name: String,
    description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SkillToken {
    row: usize,
    start_col: usize,
    end_col: usize,
    query: String,
}

#[derive(Debug, Clone, Default)]
struct SkillSearchState {
    popup: Option<SkillSearchPopupState>,
    dismissed_query: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SkillSearchPopupState {
    query: String,
    matches: Vec<SkillSearchMatch>,
    selected: usize,
}

impl SkillSearchState {
    fn sync(&mut self, token: Option<&SkillToken>, matches: Vec<SkillSearchMatch>) {
        let Some(token) = token else {
            self.close();
            self.dismissed_query = None;
            return;
        };
        if self.dismissed_query.as_deref() == Some(token.query.as_str()) {
            self.popup = None;
            return;
        }
        self.dismissed_query = None;
        let selected = self
            .popup
            .as_ref()
            .filter(|popup| popup.query == token.query)
            .map(|popup| popup.selected)
            .unwrap_or(0)
            .min(matches.len().saturating_sub(1));
        self.popup = Some(SkillSearchPopupState {
            query: token.query.clone(),
            matches,
            selected,
        });
    }

    fn close(&mut self) {
        self.popup = None;
    }

    fn dismiss(&mut self, query: Option<String>) {
        self.dismissed_query = query;
        self.close();
    }

    fn height(&self) -> u16 {
        let Some(popup) = &self.popup else {
            return 0;
        };
        let rows = popup.matches.len().clamp(1, FILE_POPUP_MAX_ROWS);
        (rows as u16 + 2).min(FILE_POPUP_MAX_ROWS as u16 + 2)
    }

    fn selected_name(&self) -> Option<String> {
        self.popup
            .as_ref()
            .and_then(|popup| popup.matches.get(popup.selected))
            .map(|entry| entry.name.clone())
    }

    fn move_selection(&mut self, direction: isize) {
        let Some(popup) = &mut self.popup else {
            return;
        };
        let len = popup.matches.len();
        if len == 0 {
            popup.selected = 0;
            return;
        }
        let current = popup.selected.min(len.saturating_sub(1)) as isize;
        popup.selected = (current + direction).rem_euclid(len as isize) as usize;
    }

    fn set_selection(&mut self, index: usize) {
        let Some(popup) = &mut self.popup else {
            return;
        };
        let len = popup.matches.len();
        popup.selected = if len == 0 {
            0
        } else {
            index.min(len.saturating_sub(1))
        };
    }
}
