use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use futures::future::BoxFuture;
use serde_json::Value;

use super::{
    AgentBindingSnapshot, AgentCapabilitySelection, AgentChildTurnDispatcher,
    AgentChildTurnTemplate, AgentEnvironmentOverlay, AgentExecutionPolicy, AgentInputPart,
    AgentModelSelection, AgentSessionAdapter, AgentTargetSelection, AgentThreadForkRequest,
    AgentThreadImportRequest, AgentThreadLifecycleRequest, AgentTurnInput, AgentTurnInvocation,
    AgentTurnPersistence, AgentTurnPreparation, AgentUnknownDelivery, Client,
    FrameworkAgentTurnPersistence, NativeAgentSessionAdapter, PreparedAgentTurn,
    ResolvedCapabilityPlan, ResolvedTurnPlan, ThreadExecutionContext, TurnEvent, TurnEventSender,
    TurnOutcome, TurnResult,
};
use crate::run::{run_live_streaming_controlled, run_live_streaming_controlled_with_provider};
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
        let result = match self.provider {
            Some(provider) => {
                run_live_streaming_controlled_with_provider(
                    options,
                    &source,
                    &[source.as_str()],
                    stream,
                    runtime_control,
                    provider,
                )
                .await
            }
            None => {
                run_live_streaming_controlled(
                    options,
                    &source,
                    &[source.as_str()],
                    stream,
                    runtime_control,
                )
                .await
            }
        }?;
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
