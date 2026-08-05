use std::fmt;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use futures::FutureExt;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tokio_util::task::task_tracker::TaskTrackerToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GatewayTaskScope {
    Infrastructure,
    Producer,
    Activity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GatewayTaskPanic {
    pub(crate) name: Arc<str>,
    pub(crate) scope: GatewayTaskScope,
    pub(crate) message: Arc<str>,
    pub(crate) recovery_backtrace: Arc<str>,
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
        formatter.write_str("gateway is shutting down and no longer accepts new activities")
    }
}

impl std::error::Error for GatewayAdmissionClosed {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GatewayActivityAdmissionError {
    Closed,
    Overloaded { limit: usize, occupancy: usize },
}

pub(crate) enum GatewayTaskOutcome<T> {
    Completed(T),
    Cancelled,
    Panicked(GatewayTaskPanic),
}

impl fmt::Display for GatewayActivityAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => GatewayAdmissionClosed.fmt(formatter),
            Self::Overloaded { limit, .. } => {
                write!(formatter, "Gateway Shell activity limit reached ({limit})")
            }
        }
    }
}

impl std::error::Error for GatewayActivityAdmissionError {}

struct GatewaySupervisorInner {
    activity_admission: Mutex<()>,
    accepting_activities: AtomicBool,
    producer_cancel: CancellationToken,
    infrastructure_cancel: CancellationToken,
    activity_cancel: CancellationToken,
    infrastructure: TaskTracker,
    producers: TaskTracker,
    activities: TaskTracker,
    shell_activities: Arc<AtomicUsize>,
    shell_activity_limit: usize,
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
    shell_activities: Option<Arc<AtomicUsize>>,
}

impl Drop for GatewayActivityPermit {
    fn drop(&mut self) {
        if let Some(shell_activities) = self.shell_activities.take() {
            shell_activities.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

impl Default for GatewaySupervisor {
    fn default() -> Self {
        Self::new(super::DEFAULT_GATEWAY_SHELL_ACTIVITY_LIMIT)
    }
}

impl GatewaySupervisor {
    pub(crate) fn new(shell_activity_limit: usize) -> Self {
        debug_assert!(shell_activity_limit > 0);
        Self {
            inner: Arc::new(GatewaySupervisorInner {
                activity_admission: Mutex::new(()),
                accepting_activities: AtomicBool::new(true),
                producer_cancel: CancellationToken::new(),
                infrastructure_cancel: CancellationToken::new(),
                activity_cancel: CancellationToken::new(),
                infrastructure: TaskTracker::new(),
                producers: TaskTracker::new(),
                activities: TaskTracker::new(),
                shell_activities: Arc::new(AtomicUsize::new(0)),
                shell_activity_limit,
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
            .field("accepting_activities", &self.accepting_activities())
            .field("producer_tasks", &self.inner.producers.len())
            .field("infrastructure_tasks", &self.inner.infrastructure.len())
            .field("activity_tasks", &self.inner.activities.len())
            .finish()
    }
}

impl GatewaySupervisor {
    pub(crate) fn accepting_activities(&self) -> bool {
        self.inner.accepting_activities.load(Ordering::Acquire)
    }

    pub(crate) fn ensure_activity_admission(&self) -> Result<(), GatewayAdmissionClosed> {
        if self.accepting_activities() {
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
        self.ensure_activity_admission()?;
        Ok(GatewayActivityPermit {
            _token: self.inner.activities.token(),
            shell_activities: None,
        })
    }

    pub(crate) fn acquire_shell_activity_admission(
        &self,
    ) -> Result<GatewayActivityPermit, GatewayActivityAdmissionError> {
        let _admission = self
            .inner
            .activity_admission
            .lock()
            .expect("gateway activity admission lock poisoned");
        self.ensure_activity_admission()
            .map_err(|_| GatewayActivityAdmissionError::Closed)?;
        let occupancy = self.inner.shell_activities.load(Ordering::Acquire);
        if occupancy >= self.inner.shell_activity_limit {
            return Err(GatewayActivityAdmissionError::Overloaded {
                limit: self.inner.shell_activity_limit,
                occupancy,
            });
        }
        self.inner.shell_activities.fetch_add(1, Ordering::AcqRel);
        Ok(GatewayActivityPermit {
            _token: self.inner.activities.token(),
            shell_activities: Some(self.inner.shell_activities.clone()),
        })
    }

    pub(crate) fn shell_activity_occupancy(&self) -> usize {
        self.inner.shell_activities.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn infrastructure_task_count(&self) -> usize {
        self.inner.infrastructure.len()
    }

    pub(crate) fn close_activity_admission(&self) {
        let _admission = self
            .inner
            .activity_admission
            .lock()
            .expect("gateway activity admission lock poisoned");
        self.inner
            .accepting_activities
            .store(false, Ordering::Release);
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

    #[cfg(test)]
    pub(crate) fn spawn_activity<F>(&self, name: impl Into<Arc<str>>, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn(
            GatewayTaskScope::Activity,
            name.into(),
            Some(self.inner.activity_cancel.clone()),
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
            GatewayTaskScope::Activity,
            name.into(),
            Some(self.inner.activity_cancel.clone()),
            Some(permit),
            future,
        );
    }

    pub(crate) fn spawn_finalizer_owned_activity<F, T, C, CF>(
        &self,
        name: impl Into<Arc<str>>,
        permit: GatewayActivityPermit,
        future: F,
        complete: C,
    ) where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
        C: FnOnce(GatewayTaskOutcome<T>, GatewayActivityPermit) -> CF + Send + 'static,
        CF: Future<Output = ()> + Send + 'static,
    {
        let cancellation = self.inner.activity_cancel.clone();
        let supervisor = self.clone();
        let name = name.into();
        let future = Box::pin(future);
        self.inner.activities.spawn(async move {
            let outcome = tokio::select! {
                _ = cancellation.cancelled() => GatewayTaskOutcome::Cancelled,
                result = AssertUnwindSafe(future).catch_unwind() => match result {
                    Ok(output) => GatewayTaskOutcome::Completed(output),
                    Err(payload) => {
                        let panic = supervisor.record_panic(
                            name,
                            GatewayTaskScope::Activity,
                            panic_payload_message(payload.as_ref()),
                        );
                        GatewayTaskOutcome::Panicked(panic)
                    }
                },
            };
            complete(outcome, permit).await;
        });
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
        // Keep activity futures behind a heap boundary so the supervisor's
        // select/catch-unwind wrapper remains stack-bounded before suspension.
        let future = Box::pin(future);
        let task = async move {
            let _permit = permit;
            let panic_message = if let Some(cancellation) = cancellation {
                tokio::select! {
                    _ = cancellation.cancelled() => None,
                    outcome = AssertUnwindSafe(future).catch_unwind() => {
                        outcome
                            .err()
                            .map(|payload| panic_payload_message(payload.as_ref()))
                    }
                }
            } else {
                AssertUnwindSafe(future)
                    .catch_unwind()
                    .await
                    .err()
                    .map(|payload| panic_payload_message(payload.as_ref()))
            };
            if let Some(message) = panic_message {
                supervisor.record_panic(name, scope, message);
            }
        };
        match scope {
            GatewayTaskScope::Infrastructure => {
                self.inner.infrastructure.spawn(task);
            }
            GatewayTaskScope::Producer => {
                self.inner.producers.spawn(task);
            }
            GatewayTaskScope::Activity => {
                self.inner.activities.spawn(task);
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

    pub(crate) fn close_activities(&self) {
        self.close_activity_admission();
        self.inner.activities.close();
    }

    pub(crate) async fn wait_for_activities(&self) {
        self.inner.activities.wait().await;
    }

    pub(crate) fn force_cancel_activities(&self) {
        self.close_activities();
        self.inner.activity_cancel.cancel();
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

    fn record_panic(
        &self,
        name: Arc<str>,
        scope: GatewayTaskScope,
        message: Arc<str>,
    ) -> GatewayTaskPanic {
        let panic = GatewayTaskPanic {
            name,
            scope,
            message,
            recovery_backtrace: bounded_recovery_backtrace(),
        };
        if self.inner.panic_count.fetch_add(1, Ordering::Relaxed) == 0 {
            *self
                .inner
                .first_panic
                .lock()
                .expect("gateway supervisor panic lock poisoned") = Some(panic.clone());
        }
        panic
    }
}

fn bounded_recovery_backtrace() -> Arc<str> {
    Arc::from(
        std::backtrace::Backtrace::force_capture()
            .to_string()
            .chars()
            .take(8_192)
            .collect::<String>(),
    )
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> Arc<str> {
    let message = if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    };
    Arc::from(message.chars().take(1_000).collect::<String>())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use tokio::sync::Notify;

    use super::{
        GatewayActivityAdmissionError, GatewayPanicSummary, GatewaySupervisor, GatewayTaskScope,
        panic_payload_message,
    };

    #[test]
    fn panic_payload_context_is_bounded_and_preserves_the_payload_kind() {
        let long_message = "x".repeat(1_200);
        let bounded = panic_payload_message(&long_message);
        assert_eq!(bounded.chars().count(), 1_000);
        assert!(bounded.chars().all(|character| character == 'x'));

        let non_string_payload = 42_u64;
        assert_eq!(
            panic_payload_message(&non_string_payload).as_ref(),
            "non-string panic payload"
        );
    }

    #[tokio::test]
    async fn shutdown_stops_producers_but_drains_accepted_activities() {
        let supervisor = GatewaySupervisor::default();
        let producer_cancelled = Arc::new(AtomicBool::new(false));
        let producer_cancelled_for_task = producer_cancelled.clone();
        supervisor.spawn_producer("tailer", async move {
            std::future::pending::<()>().await;
            producer_cancelled_for_task.store(true, Ordering::Release);
        });

        let release_activity = Arc::new(Notify::new());
        let release_activity_for_task = release_activity.clone();
        let activity_completed = Arc::new(AtomicBool::new(false));
        let activity_completed_for_task = activity_completed.clone();
        supervisor.spawn_activity("activity", async move {
            release_activity_for_task.notified().await;
            activity_completed_for_task.store(true, Ordering::Release);
        });

        supervisor.close_activity_admission();
        supervisor.stop_producers();
        supervisor.wait_for_producers().await;
        assert!(!producer_cancelled.load(Ordering::Acquire));
        assert!(supervisor.ensure_activity_admission().is_err());

        supervisor.close_activities();
        release_activity.notify_one();
        supervisor.wait_for_activities().await;

        assert!(activity_completed.load(Ordering::Acquire));
        assert_eq!(
            supervisor.panic_summary(),
            GatewayPanicSummary {
                count: 0,
                first: None,
            }
        );
    }

    #[tokio::test]
    async fn force_shutdown_cancels_hung_activity_and_records_panics() {
        let supervisor = GatewaySupervisor::default();
        supervisor.spawn_activity("hung", std::future::pending());
        supervisor.spawn_producer("panic", async move {
            panic!("boom");
        });
        tokio::task::yield_now().await;
        supervisor.stop_producers();
        supervisor.wait_for_producers().await;
        supervisor.force_cancel_activities();
        supervisor.wait_for_activities().await;

        let summary = supervisor.panic_summary();
        assert_eq!(summary.count, 1);
        let first = summary.first.expect("first panic evidence");
        assert_eq!(first.name, Arc::from("panic"));
        assert_eq!(first.scope, GatewayTaskScope::Producer);
        assert_eq!(first.message, Arc::from("boom"));
        assert!(!first.recovery_backtrace.is_empty());
        assert!(first.recovery_backtrace.len() <= 8_192);
    }

    #[tokio::test]
    async fn admitted_permit_keeps_shutdown_waiting_until_registration() {
        let supervisor = GatewaySupervisor::default();
        let permit = supervisor
            .acquire_activity_admission()
            .expect("admit before shutdown");

        supervisor.close_activities();
        let mut waiting = Box::pin(supervisor.wait_for_activities());
        assert!(
            futures::poll!(&mut waiting).is_pending(),
            "the pre-await permit must keep the closed tracker non-empty"
        );
        assert!(supervisor.acquire_activity_admission().is_err());

        supervisor.spawn_permitted_activity("admitted", permit, async {});
        waiting.await;
        assert_eq!(supervisor.panic_summary().count, 0);
    }

    #[test]
    fn shell_activity_admission_is_bounded_and_released_with_the_permit() {
        let supervisor = GatewaySupervisor::default();
        let permits = (0..64)
            .map(|_| {
                supervisor
                    .acquire_shell_activity_admission()
                    .expect("activity within capacity")
            })
            .collect::<Vec<_>>();

        assert_eq!(
            supervisor
                .acquire_shell_activity_admission()
                .expect_err("activity over capacity"),
            GatewayActivityAdmissionError::Overloaded {
                limit: 64,
                occupancy: 64,
            }
        );
        drop(permits);
        assert!(supervisor.acquire_shell_activity_admission().is_ok());
    }

    #[test]
    fn shell_activity_admission_uses_the_configured_limit() {
        let supervisor = GatewaySupervisor::new(1);
        let permit = supervisor
            .acquire_shell_activity_admission()
            .expect("first permit");
        assert!(matches!(
            supervisor.acquire_shell_activity_admission(),
            Err(GatewayActivityAdmissionError::Overloaded {
                limit: 1,
                occupancy: 1
            })
        ));
        drop(permit);
        assert!(supervisor.acquire_shell_activity_admission().is_ok());
    }
}
