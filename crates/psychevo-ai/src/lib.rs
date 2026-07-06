pub(crate) use std::collections::{BTreeMap, VecDeque};
pub(crate) use std::pin::Pin;
pub(crate) use std::sync::{Arc, Mutex};

pub(crate) use futures::StreamExt;
pub(crate) use futures::future::BoxFuture;
pub(crate) use futures::stream::{self, BoxStream};
pub(crate) use serde::{Deserialize, Serialize};
pub(crate) use serde_json::{Value, json};
pub(crate) use thiserror::Error;
pub(crate) use tokio::sync::watch;

pub type Result<T> = std::result::Result<T, Error>;
pub type GenerationStream = BoxStream<'static, Result<StreamEvent>>;

// Public crate surface is assembled from focused implementation files.
#[path = "types.rs"]
pub(crate) mod types;
pub use types::*;
#[path = "metadata.rs"]
pub(crate) mod metadata;
pub use metadata::*;
#[path = "control.rs"]
pub(crate) mod control;
pub use control::*;
#[path = "openai/provider.rs"]
pub(crate) mod openai_provider;
pub use openai_provider::*;
#[path = "openai/request.rs"]
pub(crate) mod openai_request;
pub use openai_request::*;
#[path = "stream/sse.rs"]
pub(crate) mod stream_sse;
#[allow(unused_imports)]
use stream_sse::*;
#[path = "stream/chat_chunks.rs"]
pub(crate) mod stream_chat_chunks;
#[allow(unused_imports)]
use stream_chat_chunks::*;
#[path = "stream/raw.rs"]
pub(crate) mod stream_raw;
pub use stream_raw::*;
#[path = "voice.rs"]
pub(crate) mod voice;
pub use voice::*;
#[path = "fake.rs"]
pub(crate) mod fake;
pub use fake::*;

#[cfg(test)]
pub(crate) mod tests;
