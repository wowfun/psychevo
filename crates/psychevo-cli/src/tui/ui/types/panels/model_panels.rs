
impl BottomPanel {
    pub(crate) fn selection(&self) -> &BottomSelectionPanel {
        match self {
            BottomPanel::Sessions(panel)
            | BottomPanel::Stats(panel)
            | BottomPanel::Tools(panel) => panel,
            BottomPanel::Agents(panel) => match panel.tab {
                AgentTab::Running => &panel.running,
                AgentTab::Available => &panel.available,
            },
            BottomPanel::AgentActions(panel) => panel,
            BottomPanel::Models(panel) => &panel.models,
            BottomPanel::Help(_) => {
                panic!("help panel does not expose a selection panel")
            }
            BottomPanel::AgentRunPrompt(_) | BottomPanel::AgentEditor(_) => {
                panic!("agent form panel does not expose a selection panel")
            }
            BottomPanel::ProviderWizard(_) => {
                panic!("provider wizard does not expose a selection panel")
            }
            BottomPanel::PermissionApproval(_) => {
                panic!("permission approval panel does not expose a selection panel")
            }
            BottomPanel::Clarify(_) => {
                panic!("clarify panel does not expose a selection panel")
            }
            BottomPanel::Variants { panel, .. } => panel,
        }
    }

    pub(crate) fn selection_mut(&mut self) -> &mut BottomSelectionPanel {
        match self {
            BottomPanel::Sessions(panel)
            | BottomPanel::Stats(panel)
            | BottomPanel::Tools(panel) => panel,
            BottomPanel::Agents(panel) => match panel.tab {
                AgentTab::Running => &mut panel.running,
                AgentTab::Available => &mut panel.available,
            },
            BottomPanel::AgentActions(panel) => panel,
            BottomPanel::Models(panel) => &mut panel.models,
            BottomPanel::Help(_) => {
                panic!("help panel does not expose a selection panel")
            }
            BottomPanel::AgentRunPrompt(_) | BottomPanel::AgentEditor(_) => {
                panic!("agent form panel does not expose a selection panel")
            }
            BottomPanel::ProviderWizard(_) => {
                panic!("provider wizard does not expose a selection panel")
            }
            BottomPanel::PermissionApproval(_) => {
                panic!("permission approval panel does not expose a selection panel")
            }
            BottomPanel::Clarify(_) => {
                panic!("clarify panel does not expose a selection panel")
            }
            BottomPanel::Variants { panel, .. } => panel,
        }
    }

    pub(crate) fn selected_value(&self) -> Option<BottomSelectionValue> {
        self.selection().selected_value()
    }

    pub(crate) fn session_view(&self) -> Option<SessionListView> {
        match self {
            BottomPanel::Sessions(panel) => panel.session_view,
            BottomPanel::Help(_)
            | BottomPanel::Agents(_)
            | BottomPanel::AgentActions(_)
            | BottomPanel::AgentRunPrompt(_)
            | BottomPanel::AgentEditor(_)
            | BottomPanel::Models(_)
            | BottomPanel::Stats(_)
            | BottomPanel::Tools(_)
            | BottomPanel::ProviderWizard(_)
            | BottomPanel::PermissionApproval(_)
            | BottomPanel::Clarify(_)
            | BottomPanel::Variants { .. } => None,
        }
    }

    pub(crate) fn move_selection(&mut self, direction: isize) {
        self.selection_mut().move_selection(direction);
    }
}

impl HelpPanel {
    pub(crate) fn new(sections: SlashHelpSections) -> Self {
        Self {
            sections,
            tab: HelpTab::General,
            scroll: 0,
        }
    }

    pub(crate) fn move_tab(&mut self, direction: isize) {
        let current = self.tab_index() as isize;
        let next = (current + direction).rem_euclid(Self::tabs().len() as isize) as usize;
        self.tab = Self::tabs()[next];
        self.scroll = 0;
    }

    pub(crate) fn set_tab(&mut self, tab: HelpTab) {
        self.tab = tab;
        self.scroll = 0;
    }

    pub(crate) fn scroll_by(&mut self, direction: isize) {
        if direction.is_negative() {
            self.scroll = self.scroll.saturating_sub(direction.unsigned_abs() as u16);
        } else {
            self.scroll = self.scroll.saturating_add(direction as u16);
        }
    }

    pub(crate) fn tab_index(&self) -> usize {
        Self::tabs()
            .iter()
            .position(|tab| *tab == self.tab)
            .unwrap_or(0)
    }

    pub(crate) fn tabs() -> &'static [HelpTab] {
        &[HelpTab::General, HelpTab::Commands, HelpTab::CustomCommands]
    }
}

impl HelpTab {
    pub(crate) fn label(self) -> &'static str {
        match self {
            HelpTab::General => "General",
            HelpTab::Commands => "Commands",
            HelpTab::CustomCommands => "Custom commands",
        }
    }
}
