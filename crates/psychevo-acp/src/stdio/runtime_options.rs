use std::path::PathBuf;
use std::sync::Arc;

use agent_client_protocol::schema::v2::{
    ClientCapabilities, Cost, SessionId, SessionUpdate, UsageUpdate,
};
use agent_client_protocol::{Client, ConnectionTo, Error};
use psychevo::application::{Configuration, ConfigurationQuery};
use psychevo::command_registry::{
    SlashCommandParse, SlashCommandSurface, dynamic_slash_command_effect, parse_slash_command_line,
    slash_invocation_effect,
};
use psychevo::{ApprovalHandler, ImageInput, TurnRequest};
use serde_json::Value;

use crate::commands::{SlashPromptAction, acp_command_capabilities, send_session_update};
use crate::protocol::env_flag_enabled;
use crate::stdio::{AcpSession, PsychevoAcpAgent};

use super::options_and_tests::AcpUsageUpdateContext;

impl PsychevoAcpAgent {
    pub(crate) fn turn_request(
        &self,
        session: &AcpSession,
        prompt: String,
        image_inputs: Vec<ImageInput>,
        approval_handler: Option<Arc<dyn ApprovalHandler>>,
    ) -> TurnRequest {
        TurnRequest::new(prompt)
            .with_prompt_images(image_inputs, true)
            .with_identity("acp", None)
            .with_model(session.model.clone(), session.reasoning_effort.clone())
            .with_reasoning_output(true)
            .with_execution_policy(
                session.mode,
                session.permission_mode,
                self.options.config_path.clone(),
            )
            .with_approval(approval_handler, false)
            .with_environment(Some(self.options.inherited_env.clone()), None, None)
            .with_mcp_servers(session.mcp_servers.clone())
            .with_framework_context(
                Some(self.options.home.join("snapshots")),
                None,
                Vec::new(),
                None,
            )
    }

    pub(crate) fn configuration_for_session(
        &self,
        session: &AcpSession,
    ) -> psychevo::Result<Configuration> {
        self.configuration(
            session.cwd.clone(),
            session.model.clone(),
            session.reasoning_effort.clone(),
        )
    }

    pub(crate) fn configuration(
        &self,
        cwd: PathBuf,
        model: Option<String>,
        reasoning_effort: Option<String>,
    ) -> psychevo::Result<Configuration> {
        let mut query = ConfigurationQuery::new(cwd);
        query.model = model;
        query.reasoning_effort = reasoning_effort;
        query.inherited_env = Some(self.options.inherited_env.clone());
        self.framework.configuration(query)
    }

    pub(crate) fn ready_auth_provider(&self) -> Option<String> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let configuration = self.configuration(cwd, None, None).ok()?;
        let selected = configuration.selected_model().ok().flatten()?;
        configuration
            .model_catalog_providers()
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

    pub(super) fn client_terminal_output_enabled(&self, capabilities: &ClientCapabilities) -> bool {
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

    pub(super) fn send_usage_update_from_context(
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
        let dynamic = self.dynamic_slash_commands(session);
        let effect_and_action = match parse_slash_command_line(prompt) {
            SlashCommandParse::NotSlash => return Ok(SlashPromptAction::NotSlashOrPassThrough),
            SlashCommandParse::Unknown {
                command,
                args,
                original: _,
            } => {
                if let Some(effect) = dynamic_slash_command_effect(&command, &args, &dynamic) {
                    (effect, None)
                } else {
                    return Ok(SlashPromptAction::NotSlashOrPassThrough);
                }
            }
            SlashCommandParse::Known(invocation) => {
                let active_turn = session.active_turn();
                let effect = slash_invocation_effect(
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
