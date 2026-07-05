#[allow(unused_imports)]
pub(crate) use super::*;
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillSearchMatch {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) source_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillToken {
    pub(crate) row: usize,
    pub(crate) start_col: usize,
    pub(crate) end_col: usize,
    pub(crate) query: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SkillSearchState {
    pub(crate) popup: Option<SkillSearchPopupState>,
    pub(crate) dismissed_query: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillSearchPopupState {
    pub(crate) query: String,
    pub(crate) matches: Vec<SkillSearchMatch>,
    pub(crate) selected: usize,
}

impl SkillSearchState {
    pub(crate) fn sync(&mut self, token: Option<&SkillToken>, matches: Vec<SkillSearchMatch>) {
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

    pub(crate) fn close(&mut self) {
        self.popup = None;
    }

    pub(crate) fn dismiss(&mut self, query: Option<String>) {
        self.dismissed_query = query;
        self.close();
    }

    pub(crate) fn selected_name(&self) -> Option<String> {
        self.popup
            .as_ref()
            .and_then(|popup| popup.matches.get(popup.selected))
            .map(|entry| entry.name.clone())
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
