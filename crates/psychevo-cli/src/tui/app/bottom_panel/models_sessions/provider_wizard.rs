#[allow(unused_imports)]
pub(crate) use super::*;

impl TuiApp {
    pub(crate) fn handle_provider_wizard_key(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        key: KeyEvent,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                ui.bottom_panel = Some(BottomPanel::Models(ModelPanel::new(
                    self.model_selection_panel()?,
                )));
            }
            KeyCode::Enter => {
                let save = ui
                    .bottom_panel
                    .as_ref()
                    .and_then(|panel| match panel {
                        BottomPanel::ProviderWizard(panel) => Some(panel.is_last_field()),
                        _ => None,
                    })
                    .unwrap_or(false);
                if save {
                    self.save_provider_wizard(ui)?;
                } else if let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel {
                    panel.move_field(1);
                }
            }
            KeyCode::Up => {
                if let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel {
                    panel.move_field(-1);
                }
            }
            KeyCode::Down | KeyCode::Tab => {
                if let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel {
                    panel.move_field(1);
                }
            }
            KeyCode::Home => {
                if let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel {
                    panel.move_to_first_field();
                }
            }
            KeyCode::End => {
                if let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel {
                    panel.move_to_last_field();
                }
            }
            KeyCode::Backspace => {
                if let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel {
                    panel.backspace();
                }
                self.refresh_provider_wizard_env_state(ui);
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel {
                    panel.insert_char(c);
                }
                self.refresh_provider_wizard_env_state(ui);
            }
            _ => {}
        }
        Ok(false)
    }

    pub(crate) fn provider_wizard_panel(&self) -> ProviderWizardPanel {
        let mut panel = ProviderWizardPanel::custom();
        self.refresh_provider_wizard_panel_env(&mut panel);
        panel
    }

    pub(crate) fn provider_wizard_panel_for_preset(
        &self,
        preset: ProviderSetupPresetId,
        base_url_index: Option<usize>,
    ) -> ProviderWizardPanel {
        let definition = provider_setup_preset(preset);
        let base_url = base_url_index
            .and_then(|index| definition.base_urls.get(index))
            .or_else(|| definition.base_urls.first())
            .map(|base_url| base_url.url)
            .unwrap_or_default()
            .to_string();
        let env_map = definition
            .api_key_env_candidates
            .iter()
            .find(|candidate| self.global_dotenv_has_value(candidate))
            .map(|candidate| BTreeMap::from([((*candidate).to_string(), "present".to_string())]))
            .unwrap_or_default();
        let api_key_env = default_provider_setup_api_key_env(
            definition.api_key_env_candidates,
            &env_map,
            definition.provider_id.unwrap_or_default(),
        );
        let mut panel = ProviderWizardPanel::built_in(preset, base_url, api_key_env);
        self.refresh_provider_wizard_panel_env(&mut panel);
        panel
    }

    pub(crate) fn refresh_provider_wizard_env_state(&self, ui: &mut FullscreenUi<'_>) {
        if let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel {
            self.refresh_provider_wizard_panel_env(panel);
        }
    }

    pub(crate) fn refresh_provider_wizard_panel_env(&self, panel: &mut ProviderWizardPanel) {
        panel.api_key_env_present = panel
            .env_var()
            .as_deref()
            .is_some_and(|key| self.global_dotenv_has_value(key));
        if panel.api_key_env_present {
            panel.api_key.clear();
            if panel.active_field == ProviderWizardField::ApiKey {
                panel.move_to_last_field();
            }
        }
    }

    pub(crate) fn global_dotenv_has_value(&self, key: &str) -> bool {
        let Ok(text) = fs::read_to_string(self.home.join(".env")) else {
            return false;
        };
        text.lines().any(|line| {
            let line = line.trim();
            let Some((name, value)) = line.split_once('=') else {
                return false;
            };
            name.trim() == key && !strip_dotenv_quotes(value.trim()).trim().is_empty()
        })
    }

    pub(crate) fn save_provider_wizard(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        let Some(BottomPanel::ProviderWizard(panel)) = ui.bottom_panel.as_ref() else {
            return Ok(());
        };
        let panel = panel.clone();
        let result = self.save_provider_wizard_panel(&panel);
        let provider_id = match result {
            Ok(provider_id) => provider_id,
            Err(err) => {
                if let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel {
                    panel.notice = Some(format!("error: {err}"));
                }
                return Ok(());
            }
        };
        self.sync_model_catalog_providers()?;
        let mut panel = ModelPanel::new(self.model_selection_panel()?);
        panel
            .models
            .select_value_key(&format!("fetch:provider:{provider_id}"));
        panel.models.notice = Some("provider saved; fetching models".to_string());
        ui.bottom_panel = Some(BottomPanel::Models(panel));
        self.start_model_catalog_fetch_provider(ui, &provider_id)?;
        ui.set_bottom_panel_notice("provider saved; fetching models");
        Ok(())
    }

    pub(crate) fn save_provider_wizard_panel(&self, panel: &ProviderWizardPanel) -> Result<String> {
        let api_key_env = panel
            .env_var()
            .ok_or_else(|| anyhow!("API key env var is required"))?;
        if looks_like_api_key(&api_key_env) {
            return Err(anyhow!(
                "API key env var looks like an API key; paste the key in the API key field"
            ));
        }
        let base_url = validate_base_url(&panel.base_url)?;
        let api_key_env = validate_api_key_env(&api_key_env)?;
        let api_key = (!panel.api_key_env_present)
            .then_some(panel.api_key.trim().to_string())
            .filter(|value| !value.is_empty());
        if api_key.is_none() && !panel.api_key_env_present && !is_loopback_base_url(&base_url) {
            return Err(anyhow!("provider requires API key for {api_key_env}"));
        }

        if panel.is_custom() {
            let result = create_scoped_custom_provider(ScopedCustomProviderInput {
                config_dir: self.home.clone(),
                provider_id: panel.provider_id.clone(),
                label: panel.label.clone(),
                base_url,
                api_key_env: Some(api_key_env),
                api_key,
                require_api_key: !is_loopback_base_url(&panel.base_url),
                no_auth: false,
            })?;
            return Ok(result.provider_id);
        }

        upsert_provider_options(
            &self.home,
            &panel.provider_id,
            &panel.label,
            &base_url,
            &api_key_env,
        )?;
        if let Some(api_key) = api_key {
            let options = self.run_options(String::new());
            let _ =
                set_provider_api_key(&options, self.home.clone(), &panel.provider_id, &api_key)?;
        }
        Ok(panel.provider_id.clone())
    }
}
