use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SAFE_ERROR_SUMMARY_LIMIT: usize = 4 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Aborted,
    Authentication,
    Configuration,
    InvalidRequest,
    Protocol,
    RateLimited,
    RuntimeUnavailable,
    Timeout,
    Transport,
    Provider,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorPhase {
    Runtime,
    Credentials,
    Preflight,
    Dispatch,
    ResponseHeaders,
    ResponseBody,
    Stream,
    RealtimeConnect,
    RealtimeCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[error("{summary}")]
pub struct ProviderError {
    pub kind: ErrorKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u64>,
    pub phase: ErrorPhase,
    pub summary: String,
}

impl ProviderError {
    pub fn new(kind: ErrorKind, phase: ErrorPhase, summary: impl Into<String>) -> Self {
        Self {
            kind,
            status: None,
            provider_code: None,
            retry_after_seconds: None,
            phase,
            summary: bounded_summary(summary.into()),
        }
    }

    pub fn aborted(phase: ErrorPhase) -> Self {
        Self::new(kind_for_abort(), phase, "invocation aborted")
    }

    pub fn runtime_unavailable() -> Self {
        Self::new(
            ErrorKind::RuntimeUnavailable,
            ErrorPhase::Runtime,
            "a Tokio runtime is required to start this invocation",
        )
    }

    pub fn protocol(summary: impl Into<String>) -> Self {
        Self::new(ErrorKind::Protocol, ErrorPhase::Stream, summary)
    }

    pub fn configuration(summary: impl Into<String>) -> Self {
        Self::new(ErrorKind::Configuration, ErrorPhase::Preflight, summary)
    }

    pub fn invalid_request(summary: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidRequest, ErrorPhase::Preflight, summary)
    }

    pub fn provider(
        phase: ErrorPhase,
        status: Option<u16>,
        code: Option<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            kind: status
                .map(|status| match status {
                    401 | 403 => ErrorKind::Authentication,
                    429 => ErrorKind::RateLimited,
                    _ => ErrorKind::Provider,
                })
                .unwrap_or(ErrorKind::Provider),
            status,
            provider_code: code.map(bounded_identifier),
            retry_after_seconds: None,
            phase,
            summary: bounded_summary(summary.into()),
        }
    }

    pub fn with_retry_after_seconds(mut self, seconds: Option<u64>) -> Self {
        self.retry_after_seconds = seconds;
        self
    }
}

#[cfg(any(feature = "openai", feature = "xiaomi"))]
pub(crate) fn legacy_error(error: crate::Error, phase: ErrorPhase) -> ProviderError {
    match error {
        crate::Error::Http(error) => ProviderError::new(
            ErrorKind::Transport,
            phase,
            format!("HTTP transport failed: {error}"),
        ),
        crate::Error::Json(error) => ProviderError::new(
            ErrorKind::Protocol,
            phase,
            format!("provider JSON failed: {error}"),
        ),
        crate::Error::Provider(message) => ProviderError::provider(phase, None, None, message),
        crate::Error::ProviderResponse {
            status,
            code,
            retry_after_seconds,
            summary,
        } => ProviderError::provider(phase, Some(status), code, summary)
            .with_retry_after_seconds(retry_after_seconds),
    }
}

fn kind_for_abort() -> ErrorKind {
    ErrorKind::Aborted
}

pub(crate) fn bounded_summary(value: String) -> String {
    truncate_utf8(value, SAFE_ERROR_SUMMARY_LIMIT)
}

pub(crate) fn bounded_identifier(value: String) -> String {
    truncate_utf8(value, 256)
}

fn truncate_utf8(mut value: String, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value;
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
    value.push('…');
    value
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}
