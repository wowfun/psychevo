use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use psychevo::agents::MAX_AGENT_SPAWN_DEPTH_CAP;
use psychevo::application::StartAgentTaskRequest;

use crate::tui::app_bottom_panel::{
    agent_editor_markdown, parse_agent_editor_max_spawn_depth, valid_local_agent_name,
};
use crate::tui::app_state::TuiApp;
use crate::tui::ui_types::{
    AgentEditorField, AgentEditorMode, AgentEditorPanel, AgentTab, BottomPanel, FullscreenUi,
};

impl TuiApp {
    pub(crate) async fn start_available_agent_run(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        agent_name: String,
        prompt: String,
    ) -> Result<()> {
        let parent_thread_id = self.current_session.clone();
        self.detach_foreground_for_session_switch(ui, None).await;
        let mut request = StartAgentTaskRequest::new(&self.cwd, agent_name, prompt);
        request.parent_thread_id = parent_thread_id;
        request.model = self.current_model.clone();
        request.reasoning_effort = self.current_variant.clone();
        request.mode = self.current_mode;
        request.inherited_env = Some(self.env_map.clone());
        request.selected_parent_agent = self.current_agent.clone();
        request.no_skills = self.no_skills;
        request.skill_inputs = self.skill_inputs.clone();

        let receipt = self.runtime.client().start_agent_task(request).await?;
        self.current_session = Some(receipt.thread_id);
        self.reset_live_agent_reload_poll();
        self.refresh_current_session_title().await?;
        self.clear_new_session_draft();
        ui.clear_session_local_bottom_panel();
        ui.clear_transcript();
        self.load_current_session_history(ui).await?;
        ui.push_status(format!("agent started: {}", receipt.agent.id));
        ui.refresh_sidebar(self);
        Ok(())
    }

    pub(crate) fn agent_definition_detail_text(
        &self,
        name: &str,
        path: Option<&PathBuf>,
        shadowed: bool,
    ) -> Result<String> {
        let Some(catalog) = self.current_agent_catalog() else {
            return Ok("Agents disabled.".to_string());
        };
        let agents = if shadowed {
            catalog.shadowed_agents
        } else {
            catalog.agents
        };
        let agent = agents.into_iter().find(|agent| {
            agent.name == name
                && path
                    .map(|path| agent.file_path.as_ref() == Some(path))
                    .unwrap_or(true)
        });
        let Some(agent) = agent else {
            return Ok(format!("agent not found: {name}"));
        };
        Ok(serde_json::to_string_pretty(
            &psychevo::agents::view_agent_value(&agent),
        )?)
    }

    pub(crate) fn agent_editor_for_path(&self, path: &PathBuf) -> Result<Option<AgentEditorPanel>> {
        let Some(catalog) = self.current_agent_catalog() else {
            return Ok(None);
        };
        let agent = catalog
            .agents
            .into_iter()
            .chain(catalog.shadowed_agents)
            .find(|agent| agent.file_path.as_ref() == Some(path));
        let Some(agent) = agent else {
            return Ok(None);
        };
        let tools = agent
            .tool_policy
            .allowed
            .as_ref()
            .map(|tools| tools.iter().cloned().collect::<Vec<_>>().join(", "))
            .unwrap_or_default();
        let permission_mode = agent
            .tool_policy
            .permission_mode
            .map(|mode| match mode {
                psychevo::agents::AgentPermissionMode::Default => "default",
                psychevo::agents::AgentPermissionMode::AcceptEdits => "acceptEdits",
                psychevo::agents::AgentPermissionMode::Plan => "plan",
            })
            .unwrap_or_default()
            .to_string();
        Ok(Some(AgentEditorPanel {
            mode: AgentEditorMode::Update { path: path.clone() },
            name: agent.name,
            description: agent.description,
            instructions: agent.instructions,
            model: agent.model.unwrap_or_default(),
            tools,
            permission_mode,
            background: agent.background.unwrap_or(false),
            max_spawn_depth: agent.max_spawn_depth.to_string(),
            active_field: AgentEditorField::Name,
            notice: None,
        }))
    }

    pub(crate) async fn save_agent_editor(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        let Some(BottomPanel::AgentEditor(panel)) = ui.bottom_panel.as_ref() else {
            return Ok(());
        };
        let panel = panel.clone();
        let name = panel.name.trim();
        if !valid_local_agent_name(name) {
            ui.set_bottom_panel_notice("name must use lowercase letters, digits, and hyphens");
            return Ok(());
        }
        if panel.description.trim().is_empty() {
            ui.set_bottom_panel_notice("description is required");
            return Ok(());
        }
        if parse_agent_editor_max_spawn_depth(&panel).is_none() {
            ui.set_bottom_panel_notice(format!(
                "max spawn depth must be 0..{}",
                MAX_AGENT_SPAWN_DEPTH_CAP
            ));
            return Ok(());
        }
        let path = match &panel.mode {
            AgentEditorMode::Create => self
                .cwd
                .join(".psychevo")
                .join("agents")
                .join(format!("{}.md", name)),
            AgentEditorMode::Update { path } => path.clone(),
        };
        if matches!(panel.mode, AgentEditorMode::Create) && path.exists() {
            ui.set_bottom_panel_notice("agent file already exists");
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, agent_editor_markdown(&panel))?;
        let mut agent_panel = self.agent_panel().await;
        agent_panel.tab = AgentTab::Available;
        agent_panel.available.notice = Some(format!("saved {}", path.display()));
        ui.bottom_panel = Some(BottomPanel::Agents(agent_panel));
        Ok(())
    }
}
