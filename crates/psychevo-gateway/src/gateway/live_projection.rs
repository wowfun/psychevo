use std::collections::HashMap;

#[cfg(test)]
use psychevo::application::GatewayActivityKind;
use psychevo::application::{GatewayActivityRecord, GatewayActivityState};

use super::Gateway;
use crate::gateway_now_ms;
use psychevo_gateway_protocol::events_transcript::GatewayEvent;

#[derive(Clone, Debug, Default)]
pub struct GatewayLiveProjectionContext {
    pub activity_id: Option<String>,
    pub owner_id: Option<String>,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub source_key: Option<String>,
    pub lease_expires_at_ms: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct ForeignGatewayLiveEvent {
    pub seq: i64,
    pub context: GatewayLiveProjectionContext,
    pub event: GatewayEvent,
}

#[derive(Clone, Debug)]
pub struct ForeignGatewayLiveEventPage {
    pub next_seq: i64,
    pub scanned_records: usize,
    pub events: Vec<ForeignGatewayLiveEvent>,
}

#[derive(Clone, Debug)]
pub struct GatewayLiveSnapshotObservation {
    pub snapshot_key: String,
    pub revision: i64,
    pub context: GatewayLiveProjectionContext,
    pub event: GatewayEvent,
}

impl Gateway {
    pub async fn latest_live_event_seq(&self) -> psychevo::Result<i64> {
        self.durability.latest_gateway_live_event_seq().await
    }

    pub async fn cleanup_retained_live_projection(&self, before_ms: i64) -> psychevo::Result<()> {
        self.durability
            .cleanup_gateway_live_events_before(before_ms)
            .await?;
        self.durability
            .cleanup_gateway_live_snapshots_before(before_ms)
            .await?;
        Ok(())
    }

    pub async fn poll_foreign_live_events(
        &self,
        after_seq: i64,
        thread_id: Option<&str>,
        limit: usize,
    ) -> psychevo::Result<ForeignGatewayLiveEventPage> {
        let records = self
            .durability
            .list_gateway_live_events_after(after_seq, limit)
            .await?;
        let scanned_records = records.len();
        let next_seq = records.last().map(|record| record.seq).unwrap_or(after_seq);
        let activities = self
            .activities_for_live_records(
                records
                    .iter()
                    .filter_map(|record| record.activity_id.as_ref()),
            )
            .await?;
        let mut events = Vec::with_capacity(records.len());
        for record in records {
            if record.owner_id.as_deref() == Some(self.owner_id()) {
                continue;
            }
            let Ok(event) = serde_json::from_value::<GatewayEvent>(record.event) else {
                continue;
            };
            let context = live_projection_context(
                record.activity_id,
                record.owner_id,
                record.thread_id,
                record.turn_id,
                &event,
                &activities,
            );
            if context.owner_id.as_deref() == Some(self.owner_id())
                || thread_id
                    .is_some_and(|thread_id| context.thread_id.as_deref() != Some(thread_id))
            {
                continue;
            }
            events.push(ForeignGatewayLiveEvent {
                seq: record.seq,
                context,
                event,
            });
        }
        Ok(ForeignGatewayLiveEventPage {
            next_seq,
            scanned_records,
            events,
        })
    }

    pub async fn foreign_live_snapshots(
        &self,
        thread_id: Option<&str>,
        limit: usize,
    ) -> psychevo::Result<Vec<GatewayLiveSnapshotObservation>> {
        self.live_snapshots(thread_id, None, limit, true).await
    }

    pub(crate) async fn live_snapshots_for_thread(
        &self,
        thread_id: &str,
        turn_id: &str,
        limit: usize,
    ) -> psychevo::Result<Vec<GatewayLiveSnapshotObservation>> {
        self.live_snapshots(Some(thread_id), Some(turn_id), limit, false)
            .await
    }

    async fn live_snapshots(
        &self,
        thread_id: Option<&str>,
        turn_id: Option<&str>,
        limit: usize,
        foreign_only: bool,
    ) -> psychevo::Result<Vec<GatewayLiveSnapshotObservation>> {
        let records = match thread_id {
            Some(thread_id) => {
                self.durability
                    .list_gateway_live_snapshots_for_thread(thread_id, turn_id, limit)
                    .await?
            }
            None => self.durability.list_gateway_live_snapshots(limit).await?,
        };
        let activities = self
            .activities_for_live_records(
                records
                    .iter()
                    .filter_map(|record| record.activity_id.as_ref()),
            )
            .await?;
        let now = gateway_now_ms();
        let mut snapshots = Vec::with_capacity(records.len());
        for record in records {
            if foreign_only && record.owner_id.as_deref() == Some(self.owner_id()) {
                continue;
            }
            if let Some(activity_id) = record.activity_id.as_deref()
                && !activities.get(activity_id).is_some_and(|activity| {
                    matches!(
                        activity.status,
                        GatewayActivityState::Running | GatewayActivityState::Queued
                    ) && activity.lease_expires_at_ms >= now
                })
            {
                continue;
            }
            let Ok(event) = serde_json::from_value::<GatewayEvent>(record.event) else {
                continue;
            };
            let context = live_projection_context(
                record.activity_id,
                record.owner_id,
                record.thread_id,
                record.turn_id,
                &event,
                &activities,
            );
            if (foreign_only && context.owner_id.as_deref() == Some(self.owner_id()))
                || thread_id
                    .is_some_and(|thread_id| context.thread_id.as_deref() != Some(thread_id))
            {
                continue;
            }
            snapshots.push(GatewayLiveSnapshotObservation {
                snapshot_key: record.snapshot_key,
                revision: record.revision,
                context,
                event,
            });
        }
        Ok(snapshots)
    }

    async fn activities_for_live_records<'a>(
        &self,
        activity_ids: impl Iterator<Item = &'a String>,
    ) -> psychevo::Result<HashMap<String, GatewayActivityRecord>> {
        let activity_ids = activity_ids.cloned().collect::<Vec<_>>();
        self.durability
            .gateway_activities_by_id(&activity_ids)
            .await
    }
}

fn live_projection_context(
    activity_id: Option<String>,
    owner_id: Option<String>,
    thread_id: Option<String>,
    turn_id: Option<String>,
    event: &GatewayEvent,
    activities: &HashMap<String, GatewayActivityRecord>,
) -> GatewayLiveProjectionContext {
    let activity = activity_id
        .as_deref()
        .and_then(|activity_id| activities.get(activity_id));
    GatewayLiveProjectionContext {
        activity_id,
        owner_id: owner_id.or_else(|| activity.map(|activity| activity.owner_id.clone())),
        thread_id: non_empty(thread_id)
            .or_else(|| gateway_event_thread_id(event))
            .or_else(|| activity.and_then(|activity| activity.thread_id.clone())),
        turn_id: non_empty(turn_id)
            .or_else(|| gateway_event_turn_id(event).map(str::to_string))
            .or_else(|| activity.and_then(|activity| activity.turn_id.clone())),
        source_key: activity.and_then(|activity| activity.source_key.clone()),
        lease_expires_at_ms: activity.map(|activity| activity.lease_expires_at_ms),
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

pub(crate) fn gateway_event_thread_id(event: &GatewayEvent) -> Option<String> {
    match event {
        GatewayEvent::TurnStarted { thread_id, .. }
        | GatewayEvent::TurnQueued { thread_id, .. }
        | GatewayEvent::ActivityChanged { thread_id, .. } => thread_id.clone(),
        GatewayEvent::TurnCompleted {
            thread_id, turn, ..
        } => thread_id.clone().or_else(|| turn.thread_id.clone()),
        GatewayEvent::EntryStarted { entry, .. }
        | GatewayEvent::EntryUpdated { entry, .. }
        | GatewayEvent::EntryCompleted { entry, .. } => {
            (!entry.thread_id.trim().is_empty()).then(|| entry.thread_id.clone())
        }
        GatewayEvent::EntryBlockTextDelta { thread_id, .. } => thread_id.clone(),
        GatewayEvent::ActionRequested { action } | GatewayEvent::ActionUpdated { action } => {
            action.thread_id.clone()
        }
        GatewayEvent::TitleChanged { thread_id, .. } => Some(thread_id.clone()),
        GatewayEvent::ActionResolved { .. }
        | GatewayEvent::ActionCancelled { .. }
        | GatewayEvent::Warning { .. } => None,
    }
}

pub(crate) fn gateway_event_turn_id(event: &GatewayEvent) -> Option<&str> {
    match event {
        GatewayEvent::TurnStarted { turn_id, .. }
        | GatewayEvent::TurnQueued { turn_id, .. }
        | GatewayEvent::TurnCompleted { turn_id, .. }
        | GatewayEvent::EntryStarted { turn_id, .. }
        | GatewayEvent::EntryUpdated { turn_id, .. }
        | GatewayEvent::EntryBlockTextDelta { turn_id, .. }
        | GatewayEvent::EntryCompleted { turn_id, .. } => Some(turn_id.as_str()),
        GatewayEvent::ActionRequested { action } | GatewayEvent::ActionUpdated { action } => {
            action.turn_id.as_deref()
        }
        GatewayEvent::ActivityChanged { activity, .. } => activity.active_turn_id.as_deref(),
        GatewayEvent::ActionResolved { .. }
        | GatewayEvent::ActionCancelled { .. }
        | GatewayEvent::TitleChanged { .. }
        | GatewayEvent::Warning { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use psychevo::application::{
        GatewayActivityClaimInput, GatewayLiveSnapshotInput, StartThreadRequest,
    };
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;
    use crate::composition::GatewayApplication;

    #[tokio::test]
    async fn typed_foreign_live_projection_advances_cursor_and_owns_fallback_and_lease_rules() {
        let temp = tempdir().expect("tempdir");
        let home = temp.path().join("home");
        std::fs::create_dir_all(&home).expect("home");
        let runtime =
            GatewayApplication::open(home, temp.path().join("state.db"), None, Default::default())
                .await
                .expect("test composition");
        let gateway = runtime.gateway().clone();
        let durability = runtime.application().gateway_durability();
        let thread_id = runtime
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("thread")
            .id()
            .to_string();
        let expired_thread_id = runtime
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("expired thread")
            .id()
            .to_string();
        durability
            .claim_gateway_activity(GatewayActivityClaimInput {
                activity_id: "activity-live",
                thread_id: Some(&thread_id),
                source_key: Some("source:live"),
                turn_id: Some("turn-live"),
                kind: GatewayActivityKind::Turn,
                owner_id: "foreign-owner",
                owner_surface: Some("test"),
                lease_expires_at_ms: gateway_now_ms() + 60_000,
                queued_turns: 0,
                superseded_activity_id: None,
                intent: None,
            })
            .await
            .expect("live activity");
        durability
            .claim_gateway_activity(GatewayActivityClaimInput {
                activity_id: "activity-expired",
                thread_id: Some(&expired_thread_id),
                source_key: Some("source:expired"),
                turn_id: Some("turn-expired"),
                kind: GatewayActivityKind::Turn,
                owner_id: "foreign-owner",
                owner_surface: Some("test"),
                lease_expires_at_ms: gateway_now_ms() - 1,
                queued_turns: 0,
                superseded_activity_id: None,
                intent: None,
            })
            .await
            .expect("expired activity");

        durability
            .append_gateway_live_event(
                None,
                Some(gateway.owner_id()),
                Some(&thread_id),
                None,
                None,
                &serde_json::to_value(GatewayEvent::TitleChanged {
                    thread_id: thread_id.clone(),
                    title: Some("local".to_string()),
                    display_title: None,
                })
                .expect("local event"),
            )
            .await
            .expect("append local");
        durability
            .append_gateway_live_event(
                None,
                Some("foreign-owner"),
                None,
                None,
                None,
                &json!({"type": "futureEvent"}),
            )
            .await
            .expect("append malformed");
        let expected_seq = durability
            .append_gateway_live_event(
                Some("activity-live"),
                Some("foreign-owner"),
                None,
                None,
                None,
                &serde_json::to_value(GatewayEvent::Warning {
                    kind: "test".to_string(),
                    message: "fallback".to_string(),
                    source_path: None,
                    suggestion: None,
                })
                .expect("foreign event"),
            )
            .await
            .expect("append foreign")
            .seq;

        let page = gateway
            .poll_foreign_live_events(0, Some(&thread_id), 100)
            .await
            .expect("poll");
        assert_eq!(page.scanned_records, 3);
        assert_eq!(page.next_seq, expected_seq);
        assert_eq!(page.events.len(), 1);
        assert_eq!(page.events[0].seq, expected_seq);
        assert_eq!(
            page.events[0].context.thread_id.as_deref(),
            Some(thread_id.as_str())
        );
        assert_eq!(
            page.events[0].context.source_key.as_deref(),
            Some("source:live")
        );

        for (snapshot_key, activity_id) in [
            ("snapshot-live", "activity-live"),
            ("snapshot-expired", "activity-expired"),
        ] {
            durability
                .upsert_gateway_live_snapshots(&[GatewayLiveSnapshotInput {
                    snapshot_key,
                    activity_id: Some(activity_id),
                    owner_id: Some("foreign-owner"),
                    thread_id: None,
                    turn_id: None,
                    event_kind: "warning",
                    event: serde_json::to_value(GatewayEvent::Warning {
                        kind: "test".to_string(),
                        message: snapshot_key.to_string(),
                        source_path: None,
                        suggestion: None,
                    })
                    .expect("snapshot event"),
                }])
                .await
                .expect("snapshot");
        }
        durability
            .upsert_gateway_live_snapshots(&[GatewayLiveSnapshotInput {
                snapshot_key: "snapshot-local",
                activity_id: None,
                owner_id: Some(gateway.owner_id()),
                thread_id: Some(&thread_id),
                turn_id: Some("turn-live"),
                event_kind: "titleChanged",
                event: serde_json::to_value(GatewayEvent::TitleChanged {
                    thread_id: thread_id.clone(),
                    title: Some("local".to_string()),
                    display_title: None,
                })
                .expect("local snapshot event"),
            }])
            .await
            .expect("local snapshot");

        let snapshots = gateway
            .foreign_live_snapshots(None, 1000)
            .await
            .expect("snapshots");
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].snapshot_key, "snapshot-live");
        assert_eq!(
            snapshots[0].context.thread_id.as_deref(),
            Some(thread_id.as_str())
        );
        assert_eq!(
            snapshots[0].context.lease_expires_at_ms,
            durability
                .gateway_activity("activity-live")
                .await
                .expect("live activity")
                .map(|activity| activity.lease_expires_at_ms)
        );
        runtime.shutdown().await.expect("shutdown");
    }
}
