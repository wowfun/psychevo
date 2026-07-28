#[cfg(any(feature = "openai", feature = "xiaomi"))]
pub(crate) use std::collections::BTreeMap;
#[cfg(feature = "openai")]
pub(crate) use std::collections::VecDeque;
#[cfg(feature = "openai")]
pub(crate) use std::pin::Pin;

#[cfg(feature = "openai")]
pub(crate) use futures::StreamExt;
#[cfg(any(feature = "openai", feature = "xiaomi"))]
pub(crate) use futures::future::BoxFuture;
#[cfg(feature = "openai")]
pub(crate) use futures::stream::{self, BoxStream};
pub(crate) use serde::{Deserialize, Serialize};
pub(crate) use serde_json::Value;
#[cfg(any(feature = "openai", feature = "xiaomi"))]
pub(crate) use serde_json::json;
#[cfg(any(feature = "openai", feature = "xiaomi"))]
pub(crate) use thiserror::Error;
pub(crate) use tokio::sync::watch;

#[cfg(any(feature = "openai", feature = "xiaomi"))]
pub(crate) type Result<T> = std::result::Result<T, Error>;
#[cfg(feature = "openai")]
pub(crate) type GenerationStream = BoxStream<'static, Result<StreamEvent>>;

#[path = "sdk_error.rs"]
mod sdk_error;
pub use sdk_error::*;
#[path = "media.rs"]
mod media;
pub use media::*;
#[path = "sdk_types.rs"]
mod sdk_types;
pub use sdk_types::*;
#[path = "adapter.rs"]
mod adapter;
#[cfg(any(feature = "openai", feature = "anthropic"))]
#[path = "sse_line.rs"]
mod sse_line;
pub use adapter::*;
#[path = "credentials.rs"]
mod credentials;
pub use credentials::*;
#[path = "generation.rs"]
mod generation;
pub use generation::*;
#[path = "provider.rs"]
mod provider;
pub use provider::*;
#[path = "registry.rs"]
mod registry;
pub use registry::*;
#[path = "realtime.rs"]
mod realtime;
pub use realtime::*;
#[cfg(feature = "openai")]
#[path = "builtin_language.rs"]
mod builtin_language;
#[cfg(feature = "openai")]
pub use builtin_language::*;
#[cfg(any(feature = "openai", feature = "xiaomi"))]
#[path = "builtin_media.rs"]
mod builtin_media;
#[cfg(any(feature = "openai", feature = "xiaomi"))]
pub use builtin_media::*;
#[cfg(any(feature = "openai", feature = "anthropic", feature = "xiaomi"))]
#[path = "facades.rs"]
mod facades;
#[cfg(any(feature = "openai", feature = "anthropic", feature = "xiaomi"))]
pub use facades::*;
#[cfg(feature = "anthropic")]
#[path = "anthropic.rs"]
mod anthropic;
#[cfg(feature = "anthropic")]
pub use anthropic::*;
#[path = "fake_sdk.rs"]
mod fake_sdk;
pub use fake_sdk::*;

// Public crate surface is assembled from focused implementation files.
#[path = "types.rs"]
pub(crate) mod types;
pub use types::*;
#[cfg(feature = "openai")]
#[path = "metadata.rs"]
pub(crate) mod metadata;
#[cfg(feature = "openai")]
pub(crate) use metadata::*;
#[path = "control.rs"]
pub(crate) mod control;
pub use control::*;
#[cfg(feature = "openai")]
#[path = "openai/provider.rs"]
pub(crate) mod openai_provider;
#[cfg(feature = "openai")]
pub(crate) use openai_provider::*;
#[cfg(feature = "openai")]
#[path = "openai/http.rs"]
pub(crate) mod openai_http;
#[cfg(feature = "openai")]
pub use openai_http::DEFAULT_INFERENCE_IDLE_TIMEOUT_SECS;
#[cfg(feature = "openai")]
#[path = "openai/responses.rs"]
pub(crate) mod openai_responses;
#[cfg(feature = "openai")]
pub(crate) use openai_responses::*;
#[cfg(feature = "openai")]
#[path = "openai/request.rs"]
pub(crate) mod openai_request;
#[cfg(feature = "openai")]
pub(crate) use openai_request::*;
#[cfg(feature = "openai")]
#[path = "stream/sse.rs"]
pub(crate) mod stream_sse;
#[cfg(feature = "openai")]
#[allow(unused_imports)]
use stream_sse::*;
#[cfg(feature = "openai")]
#[path = "stream/chat_chunks.rs"]
pub(crate) mod stream_chat_chunks;
#[cfg(feature = "openai")]
#[allow(unused_imports)]
use stream_chat_chunks::*;
#[cfg(feature = "xiaomi")]
#[path = "voice.rs"]
pub(crate) mod voice;
#[cfg(feature = "xiaomi")]
pub(crate) use voice::*;
#[cfg(all(test, feature = "openai"))]
pub(crate) mod tests;
#[cfg(all(test, feature = "xiaomi"))]
#[path = "tests/voice_legacy.rs"]
mod voice_legacy_tests;
