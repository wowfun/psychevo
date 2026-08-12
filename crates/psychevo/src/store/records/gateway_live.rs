use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayLiveEventRecord {
    pub seq: i64,
    pub activity_id: Option<String>,
    pub owner_id: Option<String>,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub event: Value,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayLiveEventCommit {
    pub seq: i64,
    pub idempotency_key: Option<String>,
    pub inserted: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayLiveSnapshotInput<'a> {
    pub snapshot_key: &'a str,
    pub activity_id: Option<&'a str>,
    pub owner_id: Option<&'a str>,
    pub thread_id: Option<&'a str>,
    pub turn_id: Option<&'a str>,
    pub event_kind: &'a str,
    pub event: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayLiveSnapshotRecord {
    pub snapshot_key: String,
    pub activity_id: Option<String>,
    pub owner_id: Option<String>,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub event_kind: String,
    pub event: Value,
    pub revision: i64,
    pub change_version: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayLiveSnapshotPage {
    pub high_watermark: i64,
    pub next_version: i64,
    pub snapshots: Vec<GatewayLiveSnapshotRecord>,
}
