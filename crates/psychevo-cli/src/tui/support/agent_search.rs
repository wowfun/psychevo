#[allow(unused_imports)]
pub(crate) use super::*;
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentSearchMatch {
    pub(crate) name: String,
    pub(crate) description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentToken {
    pub(crate) row: usize,
    pub(crate) start_col: usize,
    pub(crate) end_col: usize,
    pub(crate) query: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AgentSearchState {
    pub(crate) popup: Option<AgentSearchPopupState>,
    pub(crate) dismissed_query: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentSearchPopupState {
    pub(crate) query: String,
    pub(crate) matches: Vec<AgentSearchMatch>,
    pub(crate) selected: usize,
}

impl AgentSearchState {
    pub(crate) fn sync(&mut self, token: Option<&AgentToken>, matches: Vec<AgentSearchMatch>) {
        let Some(token) = token else {
            self.close();
            self.dismissed_query = None;
            return;
        };
        if matches.is_empty() {
            self.close();
            return;
        }
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
        self.popup = Some(AgentSearchPopupState {
            query: token.query.clone(),
            matches,
            selected,
        });
    }

    pub(crate) fn close(&mut self) {
        self.popup = None;
    }

    pub(crate) fn dismiss(&mut self, query: Option<String>) {
        self.dismissed_query = query;
        self.close();
    }

    pub(crate) fn height(&self) -> u16 {
        let Some(popup) = &self.popup else {
            return 0;
        };
        popup.matches.len().clamp(1, FILE_POPUP_MAX_ROWS) as u16
    }

    pub(crate) fn selected_name(&self) -> Option<String> {
        self.popup
            .as_ref()
            .and_then(|popup| popup.matches.get(popup.selected))
            .map(|entry| entry.name.clone())
    }

    pub(crate) fn move_selection(&mut self, direction: isize) {
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

    pub(crate) fn set_selection(&mut self, index: usize) {
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
