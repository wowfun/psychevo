use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayActivityClaimInput<'a> {
    pub activity_id: &'a str,
    pub thread_id: Option<&'a str>,
    pub source_key: Option<&'a str>,
    pub turn_id: Option<&'a str>,
    pub kind: GatewayActivityKind,
    pub owner_id: &'a str,
    pub owner_surface: Option<&'a str>,
    pub lease_expires_at_ms: i64,
    pub queued_turns: usize,
    pub superseded_activity_id: Option<&'a str>,
    pub intent: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayActivityRecord {
    pub activity_id: String,
    pub thread_id: Option<String>,
    pub source_key: Option<String>,
    pub turn_id: Option<String>,
    pub kind: GatewayActivityKind,
    pub status: GatewayActivityState,
    pub owner_id: String,
    pub owner_surface: Option<String>,
    pub generation: i64,
    pub started_at_ms: i64,
    pub updated_at_ms: i64,
    pub lease_expires_at_ms: i64,
    pub queued_turns: usize,
    pub superseded_activity_id: Option<String>,
    pub intent: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayActivityKind {
    Turn,
    Shell,
}

impl GatewayActivityKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Turn => "turn",
            Self::Shell => "shell",
        }
    }

    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "turn" => Some(Self::Turn),
            "shell" => Some(Self::Shell),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayActivityState {
    Running,
    Queued,
    Superseded,
    Completed,
    Failed,
    Interrupted,
}

impl GatewayActivityState {
    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "running" => Some(Self::Running),
            "queued" => Some(Self::Queued),
            "superseded" => Some(Self::Superseded),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "interrupted" => Some(Self::Interrupted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayActivityTerminalStatus {
    Completed,
    Failed,
    Interrupted,
}

impl GatewayActivityTerminalStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GatewayTurnStartReceiptRecord {
    pub(crate) client_turn_id: String,
    pub(crate) turn_id: String,
}
