#[allow(unused_imports)]
pub(crate) use super::*;

impl ModelPanel {
    pub(crate) fn new(models: BottomSelectionPanel) -> Self {
        Self::new_with_scope(models, false)
    }

    pub(crate) fn new_with_scope(models: BottomSelectionPanel, global: bool) -> Self {
        Self {
            models,
            tab: ModelTab::Models,
            info_scroll: 0,
            global,
        }
    }

    pub(crate) fn move_tab(&mut self, direction: isize) {
        let current = self.tab_index() as isize;
        let next = (current + direction).rem_euclid(Self::tabs().len() as isize) as usize;
        self.tab = Self::tabs()[next];
    }

    pub(crate) fn scroll_info_by(&mut self, direction: isize) {
        if direction.is_negative() {
            self.info_scroll = self
                .info_scroll
                .saturating_sub(direction.unsigned_abs() as u16);
        } else {
            self.info_scroll = self.info_scroll.saturating_add(direction as u16);
        }
    }

    pub(crate) fn tab_index(&self) -> usize {
        Self::tabs()
            .iter()
            .position(|tab| *tab == self.tab)
            .unwrap_or(0)
    }

    pub(crate) fn tabs() -> &'static [ModelTab] {
        &[ModelTab::Models, ModelTab::Info]
    }
}

impl ModelTab {
    pub(crate) fn label(self) -> &'static str {
        match self {
            ModelTab::Models => "Models",
            ModelTab::Info => "Info",
        }
    }
}

impl ProviderWizardPanel {
    pub(crate) fn new() -> Self {
        Self {
            label: String::new(),
            provider_id: String::new(),
            base_url: String::new(),
            api_key: String::new(),
            provider_id_touched: false,
            api_key_env_present: false,
            active_field: ProviderWizardField::Label,
            notice: None,
        }
    }

    pub(crate) fn active_fields(&self) -> Vec<ProviderWizardField> {
        let mut fields = vec![
            ProviderWizardField::Label,
            ProviderWizardField::ProviderId,
            ProviderWizardField::BaseUrl,
        ];
        if !self.api_key_env_present {
            fields.push(ProviderWizardField::ApiKey);
        }
        fields
    }

    pub(crate) fn move_field(&mut self, direction: isize) {
        let fields = self.active_fields();
        let current = fields
            .iter()
            .position(|field| *field == self.active_field)
            .unwrap_or(0) as isize;
        self.active_field =
            fields[(current + direction).rem_euclid(fields.len() as isize) as usize];
        self.notice = None;
    }

    pub(crate) fn move_to_first_field(&mut self) {
        self.active_field = ProviderWizardField::Label;
        self.notice = None;
    }

    pub(crate) fn move_to_last_field(&mut self) {
        self.active_field = *self
            .active_fields()
            .last()
            .unwrap_or(&ProviderWizardField::BaseUrl);
        self.notice = None;
    }

    pub(crate) fn insert_char(&mut self, ch: char) {
        match self.active_field {
            ProviderWizardField::Label => {
                self.label.push(ch);
                if !self.provider_id_touched {
                    self.provider_id = provider_id_slug(&self.label);
                }
            }
            ProviderWizardField::ProviderId => {
                self.provider_id.push(ch);
                self.provider_id_touched = true;
            }
            ProviderWizardField::BaseUrl => self.base_url.push(ch),
            ProviderWizardField::ApiKey => self.api_key.push(ch),
        }
        self.notice = None;
    }

    pub(crate) fn backspace(&mut self) {
        match self.active_field {
            ProviderWizardField::Label => {
                self.label.pop();
                if !self.provider_id_touched {
                    self.provider_id = provider_id_slug(&self.label);
                }
            }
            ProviderWizardField::ProviderId => {
                self.provider_id.pop();
                self.provider_id_touched = true;
            }
            ProviderWizardField::BaseUrl => {
                self.base_url.pop();
            }
            ProviderWizardField::ApiKey => {
                self.api_key.pop();
            }
        }
        self.notice = None;
    }

    pub(crate) fn env_var(&self) -> Option<String> {
        (!self.provider_id.trim().is_empty())
            .then(|| custom_provider_api_key_env(self.provider_id.trim()))
    }

    pub(crate) fn is_last_field(&self) -> bool {
        self.active_fields()
            .last()
            .is_some_and(|field| *field == self.active_field)
    }
}

pub(crate) fn provider_id_slug(label: &str) -> String {
    let mut slug = String::new();
    let mut previous_sep = false;
    for ch in label.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            previous_sep = false;
        } else if matches!(ch, '-' | '_' | ' ' | '.' | '/' | ':')
            && !previous_sep
            && !slug.is_empty()
        {
            slug.push('-');
            previous_sep = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}
