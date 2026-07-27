//! High-level in-process Framework interface.
//!
//! This module owns the public Thread/Turn vocabulary. The lower run assembly
//! and state Modules remain implementation details of an Application.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use futures::FutureExt;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex as AsyncMutex, Notify, RwLock as AsyncRwLock, oneshot};
use uuid::Uuid;

mod event_log;
mod interaction_broker;
mod runtime;

use event_log::EventLog;
use interaction_broker::{
    FrameworkApprovalHandler, FrameworkInteractionControl, InteractionBroker,
};
use runtime::{ApplicationRuntime, TurnPhase};

use crate::compaction::{CompactSessionOptions, CompactionReason, CompactionResult};
use crate::paths::canonicalize_cwd;
use crate::run::{run_live_streaming_controlled, run_live_streaming_controlled_with_provider};
use crate::state::{
    GatewayTurnDeliveryInput, GatewayTurnTerminalInput, NativeSessionForkInput, SessionListCursor,
    StateRuntime,
};
use crate::types::{
    ApprovalHandler, ApprovalMode, ImageInput, McpServerInput, PermissionApprovalDecision,
    PermissionMode, ProjectContextInstructionMode, RunMode, RunOptions, RunSandboxOverride,
    RunStreamEvent, RunStreamSink, RuntimeTool, SessionEventPayload, SessionSummary, run_control,
};
use crate::{Error, Result};

#[cfg(test)]
use crate::types::{
    ClarifyAnswer, ClarifyResponse, PermissionApprovalOutcome, PermissionApprovalRequest,
};

const DEFAULT_EVENT_CAPACITY: usize = 256;
const DEFAULT_THREAD_LIST_LIMIT: usize = 50;
const MAX_THREAD_LIST_LIMIT: usize = 200;
#[cfg(not(test))]
const FORCE_SHUTDOWN_TOTAL: std::time::Duration = std::time::Duration::from_secs(10);
#[cfg(test)]
const FORCE_SHUTDOWN_TOTAL: std::time::Duration = std::time::Duration::from_millis(250);
#[cfg(not(test))]
const FORCE_ADAPTER_BUDGET: std::time::Duration = std::time::Duration::from_secs(2);
#[cfg(test)]
const FORCE_ADAPTER_BUDGET: std::time::Duration = std::time::Duration::from_millis(25);
#[cfg(not(test))]
const FORCE_COOPERATIVE_JOIN_BUDGET: std::time::Duration = std::time::Duration::from_secs(6);
#[cfg(test)]
const FORCE_COOPERATIVE_JOIN_BUDGET: std::time::Duration = std::time::Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShutdownReport {
    pub forced: bool,
    pub adapter: ShutdownAdapterStatus,
    pub task_panics: u64,
    pub aborted_tasks: usize,
    pub pending_terminal_failures: Vec<PendingTerminalFailure>,
}

impl ShutdownReport {
    pub fn is_clean(&self) -> bool {
        matches!(self.adapter, ShutdownAdapterStatus::Completed)
            && self.task_panics == 0
            && self.aborted_tasks == 0
            && self.pending_terminal_failures.is_empty()
    }

    pub fn require_clean(self) -> Result<Self> {
        if self.is_clean() {
            return Ok(self);
        }
        let details = serde_json::to_string(&self).unwrap_or_else(|_| format!("{self:?}"));
        Err(Error::Message(format!(
            "Application shutdown was not clean: {details}"
        )))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ShutdownAdapterStatus {
    Completed,
    Failed { message: String },
    TimedOut,
    ContractViolation { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingTerminalFailure {
    pub turn_id: String,
    pub message: String,
}

#[derive(Clone)]
pub struct Application {
    inner: Arc<ApplicationInner>,
}

struct ApplicationInner {
    state: StateRuntime,
    agent_sessions: Arc<dyn AgentSessionAdapter>,
    home: PathBuf,
    config_path: Option<PathBuf>,
    event_capacity: usize,
    admission: Mutex<bool>,
    admission_gate: Arc<AsyncRwLock<()>>,
    force_shutdown_requested: AtomicBool,
    force_shutdown_notify: Notify,
    shutdown_complete: Mutex<Option<ShutdownReport>>,
    shutdown_finalizer: AsyncMutex<()>,
    runtime: Arc<ApplicationRuntime>,
}

impl fmt::Debug for Application {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Application")
            .field("home", &self.inner.home)
            .field("event_capacity", &self.inner.event_capacity)
            .finish_non_exhaustive()
    }
}

impl Application {
    pub fn builder() -> ApplicationBuilder {
        ApplicationBuilder::default()
    }

    pub fn client(&self) -> Client {
        Client {
            inner: Arc::clone(&self.inner),
        }
    }

    pub async fn shutdown(&self) -> Result<ShutdownReport> {
        self.shutdown_inner(false).await
    }

    pub async fn shutdown_force(&self) -> Result<ShutdownReport> {
        self.shutdown_inner(true).await
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __from_open_state(
        home: PathBuf,
        config_path: Option<PathBuf>,
        state: StateRuntime,
        agent_sessions: Arc<dyn AgentSessionAdapter>,
    ) -> Self {
        Self {
            inner: Arc::new(ApplicationInner {
                state,
                agent_sessions,
                home,
                config_path,
                event_capacity: DEFAULT_EVENT_CAPACITY,
                admission: Mutex::new(true),
                admission_gate: Arc::new(AsyncRwLock::new(())),
                force_shutdown_requested: AtomicBool::new(false),
                force_shutdown_notify: Notify::new(),
                shutdown_complete: Mutex::new(None),
                shutdown_finalizer: AsyncMutex::new(()),
                runtime: Arc::new(ApplicationRuntime::new()),
            }),
        }
    }

    async fn shutdown_inner(&self, force: bool) -> Result<ShutdownReport> {
        if let Some(report) = self
            .inner
            .shutdown_complete
            .lock()
            .expect("application shutdown state poisoned")
            .clone()
        {
            return Ok(report);
        }
        {
            let _admission_gate = self.inner.admission_gate.write().await;
            let mut open = self
                .inner
                .admission
                .lock()
                .expect("application admission poisoned");
            if *open {
                *open = false;
                self.inner.runtime.tasks.close();
            }
        }
        if force {
            self.inner
                .force_shutdown_requested
                .store(true, AtomicOrdering::Release);
            self.inner.force_shutdown_notify.notify_one();
        }
        let _finalizer = self.inner.shutdown_finalizer.lock().await;
        if let Some(report) = self
            .inner
            .shutdown_complete
            .lock()
            .expect("application shutdown state poisoned")
            .clone()
        {
            return Ok(report);
        }
        let force = self
            .inner
            .force_shutdown_requested
            .load(AtomicOrdering::Acquire);
        let mut report = ShutdownReport {
            forced: force,
            adapter: ShutdownAdapterStatus::Completed,
            task_panics: 0,
            aborted_tasks: 0,
            pending_terminal_failures: Vec::new(),
        };

        if force {
            self.shutdown_force_owned(&mut report).await;
        } else {
            tokio::select! {
                biased;
                _ = self.inner.force_shutdown_notify.notified() => {
                    report.forced = true;
                    self.shutdown_force_owned(&mut report).await;
                }
                _ = self.shutdown_graceful_owned(&mut report) => {}
            }
        }
        self.inner.runtime.clear_mcp_runtimes();
        report.task_panics = self.inner.runtime.task_panics.load(AtomicOrdering::Relaxed);
        *self
            .inner
            .shutdown_complete
            .lock()
            .expect("application shutdown state poisoned") = Some(report.clone());
        Ok(report)
    }

    async fn shutdown_graceful_owned(&self, report: &mut ShutdownReport) {
        self.inner.runtime.tasks.wait().await;
        if let Err(error) = self.inner.agent_sessions.shutdown(false).await {
            report.adapter = ShutdownAdapterStatus::Failed {
                message: error.to_string(),
            };
        }
        self.retry_and_settle_terminal_slots(report, None).await;
        self.inner.state.close().await;
    }

    async fn shutdown_force_owned(&self, report: &mut ShutdownReport) {
        let deadline = tokio::time::Instant::now() + FORCE_SHUTDOWN_TOTAL;
        for control in self.inner.runtime.active_controls() {
            control.abort();
        }

        let adapter_deadline =
            std::cmp::min(deadline, tokio::time::Instant::now() + FORCE_ADAPTER_BUDGET);
        report.adapter = match tokio::time::timeout_at(
            adapter_deadline,
            self.inner.agent_sessions.shutdown(true),
        )
        .await
        {
            Ok(Ok(())) => ShutdownAdapterStatus::Completed,
            Ok(Err(error)) => ShutdownAdapterStatus::Failed {
                message: error.to_string(),
            },
            Err(_) => ShutdownAdapterStatus::TimedOut,
        };

        let join_deadline = std::cmp::min(
            deadline,
            tokio::time::Instant::now() + FORCE_COOPERATIVE_JOIN_BUDGET,
        );
        if tokio::time::timeout_at(join_deadline, self.inner.runtime.tasks.wait())
            .await
            .is_err()
        {
            report.aborted_tasks = self.inner.runtime.abort_all_tasks();
            if tokio::time::timeout_at(deadline, self.inner.runtime.tasks.wait())
                .await
                .is_err()
            {
                report.adapter = ShutdownAdapterStatus::ContractViolation {
                    message: "tracked tasks remained live after forced abort".to_string(),
                };
            }
        }

        self.retry_and_settle_terminal_slots(report, Some(deadline))
            .await;
        if tokio::time::timeout_at(deadline, self.inner.state.close())
            .await
            .is_err()
        {
            report.adapter = ShutdownAdapterStatus::ContractViolation {
                message: "State close exceeded the force-shutdown deadline".to_string(),
            };
        }
    }

    async fn retry_and_settle_terminal_slots(
        &self,
        report: &mut ShutdownReport,
        deadline: Option<tokio::time::Instant>,
    ) {
        for slot in self.inner.runtime.take_turn_slots() {
            let terminal = slot
                .pending_terminal
                .unwrap_or_else(|| PendingTerminal::interrupted(slot.handle.receipt.clone()));
            let result = match deadline {
                Some(deadline) => {
                    match tokio::time::timeout_at(deadline, terminal.persist(&self.inner.state))
                        .await
                    {
                        Ok(result) => result,
                        Err(_) => Err(Error::TerminalPersistence {
                            turn_id: terminal.receipt.turn_id.clone(),
                            message: "force-shutdown deadline elapsed".to_string(),
                        }),
                    }
                }
                None => terminal.persist(&self.inner.state).await,
            };
            match result {
                Ok(()) => {
                    if slot.phase == TurnPhase::Active {
                        slot.handle.events.push(terminal.terminal_event.clone());
                        slot.handle.completion.settle(terminal.completion.clone());
                    }
                }
                Err(error) => {
                    report
                        .pending_terminal_failures
                        .push(PendingTerminalFailure {
                            turn_id: terminal.receipt.turn_id.clone(),
                            message: error.to_string(),
                        });
                    if slot.phase == TurnPhase::Active {
                        let message: Arc<str> = Arc::from(format!(
                            "failed to persist Framework Turn terminal: {error}"
                        ));
                        slot.handle.events.push(TurnEvent::Warning {
                            data: serde_json::json!({
                                "kind": "framework_terminal_persistence",
                                "message": message.as_ref(),
                                "turnId": terminal.receipt.turn_id,
                            }),
                        });
                        slot.handle.completion.settle(Err(message));
                    }
                }
            }
            slot.handle.control.abort();
            slot.handle.events.close();
        }
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __state_runtime(&self) -> StateRuntime {
        self.inner.state.clone()
    }
}

#[derive(Default)]
pub struct ApplicationBuilder {
    home: Option<PathBuf>,
    database_path: Option<PathBuf>,
    state: Option<StateRuntime>,
    config_path: Option<PathBuf>,
    event_capacity: Option<usize>,
    agent_sessions: Option<Arc<dyn AgentSessionAdapter>>,
    provider: Option<Arc<dyn psychevo_ai::GenerationProvider>>,
}

impl fmt::Debug for ApplicationBuilder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApplicationBuilder")
            .field("home", &self.home)
            .field("database_path", &self.database_path)
            .field("has_state_runtime", &self.state.is_some())
            .field("config_path", &self.config_path)
            .field("event_capacity", &self.event_capacity)
            .field("has_agent_session_adapter", &self.agent_sessions.is_some())
            .field("has_provider", &self.provider.is_some())
            .finish()
    }
}

impl ApplicationBuilder {
    pub fn home(mut self, home: impl Into<PathBuf>) -> Self {
        self.home = Some(home.into());
        self
    }

    pub fn database_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.database_path = Some(path.into());
        self
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __state_runtime(mut self, state: StateRuntime) -> Self {
        self.state = Some(state);
        self
    }

    pub fn config_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config_path = Some(path.into());
        self
    }

    pub fn event_capacity(mut self, capacity: usize) -> Self {
        self.event_capacity = Some(capacity);
        self
    }

    pub fn agent_session_adapter(mut self, adapter: Arc<dyn AgentSessionAdapter>) -> Self {
        self.agent_sessions = Some(adapter);
        self
    }

    pub fn provider(mut self, provider: Arc<dyn psychevo_ai::GenerationProvider>) -> Self {
        self.provider = Some(provider);
        self
    }

    pub async fn build(self) -> Result<Application> {
        let home = self.home.ok_or_else(|| {
            Error::Message(
                "ApplicationBuilder requires an explicit Psychevo home directory".to_string(),
            )
        })?;
        if self.state.is_some() && self.database_path.is_some() {
            return Err(Error::Message(
                "ApplicationBuilder accepts either database_path or an existing state runtime, not both"
                    .to_string(),
            ));
        }
        let database_path = self.database_path.unwrap_or_else(|| home.join("state.db"));
        let event_capacity = self.event_capacity.unwrap_or(DEFAULT_EVENT_CAPACITY);
        if event_capacity == 0 {
            return Err(Error::Message(
                "Application event capacity must be greater than zero".to_string(),
            ));
        }
        let state = match self.state {
            Some(state) => state,
            None => StateRuntime::open(database_path).await?,
        };
        let agent_sessions = self.agent_sessions.unwrap_or_else(|| {
            Arc::new(NativeAgentSessionAdapter {
                state: state.clone(),
                config_path: self.config_path.clone(),
                provider: self.provider,
            })
        });
        Ok(Application {
            inner: Arc::new(ApplicationInner {
                state,
                agent_sessions,
                home,
                config_path: self.config_path,
                event_capacity,
                admission: Mutex::new(true),
                admission_gate: Arc::new(AsyncRwLock::new(())),
                force_shutdown_requested: AtomicBool::new(false),
                force_shutdown_notify: Notify::new(),
                shutdown_complete: Mutex::new(None),
                shutdown_finalizer: AsyncMutex::new(()),
                runtime: Arc::new(ApplicationRuntime::new()),
            }),
        })
    }
}

#[derive(Clone)]
pub struct Client {
    inner: Arc<ApplicationInner>,
}

impl fmt::Debug for Client {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Client")
            .field("home", &self.inner.home)
            .finish_non_exhaustive()
    }
}

impl Client {
    pub async fn start_thread(&self, request: StartThreadRequest) -> Result<Thread> {
        self.ensure_open()?;
        let cwd = canonicalize_cwd(&request.cwd)?;
        let id = self
            .inner
            .state
            .create_session_with_metadata(
                &cwd,
                &request.source,
                "pending",
                "pending",
                request.metadata,
            )
            .await?;
        Ok(Thread {
            client: self.clone(),
            id,
        })
    }

    pub async fn resume_thread(&self, id: impl Into<String>) -> Result<Thread> {
        self.ensure_open()?;
        let id = id.into();
        self.inner
            .state
            .session_summary(&id)
            .await?
            .ok_or_else(|| Error::Message(format!("thread not found: {id}")))?;
        Ok(Thread {
            client: self.clone(),
            id,
        })
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

    pub async fn resume_turn(&self, id: impl Into<String>) -> Result<TurnHandle> {
        self.ensure_open()?;
        let id = id.into();
        if let Some(pending) = self.inner.runtime.pending_terminal(&id) {
            pending.persist(&self.inner.state).await.map_err(|error| {
                Error::TerminalPersistence {
                    turn_id: id.clone(),
                    message: error.to_string(),
                }
            })?;
            self.inner.runtime.remove_pending_terminal(&id);
            return Ok(pending.completed_handle());
        }
        if let Some(handle) = self.inner.runtime.turn_handle(&id) {
            return Ok(handle);
        }
        let Some(terminal) = self.inner.state.gateway_turn_terminal(&id).await? else {
            return if self.inner.state.gateway_turn_delivery(&id).await?.is_some() {
                Err(Error::OutcomeIndeterminate { turn_id: id })
            } else {
                Err(Error::Message(format!("turn not found: {id}")))
            };
        };
        let metadata = terminal.metadata.unwrap_or(Value::Null);
        let receipt = serde_json::from_value::<TurnReceipt>(
            metadata
                .get("frameworkReceipt")
                .cloned()
                .ok_or_else(|| Error::Message(format!("turn is not a Framework turn: {id}")))?,
        )?;
        match metadata.get("frameworkResult").cloned() {
            Some(result) if !result.is_null() => Ok(TurnHandle::completed(
                receipt,
                serde_json::from_value::<TurnResult>(result)?,
            )),
            _ if terminal.status == "failed" => Ok(TurnHandle::failed(
                receipt,
                terminal
                    .error_message
                    .unwrap_or_else(|| "Framework Turn failed".to_string()),
            )),
            _ => Err(Error::Message(format!(
                "Framework turn has no durable result: {id}"
            ))),
        }
    }

    fn ensure_open(&self) -> Result<()> {
        if *self
            .inner
            .admission
            .lock()
            .expect("application admission poisoned")
        {
            Ok(())
        } else {
            Err(Error::Message(
                "Psychevo Application is shutting down".to_string(),
            ))
        }
    }

    fn application_environment(
        &self,
        inherited: Option<BTreeMap<String, String>>,
    ) -> BTreeMap<String, String> {
        let mut environment = inherited.unwrap_or_else(|| std::env::vars().collect());
        environment.insert(
            "PSYCHEVO_HOME".to_string(),
            self.inner.home.to_string_lossy().into_owned(),
        );
        environment
    }

    fn summary_from_summary(&self, summary: SessionSummary) -> ThreadSummary {
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

#[derive(Debug, Clone)]
pub struct StartThreadRequest {
    pub cwd: PathBuf,
    pub source: String,
    pub metadata: Option<Value>,
}

impl StartThreadRequest {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            source: "sdk".to_string(),
            metadata: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ThreadListQuery {
    pub cwd: Option<PathBuf>,
    pub archived: bool,
    pub sources: Vec<String>,
    pub cursor: Option<String>,
    pub limit: usize,
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

#[derive(Debug, Clone, PartialEq)]
pub struct ThreadListPage {
    pub threads: Vec<ThreadSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadListCursor {
    cwd: Option<String>,
    archived: bool,
    sources: Vec<String>,
    position: SessionListCursor,
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

#[derive(Clone)]
pub struct Thread {
    client: Client,
    id: String,
}

impl fmt::Debug for Thread {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Thread")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
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

    pub async fn start_turn(&self, mut request: TurnRequest) -> Result<TurnHandle> {
        let admission_gate = self.client.inner.admission_gate.clone().read_owned().await;
        self.client.ensure_open()?;
        request.inherited_env = Some(
            self.client
                .application_environment(request.inherited_env.take()),
        );
        request.adapter_options.mcp_runtime = Some(self.client.inner.runtime.mcp_runtime(&self.id));
        let receipt = TurnReceipt {
            accepted: true,
            thread_id: self.id.clone(),
            turn_id: request
                .requested_turn_id
                .take()
                .unwrap_or_else(|| Uuid::now_v7().to_string()),
            client_turn_id: request.client_turn_id.clone(),
        };
        let durable_input = serde_json::to_string(&serde_json::json!({
            "prompt": request.prompt,
            "imageCount": request.image_inputs.len(),
            "clientTurnId": request.client_turn_id,
            "source": request.source,
            "model": request.model,
            "reasoningEffort": request.reasoning_effort,
            "runtimeRef": request.runtime_ref,
        }))?;
        let durable_input_hash = format!("{:x}", Sha256::digest(durable_input.as_bytes()));
        let runtime_ref = request
            .runtime_ref
            .as_deref()
            .unwrap_or("native")
            .to_string();
        let client_turn_id = request
            .client_turn_id
            .as_deref()
            .map(str::trim)
            .filter(|client_turn_id| !client_turn_id.is_empty())
            .map(ToOwned::to_owned);
        let event_observer = request.adapter_options.turn_event_observer.take();
        let events = Arc::new(EventLog::new(self.client.inner.event_capacity));
        let (control_handle, control) = request
            .prepared_control
            .take()
            .map(|prepared| (prepared.handle, prepared.control))
            .unwrap_or_else(run_control);
        let interactions = FrameworkInteractionControl::default();
        let completion = TurnCompletion::pending();
        let task_completion = completion.clone();
        let client = self.client.clone();
        let task_client = client.clone();
        let thread_id = self.id.clone();
        let turn_id = receipt.turn_id.clone();
        let task_receipt = receipt.clone();
        let task_events = Arc::clone(&events);
        let task_control_handle = control_handle.clone();
        let task_interactions = interactions.clone();
        let agent_sessions = Arc::clone(&client.inner.agent_sessions);
        let state = client.inner.state.clone();
        let interaction_broker = InteractionBroker::new(
            state.clone(),
            client.inner.runtime.clone(),
            Arc::clone(&events),
            interactions.clone(),
            control_handle.clone(),
            thread_id.clone(),
            turn_id.clone(),
        );
        let task_interaction_broker = interaction_broker.clone();
        let (acceptance_tx, acceptance_rx) = oneshot::channel();
        let handle = TurnHandle {
            receipt: receipt.clone(),
            events,
            completion,
            control: control_handle,
            interaction_broker: Some(interaction_broker),
        };
        let lane = client
            .inner
            .runtime
            .register_turn(&thread_id, &turn_id, handle.clone())?;

        if let Some(observer) = event_observer {
            let mut stream = handle.events();
            client.inner.runtime.spawn(async move {
                while let Some(event) = stream.next().await {
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observer(event)))
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }
        {
            let spawned_turn_id = turn_id.clone();
            let task = client.inner.runtime.spawn(async move {
                let acceptance = state
                    .accept_gateway_turn(
                        GatewayTurnDeliveryInput {
                            turn_id: &turn_id,
                            thread_id: &thread_id,
                            runtime_ref: &runtime_ref,
                            input_json: &durable_input,
                            input_hash: &durable_input_hash,
                        },
                        client_turn_id.as_deref(),
                    )
                    .await;
                if let Err(error) = acceptance {
                    let message: Arc<str> = Arc::from(error.to_string());
                    task_client
                        .inner
                        .runtime
                        .settle_turn(&thread_id, &turn_id, None);
                    task_interactions.cancel_permissions();
                    task_interaction_broker.finish().await;
                    task_events.close();
                    task_completion.settle(Err(message));
                    let _ = acceptance_tx.send(Err(error));
                    return;
                }
                task_events.push(TurnEvent::Accepted {
                    receipt: task_receipt.clone(),
                });
                let _ = acceptance_tx.send(Ok(()));
                drop(admission_gate);
                if lane.await.is_err() {
                    let message: Arc<str> = Arc::from("Thread operation reservation was cancelled");
                    task_client
                        .inner
                        .runtime
                        .settle_turn(&thread_id, &turn_id, None);
                    task_interactions.cancel_permissions();
                    task_interaction_broker.finish().await;
                    task_events.close();
                    task_completion.settle(Err(message));
                    return;
                }
                task_events.push(TurnEvent::Started {
                    thread_id: thread_id.clone(),
                    turn_id: turn_id.clone(),
                });
                let result = std::panic::AssertUnwindSafe(async {
                    let summary = state
                        .session_summary(&thread_id)
                        .await?
                        .ok_or_else(|| Error::Message(format!("thread not found: {thread_id}")))?;
                    let thread = ThreadExecutionContext::from_summary(summary);
                    let history = HistoryReader::new(state.clone(), thread_id.clone());
                    let event_sender = TurnEventSender {
                        log: Arc::clone(&task_events),
                        interactions: task_interaction_broker.clone(),
                    };
                    request.approval_handler = Some(Arc::new(FrameworkApprovalHandler {
                        delegate: request.approval_handler.take(),
                        interactions: task_interactions.clone(),
                        broker: task_interaction_broker.clone(),
                    }));
                    agent_sessions
                        .run_turn(AgentTurnRequest {
                            thread,
                            history,
                            receipt: task_receipt.clone(),
                            input: request,
                            events: event_sender,
                            control: TurnControl {
                                handle: task_control_handle,
                                interactions: task_interaction_broker.clone(),
                            },
                            native_control: Some(control),
                        })
                        .await
                })
                .catch_unwind()
                .await
                .unwrap_or_else(|_| {
                    Err(Error::Message(
                        "Agent Session Adapter panicked while running the Turn".to_string(),
                    ))
                });
                let (shared, terminal_event) = match result {
                    Ok(result) => {
                        let event = TurnEvent::Completed {
                            thread_id: thread_id.clone(),
                            turn_id: turn_id.clone(),
                            outcome: result.outcome,
                        };
                        (Ok(Arc::new(result)), event)
                    }
                    Err(error) => {
                        let message: Arc<str> = Arc::from(error.to_string());
                        let event = TurnEvent::Failed {
                            thread_id: thread_id.clone(),
                            turn_id: turn_id.clone(),
                            message: message.to_string(),
                        };
                        (Err(message), event)
                    }
                };
                let completed_at_ms = psychevo_agent_core::now_ms();
                let terminal = PendingTerminal {
                    receipt: task_receipt.clone(),
                    completion: shared.clone(),
                    terminal_event: terminal_event.clone(),
                    completed_at_ms,
                    last_error: String::new(),
                };
                let finalization = terminal.persist(&state).await;
                task_interactions.cancel_permissions();
                task_interaction_broker.finish().await;
                let pending_terminal = finalization.as_ref().err().map(|error| {
                    let mut terminal = terminal.clone();
                    terminal.last_error = error.to_string();
                    terminal
                });
                let completion = match finalization {
                    Ok(()) => {
                        task_events.push(terminal_event);
                        shared
                    }
                    Err(error) => {
                        let message: Arc<str> = Arc::from(format!(
                            "failed to persist Framework Turn terminal: {error}"
                        ));
                        task_events.push(TurnEvent::Warning {
                            data: serde_json::json!({
                                "kind": "framework_terminal_persistence",
                                "message": message.as_ref(),
                                "turnId": turn_id,
                            }),
                        });
                        Err(message)
                    }
                };
                task_client
                    .inner
                    .runtime
                    .settle_turn(&thread_id, &turn_id, pending_terminal);
                task_events.close();
                task_completion.settle(completion);
            });
            client.inner.runtime.set_turn_abort(&spawned_turn_id, task);
        }

        acceptance_rx.await.map_err(|_| {
            Error::Message("accepted Turn admission task ended without a receipt".to_string())
        })??;
        Ok(handle)
    }

    pub async fn archive(&self) -> Result<()> {
        let _mutation = self
            .client
            .inner
            .runtime
            .reserve_mutation(&self.id)
            .acquire()
            .await?;
        self.client.inner.state.archive_session(&self.id).await?;
        self.client.inner.runtime.remove_mcp_runtime(&self.id);
        Ok(())
    }

    pub async fn delete(&self) -> Result<()> {
        let _mutation = self
            .client
            .inner
            .runtime
            .reserve_mutation(&self.id)
            .acquire()
            .await?;
        self.client.inner.state.delete_session(&self.id).await?;
        self.client.inner.runtime.remove_mcp_runtime(&self.id);
        Ok(())
    }

    pub async fn compact(&self, request: CompactThreadRequest) -> Result<CompactionResult> {
        self.enqueue_compact(request).await
    }

    fn enqueue_compact(
        &self,
        request: CompactThreadRequest,
    ) -> BoxFuture<'static, Result<CompactionResult>> {
        let mutation = self.client.inner.runtime.reserve_mutation(&self.id);
        let thread = self.clone();
        Box::pin(async move {
            let _mutation = mutation.acquire().await?;
            thread.compact_reserved(request).await
        })
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
            reason: CompactionReason::Manual,
            instructions: request.instructions,
            force: request.force,
        })
        .await
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __enqueue_compact(
        &self,
        request: CompactThreadRequest,
    ) -> BoxFuture<'static, Result<CompactionResult>> {
        self.enqueue_compact(request)
    }

    pub async fn fork(&self, request: ForkThreadRequest) -> Result<Thread> {
        let _mutation = self
            .client
            .inner
            .runtime
            .reserve_mutation(&self.id)
            .acquire()
            .await?;
        let id = self
            .client
            .inner
            .state
            .fork_native_session_history(NativeSessionForkInput {
                source_session_id: &self.id,
                before_session_seq: request.before_session_seq,
            })
            .await?;
        Ok(Thread {
            client: self.client.clone(),
            id,
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

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __activity(&self) -> (bool, Option<String>, usize) {
        self.client.inner.runtime.thread_activity(&self.id)
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __interrupt_all(&self) -> (bool, usize) {
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

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __steer(&self, expected_turn_id: &str, input: impl Into<String>) -> bool {
        let (_, active_turn_id, _) = self.client.inner.runtime.thread_activity(&self.id);
        if active_turn_id.as_deref() != Some(expected_turn_id) {
            return false;
        }
        self.client
            .inner
            .runtime
            .turn_handle(expected_turn_id)
            .is_some_and(|turn| turn.steer(input))
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
    fn has_activity(&self) -> bool {
        self.client.inner.runtime.thread_activity(&self.id).0
    }
}

#[derive(Debug, Clone, Default)]
pub struct CompactThreadRequest {
    pub config_path: Option<PathBuf>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub inherited_env: Option<BTreeMap<String, String>>,
    pub instructions: Option<String>,
    pub force: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ForkThreadRequest {
    pub before_session_seq: Option<i64>,
}

pub trait AgentSessionAdapter: Send + Sync + fmt::Debug {
    fn run_turn(&self, request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>>;

    fn shutdown(&self, _force: bool) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

pub struct AgentTurnRequest {
    pub thread: ThreadExecutionContext,
    pub history: HistoryReader,
    pub receipt: TurnReceipt,
    pub input: TurnRequest,
    pub events: TurnEventSender,
    pub control: TurnControl,
    native_control: Option<crate::types::RunControl>,
}

impl fmt::Debug for AgentTurnRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentTurnRequest")
            .field("thread", &self.thread)
            .field("receipt", &self.receipt)
            .field("input", &self.input)
            .finish_non_exhaustive()
    }
}

impl AgentTurnRequest {
    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __take_native_control(&mut self) -> Result<crate::types::RunControl> {
        self.native_control.take().ok_or_else(|| {
            Error::Message("Agent Session Adapter is missing its Turn control".to_string())
        })
    }
}

#[derive(Clone)]
pub struct TurnEventSender {
    log: Arc<EventLog>,
    interactions: InteractionBroker,
}

impl TurnEventSender {
    pub fn emit(&self, event: TurnEvent) {
        if matches!(
            event,
            TurnEvent::InteractionRequested { .. } | TurnEvent::InteractionResolved { .. }
        ) {
            self.interactions.observe(event);
        } else {
            self.log.push(event);
        }
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __emit_run_stream(&self, event: RunStreamEvent) {
        if let Some(event) = TurnEvent::from_run_stream(event) {
            self.emit(event);
        }
    }
}

impl fmt::Debug for TurnEventSender {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TurnEventSender(..)")
    }
}

#[derive(Clone)]
pub struct TurnControl {
    handle: crate::types::RunControlHandle,
    interactions: InteractionBroker,
}

impl TurnControl {
    pub fn is_interrupted(&self) -> bool {
        self.handle.inner.is_aborted()
    }

    pub async fn respond(
        &self,
        interaction_id: &str,
        response: InteractionResponse,
    ) -> Result<InteractionResponseReceipt> {
        self.interactions.respond(interaction_id, response).await
    }
}

impl fmt::Debug for TurnControl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TurnControl(..)")
    }
}

#[derive(Clone)]
struct NativeAgentSessionAdapter {
    state: StateRuntime,
    config_path: Option<PathBuf>,
    provider: Option<Arc<dyn psychevo_ai::GenerationProvider>>,
}

impl fmt::Debug for NativeAgentSessionAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeAgentSessionAdapter")
            .field("config_path", &self.config_path)
            .finish_non_exhaustive()
    }
}

impl AgentSessionAdapter for NativeAgentSessionAdapter {
    fn run_turn(&self, mut request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
        let state = self.state.clone();
        let application_config_path = self.config_path.clone();
        let provider = self.provider.clone();
        Box::pin(async move {
            let source = request.input.source.clone();
            let turn_id = request.receipt.turn_id.clone();
            let stream_events = request.events.clone();
            let run_stream_observer = request.input.adapter_options.run_stream_observer.take();
            let stream: RunStreamSink = Arc::new(move |event| {
                if let Some(observer) = run_stream_observer.as_ref() {
                    observer(event.clone());
                }
                if let Some(event) = TurnEvent::from_run_stream(event) {
                    stream_events.emit(event);
                }
            });
            let options = request.input.into_run_options(
                state.clone(),
                PathBuf::from(request.thread.cwd.clone()),
                request.receipt.thread_id,
                application_config_path,
            );
            let control = request.native_control.take().ok_or_else(|| {
                Error::Message("Native Agent Session is missing its Turn control".to_string())
            })?;
            state.confirm_gateway_turn_delivery(&turn_id).await?;
            match provider {
                Some(provider) => {
                    run_live_streaming_controlled_with_provider(
                        options,
                        &source,
                        &[source.as_str()],
                        stream,
                        control,
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
                        control,
                    )
                    .await
                }
            }
            .map(TurnResult::from)
        })
    }
}

#[derive(Debug)]
pub struct TurnRequest {
    prompt: String,
    image_inputs: Vec<ImageInput>,
    extract_prompt_image_sources: bool,
    prompt_display: Option<crate::types::PromptDisplayMetadata>,
    client_turn_id: Option<String>,
    source: String,
    config_path: Option<PathBuf>,
    model: Option<String>,
    reasoning_effort: Option<String>,
    runtime_ref: Option<String>,
    runtime_options: BTreeMap<String, String>,
    include_reasoning: bool,
    mode: RunMode,
    permission_mode: Option<PermissionMode>,
    approval_mode: Option<ApprovalMode>,
    approval_handler: Option<Arc<dyn ApprovalHandler>>,
    clarify_enabled: bool,
    inherited_env: Option<BTreeMap<String, String>>,
    project_context: Option<ProjectContextInstructionMode>,
    sandbox: Option<RunSandboxOverride>,
    agent: Option<String>,
    no_agents: bool,
    no_skills: bool,
    skill_inputs: Vec<String>,
    mcp_servers: Vec<McpServerInput>,
    tools: Vec<RuntimeTool>,
    adapter_options: AdapterTurnOptions,
    requested_turn_id: Option<String>,
    prepared_control: Option<PreparedTurnControl>,
}

#[doc(hidden)]
#[derive(Default)]
pub struct AdapterTurnOptions {
    pub snapshot_root: Option<PathBuf>,
    pub max_context_messages: Option<usize>,
    pub selected_capability_roots: Vec<crate::extensions::SelectedCapabilityRoot>,
    pub workspace_mutations: Option<crate::types::WorkspaceMutationSink>,
    pub input_parts: Vec<Value>,
    pub run_stream_observer: Option<RunStreamSink>,
    pub initial_thread_preferences: BTreeMap<String, String>,
    pub prepared_source_key: Option<String>,
    pub turn_event_observer: Option<Arc<dyn Fn(TurnEvent) + Send + Sync>>,
    pub agent_entrypoint: Option<crate::agents::AgentEntrypoint>,
    #[doc(hidden)]
    pub mcp_runtime: Option<crate::mcp::McpRuntime>,
}

impl fmt::Debug for AdapterTurnOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdapterTurnOptions")
            .field("snapshot_root", &self.snapshot_root)
            .field("max_context_messages", &self.max_context_messages)
            .field(
                "selected_capability_root_count",
                &self.selected_capability_roots.len(),
            )
            .field(
                "has_workspace_mutations",
                &self.workspace_mutations.is_some(),
            )
            .field("input_part_count", &self.input_parts.len())
            .field(
                "has_run_stream_observer",
                &self.run_stream_observer.is_some(),
            )
            .field(
                "initial_thread_preference_count",
                &self.initial_thread_preferences.len(),
            )
            .field(
                "has_prepared_source_key",
                &self.prepared_source_key.is_some(),
            )
            .field(
                "has_turn_event_observer",
                &self.turn_event_observer.is_some(),
            )
            .field("agent_entrypoint", &self.agent_entrypoint)
            .finish()
    }
}

struct PreparedTurnControl {
    handle: crate::types::RunControlHandle,
    control: crate::types::RunControl,
}

impl fmt::Debug for PreparedTurnControl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PreparedTurnControl(..)")
    }
}

impl TurnRequest {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            image_inputs: Vec::new(),
            extract_prompt_image_sources: true,
            prompt_display: None,
            client_turn_id: None,
            source: "sdk".to_string(),
            config_path: None,
            model: None,
            reasoning_effort: None,
            runtime_ref: None,
            runtime_options: BTreeMap::new(),
            include_reasoning: false,
            mode: RunMode::default(),
            permission_mode: None,
            approval_mode: None,
            approval_handler: None,
            clarify_enabled: false,
            inherited_env: None,
            project_context: None,
            sandbox: None,
            agent: None,
            no_agents: false,
            no_skills: false,
            skill_inputs: Vec::new(),
            mcp_servers: Vec::new(),
            tools: Vec::new(),
            adapter_options: AdapterTurnOptions::default(),
            requested_turn_id: None,
            prepared_control: None,
        }
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    pub fn image_inputs(&self) -> &[ImageInput] {
        &self.image_inputs
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn clarify_enabled(&self) -> bool {
        self.clarify_enabled
    }

    pub fn with_prompt_images(
        mut self,
        image_inputs: Vec<ImageInput>,
        extract_prompt_image_sources: bool,
    ) -> Self {
        self.image_inputs = image_inputs;
        self.extract_prompt_image_sources = extract_prompt_image_sources;
        self
    }

    pub fn with_prompt_display(
        mut self,
        prompt_display: Option<crate::types::PromptDisplayMetadata>,
    ) -> Self {
        self.prompt_display = prompt_display;
        self
    }

    pub fn with_identity(
        mut self,
        source: impl Into<String>,
        client_turn_id: Option<String>,
    ) -> Self {
        self.source = source.into();
        self.client_turn_id = client_turn_id;
        self
    }

    pub fn with_model(mut self, model: Option<String>, reasoning_effort: Option<String>) -> Self {
        self.model = model;
        self.reasoning_effort = reasoning_effort;
        self
    }

    pub fn with_runtime(
        mut self,
        runtime_ref: Option<String>,
        runtime_options: BTreeMap<String, String>,
    ) -> Self {
        self.runtime_ref = runtime_ref;
        self.runtime_options = runtime_options;
        self
    }

    pub fn with_reasoning_output(mut self, include_reasoning: bool) -> Self {
        self.include_reasoning = include_reasoning;
        self
    }

    pub fn with_execution_policy(
        mut self,
        mode: RunMode,
        permission_mode: Option<PermissionMode>,
        config_path: Option<PathBuf>,
    ) -> Self {
        self.mode = mode;
        self.permission_mode = permission_mode;
        self.config_path = config_path;
        self
    }

    pub fn with_approval(
        mut self,
        approval_mode: Option<ApprovalMode>,
        approval_handler: Option<Arc<dyn ApprovalHandler>>,
        clarify_enabled: bool,
    ) -> Self {
        self.approval_mode = approval_mode;
        self.approval_handler = approval_handler;
        self.clarify_enabled = clarify_enabled;
        self
    }

    pub fn with_environment(
        mut self,
        inherited_env: Option<BTreeMap<String, String>>,
        project_context: Option<ProjectContextInstructionMode>,
        sandbox: Option<RunSandboxOverride>,
    ) -> Self {
        self.inherited_env = inherited_env;
        self.project_context = project_context;
        self.sandbox = sandbox;
        self
    }

    pub fn with_agent(mut self, agent: Option<String>, no_agents: bool, no_skills: bool) -> Self {
        self.agent = agent;
        self.no_agents = no_agents;
        self.no_skills = no_skills;
        self
    }

    pub fn with_skills(mut self, skill_inputs: Vec<String>) -> Self {
        self.skill_inputs = skill_inputs;
        self
    }

    pub fn with_mcp_servers(mut self, mcp_servers: Vec<McpServerInput>) -> Self {
        self.mcp_servers = mcp_servers;
        self
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __set_runtime_tools(&mut self, tools: Vec<RuntimeTool>) {
        self.tools = tools;
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __from_run_options(
        options: RunOptions,
        source: impl Into<String>,
        run_stream_observer: Option<RunStreamSink>,
    ) -> Self {
        Self {
            prompt: options.prompt,
            image_inputs: options.image_inputs,
            extract_prompt_image_sources: options.extract_prompt_image_sources,
            prompt_display: options.prompt_display,
            client_turn_id: None,
            source: source.into(),
            config_path: options.config_path,
            model: options.model,
            reasoning_effort: options.reasoning_effort,
            runtime_ref: options.runtime_ref,
            runtime_options: options.runtime_options,
            include_reasoning: options.include_reasoning,
            mode: options.mode,
            permission_mode: options.permission_mode,
            approval_mode: options.approval_mode,
            approval_handler: options.approval_handler,
            clarify_enabled: options.clarify_enabled,
            inherited_env: options.inherited_env,
            project_context: options.project_context_override,
            sandbox: options.sandbox_override,
            agent: options.agent,
            no_agents: options.no_agents,
            no_skills: options.no_skills,
            skill_inputs: options.skill_inputs,
            mcp_servers: options.mcp_servers,
            tools: options.runtime_tools,
            adapter_options: AdapterTurnOptions {
                snapshot_root: options.snapshot_root,
                max_context_messages: options.max_context_messages,
                selected_capability_roots: options.selected_capability_roots,
                workspace_mutations: options.workspace_mutations,
                run_stream_observer,
                ..AdapterTurnOptions::default()
            },
            requested_turn_id: None,
            prepared_control: None,
        }
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __set_control(
        &mut self,
        handle: crate::types::RunControlHandle,
        control: crate::types::RunControl,
    ) {
        self.prepared_control = Some(PreparedTurnControl { handle, control });
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __set_adapter_options(&mut self, options: AdapterTurnOptions) {
        self.adapter_options = options;
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __take_adapter_input_parts(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.adapter_options.input_parts)
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __take_run_stream_observer(&mut self) -> Option<RunStreamSink> {
        self.adapter_options.run_stream_observer.take()
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __take_initial_thread_preferences(&mut self) -> BTreeMap<String, String> {
        std::mem::take(&mut self.adapter_options.initial_thread_preferences)
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __take_prepared_source_key(&mut self) -> Option<String> {
        self.adapter_options.prepared_source_key.take()
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __take_agent_entrypoint(&mut self) -> Option<crate::agents::AgentEntrypoint> {
        self.adapter_options.agent_entrypoint.take()
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __set_agent_entrypoint(&mut self, entrypoint: crate::agents::AgentEntrypoint) {
        self.adapter_options.agent_entrypoint = Some(entrypoint);
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __set_turn_id(&mut self, turn_id: String) {
        self.requested_turn_id = Some(turn_id);
    }

    pub fn tool(mut self, tool: Arc<dyn psychevo_agent_core::ToolBinding>) -> Self {
        self.tools.push(RuntimeTool::new(tool));
        self
    }

    fn into_run_options(
        self,
        state: StateRuntime,
        cwd: PathBuf,
        thread_id: String,
        application_config_path: Option<PathBuf>,
    ) -> RunOptions {
        RunOptions {
            state,
            cwd,
            snapshot_root: self.adapter_options.snapshot_root,
            session: Some(thread_id),
            continue_latest: false,
            prompt: self.prompt,
            image_inputs: self.image_inputs,
            extract_prompt_image_sources: self.extract_prompt_image_sources,
            prompt_display: self.prompt_display,
            max_context_messages: self.adapter_options.max_context_messages,
            config_path: self.config_path.or(application_config_path),
            project_context_override: self.project_context,
            sandbox_override: self.sandbox,
            model: self.model,
            reasoning_effort: self.reasoning_effort,
            runtime_ref: self.runtime_ref,
            runtime_session_id: None,
            runtime_options: self.runtime_options,
            include_reasoning: self.include_reasoning,
            mode: self.mode,
            permission_mode: self.permission_mode,
            approval_mode: self.approval_mode,
            approval_handler: self.approval_handler,
            clarify_enabled: self.clarify_enabled,
            inherited_env: self.inherited_env,
            agent: self.agent,
            external_agent_delegate: None,
            no_agents: self.no_agents,
            no_skills: self.no_skills,
            selected_capability_roots: self.adapter_options.selected_capability_roots,
            skill_inputs: self.skill_inputs,
            mcp_servers: self.mcp_servers,
            mcp_runtime: self.adapter_options.mcp_runtime,
            workspace_mutations: self.adapter_options.workspace_mutations,
            runtime_tools: self.tools,
        }
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __into_run_options(
        self,
        state: StateRuntime,
        cwd: PathBuf,
        thread_id: String,
        application_config_path: Option<PathBuf>,
    ) -> RunOptions {
        self.into_run_options(state, cwd, thread_id, application_config_path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnReceipt {
    pub accepted: bool,
    pub thread_id: String,
    pub turn_id: String,
    pub client_turn_id: Option<String>,
}

type SharedTurnCompletion = std::result::Result<Arc<TurnResult>, Arc<str>>;

#[derive(Clone)]
struct PendingTerminal {
    receipt: TurnReceipt,
    completion: SharedTurnCompletion,
    terminal_event: TurnEvent,
    completed_at_ms: i64,
    last_error: String,
}

impl PendingTerminal {
    fn interrupted(receipt: TurnReceipt) -> Self {
        let result = TurnResult {
            thread_id: receipt.thread_id.clone(),
            outcome: TurnOutcome::Interrupted,
            final_answer: String::new(),
            provider: "application".to_string(),
            model: "forced-shutdown".to_string(),
            reasoning_effort: None,
            tool_failures: 0,
            context_limit: None,
            context_snapshot: None,
            warnings: Vec::new(),
            terminal_reason: None,
            terminal_error: None,
            selected_agent: None,
            selected_skills: Vec::new(),
        };
        Self {
            terminal_event: TurnEvent::Completed {
                thread_id: receipt.thread_id.clone(),
                turn_id: receipt.turn_id.clone(),
                outcome: TurnOutcome::Interrupted,
            },
            receipt,
            completion: Ok(Arc::new(result)),
            completed_at_ms: psychevo_agent_core::now_ms(),
            last_error: String::new(),
        }
    }

    async fn persist(&self, state: &StateRuntime) -> Result<()> {
        match &self.completion {
            Ok(result) => {
                let framework_result = serde_json::to_value(result.as_ref())?;
                let (status, outcome) = gateway_terminal_facts(result.outcome);
                state
                    .finalize_framework_turn(
                        GatewayTurnTerminalInput {
                            turn_id: &self.receipt.turn_id,
                            thread_id: &self.receipt.thread_id,
                            status,
                            outcome: Some(outcome),
                            error_message: None,
                            started_at_ms: None,
                            completed_at_ms: self.completed_at_ms,
                            metadata: Some(serde_json::json!({
                                "source": "framework",
                                "frameworkReceipt": self.receipt,
                                "frameworkResult": framework_result,
                            })),
                        },
                        "turn_finished",
                    )
                    .await
            }
            Err(message) => {
                state
                    .finalize_framework_turn(
                        GatewayTurnTerminalInput {
                            turn_id: &self.receipt.turn_id,
                            thread_id: &self.receipt.thread_id,
                            status: "failed",
                            outcome: Some("failed"),
                            error_message: Some(message.as_ref()),
                            started_at_ms: None,
                            completed_at_ms: self.completed_at_ms,
                            metadata: Some(serde_json::json!({
                                "source": "framework",
                                "frameworkReceipt": self.receipt,
                                "frameworkResult": Value::Null,
                            })),
                        },
                        "turn_finished",
                    )
                    .await
            }
        }
    }

    fn completed_handle(&self) -> TurnHandle {
        match &self.completion {
            Ok(result) => TurnHandle::completed(self.receipt.clone(), result.as_ref().clone()),
            Err(message) => TurnHandle::failed(self.receipt.clone(), message.to_string()),
        }
    }
}

struct TurnCompletion {
    value: Mutex<Option<SharedTurnCompletion>>,
    notify: Notify,
}

impl TurnCompletion {
    fn pending() -> Arc<Self> {
        Arc::new(Self {
            value: Mutex::new(None),
            notify: Notify::new(),
        })
    }

    fn ready(value: SharedTurnCompletion) -> Arc<Self> {
        Arc::new(Self {
            value: Mutex::new(Some(value)),
            notify: Notify::new(),
        })
    }

    fn settle(&self, value: SharedTurnCompletion) -> bool {
        let mut current = self.value.lock().expect("Turn completion poisoned");
        if current.is_some() {
            return false;
        }
        *current = Some(value);
        drop(current);
        self.notify.notify_waiters();
        true
    }

    async fn wait(&self) -> SharedTurnCompletion {
        loop {
            let notified = self.notify.notified();
            if let Some(value) = self.value.lock().expect("Turn completion poisoned").clone() {
                return value;
            }
            notified.await;
        }
    }
}

#[derive(Clone)]
pub struct TurnHandle {
    receipt: TurnReceipt,
    events: Arc<EventLog>,
    completion: Arc<TurnCompletion>,
    control: crate::types::RunControlHandle,
    interaction_broker: Option<InteractionBroker>,
}

impl fmt::Debug for TurnHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TurnHandle")
            .field("receipt", &self.receipt)
            .finish_non_exhaustive()
    }
}

impl TurnHandle {
    fn completed(receipt: TurnReceipt, result: TurnResult) -> Self {
        let events = Arc::new(EventLog::new(DEFAULT_EVENT_CAPACITY));
        events.push(TurnEvent::Accepted {
            receipt: receipt.clone(),
        });
        events.push(TurnEvent::Completed {
            thread_id: receipt.thread_id.clone(),
            turn_id: receipt.turn_id.clone(),
            outcome: result.outcome,
        });
        events.close();
        let result = Arc::new(result);
        let completion = TurnCompletion::ready(Ok(result));
        let (control, _) = run_control();
        Self {
            receipt,
            events,
            completion,
            control,
            interaction_broker: None,
        }
    }

    fn failed(receipt: TurnReceipt, message: String) -> Self {
        let events = Arc::new(EventLog::new(DEFAULT_EVENT_CAPACITY));
        events.push(TurnEvent::Accepted {
            receipt: receipt.clone(),
        });
        events.push(TurnEvent::Failed {
            thread_id: receipt.thread_id.clone(),
            turn_id: receipt.turn_id.clone(),
            message: message.clone(),
        });
        events.close();
        let completion = TurnCompletion::ready(Err(Arc::from(message)));
        let (control, _) = run_control();
        Self {
            receipt,
            events,
            completion,
            control,
            interaction_broker: None,
        }
    }

    pub fn receipt(&self) -> &TurnReceipt {
        &self.receipt
    }

    pub fn events(&self) -> TurnEventStream {
        TurnEventStream {
            log: Arc::clone(&self.events),
            cursor: 0,
        }
    }

    pub async fn wait(&self) -> Result<TurnResult> {
        match self.completion.wait().await {
            Ok(result) => Ok((*result).clone()),
            Err(message) => Err(Error::Message(message.to_string())),
        }
    }

    pub fn steer(&self, input: impl Into<String>) -> bool {
        self.__steer(input).is_some()
    }

    #[doc(hidden)]
    pub fn __steer(&self, input: impl Into<String>) -> Option<psychevo_agent_core::PendingInputId> {
        self.control
            .steer_user_message(psychevo_agent_core::user_text_message(input))
    }

    #[doc(hidden)]
    pub fn __cancel_steer(&self, id: psychevo_agent_core::PendingInputId) -> bool {
        self.control.cancel_pending_user_message(id)
    }

    pub fn interrupt(&self) {
        self.control.abort();
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __control_handle(&self) -> crate::types::RunControlHandle {
        self.control.clone()
    }

    pub async fn respond(
        &self,
        interaction_id: &str,
        response: InteractionResponse,
    ) -> Result<InteractionResponseReceipt> {
        match self.interaction_broker.as_ref() {
            Some(broker) => broker.respond(interaction_id, response).await,
            None => Ok(InteractionResponseReceipt { accepted: false }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum InteractionResponse {
    Permission(PermissionApprovalDecision),
    Clarify(Vec<Vec<String>>),
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionResponseReceipt {
    pub accepted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnOutcome {
    Completed,
    Stopped,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnResult {
    pub thread_id: String,
    pub outcome: TurnOutcome,
    pub final_answer: String,
    pub provider: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub tool_failures: usize,
    pub context_limit: Option<u64>,
    pub context_snapshot: Option<crate::context_usage::ContextSnapshot>,
    pub warnings: Vec<crate::types::RunWarning>,
    pub terminal_reason: Option<crate::__agent_core::TerminalReason>,
    pub terminal_error: Option<crate::types::RunTerminalError>,
    pub selected_agent: Option<crate::types::SelectedAgent>,
    pub selected_skills: Vec<crate::skills::SelectedSkill>,
}

impl From<crate::types::RunResult> for TurnResult {
    fn from(result: crate::types::RunResult) -> Self {
        let outcome = match result.outcome {
            psychevo_ai::Outcome::Normal => TurnOutcome::Completed,
            psychevo_ai::Outcome::Stopped => TurnOutcome::Stopped,
            psychevo_ai::Outcome::Failed => TurnOutcome::Failed,
            psychevo_ai::Outcome::Aborted => TurnOutcome::Interrupted,
        };
        Self {
            thread_id: result.session_id,
            outcome,
            final_answer: result.final_answer,
            provider: result.provider,
            model: result.model,
            reasoning_effort: result.reasoning_effort,
            tool_failures: result.tool_failures,
            context_limit: result.context_limit,
            context_snapshot: result.context_snapshot,
            warnings: result.warnings,
            terminal_reason: result.terminal_reason,
            terminal_error: result.terminal_error,
            selected_agent: result.selected_agent,
            selected_skills: result.selected_skills,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    pub id: String,
    pub source: String,
    pub cwd: String,
    pub title: Option<String>,
    pub started_at_ms: i64,
    pub updated_at_ms: i64,
    pub archived: bool,
    pub message_count: i64,
    pub tool_call_count: i64,
    pub active_turn_id: Option<String>,
}

impl ThreadSummary {
    fn from_summary(summary: SessionSummary, active_turn_id: Option<String>) -> Self {
        Self {
            id: summary.id,
            source: summary.source,
            cwd: summary.cwd,
            title: summary.title,
            started_at_ms: summary.started_at_ms,
            updated_at_ms: summary.updated_at_ms,
            archived: summary.archived_at_ms.is_some(),
            message_count: summary.message_count,
            tool_call_count: summary.tool_call_count,
            active_turn_id,
        }
    }
}

const DEFAULT_HISTORY_PAGE_SIZE: usize = 100;
const MAX_HISTORY_PAGE_SIZE: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadExecutionContext {
    pub id: String,
    pub cwd: String,
    pub source: String,
}

impl ThreadExecutionContext {
    fn from_summary(summary: SessionSummary) -> Self {
        Self {
            id: summary.id,
            cwd: summary.cwd,
            source: summary.source,
        }
    }
}

#[derive(Clone)]
pub struct HistoryReader {
    state: StateRuntime,
    thread_id: String,
}

impl HistoryReader {
    fn new(state: StateRuntime, thread_id: String) -> Self {
        Self { state, thread_id }
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub async fn latest(&self, limit: Option<usize>) -> Result<HistoryPage> {
        self.before(None, limit).await
    }

    pub async fn before(
        &self,
        before_session_seq: Option<i64>,
        limit: Option<usize>,
    ) -> Result<HistoryPage> {
        let limit = limit
            .unwrap_or(DEFAULT_HISTORY_PAGE_SIZE)
            .clamp(1, MAX_HISTORY_PAGE_SIZE);
        let mut items = self
            .state
            .load_tui_message_summaries_before(
                &self.thread_id,
                before_session_seq,
                limit.saturating_add(1),
            )
            .await?
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub thread_id: String,
    pub items: Vec<ThreadItem>,
    pub next_before: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSnapshot {
    #[serde(flatten)]
    pub summary: ThreadSummary,
    pub pending_interactions: Vec<PendingInteraction>,
    pub items: Vec<ThreadItem>,
    pub history_cursor: Option<i64>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadItem {
    pub session_seq: i64,
    pub message: psychevo_agent_core::Message,
    pub usage: Option<Value>,
    pub metadata: Option<Value>,
    pub accounting: Option<Value>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingInteraction {
    pub interaction_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub kind: String,
    pub status: String,
    pub payload: Value,
    pub resolution: Option<Value>,
    pub requested_at_ms: i64,
    pub resolved_at_ms: Option<i64>,
}

impl From<crate::state::FrameworkInteractionRecord> for PendingInteraction {
    fn from(record: crate::state::FrameworkInteractionRecord) -> Self {
        Self {
            interaction_id: record.interaction_id,
            thread_id: record.thread_id,
            turn_id: record.turn_id,
            kind: record.kind,
            status: record.status,
            payload: record.payload,
            resolution: record.resolution,
            requested_at_ms: record.requested_at_ms,
            resolved_at_ms: record.resolved_at_ms,
        }
    }
}

fn gateway_terminal_facts(outcome: TurnOutcome) -> (&'static str, &'static str) {
    match outcome {
        TurnOutcome::Completed => ("completed", "normal"),
        TurnOutcome::Stopped => ("interrupted", "stopped"),
        TurnOutcome::Failed => ("failed", "failed"),
        TurnOutcome::Interrupted => ("interrupted", "aborted"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStage {
    Started,
    Updated,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum TurnEvent {
    Accepted {
        receipt: TurnReceipt,
    },
    Started {
        thread_id: String,
        turn_id: String,
    },
    Message {
        stage: ItemStage,
        message: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        accounting: Option<Value>,
    },
    ReasoningDelta {
        text: String,
    },
    ReasoningCompleted {
        text: Option<String>,
    },
    Tool {
        stage: ItemStage,
        data: Value,
    },
    InteractionRequested {
        interaction_id: String,
        kind: String,
        payload: Value,
    },
    InteractionResolved {
        interaction_id: String,
        kind: String,
        reason: String,
    },
    Warning {
        data: Value,
    },
    Completed {
        thread_id: String,
        turn_id: String,
        outcome: TurnOutcome,
    },
    Failed {
        thread_id: String,
        turn_id: String,
        message: String,
    },
    ResyncRequired {
        missed: u64,
    },
}

impl TurnEvent {
    fn from_run_stream(event: RunStreamEvent) -> Option<Self> {
        match event {
            RunStreamEvent::Event(event) => match event.payload {
                SessionEventPayload::MessageStarted { message } => Some(Self::Message {
                    stage: ItemStage::Started,
                    message,
                    usage: None,
                    metadata: None,
                    accounting: None,
                }),
                SessionEventPayload::MessageUpdated { message } => Some(Self::Message {
                    stage: ItemStage::Updated,
                    message,
                    usage: None,
                    metadata: None,
                    accounting: None,
                }),
                SessionEventPayload::MessageCompleted {
                    message,
                    usage,
                    metadata,
                    accounting,
                } => Some(Self::Message {
                    stage: ItemStage::Completed,
                    message,
                    usage,
                    metadata,
                    accounting,
                }),
                SessionEventPayload::ReasoningDelta { text } => Some(Self::ReasoningDelta { text }),
                SessionEventPayload::ReasoningCompleted { text } => {
                    Some(Self::ReasoningCompleted { text })
                }
                SessionEventPayload::ToolCallPending { data }
                | SessionEventPayload::ToolExecutionStarted { data } => Some(Self::Tool {
                    stage: ItemStage::Started,
                    data,
                }),
                SessionEventPayload::ToolExecutionUpdated { data } => Some(Self::Tool {
                    stage: ItemStage::Updated,
                    data,
                }),
                SessionEventPayload::ToolExecutionCompleted { data } => Some(Self::Tool {
                    stage: ItemStage::Completed,
                    data,
                }),
                SessionEventPayload::BlockingActionRequested {
                    action_id,
                    kind,
                    payload,
                }
                | SessionEventPayload::BlockingActionUpdated {
                    action_id,
                    kind,
                    payload,
                } => Some(Self::InteractionRequested {
                    interaction_id: action_id,
                    kind: format!("{kind:?}").to_lowercase(),
                    payload,
                }),
                SessionEventPayload::BlockingActionResolved {
                    action_id,
                    kind,
                    reason,
                }
                | SessionEventPayload::BlockingActionCancelled {
                    action_id,
                    kind,
                    reason,
                } => Some(Self::InteractionResolved {
                    interaction_id: action_id,
                    kind: format!("{kind:?}").to_lowercase(),
                    reason,
                }),
                SessionEventPayload::Warning { data } => Some(Self::Warning { data }),
                SessionEventPayload::SessionConfigured { .. }
                | SessionEventPayload::TurnStarted { .. }
                | SessionEventPayload::TurnCompleted { .. }
                | SessionEventPayload::AgentSessionStarted { .. }
                | SessionEventPayload::ContextSnapshot { .. }
                | SessionEventPayload::DeliveryDiagnostic { .. }
                | SessionEventPayload::Diagnostic { .. } => None,
            },
            RunStreamEvent::ReasoningDelta { text } => Some(Self::ReasoningDelta { text }),
            RunStreamEvent::ReasoningEnd => Some(Self::ReasoningCompleted { text: None }),
            RunStreamEvent::ClarifyRequest(request) => Some(Self::InteractionRequested {
                interaction_id: request.call_id,
                kind: "clarify".to_string(),
                payload: serde_json::to_value(request.questions).unwrap_or(Value::Null),
            }),
            RunStreamEvent::ClarifyResolved(resolved) => Some(Self::InteractionResolved {
                interaction_id: resolved.call_id,
                kind: "clarify".to_string(),
                reason: format!("{:?}", resolved.reason).to_lowercase(),
            }),
            RunStreamEvent::Scoped { event, .. } => Self::from_run_stream(*event),
        }
    }
}

pub struct TurnEventStream {
    log: Arc<EventLog>,
    cursor: u64,
}

impl fmt::Debug for TurnEventStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TurnEventStream")
            .field("cursor", &self.cursor)
            .finish_non_exhaustive()
    }
}

impl TurnEventStream {
    pub async fn next(&mut self) -> Option<TurnEvent> {
        self.log.next(&mut self.cursor).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct FakeAgentSessionAdapter {
        started: Arc<Notify>,
        release: Arc<Notify>,
        completed: Arc<AtomicUsize>,
    }

    #[derive(Debug)]
    struct InteractionAgentSessionAdapter {
        started: Arc<Notify>,
        release: Arc<Notify>,
    }

    #[derive(Debug)]
    struct PermissionInteractionAgentSessionAdapter {
        started: Arc<Notify>,
        decision: Arc<Mutex<Option<PermissionApprovalDecision>>>,
    }

    #[derive(Debug)]
    struct ClarifyInteractionAgentSessionAdapter {
        started: Arc<Notify>,
        outcome: Arc<Mutex<Option<crate::types::ClarifyInteractionOutcome>>>,
    }

    #[derive(Debug)]
    struct ForceAwareAgentSessionAdapter {
        started: Arc<Notify>,
        shutdown_modes: Arc<Mutex<Vec<bool>>>,
    }

    #[derive(Debug)]
    struct ShutdownReleasesAgentSessionAdapter {
        started: Arc<Notify>,
        release: Arc<Notify>,
    }

    #[derive(Debug)]
    struct PendingAgentSessionAdapter {
        started: Arc<Notify>,
    }

    #[derive(Debug)]
    struct FailingAgentSessionAdapter;

    #[derive(Debug)]
    struct PanickingAgentSessionAdapter;

    #[derive(Debug)]
    struct SnapshotOrderingAgentSessionAdapter {
        calls: Arc<AtomicUsize>,
        first_started: Arc<Notify>,
        release_first: Arc<Notify>,
        second_snapshot_items: Arc<Mutex<Option<usize>>>,
    }

    #[test]
    fn public_turn_outcomes_map_to_distinct_gateway_terminal_facts() {
        assert_eq!(
            gateway_terminal_facts(TurnOutcome::Completed),
            ("completed", "normal")
        );
        assert_eq!(
            gateway_terminal_facts(TurnOutcome::Stopped),
            ("interrupted", "stopped")
        );
        assert_eq!(
            gateway_terminal_facts(TurnOutcome::Failed),
            ("failed", "failed")
        );
        assert_eq!(
            gateway_terminal_facts(TurnOutcome::Interrupted),
            ("interrupted", "aborted")
        );
    }

    #[test]
    fn non_clean_shutdown_report_is_a_teardown_error() {
        let report = ShutdownReport {
            forced: true,
            adapter: ShutdownAdapterStatus::TimedOut,
            task_panics: 1,
            aborted_tasks: 2,
            pending_terminal_failures: vec![PendingTerminalFailure {
                turn_id: "turn-1".to_string(),
                message: "write failed".to_string(),
            }],
        };
        let error = report.require_clean().expect_err("non-clean report");
        let message = error.to_string();
        assert!(message.contains("timed_out"));
        assert!(message.contains("turn-1"));
        assert!(message.contains("write failed"));
    }

    impl AgentSessionAdapter for ForceAwareAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
            let started = self.started.clone();
            Box::pin(async move {
                started.notify_one();
                while !request.control.is_interrupted() {
                    tokio::task::yield_now().await;
                }
                Ok(TurnResult {
                    thread_id: request.receipt.thread_id,
                    outcome: TurnOutcome::Interrupted,
                    final_answer: String::new(),
                    provider: "fake".to_string(),
                    model: "fake-model".to_string(),
                    reasoning_effort: None,
                    tool_failures: 0,
                    context_limit: None,
                    context_snapshot: None,
                    warnings: Vec::new(),
                    terminal_reason: None,
                    terminal_error: None,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                })
            })
        }

        fn shutdown(&self, force: bool) -> BoxFuture<'static, Result<()>> {
            let shutdown_modes = self.shutdown_modes.clone();
            Box::pin(async move {
                shutdown_modes
                    .lock()
                    .expect("shutdown modes poisoned")
                    .push(force);
                Ok(())
            })
        }
    }

    impl AgentSessionAdapter for ShutdownReleasesAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
            let started = self.started.clone();
            let release = self.release.clone();
            Box::pin(async move {
                started.notify_one();
                release.notified().await;
                Ok(TurnResult {
                    thread_id: request.receipt.thread_id,
                    outcome: TurnOutcome::Interrupted,
                    final_answer: String::new(),
                    provider: "fake".to_string(),
                    model: "fake-model".to_string(),
                    reasoning_effort: None,
                    tool_failures: 0,
                    context_limit: None,
                    context_snapshot: None,
                    warnings: Vec::new(),
                    terminal_reason: None,
                    terminal_error: None,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                })
            })
        }

        fn shutdown(&self, force: bool) -> BoxFuture<'static, Result<()>> {
            let release = self.release.clone();
            Box::pin(async move {
                assert!(force);
                release.notify_waiters();
                Ok(())
            })
        }
    }

    impl AgentSessionAdapter for PendingAgentSessionAdapter {
        fn run_turn(&self, _request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
            let started = self.started.clone();
            Box::pin(async move {
                started.notify_one();
                std::future::pending::<Result<TurnResult>>().await
            })
        }

        fn shutdown(&self, force: bool) -> BoxFuture<'static, Result<()>> {
            assert!(force);
            Box::pin(std::future::pending())
        }
    }

    impl AgentSessionAdapter for FailingAgentSessionAdapter {
        fn run_turn(&self, _request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
            Box::pin(async { Err(Error::Message("adapter fixture failed".to_string())) })
        }
    }

    impl AgentSessionAdapter for PanickingAgentSessionAdapter {
        fn run_turn(&self, _request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
            Box::pin(async { panic!("adapter fixture panic") })
        }
    }

    impl AgentSessionAdapter for SnapshotOrderingAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            let first_started = self.first_started.clone();
            let release_first = self.release_first.clone();
            let second_snapshot_items = self.second_snapshot_items.clone();
            Box::pin(async move {
                if call == 0 {
                    first_started.notify_one();
                    release_first.notified().await;
                } else {
                    *second_snapshot_items
                        .lock()
                        .expect("snapshot observation poisoned") = Some(
                        request
                            .history
                            .latest(Some(MAX_HISTORY_PAGE_SIZE))
                            .await?
                            .items
                            .len(),
                    );
                }
                Ok(TurnResult {
                    thread_id: request.receipt.thread_id,
                    outcome: TurnOutcome::Completed,
                    final_answer: format!("turn {call}"),
                    provider: "fake".to_string(),
                    model: "fake-model".to_string(),
                    reasoning_effort: None,
                    tool_failures: 0,
                    context_limit: None,
                    context_snapshot: None,
                    warnings: Vec::new(),
                    terminal_reason: None,
                    terminal_error: None,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                })
            })
        }
    }

    impl AgentSessionAdapter for InteractionAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
            let started = self.started.clone();
            let release = self.release.clone();
            Box::pin(async move {
                request.events.emit(TurnEvent::InteractionRequested {
                    interaction_id: "clarify-1".to_string(),
                    kind: "clarify".to_string(),
                    payload: serde_json::json!([{"question": "Proceed?"}]),
                });
                started.notify_one();
                release.notified().await;
                request.events.emit(TurnEvent::InteractionResolved {
                    interaction_id: "clarify-1".to_string(),
                    kind: "clarify".to_string(),
                    reason: "answered".to_string(),
                });
                Ok(TurnResult {
                    thread_id: request.receipt.thread_id,
                    outcome: TurnOutcome::Completed,
                    final_answer: "fake answer".to_string(),
                    provider: "fake".to_string(),
                    model: "fake-model".to_string(),
                    reasoning_effort: None,
                    tool_failures: 0,
                    context_limit: None,
                    context_snapshot: None,
                    warnings: Vec::new(),
                    terminal_reason: None,
                    terminal_error: None,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                })
            })
        }
    }

    impl AgentSessionAdapter for PermissionInteractionAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
            let started = self.started.clone();
            let observed_decision = self.decision.clone();
            Box::pin(async move {
                let handler = request
                    .input
                    .approval_handler
                    .as_ref()
                    .expect("Application must install the Framework approval handler");
                let approval = handler.request_permission(PermissionApprovalRequest {
                    tool_call_id: "permission-1".to_string(),
                    tool_name: "exec_command".to_string(),
                    summary: "Run the checked command".to_string(),
                    reason: "The fixture requires an explicit decision".to_string(),
                    matched_rule: None,
                    suggested_rule: None,
                    allow_always: false,
                    filesystem: None,
                    timeout_secs: 30,
                });
                started.notify_one();
                *observed_decision
                    .lock()
                    .expect("observed permission decision poisoned") = Some(approval.await);
                Ok(TurnResult {
                    thread_id: request.receipt.thread_id,
                    outcome: TurnOutcome::Completed,
                    final_answer: "permission accepted".to_string(),
                    provider: "fake".to_string(),
                    model: "fake-model".to_string(),
                    reasoning_effort: None,
                    tool_failures: 0,
                    context_limit: None,
                    context_snapshot: None,
                    warnings: Vec::new(),
                    terminal_reason: None,
                    terminal_error: None,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                })
            })
        }
    }

    impl AgentSessionAdapter for ClarifyInteractionAgentSessionAdapter {
        fn run_turn(
            &self,
            mut request: AgentTurnRequest,
        ) -> BoxFuture<'static, Result<TurnResult>> {
            let started = self.started.clone();
            let observed_outcome = self.outcome.clone();
            Box::pin(async move {
                let native_control = request
                    .native_control
                    .take()
                    .expect("Application must install native Turn control");
                let control = native_control.handle();
                let events = request.events.clone();
                let stream: RunStreamSink = Arc::new(move |event| {
                    if let Some(event) = TurnEvent::from_run_stream(event) {
                        events.emit(event);
                    }
                    started.notify_one();
                });
                let outcome = control
                    .request_clarification(
                        crate::types::ClarifyRequestEvent {
                            call_id: "clarify-application-1".to_string(),
                            questions: vec![crate::types::ClarifyQuestion {
                                header: "Target".to_string(),
                                question: "Which directories?".to_string(),
                                options: Vec::new(),
                                multiple: true,
                                custom: true,
                                secret: false,
                            }],
                        },
                        stream,
                        Some(native_control.abort_signal()),
                    )
                    .await;
                *observed_outcome
                    .lock()
                    .expect("observed clarify outcome poisoned") = Some(outcome);
                Ok(TurnResult {
                    thread_id: request.receipt.thread_id,
                    outcome: TurnOutcome::Completed,
                    final_answer: "clarification accepted".to_string(),
                    provider: "fake".to_string(),
                    model: "fake-model".to_string(),
                    reasoning_effort: None,
                    tool_failures: 0,
                    context_limit: None,
                    context_snapshot: None,
                    warnings: Vec::new(),
                    terminal_reason: None,
                    terminal_error: None,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                })
            })
        }
    }

    impl AgentSessionAdapter for FakeAgentSessionAdapter {
        fn run_turn(&self, request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
            let started = Arc::clone(&self.started);
            let release = Arc::clone(&self.release);
            let completed = Arc::clone(&self.completed);
            Box::pin(async move {
                request.events.emit(TurnEvent::Message {
                    stage: ItemStage::Completed,
                    message: serde_json::json!({
                        "role": "assistant",
                        "text": "fake answer",
                    }),
                    usage: None,
                    metadata: None,
                    accounting: None,
                });
                started.notify_one();
                release.notified().await;
                completed.fetch_add(1, Ordering::SeqCst);
                Ok(TurnResult {
                    thread_id: request.receipt.thread_id,
                    outcome: TurnOutcome::Completed,
                    final_answer: "fake answer".to_string(),
                    provider: "fake".to_string(),
                    model: "fake-model".to_string(),
                    reasoning_effort: None,
                    tool_failures: 0,
                    context_limit: None,
                    context_snapshot: None,
                    warnings: Vec::new(),
                    terminal_reason: None,
                    terminal_error: None,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                })
            })
        }
    }

    fn fake_adapter() -> (
        Arc<FakeAgentSessionAdapter>,
        Arc<Notify>,
        Arc<Notify>,
        Arc<AtomicUsize>,
    ) {
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let completed = Arc::new(AtomicUsize::new(0));
        (
            Arc::new(FakeAgentSessionAdapter {
                started: Arc::clone(&started),
                release: Arc::clone(&release),
                completed: Arc::clone(&completed),
            }),
            started,
            release,
            completed,
        )
    }

    #[tokio::test]
    async fn bounded_event_log_reports_resync_and_replays_retained_events() {
        let log = Arc::new(EventLog::new(2));
        for index in 0..3 {
            log.push(TurnEvent::Warning {
                data: serde_json::json!({ "index": index }),
            });
        }
        log.close();
        let mut stream = TurnEventStream { log, cursor: 0 };
        assert_eq!(
            stream.next().await,
            Some(TurnEvent::ResyncRequired { missed: 1 })
        );
        assert!(matches!(
            stream.next().await,
            Some(TurnEvent::Warning { data }) if data["index"] == 1
        ));
        assert!(matches!(
            stream.next().await,
            Some(TurnEvent::Warning { data }) if data["index"] == 2
        ));
        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn application_requires_explicit_home_and_nonzero_event_capacity() {
        let missing = Application::builder().build().await;
        assert!(matches!(missing, Err(Error::Message(message)) if message.contains("home")));

        let temp = tempfile::tempdir().expect("tempdir");
        let invalid = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .event_capacity(0)
            .build()
            .await;
        assert!(
            matches!(invalid, Err(Error::Message(message)) if message.contains("greater than zero"))
        );
    }

    #[tokio::test]
    async fn client_materializes_lists_resumes_and_archives_threads() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .build()
            .await
            .expect("application");
        let client = application.client();
        let thread = client
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        assert_eq!(thread.snapshot().await.expect("snapshot").source, "sdk");
        assert_eq!(
            client
                .resume_thread(thread.id())
                .await
                .expect("resume")
                .id(),
            thread.id()
        );
        assert_eq!(
            client
                .list_threads(ThreadListQuery::default())
                .await
                .expect("list")
                .threads
                .len(),
            1
        );
        thread.archive().await.expect("archive");
        assert!(
            client
                .list_threads(ThreadListQuery::default())
                .await
                .expect("active")
                .threads
                .is_empty()
        );
        assert_eq!(
            client
                .list_threads(ThreadListQuery {
                    archived: true,
                    ..ThreadListQuery::default()
                })
                .await
                .expect("archived")
                .threads
                .len(),
            1
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn client_lists_threads_in_stable_bounded_pages() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .build()
            .await
            .expect("application");
        let client = application.client();
        for _ in 0..3 {
            client
                .start_thread(StartThreadRequest::new(temp.path()))
                .await
                .expect("thread");
        }

        let first = client
            .list_threads(ThreadListQuery {
                cwd: Some(temp.path().to_path_buf()),
                limit: 2,
                ..ThreadListQuery::default()
            })
            .await
            .expect("first page");
        assert_eq!(first.threads.len(), 2);
        let cursor = first.next_cursor.expect("next cursor");
        let second = client
            .list_threads(ThreadListQuery {
                cwd: Some(temp.path().to_path_buf()),
                cursor: Some(cursor.clone()),
                limit: 2,
                ..ThreadListQuery::default()
            })
            .await
            .expect("second page");
        assert_eq!(second.threads.len(), 1);
        assert!(second.next_cursor.is_none());
        assert!(
            first
                .threads
                .iter()
                .all(|thread| second.threads.iter().all(|next| next.id != thread.id))
        );

        let mismatch = client
            .list_threads(ThreadListQuery {
                archived: true,
                cursor: Some(cursor),
                ..ThreadListQuery::default()
            })
            .await
            .expect_err("cursor filter mismatch");
        assert!(
            mismatch
                .to_string()
                .contains("does not match the current filters")
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn history_reader_returns_the_latest_bounded_tail_then_older_pages() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(FailingAgentSessionAdapter))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        for index in 0..205 {
            application
                .inner
                .state
                .append_message(
                    thread.id(),
                    &psychevo_agent_core::user_text_message(format!("message-{index}")),
                )
                .await
                .expect("append history");
        }

        let latest = thread.history().latest(Some(5)).await.expect("latest page");
        assert_eq!(latest.items.len(), 5);
        assert_eq!(
            latest
                .items
                .iter()
                .map(|item| item.session_seq)
                .collect::<Vec<_>>(),
            vec![201, 202, 203, 204, 205]
        );
        assert_eq!(latest.next_before, Some(201));
        let older = thread
            .history()
            .before(latest.next_before, Some(5))
            .await
            .expect("older page");
        assert_eq!(
            older
                .items
                .iter()
                .map(|item| item.session_seq)
                .collect::<Vec<_>>(),
            vec![196, 197, 198, 199, 200]
        );
        assert!(latest.items.iter().all(|item| {
            older
                .items
                .iter()
                .all(|older| older.session_seq != item.session_seq)
        }));

        let snapshot = thread.snapshot().await.expect("bounded snapshot");
        assert_eq!(snapshot.items.len(), DEFAULT_HISTORY_PAGE_SIZE);
        assert_eq!(
            snapshot.items.first().map(|item| item.session_seq),
            Some(106)
        );
        assert_eq!(snapshot.history_cursor, Some(106));
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn shutdown_closes_new_work_admission() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .build()
            .await
            .expect("application");
        let client = application.client();
        application.shutdown().await.expect("shutdown");
        let error = client
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect_err("closed admission");
        assert!(error.to_string().contains("shutting down"));
    }

    #[tokio::test]
    async fn concurrent_forced_shutdown_upgrades_graceful_owner() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let shutdown_modes = Arc::new(Mutex::new(Vec::new()));
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(ForceAwareAgentSessionAdapter {
                started: started.clone(),
                shutdown_modes: shutdown_modes.clone(),
            }))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("wait for force"))
            .await
            .expect("turn");
        started.notified().await;

        let graceful_application = application.clone();
        let graceful =
            tokio::spawn(async move { graceful_application.shutdown().await.expect("graceful") });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        assert!(
            !graceful.is_finished(),
            "graceful shutdown must be draining"
        );
        let forced = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            application.shutdown_force(),
        )
        .await;
        if forced.is_err() {
            panic!(
                "forced shutdown deadline; adapter shutdown modes: {:?}; turn interrupted: {}",
                *shutdown_modes.lock().expect("shutdown modes poisoned"),
                handle.control.inner.is_aborted()
            );
        }
        let forced_report = forced.expect("checked deadline").expect("forced shutdown");
        let graceful_report = tokio::time::timeout(std::time::Duration::from_secs(1), graceful)
            .await
            .expect("graceful owner deadline")
            .expect("graceful task");
        assert_eq!(graceful_report, forced_report);
        assert!(forced_report.forced);
        assert_eq!(
            *shutdown_modes.lock().expect("shutdown modes poisoned"),
            vec![true]
        );
        assert_eq!(
            handle.wait().await.expect("interrupted result").outcome,
            TurnOutcome::Interrupted
        );
    }

    #[tokio::test]
    async fn forced_shutdown_invokes_adapter_shutdown_before_joining_turn_tasks() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(ShutdownReleasesAgentSessionAdapter {
                started: started.clone(),
                release: Arc::new(Notify::new()),
            }))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("adapter-owned release"))
            .await
            .expect("turn");
        started.notified().await;

        let report = application.shutdown_force().await.expect("forced shutdown");
        assert_eq!(report.adapter, ShutdownAdapterStatus::Completed);
        assert_eq!(report.aborted_tasks, 0);
        assert_eq!(
            handle.wait().await.expect("interrupted result").outcome,
            TurnOutcome::Interrupted
        );
    }

    #[tokio::test]
    async fn forced_shutdown_aborts_a_cancellation_safe_pending_adapter() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let database_path = temp.path().join("state.db");
        let application = Application::builder()
            .home(temp.path())
            .database_path(&database_path)
            .agent_session_adapter(Arc::new(PendingAgentSessionAdapter {
                started: started.clone(),
            }))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("pending forever"))
            .await
            .expect("turn");
        let turn_id = handle.receipt().turn_id.clone();
        started.notified().await;

        let report = application.shutdown_force().await.expect("forced shutdown");
        assert_eq!(report.adapter, ShutdownAdapterStatus::TimedOut);
        assert!(report.aborted_tasks > 0);
        assert!(report.pending_terminal_failures.is_empty());
        assert_eq!(
            handle.wait().await.expect("forced settlement").outcome,
            TurnOutcome::Interrupted
        );
        let reopened = StateRuntime::open(&database_path)
            .await
            .expect("reopen settled state");
        let terminal = reopened
            .gateway_turn_terminal(&turn_id)
            .await
            .expect("terminal query")
            .expect("durable forced terminal");
        assert_eq!(terminal.status, "interrupted");
        assert_eq!(terminal.outcome.as_deref(), Some("aborted"));
        reopened.close().await;
    }

    #[tokio::test]
    async fn fake_agent_adapter_uses_the_public_accepted_turn_and_event_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (adapter, started, release, completed) = fake_adapter();
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(adapter)
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("test prompt"))
            .await
            .expect("accepted turn");
        let mut events = handle.events();
        assert!(matches!(
            events.next().await,
            Some(TurnEvent::Accepted { receipt }) if receipt.turn_id == handle.receipt().turn_id
        ));
        started.notified().await;
        assert!(matches!(
            events.next().await,
            Some(TurnEvent::Started { thread_id, .. }) if thread_id == thread.id()
        ));
        assert!(matches!(
            events.next().await,
            Some(TurnEvent::Message {
                stage: ItemStage::Completed,
                message,
                ..
            }) if message["text"] == "fake answer"
        ));
        release.notify_one();
        let result = handle.wait().await.expect("turn result");
        assert_eq!(result.final_answer, "fake answer");
        assert_eq!(completed.load(Ordering::SeqCst), 1);
        assert!(matches!(
            events.next().await,
            Some(TurnEvent::Completed {
                outcome: TurnOutcome::Completed,
                ..
            })
        ));
        assert_eq!(events.next().await, None);
        let terminal = application
            .inner
            .state
            .gateway_turn_terminal(&handle.receipt().turn_id)
            .await
            .expect("terminal read")
            .expect("terminal");
        assert_eq!(terminal.status, "completed");
        assert_eq!(terminal.outcome.as_deref(), Some("normal"));
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn panicking_turn_observer_cannot_orphan_accepted_work() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (adapter, started, release, completed) = fake_adapter();
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(adapter)
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let mut request = TurnRequest::new("observer panic");
        request.adapter_options.turn_event_observer =
            Some(Arc::new(|_| panic!("observer fixture panic")));

        let handle = thread
            .start_turn(request)
            .await
            .expect("observer must not escape admission");
        started.notified().await;
        release.notify_one();
        assert_eq!(
            handle.wait().await.expect("turn result").outcome,
            TurnOutcome::Completed
        );
        assert_eq!(completed.load(Ordering::SeqCst), 1);
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn dropping_turn_handle_does_not_cancel_accepted_work() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (adapter, started, release, completed) = fake_adapter();
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(adapter)
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("keep running"))
            .await
            .expect("accepted turn");
        started.notified().await;
        drop(handle);
        release.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while completed.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("accepted work completed");
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn duplicate_active_turn_id_is_rejected_without_replacing_the_original_handle() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (adapter, started, release, _) = fake_adapter();
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(adapter)
            .build()
            .await
            .expect("application");
        let client = application.client();
        let thread = client
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let turn_id = Uuid::now_v7().to_string();
        let mut first_request = TurnRequest::new("first");
        first_request.requested_turn_id = Some(turn_id.clone());
        let first = thread
            .start_turn(first_request)
            .await
            .expect("first accepted turn");
        started.notified().await;

        let mut duplicate_request = TurnRequest::new("duplicate");
        duplicate_request.requested_turn_id = Some(turn_id.clone());
        let duplicate = thread
            .start_turn(duplicate_request)
            .await
            .expect_err("active Turn id must be reserved atomically");
        assert!(
            duplicate.to_string().contains("already registered"),
            "{duplicate:#}"
        );

        let resumed = client
            .resume_turn(&turn_id)
            .await
            .expect("original active Turn remains registered");
        release.notify_one();
        assert_eq!(
            resumed.wait().await.expect("original turn result").outcome,
            TurnOutcome::Completed
        );
        first.wait().await.expect("first handle result");
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn thread_delete_removes_state_and_materialized_mcp_runtime() {
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
        application
            .inner
            .runtime
            .mcp_runtime(thread.id())
            .snapshot(
                &[McpServerInput::new(
                    "repo",
                    crate::types::McpTransportInput::Unsupported {
                        kind: "test".to_string(),
                    },
                )],
                temp.path(),
                None,
            )
            .await;
        assert_eq!(application.inner.runtime.mcp_runtime_count(), 1);

        thread.delete().await.expect("delete");

        assert_eq!(application.inner.runtime.mcp_runtime_count(), 0);
        assert!(
            application
                .inner
                .state
                .session_summary(thread.id())
                .await
                .expect("deleted lookup")
                .is_none()
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn caller_cancellation_drops_only_the_turn_acceptance_receipt() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (adapter, started, release, _) = fake_adapter();
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(adapter)
            .build()
            .await
            .expect("application");
        let client = application.client();
        let thread = client
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let entered = Arc::new(Notify::new());
        let accept = Arc::new(Notify::new());
        application
            .inner
            .state
            .set_gateway_turn_acceptance_barrier_for_test(entered.clone(), accept.clone());
        let turn_id = Uuid::now_v7().to_string();
        let client_turn_id = "cancelled-caller-1".to_string();
        let mut request = TurnRequest::new("caller goes away");
        request.requested_turn_id = Some(turn_id.clone());
        request.client_turn_id = Some(client_turn_id.clone());
        let caller_thread = thread.clone();
        let caller = tokio::spawn(async move { caller_thread.start_turn(request).await });
        entered.notified().await;
        caller.abort();
        assert!(
            caller
                .await
                .expect_err("caller task cancelled")
                .is_cancelled()
        );

        accept.notify_one();
        started.notified().await;
        let delivery = application
            .inner
            .state
            .gateway_turn_delivery(&turn_id)
            .await
            .expect("delivery query")
            .expect("accepted delivery");
        assert_eq!(delivery.thread_id, thread.id());
        let receipts = application
            .inner
            .state
            .gateway_turn_start_receipts(thread.id())
            .await
            .expect("start receipts");
        assert!(receipts.iter().any(|receipt| {
            receipt.client_turn_id == client_turn_id && receipt.turn_id == turn_id
        }));
        let resumed = client
            .resume_turn(&turn_id)
            .await
            .expect("Application retained active Turn");
        release.notify_one();
        assert_eq!(
            resumed.wait().await.expect("turn result").final_answer,
            "fake answer"
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn adapter_panic_settles_and_releases_thread_activity() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(PanickingAgentSessionAdapter))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("panic"))
            .await
            .expect("accepted turn");

        let error = handle.wait().await.expect_err("panic must fail the turn");
        assert!(error.to_string().contains("panicked"));
        assert!(!thread.has_activity());
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn queued_turn_builds_snapshot_after_acquiring_thread_lane() {
        let temp = tempfile::tempdir().expect("tempdir");
        let calls = Arc::new(AtomicUsize::new(0));
        let first_started = Arc::new(Notify::new());
        let release_first = Arc::new(Notify::new());
        let second_snapshot_items = Arc::new(Mutex::new(None));
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(SnapshotOrderingAgentSessionAdapter {
                calls,
                first_started: first_started.clone(),
                release_first: release_first.clone(),
                second_snapshot_items: second_snapshot_items.clone(),
            }))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let first = thread
            .start_turn(TurnRequest::new("first"))
            .await
            .expect("first turn");
        first_started.notified().await;
        let second = thread
            .start_turn(TurnRequest::new("second"))
            .await
            .expect("second turn");

        application
            .inner
            .state
            .append_message(
                thread.id(),
                &psychevo_agent_core::user_text_message("committed before first releases"),
            )
            .await
            .expect("append message");
        release_first.notify_one();
        first.wait().await.expect("first result");
        second.wait().await.expect("second result");
        assert_eq!(
            *second_snapshot_items
                .lock()
                .expect("snapshot observation poisoned"),
            Some(1)
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn turn_and_thread_mutation_reservations_share_one_fifo_and_evict_when_idle() {
        let runtime = Arc::new(ApplicationRuntime::new());
        let thread_id = "thread-operation-fifo";
        let first = runtime.reserve_turn_for_test(thread_id, "turn-1");
        first.await.expect("first Turn is ready");
        let mut mutation = runtime.reserve_mutation(thread_id);
        let mut second = runtime.reserve_turn_for_test(thread_id, "turn-2");

        assert!(matches!(
            mutation
                .ready
                .as_mut()
                .expect("mutation reservation")
                .try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        assert!(matches!(
            second.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));

        runtime.settle_turn(thread_id, "turn-1", None);
        mutation
            .ready
            .take()
            .expect("mutation reservation")
            .await
            .expect("mutation is next");
        assert!(matches!(
            second.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));

        drop(mutation);
        second.await.expect("second Turn follows mutation");
        runtime.settle_turn(thread_id, "turn-2", None);
        assert!(
            !runtime
                .state
                .lock()
                .expect("runtime state")
                .threads
                .contains_key(thread_id),
            "idle Thread cells must be evicted"
        );
    }

    #[tokio::test]
    async fn active_and_completed_turns_reattach_and_interactions_are_durable() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let adapter = Arc::new(InteractionAgentSessionAdapter {
            started: started.clone(),
            release: release.clone(),
        });
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(adapter)
            .build()
            .await
            .expect("application");
        let client = application.client();
        let thread = client
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("reattach"))
            .await
            .expect("accepted turn");
        let turn_id = handle.receipt().turn_id.clone();
        started.notified().await;

        let resumed = client.resume_turn(&turn_id).await.expect("active turn");
        assert_eq!(resumed.receipt(), handle.receipt());
        assert_eq!(
            thread
                .snapshot()
                .await
                .expect("active snapshot")
                .active_turn_id
                .as_deref(),
            Some(turn_id.as_str())
        );

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if !thread
                    .pending_interactions()
                    .await
                    .expect("pending interactions")
                    .is_empty()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("interaction persisted");

        release.notify_one();
        let result = handle.wait().await.expect("completion");
        assert_eq!(result.final_answer, "fake answer");

        let completed = client.resume_turn(&turn_id).await.expect("durable turn");
        assert_eq!(
            completed.wait().await.expect("durable result").final_answer,
            "fake answer"
        );
        assert!(
            thread
                .pending_interactions()
                .await
                .expect("resolved interactions")
                .is_empty()
        );
        let fork = thread
            .fork(ForkThreadRequest::default())
            .await
            .expect("fork");
        assert_ne!(fork.id(), thread.id());
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn failed_turn_reattaches_from_its_durable_terminal_error() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(FailingAgentSessionAdapter))
            .build()
            .await
            .expect("application");
        let client = application.client();
        let thread = client
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("fail"))
            .await
            .expect("accepted turn");
        let turn_id = handle.receipt().turn_id.clone();
        let error = handle.wait().await.expect_err("adapter failure");
        assert!(error.to_string().contains("adapter fixture failed"));

        let resumed = client.resume_turn(&turn_id).await.expect("failed turn");
        let resumed_error = resumed.wait().await.expect_err("durable adapter failure");
        assert!(resumed_error.to_string().contains("adapter fixture failed"));
        let mut events = resumed.events();
        assert!(matches!(
            events.next().await,
            Some(TurnEvent::Accepted { .. })
        ));
        assert!(matches!(
            events.next().await,
            Some(TurnEvent::Failed { message, .. }) if message.contains("adapter fixture failed")
        ));
        assert_eq!(events.next().await, None);
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn terminal_persistence_failure_is_not_published_as_turn_completion() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (adapter, started, release, _) = fake_adapter();
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(adapter)
            .build()
            .await
            .expect("application");
        let client = application.client();
        let thread = client
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("persist terminal"))
            .await
            .expect("accepted turn");
        let turn_id = handle.receipt().turn_id.clone();
        started.notified().await;
        application.inner.state.close().await;
        release.notify_one();

        let error = handle.wait().await.expect_err("persistence failure");
        assert!(
            error
                .to_string()
                .contains("failed to persist Framework Turn terminal")
        );
        assert!(matches!(
            client
                .resume_turn(&turn_id)
                .await
                .expect_err("pending terminal retry must preserve its typed failure"),
            Error::TerminalPersistence { .. }
        ));
        let mut events = handle.events();
        let mut saw_durability_warning = false;
        while let Some(event) = events.next().await {
            assert!(
                !matches!(
                    event,
                    TurnEvent::Completed { .. } | TurnEvent::Failed { .. }
                ),
                "a non-durable terminal must not be published"
            );
            if matches!(
                event,
                TurnEvent::Warning { ref data }
                    if data["kind"] == "framework_terminal_persistence"
            ) {
                saw_durability_warning = true;
            }
        }
        assert!(saw_durability_warning);
        assert!(
            !thread.has_activity(),
            "terminal persistence failure must release running/control projection"
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn resume_turn_retries_the_same_in_memory_pending_terminal_once() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (adapter, started, release, _) = fake_adapter();
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(adapter)
            .build()
            .await
            .expect("application");
        let client = application.client();
        let thread = client
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("retry terminal"))
            .await
            .expect("accepted turn");
        let turn_id = handle.receipt().turn_id.clone();
        started.notified().await;
        application
            .inner
            .state
            .fail_next_framework_terminal_for_test();
        release.notify_one();
        assert!(matches!(
            handle.wait().await.expect_err("first terminal commit fails"),
            Error::Message(message)
                if message.contains("failed to persist Framework Turn terminal")
        ));

        let resumed = client
            .resume_turn(&turn_id)
            .await
            .expect("same-process retry succeeds");
        assert_eq!(
            resumed
                .wait()
                .await
                .expect("retried durable result")
                .final_answer,
            "fake answer"
        );
        assert!(
            application
                .inner
                .state
                .gateway_turn_terminal(&turn_id)
                .await
                .expect("terminal query")
                .is_some()
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn restart_reports_a_durable_nonterminal_delivery_as_outcome_indeterminate() {
        let temp = tempfile::tempdir().expect("tempdir");
        let database_path = temp.path().join("state.db");
        let first = Application::builder()
            .home(temp.path())
            .database_path(&database_path)
            .agent_session_adapter(Arc::new(FailingAgentSessionAdapter))
            .build()
            .await
            .expect("first application");
        let thread = first
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let turn_id = Uuid::now_v7().to_string();
        first
            .inner
            .state
            .accept_gateway_turn(
                GatewayTurnDeliveryInput {
                    turn_id: &turn_id,
                    thread_id: thread.id(),
                    runtime_ref: "native",
                    input_json: "{}",
                    input_hash: "fixture",
                },
                None,
            )
            .await
            .expect("durable nonterminal delivery");
        first.shutdown().await.expect("first shutdown");

        let restarted = Application::builder()
            .home(temp.path())
            .database_path(&database_path)
            .agent_session_adapter(Arc::new(FailingAgentSessionAdapter))
            .build()
            .await
            .expect("restarted application");
        assert!(matches!(
            restarted
                .client()
                .resume_turn(&turn_id)
                .await
                .expect_err("unknown outcome must never be replayed or fabricated"),
            Error::OutcomeIndeterminate { turn_id: ref actual } if actual == &turn_id
        ));
        let delivery = restarted
            .inner
            .state
            .gateway_turn_delivery(&turn_id)
            .await
            .expect("delivery query")
            .expect("delivery retained");
        assert_ne!(delivery.status, "terminal");
        restarted.shutdown().await.expect("restarted shutdown");
    }

    #[tokio::test]
    async fn shutdown_waits_for_turn_admission_before_closing_it() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (adapter, started, release, _) = fake_adapter();
        let application = Application::builder()
            .home(temp.path())
            .database_path(temp.path().join("state.db"))
            .agent_session_adapter(adapter)
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let blocker = application
            .inner
            .state
            .begin_sqlx_write()
            .await
            .expect("write blocker");
        let start =
            tokio::spawn(
                async move { thread.start_turn(TurnRequest::new("admission race")).await },
            );
        while application.inner.state.diagnostics().in_flight_operations == 0 {
            tokio::task::yield_now().await;
        }
        let shutdown_application = application.clone();
        let shutdown = tokio::spawn(async move { shutdown_application.shutdown().await });
        for _ in 0..32 {
            tokio::task::yield_now().await;
        }
        assert!(
            *application
                .inner
                .admission
                .lock()
                .expect("application admission poisoned"),
            "shutdown must wait for the admitted start section"
        );
        blocker.rollback().await.expect("release write blocker");
        let handle = start
            .await
            .expect("start task")
            .expect("admitted Turn must not become a ghost rejection");
        started.notified().await;
        release.notify_one();
        handle.wait().await.expect("admitted turn result");
        shutdown.await.expect("shutdown task").expect("shutdown");
    }

    #[test]
    fn completed_message_turn_event_preserves_usage_metadata_and_accounting() {
        let event = TurnEvent::from_run_stream(RunStreamEvent::value(serde_json::json!({
            "type": "message_end",
            "message": {"role": "assistant", "content": []},
            "usage": {"input_tokens": 3},
            "metadata": {"provider": "fake"},
            "accounting": {"reported_total_tokens": 5},
        })))
        .expect("message event");
        let TurnEvent::Message {
            stage: ItemStage::Completed,
            usage,
            metadata,
            accounting,
            ..
        } = event
        else {
            panic!("expected completed message");
        };
        assert_eq!(usage, Some(serde_json::json!({"input_tokens": 3})));
        assert_eq!(metadata, Some(serde_json::json!({"provider": "fake"})));
        assert_eq!(
            accounting,
            Some(serde_json::json!({"reported_total_tokens": 5}))
        );
    }

    #[tokio::test]
    async fn application_owns_durable_permission_rendezvous() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let decision = Arc::new(Mutex::new(None));
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(PermissionInteractionAgentSessionAdapter {
                started: started.clone(),
                decision: decision.clone(),
            }))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("request permission"))
            .await
            .expect("turn");
        started.notified().await;

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                let pending = thread
                    .pending_interactions()
                    .await
                    .expect("pending interactions");
                if pending
                    .iter()
                    .any(|interaction| interaction.interaction_id == "permission-1")
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("permission interaction persisted");

        let expected_decision =
            PermissionApprovalDecision::allow_filesystem_turn(temp.path().display().to_string());
        assert!(
            thread
                .respond(
                    "permission-1",
                    InteractionResponse::Permission(expected_decision.clone()),
                )
                .await
                .expect("permission response")
                .accepted
        );
        assert!(
            !thread
                .respond(
                    "permission-1",
                    InteractionResponse::Permission(PermissionApprovalDecision::deny()),
                )
                .await
                .expect("duplicate permission response")
                .accepted,
            "Framework permission response must be accepted exactly once"
        );
        assert_eq!(
            handle.wait().await.expect("turn result").final_answer,
            "permission accepted"
        );
        assert_eq!(
            decision
                .lock()
                .expect("observed permission decision poisoned")
                .as_ref()
                .cloned(),
            Some(expected_decision.clone())
        );
        let durable = application
            .inner
            .state
            .framework_interactions_for_thread(thread.id(), false)
            .await
            .expect("durable interactions");
        assert_eq!(durable.len(), 1);
        assert_eq!(durable[0].status, "resolved");
        assert_eq!(
            durable[0].resolution,
            Some(
                serde_json::to_value(InteractionResponse::Permission(expected_decision))
                    .expect("serialize response")
            )
        );
        assert!(
            thread
                .pending_interactions()
                .await
                .expect("resolved interactions")
                .is_empty()
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn cancelling_a_permission_interaction_wakes_its_waiter() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let decision = Arc::new(Mutex::new(None));
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(PermissionInteractionAgentSessionAdapter {
                started: started.clone(),
                decision: decision.clone(),
            }))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("cancel permission"))
            .await
            .expect("turn");
        started.notified().await;
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if thread
                    .pending_interactions()
                    .await
                    .expect("pending interactions")
                    .iter()
                    .any(|interaction| interaction.interaction_id == "permission-1")
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("permission interaction persisted");

        assert!(
            !thread
                .respond(
                    "permission-1",
                    InteractionResponse::Clarify(vec![vec!["wrong kind".to_string()]]),
                )
                .await
                .expect("mismatched response")
                .accepted,
            "a mismatched typed response must not terminate the durable interaction"
        );
        assert!(
            thread
                .respond("permission-1", InteractionResponse::Cancel)
                .await
                .expect("cancel response")
                .accepted
        );
        tokio::time::timeout(std::time::Duration::from_secs(1), handle.wait())
            .await
            .expect("permission waiter must wake")
            .expect("turn result");
        assert_eq!(
            decision
                .lock()
                .expect("observed permission decision poisoned")
                .as_ref()
                .map(|decision| decision.outcome),
            Some(PermissionApprovalOutcome::Deny)
        );
        let durable = application
            .inner
            .state
            .framework_interactions_for_thread(thread.id(), false)
            .await
            .expect("durable interactions");
        assert_eq!(durable[0].status, "cancelled");
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn application_persists_and_delivers_the_same_clarify_response() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let outcome = Arc::new(Mutex::new(None));
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(ClarifyInteractionAgentSessionAdapter {
                started: started.clone(),
                outcome: outcome.clone(),
            }))
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread");
        let handle = thread
            .start_turn(TurnRequest::new("request clarification"))
            .await
            .expect("turn");
        started.notified().await;
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if thread
                    .pending_interactions()
                    .await
                    .expect("pending interactions")
                    .iter()
                    .any(|interaction| interaction.interaction_id == "clarify-application-1")
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("clarify interaction persisted");

        let answers = vec![vec!["/workspace/a".to_string(), "/workspace/b".to_string()]];
        assert!(
            thread
                .respond(
                    "clarify-application-1",
                    InteractionResponse::Clarify(answers.clone()),
                )
                .await
                .expect("clarify response")
                .accepted
        );
        handle.wait().await.expect("turn result");
        assert_eq!(
            *outcome.lock().expect("observed clarify outcome poisoned"),
            Some(crate::types::ClarifyInteractionOutcome::Answered(
                ClarifyResponse {
                    answers: vec![ClarifyAnswer {
                        answers: answers[0].clone(),
                    }],
                }
            ))
        );
        let durable = application
            .inner
            .state
            .framework_interactions_for_thread(thread.id(), false)
            .await
            .expect("durable interactions");
        assert_eq!(durable.len(), 1);
        assert_eq!(
            durable[0].resolution,
            Some(
                serde_json::to_value(InteractionResponse::Clarify(answers))
                    .expect("serialize response")
            )
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn injected_provider_runs_through_the_native_framework_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let cwd = temp.path().join("workspace");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&cwd).expect("cwd");
        std::fs::write(
            home.join("config.toml"),
            r#"
model = "fake/fake-model"

[provider.fake]
api = "http://127.0.0.1:9/v1"
no_auth = true

[provider.fake.models.fake-model]
"#,
        )
        .expect("config");
        crate::tests::seed_managed_rg(&home);
        let provider = Arc::new(psychevo_ai::FakeProvider::new(vec![vec![
            psychevo_ai::RawStreamEvent::Text("native fake answer".to_string()),
            psychevo_ai::RawStreamEvent::Done(psychevo_ai::Outcome::Normal),
        ]]));
        let application = Application::builder()
            .home(&home)
            .provider(provider)
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(&cwd))
            .await
            .expect("thread");
        let mut request = TurnRequest::new("answer through the injected provider");
        request.no_agents = true;
        request.no_skills = true;
        request.inherited_env = Some(BTreeMap::from([(
            "HOME".to_string(),
            temp.path().to_string_lossy().to_string(),
        )]));
        let handle = thread.start_turn(request).await.expect("accepted turn");
        let result = handle.wait().await.expect("turn result");
        assert_eq!(result.final_answer, "native fake answer");
        assert_eq!(result.provider, "fake");
        assert_eq!(result.model, "fake-model");
        let snapshot = thread.snapshot().await.expect("durable snapshot");
        assert!(
            snapshot.items.iter().any(|item| {
                matches!(
                    &item.message,
                    psychevo_agent_core::Message::Assistant { content, .. }
                        if content.iter().any(|block| {
                            matches!(
                                block,
                                psychevo_agent_core::AssistantBlock::Text { text }
                                    if text == "native fake answer"
                            )
                        })
                )
            }),
            "authoritative Thread snapshot must contain the durable assistant item"
        );
        application.shutdown().await.expect("shutdown");
    }
}
