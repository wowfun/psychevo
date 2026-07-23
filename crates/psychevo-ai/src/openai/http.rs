use super::*;

use std::future::Future;
use std::sync::LazyLock;
use std::time::Duration;

use futures::StreamExt;
use tokio::time::Instant;

pub const DEFAULT_INFERENCE_IDLE_TIMEOUT_SECS: u64 = 300;
pub(crate) const ERROR_BODY_LIMIT_BYTES: usize = 64 * 1024;

static GENERATION_HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .build()
        .expect("generation HTTP client")
});

pub(crate) fn generation_http_client() -> reqwest::Client {
    GENERATION_HTTP_CLIENT.clone()
}

pub(crate) fn inference_idle_timeout(seconds: u64) -> Option<Duration> {
    (seconds > 0).then(|| Duration::from_secs(seconds))
}

#[derive(Debug)]
pub(crate) enum GuardedHttpError {
    Aborted,
    Failed(Error),
}

pub(crate) async fn send_guarded(
    request: reqwest::RequestBuilder,
    abort: &mut AbortSignal,
    idle_timeout: Option<Duration>,
    label: &str,
) -> std::result::Result<reqwest::Response, GuardedHttpError> {
    guard_future(request.send(), abort, idle_timeout, label)
        .await
        .and_then(|result| result.map_err(|error| GuardedHttpError::Failed(Error::Http(error))))
}

pub(crate) async fn checked_response(
    response: reqwest::Response,
    abort: &mut AbortSignal,
    idle_timeout: Option<Duration>,
    label: &str,
) -> std::result::Result<reqwest::Response, GuardedHttpError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = error_body_guarded(response, abort, idle_timeout, label).await?;
    Err(GuardedHttpError::Failed(Error::Provider(format!(
        "{label} returned HTTP {status}: {body}"
    ))))
}

pub(crate) async fn error_body_guarded(
    response: reqwest::Response,
    abort: &mut AbortSignal,
    idle_timeout: Option<Duration>,
    label: &str,
) -> std::result::Result<String, GuardedHttpError> {
    let body = read_body_guarded(
        response,
        abort,
        idle_timeout,
        Some(ERROR_BODY_LIMIT_BYTES),
        label,
    )
    .await?;
    let truncated = body.len() > ERROR_BODY_LIMIT_BYTES;
    let body = &body[..body.len().min(ERROR_BODY_LIMIT_BYTES)];
    let mut body = String::from_utf8_lossy(body).into_owned();
    if truncated {
        body.push_str("\n<response body truncated after 65536 bytes>");
    }
    Ok(body)
}

pub(crate) async fn response_json_guarded(
    response: reqwest::Response,
    abort: &mut AbortSignal,
    idle_timeout: Option<Duration>,
    label: &str,
) -> std::result::Result<Value, GuardedHttpError> {
    let bytes = read_body_guarded(response, abort, idle_timeout, None, label).await?;
    serde_json::from_slice(&bytes)
        .map_err(Error::Json)
        .map_err(GuardedHttpError::Failed)
}

async fn guard_future<F, T>(
    future: F,
    abort: &mut AbortSignal,
    idle_timeout: Option<Duration>,
    label: &str,
) -> std::result::Result<T, GuardedHttpError>
where
    F: Future<Output = T>,
{
    tokio::pin!(future);
    tokio::select! {
        biased;
        _ = abort.wait_for_abort() => Err(GuardedHttpError::Aborted),
        _ = wait_for_deadline(idle_timeout.map(|timeout| Instant::now() + timeout)) => {
            Err(GuardedHttpError::Failed(idle_error(label, idle_timeout.expect("deadline timeout"))))
        }
        output = &mut future => Ok(output),
    }
}

async fn read_body_guarded(
    response: reqwest::Response,
    abort: &mut AbortSignal,
    idle_timeout: Option<Duration>,
    limit: Option<usize>,
    label: &str,
) -> std::result::Result<Vec<u8>, GuardedHttpError> {
    let mut stream = response.bytes_stream();
    let mut output = Vec::new();
    let mut deadline = idle_timeout.map(|timeout| Instant::now() + timeout);
    loop {
        let next = tokio::select! {
            biased;
            _ = abort.wait_for_abort() => return Err(GuardedHttpError::Aborted),
            _ = wait_for_deadline(deadline) => {
                return Err(GuardedHttpError::Failed(idle_error(
                    label,
                    idle_timeout.expect("deadline timeout"),
                )));
            }
            next = stream.next() => next,
        };
        let Some(chunk) = next else {
            return Ok(output);
        };
        let chunk = chunk
            .map_err(Error::Http)
            .map_err(GuardedHttpError::Failed)?;
        if !chunk.is_empty() {
            deadline = idle_timeout.map(|timeout| Instant::now() + timeout);
            let take = limit
                .map(|limit| limit.saturating_add(1).saturating_sub(output.len()))
                .unwrap_or(chunk.len())
                .min(chunk.len());
            output.extend_from_slice(&chunk[..take]);
            if limit.is_some_and(|limit| output.len() > limit) {
                return Ok(output);
            }
        }
    }
}

pub(crate) async fn wait_for_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

pub(crate) fn idle_error(label: &str, timeout: Duration) -> Error {
    Error::Provider(format!(
        "{label} made no progress for {} seconds",
        timeout.as_secs()
    ))
}

pub(crate) fn inference_event_is_progress(event: &StreamEvent) -> bool {
    match event {
        StreamEvent::TextDelta { text } => !text.is_empty(),
        StreamEvent::ReasoningDelta {
            text,
            reasoning_content,
        } => {
            !text.is_empty()
                || reasoning_content
                    .as_deref()
                    .is_some_and(|text| !text.is_empty())
        }
        StreamEvent::ToolCallDelta {
            id,
            name,
            arguments_delta,
            ..
        } => {
            !arguments_delta.is_empty()
                || id.as_deref().is_some_and(|value| !value.is_empty())
                || name.as_deref().is_some_and(|value| !value.is_empty())
        }
        StreamEvent::Metadata { .. } | StreamEvent::Usage { .. } => false,
        _ => true,
    }
}
