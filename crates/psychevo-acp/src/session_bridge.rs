#[allow(unused_imports)]
pub(crate) use super::*;
impl PsychevoAcpAgent {
    pub(crate) async fn apply_slash_effect(
        &self,
        session_id: &SessionId,
        session: &AcpSession,
        effect: psychevo_runtime::command_registry::SlashCommandEffect,
        action: Option<psychevo_runtime::command_registry::SlashCommandAction>,
        cx: &ConnectionTo<Client>,
    ) -> Result<SlashPromptAction, Error> {
        use psychevo_runtime::command_registry::{SlashCommandAction, SlashCommandEffect};

        match effect {
            SlashCommandEffect::LocalText => {
                let text = match action {
                    Some(SlashCommandAction::Help) => self.help_command_text(session),
                    Some(SlashCommandAction::Status) => {
                        self.status_command_text(session_id, session)
                    }
                    Some(SlashCommandAction::Usage) => self.usage_command_text(session)?,
                    Some(SlashCommandAction::Context) => self.context_command_text(session)?,
                    Some(SlashCommandAction::Refresh) => self.refresh_command_text(session)?,
                    _ => "Command completed.".to_string(),
                };
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::Diff => {
                let diff = collect_workspace_diff(&session.cwd).map_err(acp_internal_error)?;
                Ok(send_diff_tool_call(cx, session_id, &diff))
            }
            SlashCommandEffect::PassThroughPrompt(prompt)
            | SlashCommandEffect::SubmitPrompt(prompt) => Ok(SlashPromptAction::RunPrompt(prompt)),
            SlashCommandEffect::Steer(message) => self.apply_steer_effect(session_id, &message, cx),
            SlashCommandEffect::Queue(message) => {
                self.queue_prompt(session_id, message.clone())?;
                Ok(send_slash_text(
                    cx,
                    session_id,
                    format!("queued prompt: {message}"),
                ))
            }
            SlashCommandEffect::PendingCancel => {
                let total = self.cancel_pending_inputs(session_id);
                let text = if total == 0 {
                    "no pending input".to_string()
                } else {
                    format!("pending input canceled: {total}")
                };
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::NewSession => {
                let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
                let Some(session) = sessions.get_mut(&session_id.to_string()) else {
                    return Err(Error::resource_not_found(Some(session_id.to_string())));
                };
                session.runtime_session_id = None;
                session.queued_prompts.clear();
                session.pending_steers.clear();
                session.last_session_list.clear();
                Ok(send_slash_text(
                    cx,
                    session_id,
                    "New runtime session will be used for the next prompt.",
                ))
            }
            SlashCommandEffect::SessionsList => {
                let text = self.sessions_list_text(session_id, session)?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::ResumeSession { reference } => {
                let text = self.resume_session_text(session_id, session, reference.as_deref())?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::ShowModel => {
                let text = self.model_command_text(session)?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::SetModel { model, variant } => {
                let text = self.set_model_text(session_id, &model, variant.as_deref())?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::SetVariant(variant) => {
                let text = self.set_variant_text(session_id, &variant)?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::SetMode(mode) => {
                let text = self.set_mode_text(session_id, &mode, cx)?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::PermissionsShow => Ok(send_slash_text(
                cx,
                session_id,
                self.permissions_status_text(session)?,
            )),
            SlashCommandEffect::PermissionAdd { kind, rule } => {
                if !self
                    .request_command_approval(
                        session_id,
                        cx,
                        "/permissions",
                        "change local permission policy",
                    )
                    .await
                {
                    return Ok(send_slash_text(cx, session_id, "permission denied"));
                }
                let result =
                    append_local_permission_rule(self.local_config_dir(session)?, &kind, &rule)
                        .map_err(acp_internal_error)?;
                Ok(send_slash_text(
                    cx,
                    session_id,
                    format!(
                        "{} {} rule: {}",
                        if result.changed {
                            "added"
                        } else {
                            "already had"
                        },
                        result.kind,
                        result.rule
                    ),
                ))
            }
            SlashCommandEffect::PermissionRemove { kind, rule } => {
                if !self
                    .request_command_approval(
                        session_id,
                        cx,
                        "/permissions",
                        "change local permission policy",
                    )
                    .await
                {
                    return Ok(send_slash_text(cx, session_id, "permission denied"));
                }
                let result =
                    remove_local_permission_rule(self.local_config_dir(session)?, &kind, &rule)
                        .map_err(acp_internal_error)?;
                Ok(send_slash_text(
                    cx,
                    session_id,
                    format!(
                        "{} {} rule: {}",
                        if result.changed {
                            "removed"
                        } else {
                            "no matching"
                        },
                        result.kind,
                        result.rule
                    ),
                ))
            }
            SlashCommandEffect::ToolsShow => Ok(send_slash_text(
                cx,
                session_id,
                self.toolsets_status_text(session)
                    .map_err(acp_internal_error)?,
            )),
            SlashCommandEffect::ToolsetSet { name, enabled } => {
                if !self
                    .request_command_approval(
                        session_id,
                        cx,
                        "/tools",
                        "change local toolset configuration",
                    )
                    .await
                {
                    return Ok(send_slash_text(cx, session_id, "permission denied"));
                }
                let result = set_local_toolset_enabled(
                    self.local_config_dir(session)?,
                    session.mode,
                    &name,
                    enabled,
                )
                .map_err(acp_internal_error)?;
                Ok(send_slash_text(
                    cx,
                    session_id,
                    format!(
                        "{} toolset `{}` for {} mode",
                        if enabled { "enabled" } else { "disabled" },
                        result.name,
                        session.mode.as_str()
                    ),
                ))
            }
            SlashCommandEffect::Rename(title) => {
                let Some(runtime_session_id) = session.runtime_session_id.as_deref() else {
                    return Ok(send_slash_text(cx, session_id, "no runtime session yet"));
                };
                let title = self
                    .state
                    .store()
                    .set_session_title(runtime_session_id, &title)
                    .map_err(acp_internal_error)?;
                Ok(send_slash_text(
                    cx,
                    session_id,
                    format!("session renamed: {title}"),
                ))
            }
            SlashCommandEffect::Undo => {
                let result =
                    undo_session(self.undo_options(session)?).map_err(acp_internal_error)?;
                Ok(send_slash_text(
                    cx,
                    session_id,
                    format!(
                        "undone {} messages; prompt restored",
                        result.reverted_messages
                    ),
                ))
            }
            SlashCommandEffect::Redo => {
                let result =
                    redo_session(self.undo_options(session)?).map_err(acp_internal_error)?;
                Ok(send_slash_text(
                    cx,
                    session_id,
                    format!(
                        "redone {} messages ({})",
                        result.restored_messages,
                        if result.complete {
                            "complete"
                        } else {
                            "partial"
                        }
                    ),
                ))
            }
            SlashCommandEffect::Skills { args } => {
                let text = self
                    .skills_command_text(session_id, session, args.as_deref(), cx)
                    .await?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::Bundles { args } => {
                let text = self.bundles_command_text(session, args.as_deref())?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::Curator { args } => Ok(send_slash_text(
                cx,
                session_id,
                self.curator_command_text(args.as_deref()),
            )),
            SlashCommandEffect::Agents => Ok(send_slash_text(
                cx,
                session_id,
                self.agents_status_text(session)
                    .map_err(acp_internal_error)?,
            )),
            SlashCommandEffect::Fork(prompt) => Ok(SlashPromptAction::RunPrompt(prompt)),
            SlashCommandEffect::Compact { instructions } => {
                let text = self.compact_command_text(session, instructions).await?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::Export { args } => {
                if !self
                    .request_command_approval(
                        session_id,
                        cx,
                        "/export",
                        "write a local session export artifact",
                    )
                    .await
                {
                    return Ok(send_slash_text(cx, session_id, "permission denied"));
                }
                let text = self.write_artifact_text(
                    session,
                    SessionArtifactKind::Export,
                    args.as_deref(),
                )?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::Share { args } => {
                if !self
                    .request_command_approval(
                        session_id,
                        cx,
                        "/share",
                        "write a local share artifact",
                    )
                    .await
                {
                    return Ok(send_slash_text(cx, session_id, "permission denied"));
                }
                let text =
                    self.write_artifact_text(session, SessionArtifactKind::Share, args.as_deref())?;
                Ok(send_slash_text(cx, session_id, text))
            }
            SlashCommandEffect::SandboxShow => Ok(send_slash_text(
                cx,
                session_id,
                "/sandbox is not available in ACP yet.",
            )),
            SlashCommandEffect::Unsupported(text) => Ok(send_slash_text(cx, session_id, text)),
        }
    }

    pub(crate) fn status_command_text(
        &self,
        session_id: &SessionId,
        session: &AcpSession,
    ) -> String {
        let model = session.model.as_deref().unwrap_or("(configured default)");
        let runtime_session = session.runtime_session_id.as_deref().unwrap_or("(new)");
        format!(
            "ACP session: {session_id}\nruntime session: {runtime_session}\nworkdir: {}\nmode: {}\nmodel: {model}\ncommands: {}",
            session.cwd.display(),
            session.mode.as_str(),
            self.available_commands_for_session_state(session, session.control.is_some())
                .commands
                .len()
        )
    }

    pub(crate) fn toolsets_status_text(
        &self,
        session: &AcpSession,
    ) -> Result<String, psychevo_runtime::Error> {
        let options = self.run_options(session, String::new(), Vec::new(), None);
        let value = toolsets_value(&options, ConfigScope::Effective)?;
        let mode_key = session.mode.as_str();
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

    pub(crate) fn agents_status_text(
        &self,
        session: &AcpSession,
    ) -> Result<String, psychevo_runtime::Error> {
        let catalog = discover_agents(&AgentDiscoveryOptions {
            home: self.options.home.clone(),
            workdir: session.cwd.clone(),
            env: self.options.inherited_env.clone(),
            explicit_inputs: Vec::new(),
            no_agents: false,
        })?;
        let value = list_agents_value(&catalog);
        let Some(agents) = value["agents"].as_array() else {
            return Ok("No agents discovered.".to_string());
        };
        if agents.is_empty() {
            return Ok("No agents discovered.".to_string());
        }
        let mut lines = Vec::from(["Available agents:".to_string()]);
        for agent in agents {
            lines.push(format!(
                "- {} ({}) {}",
                agent["name"].as_str().unwrap_or("-"),
                agent["source"].as_str().unwrap_or("-"),
                agent["description"].as_str().unwrap_or("")
            ));
        }
        Ok(lines.join("\n"))
    }

    pub(crate) fn help_command_text(&self, session: &AcpSession) -> String {
        let available =
            self.available_commands_for_session_state(session, session.control.is_some());
        let hidden_dynamic = available.hidden_dynamic;
        let mut lines = vec!["Available commands:".to_string()];
        lines.extend(available_command_lines_from(available_commands_from(
            available,
        )));
        if hidden_dynamic > 0 {
            lines.push(format!(
                "{} dynamic skill or bundle commands hidden; type /skills or /bundles to list them.",
                hidden_dynamic
            ));
        }
        lines.join("\n")
    }

    pub(crate) fn usage_command_text(&self, session: &AcpSession) -> Result<String, Error> {
        let value = usage_stats(psychevo_runtime::StatsOptions {
            state: self.state.clone(),
            workdir: session.cwd.clone(),
            all: false,
            days: None,
            limit: 20,
        })
        .map_err(acp_internal_error)?;
        serde_json::to_string_pretty(&value).map_err(acp_internal_error)
    }

    pub(crate) fn context_command_text(&self, session: &AcpSession) -> Result<String, Error> {
        let Some(runtime_session_id) = session.runtime_session_id.clone() else {
            return Ok("no runtime session yet".to_string());
        };
        let snapshot = context_snapshot(ContextOptions {
            state: self.state.clone(),
            workdir: session.cwd.clone(),
            session: runtime_session_id,
            config_path: self.options.config_path.clone(),
            inherited_env: Some(self.options.inherited_env.clone()),
        })
        .map_err(acp_internal_error)?;
        Ok(format_context_snapshot_text_with_options(
            &snapshot,
            ContextFormatOptions {
                heading: true,
                bar_width: None,
            },
        ))
    }

    pub(crate) fn refresh_command_text(&self, session: &AcpSession) -> Result<String, Error> {
        let Some(runtime_session_id) = session.runtime_session_id.clone() else {
            return Ok("no runtime session yet".to_string());
        };
        let result =
            psychevo_runtime::reload_session_context(psychevo_runtime::ReloadContextOptions {
                state: self.state.clone(),
                session: runtime_session_id,
                config_path: self.options.config_path.clone(),
                mode: Some(session.mode),
                inherited_env: Some(self.options.inherited_env.clone()),
                agent: None,
                no_agents: false,
                no_skills: false,
                invalidation_reason: "manual_reload".to_string(),
                notice: None,
            })
            .map_err(acp_internal_error)?;
        Ok(format!(
            "reloaded context: {} v{}",
            result.prefix_hash, result.version
        ))
    }

    pub(crate) fn sessions_list_text(
        &self,
        session_id: &SessionId,
        session: &AcpSession,
    ) -> Result<String, Error> {
        let summaries = self.session_summaries_for(session)?;
        let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
        if let Some(current) = sessions.get_mut(&session_id.to_string()) {
            current.last_session_list = summaries.clone();
        }
        drop(sessions);
        if summaries.is_empty() {
            return Ok("No sessions found.".to_string());
        }
        let mut lines = vec!["Sessions:".to_string()];
        for (idx, summary) in summaries.iter().enumerate() {
            lines.push(format!(
                "{}. {}  {}  updated:{}  messages:{}",
                idx + 1,
                summary.title.as_deref().unwrap_or("(untitled)"),
                summary.id,
                summary.updated_at_ms,
                summary.message_count
            ));
        }
        lines.push("Use /resume <number|latest|id-prefix|title>.".to_string());
        Ok(lines.join("\n"))
    }

    pub(crate) fn resume_session_text(
        &self,
        session_id: &SessionId,
        session: &AcpSession,
        reference: Option<&str>,
    ) -> Result<String, Error> {
        let reference = reference.unwrap_or("latest").trim();
        let summaries = if session.last_session_list.is_empty() {
            self.session_summaries_for(session)?
        } else {
            session.last_session_list.clone()
        };
        let Some(target) = resolve_session_reference(reference, &summaries) else {
            let ambiguous = ambiguous_session_matches(reference, &summaries);
            if !ambiguous.is_empty() {
                let mut lines = vec![format!("Ambiguous session reference `{reference}`:")];
                for (idx, summary) in ambiguous.iter().enumerate() {
                    lines.push(format!(
                        "{}. {}  {}  updated:{}",
                        idx + 1,
                        summary.title.as_deref().unwrap_or("(untitled)"),
                        summary.id,
                        summary.updated_at_ms
                    ));
                }
                return Ok(lines.join("\n"));
            }
            return Ok(format!("No session matched `{reference}`."));
        };
        let store = self.state.store().clone();
        store
            .resume_session(&target.id)
            .map_err(|_| Error::resource_not_found(Some(target.id.clone())))?;
        let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
        let Some(current) = sessions.get_mut(&session_id.to_string()) else {
            return Err(Error::resource_not_found(Some(session_id.to_string())));
        };
        current.runtime_session_id = Some(target.id.clone());
        current.queued_prompts.clear();
        current.pending_steers.clear();
        Ok(format!(
            "resumed session: {} {}",
            target.id,
            target.title.unwrap_or_default()
        ))
    }

    pub(crate) fn session_summaries_for(
        &self,
        session: &AcpSession,
    ) -> Result<Vec<SessionSummary>, Error> {
        let store = self.state.store().clone();
        store
            .list_sessions_for_workdir_with_sources(&session.cwd, &[])
            .map_err(acp_internal_error)
    }

    pub(crate) fn model_command_text(&self, session: &AcpSession) -> Result<String, Error> {
        let options = self.run_options(session, String::new(), Vec::new(), None);
        let configured = configured_models(&options).map_err(acp_internal_error)?;
        let mut lines = vec![
            format!(
                "model: {}",
                session.model.as_deref().unwrap_or("(configured default)")
            ),
            format!(
                "variant: {}",
                session
                    .reasoning_effort
                    .as_deref()
                    .unwrap_or("(configured default)")
            ),
        ];
        if configured.is_empty() {
            lines.push("No locally configured models.".to_string());
        } else {
            lines.push("Configured models:".to_string());
            for model in configured {
                let id = if model.provider.is_empty() {
                    model.model
                } else {
                    format!("{}/{}", model.provider, model.model)
                };
                lines.push(format!("- {id} ({})", model.provider_label));
            }
        }
        Ok(lines.join("\n"))
    }

    pub(crate) fn set_model_text(
        &self,
        session_id: &SessionId,
        model: &str,
        variant: Option<&str>,
    ) -> Result<String, Error> {
        let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
        let Some(session) = sessions.get_mut(&session_id.to_string()) else {
            return Err(Error::resource_not_found(Some(session_id.to_string())));
        };
        session.model = Some(model.to_string());
        if let Some(variant) = variant {
            session.reasoning_effort = reasoning_effort_value(variant);
        }
        Ok(format!(
            "model: {model}{}",
            variant
                .map(|value| format!("\nvariant: {value}"))
                .unwrap_or_default()
        ))
    }

    pub(crate) fn set_variant_text(
        &self,
        session_id: &SessionId,
        variant: &str,
    ) -> Result<String, Error> {
        let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
        let Some(session) = sessions.get_mut(&session_id.to_string()) else {
            return Err(Error::resource_not_found(Some(session_id.to_string())));
        };
        session.reasoning_effort = reasoning_effort_value(variant);
        Ok(format!("variant: {variant}"))
    }

    pub(crate) fn set_mode_text(
        &self,
        session_id: &SessionId,
        value: &str,
        cx: &ConnectionTo<Client>,
    ) -> Result<String, Error> {
        let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
        let Some(session) = sessions.get_mut(&session_id.to_string()) else {
            return Err(Error::resource_not_found(Some(session_id.to_string())));
        };
        if let Some(mode) = RunMode::parse(value) {
            session.mode = mode;
            let updated_session = session.clone();
            drop(sessions);
            send_session_update(
                cx,
                session_id.clone(),
                SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(
                    self.session_config_options_for_session(&updated_session),
                )),
            );
            return Ok(format!("mode: {}", mode.as_str()));
        }
        Err(Error::invalid_params().data(format!("unsupported mode: {value}; use plan or default")))
    }

    pub(crate) fn permissions_status_text(&self, session: &AcpSession) -> Result<String, Error> {
        let options = self.run_options(session, String::new(), Vec::new(), None);
        let value =
            permission_rules_value(&options, ConfigScope::Local).map_err(acp_internal_error)?;
        let permissions = &value["permissions"];
        let mut lines = vec![
            format!("mode: {}", session.mode.as_str()),
            format!(
                "permission_mode: {}",
                session
                    .permission_mode
                    .map(PermissionMode::as_str)
                    .unwrap_or("default")
            ),
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
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
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

    pub(crate) async fn compact_command_text(
        &self,
        session: &AcpSession,
        instructions: Option<String>,
    ) -> Result<String, Error> {
        let Some(runtime_session_id) = session.runtime_session_id.clone() else {
            return Ok("no runtime session yet".to_string());
        };
        let result = compact_session(CompactSessionOptions {
            state: self.state.clone(),
            workdir: session.cwd.clone(),
            session: runtime_session_id,
            config_path: self.options.config_path.clone(),
            model: session.model.clone(),
            reasoning_effort: session.reasoning_effort.clone(),
            inherited_env: Some(self.options.inherited_env.clone()),
            reason: CompactionReason::Manual,
            instructions,
            force: true,
        })
        .await
        .map_err(acp_internal_error)?;
        Ok(format!(
            "{}\ncompacted: {}",
            result.message, result.compacted
        ))
    }

    pub(crate) fn undo_options(&self, session: &AcpSession) -> Result<SessionUndoOptions, Error> {
        let Some(runtime_session_id) = session.runtime_session_id.clone() else {
            return Err(Error::invalid_params().data("no runtime session yet"));
        };
        Ok(SessionUndoOptions {
            state: self.state.clone(),
            workdir: session.cwd.clone(),
            snapshot_root: self.options.home.join("snapshots"),
            session_id: runtime_session_id,
        })
    }

    pub(crate) fn local_config_dir(&self, session: &AcpSession) -> Result<PathBuf, Error> {
        if self.options.config_path.is_some() {
            return Err(Error::invalid_params()
                .data("cannot change project-local config while PSYCHEVO_CONFIG is active"));
        }
        canonicalize_workdir(&session.cwd)
            .map(|path| path.join(".psychevo"))
            .map_err(acp_internal_error)
    }

    pub(crate) fn available_commands_for_session(
        &self,
        session_id: &SessionId,
    ) -> Vec<AvailableCommand> {
        let session = self
            .sessions
            .lock()
            .expect("acp session lock poisoned")
            .get(&session_id.to_string())
            .cloned();
        let Some(session) = session else {
            return available_commands_from(
                psychevo_runtime::command_registry::AvailableSlashCommands {
                    commands: Vec::new(),
                    hidden_dynamic: 0,
                },
            );
        };
        let active_turn = session.control.is_some();
        available_commands_from(self.available_commands_for_session_state(&session, active_turn))
    }

    pub(crate) fn available_commands_for_session_state(
        &self,
        session: &AcpSession,
        active_turn: bool,
    ) -> psychevo_runtime::command_registry::AvailableSlashCommands {
        psychevo_runtime::command_registry::available_slash_commands_for_surface(
            acp_command_capabilities(),
            active_turn,
            &self.dynamic_slash_commands(session),
            ACP_COMMAND_ADVERTISEMENT_LIMIT,
        )
    }

    pub(crate) fn dynamic_slash_commands(
        &self,
        session: &AcpSession,
    ) -> Vec<psychevo_runtime::command_registry::DynamicSlashCommand> {
        let mut commands = Vec::new();
        if let Ok(bundles) = list_skill_bundles(&self.options.home, &session.cwd) {
            for bundle in bundles {
                commands.push(psychevo_runtime::command_registry::DynamicSlashCommand {
                    name: bundle.slug.clone(),
                    summary: bundle.description,
                    prompt: psychevo_runtime::command_registry::skill_prompt_marker(
                        &bundle.slug,
                        "",
                    ),
                });
            }
        }
        if let Ok(catalog) = discover_skills(&SkillDiscoveryOptions {
            home: self.options.home.clone(),
            workdir: session.cwd.clone(),
            config_path: self.options.config_path.clone(),
            env: self.options.inherited_env.clone(),
            explicit_inputs: Vec::new(),
            no_skills: false,
        }) {
            for skill in catalog.skills {
                commands.push(psychevo_runtime::command_registry::DynamicSlashCommand {
                    name: skill.name.clone(),
                    summary: skill.description,
                    prompt: psychevo_runtime::command_registry::skill_prompt_marker(
                        &skill.name,
                        "",
                    ),
                });
            }
        }
        commands.sort_by(|left, right| left.name.cmp(&right.name));
        commands
    }

    pub(crate) fn queue_prompt(&self, session_id: &SessionId, prompt: String) -> Result<(), Error> {
        let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
        let Some(session) = sessions.get_mut(&session_id.to_string()) else {
            return Err(Error::resource_not_found(Some(session_id.to_string())));
        };
        session.queued_prompts.push_back(prompt);
        Ok(())
    }

    pub(crate) fn pop_queued_prompt(&self, session_id: &SessionId) -> Option<String> {
        self.sessions
            .lock()
            .expect("acp session lock poisoned")
            .get_mut(&session_id.to_string())
            .and_then(|session| session.queued_prompts.pop_front())
    }

    pub(crate) fn cancel_pending_inputs(&self, session_id: &SessionId) -> usize {
        let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
        let Some(session) = sessions.get_mut(&session_id.to_string()) else {
            return 0;
        };
        let control = session.control.clone();
        let mut canceled = 0usize;
        for id in session.pending_steers.drain(..) {
            if control
                .as_ref()
                .is_some_and(|control| control.cancel_pending_user_message(id))
            {
                canceled += 1;
            }
        }
        let queued = session.queued_prompts.len();
        session.queued_prompts.clear();
        canceled + queued
    }

    pub(crate) fn apply_steer_effect(
        &self,
        session_id: &SessionId,
        message: &str,
        cx: &ConnectionTo<Client>,
    ) -> Result<SlashPromptAction, Error> {
        let prompt = message.trim().to_string();
        let mut sessions = self.sessions.lock().expect("acp session lock poisoned");
        let Some(session) = sessions.get_mut(&session_id.to_string()) else {
            return Err(Error::resource_not_found(Some(session_id.to_string())));
        };
        let Some(control) = session.control.clone() else {
            return Ok(SlashPromptAction::RunPrompt(prompt));
        };
        let Some(id) = control.steer_user_message(user_text_message(&prompt)) else {
            session.queued_prompts.push_back(prompt.clone());
            return Ok(send_slash_text(
                cx,
                session_id,
                format!("turn is not ready for steering; queued prompt: {prompt}"),
            ));
        };
        session.pending_steers.push(id);
        Ok(send_slash_text(
            cx,
            session_id,
            format!("steer queued: {prompt}"),
        ))
    }
}
