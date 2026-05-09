use std::collections::{BTreeMap, VecDeque};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use futures::StreamExt;
use futures::future::BoxFuture;
use futures::stream::{self, BoxStream};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::sync::watch;

pub type Result<T> = std::result::Result<T, Error>;
pub type GenerationStream = BoxStream<'static, Result<StreamEvent>>;

// Public crate surface is assembled from focused implementation files.
include!("types.rs");
include!("metadata.rs");
include!("control.rs");
include!("openai/provider.rs");
include!("openai/request.rs");
include!("stream/sse.rs");
include!("stream/chat_chunks.rs");
include!("stream/raw.rs");
include!("fake.rs");

#[cfg(test)]
mod tests;
