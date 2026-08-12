use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayControlCommandInput<'a> {
    pub activity_id: &'a str,
    pub owner_id: &'a str,
    pub command_kind: GatewayControlCommandKind,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayControlCommandRecord {
    pub id: i64,
    pub activity_id: String,
    pub owner_id: String,
    pub command_kind: GatewayControlCommandKind,
    pub status: GatewayControlCommandStatus,
    pub payload: Value,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayControlCommandKind {
    Interrupt,
    Steer,
    Permission,
    Clarify,
}

impl GatewayControlCommandKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Interrupt => "interrupt",
            Self::Steer => "steer",
            Self::Permission => "permission",
            Self::Clarify => "clarify",
        }
    }

    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "interrupt" => Some(Self::Interrupt),
            "steer" => Some(Self::Steer),
            "permission" => Some(Self::Permission),
            "clarify" => Some(Self::Clarify),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayControlCommandStatus {
    Pending,
    Applying,
    Applied,
    Failed,
    OutcomeIndeterminate,
}

impl GatewayControlCommandStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Applying => "applying",
            Self::Applied => "applied",
            Self::Failed => "failed",
            Self::OutcomeIndeterminate => "outcome_indeterminate",
        }
    }

    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "applying" => Some(Self::Applying),
            "applied" => Some(Self::Applied),
            "failed" => Some(Self::Failed),
            "outcome_indeterminate" => Some(Self::OutcomeIndeterminate),
            _ => None,
        }
    }
}
