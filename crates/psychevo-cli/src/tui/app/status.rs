#[allow(unused_imports)]
pub(crate) use super::*;
impl TuiApp {
    pub(crate) fn run_options(&self, prompt: String) -> RunOptions {
        self.run_options_with_images(prompt, Vec::new())
    }

    pub(crate) fn run_options_with_images(
        &self,
        prompt: String,
        image_inputs: Vec<ImageInput>,
    ) -> RunOptions {
        RunOptions {
            state: self.state_runtime.clone(),
            workdir: self.workdir.clone(),
            snapshot_root: Some(self.home.join("snapshots")),
            session: self.current_session.clone(),
            continue_latest: self.current_session.is_none() && !self.force_new_once,
            prompt,
            image_inputs,
            extract_prompt_image_sources: false,
            prompt_display: None,
            max_context_messages: None,
            config_path: self.config_path.clone(),
            project_context_override: None,
            sandbox_override: None,
            model: self.current_model.clone(),
            reasoning_effort: self.current_variant.clone(),
            runtime_ref: None,
            runtime_session_id: None,
            runtime_options: std::collections::BTreeMap::new(),
            external_agent_delegate: None,
            include_reasoning: false,
            mode: self.current_mode,
            permission_mode: Some(self.current_permission_mode),
            approval_mode: None,
            approval_handler: None,
            clarify_enabled: true,
            inherited_env: Some(self.env_map.clone()),
            agent: self.current_agent.clone(),
            no_agents: self.no_agents,
            no_skills: self.no_skills,
            skill_inputs: self.skill_inputs.clone(),
            mcp_servers: Vec::new(),
        }
    }

    pub(crate) fn user_shell_context_options(&self) -> UserShellContextOptions {
        UserShellContextOptions {
            state: self.state_runtime.clone(),
            session: self.current_session.clone(),
            continue_latest: self.current_session.is_none() && !self.force_new_once,
            source: "tui".to_string(),
            continue_sources: TUI_CONTINUE_SESSION_SOURCES
                .iter()
                .map(|source| (*source).to_string())
                .collect(),
            config_path: self.config_path.clone(),
            model: self.current_model.clone(),
            reasoning_effort: self.current_variant.clone(),
            mode: self.current_mode,
            inherited_env: Some(self.env_map.clone()),
        }
    }

    pub(crate) fn show_status(&self) -> Result<()> {
        println!("{}", self.status_text());
        Ok(())
    }

    pub(crate) fn show_session_list(&self) -> Result<()> {
        for line in self.session_list_lines()? {
            println!("{line}");
        }
        Ok(())
    }

    pub(crate) fn show_model(&self) -> Result<()> {
        for line in self.model_lines()? {
            println!("{line}");
        }
        Ok(())
    }

    pub(crate) fn toolsets_status_text(&self) -> Result<String> {
        let value = toolsets_value(&self.run_options(String::new()), ConfigScope::Effective)?;
        let mode_key = self.current_mode.as_str();
        let tools = value["modes"][mode_key]["effective_tools"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let mut lines = vec![format!("mode: {mode_key}"), format!("tools: {tools}")];
        for row in value["toolsets"].as_array().cloned().unwrap_or_default() {
            lines.push(format!(
                "- {} ({}) {}",
                row["name"].as_str().unwrap_or("-"),
                row["source"].as_str().unwrap_or("-"),
                row["description"].as_str().unwrap_or("")
            ));
        }
        Ok(lines.join("\n"))
    }

    pub(crate) fn set_variant(&mut self, variant: String) -> Result<()> {
        self.set_variant_no_print(variant.clone())?;
        println!("{}", self.renderer.status(&format!("variant: {variant}")));
        Ok(())
    }

    pub(crate) fn toggle_thinking(&mut self) -> Result<()> {
        self.set_thinking_no_print(!self.thinking_visible)?;
        self.show_thinking_status();
        Ok(())
    }

    pub(crate) fn set_thinking(&mut self, enabled: bool) -> Result<()> {
        self.set_thinking_no_print(enabled)?;
        self.show_thinking_status();
        Ok(())
    }

    pub(crate) fn toggle_raw(&mut self) -> Result<()> {
        self.set_raw_no_print(!self.raw_visible)?;
        self.show_raw_status();
        Ok(())
    }

    pub(crate) fn set_raw(&mut self, enabled: bool) -> Result<()> {
        self.set_raw_no_print(enabled)?;
        self.show_raw_status();
        Ok(())
    }

    pub(crate) fn show_thinking_status(&self) {
        println!(
            "{}",
            self.renderer
                .status(&format!("thinking: {}", on_off(self.thinking_visible)))
        );
    }

    pub(crate) fn show_raw_status(&self) {
        println!(
            "{}",
            self.renderer
                .status(&format!("raw: {}", on_off(self.raw_visible)))
        );
    }

    pub(crate) fn set_mode(&mut self, mode: String) -> Result<()> {
        self.set_mode_no_print(&mode)?;
        println!("{}", self.renderer.status(&format!("mode: {mode}")));
        Ok(())
    }

    pub(crate) fn rename_session(&mut self, title: String) -> Result<()> {
        let title = self.rename_session_no_print(title)?;
        println!(
            "{}",
            self.renderer.status(&format!("session renamed: {title}"))
        );
        Ok(())
    }

    pub(crate) fn undo_session_print(&mut self) -> Result<()> {
        let result = undo_session(self.undo_options()?)?;
        println!(
            "{}",
            self.renderer.status(&format!(
                "undone {} messages; prompt restored",
                result.reverted_messages
            ))
        );
        Ok(())
    }

    pub(crate) fn redo_session_print(&mut self) -> Result<()> {
        let result = redo_session(self.undo_options()?)?;
        let suffix = if result.complete {
            "complete"
        } else {
            "partial"
        };
        println!(
            "{}",
            self.renderer.status(&format!(
                "redone {} messages; {suffix}",
                result.restored_messages
            ))
        );
        Ok(())
    }

    pub(crate) fn status_lines(&self) -> Vec<String> {
        vec![
            format!("workdir: {}", self.workdir.display()),
            format!("home: {}", self.home.display()),
            format!("db: {}", self.db_path.display()),
            format!(
                "session: {}",
                self.current_session.as_deref().unwrap_or("(none)")
            ),
            format!("model: {}", self.model_display_value()),
            self.variant_line(),
            format!("mode: {}", self.current_mode.as_str()),
            format!("permission_mode: {}", self.current_permission_mode.as_str()),
            format!(
                "agent: {}",
                self.current_agent.as_deref().unwrap_or("(default)")
            ),
            format!("agents: {}", if self.no_agents { "off" } else { "on" }),
            format!("debug: {}", on_off(self.debug)),
        ]
    }

    pub(crate) fn status_text(&self) -> String {
        self.status_lines().join("\n")
    }

    pub(crate) fn session_list_lines(&self) -> Result<Vec<String>> {
        let sessions = self.tui_sessions(SessionListView::Active)?;
        if sessions.is_empty() {
            return Ok(vec!["no sessions".to_string()]);
        }
        Ok(sessions
            .into_iter()
            .map(|session| {
                let summary = &session.summary;
                format_session_line(
                    &summary.id,
                    &session.project_label,
                    &summary.provider,
                    &summary.model,
                    session.visible_message_count as i64,
                )
            })
            .collect())
    }
}
