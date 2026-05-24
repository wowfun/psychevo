pub(crate) use std::collections::{BTreeMap, VecDeque};
pub(crate) use std::sync::{Arc, Mutex};
pub(crate) use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) use futures::StreamExt;
pub(crate) use futures::future::{BoxFuture, join_all};
pub(crate) use psychevo_ai::{
    AbortSignal, GenerationProvider, GenerationRequest, Outcome, StreamEvent, ToolDeclaration,
    allowlisted_provider_metadata, normalize_usage,
};
pub(crate) use serde::{Deserialize, Serialize};
pub(crate) use serde_json::{Value, json};
pub(crate) use thiserror::Error;
pub(crate) use tokio::sync::{mpsc, watch};

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
#[path = "agent/loop.rs"]
pub(crate) mod agent_loop;
pub use agent_loop::*;
#[path = "agent/stream.rs"]
pub(crate) mod agent_stream;
#[allow(unused_imports)]
use agent_stream::*;
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
