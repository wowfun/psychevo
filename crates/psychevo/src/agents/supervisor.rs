use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use psychevo_agent_core::ControlHandle;
use tokio_util::task::TaskTracker;

use super::{AgentRunRecord, Error, Result};

pub(crate) const DEFAULT_CHILD_CONCURRENCY: usize = 4;
pub(crate) type BackgroundAgentFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

#[derive(Clone)]
pub(crate) struct AgentSupervisor {
    inner: Arc<AgentSupervisorInner>,
}

struct AgentSupervisorInner {
    active: Mutex<HashMap<String, AgentRunState>>,
    paused_parents: Mutex<HashSet<String>>,
    background_tasks: TaskTracker,
    background_task_aborts: Mutex<HashMap<u64, tokio::task::AbortHandle>>,
    background_admission: Mutex<bool>,
    next_background_task_id: AtomicU64,
}

pub(crate) struct AgentRunState {
    pub(crate) record: AgentRunRecord,
    pub(crate) control: Option<ControlHandle>,
}

impl Default for AgentSupervisor {
    fn default() -> Self {
        Self {
            inner: Arc::new(AgentSupervisorInner {
                active: Mutex::new(HashMap::new()),
                paused_parents: Mutex::new(HashSet::new()),
                background_tasks: TaskTracker::new(),
                background_task_aborts: Mutex::new(HashMap::new()),
                background_admission: Mutex::new(true),
                next_background_task_id: AtomicU64::new(1),
            }),
        }
    }
}

impl AgentSupervisor {
    pub(crate) fn active(&self) -> MutexGuard<'_, HashMap<String, AgentRunState>> {
        self.inner
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn register(
        &self,
        record: AgentRunRecord,
        control: Option<ControlHandle>,
        limit: usize,
    ) -> Result<()> {
        let admission = self
            .inner
            .background_admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !*admission {
            return Err(Error::Message(
                "agent supervisor is shutting down".to_string(),
            ));
        }
        let limit = limit.clamp(1, DEFAULT_CHILD_CONCURRENCY);
        let mut active = self.active();
        let count = active
            .values()
            .filter(|state| state.record.parent_session_id == record.parent_session_id)
            .count();
        if count >= limit {
            return Err(Error::Message(format!(
                "agent concurrency limit reached for parent session {}: {count}/{limit}",
                record.parent_session_id
            )));
        }
        active.insert(record.id.clone(), AgentRunState { record, control });
        Ok(())
    }

    pub(crate) fn remove(&self, id: &str) -> Option<AgentRunRecord> {
        self.active().remove(id).map(|state| state.record)
    }

    pub(crate) fn spawning_paused(&self, parent_session_id: &str) -> bool {
        self.inner
            .paused_parents
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(parent_session_id)
    }

    pub(crate) fn set_spawning_paused(&self, parent_session_id: &str, paused: bool) -> bool {
        let mut paused_parents = self
            .inner
            .paused_parents
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = paused_parents.contains(parent_session_id);
        if paused {
            paused_parents.insert(parent_session_id.to_string());
        } else {
            paused_parents.remove(parent_session_id);
        }
        previous
    }

    pub(crate) fn spawn_background(
        &self,
        future: BackgroundAgentFuture,
    ) -> std::result::Result<(), BackgroundAgentFuture> {
        let admission = self
            .inner
            .background_admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !*admission {
            return Err(future);
        }
        let task_id = self
            .inner
            .next_background_task_id
            .fetch_add(1, Ordering::Relaxed);
        let guard = BackgroundTaskGuard {
            inner: Arc::downgrade(&self.inner),
            task_id,
        };
        let (start_tx, start_rx) = tokio::sync::oneshot::channel();
        let task = self.inner.background_tasks.spawn(async move {
            let _guard = guard;
            if start_rx.await.is_err() {
                return;
            }
            future.await;
        });
        self.inner
            .background_task_aborts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(task_id, task.abort_handle());
        let _ = start_tx.send(());
        drop(admission);
        Ok(())
    }

    pub(crate) fn close_and_cancel(&self) {
        let mut admission = self
            .inner
            .background_admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *admission {
            *admission = false;
            self.inner.background_tasks.close();
        }
        drop(admission);
        let controls = self
            .active()
            .values()
            .filter_map(|state| state.control.clone())
            .collect::<Vec<_>>();
        for control in controls {
            control.stop();
            control.abort();
        }
    }

    pub(crate) async fn wait_background(&self) {
        self.inner.background_tasks.wait().await;
    }

    pub(crate) fn abort_background(&self) -> usize {
        let aborts = self
            .inner
            .background_task_aborts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let count = aborts.len();
        for abort in aborts {
            abort.abort();
        }
        count
    }

    pub(crate) async fn shutdown_graceful(&self) {
        self.close_and_cancel();
        self.wait_background().await;
    }
}

struct BackgroundTaskGuard {
    inner: Weak<AgentSupervisorInner>,
    task_id: u64,
}

impl Drop for BackgroundTaskGuard {
    fn drop(&mut self) {
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        inner
            .background_task_aborts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.task_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{AgentInvocationRole, AgentRunStatus};

    fn record(id: &str, parent: &str) -> AgentRunRecord {
        AgentRunRecord {
            id: id.to_string(),
            task_name: Some(id.to_string()),
            agent_name: "worker".to_string(),
            task: "test".to_string(),
            parent_session_id: parent.to_string(),
            child_session_id: Some(format!("child-{id}")),
            role: AgentInvocationRole::Subagent,
            background: true,
            status: AgentRunStatus::Running,
            edge_status: None,
            started_at_ms: 0,
            ended_at_ms: None,
            outcome: None,
            final_answer: None,
            error: None,
            effective_max_spawn_depth: Some(0),
            team_run_id: None,
            mission_run_id: None,
            team_name: None,
            team_member_id: None,
            agent_path: None,
        }
    }

    #[test]
    fn fifth_child_for_one_parent_fails_fast_without_affecting_another_parent() {
        let supervisor = AgentSupervisor::default();
        for index in 0..DEFAULT_CHILD_CONCURRENCY {
            supervisor
                .register(record(&format!("a-{index}"), "parent-a"), None, 4)
                .expect("slot");
        }

        let error = supervisor
            .register(record("a-overflow", "parent-a"), None, 4)
            .expect_err("fifth child must fail");
        assert!(error.to_string().contains("4/4"), "{error}");
        supervisor
            .register(record("b-0", "parent-b"), None, 4)
            .expect("other parent owns an independent quota");
    }

    #[test]
    fn configured_team_limit_can_narrow_but_never_exceed_four() {
        let supervisor = AgentSupervisor::default();
        supervisor
            .register(record("one", "parent"), None, 2)
            .expect("one");
        supervisor
            .register(record("two", "parent"), None, 2)
            .expect("two");
        assert!(
            supervisor
                .register(record("three", "parent"), None, 2)
                .is_err()
        );

        let other = AgentSupervisor::default();
        for index in 0..4 {
            other
                .register(record(&format!("wide-{index}"), "parent"), None, 99)
                .expect("hard-cap slot");
        }
        assert!(
            other
                .register(record("wide-overflow", "parent"), None, 99)
                .is_err()
        );
    }

    #[tokio::test]
    async fn graceful_shutdown_cancels_and_awaits_background_tasks() {
        let supervisor = AgentSupervisor::default();
        let (control, receivers) = ControlHandle::new();
        supervisor
            .register(record("child", "parent"), Some(control), 4)
            .expect("run");
        let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
        let mut abort = receivers.abort_signal();
        assert!(
            supervisor
                .spawn_background(Box::pin(async move {
                    abort.wait_for_abort().await;
                    let _ = finished_tx.send(());
                }))
                .is_ok(),
            "background task should be admitted"
        );

        supervisor.shutdown_graceful().await;

        finished_rx
            .await
            .expect("shutdown returned before the background task finalized");
    }

    #[tokio::test]
    async fn rejected_background_work_still_runs_its_finalizer() {
        let supervisor = AgentSupervisor::default();
        supervisor.close_and_cancel();
        let finalized = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let finalized_in_task = finalized.clone();

        let rejected = supervisor
            .spawn_background(Box::pin(async move {
                finalized_in_task.store(true, Ordering::SeqCst);
            }))
            .expect_err("closed admission");
        rejected.await;

        assert!(finalized.load(Ordering::SeqCst));
    }
}
