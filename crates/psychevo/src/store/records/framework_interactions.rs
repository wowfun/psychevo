use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrameworkInteractionRecord {
    pub interaction_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub kind: crate::types::BlockingActionKind,
    pub status: FrameworkInteractionStatus,
    pub payload: Value,
    pub resolution: Option<Value>,
    pub requested_at_ms: i64,
    pub resolved_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameworkInteractionStatus {
    Pending,
    Resolved,
    Cancelled,
}

impl FrameworkInteractionStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Resolved => "resolved",
            Self::Cancelled => "cancelled",
        }
    }

    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "resolved" => Some(Self::Resolved),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}
