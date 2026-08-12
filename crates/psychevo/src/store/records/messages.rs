use psychevo_agent_core::Message;

#[derive(Debug, Clone, PartialEq)]
pub struct SessionMessageRecord {
    pub session_seq: i64,
    pub message: Message,
}
