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
pub(crate) struct GatewayTaskPanic {
    pub(crate) name: Arc<str>,
    pub(crate) scope: GatewayTaskScope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GatewayPanicSummary {
    pub(crate) count: u64,
    pub(crate) first: Option<GatewayTaskPanic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GatewayAdmissionClosed;

impl fmt::Display for GatewayAdmissionClosed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("gateway is shutting down and no longer accepts new turns")
    }
}

impl std::error::Error for GatewayAdmissionClosed {}

struct GatewaySupervisorInner {
    activity_admission: Mutex<()>,
    accepting_turns: AtomicBool,
    producer_cancel: CancellationToken,
    infrastructure_cancel: CancellationToken,
    turn_cancel: CancellationToken,
    infrastructure: TaskTracker,
    producers: TaskTracker,
    turns: TaskTracker,
    panic_count: AtomicU64,
    first_panic: Mutex<Option<GatewayTaskPanic>>,
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
                producer_cancel: CancellationToken::new(),
                infrastructure_cancel: CancellationToken::new(),
                turn_cancel: CancellationToken::new(),
                infrastructure: TaskTracker::new(),
                producers: TaskTracker::new(),
                turns: TaskTracker::new(),
                panic_count: AtomicU64::new(0),
                first_panic: Mutex::new(None),
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
            Some(self.inner.producer_cancel.clone()),
            None,
            future,
        );
    }

    pub(crate) fn spawn_shutdown_aware_producer<B, F>(&self, name: impl Into<Arc<str>>, build: B)
    where
        B: FnOnce(CancellationToken) -> F,
        F: Future<Output = ()> + Send + 'static,
    {
        let shutdown = self.inner.producer_cancel.clone();
        self.spawn(
            GatewayTaskScope::Producer,
            name.into(),
            None,
            None,
            build(shutdown),
        );
    }

    pub(crate) fn spawn_turn<F>(&self, name: impl Into<Arc<str>>, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn(
            GatewayTaskScope::Turn,
            name.into(),
            Some(self.inner.turn_cancel.clone()),
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
            Some(self.inner.turn_cancel.clone()),
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
            Some(self.inner.infrastructure_cancel.clone()),
            None,
            future,
        );
    }

    fn spawn<F>(
        &self,
        scope: GatewayTaskScope,
        name: Arc<str>,
        cancellation: Option<CancellationToken>,
        permit: Option<GatewayActivityPermit>,
        future: F,
    ) where
        F: Future<Output = ()> + Send + 'static,
    {
        let supervisor = self.clone();
        // Keep large turn futures behind a heap boundary. `run_routed_turn`
        // contains the complete application workflow and otherwise makes the
        // supervisor's select/catch-unwind wrapper exceed the default Rust test
        // thread stack before its first suspension point.
        let future = Box::pin(future);
        let task = async move {
            let _permit = permit;
            let panicked = if let Some(cancellation) = cancellation {
                tokio::select! {
                    _ = cancellation.cancelled() => false,
                    outcome = AssertUnwindSafe(future).catch_unwind() => {
                        outcome.is_err()
                    }
                }
            } else {
                AssertUnwindSafe(future).catch_unwind().await.is_err()
            };
            if panicked {
                supervisor.record_panic(name, scope);
            }
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

    pub(crate) fn panic_summary(&self) -> GatewayPanicSummary {
        GatewayPanicSummary {
            count: self.inner.panic_count.load(Ordering::Relaxed),
            first: self
                .inner
                .first_panic
                .lock()
                .expect("gateway supervisor panic lock poisoned")
                .clone(),
        }
    }

    fn record_panic(&self, name: Arc<str>, scope: GatewayTaskScope) {
        if self.inner.panic_count.fetch_add(1, Ordering::Relaxed) == 0 {
            *self
                .inner
                .first_panic
                .lock()
                .expect("gateway supervisor panic lock poisoned") =
                Some(GatewayTaskPanic { name, scope });
        }
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
        let turn_completed = Arc::new(AtomicBool::new(false));
        let turn_completed_for_task = turn_completed.clone();
        supervisor.spawn_turn("turn", async move {
            release_turn_for_task.notified().await;
            turn_completed_for_task.store(true, Ordering::Release);
        });

        supervisor.close_turn_admission();
        supervisor.stop_producers();
        supervisor.wait_for_producers().await;
        assert!(!producer_cancelled.load(Ordering::Acquire));
        assert!(supervisor.ensure_turn_admission().is_err());

        supervisor.close_turns();
        release_turn.notify_one();
        supervisor.wait_for_turns().await;

        assert!(turn_completed.load(Ordering::Acquire));
        assert_eq!(
            supervisor.panic_summary(),
            GatewayPanicSummary {
                count: 0,
                first: None,
            }
        );
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

        assert_eq!(
            supervisor.panic_summary(),
            GatewayPanicSummary {
                count: 1,
                first: Some(GatewayTaskPanic {
                    name: Arc::from("panic"),
                    scope: GatewayTaskScope::Producer,
                }),
            }
        );
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
        assert_eq!(supervisor.panic_summary().count, 0);
    }
}
