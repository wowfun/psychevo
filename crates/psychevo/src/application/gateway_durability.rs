use std::collections::HashMap;
use std::fmt;

use serde_json::Value;

use super::Application;
use crate::Result;
use crate::state::StateRuntime;

pub use crate::state::{
    AutomationRunFinishInput, AutomationRunRecord, AutomationRunRecoveryCandidate,
    AutomationRunStatus, AutomationRunTerminalStatus, AutomationTaskInput, AutomationTaskKind,
    AutomationTaskRecord, GatewayActivityClaimInput, GatewayActivityKind, GatewayActivityRecord,
    GatewayActivityState, GatewayActivityTerminalStatus, GatewayChannelOutboxInput,
    GatewayChannelOutboxRecord, GatewayChannelOutboxStatus, GatewayControlCommandInput,
    GatewayControlCommandKind, GatewayControlCommandRecord, GatewayControlCommandStatus,
    GatewayLiveEventCommit, GatewayLiveEventRecord, GatewayLiveSnapshotInput,
    GatewayLiveSnapshotRecord, GatewaySourceBindingRecord, GatewaySourceLaneInput,
    GatewaySourceLaneRecord,
};

/// Gateway-owned durability issued by an [`Application`].
///
/// This capability deliberately exposes only Gateway-owned persistence. It
/// cannot reveal the Framework state runtime, open another database, execute
/// SQL, or issue Framework-owned Thread and Turn lifecycle operations.
#[derive(Clone)]
pub struct GatewayDurability {
    state: StateRuntime,
}

impl fmt::Debug for GatewayDurability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GatewayDurability")
    }
}

impl Application {
    /// Issue a cloneable Gateway durability capability over this Application's
    /// single owned state runtime.
    pub fn gateway_durability(&self) -> GatewayDurability {
        GatewayDurability {
            state: self.inner.state.clone(),
        }
    }
}

impl GatewayDurability {
    pub async fn gateway_source_lane(
        &self,
        source_key: &str,
    ) -> Result<Option<GatewaySourceLaneRecord>> {
        self.state.gateway_source_lane(source_key).await
    }

    pub async fn upsert_gateway_source_lane(
        &self,
        input: GatewaySourceLaneInput<'_>,
    ) -> Result<GatewaySourceLaneRecord> {
        self.state.upsert_gateway_source_lane(input).await
    }

    pub async fn gateway_source_bindings_for_connection_id(
        &self,
        connection_id: &str,
    ) -> Result<Vec<GatewaySourceBindingRecord>> {
        self.state
            .gateway_source_bindings_for_connection_id(connection_id)
            .await
    }

    pub async fn delete_gateway_source_binding(&self, source_key: &str) -> Result<bool> {
        self.state.delete_gateway_source_binding(source_key).await
    }

    pub async fn claim_gateway_activity(
        &self,
        input: GatewayActivityClaimInput<'_>,
    ) -> Result<GatewayActivityRecord> {
        self.state.claim_gateway_activity(input).await
    }

    pub async fn gateway_activity(
        &self,
        activity_id: &str,
    ) -> Result<Option<GatewayActivityRecord>> {
        self.state.gateway_activity(activity_id).await
    }

    pub async fn gateway_activities_by_id(
        &self,
        activity_ids: &[String],
    ) -> Result<HashMap<String, GatewayActivityRecord>> {
        self.state.gateway_activities_by_id(activity_ids).await
    }

    pub async fn active_gateway_activity_for_thread(
        &self,
        thread_id: &str,
    ) -> Result<Option<GatewayActivityRecord>> {
        self.state
            .active_gateway_activity_for_thread(thread_id)
            .await
    }

    pub async fn active_gateway_activity_for_source(
        &self,
        source_key: &str,
    ) -> Result<Option<GatewayActivityRecord>> {
        self.state
            .active_gateway_activity_for_source(source_key)
            .await
    }

    pub async fn active_gateway_activities(&self) -> Result<Vec<GatewayActivityRecord>> {
        self.state.active_gateway_activities().await
    }

    pub async fn update_gateway_activity_thread(
        &self,
        activity_id: &str,
        owner_id: &str,
        generation: i64,
        thread_id: &str,
        lease_expires_at_ms: i64,
    ) -> Result<bool> {
        self.state
            .update_gateway_activity_thread(
                activity_id,
                owner_id,
                generation,
                thread_id,
                lease_expires_at_ms,
            )
            .await
    }

    pub async fn heartbeat_gateway_activities(
        &self,
        owner_id: &str,
        activities: &[(String, i64)],
        lease_expires_at_ms: i64,
    ) -> Result<Vec<String>> {
        self.state
            .heartbeat_gateway_activities(owner_id, activities, lease_expires_at_ms)
            .await
    }

    pub async fn set_gateway_activity_queued_turns(
        &self,
        activity_id: &str,
        queued_turns: usize,
    ) -> Result<bool> {
        self.state
            .set_gateway_activity_queued_turns(activity_id, queued_turns)
            .await
    }

    pub async fn finish_gateway_activity(
        &self,
        activity_id: &str,
        owner_id: &str,
        generation: i64,
        status: GatewayActivityTerminalStatus,
    ) -> Result<bool> {
        self.state
            .finish_gateway_activity(activity_id, owner_id, generation, status)
            .await
    }

    pub async fn enqueue_gateway_control_command(
        &self,
        input: GatewayControlCommandInput<'_>,
    ) -> Result<i64> {
        self.state.enqueue_gateway_control_command(input).await
    }

    pub async fn claim_pending_gateway_control_commands(
        &self,
        owner_id: &str,
        limit: usize,
    ) -> Result<Vec<GatewayControlCommandRecord>> {
        self.state
            .claim_pending_gateway_control_commands(owner_id, limit)
            .await
    }

    pub async fn recover_indeterminate_gateway_control_commands(
        &self,
        now_ms: i64,
    ) -> Result<Vec<GatewayControlCommandRecord>> {
        self.state
            .recover_indeterminate_gateway_control_commands(now_ms)
            .await
    }

    pub async fn mark_gateway_control_command_applied(&self, id: i64) -> Result<bool> {
        self.state.mark_gateway_control_command_applied(id).await
    }

    pub async fn mark_gateway_control_command_failed(&self, id: i64, error: &str) -> Result<bool> {
        self.state
            .mark_gateway_control_command_failed(id, error)
            .await
    }

    pub async fn append_gateway_live_event(
        &self,
        activity_id: Option<&str>,
        owner_id: Option<&str>,
        thread_id: Option<&str>,
        turn_id: Option<&str>,
        idempotency_key: Option<&str>,
        event: &Value,
    ) -> Result<GatewayLiveEventCommit> {
        self.state
            .append_gateway_live_event(
                activity_id,
                owner_id,
                thread_id,
                turn_id,
                idempotency_key,
                event,
            )
            .await
    }

    pub async fn latest_gateway_live_event_seq(&self) -> Result<i64> {
        self.state.latest_gateway_live_event_seq().await
    }

    pub async fn list_gateway_live_events_after(
        &self,
        after_seq: i64,
        limit: usize,
    ) -> Result<Vec<GatewayLiveEventRecord>> {
        self.state
            .list_gateway_live_events_after(after_seq, limit)
            .await
    }

    pub async fn cleanup_gateway_live_events_before(&self, before_ms: i64) -> Result<usize> {
        self.state
            .cleanup_gateway_live_events_before(before_ms)
            .await
    }

    pub async fn cleanup_gateway_live_snapshots_before(&self, before_ms: i64) -> Result<usize> {
        self.state
            .cleanup_gateway_live_snapshots_before(before_ms)
            .await
    }

    pub async fn list_gateway_live_snapshots(
        &self,
        limit: usize,
    ) -> Result<Vec<GatewayLiveSnapshotRecord>> {
        self.state.list_gateway_live_snapshots(limit).await
    }

    pub async fn list_gateway_live_snapshots_for_thread(
        &self,
        thread_id: &str,
        turn_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<GatewayLiveSnapshotRecord>> {
        self.state
            .list_gateway_live_snapshots_for_thread(thread_id, turn_id, limit)
            .await
    }

    pub async fn upsert_gateway_live_snapshots(
        &self,
        inputs: &[GatewayLiveSnapshotInput<'_>],
    ) -> Result<Vec<i64>> {
        self.state.upsert_gateway_live_snapshots(inputs).await
    }

    pub async fn delete_gateway_live_snapshots_for_activity(
        &self,
        activity_id: &str,
    ) -> Result<usize> {
        self.state
            .delete_gateway_live_snapshots_for_activity(activity_id)
            .await
    }

    pub async fn upsert_gateway_channel_outbox(
        &self,
        input: GatewayChannelOutboxInput<'_>,
    ) -> Result<GatewayChannelOutboxRecord> {
        self.state.upsert_gateway_channel_outbox(input).await
    }

    pub async fn acknowledge_gateway_channel_outbox(&self, delivery_id: &str) -> Result<bool> {
        self.state
            .acknowledge_gateway_channel_outbox(delivery_id)
            .await
    }

    pub async fn fail_gateway_channel_outbox(&self, delivery_id: &str) -> Result<bool> {
        self.state.fail_gateway_channel_outbox(delivery_id).await
    }

    pub async fn retryable_gateway_channel_outbox(
        &self,
        connection_id: &str,
    ) -> Result<Vec<GatewayChannelOutboxRecord>> {
        self.state
            .retryable_gateway_channel_outbox(connection_id)
            .await
    }

    pub async fn upsert_automation_task(
        &self,
        input: AutomationTaskInput,
    ) -> Result<AutomationTaskRecord> {
        self.state.upsert_automation_task(input).await
    }

    pub async fn automation_task(&self, id: &str) -> Result<Option<AutomationTaskRecord>> {
        self.state.automation_task(id).await
    }

    pub async fn automation_tasks_for_cwd(&self, cwd: &str) -> Result<Vec<AutomationTaskRecord>> {
        self.state.automation_tasks_for_cwd(cwd).await
    }

    pub async fn automation_tasks_for_optional_cwd(
        &self,
        cwd: Option<&str>,
    ) -> Result<Vec<AutomationTaskRecord>> {
        self.state.automation_tasks_for_optional_cwd(cwd).await
    }

    pub async fn due_automation_tasks(
        &self,
        now_ms: i64,
        limit: usize,
    ) -> Result<Vec<AutomationTaskRecord>> {
        self.state.due_automation_tasks(now_ms, limit).await
    }

    pub async fn delete_automation_task(&self, id: &str) -> Result<bool> {
        self.state.delete_automation_task(id).await
    }

    pub async fn claim_automation_run(
        &self,
        automation_id: &str,
        trigger: &str,
    ) -> Result<Option<AutomationRunRecord>> {
        self.state
            .claim_automation_run(automation_id, trigger)
            .await
    }

    pub async fn automation_runs_for_task(
        &self,
        automation_id: &str,
        limit: usize,
    ) -> Result<Vec<AutomationRunRecord>> {
        self.state
            .automation_runs_for_task(automation_id, limit)
            .await
    }

    pub async fn stale_automation_runs_for_recovery(
        &self,
        now_ms: i64,
        stale_after_ms: i64,
        limit: usize,
    ) -> Result<Vec<AutomationRunRecoveryCandidate>> {
        self.state
            .stale_automation_runs_for_recovery(now_ms, stale_after_ms, limit)
            .await
    }

    pub async fn finish_automation_run(
        &self,
        input: AutomationRunFinishInput<'_>,
    ) -> Result<Option<AutomationRunRecord>> {
        self.state.finish_automation_run(input).await
    }
}
