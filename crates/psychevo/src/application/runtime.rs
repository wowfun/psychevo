#[cfg(test)]
use std::collections::BTreeMap;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Instant;

use futures::FutureExt;
use tokio::sync::{OwnedRwLockReadGuard, RwLock, oneshot};
use tokio_util::task::TaskTracker;

use super::{
    ApplicationActivitySnapshot, ApplicationLimits, ApplicationOperationalSnapshot,
    ApplicationPanicDiagnostic, ApplicationQueuedOperationSnapshot, ApplicationStorageSnapshot,
    Error, PendingTerminal, Result, ThreadActivitySnapshot, TurnHandle,
};
use crate::panic_evidence::{MAX_PANIC_PAYLOAD_BYTES, PanicEvidence, bounded_text};

const MAX_PANIC_DIAGNOSTICS: usize = 32;

pub(super) struct ApplicationRuntime {
    pub(super) tasks: TaskTracker,
    pub(super) state: Mutex<ApplicationRuntimeState>,
    admission_gate: Arc<RwLock<()>>,
    task_aborts: Mutex<HashMap<u64, tokio::task::AbortHandle>>,
    next_task_id: AtomicU64,
    next_operation_id: AtomicU64,
    pub(super) task_panics: AtomicU64,
    task_panic_diagnostics: Mutex<VecDeque<ApplicationPanicDiagnostic>>,
    limits: ApplicationLimits,
    mcp_runtimes: crate::mcp::McpRuntimeRegistry,
    pub(super) agent_supervisor: crate::agents::AgentSupervisor,
}

pub(super) struct ApplicationRuntimeState {
    open: bool,
    accepted_operations: usize,
    activity_revision: u64,
    application_operations: ThreadCell,
    pub(super) threads: HashMap<String, ThreadCell>,
    pub(super) turns: HashMap<String, TurnSlot>,
}

#[derive(Default)]
pub(super) struct ThreadCell {
    pub(super) operations: VecDeque<ThreadOperation>,
}

pub(super) struct ThreadOperation {
    pub(super) kind: ThreadOperationKind,
    pub(super) ready: Option<oneshot::Sender<()>>,
    pub(super) durable_accepted: bool,
    enqueued_at: Instant,
}

pub(super) enum ThreadOperationKind {
    Turn(String),
    Mutation(u64),
    IdleMutation(u64),
}

pub(super) struct TurnSlot {
    pub(super) handle: TurnHandle,
    pub(super) abort: Option<tokio::task::AbortHandle>,
    pub(super) phase: TurnPhase,
    pub(super) pending_terminal: Option<PendingTerminal>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TurnPhase {
    PendingAcceptance,
    Active,
    PendingTerminal,
}

impl ApplicationRuntime {
    #[cfg(test)]
    pub(super) fn new(limits: ApplicationLimits) -> Self {
        Self::new_with_mcp_runtime_registry(limits, crate::mcp::McpRuntimeRegistry::default())
    }

    pub(super) fn new_with_mcp_oauth_credentials(
        limits: ApplicationLimits,
        profile_home: PathBuf,
        mcp_oauth_credentials: Arc<dyn crate::config::McpOAuthCredentialStore>,
    ) -> Self {
        Self::new_with_mcp_runtime_registry(
            limits,
            crate::mcp::McpRuntimeRegistry::new(profile_home, mcp_oauth_credentials),
        )
    }

    fn new_with_mcp_runtime_registry(
        limits: ApplicationLimits,
        mcp_runtimes: crate::mcp::McpRuntimeRegistry,
    ) -> Self {
        Self {
            tasks: TaskTracker::new(),
            state: Mutex::new(ApplicationRuntimeState {
                open: true,
                accepted_operations: 0,
                activity_revision: 0,
                application_operations: ThreadCell::default(),
                threads: HashMap::new(),
                turns: HashMap::new(),
            }),
            admission_gate: Arc::new(RwLock::new(())),
            task_aborts: Mutex::new(HashMap::new()),
            next_task_id: AtomicU64::new(1),
            next_operation_id: AtomicU64::new(1),
            task_panics: AtomicU64::new(0),
            task_panic_diagnostics: Mutex::new(VecDeque::new()),
            limits,
            mcp_runtimes,
            agent_supervisor: crate::agents::AgentSupervisor::default(),
        }
    }

    pub(super) async fn begin_admission(&self) -> Result<OwnedRwLockReadGuard<()>> {
        let guard = self.admission_gate.clone().read_owned().await;
        self.ensure_open()?;
        Ok(guard)
    }

    pub(super) fn ensure_open(&self) -> Result<()> {
        if self.lock_state().open {
            Ok(())
        } else {
            Err(Error::Message(
                "Psychevo Application is shutting down".to_string(),
            ))
        }
    }

    pub(super) async fn close_admission(&self) {
        let _gate = self.admission_gate.write().await;
        let mut state = self.lock_state();
        if state.open {
            state.open = false;
            self.tasks.close();
        }
    }

    pub(super) fn spawn<F>(self: &Arc<Self>, future: F) -> tokio::task::AbortHandle
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.spawn_named("application_task", future)
    }

    pub(super) fn spawn_named<F>(
        self: &Arc<Self>,
        actor: impl Into<String>,
        future: F,
    ) -> tokio::task::AbortHandle
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let task_id = self.next_task_id.fetch_add(1, Ordering::Relaxed);
        let actor = actor.into();
        let (start_tx, start_rx) = oneshot::channel();
        let runtime = Arc::downgrade(self);
        let panic_runtime = Arc::downgrade(self);
        let task = self.tasks.spawn(async move {
            let _guard = TrackedTaskGuard { runtime, task_id };
            if start_rx.await.is_err() {
                return;
            }
            if let Err(payload) = std::panic::AssertUnwindSafe(future).catch_unwind().await {
                let Some(runtime) = panic_runtime.upgrade() else {
                    return;
                };
                runtime.record_task_panic(task_id, &actor, payload);
            }
        });
        let abort = task.abort_handle();
        self.lock_task_aborts().insert(task_id, abort.clone());
        let _ = start_tx.send(());
        abort
    }

    fn record_task_panic(&self, task_id: u64, actor: &str, payload: Box<dyn std::any::Any + Send>) {
        self.task_panics.fetch_add(1, Ordering::Relaxed);
        let evidence = PanicEvidence::capture(payload.as_ref());
        let diagnostic = ApplicationPanicDiagnostic {
            diagnostic_id: format!("application-actor-panic-{task_id}"),
            actor: bounded_text(actor.to_string(), MAX_PANIC_PAYLOAD_BYTES),
            task_id,
            payload: evidence.payload,
            backtrace: evidence.backtrace,
        };
        let mut diagnostics = self
            .task_panic_diagnostics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if diagnostics.len() == MAX_PANIC_DIAGNOSTICS {
            diagnostics.pop_front();
        }
        diagnostics.push_back(diagnostic.clone());
        drop(diagnostics);
        let emitted = serde_json::json!({
            "target": "psychevo.application",
            "event": "actor_panicked",
            "diagnosticId": diagnostic.diagnostic_id,
            "actor": diagnostic.actor,
            "taskId": diagnostic.task_id,
            "payload": diagnostic.payload,
            "backtrace": diagnostic.backtrace,
        });
        eprintln!("{emitted}");
    }

    pub(super) fn abort_all_tasks(&self) -> usize {
        let aborts = self
            .lock_task_aborts()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let count = aborts.len();
        for abort in aborts {
            abort.abort();
        }
        count
    }

    pub(super) fn mcp_runtime(&self, thread_id: &str) -> crate::mcp::McpRuntime {
        self.mcp_runtimes.runtime(thread_id)
    }

    pub(super) fn remove_mcp_runtime(&self, thread_id: &str) {
        self.mcp_runtimes.remove(thread_id);
    }

    pub(super) fn clear_mcp_runtimes(&self) {
        self.mcp_runtimes.clear();
    }

    #[cfg(test)]
    pub(super) fn mcp_runtime_count(&self) -> usize {
        self.mcp_runtimes.len()
    }

    #[cfg(test)]
    pub(super) fn reserve_turn_for_test(
        &self,
        thread_id: &str,
        turn_id: &str,
    ) -> Result<oneshot::Receiver<()>> {
        let mut state = self.lock_state();
        self.admit_turn_operation(&state, thread_id)?;
        let before = state
            .threads
            .get(thread_id)
            .map(thread_cell_activity)
            .unwrap_or((false, None, 0));
        let (ready, after) = {
            let cell = state.threads.entry(thread_id.to_string()).or_default();
            let ready = cell.reserve(ThreadOperationKind::Turn(turn_id.to_string()), true);
            (ready, thread_cell_activity(cell))
        };
        Self::record_activity_transition(&mut state, before, after);
        state.accepted_operations += 1;
        Ok(ready)
    }

    pub(super) fn register_turn(
        &self,
        thread_id: &str,
        turn_id: &str,
        handle: TurnHandle,
    ) -> Result<(oneshot::Receiver<()>, usize)> {
        let mut state = self.lock_state();
        self.admit_turn_operation(&state, thread_id)?;
        if state.turns.contains_key(turn_id) {
            return Err(Error::Message(format!(
                "Turn id is already registered: {turn_id}"
            )));
        }
        let lane = state
            .threads
            .entry(thread_id.to_string())
            .or_default()
            .reserve(ThreadOperationKind::Turn(turn_id.to_string()), false);
        let queue_position = state
            .threads
            .get(thread_id)
            .map(thread_turn_queue_position)
            .unwrap_or_default();
        state.accepted_operations += 1;
        state.turns.insert(
            turn_id.to_string(),
            TurnSlot {
                handle,
                abort: None,
                phase: TurnPhase::PendingAcceptance,
                pending_terminal: None,
            },
        );
        Ok((lane, queue_position))
    }

    pub(super) fn mark_turn_accepted(
        &self,
        thread_id: &str,
        turn_id: &str,
    ) -> Result<ThreadActivitySnapshot> {
        let mut state = self.lock_state();
        let phase = state
            .turns
            .get(turn_id)
            .map(|slot| slot.phase)
            .ok_or_else(|| {
                Error::Message(format!(
                    "Turn slot disappeared before durable acceptance: {turn_id}"
                ))
            })?;
        if phase != TurnPhase::PendingAcceptance {
            return Err(Error::Message(format!(
                "Turn is not pending durable acceptance: {turn_id}"
            )));
        }
        let before = state
            .threads
            .get(thread_id)
            .map(thread_cell_activity)
            .unwrap_or((false, None, 0));
        let operation = state
            .threads
            .get_mut(thread_id)
            .and_then(|cell| {
                cell.operations.iter_mut().find(|operation| {
                    matches!(
                        &operation.kind,
                        ThreadOperationKind::Turn(candidate) if candidate == turn_id
                    )
                })
            })
            .ok_or_else(|| {
                Error::Message(format!(
                    "Turn reservation disappeared before durable acceptance: {turn_id}"
                ))
            })?;
        operation.durable_accepted = true;
        state
            .turns
            .get_mut(turn_id)
            .expect("Turn slot was validated while holding the runtime lock")
            .phase = TurnPhase::Active;
        let after = state
            .threads
            .get(thread_id)
            .map(thread_cell_activity)
            .unwrap_or((false, None, 0));
        Ok(
            Self::record_activity_transition(&mut state, before, after.clone())
                .unwrap_or_else(|| thread_activity_snapshot(state.activity_revision, after)),
        )
    }

    pub(super) fn set_turn_abort(&self, turn_id: &str, abort: tokio::task::AbortHandle) {
        if let Some(slot) = self.lock_state().turns.get_mut(turn_id) {
            slot.abort = Some(abort);
        }
    }

    pub(super) fn settle_turn(
        &self,
        thread_id: &str,
        turn_id: &str,
        pending_terminal: Option<PendingTerminal>,
    ) -> Option<ThreadActivitySnapshot> {
        let mut state = self.lock_state();
        let before = state
            .threads
            .get(thread_id)
            .map(thread_cell_activity)
            .unwrap_or((false, None, 0));
        Self::remove_thread_turn(&mut state, thread_id, turn_id);
        if let Some(pending_terminal) = pending_terminal {
            if let Some(slot) = state.turns.get_mut(turn_id) {
                slot.abort = None;
                slot.phase = TurnPhase::PendingTerminal;
                slot.pending_terminal = Some(pending_terminal);
            }
        } else {
            state.turns.remove(turn_id);
        }
        let after = state
            .threads
            .get(thread_id)
            .map(thread_cell_activity)
            .unwrap_or((false, None, 0));
        Self::record_activity_transition(&mut state, before, after)
    }

    fn remove_thread_turn(state: &mut ApplicationRuntimeState, thread_id: &str, turn_id: &str) {
        let remove = if let Some(cell) = state.threads.get_mut(thread_id) {
            cell.release(
                |kind| matches!(kind, ThreadOperationKind::Turn(queued) if queued == turn_id),
            );
            cell.operations.is_empty()
        } else {
            false
        };
        if remove {
            state.threads.remove(thread_id);
        }
        state.accepted_operations = state.accepted_operations.saturating_sub(1);
    }

    pub(super) fn turn_handle(&self, turn_id: &str) -> Option<TurnHandle> {
        self.lock_state()
            .turns
            .get(turn_id)
            .map(|slot| slot.handle.clone())
    }

    pub(super) fn pending_terminal(&self, turn_id: &str) -> Option<PendingTerminal> {
        self.lock_state()
            .turns
            .get(turn_id)
            .and_then(|slot| slot.pending_terminal.clone())
    }

    pub(super) fn remove_pending_terminal(&self, turn_id: &str) {
        self.lock_state().turns.remove(turn_id);
    }

    pub(super) fn thread_activity(&self, thread_id: &str) -> (bool, Option<String>, usize) {
        self.lock_state()
            .threads
            .get(thread_id)
            .map(thread_cell_activity)
            .unwrap_or((false, None, 0))
    }

    #[cfg(test)]
    pub(super) fn thread_activity_snapshot(
        &self,
    ) -> BTreeMap<String, (bool, Option<String>, usize)> {
        self.lock_state()
            .threads
            .iter()
            .filter_map(|(thread_id, cell)| {
                let activity = thread_cell_activity(cell);
                activity.0.then(|| (thread_id.clone(), activity))
            })
            .collect()
    }

    pub(super) fn versioned_thread_activity(&self, thread_id: &str) -> ThreadActivitySnapshot {
        let state = self.lock_state();
        let activity = state
            .threads
            .get(thread_id)
            .map(thread_cell_activity)
            .unwrap_or((false, None, 0));
        thread_activity_snapshot(state.activity_revision, activity)
    }

    pub(super) fn versioned_thread_activity_snapshot(&self) -> ApplicationActivitySnapshot {
        let state = self.lock_state();
        let threads = state
            .threads
            .iter()
            .filter_map(|(thread_id, cell)| {
                let activity = thread_cell_activity(cell);
                activity.0.then(|| {
                    (
                        thread_id.clone(),
                        thread_activity_snapshot(state.activity_revision, activity),
                    )
                })
            })
            .collect();
        ApplicationActivitySnapshot {
            revision: state.activity_revision,
            threads,
        }
    }

    pub(super) fn operational_snapshot(
        &self,
        storage: ApplicationStorageSnapshot,
    ) -> ApplicationOperationalSnapshot {
        let state = self.lock_state();
        ApplicationOperationalSnapshot {
            open: state.open,
            limits: self.limits,
            accepted_operations: state.accepted_operations,
            tracked_threads: state.threads.len(),
            tracked_tasks: self.tasks.len(),
            oldest_queued: application_oldest_queued(&state),
            storage,
            task_panics: self.task_panics.load(Ordering::Relaxed),
            panic_diagnostics: self
                .task_panic_diagnostics
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
                .cloned()
                .collect(),
        }
    }

    pub(super) fn thread_turn_handles(&self, thread_id: &str) -> Vec<TurnHandle> {
        let state = self.lock_state();
        let Some(cell) = state.threads.get(thread_id) else {
            return Vec::new();
        };
        cell.operations
            .iter()
            .filter_map(|operation| match &operation.kind {
                ThreadOperationKind::Turn(turn_id) => state.turns.get(turn_id),
                ThreadOperationKind::Mutation(_) | ThreadOperationKind::IdleMutation(_) => None,
            })
            .map(|slot| slot.handle.clone())
            .collect()
    }

    pub(super) fn active_controls(&self) -> Vec<crate::types::RunControlHandle> {
        self.lock_state()
            .turns
            .values()
            .filter(|slot| slot.phase == TurnPhase::Active)
            .map(|slot| slot.handle.control.clone())
            .collect()
    }

    pub(super) fn reserve_application_operation(
        self: &Arc<Self>,
    ) -> Result<ApplicationOperationReservation> {
        let operation_id = self.next_operation_id.fetch_add(1, Ordering::Relaxed);
        let mut state = self.lock_state();
        self.admit_application_operation(&state)?;
        let ready = state
            .application_operations
            .reserve(ThreadOperationKind::Mutation(operation_id), true);
        state.accepted_operations += 1;
        Ok(ApplicationOperationReservation {
            runtime: Arc::clone(self),
            operation_id,
            ready: Some(ready),
        })
    }

    pub(super) fn reserve_mutation(
        self: &Arc<Self>,
        thread_id: &str,
    ) -> Result<ThreadMutationReservation> {
        let operation_id = self.next_operation_id.fetch_add(1, Ordering::Relaxed);
        let mut state = self.lock_state();
        self.admit_thread_operation(&state, thread_id)?;
        let ready = state
            .threads
            .entry(thread_id.to_string())
            .or_default()
            .reserve(ThreadOperationKind::Mutation(operation_id), true);
        state.accepted_operations += 1;
        Ok(ThreadMutationReservation {
            runtime: Arc::clone(self),
            thread_id: thread_id.to_string(),
            operation_id,
            ready: Some(ready),
        })
    }

    pub(super) fn reserve_idle_mutation(
        self: &Arc<Self>,
        thread_id: &str,
    ) -> Result<ThreadMutationReservation> {
        let operation_id = self.next_operation_id.fetch_add(1, Ordering::Relaxed);
        let mut state = self.lock_state();
        if !state.open {
            return Err(Error::Message(
                "Psychevo Application is shutting down".to_string(),
            ));
        }
        if let Some(blocking_operation) =
            state.threads.get(thread_id).and_then(|cell| {
                if thread_cell_has_turn(cell) {
                    Some("turn")
                } else if cell.operations.iter().any(|operation| {
                    matches!(&operation.kind, ThreadOperationKind::IdleMutation(_))
                }) {
                    Some("history_editing")
                } else {
                    None
                }
            })
        {
            return Err(thread_busy(thread_id, blocking_operation));
        }
        self.admit_thread_operation(&state, thread_id)?;
        let ready = state
            .threads
            .entry(thread_id.to_string())
            .or_default()
            .reserve(ThreadOperationKind::IdleMutation(operation_id), true);
        state.accepted_operations += 1;
        Ok(ThreadMutationReservation {
            runtime: Arc::clone(self),
            thread_id: thread_id.to_string(),
            operation_id,
            ready: Some(ready),
        })
    }

    pub(super) fn thread_history_editing_busy(&self, thread_id: &str) -> bool {
        self.lock_state()
            .threads
            .get(thread_id)
            .is_some_and(|cell| {
                cell.operations.iter().any(|operation| {
                    matches!(
                        &operation.kind,
                        ThreadOperationKind::Turn(_) | ThreadOperationKind::IdleMutation(_)
                    )
                })
            })
    }

    #[cfg(test)]
    pub(super) fn thread_operation_count_for_test(&self, thread_id: &str) -> usize {
        self.lock_state()
            .threads
            .get(thread_id)
            .map_or(0, |cell| cell.operations.len())
    }

    fn finish_mutation(&self, thread_id: &str, operation_id: u64) {
        let mut state = self.lock_state();
        let remove = if let Some(cell) = state.threads.get_mut(thread_id) {
            cell.release(|kind| {
                matches!(
                    kind,
                    ThreadOperationKind::Mutation(id) | ThreadOperationKind::IdleMutation(id)
                        if *id == operation_id
                )
            });
            cell.operations.is_empty()
        } else {
            false
        };
        if remove {
            state.threads.remove(thread_id);
        }
        state.accepted_operations = state.accepted_operations.saturating_sub(1);
    }

    fn finish_application_operation(&self, operation_id: u64) {
        let mut state = self.lock_state();
        state.application_operations.release(
            |kind| matches!(kind, ThreadOperationKind::Mutation(id) if *id == operation_id),
        );
        state.accepted_operations = state.accepted_operations.saturating_sub(1);
    }

    fn admit_application_operation(&self, state: &ApplicationRuntimeState) -> Result<()> {
        if !state.open {
            return Err(Error::Message(
                "Psychevo Application is shutting down".to_string(),
            ));
        }
        if state.accepted_operations >= self.limits.max_operations {
            return Err(application_overloaded(
                "application",
                self.limits.max_operations,
                state.accepted_operations,
                application_oldest_queued(state),
                None,
            ));
        }
        Ok(())
    }

    fn admit_thread_operation(
        &self,
        state: &ApplicationRuntimeState,
        thread_id: &str,
    ) -> Result<()> {
        self.admit_application_operation(state)?;
        let thread_operations = state
            .threads
            .get(thread_id)
            .map_or(0, |cell| cell.operations.len());
        if thread_operations >= self.limits.max_thread_operations {
            let oldest_queued = state
                .threads
                .get(thread_id)
                .and_then(|cell| operations_oldest_queued(&cell.operations, Some(thread_id)));
            return Err(application_overloaded(
                "thread",
                self.limits.max_thread_operations,
                thread_operations,
                oldest_queued,
                Some(thread_id),
            ));
        }
        Ok(())
    }

    fn admit_turn_operation(&self, state: &ApplicationRuntimeState, thread_id: &str) -> Result<()> {
        if !state.open {
            return Err(Error::Message(
                "Psychevo Application is shutting down".to_string(),
            ));
        }
        if state.threads.get(thread_id).is_some_and(|cell| {
            cell.operations
                .iter()
                .any(|operation| matches!(&operation.kind, ThreadOperationKind::IdleMutation(_)))
        }) {
            return Err(thread_busy(thread_id, "history_editing"));
        }
        self.admit_thread_operation(state, thread_id)
    }

    fn record_activity_transition(
        state: &mut ApplicationRuntimeState,
        before: (bool, Option<String>, usize),
        after: (bool, Option<String>, usize),
    ) -> Option<ThreadActivitySnapshot> {
        if before == after {
            return None;
        }
        state.activity_revision = state.activity_revision.saturating_add(1);
        Some(thread_activity_snapshot(state.activity_revision, after))
    }

    pub(super) fn take_turn_slots(&self) -> Vec<TurnSlot> {
        let mut state = self.lock_state();
        let slots = state
            .turns
            .drain()
            .map(|(_, slot)| slot)
            .collect::<Vec<_>>();
        state.threads.clear();
        state.application_operations.operations.clear();
        state.accepted_operations = 0;
        slots
    }

    fn lock_state(&self) -> MutexGuard<'_, ApplicationRuntimeState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn lock_task_aborts(&self) -> MutexGuard<'_, HashMap<u64, tokio::task::AbortHandle>> {
        self.task_aborts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn application_overloaded(
    scope: &str,
    limit: usize,
    occupancy: usize,
    oldest_queued: Option<ApplicationQueuedOperationSnapshot>,
    thread_id: Option<&str>,
) -> Error {
    let mut data = serde_json::json!({
        "kind": "application_overloaded",
        "scope": scope,
        "limit": limit,
        "occupancy": occupancy,
        "retryable": true,
        "oldestQueuedAgeMs": oldest_queued.as_ref().map_or(0, |queued| queued.age_ms),
    });
    if let Some(queued) = oldest_queued {
        data["oldestQueuedOperationKind"] = serde_json::Value::String(queued.kind);
        data["oldestQueuedOperationId"] = serde_json::Value::String(queued.id);
        if let Some(thread_id) = queued.thread_id {
            data["oldestQueuedThreadId"] = serde_json::Value::String(thread_id);
        }
    }
    if let Some(thread_id) = thread_id {
        data["threadId"] = serde_json::Value::String(thread_id.to_string());
    }
    Error::structured(
        format!("Psychevo Application {scope} operation limit reached ({limit})"),
        data,
    )
}

fn application_oldest_queued(
    state: &ApplicationRuntimeState,
) -> Option<ApplicationQueuedOperationSnapshot> {
    let mut oldest = oldest_queued_operation(&state.application_operations.operations)
        .map(|operation| (None, operation));
    for (thread_id, cell) in &state.threads {
        let Some(candidate) = oldest_queued_operation(&cell.operations) else {
            continue;
        };
        if oldest
            .as_ref()
            .is_none_or(|(_, current)| candidate.enqueued_at < current.enqueued_at)
        {
            oldest = Some((Some(thread_id.as_str()), candidate));
        }
    }
    oldest.map(|(thread_id, operation)| queued_operation_snapshot(operation, thread_id))
}

fn operations_oldest_queued(
    operations: &VecDeque<ThreadOperation>,
    thread_id: Option<&str>,
) -> Option<ApplicationQueuedOperationSnapshot> {
    oldest_queued_operation(operations)
        .map(|operation| queued_operation_snapshot(operation, thread_id))
}

fn oldest_queued_operation(operations: &VecDeque<ThreadOperation>) -> Option<&ThreadOperation> {
    operations
        .iter()
        .filter(|operation| operation.ready.is_some())
        .min_by_key(|operation| operation.enqueued_at)
}

fn queued_operation_snapshot(
    operation: &ThreadOperation,
    thread_id: Option<&str>,
) -> ApplicationQueuedOperationSnapshot {
    let (kind, id) = match &operation.kind {
        ThreadOperationKind::Turn(id) => ("turn", id.clone()),
        ThreadOperationKind::Mutation(id) => ("mutation", id.to_string()),
        ThreadOperationKind::IdleMutation(id) => ("idle_mutation", id.to_string()),
    };
    ApplicationQueuedOperationSnapshot {
        kind: kind.to_string(),
        id,
        thread_id: thread_id.map(str::to_string),
        age_ms: u64::try_from(operation.enqueued_at.elapsed().as_millis()).unwrap_or(u64::MAX),
    }
}

impl ThreadCell {
    fn reserve(
        &mut self,
        kind: ThreadOperationKind,
        durable_accepted: bool,
    ) -> oneshot::Receiver<()> {
        let (ready_tx, ready_rx) = oneshot::channel();
        self.operations.push_back(ThreadOperation {
            kind,
            ready: Some(ready_tx),
            durable_accepted,
            enqueued_at: Instant::now(),
        });
        if self.operations.len() == 1 {
            Self::release_front_waiter(&mut self.operations);
        }
        ready_rx
    }

    fn release(&mut self, mut matches: impl FnMut(&ThreadOperationKind) -> bool) {
        let Some(index) = self
            .operations
            .iter()
            .position(|operation| matches(&operation.kind))
        else {
            return;
        };
        let released_front = index == 0;
        self.operations.remove(index);
        if released_front {
            Self::release_front_waiter(&mut self.operations);
        }
    }

    fn release_front_waiter(operations: &mut VecDeque<ThreadOperation>) {
        if let Some(ready) = operations
            .front_mut()
            .and_then(|operation| operation.ready.take())
        {
            let _ = ready.send(());
        }
    }
}

struct TrackedTaskGuard {
    runtime: Weak<ApplicationRuntime>,
    task_id: u64,
}

impl Drop for TrackedTaskGuard {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.upgrade() {
            runtime.lock_task_aborts().remove(&self.task_id);
        }
    }
}

pub(super) struct ThreadMutationReservation {
    runtime: Arc<ApplicationRuntime>,
    thread_id: String,
    operation_id: u64,
    pub(super) ready: Option<oneshot::Receiver<()>>,
}

pub(super) struct ApplicationOperationReservation {
    runtime: Arc<ApplicationRuntime>,
    operation_id: u64,
    ready: Option<oneshot::Receiver<()>>,
}

impl ThreadMutationReservation {
    pub(super) async fn acquire(mut self) -> Result<Self> {
        self.ready
            .take()
            .expect("Thread mutation reservation already acquired")
            .await
            .map_err(|_| Error::Message("Thread mutation reservation was cancelled".to_string()))?;
        Ok(self)
    }
}

impl Drop for ThreadMutationReservation {
    fn drop(&mut self) {
        self.runtime
            .finish_mutation(&self.thread_id, self.operation_id);
    }
}

impl ApplicationOperationReservation {
    pub(super) async fn acquire(mut self) -> Result<Self> {
        self.ready
            .take()
            .expect("Application operation reservation already acquired")
            .await
            .map_err(|_| {
                Error::Message("Application operation reservation was cancelled".to_string())
            })?;
        Ok(self)
    }
}

fn thread_cell_activity(cell: &ThreadCell) -> (bool, Option<String>, usize) {
    let mut pending_turn_precedes_head = false;
    let mut active_turn_id = None;
    let mut accepted_turns = 0_usize;
    for operation in &cell.operations {
        let ThreadOperationKind::Turn(turn_id) = &operation.kind else {
            continue;
        };
        if !operation.durable_accepted {
            if active_turn_id.is_none() {
                pending_turn_precedes_head = true;
            }
            continue;
        }
        accepted_turns += 1;
        if active_turn_id.is_none() && !pending_turn_precedes_head {
            active_turn_id = Some(turn_id.clone());
        }
    }
    (
        accepted_turns > 0,
        active_turn_id.clone(),
        accepted_turns.saturating_sub(usize::from(active_turn_id.is_some())),
    )
}

fn thread_cell_has_turn(cell: &ThreadCell) -> bool {
    cell.operations
        .iter()
        .any(|operation| matches!(&operation.kind, ThreadOperationKind::Turn(_)))
}

fn thread_busy(thread_id: &str, blocking_operation: &str) -> Error {
    Error::structured(
        format!("Thread `{thread_id}` is busy with {blocking_operation}"),
        serde_json::json!({
            "kind": "thread_busy",
            "threadId": thread_id,
            "blockingOperation": blocking_operation,
            "retryable": true,
        }),
    )
}

fn thread_turn_queue_position(cell: &ThreadCell) -> usize {
    let active_turn = cell
        .operations
        .front()
        .is_some_and(|operation| matches!(&operation.kind, ThreadOperationKind::Turn(_)));
    cell.operations
        .iter()
        .filter(|operation| matches!(&operation.kind, ThreadOperationKind::Turn(_)))
        .count()
        .saturating_sub(usize::from(active_turn))
}

fn thread_activity_snapshot(
    revision: u64,
    (running, active_turn_id, queued_turns): (bool, Option<String>, usize),
) -> ThreadActivitySnapshot {
    ThreadActivitySnapshot {
        revision,
        running,
        active_turn_id,
        queued_turns,
    }
}

impl Drop for ApplicationOperationReservation {
    fn drop(&mut self) {
        self.runtime.finish_application_operation(self.operation_id);
    }
}

#[cfg(test)]
mod tests {
    use super::ApplicationRuntime;
    use crate::application::ApplicationLimits;
    use crate::types::{McpServerInput, McpTransportInput};
    use std::path::Path;
    use std::sync::Arc;

    #[test]
    fn idle_mutation_and_turn_admission_are_mutually_exclusive_under_runtime_state() {
        let runtime = Arc::new(ApplicationRuntime::new(ApplicationLimits::default()));
        let idle = runtime
            .reserve_idle_mutation("thread")
            .expect("idle reservation");

        let error = runtime
            .reserve_idle_mutation("thread")
            .err()
            .expect("idle-only history mutations cannot queue behind each other");
        assert_eq!(
            error.structured_data().expect("structured busy")["kind"],
            "thread_busy"
        );
        assert_eq!(
            error.structured_data().expect("structured busy")["blockingOperation"],
            "history_editing"
        );
        assert_eq!(runtime.thread_operation_count_for_test("thread"), 1);

        let error = runtime
            .reserve_turn_for_test("thread", "turn-after-idle")
            .expect_err("Turn cannot pass an idle-only reservation");
        assert_eq!(
            error.structured_data().expect("structured busy")["kind"],
            "thread_busy"
        );
        assert_eq!(
            error.structured_data().expect("structured busy")["blockingOperation"],
            "history_editing"
        );

        drop(idle);
        let turn = runtime
            .reserve_turn_for_test("thread", "turn-first")
            .expect("Turn reservation");
        let error = runtime
            .reserve_idle_mutation("thread")
            .err()
            .expect("idle-only mutation cannot pass a Turn reservation");
        assert_eq!(
            error.structured_data().expect("structured busy")["kind"],
            "thread_busy"
        );
        assert_eq!(
            error.structured_data().expect("structured busy")["blockingOperation"],
            "turn"
        );
        drop(turn);
        runtime.settle_turn("thread", "turn-first", None);
    }

    #[test]
    fn ordinary_thread_mutation_keeps_existing_fifo_admission_semantics() {
        let runtime = Arc::new(ApplicationRuntime::new(ApplicationLimits::default()));
        let mutation = runtime
            .reserve_mutation("thread")
            .expect("ordinary mutation");
        let turn = runtime
            .reserve_turn_for_test("thread", "turn")
            .expect("ordinary mutation does not reject Turn admission");

        drop(turn);
        runtime.settle_turn("thread", "turn", None);
        drop(mutation);
    }

    #[test]
    fn thread_capacity_rejects_before_admitting_the_thirty_third_operation() {
        let limits = ApplicationLimits::default();
        let runtime = Arc::new(ApplicationRuntime::new(limits));
        let mut reservations = Vec::new();
        for _ in 0..limits.max_thread_operations {
            reservations.push(
                runtime
                    .reserve_mutation("thread")
                    .expect("operation below Thread capacity"),
            );
        }

        let error = runtime
            .reserve_mutation("thread")
            .err()
            .expect("thirty-third Thread operation");
        let data = error.structured_data().expect("structured overload");
        assert_eq!(data["kind"], "application_overloaded");
        assert_eq!(data["scope"], "thread");
        assert_eq!(data["limit"], limits.max_thread_operations);
        assert_eq!(data["occupancy"], limits.max_thread_operations);
        assert_eq!(data["retryable"], true);
        assert_eq!(data["threadId"], "thread");
        assert!(data["oldestQueuedAgeMs"].as_u64().is_some());
        assert_eq!(data["oldestQueuedOperationKind"], "mutation");
        assert!(data["oldestQueuedOperationId"].as_str().is_some());
        assert_eq!(data["oldestQueuedThreadId"], "thread");
        assert_eq!(
            runtime.lock_state().accepted_operations,
            limits.max_thread_operations
        );
    }

    #[test]
    fn application_capacity_rejects_before_admitting_the_sixty_fifth_operation() {
        let limits = ApplicationLimits::default();
        let runtime = Arc::new(ApplicationRuntime::new(limits));
        let mut reservations = Vec::new();
        for index in 0..limits.max_operations {
            reservations.push(
                runtime
                    .reserve_mutation(&format!("thread-{index}"))
                    .expect("operation below Application capacity"),
            );
        }

        let error = runtime
            .reserve_mutation("overflow")
            .err()
            .expect("sixty-fifth Application operation");
        let data = error.structured_data().expect("structured overload");
        assert_eq!(data["kind"], "application_overloaded");
        assert_eq!(data["scope"], "application");
        assert_eq!(data["limit"], limits.max_operations);
        assert_eq!(data["occupancy"], limits.max_operations);
        assert_eq!(data["retryable"], true);
        assert!(data.get("threadId").is_none());
        assert!(data["oldestQueuedAgeMs"].as_u64().is_some());
        assert!(data.get("oldestQueuedOperationId").is_none());
        assert_eq!(
            runtime.lock_state().accepted_operations,
            limits.max_operations
        );
    }

    #[tokio::test]
    async fn mcp_runtime_is_lazy_thread_owned_and_released_with_thread_lifecycle() {
        let runtime = ApplicationRuntime::new(ApplicationLimits::default());
        let first = runtime.mcp_runtime("thread-a");
        let same_thread = runtime.mcp_runtime("thread-a");
        let other_thread = runtime.mcp_runtime("thread-b");

        assert_eq!(
            runtime.mcp_runtimes.len(),
            0,
            "constructing a lazy Thread handle must not materialize a registry entry"
        );
        first.snapshot(&[], Path::new("."), None, false).await;
        assert_eq!(
            runtime.mcp_runtimes.len(),
            0,
            "an empty MCP input must not materialize a registry entry"
        );
        first
            .snapshot(
                &[McpServerInput::new(
                    "repo",
                    McpTransportInput::Unsupported {
                        kind: "test".to_string(),
                    },
                )],
                Path::new("."),
                None,
                false,
            )
            .await;
        assert_eq!(runtime.mcp_runtimes.len(), 1);
        assert!(first.same_instance(&same_thread));
        assert!(!first.same_instance(&other_thread));

        runtime.remove_mcp_runtime("thread-a");
        assert_eq!(runtime.mcp_runtimes.len(), 0);
        let reopened = runtime.mcp_runtime("thread-a");
        reopened
            .snapshot(
                &[McpServerInput::new(
                    "repo",
                    McpTransportInput::Unsupported {
                        kind: "test".to_string(),
                    },
                )],
                Path::new("."),
                None,
                false,
            )
            .await;
        assert_eq!(runtime.mcp_runtimes.len(), 1);

        runtime.clear_mcp_runtimes();
        assert_eq!(runtime.mcp_runtimes.len(), 0);
    }

    #[test]
    fn agent_supervisor_pause_state_is_application_scoped_and_parent_scoped() {
        let first = ApplicationRuntime::new(ApplicationLimits::default());
        let second = ApplicationRuntime::new(ApplicationLimits::default());

        first.agent_supervisor.set_spawning_paused("thread-a", true);

        assert!(first.agent_supervisor.spawning_paused("thread-a"));
        assert!(!first.agent_supervisor.spawning_paused("thread-b"));
        assert!(!second.agent_supervisor.spawning_paused("thread-a"));
    }
}
