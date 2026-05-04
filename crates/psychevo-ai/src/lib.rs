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

#[derive(Debug, Error)]
pub enum Error {
    #[error("fake provider script exhausted")]
    ScriptExhausted,
    #[error("provider failed: {0}")]
    Provider(String),
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON failed: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Normal,
    Stopped,
    Failed,
    Aborted,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
            Self::Aborted => "aborted",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelTarget {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationRequest {
    pub model: ModelTarget,
    pub messages: Vec<Value>,
    pub tools: Vec<ToolDeclaration>,
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDeclaration {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    TextDelta {
        text: String,
    },
    ReasoningDelta {
        text: String,
    },
    ToolCallStart {
        content_index: usize,
        call_index: usize,
        id: String,
        name: String,
    },
    ToolCallDelta {
        content_index: usize,
        call_index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    },
    ToolCallEnd {
        content_index: usize,
        call_index: usize,
    },
    Usage {
        usage: Value,
    },
    Metadata {
        metadata: Value,
    },
    Done {
        outcome: Outcome,
        finish_reason: Option<String>,
    },
}

#[derive(Clone)]
pub struct AbortSignal {
    rx: watch::Receiver<bool>,
}

impl AbortSignal {
    pub fn new(rx: watch::Receiver<bool>) -> Self {
        Self { rx }
    }

    pub fn aborted(&self) -> bool {
        *self.rx.borrow()
    }
}

pub trait GenerationProvider: Send + Sync {
    fn stream(
        &self,
        request: GenerationRequest,
        abort: AbortSignal,
    ) -> BoxFuture<'static, Result<GenerationStream>>;
}

#[derive(Debug, Clone)]
pub struct OpenAiChatProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    provider_name: String,
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
            if abort.aborted() {
                let events = vec![Ok(StreamEvent::Done {
                    outcome: Outcome::Aborted,
                    finish_reason: Some("aborted".to_string()),
                })];
                return Ok(Box::pin(stream::iter(events)) as Pin<Box<_>>);
            }

            let endpoint = chat_completions_endpoint(&base_url);
            let body = build_chat_request(&request);
            let response = client
                .post(endpoint)
                .bearer_auth(api_key)
                .header("accept", "text/event-stream")
                .json(&body)
                .send()
                .await?;

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
                    match state.bytes.next().await {
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

struct OpenAiChatStreamState {
    bytes: Pin<
        Box<dyn futures::Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Send>,
    >,
    parser: SseParser,
    normalizer: ChatChunkNormalizer,
    pending: VecDeque<Result<StreamEvent>>,
    done: bool,
    abort: AbortSignal,
}

fn chat_completions_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/chat/completions")
    }
}

fn build_chat_request(request: &GenerationRequest) -> Value {
    let mut body = json!({
        "model": request.model.model,
        "messages": translate_messages(&request.messages),
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    if !request.tools.is_empty() {
        body["tools"] = Value::Array(
            request
                .tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.parameters,
                        }
                    })
                })
                .collect(),
        );
    }
    if let Some(reasoning_effort) = request
        .metadata
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        body["reasoning_effort"] = Value::String(reasoning_effort.to_string());
    }
    body
}

fn translate_messages(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .flat_map(translate_message)
        .collect::<Vec<_>>()
}

fn translate_message(message: &Value) -> Vec<Value> {
    match message.get("role").and_then(Value::as_str) {
        Some("user") => user_messages(message),
        Some("assistant") => assistant_messages(message),
        Some("tool_result") => tool_result_messages(message),
        _ => Vec::new(),
    }
}

fn user_messages(message: &Value) -> Vec<Value> {
    message
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .filter(|text| !text.is_empty())
        .map(|text| json!({ "role": "user", "content": text }))
        .collect()
}

fn assistant_messages(message: &Value) -> Vec<Value> {
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    if let Some(blocks) = message.get("content").and_then(Value::as_array) {
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(value) = block.get("text").and_then(Value::as_str) {
                        text.push_str(value);
                    }
                }
                Some("tool_call") => {
                    let id = block.get("id").and_then(Value::as_str).unwrap_or_default();
                    let name = block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let arguments = block
                        .get("arguments_json")
                        .and_then(Value::as_str)
                        .unwrap_or("{}");
                    if !id.is_empty() && !name.is_empty() {
                        tool_calls.push(json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": arguments,
                            }
                        }));
                    }
                }
                _ => {}
            }
        }
    }
    if text.is_empty() && tool_calls.is_empty() {
        return Vec::new();
    }
    let mut output = json!({
        "role": "assistant",
        "content": (!text.is_empty()).then_some(text),
    });
    if !tool_calls.is_empty() {
        output["tool_calls"] = Value::Array(tool_calls);
    }
    vec![output]
}

fn tool_result_messages(message: &Value) -> Vec<Value> {
    let tool_call_id = message
        .get("tool_call_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if tool_call_id.is_empty() {
        return Vec::new();
    }
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    vec![json!({
        "role": "tool",
        "tool_call_id": tool_call_id,
        "content": content,
    })]
}

#[derive(Debug)]
struct SseParser {
    buffer: Vec<u8>,
    current_event: SseEvent,
    saw_data: bool,
    bom_checked: bool,
    done_seen: bool,
}

#[derive(Debug)]
struct SseEvent {
    event: String,
    data: String,
}

impl Default for SseEvent {
    fn default() -> Self {
        Self {
            event: "message".to_string(),
            data: String::new(),
        }
    }
}

impl SseParser {
    fn new() -> Self {
        Self {
            buffer: Vec::new(),
            current_event: SseEvent::default(),
            saw_data: false,
            bom_checked: false,
            done_seen: false,
        }
    }

    fn push(&mut self, chunk: &[u8]) -> Result<Vec<ChatCompletionChunk>> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        self.drain_complete_lines(false, &mut events)?;
        Ok(events)
    }

    fn finish(&mut self) -> Result<Vec<ChatCompletionChunk>> {
        let mut events = Vec::new();
        self.drain_complete_lines(true, &mut events)?;
        if self.saw_data {
            self.dispatch_current(&mut events)?;
        }
        Ok(events)
    }

    fn done_seen(&self) -> bool {
        self.done_seen
    }

    fn drain_complete_lines(
        &mut self,
        finish: bool,
        events: &mut Vec<ChatCompletionChunk>,
    ) -> Result<()> {
        if !self.strip_bom_if_ready(finish) {
            return Ok(());
        }

        loop {
            let Some((line_end, consumed)) = next_sse_line(&self.buffer, finish) else {
                break;
            };
            let line = std::str::from_utf8(&self.buffer[..line_end])
                .map_err(|err| Error::Provider(format!("SSE line is not UTF-8: {err}")))?
                .to_string();
            self.buffer.drain(..consumed);
            self.process_line(&line, events)?;
        }
        Ok(())
    }

    fn strip_bom_if_ready(&mut self, finish: bool) -> bool {
        if self.bom_checked {
            return true;
        }
        const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
        if self.buffer.len() >= BOM.len() {
            self.bom_checked = true;
            if self.buffer.starts_with(BOM) {
                self.buffer.drain(..BOM.len());
            }
            return true;
        }
        if !finish && BOM.starts_with(&self.buffer) {
            return false;
        }
        self.bom_checked = true;
        true
    }

    fn process_line(&mut self, line: &str, events: &mut Vec<ChatCompletionChunk>) -> Result<()> {
        if line.is_empty() {
            if self.saw_data {
                self.dispatch_current(events)?;
            }
            self.current_event = SseEvent::default();
            self.saw_data = false;
            return Ok(());
        }
        if line.starts_with(':') {
            return Ok(());
        }
        let (field, value) = line.split_once(':').map_or((line, ""), |(field, value)| {
            (field, value.strip_prefix(' ').unwrap_or(value))
        });
        match field {
            "event" => self.current_event.event = value.to_string(),
            "data" => {
                if self.saw_data {
                    self.current_event.data.push('\n');
                }
                self.current_event.data.push_str(value);
                self.saw_data = true;
            }
            _ => {}
        }
        Ok(())
    }

    fn dispatch_current(&mut self, events: &mut Vec<ChatCompletionChunk>) -> Result<()> {
        let data = self.current_event.data.trim();
        if data.is_empty() {
            return Ok(());
        }
        if data == "[DONE]" {
            self.done_seen = true;
            return Ok(());
        }
        if let Ok(raw) = serde_json::from_str::<Value>(data)
            && let Some(error) = raw.get("error")
        {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("provider returned an error in stream");
            return Err(Error::Provider(message.to_string()));
        }
        events.push(serde_json::from_str(data)?);
        Ok(())
    }
}

fn next_sse_line(buffer: &[u8], finish: bool) -> Option<(usize, usize)> {
    let pos = buffer
        .iter()
        .position(|byte| *byte == b'\n' || *byte == b'\r');
    match pos {
        Some(index) => {
            if buffer[index] == b'\r' && buffer.get(index + 1).is_none() && !finish {
                return None;
            }
            let consumed =
                if buffer[index] == b'\r' && buffer.get(index + 1).copied() == Some(b'\n') {
                    index + 2
                } else {
                    index + 1
                };
            Some((index, consumed))
        }
        None if finish && !buffer.is_empty() => Some((buffer.len(), buffer.len())),
        None => None,
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChunk {
    id: Option<String>,
    model: Option<String>,
    #[serde(default)]
    choices: Vec<ChatChoice>,
    usage: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    delta: ChatDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatDelta {
    content: Option<String>,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    tool_calls: Vec<ChatDeltaToolCall>,
}

fn null_as_empty_vec<'de, D, T>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Deserialize)]
struct ChatDeltaToolCall {
    index: usize,
    id: Option<String>,
    function: Option<ChatDeltaFunction>,
}

#[derive(Debug, Deserialize)]
struct ChatDeltaFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Debug)]
struct ChatChunkNormalizer {
    model: String,
    tool_calls: BTreeMap<usize, NormalizedToolCallState>,
    finish_reason: Option<String>,
}

#[derive(Debug, Default)]
struct NormalizedToolCallState {
    id: String,
    name: String,
    started: bool,
    ended: bool,
}

impl ChatChunkNormalizer {
    fn new(model: String) -> Self {
        Self {
            model,
            tool_calls: BTreeMap::new(),
            finish_reason: None,
        }
    }

    fn ingest(&mut self, chunk: ChatCompletionChunk) -> Result<Vec<StreamEvent>> {
        let mut output = Vec::new();
        if let Some(usage) = chunk.usage {
            output.push(StreamEvent::Usage { usage });
        }
        if let Some(id) = chunk.id {
            output.push(StreamEvent::Metadata {
                metadata: json!({ "provider_response_id": id, "model": chunk.model.unwrap_or_else(|| self.model.clone()) }),
            });
        }

        for choice in chunk.choices {
            if let Some(text) = choice.delta.content.filter(|value| !value.is_empty()) {
                output.push(StreamEvent::TextDelta { text });
            }
            for call in choice.delta.tool_calls {
                let state = self.tool_calls.entry(call.index).or_default();
                if let Some(id) = call.id.filter(|value| !value.is_empty()) {
                    state.id = id;
                }
                if let Some(function) = call.function {
                    if let Some(name) = function.name.filter(|value| !value.is_empty()) {
                        state.name = name;
                    }
                    if !state.started && !state.id.is_empty() && !state.name.is_empty() {
                        state.started = true;
                        output.push(StreamEvent::ToolCallStart {
                            content_index: call.index,
                            call_index: call.index,
                            id: state.id.clone(),
                            name: state.name.clone(),
                        });
                    }
                    if let Some(arguments_delta) =
                        function.arguments.filter(|value| !value.is_empty())
                    {
                        output.push(StreamEvent::ToolCallDelta {
                            content_index: call.index,
                            call_index: call.index,
                            id: (!state.id.is_empty()).then_some(state.id.clone()),
                            name: (!state.name.is_empty()).then_some(state.name.clone()),
                            arguments_delta,
                        });
                    }
                }
            }
            if let Some(reason) = choice.finish_reason {
                if reason == "tool_calls" {
                    output.extend(self.end_started_tool_calls());
                }
                self.finish_reason = Some(reason);
            }
        }
        Ok(output)
    }

    fn finish(&mut self) -> Vec<StreamEvent> {
        let mut output = self.end_started_tool_calls();
        output.push(StreamEvent::Done {
            outcome: Outcome::Normal,
            finish_reason: self.finish_reason.clone(),
        });
        output
    }

    fn end_started_tool_calls(&mut self) -> Vec<StreamEvent> {
        let mut output = Vec::new();
        for (index, state) in &mut self.tool_calls {
            if state.started && !state.ended {
                state.ended = true;
                output.push(StreamEvent::ToolCallEnd {
                    content_index: *index,
                    call_index: *index,
                });
            }
        }
        output
    }
}

#[derive(Debug, Clone)]
pub enum RawStreamEvent {
    Text(String),
    Reasoning(String),
    ToolStart {
        content_index: usize,
        call_index: usize,
        id: String,
        name: String,
    },
    ToolArgs {
        content_index: usize,
        call_index: usize,
        delta: String,
    },
    ToolEnd {
        content_index: usize,
        call_index: usize,
    },
    Done(Outcome),
}

impl RawStreamEvent {
    fn normalize(self) -> StreamEvent {
        match self {
            Self::Text(text) => StreamEvent::TextDelta { text },
            Self::Reasoning(text) => StreamEvent::ReasoningDelta { text },
            Self::ToolStart {
                content_index,
                call_index,
                id,
                name,
            } => StreamEvent::ToolCallStart {
                content_index,
                call_index,
                id,
                name,
            },
            Self::ToolArgs {
                content_index,
                call_index,
                delta,
            } => StreamEvent::ToolCallDelta {
                content_index,
                call_index,
                id: None,
                name: None,
                arguments_delta: delta,
            },
            Self::ToolEnd {
                content_index,
                call_index,
            } => StreamEvent::ToolCallEnd {
                content_index,
                call_index,
            },
            Self::Done(outcome) => StreamEvent::Done {
                outcome,
                finish_reason: None,
            },
        }
    }
}

#[derive(Clone)]
pub struct FakeProvider {
    scripts: Arc<Mutex<VecDeque<Vec<RawStreamEvent>>>>,
}

impl FakeProvider {
    pub fn new(scripts: Vec<Vec<RawStreamEvent>>) -> Self {
        Self {
            scripts: Arc::new(Mutex::new(scripts.into())),
        }
    }
}

impl GenerationProvider for FakeProvider {
    fn stream(
        &self,
        _request: GenerationRequest,
        abort: AbortSignal,
    ) -> BoxFuture<'static, Result<GenerationStream>> {
        let scripts = Arc::clone(&self.scripts);
        Box::pin(async move {
            if abort.aborted() {
                let events = vec![Ok(StreamEvent::Done {
                    outcome: Outcome::Aborted,
                    finish_reason: Some("aborted".to_string()),
                })];
                return Ok(Box::pin(stream::iter(events)) as Pin<Box<_>>);
            }

            let script = scripts
                .lock()
                .expect("fake provider script lock poisoned")
                .pop_front()
                .ok_or(Error::ScriptExhausted)?;
            let events = script.into_iter().map(|event| Ok(event.normalize()));
            Ok(Box::pin(stream::iter(events)) as Pin<Box<_>>)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn chat_request_maps_messages_and_tools() {
        let request = GenerationRequest {
            model: ModelTarget {
                provider: "openai".to_string(),
                model: "gpt-test".to_string(),
            },
            messages: vec![
                json!({
                    "role": "user",
                    "content": [{ "text": "hello" }],
                    "timestamp_ms": 1
                }),
                json!({
                    "role": "assistant",
                    "content": [{
                        "type": "tool_call",
                        "id": "call_1",
                        "name": "read",
                        "arguments": { "path": "a" },
                        "arguments_json": "{\"path\":\"a\"}",
                        "arguments_error": null,
                        "content_index": 0,
                        "call_index": 0
                    }],
                    "timestamp_ms": 2,
                    "finish_reason": "tool_calls",
                    "outcome": "normal",
                    "model": "gpt-test",
                    "provider": "openai"
                }),
                json!({
                    "role": "tool_result",
                    "tool_call_id": "call_1",
                    "tool_name": "read",
                    "content": "{\"ok\":true}",
                    "is_error": false,
                    "timestamp_ms": 3
                }),
            ],
            tools: vec![ToolDeclaration {
                name: "read".to_string(),
                description: "read file".to_string(),
                parameters: json!({ "type": "object" }),
            }],
            metadata: json!({ "reasoning_effort": "medium" }),
        };

        let body = build_chat_request(&request);
        assert_eq!(body["model"], "gpt-test");
        assert_eq!(body["stream"], true);
        assert_eq!(
            body["messages"][0],
            json!({"role": "user", "content": "hello"})
        );
        assert_eq!(
            body["messages"][1]["tool_calls"][0]["function"]["arguments"],
            "{\"path\":\"a\"}"
        );
        assert_eq!(body["messages"][2]["role"], "tool");
        assert_eq!(body["tools"][0]["function"]["name"], "read");
        assert_eq!(body["reasoning_effort"], "medium");
    }

    #[test]
    fn sse_parser_handles_chunking_bom_crlf_and_done() {
        let mut parser = SseParser::new();
        let first =
            "\u{FEFF}: keepalive\r\ndata: {\"id\":\"x\",\"choices\":[{\"delta\":{\"content\":\"he";
        let second = "llo\"},\"finish_reason\":null}],\"usage\":null}\r\n\r\ndata: [DONE]\r\n\r\n";
        assert!(parser.push(first.as_bytes()).expect("first").is_empty());
        let events = parser.push(second.as_bytes()).expect("second");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].choices[0].delta.content.as_deref(), Some("hello"));
        assert!(parser.done_seen());
    }

    #[test]
    fn sse_parser_handles_split_utf8_and_line_endings() {
        let mut parser = SseParser::new();
        let payload = "data: {\"choices\":[{\"delta\":{\"content\":\"hi 中\"},\"finish_reason\":\"stop\"}]}\r";
        let split = payload.find("中").expect("utf8");
        assert!(
            parser
                .push(&payload.as_bytes()[..split + 1])
                .expect("partial")
                .is_empty()
        );
        let mut rest = payload.as_bytes()[split + 1..].to_vec();
        rest.extend_from_slice(b"\n\rdata: [DONE]\r\n\r\n");
        let events = parser.push(&rest).expect("rest");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].choices[0].delta.content.as_deref(), Some("hi 中"));
        assert_eq!(events[0].choices[0].finish_reason.as_deref(), Some("stop"));
        assert!(parser.done_seen());
    }

    #[test]
    fn sse_parser_handles_multiline_data_lf_and_comments() {
        let mut parser = SseParser::new();
        let input = concat!(
            ": ignore\n",
            "event: message\n",
            "data: {\"choices\":\n",
            "data: [{\"delta\":{\"content\":\"multi\"},\"finish_reason\":null}]}\n",
            "\n",
            "data: [DONE]\n\n"
        );
        let events = parser.push(input.as_bytes()).expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].choices[0].delta.content.as_deref(), Some("multi"));
        assert!(parser.done_seen());
    }

    #[test]
    fn sse_parser_reports_provider_error_objects() {
        let mut parser = SseParser::new();
        let err = parser
            .push(b"data: {\"error\":{\"message\":\"bad key\"}}\n\n")
            .expect_err("provider error");
        assert!(err.to_string().contains("bad key"));
    }

    #[test]
    fn sse_parser_rejects_premature_eof() {
        let mut parser = SseParser::new();
        let events = parser
            .push(b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":\"stop\"}]}\n\n")
            .expect("event");
        assert_eq!(events.len(), 1);
        assert!(!parser.done_seen());
    }

    #[test]
    fn chat_chunk_normalizer_streams_tool_calls() {
        let mut normalizer = ChatChunkNormalizer::new("gpt-test".to_string());
        let chunks = vec![
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read","arguments":"{\"pa"}}]},"finish_reason":null}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"a\"}"}}]},"finish_reason":"tool_calls"}]}"#,
        ];
        let mut events = Vec::new();
        for chunk in chunks {
            let chunk = serde_json::from_str::<ChatCompletionChunk>(chunk).expect("chunk");
            events.extend(normalizer.ingest(chunk).expect("ingest"));
        }
        events.extend(normalizer.finish());

        assert!(events.iter().any(|event| {
            matches!(
                event,
                StreamEvent::ToolCallStart {
                    id,
                    name,
                    ..
                } if id == "call_1" && name == "read"
            )
        }));
        assert!(
            events
                .iter()
                .any(|event| { matches!(event, StreamEvent::ToolCallEnd { call_index: 0, .. }) })
        );
        assert_eq!(
            events.last(),
            Some(&StreamEvent::Done {
                outcome: Outcome::Normal,
                finish_reason: Some("tool_calls".to_string())
            })
        );
    }

    #[test]
    fn chat_chunk_normalizer_handles_null_tool_calls_and_usage() {
        let mut normalizer = ChatChunkNormalizer::new("gpt-test".to_string());
        let chunk = serde_json::from_str::<ChatCompletionChunk>(
            r#"{"id":"resp_1","model":"gpt-test","choices":[{"delta":{"content":"ok","tool_calls":null},"finish_reason":"stop"}],"usage":{"total_tokens":7}}"#,
        )
        .expect("chunk");
        let mut events = normalizer.ingest(chunk).expect("ingest");
        events.extend(normalizer.finish());

        assert!(events.iter().any(|event| {
            matches!(
                event,
                StreamEvent::Usage { usage } if usage["total_tokens"] == 7
            )
        }));
        assert!(events.iter().any(|event| {
            matches!(
                event,
                StreamEvent::Metadata { metadata } if metadata["provider_response_id"] == "resp_1"
            )
        }));
        assert!(
            events
                .iter()
                .any(|event| { matches!(event, StreamEvent::TextDelta { text } if text == "ok") })
        );
        assert_eq!(
            events.last(),
            Some(&StreamEvent::Done {
                outcome: Outcome::Normal,
                finish_reason: Some("stop".to_string())
            })
        );
    }
}
