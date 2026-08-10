use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use futures::future::BoxFuture;
use serde_json::Value;

use super::{
    AgentBindingSnapshot, AgentCapabilitySelection, AgentChildTurnDispatcher,
    AgentChildTurnTemplate, AgentEnvironmentOverlay, AgentExecutionPolicy,
    AgentFilesystemAuthorizer, AgentInputPart, AgentModelSelection, AgentSessionAdapter,
    AgentTargetSelection, AgentThreadForkRequest, AgentThreadImportRequest,
    AgentThreadLifecycleRequest, AgentTurnInput, AgentTurnInvocation, AgentTurnPersistence,
    AgentTurnPreparation, AgentUnknownDelivery, Client, FrameworkAgentTurnPersistence,
    NativeAgentSessionAdapter, PreparedAgentTurn, ResolvedCapabilityPlan, ResolvedTurnPlan,
    ThreadExecutionContext, TurnEvent, TurnEventSender, TurnOutcome, TurnResult,
};
use crate::run::run_live_streaming_controlled_with_capture;
use crate::state::GatewayRuntimeBindingRecord;
use crate::types::{RunOptions, RunStreamSink};
use crate::{Error, Result};

pub(super) const AGENT_SESSION_METADATA_KEY: &str = "peer_agent";

#[derive(Clone)]
pub(super) struct AgentMcpServerResolver {
    resolution: crate::extensions::McpServerResolution,
}

impl fmt::Debug for AgentMcpServerResolver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AgentMcpServerResolver(..)")
    }
}

impl AgentMcpServerResolver {
    fn for_thread(client: &Client, thread: &ThreadExecutionContext) -> Self {
        Self {
            resolution: crate::extensions::McpServerResolution::new(
                client.inner.home.clone(),
                Arc::clone(&client.inner.mcp_oauth_credentials),
                PathBuf::from(&thread.cwd),
                client.inner.config_path.clone(),
                client.application_environment(None),
                Vec::new(),
                Vec::new(),
            ),
        }
    }

    pub(super) fn for_turn(
        thread: &ThreadExecutionContext,
        profile_home: PathBuf,
        mcp_oauth_credentials: Arc<dyn crate::config::McpOAuthCredentialStore>,
        config_path: Option<PathBuf>,
        inherited_env: BTreeMap<String, String>,
        selected_capability_roots: Vec<crate::extensions::SelectedCapabilityRoot>,
        mcp_servers: Vec<crate::types::McpServerInput>,
    ) -> Self {
        Self {
            resolution: crate::extensions::McpServerResolution::new(
                profile_home,
                mcp_oauth_credentials,
                PathBuf::from(&thread.cwd),
                config_path,
                inherited_env,
                selected_capability_roots,
                mcp_servers,
            ),
        }
    }

    async fn resolve(
        &self,
        names: &BTreeSet<String>,
    ) -> Result<Vec<crate::types::ResolvedMcpServerInput>> {
        if names.is_empty() {
            return Ok(Vec::new());
        }
        crate::extensions::resolve_mcp_server_handoffs(&self.resolution, names).await
    }
}

impl Client {
    pub(super) fn agent_mcp_server_resolver(
        &self,
        thread: &ThreadExecutionContext,
    ) -> AgentMcpServerResolver {
        AgentMcpServerResolver::for_thread(self, thread)
    }
}

impl AgentThreadLifecycleRequest {
    pub async fn resolve_mcp_server_handoffs(
        &self,
        names: &BTreeSet<String>,
    ) -> Result<Vec<crate::types::ResolvedMcpServerInput>> {
        self.mcp_resolver.resolve(names).await
    }
}

impl AgentThreadImportRequest {
    pub async fn resolve_mcp_server_handoffs(
        &self,
        names: &BTreeSet<String>,
    ) -> Result<Vec<crate::types::ResolvedMcpServerInput>> {
        self.mcp_resolver.resolve(names).await
    }
}

impl AgentThreadForkRequest {
    pub async fn resolve_mcp_server_handoffs(
        &self,
        names: &BTreeSet<String>,
    ) -> Result<Vec<crate::types::ResolvedMcpServerInput>> {
        self.mcp_resolver.resolve(names).await
    }
}

impl fmt::Debug for AgentTurnInvocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentTurnInvocation")
            .field("thread", &self.thread)
            .field("receipt", &self.receipt)
            .field("binding", &self.binding)
            .field("target", &self.target)
            .field("input", &self.input)
            .field("model", &self.model)
            .field("environment", &self.environment)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for AgentFilesystemAuthorizer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AgentFilesystemAuthorizer(..)")
    }
}

pub(super) struct AgentFilesystemCapture<'a> {
    pub(super) thread: &'a ThreadExecutionContext,
    pub(super) execution: &'a AgentExecutionPolicy,
    pub(super) environment: &'a AgentEnvironmentOverlay,
    pub(super) binding: Option<&'a AgentBindingSnapshot>,
    pub(super) initial_binding: Option<&'a super::InitialAgentBinding>,
    pub(super) model: &'a AgentModelSelection,
    pub(super) state: &'a crate::state::StateRuntime,
    pub(super) turn_grant_owner: Option<&'a str>,
    pub(super) abort: psychevo_ai::AbortSignal,
    pub(super) workspace_root_capture: &'a crate::filesystem_identity::WorkspaceRootCapture,
}

struct AgentFilesystemRuntimeInput<'a> {
    thread: &'a ThreadExecutionContext,
    execution: &'a AgentExecutionPolicy,
    environment: &'a AgentEnvironmentOverlay,
    selected_agent: Option<&'a crate::agents::AgentDefinition>,
    model: &'a AgentModelSelection,
    state: &'a crate::state::StateRuntime,
    sandbox_grants: crate::sandbox::SandboxWriteGrants,
    workspace_root_capture: &'a crate::filesystem_identity::WorkspaceRootCapture,
}

impl AgentFilesystemAuthorizer {
    pub(super) fn unavailable_for_native(abort: psychevo_ai::AbortSignal) -> Self {
        Self {
            runtime: Err("filesystem callbacks are unavailable for the Native runtime".to_string()),
            abort,
        }
    }

    pub(super) fn capture_for_turn(capture: AgentFilesystemCapture<'_>) -> Result<Self> {
        let AgentFilesystemCapture {
            thread,
            execution,
            environment,
            binding,
            initial_binding,
            model,
            state,
            turn_grant_owner,
            abort,
            workspace_root_capture,
        } = capture;
        let selected_agent = selected_agent_definition(
            binding
                .and_then(|binding| binding.agent_ref.as_deref())
                .or_else(|| initial_binding.and_then(|binding| binding.agent_ref.as_deref())),
            binding
                .map(|binding| binding.agent_definition_json.as_str())
                .or_else(|| initial_binding.map(|binding| binding.agent_definition_json.as_str())),
        )?;
        let grants = match turn_grant_owner {
            Some(owner) if owner != thread.id => {
                state.filesystem_grants_with_turn_scopes(&thread.id, owner)
            }
            _ => state.filesystem_grants(&thread.id),
        };
        Ok(Self {
            runtime: Ok(Self::build_runtime(AgentFilesystemRuntimeInput {
                thread,
                execution,
                environment,
                selected_agent: selected_agent.as_ref(),
                model,
                state,
                sandbox_grants: grants,
                workspace_root_capture,
            })?),
            abort,
        })
    }

    #[cfg(test)]
    fn capture_runtime(
        thread: &ThreadExecutionContext,
        execution: &AgentExecutionPolicy,
        environment: &AgentEnvironmentOverlay,
        binding: Option<&AgentBindingSnapshot>,
        model: &AgentModelSelection,
        state: &crate::state::StateRuntime,
    ) -> Result<crate::permissions::PermissionRuntime> {
        let selected_agent = selected_agent_definition(
            binding.and_then(|binding| binding.agent_ref.as_deref()),
            binding.map(|binding| binding.agent_definition_json.as_str()),
        )?;
        let workspace_root_capture = crate::filesystem_identity::WorkspaceRootCapture::capture(
            &thread.roots.iter().map(PathBuf::from).collect::<Vec<_>>(),
        )?;
        Self::build_runtime(AgentFilesystemRuntimeInput {
            thread,
            execution,
            environment,
            selected_agent: selected_agent.as_ref(),
            model,
            state,
            sandbox_grants: state.filesystem_grants(&thread.id),
            workspace_root_capture: &workspace_root_capture,
        })
    }

    fn build_runtime(
        input: AgentFilesystemRuntimeInput<'_>,
    ) -> Result<crate::permissions::PermissionRuntime> {
        let AgentFilesystemRuntimeInput {
            thread,
            execution,
            environment,
            selected_agent,
            model,
            state,
            sandbox_grants,
            workspace_root_capture,
        } = input;
        let cwd = crate::paths::canonical_cwd(&PathBuf::from(&thread.cwd))?;
        let workspace_roots = thread.roots.iter().map(PathBuf::from).collect::<Vec<_>>();
        let permission_mode = crate::agents::narrow_permission_mode_for_agent(
            execution.permission_mode.unwrap_or_default(),
            selected_agent,
        );
        let effective_mode = crate::agents::effective_run_mode(execution.mode, selected_agent);
        let options = RunOptions {
            state: state.clone(),
            cwd: cwd.clone(),
            workspace_roots: workspace_roots.clone(),
            snapshot_root: execution.snapshot_root.clone(),
            session: Some(thread.id.clone()),
            continue_latest: false,
            prompt: String::new(),
            image_inputs: Vec::new(),
            extract_prompt_image_sources: false,
            prompt_display: None,
            max_context_messages: execution.max_context_messages,
            config_path: execution.config_path.clone(),
            project_context_override: execution.project_context,
            sandbox_override: execution.sandbox.clone(),
            model: model
                .model
                .clone()
                .or_else(|| selected_agent.and_then(|agent| agent.model.clone())),
            reasoning_effort: model
                .reasoning_effort
                .clone()
                .or_else(|| selected_agent.and_then(|agent| agent.effort.clone())),
            runtime_ref: None,
            runtime_session_id: None,
            runtime_options: BTreeMap::new(),
            include_reasoning: model.include_reasoning,
            mode: effective_mode,
            permission_mode: Some(permission_mode),
            approval_handler: execution.approval_handler.clone(),
            clarify_enabled: execution.clarify_enabled,
            inherited_env: Some(environment.inherited_env.clone()),
            agent: selected_agent.map(|agent| agent.name.clone()),
            external_agent_delegate: None,
            no_agents: true,
            no_skills: true,
            selected_capability_roots: Vec::new(),
            skill_inputs: Vec::new(),
            mcp_servers: Vec::new(),
            mcp_runtime: None,
            workspace_mutations: execution.workspace_mutations.clone(),
            runtime_tools: Vec::new(),
        };
        let loaded = crate::config::load_run_config(&options, &cwd)?;
        let sandbox_policy = crate::sandbox::SandboxPolicy::from_config(
            &loaded.config.sandbox,
            &cwd,
            effective_mode,
            &loaded.env,
        )?
        .with_workspace_root_capture(workspace_root_capture);
        let smart_approval_handler = if loaded.config.permissions.approvals_reviewer
            == crate::types::ApprovalsReviewer::Smart
        {
            let resolved = crate::config::resolve_run_provider(&options, &loaded)?;
            let provider = crate::run::generation_provider(
                resolved.base_url.clone(),
                resolved.api_key.clone(),
                resolved.provider.clone(),
                resolved.inference_idle_timeout_secs,
            )?;
            let reviewer_model = crate::run::smart_reviewer_model(
                None,
                &provider,
                &options,
                &loaded,
                &resolved,
                &loaded.config.permissions,
            );
            crate::run::smart_approval_handler(
                reviewer_model,
                &loaded.config.permissions,
                serde_json::json!({ "source": "acp_filesystem_callback" }),
            )
        } else {
            None
        };
        let mut runtime = crate::permissions::PermissionRuntime::new(
            cwd.clone(),
            cwd.join(".psychevo"),
            loaded.config.permissions.clone(),
            permission_mode,
            execution.approval_handler.clone(),
            smart_approval_handler,
        )
        .with_workspace_root_capture(workspace_root_capture)
        .with_protected_config_paths(loaded.sources.clone())
        .with_sandbox(sandbox_policy, sandbox_grants);
        let hook_config =
            crate::hooks::hook_runtime_config_with_plugin_sources_from_options(&options, &cwd)?;
        if let Some(hook_runtime) =
            crate::agents::build_hook_runtime(selected_agent, Vec::new(), hook_config, &cwd)
        {
            runtime = runtime.with_hook_runtime(hook_runtime);
        }
        Ok(runtime)
    }

    pub async fn authorize_read(
        &self,
        tool_call_id: &str,
        path: &std::path::Path,
    ) -> std::result::Result<super::AgentAuthorizedFile, String> {
        self.authorize_file(tool_call_id, "read", path, false, false)
            .await
    }

    pub async fn authorize_write(
        &self,
        tool_call_id: &str,
        path: &std::path::Path,
    ) -> std::result::Result<super::AgentAuthorizedFile, String> {
        self.authorize_file(tool_call_id, "write", path, true, false)
            .await
    }

    pub async fn authorize_workspace_read(
        &self,
        tool_call_id: &str,
        path: &std::path::Path,
    ) -> std::result::Result<super::AgentAuthorizedFile, String> {
        self.authorize_file(tool_call_id, "read", path, false, true)
            .await
    }

    pub async fn authorize_workspace_write(
        &self,
        tool_call_id: &str,
        path: &std::path::Path,
    ) -> std::result::Result<super::AgentAuthorizedFile, String> {
        self.authorize_file(tool_call_id, "write", path, true, true)
            .await
    }

    async fn authorize_file(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        path: &std::path::Path,
        writable: bool,
        require_workspace_containment: bool,
    ) -> std::result::Result<super::AgentAuthorizedFile, String> {
        let runtime = self
            .runtime
            .as_ref()
            .map_err(|error| format!("filesystem authorization unavailable: {error}"))?;
        runtime
            .authorize_filesystem_callback_target(
                tool_call_id,
                tool_name,
                path,
                writable,
                self.abort.clone(),
                require_workspace_containment,
            )
            .await
            .map(|target| super::AgentAuthorizedFile { target, writable })
            .map_err(|output| {
                output
                    .json
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("permission denied")
                    .to_string()
            })
    }

    pub fn open_workspace_directory(
        &self,
        path: &std::path::Path,
    ) -> std::result::Result<std::fs::File, String> {
        if self.abort.aborted() {
            return Err("aborted".to_string());
        }
        let runtime = self
            .runtime
            .as_ref()
            .map_err(|error| format!("filesystem authorization unavailable: {error}"))?;
        runtime
            .open_captured_workspace_directory(path)
            .map_err(|error| error.to_string())
    }
}

fn selected_agent_definition(
    agent_ref: Option<&str>,
    agent_definition_json: Option<&str>,
) -> Result<Option<crate::agents::AgentDefinition>> {
    if agent_ref.is_none() {
        return Ok(None);
    }
    let definition = agent_definition_json.ok_or_else(|| {
        Error::Message("captured Agent binding is missing its definition".to_string())
    })?;
    serde_json::from_str(definition).map(Some).map_err(|error| {
        Error::Message(format!(
            "captured Agent definition is invalid for filesystem authorization: {error}"
        ))
    })
}

impl fmt::Debug for AgentExecutionPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentExecutionPolicy")
            .field("source", &self.source)
            .field("config_path", &self.config_path)
            .field("mode", &self.mode)
            .field("permission_mode", &self.permission_mode)
            .field("clarify_enabled", &self.clarify_enabled)
            .field("snapshot_root", &self.snapshot_root)
            .field("max_context_messages", &self.max_context_messages)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for AgentCapabilitySelection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentCapabilitySelection")
            .field("no_agents", &self.no_agents)
            .field("no_skills", &self.no_skills)
            .field("skill_inputs", &self.skill_inputs)
            .field("mcp_server_count", &self.mcp_servers.len())
            .field("tool_count", &self.tools.len())
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for FrameworkAgentTurnPersistence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FrameworkAgentTurnPersistence")
            .field("thread_id", &self.thread_id)
            .field("turn_id", &self.turn_id)
            .finish_non_exhaustive()
    }
}

impl AgentTurnPersistence for FrameworkAgentTurnPersistence {
    fn confirm_delivery(&self) -> BoxFuture<'static, Result<()>> {
        let state = self.state.clone();
        let turn_id = self.turn_id.clone();
        Box::pin(async move {
            state
                .confirm_gateway_turn_delivery(&turn_id)
                .await
                .map(|_| ())
        })
    }

    fn mark_delivery_unknown(&self) -> BoxFuture<'static, Result<()>> {
        let state = self.state.clone();
        let turn_id = self.turn_id.clone();
        Box::pin(async move {
            state
                .mark_gateway_turn_delivery_unknown(&turn_id)
                .await
                .map(|_| ())
        })
    }

    fn attach_native_session(
        &self,
        binding_revision: i64,
        native_session_id: String,
    ) -> BoxFuture<'static, Result<AgentBindingSnapshot>> {
        let state = self.state.clone();
        let thread_id = self.thread_id.clone();
        Box::pin(async move {
            state
                .attach_gateway_runtime_native_session(
                    &thread_id,
                    binding_revision,
                    &native_session_id,
                )
                .await?;
            state
                .gateway_runtime_binding(&thread_id)
                .await?
                .map(AgentBindingSnapshot::try_from)
                .transpose()?
                .ok_or_else(|| {
                    Error::Message(format!(
                        "Agent binding disappeared for Thread `{thread_id}`"
                    ))
                })
        })
    }

    fn clear_agent_usage_observation(&self) -> BoxFuture<'static, Result<()>> {
        let state = self.state.clone();
        let thread_id = self.thread_id.clone();
        Box::pin(async move {
            let Some(metadata) = state.session_metadata(&thread_id).await? else {
                return Ok(());
            };
            let Some(mut object) = metadata
                .get(AGENT_SESSION_METADATA_KEY)
                .and_then(Value::as_object)
                .cloned()
            else {
                return Ok(());
            };
            if object.remove("usageUpdate").is_none() {
                return Ok(());
            }
            let value = (!object.is_empty()).then_some(Value::Object(object));
            state
                .set_session_metadata_field(&thread_id, AGENT_SESSION_METADATA_KEY, value)
                .await
        })
    }

    fn has_prior_terminal(&self) -> BoxFuture<'static, Result<bool>> {
        let state = self.state.clone();
        let thread_id = self.thread_id.clone();
        Box::pin(async move {
            state
                .gateway_turn_terminal_exists_for_thread(&thread_id)
                .await
        })
    }

    fn append_message(
        &self,
        message: psychevo_agent_core::Message,
    ) -> BoxFuture<'static, Result<()>> {
        let state = self.state.clone();
        let thread_id = self.thread_id.clone();
        let boundary_session_seq = Arc::clone(&self.boundary_session_seq);
        Box::pin(async move {
            let seq = state
                .append_framework_message(&thread_id, &message, None, None)
                .await?;
            boundary_session_seq.store(seq, Ordering::Release);
            Ok(())
        })
    }

    fn append_message_with_metrics(
        &self,
        message: psychevo_agent_core::Message,
        usage: Option<Value>,
        metadata: Option<Value>,
    ) -> BoxFuture<'static, Result<()>> {
        let state = self.state.clone();
        let thread_id = self.thread_id.clone();
        let boundary_session_seq = Arc::clone(&self.boundary_session_seq);
        Box::pin(async move {
            let seq = state
                .append_framework_message(&thread_id, &message, usage, metadata)
                .await?;
            boundary_session_seq.store(seq, Ordering::Release);
            Ok(())
        })
    }

    fn set_metadata_field(
        &self,
        key: String,
        value: Option<Value>,
    ) -> BoxFuture<'static, Result<()>> {
        let state = self.state.clone();
        let thread_id = self.thread_id.clone();
        Box::pin(async move {
            state
                .set_session_metadata_field(&thread_id, &key, value)
                .await
        })
    }

    fn set_visible_title_if_empty(&self, title: String) -> BoxFuture<'static, Result<()>> {
        let state = self.state.clone();
        let thread_id = self.thread_id.clone();
        Box::pin(async move {
            let Some(summary) = state.session_summary(&thread_id).await? else {
                return Ok(());
            };
            if summary.parent_session_id.is_some()
                || !crate::run::visible_session_source_allows_auto_title(&summary.source)
            {
                return Ok(());
            }
            state
                .set_session_title_if_empty(&thread_id, &title)
                .await
                .map(|_| ())
        })
    }

    fn prior_unknown_delivery(&self) -> BoxFuture<'static, Result<Option<AgentUnknownDelivery>>> {
        let state = self.state.clone();
        let thread_id = self.thread_id.clone();
        let turn_id = self.turn_id.clone();
        Box::pin(async move {
            let unknown = state
                .unknown_gateway_turn_deliveries_for_thread(&thread_id, &turn_id)
                .await?;
            if unknown.len() > 1 {
                return Err(Error::Message(
                    "A Thread has multiple unresolved unknown deliveries".to_string(),
                ));
            }
            Ok(unknown
                .into_iter()
                .next()
                .map(|delivery| AgentUnknownDelivery {
                    turn_id: delivery.turn_id,
                }))
        })
    }

    fn reconcile_unknown_delivery(
        &self,
        turn_id: String,
        metadata: Value,
    ) -> BoxFuture<'static, Result<bool>> {
        let state = self.state.clone();
        let thread_id = self.thread_id.clone();
        Box::pin(async move {
            state
                .reconcile_unknown_gateway_turn_delivery(&turn_id, &thread_id, Some(&metadata))
                .await
        })
    }
}

impl TryFrom<GatewayRuntimeBindingRecord> for AgentBindingSnapshot {
    type Error = Error;

    fn try_from(binding: GatewayRuntimeBindingRecord) -> Result<Self> {
        let required = |field: Option<String>, name: &str| {
            field.ok_or_else(|| {
                Error::Message(format!(
                    "resolved Agent binding `{}` is missing {name}",
                    binding.thread_id
                ))
            })
        };
        Ok(Self {
            thread_id: binding.thread_id.clone(),
            agent_ref: binding.agent_ref,
            agent_fingerprint: required(binding.agent_fingerprint, "agent_fingerprint")?,
            agent_definition_json: required(
                binding.agent_definition_json,
                "agent_definition_json",
            )?,
            runtime_ref: required(binding.runtime_ref, "runtime_ref")?,
            backend_kind: required(binding.backend_kind, "backend_kind")?,
            native_kind: required(binding.native_kind, "native_kind")?,
            native_session_id: binding.native_session_id,
            cwd: binding.cwd,
            profile_fingerprint: required(binding.profile_fingerprint, "profile_fingerprint")?,
            profile_revision: required(binding.profile_revision, "profile_revision")?,
            profile_config_json: required(binding.profile_config_json, "profile_config_json")?,
            adapter_kind: required(binding.adapter_kind, "adapter_kind")?,
            adapter_revision: required(binding.adapter_revision, "adapter_revision")?,
            binding_revision: binding.binding_revision,
            control_revision: binding.control_revision,
        })
    }
}

impl fmt::Debug for NativeAgentSessionAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NativeAgentSessionAdapter")
    }
}

#[derive(Debug)]
struct PreparedNativeAgentTurn {
    backend: super::NativeTurnBackend,
}

impl PreparedAgentTurn for PreparedNativeAgentTurn {
    fn invoke(
        self: Box<Self>,
        invocation: AgentTurnInvocation,
    ) -> BoxFuture<'static, Result<TurnResult>> {
        Box::pin(self.backend.execute(invocation))
    }
}

impl AgentSessionAdapter for NativeAgentSessionAdapter {
    fn prepare_turn(
        self: Arc<Self>,
        request: AgentTurnPreparation,
    ) -> BoxFuture<'static, Result<Box<dyn PreparedAgentTurn>>> {
        Box::pin(async move {
            Ok(Box::new(PreparedNativeAgentTurn {
                backend: request.native_backend,
            }) as Box<dyn PreparedAgentTurn>)
        })
    }
}

impl AgentTurnInvocation {
    pub fn filesystem_authorizer(&self) -> AgentFilesystemAuthorizer {
        self.filesystem_authorizer.clone()
    }

    pub fn workspace_root_capture(&self) -> crate::filesystem_identity::WorkspaceRootCapture {
        self.workspace_root_capture.clone()
    }

    pub async fn resolve_mcp_server_handoffs(
        &self,
        names: &BTreeSet<String>,
    ) -> Result<Vec<crate::types::ResolvedMcpServerInput>> {
        if names.is_empty() {
            return Ok(Vec::new());
        }
        self.mcp_resolver.resolve(names).await
    }
}

impl AgentChildTurnTemplate {
    fn capture(
        input: &AgentTurnInput,
        model: &AgentModelSelection,
        execution: &AgentExecutionPolicy,
        capabilities: &AgentCapabilitySelection,
        environment: &AgentEnvironmentOverlay,
    ) -> Self {
        let mut execution = execution.clone();
        execution.approval_handler = None;
        Self {
            extract_prompt_image_sources: input.extract_prompt_image_sources,
            model: model.clone(),
            execution,
            capabilities: ResolvedCapabilityPlan {
                no_agents: capabilities.no_agents,
                no_skills: capabilities.no_skills,
                selected_capability_roots: capabilities.selected_capability_roots.clone(),
                skill_inputs: capabilities.skill_inputs.clone(),
                mcp_servers: capabilities.mcp_servers.clone(),
                tools: capabilities.tools.clone(),
            },
            environment: environment.clone(),
        }
    }

    fn resolve_child(
        self,
        request: crate::types::ExternalAgentDelegateRequest,
    ) -> ResolvedAgentChildTurn {
        let crate::types::ExternalAgentDelegateRequest {
            run_id,
            parent_session_id,
            child_session_id,
            agent_name,
            runtime_ref,
            backend_ref,
            prompt,
            model: selected_model,
            runtime_options,
            expected_runtime_profile_revision,
            abort,
            ..
        } = request;
        let parts = (!prompt.is_empty())
            .then(|| AgentInputPart::Text {
                text: prompt.clone(),
            })
            .into_iter()
            .collect();
        let mut execution = self.execution;
        execution.source = "agent".to_string();
        let mut model = self.model;
        model.model = selected_model;
        ResolvedAgentChildTurn {
            parent_thread_id: parent_session_id,
            child_thread_id: child_session_id,
            turn_id: run_id.clone(),
            abort,
            plan: ResolvedTurnPlan {
                client_turn_id: None,
                requested_turn_id: Some(run_id),
                initial_thread_preferences: BTreeMap::new(),
                admission_mission: None,
                target: AgentTargetSelection {
                    agent_ref: Some(agent_name),
                    runtime_profile_ref: Some(runtime_ref),
                    runtime_options,
                    preparation: None,
                    expected_profile_revision: expected_runtime_profile_revision,
                    expected_backend_ref: backend_ref,
                },
                input: AgentTurnInput {
                    prompt,
                    image_inputs: Vec::new(),
                    parts,
                    extract_prompt_image_sources: self.extract_prompt_image_sources,
                    prompt_display: None,
                },
                model,
                execution,
                capabilities: self.capabilities,
                environment: self.environment,
                admission_cancellation: None,
            },
        }
    }
}

impl super::NativeTurnBackend {
    /// Execute the captured invocation with Psychevo's in-process Native
    /// runtime. A prepared Adapter owns this backend handle; the invocation
    /// carries only the shared semantic contract.
    pub async fn execute(self, invocation: AgentTurnInvocation) -> Result<TurnResult> {
        let AgentTurnInvocation {
            thread,
            receipt,
            target,
            input,
            model,
            execution,
            capabilities,
            environment,
            persistence,
            events,
            control,
            child_turns,
            workspace_root_capture,
            ..
        } = invocation;
        let runtime_control = control.take_runtime_control()?;
        let child_template = (!capabilities.no_agents).then(|| {
            AgentChildTurnTemplate::capture(&input, &model, &execution, &capabilities, &environment)
        });
        let source = execution.source.clone();
        let stream_events = events.clone();
        let stream: RunStreamSink = Arc::new(move |event| {
            stream_events.emit_agent_event(event);
        });
        let mut options = RunOptions {
            state: self.state,
            cwd: PathBuf::from(&thread.cwd),
            workspace_roots: thread.roots.iter().map(PathBuf::from).collect(),
            snapshot_root: execution.snapshot_root,
            session: Some(thread.id.clone()),
            continue_latest: false,
            prompt: input.prompt,
            image_inputs: input.image_inputs,
            extract_prompt_image_sources: input.extract_prompt_image_sources,
            prompt_display: input.prompt_display,
            max_context_messages: execution.max_context_messages,
            config_path: execution.config_path,
            project_context_override: execution.project_context,
            sandbox_override: execution.sandbox,
            model: model.model,
            reasoning_effort: model.reasoning_effort,
            runtime_ref: target.runtime_profile_ref,
            runtime_session_id: None,
            runtime_options: target.runtime_options,
            include_reasoning: model.include_reasoning,
            mode: execution.mode,
            permission_mode: execution.permission_mode,
            approval_handler: execution.approval_handler,
            clarify_enabled: execution.clarify_enabled,
            inherited_env: Some(environment.inherited_env),
            agent: target.agent_ref,
            external_agent_delegate: None,
            no_agents: capabilities.no_agents,
            no_skills: capabilities.no_skills,
            selected_capability_roots: capabilities.selected_capability_roots,
            skill_inputs: capabilities.skill_inputs,
            mcp_servers: capabilities.mcp_servers,
            mcp_runtime: Some(capabilities.mcp_runtime),
            workspace_mutations: execution.workspace_mutations,
            runtime_tools: capabilities.tools,
        };
        options.external_agent_delegate = child_template.map(|child_template| {
            Arc::new(FrameworkExternalAgentDelegate {
                child_turns,
                child_template,
                events,
            }) as Arc<dyn crate::types::ExternalAgentDelegate>
        });
        persistence.confirm_delivery().await?;
        let result = run_live_streaming_controlled_with_capture(
            options,
            &source,
            &[source.as_str()],
            stream,
            runtime_control,
            workspace_root_capture,
            self.provider,
        )
        .await?;
        debug_assert_eq!(result.session_id, receipt.thread_id);
        Ok(TurnResult::from(result))
    }
}

#[derive(Clone)]
struct FrameworkExternalAgentDelegate {
    child_turns: AgentChildTurnDispatcher,
    child_template: AgentChildTurnTemplate,
    events: TurnEventSender,
}

struct ResolvedAgentChildTurn {
    parent_thread_id: String,
    child_thread_id: String,
    turn_id: String,
    abort: psychevo_ai::AbortSignal,
    plan: ResolvedTurnPlan,
}

impl fmt::Debug for FrameworkExternalAgentDelegate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FrameworkExternalAgentDelegate")
            .finish_non_exhaustive()
    }
}

impl crate::types::ExternalAgentDelegate for FrameworkExternalAgentDelegate {
    fn run(
        &self,
        request: crate::types::ExternalAgentDelegateRequest,
    ) -> BoxFuture<'static, Result<crate::types::ExternalAgentDelegateResult>> {
        let delegate = self.clone();
        Box::pin(async move { delegate.run_inner(request).await })
    }
}

impl FrameworkExternalAgentDelegate {
    async fn run_inner(
        self,
        request: crate::types::ExternalAgentDelegateRequest,
    ) -> Result<crate::types::ExternalAgentDelegateResult> {
        let Self {
            child_turns,
            child_template,
            events,
        } = self;
        let ResolvedAgentChildTurn {
            parent_thread_id,
            child_thread_id,
            turn_id,
            abort,
            plan,
        } = child_template.resolve_child(request);
        let result = async {
            let handle = child_turns
                .start_child_turn(&parent_thread_id, &child_thread_id, plan)
                .await?;
            let mut event_stream = handle.events();
            let wait_handle = handle.clone();
            let mut completion = Box::pin(async move { wait_handle.wait().await });
            let mut abort = abort;
            let mut interrupted = Box::pin(async move { abort.wait_for_abort().await });
            loop {
                tokio::select! {
                    completed = &mut completion => break completed,
                    _ = &mut interrupted => {
                        handle.interrupt();
                        break completion.await;
                    }
                    event = event_stream.next() => {
                        if let Some(event) = event {
                            events.emit(TurnEvent::Scoped {
                                thread_id: child_thread_id.clone(),
                                turn_id: turn_id.clone(),
                                event: Box::new(event),
                            });
                        }
                    }
                }
            }
            .map(|turn| crate::types::ExternalAgentDelegateResult {
                child_session_id: child_thread_id.clone(),
                final_answer: turn.final_answer,
                outcome: match turn.outcome {
                    TurnOutcome::Completed => psychevo_ai::Outcome::Normal,
                    TurnOutcome::Stopped => psychevo_ai::Outcome::Stopped,
                    TurnOutcome::Failed => psychevo_ai::Outcome::Failed,
                    TurnOutcome::Interrupted => psychevo_ai::Outcome::Aborted,
                },
            })
        }
        .await;
        child_turns
            .close_child_relationship(&child_thread_id)
            .await?;
        result
    }
}

#[cfg(test)]
mod filesystem_authorizer_tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::sync::atomic::{AtomicBool, Ordering};

    use tokio::sync::Notify;

    use super::*;
    use crate::types::RunMode;

    #[derive(Debug)]
    struct PendingApproval {
        started: Arc<Notify>,
        cancelled: Arc<AtomicBool>,
    }

    #[derive(Debug)]
    struct AllowApproval;

    impl crate::types::ApprovalHandler for AllowApproval {
        fn request_permission(
            &self,
            _request: crate::types::PermissionApprovalRequest,
        ) -> BoxFuture<'static, crate::types::PermissionApprovalDecision> {
            Box::pin(async { crate::types::PermissionApprovalDecision::allow_once() })
        }
    }

    impl crate::types::ApprovalHandler for PendingApproval {
        fn request_permission(
            &self,
            _request: crate::types::PermissionApprovalRequest,
        ) -> BoxFuture<'static, crate::types::PermissionApprovalDecision> {
            self.started.notify_one();
            Box::pin(std::future::pending())
        }

        fn cancel_permission_with_reason(
            &self,
            _tool_call_id: &str,
            _reason: &str,
        ) -> BoxFuture<'static, ()> {
            self.cancelled.store(true, Ordering::SeqCst);
            Box::pin(async {})
        }
    }

    #[tokio::test]
    async fn captured_authorizer_applies_explicit_filesystem_denies_to_secondary_roots() {
        let temp = tempfile::tempdir().expect("temp");
        let primary = temp.path().join("primary");
        let secondary = temp.path().join("secondary");
        fs::create_dir_all(&primary).expect("primary");
        fs::create_dir_all(&secondary).expect("secondary");
        let denied = secondary.join("secret.txt");
        fs::write(&denied, "secret").expect("secret");
        let config_path = temp.path().join("config.toml");
        fs::write(
            &config_path,
            format!(
                "default_permissions = \"local\"\n\
                 [permissions.local]\n\
                 extends = \":workspace\"\n\
                 [permissions.local.filesystem]\n\
                 {:?} = \"deny\"\n",
                denied.to_string_lossy()
            ),
        )
        .expect("config");
        let thread = ThreadExecutionContext {
            id: "thread".to_string(),
            cwd: primary.to_string_lossy().into_owned(),
            workspace_id: Some("workspace".to_string()),
            roots: vec![
                primary.to_string_lossy().into_owned(),
                secondary.to_string_lossy().into_owned(),
            ],
            source: "test".to_string(),
            source_key: None,
        };
        let execution = AgentExecutionPolicy {
            source: "test".to_string(),
            config_path: Some(config_path),
            mode: RunMode::Default,
            permission_mode: None,
            approval_handler: None,
            clarify_enabled: false,
            project_context: None,
            sandbox: None,
            snapshot_root: None,
            max_context_messages: None,
            workspace_mutations: None,
        };
        let state = crate::state::StateRuntime::open(":memory:")
            .await
            .expect("state");
        let (_handle, control) = crate::types::run_control();
        let authorizer = AgentFilesystemAuthorizer {
            runtime: AgentFilesystemAuthorizer::capture_runtime(
                &thread,
                &execution,
                &AgentEnvironmentOverlay {
                    inherited_env: BTreeMap::from([(
                        "HOME".to_string(),
                        temp.path().to_string_lossy().into_owned(),
                    )]),
                },
                None,
                &AgentModelSelection {
                    model: None,
                    reasoning_effort: None,
                    include_reasoning: false,
                },
                &state,
            )
            .map_err(|error| error.to_string()),
            abort: control.abort_signal(),
        };

        let error = authorizer
            .authorize_read("acp-read", &denied)
            .await
            .expect_err("explicit deny");

        assert!(error.contains("denied"), "{error}");
    }

    #[tokio::test]
    async fn captured_authorizer_rejects_an_acp_read_outside_runtime_roots() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("workspace");
        let outside = temp.path().join("outside.txt");
        fs::create_dir_all(&cwd).expect("workspace");
        fs::write(&outside, "host secret").expect("outside file");
        let runtime = crate::permissions::PermissionRuntime::new(
            cwd.clone(),
            cwd.join(".psychevo"),
            crate::types::PermissionConfig::default(),
            crate::types::PermissionMode::Default,
            None,
            None,
        )
        .with_workspace_root_capture(
            &crate::WorkspaceRootCapture::capture(std::slice::from_ref(&cwd))
                .expect("Workspace capture"),
        );
        let (_handle, control) = crate::types::run_control();
        let authorizer = AgentFilesystemAuthorizer {
            runtime: Ok(runtime),
            abort: control.abort_signal(),
        };

        let error = authorizer
            .authorize_workspace_read("acp-outside-read", &outside)
            .await
            .expect_err("ACP transport containment must reject host reads outside runtime roots");

        assert!(error.contains("outside the captured Workspace"), "{error}");
    }

    #[tokio::test]
    async fn captured_authorizer_does_not_rescan_an_unrelated_workspace_root() {
        let temp = tempfile::tempdir().expect("temp");
        let primary = temp.path().join("primary");
        let secondary = temp.path().join("secondary");
        fs::create_dir_all(&primary).expect("primary");
        fs::create_dir_all(&secondary).expect("secondary");
        let target = primary.join("visible.txt");
        fs::write(&target, "visible").expect("target");
        let capture = crate::WorkspaceRootCapture::capture(&[primary.clone(), secondary.clone()])
            .expect("Workspace capture");
        let runtime = crate::permissions::PermissionRuntime::new(
            primary.clone(),
            primary.join(".psychevo"),
            crate::types::PermissionConfig::default(),
            crate::types::PermissionMode::Default,
            None,
            None,
        )
        .with_workspace_root_capture(&capture);
        std::fs::rename(&secondary, temp.path().join("secondary-moved"))
            .expect("replace unrelated root path");
        let (_handle, control) = crate::types::run_control();
        let authorizer = AgentFilesystemAuthorizer {
            runtime: Ok(runtime),
            abort: control.abort_signal(),
        };

        authorizer
            .authorize_read("acp-primary-read", &target)
            .await
            .expect("an unrelated root change must not block the target root");
    }

    #[tokio::test]
    async fn captured_authorizer_applies_the_selected_agents_plan_ceiling() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("workspace");
        fs::create_dir_all(&cwd).expect("workspace");
        let target = cwd.join("write.txt");
        let config_path = temp.path().join("config.toml");
        fs::write(
            &config_path,
            "default_permissions = \"local\"\n[permissions.local]\nextends = \":workspace\"\n",
        )
        .expect("config");
        let state = crate::state::StateRuntime::open(":memory:")
            .await
            .expect("state");
        let agent = crate::agents::AgentDefinition {
            name: "planner".to_string(),
            description: "Plan-only Agent".to_string(),
            instructions: String::new(),
            enabled: true,
            file_path: None,
            source: crate::agents::AgentSource::BuiltIn,
            backend: None,
            entrypoints: BTreeSet::new(),
            model: None,
            tool_policy: crate::agents::AgentToolPolicy {
                permission_mode: Some(crate::agents::AgentPermissionMode::Plan),
                ..crate::agents::AgentToolPolicy::default()
            },
            skills: Vec::new(),
            optional_contributions: BTreeSet::new(),
            hooks: None,
            background: None,
            initial_prompt: None,
            max_turns: None,
            max_spawn_depth: 0,
            project_instructions: None,
            effort: None,
            diagnostics: Vec::new(),
        };
        let binding = AgentBindingSnapshot {
            thread_id: "thread".to_string(),
            agent_ref: Some("planner".to_string()),
            agent_fingerprint: "agent".to_string(),
            agent_definition_json: serde_json::to_string(&agent).expect("Agent JSON"),
            runtime_ref: "runtime".to_string(),
            backend_kind: "acp".to_string(),
            native_kind: "acp".to_string(),
            native_session_id: None,
            cwd: cwd.to_string_lossy().into_owned(),
            profile_fingerprint: "profile".to_string(),
            profile_revision: "1".to_string(),
            profile_config_json: "{}".to_string(),
            adapter_kind: "acp".to_string(),
            adapter_revision: "1".to_string(),
            binding_revision: 1,
            control_revision: 1,
        };
        let execution = AgentExecutionPolicy {
            source: "test".to_string(),
            config_path: Some(config_path),
            mode: RunMode::Default,
            permission_mode: None,
            approval_handler: None,
            clarify_enabled: false,
            project_context: None,
            sandbox: None,
            snapshot_root: None,
            max_context_messages: None,
            workspace_mutations: None,
        };
        let thread = ThreadExecutionContext {
            id: "thread".to_string(),
            cwd: cwd.to_string_lossy().into_owned(),
            workspace_id: None,
            roots: vec![cwd.to_string_lossy().into_owned()],
            source: "test".to_string(),
            source_key: None,
        };
        let (_handle, control) = crate::types::run_control();
        let authorizer = AgentFilesystemAuthorizer {
            runtime: AgentFilesystemAuthorizer::capture_runtime(
                &thread,
                &execution,
                &AgentEnvironmentOverlay {
                    inherited_env: BTreeMap::from([(
                        "HOME".to_string(),
                        temp.path().to_string_lossy().into_owned(),
                    )]),
                },
                Some(&binding),
                &AgentModelSelection {
                    model: None,
                    reasoning_effort: None,
                    include_reasoning: false,
                },
                &state,
            )
            .map_err(|error| error.to_string()),
            abort: control.abort_signal(),
        };

        let error = authorizer
            .authorize_write("acp-write", &target)
            .await
            .expect_err("Plan Agent write");

        assert!(error.contains("read-only"), "{error}");
    }

    #[tokio::test]
    async fn reconstructed_authorizer_uses_framework_session_filesystem_grants() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("workspace");
        let approved = cwd.join("approved");
        fs::create_dir_all(&approved).expect("approved directory");
        let config_path = temp.path().join("config.toml");
        fs::write(
            &config_path,
            format!(
                "default_permissions = \"local\"\n\
                 [permissions.local]\n\
                 extends = \":workspace\"\n\
                 [permissions.local.filesystem]\n\
                 {:?} = \"prompt\"\n",
                approved.to_string_lossy()
            ),
        )
        .expect("config");
        let state = crate::state::StateRuntime::open(":memory:")
            .await
            .expect("state");
        state
            .filesystem_grants("thread")
            .grant_scope(&crate::types::FilesystemApprovalScope {
                directory: approved.to_string_lossy().into_owned(),
                lifetime: crate::types::FilesystemApprovalLifetime::Session,
            })
            .expect("session scope");
        let thread = ThreadExecutionContext {
            id: "thread".to_string(),
            cwd: cwd.to_string_lossy().into_owned(),
            workspace_id: None,
            roots: vec![cwd.to_string_lossy().into_owned()],
            source: "test".to_string(),
            source_key: None,
        };
        let execution = AgentExecutionPolicy {
            source: "test".to_string(),
            config_path: Some(config_path),
            mode: RunMode::Default,
            permission_mode: None,
            approval_handler: None,
            clarify_enabled: false,
            project_context: None,
            sandbox: None,
            snapshot_root: None,
            max_context_messages: None,
            workspace_mutations: None,
        };
        let (_handle, control) = crate::types::run_control();
        let authorizer = AgentFilesystemAuthorizer {
            runtime: AgentFilesystemAuthorizer::capture_runtime(
                &thread,
                &execution,
                &AgentEnvironmentOverlay {
                    inherited_env: BTreeMap::from([(
                        "HOME".to_string(),
                        temp.path().to_string_lossy().into_owned(),
                    )]),
                },
                None,
                &AgentModelSelection {
                    model: None,
                    reasoning_effort: None,
                    include_reasoning: false,
                },
                &state,
            )
            .map_err(|error| error.to_string()),
            abort: control.abort_signal(),
        };

        authorizer
            .authorize_write("acp-session-grant", &approved.join("allowed.txt"))
            .await
            .expect("shared session scope");
    }

    #[tokio::test]
    async fn callback_filesystem_approval_is_cancelled_with_its_turn() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("workspace");
        fs::create_dir_all(&cwd).expect("workspace");
        let blocked = cwd.join("blocked.txt");
        let config_path = temp.path().join("config.toml");
        fs::write(
            &config_path,
            format!(
                "default_permissions = \"local\"\n\
                 [permissions.local]\n\
                 extends = \":workspace\"\n\
                 [permissions.local.filesystem]\n\
                 {:?} = \"prompt\"\n",
                blocked.to_string_lossy()
            ),
        )
        .expect("config");
        let started = Arc::new(Notify::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let approval = Arc::new(PendingApproval {
            started: Arc::clone(&started),
            cancelled: Arc::clone(&cancelled),
        });
        let state = crate::state::StateRuntime::open(":memory:")
            .await
            .expect("state");
        let thread = ThreadExecutionContext {
            id: "thread".to_string(),
            cwd: cwd.to_string_lossy().into_owned(),
            workspace_id: None,
            roots: vec![cwd.to_string_lossy().into_owned()],
            source: "test".to_string(),
            source_key: None,
        };
        let execution = AgentExecutionPolicy {
            source: "test".to_string(),
            config_path: Some(config_path),
            mode: RunMode::Default,
            permission_mode: None,
            approval_handler: Some(approval),
            clarify_enabled: false,
            project_context: None,
            sandbox: None,
            snapshot_root: None,
            max_context_messages: None,
            workspace_mutations: None,
        };
        let (handle, control) = crate::types::run_control();
        let authorizer = AgentFilesystemAuthorizer {
            runtime: AgentFilesystemAuthorizer::capture_runtime(
                &thread,
                &execution,
                &AgentEnvironmentOverlay {
                    inherited_env: BTreeMap::from([(
                        "HOME".to_string(),
                        temp.path().to_string_lossy().into_owned(),
                    )]),
                },
                None,
                &AgentModelSelection {
                    model: None,
                    reasoning_effort: None,
                    include_reasoning: false,
                },
                &state,
            )
            .map_err(|error| error.to_string()),
            abort: control.abort_signal(),
        };
        let task = tokio::spawn(async move {
            authorizer
                .authorize_write("acp-cancelled-write", &blocked)
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), started.notified())
            .await
            .expect("callback approval started");
        handle.abort();

        let error = task
            .await
            .expect("authorization task")
            .expect_err("cancelled approval");
        assert!(error.contains("aborted"), "{error}");
        assert!(cancelled.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn callback_filesystem_authorization_preserves_permission_request_hooks() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("workspace");
        let denied = cwd.join("denied.txt");
        let project_config = cwd.join(".psychevo/config.toml");
        fs::create_dir_all(project_config.parent().expect("config directory"))
            .expect("project config directory");
        fs::write(
            &project_config,
            format!(
                "default_permissions = \"local\"\n\
                 [permissions.local]\n\
                 extends = \":workspace\"\n\
                 [permissions.local.filesystem]\n\
                 {:?} = \"prompt\"\n",
                denied.to_string_lossy()
            ),
        )
        .expect("project config");
        fs::write(
            cwd.join(".psychevo/hooks.json"),
            serde_json::to_string(&serde_json::json!({
                "hooks": {
                    "PermissionRequest": [{
                        "hooks": [{
                            "type": "command",
                            "command": "printf '{\"decision\":\"deny\",\"feedback\":\"ACP hook denial\"}'"
                        }]
                    }]
                }
            }))
            .expect("hook JSON"),
        )
        .expect("hooks");
        let state = crate::state::StateRuntime::open(":memory:")
            .await
            .expect("state");
        let thread = ThreadExecutionContext {
            id: "thread".to_string(),
            cwd: cwd.to_string_lossy().into_owned(),
            workspace_id: None,
            roots: vec![cwd.to_string_lossy().into_owned()],
            source: "test".to_string(),
            source_key: None,
        };
        let execution = AgentExecutionPolicy {
            source: "test".to_string(),
            config_path: Some(project_config),
            mode: RunMode::Default,
            permission_mode: None,
            approval_handler: Some(Arc::new(AllowApproval)),
            clarify_enabled: false,
            project_context: None,
            sandbox: None,
            snapshot_root: None,
            max_context_messages: None,
            workspace_mutations: None,
        };
        let (_handle, control) = crate::types::run_control();
        let authorizer = AgentFilesystemAuthorizer {
            runtime: AgentFilesystemAuthorizer::capture_runtime(
                &thread,
                &execution,
                &AgentEnvironmentOverlay {
                    inherited_env: BTreeMap::from([
                        (
                            "HOME".to_string(),
                            temp.path().to_string_lossy().into_owned(),
                        ),
                        ("PSYCHEVO_BYPASS_HOOK_TRUST".to_string(), "true".to_string()),
                    ]),
                },
                None,
                &AgentModelSelection {
                    model: None,
                    reasoning_effort: None,
                    include_reasoning: false,
                },
                &state,
            )
            .map_err(|error| error.to_string()),
            abort: control.abort_signal(),
        };

        let error = authorizer
            .authorize_write("acp-hook-write", &denied)
            .await
            .expect_err("PermissionRequest hook denial");

        assert!(error.contains("ACP hook denial"), "{error}");
    }

    #[tokio::test]
    async fn callback_filesystem_authorization_preserves_the_smart_reviewer() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("workspace");
        fs::create_dir_all(&cwd).expect("workspace");
        let config_path = temp.path().join("config.toml");
        fs::write(
            &config_path,
            r#"
model = "primary/main"
approvals_reviewer = "smart"
default_permissions = "local"

[permissions.local]
extends = ":workspace"

[provider.primary]
api = "http://127.0.0.1:9/v1"
no_auth = true

[provider.primary.models.main]
"#,
        )
        .expect("config");
        let state = crate::state::StateRuntime::open(":memory:")
            .await
            .expect("state");
        let thread = ThreadExecutionContext {
            id: "thread".to_string(),
            cwd: cwd.to_string_lossy().into_owned(),
            workspace_id: None,
            roots: vec![cwd.to_string_lossy().into_owned()],
            source: "test".to_string(),
            source_key: None,
        };
        let execution = AgentExecutionPolicy {
            source: "test".to_string(),
            config_path: Some(config_path),
            mode: RunMode::Default,
            permission_mode: None,
            approval_handler: None,
            clarify_enabled: false,
            project_context: None,
            sandbox: None,
            snapshot_root: None,
            max_context_messages: None,
            workspace_mutations: None,
        };

        let runtime = AgentFilesystemAuthorizer::capture_runtime(
            &thread,
            &execution,
            &AgentEnvironmentOverlay {
                inherited_env: BTreeMap::from([(
                    "HOME".to_string(),
                    temp.path().to_string_lossy().into_owned(),
                )]),
            },
            None,
            &AgentModelSelection {
                model: None,
                reasoning_effort: None,
                include_reasoning: false,
            },
            &state,
        )
        .expect("callback permission runtime");

        assert!(runtime.has_smart_approval_handler());
    }
}
