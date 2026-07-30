pub(crate) use std::collections::{BTreeMap, VecDeque};
pub(crate) use std::sync::{Arc, Mutex};
pub(crate) use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) use futures::{StreamExt, future::BoxFuture, stream::FuturesUnordered};
pub(crate) use psychevo_ai::{
    AbortSignal, AssistantSource, FinishReasonKind, GenerationEvent, GenerationOutcome,
    LanguageModel, LanguageRequest, LanguageSettings, LanguageTool, Outcome, ToolDeclaration,
    WebSearchTool,
};
pub(crate) use serde::{Deserialize, Serialize};
pub(crate) use serde_json::{Value, json};
pub(crate) use thiserror::Error;
pub(crate) use tokio::sync::watch;

pub type Result<T> = std::result::Result<T, Error>;

// Agent core is assembled from focused type, control, loop, stream, and tool files.
#[path = "types.rs"]
pub(crate) mod types;
pub use types::*;
#[path = "events.rs"]
pub(crate) mod events;
pub use events::*;
#[path = "control.rs"]
pub(crate) mod control;
pub use control::*;
#[path = "request.rs"]
pub(crate) mod request;
pub use request::*;
#[path = "tool_router.rs"]
pub(crate) mod tool_router;
pub use tool_router::*;
#[path = "agent/loop.rs"]
pub(crate) mod agent_loop;
pub use agent_loop::*;
#[path = "agent/stream.rs"]
pub(crate) mod agent_stream;
pub use agent_stream::{contextual_user_message_to_ai, message_to_ai, prompt_instruction_to_ai};
pub(crate) use agent_stream::{emit, stream_assistant};
#[path = "agent/assistant.rs"]
pub(crate) mod agent_assistant;
#[allow(unused_imports)]
use agent_assistant::*;
#[path = "agent/tools.rs"]
pub(crate) mod agent_tools;
#[allow(unused_imports)]
use agent_tools::*;
#[path = "support.rs"]
pub(crate) mod support;
pub use support::*;

#[cfg(test)]
pub(crate) mod tests;
