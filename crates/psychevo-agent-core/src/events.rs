#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    AgentStart,
    AgentEnd {
        outcome: Outcome,
        messages: Vec<Message>,
    },
    TurnStart {
        turn_index: usize,
    },
    TurnEnd {
        turn_index: usize,
        outcome: Outcome,
    },
    MessageStart {
        message: Message,
    },
    MessageUpdate {
        message: Message,
    },
    MessageEnd {
        message: Message,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
    },
    ReasoningDelta {
        text: String,
    },
    ReasoningEnd {
        text: String,
    },
    ToolExecutionStart {
        tool_call_id: String,
        tool_name: String,
        args: Value,
        started_at_ms: i64,
    },
    ToolExecutionUpdate {
        tool_call_id: String,
        tool_name: String,
        partial_result: Value,
    },
    ToolExecutionEnd {
        tool_call_id: String,
        tool_name: String,
        result: Value,
        outcome: Outcome,
        elapsed_ms: u64,
    },
}

pub trait EventSink: Send + Sync {
    fn emit(&self, event: AgentEvent) -> BoxFuture<'static, Result<()>>;
}

