use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use futures::FutureExt;
use tokio::sync::{OwnedRwLockReadGuard, RwLock, oneshot};
use tokio_util::task::TaskTracker;

use super::{Error, PendingTerminal, Result, TurnHandle};

pub(super) const MAX_APPLICATION_OPERATIONS: usize = 64;
pub(super) const MAX_THREAD_OPERATIONS: usize = 32;

pub(super) struct ApplicationRuntime {
    pub(super) tasks: TaskTracker,
    pub(super) state: Mutex<ApplicationRuntimeState>,
    admission_gate: Arc<RwLock<()>>,
    task_aborts: Mutex<HashMap<u64, tokio::task::AbortHandle>>,
    next_task_id: AtomicU64,
    next_operation_id: AtomicU64,
    pub(super) task_panics: AtomicU64,
    mcp_runtimes: crate::mcp::McpRuntimeRegistry,
    pub(super) agent_supervisor: crate::agents::AgentSupervisor,
}

pub(super) struct ApplicationRuntimeState {
    open: bool,
    accepted_operations: usize,
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
}

pub(super) enum ThreadOperationKind {
    Turn(String),
    Mutation(u64),
}

pub(super) struct TurnSlot {
    pub(super) handle: TurnHandle,
    pub(super) abort: Option<tokio::task::AbortHandle>,
    pub(super) phase: TurnPhase,
    pub(super) pending_terminal: Option<PendingTerminal>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TurnPhase {
    Active,
    PendingTerminal,
}

impl ApplicationRuntime {
    pub(super) fn new() -> Self {
        Self {
            tasks: TaskTracker::new(),
            state: Mutex::new(ApplicationRuntimeState {
                open: true,
                accepted_operations: 0,
                application_operations: ThreadCell::default(),
                threads: HashMap::new(),
                turns: HashMap::new(),
            }),
            admission_gate: Arc::new(RwLock::new(())),
            task_aborts: Mutex::new(HashMap::new()),
            next_task_id: AtomicU64::new(1),
            next_operation_id: AtomicU64::new(1),
            task_panics: AtomicU64::new(0),
            mcp_runtimes: crate::mcp::McpRuntimeRegistry::default(),
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
        let task_id = self.next_task_id.fetch_add(1, Ordering::Relaxed);
        let (start_tx, start_rx) = oneshot::channel();
        let runtime = Arc::downgrade(self);
        let panic_runtime = Arc::downgrade(self);
        let task = self.tasks.spawn(async move {
            let _guard = TrackedTaskGuard { runtime, task_id };
            if start_rx.await.is_err() {
                return;
            }
            let panicked = std::panic::AssertUnwindSafe(future)
                .catch_unwind()
                .await
                .is_err();
            if panicked {
                let Some(runtime) = panic_runtime.upgrade() else {
                    return;
                };
                runtime.task_panics.fetch_add(1, Ordering::Relaxed);
            }
        });
        let abort = task.abort_handle();
        self.lock_task_aborts().insert(task_id, abort.clone());
        let _ = start_tx.send(());
        abort
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
        Self::admit_thread_operation(&state, thread_id)?;
        let cell = state.threads.entry(thread_id.to_string()).or_default();
        let ready = cell.reserve(ThreadOperationKind::Turn(turn_id.to_string()));
        state.accepted_operations += 1;
        Ok(ready)
    }

    pub(super) fn register_turn(
        &self,
        thread_id: &str,
        turn_id: &str,
        handle: TurnHandle,
    ) -> Result<oneshot::Receiver<()>> {
        let mut state = self.lock_state();
        Self::admit_thread_operation(&state, thread_id)?;
        if state.turns.contains_key(turn_id) {
            return Err(Error::Message(format!(
                "Turn id is already registered: {turn_id}"
            )));
        }
        let lane = state
            .threads
            .entry(thread_id.to_string())
            .or_default()
            .reserve(ThreadOperationKind::Turn(turn_id.to_string()));
        state.accepted_operations += 1;
        state.turns.insert(
            turn_id.to_string(),
            TurnSlot {
                handle,
                abort: None,
                phase: TurnPhase::Active,
                pending_terminal: None,
            },
        );
        Ok(lane)
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
    ) {
        let mut state = self.lock_state();
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
            .map(|cell| {
                let active_turn_id = cell
                    .operations
                    .front()
                    .and_then(|operation| match &operation.kind {
                        ThreadOperationKind::Turn(turn_id) => Some(turn_id.clone()),
                        ThreadOperationKind::Mutation(_) => None,
                    });
                let running = !cell.operations.is_empty();
                let queued = cell
                    .operations
                    .iter()
                    .filter(|operation| matches!(&operation.kind, ThreadOperationKind::Turn(_)))
                    .count()
                    .saturating_sub(usize::from(active_turn_id.is_some()));
                (running, active_turn_id, queued)
            })
            .unwrap_or((false, None, 0))
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
                ThreadOperationKind::Mutation(_) => None,
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
        Self::admit_application_operation(&state)?;
        let ready = state
            .application_operations
            .reserve(ThreadOperationKind::Mutation(operation_id));
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
        Self::admit_thread_operation(&state, thread_id)?;
        let cell = state.threads.entry(thread_id.to_string()).or_default();
        let ready = cell.reserve(ThreadOperationKind::Mutation(operation_id));
        state.accepted_operations += 1;
        Ok(ThreadMutationReservation {
            runtime: Arc::clone(self),
            thread_id: thread_id.to_string(),
            operation_id,
            ready: Some(ready),
        })
    }

    fn finish_mutation(&self, thread_id: &str, operation_id: u64) {
        let mut state = self.lock_state();
        let remove = if let Some(cell) = state.threads.get_mut(thread_id) {
            cell.release(
                |kind| matches!(kind, ThreadOperationKind::Mutation(id) if *id == operation_id),
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

    fn finish_application_operation(&self, operation_id: u64) {
        let mut state = self.lock_state();
        state.application_operations.release(
            |kind| matches!(kind, ThreadOperationKind::Mutation(id) if *id == operation_id),
        );
        state.accepted_operations = state.accepted_operations.saturating_sub(1);
    }

    fn admit_application_operation(state: &ApplicationRuntimeState) -> Result<()> {
        if !state.open {
            return Err(Error::Message(
                "Psychevo Application is shutting down".to_string(),
            ));
        }
        if state.accepted_operations >= MAX_APPLICATION_OPERATIONS {
            return Err(application_overloaded(
                "application",
                MAX_APPLICATION_OPERATIONS,
            ));
        }
        Ok(())
    }

    fn admit_thread_operation(state: &ApplicationRuntimeState, thread_id: &str) -> Result<()> {
        Self::admit_application_operation(state)?;
        let thread_operations = state
            .threads
            .get(thread_id)
            .map_or(0, |cell| cell.operations.len());
        if thread_operations >= MAX_THREAD_OPERATIONS {
            return Err(application_overloaded("thread", MAX_THREAD_OPERATIONS));
        }
        Ok(())
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

fn application_overloaded(scope: &str, limit: usize) -> Error {
    Error::structured(
        format!("Psychevo Application {scope} operation limit reached ({limit})"),
        serde_json::json!({
            "kind": "application_overloaded",
            "scope": scope,
            "limit": limit,
        }),
    )
}

impl ThreadCell {
    fn reserve(&mut self, kind: ThreadOperationKind) -> oneshot::Receiver<()> {
        let (ready_tx, ready_rx) = oneshot::channel();
        self.operations.push_back(ThreadOperation {
            kind,
            ready: Some(ready_tx),
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

impl Drop for ApplicationOperationReservation {
    fn drop(&mut self) {
        self.runtime.finish_application_operation(self.operation_id);
    }
}

#[cfg(test)]
mod tests {
    use super::{ApplicationRuntime, MAX_APPLICATION_OPERATIONS, MAX_THREAD_OPERATIONS};
    use crate::types::{McpServerInput, McpTransportInput};
    use serde_json::json;
    use std::path::Path;
    use std::sync::Arc;

    #[test]
    fn thread_capacity_rejects_before_admitting_the_thirty_third_operation() {
        let runtime = Arc::new(ApplicationRuntime::new());
        let mut reservations = Vec::new();
        for _ in 0..MAX_THREAD_OPERATIONS {
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
        assert_eq!(
            error.structured_data(),
            Some(&json!({
                "kind": "application_overloaded",
                "scope": "thread",
                "limit": MAX_THREAD_OPERATIONS,
            }))
        );
        assert_eq!(
            runtime.lock_state().accepted_operations,
            MAX_THREAD_OPERATIONS
        );
    }

    #[test]
    fn application_capacity_rejects_before_admitting_the_sixty_fifth_operation() {
        let runtime = Arc::new(ApplicationRuntime::new());
        let mut reservations = Vec::new();
        for index in 0..MAX_APPLICATION_OPERATIONS {
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
        assert_eq!(
            error.structured_data(),
            Some(&json!({
                "kind": "application_overloaded",
                "scope": "application",
                "limit": MAX_APPLICATION_OPERATIONS,
            }))
        );
        assert_eq!(
            runtime.lock_state().accepted_operations,
            MAX_APPLICATION_OPERATIONS
        );
    }

    #[tokio::test]
    async fn mcp_runtime_is_lazy_thread_owned_and_released_with_thread_lifecycle() {
        let runtime = ApplicationRuntime::new();
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
        let first = ApplicationRuntime::new();
        let second = ApplicationRuntime::new();

        first.agent_supervisor.set_spawning_paused("thread-a", true);

        assert!(first.agent_supervisor.spawning_paused("thread-a"));
        assert!(!first.agent_supervisor.spawning_paused("thread-b"));
        assert!(!second.agent_supervisor.spawning_paused("thread-a"));
    }
}
