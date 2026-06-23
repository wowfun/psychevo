#[allow(unused_imports)]
pub(crate) use super::*;
#[derive(Debug, Clone)]
pub struct OpenAiChatProvider {
    pub(crate) client: reqwest::Client,
    pub(crate) base_url: String,
    pub(crate) api_key: String,
    pub(crate) provider_name: String,
}

impl OpenAiChatProvider {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        provider_name: impl Into<String>,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            provider_name: provider_name.into(),
        }
    }

    #[cfg(test)]
    pub fn with_client(mut self, client: reqwest::Client) -> Self {
        self.client = client;
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
        Box::pin(async move {
            let mut abort = abort;
            if abort.aborted() {
                return Ok(aborted_generation_stream());
            }

            let endpoint = openai_chat_completions_endpoint(&base_url);
            let body = openai_chat_request_body(&request, &base_url);
            let mut http_request = client
                .post(endpoint)
                .header("accept", "text/event-stream")
                .json(&body);
            if !api_key.trim().is_empty() {
                http_request = http_request.bearer_auth(api_key);
            }
            let send = http_request.send();
            let response = tokio::select! {
                biased;
                _ = abort.wait_for_abort() => return Ok(aborted_generation_stream()),
                response = send => response?,
            };

            let status = response.status();
            if !status.is_success() {
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|err| format!("<failed to read error body: {err}>"));
                return Err(Error::Provider(format!(
                    "{provider_name} returned HTTP {status}: {body}"
                )));
            }

            let bytes = Box::pin(response.bytes_stream());
            let state = OpenAiChatStreamState {
                bytes,
                parser: SseParser::new(),
                normalizer: ChatChunkNormalizer::new(request.model.model.clone()),
                pending: VecDeque::new(),
                done: false,
                abort,
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
                                state.pending.extend(normalized.into_iter().map(Ok));
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
                                                state.pending.extend(events.into_iter().map(Ok));
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
}
