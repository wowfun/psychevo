#[allow(unused_imports)]
pub(crate) use super::*;
#[derive(Debug, Clone)]
pub enum RawStreamEvent {
    Text(String),
    Reasoning(String),
    ToolStart {
        content_index: usize,
        call_index: usize,
        id: String,
        name: String,
    },
    ToolArgs {
        content_index: usize,
        call_index: usize,
        delta: String,
    },
    ToolEnd {
        content_index: usize,
        call_index: usize,
    },
    Done(Outcome),
}

impl RawStreamEvent {
    pub(crate) fn normalize(self) -> StreamEvent {
        match self {
            Self::Text(text) => StreamEvent::TextDelta { text },
            Self::Reasoning(text) => StreamEvent::ReasoningDelta {
                text,
                reasoning_content: None,
            },
            Self::ToolStart {
                content_index,
                call_index,
                id,
                name,
            } => StreamEvent::ToolCallStart {
                content_index,
                call_index,
                id,
                name,
            },
            Self::ToolArgs {
                content_index,
                call_index,
                delta,
            } => StreamEvent::ToolCallDelta {
                content_index,
                call_index,
                id: None,
                name: None,
                arguments_delta: delta,
            },
            Self::ToolEnd {
                content_index,
                call_index,
            } => StreamEvent::ToolCallEnd {
                content_index,
                call_index,
            },
            Self::Done(outcome) => StreamEvent::Done {
                outcome,
                finish_reason: None,
            },
        }
    }
}
