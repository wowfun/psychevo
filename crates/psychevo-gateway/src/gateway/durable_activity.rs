use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use psychevo::{
    Error,
    application::{
        GatewayActivityClaimInput, GatewayActivityKind, GatewayActivityRecord,
        GatewayActivityState, GatewayActivityTerminalStatus, GatewayControlCommandKind,
        GatewayLiveSnapshotInput,
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use super::activity::{ActiveActivityControl, GatewayActivity};
use super::event_ingress::GatewayEventEnvelope;
use super::live_projection::{gateway_event_thread_id, gateway_event_turn_id};
#[cfg(test)]
use super::turn_shell::gateway_activity_status_for_shell_outcome;
use super::{Gateway, PendingGatewayLiveSnapshot};
use crate::{GatewayEventEmitter, gateway_now_ms};
use psychevo_gateway_protocol::events_transcript::{
    GatewayActivityView, GatewayEvent, TranscriptEntry,
};
#[cfg(test)]
use psychevo_gateway_protocol::source::GatewayTurn;
use psychevo_gateway_protocol::source::GatewayTurnStatus;

const GATEWAY_ACTIVITY_LEASE_MS: i64 = 30_000;
const GATEWAY_ACTIVITY_HEARTBEAT_MS: i64 = 5_000;
const GATEWAY_CONTROL_POLL_MS: u64 = 500;
const GATEWAY_CONTROL_LATENCY_SAMPLES: usize = 256;

#[derive(Clone, Debug)]
pub(super) struct DurableGatewayActivity {
    pub(super) activity_id: String,
    pub(super) owner_id: String,
    pub(super) generation: i64,
    pub(super) turn_id: Option<String>,
    pub(super) kind: GatewayActivityKind,
}

pub(super) struct DurableGatewayActivityClaim<'a> {
    pub(super) activity_id: &'a str,
    pub(super) thread_id: Option<&'a str>,
    pub(super) source_key: Option<&'a str>,
    pub(super) turn_id: Option<&'a str>,
    pub(super) kind: GatewayActivityKind,
    pub(super) owner_surface: Option<&'a str>,
    pub(super) queued_turns: usize,
    pub(super) intent: Option<Value>,
}

#[derive(Clone)]
struct TrackedShellActivity {
    activity: DurableGatewayActivity,
    lease_lost: CancellationToken,
}

pub(super) struct ShellActivityRuntime {
    activities: Mutex<HashMap<String, TrackedShellActivity>>,
    #[cfg(test)]
    automatic_ticks: AtomicBool,
    control_wakeup: Notify,
    control_commands_applied: AtomicU64,
    control_commands_claimed: AtomicU64,
    control_commands_failed: AtomicU64,
    control_commands_indeterminate: AtomicU64,
    control_dispatch_latencies_ms: Mutex<VecDeque<u64>>,
    control_poll_ticks: AtomicU64,
    dispatcher_tasks_started: AtomicU64,
    failed_operations: AtomicU64,
    first_failure: Mutex<Option<Arc<str>>>,
    heartbeat_transactions: AtomicU64,
    lease_cancellations: AtomicU64,
    pub(super) overload_rejections: AtomicU64,
    shutdown: CancellationToken,
    started: AtomicBool,
}

impl Default for ShellActivityRuntime {
    fn default() -> Self {
        Self {
            activities: Mutex::new(HashMap::new()),
            #[cfg(test)]
            automatic_ticks: AtomicBool::new(true),
            control_wakeup: Notify::new(),
            control_commands_applied: AtomicU64::new(0),
            control_commands_claimed: AtomicU64::new(0),
            control_commands_failed: AtomicU64::new(0),
            control_commands_indeterminate: AtomicU64::new(0),
            control_dispatch_latencies_ms: Mutex::new(VecDeque::new()),
            control_poll_ticks: AtomicU64::new(0),
            dispatcher_tasks_started: AtomicU64::new(0),
            failed_operations: AtomicU64::new(0),
            first_failure: Mutex::new(None),
            heartbeat_transactions: AtomicU64::new(0),
            lease_cancellations: AtomicU64::new(0),
            overload_rejections: AtomicU64::new(0),
            shutdown: CancellationToken::new(),
            started: AtomicBool::new(false),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ShellActivityRuntimeDiagnostics {
    pub(crate) active_activities: usize,
    pub(crate) admitted_activities: usize,
    pub(crate) queued_activities: u64,
    pub(crate) overload_rejections: u64,
    pub(crate) dispatcher_tasks_started: u64,
    pub(crate) control_poll_ticks: u64,
    pub(crate) heartbeat_transactions: u64,
    pub(crate) lease_cancellations: u64,
    pub(crate) control_commands_claimed: u64,
    pub(crate) control_commands_applied: u64,
    pub(crate) control_commands_failed: u64,
    pub(crate) control_commands_indeterminate: u64,
    pub(crate) control_dispatch_latency_samples: u64,
    pub(crate) control_dispatch_latency_p50_ms: Option<u64>,
    pub(crate) control_dispatch_latency_p95_ms: Option<u64>,
    pub(crate) control_dispatch_latency_p99_ms: Option<u64>,
    pub(crate) failed_operations: u64,
    pub(crate) first_failure: Option<Arc<str>>,
}

impl ShellActivityRuntime {
    fn has_tracked_activity(&self) -> bool {
        !self
            .activities
            .lock()
            .expect("Gateway Shell activity runtime poisoned")
            .is_empty()
    }

    pub(super) fn record_failure(&self, message: impl fmt::Display) {
        if self.failed_operations.fetch_add(1, Ordering::Relaxed) == 0 {
            let bounded = message.to_string().chars().take(1_000).collect::<String>();
            *self
                .first_failure
                .lock()
                .expect("Gateway Shell activity failure lock poisoned") = Some(Arc::from(bounded));
        }
    }

    fn record_control_dispatch_latency(&self, created_at_ms: i64) {
        let latency_ms = gateway_now_ms().saturating_sub(created_at_ms).max(0) as u64;
        let mut samples = self
            .control_dispatch_latencies_ms
            .lock()
            .expect("Gateway Shell control latency lock poisoned");
        if samples.len() == GATEWAY_CONTROL_LATENCY_SAMPLES {
            samples.pop_front();
        }
        samples.push_back(latency_ms);
    }

    fn diagnostics(
        &self,
        admitted_activities: usize,
        queued_activities: usize,
    ) -> ShellActivityRuntimeDiagnostics {
        let (latency_samples, latency_p50_ms, latency_p95_ms, latency_p99_ms) = {
            let samples = self
                .control_dispatch_latencies_ms
                .lock()
                .expect("Gateway Shell control latency lock poisoned");
            let mut sorted = samples.iter().copied().collect::<Vec<_>>();
            sorted.sort_unstable();
            (
                sorted.len() as u64,
                percentile(&sorted, 50),
                percentile(&sorted, 95),
                percentile(&sorted, 99),
            )
        };
        ShellActivityRuntimeDiagnostics {
            active_activities: self
                .activities
                .lock()
                .expect("Gateway Shell activity runtime poisoned")
                .len(),
            admitted_activities,
            queued_activities: queued_activities as u64,
            overload_rejections: self.overload_rejections.load(Ordering::Relaxed),
            dispatcher_tasks_started: self.dispatcher_tasks_started.load(Ordering::Relaxed),
            control_poll_ticks: self.control_poll_ticks.load(Ordering::Relaxed),
            heartbeat_transactions: self.heartbeat_transactions.load(Ordering::Relaxed),
            lease_cancellations: self.lease_cancellations.load(Ordering::Relaxed),
            control_commands_claimed: self.control_commands_claimed.load(Ordering::Relaxed),
            control_commands_applied: self.control_commands_applied.load(Ordering::Relaxed),
            control_commands_failed: self.control_commands_failed.load(Ordering::Relaxed),
            control_commands_indeterminate: self
                .control_commands_indeterminate
                .load(Ordering::Relaxed),
            control_dispatch_latency_samples: latency_samples,
            control_dispatch_latency_p50_ms: latency_p50_ms,
            control_dispatch_latency_p95_ms: latency_p95_ms,
            control_dispatch_latency_p99_ms: latency_p99_ms,
            failed_operations: self.failed_operations.load(Ordering::Relaxed),
            first_failure: self
                .first_failure
                .lock()
                .expect("Gateway Shell activity failure lock poisoned")
                .clone(),
        }
    }
}

fn percentile(sorted: &[u64], percentile: usize) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = sorted.len().saturating_mul(percentile).saturating_add(99) / 100;
    sorted.get(rank.saturating_sub(1)).copied()
}

enum GatewayControlApply {
    Applied,
    Failed(&'static str),
}

pub(super) enum GatewayEventPersistence {
    Committed(GatewayEventCommit),
    PendingSnapshot {
        snapshot_key: String,
        coalescible: bool,
    },
}

pub(super) enum GatewayEventCommit {
    LiveEvent {
        seq: i64,
    },
    FrameworkTerminal {
        turn_id: String,
        thread_id: String,
        status: String,
        completed_at_ms: i64,
    },
}

pub(super) struct GatewaySnapshotCommit {
    pub(super) snapshot_key: String,
    pub(super) fingerprint: String,
    pub(super) revision: i64,
}

#[derive(Debug)]
pub(super) enum GatewayEventPersistenceError {
    Retryable(Error),
    Permanent(Error),
}

impl GatewayEventPersistenceError {
    pub(super) fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable(_))
    }

    #[cfg(test)]
    pub(super) fn retryable_test(message: impl Into<String>) -> Self {
        Self::Retryable(Error::Message(message.into()))
    }
}

impl From<Error> for GatewayEventPersistenceError {
    fn from(error: Error) -> Self {
        if error.is_retryable_state_write() {
            Self::Retryable(error)
        } else {
            Self::Permanent(error)
        }
    }
}

impl fmt::Display for GatewayEventPersistenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Retryable(error) | Self::Permanent(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for GatewayEventPersistenceError {}

struct PersistGatewayEventResult {
    accepted_thread_id: Option<String>,
    persistence: GatewayEventPersistence,
}

struct ResolvedGatewayEventActivity {
    activity: DurableGatewayActivity,
    record: GatewayActivityRecord,
}

fn resolved_gateway_event_activity(record: GatewayActivityRecord) -> ResolvedGatewayEventActivity {
    let activity = DurableGatewayActivity {
        activity_id: record.activity_id.clone(),
        owner_id: record.owner_id.clone(),
        generation: record.generation,
        turn_id: record.turn_id.clone(),
        kind: record.kind,
    };
    ResolvedGatewayEventActivity { activity, record }
}

fn gateway_event_initial_thread_binding<'a>(
    activity: &DurableGatewayActivity,
    event: &'a GatewayEvent,
) -> Option<&'a str> {
    let event_turn_id = gateway_event_turn_id(event)?;
    if activity.turn_id.as_deref() != Some(event_turn_id) && activity.activity_id != event_turn_id {
        return None;
    }
    match event {
        GatewayEvent::TurnStarted {
            thread_id: Some(thread_id),
            ..
        } if activity.kind == GatewayActivityKind::Turn => Some(thread_id.as_str()),
        GatewayEvent::EntryStarted { entry, .. }
            if activity.kind == GatewayActivityKind::Shell
                && !entry.thread_id.trim().is_empty() =>
        {
            Some(entry.thread_id.as_str())
        }
        _ => None,
    }
}

impl Gateway {
    pub(super) async fn claim_durable_gateway_activity(
        &self,
        claim: DurableGatewayActivityClaim<'_>,
    ) -> psychevo::Result<DurableGatewayActivity> {
        let record = self
            .durability
            .claim_gateway_activity(GatewayActivityClaimInput {
                activity_id: claim.activity_id,
                thread_id: claim.thread_id,
                source_key: claim.source_key,
                turn_id: claim.turn_id,
                kind: claim.kind,
                owner_id: self.owner_id(),
                owner_surface: claim.owner_surface,
                lease_expires_at_ms: gateway_now_ms() + GATEWAY_ACTIVITY_LEASE_MS,
                queued_turns: claim.queued_turns,
                superseded_activity_id: None,
                intent: claim.intent,
            })
            .await?;
        Ok(DurableGatewayActivity {
            activity_id: record.activity_id,
            owner_id: record.owner_id,
            generation: record.generation,
            turn_id: record.turn_id,
            kind: record.kind,
        })
    }

    fn ensure_shell_activity_runtime(&self) {
        if self
            .shell_activity_runtime
            .started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        self.shell_activity_runtime
            .dispatcher_tasks_started
            .fetch_add(1, Ordering::Relaxed);
        let gateway = self.clone();
        let runtime = self.shell_activity_runtime.clone();
        self.supervisor
            .spawn_infrastructure("gateway-shell-activity-runtime", async move {
                #[cfg(test)]
                if !runtime.automatic_ticks.load(Ordering::Acquire) {
                    runtime.shutdown.cancelled().await;
                    return;
                }
                'runtime: loop {
                    tokio::select! {
                        _ = runtime.shutdown.cancelled() => break 'runtime,
                        _ = runtime.control_wakeup.notified() => {}
                    }
                    if !runtime.has_tracked_activity() {
                        continue;
                    }

                    // A newly tracked Shell may already have a command written by another
                    // Gateway owner. Dispatch it before arming the fallback poll clock.
                    gateway.apply_pending_gateway_control_commands().await;
                    if !runtime.has_tracked_activity() {
                        continue;
                    }

                    let control_poll =
                        tokio::time::sleep(Duration::from_millis(GATEWAY_CONTROL_POLL_MS));
                    let heartbeat = tokio::time::sleep(Duration::from_millis(
                        GATEWAY_ACTIVITY_HEARTBEAT_MS as u64,
                    ));
                    tokio::pin!(control_poll);
                    tokio::pin!(heartbeat);
                    loop {
                        tokio::select! {
                            _ = runtime.shutdown.cancelled() => break 'runtime,
                            _ = runtime.control_wakeup.notified() => {
                                if !runtime.has_tracked_activity() {
                                    break;
                                }
                                gateway.apply_pending_gateway_control_commands().await;
                                control_poll.as_mut().reset(
                                    tokio::time::Instant::now()
                                        + Duration::from_millis(GATEWAY_CONTROL_POLL_MS),
                                );
                            }
                            _ = &mut control_poll => {
                                if !runtime.has_tracked_activity() {
                                    break;
                                }
                                runtime.control_poll_ticks.fetch_add(1, Ordering::Relaxed);
                                gateway.apply_pending_gateway_control_commands().await;
                                control_poll.as_mut().reset(
                                    tokio::time::Instant::now()
                                        + Duration::from_millis(GATEWAY_CONTROL_POLL_MS),
                                );
                            }
                            _ = &mut heartbeat => {
                                if !runtime.has_tracked_activity() {
                                    break;
                                }
                                gateway.refresh_shell_activity_leases().await;
                                heartbeat.as_mut().reset(
                                    tokio::time::Instant::now()
                                        + Duration::from_millis(
                                            GATEWAY_ACTIVITY_HEARTBEAT_MS as u64,
                                        ),
                                );
                            }
                        }
                    }
                }
            });
    }

    #[cfg(test)]
    fn use_manual_shell_activity_scheduler(&self) {
        assert!(
            !self.shell_activity_runtime.started.load(Ordering::Acquire),
            "manual scheduler must be selected before the runtime starts"
        );
        self.shell_activity_runtime
            .automatic_ticks
            .store(false, Ordering::Release);
    }

    pub(super) fn track_shell_activity(
        &self,
        activity: DurableGatewayActivity,
    ) -> CancellationToken {
        self.ensure_shell_activity_runtime();
        let lease_lost = CancellationToken::new();
        self.shell_activity_runtime
            .activities
            .lock()
            .expect("Gateway Shell activity runtime poisoned")
            .insert(
                activity.activity_id.clone(),
                TrackedShellActivity {
                    activity,
                    lease_lost: lease_lost.clone(),
                },
            );
        self.shell_activity_runtime.control_wakeup.notify_one();
        lease_lost
    }

    pub(super) fn untrack_shell_activity(&self, activity_id: &str) {
        let became_idle = {
            let mut activities = self
                .shell_activity_runtime
                .activities
                .lock()
                .expect("Gateway Shell activity runtime poisoned");
            activities.remove(activity_id).is_some() && activities.is_empty()
        };
        if became_idle {
            self.shell_activity_runtime.control_wakeup.notify_one();
        }
    }

    fn cancel_shell_activity_if_still_tracked(&self, tracked: &TrackedShellActivity) -> bool {
        {
            let activities = self
                .shell_activity_runtime
                .activities
                .lock()
                .expect("Gateway Shell activity runtime poisoned");
            let Some(current) = activities.get(&tracked.activity.activity_id) else {
                return false;
            };
            if current.activity.owner_id != tracked.activity.owner_id
                || current.activity.generation != tracked.activity.generation
            {
                return false;
            }
            current.lease_lost.cancel();
        }
        self.shell_activity_runtime
            .lease_cancellations
            .fetch_add(1, Ordering::Relaxed);
        true
    }

    pub(super) fn stop_shell_activity_runtime(&self) {
        self.shell_activity_runtime.shutdown.cancel();
        self.shell_activity_runtime.control_wakeup.notify_waiters();
    }

    pub(crate) fn shell_activity_diagnostics(&self) -> ShellActivityRuntimeDiagnostics {
        let queued_activities = self
            .active_queue
            .lock()
            .expect("gateway active queue poisoned")
            .activities
            .values()
            .map(|state| state.queued.len())
            .sum();
        self.shell_activity_runtime.diagnostics(
            self.supervisor.shell_activity_occupancy(),
            queued_activities,
        )
    }

    async fn refresh_shell_activity_leases(&self) {
        let tracked = self
            .shell_activity_runtime
            .activities
            .lock()
            .expect("Gateway Shell activity runtime poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        if tracked.is_empty() {
            return;
        }
        self.shell_activity_runtime
            .heartbeat_transactions
            .fetch_add(1, Ordering::Relaxed);
        let leases = tracked
            .iter()
            .map(|tracked| {
                (
                    tracked.activity.activity_id.clone(),
                    tracked.activity.generation,
                )
            })
            .collect::<Vec<_>>();
        let refreshed = self
            .durability
            .heartbeat_gateway_activities(
                self.owner_id(),
                &leases,
                gateway_now_ms() + GATEWAY_ACTIVITY_LEASE_MS,
            )
            .await;
        match refreshed {
            Ok(refreshed) => {
                let refreshed = refreshed.into_iter().collect::<HashSet<_>>();
                for tracked in tracked {
                    if !refreshed.contains(&tracked.activity.activity_id)
                        && self.cancel_shell_activity_if_still_tracked(&tracked)
                    {
                        self.shell_activity_runtime.record_failure(format!(
                            "Gateway Shell activity `{}` lost its owner or generation while refreshing its lease",
                            tracked.activity.activity_id
                        ));
                    }
                }
            }
            Err(error) => {
                let cancelled = tracked
                    .iter()
                    .filter(|tracked| self.cancel_shell_activity_if_still_tracked(tracked))
                    .count();
                if cancelled > 0 {
                    self.shell_activity_runtime.record_failure(format!(
                        "Gateway Shell lease heartbeat transaction failed: {error}"
                    ));
                }
            }
        }
    }

    pub(super) fn wrap_gateway_event_sink(
        &self,
        event_sink: Option<GatewayEventEmitter>,
        activity: Option<DurableGatewayActivity>,
        queue_key: Option<String>,
        default_turn_id: Option<String>,
    ) -> Option<GatewayEventEmitter> {
        if event_sink.is_none() && activity.is_none() {
            return None;
        }
        let immediate_gateway = self.clone();
        let immediate_event_sink = event_sink.clone();
        let immediate_activity = activity.clone();
        let immediate_queue_key = queue_key.clone();
        let immediate_default_turn_id = default_turn_id.clone();
        Some(GatewayEventEmitter::try_new(move |event: GatewayEvent| {
            if let (Some(root_activity), Some(queue_key)) =
                (immediate_activity.as_ref(), immediate_queue_key.as_deref())
                && let Some(thread_id) = gateway_event_initial_thread_binding(root_activity, &event)
            {
                immediate_gateway
                    .register_active_thread_alias(queue_key, thread_id)
                    .map_err(|error| crate::GatewayEventEmitError::new(error.to_string()))?;
            }
            let local_result = immediate_event_sink
                .as_ref()
                .map(|event_sink| event_sink.emit(event.clone()))
                .transpose();
            let durable_result = immediate_activity
                .as_ref()
                .map(|root_activity| {
                    immediate_gateway.event_ingress.submit(
                        immediate_gateway.clone(),
                        GatewayEventEnvelope {
                            root_activity_id: Some(root_activity.activity_id.clone()),
                            activity: root_activity.clone(),
                            default_turn_id: immediate_default_turn_id.clone(),
                            event,
                            queue_key: immediate_queue_key.clone(),
                        },
                    )
                })
                .transpose();
            match (local_result, durable_result) {
                (Err(error), _) | (_, Err(error)) => Err(error),
                (Ok(_), Ok(_)) => Ok(()),
            }
        }))
    }

    pub(super) async fn persist_gateway_event_envelope(
        &self,
        envelope: &GatewayEventEnvelope,
        idempotency_key: &str,
    ) -> Result<GatewayEventPersistence, GatewayEventPersistenceError> {
        let effective_activity = self
            .activity_for_gateway_event(&envelope.activity, &envelope.event)
            .await?;
        let event = self
            .attention_event_with_public_provenance(
                envelope.event.clone(),
                &effective_activity.record,
            )
            .await?;
        let result = self
            .persist_gateway_event(
                &effective_activity.activity,
                &event,
                envelope.default_turn_id.as_deref(),
                idempotency_key,
            )
            .await?;
        if let Some(thread_id) = result.accepted_thread_id.as_deref()
            && let Some(queue_key) = envelope.queue_key.as_deref()
            && envelope
                .root_activity_id
                .as_deref()
                .is_some_and(|root| root == effective_activity.activity.activity_id)
        {
            self.register_active_thread_alias(queue_key, thread_id)?;
        }
        Ok(result.persistence)
    }

    async fn activity_for_gateway_event(
        &self,
        root: &DurableGatewayActivity,
        event: &GatewayEvent,
    ) -> Result<ResolvedGatewayEventActivity, GatewayEventPersistenceError> {
        let root_record = self
            .durability
            .gateway_activity(&root.activity_id)
            .await?
            .ok_or_else(|| {
                Error::Message(format!(
                    "Gateway event root activity `{}` is missing",
                    root.activity_id
                ))
            })?;
        if root_record.owner_id != root.owner_id || root_record.generation != root.generation {
            return Err(Error::Message(format!(
                "Gateway event root activity `{}` is no longer owned by generation {}",
                root.activity_id, root.generation
            ))
            .into());
        }
        let Some(thread_id) = gateway_event_thread_id(event) else {
            return Ok(resolved_gateway_event_activity(root_record));
        };
        if root_record.thread_id.as_deref() == Some(thread_id.as_str()) {
            return Ok(resolved_gateway_event_activity(root_record));
        }
        let event_turn_id = gateway_event_turn_id(event);
        if root_record.thread_id.is_none()
            && gateway_event_initial_thread_binding(root, event) == Some(thread_id.as_str())
        {
            return Ok(resolved_gateway_event_activity(root_record));
        }
        let turn_activity = async {
            match event_turn_id {
                Some(turn_id) => self.durability.gateway_activity(turn_id).await,
                None => Ok(None),
            }
        };
        let (matching_turn, matching_thread) = tokio::try_join!(
            turn_activity,
            self.durability
                .active_gateway_activity_for_thread(&thread_id)
        )?;
        let matching_turn = matching_turn.filter(|record| {
            record.owner_id == self.owner_id()
                && record.thread_id.as_deref() == Some(thread_id.as_str())
        });
        let matching_thread = matching_thread.filter(|record| {
            record.owner_id == self.owner_id()
                && event_turn_id.is_none_or(|turn_id| record.turn_id.as_deref() == Some(turn_id))
        });
        let record = matching_turn.or(matching_thread).ok_or_else(|| {
            Error::Message(format!(
                "Gateway event Thread `{thread_id}` has no matching durable activity"
            ))
        })?;
        Ok(resolved_gateway_event_activity(record))
    }

    async fn attention_event_with_public_provenance(
        &self,
        event: GatewayEvent,
        activity: &GatewayActivityRecord,
    ) -> Result<GatewayEvent, GatewayEventPersistenceError> {
        let (mut action, updated) = match event {
            GatewayEvent::ActionRequested { action } => (action, false),
            GatewayEvent::ActionUpdated { action } => (action, true),
            event => return Ok(event),
        };
        let thread_id = action
            .thread_id
            .clone()
            .or_else(|| activity.thread_id.clone())
            .ok_or_else(|| {
                Error::Message(format!(
                    "Gateway action `{}` is missing its Thread identity",
                    action.action_id
                ))
            })?;
        action.thread_id.get_or_insert_with(|| thread_id.clone());
        action
            .activity_id
            .get_or_insert_with(|| activity.activity_id.clone());
        if action.turn_id.is_none() {
            action.turn_id.clone_from(&activity.turn_id);
        }
        if action.source_key.is_none() {
            action.source_key.clone_from(&activity.source_key);
        }
        action
            .owner_id
            .get_or_insert_with(|| activity.owner_id.clone());
        action
            .lease_expires_at_ms
            .get_or_insert(activity.lease_expires_at_ms);
        let (binding, relationship) = tokio::try_join!(
            self.framework_agent_binding(&thread_id),
            self.framework_client.agent_relationship(&thread_id),
        )?;
        let binding = binding.ok_or_else(|| {
            Error::Message(format!(
                "Gateway action Thread `{thread_id}` has no resolved immutable runtime binding"
            ))
        })?;
        let runtime_ref = binding.runtime_ref.clone();
        let runtime_kind = binding.native_kind.clone();
        let profile =
            serde_json::from_str::<Value>(&binding.profile_config_json).map_err(Error::from)?;
        let profile_label = profile
            .get("label")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                if runtime_ref == "native" {
                    "Psychevo".to_string()
                } else {
                    runtime_ref.clone()
                }
            });
        let parent_thread_id = relationship
            .as_ref()
            .map(|relationship| relationship.parent_thread_id.clone())
            .unwrap_or_else(|| thread_id.clone());
        let child_thread_id = relationship
            .as_ref()
            .map(|relationship| relationship.child_thread_id.clone());
        let mut payload = action.payload.as_object().cloned().unwrap_or_default();
        payload
            .entry("runtimeRef".to_string())
            .or_insert_with(|| json!(runtime_ref));
        payload
            .entry("runtimeKind".to_string())
            .or_insert_with(|| json!(runtime_kind));
        payload
            .entry("profileLabel".to_string())
            .or_insert_with(|| json!(profile_label));
        payload.entry("origin".to_string()).or_insert_with(|| {
            json!({
                "parentThreadId": parent_thread_id,
                "childThreadId": child_thread_id,
            })
        });
        action.payload = Value::Object(payload);
        if updated {
            Ok(GatewayEvent::ActionUpdated { action })
        } else {
            Ok(GatewayEvent::ActionRequested { action })
        }
    }

    async fn persist_gateway_event(
        &self,
        activity: &DurableGatewayActivity,
        event: &GatewayEvent,
        default_turn_id: Option<&str>,
        idempotency_key: &str,
    ) -> Result<PersistGatewayEventResult, GatewayEventPersistenceError> {
        let mut accepted_thread_id = None;
        if let Some(thread_id) = gateway_event_initial_thread_binding(activity, event)
            && self
                .durability
                .update_gateway_activity_thread(
                    &activity.activity_id,
                    &activity.owner_id,
                    activity.generation,
                    thread_id,
                    gateway_now_ms() + GATEWAY_ACTIVITY_LEASE_MS,
                )
                .await?
        {
            accepted_thread_id = Some(thread_id.to_string());
        }

        let persistence = if should_append_gateway_live_event(activity, event) {
            let event_value = serde_json::to_value(event).map_err(Error::from)?;
            let commit = self
                .durability
                .append_gateway_live_event(
                    Some(&activity.activity_id),
                    Some(&activity.owner_id),
                    gateway_event_thread_id(event).as_deref(),
                    gateway_event_turn_id(event)
                        .or(default_turn_id)
                        .or(activity.turn_id.as_deref()),
                    Some(idempotency_key),
                    &event_value,
                )
                .await?;
            GatewayEventPersistence::Committed(GatewayEventCommit::LiveEvent { seq: commit.seq })
        } else if let Some((event_kind, entry)) = gateway_live_snapshot_entry(event) {
            let snapshot_key = self.retain_gateway_live_snapshot(
                activity,
                event_kind,
                gateway_event_turn_id(event)
                    .or(default_turn_id)
                    .or(activity.turn_id.as_deref()),
                entry,
                event.clone(),
            )?;
            GatewayEventPersistence::PendingSnapshot {
                snapshot_key,
                coalescible: matches!(event, GatewayEvent::EntryUpdated { .. }),
            }
        } else if matches!(event, GatewayEvent::EntryBlockTextDelta { .. }) {
            GatewayEventPersistence::PendingSnapshot {
                snapshot_key: self.retain_gateway_live_text_delta(activity, event)?,
                coalescible: true,
            }
        } else {
            GatewayEventPersistence::Committed(self.authoritative_framework_terminal(event).await?)
        };
        Ok(PersistGatewayEventResult {
            accepted_thread_id,
            persistence,
        })
    }

    fn retain_gateway_live_snapshot(
        &self,
        activity: &DurableGatewayActivity,
        event_kind: &'static str,
        turn_id: Option<&str>,
        entry: &TranscriptEntry,
        event: GatewayEvent,
    ) -> Result<String, GatewayEventPersistenceError> {
        let Some(turn_id) = turn_id else {
            return Err(Error::Message(
                "Gateway retained-live entry is missing its Turn identity".to_string(),
            )
            .into());
        };
        if entry.id.trim().is_empty() || entry.thread_id.trim().is_empty() {
            return Err(Error::Message(
                "Gateway retained-live entry is missing its entry or Thread identity".to_string(),
            )
            .into());
        }
        let snapshot_key = format!("{}:{turn_id}:{}", activity.activity_id, entry.id);
        {
            let mut pending = self
                .live_snapshots
                .lock()
                .expect("gateway live snapshot map poisoned");
            let snapshot =
                pending
                    .entry(snapshot_key.clone())
                    .or_insert_with(|| PendingGatewayLiveSnapshot {
                        snapshot_key: snapshot_key.clone(),
                        activity_id: Some(activity.activity_id.clone()),
                        owner_id: Some(activity.owner_id.clone()),
                        thread_id: Some(entry.thread_id.clone()),
                        turn_id: Some(turn_id.to_string()),
                        event_kind: event_kind.to_string(),
                        event: event.clone(),
                        dirty: false,
                    });
            snapshot.activity_id = Some(activity.activity_id.clone());
            snapshot.owner_id = Some(activity.owner_id.clone());
            snapshot.thread_id = Some(entry.thread_id.clone());
            snapshot.turn_id = Some(turn_id.to_string());
            snapshot.event_kind = event_kind.to_string();
            snapshot.event = event;
            snapshot.dirty = true;
        }
        Ok(snapshot_key)
    }

    fn retain_gateway_live_text_delta(
        &self,
        activity: &DurableGatewayActivity,
        event: &GatewayEvent,
    ) -> Result<String, GatewayEventPersistenceError> {
        let GatewayEvent::EntryBlockTextDelta {
            thread_id,
            turn_id,
            entry_id,
            block_id,
            text,
            updated_at_ms,
        } = event
        else {
            return Err(Error::Message(
                "Gateway retained-live delta has an invalid event kind".to_string(),
            )
            .into());
        };
        if text.is_empty() || turn_id.is_empty() || entry_id.is_empty() || block_id.is_empty() {
            return Err(Error::Message(
                "Gateway retained-live delta is missing text, Turn, entry, or block identity"
                    .to_string(),
            )
            .into());
        }
        let snapshot_key = format!("{}:{turn_id}:{entry_id}", activity.activity_id);
        {
            let mut pending = self
                .live_snapshots
                .lock()
                .expect("gateway live snapshot map poisoned");
            let Some(snapshot) = pending.get_mut(&snapshot_key) else {
                return Err(Error::Message(format!(
                    "Gateway retained-live delta is missing retained base snapshot `{snapshot_key}`"
                ))
                .into());
            };
            if thread_id
                .as_deref()
                .is_some_and(|thread_id| snapshot.thread_id.as_deref() != Some(thread_id))
            {
                return Err(Error::Message(format!(
                    "Gateway retained-live delta Thread identity does not match base snapshot `{snapshot_key}`"
                ))
                .into());
            }
            if !apply_gateway_live_text_delta(
                &mut snapshot.event,
                turn_id,
                entry_id,
                block_id,
                text,
                *updated_at_ms,
            ) {
                return Err(Error::Message(format!(
                    "Gateway retained-live delta does not match entry/block identity in base snapshot `{snapshot_key}`"
                ))
                .into());
            }
            snapshot.dirty = true;
        }
        Ok(snapshot_key)
    }

    async fn authoritative_framework_terminal(
        &self,
        event: &GatewayEvent,
    ) -> Result<GatewayEventCommit, GatewayEventPersistenceError> {
        let GatewayEvent::TurnCompleted {
            thread_id,
            turn_id,
            turn,
            committed_entries,
        } = event
        else {
            return Err(Error::Message(
                "Gateway retained-live event has no durable commit target".to_string(),
            )
            .into());
        };
        if !committed_entries.is_empty() || turn.id != turn_id.as_str() {
            return Err(Error::Message(format!(
                "Gateway Framework terminal identity does not match Turn `{turn_id}`"
            ))
            .into());
        }
        let terminal = self
            .framework_client
            .framework_turn_terminal_evidence(turn_id)
            .await?
            .ok_or_else(|| {
                Error::Message(format!(
                    "Gateway Framework terminal `{turn_id}` is not durably committed"
                ))
            })?;
        let expected_thread_id = thread_id.as_deref().or(turn.thread_id.as_deref());
        if expected_thread_id != Some(terminal.thread_id.as_str())
            || terminal.status.as_str() != gateway_turn_status_name(turn.status)
            || turn
                .completed_at_ms
                .is_some_and(|completed_at_ms| completed_at_ms != terminal.completed_at_ms)
            || turn.outcome.as_deref() != Some(terminal.outcome.as_str())
        {
            return Err(Error::Message(format!(
                "Gateway Framework terminal `{turn_id}` does not match the authoritative durable terminal"
            ))
            .into());
        }
        Ok(GatewayEventCommit::FrameworkTerminal {
            turn_id: terminal.turn_id,
            thread_id: terminal.thread_id,
            status: terminal.status.as_str().to_string(),
            completed_at_ms: terminal.completed_at_ms,
        })
    }

    pub(super) async fn flush_gateway_live_snapshots(
        &self,
        snapshot_keys: &[String],
    ) -> Result<Vec<GatewaySnapshotCommit>, GatewayEventPersistenceError> {
        let snapshots = {
            let pending = self
                .live_snapshots
                .lock()
                .expect("gateway live snapshot map poisoned");
            snapshot_keys
                .iter()
                .filter_map(|snapshot_key| pending.get(snapshot_key))
                .filter(|snapshot| snapshot.dirty)
                .cloned()
                .collect::<Vec<_>>()
        };
        let mut fingerprints = Vec::with_capacity(snapshots.len());
        let mut inputs = Vec::with_capacity(snapshots.len());
        for snapshot in &snapshots {
            let event = serde_json::to_value(&snapshot.event).map_err(Error::from)?;
            let encoded = serde_json::to_vec(&event).map_err(Error::from)?;
            fingerprints.push(format!("{:x}", Sha256::digest(&encoded)));
            inputs.push(GatewayLiveSnapshotInput {
                snapshot_key: &snapshot.snapshot_key,
                activity_id: snapshot.activity_id.as_deref(),
                owner_id: snapshot.owner_id.as_deref(),
                thread_id: snapshot.thread_id.as_deref(),
                turn_id: snapshot.turn_id.as_deref(),
                event_kind: &snapshot.event_kind,
                event,
            });
        }
        let revisions = self
            .durability
            .upsert_gateway_live_snapshots(&inputs)
            .await?;
        if revisions.len() != snapshots.len() {
            return Err(Error::Message(
                "Gateway retained-live batch returned an incomplete revision set".to_string(),
            )
            .into());
        }
        Ok(snapshots
            .into_iter()
            .zip(fingerprints)
            .zip(revisions)
            .map(
                |((snapshot, fingerprint), revision)| GatewaySnapshotCommit {
                    snapshot_key: snapshot.snapshot_key,
                    fingerprint,
                    revision,
                },
            )
            .collect())
    }

    pub(super) fn finish_gateway_live_snapshot_updates<'a>(
        &self,
        snapshot_keys: impl IntoIterator<Item = &'a str>,
    ) {
        let mut pending = self
            .live_snapshots
            .lock()
            .expect("gateway live snapshot map poisoned");
        for snapshot_key in snapshot_keys {
            if let Some(snapshot) = pending.get_mut(snapshot_key) {
                snapshot.dirty = false;
            }
        }
    }

    async fn clear_gateway_live_snapshots_for_activity(
        &self,
        activity_id: &str,
    ) -> psychevo::Result<()> {
        {
            let mut pending = self
                .live_snapshots
                .lock()
                .expect("gateway live snapshot map poisoned");
            pending.retain(|_, snapshot| snapshot.activity_id.as_deref() != Some(activity_id));
        }
        self.durability
            .delete_gateway_live_snapshots_for_activity(activity_id)
            .await?;
        Ok(())
    }

    pub(super) async fn finish_durable_gateway_activity(
        &self,
        activity: Option<&DurableGatewayActivity>,
        status: GatewayActivityTerminalStatus,
    ) -> psychevo::Result<()> {
        let Some(activity) = activity else {
            return Ok(());
        };
        let fence = self.event_ingress.fence().await.map_err(|error| {
            Error::Message(format!(
                "Gateway Shell activity `{}` retained-live fence failed: {error}",
                activity.activity_id
            ))
        });
        let terminal = async {
            let finished = self
                .durability
                .finish_gateway_activity(
                    &activity.activity_id,
                    &activity.owner_id,
                    activity.generation,
                    status,
                )
                .await?;
            if !finished {
                return Err(Error::Message(format!(
                    "Gateway Shell activity `{}` could not be finalized because its ownership or status changed",
                    activity.activity_id
                )));
            }
            self.clear_gateway_live_snapshots_for_activity(&activity.activity_id)
                .await
        }
        .await;
        match (fence, terminal) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(fence_error), Err(terminal_error)) => Err(Error::Message(format!(
                "{fence_error}; Gateway Shell terminal cleanup also failed: {terminal_error}"
            ))),
        }
    }

    pub(super) async fn finalize_abandoned_shell_activity(
        &self,
        activity_id: &str,
        status: GatewayActivityTerminalStatus,
    ) -> psychevo::Result<()> {
        self.untrack_shell_activity(activity_id);
        let Some(record) = self.durability.gateway_activity(activity_id).await? else {
            return Ok(());
        };
        if record.owner_id != self.owner_id()
            || !matches!(
                record.status,
                GatewayActivityState::Running | GatewayActivityState::Queued
            )
        {
            return Ok(());
        }
        self.finish_durable_gateway_activity(
            Some(&DurableGatewayActivity {
                activity_id: record.activity_id,
                owner_id: record.owner_id,
                generation: record.generation,
                turn_id: record.turn_id,
                kind: record.kind,
            }),
            status,
        )
        .await
    }

    pub(crate) fn interrupt_local_activity(&self, activity_id: &str) -> bool {
        self.control_for_activity_id(activity_id)
            .map(|control| {
                control.interrupt();
                true
            })
            .unwrap_or(false)
    }

    pub(super) async fn apply_pending_gateway_control_commands(&self) {
        match self
            .durability
            .recover_indeterminate_gateway_control_commands(gateway_now_ms())
            .await
        {
            Ok(recovered) if !recovered.is_empty() => {
                self.shell_activity_runtime
                    .control_commands_indeterminate
                    .fetch_add(recovered.len() as u64, Ordering::Relaxed);
                let ids = recovered
                    .iter()
                    .take(16)
                    .map(|command| command.id.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                self.shell_activity_runtime.record_failure(format!(
                    "{} Gateway Shell control command(s) have outcome_indeterminate after owner loss; ids={ids}",
                    recovered.len()
                ));
            }
            Ok(_) => {}
            Err(error) => self.shell_activity_runtime.record_failure(format!(
                "Gateway Shell indeterminate control recovery failed: {error}"
            )),
        }
        let commands = match self
            .durability
            .claim_pending_gateway_control_commands(self.owner_id(), 50)
            .await
        {
            Ok(commands) => commands,
            Err(error) => {
                self.shell_activity_runtime
                    .record_failure(format!("Gateway Shell control claim failed: {error}"));
                return;
            }
        };
        self.shell_activity_runtime
            .control_commands_claimed
            .fetch_add(commands.len() as u64, Ordering::Relaxed);
        for command in commands {
            self.shell_activity_runtime
                .record_control_dispatch_latency(command.created_at_ms);
            let outcome = match command.command_kind {
                GatewayControlCommandKind::Interrupt => self
                    .control_for_activity_id(&command.activity_id)
                    .map(|control| {
                        control.interrupt();
                        GatewayControlApply::Applied
                    })
                    .unwrap_or(GatewayControlApply::Failed("no matching active control")),
                GatewayControlCommandKind::Steer => {
                    self.apply_steer_control_command(&command.activity_id, &command.payload)
                }
                GatewayControlCommandKind::Clarify => GatewayControlApply::Failed(
                    "clarify control is unsupported for Gateway Shell activities",
                ),
                GatewayControlCommandKind::Permission => GatewayControlApply::Failed(
                    "permission control is unsupported for Gateway Shell activities",
                ),
            };
            let store = &self.durability;
            let (transition, counter) = match outcome {
                GatewayControlApply::Applied => (
                    store.mark_gateway_control_command_applied(command.id).await,
                    &self.shell_activity_runtime.control_commands_applied,
                ),
                GatewayControlApply::Failed(error) => (
                    store
                        .mark_gateway_control_command_failed(command.id, error)
                        .await,
                    &self.shell_activity_runtime.control_commands_failed,
                ),
            };
            match transition {
                Ok(true) => {
                    counter.fetch_add(1, Ordering::Relaxed);
                }
                Ok(false) => self.shell_activity_runtime.record_failure(format!(
                    "Gateway Shell control command {} lost its applying state after the side-effect decision",
                    command.id
                )),
                Err(error) => self.shell_activity_runtime.record_failure(format!(
                    "Gateway Shell control command {} could not persist its post-application state: {error}",
                    command.id
                )),
            }
        }
    }

    fn apply_steer_control_command(
        &self,
        activity_id: &str,
        payload: &Value,
    ) -> GatewayControlApply {
        if let Some(expected_turn_id) = payload.get("expectedTurnId").and_then(Value::as_str)
            && expected_turn_id != activity_id
        {
            return GatewayControlApply::Failed("stale expected turn");
        }
        if self.control_for_activity_id(activity_id).is_none() {
            return GatewayControlApply::Failed("no matching active control");
        }
        GatewayControlApply::Failed("steer control is unsupported for Gateway Shell activities")
    }

    fn control_for_activity_id(&self, activity_id: &str) -> Option<ActiveActivityControl> {
        let queue = self
            .active_queue
            .lock()
            .expect("gateway active queue poisoned");
        queue.activities.values().find_map(|state| {
            if state.active_turn_id.as_deref() == Some(activity_id) {
                state.control.clone()
            } else {
                None
            }
        })
    }
}

pub(crate) fn gateway_activity_view(activity: &GatewayActivity) -> GatewayActivityView {
    GatewayActivityView {
        activities: activity.activities.clone(),
        framework_revision: activity.framework_revision.clone(),
        running: activity.running,
        active_turn_id: activity.active_turn_id.clone(),
        queued_turns: activity.queued_turns,
        started_at_ms: activity.started_at_ms,
        updated_at_ms: activity.updated_at_ms,
        owner_id: activity.owner_id.clone(),
        owner_surface: activity.owner_surface.clone(),
        lease_expires_at_ms: activity.lease_expires_at_ms,
        takeover_state: activity.takeover_state.clone(),
    }
}

fn should_append_gateway_live_event(
    activity: &DurableGatewayActivity,
    event: &GatewayEvent,
) -> bool {
    if let GatewayEvent::TurnCompleted {
        committed_entries, ..
    } = event
        && activity.kind == GatewayActivityKind::Turn
        && committed_entries.is_empty()
    {
        return false;
    }
    matches!(
        event,
        GatewayEvent::TurnStarted { .. }
            | GatewayEvent::TurnQueued { .. }
            | GatewayEvent::TurnCompleted { .. }
            | GatewayEvent::ActionRequested { .. }
            | GatewayEvent::ActionUpdated { .. }
            | GatewayEvent::ActionResolved { .. }
            | GatewayEvent::ActionCancelled { .. }
            | GatewayEvent::Warning { .. }
            | GatewayEvent::ActivityChanged { .. }
            | GatewayEvent::TitleChanged { .. }
    )
}

fn gateway_turn_status_name(status: GatewayTurnStatus) -> &'static str {
    match status {
        GatewayTurnStatus::Queued => "queued",
        GatewayTurnStatus::Running => "running",
        GatewayTurnStatus::Completed => "completed",
        GatewayTurnStatus::Failed => "failed",
        GatewayTurnStatus::Interrupted => "interrupted",
    }
}

fn gateway_live_snapshot_entry(event: &GatewayEvent) -> Option<(&'static str, &TranscriptEntry)> {
    match event {
        GatewayEvent::EntryStarted { entry, .. } => Some(("entryStarted", entry)),
        GatewayEvent::EntryUpdated { entry, .. } => Some(("entryUpdated", entry)),
        GatewayEvent::EntryCompleted { entry, .. } => Some(("entryCompleted", entry)),
        _ => None,
    }
}

fn gateway_live_snapshot_entry_mut(event: &mut GatewayEvent) -> Option<&mut TranscriptEntry> {
    match event {
        GatewayEvent::EntryStarted { entry, .. }
        | GatewayEvent::EntryUpdated { entry, .. }
        | GatewayEvent::EntryCompleted { entry, .. } => Some(entry),
        _ => None,
    }
}

fn apply_gateway_live_text_delta(
    event: &mut GatewayEvent,
    turn_id: &str,
    entry_id: &str,
    block_id: &str,
    text: &str,
    updated_at_ms: i64,
) -> bool {
    let Some(entry) = gateway_live_snapshot_entry_mut(event) else {
        return false;
    };
    if entry.id != entry_id || entry.turn_id.as_deref() != Some(turn_id) {
        return false;
    }
    let Some(block) = entry.blocks.iter_mut().find(|block| block.id == block_id) else {
        return false;
    };
    block.body.get_or_insert_default().push_str(text);
    block.updated_at_ms = updated_at_ms;
    entry.updated_at_ms = updated_at_ms;
    true
}

#[cfg(test)]
mod event_ingress_tests {
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use psychevo::application::{
        GatewayControlCommandInput, GatewayDurability, RunStreamEvent, StartThreadRequest,
    };
    use psychevo::{Application, ShellCommandEvent, ShellCommandOutcome, ShellCommandRequest};
    use serde_json::json;
    use tokio::sync::oneshot;
    use uuid::Uuid;

    use super::{
        DurableGatewayActivityClaim, Gateway, GatewayActivityKind, GatewayActivityRecord,
        GatewayActivityState, GatewayActivityTerminalStatus, GatewayControlCommandKind,
        GatewayEvent, GatewayEventEmitter, GatewayTurn, GatewayTurnStatus,
        PendingGatewayLiveSnapshot, apply_gateway_live_text_delta,
        gateway_activity_status_for_shell_outcome, gateway_live_snapshot_entry, percentile,
    };
    use crate::composition::GatewayApplication;
    use crate::gateway::activity::{ActiveActivityControl, ActiveActivityKind};
    use crate::gateway::activity::{
        PendingQueuedActivity, PendingQueuedShell, SendShellRequest, ShellExecutionIntent,
    };
    use crate::gateway::supervisor::{GatewayActivityAdmissionError, GatewayTaskScope};
    use crate::gateway::{GatewayEventCommitEvidence, GatewayLimits};
    use crate::gateway_now_ms;
    use crate::projection::GatewayLiveProjector;
    use psychevo_gateway_protocol::source::{
        BackendKind, GatewayBackendInfo, GatewaySource, GatewayThreadSelector,
    };

    fn configured_shell_request(
        temp: &tempfile::TempDir,
        cwd: &Path,
        source: GatewaySource,
        command: &str,
    ) -> SendShellRequest {
        let home = temp.path().join("home");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::write(
            home.join("config.toml"),
            r#"
model = "lmstudio/test-model"

[provider.lmstudio.models.test-model]
"#,
        )
        .expect("config");
        SendShellRequest {
            thread_id: None,
            source: Some(source),
            bind_source: None,
            cwd: cwd.to_path_buf(),
            command: command.to_string(),
            execution: ShellExecutionIntent::new("test")
                .continue_latest(["test".to_string()])
                .inherited_environment(BTreeMap::from([
                    (
                        "HOME".to_string(),
                        temp.path().to_string_lossy().into_owned(),
                    ),
                    (
                        "PSYCHEVO_HOME".to_string(),
                        home.to_string_lossy().into_owned(),
                    ),
                ])),
            event_sink: None,
            lineage: None,
        }
    }

    async fn compose_test_framework(
        temp: &tempfile::TempDir,
    ) -> (Application, Gateway, GatewayDurability) {
        compose_test_framework_with_limits(temp, GatewayLimits::default()).await
    }

    async fn compose_test_framework_with_limits(
        temp: &tempfile::TempDir,
        limits: GatewayLimits,
    ) -> (Application, Gateway, GatewayDurability) {
        let home = temp.path().join("home");
        std::fs::create_dir_all(&home).expect("home");
        let executor: crate::FrameworkNativeTestExecutor = Arc::new(|invocation| {
            Box::pin(async move {
                invocation.persistence.confirm_delivery().await?;
                Ok(psychevo::TurnResult {
                    thread_id: invocation.receipt.thread_id,
                    outcome: psychevo::TurnOutcome::Completed,
                    final_answer: String::new(),
                    provider: "fixture-provider".to_string(),
                    model: "fixture-model".to_string(),
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
        });
        let runtime = GatewayApplication::open_with_native_test_executor_and_limits(
            home.clone(),
            temp.path().join("state.db"),
            None,
            BTreeMap::new(),
            limits,
            executor,
        )
        .await
        .expect("test composition");
        (
            runtime.application().clone(),
            runtime.gateway().clone(),
            runtime.application().gateway_durability(),
        )
    }

    async fn test_ingress_sink(
        gateway: &Gateway,
        thread_id: Option<&str>,
        turn_id: &str,
    ) -> GatewayEventEmitter {
        let activity = gateway
            .claim_durable_gateway_activity(DurableGatewayActivityClaim {
                activity_id: turn_id,
                thread_id,
                source_key: None,
                turn_id: Some(turn_id),
                kind: GatewayActivityKind::Turn,
                owner_surface: Some("test"),
                queued_turns: 0,
                intent: None,
            })
            .await
            .expect("activity");
        gateway
            .wrap_gateway_event_sink(None, Some(activity), None, Some(turn_id.to_string()))
            .expect("wrapped sink")
    }

    #[tokio::test]
    async fn first_shell_thread_alias_precedes_local_event_history_admission() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (application, gateway, _durability) = compose_test_framework(&temp).await;
        let thread_id = application
            .client()
            .start_thread(StartThreadRequest::new(Path::new(".")))
            .await
            .expect("thread")
            .id()
            .to_string();
        let queue_key = "source:test-shell-alias";
        {
            let mut queue = gateway.active_queue.lock().expect("active queue");
            let state = queue.activities.entry(queue_key.to_string()).or_default();
            state.running = true;
            state.active_turn_id = Some("shell-alias".to_string());
            state.active_kind = Some(ActiveActivityKind::Shell);
        }
        let activity = gateway
            .claim_durable_gateway_activity(DurableGatewayActivityClaim {
                activity_id: "shell-alias",
                thread_id: None,
                source_key: Some("test-shell-alias"),
                turn_id: Some("shell-alias"),
                kind: GatewayActivityKind::Shell,
                owner_surface: Some("test"),
                queued_turns: 0,
                intent: None,
            })
            .await
            .expect("activity");
        gateway.event_ingress.pause_worker();
        let reservation_rejected = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&reservation_rejected);
        let callback_gateway = gateway.clone();
        let callback_thread_id = thread_id.clone();
        let local_sink = GatewayEventEmitter::new(move |_| {
            observed.store(
                callback_gateway
                    .reserve_history_mutation(&callback_thread_id, "busy")
                    .is_err(),
                Ordering::SeqCst,
            );
        });
        let sink = gateway
            .wrap_gateway_event_sink(
                Some(local_sink),
                Some(activity),
                Some(queue_key.to_string()),
                Some("shell-alias".to_string()),
            )
            .expect("sink");
        let (event, _, _) = initial_assistant_entry(&thread_id, "shell-alias");

        sink.emit(event).expect("local-first event");

        assert!(
            reservation_rejected.load(Ordering::SeqCst),
            "the observed Thread id must already resolve to the active source lane"
        );
        gateway.event_ingress.resume_worker();
        gateway.event_ingress.fence().await.expect("event fence");
    }

    fn initial_assistant_entry(thread_id: &str, turn_id: &str) -> (GatewayEvent, String, String) {
        let mut projector = GatewayLiveProjector::new(Some(thread_id.to_string()));
        let event = projector
            .project(
                turn_id,
                &RunStreamEvent::AssistantTextDelta {
                    text: "first".to_string(),
                },
            )
            .expect("initial assistant entry");
        let entry = gateway_live_snapshot_entry(&event)
            .expect("entry snapshot")
            .1;
        (event.clone(), entry.id.clone(), entry.blocks[0].id.clone())
    }

    async fn wait_for_active_shell(
        durability: &GatewayDurability,
        source_key: &str,
    ) -> GatewayActivityRecord {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(activity) = durability
                    .active_gateway_activity_for_source(source_key)
                    .await
                    .expect("active activity")
                {
                    return activity;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Shell activity claim timeout")
    }

    async fn wait_for_terminal_shell(
        durability: &GatewayDurability,
        activity_id: &str,
    ) -> GatewayActivityRecord {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(activity) = durability
                    .gateway_activity(activity_id)
                    .await
                    .expect("latest activity")
                    && !matches!(
                        activity.status,
                        GatewayActivityState::Running | GatewayActivityState::Queued
                    )
                {
                    return activity;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Shell activity finalization timeout")
    }

    #[test]
    fn shell_outcomes_map_to_durable_terminals() {
        assert_eq!(
            gateway_activity_status_for_shell_outcome(ShellCommandOutcome::Completed),
            GatewayActivityTerminalStatus::Completed
        );
        assert_eq!(
            gateway_activity_status_for_shell_outcome(ShellCommandOutcome::Failed),
            GatewayActivityTerminalStatus::Failed
        );
        assert_eq!(
            gateway_activity_status_for_shell_outcome(ShellCommandOutcome::Interrupted),
            GatewayActivityTerminalStatus::Interrupted
        );
    }

    #[test]
    fn retained_live_snapshot_accumulates_typed_text_delta() {
        let mut projector = GatewayLiveProjector::new(Some("thread-1".to_string()));
        let snapshot = projector
            .project(
                "turn-1",
                &RunStreamEvent::AssistantTextDelta {
                    text: "first".to_string(),
                },
            )
            .expect("initial snapshot");
        let mut pending = PendingGatewayLiveSnapshot {
            snapshot_key: "activity:turn-1:live:turn-1:assistant:0".to_string(),
            activity_id: Some("activity".to_string()),
            owner_id: Some("owner".to_string()),
            thread_id: Some("thread-1".to_string()),
            turn_id: Some("turn-1".to_string()),
            event_kind: "entry_started".to_string(),
            event: snapshot,
            dirty: false,
        };

        assert!(apply_gateway_live_text_delta(
            &mut pending.event,
            "turn-1",
            "live:turn-1:assistant:0",
            "live:turn-1:assistant:0:text:0",
            " second",
            42,
        ));
        let entry = gateway_live_snapshot_entry(&pending.event)
            .expect("retained entry")
            .1;
        assert_eq!(entry.blocks[0].body.as_deref(), Some("first second"));
        assert_eq!(entry.blocks[0].updated_at_ms, 42);
        assert_eq!(entry.updated_at_ms, 42);
    }

    #[tokio::test]
    async fn ingress_rejects_text_delta_without_a_retained_base_snapshot() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (_application, gateway, _durability) = compose_test_framework(&temp).await;
        let activity = gateway
            .claim_durable_gateway_activity(DurableGatewayActivityClaim {
                activity_id: "turn-missing-base",
                thread_id: None,
                source_key: None,
                turn_id: Some("turn-missing-base"),
                kind: GatewayActivityKind::Turn,
                owner_surface: Some("test"),
                queued_turns: 0,
                intent: None,
            })
            .await
            .expect("activity");
        let sink = gateway
            .wrap_gateway_event_sink(
                None,
                Some(activity),
                None,
                Some("turn-missing-base".to_string()),
            )
            .expect("wrapped sink");

        sink.emit(GatewayEvent::EntryBlockTextDelta {
            thread_id: None,
            turn_id: "turn-missing-base".to_string(),
            entry_id: "entry-missing".to_string(),
            block_id: "block-missing".to_string(),
            text: "orphan".to_string(),
            updated_at_ms: 1,
        })
        .expect("bounded admission");

        let error = gateway
            .event_ingress
            .fence()
            .await
            .expect_err("missing delta base must fail persistence");
        assert!(error.to_string().contains("missing retained base snapshot"));
        let diagnostics = gateway.event_ingress_diagnostics();
        assert_eq!(diagnostics.processed, 1);
        assert_eq!(diagnostics.committed, 0);
        assert_eq!(diagnostics.failed, 1);
        assert_eq!(diagnostics.occupancy, 0);
    }

    #[tokio::test]
    async fn coalesced_entry_deltas_stay_occupied_until_the_single_flush_deadline() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (application, gateway, durability) = compose_test_framework(&temp).await;
        let thread_id = application
            .client()
            .start_thread(StartThreadRequest::new(Path::new(".")))
            .await
            .expect("thread")
            .id()
            .to_string();
        let sink = test_ingress_sink(&gateway, Some(&thread_id), "turn-coalesced").await;
        let (initial, entry_id, block_id) = initial_assistant_entry(&thread_id, "turn-coalesced");

        sink.emit(initial).expect("initial entry admission");
        gateway
            .event_ingress
            .fence()
            .await
            .expect("initial snapshot fence");
        tokio::time::pause();
        gateway.event_ingress.pause_worker_after_processed(3);
        for (text, updated_at_ms) in [(" second", 2), (" third", 3)] {
            sink.emit(GatewayEvent::EntryBlockTextDelta {
                thread_id: Some(thread_id.clone()),
                turn_id: "turn-coalesced".to_string(),
                entry_id: entry_id.clone(),
                block_id: block_id.clone(),
                text: text.to_string(),
                updated_at_ms,
            })
            .expect("delta admission");
        }
        gateway.event_ingress.wait_until_processed(3).await;

        let pending = gateway.event_ingress_diagnostics();
        assert_eq!(pending.processed, 3);
        assert_eq!(pending.committed, 1);
        assert_eq!(pending.occupancy, 2);
        tokio::time::advance(Duration::from_millis(249)).await;
        assert_eq!(gateway.event_ingress_diagnostics().committed, 1);

        tokio::time::advance(Duration::from_millis(1)).await;
        assert_eq!(gateway.event_ingress_diagnostics().committed, 1);
        gateway.event_ingress.resume_worker();
        gateway.event_ingress.wait_until_committed(3).await;
        tokio::time::resume();
        let committed = gateway.event_ingress_diagnostics();
        assert_eq!(committed.committed, 3);
        assert_eq!(committed.occupancy, 0);
        assert!(matches!(
            committed.last_commit,
            Some(GatewayEventCommitEvidence::LiveSnapshot {
                revision: 2,
                ref fingerprint,
                ..
            }) if fingerprint.len() == 64
        ));
        let snapshots = durability
            .list_gateway_live_snapshots(10)
            .await
            .expect("snapshots");
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].revision, 2);
        let entry = gateway_live_snapshot_entry(
            &serde_json::from_value(snapshots[0].event.clone()).expect("snapshot event"),
        )
        .expect("snapshot entry")
        .1
        .clone();
        assert_eq!(entry.blocks[0].body.as_deref(), Some("first second third"));
    }

    #[tokio::test]
    async fn ingress_retries_a_lost_commit_ack_with_the_same_key_and_one_live_row() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (_application, gateway, durability) = compose_test_framework(&temp).await;
        let sink = test_ingress_sink(&gateway, None, "turn-retry").await;
        gateway.event_ingress.retry_after_next_commit();

        sink.emit(GatewayEvent::TurnQueued {
            thread_id: None,
            turn_id: "turn-retry".to_string(),
            queue_position: 1,
        })
        .expect("event admission");
        gateway.event_ingress.fence().await.expect("retrying fence");

        let events = durability
            .list_gateway_live_events_after(0, 10)
            .await
            .expect("live events");
        assert_eq!(events.len(), 1);
        let idempotency_key = events[0]
            .idempotency_key
            .as_deref()
            .expect("ingress idempotency key");
        assert!(idempotency_key.starts_with("gateway-ingress:v1:"));
        let diagnostics = gateway.event_ingress_diagnostics();
        assert_eq!(diagnostics.processed, 1);
        assert_eq!(diagnostics.retried, 1);
        assert_eq!(diagnostics.committed, 1);
        assert_eq!(diagnostics.failed, 0);
        assert_eq!(diagnostics.occupancy, 0);
        assert!(matches!(
            diagnostics.last_commit,
            Some(GatewayEventCommitEvidence::LiveEvent { seq, .. }) if seq == events[0].seq
        ));
    }

    #[tokio::test]
    async fn closed_framework_pool_fails_preprocessing_without_a_root_fallback_commit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let database = temp.path().join("state.db");
        let (application, gateway, durability) = compose_test_framework(&temp).await;
        let thread_id = application
            .client()
            .start_thread(StartThreadRequest::new(Path::new(".")))
            .await
            .expect("thread")
            .id()
            .to_string();
        let sink = test_ingress_sink(&gateway, Some(&thread_id), "turn-closed-pool").await;
        assert!(
            durability
                .list_gateway_live_events_after(0, 10)
                .await
                .expect("initial live events")
                .is_empty()
        );
        gateway.event_ingress.pause_worker();
        sink.emit(GatewayEvent::TurnQueued {
            thread_id: Some(thread_id),
            turn_id: "turn-closed-pool".to_string(),
            queue_position: 1,
        })
        .expect("bounded event admission");
        application.shutdown().await.expect("close Framework pool");
        let closed_read_error = durability
            .gateway_activity("turn-closed-pool")
            .await
            .expect_err("Gateway Store read must observe the closed pool")
            .to_string();
        assert!(closed_read_error.to_ascii_lowercase().contains("closed"));
        gateway.event_ingress.resume_worker();
        let error = gateway
            .event_ingress
            .fence()
            .await
            .expect_err("closed pool must fail the durability fence")
            .to_string();
        assert!(error.contains("event_kind=turnQueued"));
        assert!(error.to_ascii_lowercase().contains("closed"));
        let diagnostics = gateway.event_ingress_diagnostics();
        assert_eq!(diagnostics.accepted, 1);
        assert_eq!(diagnostics.processed, 1);
        assert_eq!(diagnostics.failed, 1);
        assert_eq!(diagnostics.committed, 0);
        assert_eq!(diagnostics.occupancy, 0);
        let connection = rusqlite::Connection::open(database).expect("inspect closed Store");
        let committed: i64 = connection
            .query_row("SELECT COUNT(*) FROM gateway_live_events", [], |row| {
                row.get(0)
            })
            .expect("retained event count");
        assert_eq!(committed, 0, "root fallback must not append an event");

        gateway.event_ingress.close();
        gateway.supervisor.close_infrastructure();
        gateway.supervisor.wait_for_infrastructure().await;
    }

    #[tokio::test]
    async fn retained_event_ingress_stays_within_the_persistence_budget() {
        const BATCH_COUNT: usize = 8;
        const EVENTS_PER_BATCH: usize = 32;

        let budgets: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../non-functional-budgets.json"
        )))
        .expect("non-functional budgets");
        let gateway_budget = budgets
            .pointer("/gateway")
            .and_then(serde_json::Value::as_object)
            .expect("Gateway non-functional budget");
        let maximum = gateway_budget["maximum"]
            .as_object()
            .expect("Gateway maximum budget");
        let maximum_value = |name: &str| {
            maximum[name]
                .as_u64()
                .unwrap_or_else(|| panic!("missing retained-event budget `{name}`"))
        };
        let temp = tempfile::tempdir().expect("tempdir");
        let (application, gateway, durability) = compose_test_framework(&temp).await;
        let turn_id = "turn-retained-event-budget";
        let sink = test_ingress_sink(&gateway, None, turn_id).await;
        let storage_before = application.operational_snapshot().storage;
        let total_started = Instant::now();
        let mut batch_commit_latency_micros = Vec::with_capacity(BATCH_COUNT);
        for batch in 0..BATCH_COUNT {
            let batch_started = Instant::now();
            for offset in 0..EVENTS_PER_BATCH {
                sink.emit(GatewayEvent::TurnQueued {
                    thread_id: None,
                    turn_id: turn_id.to_string(),
                    queue_position: batch * EVENTS_PER_BATCH + offset + 1,
                })
                .expect("retained event admission");
            }
            gateway
                .event_ingress
                .fence()
                .await
                .expect("retained event batch fence");
            batch_commit_latency_micros
                .push(u64::try_from(batch_started.elapsed().as_micros()).unwrap_or(u64::MAX));
        }
        let total_micros = u64::try_from(total_started.elapsed().as_micros()).unwrap_or(u64::MAX);
        let event_count = BATCH_COUNT * EVENTS_PER_BATCH;
        let mut ordered_batch_commit_latency_micros = batch_commit_latency_micros.clone();
        ordered_batch_commit_latency_micros.sort_unstable();
        let batch_commit_latency_p50_ms = percentile(&ordered_batch_commit_latency_micros, 50)
            .expect("batch p50 sample")
            .div_ceil(1_000);
        let batch_commit_latency_p95_ms = percentile(&ordered_batch_commit_latency_micros, 95)
            .expect("batch p95 sample")
            .div_ceil(1_000);
        let batch_commit_latency_p99_ms = percentile(&ordered_batch_commit_latency_micros, 99)
            .expect("batch p99 sample")
            .div_ceil(1_000);
        let event_commit_latency_micros = gateway.event_ingress.commit_latency_samples_micros();
        assert_eq!(event_commit_latency_micros.len(), event_count);
        let mut ordered_event_commit_latency_micros = event_commit_latency_micros.clone();
        ordered_event_commit_latency_micros.sort_unstable();
        let event_commit_latency_p50_micros =
            percentile(&ordered_event_commit_latency_micros, 50).expect("event p50 sample");
        let event_commit_latency_p95_micros =
            percentile(&ordered_event_commit_latency_micros, 95).expect("event p95 sample");
        let event_commit_latency_p99_micros =
            percentile(&ordered_event_commit_latency_micros, 99).expect("event p99 sample");
        let micros_per_event = total_micros.div_ceil(event_count as u64);
        let peak_ingress_queue_depth = gateway.event_ingress.peak_occupancy();
        let storage_after = application.operational_snapshot().storage;
        let sqlite_busy_operations = storage_after
            .busy_operations
            .saturating_sub(storage_before.busy_operations);

        let diagnostics = gateway.event_ingress_diagnostics();
        assert_eq!(diagnostics.accepted, event_count as u64);
        assert_eq!(diagnostics.processed, event_count as u64);
        assert_eq!(diagnostics.committed, event_count as u64);
        assert_eq!(diagnostics.failed, 0);
        assert_eq!(diagnostics.rejected, 0);
        assert_eq!(diagnostics.occupancy, 0);
        assert_eq!(
            durability
                .list_gateway_live_events_after(0, event_count)
                .await
                .expect("retained events")
                .len(),
            event_count
        );

        if let Some(root) = std::env::var_os("PSYCHEVO_CI_ARTIFACT_ROOT") {
            let output = std::path::PathBuf::from(root).join("non-functional");
            std::fs::create_dir_all(&output).expect("non-functional evidence directory");
            let report = serde_json::json!({
                "schemaVersion": 1,
                "scope": "gateway-retained-events",
                "fixture": {
                    "batchCount": BATCH_COUNT,
                    "eventsPerBatch": EVENTS_PER_BATCH,
                    "eventCount": event_count,
                },
                "observed": {
                    "retainedEventBatchCommitLatencyP50Ms": batch_commit_latency_p50_ms,
                    "retainedEventBatchCommitLatencyP95Ms": batch_commit_latency_p95_ms,
                    "retainedEventBatchCommitLatencyP99Ms": batch_commit_latency_p99_ms,
                    "retainedEventCommitLatencyP50Micros": event_commit_latency_p50_micros,
                    "retainedEventCommitLatencyP95Micros": event_commit_latency_p95_micros,
                    "retainedEventCommitLatencyP99Micros": event_commit_latency_p99_micros,
                    "retainedEventMicrosPerEvent": micros_per_event,
                    "retainedEventPeakIngressQueueDepth": peak_ingress_queue_depth,
                    "retainedEventSqliteBusyOperations": sqlite_busy_operations,
                },
                "samples": {
                    "batchCommitLatencyMicros": batch_commit_latency_micros,
                    "eventCommitLatencyMicros": event_commit_latency_micros,
                    "totalMicros": total_micros,
                },
                "baseline": gateway_budget["baseline"].clone(),
                "maximum": gateway_budget["maximum"].clone(),
            });
            std::fs::write(
                output.join("gateway-retained-events.json"),
                serde_json::to_vec_pretty(&report).expect("serialize retained-event evidence"),
            )
            .expect("write retained-event evidence");
        }
        for (name, observed) in [
            (
                "retainedEventBatchCommitLatencyP50Ms",
                batch_commit_latency_p50_ms,
            ),
            (
                "retainedEventBatchCommitLatencyP95Ms",
                batch_commit_latency_p95_ms,
            ),
            (
                "retainedEventBatchCommitLatencyP99Ms",
                batch_commit_latency_p99_ms,
            ),
            (
                "retainedEventCommitLatencyP50Micros",
                event_commit_latency_p50_micros,
            ),
            (
                "retainedEventCommitLatencyP95Micros",
                event_commit_latency_p95_micros,
            ),
            (
                "retainedEventCommitLatencyP99Micros",
                event_commit_latency_p99_micros,
            ),
            ("retainedEventMicrosPerEvent", micros_per_event),
            (
                "retainedEventPeakIngressQueueDepth",
                peak_ingress_queue_depth as u64,
            ),
            ("retainedEventSqliteBusyOperations", sqlite_busy_operations),
        ] {
            let maximum = maximum_value(name);
            assert!(
                observed <= maximum,
                "retained-event metric {name} observed {observed}, exceeding {maximum}"
            );
        }
        assert!(batch_commit_latency_p50_ms <= batch_commit_latency_p95_ms);
        assert!(batch_commit_latency_p95_ms <= batch_commit_latency_p99_ms);
        assert!(event_commit_latency_p50_micros <= event_commit_latency_p95_micros);
        assert!(event_commit_latency_p95_micros <= event_commit_latency_p99_micros);
    }

    #[tokio::test]
    async fn snapshot_retry_after_lost_commit_ack_keeps_one_revision_change() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (application, gateway, durability) = compose_test_framework(&temp).await;
        let thread_id = application
            .client()
            .start_thread(StartThreadRequest::new(Path::new(".")))
            .await
            .expect("thread")
            .id()
            .to_string();
        let sink = test_ingress_sink(&gateway, Some(&thread_id), "turn-snapshot-retry").await;
        let (initial, entry_id, block_id) =
            initial_assistant_entry(&thread_id, "turn-snapshot-retry");
        sink.emit(initial).expect("initial entry admission");
        gateway
            .event_ingress
            .fence()
            .await
            .expect("initial snapshot fence");
        gateway.event_ingress.retry_after_next_commit();

        sink.emit(GatewayEvent::EntryBlockTextDelta {
            thread_id: Some(thread_id),
            turn_id: "turn-snapshot-retry".to_string(),
            entry_id,
            block_id,
            text: " retry".to_string(),
            updated_at_ms: 2,
        })
        .expect("delta admission");
        gateway
            .event_ingress
            .fence()
            .await
            .expect("retrying snapshot fence");

        let snapshots = durability
            .list_gateway_live_snapshots(10)
            .await
            .expect("snapshots");
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].revision, 2);
        let diagnostics = gateway.event_ingress_diagnostics();
        assert_eq!(diagnostics.retried, 1);
        assert_eq!(diagnostics.committed, 2);
        assert_eq!(diagnostics.failed, 0);
        assert!(matches!(
            diagnostics.last_commit,
            Some(GatewayEventCommitEvidence::LiveSnapshot { revision: 2, .. })
        ));
    }

    #[tokio::test]
    async fn pending_snapshot_flush_failure_fails_the_fence_and_releases_occupancy() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (application, gateway, _durability) = compose_test_framework(&temp).await;
        let thread_id = application
            .client()
            .start_thread(StartThreadRequest::new(Path::new(".")))
            .await
            .expect("thread")
            .id()
            .to_string();
        let sink = test_ingress_sink(&gateway, Some(&thread_id), "turn-flush-failure").await;
        let (initial, entry_id, block_id) =
            initial_assistant_entry(&thread_id, "turn-flush-failure");
        sink.emit(initial).expect("initial entry admission");
        gateway
            .event_ingress
            .fence()
            .await
            .expect("initial snapshot fence");
        let fault_connection =
            rusqlite::Connection::open(temp.path().join("state.db")).expect("fault connection");
        fault_connection
            .execute_batch(
                r#"
                CREATE TRIGGER fail_gateway_live_snapshot_update
                BEFORE UPDATE ON gateway_live_snapshots
                BEGIN
                    SELECT RAISE(FAIL, 'injected retained snapshot failure');
                END
                "#,
            )
            .expect("install snapshot fault");
        sink.emit(GatewayEvent::EntryBlockTextDelta {
            thread_id: Some(thread_id),
            turn_id: "turn-flush-failure".to_string(),
            entry_id,
            block_id,
            text: " pending".to_string(),
            updated_at_ms: 2,
        })
        .expect("delta admission");

        let error = gateway
            .event_ingress
            .fence()
            .await
            .expect_err("injected write failure must fail the snapshot fence");
        assert!(error.to_string().contains("entryBlockTextDelta"));
        let diagnostics = gateway.event_ingress_diagnostics();
        assert_eq!(diagnostics.processed, 2);
        assert_eq!(diagnostics.committed, 1);
        assert_eq!(diagnostics.failed, 1);
        assert_eq!(diagnostics.retried, 0);
        assert_eq!(diagnostics.occupancy, 0);
    }

    #[tokio::test]
    async fn closing_ingress_drains_a_pending_snapshot_before_worker_shutdown() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (application, gateway, durability) = compose_test_framework(&temp).await;
        let thread_id = application
            .client()
            .start_thread(StartThreadRequest::new(Path::new(".")))
            .await
            .expect("thread")
            .id()
            .to_string();
        let sink = test_ingress_sink(&gateway, Some(&thread_id), "turn-drain").await;
        let (initial, entry_id, block_id) = initial_assistant_entry(&thread_id, "turn-drain");
        sink.emit(initial).expect("initial entry admission");
        gateway
            .event_ingress
            .fence()
            .await
            .expect("initial snapshot fence");
        sink.emit(GatewayEvent::EntryBlockTextDelta {
            thread_id: Some(thread_id),
            turn_id: "turn-drain".to_string(),
            entry_id,
            block_id,
            text: " drained".to_string(),
            updated_at_ms: 2,
        })
        .expect("delta admission");
        gateway.event_ingress.close();
        gateway.supervisor.close_infrastructure();
        gateway.supervisor.wait_for_infrastructure().await;

        let diagnostics = gateway.event_ingress_diagnostics();
        assert_eq!(diagnostics.committed, 2);
        assert_eq!(diagnostics.failed, 0);
        assert_eq!(diagnostics.occupancy, 0);
        assert_eq!(
            durability
                .list_gateway_live_snapshots(10)
                .await
                .expect("drained snapshot")[0]
                .revision,
            2
        );
    }

    #[tokio::test]
    async fn local_delivery_precedes_durability_and_fence_reports_write_failure() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let (application, gateway, durability) = compose_test_framework(&temp).await;
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(&cwd))
            .await
            .expect("thread");
        let thread_id = thread.id().to_string();
        let activity = gateway
            .claim_durable_gateway_activity(DurableGatewayActivityClaim {
                activity_id: "turn-ingress",
                thread_id: Some(&thread_id),
                source_key: None,
                turn_id: Some("turn-ingress"),
                kind: GatewayActivityKind::Turn,
                owner_surface: Some("test"),
                queued_turns: 0,
                intent: None,
            })
            .await
            .expect("activity");

        let local_deliveries = Arc::new(AtomicUsize::new(0));
        let local_deliveries_for_sink = local_deliveries.clone();
        let local_sink = GatewayEventEmitter::new(move |_| {
            local_deliveries_for_sink.fetch_add(1, Ordering::SeqCst);
        });
        let sink = gateway
            .wrap_gateway_event_sink(
                Some(local_sink),
                Some(activity),
                Some("thread:test".to_string()),
                Some("turn-ingress".to_string()),
            )
            .expect("wrapped sink");

        sink.emit(GatewayEvent::TurnStarted {
            thread_id: Some(thread_id.clone()),
            turn_id: "turn-ingress".to_string(),
            selected_skills: Vec::new(),
        })
        .expect("accepted ingress event");
        assert_eq!(
            local_deliveries.load(Ordering::SeqCst),
            1,
            "local observers must run before asynchronous durable relay completion"
        );

        gateway
            .event_ingress
            .fence()
            .await
            .expect("retained-live completion fence");
        let durable_events = durability
            .list_gateway_live_events_after(0, 10)
            .await
            .expect("durable events");
        assert_eq!(durable_events.len(), 1);
        let diagnostics = gateway.event_ingress.diagnostics();
        assert_eq!(diagnostics.accepted, 1);
        assert_eq!(diagnostics.processed, 1);
        assert_eq!(diagnostics.committed, 1);
        assert_eq!(diagnostics.rejected, 0);

        thread
            .start_turn(
                psychevo::TurnRequest::new("fixture terminal")
                    .with_requested_turn_id("turn-ingress".to_string()),
            )
            .await
            .expect("accepted fixture terminal")
            .wait()
            .await
            .expect("authoritative Framework terminal");
        let terminal_completed_at_ms = application
            .client()
            .framework_turn_terminal_evidence("turn-ingress")
            .await
            .expect("terminal evidence")
            .expect("durable terminal")
            .completed_at_ms;

        sink.emit(GatewayEvent::TurnCompleted {
            thread_id: Some(thread_id.clone()),
            turn_id: "turn-ingress".to_string(),
            turn: GatewayTurn {
                id: "turn-ingress".to_string(),
                thread_id: Some(thread_id.clone()),
                status: GatewayTurnStatus::Completed,
                outcome: Some("normal".to_string()),
                error: None,
                started_at_ms: Some(1),
                completed_at_ms: Some(terminal_completed_at_ms),
            },
            committed_entries: Vec::new(),
        })
        .expect("terminal relay admission");
        gateway
            .event_ingress
            .fence()
            .await
            .expect("terminal retained-live completion fence");
        let diagnostics = gateway.event_ingress.diagnostics();
        assert_eq!(diagnostics.accepted, 2);
        assert_eq!(
            diagnostics.processed, 2,
            "the retained-live worker must account for every accepted envelope"
        );
        assert_eq!(diagnostics.committed, 2);
        assert!(matches!(
            diagnostics.last_commit,
            Some(GatewayEventCommitEvidence::FrameworkTerminal {
                ref turn_id,
                thread_id: ref evidence_thread_id,
                ref status,
                completed_at_ms,
                ..
            }) if turn_id.as_ref() == "turn-ingress"
                && evidence_thread_id.as_ref() == thread_id.as_str()
                && status.as_ref() == "completed"
                && completed_at_ms == terminal_completed_at_ms
        ));

        let fault_connection =
            rusqlite::Connection::open(temp.path().join("state.db")).expect("fault connection");
        fault_connection
            .execute_batch(
                r#"
                CREATE TRIGGER fail_gateway_live_event_insert
                BEFORE INSERT ON gateway_live_events
                BEGIN
                    SELECT RAISE(FAIL, 'injected retained event failure');
                END
                "#,
            )
            .expect("install retained-event fault");
        sink.emit(GatewayEvent::TurnQueued {
            thread_id: Some(thread_id.clone()),
            turn_id: "turn-ingress-failed-relay".to_string(),
            queue_position: 1,
        })
        .expect("retained-live failure happens after bounded admission");
        let fence_error = gateway
            .event_ingress
            .fence()
            .await
            .expect_err("failed retained-live write must fail the completion fence");
        let fence_error = fence_error.to_string();
        assert!(fence_error.contains("activity_id=turn-ingress"));
        assert!(fence_error.contains("turn_id=turn-ingress-failed-relay"));
        assert!(fence_error.contains("event_kind=turnQueued"));
        let diagnostics = gateway.event_ingress.diagnostics();
        assert_eq!(diagnostics.processed, 3);
        assert_eq!(diagnostics.committed, 2);
        assert_eq!(diagnostics.failed, 1);
        let first_failure = diagnostics.first_failure.expect("failure context");
        assert!(first_failure.contains("activity_id=turn-ingress"));
        assert!(first_failure.contains("turn_id=turn-ingress-failed-relay"));
        assert!(first_failure.contains("event_kind=turnQueued"));

        gateway.event_ingress.close();
        let error = sink
            .emit(GatewayEvent::TurnQueued {
                thread_id: Some(thread_id),
                turn_id: "turn-ingress-queued".to_string(),
                queue_position: 1,
            })
            .expect_err("closed durable ingress");
        assert!(error.to_string().contains("ingress is closed"));
        assert_eq!(
            local_deliveries.load(Ordering::SeqCst),
            4,
            "durable relay rejection must not suppress local delivery"
        );
        assert_eq!(gateway.event_ingress.diagnostics().rejected, 1);

        let shutdown_error = gateway
            .shutdown_activity_runtime(false)
            .await
            .expect_err("retained-live failure must be included in shutdown evidence");
        assert!(shutdown_error.to_string().contains("retained-live relay"));
    }

    #[tokio::test]
    async fn ingress_limit_rejects_capacity_plus_one_with_retry_context() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let database_path = temp.path().join("state.db");
        let home = temp.path().join("home");
        std::fs::create_dir_all(&home).expect("home");
        let invalid = GatewayApplication::open_with_limits(
            home.clone(),
            database_path.clone(),
            None,
            BTreeMap::new(),
            GatewayLimits {
                event_ingress_capacity: 0,
                ..GatewayLimits::default()
            },
        )
        .await
        .expect_err("zero ingress capacity must be rejected");
        assert!(invalid.to_string().contains("greater than zero"));

        let runtime = GatewayApplication::open_with_limits(
            home,
            database_path,
            None,
            BTreeMap::new(),
            GatewayLimits {
                event_ingress_capacity: 1,
                ..GatewayLimits::default()
            },
        )
        .await
        .expect("bounded Gateway");
        let gateway = runtime.gateway().clone();
        let thread_id = runtime
            .client()
            .start_thread(StartThreadRequest::new(&cwd))
            .await
            .expect("thread")
            .id()
            .to_string();
        let activity = gateway
            .claim_durable_gateway_activity(DurableGatewayActivityClaim {
                activity_id: "turn-capacity",
                thread_id: Some(&thread_id),
                source_key: None,
                turn_id: Some("turn-capacity"),
                kind: GatewayActivityKind::Turn,
                owner_surface: Some("test"),
                queued_turns: 0,
                intent: None,
            })
            .await
            .expect("activity");
        gateway.event_ingress.pause_worker();
        let sink = gateway
            .wrap_gateway_event_sink(
                None,
                Some(activity),
                Some("thread:test".to_string()),
                Some("turn-capacity".to_string()),
            )
            .expect("wrapped sink");

        sink.emit(GatewayEvent::TurnStarted {
            thread_id: Some(thread_id.clone()),
            turn_id: "turn-capacity".to_string(),
            selected_skills: Vec::new(),
        })
        .expect("first event fills capacity");
        let error = sink
            .emit(GatewayEvent::TurnQueued {
                thread_id: Some(thread_id),
                turn_id: "turn-overload".to_string(),
                queue_position: 1,
            })
            .expect_err("capacity plus one must be rejected");
        let overload = error.overload().expect("typed overload context");
        assert_eq!(overload.occupancy, 1);
        assert_eq!(overload.limit, 1);
        assert!(overload.retryable);
        let oldest = overload.oldest.as_ref().expect("oldest accepted event");
        assert_eq!(oldest.activity_id, "turn-capacity");
        assert_eq!(oldest.turn_id.as_deref(), Some("turn-capacity"));
        assert_eq!(oldest.event_kind, "turnStarted");

        let diagnostics = gateway.event_ingress_diagnostics();
        assert_eq!(diagnostics.occupancy, 1);
        assert_eq!(diagnostics.limit, 1);
        assert_eq!(diagnostics.rejected, 1);
        let diagnostic_oldest = diagnostics
            .oldest
            .as_ref()
            .expect("diagnostic oldest event");
        assert_eq!(diagnostic_oldest.activity_id, oldest.activity_id);
        assert_eq!(diagnostic_oldest.turn_id, oldest.turn_id);
        assert_eq!(diagnostic_oldest.event_kind, oldest.event_kind);
        assert!(diagnostic_oldest.age_ms >= oldest.age_ms);

        gateway.event_ingress.resume_worker();
        gateway
            .event_ingress
            .fence()
            .await
            .expect("accepted event commits before the fence");
        let diagnostics = gateway.event_ingress_diagnostics();
        assert_eq!(diagnostics.occupancy, 0);
        assert_eq!(diagnostics.committed, 1);
        assert_eq!(diagnostics.processed, 1);
        assert!(diagnostics.oldest.is_none());
    }

    #[tokio::test]
    async fn lost_shell_activity_generation_cancels_the_runner_and_is_reported() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (_application, gateway, durability) = compose_test_framework(&temp).await;
        let activity = gateway
            .claim_durable_gateway_activity(DurableGatewayActivityClaim {
                activity_id: "shell-lease-loss",
                thread_id: None,
                source_key: Some("source:test"),
                turn_id: Some("shell-lease-loss"),
                kind: GatewayActivityKind::Shell,
                owner_surface: Some("test"),
                queued_turns: 0,
                intent: None,
            })
            .await
            .expect("activity");
        let lease_lost = gateway.track_shell_activity(activity.clone());
        assert!(
            durability
                .finish_gateway_activity(
                    &activity.activity_id,
                    &activity.owner_id,
                    activity.generation,
                    GatewayActivityTerminalStatus::Completed,
                )
                .await
                .expect("terminal transition")
        );

        gateway.refresh_shell_activity_leases().await;

        assert!(lease_lost.is_cancelled());
        assert_eq!(gateway.shell_activity_diagnostics().failed_operations, 1);
        let shutdown_error = gateway
            .shutdown_activity_runtime(false)
            .await
            .expect_err("lease loss must be reported during shutdown");
        assert!(
            shutdown_error
                .to_string()
                .contains("lost its owner or generation")
        );
    }

    #[tokio::test]
    async fn shell_heartbeat_storage_error_cancels_every_runner_and_is_reported() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (_application, gateway, _durability) = compose_test_framework(&temp).await;
        let activity = gateway
            .claim_durable_gateway_activity(DurableGatewayActivityClaim {
                activity_id: "shell-heartbeat-error",
                thread_id: None,
                source_key: Some("source:test"),
                turn_id: Some("shell-heartbeat-error"),
                kind: GatewayActivityKind::Shell,
                owner_surface: Some("test"),
                queued_turns: 0,
                intent: None,
            })
            .await
            .expect("activity");
        let lease_lost = gateway.track_shell_activity(activity.clone());
        let fault_connection =
            rusqlite::Connection::open(temp.path().join("state.db")).expect("fault connection");
        fault_connection
            .execute_batch(
                r#"
                CREATE TRIGGER fail_gateway_activity_heartbeat
                BEFORE UPDATE OF lease_expires_at_ms ON gateway_activities
                BEGIN
                    SELECT RAISE(FAIL, 'injected heartbeat failure');
                END
                "#,
            )
            .expect("install heartbeat fault");

        gateway.refresh_shell_activity_leases().await;

        assert!(lease_lost.is_cancelled());
        let diagnostics = gateway.shell_activity_diagnostics();
        assert_eq!(diagnostics.heartbeat_transactions, 1);
        assert_eq!(diagnostics.lease_cancellations, 1);
        assert!(
            diagnostics
                .first_failure
                .as_deref()
                .is_some_and(|failure| failure.contains("heartbeat transaction failed"))
        );
        gateway.untrack_shell_activity(&activity.activity_id);
        let shutdown_error = gateway
            .shutdown_activity_runtime(false)
            .await
            .expect_err("heartbeat error must be shutdown evidence");
        assert!(
            shutdown_error
                .to_string()
                .contains("heartbeat transaction failed")
        );
    }

    #[tokio::test]
    async fn stale_heartbeat_snapshot_does_not_cancel_a_completed_shell() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (_application, gateway, durability) = compose_test_framework(&temp).await;
        let activity = gateway
            .claim_durable_gateway_activity(DurableGatewayActivityClaim {
                activity_id: "shell-completed-before-heartbeat-result",
                thread_id: None,
                source_key: Some("source:test"),
                turn_id: Some("shell-completed-before-heartbeat-result"),
                kind: GatewayActivityKind::Shell,
                owner_surface: Some("test"),
                queued_turns: 0,
                intent: None,
            })
            .await
            .expect("activity");
        let lease_lost = gateway.track_shell_activity(activity.clone());
        let stale_snapshot = gateway
            .shell_activity_runtime
            .activities
            .lock()
            .expect("tracked activity")
            .get(&activity.activity_id)
            .expect("tracked shell")
            .clone();

        gateway.untrack_shell_activity(&activity.activity_id);
        assert!(
            durability
                .finish_gateway_activity(
                    &activity.activity_id,
                    &activity.owner_id,
                    activity.generation,
                    GatewayActivityTerminalStatus::Completed,
                )
                .await
                .expect("finish activity")
        );

        assert!(!gateway.cancel_shell_activity_if_still_tracked(&stale_snapshot));
        assert!(!lease_lost.is_cancelled());
        let diagnostics = gateway.shell_activity_diagnostics();
        assert_eq!(diagnostics.lease_cancellations, 0);
        assert_eq!(diagnostics.failed_operations, 0);
        gateway
            .shutdown_activity_runtime(false)
            .await
            .expect("shutdown");
    }

    #[tokio::test]
    async fn one_dispatcher_and_one_manual_heartbeat_transaction_cover_the_full_shell_limit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let (application, gateway, durability) = compose_test_framework(&temp).await;
        gateway.use_manual_shell_activity_scheduler();
        let mut activities = Vec::new();
        for index in 0..64 {
            let activity_id = format!("shell-{index}");
            let source_key = format!("source:{index}");
            let activity = gateway
                .claim_durable_gateway_activity(DurableGatewayActivityClaim {
                    activity_id: &activity_id,
                    thread_id: None,
                    source_key: Some(&source_key),
                    turn_id: Some(&activity_id),
                    kind: GatewayActivityKind::Shell,
                    owner_surface: Some("test"),
                    queued_turns: 0,
                    intent: None,
                })
                .await
                .expect("activity");
            gateway.track_shell_activity(activity.clone());
            activities.push(activity);
        }
        let admissions = (0..64)
            .map(|_| {
                gateway
                    .supervisor
                    .acquire_shell_activity_admission()
                    .expect("full-limit admission")
            })
            .collect::<Vec<_>>();

        let before = gateway.shell_activity_diagnostics();
        assert_eq!(before.active_activities, 64);
        assert_eq!(before.admitted_activities, 64);
        assert_eq!(before.queued_activities, 0);
        assert_eq!(before.overload_rejections, 0);
        assert_eq!(before.dispatcher_tasks_started, 1);
        assert_eq!(before.control_poll_ticks, 0);
        assert_eq!(gateway.supervisor.infrastructure_task_count(), 1);
        let store_before = application.operational_snapshot().storage;
        gateway.refresh_shell_activity_leases().await;
        let store_after = application.operational_snapshot().storage;
        let after = gateway.shell_activity_diagnostics();
        assert_eq!(after.active_activities, 64);
        assert_eq!(after.admitted_activities, 64);
        assert_eq!(after.dispatcher_tasks_started, 1);
        assert_eq!(after.control_poll_ticks, 0);
        assert_eq!(after.lease_cancellations, 0);
        assert_eq!(after.failed_operations, 0);
        assert_eq!(gateway.supervisor.infrastructure_task_count(), 1);
        assert_eq!(
            after.heartbeat_transactions,
            before.heartbeat_transactions + 1,
            "one tick must use one batch transaction independent of activity count"
        );
        assert_eq!(
            store_after.completed_operations,
            store_before.completed_operations + 1,
            "the full heartbeat batch must be one observed SQLite operation"
        );
        assert_eq!(store_after.busy_operations, store_before.busy_operations);
        assert_eq!(
            store_after.failed_operations,
            store_before.failed_operations
        );
        assert_eq!(store_after.in_flight_operations, 0);

        let rejected = configured_shell_request(
            &temp,
            &cwd,
            GatewaySource::new("test", "global-overload").persistent(),
            "printf rejected",
        );
        let error = gateway
            .send_shell(rejected)
            .await
            .expect_err("sixty-fifth Shell must be rejected before execution");
        let overload = error.structured_data().expect("structured overload");
        assert_eq!(overload["kind"], "gateway_overloaded");
        assert_eq!(overload["scope"], "activity");
        assert_eq!(overload["limit"], 64);
        assert_eq!(overload["occupancy"], 64);
        assert_eq!(overload["retryable"], true);
        assert_eq!(overload["oldestQueuedAgeMs"], 0);
        assert_eq!(overload["threadId"], serde_json::Value::Null);
        assert_eq!(overload["sourceKey"], "test:global-overload");
        assert_eq!(overload["activeActivityId"], serde_json::Value::Null);
        assert_eq!(overload["turnId"], serde_json::Value::Null);
        let rejected_activity_id = Uuid::parse_str(
            overload["activityId"]
                .as_str()
                .expect("rejected Shell activity id"),
        )
        .expect("stable UUID Shell activity id");
        assert_eq!(rejected_activity_id.get_version_num(), 7);
        assert!(
            durability
                .gateway_activity(&rejected_activity_id.to_string())
                .await
                .expect("rejected activity lookup")
                .is_none(),
            "overload rejection must precede the durable activity claim"
        );
        let overloaded = gateway.shell_activity_diagnostics();
        assert_eq!(overloaded.admitted_activities, 64);
        assert_eq!(overloaded.queued_activities, 0);
        assert_eq!(overloaded.overload_rejections, 1);
        drop(admissions);

        for activity in activities {
            gateway.untrack_shell_activity(&activity.activity_id);
            assert!(
                durability
                    .finish_gateway_activity(
                        &activity.activity_id,
                        &activity.owner_id,
                        activity.generation,
                        GatewayActivityTerminalStatus::Completed,
                    )
                    .await
                    .expect("finish fixture")
            );
        }
        gateway
            .shutdown_activity_runtime(false)
            .await
            .expect("shutdown");
    }

    #[tokio::test]
    async fn shell_scheduler_parks_without_tracked_activity_and_track_wakes_foreign_control() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (application, gateway, durability) = compose_test_framework(&temp).await;
        let initial = gateway
            .claim_durable_gateway_activity(DurableGatewayActivityClaim {
                activity_id: "shell-idle-initial",
                thread_id: None,
                source_key: Some("source:idle-initial"),
                turn_id: Some("shell-idle-initial"),
                kind: GatewayActivityKind::Shell,
                owner_surface: Some("test"),
                queued_turns: 0,
                intent: None,
            })
            .await
            .expect("initial activity");
        gateway.track_shell_activity(initial.clone());
        gateway.untrack_shell_activity(&initial.activity_id);
        assert!(
            durability
                .finish_gateway_activity(
                    &initial.activity_id,
                    &initial.owner_id,
                    initial.generation,
                    GatewayActivityTerminalStatus::Completed,
                )
                .await
                .expect("finish initial activity")
        );
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }

        tokio::time::pause();
        let idle_before = application
            .operational_snapshot()
            .storage
            .completed_operations;
        for _ in 0..4 {
            tokio::time::advance(Duration::from_secs(5)).await;
            tokio::task::yield_now().await;
        }
        let idle_after = application
            .operational_snapshot()
            .storage
            .completed_operations;
        assert_eq!(
            idle_after, idle_before,
            "control and heartbeat ticks must not touch SQLite after the last Shell completes"
        );
        tokio::time::resume();

        let next = gateway
            .claim_durable_gateway_activity(DurableGatewayActivityClaim {
                activity_id: "shell-idle-next",
                thread_id: None,
                source_key: Some("source:idle-next"),
                turn_id: Some("shell-idle-next"),
                kind: GatewayActivityKind::Shell,
                owner_surface: Some("test"),
                queued_turns: 0,
                intent: None,
            })
            .await
            .expect("next activity");
        let shell = application
            .client()
            .shell_command(ShellCommandRequest::new(temp.path(), "printf unused"))
            .expect("Shell command");
        let control = shell.control();
        gateway.register_active(
            "source:idle-next",
            next.activity_id.clone(),
            Some(ActiveActivityControl::Shell(control.clone())),
            ActiveActivityKind::Shell,
        );
        durability
            .enqueue_gateway_control_command(GatewayControlCommandInput {
                activity_id: &next.activity_id,
                owner_id: gateway.owner_id(),
                command_kind: GatewayControlCommandKind::Interrupt,
                payload: json!({}),
            })
            .await
            .expect("enqueue foreign interrupt");
        let poll_ticks_before = gateway.shell_activity_diagnostics().control_poll_ticks;
        gateway.track_shell_activity(next.clone());
        let real_deadline = std::time::Instant::now() + Duration::from_secs(5);
        while gateway
            .shell_activity_diagnostics()
            .control_commands_applied
            == 0
            && std::time::Instant::now() < real_deadline
        {
            tokio::task::yield_now().await;
        }
        assert!(
            control.is_interrupted(),
            "tracking a Shell must wake the parked dispatcher"
        );
        assert_eq!(
            gateway.shell_activity_diagnostics().control_poll_ticks,
            poll_ticks_before,
            "foreign control must be applied by the track wakeup, before a fallback poll tick"
        );
        gateway.untrack_shell_activity(&next.activity_id);
        assert!(
            durability
                .finish_gateway_activity(
                    &next.activity_id,
                    &next.owner_id,
                    next.generation,
                    GatewayActivityTerminalStatus::Interrupted,
                )
                .await
                .expect("finish next activity")
        );
        gateway
            .shutdown_activity_runtime(false)
            .await
            .expect("shutdown");
    }

    #[tokio::test]
    async fn dropping_shell_caller_keeps_admission_owned_by_the_accepted_activity() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let (_application, gateway, durability) = compose_test_framework(&temp).await;
        let source = GatewaySource::new("test", "dropped-caller").persistent();
        let source_key = source.source_key().0;
        let request = configured_shell_request(&temp, &cwd, source, "sleep 60");
        let caller_gateway = gateway.clone();
        let caller = tokio::spawn(async move { caller_gateway.send_shell(request).await });
        let activity = wait_for_active_shell(&durability, &source_key).await;

        caller.abort();
        let _ = caller.await;
        assert_eq!(gateway.shell_activity_diagnostics().admitted_activities, 1);
        let remaining = (1..64)
            .map(|_| {
                gateway
                    .supervisor
                    .acquire_shell_activity_admission()
                    .expect("remaining capacity")
            })
            .collect::<Vec<_>>();
        assert!(matches!(
            gateway.supervisor.acquire_shell_activity_admission(),
            Err(GatewayActivityAdmissionError::Overloaded {
                limit: 64,
                occupancy: 64,
            })
        ));

        assert!(gateway.interrupt_local_activity(&activity.activity_id));
        drop(remaining);
        let terminal = wait_for_terminal_shell(&durability, &activity.activity_id).await;
        assert_eq!(terminal.status, GatewayActivityState::Interrupted);
        gateway
            .shutdown_activity_runtime(false)
            .await
            .expect("shutdown");
    }

    #[tokio::test]
    async fn unbound_shell_rejects_an_entry_for_another_activity_identity() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (application, gateway, durability) = compose_test_framework(&temp).await;
        let thread_id = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread")
            .id()
            .to_string();
        let activity = gateway
            .claim_durable_gateway_activity(DurableGatewayActivityClaim {
                activity_id: "shell-root",
                thread_id: None,
                source_key: Some("source:shell-root"),
                turn_id: Some("shell-root"),
                kind: GatewayActivityKind::Shell,
                owner_surface: Some("test"),
                queued_turns: 0,
                intent: None,
            })
            .await
            .expect("activity");
        let sink = gateway
            .wrap_gateway_event_sink(None, Some(activity), None, Some("shell-root".to_string()))
            .expect("wrapped sink");
        let event = GatewayLiveProjector::new(None).project_shell_event(
            "shell-other",
            &ShellCommandEvent::Started {
                thread_id: Some(thread_id.clone()),
                command: "printf wrong-root".to_string(),
                started_at_ms: 1,
            },
        );

        sink.emit(event).expect("bounded event admission");
        let error = gateway
            .event_ingress
            .fence()
            .await
            .expect_err("a foreign Shell identity must fail retained-live routing");
        assert!(
            error
                .to_string()
                .contains("has no matching durable activity")
        );
        assert_eq!(
            durability
                .gateway_activity("shell-root")
                .await
                .expect("activity lookup")
                .expect("activity")
                .thread_id,
            None,
            "a rejected entry must not bind its Thread to the root activity"
        );
        let shutdown_error = gateway
            .shutdown_activity_runtime(false)
            .await
            .expect_err("the retained-live failure remains shutdown evidence");
        assert!(shutdown_error.to_string().contains("retained-live relay"));
    }

    #[tokio::test]
    async fn new_thread_shell_binds_its_durable_activity_and_outcome_drives_the_terminal() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let (_application, gateway, durability) = compose_test_framework(&temp).await;

        let completed_source = GatewaySource::new("test", "completed-outcome").persistent();
        let completed_source_key = completed_source.source_key().0;
        let completed_request = configured_shell_request(
            &temp,
            &cwd,
            completed_source,
            "sleep 0.05; printf completed",
        );
        let completed_gateway = gateway.clone();
        let completed_call =
            tokio::spawn(async move { completed_gateway.send_shell(completed_request).await });
        let completed_activity = wait_for_active_shell(&durability, &completed_source_key).await;
        let completed = completed_call
            .await
            .expect("completed Shell task")
            .expect("completed Shell result");
        assert_eq!(completed.result.outcome, ShellCommandOutcome::Completed);
        let completed_thread_id = completed.thread.id.clone();
        let completed_terminal =
            wait_for_terminal_shell(&durability, &completed_activity.activity_id).await;
        assert_eq!(
            completed_terminal.thread_id.as_deref(),
            Some(completed_thread_id.as_str()),
            "the first Shell lifecycle event must bind the created Thread to its durable activity"
        );
        assert_eq!(completed_terminal.status, GatewayActivityState::Completed);

        let failed_source = GatewaySource::new("test", "failed-outcome").persistent();
        let failed_source_key = failed_source.source_key().0;
        let failed_request =
            configured_shell_request(&temp, &cwd, failed_source, "sleep 0.05; exit 7");
        let failed_gateway = gateway.clone();
        let failed_call =
            tokio::spawn(async move { failed_gateway.send_shell(failed_request).await });
        let failed_activity = wait_for_active_shell(&durability, &failed_source_key).await;
        let failed = failed_call
            .await
            .expect("failed Shell task")
            .expect("failed process remains a typed Shell result");
        assert_eq!(failed.result.outcome, ShellCommandOutcome::Failed);
        let failed_thread_id = failed.thread.id.clone();
        let failed_terminal =
            wait_for_terminal_shell(&durability, &failed_activity.activity_id).await;
        assert_eq!(
            failed_terminal.thread_id.as_deref(),
            Some(failed_thread_id.as_str()),
            "failed Shells must retain the same accepted Thread identity"
        );
        assert_eq!(failed_terminal.status, GatewayActivityState::Failed);

        gateway
            .shutdown_activity_runtime(false)
            .await
            .expect("shutdown");
    }

    #[tokio::test]
    async fn shell_queue_diagnostics_read_the_authoritative_lanes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let (_application, gateway, _durability) = compose_test_framework(&temp).await;
        let source = GatewaySource::new("test", "authoritative-queue").persistent();
        let request = configured_shell_request(&temp, &cwd, source, "printf queued");
        let permit = gateway
            .supervisor
            .acquire_shell_activity_admission()
            .expect("queue admission");
        let (responder, _receiver) = oneshot::channel();
        {
            let mut queue = gateway
                .active_queue
                .lock()
                .expect("gateway active queue poisoned");
            let lane = queue
                .activities
                .entry("source:authoritative-queue".to_string())
                .or_default();
            lane.running = true;
            lane.queued
                .push_back(PendingQueuedActivity::Shell(Box::new(PendingQueuedShell {
                    shell_id: "queued-shell".to_string(),
                    queued_at_ms: gateway_now_ms(),
                    request,
                    permit,
                    responder,
                })));
        }

        let queued = gateway.shell_activity_diagnostics();
        assert_eq!(queued.queued_activities, 1);
        assert_eq!(queued.admitted_activities, 1);

        gateway.cancel_active_queue();
        let cleared = gateway.shell_activity_diagnostics();
        assert_eq!(cleared.queued_activities, 0);
        assert_eq!(cleared.admitted_activities, 0);
        gateway
            .shutdown_activity_runtime(false)
            .await
            .expect("shutdown");
    }

    #[tokio::test]
    async fn configured_shell_source_queue_reports_age_and_non_turn_identity_at_capacity() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let queue_limit = 2;
        let (application, gateway, durability) = compose_test_framework_with_limits(
            &temp,
            GatewayLimits {
                shell_queue_limit: queue_limit,
                ..GatewayLimits::default()
            },
        )
        .await;
        let source = GatewaySource::new("test", "bounded-queue").persistent();
        let source_key = source.source_key().0;
        let selector = GatewayThreadSelector::source(source.source_key());
        let thread_id = application
            .client()
            .start_thread(StartThreadRequest::new(&cwd))
            .await
            .expect("bound Thread")
            .id()
            .to_string();
        gateway
            .bind_source_thread(
                &source,
                &thread_id,
                &GatewayBackendInfo {
                    kind: BackendKind::Native,
                    runtime_ref: Some("native".to_string()),
                    native_id: Some(thread_id.clone()),
                },
                None,
            )
            .await
            .expect("source binding");
        let running_request = configured_shell_request(&temp, &cwd, source.clone(), "sleep 60");
        let running_gateway = gateway.clone();
        let running =
            tokio::spawn(async move { running_gateway.send_shell(running_request).await });
        let running_activity = wait_for_active_shell(&durability, &source_key).await;

        let mut queued_callers = Vec::new();
        for index in 0..queue_limit {
            let request = configured_shell_request(
                &temp,
                &cwd,
                source.clone(),
                &format!("printf queued-{index}"),
            );
            let queued_gateway = gateway.clone();
            queued_callers.push(tokio::spawn(async move {
                queued_gateway.send_shell(request).await
            }));
            tokio::time::timeout(Duration::from_secs(5), async {
                while gateway.shell_activity_diagnostics().queued_activities < (index + 1) as u64 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("queue admission timeout");
        }
        {
            let mut queue = gateway
                .active_queue
                .lock()
                .expect("gateway active queue poisoned");
            let lane = queue
                .activities
                .values_mut()
                .find(|state| {
                    state.active_turn_id.as_deref() == Some(&running_activity.activity_id)
                })
                .expect("running source lane");
            let PendingQueuedActivity::Shell(oldest) =
                lane.queued.front_mut().expect("oldest queued Shell");
            oldest.queued_at_ms = gateway_now_ms().saturating_sub(5_000);
        }
        let dropped_queued_caller = queued_callers.remove(0);
        dropped_queued_caller.abort();
        let _ = dropped_queued_caller.await;
        assert_eq!(
            gateway.shell_activity_diagnostics().queued_activities,
            queue_limit as u64
        );
        assert_eq!(
            gateway.shell_activity_diagnostics().admitted_activities,
            queue_limit + 1
        );

        let rejected = configured_shell_request(&temp, &cwd, source, "printf rejected");
        let error = gateway
            .send_shell(rejected)
            .await
            .expect_err("configured queue limit plus one must be rejected");
        let overload = error.structured_data().expect("structured overload");
        assert_eq!(overload["kind"], "gateway_overloaded");
        assert_eq!(overload["scope"], "source");
        assert_eq!(overload["limit"], queue_limit);
        assert_eq!(overload["occupancy"], queue_limit);
        assert_eq!(overload["retryable"], true);
        assert!(overload["oldestQueuedAgeMs"].as_u64().unwrap_or(0) >= 5_000);
        assert_eq!(overload["threadId"], thread_id);
        assert_eq!(overload["sourceKey"], source_key);
        assert_eq!(overload["activeActivityId"], running_activity.activity_id);
        assert_eq!(overload["turnId"], serde_json::Value::Null);
        let rejected_activity_id = Uuid::parse_str(
            overload["activityId"]
                .as_str()
                .expect("rejected Shell activity id"),
        )
        .expect("stable UUID Shell activity id");
        assert_eq!(rejected_activity_id.get_version_num(), 7);
        assert!(
            durability
                .gateway_activity(&rejected_activity_id.to_string())
                .await
                .expect("rejected activity lookup")
                .is_none(),
            "queue rejection must precede the durable activity claim"
        );
        assert_eq!(gateway.shell_activity_diagnostics().overload_rejections, 1);

        assert_eq!(gateway.clear_queue(selector), queue_limit);
        assert_eq!(gateway.shell_activity_diagnostics().queued_activities, 0);
        for caller in queued_callers {
            let error = caller
                .await
                .expect("queued caller task")
                .expect_err("cleared queue result");
            assert!(error.to_string().contains("queue cleared"));
        }

        running.abort();
        gateway
            .shutdown_activity_runtime(true)
            .await
            .expect("force shutdown");
        assert_eq!(gateway.shell_activity_diagnostics().queued_activities, 0);
    }

    #[tokio::test]
    async fn forced_shell_cancellation_finalizes_interrupted_outside_the_cancellable_future() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let (_application, gateway, durability) = compose_test_framework(&temp).await;
        let source = GatewaySource::new("test", "forced-cancel").persistent();
        let source_key = source.source_key().0;
        let request = configured_shell_request(&temp, &cwd, source, "sleep 60");
        let caller_gateway = gateway.clone();
        let caller = tokio::spawn(async move { caller_gateway.send_shell(request).await });
        let activity = wait_for_active_shell(&durability, &source_key).await;

        gateway
            .shutdown_activity_runtime(true)
            .await
            .expect("forced shutdown drains finalizer");
        let error = caller
            .await
            .expect("caller task")
            .expect_err("forced cancellation result");
        assert!(error.to_string().contains("forced shutdown"));
        let terminal = wait_for_terminal_shell(&durability, &activity.activity_id).await;
        assert_eq!(terminal.status, GatewayActivityState::Interrupted);
        let diagnostics = gateway.shell_activity_diagnostics();
        assert_eq!(diagnostics.active_activities, 0);
        assert_eq!(diagnostics.admitted_activities, 0);
        assert_eq!(diagnostics.failed_operations, 0);
    }

    #[tokio::test]
    async fn shell_finalization_storage_failure_reaches_the_caller_and_shutdown_evidence() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let (_application, gateway, durability) = compose_test_framework(&temp).await;
        let fault_connection =
            rusqlite::Connection::open(temp.path().join("state.db")).expect("fault connection");
        fault_connection
            .execute_batch(
                r#"
                CREATE TRIGGER fail_gateway_activity_terminal_update
                BEFORE UPDATE OF status ON gateway_activities
                WHEN NEW.status IN ('completed', 'failed', 'interrupted')
                BEGIN
                    SELECT RAISE(FAIL, 'injected activity finalization failure');
                END
                "#,
            )
            .expect("install finalization fault");
        let source = GatewaySource::new("test", "finish-failure").persistent();
        let source_key = source.source_key().0;
        let request = configured_shell_request(&temp, &cwd, source, "sleep 60");
        let caller_gateway = gateway.clone();
        let caller = tokio::spawn(async move { caller_gateway.send_shell(request).await });
        let activity = wait_for_active_shell(&durability, &source_key).await;

        assert!(gateway.interrupt_local_activity(&activity.activity_id));
        let error = tokio::time::timeout(Duration::from_secs(5), caller)
            .await
            .expect("Shell caller finalization timeout")
            .expect("caller task")
            .expect_err("finalization failure must reach the Shell caller");
        assert!(error.to_string().contains("finalization failed"));
        let record = durability
            .gateway_activity(&activity.activity_id)
            .await
            .expect("activity query")
            .expect("activity record");
        assert_eq!(record.status, GatewayActivityState::Running);
        let diagnostics = gateway.shell_activity_diagnostics();
        assert_eq!(diagnostics.failed_operations, 1);
        assert!(
            diagnostics
                .first_failure
                .as_deref()
                .is_some_and(|failure| failure.contains("finalization failed"))
        );
        fault_connection
            .execute_batch("DROP TRIGGER fail_gateway_activity_terminal_update")
            .expect("remove finalization fault");
        drop(fault_connection);

        let shutdown_error = gateway
            .shutdown_activity_runtime(false)
            .await
            .expect_err("finalization failure must remain shutdown evidence");
        assert!(shutdown_error.to_string().contains("finalization failed"));
    }

    #[tokio::test]
    async fn panicking_shell_projection_finalizes_failed_and_is_shutdown_evidence() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let (_application, gateway, durability) = compose_test_framework(&temp).await;
        let source = GatewaySource::new("test", "panic").persistent();
        let source_key = source.source_key().0;
        let mut request = configured_shell_request(&temp, &cwd, source, "sleep 0.1; printf ok");
        let panic_enabled = Arc::new(AtomicBool::new(false));
        let panic_enabled_for_sink = panic_enabled.clone();
        request.event_sink = Some(GatewayEventEmitter::new(move |_| {
            if panic_enabled_for_sink.load(Ordering::Acquire) {
                panic!("injected Shell projection panic");
            }
        }));

        let caller_gateway = gateway.clone();
        let caller = tokio::spawn(async move { caller_gateway.send_shell(request).await });
        let activity = wait_for_active_shell(&durability, &source_key).await;
        panic_enabled.store(true, Ordering::Release);
        let error = caller
            .await
            .expect("Shell caller task")
            .expect_err("panic must become a Shell error");
        assert!(error.to_string().contains("panicked"));
        let terminal = wait_for_terminal_shell(&durability, &activity.activity_id).await;
        assert_eq!(terminal.status, GatewayActivityState::Failed);
        let diagnostics = gateway.shell_activity_diagnostics();
        assert_eq!(diagnostics.active_activities, 0);
        assert_eq!(diagnostics.admitted_activities, 0);
        let panics = gateway.supervisor.panic_summary();
        assert_eq!(panics.count, 1);
        let first_panic = panics.first.expect("structured panic context");
        assert_eq!(first_panic.scope, GatewayTaskScope::Activity);
        assert!(first_panic.name.starts_with("shell:"));
        assert_eq!(
            first_panic.message.as_ref(),
            "injected Shell projection panic"
        );
        let shutdown_error = gateway
            .shutdown_activity_runtime(false)
            .await
            .expect_err("panic must be shutdown evidence");
        assert!(
            shutdown_error
                .to_string()
                .contains("injected Shell projection panic")
        );
    }
}
