use super::formatting::format_exec_prefix_for_status;
use crate::tui::app_commands::resolve_tui_turn_admission_target;
use crate::tui::{
    AgentMissionRegistration, ConfigScope, ContextSnapshot, FullscreenUi, HelpPanel,
    MAX_TEAM_PARALLEL_AGENTS_CAP, PathBuf, RefreshThreadContextResult, SessionArtifactKind,
    SessionExportFormat, SessionExportOptions, SessionExportWriteResult, TuiApp, TurnOutcome,
    TurnPrinter, UsageQuery, Value, assistant_text_from_message, default_session_export_filename,
    format_nanodollars, format_slash_help_with_config, json_i64, slash_help_sections_with_config,
};
use anyhow::{Result, anyhow};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

impl TuiApp {
    pub(crate) fn permissions_status_text(&self) -> Result<String> {
        let value = self.configuration()?.permission_rules(ConfigScope::Local)?;
        let permissions = &value["permissions"];
        let mut lines = vec![
            format!("mode: {}", self.current_mode.as_str()),
            format!("permission_mode: {}", self.current_permission_mode.as_str()),
            format!(
                "approval_policy: {}",
                permissions["approval_policy"]
                    .as_str()
                    .unwrap_or("on-request")
            ),
            format!(
                "approvals_reviewer: {}",
                permissions["approvals_reviewer"].as_str().unwrap_or("user")
            ),
            format!(
                "default_permissions: {}",
                permissions["default_permissions"]
                    .as_str()
                    .unwrap_or(":workspace")
            ),
            format!(
                "path: {}",
                value["path"].as_str().unwrap_or(".psychevo/config.toml")
            ),
        ];
        lines.push("profiles:".to_string());
        let profiles = permissions["profiles"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        if profiles.is_empty() {
            lines.push("  (none)".to_string());
        } else {
            for name in profiles.keys() {
                lines.push(format!("  {name}"));
            }
        }
        lines.push("exec_policy:".to_string());
        let rules = permissions["exec_policy"]["rules"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if rules.is_empty() {
            lines.push("  (none)".to_string());
        } else {
            for rule in rules {
                let prefix = rule["prefix"]
                    .as_array()
                    .map(|values| format_exec_prefix_for_status(values))
                    .unwrap_or_else(|| "-".to_string());
                lines.push(format!(
                    "  {} -> {}",
                    prefix,
                    rule["decision"].as_str().unwrap_or("-")
                ));
            }
        }
        Ok(lines.join("\n"))
    }

    pub(crate) fn sandbox_status_text(&self) -> Result<String> {
        Ok(self
            .configuration()?
            .sandbox_status_text(self.current_mode)?)
    }

    pub(crate) async fn agents_status_text(&self) -> String {
        let Some(catalog) = self.current_agent_catalog() else {
            return "Agents disabled.".to_string();
        };
        let mut sections = Vec::new();
        if catalog.agents.is_empty() {
            sections.push("Library\nNo agents found.".to_string());
        } else {
            sections.push(format!(
                "Library\n{}",
                catalog
                    .agents
                    .iter()
                    .map(|agent| format!("{}: {}", agent.name, agent.description))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if let Some(parent) = self.current_session.as_deref() {
            let control = self.runtime.application().agent_control();
            let agents = control.status_records(Some(parent), false).await;
            if agents.is_empty() {
                sections.push("Running/Completed\nNo child agents for this session.".to_string());
            } else {
                let spawning = if control.spawning_paused(parent) {
                    "spawning paused"
                } else {
                    "spawning active"
                };
                sections.push(format!(
                    "Running/Completed ({spawning}, cap {MAX_TEAM_PARALLEL_AGENTS_CAP})\n{}",
                    agents
                        .iter()
                        .map(|agent| format!(
                            "{}\t{}\t{}\tteam:{}\tmission:{}\tmember:{}",
                            agent.id,
                            agent.agent_name,
                            agent.status.as_str(),
                            agent.team_name.as_deref().unwrap_or("-"),
                            agent.mission_run_id.as_deref().unwrap_or("-"),
                            agent.team_member_id.as_deref().unwrap_or("-")
                        ))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
        }
        sections.join("\n\n")
    }

    pub(crate) fn help_status_text(&self) -> String {
        format_slash_help_with_config(self.current_skill_count(), &self.slash_config)
    }

    pub(crate) fn help_panel(&self) -> HelpPanel {
        HelpPanel::new(slash_help_sections_with_config(
            self.current_skill_count(),
            &self.slash_config,
        ))
    }

    pub(crate) fn current_skill_count(&self) -> Option<usize> {
        self.current_skill_catalog()
            .map(|catalog| catalog.skills.len())
    }

    pub(crate) async fn stats_status_text(&self) -> Result<String> {
        let mut query = UsageQuery::new(&self.cwd);
        query.limit = 5;
        let report = self.runtime.client().usage(query).await?;
        let totals = report.get("totals").unwrap_or(&Value::Null);
        Ok(format!(
            "sessions: {}  messages: {}  tokens: {}  cost: {}",
            json_i64(totals, "sessions"),
            json_i64(totals, "messages"),
            json_i64(totals, "reported_total_tokens"),
            format_nanodollars(json_i64(totals, "estimated_cost_nanodollars"))
        ))
    }

    pub(crate) async fn write_tui_export(
        &self,
        options: &crate::tui::slash::TuiExportOptions,
    ) -> Result<SessionExportWriteResult> {
        let session_id = self
            .current_session
            .as_deref()
            .ok_or_else(|| anyhow!("no session context yet"))?;
        let output = self.resolve_tui_export_path(
            options.path.as_deref(),
            options.format,
            SessionArtifactKind::Export,
            session_id,
        );
        Ok(self
            .runtime
            .client()
            .resume_thread(session_id.to_string())
            .await?
            .write_export(
                &output,
                SessionExportOptions {
                    format: options.format,
                    include: options.include.clone(),
                    artifact_kind: SessionArtifactKind::Export,
                },
            )
            .await?)
    }

    pub(crate) async fn write_tui_share(
        &self,
        options: &crate::tui::slash::TuiShareOptions,
    ) -> Result<SessionExportWriteResult> {
        let session_id = self
            .current_session
            .as_deref()
            .ok_or_else(|| anyhow!("no session context yet"))?;
        let output = self.resolve_tui_export_path(
            options.path.as_deref(),
            SessionExportFormat::Markdown,
            SessionArtifactKind::Share,
            session_id,
        );
        Ok(self
            .runtime
            .client()
            .resume_thread(session_id.to_string())
            .await?
            .write_export(
                &output,
                SessionExportOptions {
                    format: SessionExportFormat::Markdown,
                    include: options.include.clone(),
                    artifact_kind: SessionArtifactKind::Share,
                },
            )
            .await?)
    }

    pub(crate) fn resolve_tui_export_path(
        &self,
        path: Option<&str>,
        format: SessionExportFormat,
        artifact_kind: SessionArtifactKind,
        session_id: &str,
    ) -> PathBuf {
        let path = path.map(PathBuf::from).unwrap_or_else(|| {
            self.cwd.join(default_session_export_filename(
                session_id,
                format,
                artifact_kind,
            ))
        });
        if path.is_absolute() {
            path
        } else {
            self.cwd.join(path)
        }
    }

    pub(crate) async fn context_status_snapshot(
        &self,
        live: Option<&ContextSnapshot>,
    ) -> Result<ContextSnapshot> {
        if let Some(snapshot) = live {
            return Ok(snapshot.clone());
        }
        if let Some(snapshot) = self.last_context_snapshot.as_ref() {
            return Ok(snapshot.clone());
        }
        let session = self
            .current_session
            .clone()
            .ok_or_else(|| anyhow!("no session context yet"))?;
        Ok(self
            .runtime
            .client()
            .resume_thread(session)
            .await?
            .context_snapshot(Some(self.env_map.clone()))
            .await?)
    }

    pub(crate) async fn reload_context_for_current_session(
        &self,
        ui: &FullscreenUi<'_>,
    ) -> Result<RefreshThreadContextResult> {
        if ui.foreground_turn_active() {
            return Err(anyhow!("finish the current turn before reloading context"));
        }
        let session = self
            .current_session
            .clone()
            .ok_or_else(|| anyhow!("no session context yet"))?;
        Ok(self
            .runtime
            .client()
            .resume_thread(session)
            .await?
            .refresh_context(self.refresh_thread_context_request("manual_reload", None))
            .await?)
    }

    pub(crate) async fn submit_prompt(&mut self, prompt: String) -> Result<()> {
        self.submit_prompt_with_mission(prompt, None).await
    }

    pub(crate) async fn submit_prompt_with_mission(
        &mut self,
        prompt: String,
        mission: Option<AgentMissionRegistration>,
    ) -> Result<()> {
        let stdout = Arc::new(Mutex::new(io::stdout()));
        let turn = Arc::new(Mutex::new(TurnPrinter::new(
            self.renderer,
            self.thinking_visible,
            self.debug,
        )));
        {
            let mut stdout = stdout.lock().expect("stdout lock poisoned");
            writeln!(stdout, "Prompt: {prompt}")?;
        }
        let request = self
            .framework_turn_request_with_images(prompt, Vec::new())
            .with_framework_context(Some(self.home.join("snapshots")), None, Vec::new(), None);
        let request = if let Some(mission) = mission {
            request.with_admission_mission(mission)
        } else {
            request
        };
        let target = resolve_tui_turn_admission_target(
            self.runtime.client(),
            self.current_session.as_deref(),
            self.force_new_once,
            &self.cwd,
        )
        .await?;
        let handle = target.start(self.runtime.client(), request).await?;
        let mut events = handle.events();
        let render = async {
            while let Some(event) = events.next().await {
                let mut turn = turn.lock().expect("turn lock poisoned");
                let mut stdout = stdout.lock().expect("stdout lock poisoned");
                let _ = turn.render_event(&event, &mut *stdout);
            }
        };
        let (result, ()) = tokio::join!(handle.wait(), render);
        let result = result?;
        self.last_context_snapshot = result.context_snapshot.clone();
        let needs_authoritative_reload = turn
            .lock()
            .expect("turn lock poisoned")
            .needs_authoritative_reload();
        let authoritative_answer = if needs_authoritative_reload {
            let history = self
                .runtime
                .client()
                .resume_thread(result.thread_id.clone())
                .await?
                .history()
                .latest(Some(200))
                .await?;
            history
                .items
                .iter()
                .rev()
                .find_map(|item| {
                    serde_json::to_value(&item.message)
                        .ok()
                        .and_then(|message| assistant_text_from_message(&message))
                })
                .unwrap_or_else(|| result.final_answer.clone())
        } else {
            String::new()
        };
        {
            let mut turn = turn.lock().expect("turn lock poisoned");
            let mut stdout = stdout.lock().expect("stdout lock poisoned");
            if needs_authoritative_reload {
                turn.finish_after_authoritative_reload(&authoritative_answer, &mut *stdout)?;
            } else {
                turn.finish(&mut *stdout)?;
            }
        }
        self.current_session = Some(result.thread_id);
        self.reset_live_agent_reload_poll();
        self.refresh_current_session_title().await?;
        self.clear_new_session_draft();
        let success = result.outcome == TurnOutcome::Completed && result.tool_failures == 0;
        if !success {
            self.had_error = true;
        }
        Ok(())
    }
}
