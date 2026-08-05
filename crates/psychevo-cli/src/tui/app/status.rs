use anyhow::Result;
use psychevo::{
    ConfigurationQuery, ImageInput, RefreshThreadContextRequest, ShellCommandRequest, TurnRequest,
    config::ConfigScope,
};
use serde_json::Value;

use crate::tui::{
    TUI_CONTINUE_SESSION_SOURCES, app_state::TuiApp, plain::format_session_line,
    render_helpers::on_off, ui_types::SessionListView,
};

impl TuiApp {
    pub(crate) fn framework_turn_request_with_images(
        &self,
        prompt: String,
        image_inputs: Vec<ImageInput>,
    ) -> TurnRequest {
        TurnRequest::new(prompt)
            .with_prompt_images(image_inputs, false)
            .with_identity("tui", None)
            .with_model(self.current_model.clone(), self.current_variant.clone())
            .with_execution_policy(
                self.current_mode,
                Some(self.current_permission_mode),
                self.config_path.clone(),
            )
            .with_approval(None, true)
            .with_environment(Some(self.env_map.clone()), None, None)
            .with_agent(self.current_agent.clone(), self.no_agents, self.no_skills)
            .with_skills(self.skill_inputs.clone())
    }

    pub(crate) fn configuration(&self) -> Result<psychevo::Configuration> {
        let mut query = ConfigurationQuery::new(&self.cwd);
        query.model = self.current_model.clone();
        query.reasoning_effort = self.current_variant.clone();
        query.inherited_env = Some(self.env_map.clone());
        Ok(self.runtime.client().configuration(query)?)
    }

    pub(crate) fn refresh_thread_context_request(
        &self,
        invalidation_reason: impl Into<String>,
        notice: Option<String>,
    ) -> RefreshThreadContextRequest {
        RefreshThreadContextRequest {
            mode: Some(self.current_mode),
            inherited_env: Some(self.env_map.clone()),
            agent: self.current_agent.clone(),
            no_agents: self.no_agents,
            no_skills: self.no_skills,
            invalidation_reason: invalidation_reason.into(),
            notice,
        }
    }

    pub(crate) fn shell_command_request(&self, command: String) -> ShellCommandRequest {
        self.shell_command_request_for_session(command, self.current_session.clone())
    }

    pub(crate) fn shell_command_request_for_session(
        &self,
        command: String,
        session_id: Option<String>,
    ) -> ShellCommandRequest {
        let request = ShellCommandRequest::new(&self.cwd, command)
            .source("tui")
            .model(self.current_model.clone(), self.current_variant.clone())
            .mode(self.current_mode)
            .inherited_environment(self.env_map.clone());
        if let Some(thread_id) = session_id {
            request.thread(thread_id)
        } else if !self.force_new_once {
            request.continue_latest(
                TUI_CONTINUE_SESSION_SOURCES
                    .iter()
                    .map(|source| (*source).to_string()),
            )
        } else {
            request
        }
    }

    pub(crate) fn show_status(&self) -> Result<()> {
        println!("{}", self.status_text());
        Ok(())
    }

    pub(crate) async fn show_session_list(&self) -> Result<()> {
        for line in self.session_list_lines().await? {
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
        let value = self.configuration()?.toolsets(ConfigScope::Effective)?;
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

    pub(crate) async fn set_variant(&mut self, variant: String) -> Result<()> {
        self.set_variant_no_print(variant.clone()).await?;
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

    pub(crate) async fn rename_session(&mut self, title: String) -> Result<()> {
        let title = self.rename_session_no_print(title).await?;
        println!(
            "{}",
            self.renderer.status(&format!("session renamed: {title}"))
        );
        Ok(())
    }

    pub(crate) async fn undo_session_print(&mut self) -> Result<()> {
        let result = self.current_framework_thread().await?.undo().await?;
        println!(
            "{}",
            self.renderer.status(&format!(
                "undone {} messages; prompt restored",
                result.reverted_messages
            ))
        );
        Ok(())
    }

    pub(crate) async fn redo_session_print(&mut self) -> Result<()> {
        let result = self.current_framework_thread().await?.redo().await?;
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
            format!("cwd: {}", self.cwd.display()),
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

    pub(crate) async fn session_list_lines(&self) -> Result<Vec<String>> {
        let sessions = self.tui_sessions(SessionListView::Active).await?;
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
