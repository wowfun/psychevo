use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::agent_session::AGENT_SESSION_METADATA_KEY;
use super::{AgentBindingSnapshot, Client, Thread, ThreadSummary};
pub use crate::agents::{AgentMailboxWaitOutcome, AgentSource};
use crate::agents::{
    LoadedMainAgent, main_agent_default_metadata, main_agent_from_session_metadata,
    main_agent_metadata, session_base_agent_name_from_metadata, wait_agent_mailbox,
};
use crate::context_usage::{ContextOptions, ContextSnapshot};
use crate::run::reload_session_context;
use crate::session_export::{
    SessionExportArtifact, SessionExportOptions, SessionExportWriteResult,
};
#[cfg(test)]
use crate::state::store_agents::AgentCoordinationRunStatus;
use crate::state::store_agents::{AgentEdgeRecord, AgentEdgeStatus};
use crate::state::{
    ChildSessionRuntimeBindingSnapshotInput, ChildSessionSnapshotInput,
    GatewayRuntimeBindingOwnership, GatewayRuntimeBindingStatus,
};
use crate::types::{
    PermissionMode, ReloadContextOptions, RunMode, SessionUndoOptions, StatsOptions,
};
use crate::{Error, Result, stats, undo};

pub fn suggested_thread_title(prompt: &str) -> String {
    crate::run::normalize_session_title(prompt).unwrap_or_else(|| "New session".to_string())
}

pub type ThreadUndoResult = crate::types::SessionUndoResult;
pub type ThreadRedoResult = crate::types::SessionRedoResult;
pub type ThreadUsageSummary = crate::types::SessionUsageSummary;
pub type UsageOverview = crate::types::UsageReadResult;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsageObservation {
    pub used_tokens: Option<u64>,
    pub context_limit: Option<u64>,
    pub estimated_cost_nanodollars: Option<i64>,
}

#[derive(Debug, Clone)]
pub enum ThreadAgentBinding {
    Resolved {
        binding: Box<AgentBindingSnapshot>,
        writable: bool,
        thread_preferences: BTreeMap<String, Value>,
        runtime_observed: BTreeMap<String, Value>,
    },
    Unresolved {
        thread_id: String,
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct UpdateThreadAgentControlState {
    pub expected_binding_revision: i64,
    pub expected_control_revision: i64,
    pub thread_preferences: Option<BTreeMap<String, Value>>,
    pub runtime_observed: Option<BTreeMap<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ThreadMainAgentSelection {
    Missing { base_agent: Option<String> },
    Default { base_agent: Option<String> },
    Agent { input: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetThreadMainAgentSelection {
    Default,
    Agent {
        input: String,
        name: String,
        source: AgentSource,
        path: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadModelSelection {
    pub provider: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRelationshipStatus {
    Open,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRelationshipAgent {
    pub id: Option<String>,
    pub name: Option<String>,
    pub task_name: Option<String>,
    pub task: Option<String>,
    pub description: Option<String>,
    pub parent_tool_call_id: Option<String>,
    pub team_run_id: Option<String>,
    pub mission_run_id: Option<String>,
    pub team_name: Option<String>,
    pub team_member_id: Option<String>,
    pub runtime_ref: Option<String>,
    pub role: Option<String>,
    pub background: Option<bool>,
    pub fork_context: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRelationship {
    pub parent_thread_id: String,
    pub child_thread_id: String,
    pub status: AgentRelationshipStatus,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub agent: Option<AgentRelationshipAgent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideConversationSurface {
    Tui,
    Web,
}

impl SideConversationSurface {
    fn source(self) -> &'static str {
        match self {
            Self::Tui => crate::thread_lineage::TUI_SIDE_CONVERSATION_SESSION_SOURCE,
            Self::Web => crate::thread_lineage::WEB_SIDE_CONVERSATION_SESSION_SOURCE,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SideConversationAgentBindingSnapshot {
    parent_thread_id: String,
    expected_binding_revision: i64,
    expected_control_revision: i64,
    effective_controls: BTreeMap<String, Value>,
}

impl SideConversationAgentBindingSnapshot {
    pub fn new(
        binding: &AgentBindingSnapshot,
        effective_controls: BTreeMap<String, Value>,
    ) -> Self {
        Self {
            parent_thread_id: binding.thread_id.clone(),
            expected_binding_revision: binding.binding_revision,
            expected_control_revision: binding.control_revision,
            effective_controls,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StartSideConversationRequest {
    pub surface: SideConversationSurface,
    pub model: ThreadModelSelection,
    pub mode: RunMode,
    pub permission_mode: PermissionMode,
    pub selected_agent: Option<String>,
    pub agent_binding: Option<SideConversationAgentBindingSnapshot>,
}

#[derive(Debug, Clone)]
pub struct AutoCompactionRequest {
    pub snapshot: ContextSnapshot,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub inherited_env: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone)]
pub struct RefreshThreadContextRequest {
    pub mode: Option<RunMode>,
    pub inherited_env: Option<BTreeMap<String, String>>,
    pub agent: Option<String>,
    pub no_agents: bool,
    pub no_skills: bool,
    pub invalidation_reason: String,
    pub notice: Option<String>,
}

impl Default for RefreshThreadContextRequest {
    fn default() -> Self {
        Self {
            mode: None,
            inherited_env: None,
            agent: None,
            no_agents: false,
            no_skills: false,
            invalidation_reason: "manual_reload".to_string(),
            notice: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshThreadContextResult {
    pub thread_id: String,
    pub prefix_hash: String,
    pub version: i64,
    pub provider: String,
    pub model: String,
    pub invalidation_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UsageQuery {
    pub cwd: PathBuf,
    pub all: bool,
    pub days: Option<u64>,
    pub limit: usize,
}

impl UsageQuery {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            all: false,
            days: None,
            limit: 20,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentTeamRegistration {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub source_path: Option<String>,
    pub leader_agent_name: String,
    pub members: Value,
    pub max_parallel_agents: u64,
}

#[derive(Debug, Clone)]
pub struct AgentMissionRegistration {
    pub id: String,
    pub goal: String,
    pub lead_agent_name: String,
    pub team: Option<AgentTeamRegistration>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCoordinationStatus {
    pub team: Option<AgentTeamRunStatus>,
    pub mission: Option<AgentMissionRunStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTeamRunStatus {
    pub id: String,
    pub parent_thread_id: String,
    pub mission_run_id: Option<String>,
    pub team_name: String,
    pub description: Option<String>,
    pub source_path: Option<String>,
    pub leader_agent_name: String,
    pub members: Vec<crate::agents::AgentTeamMember>,
    pub max_parallel_agents: u64,
    pub status: String,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub final_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMissionRunStatus {
    pub id: String,
    pub parent_thread_id: String,
    pub team_run_id: Option<String>,
    pub team_name: Option<String>,
    pub goal: String,
    pub lead_agent_name: String,
    pub status: String,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub final_summary: Option<String>,
}

impl Client {
    pub async fn agent_thread_by_native_session(
        &self,
        runtime_profile: &str,
        native_session_id: &str,
    ) -> Result<Option<Thread>> {
        self.ensure_open()?;
        let runtime_profile = required_trimmed(
            runtime_profile.to_string(),
            "Agent native-session Runtime Profile",
        )?;
        let native_session_id =
            required_trimmed(native_session_id.to_string(), "Agent native session id")?;
        Ok(self
            .inner
            .state
            .gateway_runtime_binding_by_native_session(&runtime_profile, &native_session_id)
            .await?
            .map(|binding| Thread {
                client: self.clone(),
                id: binding.thread_id,
            }))
    }

    /// Return the bounded process-wide usage projection owned by this Application.
    pub async fn usage_overview(&self, activity_days: usize) -> Result<UsageOverview> {
        self.ensure_open()?;
        if activity_days == 0 {
            return Err(Error::Message(
                "usage activity days must be greater than zero".to_string(),
            ));
        }
        stats::usage_read(crate::types::UsageReadOptions {
            state: self.inner.state.clone(),
            activity_days,
        })
        .await
    }

    pub async fn usage(&self, query: UsageQuery) -> Result<Value> {
        self.ensure_open()?;
        stats::usage_stats(StatsOptions {
            state: self.inner.state.clone(),
            cwd: query.cwd,
            all: query.all,
            days: query.days,
            limit: query.limit,
        })
        .await
    }

    pub async fn agent_relationship(&self, target: &str) -> Result<Option<AgentRelationship>> {
        self.ensure_open()?;
        self.inner
            .state
            .find_agent_edge(target)
            .await
            .map(|edge| edge.map(agent_relationship))
    }

    /// Read one Thread's typed Agent binding without exposing the persistence
    /// record or performing a redundant Thread-summary lookup.
    pub async fn thread_agent_binding(
        &self,
        thread_id: &str,
    ) -> Result<Option<ThreadAgentBinding>> {
        self.ensure_open()?;
        self.inner
            .state
            .gateway_runtime_binding(thread_id)
            .await?
            .map(thread_agent_binding_from_record)
            .transpose()
    }

    pub async fn update_thread_agent_control_state(
        &self,
        thread_id: &str,
        update: UpdateThreadAgentControlState,
    ) -> Result<ThreadAgentBinding> {
        self.ensure_open()?;
        let record = self
            .inner
            .state
            .compare_and_set_gateway_runtime_control_state(
                thread_id,
                update.expected_binding_revision,
                update.expected_control_revision,
                crate::state::GatewayRuntimeControlStatePatch {
                    thread_preferences: update.thread_preferences.as_ref(),
                    runtime_observed: update.runtime_observed.as_ref(),
                },
            )
            .await?;
        thread_agent_binding_from_record(record)
    }

    /// Read the opaque outbound-Agent session projection for one Thread.
    pub async fn thread_agent_session_projection(&self, thread_id: &str) -> Result<Option<Value>> {
        self.ensure_open()?;
        let Some(metadata) = self.inner.state.session_metadata(thread_id).await? else {
            return Ok(None);
        };
        let Some(agent_session) = metadata.get(AGENT_SESSION_METADATA_KEY) else {
            return Ok(None);
        };
        let object = agent_session.as_object().ok_or_else(|| {
            Error::structured(
                "Persisted Agent session metadata is invalid.",
                json!({
                    "kind": "corrupt_agent_session_metadata",
                    "threadId": thread_id,
                }),
            )
        })?;
        Ok(object.get("sessionProjection").cloned())
    }

    pub async fn thread_summary(&self, thread_id: &str) -> Result<Option<ThreadSummary>> {
        self.ensure_open()?;
        self.inner
            .state
            .session_summary(thread_id)
            .await
            .map(|summary| summary.map(|summary| self.summary_from_summary(summary)))
    }

    pub async fn thread_summaries(
        &self,
        thread_ids: &[String],
    ) -> Result<BTreeMap<String, ThreadSummary>> {
        self.ensure_open()?;
        self.inner
            .state
            .session_summaries_by_ids(thread_ids)
            .await
            .map(|summaries| {
                summaries
                    .into_iter()
                    .map(|summary| {
                        let summary = self.summary_from_summary(summary);
                        (summary.id.clone(), summary)
                    })
                    .collect()
            })
    }

    pub async fn thread_model_selection(
        &self,
        thread_id: &str,
    ) -> Result<Option<ThreadModelSelection>> {
        self.ensure_open()?;
        self.inner
            .state
            .session_composer_model_selection(thread_id)
            .await
            .map(|selection| {
                selection.map(|(provider, model, reasoning_effort)| ThreadModelSelection {
                    provider,
                    model,
                    reasoning_effort: crate::model_state::normalize_reasoning_effort(
                        reasoning_effort,
                    ),
                })
            })
    }

    pub async fn cleanup_side_conversations(
        &self,
        cwd: impl AsRef<std::path::Path>,
        surface: SideConversationSurface,
    ) -> Result<usize> {
        self.ensure_open()?;
        let cwd = crate::paths::canonicalize_cwd(cwd.as_ref())?;
        self.inner
            .state
            .delete_sessions_for_cwd_with_source(&cwd, surface.source())
            .await
    }
}

impl Thread {
    pub async fn agent_relationship(&self) -> Result<Option<AgentRelationship>> {
        self.client.agent_relationship(&self.id).await
    }

    pub async fn agent_children(&self) -> Result<Vec<AgentRelationship>> {
        self.client.ensure_open()?;
        self.client
            .inner
            .state
            .list_agent_edges_for_parent(&self.id)
            .await
            .map(|edges| edges.into_iter().map(agent_relationship).collect())
    }

    pub async fn agent_children_matching(
        &self,
        candidates: &[String],
    ) -> Result<Vec<AgentRelationship>> {
        self.client.ensure_open()?;
        self.client
            .inner
            .state
            .list_agent_edges_for_parent_candidates(&self.id, candidates)
            .await
            .map(|edges| edges.into_iter().map(agent_relationship).collect())
    }

    pub async fn wait_for_agent_mailbox(
        &self,
        timeout: std::time::Duration,
    ) -> Result<AgentMailboxWaitOutcome> {
        self.client.ensure_open()?;
        wait_agent_mailbox(&self.id, timeout, &self.client.inner.state).await
    }

    pub async fn start_side_conversation(
        &self,
        request: StartSideConversationRequest,
    ) -> Result<Thread> {
        let thread = self.clone();
        let runtime = thread.client.inner.runtime.clone();
        let admission = runtime.begin_admission().await?;
        let reservation = runtime.reserve_application_operation()?;
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
        runtime.spawn(async move {
            drop(admission);
            let result = async {
                let _reservation = reservation.acquire().await?;
                thread.create_side_conversation(request).await
            }
            .await;
            let _ = result_tx.send(result);
        });
        result_rx.await.map_err(|_| {
            Error::Message("accepted side-conversation creation ended without a result".to_string())
        })?
    }

    async fn create_side_conversation(
        &self,
        request: StartSideConversationRequest,
    ) -> Result<Thread> {
        let summary = self.summary().await?;
        let provider = required_trimmed(request.model.provider, "side conversation provider")?;
        let model = required_trimmed(request.model.model, "side conversation model")?;
        let reasoning_effort = request
            .model
            .reasoning_effort
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let selected_agent = request
            .selected_agent
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let runtime_binding = request
            .agent_binding
            .as_ref()
            .map(|binding| {
                if binding.parent_thread_id != self.id {
                    return Err(Error::Message(format!(
                        "side conversation binding snapshot belongs to Thread `{}`, not `{}`",
                        binding.parent_thread_id, self.id
                    )));
                }
                Ok(ChildSessionRuntimeBindingSnapshotInput {
                    expected_binding_revision: binding.expected_binding_revision,
                    expected_control_revision: binding.expected_control_revision,
                    effective_controls: &binding.effective_controls,
                })
            })
            .transpose()?;
        let child_id = self
            .client
            .inner
            .state
            .create_child_session_from_parent_snapshot(ChildSessionSnapshotInput {
                parent_session_id: &self.id,
                cwd: std::path::Path::new(&summary.cwd),
                source: request.surface.source(),
                model: &model,
                provider: &provider,
                metadata: Some(json!({
                    crate::thread_lineage::SIDE_CONVERSATION_METADATA_KEY: {
                        "ephemeral": true,
                        "parent_session_id": self.id,
                    },
                    "provider_label": provider,
                    "reasoning_effort": reasoning_effort,
                    "mode": request.mode.as_str(),
                    "permission_mode": request.permission_mode.as_str(),
                    "selected_agent": selected_agent,
                })),
                inherited_message_metadata: json!({
                    crate::thread_lineage::SIDE_INHERITED_METADATA_KEY: {
                        "hidden": true,
                        "parent_session_id": self.id,
                    }
                }),
                boundary_text: crate::prompt_templates::side_conversation_boundary_prompt(),
                runtime_binding,
            })
            .await?;
        Ok(Thread {
            client: self.client.clone(),
            id: child_id,
        })
    }

    pub async fn auto_compaction_due(&self, request: AutoCompactionRequest) -> Result<bool> {
        self.client.ensure_open()?;
        let summary = self.summary().await?;
        if crate::thread_lineage::side_conversation_session_source(&summary.source) {
            return Ok(false);
        }
        crate::compaction::auto_compaction_due_for_snapshot(
            &crate::compaction::AutoCompactionCheckOptions {
                state: self.client.inner.state.clone(),
                cwd: PathBuf::from(summary.cwd),
                session: self.id.clone(),
                config_path: self.client.inner.config_path.clone(),
                model: request.model,
                reasoning_effort: request.reasoning_effort,
                inherited_env: Some(self.client.application_environment(request.inherited_env)),
            },
            &request.snapshot,
        )
    }

    pub async fn set_title(&self, title: impl AsRef<str>) -> Result<String> {
        self.client.ensure_open()?;
        self.client
            .inner
            .state
            .set_session_title(&self.id, title.as_ref())
            .await
    }

    pub async fn refresh_context(
        &self,
        request: RefreshThreadContextRequest,
    ) -> Result<RefreshThreadContextResult> {
        self.client.ensure_open()?;
        let result = reload_session_context(ReloadContextOptions {
            state: self.client.inner.state.clone(),
            session: self.id.clone(),
            config_path: self.client.inner.config_path.clone(),
            mode: request.mode,
            inherited_env: Some(self.client.application_environment(request.inherited_env)),
            agent: request.agent,
            no_agents: request.no_agents,
            no_skills: request.no_skills,
            invalidation_reason: request.invalidation_reason,
            notice: request.notice,
        })
        .await?;
        Ok(RefreshThreadContextResult {
            thread_id: result.session_id,
            prefix_hash: result.prefix_hash,
            version: result.version,
            provider: result.provider,
            model: result.model,
            invalidation_reason: result.invalidation_reason,
        })
    }

    pub async fn undo(&self) -> Result<ThreadUndoResult> {
        let summary = self.summary().await?;
        undo::undo_session(SessionUndoOptions {
            state: self.client.inner.state.clone(),
            cwd: PathBuf::from(summary.cwd),
            snapshot_root: self.client.inner.home.join("snapshots"),
            session_id: self.id.clone(),
        })
        .await
    }

    pub async fn redo(&self) -> Result<ThreadRedoResult> {
        let summary = self.summary().await?;
        undo::redo_session(SessionUndoOptions {
            state: self.client.inner.state.clone(),
            cwd: PathBuf::from(summary.cwd),
            snapshot_root: self.client.inner.home.join("snapshots"),
            session_id: self.id.clone(),
        })
        .await
    }

    pub async fn context_snapshot(
        &self,
        inherited_env: Option<BTreeMap<String, String>>,
    ) -> Result<ContextSnapshot> {
        let summary = self.summary().await?;
        crate::context_usage::context_snapshot(ContextOptions {
            state: self.client.inner.state.clone(),
            cwd: PathBuf::from(summary.cwd),
            session: self.id.clone(),
            config_path: self.client.inner.config_path.clone(),
            inherited_env: Some(self.client.application_environment(inherited_env)),
        })
        .await
    }

    /// Return the persisted usage/accounting projection for this Thread.
    pub async fn usage_summary(&self) -> Result<ThreadUsageSummary> {
        self.client.ensure_open()?;
        crate::stats::session_usage_summary(crate::types::SessionUsageOptions {
            state: self.client.inner.state.clone(),
            session_id: self.id.clone(),
        })
        .await
    }

    pub async fn trace(
        &self,
        after_seq: Option<u64>,
        limit: Option<usize>,
    ) -> Result<crate::session_trace::SessionTraceReadResult> {
        self.summary().await?;
        let state = self.client.inner.state.clone();
        let thread_id = self.id.clone();
        tokio::task::spawn_blocking(move || {
            state.read_session_trace(
                &thread_id,
                crate::session_trace::SessionTraceReadOptions { after_seq, limit },
            )
        })
        .await
        .map_err(|error| Error::Message(format!("Thread trace read task failed: {error}")))
    }

    /// Return the latest bounded usage observation emitted by an external Agent.
    pub async fn agent_usage_observation(&self) -> Result<Option<AgentUsageObservation>> {
        self.client.ensure_open()?;
        let metadata = self.client.inner.state.session_metadata(&self.id).await?;
        Ok(metadata.as_ref().and_then(agent_usage_observation))
    }

    pub async fn agent_binding(&self) -> Result<Option<ThreadAgentBinding>> {
        self.client.thread_agent_binding(&self.id).await
    }

    pub async fn update_agent_control_state(
        &self,
        update: UpdateThreadAgentControlState,
    ) -> Result<ThreadAgentBinding> {
        self.client
            .update_thread_agent_control_state(&self.id, update)
            .await
    }

    /// Read the persisted outbound-Agent session projection without exposing
    /// Thread metadata or the state runtime. The owning Adapter validates the
    /// opaque projection schema.
    pub async fn agent_session_projection(&self) -> Result<Option<Value>> {
        self.client.thread_agent_session_projection(&self.id).await
    }

    pub async fn context_limit_with_parent_fallback(&self) -> Result<Option<u64>> {
        self.client.ensure_open()?;
        let metadata = self.client.inner.state.session_metadata(&self.id).await?;
        if let Some(limit) = metadata.as_ref().and_then(thread_context_limit) {
            return Ok(Some(limit));
        }
        let Some(edge) = self.client.inner.state.find_agent_edge(&self.id).await? else {
            return Ok(None);
        };
        let parent_metadata = self
            .client
            .inner
            .state
            .session_metadata(&edge.parent_session_id)
            .await?;
        Ok(parent_metadata.as_ref().and_then(thread_context_limit))
    }

    pub async fn main_agent_selection(&self) -> Result<ThreadMainAgentSelection> {
        self.client.ensure_open()?;
        let metadata = self.client.inner.state.session_metadata(&self.id).await?;
        Ok(thread_main_agent_selection(metadata.as_ref()))
    }

    pub async fn set_main_agent_selection(
        &self,
        selection: SetThreadMainAgentSelection,
    ) -> Result<ThreadMainAgentSelection> {
        self.client.ensure_open()?;
        let current_metadata = self.client.inner.state.session_metadata(&self.id).await?;
        let base_agent = session_base_agent_name_from_metadata(current_metadata.as_ref());
        let (metadata, selected) = match selection {
            SetThreadMainAgentSelection::Default => (
                main_agent_default_metadata(),
                ThreadMainAgentSelection::Default { base_agent },
            ),
            SetThreadMainAgentSelection::Agent {
                input,
                name,
                source,
                path,
            } => {
                let input = required_trimmed(input, "main Agent input")?;
                let name = required_trimmed(name, "main Agent name")?;
                (
                    main_agent_metadata(&input, &name, source, path.as_ref()),
                    ThreadMainAgentSelection::Agent { input },
                )
            }
        };
        self.client
            .inner
            .state
            .set_session_metadata_field(
                &self.id,
                crate::agents::SESSION_MAIN_AGENT_METADATA_KEY,
                Some(metadata),
            )
            .await?;
        Ok(selected)
    }

    pub async fn set_model_selection(
        &self,
        selection: ThreadModelSelection,
    ) -> Result<ThreadModelSelection> {
        self.client.ensure_open()?;
        let provider = required_trimmed(selection.provider, "model provider")?;
        let model = required_trimmed(selection.model, "model id")?;
        let reasoning_effort = crate::config::config_parse::validate_reasoning_effort(
            selection
                .reasoning_effort
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        )?;
        self.client
            .inner
            .state
            .set_session_composer_model(&self.id, &provider, &model, reasoning_effort.as_deref())
            .await?;
        Ok(ThreadModelSelection {
            provider,
            model,
            reasoning_effort,
        })
    }

    pub async fn model_selection(&self) -> Result<Option<ThreadModelSelection>> {
        self.client.thread_model_selection(&self.id).await
    }

    pub async fn write_export(
        &self,
        path: impl Into<PathBuf>,
        options: SessionExportOptions,
    ) -> Result<SessionExportWriteResult> {
        self.client.ensure_open()?;
        crate::session_export::write_session_export(
            &self.client.inner.state,
            &self.id,
            &path.into(),
            options,
        )
        .await
    }

    pub async fn render_export(
        &self,
        options: SessionExportOptions,
    ) -> Result<SessionExportArtifact> {
        self.client.ensure_open()?;
        crate::session_export::render_session_export(&self.client.inner.state, &self.id, options)
            .await
    }

    pub async fn register_agent_mission(&self, request: AgentMissionRegistration) -> Result<()> {
        self.client.ensure_open()?;
        self.client
            .inner
            .state
            .register_agent_mission(&self.id, &request)
            .await
    }

    pub async fn agent_coordination_status(&self) -> Result<AgentCoordinationStatus> {
        self.client.ensure_open()?;
        let (team, mission) = self
            .client
            .inner
            .state
            .current_agent_coordination_runs(&self.id)
            .await?;
        let team = team
            .map(|record| {
                let members = serde_json::from_value(record.members).map_err(|error| {
                    Error::structured(
                        format!(
                            "Agent team run `{}` has invalid members: {error}",
                            record.id
                        ),
                        json!({
                            "kind": "corrupt_agent_team_members",
                            "threadId": self.id,
                            "teamRunId": record.id,
                        }),
                    )
                })?;
                Ok::<_, Error>(AgentTeamRunStatus {
                    id: record.id,
                    parent_thread_id: record.parent_session_id,
                    mission_run_id: record.mission_run_id,
                    team_name: record.team_name,
                    description: record.description,
                    source_path: record.source_path,
                    leader_agent_name: record.leader_agent_name,
                    members,
                    max_parallel_agents: record.max_parallel_agents,
                    status: record.status.as_str().to_string(),
                    started_at_ms: record.started_at_ms,
                    ended_at_ms: record.ended_at_ms,
                    final_summary: record.final_summary,
                })
            })
            .transpose()?;
        let mission = mission.map(|record| AgentMissionRunStatus {
            id: record.id,
            parent_thread_id: record.parent_session_id,
            team_run_id: record.team_run_id,
            team_name: record.team_name,
            goal: record.goal,
            lead_agent_name: record.lead_agent_name,
            status: record.status.as_str().to_string(),
            started_at_ms: record.started_at_ms,
            ended_at_ms: record.ended_at_ms,
            final_summary: record.final_summary,
        });
        Ok(AgentCoordinationStatus { team, mission })
    }

    /// Read only this Thread's bounded administration summary.
    pub async fn summary(&self) -> Result<ThreadSummary> {
        self.client
            .thread_summary(&self.id)
            .await?
            .ok_or_else(|| crate::Error::Message(format!("thread not found: {}", self.id)))
    }
}

fn thread_agent_binding_from_record(
    record: crate::state::GatewayRuntimeBindingRecord,
) -> Result<ThreadAgentBinding> {
    match record.status {
        GatewayRuntimeBindingStatus::Resolved => {
            let writable = record.ownership == GatewayRuntimeBindingOwnership::ReadWrite;
            let thread_preferences = record.thread_preferences.clone();
            let runtime_observed = record.runtime_observed.clone();
            Ok(ThreadAgentBinding::Resolved {
                binding: Box::new(AgentBindingSnapshot::try_from(record)?),
                writable,
                thread_preferences,
                runtime_observed,
            })
        }
        GatewayRuntimeBindingStatus::Unresolved => Ok(ThreadAgentBinding::Unresolved {
            thread_id: record.thread_id,
            reason: record.unresolved_reason,
        }),
    }
}

fn agent_usage_observation(metadata: &Value) -> Option<AgentUsageObservation> {
    let agent = metadata.get(AGENT_SESSION_METADATA_KEY)?;
    if agent.get("backendKind").and_then(Value::as_str) != Some("acp") {
        return None;
    }
    let usage = agent.get("usageUpdate");
    Some(AgentUsageObservation {
        used_tokens: usage
            .and_then(|usage| usage.get("used"))
            .and_then(nonnegative_u64),
        context_limit: usage
            .and_then(|usage| usage.get("size"))
            .and_then(nonnegative_u64),
        estimated_cost_nanodollars: usd_cost_nanodollars(usage.and_then(|usage| usage.get("cost"))),
    })
}

fn nonnegative_u64(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        value
            .as_f64()
            .filter(|number| number.is_finite() && *number >= 0.0)
            .map(|number| number as u64)
    })
}

fn usd_cost_nanodollars(cost: Option<&Value>) -> Option<i64> {
    let cost = cost?;
    let amount = cost
        .get("amount")?
        .as_f64()
        .filter(|amount| amount.is_finite() && *amount >= 0.0)?;
    let currency = cost
        .get("currency")
        .and_then(Value::as_str)
        .unwrap_or("USD");
    currency
        .eq_ignore_ascii_case("USD")
        .then(|| (amount * 1_000_000_000.0).round() as i64)
}

fn agent_relationship(edge: AgentEdgeRecord) -> AgentRelationship {
    let agent = edge
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("agent"))
        .and_then(Value::as_object)
        .map(|agent| AgentRelationshipAgent {
            id: agent_string(agent, "id"),
            name: agent_string(agent, "agent_type").or_else(|| agent_string(agent, "name")),
            task_name: agent_string(agent, "task_name"),
            task: agent_string(agent, "task").or_else(|| agent_string(agent, "message")),
            description: agent_string(agent, "description"),
            parent_tool_call_id: agent_string(agent, "parent_tool_call_id"),
            team_run_id: agent_string(agent, "team_run_id"),
            mission_run_id: agent_string(agent, "mission_run_id"),
            team_name: agent_string(agent, "team_name"),
            team_member_id: agent_string(agent, "team_member_id"),
            runtime_ref: agent_string(agent, "runtime_ref"),
            role: agent_string(agent, "role"),
            background: agent.get("background").and_then(Value::as_bool),
            fork_context: agent.get("fork_context").and_then(Value::as_bool),
        });
    AgentRelationship {
        parent_thread_id: edge.parent_session_id,
        child_thread_id: edge.child_session_id,
        status: match edge.status {
            AgentEdgeStatus::Open => AgentRelationshipStatus::Open,
            AgentEdgeStatus::Closed => AgentRelationshipStatus::Closed,
        },
        created_at_ms: edge.created_at_ms,
        updated_at_ms: edge.updated_at_ms,
        agent,
    }
}

fn agent_string(agent: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    agent
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn thread_context_limit(metadata: &Value) -> Option<u64> {
    metadata.get("context_limit").and_then(Value::as_u64)
}

fn thread_main_agent_selection(metadata: Option<&Value>) -> ThreadMainAgentSelection {
    let base_agent = session_base_agent_name_from_metadata(metadata);
    match main_agent_from_session_metadata(metadata) {
        LoadedMainAgent::Missing => ThreadMainAgentSelection::Missing { base_agent },
        LoadedMainAgent::Default => ThreadMainAgentSelection::Default { base_agent },
        LoadedMainAgent::Agent(input) => ThreadMainAgentSelection::Agent { input },
    }
}

fn required_trimmed(value: String, label: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::Error::Message(format!("{label} is required")));
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::application::{Application, StartThreadRequest, ThreadItem, user_text_message};
    use crate::context_usage::{ContextScope, ContextTokenizer, ContextTotal};
    use crate::state::GatewayRuntimeBindingInput;

    #[test]
    fn agent_usage_observation_is_typed_and_external_agent_scoped() {
        let observation = agent_usage_observation(&json!({
            "peer_agent": {
                "backendKind": "acp",
                "usageUpdate": {
                    "used": 12.9,
                    "size": 128,
                    "cost": {"amount": 0.0025, "currency": "usd"}
                }
            }
        }))
        .expect("typed Agent usage");
        assert_eq!(
            observation,
            AgentUsageObservation {
                used_tokens: Some(12),
                context_limit: Some(128),
                estimated_cost_nanodollars: Some(2_500_000),
            }
        );
        assert_eq!(
            agent_usage_observation(&json!({
                "peer_agent": {"backendKind": "acp"}
            })),
            Some(AgentUsageObservation {
                used_tokens: None,
                context_limit: None,
                estimated_cost_nanodollars: None,
            })
        );
        assert!(
            agent_usage_observation(&json!({
                "peer_agent": {
                    "backendKind": "native",
                    "usageUpdate": {"used": 12, "size": 128}
                }
            }))
            .is_none()
        );
    }

    #[tokio::test]
    async fn usage_overview_owns_validation_and_bounded_windows() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .build()
            .await
            .expect("application");
        let client = application.client();
        assert!(
            client
                .usage_overview(0)
                .await
                .expect_err("zero activity days")
                .to_string()
                .contains("greater than zero")
        );
        let overview = client.usage_overview(7).await.expect("usage overview");
        assert_eq!(
            overview
                .windows
                .iter()
                .map(|window| window.id.as_str())
                .collect::<Vec<_>>(),
            ["all", "30d", "7d"]
        );
        assert_eq!(overview.activity.days.len(), 7);
        application
            .shutdown()
            .await
            .expect("shutdown")
            .require_clean()
            .expect("clean shutdown");
    }

    #[tokio::test]
    async fn thread_trace_read_is_bounded_by_the_thread_owner() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(temp.path().join("state.db"))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let trace = thread.trace(None, Some(1)).await.expect("trace");
        assert_eq!(trace.thread_id, thread.id());
        assert!(!trace.available);
        assert!(trace.events.is_empty());
        application
            .shutdown()
            .await
            .expect("shutdown")
            .require_clean()
            .expect("clean shutdown");
    }

    #[tokio::test]
    async fn thread_agent_binding_hides_the_durable_record_shape() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let cwd = temp.path().to_string_lossy();
        application
            .inner
            .state
            .create_gateway_runtime_binding(GatewayRuntimeBindingInput {
                thread_id: thread.id(),
                agent_ref: Some("reviewer"),
                agent_fingerprint: "agent-fingerprint",
                agent_definition_json: r#"{"name":"reviewer"}"#,
                runtime_ref: "runtime:reviewer",
                backend_kind: "acp",
                native_kind: "acp",
                native_session_id: Some("native-1"),
                cwd: &cwd,
                profile_fingerprint: "profile-fingerprint",
                profile_revision: "profile-revision",
                profile_config_json: "{}",
                adapter_kind: "acp",
                adapter_revision: "adapter-revision",
                ownership: GatewayRuntimeBindingOwnership::ReadWrite,
                parent_thread_id: None,
            })
            .await
            .expect("binding");

        let binding = thread
            .agent_binding()
            .await
            .expect("binding read")
            .expect("binding state");
        let ThreadAgentBinding::Resolved {
            binding,
            writable,
            thread_preferences,
            runtime_observed,
        } = binding
        else {
            panic!("binding must be resolved")
        };
        assert!(writable);
        assert!(thread_preferences.is_empty());
        assert!(runtime_observed.is_empty());
        assert_eq!(binding.runtime_ref, "runtime:reviewer");
        assert_eq!(binding.native_session_id.as_deref(), Some("native-1"));

        let preferences = BTreeMap::from([("model".to_string(), json!("fast"))]);
        let observed = BTreeMap::from([("mode".to_string(), json!("review"))]);
        let updated = thread
            .update_agent_control_state(UpdateThreadAgentControlState {
                expected_binding_revision: binding.binding_revision,
                expected_control_revision: binding.control_revision,
                thread_preferences: Some(preferences.clone()),
                runtime_observed: Some(observed.clone()),
            })
            .await
            .expect("control update");
        let ThreadAgentBinding::Resolved {
            binding: updated_binding,
            thread_preferences,
            runtime_observed,
            ..
        } = updated
        else {
            panic!("updated binding must remain resolved")
        };
        assert_eq!(updated_binding.binding_revision, binding.binding_revision);
        assert_eq!(
            updated_binding.control_revision,
            binding.control_revision + 1
        );
        assert_eq!(thread_preferences, preferences);
        assert_eq!(runtime_observed, observed);

        let effective_controls = BTreeMap::from([
            ("mode".to_string(), json!("plan")),
            ("model".to_string(), json!("live-acp-model")),
        ]);
        let side = thread
            .start_side_conversation(StartSideConversationRequest {
                surface: SideConversationSurface::Web,
                model: ThreadModelSelection {
                    provider: "fake".to_string(),
                    model: "live-acp-model".to_string(),
                    reasoning_effort: None,
                },
                mode: RunMode::Plan,
                permission_mode: PermissionMode::Default,
                selected_agent: Some("reviewer".to_string()),
                agent_binding: Some(SideConversationAgentBindingSnapshot::new(
                    &updated_binding,
                    effective_controls.clone(),
                )),
            })
            .await
            .expect("bound side conversation");
        let side_binding = side
            .agent_binding()
            .await
            .expect("side binding read")
            .expect("side binding");
        let ThreadAgentBinding::Resolved {
            binding: side_binding,
            thread_preferences,
            runtime_observed,
            ..
        } = side_binding
        else {
            panic!("side binding must be resolved")
        };
        assert_eq!(thread_preferences, effective_controls);
        assert!(runtime_observed.is_empty());
        assert_eq!(side_binding.native_session_id, None);

        let stale = thread
            .update_agent_control_state(UpdateThreadAgentControlState {
                expected_binding_revision: binding.binding_revision,
                expected_control_revision: binding.control_revision,
                thread_preferences: Some(BTreeMap::new()),
                runtime_observed: None,
            })
            .await
            .expect_err("stale control revision must fail");
        assert!(stale.to_string().contains("control revision"));
        application
            .shutdown()
            .await
            .expect("shutdown")
            .require_clean()
            .expect("clean shutdown");
    }

    #[tokio::test]
    async fn agent_children_matching_preserves_thread_administration_ownership() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let cwd = temp.path().join("workspace");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let application = Application::builder()
            .home(&home)
            .build()
            .await
            .expect("application");
        let client = application.client();
        let parent = client
            .start_thread(StartThreadRequest::new(&cwd))
            .await
            .expect("parent");
        let child = client
            .start_thread(StartThreadRequest::new(&cwd))
            .await
            .expect("child");
        client
            .inner
            .state
            .upsert_agent_edge(
                parent.id(),
                child.id(),
                AgentEdgeStatus::Open,
                Some(json!({
                    "agent": {
                        "id": "agent-1",
                        "name": "reviewer",
                        "task_name": "review",
                        "task": "Review the patch",
                        "parent_tool_call_id": "call-1",
                        "team_run_id": "team-1",
                        "team_name": "reviewers",
                        "background": true,
                    }
                })),
            )
            .await
            .expect("edge");

        let relationship = client
            .agent_relationship("review")
            .await
            .expect("relationship")
            .expect("found");
        assert_eq!(relationship.parent_thread_id, parent.id());
        assert_eq!(relationship.child_thread_id, child.id());
        assert_eq!(relationship.status, AgentRelationshipStatus::Open);
        assert_eq!(
            relationship
                .agent
                .as_ref()
                .and_then(|agent| agent.name.as_deref()),
            Some("reviewer")
        );
        assert_eq!(
            parent.agent_children().await.expect("children"),
            vec![relationship.clone()]
        );
        assert_eq!(
            parent
                .agent_children_matching(&["missing".to_string(), "call-1".to_string()])
                .await
                .expect("matching children"),
            vec![relationship]
        );
        assert!(
            parent
                .agent_children_matching(&[])
                .await
                .expect("empty candidate set")
                .is_empty()
        );

        client
            .inner
            .state
            .append_agent_mailbox_event(crate::state::AgentMailboxEventInput {
                parent_session_id: parent.id().to_string(),
                child_session_id: Some(child.id().to_string()),
                agent_id: "agent-1".to_string(),
                task_name: Some("review".to_string()),
                agent_name: "reviewer".to_string(),
                content_text: "private child result".to_string(),
                payload: json!({"private": true}),
                metadata: None,
            })
            .await
            .expect("mailbox event");
        assert_eq!(
            parent
                .wait_for_agent_mailbox(std::time::Duration::ZERO)
                .await
                .expect("mailbox ready"),
            AgentMailboxWaitOutcome::Ready
        );
        assert_eq!(
            parent
                .wait_for_agent_mailbox(std::time::Duration::ZERO)
                .await
                .expect("mailbox remains ready"),
            AgentMailboxWaitOutcome::Ready,
            "the administration observation must not claim or consume the event"
        );
        assert_eq!(
            child
                .wait_for_agent_mailbox(std::time::Duration::ZERO)
                .await
                .expect("empty mailbox"),
            AgentMailboxWaitOutcome::TimedOut
        );

        client
            .inner
            .state
            .append_message(parent.id(), &user_text_message("parent context"))
            .await
            .expect("parent message");
        let side = parent
            .start_side_conversation(StartSideConversationRequest {
                surface: SideConversationSurface::Tui,
                model: ThreadModelSelection {
                    provider: "fake".to_string(),
                    model: "fake-model".to_string(),
                    reasoning_effort: Some("high".to_string()),
                },
                mode: RunMode::Default,
                permission_mode: PermissionMode::Default,
                selected_agent: Some("reviewer".to_string()),
                agent_binding: None,
            })
            .await
            .expect("side conversation");
        let side_summary = side.summary().await.expect("side summary");
        assert_eq!(side_summary.parent_thread_id.as_deref(), Some(parent.id()));
        assert_eq!(
            side_summary.source,
            crate::thread_lineage::TUI_SIDE_CONVERSATION_SESSION_SOURCE
        );
        let history = side
            .history()
            .latest(Some(200))
            .await
            .expect("side history")
            .items;
        assert_eq!(history.len(), 2);
        assert!(history.iter().all(ThreadItem::is_side_inherited));

        let due = side
            .auto_compaction_due(AutoCompactionRequest {
                snapshot: ContextSnapshot {
                    event_type: "context_snapshot".to_string(),
                    scope: ContextScope::SessionEstimate,
                    status: "ok".to_string(),
                    basis: "test".to_string(),
                    applies_to_session_seq: None,
                    session_id: Some(side.id().to_string()),
                    provider: "fake".to_string(),
                    model: "fake-model".to_string(),
                    mode: None,
                    context_limit: Some(100),
                    tokenizer: ContextTokenizer {
                        encoding: "test".to_string(),
                        source: "test".to_string(),
                        fallback: false,
                    },
                    total: ContextTotal {
                        tokens: 100,
                        estimated_tokens: 0,
                        estimated: false,
                        source: "test".to_string(),
                        percent: Some(100.0),
                    },
                    categories: BTreeMap::new(),
                    advice: Vec::new(),
                },
                model: None,
                reasoning_effort: None,
                inherited_env: Some(BTreeMap::new()),
            })
            .await
            .expect("side compaction check");
        assert!(!due, "temporary side conversations never auto-compact");

        assert_eq!(
            client
                .cleanup_side_conversations(&cwd, SideConversationSurface::Tui)
                .await
                .expect("cleanup"),
            1
        );
        assert!(client.resume_thread(side.id()).await.is_err());
        application
            .shutdown()
            .await
            .expect("shutdown")
            .require_clean()
            .expect("clean shutdown");
    }

    #[tokio::test]
    async fn side_conversation_does_not_wait_for_parent_turn_fifo() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .build()
            .await
            .expect("application");
        let parent = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("parent");
        let ready = application
            .inner
            .runtime
            .reserve_turn_for_test(parent.id(), "active-parent-turn")
            .expect("turn reservation");
        ready.await.expect("active turn");

        let side = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            parent.start_side_conversation(StartSideConversationRequest {
                surface: SideConversationSurface::Tui,
                model: ThreadModelSelection {
                    provider: "fake".to_string(),
                    model: "fake-model".to_string(),
                    reasoning_effort: None,
                },
                mode: RunMode::Default,
                permission_mode: PermissionMode::Default,
                selected_agent: None,
                agent_binding: None,
            }),
        )
        .await
        .expect("side conversation must not wait for the active parent Turn")
        .expect("side conversation");

        application
            .inner
            .runtime
            .settle_turn(parent.id(), "active-parent-turn", None);
        assert_eq!(
            side.summary()
                .await
                .expect("side summary")
                .parent_thread_id
                .as_deref(),
            Some(parent.id())
        );
        application
            .shutdown()
            .await
            .expect("shutdown")
            .require_clean()
            .expect("clean shutdown");
    }

    #[tokio::test]
    async fn agent_coordination_status_selects_active_then_latest_and_rejects_corrupt_members() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("Thread");
        let members = vec![
            crate::agents::AgentTeamMember {
                id: "research".to_string(),
                agent: "researcher".to_string(),
                runtime_ref: None,
                runtime_options: BTreeMap::new(),
                runtime_profile_revision: None,
                role: Some("research".to_string()),
                description: None,
                max_turns: Some(4),
            },
            crate::agents::AgentTeamMember {
                id: "verify".to_string(),
                agent: "reviewer".to_string(),
                runtime_ref: Some("acp".to_string()),
                runtime_options: BTreeMap::from([("model".to_string(), "fast".to_string())]),
                runtime_profile_revision: Some(7),
                role: Some("verification".to_string()),
                description: Some("Verify the result".to_string()),
                max_turns: None,
            },
        ];

        for suffix in ["old", "active"] {
            thread
                .register_agent_mission(AgentMissionRegistration {
                    id: format!("mission-{suffix}"),
                    goal: format!("goal-{suffix}"),
                    lead_agent_name: "lead".to_string(),
                    team: Some(AgentTeamRegistration {
                        id: format!("team-{suffix}"),
                        name: format!("team-{suffix}"),
                        description: Some(format!("description-{suffix}")),
                        source_path: Some(format!("/teams/{suffix}.md")),
                        leader_agent_name: "lead".to_string(),
                        members: serde_json::to_value(&members).expect("members"),
                        max_parallel_agents: 2,
                    }),
                    metadata: None,
                })
                .await
                .expect("register coordination run");
            if suffix == "old" {
                application
                    .inner
                    .state
                    .update_agent_team_run_status(
                        "team-old",
                        AgentCoordinationRunStatus::Completed,
                        Some("old team"),
                        true,
                    )
                    .await
                    .expect("complete old team");
                application
                    .inner
                    .state
                    .update_agent_mission_run_status(
                        "mission-old",
                        AgentCoordinationRunStatus::Completed,
                        Some("old mission"),
                        true,
                    )
                    .await
                    .expect("complete old mission");
            }
        }

        let active = thread
            .agent_coordination_status()
            .await
            .expect("active coordination");
        assert_eq!(
            active.team.as_ref().map(|team| team.id.as_str()),
            Some("team-active")
        );
        assert_eq!(
            active
                .team
                .as_ref()
                .expect("active team")
                .members
                .iter()
                .map(|member| member.id.as_str())
                .collect::<Vec<_>>(),
            ["research", "verify"]
        );
        assert_eq!(
            active.mission.as_ref().map(|mission| mission.id.as_str()),
            Some("mission-active")
        );

        application
            .inner
            .state
            .update_agent_team_run_status(
                "team-active",
                AgentCoordinationRunStatus::Completed,
                Some("latest team"),
                true,
            )
            .await
            .expect("complete active team");
        application
            .inner
            .state
            .update_agent_mission_run_status(
                "mission-active",
                AgentCoordinationRunStatus::Completed,
                Some("latest mission"),
                true,
            )
            .await
            .expect("complete active mission");
        let mut conn = application
            .inner
            .state
            .acquire_sqlx()
            .await
            .expect("connection");
        sqlx::query(
            "UPDATE agent_team_runs SET started_at_ms = CASE id \
             WHEN 'team-old' THEN 10 ELSE 20 END WHERE parent_session_id = ?1",
        )
        .bind(thread.id())
        .execute(&mut *conn)
        .await
        .expect("team timestamps");
        sqlx::query(
            "UPDATE agent_mission_runs SET started_at_ms = CASE id \
             WHEN 'mission-old' THEN 10 ELSE 20 END WHERE parent_session_id = ?1",
        )
        .bind(thread.id())
        .execute(&mut *conn)
        .await
        .expect("mission timestamps");
        drop(conn);
        let latest = thread
            .agent_coordination_status()
            .await
            .expect("latest completed coordination");
        assert_eq!(
            latest.team.as_ref().map(|team| team.id.as_str()),
            Some("team-active")
        );
        assert_eq!(
            latest.mission.as_ref().map(|mission| mission.id.as_str()),
            Some("mission-active")
        );

        let mut conn = application
            .inner
            .state
            .acquire_sqlx()
            .await
            .expect("connection");
        sqlx::query("UPDATE agent_team_runs SET members_json = '{}' WHERE id = 'team-active'")
            .execute(&mut *conn)
            .await
            .expect("corrupt members");
        drop(conn);
        let error = thread
            .agent_coordination_status()
            .await
            .expect_err("corrupt members must not project an empty team");
        assert_eq!(
            error.structured_data().expect("structured corruption")["kind"],
            "corrupt_agent_team_members"
        );

        application
            .shutdown()
            .await
            .expect("shutdown")
            .require_clean()
            .expect("clean shutdown");
    }
}
