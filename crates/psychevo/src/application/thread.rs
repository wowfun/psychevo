use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use futures::future::BoxFuture;
use serde_json::Value;
use tokio::sync::oneshot;
use uuid::Uuid;

use super::{
    AgentBindingSnapshot, AgentMissionRegistration, AgentRemoteDeleteState, AgentThreadForkRequest,
    AgentThreadImportRequest, AgentThreadLifecycleAction, AgentThreadLifecycleOutcome,
    AgentThreadLifecycleRequest, AgentThreadLifecycleSnapshot, AgentThreadPublication,
    AgentThreadPublicationAbortRequest, AgentTurnPurpose, ApplicationActivitySnapshot, Client,
    CompactThreadRequest, DEFAULT_HISTORY_PAGE_SIZE, DEFAULT_THREAD_LIST_LIMIT,
    ForkAgentThreadRequest, ForkThreadRequest, HistoryPage, HistoryReader,
    ImportAgentThreadRequest, ImportAgentThreadResult, InitialAgentBinding,
    InitialThreadSourceAssociation, InteractionResponse, InteractionResponseReceipt,
    MAX_HISTORY_PAGE_SIZE, MAX_THREAD_LIST_LIMIT, PendingInteraction, StartThreadRequest, Thread,
    ThreadActivitySnapshot, ThreadExecutionContext, ThreadItem, ThreadListCursor, ThreadListPage,
    ThreadListQuery, ThreadSnapshot, ThreadSummary, TurnHandle, TurnRequest,
};
use crate::compaction::{CompactSessionOptions, CompactionResult};
use crate::paths::canonicalize_cwd;
use crate::state::{
    AgentThreadImportCommit, AgentThreadImportCommitInput, AgentThreadImportMessageInput,
    GatewayRuntimeBindingInput, GatewayRuntimeBindingOwnership, GatewaySourceLaneInput,
    GatewayTurnDeliveryInput, NativeSessionForkInput, NewFrameworkThreadTurnInput,
    SessionListCursor, StateRuntime,
};
use crate::types::SessionSummary;
use crate::{Error, Result};

impl Client {
    pub async fn reconcile_acknowledged_agent_deletes(&self) -> Result<usize> {
        self.ensure_open()?;
        let thread_ids = self
            .inner
            .state
            .acknowledged_agent_delete_thread_ids()
            .await?;
        let count = thread_ids.len();
        for thread_id in thread_ids {
            self.resume_thread(&thread_id).await?.delete().await?;
        }
        Ok(count)
    }

    pub async fn import_agent_thread(
        &self,
        request: ImportAgentThreadRequest,
    ) -> Result<ImportAgentThreadResult> {
        let cwd = canonicalize_cwd(&request.cwd)?;
        let source = request.source.trim().to_string();
        if source.is_empty() {
            return Err(Error::Message(
                "Agent import Thread source must not be empty".to_string(),
            ));
        }
        let id = Uuid::now_v7().to_string();
        let thread = Thread {
            client: self.clone(),
            id: id.clone(),
        };
        let context = ThreadExecutionContext {
            id: id.clone(),
            cwd: cwd.to_string_lossy().into_owned(),
            source: source.clone(),
            source_key: None,
        };
        let mcp_resolver = self.agent_mcp_server_resolver(&context);
        let runtime = self.inner.runtime.clone();
        let admission = runtime.begin_admission().await?;
        let reservation = runtime.reserve_mutation(&id)?;
        let state = self.inner.state.clone();
        let adapter = self.inner.agent_sessions.clone();
        let (result_tx, result_rx) = oneshot::channel();
        runtime.spawn(async move {
            drop(admission);
            let result = async {
                let _reservation = reservation.acquire().await?;
                let imported = adapter
                    .clone()
                    .import_thread(AgentThreadImportRequest {
                        thread: context.clone(),
                        preparation: request.preparation,
                        mcp_resolver,
                    })
                    .await?;
                let commit = commit_agent_thread_publication(
                    &state, &context, &cwd, &source, None, &imported,
                )
                .await;
                match commit {
                    Ok(AgentThreadImportCommit::Published) => Ok(ImportAgentThreadResult {
                        thread,
                        existing: false,
                    }),
                    Ok(AgentThreadImportCommit::Existing { thread_id }) => {
                        adapter
                            .abort_thread_publication(AgentThreadPublicationAbortRequest {
                                thread: context,
                                binding: imported.binding,
                            })
                            .await?;
                        Ok(ImportAgentThreadResult {
                            thread: Thread {
                                client: thread.client,
                                id: thread_id,
                            },
                            existing: true,
                        })
                    }
                    Err(error) => {
                        let abort = adapter
                            .abort_thread_publication(AgentThreadPublicationAbortRequest {
                                thread: context,
                                binding: imported.binding,
                            })
                            .await;
                        match abort {
                            Ok(()) => Err(error),
                            Err(abort_error) => Err(Error::Message(format!(
                                "{error}; Agent import release also failed: {abort_error}"
                            ))),
                        }
                    }
                }
            }
            .await;
            let _ = result_tx.send(result);
        });
        result_rx.await.map_err(|_| {
            Error::Message("accepted Agent Thread import ended without a result".to_string())
        })?
    }

    pub async fn start_thread_with_turn(
        &self,
        mut start: StartThreadRequest,
        request: TurnRequest,
    ) -> Result<TurnHandle> {
        let cwd = canonicalize_cwd(&start.cwd)?;
        let id = start
            .requested_id
            .take()
            .unwrap_or_else(|| Uuid::now_v7().to_string());
        Thread {
            client: self.clone(),
            id,
        }
        .start_turn_inner(
            request,
            Some(NewThreadAdmission {
                cwd,
                source: start.source,
                metadata: start.metadata,
                initial_source: start.initial_source,
                execution_source_key: start.execution_source_key,
                initial_binding: start.initial_binding,
                initial_thread_preferences: start.initial_thread_preferences,
            }),
            AgentTurnPurpose::Peer,
        )
        .await
    }

    pub async fn start_thread(&self, request: StartThreadRequest) -> Result<Thread> {
        let cwd = canonicalize_cwd(&request.cwd)?;
        let runtime = self.inner.runtime.clone();
        let admission = runtime.begin_admission().await?;
        let reservation = runtime.reserve_application_operation()?;
        let state = self.inner.state.clone();
        let (result_tx, result_rx) = oneshot::channel();
        runtime.spawn(async move {
            drop(admission);
            let result = async {
                let _reservation = reservation.acquire().await?;
                state
                    .create_session_with_metadata(
                        &cwd,
                        &request.source,
                        "pending",
                        "pending",
                        request.metadata,
                    )
                    .await
            }
            .await;
            let _ = result_tx.send(result);
        });
        let id = result_rx.await.map_err(|_| {
            Error::Message("accepted start_thread task ended without a result".to_string())
        })??;
        Ok(Thread {
            client: self.clone(),
            id,
        })
    }

    pub async fn resume_thread(&self, id: impl Into<String>) -> Result<Thread> {
        let id = id.into();
        self.try_resume_thread(id.clone())
            .await?
            .ok_or_else(|| Error::Message(format!("thread not found: {id}")))
    }

    pub async fn try_resume_thread(&self, id: impl Into<String>) -> Result<Option<Thread>> {
        self.ensure_open()?;
        let id = id.into();
        Ok(self
            .inner
            .state
            .session_summary(&id)
            .await?
            .map(|_| Thread {
                client: self.clone(),
                id,
            }))
    }

    pub async fn list_threads(&self, mut query: ThreadListQuery) -> Result<ThreadListPage> {
        self.ensure_open()?;
        let cwd = query
            .cwd
            .as_deref()
            .map(canonicalize_cwd)
            .transpose()?
            .map(|cwd| cwd.to_string_lossy().into_owned());
        query.sources.sort();
        query.sources.dedup();
        let cursor = query
            .cursor
            .as_deref()
            .map(|cursor| {
                decode_thread_list_cursor(cursor, cwd.as_deref(), query.archived, &query.sources)
            })
            .transpose()?;
        let page = self
            .inner
            .state
            .list_session_summary_page(
                cwd.as_deref(),
                &query.sources,
                query.archived,
                cursor.as_ref(),
                query.limit.clamp(1, MAX_THREAD_LIST_LIMIT),
            )
            .await?;
        let threads = page
            .summaries
            .into_iter()
            .map(|summary| self.summary_from_summary(summary))
            .collect();
        let next_cursor = page
            .next_cursor
            .map(|cursor| encode_thread_list_cursor(cwd, query.archived, query.sources, cursor))
            .transpose()?;
        Ok(ThreadListPage {
            threads,
            next_cursor,
        })
    }

    pub fn activity_snapshot(&self) -> ApplicationActivitySnapshot {
        self.inner.runtime.versioned_thread_activity_snapshot()
    }

    pub(super) fn summary_from_summary(&self, summary: SessionSummary) -> ThreadSummary {
        let (_, active_turn_id, _) = self.inner.runtime.thread_activity(&summary.id);
        ThreadSummary::from_summary(summary, active_turn_id)
    }

    async fn snapshot_from_summary(&self, summary: SessionSummary) -> Result<ThreadSnapshot> {
        let summary = self.summary_from_summary(summary);
        let pending_interactions = self
            .inner
            .state
            .framework_interactions_for_thread(&summary.id, true)
            .await?
            .into_iter()
            .map(PendingInteraction::from)
            .collect();
        let history = HistoryReader::new(self.inner.state.clone(), summary.id.clone())
            .latest(None)
            .await?;
        Ok(ThreadSnapshot::from_summary(
            summary,
            pending_interactions,
            history.items,
            history.next_before,
        ))
    }
}

impl StartThreadRequest {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            source: "sdk".to_string(),
            metadata: None,
            requested_id: None,
            initial_source: None,
            execution_source_key: None,
            initial_binding: None,
            initial_thread_preferences: BTreeMap::new(),
        }
    }

    pub fn with_initial_context(
        mut self,
        thread_id: String,
        source: Option<InitialThreadSourceAssociation>,
        thread_preferences: BTreeMap<String, Value>,
    ) -> Self {
        self.requested_id = Some(thread_id);
        self.initial_source = source;
        self.initial_thread_preferences = thread_preferences;
        self
    }

    pub fn with_execution_source_key(mut self, source_key: Option<String>) -> Self {
        self.execution_source_key = source_key;
        self
    }
}

pub(super) struct NewThreadAdmission {
    cwd: PathBuf,
    source: String,
    metadata: Option<Value>,
    initial_source: Option<InitialThreadSourceAssociation>,
    execution_source_key: Option<String>,
    initial_binding: Option<InitialAgentBinding>,
    initial_thread_preferences: BTreeMap<String, Value>,
}

impl NewThreadAdmission {
    pub(super) fn execution_context(&self, thread_id: &str) -> ThreadExecutionContext {
        ThreadExecutionContext {
            id: thread_id.to_string(),
            cwd: self.cwd.to_string_lossy().into_owned(),
            source: self.source.clone(),
            source_key: self.execution_source_key.clone().or_else(|| {
                self.initial_source
                    .as_ref()
                    .map(|source| source.source_key.clone())
            }),
        }
    }

    pub(super) async fn accept(
        mut self,
        state: &StateRuntime,
        delivery: GatewayTurnDeliveryInput<'_>,
        client_turn_id: Option<&str>,
        captured_binding: Option<InitialAgentBinding>,
        captured_preferences: &BTreeMap<String, String>,
        mission: Option<AgentMissionRegistration>,
    ) -> Result<()> {
        if captured_binding.is_some() {
            self.initial_binding = captured_binding;
        }
        self.initial_thread_preferences.extend(
            captured_preferences
                .iter()
                .map(|(key, value)| (key.clone(), Value::String(value.clone()))),
        );
        let empty_draft_controls = BTreeMap::new();
        let binding_cwd = self.cwd.to_string_lossy().into_owned();
        let source_lane = self
            .initial_source
            .as_ref()
            .map(|source| GatewaySourceLaneInput {
                source_key: &source.source_key,
                source_kind: &source.source_kind,
                raw_identity: source.raw_identity.clone(),
                visible_name: source.visible_name.as_deref(),
                thread_id: Some(delivery.thread_id),
                draft_agent_ref: None,
                draft_profile_ref: None,
                draft_control_values: &empty_draft_controls,
                lineage: source.lineage.clone(),
            });
        let runtime_binding =
            self.initial_binding
                .as_ref()
                .map(|binding| GatewayRuntimeBindingInput {
                    thread_id: delivery.thread_id,
                    agent_ref: binding.agent_ref.as_deref(),
                    agent_fingerprint: &binding.agent_fingerprint,
                    agent_definition_json: &binding.agent_definition_json,
                    runtime_ref: &binding.runtime_ref,
                    backend_kind: &binding.backend_kind,
                    native_kind: &binding.native_kind,
                    native_session_id: binding.native_session_id.as_deref(),
                    cwd: &binding_cwd,
                    profile_fingerprint: &binding.profile_fingerprint,
                    profile_revision: &binding.profile_revision,
                    profile_config_json: &binding.profile_config_json,
                    adapter_kind: &binding.adapter_kind,
                    adapter_revision: &binding.adapter_revision,
                    ownership: GatewayRuntimeBindingOwnership::ReadWrite,
                    parent_thread_id: None,
                });
        state
            .accept_new_framework_thread_turn(NewFrameworkThreadTurnInput {
                thread_id: delivery.thread_id,
                cwd: &self.cwd,
                source: &self.source,
                metadata: self.metadata,
                delivery,
                client_turn_id,
                source_lane,
                runtime_binding,
                initial_thread_preferences: &self.initial_thread_preferences,
                mission,
            })
            .await
    }
}

impl Default for ThreadListQuery {
    fn default() -> Self {
        Self {
            cwd: None,
            archived: false,
            sources: Vec::new(),
            cursor: None,
            limit: DEFAULT_THREAD_LIST_LIMIT,
        }
    }
}

fn encode_thread_list_cursor(
    cwd: Option<String>,
    archived: bool,
    sources: Vec<String>,
    position: SessionListCursor,
) -> Result<String> {
    let cursor = ThreadListCursor {
        cwd,
        archived,
        sources,
        position,
    };
    Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&cursor)?))
}

fn decode_thread_list_cursor(
    encoded: &str,
    cwd: Option<&str>,
    archived: bool,
    sources: &[String],
) -> Result<SessionListCursor> {
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| Error::Message("invalid thread list cursor".to_string()))?;
    let cursor = serde_json::from_slice::<ThreadListCursor>(&bytes)
        .map_err(|_| Error::Message("invalid thread list cursor".to_string()))?;
    if cursor.cwd.as_deref() != cwd || cursor.archived != archived || cursor.sources != sources {
        return Err(Error::Message(
            "thread list cursor does not match the current filters".to_string(),
        ));
    }
    Ok(cursor.position)
}

impl fmt::Debug for Thread {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Thread")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

const AGENT_SESSION_LIFECYCLE_METADATA_KEY: &str = "agentSessionLifecycle";
const AGENT_SESSION_DELETE_INTENT_METADATA_KEY: &str = "agentSessionDeleteIntent";

enum AgentThreadLifecycleMetadataWrite {
    None,
    Projection(super::AgentImportedLifecycle),
    RemoteDeletePrepared { at_ms: i64 },
    RemoteDeleteAcknowledged { at_ms: i64 },
}

pub(super) fn decode_agent_thread_lifecycle(
    metadata: Option<&Value>,
) -> Result<AgentThreadLifecycleSnapshot> {
    let projection = metadata
        .and_then(|metadata| metadata.get(AGENT_SESSION_LIFECYCLE_METADATA_KEY))
        .cloned()
        .map(serde_json::from_value)
        .transpose()?;
    let remote_delete = match metadata
        .and_then(|metadata| metadata.get(AGENT_SESSION_DELETE_INTENT_METADATA_KEY))
    {
        None => AgentRemoteDeleteState::NotRequested,
        Some(intent) => match intent.get("state").and_then(Value::as_str) {
            Some("prepared") => AgentRemoteDeleteState::Prepared {
                at_ms: required_lifecycle_timestamp(intent, "createdAtMs")?,
            },
            Some("remoteAcknowledged") => AgentRemoteDeleteState::Acknowledged {
                at_ms: required_lifecycle_timestamp(intent, "updatedAtMs")?,
            },
            _ => {
                return Err(Error::Message(
                    "Agent Thread remote-delete metadata has an invalid state".to_string(),
                ));
            }
        },
    };
    Ok(AgentThreadLifecycleSnapshot {
        projection,
        remote_delete,
    })
}

fn required_lifecycle_timestamp(value: &Value, field: &str) -> Result<i64> {
    value
        .get(field)
        .and_then(Value::as_i64)
        .filter(|at_ms| *at_ms >= 0)
        .ok_or_else(|| {
            Error::Message(format!(
                "Agent Thread lifecycle metadata has an invalid `{field}`"
            ))
        })
}

fn validate_agent_thread_lifecycle_outcome(
    action: &AgentThreadLifecycleAction,
    current: &AgentThreadLifecycleSnapshot,
    outcome: AgentThreadLifecycleOutcome,
) -> Result<(
    AgentThreadLifecycleSnapshot,
    AgentThreadLifecycleMetadataWrite,
)> {
    let mut next = current.clone();
    let write = match outcome {
        AgentThreadLifecycleOutcome::Unchanged => {
            if matches!(action, AgentThreadLifecycleAction::Delete)
                && matches!(
                    current.remote_delete,
                    AgentRemoteDeleteState::Prepared { .. }
                )
            {
                return Err(invalid_lifecycle_outcome(
                    action,
                    "a prepared remote delete requires acknowledgement",
                ));
            }
            AgentThreadLifecycleMetadataWrite::None
        }
        AgentThreadLifecycleOutcome::Projection(projection) => {
            if !matches!(action, AgentThreadLifecycleAction::Restore) {
                return Err(invalid_lifecycle_outcome(
                    action,
                    "only restore may update the Agent lifecycle projection",
                ));
            }
            next.projection = Some(projection.clone());
            AgentThreadLifecycleMetadataWrite::Projection(projection)
        }
        AgentThreadLifecycleOutcome::RemoteDeletePrepared { at_ms } => {
            if !matches!(action, AgentThreadLifecycleAction::Delete)
                || !matches!(current.remote_delete, AgentRemoteDeleteState::NotRequested)
                || at_ms < 0
            {
                return Err(invalid_lifecycle_outcome(
                    action,
                    "remote delete can only be prepared once from its initial state",
                ));
            }
            next.remote_delete = AgentRemoteDeleteState::Prepared { at_ms };
            AgentThreadLifecycleMetadataWrite::RemoteDeletePrepared { at_ms }
        }
        AgentThreadLifecycleOutcome::RemoteDeleteAcknowledged { at_ms } => {
            if !matches!(action, AgentThreadLifecycleAction::Delete)
                || !matches!(
                    current.remote_delete,
                    AgentRemoteDeleteState::Prepared { .. }
                )
                || at_ms < 0
            {
                return Err(invalid_lifecycle_outcome(
                    action,
                    "remote delete acknowledgement requires a persisted preparation",
                ));
            }
            next.remote_delete = AgentRemoteDeleteState::Acknowledged { at_ms };
            AgentThreadLifecycleMetadataWrite::RemoteDeleteAcknowledged { at_ms }
        }
    };
    Ok((next, write))
}

fn invalid_lifecycle_outcome(action: &AgentThreadLifecycleAction, reason: &str) -> Error {
    Error::Message(format!(
        "Agent Session Adapter returned an invalid {action:?} lifecycle outcome: {reason}"
    ))
}

async fn persist_agent_thread_lifecycle_outcome(
    state: &StateRuntime,
    thread_id: &str,
    write: AgentThreadLifecycleMetadataWrite,
) -> Result<()> {
    let (key, value) = match write {
        AgentThreadLifecycleMetadataWrite::None => return Ok(()),
        AgentThreadLifecycleMetadataWrite::Projection(projection) => (
            AGENT_SESSION_LIFECYCLE_METADATA_KEY,
            serde_json::to_value(projection)?,
        ),
        AgentThreadLifecycleMetadataWrite::RemoteDeletePrepared { at_ms } => (
            AGENT_SESSION_DELETE_INTENT_METADATA_KEY,
            serde_json::json!({"state": "prepared", "createdAtMs": at_ms}),
        ),
        AgentThreadLifecycleMetadataWrite::RemoteDeleteAcknowledged { at_ms } => (
            AGENT_SESSION_DELETE_INTENT_METADATA_KEY,
            serde_json::json!({"state": "remoteAcknowledged", "updatedAtMs": at_ms}),
        ),
    };
    state
        .set_session_metadata_field(thread_id, key, Some(value))
        .await
}

impl Thread {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub async fn snapshot(&self) -> Result<ThreadSnapshot> {
        let summary = self
            .client
            .inner
            .state
            .session_summary(&self.id)
            .await?
            .ok_or_else(|| Error::Message(format!("thread not found: {}", self.id)))?;
        self.client.snapshot_from_summary(summary).await
    }

    pub async fn archive(&self) -> Result<()> {
        self.enqueue_lifecycle(AgentThreadLifecycleAction::Archive { reason: None })
            .await
    }

    pub async fn archive_with_reason(&self, reason: impl Into<String>) -> Result<()> {
        self.enqueue_lifecycle(AgentThreadLifecycleAction::Archive {
            reason: Some(reason.into()),
        })
        .await
    }

    pub async fn restore(&self) -> Result<()> {
        self.enqueue_lifecycle(AgentThreadLifecycleAction::Restore)
            .await
    }

    pub async fn delete(&self) -> Result<()> {
        self.enqueue_lifecycle(AgentThreadLifecycleAction::Delete)
            .await
    }

    fn enqueue_lifecycle(
        &self,
        action: AgentThreadLifecycleAction,
    ) -> BoxFuture<'static, Result<()>> {
        self.enqueue_mutation(move |thread| async move {
            let summary = thread
                .client
                .inner
                .state
                .session_summary(&thread.id)
                .await?
                .ok_or_else(|| Error::Message(format!("thread not found: {}", thread.id)))?;
            let binding = thread
                .client
                .inner
                .state
                .gateway_runtime_binding(&thread.id)
                .await?
                .map(AgentBindingSnapshot::try_from)
                .transpose()?;
            let context = ThreadExecutionContext::from_summary(summary);
            let mcp_resolver = thread.client.agent_mcp_server_resolver(&context);
            let metadata = thread
                .client
                .inner
                .state
                .session_metadata(&thread.id)
                .await?;
            let mut current = decode_agent_thread_lifecycle(metadata.as_ref())?;
            for attempt in 0..2 {
                let outcome = thread
                    .client
                    .inner
                    .agent_sessions
                    .apply_thread_lifecycle(AgentThreadLifecycleRequest {
                        thread: context.clone(),
                        binding: binding.clone(),
                        action: action.clone(),
                        current: current.clone(),
                        mcp_resolver: mcp_resolver.clone(),
                    })
                    .await?;
                let (next, write) =
                    validate_agent_thread_lifecycle_outcome(&action, &current, outcome)?;
                persist_agent_thread_lifecycle_outcome(
                    &thread.client.inner.state,
                    &thread.id,
                    write,
                )
                .await?;
                current = next;
                if !matches!(action, AgentThreadLifecycleAction::Delete)
                    || !matches!(
                        current.remote_delete,
                        AgentRemoteDeleteState::Prepared { .. }
                    )
                {
                    break;
                }
                if attempt == 1 {
                    return Err(Error::Message(
                        "Agent Thread delete did not reach a remote acknowledgement".to_string(),
                    ));
                }
            }
            match action {
                AgentThreadLifecycleAction::Archive { reason } => {
                    if let Some(reason) = reason {
                        thread
                            .client
                            .inner
                            .state
                            .mark_session_ended_with_reason(&thread.id, &reason)
                            .await?;
                    }
                    thread
                        .client
                        .inner
                        .state
                        .archive_session(&thread.id)
                        .await?;
                    thread.client.inner.runtime.remove_mcp_runtime(&thread.id);
                }
                AgentThreadLifecycleAction::Restore => {
                    thread
                        .client
                        .inner
                        .state
                        .restore_session(&thread.id)
                        .await?;
                }
                AgentThreadLifecycleAction::Delete => {
                    thread.client.inner.state.delete_session(&thread.id).await?;
                    thread.client.inner.runtime.remove_mcp_runtime(&thread.id);
                }
            }
            Ok(())
        })
    }

    pub async fn compact(&self, request: CompactThreadRequest) -> Result<CompactionResult> {
        self.enqueue_compact(request).await
    }

    fn enqueue_compact(
        &self,
        request: CompactThreadRequest,
    ) -> BoxFuture<'static, Result<CompactionResult>> {
        self.enqueue_mutation(move |thread| async move { thread.compact_reserved(request).await })
    }

    async fn compact_reserved(&self, request: CompactThreadRequest) -> Result<CompactionResult> {
        let snapshot = self.snapshot().await?;
        let inherited_env = self.client.application_environment(request.inherited_env);
        crate::compaction::compact_session(CompactSessionOptions {
            state: self.client.inner.state.clone(),
            cwd: PathBuf::from(snapshot.cwd.clone()),
            session: self.id.clone(),
            config_path: request
                .config_path
                .or_else(|| self.client.inner.config_path.clone()),
            model: request.model,
            reasoning_effort: request.reasoning_effort,
            inherited_env: Some(inherited_env),
            reason: request.reason,
            instructions: request.instructions,
            force: request.force,
        })
        .await
    }

    pub async fn fork(&self, request: ForkThreadRequest) -> Result<Thread> {
        self.enqueue_idle_history_mutation(move |thread| async move {
            let id = thread
                .client
                .inner
                .state
                .fork_native_session_history(NativeSessionForkInput {
                    source_session_id: &thread.id,
                    before_session_seq: request.before_session_seq,
                })
                .await?;
            Ok(Thread {
                client: thread.client,
                id,
            })
        })
        .await
    }

    pub async fn fork_agent(&self, request: ForkAgentThreadRequest) -> Result<Thread> {
        let source = request.source.trim().to_string();
        if source.is_empty() {
            return Err(Error::Message(
                "Agent fork Thread source must not be empty".to_string(),
            ));
        }
        self.enqueue_mutation(move |thread| async move {
            let summary = thread
                .client
                .inner
                .state
                .session_summary(&thread.id)
                .await?
                .ok_or_else(|| Error::Message(format!("thread not found: {}", thread.id)))?;
            let binding = thread
                .client
                .inner
                .state
                .gateway_runtime_binding(&thread.id)
                .await?
                .map(AgentBindingSnapshot::try_from)
                .transpose()?
                .ok_or_else(|| {
                    Error::Message(format!(
                        "Thread `{}` has no resolved Agent binding",
                        thread.id
                    ))
                })?;
            let source_context = ThreadExecutionContext::from_summary(summary);
            let destination_context = ThreadExecutionContext {
                id: Uuid::now_v7().to_string(),
                cwd: source_context.cwd.clone(),
                source: source.clone(),
                source_key: None,
            };
            let cwd = PathBuf::from(&destination_context.cwd);
            let publication = thread
                .client
                .inner
                .agent_sessions
                .clone()
                .fork_thread(AgentThreadForkRequest {
                    source: source_context.clone(),
                    destination: destination_context.clone(),
                    binding,
                    mcp_resolver: thread.client.agent_mcp_server_resolver(&source_context),
                })
                .await?;
            let commit = commit_agent_thread_publication(
                &thread.client.inner.state,
                &destination_context,
                &cwd,
                &source,
                Some(&thread.id),
                &publication,
            )
            .await;
            match commit {
                Ok(AgentThreadImportCommit::Published) => Ok(Thread {
                    client: thread.client,
                    id: destination_context.id,
                }),
                Ok(AgentThreadImportCommit::Existing { thread_id }) => {
                    thread
                        .client
                        .inner
                        .agent_sessions
                        .abort_thread_publication(AgentThreadPublicationAbortRequest {
                            thread: destination_context,
                            binding: publication.binding,
                        })
                        .await?;
                    Ok(Thread {
                        client: thread.client,
                        id: thread_id,
                    })
                }
                Err(error) => {
                    let abort = thread
                        .client
                        .inner
                        .agent_sessions
                        .abort_thread_publication(AgentThreadPublicationAbortRequest {
                            thread: destination_context,
                            binding: publication.binding,
                        })
                        .await;
                    match abort {
                        Ok(()) => Err(error),
                        Err(abort_error) => Err(Error::Message(format!(
                            "{error}; Agent fork release also failed: {abort_error}"
                        ))),
                    }
                }
            }
        })
        .await
    }

    pub(super) fn enqueue_mutation<T, F, Fut>(&self, operation: F) -> BoxFuture<'static, Result<T>>
    where
        T: Send + 'static,
        F: FnOnce(Thread) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T>> + Send + 'static,
    {
        let thread = self.clone();
        Box::pin(async move {
            let runtime = thread.client.inner.runtime.clone();
            let admission = runtime.begin_admission().await?;
            let reservation = runtime.reserve_mutation(&thread.id)?;
            let (result_tx, result_rx) = oneshot::channel();
            runtime.spawn(async move {
                drop(admission);
                let result = async {
                    let _reservation = reservation.acquire().await?;
                    operation(thread).await
                }
                .await;
                let _ = result_tx.send(result);
            });
            result_rx.await.map_err(|_| {
                Error::Message("accepted Thread mutation ended without a result".to_string())
            })?
        })
    }

    pub async fn respond(
        &self,
        interaction_id: &str,
        response: InteractionResponse,
    ) -> Result<InteractionResponseReceipt> {
        match self
            .client
            .inner
            .runtime
            .thread_turn_handles(&self.id)
            .into_iter()
            .next()
        {
            Some(turn) => turn.respond(interaction_id, response).await,
            None => Ok(InteractionResponseReceipt { accepted: false }),
        }
    }

    pub fn activity(&self) -> ThreadActivitySnapshot {
        self.client
            .inner
            .runtime
            .versioned_thread_activity(&self.id)
    }

    pub fn interrupt(&self) -> (bool, usize) {
        let handles = self.client.inner.runtime.thread_turn_handles(&self.id);
        let mut interrupted = false;
        let mut cleared = 0;
        for (index, handle) in handles.into_iter().enumerate() {
            handle.interrupt();
            if index == 0 {
                interrupted = true;
            } else {
                cleared += 1;
            }
        }
        (interrupted, cleared)
    }

    pub fn steer(
        &self,
        expected_turn_id: &str,
        input: impl Into<String>,
    ) -> std::result::Result<bool, psychevo_agent_core::ControlInputError> {
        self.steer_message(
            expected_turn_id,
            psychevo_agent_core::user_text_message(input),
        )
    }

    pub fn steer_message(
        &self,
        expected_turn_id: &str,
        message: psychevo_agent_core::Message,
    ) -> std::result::Result<bool, psychevo_agent_core::ControlInputError> {
        let (_, active_turn_id, _) = self.client.inner.runtime.thread_activity(&self.id);
        if active_turn_id.as_deref() != Some(expected_turn_id) {
            return Ok(false);
        }
        match self.client.inner.runtime.turn_handle(expected_turn_id) {
            Some(turn) => turn.queue_steer_message(message).map(|_| true),
            None => Ok(false),
        }
    }

    pub async fn pending_interactions(&self) -> Result<Vec<PendingInteraction>> {
        Ok(self
            .client
            .inner
            .state
            .framework_interactions_for_thread(&self.id, true)
            .await?
            .into_iter()
            .map(PendingInteraction::from)
            .collect())
    }

    pub fn history(&self) -> HistoryReader {
        HistoryReader::new(self.client.inner.state.clone(), self.id.clone())
    }

    #[cfg(test)]
    pub(super) fn has_activity(&self) -> bool {
        self.client.inner.runtime.thread_activity(&self.id).0
    }
}

async fn commit_agent_thread_publication(
    state: &StateRuntime,
    context: &ThreadExecutionContext,
    cwd: &Path,
    source: &str,
    parent_thread_id: Option<&str>,
    publication: &AgentThreadPublication,
) -> Result<AgentThreadImportCommit> {
    let mut metadata = publication.metadata.clone();
    metadata.insert(
        AGENT_SESSION_LIFECYCLE_METADATA_KEY.to_string(),
        serde_json::to_value(&publication.lifecycle)?,
    );
    metadata.insert(
        "agentSessionHistory".to_string(),
        serde_json::to_value(&publication.history)?,
    );
    if let Some(parent_thread_id) = parent_thread_id {
        metadata.insert(
            "forkedFromThreadId".to_string(),
            Value::String(parent_thread_id.to_string()),
        );
    }
    let message_inputs = publication
        .messages
        .iter()
        .map(|message| AgentThreadImportMessageInput {
            message: &message.message,
            usage: &message.usage,
            metadata: &message.metadata,
        })
        .collect::<Vec<_>>();
    let binding = &publication.binding;
    state
        .commit_agent_thread_import(AgentThreadImportCommitInput {
            thread_id: &context.id,
            parent_thread_id,
            cwd,
            source,
            binding: GatewayRuntimeBindingInput {
                thread_id: &context.id,
                agent_ref: binding.agent_ref.as_deref(),
                agent_fingerprint: &binding.agent_fingerprint,
                agent_definition_json: &binding.agent_definition_json,
                runtime_ref: &binding.runtime_ref,
                backend_kind: &binding.backend_kind,
                native_kind: &binding.native_kind,
                native_session_id: binding.native_session_id.as_deref(),
                cwd: &context.cwd,
                profile_fingerprint: &binding.profile_fingerprint,
                profile_revision: &binding.profile_revision,
                profile_config_json: &binding.profile_config_json,
                adapter_kind: &binding.adapter_kind,
                adapter_revision: &binding.adapter_revision,
                ownership: GatewayRuntimeBindingOwnership::ReadWrite,
                parent_thread_id,
            },
            messages: &message_inputs,
            metadata: &metadata,
            title: publication.title.as_deref(),
        })
        .await
}

impl ThreadSummary {
    pub(super) fn from_summary(summary: SessionSummary, active_turn_id: Option<String>) -> Self {
        let archived = summary.archived_at_ms.is_some();
        Self {
            id: summary.id,
            source: summary.source,
            parent_thread_id: summary.parent_session_id,
            cwd: summary.cwd,
            model: summary.model,
            provider: summary.provider,
            title: summary.title,
            started_at_ms: summary.started_at_ms,
            updated_at_ms: summary.updated_at_ms,
            ended_at_ms: summary.ended_at_ms,
            end_reason: summary.end_reason,
            archived_at_ms: summary.archived_at_ms,
            forked_from_thread_id: summary.forked_from_thread_id,
            archived,
            message_count: summary.message_count,
            tool_call_count: summary.tool_call_count,
            active_turn_id,
        }
    }
}

impl ThreadExecutionContext {
    pub(super) fn from_summary(summary: SessionSummary) -> Self {
        Self {
            id: summary.id,
            cwd: summary.cwd,
            source: summary.source,
            source_key: None,
        }
    }
}

impl HistoryReader {
    pub(super) fn new(state: StateRuntime, thread_id: String) -> Self {
        Self { state, thread_id }
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub async fn latest(&self, limit: Option<usize>) -> Result<HistoryPage> {
        self.before(None, limit).await
    }

    pub async fn latest_assistant_usage(&self) -> Result<Option<Value>> {
        self.state.latest_assistant_usage(&self.thread_id).await
    }

    pub async fn display_message_count(&self) -> Result<usize> {
        self.state.display_message_count(&self.thread_id).await
    }

    /// Return whether one stable message cursor belongs to this Thread's
    /// visible history projection.
    pub async fn contains(&self, session_seq: i64) -> Result<bool> {
        if session_seq <= 0 {
            return Ok(false);
        }
        self.state
            .visible_history_message_exists(&self.thread_id, session_seq)
            .await
    }

    pub async fn before(
        &self,
        before_session_seq: Option<i64>,
        limit: Option<usize>,
    ) -> Result<HistoryPage> {
        self.before_with_visibility(before_session_seq, limit, false)
            .await
    }

    /// Read the product-visible transcript page. Hidden inherited context is
    /// filtered by Store before `LIMIT`; execution history remains available
    /// through [`Self::before`].
    pub async fn visible_before(
        &self,
        before_session_seq: Option<i64>,
        limit: Option<usize>,
    ) -> Result<HistoryPage> {
        self.before_with_visibility(before_session_seq, limit, true)
            .await
    }

    async fn before_with_visibility(
        &self,
        before_session_seq: Option<i64>,
        limit: Option<usize>,
        visible_only: bool,
    ) -> Result<HistoryPage> {
        let limit = limit
            .unwrap_or(DEFAULT_HISTORY_PAGE_SIZE)
            .clamp(1, MAX_HISTORY_PAGE_SIZE);
        let summaries = if visible_only {
            self.state
                .load_visible_tui_message_summaries_before(
                    &self.thread_id,
                    before_session_seq,
                    limit.saturating_add(1),
                )
                .await?
        } else {
            self.state
                .load_tui_message_summaries_before(
                    &self.thread_id,
                    before_session_seq,
                    limit.saturating_add(1),
                )
                .await?
        };
        let mut items = summaries
            .into_iter()
            .map(ThreadItem::from)
            .collect::<Vec<_>>();
        let has_more = items.len() > limit;
        if has_more {
            items.remove(0);
        }
        let next_before = has_more
            .then(|| items.first().map(|item| item.session_seq))
            .flatten();
        Ok(HistoryPage {
            thread_id: self.thread_id.clone(),
            items,
            next_before,
        })
    }
}

impl fmt::Debug for HistoryReader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HistoryReader")
            .field("thread_id", &self.thread_id)
            .finish_non_exhaustive()
    }
}

impl ThreadSnapshot {
    fn from_summary(
        summary: ThreadSummary,
        pending_interactions: Vec<PendingInteraction>,
        items: Vec<ThreadItem>,
        history_cursor: Option<i64>,
    ) -> Self {
        Self {
            summary,
            pending_interactions,
            items,
            history_cursor,
        }
    }
}

impl std::ops::Deref for ThreadSnapshot {
    type Target = ThreadSummary;

    fn deref(&self) -> &Self::Target {
        &self.summary
    }
}

impl From<crate::types::TuiMessageSummary> for ThreadItem {
    fn from(summary: crate::types::TuiMessageSummary) -> Self {
        Self {
            session_seq: summary.session_seq,
            message: summary.message,
            usage: summary.usage,
            metadata: summary.metadata,
            accounting: summary.accounting,
        }
    }
}

impl From<crate::state::FrameworkInteractionRecord> for PendingInteraction {
    fn from(record: crate::state::FrameworkInteractionRecord) -> Self {
        Self {
            interaction_id: record.interaction_id,
            thread_id: record.thread_id,
            turn_id: record.turn_id,
            kind: record.kind.as_str().to_string(),
            status: record.status.as_str().to_string(),
            payload: record.payload,
            resolution: record.resolution,
            requested_at_ms: record.requested_at_ms,
            resolved_at_ms: record.resolved_at_ms,
        }
    }
}
