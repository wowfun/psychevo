impl PsychevoAcpAgent {
    pub(crate) fn refresh_command_text(&self, session: &AcpSession) -> Result<String, Error> {
        let Some(runtime_session_id) = session.runtime_session_id.clone() else {
            return Ok("no runtime session yet".to_string());
        };
        let result =
            psychevo_runtime::run::reload_session_context(psychevo_runtime::types::ReloadContextOptions {
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
        let store = self.state.clone();
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
        let store = self.state.clone();
        store
            .list_sessions_for_cwd_with_sources(&session.cwd, &[])
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
            cwd: session.cwd.clone(),
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
            cwd: session.cwd.clone(),
            snapshot_root: self.options.home.join("snapshots"),
            session_id: runtime_session_id,
        })
    }

    pub(crate) fn local_config_dir(&self, session: &AcpSession) -> Result<PathBuf, Error> {
        if self.options.config_path.is_some() {
            return Err(Error::invalid_params()
                .data("cannot change project-local config while PSYCHEVO_CONFIG is active"));
        }
        canonicalize_cwd(&session.cwd)
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
            cwd: session.cwd.clone(),
            config_path: self.options.config_path.clone(),
            env: self.options.inherited_env.clone(),
            explicit_inputs: Vec::new(),
            additional_roots: Vec::new(),
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
