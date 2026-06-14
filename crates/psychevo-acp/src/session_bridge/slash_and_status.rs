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
}
