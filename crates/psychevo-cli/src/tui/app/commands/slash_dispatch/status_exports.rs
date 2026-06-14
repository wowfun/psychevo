impl TuiApp {
    pub(crate) fn permissions_status_text(&self) -> Result<String> {
        let options = self.run_options(String::new());
        let value = permission_rules_value(&options, ConfigScope::Local)?;
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
        let options = self.run_options(String::new());
        Ok(psychevo_runtime::sandbox_status_text(
            &options,
            self.current_mode,
        )?)
    }

    pub(crate) fn agents_status_text(&self) -> String {
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
            let store = self.state_runtime.store();
            let value = agent_status_value(Some(store), Some(parent), false);
            let running = value
                .get("agents")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if running.is_empty() {
                sections.push("Running/Completed\nNo child agents for this session.".to_string());
            } else {
                sections.push(format!(
                    "Running/Completed\n{}",
                    running
                        .iter()
                        .map(|agent| format!(
                            "{}\t{}\t{}",
                            agent.get("id").and_then(Value::as_str).unwrap_or_default(),
                            agent
                                .get("agent_name")
                                .and_then(Value::as_str)
                                .unwrap_or_default(),
                            agent
                                .get("status")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
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

    pub(crate) fn stats_status_text(&self) -> Result<String> {
        let report = usage_stats(StatsOptions {
            state: self.state_runtime.clone(),
            workdir: self.workdir.clone(),
            all: false,
            days: None,
            limit: 5,
        })?;
        let totals = report.get("totals").unwrap_or(&Value::Null);
        Ok(format!(
            "sessions: {}  messages: {}  tokens: {}  cost: {}",
            json_i64(totals, "sessions"),
            json_i64(totals, "messages"),
            json_i64(totals, "reported_total_tokens"),
            format_nanodollars(json_i64(totals, "estimated_cost_nanodollars"))
        ))
    }

    pub(crate) fn write_tui_export(
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
        let store = self.state_runtime.store();
        Ok(write_session_export(
            store,
            session_id,
            &output,
            SessionExportOptions {
                format: options.format,
                include: options.include.clone(),
                artifact_kind: SessionArtifactKind::Export,
            },
        )?)
    }

    pub(crate) fn write_tui_share(
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
        let store = self.state_runtime.store();
        Ok(write_session_export(
            store,
            session_id,
            &output,
            SessionExportOptions {
                format: SessionExportFormat::Markdown,
                include: options.include.clone(),
                artifact_kind: SessionArtifactKind::Share,
            },
        )?)
    }

    pub(crate) fn resolve_tui_export_path(
        &self,
        path: Option<&str>,
        format: SessionExportFormat,
        artifact_kind: SessionArtifactKind,
        session_id: &str,
    ) -> PathBuf {
        let path = path.map(PathBuf::from).unwrap_or_else(|| {
            self.workdir.join(default_session_export_filename(
                session_id,
                format,
                artifact_kind,
            ))
        });
        if path.is_absolute() {
            path
        } else {
            self.workdir.join(path)
        }
    }

    pub(crate) fn context_status_snapshot(
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
        Ok(context_snapshot(ContextOptions {
            state: self.state_runtime.clone(),
            workdir: self.workdir.clone(),
            session,
            config_path: self.config_path.clone(),
            inherited_env: Some(self.env_map.clone()),
        })?)
    }

    pub(crate) fn reload_context_for_current_session(
        &self,
        ui: &FullscreenUi<'_>,
    ) -> Result<psychevo_runtime::ReloadContextResult> {
        if ui.running.is_some() {
            return Err(anyhow!("finish the current turn before reloading context"));
        }
        let session = self
            .current_session
            .clone()
            .ok_or_else(|| anyhow!("no session context yet"))?;
        Ok(reload_session_context(ReloadContextOptions {
            state: self.state_runtime.clone(),
            session,
            config_path: self.config_path.clone(),
            mode: Some(self.current_mode),
            inherited_env: Some(self.env_map.clone()),
            agent: self.current_agent.clone(),
            no_agents: self.no_agents,
            no_skills: self.no_skills,
            invalidation_reason: "manual_reload".to_string(),
            notice: None,
        })?)
    }

    pub(crate) async fn submit_prompt(&mut self, prompt: String) -> Result<()> {
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
        let turn_for_sink = Arc::clone(&turn);
        let stdout_for_sink = Arc::clone(&stdout);
        let sink: GatewayEventSink = Arc::new(move |event| {
            let mut turn = turn_for_sink.lock().expect("turn lock poisoned");
            let mut stdout = stdout_for_sink.lock().expect("stdout lock poisoned");
            let _ = turn.render_gateway_event(&event, &mut *stdout);
        });
        let options = self.run_options(prompt);
        let source = self.gateway_source();
        let bind_source = self.canonical_gateway_source();
        let reset_source_binding = self.force_new_once && self.current_session.is_none();
        let result = self
            .gateway
            .send_turn(SendTurnRequest {
                thread_id: options.session.clone(),
                source: Some(source),
                bind_source: Some(bind_source),
                reset_source_binding,
                input: Vec::new(),
                options,
                runtime_source: Some("tui".to_string()),
                continue_sources: TUI_CONTINUE_SESSION_SOURCES
                    .iter()
                    .map(|source| (*source).to_string())
                    .collect(),
                stream: None,
                event_sink: Some(sink),
                control_handle: None,
                control: None,
                lineage: None,
            })
            .await?
            .result;
        self.last_context_snapshot = result.context_snapshot.clone();
        {
            let mut turn = turn.lock().expect("turn lock poisoned");
            let mut stdout = stdout.lock().expect("stdout lock poisoned");
            turn.finish(&mut *stdout)?;
        }
        self.current_session = Some(result.session_id);
        self.reset_live_agent_reload_poll();
        self.refresh_current_session_title()?;
        self.clear_new_session_draft();
        let success = result.outcome == Outcome::Normal && result.tool_failures == 0;
        if !success {
            self.had_error = true;
        }
        Ok(())
    }
}
