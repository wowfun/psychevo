use std::collections::{BTreeMap, VecDeque};
use std::pin::Pin;
use std::time::Duration;

use futures::{StreamExt, future::BoxFuture, stream};

use super::http::{
    GuardedHttpError, error_body_guarded, inference_event_is_progress, provider_response_error,
    send_guarded, wait_for_deadline,
};
#[cfg(test)]
use super::http::{generation_http_client, inference_idle_timeout};
use super::request::{
    openai_chat_completions_endpoint, openai_chat_request_body,
    openai_chat_request_body_text_only_images, request_has_image_blocks,
};
use crate::control::{AbortSignal, GenerationProvider};
use crate::stream::chat_chunks::ChatChunkNormalizer;
use crate::stream::sse::SseParser;
use crate::types::{Error, GenerationRequest, Outcome, StreamEvent};
use crate::{GenerationStream, Result};

#[derive(Debug, Clone)]
pub struct OpenAiChatProvider {
    pub(crate) client: reqwest::Client,
    pub(crate) base_url: String,
    pub(crate) api_key: String,
    pub(crate) provider_name: String,
    pub(crate) inference_idle_timeout: Option<Duration>,
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) allow_image_text_fallback: bool,
}

impl OpenAiChatProvider {
    #[cfg(test)]
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        provider_name: impl Into<String>,
    ) -> Self {
        Self {
            client: generation_http_client(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            provider_name: provider_name.into(),
            inference_idle_timeout: inference_idle_timeout(
                super::http::DEFAULT_INFERENCE_IDLE_TIMEOUT_SECS,
            ),
            headers: BTreeMap::new(),
            allow_image_text_fallback: true,
        }
    }

    #[cfg(test)]
    pub fn with_inference_idle_timeout_secs(mut self, seconds: u64) -> Self {
        self.inference_idle_timeout = inference_idle_timeout(seconds);
        self
    }
}

impl GenerationProvider for OpenAiChatProvider {
    fn stream(
        &self,
        request: GenerationRequest,
        abort: AbortSignal,
    ) -> BoxFuture<'static, Result<GenerationStream>> {
        let client = self.client.clone();
        let base_url = self.base_url.clone();
        let api_key = self.api_key.clone();
        let provider_name = self.provider_name.clone();
        let inference_idle_timeout = self.inference_idle_timeout;
        let headers = self.headers.clone();
        let allow_image_text_fallback = self.allow_image_text_fallback;
        Box::pin(async move {
            let mut abort = abort;
            if abort.aborted() {
                return Ok(aborted_generation_stream());
            }

            let endpoint = openai_chat_completions_endpoint(&base_url);
            let has_image_blocks = request_has_image_blocks(&request);
            let mut force_text_images = false;
            let response = loop {
                let body = if force_text_images {
                    openai_chat_request_body_text_only_images(&request, &base_url)
                } else {
                    openai_chat_request_body(&request, &base_url)
                };
                let mut http_request = client
                    .post(endpoint.clone())
                    .header("accept", "text/event-stream")
                    .json(&body);
                for (name, value) in &headers {
                    http_request = http_request.header(name, value);
                }
                if !api_key.trim().is_empty() {
                    http_request = http_request.bearer_auth(&api_key);
                }
                let response = match send_guarded(
                    http_request,
                    &mut abort,
                    inference_idle_timeout,
                    &provider_name,
                )
                .await
                {
                    Ok(response) => response,
                    Err(GuardedHttpError::Aborted) => return Ok(aborted_generation_stream()),
                    Err(GuardedHttpError::Failed(error)) => return Err(error),
                };

                let status = response.status();
                if status.is_success() {
                    break response;
                }
                let retry_after_seconds = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.trim().parse::<u64>().ok());
                let body = match error_body_guarded(
                    response,
                    &mut abort,
                    inference_idle_timeout,
                    &provider_name,
                )
                .await
                {
                    Ok(body) => body,
                    Err(GuardedHttpError::Aborted) => return Ok(aborted_generation_stream()),
                    Err(GuardedHttpError::Failed(error)) => return Err(error),
                };
                if should_retry_image_rejection_as_text(
                    status,
                    &body,
                    has_image_blocks,
                    force_text_images,
                ) && allow_image_text_fallback
                {
                    force_text_images = true;
                    continue;
                }
                return Err(provider_response_error(
                    status,
                    retry_after_seconds,
                    &provider_name,
                    &body,
                ));
            };

            let bytes = Box::pin(response.bytes_stream());
            let state = OpenAiChatStreamState {
                bytes,
                parser: SseParser::new(),
                normalizer: ChatChunkNormalizer::new(request.model.model.clone()),
                pending: VecDeque::new(),
                done: false,
                abort,
                inference_idle_timeout,
                inference_deadline: inference_idle_timeout
                    .map(|timeout| tokio::time::Instant::now() + timeout),
                provider_name,
            };
            let output = stream::unfold(state, |mut state| async move {
                loop {
                    if let Some(event) = state.pending.pop_front() {
                        return Some((event, state));
                    }
                    if state.done {
                        return None;
                    }
                    if state.abort.aborted() {
                        state.done = true;
                        return Some((
                            Ok(StreamEvent::Done {
                                outcome: Outcome::Aborted,
                                finish_reason: Some("aborted".to_string()),
                            }),
                            state,
                        ));
                    }
                    let next = tokio::select! {
                        biased;
                        _ = state.abort.wait_for_abort() => {
                            state.done = true;
                            return Some((
                                Ok(StreamEvent::Done {
                                    outcome: Outcome::Aborted,
                                    finish_reason: Some("aborted".to_string()),
                                }),
                                state,
                            ));
                        }
                        _ = wait_for_deadline(state.inference_deadline) => {
                            state.done = true;
                            let timeout = state.inference_idle_timeout.expect("deadline timeout");
                            return Some((Err(Error::Provider(format!(
                                "{} made no inference progress for {} seconds",
                                state.provider_name,
                                timeout.as_secs(),
                            ))), state));
                        }
                        next = state.bytes.next() => next,
                    };
                    match next {
                        Some(Ok(chunk)) => {
                            let events = match state.parser.push(&chunk) {
                                Ok(events) => events,
                                Err(err) => {
                                    state.done = true;
                                    return Some((Err(err), state));
                                }
                            };
                            for event in events {
                                let normalized = match state.normalizer.ingest(event) {
                                    Ok(events) => events,
                                    Err(err) => {
                                        state.done = true;
                                        return Some((Err(err), state));
                                    }
                                };
                                state.push_normalized(normalized);
                            }
                        }
                        Some(Err(err)) => {
                            state.done = true;
                            return Some((Err(Error::Http(err)), state));
                        }
                        None => {
                            state.done = true;
                            match state.parser.finish() {
                                Ok(events) => {
                                    for event in events {
                                        match state.normalizer.ingest(event) {
                                            Ok(events) => {
                                                state.push_normalized(events);
                                            }
                                            Err(err) => {
                                                return Some((Err(err), state));
                                            }
                                        }
                                    }
                                    if state.parser.done_seen() {
                                        state
                                            .pending
                                            .extend(state.normalizer.finish().into_iter().map(Ok));
                                    } else {
                                        state.pending.push_back(Err(Error::Provider(
                                            "SSE stream ended before [DONE]".to_string(),
                                        )));
                                    }
                                }
                                Err(err) => return Some((Err(err), state)),
                            }
                        }
                    }
                }
            });
            Ok(Box::pin(output) as Pin<Box<_>>)
        })
    }
}

pub(crate) fn should_retry_image_rejection_as_text(
    status: reqwest::StatusCode,
    body: &str,
    has_image_blocks: bool,
    force_text_images: bool,
) -> bool {
    has_image_blocks
        && !force_text_images
        && status.is_client_error()
        && is_image_input_rejection_body(body)
}

pub(crate) fn is_image_input_rejection_body(body: &str) -> bool {
    let body = body.to_ascii_lowercase();
    [
        "no endpoints found that support image input",
        "does not support image input",
        "does not support images",
        "does not support image",
        "only text content type",
        "only 'text' content type",
        "image_url is not supported",
        "image content is not supported",
        "image input is not supported",
        "multimodal content is not supported",
        "multimodal input is not supported",
        "multimodal is not supported",
        "vision input is not supported",
        "vision is not supported",
        "unknown variant `image_url`, expected `text`",
        "unknown variant image_url, expected text",
    ]
    .iter()
    .any(|phrase| body.contains(phrase))
}

pub(crate) fn aborted_generation_stream() -> GenerationStream {
    let events = vec![Ok(StreamEvent::Done {
        outcome: Outcome::Aborted,
        finish_reason: Some("aborted".to_string()),
    })];
    Box::pin(stream::iter(events)) as Pin<Box<_>>
}

pub(crate) struct OpenAiChatStreamState {
    pub(crate) bytes: Pin<
        Box<dyn futures::Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Send>,
    >,
    pub(crate) parser: SseParser,
    pub(crate) normalizer: ChatChunkNormalizer,
    pub(crate) pending: VecDeque<Result<StreamEvent>>,
    pub(crate) done: bool,
    pub(crate) abort: AbortSignal,
    pub(crate) inference_idle_timeout: Option<Duration>,
    pub(crate) inference_deadline: Option<tokio::time::Instant>,
    pub(crate) provider_name: String,
}

impl OpenAiChatStreamState {
    fn push_normalized(&mut self, events: Vec<StreamEvent>) {
        if events.iter().any(inference_event_is_progress) {
            self.inference_deadline = self
                .inference_idle_timeout
                .map(|timeout| tokio::time::Instant::now() + timeout);
        }
        self.pending.extend(events.into_iter().map(Ok));
    }
}
