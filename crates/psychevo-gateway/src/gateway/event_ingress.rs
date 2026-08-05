use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sha2::{Digest, Sha256};
#[cfg(test)]
use tokio::sync::Notify;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use super::Gateway;
use super::durable_activity::{
    DurableGatewayActivity, GatewayEventCommit, GatewayEventPersistence,
    GatewayEventPersistenceError,
};
use super::live_projection::gateway_event_turn_id;
use super::supervisor::GatewaySupervisor;
use crate::{GatewayEventEmitError, GatewayEventIngressOldest, GatewayEventIngressOverload};
use psychevo_gateway_protocol::events_transcript::GatewayEvent;

const GATEWAY_LIVE_SNAPSHOT_FLUSH_INTERVAL: Duration = Duration::from_millis(250);
const GATEWAY_EVENT_PERSIST_ATTEMPTS: usize = 3;
const MAX_DIAGNOSTIC_TEXT_CHARS: usize = 1_000;
const MAX_EVIDENCE_ID_CHARS: usize = 256;

#[derive(Clone)]
pub(crate) struct GatewayEventIngress {
    inner: Arc<GatewayEventIngressInner>,
}

struct GatewayEventIngressInner {
    accepted: AtomicU64,
    closed: AtomicBool,
    committed: AtomicU64,
    #[cfg(test)]
    commit_latencies_micros: Mutex<Vec<u64>>,
    #[cfg(test)]
    committed_wakeup: Notify,
    failed: AtomicU64,
    first_failure: Mutex<Option<Arc<str>>>,
    last_commit: Mutex<Option<GatewayEventCommitEvidence>>,
    limit: usize,
    next_sequence: AtomicU64,
    pending: Mutex<VecDeque<PendingGatewayEvent>>,
    #[cfg(test)]
    pause_after_processed: AtomicU64,
    #[cfg(test)]
    peak_occupancy: AtomicUsize,
    rejected: AtomicU64,
    retried: AtomicU64,
    processed: AtomicU64,
    sender: Mutex<Option<mpsc::Sender<GatewayEventIngressItem>>>,
    supervisor: GatewaySupervisor,
    #[cfg(test)]
    processed_wakeup: Notify,
    #[cfg(test)]
    retry_after_commit: AtomicU64,
    #[cfg(test)]
    worker_paused: AtomicBool,
    #[cfg(test)]
    worker_wakeup: Notify,
}

struct PendingGatewayEvent {
    sequence: u64,
    accepted_at: Instant,
    activity_id: String,
    turn_id: Option<String>,
    event_kind: &'static str,
}

pub(crate) struct GatewayEventEnvelope {
    pub(crate) activity: DurableGatewayActivity,
    pub(crate) default_turn_id: Option<String>,
    pub(crate) event: GatewayEvent,
    pub(crate) queue_key: Option<String>,
    pub(crate) root_activity_id: Option<String>,
}

enum GatewayEventIngressItem {
    Event {
        sequence: u64,
        idempotency_key: String,
        envelope: Box<GatewayEventEnvelope>,
    },
    Fence(oneshot::Sender<Result<(), GatewayEventEmitError>>),
}

struct PendingSnapshotEnvelope {
    sequence: u64,
    idempotency_key: String,
    snapshot_key: String,
}

struct GatewayEventIngressWorker {
    gateway: Gateway,
    ingress: GatewayEventIngress,
    receiver: mpsc::Receiver<GatewayEventIngressItem>,
    snapshot_deadline: Option<Instant>,
    pending_snapshots: Vec<PendingSnapshotEnvelope>,
    pending_failure: Option<Arc<str>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GatewayEventCommitEvidence {
    LiveEvent {
        idempotency_key: Arc<str>,
        seq: i64,
    },
    LiveSnapshot {
        idempotency_key: Arc<str>,
        snapshot_key: Arc<str>,
        fingerprint: Arc<str>,
        revision: i64,
    },
    FrameworkTerminal {
        idempotency_key: Arc<str>,
        turn_id: Arc<str>,
        thread_id: Arc<str>,
        status: Arc<str>,
        completed_at_ms: i64,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GatewayEventIngressDiagnostics {
    pub accepted: u64,
    pub committed: u64,
    pub failed: u64,
    pub first_failure: Option<Arc<str>>,
    pub last_commit: Option<GatewayEventCommitEvidence>,
    pub occupancy: usize,
    pub limit: usize,
    pub oldest: Option<GatewayEventIngressOldest>,
    pub rejected: u64,
    pub retried: u64,
    pub processed: u64,
}

impl GatewayEventIngress {
    pub(crate) fn new(supervisor: GatewaySupervisor, limit: usize) -> Self {
        debug_assert!(limit > 0, "Gateway owner validates ingress capacity");
        Self {
            inner: Arc::new(GatewayEventIngressInner {
                accepted: AtomicU64::new(0),
                closed: AtomicBool::new(false),
                committed: AtomicU64::new(0),
                #[cfg(test)]
                commit_latencies_micros: Mutex::new(Vec::new()),
                #[cfg(test)]
                committed_wakeup: Notify::new(),
                failed: AtomicU64::new(0),
                first_failure: Mutex::new(None),
                last_commit: Mutex::new(None),
                limit,
                next_sequence: AtomicU64::new(1),
                pending: Mutex::new(VecDeque::new()),
                #[cfg(test)]
                pause_after_processed: AtomicU64::new(0),
                #[cfg(test)]
                peak_occupancy: AtomicUsize::new(0),
                rejected: AtomicU64::new(0),
                retried: AtomicU64::new(0),
                processed: AtomicU64::new(0),
                sender: Mutex::new(None),
                supervisor,
                #[cfg(test)]
                processed_wakeup: Notify::new(),
                #[cfg(test)]
                retry_after_commit: AtomicU64::new(0),
                #[cfg(test)]
                worker_paused: AtomicBool::new(false),
                #[cfg(test)]
                worker_wakeup: Notify::new(),
            }),
        }
    }

    pub(crate) fn submit(
        &self,
        gateway: Gateway,
        envelope: GatewayEventEnvelope,
    ) -> Result<(), GatewayEventEmitError> {
        let sender = match self.sender(gateway) {
            Ok(sender) => sender,
            Err(error) => {
                self.inner.rejected.fetch_add(1, Ordering::Relaxed);
                return Err(error);
            }
        };
        let sequence = self.inner.next_sequence.fetch_add(1, Ordering::Relaxed);
        let idempotency_key = ingress_idempotency_key(sequence, &envelope);
        let pending = pending_gateway_event(sequence, &envelope);
        let mut queue = self
            .inner
            .pending
            .lock()
            .expect("gateway event ingress pending queue poisoned");
        if queue.len() == self.inner.limit {
            let overload = overload_from_pending(self.inner.limit, &queue);
            drop(queue);
            self.inner.rejected.fetch_add(1, Ordering::Relaxed);
            return Err(GatewayEventEmitError::overloaded(overload));
        }
        queue.push_back(pending);
        match sender.try_send(GatewayEventIngressItem::Event {
            sequence,
            idempotency_key,
            envelope: Box::new(envelope),
        }) {
            Ok(()) => {
                #[cfg(test)]
                self.inner
                    .peak_occupancy
                    .fetch_max(queue.len(), Ordering::Relaxed);
                self.inner.accepted.fetch_add(1, Ordering::Relaxed);
                drop(queue);
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                let removed = queue.pop_back();
                debug_assert_eq!(removed.map(|item| item.sequence), Some(sequence));
                drop(queue);
                self.inner.rejected.fetch_add(1, Ordering::Relaxed);
                Err(GatewayEventEmitError::new(
                    "Gateway event durability ingress queue is temporarily unavailable.",
                ))
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                let removed = queue.pop_back();
                debug_assert_eq!(removed.map(|item| item.sequence), Some(sequence));
                drop(queue);
                self.inner.rejected.fetch_add(1, Ordering::Relaxed);
                Err(GatewayEventEmitError::new(
                    "Gateway event durability ingress is closed.",
                ))
            }
        }
    }

    pub(crate) fn close(&self) {
        self.inner.closed.store(true, Ordering::Release);
        self.inner
            .sender
            .lock()
            .expect("gateway event ingress sender poisoned")
            .take();
    }

    pub(crate) fn diagnostics(&self) -> GatewayEventIngressDiagnostics {
        let (occupancy, oldest) = self.pending_snapshot();
        GatewayEventIngressDiagnostics {
            accepted: self.inner.accepted.load(Ordering::Relaxed),
            committed: self.inner.committed.load(Ordering::Relaxed),
            failed: self.inner.failed.load(Ordering::Relaxed),
            first_failure: self
                .inner
                .first_failure
                .lock()
                .expect("gateway event ingress failure lock poisoned")
                .clone(),
            last_commit: self
                .inner
                .last_commit
                .lock()
                .expect("gateway event ingress commit evidence lock poisoned")
                .clone(),
            occupancy,
            limit: self.inner.limit,
            oldest,
            rejected: self.inner.rejected.load(Ordering::Relaxed),
            retried: self.inner.retried.load(Ordering::Relaxed),
            processed: self.inner.processed.load(Ordering::Relaxed),
        }
    }

    pub(crate) async fn fence(&self) -> Result<(), GatewayEventEmitError> {
        let sender = {
            if self.inner.closed.load(Ordering::Acquire) {
                return Err(GatewayEventEmitError::new(
                    "Gateway event durability ingress is closed before its completion fence.",
                ));
            }
            self.inner
                .sender
                .lock()
                .expect("gateway event ingress sender poisoned")
                .clone()
        };
        let Some(sender) = sender else {
            return Ok(());
        };
        let (complete, completed) = oneshot::channel();
        sender
            .send(GatewayEventIngressItem::Fence(complete))
            .await
            .map_err(|_| {
                GatewayEventEmitError::new(
                    "Gateway event durability ingress closed before accepting its completion fence.",
                )
            })?;
        completed.await.map_err(|_| {
            GatewayEventEmitError::new(
                "Gateway event durability ingress stopped before completing its fence.",
            )
        })?
    }

    fn sender(
        &self,
        gateway: Gateway,
    ) -> Result<mpsc::Sender<GatewayEventIngressItem>, GatewayEventEmitError> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(GatewayEventEmitError::new(
                "Gateway event durability ingress is closed.",
            ));
        }
        let mut slot = self
            .inner
            .sender
            .lock()
            .expect("gateway event ingress sender poisoned");
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(GatewayEventEmitError::new(
                "Gateway event durability ingress is closed.",
            ));
        }
        if let Some(sender) = slot.as_ref() {
            return Ok(sender.clone());
        }
        tokio::runtime::Handle::try_current().map_err(|_| {
            GatewayEventEmitError::new(
                "Gateway event durability ingress requires an active Tokio runtime.",
            )
        })?;
        let (sender, receiver) = mpsc::channel::<GatewayEventIngressItem>(self.inner.limit);
        let worker = GatewayEventIngressWorker {
            gateway,
            ingress: self.clone(),
            receiver,
            snapshot_deadline: None,
            pending_snapshots: Vec::new(),
            pending_failure: None,
        };
        self.inner
            .supervisor
            .spawn_infrastructure("gateway-event-ingress", worker.run());
        *slot = Some(sender.clone());
        Ok(sender)
    }

    fn record_failure(&self, error: &str) {
        if self.inner.failed.fetch_add(1, Ordering::Relaxed) == 0 {
            *self
                .inner
                .first_failure
                .lock()
                .expect("gateway event ingress failure lock poisoned") =
                Some(bounded_arc(error, MAX_DIAGNOSTIC_TEXT_CHARS));
        }
    }

    fn record_commit(&self, sequence: u64, evidence: GatewayEventCommitEvidence) {
        #[cfg(test)]
        {
            let latency_micros = self
                .inner
                .pending
                .lock()
                .expect("gateway event ingress pending queue poisoned")
                .iter()
                .find(|pending| pending.sequence == sequence)
                .map(|pending| {
                    u64::try_from(pending.accepted_at.elapsed().as_micros()).unwrap_or(u64::MAX)
                })
                .expect("committed ingress item has pending admission metadata");
            self.inner
                .commit_latencies_micros
                .lock()
                .expect("gateway event ingress latency samples poisoned")
                .push(latency_micros);
        }
        *self
            .inner
            .last_commit
            .lock()
            .expect("gateway event ingress commit evidence lock poisoned") = Some(evidence);
        self.release(sequence);
        self.inner.committed.fetch_add(1, Ordering::Release);
        #[cfg(test)]
        self.inner.committed_wakeup.notify_one();
    }

    fn failure_context(&self, sequence: u64, error: &GatewayEventPersistenceError) -> Arc<str> {
        let pending = self
            .inner
            .pending
            .lock()
            .expect("gateway event ingress pending queue poisoned");
        let Some(pending) = pending.iter().find(|pending| pending.sequence == sequence) else {
            return bounded_arc(&error.to_string(), MAX_DIAGNOSTIC_TEXT_CHARS);
        };
        bounded_arc(
            &format!(
                "activity_id={} turn_id={} event_kind={}: {error}",
                pending.activity_id,
                pending.turn_id.as_deref().unwrap_or("none"),
                pending.event_kind,
            ),
            MAX_DIAGNOSTIC_TEXT_CHARS,
        )
    }

    fn release(&self, sequence: u64) {
        let removed = {
            let mut pending = self
                .inner
                .pending
                .lock()
                .expect("gateway event ingress pending queue poisoned");
            pending
                .iter()
                .position(|item| item.sequence == sequence)
                .and_then(|position| pending.remove(position))
                .is_some()
        };
        debug_assert!(
            removed,
            "accepted ingress item has pending admission metadata"
        );
    }

    fn pending_snapshot(&self) -> (usize, Option<GatewayEventIngressOldest>) {
        let pending = self
            .inner
            .pending
            .lock()
            .expect("gateway event ingress pending queue poisoned");
        let oldest = pending.front().map(|pending| GatewayEventIngressOldest {
            age_ms: u64::try_from(pending.accepted_at.elapsed().as_millis()).unwrap_or(u64::MAX),
            activity_id: pending.activity_id.clone(),
            turn_id: pending.turn_id.clone(),
            event_kind: pending.event_kind.to_string(),
        });
        (pending.len(), oldest)
    }

    #[cfg(test)]
    pub(crate) async fn wait_until_committed(&self, expected: u64) {
        while self.inner.committed.load(Ordering::Acquire) < expected {
            self.inner.committed_wakeup.notified().await;
        }
    }

    #[cfg(test)]
    pub(crate) async fn wait_until_processed(&self, expected: u64) {
        while self.inner.processed.load(Ordering::Acquire) < expected {
            self.inner.processed_wakeup.notified().await;
        }
    }

    #[cfg(test)]
    pub(crate) fn commit_latency_samples_micros(&self) -> Vec<u64> {
        self.inner
            .commit_latencies_micros
            .lock()
            .expect("gateway event ingress latency samples poisoned")
            .clone()
    }

    #[cfg(test)]
    pub(crate) fn peak_occupancy(&self) -> usize {
        self.inner.peak_occupancy.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn retry_after_next_commit(&self) {
        self.inner
            .retry_after_commit
            .fetch_add(1, Ordering::Release);
    }

    fn after_commit_test_fault<T>(
        &self,
        result: Result<T, GatewayEventPersistenceError>,
    ) -> Result<T, GatewayEventPersistenceError> {
        #[cfg(test)]
        if result.is_ok()
            && self
                .inner
                .retry_after_commit
                .try_update(Ordering::AcqRel, Ordering::Acquire, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
        {
            return Err(GatewayEventPersistenceError::retryable_test(
                "injected lost durable commit acknowledgement",
            ));
        }
        result
    }

    #[cfg(test)]
    pub(crate) fn pause_worker(&self) {
        self.inner.worker_paused.store(true, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn pause_worker_after_processed(&self, expected: u64) {
        assert!(expected > 0, "processed pause boundary must be positive");
        self.inner
            .pause_after_processed
            .store(expected, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn resume_worker(&self) {
        self.inner.worker_paused.store(false, Ordering::Release);
        self.inner.worker_wakeup.notify_waiters();
    }
}

impl GatewayEventIngressWorker {
    async fn run(mut self) {
        loop {
            let item = if let Some(deadline) = self.snapshot_deadline {
                tokio::select! {
                    item = self.receiver.recv() => item,
                    _ = tokio::time::sleep_until(deadline) => {
                        self.flush_pending_snapshots().await;
                        continue;
                    }
                }
            } else {
                self.receiver.recv().await
            };
            let Some(item) = item else {
                self.flush_pending_snapshots().await;
                break;
            };
            match item {
                GatewayEventIngressItem::Event {
                    sequence,
                    idempotency_key,
                    envelope,
                } => {
                    #[cfg(test)]
                    while self.ingress.inner.worker_paused.load(Ordering::Acquire) {
                        self.ingress.inner.worker_wakeup.notified().await;
                    }
                    self.process_event(sequence, idempotency_key, *envelope)
                        .await;
                }
                GatewayEventIngressItem::Fence(complete) => {
                    self.flush_pending_snapshots().await;
                    let result = self.pending_failure.as_deref().map_or(Ok(()), |failure| {
                        Err(GatewayEventEmitError::new(format!(
                            "Gateway event durability ingress failed before its completion fence: {failure}"
                        )))
                    });
                    if complete.send(result).is_ok() {
                        self.pending_failure = None;
                    }
                }
            }
        }
    }

    async fn process_event(
        &mut self,
        sequence: u64,
        idempotency_key: String,
        envelope: GatewayEventEnvelope,
    ) {
        if !gateway_event_is_coalescible(&envelope.event) {
            self.flush_pending_snapshots().await;
        }
        let result = self
            .persist_event_with_retry(&envelope, &idempotency_key)
            .await;
        match result {
            Ok(GatewayEventPersistence::Committed(commit)) => {
                self.ingress
                    .record_commit(sequence, commit_evidence(&idempotency_key, commit));
            }
            Ok(GatewayEventPersistence::PendingSnapshot {
                snapshot_key,
                coalescible,
            }) => {
                self.pending_snapshots.push(PendingSnapshotEnvelope {
                    sequence,
                    idempotency_key,
                    snapshot_key,
                });
                if coalescible {
                    self.snapshot_deadline.get_or_insert_with(|| {
                        Instant::now() + GATEWAY_LIVE_SNAPSHOT_FLUSH_INTERVAL
                    });
                } else {
                    self.flush_pending_snapshots().await;
                }
            }
            Err(error) => self.fail_envelope(sequence, &error),
        }
        #[cfg(test)]
        {
            let next_processed = self.ingress.inner.processed.load(Ordering::Relaxed) + 1;
            if self
                .ingress
                .inner
                .pause_after_processed
                .compare_exchange(next_processed, 0, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                self.ingress
                    .inner
                    .worker_paused
                    .store(true, Ordering::Release);
            }
        }
        self.ingress.inner.processed.fetch_add(1, Ordering::Release);
        #[cfg(test)]
        {
            self.ingress.inner.processed_wakeup.notify_one();
            while self.ingress.inner.worker_paused.load(Ordering::Acquire) {
                self.ingress.inner.worker_wakeup.notified().await;
            }
        }
    }

    async fn persist_event_with_retry(
        &self,
        envelope: &GatewayEventEnvelope,
        idempotency_key: &str,
    ) -> Result<GatewayEventPersistence, GatewayEventPersistenceError> {
        let mut attempt = 1;
        loop {
            let result = self
                .gateway
                .persist_gateway_event_envelope(envelope, idempotency_key)
                .await;
            let result = match result {
                Ok(persistence @ GatewayEventPersistence::Committed(_)) => {
                    self.ingress.after_commit_test_fault(Ok(persistence))
                }
                result => result,
            };
            match result {
                Err(error) if error.is_retryable() && attempt < GATEWAY_EVENT_PERSIST_ATTEMPTS => {
                    attempt += 1;
                    self.ingress.inner.retried.fetch_add(1, Ordering::Relaxed);
                    tokio::task::yield_now().await;
                }
                result => return result,
            }
        }
    }

    async fn flush_pending_snapshots(&mut self) {
        self.snapshot_deadline = None;
        if self.pending_snapshots.is_empty() {
            return;
        }
        let snapshot_keys = self.pending_snapshot_keys();
        let envelope_count = self.pending_snapshots.len() as u64;
        let mut attempt = 1;
        let commits = loop {
            let result = self.ingress.after_commit_test_fault(
                self.gateway
                    .flush_gateway_live_snapshots(&snapshot_keys)
                    .await,
            );
            match result {
                Err(error) if error.is_retryable() && attempt < GATEWAY_EVENT_PERSIST_ATTEMPTS => {
                    attempt += 1;
                    self.ingress
                        .inner
                        .retried
                        .fetch_add(envelope_count, Ordering::Relaxed);
                    tokio::task::yield_now().await;
                }
                Ok(commits) => break commits,
                Err(error) => {
                    self.fail_snapshot_batch(&snapshot_keys, &error);
                    return;
                }
            }
        };
        let commits = commits
            .into_iter()
            .map(|commit| (commit.snapshot_key.clone(), commit))
            .collect::<HashMap<_, _>>();
        if let Some(snapshot_key) = snapshot_keys
            .iter()
            .find(|snapshot_key| !commits.contains_key(snapshot_key.as_str()))
        {
            let error = GatewayEventPersistenceError::Permanent(psychevo::Error::Message(format!(
                "Gateway retained-live flush returned no commit evidence for snapshot `{snapshot_key}`"
            )));
            self.fail_snapshot_batch(&snapshot_keys, &error);
            return;
        }
        self.gateway
            .finish_gateway_live_snapshot_updates(snapshot_keys.iter().map(String::as_str));
        for pending in std::mem::take(&mut self.pending_snapshots) {
            let commit = commits
                .get(&pending.snapshot_key)
                .expect("all pending snapshots have durable commit evidence");
            self.ingress.record_commit(
                pending.sequence,
                GatewayEventCommitEvidence::LiveSnapshot {
                    idempotency_key: bounded_arc(&pending.idempotency_key, MAX_EVIDENCE_ID_CHARS),
                    snapshot_key: bounded_arc(&pending.snapshot_key, MAX_EVIDENCE_ID_CHARS),
                    fingerprint: bounded_arc(&commit.fingerprint, MAX_EVIDENCE_ID_CHARS),
                    revision: commit.revision,
                },
            );
        }
    }

    fn pending_snapshot_keys(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        self.pending_snapshots
            .iter()
            .filter(|pending| seen.insert(pending.snapshot_key.as_str()))
            .map(|pending| pending.snapshot_key.clone())
            .collect()
    }

    fn fail_snapshot_batch(
        &mut self,
        snapshot_keys: &[String],
        error: &GatewayEventPersistenceError,
    ) {
        self.gateway
            .finish_gateway_live_snapshot_updates(snapshot_keys.iter().map(String::as_str));
        for pending in std::mem::take(&mut self.pending_snapshots) {
            self.fail_envelope(pending.sequence, error);
        }
    }

    fn fail_envelope(&mut self, sequence: u64, error: &GatewayEventPersistenceError) {
        let failure = self.ingress.failure_context(sequence, error);
        self.ingress.record_failure(&failure);
        self.pending_failure.get_or_insert(failure);
        self.ingress.release(sequence);
    }
}

fn commit_evidence(
    idempotency_key: &str,
    commit: GatewayEventCommit,
) -> GatewayEventCommitEvidence {
    let idempotency_key = bounded_arc(idempotency_key, MAX_EVIDENCE_ID_CHARS);
    match commit {
        GatewayEventCommit::LiveEvent { seq } => GatewayEventCommitEvidence::LiveEvent {
            idempotency_key,
            seq,
        },
        GatewayEventCommit::FrameworkTerminal {
            turn_id,
            thread_id,
            status,
            completed_at_ms,
        } => GatewayEventCommitEvidence::FrameworkTerminal {
            idempotency_key,
            turn_id: bounded_arc(&turn_id, MAX_EVIDENCE_ID_CHARS),
            thread_id: bounded_arc(&thread_id, MAX_EVIDENCE_ID_CHARS),
            status: bounded_arc(&status, MAX_EVIDENCE_ID_CHARS),
            completed_at_ms,
        },
    }
}

fn ingress_idempotency_key(sequence: u64, envelope: &GatewayEventEnvelope) -> String {
    let mut identity = Sha256::new();
    identity.update(envelope.activity.owner_id.as_bytes());
    identity.update([0]);
    identity.update(envelope.activity.activity_id.as_bytes());
    identity.update([0]);
    identity.update(envelope.activity.generation.to_le_bytes());
    format!(
        "gateway-ingress:v1:{:x}:{}:{sequence}",
        identity.finalize(),
        envelope.activity.generation,
    )
}

fn bounded_arc(value: &str, limit: usize) -> Arc<str> {
    Arc::from(value.chars().take(limit).collect::<String>())
}

fn overload_from_pending(
    limit: usize,
    pending: &VecDeque<PendingGatewayEvent>,
) -> GatewayEventIngressOverload {
    let oldest = pending.front().map(|pending| GatewayEventIngressOldest {
        age_ms: u64::try_from(pending.accepted_at.elapsed().as_millis()).unwrap_or(u64::MAX),
        activity_id: pending.activity_id.clone(),
        turn_id: pending.turn_id.clone(),
        event_kind: pending.event_kind.to_string(),
    });
    GatewayEventIngressOverload {
        occupancy: pending.len(),
        limit,
        retryable: true,
        oldest,
    }
}

fn pending_gateway_event(sequence: u64, envelope: &GatewayEventEnvelope) -> PendingGatewayEvent {
    PendingGatewayEvent {
        sequence,
        accepted_at: Instant::now(),
        activity_id: envelope.activity.activity_id.clone(),
        turn_id: gateway_event_turn_id(&envelope.event)
            .map(str::to_string)
            .or_else(|| envelope.default_turn_id.clone()),
        event_kind: gateway_event_kind(&envelope.event),
    }
}

fn gateway_event_is_coalescible(event: &GatewayEvent) -> bool {
    matches!(
        event,
        GatewayEvent::EntryUpdated { .. } | GatewayEvent::EntryBlockTextDelta { .. }
    )
}

fn gateway_event_kind(event: &GatewayEvent) -> &'static str {
    match event {
        GatewayEvent::TurnStarted { .. } => "turnStarted",
        GatewayEvent::TurnQueued { .. } => "turnQueued",
        GatewayEvent::TurnCompleted { .. } => "turnCompleted",
        GatewayEvent::EntryStarted { .. } => "entryStarted",
        GatewayEvent::EntryUpdated { .. } => "entryUpdated",
        GatewayEvent::EntryBlockTextDelta { .. } => "entryBlockTextDelta",
        GatewayEvent::EntryCompleted { .. } => "entryCompleted",
        GatewayEvent::ActionRequested { .. } => "actionRequested",
        GatewayEvent::ActionUpdated { .. } => "actionUpdated",
        GatewayEvent::ActionResolved { .. } => "actionResolved",
        GatewayEvent::ActionCancelled { .. } => "actionCancelled",
        GatewayEvent::Warning { .. } => "warning",
        GatewayEvent::ActivityChanged { .. } => "activityChanged",
        GatewayEvent::TitleChanged { .. } => "titleChanged",
    }
}

impl fmt::Debug for GatewayEventIngress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayEventIngress")
            .field("diagnostics", &self.diagnostics())
            .finish()
    }
}
