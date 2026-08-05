use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use psychevo::{
    AgentBindingSnapshot, ConfigurationQuery, Error, FrameworkTurnTerminalStatus, Thread,
    ThreadAgentBinding, ThreadHistoryEditingStaged, ThreadItem,
    application::{
        ClarifyResult, GatewayActivityKind, GatewayActivityRecord, GatewayActivityState,
        GatewayControlCommandInput, GatewayControlCommandKind, GatewayDurability,
        PermissionApprovalDecision,
    },
    config::RuntimeProfileConfig,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard, oneshot};
use uuid::Uuid;

use super::activity::{
    ActiveActivityKind, ActiveThreadState, GatewayActivity, PendingQueuedActivity,
    PendingQueuedShell, SendShellRequest, ShellStartState,
};
use super::agent_session::{
    AgentSessionDiscoveryQuery, AgentSessionHost, AgentSessionRef, CapturedAgentSessionTarget,
    CapturedFrameworkAgentImport, CapturedFrameworkAgentImportReservation,
    agent_session_configuration_error,
};
use super::durable_activity::ShellActivityRuntime;
use super::event_ingress::{GatewayEventIngress, GatewayEventIngressDiagnostics};
use super::peer_runtime::ResolvedPeerTurn;
use super::results::GatewayShellResult;
use super::stream_input::{source_key_key, thread_key};
use super::supervisor::{
    GatewayActivityAdmissionError, GatewayActivityPermit, GatewayAdmissionClosed, GatewaySupervisor,
};
use super::{Gateway, GatewayLimits};
use crate::acp_peer;
use crate::gateway_now_ms;
use crate::transcript;
use psychevo_gateway_protocol::events_transcript::{
    GatewayLocalOperationView, ThreadActivityView, TranscriptEntry,
};
use psychevo_gateway_protocol::source::{GatewaySource, GatewayThreadSelector, SourceKey};

pub(crate) struct BoundedTranscriptPage {
    pub(crate) entries: Vec<TranscriptEntry>,
    pub(crate) next_cursor: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum TranscriptPositionKind {
    Message,
    Structural,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptPosition {
    boundary_session_seq: i64,
    kind: TranscriptPositionKind,
    created_at_ms: i64,
    entry_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptCursor {
    version: u8,
    thread_id: String,
    position: TranscriptPosition,
}

fn encode_transcript_cursor(
    thread_id: &str,
    position: TranscriptPosition,
) -> psychevo::Result<String> {
    Ok(
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&TranscriptCursor {
            version: 1,
            thread_id: thread_id.to_string(),
            position,
        })?),
    )
}

fn decode_transcript_cursor(
    thread_id: &str,
    encoded: &str,
) -> psychevo::Result<TranscriptPosition> {
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| psychevo::Error::Message("history cursor is invalid".to_string()))?;
    let cursor = serde_json::from_slice::<TranscriptCursor>(&bytes)
        .map_err(|_| psychevo::Error::Message("history cursor is invalid".to_string()))?;
    if cursor.version != 1 || cursor.thread_id != thread_id {
        return Err(psychevo::Error::Message(
            "history cursor does not belong to this Thread".to_string(),
        ));
    }
    Ok(cursor.position)
}

fn message_position(entry: &TranscriptEntry) -> Option<TranscriptPosition> {
    Some(TranscriptPosition {
        boundary_session_seq: entry.message_seq?,
        kind: TranscriptPositionKind::Message,
        created_at_ms: entry.created_at_ms,
        entry_id: entry.id.clone(),
    })
}

fn structural_position(boundary_session_seq: i64, entry: &TranscriptEntry) -> TranscriptPosition {
    TranscriptPosition {
        boundary_session_seq,
        kind: TranscriptPositionKind::Structural,
        created_at_ms: entry.created_at_ms,
        entry_id: entry.id.clone(),
    }
}

async fn transcript_revert_boundary(thread: &Thread) -> psychevo::Result<i64> {
    Ok(match thread.history_editing_state().await?.staged {
        Some(
            ThreadHistoryEditingStaged::WorkspaceUndo {
                boundary_message_seq,
                ..
            }
            | ThreadHistoryEditingStaged::ConversationEdit {
                boundary_message_seq,
                ..
            },
        ) => boundary_message_seq,
        None => i64::MAX,
    })
}

async fn load_thread_history_before(
    thread: &Thread,
    before_session_seq: Option<i64>,
    limit: usize,
) -> psychevo::Result<(Vec<ThreadItem>, bool)> {
    let history = thread.history();
    let mut cursor = before_session_seq;
    let mut remaining = limit;
    let mut pages = Vec::new();
    let mut has_older = false;
    while remaining > 0 {
        let page = history.visible_before(cursor, Some(remaining)).await?;
        let fetched = page.items.len();
        has_older = page.next_before.is_some();
        pages.push(page.items);
        if fetched == 0 || !has_older {
            break;
        }
        remaining = remaining.saturating_sub(fetched);
        cursor = page.next_before;
    }
    pages.reverse();
    Ok((pages.into_iter().flatten().collect(), has_older))
}

impl Gateway {
    pub(crate) fn capture_framework_agent_import(
        &self,
        captured: CapturedFrameworkAgentImport,
    ) -> CapturedFrameworkAgentImportReservation {
        self.agent_sessions.reserve_framework_import(captured)
    }

    pub(crate) async fn discover_agent_sessions(
        &self,
        profile: RuntimeProfileConfig,
        peer: ResolvedPeerTurn,
        cwd: PathBuf,
        cursor: Option<String>,
    ) -> psychevo::Result<acp_peer::lifecycle::AcpSessionListPage> {
        self.agent_sessions
            .discover(
                CapturedAgentSessionTarget::invocation(
                    format!("session-discovery:{}", Uuid::now_v7()),
                    profile,
                    Some(peer),
                ),
                AgentSessionDiscoveryQuery {
                    cwd_filter: Some(cwd),
                    cursor,
                },
            )
            .await
    }

    pub(crate) async fn lock_source_mutation(&self, source_key: &SourceKey) -> OwnedMutexGuard<()> {
        let lock = self
            .source_mutations
            .lock()
            .expect("gateway source mutation map poisoned")
            .entry(source_key.0.clone())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone();
        lock.lock_owned().await
    }

    pub(crate) fn from_composition(
        durability: GatewayDurability,
        agent_sessions: AgentSessionHost,
        framework_client: psychevo::Client,
        limits: GatewayLimits,
    ) -> Self {
        let supervisor = GatewaySupervisor::new(limits.shell_activity_limit);
        Self {
            durability,
            agent_sessions,
            framework_client,
            event_ingress: GatewayEventIngress::new(
                supervisor.clone(),
                limits.event_ingress_capacity,
            ),
            supervisor,
            active_queue: Arc::new(Mutex::new(Default::default())),
            process_bindings: Arc::new(Mutex::new(HashMap::new())),
            source_generations: Arc::new(Mutex::new(HashMap::new())),
            source_mutations: Arc::new(Mutex::new(HashMap::new())),
            live_snapshots: Arc::new(Mutex::new(HashMap::new())),
            shell_activity_runtime: Arc::new(ShellActivityRuntime::default()),
            shell_queue_limit: limits.shell_queue_limit,
            owner_id: Arc::new(format!("gateway:{}:{}", std::process::id(), Uuid::now_v7())),
        }
    }

    pub fn event_ingress_diagnostics(&self) -> GatewayEventIngressDiagnostics {
        self.event_ingress.diagnostics()
    }

    pub(crate) fn framework_client(&self) -> psychevo::Client {
        self.framework_client.clone()
    }

    pub(crate) async fn framework_thread(&self, thread_id: &str) -> psychevo::Result<Thread> {
        self.framework_client
            .resume_thread(thread_id.to_string())
            .await
    }

    async fn transcript_position_exists(
        &self,
        thread: &Thread,
        position: &TranscriptPosition,
    ) -> psychevo::Result<bool> {
        match position.kind {
            TranscriptPositionKind::Message => {
                let Some(before) = position.boundary_session_seq.checked_add(1) else {
                    return Ok(false);
                };
                let page = thread
                    .history()
                    .visible_before(Some(before), Some(1))
                    .await?;
                let Some(item) = page.items.first() else {
                    return Ok(false);
                };
                if item.session_seq != position.boundary_session_seq {
                    return Ok(false);
                }
                let entries =
                    transcript::project_transcript_entries(thread.id(), std::slice::from_ref(item));
                Ok(entries
                    .first()
                    .and_then(message_position)
                    .is_some_and(|expected| expected.eq(position)))
            }
            TranscriptPositionKind::Structural => {
                let revert_boundary = transcript_revert_boundary(thread).await?;
                if let Some(checkpoint_id) = position
                    .entry_id
                    .strip_prefix("compaction:")
                    .and_then(|value| value.parse::<i64>().ok())
                {
                    let Some(compaction) = thread.compaction(checkpoint_id).await? else {
                        return Ok(false);
                    };
                    if compaction.metadata.as_ref().is_some_and(|metadata| {
                        metadata.get("projection_only").and_then(Value::as_bool) == Some(true)
                    }) {
                        return Ok(false);
                    }
                    if compaction.boundary_session_seq >= revert_boundary {
                        return Ok(false);
                    }
                    let entries = transcript::project_compaction_entries(
                        thread.id(),
                        std::slice::from_ref(&compaction),
                    );
                    return Ok(entries.first().is_some_and(|entry| {
                        structural_position(compaction.boundary_session_seq, entry).eq(position)
                    }));
                }
                let Some(turn_id) = position
                    .entry_id
                    .strip_prefix("turn:")
                    .and_then(|value| value.strip_suffix(":terminal"))
                else {
                    return Ok(false);
                };
                let Some(terminal) = self
                    .framework_client
                    .framework_turn_terminal_evidence(turn_id)
                    .await?
                else {
                    return Ok(false);
                };
                Ok(terminal.thread_id == thread.id()
                    && matches!(
                        terminal.status,
                        FrameworkTurnTerminalStatus::Failed
                            | FrameworkTurnTerminalStatus::Interrupted
                    )
                    && terminal.boundary_session_seq < revert_boundary
                    && TranscriptPosition {
                        boundary_session_seq: terminal.boundary_session_seq,
                        kind: TranscriptPositionKind::Structural,
                        created_at_ms: terminal.completed_at_ms,
                        entry_id: format!("turn:{}:terminal", terminal.turn_id),
                    }
                    .eq(position))
            }
        }
    }

    pub(crate) async fn framework_agent_binding(
        &self,
        thread_id: &str,
    ) -> psychevo::Result<Option<AgentBindingSnapshot>> {
        Ok(
            match self
                .framework_client
                .thread_agent_binding(thread_id)
                .await?
            {
                Some(ThreadAgentBinding::Resolved { binding, .. }) => Some(*binding),
                Some(ThreadAgentBinding::Unresolved { .. }) | None => None,
            },
        )
    }

    pub async fn shutdown_runtimes(&self, force: bool) -> psychevo::Result<()> {
        self.agent_sessions.shutdown(force).await
    }

    pub(crate) async fn shutdown_activity_runtime(&self, force: bool) -> psychevo::Result<()> {
        self.supervisor.close_activity_admission();
        self.supervisor.stop_producers();
        self.supervisor.wait_for_producers().await;
        if force {
            self.supervisor.force_cancel_activities();
            self.cancel_active_queue();
        } else {
            self.supervisor.close_activities();
        }
        self.supervisor.wait_for_activities().await;
        self.stop_shell_activity_runtime();
        self.event_ingress.close();
        if force {
            self.supervisor.force_cancel_infrastructure();
        } else {
            self.supervisor.close_infrastructure();
        }
        self.supervisor.wait_for_infrastructure().await;
        let panics = self.supervisor.panic_summary();
        let mut failures = Vec::new();
        if panics.count > 0 {
            let first = panics
                .first
                .map(|panic| {
                    format!(
                        "{} ({:?}): {}; recovery backtrace: {}",
                        panic.name, panic.scope, panic.message, panic.recovery_backtrace
                    )
                })
                .unwrap_or_else(|| "unknown task".to_string());
            failures.push(format!(
                "{} supervised Gateway task(s) panicked; first: {first}",
                panics.count
            ));
        }
        let shell_diagnostics = self.shell_activity_diagnostics();
        if shell_diagnostics.failed_operations > 0 {
            failures.push(format!(
                "{} Gateway Shell durability operation(s) failed; first: {}",
                shell_diagnostics.failed_operations,
                shell_diagnostics
                    .first_failure
                    .as_deref()
                    .unwrap_or("unknown failure")
            ));
        }
        let ingress_diagnostics = self.event_ingress.diagnostics();
        if ingress_diagnostics.failed > 0 {
            failures.push(format!(
                "{} Gateway retained-live relay operation(s) failed; first: {}",
                ingress_diagnostics.failed,
                ingress_diagnostics
                    .first_failure
                    .as_deref()
                    .unwrap_or("unknown failure")
            ));
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(Error::Message(format!(
                "Gateway shutdown was not clean: {}",
                failures.join("; ")
            )))
        }
    }

    #[cfg(test)]
    pub(crate) async fn wait_for_gateway_events(&self) {
        self.event_ingress
            .fence()
            .await
            .expect("Gateway event ingress fence");
    }

    pub(crate) fn spawn_background<F>(&self, name: impl Into<Arc<str>>, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.supervisor.spawn_producer(name, future);
    }

    pub(crate) fn spawn_shutdown_aware_background<B, F>(&self, name: impl Into<Arc<str>>, build: B)
    where
        B: FnOnce(tokio_util::sync::CancellationToken) -> F,
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.supervisor.spawn_shutdown_aware_producer(name, build);
    }

    pub(crate) fn acquire_activity_permit(
        &self,
    ) -> Result<GatewayActivityPermit, GatewayAdmissionClosed> {
        self.supervisor.acquire_activity_admission()
    }

    pub(crate) fn spawn_permitted_activity<F>(
        &self,
        name: impl Into<Arc<str>>,
        permit: GatewayActivityPermit,
        future: F,
    ) where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.supervisor
            .spawn_permitted_activity(name, permit, future);
    }

    pub(crate) async fn inspect_cached_bound_agent_session(
        &self,
        local_session_id: String,
        native_session_id: String,
    ) -> psychevo::Result<Option<acp_peer::session_projection::AcpSessionSnapshot>> {
        self.agent_sessions
            .inspect_cached_acp_session(local_session_id, native_session_id)
            .await
    }

    pub(crate) async fn prepare_agent_session(
        &self,
        peer: ResolvedPeerTurn,
        profile: RuntimeProfileConfig,
        cwd: PathBuf,
        source_key: String,
        target_id: String,
        agent_ref: Option<String>,
    ) -> psychevo::Result<acp_peer::session_projection::AcpSessionSnapshot> {
        let configuration = self
            .framework_client
            .configuration(ConfigurationQuery::new(&cwd))?;
        let mcp_servers =
            acp_peer::stdio_turn::resolve_peer_mcp_server_handoffs(&peer, &configuration).await?;
        self.agent_sessions
            .prepare(
                CapturedAgentSessionTarget::invocation(
                    format!("draft:{source_key}"),
                    profile,
                    Some(peer),
                ),
                source_key,
                target_id,
                agent_ref,
                cwd,
                mcp_servers,
            )
            .await
    }

    pub(crate) async fn inspect_prepared_agent_session(
        &self,
        source_key: &str,
        target_id: &str,
    ) -> psychevo::Result<Option<acp_peer::session_projection::AcpSessionSnapshot>> {
        self.agent_sessions
            .inspect_prepared(source_key, target_id)
            .await
    }

    pub(crate) async fn set_prepared_agent_session_control(
        &self,
        source_key: &str,
        target_id: &str,
        control_id: String,
        value: Value,
    ) -> psychevo::Result<Option<acp_peer::session_projection::AcpSessionSnapshot>> {
        self.agent_sessions
            .set_prepared_control(source_key, target_id, control_id, value)
            .await
    }

    pub(crate) async fn release_prepared_agent_session(
        &self,
        source_key: &str,
    ) -> psychevo::Result<bool> {
        self.agent_sessions.release_prepared(source_key).await
    }

    pub(crate) async fn set_bound_agent_session_control(
        &self,
        peer: ResolvedPeerTurn,
        profile: RuntimeProfileConfig,
        local_session_id: String,
        native_session_id: String,
        control_id: String,
        value: Value,
    ) -> psychevo::Result<acp_peer::session_projection::AcpSessionSnapshot> {
        let binding = self
            .framework_agent_binding(&local_session_id)
            .await?
            .ok_or_else(|| {
                agent_session_configuration_error(format!(
                    "Agent binding not found for thread `{local_session_id}`."
                ))
            })?;
        if binding.native_session_id.as_deref() != Some(native_session_id.as_str()) {
            return Err(agent_session_configuration_error(format!(
                "Agent binding for thread `{local_session_id}` does not own native session `{native_session_id}`."
            )));
        }
        let cwd = PathBuf::from(&binding.cwd);
        let configuration = self
            .framework_client
            .configuration(ConfigurationQuery::new(&cwd))?;
        let mcp_servers =
            acp_peer::stdio_turn::resolve_peer_mcp_server_handoffs(&peer, &configuration).await?;
        self.agent_sessions
            .attach(CapturedAgentSessionTarget::application_bound(
                &binding,
                profile,
                Some(peer),
            )?)?
            .set_control(
                AgentSessionRef {
                    cwd,
                    local_session_id,
                    native_session_id,
                    mcp_servers,
                },
                control_id,
                value,
            )
            .await?
            .into_acp()
    }

    pub(crate) async fn probe_acp_backend_authentication(
        &self,
        peer: ResolvedPeerTurn,
        cwd: PathBuf,
    ) -> psychevo::Result<acp_peer::process_pool::AcpAuthDoctorStatus> {
        self.agent_sessions
            .probe_acp_authentication(peer, cwd)
            .await
    }

    pub(crate) async fn probe_acp_backend_protocol_compatibility(
        &self,
        peer: ResolvedPeerTurn,
        cwd: PathBuf,
    ) -> psychevo::Result<acp_peer::process_pool::AcpProtocolDoctorStatus> {
        self.agent_sessions
            .probe_acp_protocol_compatibility(peer, cwd)
            .await
    }

    pub fn owner_id(&self) -> &str {
        self.owner_id.as_str()
    }

    pub async fn resolve_source_thread(
        &self,
        source: &GatewaySource,
    ) -> psychevo::Result<Option<String>> {
        self.lookup_source_thread(source).await
    }

    #[cfg(test)]
    pub(crate) async fn thread_transcript(
        &self,
        thread_id: &str,
    ) -> psychevo::Result<Vec<TranscriptEntry>> {
        let mut pages = Vec::new();
        let mut before = None;
        loop {
            let page = self
                .thread_transcript_page(thread_id, before.as_deref(), 200)
                .await?;
            let next_before = page.next_cursor.clone();
            pages.push(page.entries);
            let Some(next_before) = next_before else {
                break;
            };
            before = Some(next_before);
        }
        Ok(pages.into_iter().rev().flatten().collect())
    }

    pub(crate) async fn thread_transcript_page(
        &self,
        thread_id: &str,
        before: Option<&str>,
        limit: usize,
    ) -> psychevo::Result<BoundedTranscriptPage> {
        let before_position = before
            .map(|cursor| decode_transcript_cursor(thread_id, cursor))
            .transpose()?;
        let thread = self.framework_thread(thread_id).await?;
        if let Some(position) = before_position.as_ref()
            && !self.transcript_position_exists(&thread, position).await?
        {
            return Err(psychevo::Error::Message(
                "history cursor does not identify a visible Thread entry".to_string(),
            ));
        }
        let before_message_seq = before_position
            .as_ref()
            .map(|position| match position.kind {
                TranscriptPositionKind::Message => position.boundary_session_seq,
                TranscriptPositionKind::Structural => {
                    position.boundary_session_seq.saturating_add(1)
                }
            });
        let fetch_limit = limit.saturating_mul(4).saturating_add(8);
        let (summaries, has_older_messages) =
            load_thread_history_before(&thread, before_message_seq, fetch_limit).await?;
        let lower_boundary = if has_older_messages {
            summaries
                .first()
                .map(|summary| summary.session_seq)
                .unwrap_or(i64::MIN)
        } else {
            i64::MIN
        };
        let mut entries = transcript::project_transcript_entries(thread_id, &summaries);
        let relationships = thread
            .agent_children_matching(&transcript::agent_relationship_lookup_candidates(&entries))
            .await?;
        transcript::enrich_agent_blocks_from_relationships(&mut entries, &relationships);

        let (structural_history, has_older_structural) = thread
            .structural_history_window(
                lower_boundary,
                before_position
                    .as_ref()
                    .map(|position| position.boundary_session_seq),
                before_position.as_ref().and_then(|position| {
                    (position.kind == TranscriptPositionKind::Structural)
                        .then_some((position.created_at_ms, position.entry_id.as_str()))
                }),
                fetch_limit,
            )
            .await?;
        let mut synthetic_entries = structural_history
            .compactions
            .iter()
            .zip(transcript::project_compaction_entries(
                thread_id,
                &structural_history.compactions,
            ))
            .map(|(compaction, entry)| (compaction.boundary_session_seq, entry))
            .collect::<Vec<_>>();
        transcript::reconcile_terminal_bounded_running_blocks(
            &mut entries,
            thread_id,
            &structural_history.turn_terminals,
        );
        synthetic_entries.extend(structural_history.turn_terminals.iter().map(|terminal| {
            (
                terminal.boundary_session_seq,
                transcript::project_turn_terminal_entry(thread_id, terminal),
            )
        }));
        let structural_boundaries = synthetic_entries
            .iter()
            .map(|(boundary, entry)| (entry.id.clone(), *boundary))
            .collect::<HashMap<_, _>>();
        let entries = transcript::merge_entries_at_session_boundaries(entries, synthetic_entries);
        let mut entries = entries
            .into_iter()
            .map(|entry| {
                let position = if entry.message_seq.is_some() {
                    message_position(&entry).expect("message position")
                } else {
                    structural_position(structural_boundaries[&entry.id], &entry)
                };
                (position, entry)
            })
            .filter(|(position, _)| {
                before_position
                    .as_ref()
                    .is_none_or(|before| position < before)
            })
            .collect::<Vec<_>>();
        let projected_overflow = entries.len() > limit;
        if projected_overflow {
            let drain = entries.len() - limit;
            entries.drain(..drain);
        }
        let next_cursor = (has_older_messages || has_older_structural || projected_overflow)
            .then(|| entries.first().map(|(position, _)| position.clone()))
            .flatten()
            .map(|position| encode_transcript_cursor(thread_id, position))
            .transpose()?;
        Ok(BoundedTranscriptPage {
            entries: entries.into_iter().map(|(_, entry)| entry).collect(),
            next_cursor,
        })
    }

    pub fn local_activity_for_selector(&self, selector: &GatewayThreadSelector) -> GatewayActivity {
        let selector_keys = self.selector_keys(selector);
        let queue = self
            .active_queue
            .lock()
            .expect("gateway active queue poisoned");
        let mut activity = GatewayActivity::default();
        let mut seen = HashSet::new();
        for key in selector_keys {
            let key = queue.aliases.get(&key).cloned().unwrap_or(key);
            if !seen.insert(key.clone()) {
                continue;
            }
            if let Some(state) = queue.activities.get(&key) {
                activity.running |= state.running && state.active_turn_id.is_some();
                if state.running
                    && let (Some(activity_id), Some(kind)) =
                        (state.active_turn_id.as_ref(), state.active_kind)
                {
                    let provenance = match kind {
                        ActiveActivityKind::Shell => ThreadActivityView::GatewayLocal {
                            operation: GatewayLocalOperationView::Shell,
                            activity_id: activity_id.clone(),
                        },
                    };
                    if !activity.activities.contains(&provenance) {
                        activity.activities.push(provenance);
                    }
                }
            }
        }
        activity
    }

    pub async fn activity_for_selector(&self, selector: GatewayThreadSelector) -> GatewayActivity {
        let selector_keys = self.selector_keys(&selector);
        let mut activity = self.local_activity_for_selector(&selector);
        let durable_keys = {
            let mut durable_keys = Vec::new();
            let mut seen = HashSet::new();
            let queue = self
                .active_queue
                .lock()
                .expect("gateway active queue poisoned");
            for key in selector_keys {
                let key = queue.aliases.get(&key).cloned().unwrap_or(key);
                if seen.insert(key.clone()) {
                    durable_keys.push(key);
                }
            }
            durable_keys
        };
        for key in durable_keys {
            if let Ok(Some(record)) = self.durable_activity_for_key(&key).await {
                self.merge_durable_activity(&mut activity, record);
            }
        }
        activity
    }

    pub async fn session_activity_snapshot(
        &self,
    ) -> psychevo::Result<BTreeMap<String, GatewayActivity>> {
        let mut snapshot = {
            let queue = self
                .active_queue
                .lock()
                .expect("gateway active queue poisoned");
            let mut snapshot = BTreeMap::new();
            for (key, state) in &queue.activities {
                if let Some(thread_id) = key.strip_prefix("thread:") {
                    merge_in_memory_activity(
                        snapshot.entry(thread_id.to_string()).or_default(),
                        state,
                    );
                }
            }
            for (alias, primary) in &queue.aliases {
                let Some(thread_id) = alias.strip_prefix("thread:") else {
                    continue;
                };
                let Some(state) = queue.activities.get(primary) else {
                    continue;
                };
                merge_in_memory_activity(snapshot.entry(thread_id.to_string()).or_default(), state);
            }
            snapshot
        };
        for record in self.durability.active_gateway_activities().await? {
            let Some(thread_id) = record.thread_id.clone() else {
                continue;
            };
            self.merge_durable_activity(snapshot.entry(thread_id).or_default(), record);
        }
        Ok(snapshot)
    }

    async fn durable_activity_for_key(
        &self,
        key: &str,
    ) -> psychevo::Result<Option<GatewayActivityRecord>> {
        if let Some(thread_id) = key.strip_prefix("thread:") {
            return self
                .durability
                .active_gateway_activity_for_thread(thread_id)
                .await;
        }
        if let Some(source_key) = key.strip_prefix("source:") {
            return self
                .durability
                .active_gateway_activity_for_source(source_key)
                .await;
        }
        Ok(None)
    }

    fn merge_durable_activity(
        &self,
        activity: &mut GatewayActivity,
        record: GatewayActivityRecord,
    ) {
        let stale = record.status == GatewayActivityState::Running
            && record.lease_expires_at_ms < gateway_now_ms();
        if matches!(
            record.status,
            GatewayActivityState::Running | GatewayActivityState::Queued
        ) && !stale
        {
            activity.running = true;
            if record.kind == GatewayActivityKind::Turn
                && activity.active_turn_id.is_none()
                && let Some(turn_id) = record.turn_id.clone()
            {
                activity.active_turn_id = Some(turn_id);
            }
            let provenance = if record.owner_id != self.owner_id() {
                Some(ThreadActivityView::Foreign {
                    owner_id: record.owner_id.clone(),
                    activity_id: record.activity_id.clone(),
                    owner_surface: record.owner_surface.clone(),
                })
            } else {
                match record.kind {
                    GatewayActivityKind::Shell => Some(ThreadActivityView::GatewayLocal {
                        operation: GatewayLocalOperationView::Shell,
                        activity_id: record.activity_id.clone(),
                    }),
                    GatewayActivityKind::Turn => None,
                }
            };
            if let Some(provenance) = provenance
                && !activity.activities.contains(&provenance)
            {
                activity.activities.push(provenance);
            }
        }
        if stale && activity.takeover_state.is_none() {
            activity.takeover_state = Some("stale".to_string());
        }
        activity.started_at_ms = match (activity.started_at_ms, Some(record.started_at_ms)) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (None, value) => value,
            (value, None) => value,
        };
        activity.updated_at_ms = match (activity.updated_at_ms, Some(record.updated_at_ms)) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (None, value) => value,
            (value, None) => value,
        };
        if activity.owner_id.is_none() {
            activity.owner_id = Some(record.owner_id);
            activity.owner_surface = record.owner_surface;
            activity.lease_expires_at_ms = Some(record.lease_expires_at_ms);
        }
    }

    fn global_shell_overload_context(
        &self,
        request: &SendShellRequest,
        activity_id: String,
    ) -> GatewayShellOverloadContext {
        let requested_queue_key = request.thread_id.as_deref().map(thread_key).or_else(|| {
            request
                .source
                .as_ref()
                .map(|source| source_key_key(&source.source_key()))
        });
        let queue = self
            .active_queue
            .lock()
            .expect("gateway active queue poisoned");
        let requested_queue_key =
            requested_queue_key.map(|key| queue.aliases.get(&key).cloned().unwrap_or(key));
        let thread_id = request.thread_id.clone().or_else(|| {
            requested_queue_key
                .as_deref()
                .and_then(|queue_key| queue_key.strip_prefix("thread:"))
                .map(str::to_string)
        });
        let active_activity_id = requested_queue_key
            .as_ref()
            .and_then(|queue_key| queue.activities.get(queue_key))
            .and_then(|state| state.active_turn_id.clone());
        let oldest_queued_age_ms = queue
            .activities
            .values()
            .filter_map(|state| state.queued.front())
            .map(|pending| pending.queued_at_ms())
            .min()
            .map_or(0, queued_shell_age_ms);
        GatewayShellOverloadContext {
            oldest_queued_age_ms,
            activity_id,
            thread_id,
            source_key: shell_request_source_key(request),
            active_activity_id,
        }
    }

    pub async fn send_shell(
        &self,
        request: SendShellRequest,
    ) -> psychevo::Result<GatewayShellResult> {
        let shell_id = Uuid::now_v7().to_string();
        let admission = match self.supervisor.acquire_shell_activity_admission() {
            Ok(admission) => admission,
            Err(error) => {
                if matches!(error, GatewayActivityAdmissionError::Overloaded { .. }) {
                    self.shell_activity_runtime
                        .overload_rejections
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                return Err(match error {
                    closed @ GatewayActivityAdmissionError::Closed => {
                        Error::Message(closed.to_string())
                    }
                    GatewayActivityAdmissionError::Overloaded { limit, occupancy } => {
                        gateway_shell_overloaded(
                            "activity",
                            limit,
                            occupancy,
                            self.global_shell_overload_context(&request, shell_id),
                        )
                    }
                });
            }
        };
        let (queue_key, resolved_thread_id) = self.queue_key_for_shell_request(&request).await?;
        let source_key = shell_request_source_key(&request);
        let mut request = Some(request);
        let active = {
            let mut queue = self
                .active_queue
                .lock()
                .expect("gateway active queue poisoned");
            let state = queue.activities.entry(queue_key.clone()).or_default();
            if state.history_mutation_reserved {
                return Err(super::active_queue::gateway_thread_busy(
                    resolved_thread_id.as_deref().unwrap_or(&queue_key),
                    "history_editing",
                    "Finish the history operation before starting a Shell command.",
                ));
            }
            if state.running {
                if state.queued.len() >= self.shell_queue_limit {
                    self.shell_activity_runtime
                        .overload_rejections
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    return Err(gateway_shell_overloaded(
                        "source",
                        self.shell_queue_limit,
                        state.queued.len(),
                        GatewayShellOverloadContext {
                            oldest_queued_age_ms: state
                                .queued
                                .front()
                                .map_or(0, |pending| queued_shell_age_ms(pending.queued_at_ms())),
                            activity_id: shell_id,
                            thread_id: resolved_thread_id.clone(),
                            source_key,
                            active_activity_id: state.active_turn_id.clone(),
                        },
                    ));
                }
                let (responder, receiver) = oneshot::channel();
                let queue_position = state.queued.len() + 1;
                let active_activity_id = state.active_turn_id.clone();
                state
                    .queued
                    .push_back(PendingQueuedActivity::Shell(Box::new(PendingQueuedShell {
                        shell_id: shell_id.clone(),
                        queued_at_ms: gateway_now_ms(),
                        request: request.take().expect("gateway shell request missing"),
                        permit: admission,
                        responder,
                    })));
                ShellStartState::Queued {
                    receiver,
                    active_activity_id,
                    queue_position,
                }
            } else {
                state.running = true;
                ShellStartState::Standalone { permit: admission }
            }
        };

        match active {
            ShellStartState::Queued {
                receiver,
                active_activity_id,
                queue_position,
            } => {
                if let Some(active_activity_id) = active_activity_id {
                    let _ = self
                        .durability
                        .set_gateway_activity_queued_turns(&active_activity_id, queue_position)
                        .await;
                }
                receiver
                    .await
                    .map_err(|_| Error::Message("gateway shell queue closed".to_string()))?
            }
            ShellStartState::Standalone { permit } => {
                let (responder, receiver) = oneshot::channel();
                self.spawn_shell_activity(
                    queue_key,
                    shell_id,
                    request.take().expect("gateway shell request missing"),
                    permit,
                    responder,
                );
                receiver
                    .await
                    .map_err(|_| Error::Message("gateway shell cancelled".to_string()))?
            }
        }
    }

    pub async fn submit_clarify(
        &self,
        selector: GatewayThreadSelector,
        call_id: &str,
        result: ClarifyResult,
    ) -> bool {
        let response = match result {
            ClarifyResult::Answered(response) => psychevo::InteractionResponse::Clarify(
                response
                    .answers
                    .into_iter()
                    .map(|answer| answer.answers)
                    .collect(),
            ),
            ClarifyResult::Cancelled => psychevo::InteractionResponse::Cancel,
        };
        let foreign_payload = match &response {
            psychevo::InteractionResponse::Clarify(answers) => json!({
                "requestId": call_id,
                "answers": answers,
            }),
            psychevo::InteractionResponse::Cancel => json!({
                "requestId": call_id,
                "cancel": true,
            }),
            psychevo::InteractionResponse::Permission(_) => unreachable!(),
        };
        if let Some(accepted) = self
            .enqueue_foreign_control_command(
                &selector,
                GatewayControlCommandKind::Clarify,
                foreign_payload,
            )
            .await
        {
            return accepted;
        }
        let Some(thread) = self.framework_thread_for_selector(&selector).await else {
            return false;
        };
        thread
            .respond(call_id, response)
            .await
            .is_ok_and(|receipt| receipt.accepted)
    }

    pub async fn steer_turn(
        &self,
        selector: GatewayThreadSelector,
        expected_turn_id: Option<&str>,
        message: psychevo::application::Message,
    ) -> bool {
        if !self.agent_supports_steer_for_selector(&selector).await {
            return false;
        }
        let expected_turn_id = match expected_turn_id {
            Some(turn_id) => turn_id,
            None => return false,
        };
        if psychevo::application::validate_queued_steer(&message).is_err() {
            return false;
        }
        let Ok(foreign_message) = serde_json::to_value(&message) else {
            return false;
        };
        if let Some(accepted) = self
            .enqueue_foreign_control_command(
                &selector,
                GatewayControlCommandKind::Steer,
                json!({
                    "expectedTurnId": expected_turn_id,
                    "message": foreign_message,
                }),
            )
            .await
        {
            return accepted;
        }
        let Some(thread) = self.framework_thread_for_selector(&selector).await else {
            return false;
        };
        thread
            .steer_message(expected_turn_id, message)
            .unwrap_or(false)
    }

    pub async fn interrupt_turn(&self, selector: GatewayThreadSelector) -> bool {
        if let Some(accepted) = self
            .enqueue_foreign_control_command(
                &selector,
                GatewayControlCommandKind::Interrupt,
                json!({}),
            )
            .await
        {
            return accepted;
        }
        let Some(thread) = self.framework_thread_for_selector(&selector).await else {
            return false;
        };
        thread.interrupt().0
    }

    async fn agent_supports_steer_for_selector(&self, selector: &GatewayThreadSelector) -> bool {
        let thread_id = match selector {
            GatewayThreadSelector::ThreadId { thread_id } => Some(thread_id.clone()),
            GatewayThreadSelector::Source { source_key } => {
                match self.durability.gateway_source_lane(&source_key.0).await {
                    Ok(lane) => lane.and_then(|lane| lane.thread_id),
                    Err(_) => return false,
                }
            }
        };
        let Some(thread_id) = thread_id else {
            return true;
        };
        match self.framework_agent_binding(&thread_id).await {
            Ok(Some(binding)) => binding.backend_kind == "native",
            Ok(None) => true,
            Err(_) => false,
        }
    }

    pub async fn submit_permission(
        &self,
        selector: GatewayThreadSelector,
        request_id: &str,
        decision: PermissionApprovalDecision,
    ) -> bool {
        if let Some(accepted) = self
            .enqueue_foreign_control_command(
                &selector,
                GatewayControlCommandKind::Permission,
                json!({
                    "requestId": request_id,
                    "decision": permission_decision_label(&decision),
                    "filesystemScope": decision.filesystem_scope,
                }),
            )
            .await
        {
            return accepted;
        }
        let Some(thread) = self.framework_thread_for_selector(&selector).await else {
            return false;
        };
        thread
            .respond(
                request_id,
                psychevo::InteractionResponse::Permission(decision),
            )
            .await
            .is_ok_and(|receipt| receipt.accepted)
    }

    /// Routes one control command to the exact live foreign owner. `None`
    /// means the selector has no current foreign owner; `Some(false)` is a
    /// failed delivery and must not fall through to an unrelated local Thread.
    async fn enqueue_foreign_control_command(
        &self,
        selector: &GatewayThreadSelector,
        command_kind: GatewayControlCommandKind,
        payload: Value,
    ) -> Option<bool> {
        let now = gateway_now_ms();
        for key in self.selector_keys(selector) {
            let Ok(Some(record)) = self.durable_activity_for_key(&key).await else {
                continue;
            };
            if record.owner_id == self.owner_id()
                || record.status != GatewayActivityState::Running
                || record.lease_expires_at_ms < now
            {
                continue;
            }
            return Some(
                self.durability
                    .enqueue_gateway_control_command(GatewayControlCommandInput {
                        activity_id: &record.activity_id,
                        owner_id: &record.owner_id,
                        command_kind,
                        payload,
                    })
                    .await
                    .is_ok(),
            );
        }
        None
    }

    async fn framework_thread_for_selector(
        &self,
        selector: &GatewayThreadSelector,
    ) -> Option<psychevo::Thread> {
        let thread_id = match selector {
            GatewayThreadSelector::ThreadId { thread_id } => thread_id.clone(),
            GatewayThreadSelector::Source { source_key } => {
                self.durability
                    .gateway_source_lane(&source_key.0)
                    .await
                    .ok()
                    .flatten()?
                    .thread_id?
            }
        };
        self.framework_thread(&thread_id).await.ok()
    }

    pub fn clear_queue(&self, selector: GatewayThreadSelector) -> usize {
        let selector_keys = self.selector_keys(&selector);
        let mut dropped = Vec::new();
        {
            let mut queue = self
                .active_queue
                .lock()
                .expect("gateway active queue poisoned");
            let mut seen = HashSet::new();
            for key in selector_keys {
                let key = queue.aliases.get(&key).cloned().unwrap_or(key);
                if !seen.insert(key.clone()) {
                    continue;
                }
                if let Some(state) = queue.activities.get_mut(&key) {
                    dropped.extend(state.queued.drain(..));
                }
            }
        }
        let count = dropped.len();
        for PendingQueuedActivity::Shell(pending) in dropped {
            let _ = pending.responder.send(Err(Error::Message(
                "gateway shell queue cleared".to_string(),
            )));
        }
        count
    }
}

fn permission_decision_label(decision: &PermissionApprovalDecision) -> &'static str {
    match decision.outcome {
        psychevo::application::PermissionApprovalOutcome::AllowOnce => "allow_once",
        psychevo::application::PermissionApprovalOutcome::AllowTurn => "allow_turn",
        psychevo::application::PermissionApprovalOutcome::AllowSession => "allow_session",
        psychevo::application::PermissionApprovalOutcome::AllowAlways => "allow_always",
        psychevo::application::PermissionApprovalOutcome::Deny => "deny",
    }
}

#[derive(Debug)]
struct GatewayShellOverloadContext {
    oldest_queued_age_ms: u64,
    activity_id: String,
    thread_id: Option<String>,
    source_key: Option<String>,
    active_activity_id: Option<String>,
}

fn gateway_shell_overloaded(
    scope: &str,
    limit: usize,
    occupancy: usize,
    context: GatewayShellOverloadContext,
) -> Error {
    Error::structured(
        format!("Gateway Shell {scope} limit reached ({limit})"),
        json!({
            "kind": "gateway_overloaded",
            "scope": scope,
            "limit": limit,
            "occupancy": occupancy,
            "retryable": true,
            "oldestQueuedAgeMs": context.oldest_queued_age_ms,
            "activityId": context.activity_id,
            "threadId": context.thread_id,
            "sourceKey": context.source_key,
            "activeActivityId": context.active_activity_id,
            "turnId": Value::Null,
        }),
    )
}

fn shell_request_source_key(request: &SendShellRequest) -> Option<String> {
    request
        .source
        .as_ref()
        .or(request.bind_source.as_ref())
        .map(|source| source.source_key().0)
}

fn queued_shell_age_ms(queued_at_ms: i64) -> u64 {
    u64::try_from(gateway_now_ms().saturating_sub(queued_at_ms)).unwrap_or_default()
}

fn merge_in_memory_activity(activity: &mut GatewayActivity, state: &ActiveThreadState) {
    activity.running |= state.running;
    if activity.active_turn_id.is_none() {
        activity.active_turn_id = state.active_turn_id.clone();
    }
    activity.queued_turns = activity.queued_turns.max(state.queued.len());
}
