use std::fmt;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use futures::FutureExt;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tokio_util::task::task_tracker::TaskTrackerToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GatewayTaskScope {
    Infrastructure,
    Producer,
    Turn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GatewayTaskOutcome {
    Completed,
    Cancelled,
    Panicked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GatewayTaskReport {
    pub(crate) id: u64,
    pub(crate) name: Arc<str>,
    pub(crate) scope: GatewayTaskScope,
    pub(crate) outcome: GatewayTaskOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GatewayAdmissionClosed;

impl fmt::Display for GatewayAdmissionClosed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("gateway is shutting down and no longer accepts new turns")
    }
}

impl std::error::Error for GatewayAdmissionClosed {}

#[derive(Default)]
struct GatewaySupervisorReports {
    tasks: Vec<GatewayTaskReport>,
}

struct GatewaySupervisorInner {
    activity_admission: Mutex<()>,
    accepting_turns: AtomicBool,
    next_task_id: AtomicU64,
    producer_cancel: CancellationToken,
    infrastructure_cancel: CancellationToken,
    turn_cancel: CancellationToken,
    infrastructure: TaskTracker,
    producers: TaskTracker,
    turns: TaskTracker,
    reports: Mutex<GatewaySupervisorReports>,
}

#[derive(Clone)]
pub(crate) struct GatewaySupervisor {
    inner: Arc<GatewaySupervisorInner>,
}

#[derive(Debug)]
pub(crate) struct GatewayActivityPermit {
    _token: TaskTrackerToken,
}

impl Default for GatewaySupervisor {
    fn default() -> Self {
        Self {
            inner: Arc::new(GatewaySupervisorInner {
                activity_admission: Mutex::new(()),
                accepting_turns: AtomicBool::new(true),
                next_task_id: AtomicU64::new(1),
                producer_cancel: CancellationToken::new(),
                infrastructure_cancel: CancellationToken::new(),
                turn_cancel: CancellationToken::new(),
                infrastructure: TaskTracker::new(),
                producers: TaskTracker::new(),
                turns: TaskTracker::new(),
                reports: Mutex::new(GatewaySupervisorReports::default()),
            }),
        }
    }
}

impl fmt::Debug for GatewaySupervisor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewaySupervisor")
            .field("accepting_turns", &self.accepting_turns())
            .field("producer_tasks", &self.inner.producers.len())
            .field("infrastructure_tasks", &self.inner.infrastructure.len())
            .field("turn_tasks", &self.inner.turns.len())
            .finish()
    }
}

impl GatewaySupervisor {
    pub(crate) fn accepting_turns(&self) -> bool {
        self.inner.accepting_turns.load(Ordering::Acquire)
    }

    pub(crate) fn ensure_turn_admission(&self) -> Result<(), GatewayAdmissionClosed> {
        if self.accepting_turns() {
            Ok(())
        } else {
            Err(GatewayAdmissionClosed)
        }
    }

    pub(crate) fn acquire_activity_admission(
        &self,
    ) -> Result<GatewayActivityPermit, GatewayAdmissionClosed> {
        let _admission = self
            .inner
            .activity_admission
            .lock()
            .expect("gateway activity admission lock poisoned");
        self.ensure_turn_admission()?;
        Ok(GatewayActivityPermit {
            _token: self.inner.turns.token(),
        })
    }

    pub(crate) fn close_turn_admission(&self) {
        let _admission = self
            .inner
            .activity_admission
            .lock()
            .expect("gateway activity admission lock poisoned");
        self.inner.accepting_turns.store(false, Ordering::Release);
    }

    pub(crate) fn spawn_producer<F>(&self, name: impl Into<Arc<str>>, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn(
            GatewayTaskScope::Producer,
            name.into(),
            self.inner.producer_cancel.clone(),
            None,
            future,
        );
    }

    pub(crate) fn spawn_turn<F>(&self, name: impl Into<Arc<str>>, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn(
            GatewayTaskScope::Turn,
            name.into(),
            self.inner.turn_cancel.clone(),
            None,
            future,
        );
    }

    pub(crate) fn spawn_permitted_activity<F>(
        &self,
        name: impl Into<Arc<str>>,
        permit: GatewayActivityPermit,
        future: F,
    ) where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn(
            GatewayTaskScope::Turn,
            name.into(),
            self.inner.turn_cancel.clone(),
            Some(permit),
            future,
        );
    }

    pub(crate) fn spawn_infrastructure<F>(&self, name: impl Into<Arc<str>>, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn(
            GatewayTaskScope::Infrastructure,
            name.into(),
            self.inner.infrastructure_cancel.clone(),
            None,
            future,
        );
    }

    fn spawn<F>(
        &self,
        scope: GatewayTaskScope,
        name: Arc<str>,
        cancellation: CancellationToken,
        permit: Option<GatewayActivityPermit>,
        future: F,
    ) where
        F: Future<Output = ()> + Send + 'static,
    {
        let id = self.inner.next_task_id.fetch_add(1, Ordering::Relaxed);
        let supervisor = self.clone();
        // Keep large turn futures behind a heap boundary. `run_routed_turn`
        // contains the complete application workflow and otherwise makes the
        // supervisor's select/catch-unwind wrapper exceed the default Rust test
        // thread stack before its first suspension point.
        let future = Box::pin(future);
        let task = async move {
            let _permit = permit;
            let outcome = tokio::select! {
                _ = cancellation.cancelled() => GatewayTaskOutcome::Cancelled,
                outcome = AssertUnwindSafe(future).catch_unwind() => {
                    match outcome {
                        Ok(()) => GatewayTaskOutcome::Completed,
                        Err(_) => GatewayTaskOutcome::Panicked,
                    }
                }
            };
            supervisor
                .inner
                .reports
                .lock()
                .expect("gateway supervisor report lock poisoned")
                .tasks
                .push(GatewayTaskReport {
                    id,
                    name,
                    scope,
                    outcome,
                });
        };
        match scope {
            GatewayTaskScope::Infrastructure => {
                self.inner.infrastructure.spawn(task);
            }
            GatewayTaskScope::Producer => {
                self.inner.producers.spawn(task);
            }
            GatewayTaskScope::Turn => {
                self.inner.turns.spawn(task);
            }
        }
    }

    pub(crate) fn stop_producers(&self) {
        self.inner.producer_cancel.cancel();
        self.inner.producers.close();
    }

    pub(crate) async fn wait_for_producers(&self) {
        self.inner.producers.wait().await;
    }

    pub(crate) fn close_infrastructure(&self) {
        self.inner.infrastructure.close();
    }

    pub(crate) fn force_cancel_infrastructure(&self) {
        self.inner.infrastructure_cancel.cancel();
        self.close_infrastructure();
    }

    pub(crate) async fn wait_for_infrastructure(&self) {
        self.inner.infrastructure.wait().await;
    }

    pub(crate) fn close_turns(&self) {
        self.close_turn_admission();
        self.inner.turns.close();
    }

    pub(crate) async fn wait_for_turns(&self) {
        self.inner.turns.wait().await;
    }

    pub(crate) fn force_cancel_turns(&self) {
        self.close_turns();
        self.inner.turn_cancel.cancel();
    }

    #[cfg(test)]
    pub(crate) fn reports(&self) -> Vec<GatewayTaskReport> {
        self.inner
            .reports
            .lock()
            .expect("gateway supervisor report lock poisoned")
            .tasks
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use tokio::sync::Notify;

    use super::*;

    #[tokio::test]
    async fn shutdown_stops_producers_but_drains_accepted_turns() {
        let supervisor = GatewaySupervisor::default();
        let producer_cancelled = Arc::new(AtomicBool::new(false));
        let producer_cancelled_for_task = producer_cancelled.clone();
        supervisor.spawn_producer("tailer", async move {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            producer_cancelled_for_task.store(true, Ordering::Release);
        });

        let release_turn = Arc::new(Notify::new());
        let release_turn_for_task = release_turn.clone();
        supervisor.spawn_turn("turn", async move {
            release_turn_for_task.notified().await;
        });

        supervisor.close_turn_admission();
        supervisor.stop_producers();
        supervisor.wait_for_producers().await;
        assert!(!producer_cancelled.load(Ordering::Acquire));
        assert!(supervisor.ensure_turn_admission().is_err());

        supervisor.close_turns();
        release_turn.notify_one();
        supervisor.wait_for_turns().await;

        let reports = supervisor.reports();
        assert!(reports.iter().any(|report| {
            report.name.as_ref() == "tailer"
                && report.scope == GatewayTaskScope::Producer
                && report.outcome == GatewayTaskOutcome::Cancelled
        }));
        assert!(reports.iter().any(|report| {
            report.name.as_ref() == "turn"
                && report.scope == GatewayTaskScope::Turn
                && report.outcome == GatewayTaskOutcome::Completed
        }));
    }

    #[tokio::test]
    async fn force_shutdown_cancels_hung_turn_and_records_panics() {
        let supervisor = GatewaySupervisor::default();
        supervisor.spawn_turn("hung", std::future::pending());
        supervisor.spawn_producer("panic", async move {
            panic!("boom");
        });
        tokio::task::yield_now().await;
        supervisor.stop_producers();
        supervisor.wait_for_producers().await;
        supervisor.force_cancel_turns();
        supervisor.wait_for_turns().await;

        let reports = supervisor.reports();
        assert!(reports.iter().any(|report| {
            report.name.as_ref() == "panic" && report.outcome == GatewayTaskOutcome::Panicked
        }));
        assert!(reports.iter().any(|report| {
            report.name.as_ref() == "hung" && report.outcome == GatewayTaskOutcome::Cancelled
        }));
    }

    #[tokio::test]
    async fn admitted_permit_keeps_shutdown_waiting_until_registration() {
        let supervisor = GatewaySupervisor::default();
        let permit = supervisor
            .acquire_activity_admission()
            .expect("admit before shutdown");

        supervisor.close_turns();
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(20),
                supervisor.wait_for_turns(),
            )
            .await
            .is_err(),
            "the pre-await permit must keep the closed tracker non-empty"
        );
        assert!(supervisor.acquire_activity_admission().is_err());

        supervisor.spawn_permitted_activity("admitted", permit, async {});
        supervisor.wait_for_turns().await;
        assert!(supervisor.reports().iter().any(|report| {
            report.name.as_ref() == "admitted"
                && report.scope == GatewayTaskScope::Turn
                && report.outcome == GatewayTaskOutcome::Completed
        }));
    }
}
