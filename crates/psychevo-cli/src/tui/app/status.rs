impl TuiApp {
    fn run_options(&self, prompt: String) -> RunOptions {
        self.run_options_with_images(prompt, Vec::new())
    }

    fn run_options_with_images(&self, prompt: String, image_inputs: Vec<ImageInput>) -> RunOptions {
        RunOptions {
            db_path: self.db_path.clone(),
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
            model: self.current_model.clone(),
            reasoning_effort: self.current_variant.clone(),
            include_reasoning: false,
            mode: self.current_mode,
            inherited_env: Some(self.env_map.clone()),
            agent: self.current_agent.clone(),
            no_agents: self.no_agents,
            no_skills: self.no_skills,
            skill_inputs: self.skill_inputs.clone(),
        }
    }

    fn user_shell_context_options(&self) -> UserShellContextOptions {
        UserShellContextOptions {
            db_path: self.db_path.clone(),
            session: self.current_session.clone(),
            continue_latest: self.current_session.is_none() && !self.force_new_once,
            source: "tui".to_string(),
            continue_sources: TUI_SESSION_SOURCES
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

    fn show_status(&self) -> Result<()> {
        println!("{}", self.status_text());
        Ok(())
    }

    fn show_session_list(&self) -> Result<()> {
        for line in self.session_list_lines()? {
            println!("{line}");
        }
        Ok(())
    }

    fn show_model(&self) -> Result<()> {
        for line in self.model_lines()? {
            println!("{line}");
        }
        Ok(())
    }

    fn set_variant(&mut self, variant: String) -> Result<()> {
        self.set_variant_no_print(variant.clone())?;
        println!("{}", self.renderer.status(&format!("variant: {variant}")));
        Ok(())
    }

    fn toggle_thinking(&mut self) -> Result<()> {
        self.set_thinking_no_print(!self.thinking_visible)?;
        self.show_thinking_status();
        Ok(())
    }

    fn set_thinking(&mut self, enabled: bool) -> Result<()> {
        self.set_thinking_no_print(enabled)?;
        self.show_thinking_status();
        Ok(())
    }

    fn toggle_raw(&mut self) -> Result<()> {
        self.set_raw_no_print(!self.raw_visible)?;
        self.show_raw_status();
        Ok(())
    }

    fn set_raw(&mut self, enabled: bool) -> Result<()> {
        self.set_raw_no_print(enabled)?;
        self.show_raw_status();
        Ok(())
    }

    fn show_thinking_status(&self) {
        println!(
            "{}",
            self.renderer
                .status(&format!("thinking: {}", on_off(self.thinking_visible)))
        );
    }

    fn show_raw_status(&self) {
        println!(
            "{}",
            self.renderer
                .status(&format!("raw: {}", on_off(self.raw_visible)))
        );
    }

    fn set_mode(&mut self, mode: String) -> Result<()> {
        self.set_mode_no_print(&mode)?;
        println!("{}", self.renderer.status(&format!("mode: {mode}")));
        Ok(())
    }

    fn rename_session(&mut self, title: String) -> Result<()> {
        let title = self.rename_session_no_print(title)?;
        println!(
            "{}",
            self.renderer.status(&format!("session renamed: {title}"))
        );
        Ok(())
    }

    fn undo_session_print(&mut self) -> Result<()> {
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

    fn redo_session_print(&mut self) -> Result<()> {
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

    fn status_lines(&self) -> Vec<String> {
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
            format!(
                "agent: {}",
                self.current_agent.as_deref().unwrap_or("(default)")
            ),
            format!("agents: {}", if self.no_agents { "off" } else { "on" }),
            format!("debug: {}", on_off(self.debug)),
        ]
    }

    fn status_text(&self) -> String {
        self.status_lines().join("\n")
    }

    fn session_list_lines(&self) -> Result<Vec<String>> {
        let sessions = self.tui_sessions_for_workdir(SessionListView::Active)?;
        if sessions.is_empty() {
            return Ok(vec!["no sessions for this workdir".to_string()]);
        }
        Ok(sessions
            .into_iter()
            .map(|session| {
                let summary = &session.summary;
                format_session_line(
                    &summary.id,
                    &summary.source,
                    &summary.provider,
                    &summary.model,
                    session.visible_message_count as i64,
                )
            })
            .collect())
    }

}
