use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayTurnTerminalInput<'a> {
    pub turn_id: &'a str,
    pub thread_id: &'a str,
    pub status: crate::application::FrameworkTurnTerminalStatus,
    pub outcome: Option<crate::application::FrameworkTurnTerminalOutcome>,
    pub error_message: Option<&'a str>,
    pub started_at_ms: Option<i64>,
    pub completed_at_ms: i64,
    /// Last committed message sequence visible when this terminal settles.
    /// `None` captures the Thread's current boundary in the same transaction.
    pub boundary_session_seq: Option<i64>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayTurnTerminalRecord {
    pub turn_id: String,
    pub thread_id: String,
    pub status: crate::application::FrameworkTurnTerminalStatus,
    pub outcome: Option<crate::application::FrameworkTurnTerminalOutcome>,
    pub error_message: Option<String>,
    pub started_at_ms: Option<i64>,
    pub completed_at_ms: i64,
    pub boundary_session_seq: i64,
    pub metadata: Option<Value>,
}
