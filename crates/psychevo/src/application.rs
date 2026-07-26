//! High-level in-process Framework interface.
//!
//! This module owns the public Thread/Turn vocabulary. The lower run assembly
//! and state Modules remain implementation details of an Application.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex as AsyncMutex, Notify, RwLock as AsyncRwLock, oneshot};
use tokio_util::task::TaskTracker;
use uuid::Uuid;

use crate::compaction::{CompactSessionOptions, CompactionReason, CompactionResult};
use crate::paths::canonicalize_cwd;
use crate::run::{run_live_streaming_controlled, run_live_streaming_controlled_with_provider};
use crate::state::{
    GatewayTurnDeliveryInput, GatewayTurnTerminalInput, NativeSessionForkInput, StateRuntime,
};
use crate::types::{
    ApprovalHandler, ApprovalMode, ClarifyAnswer, ClarifyResponse, ClarifyResult, ImageInput,
    McpServerInput, PermissionApprovalDecision, PermissionApprovalOutcome,
    PermissionApprovalRequest, PermissionMode, ProjectContextInstructionMode, RunMode, RunOptions,
    RunSandboxOverride, RunStreamEvent, RunStreamSink, RuntimeTool, SessionEventPayload,
    SessionSummary, run_control,
};
use crate::{Error, Result};

const DEFAULT_EVENT_CAPACITY: usize = 256;

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
    admission_gate: AsyncRwLock<()>,
    force_shutdown_requested: Mutex<bool>,
    shutdown_complete: Mutex<bool>,
    shutdown_finalizer: AsyncMutex<()>,
    tasks: TaskTracker,
    lanes: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
    controls: Mutex<HashMap<String, crate::types::RunControlHandle>>,
    active_turns: Mutex<HashMap<String, TurnHandle>>,
    active_by_thread: Mutex<HashMap<String, VecDeque<String>>>,
    active_operations_by_thread: Mutex<HashMap<String, usize>>,
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

    pub async fn shutdown(&self) -> Result<()> {
        self.shutdown_inner(false).await
    }

    pub async fn shutdown_force(&self) -> Result<()> {
        self.shutdown_inner(true).await
    }

    #[doc(hidden)]
    #[cfg(feature = "internal")]
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
                admission_gate: AsyncRwLock::new(()),
                force_shutdown_requested: Mutex::new(false),
                shutdown_complete: Mutex::new(false),
                shutdown_finalizer: AsyncMutex::new(()),
                tasks: TaskTracker::new(),
                lanes: Mutex::new(HashMap::new()),
                controls: Mutex::new(HashMap::new()),
                active_turns: Mutex::new(HashMap::new()),
                active_by_thread: Mutex::new(HashMap::new()),
                active_operations_by_thread: Mutex::new(HashMap::new()),
            }),
        }
    }

    async fn shutdown_inner(&self, force: bool) -> Result<()> {
        if *self
            .inner
            .shutdown_complete
            .lock()
            .expect("application shutdown state poisoned")
        {
            return Ok(());
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
                self.inner.tasks.close();
            }
        }
        if force {
            *self
                .inner
                .force_shutdown_requested
                .lock()
                .expect("application shutdown mode poisoned") = true;
        }
        let controls = self
            .inner
            .controls
            .lock()
            .expect("application control map poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        if *self
            .inner
            .force_shutdown_requested
            .lock()
            .expect("application shutdown mode poisoned")
        {
            for control in controls {
                control.abort();
            }
        }
        self.inner.tasks.wait().await;

        // A graceful shutdown future may be cancelled by an outer deadline.
        // Serializing only finalization lets a subsequent forced shutdown
        // escalate the already-closed Application instead of returning early.
        let _finalizer = self.inner.shutdown_finalizer.lock().await;
        if *self
            .inner
            .shutdown_complete
            .lock()
            .expect("application shutdown state poisoned")
        {
            return Ok(());
        }
        let force = *self
            .inner
            .force_shutdown_requested
            .lock()
            .expect("application shutdown mode poisoned");
        self.inner.agent_sessions.shutdown(force).await?;
        self.inner.state.close().await;
        *self
            .inner
            .shutdown_complete
            .lock()
            .expect("application shutdown state poisoned") = true;
        Ok(())
    }

    #[doc(hidden)]
    #[cfg(feature = "internal")]
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
    #[cfg(feature = "internal")]
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
                admission_gate: AsyncRwLock::new(()),
                force_shutdown_requested: Mutex::new(false),
                shutdown_complete: Mutex::new(false),
                shutdown_finalizer: AsyncMutex::new(()),
                tasks: TaskTracker::new(),
                lanes: Mutex::new(HashMap::new()),
                controls: Mutex::new(HashMap::new()),
                active_turns: Mutex::new(HashMap::new()),
                active_by_thread: Mutex::new(HashMap::new()),
                active_operations_by_thread: Mutex::new(HashMap::new()),
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

    pub async fn list_threads(&self, query: ThreadListQuery) -> Result<Vec<ThreadSummary>> {
        self.ensure_open()?;
        let source_refs = query.sources.iter().map(String::as_str).collect::<Vec<_>>();
        let summaries = match (query.cwd.as_deref(), query.archived) {
            (Some(cwd), false) => {
                let cwd = canonicalize_cwd(cwd)?;
                self.inner
                    .state
                    .list_sessions_for_cwd_with_sources(&cwd, &source_refs)
                    .await?
            }
            (Some(cwd), true) => {
                let cwd = canonicalize_cwd(cwd)?;
                self.inner
                    .state
                    .list_archived_sessions_for_cwd_with_sources(&cwd, &source_refs)
                    .await?
            }
            (None, false) => {
                self.inner
                    .state
                    .list_sessions_with_sources(&source_refs)
                    .await?
            }
            (None, true) => {
                self.inner
                    .state
                    .list_archived_sessions_with_sources(&source_refs)
                    .await?
            }
        };
        let mut snapshots = Vec::with_capacity(summaries.len());
        for summary in summaries {
            snapshots.push(self.summary_from_summary(summary));
        }
        Ok(snapshots)
    }

    pub async fn resume_turn(&self, id: impl Into<String>) -> Result<TurnHandle> {
        self.ensure_open()?;
        let id = id.into();
        if let Some(handle) = self
            .inner
            .active_turns
            .lock()
            .expect("application active turn map poisoned")
            .get(&id)
            .cloned()
        {
            return Ok(handle);
        }
        let terminal = self
            .inner
            .state
            .gateway_turn_terminal(&id)
            .await?
            .ok_or_else(|| Error::Message(format!("turn not found: {id}")))?;
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
                self.inner.state.clone(),
                self.inner.tasks.clone(),
            )),
            _ if terminal.status == "failed" => Ok(TurnHandle::failed(
                receipt,
                terminal
                    .error_message
                    .unwrap_or_else(|| "Framework Turn failed".to_string()),
                self.inner.state.clone(),
                self.inner.tasks.clone(),
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

    fn lane(&self, thread_id: &str) -> Arc<AsyncMutex<()>> {
        self.inner
            .lanes
            .lock()
            .expect("application lane map poisoned")
            .entry(thread_id.to_string())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
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
        let active_turn_id = self
            .inner
            .active_by_thread
            .lock()
            .expect("application active thread map poisoned")
            .get(&summary.id)
            .and_then(|turns| turns.front().cloned());
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
        let items = self
            .inner
            .state
            .load_tui_message_summaries(&summary.id)
            .await?
            .into_iter()
            .map(ThreadItem::from)
            .collect();
        Ok(ThreadSnapshot::from_summary(
            summary,
            pending_interactions,
            items,
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

#[derive(Debug, Clone, Default)]
pub struct ThreadListQuery {
    pub cwd: Option<PathBuf>,
    pub archived: bool,
    pub sources: Vec<String>,
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
        let _admission_gate = self.client.inner.admission_gate.read().await;
        self.client.ensure_open()?;
        request.inherited_env = Some(
            self.client
                .application_environment(request.inherited_env.take()),
        );
        let snapshot = self.snapshot().await?;
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
        let runtime_ref = request.runtime_ref.as_deref().unwrap_or("native");
        self.client
            .inner
            .state
            .insert_gateway_turn_delivery(GatewayTurnDeliveryInput {
                turn_id: &receipt.turn_id,
                thread_id: &receipt.thread_id,
                runtime_ref,
                input_json: &durable_input,
                input_hash: &durable_input_hash,
            })
            .await?;
        if let Some(client_turn_id) = request
            .client_turn_id
            .as_deref()
            .map(str::trim)
            .filter(|client_turn_id| !client_turn_id.is_empty())
        {
            self.client
                .inner
                .state
                .record_gateway_turn_start_receipt(
                    &receipt.thread_id,
                    client_turn_id,
                    &receipt.turn_id,
                )
                .await?;
        }
        let event_observer = request.adapter_options.turn_event_observer.take();
        let events = Arc::new(EventLog::observed(
            self.client.inner.event_capacity,
            event_observer,
        ));
        events.push(TurnEvent::Accepted {
            receipt: receipt.clone(),
        });
        let (control_handle, control) = request
            .prepared_control
            .take()
            .map(|prepared| (prepared.handle, prepared.control))
            .unwrap_or_else(run_control);
        let interactions = FrameworkInteractionControl::default();
        self.client
            .inner
            .controls
            .lock()
            .expect("application control map poisoned")
            .insert(receipt.turn_id.clone(), control_handle.clone());
        let (completion_tx, completion_rx) = oneshot::channel();
        let completion: Shared<BoxFuture<'static, SharedTurnCompletion>> = async move {
            completion_rx
                .await
                .unwrap_or_else(|_| Err(Arc::from("accepted Turn task ended without completion")))
        }
        .boxed()
        .shared();
        let client = self.client.clone();
        let task_client = client.clone();
        let thread_id = self.id.clone();
        let turn_id = receipt.turn_id.clone();
        let task_receipt = receipt.clone();
        let task_events = Arc::clone(&events);
        let task_control_handle = control_handle.clone();
        let task_interactions = interactions.clone();
        let lane = client.lane(&thread_id);
        let agent_sessions = Arc::clone(&client.inner.agent_sessions);
        let state = client.inner.state.clone();
        let tasks = client.inner.tasks.clone();
        let handle = TurnHandle {
            receipt: receipt.clone(),
            events,
            completion,
            control: control_handle,
            interactions,
            state: state.clone(),
            tasks: tasks.clone(),
        };

        {
            client
                .inner
                .active_turns
                .lock()
                .expect("application active turn map poisoned")
                .insert(turn_id.clone(), handle.clone());
            client
                .inner
                .active_by_thread
                .lock()
                .expect("application active thread map poisoned")
                .entry(thread_id.clone())
                .or_default()
                .push_back(turn_id.clone());
            client.inner.tasks.spawn(async move {
                let _lane = lane.lock().await;
                task_events.push(TurnEvent::Started {
                    thread_id: thread_id.clone(),
                    turn_id: turn_id.clone(),
                });
                let result = async {
                    let event_sender = TurnEventSender {
                        log: Arc::clone(&task_events),
                        state: state.clone(),
                        tasks: tasks.clone(),
                        thread_id: thread_id.clone(),
                        turn_id: turn_id.clone(),
                    };
                    request.approval_handler = Some(Arc::new(FrameworkApprovalHandler {
                        delegate: request.approval_handler.take(),
                        events: event_sender.clone(),
                        interactions: task_interactions.clone(),
                    }));
                    agent_sessions
                        .run_turn(AgentTurnRequest {
                            thread: snapshot,
                            receipt: task_receipt.clone(),
                            input: request,
                            events: event_sender,
                            control: TurnControl {
                                handle: task_control_handle,
                                interactions: task_interactions.clone(),
                            },
                            native_control: Some(control),
                        })
                        .await
                }
                .await;
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
                let finalization = match &shared {
                    Ok(result) => match serde_json::to_value(result.as_ref()) {
                        Ok(framework_result) => {
                            let (status, outcome) = gateway_terminal_facts(result.outcome);
                            state
                                .finalize_framework_turn(
                                    GatewayTurnTerminalInput {
                                        turn_id: &turn_id,
                                        thread_id: &thread_id,
                                        status,
                                        outcome: Some(outcome),
                                        error_message: None,
                                        started_at_ms: None,
                                        completed_at_ms,
                                        metadata: Some(serde_json::json!({
                                            "source": "framework",
                                            "frameworkReceipt": task_receipt,
                                            "frameworkResult": framework_result,
                                        })),
                                    },
                                    "turn_finished",
                                )
                                .await
                        }
                        Err(error) => Err(Error::from(error)),
                    },
                    Err(message) => {
                        state
                            .finalize_framework_turn(
                                GatewayTurnTerminalInput {
                                    turn_id: &turn_id,
                                    thread_id: &thread_id,
                                    status: "failed",
                                    outcome: Some("failed"),
                                    error_message: Some(message.as_ref()),
                                    started_at_ms: None,
                                    completed_at_ms,
                                    metadata: Some(serde_json::json!({
                                        "source": "framework",
                                        "frameworkReceipt": task_receipt,
                                        "frameworkResult": Value::Null,
                                    })),
                                },
                                "turn_finished",
                            )
                            .await
                    }
                };
                task_interactions.cancel_permissions();
                let completion = match finalization {
                    Ok(()) => {
                        task_client
                            .inner
                            .controls
                            .lock()
                            .expect("application control map poisoned")
                            .remove(&turn_id);
                        task_client
                            .inner
                            .active_turns
                            .lock()
                            .expect("application active turn map poisoned")
                            .remove(&turn_id);
                        let mut turns = task_client
                            .inner
                            .active_by_thread
                            .lock()
                            .expect("application active thread map poisoned");
                        if let Some(thread_turns) = turns.get_mut(&thread_id) {
                            thread_turns.retain(|queued_turn_id| queued_turn_id != &turn_id);
                            if thread_turns.is_empty() {
                                turns.remove(&thread_id);
                            }
                        }
                        drop(turns);
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
                task_events.close();
                let _ = completion_tx.send(completion);
            });
        }

        Ok(handle)
    }

    pub async fn archive(&self) -> Result<()> {
        if self.has_activity() {
            return Err(Error::Message(format!(
                "running Thread cannot be archived: {}",
                self.id
            )));
        }
        self.client.inner.state.archive_session(&self.id).await
    }

    pub async fn compact(&self, request: CompactThreadRequest) -> Result<CompactionResult> {
        let _operation = ThreadOperationGuard::new(self.client.inner.clone(), self.id.clone());
        let snapshot = self.snapshot().await?;
        let inherited_env = self.client.application_environment(request.inherited_env);
        let lane = self.client.lane(&self.id);
        let _lane = lane.lock().await;
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

    pub async fn fork(&self, request: ForkThreadRequest) -> Result<Thread> {
        if self.has_activity() {
            return Err(Error::Message(format!(
                "running Thread cannot be forked: {}",
                self.id
            )));
        }
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

    pub fn respond(&self, interaction_id: &str, response: InteractionResponse) -> bool {
        let turn_id = self
            .client
            .inner
            .active_by_thread
            .lock()
            .expect("application active thread map poisoned")
            .get(&self.id)
            .and_then(|turns| turns.front().cloned());
        let Some(turn_id) = turn_id else {
            return false;
        };
        self.client
            .inner
            .active_turns
            .lock()
            .expect("application active turn map poisoned")
            .get(&turn_id)
            .is_some_and(|turn| turn.respond(interaction_id, response))
    }

    #[doc(hidden)]
    #[cfg(feature = "internal")]
    pub fn __activity(&self) -> (bool, Option<String>, usize) {
        let (active_turn_id, turn_count) = self
            .client
            .inner
            .active_by_thread
            .lock()
            .expect("application active thread map poisoned")
            .get(&self.id)
            .map(|turns| (turns.front().cloned(), turns.len()))
            .unwrap_or((None, 0));
        let operation_count = self
            .client
            .inner
            .active_operations_by_thread
            .lock()
            .expect("application active operation map poisoned")
            .get(&self.id)
            .copied()
            .unwrap_or(0);
        let running = active_turn_id.is_some() || operation_count > 0;
        let queued = turn_count
            .saturating_add(operation_count)
            .saturating_sub(usize::from(running));
        (running, active_turn_id, queued)
    }

    #[doc(hidden)]
    #[cfg(feature = "internal")]
    pub fn __interrupt_all(&self) -> (bool, usize) {
        let turn_ids = self
            .client
            .inner
            .active_by_thread
            .lock()
            .expect("application active thread map poisoned")
            .get(&self.id)
            .cloned()
            .unwrap_or_default();
        let handles = self
            .client
            .inner
            .active_turns
            .lock()
            .expect("application active turn map poisoned");
        let mut interrupted = false;
        let mut cleared = 0;
        for (index, turn_id) in turn_ids.iter().enumerate() {
            if let Some(handle) = handles.get(turn_id) {
                handle.interrupt();
                if index == 0 {
                    interrupted = true;
                } else {
                    cleared += 1;
                }
            }
        }
        (interrupted, cleared)
    }

    #[doc(hidden)]
    #[cfg(feature = "internal")]
    pub fn __steer(&self, expected_turn_id: &str, input: impl Into<String>) -> bool {
        let active_turn_id = self
            .client
            .inner
            .active_by_thread
            .lock()
            .expect("application active thread map poisoned")
            .get(&self.id)
            .and_then(|turns| turns.front().cloned());
        if active_turn_id.as_deref() != Some(expected_turn_id) {
            return false;
        }
        self.client
            .inner
            .active_turns
            .lock()
            .expect("application active turn map poisoned")
            .get(expected_turn_id)
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

    fn has_activity(&self) -> bool {
        let has_turns = self
            .client
            .inner
            .active_by_thread
            .lock()
            .expect("application active thread map poisoned")
            .get(&self.id)
            .is_some_and(|turns| !turns.is_empty());
        has_turns
            || self
                .client
                .inner
                .active_operations_by_thread
                .lock()
                .expect("application active operation map poisoned")
                .get(&self.id)
                .is_some_and(|count| *count > 0)
    }
}

struct ThreadOperationGuard {
    inner: Arc<ApplicationInner>,
    thread_id: String,
}

impl ThreadOperationGuard {
    fn new(inner: Arc<ApplicationInner>, thread_id: String) -> Self {
        *inner
            .active_operations_by_thread
            .lock()
            .expect("application active operation map poisoned")
            .entry(thread_id.clone())
            .or_default() += 1;
        Self { inner, thread_id }
    }
}

impl Drop for ThreadOperationGuard {
    fn drop(&mut self) {
        let mut operations = self
            .inner
            .active_operations_by_thread
            .lock()
            .expect("application active operation map poisoned");
        if let Some(count) = operations.get_mut(&self.thread_id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                operations.remove(&self.thread_id);
            }
        }
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
    pub thread: ThreadSnapshot,
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
    #[cfg(feature = "internal")]
    pub fn __take_native_control(&mut self) -> Result<crate::types::RunControl> {
        self.native_control.take().ok_or_else(|| {
            Error::Message("Agent Session Adapter is missing its Turn control".to_string())
        })
    }
}

#[derive(Clone)]
pub struct TurnEventSender {
    log: Arc<EventLog>,
    state: StateRuntime,
    tasks: TaskTracker,
    thread_id: String,
    turn_id: String,
}

impl TurnEventSender {
    pub fn emit(&self, event: TurnEvent) {
        match &event {
            TurnEvent::InteractionRequested {
                interaction_id,
                kind,
                payload,
            } => {
                let state = self.state.clone();
                let interaction_id = interaction_id.clone();
                let thread_id = self.thread_id.clone();
                let turn_id = self.turn_id.clone();
                let kind = kind.clone();
                let payload = payload.clone();
                self.tasks.spawn(async move {
                    let _ = state
                        .request_framework_interaction(
                            &interaction_id,
                            &thread_id,
                            &turn_id,
                            &kind,
                            payload,
                        )
                        .await;
                });
            }
            TurnEvent::InteractionResolved {
                interaction_id,
                kind: _,
                reason,
            } => {
                let state = self.state.clone();
                let interaction_id = interaction_id.clone();
                let thread_id = self.thread_id.clone();
                let turn_id = self.turn_id.clone();
                let reason = reason.clone();
                self.tasks.spawn(async move {
                    let _ = state
                        .resolve_framework_interaction(
                            &interaction_id,
                            &thread_id,
                            &turn_id,
                            &reason,
                        )
                        .await;
                });
            }
            _ => {}
        }
        self.log.push(event);
    }

    #[doc(hidden)]
    #[cfg(feature = "internal")]
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

#[derive(Clone, Default)]
struct FrameworkInteractionControl {
    permissions: Arc<Mutex<HashMap<String, oneshot::Sender<PermissionApprovalDecision>>>>,
}

impl FrameworkInteractionControl {
    fn register_permission(
        &self,
        interaction_id: String,
    ) -> oneshot::Receiver<PermissionApprovalDecision> {
        let (sender, receiver) = oneshot::channel();
        self.permissions
            .lock()
            .expect("Framework permission interaction map poisoned")
            .insert(interaction_id, sender);
        receiver
    }

    fn submit_permission(
        &self,
        interaction_id: &str,
        decision: PermissionApprovalDecision,
    ) -> bool {
        self.permissions
            .lock()
            .expect("Framework permission interaction map poisoned")
            .remove(interaction_id)
            .and_then(|sender| sender.send(decision).ok())
            .is_some()
    }

    fn remove_permission(&self, interaction_id: &str) {
        self.permissions
            .lock()
            .expect("Framework permission interaction map poisoned")
            .remove(interaction_id);
    }

    fn cancel_permissions(&self) {
        self.permissions
            .lock()
            .expect("Framework permission interaction map poisoned")
            .clear();
    }
}

#[derive(Clone)]
struct FrameworkApprovalHandler {
    delegate: Option<Arc<dyn ApprovalHandler>>,
    events: TurnEventSender,
    interactions: FrameworkInteractionControl,
}

impl fmt::Debug for FrameworkApprovalHandler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FrameworkApprovalHandler")
            .field("has_delegate", &self.delegate.is_some())
            .finish_non_exhaustive()
    }
}

impl ApprovalHandler for FrameworkApprovalHandler {
    fn timeout_secs(&self) -> u64 {
        self.delegate
            .as_ref()
            .map_or(300, |delegate| delegate.timeout_secs())
    }

    fn request_permission(
        &self,
        request: PermissionApprovalRequest,
    ) -> BoxFuture<'static, PermissionApprovalDecision> {
        let interaction_id = if request.tool_call_id.trim().is_empty() {
            Uuid::now_v7().to_string()
        } else {
            request.tool_call_id.clone()
        };
        let receiver = self
            .interactions
            .register_permission(interaction_id.clone());
        self.events.emit(TurnEvent::InteractionRequested {
            interaction_id: interaction_id.clone(),
            kind: "permission".to_string(),
            payload: serde_json::json!({
                "toolName": request.tool_name,
                "summary": request.summary,
                "reason": request.reason,
                "matchedRule": request.matched_rule,
                "suggestedRule": request.suggested_rule,
                "allowSession": true,
                "allowAlways": request.allow_always,
                "filesystem": request.filesystem,
                "timeoutSecs": request.timeout_secs,
            }),
        });
        let delegate = self.delegate.clone();
        let events = self.events.clone();
        let interactions = self.interactions.clone();
        Box::pin(async move {
            let timeout_secs = request.timeout_secs.max(1);
            let wait_for_response = async move {
                match delegate {
                    Some(delegate) => tokio::select! {
                        decision = delegate.request_permission(request) => decision,
                        decision = receiver => decision.unwrap_or_else(|_| PermissionApprovalDecision::deny()),
                    },
                    None => receiver
                        .await
                        .unwrap_or_else(|_| PermissionApprovalDecision::deny()),
                }
            };
            let (decision, reason) = match tokio::time::timeout(
                std::time::Duration::from_secs(timeout_secs),
                wait_for_response,
            )
            .await
            {
                Ok(decision) => {
                    let reason = permission_approval_reason(decision.outcome);
                    (decision, reason)
                }
                Err(_) => (PermissionApprovalDecision::deny(), "timed_out"),
            };
            interactions.remove_permission(&interaction_id);
            events.emit(TurnEvent::InteractionResolved {
                interaction_id,
                kind: "permission".to_string(),
                reason: reason.to_string(),
            });
            decision
        })
    }
}

fn permission_approval_reason(outcome: PermissionApprovalOutcome) -> &'static str {
    match outcome {
        PermissionApprovalOutcome::AllowOnce => "allow_once",
        PermissionApprovalOutcome::AllowTurn => "allow_turn",
        PermissionApprovalOutcome::AllowSession => "allow_session",
        PermissionApprovalOutcome::AllowAlways => "allow_always",
        PermissionApprovalOutcome::Deny => "deny",
    }
}

#[derive(Clone)]
pub struct TurnControl {
    handle: crate::types::RunControlHandle,
    interactions: FrameworkInteractionControl,
}

impl TurnControl {
    pub fn is_interrupted(&self) -> bool {
        self.handle.inner.is_aborted()
    }

    pub fn respond(&self, interaction_id: &str, response: InteractionResponse) -> bool {
        submit_interaction_response(&self.handle, &self.interactions, interaction_id, response)
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
    pub prompt: String,
    pub image_inputs: Vec<ImageInput>,
    pub extract_prompt_image_sources: bool,
    pub prompt_display: Option<crate::types::PromptDisplayMetadata>,
    pub client_turn_id: Option<String>,
    pub source: String,
    pub config_path: Option<PathBuf>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub runtime_ref: Option<String>,
    pub runtime_options: BTreeMap<String, String>,
    pub include_reasoning: bool,
    pub mode: RunMode,
    pub permission_mode: Option<PermissionMode>,
    pub approval_mode: Option<ApprovalMode>,
    pub approval_handler: Option<Arc<dyn ApprovalHandler>>,
    pub clarify_enabled: bool,
    pub inherited_env: Option<BTreeMap<String, String>>,
    pub project_context: Option<ProjectContextInstructionMode>,
    pub sandbox: Option<RunSandboxOverride>,
    pub agent: Option<String>,
    pub no_agents: bool,
    pub no_skills: bool,
    pub skill_inputs: Vec<String>,
    pub mcp_servers: Vec<McpServerInput>,
    pub tools: Vec<RuntimeTool>,
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

    #[doc(hidden)]
    #[cfg(feature = "internal")]
    pub fn __set_control(
        &mut self,
        handle: crate::types::RunControlHandle,
        control: crate::types::RunControl,
    ) {
        self.prepared_control = Some(PreparedTurnControl { handle, control });
    }

    #[doc(hidden)]
    #[cfg(feature = "internal")]
    pub fn __set_adapter_options(&mut self, options: AdapterTurnOptions) {
        self.adapter_options = options;
    }

    #[doc(hidden)]
    #[cfg(feature = "internal")]
    pub fn __take_adapter_input_parts(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.adapter_options.input_parts)
    }

    #[doc(hidden)]
    #[cfg(feature = "internal")]
    pub fn __take_run_stream_observer(&mut self) -> Option<RunStreamSink> {
        self.adapter_options.run_stream_observer.take()
    }

    #[doc(hidden)]
    #[cfg(feature = "internal")]
    pub fn __take_initial_thread_preferences(&mut self) -> BTreeMap<String, String> {
        std::mem::take(&mut self.adapter_options.initial_thread_preferences)
    }

    #[doc(hidden)]
    #[cfg(feature = "internal")]
    pub fn __take_prepared_source_key(&mut self) -> Option<String> {
        self.adapter_options.prepared_source_key.take()
    }

    #[doc(hidden)]
    #[cfg(feature = "internal")]
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
            workspace_mutations: self.adapter_options.workspace_mutations,
            runtime_tools: self.tools,
        }
    }

    #[doc(hidden)]
    #[cfg(feature = "internal")]
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
pub struct TurnHandle {
    receipt: TurnReceipt,
    events: Arc<EventLog>,
    completion: Shared<BoxFuture<'static, SharedTurnCompletion>>,
    control: crate::types::RunControlHandle,
    interactions: FrameworkInteractionControl,
    state: StateRuntime,
    tasks: TaskTracker,
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
    fn completed(
        receipt: TurnReceipt,
        result: TurnResult,
        state: StateRuntime,
        tasks: TaskTracker,
    ) -> Self {
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
        let completion = async move { Ok(result) }.boxed().shared();
        let (control, _) = run_control();
        Self {
            receipt,
            events,
            completion,
            control,
            interactions: FrameworkInteractionControl::default(),
            state,
            tasks,
        }
    }

    fn failed(
        receipt: TurnReceipt,
        message: String,
        state: StateRuntime,
        tasks: TaskTracker,
    ) -> Self {
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
        let completion = async move { Err(Arc::from(message)) }.boxed().shared();
        let (control, _) = run_control();
        Self {
            receipt,
            events,
            completion,
            control,
            interactions: FrameworkInteractionControl::default(),
            state,
            tasks,
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
        match self.completion.clone().await {
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
    #[cfg(feature = "internal")]
    pub fn __control_handle(&self) -> crate::types::RunControlHandle {
        self.control.clone()
    }

    pub fn respond(&self, interaction_id: &str, response: InteractionResponse) -> bool {
        let accepted = submit_interaction_response(
            &self.control,
            &self.interactions,
            interaction_id,
            response,
        );
        if accepted {
            let state = self.state.clone();
            let interaction_id = interaction_id.to_string();
            let thread_id = self.receipt.thread_id.clone();
            let turn_id = self.receipt.turn_id.clone();
            self.tasks.spawn(async move {
                let _ = state
                    .resolve_framework_interaction(
                        &interaction_id,
                        &thread_id,
                        &turn_id,
                        "answered",
                    )
                    .await;
            });
        }
        accepted
    }
}

fn submit_interaction_response(
    control: &crate::types::RunControlHandle,
    interactions: &FrameworkInteractionControl,
    interaction_id: &str,
    response: InteractionResponse,
) -> bool {
    match response {
        InteractionResponse::Permission(decision) => {
            interactions.submit_permission(interaction_id, decision)
        }
        InteractionResponse::Clarify(answers) => control.submit_clarify_result(
            interaction_id,
            ClarifyResult::Answered(ClarifyResponse {
                answers: answers
                    .into_iter()
                    .map(|answers| ClarifyAnswer { answers })
                    .collect(),
            }),
        ),
        InteractionResponse::Cancel => {
            control.submit_clarify_result(interaction_id, ClarifyResult::Cancelled)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractionResponse {
    Permission(PermissionApprovalDecision),
    Clarify(Vec<Vec<String>>),
    Cancel,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSnapshot {
    #[serde(flatten)]
    pub summary: ThreadSummary,
    pub pending_interactions: Vec<PendingInteraction>,
    pub items: Vec<ThreadItem>,
}

impl ThreadSnapshot {
    fn from_summary(
        summary: ThreadSummary,
        pending_interactions: Vec<PendingInteraction>,
        items: Vec<ThreadItem>,
    ) -> Self {
        Self {
            summary,
            pending_interactions,
            items,
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

struct EventLog {
    inner: Mutex<EventLogState>,
    notify: Notify,
    capacity: usize,
    observer: Option<Arc<dyn Fn(TurnEvent) + Send + Sync>>,
}

struct EventLogState {
    first_sequence: u64,
    next_sequence: u64,
    events: VecDeque<TurnEvent>,
    closed: bool,
}

impl EventLog {
    fn new(capacity: usize) -> Self {
        Self::observed(capacity, None)
    }

    fn observed(capacity: usize, observer: Option<Arc<dyn Fn(TurnEvent) + Send + Sync>>) -> Self {
        Self {
            inner: Mutex::new(EventLogState {
                first_sequence: 0,
                next_sequence: 0,
                events: VecDeque::with_capacity(capacity),
                closed: false,
            }),
            notify: Notify::new(),
            capacity,
            observer,
        }
    }

    fn push(&self, event: TurnEvent) {
        let observed = event.clone();
        let mut state = self.inner.lock().expect("turn event log poisoned");
        if state.events.len() == self.capacity {
            state.events.pop_front();
            state.first_sequence += 1;
        }
        state.events.push_back(event);
        state.next_sequence += 1;
        drop(state);
        if let Some(observer) = self.observer.as_ref() {
            observer(observed);
        }
        self.notify.notify_waiters();
    }

    fn close(&self) {
        self.inner.lock().expect("turn event log poisoned").closed = true;
        self.notify.notify_waiters();
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
        loop {
            let notified = self.log.notify.notified();
            {
                let state = self.log.inner.lock().expect("turn event log poisoned");
                if self.cursor < state.first_sequence {
                    let missed = state.first_sequence - self.cursor;
                    self.cursor = state.first_sequence;
                    return Some(TurnEvent::ResyncRequired { missed });
                }
                if self.cursor < state.next_sequence {
                    let offset = (self.cursor - state.first_sequence) as usize;
                    let event = state.events.get(offset).cloned();
                    self.cursor += 1;
                    if event.is_some() {
                        return event;
                    }
                }
                if state.closed {
                    return None;
                }
            }
            notified.await;
        }
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
    struct ForceAwareAgentSessionAdapter {
        started: Arc<Notify>,
        shutdown_modes: Arc<Mutex<Vec<bool>>>,
    }

    #[derive(Debug)]
    struct FailingAgentSessionAdapter;

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

    impl AgentSessionAdapter for FailingAgentSessionAdapter {
        fn run_turn(&self, _request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
            Box::pin(async { Err(Error::Message("adapter fixture failed".to_string())) })
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
                .len(),
            1
        );
        thread.archive().await.expect("archive");
        assert!(
            client
                .list_threads(ThreadListQuery::default())
                .await
                .expect("active")
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
                .len(),
            1
        );
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
    async fn forced_shutdown_escalates_after_graceful_deadline_cancels() {
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

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), application.shutdown())
                .await
                .is_err(),
            "graceful shutdown must still be draining the accepted turn"
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
        forced.expect("checked deadline").expect("forced shutdown");
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
        let resumed = client
            .resume_turn(&turn_id)
            .await
            .expect("active error handle");
        assert!(
            resumed
                .wait()
                .await
                .expect_err("same persistence failure")
                .to_string()
                .contains("failed to persist Framework Turn terminal")
        );
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
        application.shutdown().await.expect("shutdown");
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

        assert!(thread.respond(
            "permission-1",
            InteractionResponse::Permission(PermissionApprovalDecision::allow_once()),
        ));
        assert!(
            !thread.respond(
                "permission-1",
                InteractionResponse::Permission(PermissionApprovalDecision::deny()),
            ),
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
                .map(|decision| decision.outcome),
            Some(PermissionApprovalOutcome::AllowOnce)
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
