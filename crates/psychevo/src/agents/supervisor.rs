use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use psychevo_agent_core::{ControlHandle, Message};
use tokio_util::task::TaskTracker;

use super::{AgentRunRecord, AgentRunStatus, Error, Result, StateRuntime};

pub(crate) const DEFAULT_CHILD_CONCURRENCY: usize = 4;
pub(crate) type BackgroundAgentFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

#[derive(Clone)]
pub(crate) struct AgentSupervisor {
    inner: Arc<AgentSupervisorInner>,
}

struct AgentSupervisorInner {
    slots: Mutex<HashMap<String, AgentRunState>>,
    paused_parents: Mutex<HashSet<String>>,
    background_tasks: TaskTracker,
    background_task_aborts: Mutex<HashMap<u64, tokio::task::AbortHandle>>,
    background_admission: Mutex<bool>,
    next_background_task_id: AtomicU64,
    task_panics: AtomicU64,
}

pub(crate) struct AgentRunState {
    pub(crate) record: AgentRunRecord,
    pub(crate) control: Option<ControlHandle>,
    pub(crate) phase: AgentRunPhase,
    terminal_commit: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentRunPhase {
    Active,
    PendingTerminal,
}

pub(crate) enum ContinuationAdmission {
    Registered,
    Injected(Box<AgentRunRecord>),
}

impl Default for AgentSupervisor {
    fn default() -> Self {
        Self {
            inner: Arc::new(AgentSupervisorInner {
                slots: Mutex::new(HashMap::new()),
                paused_parents: Mutex::new(HashSet::new()),
                background_tasks: TaskTracker::new(),
                background_task_aborts: Mutex::new(HashMap::new()),
                background_admission: Mutex::new(true),
                next_background_task_id: AtomicU64::new(1),
                task_panics: AtomicU64::new(0),
            }),
        }
    }
}

impl AgentSupervisor {
    pub(crate) fn slots(&self) -> MutexGuard<'_, HashMap<String, AgentRunState>> {
        self.inner
            .slots
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
        let mut slots = self.slots();
        let count = slots
            .values()
            .filter(|state| state.phase == AgentRunPhase::Active)
            .filter(|state| state.record.parent_session_id == record.parent_session_id)
            .count();
        if count >= limit {
            return Err(Error::Message(format!(
                "agent concurrency limit reached for parent session {}: {count}/{limit}",
                record.parent_session_id
            )));
        }
        match slots.entry(record.id.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(AgentRunState {
                    record,
                    control,
                    phase: AgentRunPhase::Active,
                    terminal_commit: Arc::new(tokio::sync::Mutex::new(())),
                });
                Ok(())
            }
            Entry::Occupied(_) => Err(Error::Message(format!(
                "agent id is already supervised: {}",
                record.id
            ))),
        }
    }

    pub(crate) fn register_continuation_or_inject(
        &self,
        record: AgentRunRecord,
        control: ControlHandle,
        message: Message,
        limit: usize,
    ) -> Result<ContinuationAdmission> {
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
        let mut slots = self.slots();
        let active_for_parent = slots
            .values()
            .filter(|state| state.phase == AgentRunPhase::Active)
            .filter(|state| state.record.parent_session_id == record.parent_session_id)
            .count();
        match slots.entry(record.id.clone()) {
            Entry::Occupied(entry) => {
                let state = entry.get();
                if state.phase == AgentRunPhase::PendingTerminal {
                    return Err(Error::TerminalPersistence {
                        turn_id: format!("agent:{}", record.id),
                        message: "the prior Agent terminal is still pending".to_string(),
                    });
                }
                let active_record = state.record.clone();
                let active_control = state.control.clone().ok_or_else(|| {
                    Error::Message(format!(
                        "active Agent `{}` has no control handle",
                        record.id
                    ))
                })?;
                active_control
                    .inject_user_message(message)
                    .map_err(|error| {
                        Error::Message(format!(
                            "failed to deliver message to agent `{}`: {error}",
                            record.id
                        ))
                    })?;
                Ok(ContinuationAdmission::Injected(Box::new(active_record)))
            }
            Entry::Vacant(entry) => {
                if active_for_parent >= limit {
                    return Err(Error::Message(format!(
                        "agent concurrency limit reached for parent session {}: {active_for_parent}/{limit}",
                        record.parent_session_id
                    )));
                }
                entry.insert(AgentRunState {
                    record,
                    control: Some(control),
                    phase: AgentRunPhase::Active,
                    terminal_commit: Arc::new(tokio::sync::Mutex::new(())),
                });
                Ok(ContinuationAdmission::Registered)
            }
        }
    }

    pub(crate) fn remove(&self, id: &str) -> Option<AgentRunRecord> {
        self.slots().remove(id).map(|state| state.record)
    }

    pub(crate) fn remove_unpersisted(&self, id: &str) -> Option<AgentRunRecord> {
        let mut slots = self.slots();
        if slots
            .get(id)
            .is_some_and(|state| state.record.child_session_id.is_none())
        {
            return slots.remove(id).map(|state| state.record);
        }
        None
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
            .slots()
            .values()
            .filter(|state| state.phase == AgentRunPhase::Active)
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

    pub(crate) fn stage_task_panic(&self, id: &str, evidence: &str) {
        self.inner.task_panics.fetch_add(1, Ordering::Relaxed);
        let mut slots = self.slots();
        let Some(state) = slots.get_mut(id) else {
            return;
        };
        if state.phase != AgentRunPhase::Active
            || super::lifecycle::agent_status_is_final(state.record.status)
        {
            return;
        }
        state.record.status = AgentRunStatus::Errored;
        state.record.edge_status = Some(super::AgentEdgeStatus::Closed);
        state.record.ended_at_ms = Some(super::mailbox_tools::now_ms());
        state.record.outcome = Some("failed".to_string());
        state.record.error = Some(evidence.to_string());
    }

    #[cfg(test)]
    pub(crate) fn record_task_panic(&self) {
        self.inner.task_panics.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn task_panics(&self) -> u64 {
        self.inner.task_panics.load(Ordering::Relaxed)
    }

    pub(crate) fn stage_remaining_interrupted(&self, reason: &str) {
        let mut slots = self.slots();
        slots.retain(|_, state| {
            if state.phase != AgentRunPhase::Active {
                return true;
            }
            if state.record.child_session_id.is_none() {
                return false;
            }
            if !super::lifecycle::agent_status_is_final(state.record.status) {
                state.record.status = AgentRunStatus::Interrupted;
                state.record.edge_status = Some(super::AgentEdgeStatus::Closed);
                state.record.ended_at_ms = Some(super::mailbox_tools::now_ms());
                state.record.outcome = Some("interrupted".to_string());
                state.record.error = Some(reason.to_string());
            }
            state.control = None;
            state.phase = AgentRunPhase::PendingTerminal;
            true
        });
    }

    pub(crate) async fn finish(&self, state: &StateRuntime, id: &str) -> Result<()> {
        {
            let mut slots = self.slots();
            let Some(slot) = slots.get_mut(id) else {
                return Ok(());
            };
            if slot.record.child_session_id.is_none() {
                slots.remove(id);
                return Ok(());
            }
            slot.control = None;
            slot.phase = AgentRunPhase::PendingTerminal;
        }
        self.retry_terminal(state, id).await
    }

    pub(crate) async fn retry_terminal(&self, state: &StateRuntime, id: &str) -> Result<()> {
        let terminal_commit = {
            let slots = self.slots();
            let Some(slot) = slots.get(id) else {
                return Ok(());
            };
            Arc::clone(&slot.terminal_commit)
        };
        let _commit = terminal_commit.lock().await;
        let record = {
            let slots = self.slots();
            let Some(slot) = slots.get(id) else {
                return Ok(());
            };
            if slot.phase != AgentRunPhase::PendingTerminal
                || !Arc::ptr_eq(&slot.terminal_commit, &terminal_commit)
            {
                return Ok(());
            }
            slot.record.clone()
        };
        let child_session_id = record
            .child_session_id
            .clone()
            .expect("pending Agent terminal owns a child session");
        let mailbox = if record.background {
            let outcome = record.outcome.as_deref().unwrap_or("failed");
            let summary = record
                .final_answer
                .as_deref()
                .or(record.error.as_deref())
                .unwrap_or_default();
            Some(
                super::mailbox_tools::parent_agent_mailbox_event_input(
                    state,
                    &record.parent_session_id,
                    &record,
                    outcome,
                    summary,
                )
                .await?,
            )
        } else {
            None
        };
        state
            .commit_agent_terminal(&child_session_id, mailbox)
            .await?;
        let mut slots = self.slots();
        if slots.get(id).is_some_and(|slot| {
            slot.phase == AgentRunPhase::PendingTerminal
                && Arc::ptr_eq(&slot.terminal_commit, &terminal_commit)
        }) {
            slots.remove(id);
        }
        Ok(())
    }

    pub(crate) async fn flush_pending_terminals(
        &self,
        state: &StateRuntime,
    ) -> Vec<(String, String)> {
        let ids = self
            .slots()
            .iter()
            .filter(|(_, slot)| slot.phase == AgentRunPhase::PendingTerminal)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let mut failures = Vec::new();
        for id in ids {
            if let Err(error) = self.retry_terminal(state, &id).await {
                failures.push((id, error.to_string()));
            }
        }
        failures
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
    use crate::agents::{AgentEdgeStatus, AgentInvocationRole, AgentRunStatus};
    use psychevo_agent_core::user_text_message;

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

    #[test]
    fn duplicate_registration_never_replaces_the_existing_agent_slot() {
        let supervisor = AgentSupervisor::default();
        supervisor
            .register(record("same", "parent"), None, 4)
            .expect("first registration");
        let mut replacement = record("same", "parent");
        replacement.task = "replacement".to_string();

        let error = supervisor
            .register(replacement, None, 4)
            .expect_err("duplicate Agent id");
        assert!(error.to_string().contains("already supervised"));
        assert_eq!(
            supervisor
                .slots()
                .get("same")
                .expect("original slot")
                .record
                .task,
            "test"
        );
    }

    #[test]
    fn concurrent_continuations_create_one_run_and_inject_the_other_message() {
        let supervisor = AgentSupervisor::default();
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let mut threads = Vec::new();
        for index in 0..2 {
            let supervisor = supervisor.clone();
            let barrier = barrier.clone();
            threads.push(std::thread::spawn(move || {
                let (control, receivers) = ControlHandle::new();
                barrier.wait();
                let result = supervisor.register_continuation_or_inject(
                    record("continuation", "parent"),
                    control,
                    user_text_message(format!("message-{index}")),
                    4,
                );
                (result, receivers)
            }));
        }
        barrier.wait();
        let outcomes = threads
            .into_iter()
            .map(|thread| thread.join().expect("continuation thread"))
            .collect::<Vec<_>>();

        assert_eq!(
            outcomes
                .iter()
                .filter(|(outcome, _)| matches!(outcome, Ok(ContinuationAdmission::Registered)))
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|(outcome, _)| matches!(outcome, Ok(ContinuationAdmission::Injected(_))))
                .count(),
            1
        );
        assert_eq!(supervisor.slots().len(), 1);
    }

    #[tokio::test]
    async fn failed_terminal_commit_retains_the_id_and_retry_commits_once() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = StateRuntime::open(temp.path().join("state.db"))
            .await
            .expect("state");
        let parent = state
            .create_session_with_metadata(temp.path(), "run", "parent-model", "provider", None)
            .await
            .expect("parent");
        let child = state
            .create_child_session_with_metadata(
                &parent,
                temp.path(),
                "agent",
                "child-model",
                "provider",
                None,
            )
            .await
            .expect("child");
        state
            .upsert_agent_edge(&parent, &child, AgentEdgeStatus::Open, None)
            .await
            .expect("durable edge");
        let supervisor = AgentSupervisor::default();
        let mut terminal = record("terminal-retry", &parent);
        terminal.child_session_id = Some(child.clone());
        terminal.status = AgentRunStatus::Completed;
        terminal.edge_status = Some(AgentEdgeStatus::Closed);
        terminal.ended_at_ms = Some(1);
        terminal.outcome = Some("normal".to_string());
        terminal.final_answer = Some("done".to_string());
        supervisor
            .register(terminal, None, 4)
            .expect("terminal slot");
        state.fail_next_agent_terminal_for_test();

        let error = supervisor
            .finish(&state, "terminal-retry")
            .await
            .expect_err("transaction failure must retain the terminal");
        assert!(error.to_string().contains("injected Agent terminal"));
        assert_eq!(
            supervisor
                .slots()
                .get("terminal-retry")
                .expect("retained terminal")
                .phase,
            AgentRunPhase::PendingTerminal
        );

        let (first, second) = tokio::join!(
            supervisor.retry_terminal(&state, "terminal-retry"),
            supervisor.retry_terminal(&state, "terminal-retry"),
        );
        first.expect("first terminal retry");
        second.expect("second terminal retry");

        assert!(!supervisor.slots().contains_key("terminal-retry"));
        let edges = state.list_agent_edges().await.expect("edges");
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].status, AgentEdgeStatus::Closed);
        let mailbox = state
            .load_agent_mailbox_events(&parent)
            .await
            .expect("mailbox");
        assert_eq!(mailbox.len(), 1);
        assert_eq!(mailbox[0].agent_id, "terminal-retry");
    }

    #[tokio::test]
    async fn close_retries_pending_terminal_without_overwriting_its_outcome() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = StateRuntime::open(temp.path().join("state.db"))
            .await
            .expect("state");
        let parent = state
            .create_session_with_metadata(temp.path(), "run", "parent-model", "provider", None)
            .await
            .expect("parent");
        let child = state
            .create_child_session_with_metadata(
                &parent,
                temp.path(),
                "agent",
                "child-model",
                "provider",
                None,
            )
            .await
            .expect("child");
        state
            .upsert_agent_edge(&parent, &child, AgentEdgeStatus::Open, None)
            .await
            .expect("durable edge");
        let supervisor = AgentSupervisor::default();
        let mut terminal = record("terminal-close", &parent);
        terminal.child_session_id = Some(child.clone());
        terminal.status = AgentRunStatus::Completed;
        terminal.edge_status = Some(AgentEdgeStatus::Closed);
        terminal.ended_at_ms = Some(1);
        terminal.outcome = Some("normal".to_string());
        terminal.final_answer = Some("done".to_string());
        supervisor
            .register(terminal, None, 4)
            .expect("terminal slot");
        state.fail_next_agent_terminal_for_test();
        supervisor
            .finish(&state, "terminal-close")
            .await
            .expect_err("first terminal transaction fails");

        let previous = crate::agents::catalog_surface::close_agent_id(
            &supervisor,
            "terminal-close",
            Some(&state),
        )
        .await
        .expect("close retries terminal")
        .expect("previous record");

        assert_eq!(previous.status, AgentRunStatus::Completed);
        assert_eq!(previous.outcome.as_deref(), Some("normal"));
        assert!(!supervisor.slots().contains_key("terminal-close"));
        let mailbox = state
            .load_agent_mailbox_events(&parent)
            .await
            .expect("mailbox");
        assert_eq!(mailbox.len(), 1);
        assert_eq!(
            mailbox[0].metadata.as_ref().expect("metadata")["status"],
            "completed"
        );
    }

    #[tokio::test]
    async fn stale_terminal_retry_cannot_commit_a_reused_agent_id() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = StateRuntime::open(temp.path().join("state.db"))
            .await
            .expect("state");
        let parent = state
            .create_session_with_metadata(temp.path(), "run", "parent-model", "provider", None)
            .await
            .expect("parent");
        let old_child = state
            .create_child_session_with_metadata(
                &parent,
                temp.path(),
                "agent",
                "old-model",
                "provider",
                None,
            )
            .await
            .expect("old child");
        let new_child = state
            .create_child_session_with_metadata(
                &parent,
                temp.path(),
                "agent",
                "new-model",
                "provider",
                None,
            )
            .await
            .expect("new child");
        for child in [&old_child, &new_child] {
            state
                .upsert_agent_edge(&parent, child, AgentEdgeStatus::Open, None)
                .await
                .expect("durable edge");
        }

        let supervisor = AgentSupervisor::default();
        let mut old_terminal = record("reused-id", &parent);
        old_terminal.child_session_id = Some(old_child);
        old_terminal.status = AgentRunStatus::Completed;
        old_terminal.outcome = Some("normal".to_string());
        old_terminal.final_answer = Some("old".to_string());
        supervisor
            .register(old_terminal, None, 4)
            .expect("old slot");
        let old_commit = {
            let mut slots = supervisor.slots();
            let slot = slots.get_mut("reused-id").expect("old slot");
            slot.phase = AgentRunPhase::PendingTerminal;
            Arc::clone(&slot.terminal_commit)
        };
        let old_guard = Arc::clone(&old_commit).lock_owned().await;
        let stale_retry = {
            let supervisor = supervisor.clone();
            let state = state.clone();
            tokio::spawn(async move { supervisor.retry_terminal(&state, "reused-id").await })
        };
        tokio::task::yield_now().await;

        supervisor.remove("reused-id");
        let mut new_terminal = record("reused-id", &parent);
        new_terminal.child_session_id = Some(new_child.clone());
        new_terminal.status = AgentRunStatus::Completed;
        new_terminal.outcome = Some("normal".to_string());
        new_terminal.final_answer = Some("new".to_string());
        supervisor
            .register(new_terminal, None, 4)
            .expect("new slot");
        supervisor
            .slots()
            .get_mut("reused-id")
            .expect("new slot")
            .phase = AgentRunPhase::PendingTerminal;

        drop(old_guard);
        stale_retry
            .await
            .expect("retry task")
            .expect("stale retry is a no-op");

        assert_eq!(
            supervisor
                .slots()
                .get("reused-id")
                .expect("new generation remains")
                .phase,
            AgentRunPhase::PendingTerminal
        );
        assert_eq!(
            state
                .find_agent_edge(&new_child)
                .await
                .expect("edge query")
                .expect("new edge")
                .status,
            AgentEdgeStatus::Open
        );

        supervisor
            .retry_terminal(&state, "reused-id")
            .await
            .expect("current generation retry");
        assert!(!supervisor.slots().contains_key("reused-id"));
    }

    #[tokio::test]
    async fn agent_task_panic_commits_one_failed_terminal_and_is_reported() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = StateRuntime::open(temp.path().join("state.db"))
            .await
            .expect("state");
        let parent = state
            .create_session_with_metadata(temp.path(), "run", "model", "provider", None)
            .await
            .expect("parent");
        let child = state
            .create_child_session_with_metadata(
                &parent,
                temp.path(),
                "agent",
                "model",
                "provider",
                None,
            )
            .await
            .expect("child");
        state
            .upsert_agent_edge(&parent, &child, AgentEdgeStatus::Open, None)
            .await
            .expect("edge");
        let supervisor = AgentSupervisor::default();
        let mut running = record("panic-agent", &parent);
        running.child_session_id = Some(child.clone());
        supervisor.register(running, None, 4).expect("active Agent");

        supervisor.stage_task_panic(
            "panic-agent",
            "Agent task panicked: fixture panic\nCaptured unwind backtrace:\nfixture",
        );
        supervisor
            .finish(&state, "panic-agent")
            .await
            .expect("failed terminal");
        supervisor
            .finish(&state, "panic-agent")
            .await
            .expect("idempotent finish");

        assert_eq!(supervisor.task_panics(), 1);
        assert!(!supervisor.slots().contains_key("panic-agent"));
        let mailbox = state
            .load_agent_mailbox_events(&parent)
            .await
            .expect("mailbox");
        assert_eq!(mailbox.len(), 1);
        assert_eq!(
            mailbox[0].metadata.as_ref().expect("metadata")["status"],
            "errored"
        );
        assert!(
            mailbox[0].content_text.contains("fixture panic"),
            "the durable failed terminal must retain bounded panic evidence"
        );
        assert_eq!(
            state
                .find_agent_edge(&child)
                .await
                .expect("edge query")
                .expect("edge")
                .status,
            AgentEdgeStatus::Closed
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
