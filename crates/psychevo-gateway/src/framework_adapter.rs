use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::agent_session::{
    AgentErrorStage, AgentSessionHost, AgentSessionRef, AttachedAgent, CapturedAgentSessionTarget,
    CapturedFrameworkAgentImport, agent_session_configuration_error, agent_session_error,
};
use super::agent_session_binding::{
    self, GatewayAgentTurnPreparation, PreparedGatewayAgentTurn,
    prepare_framework_gateway_agent_turn, runtime_profile_config_fingerprint,
};
use super::peer_runtime::ResolvedPeerTurn;
#[cfg(test)]
use crate::FrameworkNativeTestExecutor;
use crate::acp_peer;
use crate::{ACP_PEER_METADATA_KEY, gateway_now_ms};
use futures::future::BoxFuture;
use psychevo::{
    Error, ImageInput, PermissionMode, RunMode,
    application::RunStreamSink,
    config::{RuntimeProfileConfig, RuntimeProfileKind},
};
use psychevo_gateway_protocol::source::{GatewayImageInput, GatewayInputPart};

#[derive(Clone)]
pub(crate) struct GatewayAgentSessionAdapter {
    agent_sessions: AgentSessionHost,
    inherited_env: BTreeMap<String, String>,
    #[cfg(test)]
    native_test_executor: Option<FrameworkNativeTestExecutor>,
}

impl GatewayAgentSessionAdapter {
    pub(crate) fn new(
        agent_sessions: AgentSessionHost,
        home: PathBuf,
        mut inherited_env: BTreeMap<String, String>,
    ) -> Self {
        inherited_env
            .entry("PSYCHEVO_HOME".to_string())
            .or_insert_with(|| home.to_string_lossy().into_owned());
        Self {
            agent_sessions,
            inherited_env,
            #[cfg(test)]
            native_test_executor: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_native_test_executor(
        agent_sessions: AgentSessionHost,
        home: PathBuf,
        inherited_env: BTreeMap<String, String>,
        native_test_executor: FrameworkNativeTestExecutor,
    ) -> Self {
        let mut adapter = Self::new(agent_sessions, home, inherited_env);
        adapter.native_test_executor = Some(native_test_executor);
        adapter
    }
}

impl fmt::Debug for GatewayAgentSessionAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayAgentSessionAdapter")
            .finish_non_exhaustive()
    }
}

struct PreparedFrameworkGatewayTurn {
    agent_sessions: AgentSessionHost,
    native_backend: psychevo::NativeTurnBackend,
    target: PreparedGatewayAgentTurn,
    prepared_source_key: Option<String>,
    initial_binding: Option<psychevo::InitialAgentBinding>,
    #[cfg(test)]
    native_test_executor: Option<FrameworkNativeTestExecutor>,
}

impl fmt::Debug for PreparedFrameworkGatewayTurn {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedFrameworkGatewayTurn")
            .field("target", &self.target)
            .field("creates_binding", &self.initial_binding.is_some())
            .finish_non_exhaustive()
    }
}

impl psychevo::PreparedAgentTurn for PreparedFrameworkGatewayTurn {
    fn admission(&self) -> psychevo::AgentAdmissionFacts {
        psychevo::AgentAdmissionFacts {
            initial_binding: self.initial_binding.clone(),
        }
    }

    fn invoke(
        self: Box<Self>,
        invocation: psychevo::AgentTurnInvocation,
    ) -> BoxFuture<'static, psychevo::Result<psychevo::TurnResult>> {
        Box::pin(run_prepared_framework_gateway_turn(*self, invocation))
    }
}

impl psychevo::AgentSessionAdapter for GatewayAgentSessionAdapter {
    fn prepare_turn(
        self: Arc<Self>,
        request: psychevo::AgentTurnPreparation,
    ) -> BoxFuture<'static, psychevo::Result<Box<dyn psychevo::PreparedAgentTurn>>> {
        Box::pin(async move {
            let thread_id = request.thread.id.clone();
            let (target, creates_binding) =
                prepare_framework_gateway_agent_turn(GatewayAgentTurnPreparation {
                    thread: &request.thread,
                    binding: request.binding.as_ref(),
                    target: &request.target,
                    inherited_env: &request.inherited_env,
                    purpose: request.purpose,
                })?;
            let prepared_source_key = request.thread.source_key.clone();
            let initial_binding = creates_binding
                .then(|| target.initial_binding(&thread_id))
                .transpose()?;
            Ok(Box::new(PreparedFrameworkGatewayTurn {
                agent_sessions: self.agent_sessions.clone(),
                native_backend: request.native_backend,
                target,
                prepared_source_key,
                initial_binding,
                #[cfg(test)]
                native_test_executor: self.native_test_executor.clone(),
            }) as Box<dyn psychevo::PreparedAgentTurn>)
        })
    }

    fn shutdown(&self, force: bool) -> BoxFuture<'static, psychevo::Result<()>> {
        let agent_sessions = self.agent_sessions.clone();
        Box::pin(async move { agent_sessions.shutdown(force).await })
    }

    fn apply_thread_lifecycle(
        &self,
        request: psychevo::AgentThreadLifecycleRequest,
    ) -> BoxFuture<'static, psychevo::Result<psychevo::AgentThreadLifecycleOutcome>> {
        let adapter = self.clone();
        Box::pin(async move { adapter.apply_thread_lifecycle(request).await })
    }

    fn import_thread(
        self: Arc<Self>,
        request: psychevo::AgentThreadImportRequest,
    ) -> BoxFuture<'static, psychevo::Result<psychevo::AgentThreadPublication>> {
        Box::pin(async move { self.import_agent_thread(request).await })
    }

    fn fork_thread(
        self: Arc<Self>,
        request: psychevo::AgentThreadForkRequest,
    ) -> BoxFuture<'static, psychevo::Result<psychevo::AgentThreadPublication>> {
        Box::pin(async move { self.fork_agent_thread(request).await })
    }

    fn abort_thread_publication(
        &self,
        request: psychevo::AgentThreadPublicationAbortRequest,
    ) -> BoxFuture<'static, psychevo::Result<()>> {
        let agent_sessions = self.agent_sessions.clone();
        Box::pin(async move {
            let Some(native_session_id) = request.binding.native_session_id else {
                return Ok(());
            };
            agent_sessions
                .release_acp_session(request.thread.id, native_session_id)
                .await
        })
    }
}

impl GatewayAgentSessionAdapter {
    async fn fork_agent_thread(
        &self,
        request: psychevo::AgentThreadForkRequest,
    ) -> psychevo::Result<psychevo::AgentThreadPublication> {
        if request.binding.thread_id != request.source.id
            || request.binding.cwd != request.source.cwd
            || request.destination.cwd != request.source.cwd
            || request.destination.id == request.source.id
        {
            return Err(agent_session_error(
                "bound_thread_snapshot_mismatch",
                AgentErrorStage::Binding,
                "never",
                "not_delivered",
                "The immutable Agent binding does not match the requested Thread fork.",
                Some(format!("agent-binding:{}", request.source.id)),
            ));
        }
        if request.binding.backend_kind != "acp" {
            return Err(agent_session_error(
                "agent_session_fork_unsupported",
                AgentErrorStage::History,
                "user_action",
                "not_delivered",
                "This Agent does not expose session fork.",
                Some(format!("thread:{}", request.source.id)),
            ));
        }
        let native_session_id = required_native_session_id(&request.binding)?;
        let (attached, peer, label) = self.lifecycle_agent(&request.source, &request.binding)?;
        if self
            .agent_sessions
            .inspect_cached_acp_session(request.source.id.clone(), native_session_id.clone())
            .await?
            .is_none()
        {
            let mcp_names = acp_peer::stdio_turn::requested_peer_mcp_server_names(&peer)?;
            let mcp_servers = request
                .resolve_mcp_server_handoffs(&mcp_names)
                .await
                .map_err(|error| acp_mcp_resolution_error(&peer, error))?;
            attached
                .resume_session(AgentSessionRef {
                    cwd: PathBuf::from(&request.source.cwd),
                    local_session_id: request.source.id.clone(),
                    native_session_id: native_session_id.clone(),
                    mcp_servers,
                })
                .await?;
        }
        let snapshot = attached
            .fork_session(
                AgentSessionRef {
                    cwd: PathBuf::from(&request.source.cwd),
                    local_session_id: request.source.id.clone(),
                    native_session_id,
                    mcp_servers: Vec::new(),
                },
                request.destination.id.clone(),
            )
            .await?
            .into_acp()?;
        let binding = psychevo::InitialAgentBinding {
            agent_ref: request.binding.agent_ref.clone(),
            agent_fingerprint: request.binding.agent_fingerprint.clone(),
            agent_definition_json: request.binding.agent_definition_json.clone(),
            runtime_ref: request.binding.runtime_ref.clone(),
            backend_kind: request.binding.backend_kind.clone(),
            native_kind: request.binding.native_kind.clone(),
            native_session_id: Some(snapshot.native_session_id.clone()),
            profile_fingerprint: request.binding.profile_fingerprint.clone(),
            profile_revision: request.binding.profile_revision.clone(),
            profile_config_json: request.binding.profile_config_json.clone(),
            adapter_kind: request.binding.adapter_kind.clone(),
            adapter_revision: request.binding.adapter_revision.clone(),
        };
        let metadata = BTreeMap::from([(
            ACP_PEER_METADATA_KEY.to_string(),
            acp_peer::metadata_permissions::peer_session_metadata(
                &peer,
                Some(&snapshot.native_session_id),
                None,
                &BTreeMap::new(),
                Some(&snapshot),
            ),
        )]);
        Ok(psychevo::AgentThreadPublication {
            binding,
            messages: Vec::new(),
            metadata,
            title: snapshot.session_info.title.clone(),
            lifecycle: agent_session_lifecycle_projection(&label, &snapshot),
            history: imported_history_facts(&snapshot),
        })
    }

    async fn import_agent_thread(
        &self,
        request: psychevo::AgentThreadImportRequest,
    ) -> psychevo::Result<psychevo::AgentThreadPublication> {
        let captured = self
            .agent_sessions
            .consume_framework_import(&request.preparation)?;
        let native_session_id = captured.native_session_id.clone();
        let local_session_id = request.thread.id.clone();
        let result = self.import_captured_agent_thread(request, captured).await;
        if result.is_err() {
            let _ = self
                .agent_sessions
                .release_acp_session(local_session_id, native_session_id)
                .await;
        }
        result
    }

    async fn import_captured_agent_thread(
        &self,
        request: psychevo::AgentThreadImportRequest,
        captured: CapturedFrameworkAgentImport,
    ) -> psychevo::Result<psychevo::AgentThreadPublication> {
        let thread = &request.thread;
        if captured.target.profile.runtime != RuntimeProfileKind::Acp {
            return Err(agent_session_configuration_error(
                "Only an ACP Runtime Profile can import an Agent-owned session.",
            ));
        }
        if captured.context.cwd.as_path() != Path::new(&thread.cwd) {
            return Err(agent_session_error(
                "agent_session_candidate_scope_mismatch",
                AgentErrorStage::Binding,
                "user_action",
                "not_delivered",
                "The captured Agent import belongs to another workspace.",
                None,
            ));
        }
        let peer = captured.target.peer.clone().ok_or_else(|| {
            agent_session_configuration_error("The selected ACP Agent is unavailable.")
        })?;
        let mcp_names = acp_peer::stdio_turn::requested_peer_mcp_server_names(&peer)?;
        let mcp_servers = request
            .resolve_mcp_server_handoffs(&mcp_names)
            .await
            .map_err(|error| acp_mcp_resolution_error(&peer, error))?;
        let loaded = self
            .agent_sessions
            .attach(CapturedAgentSessionTarget::invocation(
                format!("session-import:{}", thread.id),
                captured.target.profile.clone(),
                Some(peer.clone()),
            ))?
            .load_session(AgentSessionRef {
                cwd: captured.context.cwd.clone(),
                local_session_id: thread.id.clone(),
                native_session_id: captured.native_session_id.clone(),
                mcp_servers,
            })
            .await?;
        if loaded.snapshot.native_session_id != captured.native_session_id {
            return Err(agent_session_error(
                "agent_session_load_identity_mismatch",
                AgentErrorStage::Binding,
                "never",
                "unknown",
                "The ACP Agent loaded a different native session than requested.",
                None,
            ));
        }

        let mut binding = captured.target.initial_binding(&thread.id)?;
        binding.native_session_id = Some(captured.native_session_id.clone());
        let snapshot = loaded.snapshot;
        let metadata = BTreeMap::from([(
            ACP_PEER_METADATA_KEY.to_string(),
            acp_peer::metadata_permissions::peer_session_metadata(
                &peer,
                Some(&captured.native_session_id),
                None,
                &captured.context.runtime_options,
                Some(&snapshot),
            ),
        )]);
        let history = imported_history_facts(&snapshot);
        Ok(psychevo::AgentThreadPublication {
            binding,
            messages: acp_peer::turn::project_imported_acp_replay(&peer, &loaded.replay),
            metadata,
            title: captured
                .title
                .or_else(|| snapshot.session_info.title.clone()),
            lifecycle: psychevo::AgentImportedLifecycle {
                target_label: captured.target_label,
                fork: snapshot.capabilities.session.fork,
                delete: snapshot.capabilities.session.delete,
                close: snapshot.capabilities.session.close,
                resume: snapshot.capabilities.session.resume,
            },
            history,
        })
    }

    async fn apply_thread_lifecycle(
        &self,
        request: psychevo::AgentThreadLifecycleRequest,
    ) -> psychevo::Result<psychevo::AgentThreadLifecycleOutcome> {
        let Some(binding) = request.binding.as_ref() else {
            return Ok(psychevo::AgentThreadLifecycleOutcome::Unchanged);
        };
        if binding.backend_kind != "acp" {
            return Ok(psychevo::AgentThreadLifecycleOutcome::Unchanged);
        }
        if binding.thread_id != request.thread.id || binding.cwd != request.thread.cwd {
            return Err(agent_session_error(
                "bound_thread_snapshot_mismatch",
                AgentErrorStage::Binding,
                "never",
                "not_delivered",
                "The immutable Agent binding does not match its Thread lifecycle context.",
                Some(format!("agent-binding:{}", request.thread.id)),
            ));
        }
        match &request.action {
            psychevo::AgentThreadLifecycleAction::Archive { .. } => {
                self.archive_bound_agent_session(&request.thread, binding)
                    .await?;
                Ok(psychevo::AgentThreadLifecycleOutcome::Unchanged)
            }
            psychevo::AgentThreadLifecycleAction::Restore => {
                self.restore_bound_agent_session(&request, binding).await
            }
            psychevo::AgentThreadLifecycleAction::Delete => {
                self.delete_bound_agent_session(&request, binding).await
            }
        }
    }

    async fn archive_bound_agent_session(
        &self,
        thread: &psychevo::ThreadExecutionContext,
        binding: &psychevo::AgentBindingSnapshot,
    ) -> psychevo::Result<()> {
        let Some(native_session_id) = binding.native_session_id.clone() else {
            return Ok(());
        };
        let Some(snapshot) = self
            .agent_sessions
            .inspect_cached_acp_session(thread.id.clone(), native_session_id.clone())
            .await?
        else {
            return Ok(());
        };
        if !snapshot.capabilities.session.close {
            return Ok(());
        }
        let (attached, _, _) = self.lifecycle_agent(thread, binding)?;
        attached
            .close_session(AgentSessionRef {
                cwd: PathBuf::from(&thread.cwd),
                local_session_id: thread.id.clone(),
                native_session_id,
                mcp_servers: Vec::new(),
            })
            .await
    }

    async fn restore_bound_agent_session(
        &self,
        request: &psychevo::AgentThreadLifecycleRequest,
        binding: &psychevo::AgentBindingSnapshot,
    ) -> psychevo::Result<psychevo::AgentThreadLifecycleOutcome> {
        let thread = &request.thread;
        let native_session_id = required_native_session_id(binding)?;
        let (attached, peer, label) = self.lifecycle_agent(thread, binding)?;
        let snapshot = match self
            .agent_sessions
            .inspect_cached_acp_session(thread.id.clone(), native_session_id.clone())
            .await?
        {
            Some(snapshot) => snapshot,
            None => {
                let mcp_names = acp_peer::stdio_turn::requested_peer_mcp_server_names(&peer)?;
                let mcp_servers = request
                    .resolve_mcp_server_handoffs(&mcp_names)
                    .await
                    .map_err(|error| acp_mcp_resolution_error(&peer, error))?;
                attached
                    .resume_session(AgentSessionRef {
                        cwd: PathBuf::from(&thread.cwd),
                        local_session_id: thread.id.clone(),
                        native_session_id,
                        mcp_servers,
                    })
                    .await?
                    .into_acp()?
            }
        };
        Ok(psychevo::AgentThreadLifecycleOutcome::Projection(
            agent_session_lifecycle_projection(&label, &snapshot),
        ))
    }

    async fn delete_bound_agent_session(
        &self,
        request: &psychevo::AgentThreadLifecycleRequest,
        binding: &psychevo::AgentBindingSnapshot,
    ) -> psychevo::Result<psychevo::AgentThreadLifecycleOutcome> {
        let thread = &request.thread;
        let current = &request.current;
        if matches!(
            current.remote_delete,
            psychevo::AgentRemoteDeleteState::Acknowledged { .. }
        ) {
            return Ok(psychevo::AgentThreadLifecycleOutcome::Unchanged);
        }
        let native_session_id = required_native_session_id(binding)?;
        let (attached, peer, _) = self.lifecycle_agent(thread, binding)?;
        let snapshot = match self
            .agent_sessions
            .inspect_cached_acp_session(thread.id.clone(), native_session_id.clone())
            .await?
        {
            Some(snapshot) => snapshot,
            None => {
                let mcp_names = acp_peer::stdio_turn::requested_peer_mcp_server_names(&peer)?;
                let mcp_servers = request
                    .resolve_mcp_server_handoffs(&mcp_names)
                    .await
                    .map_err(|error| acp_mcp_resolution_error(&peer, error))?;
                attached
                    .resume_session(AgentSessionRef {
                        cwd: PathBuf::from(&thread.cwd),
                        local_session_id: thread.id.clone(),
                        native_session_id: native_session_id.clone(),
                        mcp_servers,
                    })
                    .await?
                    .into_acp()?
            }
        };
        if !snapshot.capabilities.session.delete {
            return Err(agent_session_error(
                "agent_session_delete_unsupported",
                AgentErrorStage::History,
                "user_action",
                "not_delivered",
                "This ACP Agent does not support deleting its persistent session.",
                Some(format!("thread:{}", thread.id)),
            ));
        }
        if matches!(
            current.remote_delete,
            psychevo::AgentRemoteDeleteState::NotRequested
        ) {
            return Ok(
                psychevo::AgentThreadLifecycleOutcome::RemoteDeletePrepared {
                    at_ms: gateway_now_ms(),
                },
            );
        }
        attached
            .delete_session(AgentSessionRef {
                cwd: PathBuf::from(&thread.cwd),
                local_session_id: thread.id.clone(),
                native_session_id,
                mcp_servers: Vec::new(),
            })
            .await?;
        Ok(
            psychevo::AgentThreadLifecycleOutcome::RemoteDeleteAcknowledged {
                at_ms: gateway_now_ms(),
            },
        )
    }

    fn lifecycle_agent(
        &self,
        thread: &psychevo::ThreadExecutionContext,
        binding: &psychevo::AgentBindingSnapshot,
    ) -> psychevo::Result<(AttachedAgent, ResolvedPeerTurn, String)> {
        let profile = lifecycle_profile(binding)?;
        let peer = agent_session_binding::resolve_captured_agent_peer_at(
            agent_session_binding::CapturedAgentPeerInput {
                cwd: Path::new(&thread.cwd),
                env: &self.inherited_env,
                thread_id: &thread.id,
                agent_ref: binding.agent_ref.as_deref(),
                encoded: &binding.agent_definition_json,
                fingerprint: &binding.agent_fingerprint,
                profile: &profile,
                profile_fingerprint: &binding.profile_fingerprint,
            },
        )?
        .ok_or_else(|| {
            agent_session_configuration_error(format!(
                "ACP Runtime Profile `{}` did not resolve an Agent backend.",
                profile.id
            ))
        })?;
        let label = format!("{} · {}", peer.agent.name, profile.label);
        let attached =
            self.agent_sessions
                .attach(CapturedAgentSessionTarget::application_bound(
                    binding,
                    profile,
                    Some(peer.clone()),
                )?)?;
        Ok((attached, peer, label))
    }
}

fn acp_mcp_resolution_error(peer: &ResolvedPeerTurn, error: Error) -> Error {
    agent_session_error(
        "acp_mcp_configuration_invalid",
        AgentErrorStage::Binding,
        "user_action",
        "not_delivered",
        error.to_string(),
        Some(format!("acp-mcp:{}", peer.backend.id)),
    )
}

fn imported_history_facts(
    snapshot: &acp_peer::session_projection::AcpSessionSnapshot,
) -> psychevo::AgentImportedHistory {
    let owner = match snapshot.history.owner {
        acp_peer::session_projection::AcpHistoryOwnerSnapshot::Agent => {
            psychevo::AgentHistoryOwner::Agent
        }
        acp_peer::session_projection::AcpHistoryOwnerSnapshot::Process => {
            psychevo::AgentHistoryOwner::Process
        }
    };
    let fidelity = if snapshot.history.replay_complete {
        psychevo::AgentHistoryFidelity::Full
    } else {
        psychevo::AgentHistoryFidelity::Partial
    };
    let hint = if !snapshot.history.resumable {
        Some(
            "This ACP Agent history is process-ephemeral and cannot be resumed after restart."
                .to_string(),
        )
    } else if snapshot.history.loaded_from_agent && !snapshot.history.replay_complete {
        Some(
            "ACP Agent history replay is incomplete because some content lacked stable identity or exceeded product projection limits."
                .to_string(),
        )
    } else if !snapshot.history.loaded_from_agent {
        Some(
            "History is Agent-authoritative and resumable; this process has not loaded a prior session."
                .to_string(),
        )
    } else {
        None
    };
    psychevo::AgentImportedHistory {
        owner,
        fidelity,
        resumable: snapshot.history.resumable,
        hint,
    }
}

fn lifecycle_profile(
    binding: &psychevo::AgentBindingSnapshot,
) -> psychevo::Result<RuntimeProfileConfig> {
    let profile: RuntimeProfileConfig = serde_json::from_str(&binding.profile_config_json)
        .map_err(|error| {
            agent_session_error(
                "bound_profile_snapshot_invalid",
                AgentErrorStage::Binding,
                "never",
                "not_delivered",
                format!("Bound Runtime Profile snapshot could not be decoded: {error}"),
                Some(format!("agent-binding:{}", binding.thread_id)),
            )
        })?;
    if profile.id != binding.runtime_ref
        || runtime_profile_config_fingerprint(&profile) != binding.profile_fingerprint
    {
        return Err(agent_session_error(
            "bound_profile_snapshot_mismatch",
            AgentErrorStage::Binding,
            "never",
            "not_delivered",
            "Bound Runtime Profile snapshot does not match its immutable identity.",
            Some(format!("agent-binding:{}", binding.thread_id)),
        ));
    }
    Ok(profile)
}

fn required_native_session_id(
    binding: &psychevo::AgentBindingSnapshot,
) -> psychevo::Result<String> {
    binding.native_session_id.clone().ok_or_else(|| {
        agent_session_configuration_error(format!(
            "Agent binding for thread `{}` has no native session id.",
            binding.thread_id
        ))
    })
}

pub(crate) fn agent_session_lifecycle_projection(
    target_label: &str,
    snapshot: &acp_peer::session_projection::AcpSessionSnapshot,
) -> psychevo::AgentImportedLifecycle {
    psychevo::AgentImportedLifecycle {
        target_label: target_label.to_string(),
        fork: snapshot.capabilities.session.fork,
        delete: snapshot.capabilities.session.delete,
        close: snapshot.capabilities.session.close,
        resume: snapshot.capabilities.session.resume,
    }
}

async fn run_prepared_framework_gateway_turn(
    prepared: PreparedFrameworkGatewayTurn,
    mut invocation: psychevo::AgentTurnInvocation,
) -> psychevo::Result<psychevo::TurnResult> {
    if prepared.target.profile.runtime == RuntimeProfileKind::Native {
        validate_native_input(&invocation.input.parts)?;
        lower_native_invocation_controls(&mut invocation)?;
        invocation
            .persistence
            .clear_agent_usage_observation()
            .await?;
        #[cfg(test)]
        if let Some(executor) = prepared.native_test_executor {
            return executor(invocation).await;
        }
        return prepared.native_backend.execute(invocation).await;
    }
    let peer = prepared.target.peer.as_ref().ok_or_else(|| {
        Error::Message(format!(
            "ACP Runtime Profile `{}` has no captured Agent backend",
            prepared.target.profile.id
        ))
    })?;
    let mcp_names = acp_peer::stdio_turn::requested_peer_mcp_server_names(peer)?;
    let mcp_servers = invocation.resolve_mcp_server_handoffs(&mcp_names).await?;
    let psychevo::AgentTurnInvocation {
        thread,
        history,
        receipt,
        binding,
        target,
        input,
        model,
        execution,
        persistence,
        events,
        control,
        ..
    } = invocation;
    let adapter_input = framework_gateway_input(input.parts.clone());
    let stream: RunStreamSink = Arc::new(move |event| {
        events.emit_agent_event(event);
    });
    let PreparedGatewayAgentTurn {
        profile,
        profile_revision: _,
        profile_fingerprint,
        peer,
        agent,
    } = prepared.target;
    let binding = binding.ok_or_else(|| {
        Error::Message(format!(
            "captured Agent binding disappeared for Thread `{}`",
            receipt.thread_id
        ))
    })?;
    if binding.runtime_ref != profile.id
        || binding.profile_fingerprint != profile_fingerprint
        || binding.agent_ref != agent.agent_ref
        || binding.agent_fingerprint != agent.fingerprint
    {
        return Err(Error::Message(format!(
            "durable Agent binding no longer matches the accepted capture for Thread `{}`",
            receipt.thread_id
        )));
    }
    let peer = peer.ok_or_else(|| {
        Error::Message(format!(
            "ACP Runtime Profile `{}` has no captured Agent backend",
            profile.id
        ))
    })?;
    persistence.clear_agent_usage_observation().await?;
    let binding_revision = binding.binding_revision;
    let mut native_session_id = binding.native_session_id;
    if native_session_id.is_none()
        && let Some(source_key) = prepared.prepared_source_key.as_deref()
        && let Some(promoted_native_session_id) = prepared
            .agent_sessions
            .promote_prepared(
                source_key,
                agent.agent_ref.as_deref(),
                &profile.id,
                &profile_fingerprint,
                &receipt.thread_id,
            )
            .await?
    {
        persistence
            .attach_native_session(binding_revision, promoted_native_session_id.clone())
            .await?;
        native_session_id = Some(promoted_native_session_id);
    }
    let session_persistence = persistence.clone();
    let session_ready: acp_peer::process_pool::AcpSessionReadyCallback =
        Arc::new(move |native_session_id| {
            let persistence = session_persistence.clone();
            Box::pin(async move {
                persistence
                    .attach_native_session(binding_revision, native_session_id)
                    .await
                    .map(|_| ())
            })
        });
    let result = prepared
        .agent_sessions
        .run_framework_acp_turn(
            peer,
            profile,
            acp_peer::turn::AcpPeerTurnRequest {
                thread,
                history,
                turn_id: receipt.turn_id,
                native_session_id,
                input: adapter_input,
                prompt: input.prompt,
                images: input.image_inputs,
                model: model.model,
                reasoning_effort: model.reasoning_effort,
                runtime_options: target.runtime_options,
                mcp_servers,
                stream: Some(stream),
                workspace_mutations: execution.workspace_mutations,
                approval_handler: execution.approval_handler,
                control,
                persistence,
            },
            session_ready,
        )
        .await?;
    Ok(result.turn)
}

fn validate_native_input(parts: &[psychevo::AgentInputPart]) -> psychevo::Result<()> {
    let unsupported = parts.iter().find_map(|part| match part {
        psychevo::AgentInputPart::Resource { .. } => Some("resource"),
        psychevo::AgentInputPart::ResourceLink { .. } => Some("resource link"),
        psychevo::AgentInputPart::Text { .. }
        | psychevo::AgentInputPart::Image { .. }
        | psychevo::AgentInputPart::Context { .. } => None,
    });
    let Some(kind) = unsupported else {
        return Ok(());
    };
    Err(agent_session_error(
        "unsupported_input",
        AgentErrorStage::Delivery,
        "user_action",
        "not_delivered",
        format!("Psychevo (Native) Adapter does not implement {kind} input."),
        None,
    ))
}

fn lower_native_invocation_controls(
    invocation: &mut psychevo::AgentTurnInvocation,
) -> psychevo::Result<()> {
    for (control_id, value) in std::mem::take(&mut invocation.target.runtime_options) {
        match control_id.as_str() {
            "model" => invocation.model.model = Some(value),
            "reasoning" | "effort" => invocation.model.reasoning_effort = Some(value),
            "mode" => {
                invocation.execution.mode = RunMode::parse(&value).ok_or_else(|| {
                    agent_session_error(
                        "invalid_control",
                        AgentErrorStage::Control,
                        "user_action",
                        "not_delivered",
                        format!("Unknown Native mode `{value}`."),
                        None,
                    )
                })?;
            }
            "permission" | "permissionMode" => {
                invocation.execution.permission_mode =
                    Some(PermissionMode::parse(&value).ok_or_else(|| {
                        agent_session_error(
                            "invalid_control",
                            AgentErrorStage::Control,
                            "user_action",
                            "not_delivered",
                            format!("Unknown permission mode `{value}`."),
                            None,
                        )
                    })?);
            }
            _ => {
                return Err(agent_session_error(
                    "unsupported_control",
                    AgentErrorStage::Control,
                    "user_action",
                    "not_delivered",
                    format!("Psychevo (Native) does not expose control `{control_id}`."),
                    None,
                ));
            }
        }
    }
    Ok(())
}

fn framework_gateway_input(parts: Vec<psychevo::AgentInputPart>) -> Vec<GatewayInputPart> {
    parts
        .into_iter()
        .map(|part| match part {
            psychevo::AgentInputPart::Text { text } => GatewayInputPart::Text { text },
            psychevo::AgentInputPart::Image { input } => GatewayInputPart::Image {
                input: match input {
                    ImageInput::LocalPath(path) => GatewayImageInput::LocalPath {
                        path: path.display().to_string(),
                    },
                    ImageInput::ImageUrl(url) => GatewayImageInput::Url { url },
                },
            },
            psychevo::AgentInputPart::Context {
                label,
                text,
                visible_to_model,
            } => GatewayInputPart::Context {
                label,
                text,
                visible_to_model,
            },
            psychevo::AgentInputPart::Resource {
                uri,
                mime_type,
                text,
                blob,
            } => GatewayInputPart::Resource {
                uri,
                mime_type,
                text,
                blob,
            },
            psychevo::AgentInputPart::ResourceLink {
                name,
                uri,
                description,
                mime_type,
                size,
            } => GatewayInputPart::ResourceLink {
                name,
                uri,
                description,
                mime_type,
                size,
            },
        })
        .collect()
}
