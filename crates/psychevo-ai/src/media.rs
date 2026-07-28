use std::fmt;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use bytes::Bytes;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct MediaError {
    message: String,
}

impl MediaError {
    fn invalid_base64(error: base64::DecodeError) -> Self {
        Self {
            message: format!("invalid base64 media: {error}"),
        }
    }

    fn io(error: std::io::Error) -> Self {
        Self {
            message: format!("media file I/O failed: {error}"),
        }
    }
}

#[derive(Clone)]
pub struct Media {
    inner: Arc<MediaInner>,
}

struct MediaInner {
    mime_type: String,
    original: MediaStorage,
    bytes: OnceLock<Result<Bytes, MediaError>>,
    base64: OnceLock<String>,
}

enum MediaStorage {
    Bytes(Bytes),
    Base64(String),
}

impl fmt::Debug for Media {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Media")
            .field("mime_type", &self.mime_type())
            .field("size_bytes", &self.bytes().ok().map(|bytes| bytes.len()))
            .finish_non_exhaustive()
    }
}

impl PartialEq for Media {
    fn eq(&self, other: &Self) -> bool {
        self.mime_type() == other.mime_type()
            && self.base64().ok().map(str::to_owned) == other.base64().ok().map(str::to_owned)
    }
}

impl Eq for Media {}

impl Media {
    pub fn from_bytes(mime_type: impl Into<String>, bytes: impl Into<Bytes>) -> Self {
        Self {
            inner: Arc::new(MediaInner {
                mime_type: mime_type.into(),
                original: MediaStorage::Bytes(bytes.into()),
                bytes: OnceLock::new(),
                base64: OnceLock::new(),
            }),
        }
    }

    pub fn from_base64(mime_type: impl Into<String>, base64: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(MediaInner {
                mime_type: mime_type.into(),
                original: MediaStorage::Base64(base64.into()),
                bytes: OnceLock::new(),
                base64: OnceLock::new(),
            }),
        }
    }

    pub async fn from_file(
        path: impl AsRef<Path>,
        mime_type: impl Into<String>,
    ) -> Result<Self, MediaError> {
        tokio::fs::read(path)
            .await
            .map(|bytes| Self::from_bytes(mime_type, bytes))
            .map_err(MediaError::io)
    }

    pub fn mime_type(&self) -> &str {
        &self.inner.mime_type
    }

    pub fn bytes(&self) -> Result<Bytes, MediaError> {
        self.inner
            .bytes
            .get_or_init(|| match &self.inner.original {
                MediaStorage::Bytes(bytes) => Ok(bytes.clone()),
                MediaStorage::Base64(value) => BASE64_STANDARD
                    .decode(value)
                    .map(Bytes::from)
                    .map_err(MediaError::invalid_base64),
            })
            .clone()
    }

    pub fn base64(&self) -> Result<&str, MediaError> {
        let bytes = self.bytes()?;
        let encoded = self
            .inner
            .base64
            .get_or_init(|| BASE64_STANDARD.encode(bytes));
        Ok(encoded)
    }
}

#[derive(Serialize, Deserialize)]
struct SerializedMedia {
    mime_type: String,
    data_base64: String,
}

impl Serialize for Media {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        SerializedMedia {
            mime_type: self.mime_type().to_string(),
            data_base64: self
                .base64()
                .map_err(serde::ser::Error::custom)?
                .to_string(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Media {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = SerializedMedia::deserialize(deserializer)?;
        Ok(Self::from_base64(value.mime_type, value.data_base64))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaInput {
    Inline {
        media: Media,
    },
    Url {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
    },
}
