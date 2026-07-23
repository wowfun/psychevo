#[allow(unused_imports)]
pub(crate) use super::*;

use futures::StreamExt;
use std::time::Duration;

use crate::openai_http::{
    GuardedHttpError, checked_response, generation_http_client, inference_event_is_progress,
    inference_idle_timeout, response_json_guarded, send_guarded, wait_for_deadline,
};

#[derive(Debug, Clone)]
pub struct OpenAiResponsesProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    inference_idle_timeout: Option<Duration>,
}

impl OpenAiResponsesProvider {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            client: generation_http_client(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            inference_idle_timeout: inference_idle_timeout(
                crate::openai_http::DEFAULT_INFERENCE_IDLE_TIMEOUT_SECS,
            ),
        }
    }

    pub fn with_inference_idle_timeout_secs(mut self, seconds: u64) -> Self {
        self.inference_idle_timeout = inference_idle_timeout(seconds);
        self
    }

    #[cfg(test)]
    pub fn with_client(mut self, client: reqwest::Client) -> Self {
        self.client = client;
        self
    }
}

impl GenerationProvider for OpenAiResponsesProvider {
    fn stream(
        &self,
        request: GenerationRequest,
        abort: AbortSignal,
    ) -> BoxFuture<'static, Result<GenerationStream>> {
        let this = self.clone();
        Box::pin(async move {
            if abort.aborted() {
                return Ok(aborted_generation_stream());
            }
            let body = openai_responses_request_body(&request, &this.base_url);
            if body.get("background").and_then(Value::as_bool) == Some(true) {
                return this.background_stream(body, abort).await;
            }
            this.streaming_response(body, abort).await
        })
    }
}

impl OpenAiResponsesProvider {
    async fn streaming_response(
        &self,
        body: Value,
        mut abort: AbortSignal,
    ) -> Result<GenerationStream> {
        let response = match send_guarded(
            self.authorized(self.client.post(responses_endpoint(&self.base_url)))
                .json(&body),
            &mut abort,
            self.inference_idle_timeout,
            "OpenAI Responses",
        )
        .await
        {
            Ok(response) => response,
            Err(GuardedHttpError::Aborted) => return Ok(aborted_generation_stream()),
            Err(GuardedHttpError::Failed(error)) => return Err(error),
        };
        let response = match checked_response(
            response,
            &mut abort,
            self.inference_idle_timeout,
            "OpenAI Responses",
        )
        .await
        {
            Ok(response) => response,
            Err(GuardedHttpError::Aborted) => return Ok(aborted_generation_stream()),
            Err(GuardedHttpError::Failed(error)) => return Err(error),
        };
        let state = ResponsesStreamState {
            bytes: Box::pin(response.bytes_stream()),
            parser: ResponsesSseParser::default(),
            pending: VecDeque::new(),
            abort,
            done: false,
            inference_idle_timeout: self.inference_idle_timeout,
            inference_deadline: self
                .inference_idle_timeout
                .map(|timeout| tokio::time::Instant::now() + timeout),
        };
        let output = stream::unfold(state, |mut state| async move {
            loop {
                if let Some(event) = state.pending.pop_front() {
                    return Some((event, state));
                }
                if state.done {
                    return None;
                }
                let next = tokio::select! {
                    biased;
                    _ = state.abort.wait_for_abort() => {
                        state.done = true;
                        return Some((Ok(StreamEvent::Done { outcome: Outcome::Aborted, finish_reason: Some("aborted".into()) }), state));
                    }
                    _ = wait_for_deadline(state.inference_deadline) => {
                        state.done = true;
                        let timeout = state.inference_idle_timeout.expect("deadline timeout");
                        return Some((Err(Error::Provider(format!(
                            "OpenAI Responses made no inference progress for {} seconds",
                            timeout.as_secs(),
                        ))), state));
                    }
                    next = state.bytes.next() => next,
                };
                match next {
                    Some(Ok(bytes)) => match state.parser.push(&bytes) {
                        Ok(values) => {
                            for value in values {
                                state.push_normalized(normalize_response_event(&value));
                            }
                        }
                        Err(error) => {
                            state.done = true;
                            return Some((Err(error), state));
                        }
                    },
                    Some(Err(error)) => {
                        state.done = true;
                        return Some((Err(Error::Http(error)), state));
                    }
                    None => {
                        state.done = true;
                        match state.parser.finish() {
                            Ok(values) => {
                                for value in values {
                                    state.push_normalized(normalize_response_event(&value));
                                }
                            }
                            Err(error) => return Some((Err(error), state)),
                        }
                        if state.pending.is_empty() {
                            state.pending.push_back(Err(Error::Provider(
                                "Responses stream ended before a terminal event".into(),
                            )));
                        }
                    }
                }
            }
        });
        Ok(Box::pin(output))
    }

    async fn background_stream(
        &self,
        body: Value,
        mut abort: AbortSignal,
    ) -> Result<GenerationStream> {
        let response = match send_guarded(
            self.authorized(self.client.post(responses_endpoint(&self.base_url)))
                .json(&body),
            &mut abort,
            self.inference_idle_timeout,
            "OpenAI background create",
        )
        .await
        {
            Ok(response) => response,
            Err(GuardedHttpError::Aborted) => return Ok(aborted_generation_stream()),
            Err(GuardedHttpError::Failed(error)) => return Err(error),
        };
        let response = match checked_response(
            response,
            &mut abort,
            self.inference_idle_timeout,
            "OpenAI background create",
        )
        .await
        {
            Ok(response) => response,
            Err(GuardedHttpError::Aborted) => return Ok(aborted_generation_stream()),
            Err(GuardedHttpError::Failed(error)) => return Err(error),
        };
        let mut response_value = match response_json_guarded(
            response,
            &mut abort,
            self.inference_idle_timeout,
            "OpenAI background create body",
        )
        .await
        {
            Ok(value) => value,
            Err(GuardedHttpError::Aborted) => return Ok(aborted_generation_stream()),
            Err(GuardedHttpError::Failed(error)) => return Err(error),
        };
        let id = response_value
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Provider("background response did not include id".into()))?
            .to_string();
        while let Some("queued" | "in_progress") =
            response_value.get("status").and_then(Value::as_str)
        {
            tokio::select! {
                biased;
                _ = abort.wait_for_abort() => {
                    self.cancel_background_response(id.clone());
                    return Ok(aborted_generation_stream());
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {}
            }
            let response = match send_guarded(
                self.authorized(
                    self.client
                        .get(response_retrieve_endpoint(&self.base_url, &id)),
                ),
                &mut abort,
                self.inference_idle_timeout,
                "OpenAI background poll",
            )
            .await
            {
                Ok(response) => response,
                Err(GuardedHttpError::Aborted) => {
                    self.cancel_background_response(id.clone());
                    return Ok(aborted_generation_stream());
                }
                Err(GuardedHttpError::Failed(error)) => return Err(error),
            };
            let response = match checked_response(
                response,
                &mut abort,
                self.inference_idle_timeout,
                "OpenAI background poll",
            )
            .await
            {
                Ok(response) => response,
                Err(GuardedHttpError::Aborted) => {
                    self.cancel_background_response(id.clone());
                    return Ok(aborted_generation_stream());
                }
                Err(GuardedHttpError::Failed(error)) => return Err(error),
            };
            response_value = match response_json_guarded(
                response,
                &mut abort,
                self.inference_idle_timeout,
                "OpenAI background poll body",
            )
            .await
            {
                Ok(value) => value,
                Err(GuardedHttpError::Aborted) => {
                    self.cancel_background_response(id.clone());
                    return Ok(aborted_generation_stream());
                }
                Err(GuardedHttpError::Failed(error)) => return Err(error),
            };
        }
        let events = normalize_complete_response(&response_value)
            .into_iter()
            .map(Ok);
        Ok(Box::pin(stream::iter(events)))
    }

    fn authorized(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.api_key.trim().is_empty() {
            request
        } else {
            request.bearer_auth(&self.api_key)
        }
    }

    fn cancel_background_response(&self, id: String) {
        let request = self.authorized(
            self.client
                .post(response_cancel_endpoint(&self.base_url, &id)),
        );
        let timeout = self
            .inference_idle_timeout
            .unwrap_or_else(|| Duration::from_secs(10));
        tokio::spawn(async move {
            let _ = tokio::time::timeout(timeout, request.send()).await;
        });
    }
}

pub fn responses_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/responses") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/responses")
    }
}

fn response_retrieve_endpoint(base_url: &str, id: &str) -> String {
    format!("{}/{}", responses_endpoint(base_url), id)
}
fn response_cancel_endpoint(base_url: &str, id: &str) -> String {
    format!("{}/{}/cancel", responses_endpoint(base_url), id)
}

pub fn openai_responses_request_body(request: &GenerationRequest, base_url: &str) -> Value {
    let mut body = json!({
        "model": request.model.model,
        "input": responses_input(request, base_url),
        "stream": true,
    });
    let mut tools = Vec::new();
    let mut background = false;
    for tool in &request.tools {
        match tool {
            GenerationTool::Function { declaration } => tools.push(json!({
                "type": "function", "name": declaration.name,
                "description": declaration.description, "parameters": declaration.parameters,
            })),
            GenerationTool::WebSearch(hosted) => {
                let mut search = hosted.config.clone();
                search
                    .as_object_mut()
                    .map(|object| object.insert("type".into(), Value::String("web_search".into())));
                background = hosted
                    .config
                    .get("return_token_budget")
                    .and_then(Value::as_str)
                    == Some("unlimited");
                tools.push(search);
            }
        }
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    if request
        .tools
        .iter()
        .any(|tool| matches!(tool, GenerationTool::WebSearch(_)))
    {
        body["include"] = json!(["web_search_call.action.sources", "web_search_call.results"]);
    }
    if background {
        body["background"] = Value::Bool(true);
        body["store"] = Value::Bool(true);
        body["stream"] = Value::Bool(false);
    }
    if let Some(effort) = request
        .metadata
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        body["reasoning"] = json!({"effort": effort, "summary": "auto"});
    }
    body
}

fn responses_input(request: &GenerationRequest, base_url: &str) -> Vec<Value> {
    let chat = translate_messages(
        &request.messages,
        &request.model,
        &request.metadata,
        base_url,
        ImageInputTranslationMode::ModelMetadata,
    );
    let mut output = Vec::new();
    for message in chat {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        if role == "tool" {
            output.push(json!({
                "type": "function_call_output",
                "call_id": message.get("tool_call_id").cloned().unwrap_or(Value::Null),
                "output": message.get("content").cloned().unwrap_or(Value::String(String::new())),
            }));
            continue;
        }
        if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
            if message
                .get("content")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.is_empty())
            {
                output.push(response_message(
                    role,
                    message.get("content").cloned().unwrap_or_default(),
                ));
            }
            for call in tool_calls {
                output.push(json!({
                    "type": "function_call",
                    "call_id": call.get("id").cloned().unwrap_or(Value::Null),
                    "name": call.pointer("/function/name").cloned().unwrap_or(Value::Null),
                    "arguments": call.pointer("/function/arguments").cloned().unwrap_or(Value::String("{}".into())),
                }));
            }
            continue;
        }
        output.push(response_message(
            role,
            message
                .get("content")
                .cloned()
                .unwrap_or(Value::String(String::new())),
        ));
    }
    output
}

fn response_message(role: &str, content: Value) -> Value {
    let content = match content {
        Value::Array(parts) => Value::Array(parts.into_iter().map(|part| {
            if part.get("type").and_then(Value::as_str) == Some("image_url") {
                json!({"type":"input_image", "image_url": part.pointer("/image_url/url").cloned().unwrap_or(Value::Null)})
            } else if part.get("type").and_then(Value::as_str) == Some("text") {
                json!({"type":"input_text", "text": part.get("text").cloned().unwrap_or(Value::String(String::new()))})
            } else { part }
        }).collect()),
        other => other,
    };
    json!({"role": role, "content": content})
}

#[derive(Default)]
struct ResponsesSseParser {
    buffer: Vec<u8>,
}

impl ResponsesSseParser {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<Value>> {
        self.buffer.extend_from_slice(bytes);
        self.drain(false)
    }
    fn finish(&mut self) -> Result<Vec<Value>> {
        self.drain(true)
    }
    fn drain(&mut self, finish: bool) -> Result<Vec<Value>> {
        let mut values = Vec::new();
        loop {
            let boundary = self
                .buffer
                .windows(2)
                .position(|window| window == b"\n\n")
                .map(|index| (index, 2))
                .or_else(|| {
                    self.buffer
                        .windows(4)
                        .position(|window| window == b"\r\n\r\n")
                        .map(|index| (index, 4))
                });
            let Some((index, consumed)) = boundary else {
                if finish && !self.buffer.is_empty() {
                    let block = std::mem::take(&mut self.buffer);
                    parse_response_sse_block(&block, &mut values)?;
                }
                break;
            };
            let block = self.buffer[..index].to_vec();
            self.buffer.drain(..index + consumed);
            parse_response_sse_block(&block, &mut values)?;
        }
        Ok(values)
    }
}

fn parse_response_sse_block(block: &[u8], values: &mut Vec<Value>) -> Result<()> {
    let block = std::str::from_utf8(block)
        .map_err(|_| Error::Provider("Responses SSE was not UTF-8".into()))?;
    let data = block
        .lines()
        .filter_map(|line| line.strip_prefix("data:").map(str::trim))
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() || data == "[DONE]" {
        return Ok(());
    }
    values.push(serde_json::from_str(&data)?);
    Ok(())
}

pub(crate) fn normalize_response_event(value: &Value) -> Vec<StreamEvent> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match kind {
        "response.output_text.delta" => value
            .get("delta")
            .and_then(Value::as_str)
            .map(|text| vec![StreamEvent::TextDelta { text: text.into() }])
            .unwrap_or_default(),
        "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => value
            .get("delta")
            .and_then(Value::as_str)
            .map(|text| {
                vec![StreamEvent::ReasoningDelta {
                    text: text.into(),
                    reasoning_content: None,
                }]
            })
            .unwrap_or_default(),
        "response.output_item.added" => {
            normalize_output_item(value.get("item").unwrap_or(&Value::Null), false)
        }
        "response.output_item.done" => {
            normalize_output_item(value.get("item").unwrap_or(&Value::Null), true)
        }
        "response.function_call_arguments.delta" => vec![StreamEvent::ToolCallDelta {
            content_index: value
                .get("output_index")
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize,
            call_index: value
                .get("output_index")
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize,
            id: value
                .get("item_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            name: None,
            arguments_delta: value
                .get("delta")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
        }],
        "response.function_call_arguments.done" => vec![StreamEvent::ToolCallEnd {
            content_index: value
                .get("output_index")
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize,
            call_index: value
                .get("output_index")
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize,
        }],
        "response.output_text.annotation.added" => value
            .get("annotation")
            .and_then(source_from_annotation)
            .map(|source| vec![StreamEvent::Source { source }])
            .unwrap_or_default(),
        "response.completed" => value
            .get("response")
            .map(normalize_stream_terminal)
            .unwrap_or_else(|| {
                vec![StreamEvent::Done {
                    outcome: Outcome::Normal,
                    finish_reason: Some("completed".into()),
                }]
            }),
        "response.web_search_call.in_progress" | "response.web_search_call.searching" => {
            vec![StreamEvent::ProviderToolStart {
                id: value
                    .get("item_id")
                    .and_then(Value::as_str)
                    .unwrap_or("web_search")
                    .into(),
                name: "web_search".into(),
                action: value.get("action").cloned(),
            }]
        }
        "response.web_search_call.completed" => vec![StreamEvent::ProviderToolEnd {
            id: value
                .get("item_id")
                .and_then(Value::as_str)
                .unwrap_or("web_search")
                .into(),
            name: "web_search".into(),
            action: value.get("action").cloned(),
            status: "completed".into(),
        }],
        "response.failed" => vec![StreamEvent::Done {
            outcome: Outcome::Failed,
            finish_reason: Some("failed".into()),
        }],
        "response.incomplete" => vec![StreamEvent::Done {
            outcome: Outcome::Failed,
            finish_reason: Some("incomplete".into()),
        }],
        "error" => vec![StreamEvent::Done {
            outcome: Outcome::Failed,
            finish_reason: value
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }],
        _ => Vec::new(),
    }
}

fn normalize_output_item(item: &Value, done: bool) -> Vec<StreamEvent> {
    let kind = item.get("type").and_then(Value::as_str).unwrap_or_default();
    let id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    match kind {
        "function_call" if !done => vec![StreamEvent::ToolCallStart {
            content_index: item
                .get("output_index")
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize,
            call_index: item
                .get("output_index")
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize,
            id,
            name: item
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
        }],
        "web_search_call" if !done => vec![StreamEvent::ProviderToolStart {
            id,
            name: "web_search".into(),
            action: item.get("action").cloned(),
        }],
        "web_search_call" => {
            let mut events = vec![StreamEvent::ProviderToolEnd {
                id,
                name: "web_search".into(),
                action: item.get("action").cloned(),
                status: item
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("completed")
                    .into(),
            }];
            events.extend(
                sources_from_web_item(item)
                    .into_iter()
                    .map(|source| StreamEvent::Source { source }),
            );
            events
        }
        _ => Vec::new(),
    }
}

fn normalize_complete_response(response: &Value) -> Vec<StreamEvent> {
    let mut events = Vec::new();
    if let Some(output) = response.get("output").and_then(Value::as_array) {
        for item in output {
            if item.get("type").and_then(Value::as_str) == Some("message") {
                for content in item
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(text) = content.get("text").and_then(Value::as_str) {
                        events.push(StreamEvent::TextDelta { text: text.into() });
                    }
                    for source in content
                        .get("annotations")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(source_from_annotation)
                    {
                        events.push(StreamEvent::Source { source });
                    }
                }
            } else if item.get("type").and_then(Value::as_str) == Some("function_call") {
                let index = item
                    .get("output_index")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize;
                let id = item
                    .get("call_id")
                    .or_else(|| item.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                events.push(StreamEvent::ToolCallStart {
                    content_index: index,
                    call_index: index,
                    id: id.clone(),
                    name: item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .into(),
                });
                events.push(StreamEvent::ToolCallDelta {
                    content_index: index,
                    call_index: index,
                    id: Some(id),
                    name: item.get("name").and_then(Value::as_str).map(str::to_owned),
                    arguments_delta: item
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("{}")
                        .into(),
                });
                events.push(StreamEvent::ToolCallEnd {
                    content_index: index,
                    call_index: index,
                });
            } else {
                events.extend(normalize_output_item(item, true));
            }
        }
    }
    if let Some(usage) = response.get("usage") {
        events.push(StreamEvent::Usage {
            usage: usage.clone(),
        });
    }
    let status = response
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("completed");
    events.push(StreamEvent::Done {
        outcome: if status == "completed" {
            Outcome::Normal
        } else {
            Outcome::Failed
        },
        finish_reason: Some(status.into()),
    });
    events
}

fn normalize_stream_terminal(response: &Value) -> Vec<StreamEvent> {
    let mut events = Vec::new();
    if let Some(output) = response.get("output").and_then(Value::as_array) {
        for item in output {
            if item.get("type").and_then(Value::as_str) == Some("web_search_call") {
                events.extend(
                    sources_from_web_item(item)
                        .into_iter()
                        .map(|source| StreamEvent::Source { source }),
                );
            } else if item.get("type").and_then(Value::as_str) == Some("message") {
                for content in item
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    for source in content
                        .get("annotations")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(source_from_annotation)
                    {
                        events.push(StreamEvent::Source { source });
                    }
                }
            }
        }
    }
    if let Some(usage) = response.get("usage") {
        events.push(StreamEvent::Usage {
            usage: usage.clone(),
        });
    }
    events.push(StreamEvent::Done {
        outcome: Outcome::Normal,
        finish_reason: Some("completed".into()),
    });
    events
}

fn source_from_annotation(annotation: &Value) -> Option<AssistantSource> {
    if annotation.get("type").and_then(Value::as_str) != Some("url_citation") {
        return None;
    }
    Some(AssistantSource::UrlCitation(UrlCitationSource {
        url: annotation.get("url")?.as_str()?.to_string(),
        title: annotation
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        start_index: annotation
            .get("start_index")
            .and_then(Value::as_u64)
            .map(|value| value as usize),
        end_index: annotation
            .get("end_index")
            .and_then(Value::as_u64)
            .map(|value| value as usize),
    }))
}

fn sources_from_web_item(item: &Value) -> Vec<AssistantSource> {
    let mut out = Vec::new();
    for source in item
        .pointer("/action/sources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(url) = source.get("url").and_then(Value::as_str) {
            out.push(AssistantSource::UrlCitation(UrlCitationSource {
                url: url.into(),
                title: source
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .into(),
                start_index: None,
                end_index: None,
            }));
        }
    }
    for result in item
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if (result.get("type").and_then(Value::as_str) == Some("image_result")
            || result.get("image_url").is_some())
            && let (Some(image_url), Some(source_website_url)) = (
                result.get("image_url").and_then(Value::as_str),
                result.get("source_website_url").and_then(Value::as_str),
            )
        {
            out.push(AssistantSource::Image(ImageSearchSource {
                image_url: image_url.into(),
                thumbnail_url: result
                    .get("thumbnail_url")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                source_website_url: source_website_url.into(),
                caption: result
                    .get("caption")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            }));
        }
    }
    out
}

struct ResponsesStreamState {
    bytes: Pin<
        Box<dyn futures::Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Send>,
    >,
    parser: ResponsesSseParser,
    pending: VecDeque<Result<StreamEvent>>,
    abort: AbortSignal,
    done: bool,
    inference_idle_timeout: Option<Duration>,
    inference_deadline: Option<tokio::time::Instant>,
}

impl ResponsesStreamState {
    fn push_normalized(&mut self, events: Vec<StreamEvent>) {
        if events.iter().any(inference_event_is_progress) {
            self.inference_deadline = self
                .inference_idle_timeout
                .map(|timeout| tokio::time::Instant::now() + timeout);
        }
        self.pending.extend(events.into_iter().map(Ok));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_contains_function_and_hosted_tool_once() {
        let request = GenerationRequest {
            model: ModelTarget {
                provider: "openai".into(),
                model: "gpt-5".into(),
            },
            messages: vec![json!({"role":"user","content":"latest news"})],
            tools: vec![
                ToolDeclaration::new("read", "Read", json!({"type":"object"})).into(),
                GenerationTool::WebSearch(HostedWebSearchTool {
                    config: json!({"search_context_size":"medium","return_token_budget":"default"}),
                }),
            ],
            metadata: json!({}),
        };
        let body = openai_responses_request_body(&request, "https://api.openai.com/v1");
        assert_eq!(body["tools"].as_array().unwrap().len(), 2);
        assert_eq!(
            body.pointer("/tools/1/type").and_then(Value::as_str),
            Some("web_search")
        );
    }

    #[test]
    fn normalizes_citations_and_provider_tool_without_function_call() {
        let events = normalize_response_event(
            &json!({"type":"response.output_item.added","item":{"type":"web_search_call","id":"ws_1","action":{"type":"search","query":"rust"}}}),
        );
        assert!(matches!(
            events.as_slice(),
            [StreamEvent::ProviderToolStart { .. }]
        ));
        let events = normalize_response_event(
            &json!({"type":"response.output_text.annotation.added","annotation":{"type":"url_citation","url":"https://example.com","title":"Example","start_index":0,"end_index":7}}),
        );
        assert!(matches!(
            events.as_slice(),
            [StreamEvent::Source {
                source: AssistantSource::UrlCitation(_)
            }]
        ));
    }

    #[test]
    fn unlimited_hosted_search_uses_acknowledged_background_contract() {
        let request = GenerationRequest {
            model: ModelTarget {
                provider: "openai".into(),
                model: "gpt-5".into(),
            },
            messages: vec![json!({"role":"user","content":"research"})],
            tools: vec![GenerationTool::WebSearch(HostedWebSearchTool {
                config: json!({
                    "search_context_size":"high", "return_token_budget":"unlimited",
                    "external_web_access":true, "content_types":["text","image"]
                }),
            })],
            metadata: json!({}),
        };
        let body = openai_responses_request_body(&request, "https://api.openai.com/v1");
        assert_eq!(body["background"], true);
        assert_eq!(body["store"], true);
        assert_eq!(body["stream"], false);
        assert_eq!(
            body["include"],
            json!(["web_search_call.action.sources", "web_search_call.results"])
        );
    }
}
