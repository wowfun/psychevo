impl PsychevoAcpAgent {

    pub(crate) fn run_options(
        &self,
        session: &AcpSession,
        prompt: String,
        image_inputs: Vec<ImageInput>,
        approval_handler: Option<Arc<dyn ApprovalHandler>>,
    ) -> RunOptions {
        RunOptions {
            state: self.state.clone(),
            cwd: session.cwd.clone(),
            snapshot_root: Some(self.options.home.join("snapshots")),
            session: session.runtime_session_id.clone(),
            continue_latest: false,
            prompt,
            image_inputs,
            extract_prompt_image_sources: false,
            prompt_display: None,
            max_context_messages: None,
            config_path: self.options.config_path.clone(),
            project_context_override: None,
            model: session.model.clone(),
            reasoning_effort: session.reasoning_effort.clone(),
            runtime_ref: None,
            runtime_session_id: None,
            runtime_options: std::collections::BTreeMap::new(),
            external_agent_delegate: None,
            include_reasoning: true,
            mode: session.mode,
            permission_mode: session.permission_mode,
            sandbox_override: None,
            approval_mode: Some(ApprovalMode::Manual),
            approval_handler,
            clarify_enabled: false,
            inherited_env: Some(self.options.inherited_env.clone()),
            agent: None,
            no_agents: false,
            no_skills: false,
            selected_capability_roots: Vec::new(),
            skill_inputs: Vec::new(),
            mcp_servers: session.mcp_servers.clone(),
            runtime_tools: Vec::new(),
        }
    }

    pub(crate) fn probe_run_options(&self, cwd: PathBuf, model: Option<String>) -> RunOptions {
        RunOptions {
            state: self.state.clone(),
            cwd,
            snapshot_root: None,
            session: None,
            continue_latest: false,
            prompt: String::new(),
            image_inputs: Vec::new(),
            extract_prompt_image_sources: false,
            prompt_display: None,
            max_context_messages: None,
            config_path: self.options.config_path.clone(),
            project_context_override: None,
            model,
            reasoning_effort: None,
            runtime_ref: None,
            runtime_session_id: None,
            runtime_options: std::collections::BTreeMap::new(),
            external_agent_delegate: None,
            include_reasoning: false,
            mode: RunMode::Default,
            permission_mode: None,
            sandbox_override: None,
            approval_mode: None,
            approval_handler: None,
            clarify_enabled: false,
            inherited_env: Some(self.options.inherited_env.clone()),
            agent: None,
            no_agents: false,
            no_skills: false,
            selected_capability_roots: Vec::new(),
            skill_inputs: Vec::new(),
            mcp_servers: Vec::new(),
            runtime_tools: Vec::new(),
        }
    }

    pub(crate) fn ready_auth_provider(&self) -> Option<String> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let options = self.probe_run_options(cwd, None);
        let selected = selected_configured_model(&options).ok().flatten()?;
        model_catalog_providers(&options)
            .ok()?
            .into_iter()
            .find(|provider| provider.provider == selected.provider && provider.fetchable())
            .map(|provider| provider.provider)
    }

    pub(crate) fn terminal_auth_available(&self) -> bool {
        self.client_terminal_auth
            .lock()
            .map(|value| *value)
            .unwrap_or(false)
    }

    pub(crate) fn terminal_output_available(&self) -> bool {
        self.client_terminal_output
            .lock()
            .map(|value| *value)
            .unwrap_or(false)
    }

    fn client_terminal_output_enabled(&self, capabilities: &ClientCapabilities) -> bool {
        self.options
            .inherited_env
            .get("PSYCHEVO_ACP_TERMINAL_OUTPUT")
            .is_some_and(|value| env_flag_enabled(value))
            && capabilities.meta.as_ref().is_some_and(|meta| {
                meta.get("terminal_output")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            })
    }

    fn send_usage_update_from_context(
        &self,
        cx: &ConnectionTo<Client>,
        session_id: SessionId,
        context: AcpUsageUpdateContext<'_>,
    ) {
        let (used, size, source, provider, model) = if let Some(snapshot) = context.snapshot
            && let Some(size) = snapshot.context_limit
        {
            (
                snapshot.total.estimated_tokens,
                size,
                "runtime_context_snapshot",
                snapshot.provider.as_str(),
                snapshot.model.as_str(),
            )
        } else {
            let Some(size) = context.context_limit else {
                return;
            };
            let Some(used) = context
                .usage
                .lock()
                .ok()
                .and_then(|usage| usage.context_tokens_for_usage_update())
            else {
                return;
            };
            (
                used,
                size,
                "runtime_usage_accounting",
                context.provider,
                context.model,
            )
        };
        let mut update = UsageUpdate::new(used, size);
        if let Ok(usage) = context.usage.lock()
            && let Some(cost) = usage.cumulative_cost_usd()
        {
            update = update.cost(Cost::new(cost, "USD"));
        }
        let mut psychevo = serde_json::Map::new();
        psychevo.insert("source".to_string(), Value::String(source.to_string()));
        psychevo.insert("provider".to_string(), Value::String(provider.to_string()));
        psychevo.insert("model".to_string(), Value::String(model.to_string()));
        let mut meta = serde_json::Map::new();
        meta.insert("psychevo".to_string(), Value::Object(psychevo));
        update = update.meta(meta);
        send_session_update(cx, session_id, SessionUpdate::UsageUpdate(update));
    }

    pub(crate) async fn handle_slash_prompt(
        &self,
        session_id: &SessionId,
        session: &AcpSession,
        prompt: &str,
        cx: &ConnectionTo<Client>,
    ) -> Result<SlashPromptAction, Error> {
        use psychevo_runtime::command_registry::{SlashCommandParse, SlashCommandSurface};

        let dynamic = self.dynamic_slash_commands(session);
        let effect_and_action =
            match psychevo_runtime::command_registry::parse_slash_command_line(prompt) {
                SlashCommandParse::NotSlash => return Ok(SlashPromptAction::NotSlashOrPassThrough),
                SlashCommandParse::Unknown {
                    command,
                    args,
                    original: _,
                } => {
                    if let Some(effect) =
                        psychevo_runtime::command_registry::dynamic_slash_command_effect(
                            &command, &args, &dynamic,
                        )
                    {
                        (effect, None)
                    } else {
                        return Ok(SlashPromptAction::NotSlashOrPassThrough);
                    }
                }
                SlashCommandParse::Known(invocation) => {
                    let active_turn = session.control.is_some();
                    let effect = psychevo_runtime::command_registry::slash_invocation_effect(
                        &invocation,
                        acp_command_capabilities(),
                        SlashCommandSurface::Acp,
                        active_turn,
                    )
                    .map_err(|message| Error::invalid_params().data(message))?;
                    (effect, Some(invocation.spec.action))
                }
            };

        self.apply_slash_effect(
            session_id,
            session,
            effect_and_action.0,
            effect_and_action.1,
            cx,
        )
        .await
    }
}
