use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures::StreamExt;
use futures::future::{BoxFuture, join_all};
use psychevo_ai::{
    AbortSignal, GenerationProvider, GenerationRequest, Outcome, StreamEvent, ToolDeclaration,
    allowlisted_provider_metadata, normalize_usage,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::sync::watch;

pub type Result<T> = std::result::Result<T, Error>;

// Agent core is assembled from focused type, control, loop, stream, and tool files.
include!("types.rs");
include!("events.rs");
include!("control.rs");
include!("request.rs");
include!("agent/loop.rs");
include!("agent/stream.rs");
include!("agent/assistant.rs");
include!("agent/tools.rs");
include!("support.rs");

#[cfg(test)]
mod tests;
