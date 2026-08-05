pub mod agent;
pub mod control;
pub mod events;
pub mod request;
pub mod support;
pub mod tool_router;
pub mod types;

pub use agent::{
    contextual_user_message_to_ai, message_to_ai, prompt_instruction_to_ai, run_agent_loop,
};
pub use control::{
    ControlHandle, ControlInputError, ControlReceivers, MAX_CONTROL_INPUT_BYTES,
    MAX_CONTROL_INPUT_ITEMS, PendingInputId, validate_steer_message,
};
pub use events::{AgentEvent, EventSink};
pub use request::{AgentCompletion, AgentLoopRequest, PromptInstruction, ToolSearchOptions};
pub use support::{NoopEventSink, now_ms, user_text_message};
pub use tool_router::{ToolRouter, ToolRouterError};
pub use types::{
    AssistantBlock, ContextualUserBlock, ContextualUserMessage, Error, ImageUrlBlock,
    ImageUrlBlockKind, LocalImageBlock, LocalImageBlockKind, Message, ProviderToolBlock,
    TerminalReason, TextBlock, ToolAttachment, ToolBinding, ToolCallBlock, ToolDisplayBodyPolicy,
    ToolDisplayCategory, ToolDisplaySpec, ToolExecutionMode, ToolExposure, ToolOutput,
    UserContentBlock,
};

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests;
