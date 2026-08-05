use std::collections::{BTreeMap, VecDeque};
use std::pin::Pin;

use futures::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{
    AdapterCall, AdapterFuture, AdapterResult, AdapterStream, AssistantContent, ErrorKind,
    ErrorPhase, FinishReason, FinishReasonKind, LanguageAdapter, LanguageAdapterEvent,
    LanguageRequest, LanguageTool, MediaInput, Message, ProviderError, TextContent, ToolChoice,
    Usage, UserContent, Warning,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnthropicAuth {
    #[default]
    ApiKey,
    Bearer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnthropicMessagesAdapter {
    pub auth: AnthropicAuth,
    pub version: String,
}

impl Default for AnthropicMessagesAdapter {
    fn default() -> Self {
        Self {
            auth: AnthropicAuth::ApiKey,
            version: "2023-06-01".to_string(),
        }
    }
}

impl AnthropicMessagesAdapter {
    pub fn new(auth: AnthropicAuth) -> Self {
        Self {
            auth,
            ..Self::default()
        }
    }

    pub fn preview(
        descriptor: &crate::ModelDescriptor,
        request: &LanguageRequest,
    ) -> AdapterResult<crate::RequestPreview> {
        let (body, warnings) = anthropic_request_body(&descriptor.model_id, request)?;
        Ok(crate::RequestPreview { body, warnings })
    }

    pub fn endpoint(base_url: &str) -> String {
        anthropic_messages_endpoint(base_url)
    }
}

impl LanguageAdapter for AnthropicMessagesAdapter {
    fn stream(
        &self,
        call: AdapterCall<LanguageRequest>,
    ) -> AdapterFuture<'_, AdapterStream<LanguageAdapterEvent>> {
        let auth = self.auth;
        let version = self.version.clone();
        Box::pin(async move {
            if call
                .context
                .profile
                .as_ref()
                .and_then(|profile| profile.capabilities.get("image_input"))
                .is_some_and(|supported| !supported)
                && call.request.messages.iter().any(message_has_image)
            {
                return Err(ProviderError::invalid_request(
                    "the selected model profile does not support image input",
                ));
            }
            let (body, warnings) = anthropic_request_body(&call.model, &call.request)?;
            let endpoint = anthropic_messages_endpoint(&call.context.endpoint);
            let mut request = call
                .context
                .client
                .post(endpoint)
                .header("accept", "text/event-stream")
                .header("anthropic-version", version)
                .json(&body);
            for (name, value) in &call.context.headers {
                request = request.header(name, value);
            }
            request = match auth {
                AnthropicAuth::ApiKey => request.header(
                    "x-api-key",
                    call.context.credentials.require("api_key")?.expose_secret(),
                ),
                AnthropicAuth::Bearer => request.bearer_auth(
                    call.context
                        .credentials
                        .require("bearer_token")?
                        .expose_secret(),
                ),
            };
            let response = request.send().await.map_err(|error| {
                ProviderError::new(
                    ErrorKind::Transport,
                    ErrorPhase::Dispatch,
                    format!("Anthropic request failed: {error}"),
                )
            })?;
            let status = response.status();
            if !status.is_success() {
                let retry_after_seconds = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.trim().parse::<u64>().ok());
                let value = read_bounded_error(response).await;
                return Err(ProviderError::provider(
                    ErrorPhase::ResponseBody,
                    Some(status.as_u16()),
                    value
                        .pointer("/error/type")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    value
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or("Anthropic returned an unsuccessful response"),
                )
                .with_retry_after_seconds(retry_after_seconds));
            }
            Ok(anthropic_stream(response, warnings))
        })
    }
}

fn anthropic_messages_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/messages") {
        trimmed.to_string()
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/messages")
    } else {
        format!("{trimmed}/v1/messages")
    }
}

fn message_has_image(message: &Message) -> bool {
    matches!(
        message,
        Message::User { content, .. }
            if content.iter().any(|content| matches!(content, UserContent::Image(_)))
    )
}

fn anthropic_request_body(
    model: &str,
    request: &LanguageRequest,
) -> AdapterResult<(Value, Vec<Warning>)> {
    if request
        .tools
        .iter()
        .any(|tool| matches!(tool, LanguageTool::WebSearch(_)))
    {
        return Err(ProviderError::invalid_request(
            "Anthropic Messages Adapter does not provide hosted web search",
        ));
    }
    if request
        .tools
        .iter()
        .any(|tool| matches!(tool, LanguageTool::Extension { .. }))
    {
        return Err(ProviderError::invalid_request(
            "Anthropic Messages Adapter does not support extension tools",
        ));
    }
    if request.settings.response_format.is_some() {
        return Err(ProviderError::invalid_request(
            "Anthropic Messages does not satisfy the requested structured response format",
        ));
    }

    let mut warnings = Vec::new();
    let mut system = Vec::new();
    let mut messages = Vec::new();
    for message in &request.messages {
        match message {
            Message::System { content, .. } => system.extend(text_blocks(content)),
            Message::Developer { content, .. } => {
                system.extend(text_blocks(content));
                warnings.push(Warning::new(
                    "developer_role_folded",
                    "Anthropic Messages folded developer messages into system",
                ));
            }
            Message::User { content, .. } => {
                let content = content
                    .iter()
                    .map(anthropic_user_content)
                    .collect::<AdapterResult<Vec<_>>>()?;
                push_anthropic_message(&mut messages, "user", content);
            }
            Message::Assistant { message } => {
                let mut content = Vec::new();
                for block in &message.content {
                    if let Some(block) = anthropic_assistant_content(block, &mut warnings)? {
                        content.push(block);
                    }
                }
                push_anthropic_message(&mut messages, "assistant", content);
            }
            Message::Tool {
                tool_call_id,
                content,
                is_error,
                ..
            } => {
                push_anthropic_message(
                    &mut messages,
                    "user",
                    vec![json!({
                        "type": "tool_result",
                        "tool_use_id": tool_call_id,
                        "content": joined_text(content),
                        "is_error": is_error,
                    })],
                );
            }
        }
    }
    if messages.is_empty() {
        return Err(ProviderError::invalid_request(
            "Anthropic Messages requires at least one user or assistant message",
        ));
    }

    let mut body = json!({
        "model": model,
        "messages": messages,
        "max_tokens": request.settings.max_output_tokens.unwrap_or(4096),
        "stream": true,
    });
    if !system.is_empty() {
        body["system"] = Value::Array(system);
    }
    let tools = request
        .tools
        .iter()
        .filter_map(|tool| match tool {
            LanguageTool::Function { declaration } => Some(json!({
                "name": declaration.name,
                "description": declaration.description,
                "input_schema": declaration.parameters,
            })),
            _ => None,
        })
        .collect::<Vec<_>>();
    if !tools.is_empty() && !matches!(request.settings.tool_choice, Some(ToolChoice::None)) {
        body["tools"] = Value::Array(tools);
    }
    if let Some(value) = request.settings.temperature {
        body["temperature"] = json!(value);
    }
    if let Some(value) = request.settings.top_p {
        body["top_p"] = json!(value);
    }
    if !request.settings.stop_sequences.is_empty() {
        body["stop_sequences"] = json!(request.settings.stop_sequences);
    }
    if let Some(choice) = &request.settings.tool_choice
        && !matches!(choice, ToolChoice::None)
    {
        body["tool_choice"] = match choice {
            ToolChoice::Auto => json!({"type": "auto"}),
            ToolChoice::Required => json!({"type": "any"}),
            ToolChoice::Tool { name } => json!({"type": "tool", "name": name}),
            ToolChoice::None => unreachable!("filtered above"),
        };
    }
    if request.settings.frequency_penalty.is_some() {
        warnings.push(Warning::new(
            "unsupported_frequency_penalty",
            "Anthropic Messages omitted frequency_penalty",
        ));
    }
    if request.settings.presence_penalty.is_some() {
        warnings.push(Warning::new(
            "unsupported_presence_penalty",
            "Anthropic Messages omitted presence_penalty",
        ));
    }
    if request.settings.seed.is_some() {
        warnings.push(Warning::new(
            "unsupported_seed",
            "Anthropic Messages omitted seed",
        ));
    }
    if request.settings.reasoning_effort.is_some() {
        warnings.push(Warning::new(
            "unsupported_reasoning_effort",
            "Anthropic Messages omitted the provider-neutral reasoning effort preference",
        ));
    }
    if let Some(extension) = request.extensions.get("anthropic") {
        if let Some(metadata) = extension.get("metadata") {
            body["metadata"] = metadata.clone();
        }
        if let Some(service_tier) = extension.get("service_tier") {
            body["service_tier"] = service_tier.clone();
        }
    }
    Ok((body, warnings))
}

fn text_blocks(content: &[TextContent]) -> Vec<Value> {
    content
        .iter()
        .filter(|content| !content.text.is_empty())
        .map(|content| json!({"type": "text", "text": content.text}))
        .collect()
}

fn joined_text(content: &[TextContent]) -> String {
    content
        .iter()
        .map(|content| content.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn anthropic_user_content(content: &UserContent) -> AdapterResult<Value> {
    match content {
        UserContent::Text(content) => Ok(json!({
            "type": "text",
            "text": content.text,
        })),
        UserContent::Image(content) => match &content.source {
            MediaInput::Inline { media } => Ok(json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": media.mime_type(),
                    "data": media
                        .base64()
                        .map_err(|error| ProviderError::invalid_request(error.to_string()))?,
                }
            })),
            MediaInput::Url { url, .. } => Ok(json!({
                "type": "image",
                "source": {
                    "type": "url",
                    "url": url,
                }
            })),
        },
        UserContent::Extension { namespace, .. } => Err(ProviderError::invalid_request(format!(
            "unsupported Anthropic user-content extension `{namespace}`"
        ))),
    }
}

fn anthropic_assistant_content(
    content: &AssistantContent,
    warnings: &mut Vec<Warning>,
) -> AdapterResult<Option<Value>> {
    match content {
        AssistantContent::Text(content) => Ok(Some(json!({
            "type": "text",
            "text": content.text,
        }))),
        AssistantContent::Reasoning {
            text,
            provider_evidence,
        } => {
            let signature = provider_evidence
                .as_ref()
                .and_then(|evidence| evidence.get("signature"))
                .and_then(Value::as_str);
            if let Some(signature) = signature {
                Ok(Some(json!({
                    "type": "thinking",
                    "thinking": text,
                    "signature": signature,
                })))
            } else {
                warnings.push(Warning::new(
                    "reasoning_replay_omitted",
                    "Anthropic reasoning replay was omitted because no signature was retained",
                ));
                Ok(None)
            }
        }
        AssistantContent::ToolCall(call) => {
            let input = call
                .arguments
                .clone()
                .filter(Value::is_object)
                .unwrap_or_else(|| {
                    warnings.push(Warning::new(
                        "invalid_tool_arguments_replayed_as_empty",
                        format!(
                            "tool call `{}` had invalid arguments; Anthropic replay used an empty object",
                            call.id
                        ),
                    ));
                    json!({})
                });
            Ok(Some(json!({
                "type": "tool_use",
                "id": call.id,
                "name": call.name,
                "input": input,
            })))
        }
        AssistantContent::ProviderTool(_) | AssistantContent::Source { .. } => Ok(None),
        AssistantContent::Extension { namespace, .. } => Err(ProviderError::invalid_request(
            format!("unsupported Anthropic assistant-content extension `{namespace}`"),
        )),
    }
}

fn push_anthropic_message(messages: &mut Vec<Value>, role: &str, content: Vec<Value>) {
    if content.is_empty() {
        return;
    }
    if let Some(last) = messages.last_mut()
        && last.get("role").and_then(Value::as_str) == Some(role)
        && let Some(existing) = last.get_mut("content").and_then(Value::as_array_mut)
    {
        existing.extend(content);
        return;
    }
    messages.push(json!({"role": role, "content": content}));
}

fn anthropic_stream(
    response: reqwest::Response,
    warnings: Vec<Warning>,
) -> AdapterStream<LanguageAdapterEvent> {
    let state = AnthropicStreamState {
        bytes: Box::pin(response.bytes_stream()),
        parser: AnthropicSseParser::default(),
        normalizer: AnthropicNormalizer::default(),
        pending: warnings
            .into_iter()
            .map(|warning| Ok(LanguageAdapterEvent::Warning { warning }))
            .collect(),
        done: false,
    };
    Box::pin(stream::unfold(state, |mut state| async move {
        loop {
            if let Some(event) = state.pending.pop_front() {
                return Some((event, state));
            }
            if state.done {
                return None;
            }
            match state.bytes.next().await {
                Some(Ok(bytes)) => match state.parser.push(&bytes) {
                    Ok(events) => {
                        for event in events {
                            if event.trim().is_empty() {
                                continue;
                            }
                            let value = match serde_json::from_str::<Value>(&event) {
                                Ok(value) => value,
                                Err(error) => {
                                    state.done = true;
                                    return Some((
                                        Err(ProviderError::new(
                                            ErrorKind::Protocol,
                                            ErrorPhase::Stream,
                                            format!("Anthropic SSE JSON failed: {error}"),
                                        )),
                                        state,
                                    ));
                                }
                            };
                            match state.normalizer.ingest(&value) {
                                Ok(events) => {
                                    state.pending.extend(events.into_iter().map(Ok));
                                }
                                Err(error) => {
                                    state.done = true;
                                    return Some((Err(error), state));
                                }
                            }
                        }
                    }
                    Err(error) => {
                        state.done = true;
                        return Some((Err(error), state));
                    }
                },
                Some(Err(error)) => {
                    state.done = true;
                    return Some((
                        Err(ProviderError::new(
                            ErrorKind::Transport,
                            ErrorPhase::Stream,
                            format!("Anthropic stream failed: {error}"),
                        )),
                        state,
                    ));
                }
                None => {
                    state.done = true;
                    match state.parser.finish() {
                        Ok(events) => {
                            for event in events {
                                if let Ok(value) = serde_json::from_str::<Value>(&event)
                                    && let Ok(events) = state.normalizer.ingest(&value)
                                {
                                    state.pending.extend(events.into_iter().map(Ok));
                                }
                            }
                        }
                        Err(error) => {
                            return Some((Err(error), state));
                        }
                    }
                }
            }
        }
    }))
}

struct AnthropicStreamState {
    bytes: Pin<Box<dyn futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>,
    parser: AnthropicSseParser,
    normalizer: AnthropicNormalizer,
    pending: VecDeque<AdapterResult<LanguageAdapterEvent>>,
    done: bool,
}

#[derive(Default)]
struct AnthropicSseParser {
    buffer: Vec<u8>,
    data: String,
    bom_checked: bool,
}

impl AnthropicSseParser {
    fn push(&mut self, chunk: &[u8]) -> AdapterResult<Vec<String>> {
        self.buffer.extend_from_slice(chunk);
        self.drain(false)
    }

    fn finish(&mut self) -> AdapterResult<Vec<String>> {
        let mut events = self.drain(true)?;
        if !self.data.is_empty() {
            events.push(std::mem::take(&mut self.data));
        }
        Ok(events)
    }

    fn drain(&mut self, finish: bool) -> AdapterResult<Vec<String>> {
        let mut events = Vec::new();
        if !self.bom_checked {
            const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
            if self.buffer.len() >= BOM.len() {
                self.bom_checked = true;
                if self.buffer.starts_with(BOM) {
                    self.buffer.drain(..BOM.len());
                }
            } else if !finish && BOM.starts_with(&self.buffer) {
                return Ok(events);
            } else {
                self.bom_checked = true;
            }
        }
        while let Some((line_end, consumed)) = crate::sse_line::next_sse_line(&self.buffer, finish)
        {
            let line = std::str::from_utf8(&self.buffer[..line_end])
                .map_err(|error| {
                    ProviderError::protocol(format!("Anthropic SSE line is not UTF-8: {error}"))
                })?
                .to_string();
            self.buffer.drain(..consumed);
            if line.is_empty() {
                if !self.data.is_empty() {
                    events.push(std::mem::take(&mut self.data));
                }
                continue;
            }
            if line.starts_with(':') {
                continue;
            }
            let (field, value) = line
                .split_once(':')
                .map_or((line.as_str(), ""), |(field, value)| {
                    (field, value.strip_prefix(' ').unwrap_or(value))
                });
            if field == "data" {
                if !self.data.is_empty() {
                    self.data.push('\n');
                }
                self.data.push_str(value);
            }
        }
        Ok(events)
    }
}

#[derive(Default)]
struct AnthropicNormalizer {
    next_content_index: usize,
    blocks: BTreeMap<usize, AnthropicBlock>,
    stop_reason: Option<String>,
    uncached_input_tokens: Option<u64>,
    usage: Usage,
}

enum AnthropicBlock {
    Text {
        content_index: usize,
    },
    Reasoning {
        content_index: usize,
    },
    Tool {
        content_index: usize,
        raw: String,
        saw_delta: bool,
    },
    ProviderTool {
        content_index: usize,
        id: String,
        name: String,
        action: Option<Value>,
    },
    Ignored,
}

impl AnthropicNormalizer {
    fn ingest(&mut self, value: &Value) -> AdapterResult<Vec<LanguageAdapterEvent>> {
        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mut output = Vec::new();
        match event_type {
            "message_start" => {
                if let Some(message) = value.get("message") {
                    self.update_usage(message.get("usage"));
                    let metadata = allowlisted_anthropic_metadata(message);
                    if !metadata.is_empty() {
                        output.push(LanguageAdapterEvent::Metadata { metadata });
                    }
                }
            }
            "content_block_start" => {
                let provider_index = provider_content_index(value)?;
                if self.blocks.contains_key(&provider_index) {
                    return Err(ProviderError::protocol(
                        "duplicate Anthropic content block start",
                    ));
                }
                let block = value.get("content_block").unwrap_or(&Value::Null);
                let block_type = block
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let state = match block_type {
                    "text" => {
                        let content_index = self.allocate();
                        output.push(LanguageAdapterEvent::TextStart { content_index });
                        if let Some(text) = block.get("text").and_then(Value::as_str)
                            && !text.is_empty()
                        {
                            output.push(LanguageAdapterEvent::TextDelta {
                                content_index,
                                delta: text.to_string(),
                            });
                        }
                        AnthropicBlock::Text { content_index }
                    }
                    "thinking" => {
                        let content_index = self.allocate();
                        output.push(LanguageAdapterEvent::ReasoningStart { content_index });
                        if let Some(text) = block.get("thinking").and_then(Value::as_str)
                            && !text.is_empty()
                        {
                            output.push(LanguageAdapterEvent::ReasoningDelta {
                                content_index,
                                delta: text.to_string(),
                                provider_evidence: None,
                            });
                        }
                        AnthropicBlock::Reasoning { content_index }
                    }
                    "tool_use" => {
                        let content_index = self.allocate();
                        let id = required_string(block, "id", "Anthropic tool use")?;
                        let name = required_string(block, "name", "Anthropic tool use")?;
                        output.push(LanguageAdapterEvent::ToolCallStart {
                            content_index,
                            id,
                            name,
                        });
                        let raw = block
                            .get("input")
                            .filter(|input| input.as_object().is_some_and(|v| !v.is_empty()))
                            .map(deterministic_json)
                            .transpose()?
                            .unwrap_or_default();
                        AnthropicBlock::Tool {
                            content_index,
                            raw,
                            saw_delta: false,
                        }
                    }
                    "server_tool_use" => {
                        let content_index = self.allocate();
                        let id = required_string(block, "id", "Anthropic server tool")?;
                        let name = required_string(block, "name", "Anthropic server tool")?;
                        let action = block.get("input").cloned();
                        output.push(LanguageAdapterEvent::ProviderToolStart {
                            content_index,
                            id: id.clone(),
                            name: name.clone(),
                            action: action.clone(),
                        });
                        AnthropicBlock::ProviderTool {
                            content_index,
                            id,
                            name,
                            action,
                        }
                    }
                    _ => AnthropicBlock::Ignored,
                };
                self.blocks.insert(provider_index, state);
            }
            "content_block_delta" => {
                let provider_index = provider_content_index(value)?;
                let delta = value.get("delta").unwrap_or(&Value::Null);
                let delta_type = delta
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if delta_type == "citations_delta" {
                    if let Some(source) = anthropic_citation_source(delta.get("citation")) {
                        let content_index = self.allocate();
                        output.push(LanguageAdapterEvent::Source {
                            content_index,
                            source,
                        });
                    }
                    return Ok(output);
                }
                let block = self.blocks.get_mut(&provider_index).ok_or_else(|| {
                    ProviderError::protocol("Anthropic content delta arrived before start")
                })?;
                match (block, delta_type) {
                    (AnthropicBlock::Text { content_index }, "text_delta") => {
                        output.push(LanguageAdapterEvent::TextDelta {
                            content_index: *content_index,
                            delta: delta
                                .get("text")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        });
                    }
                    (AnthropicBlock::Reasoning { content_index }, "thinking_delta") => {
                        output.push(LanguageAdapterEvent::ReasoningDelta {
                            content_index: *content_index,
                            delta: delta
                                .get("thinking")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            provider_evidence: None,
                        });
                    }
                    (AnthropicBlock::Reasoning { content_index }, "signature_delta") => {
                        output.push(LanguageAdapterEvent::ReasoningDelta {
                            content_index: *content_index,
                            delta: String::new(),
                            provider_evidence: Some(json!({
                                "signature": delta
                                    .get("signature")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default(),
                            })),
                        });
                    }
                    (
                        AnthropicBlock::Tool {
                            content_index,
                            raw,
                            saw_delta,
                        },
                        "input_json_delta",
                    ) => {
                        let partial = delta
                            .get("partial_json")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        if !*saw_delta {
                            raw.clear();
                            *saw_delta = true;
                        }
                        raw.push_str(partial);
                        output.push(LanguageAdapterEvent::ToolCallArgumentsDelta {
                            content_index: *content_index,
                            delta: partial.to_string(),
                        });
                    }
                    (AnthropicBlock::Ignored, _) => {}
                    _ => {
                        return Err(ProviderError::protocol(format!(
                            "Anthropic delta `{delta_type}` did not match its content block"
                        )));
                    }
                }
            }
            "content_block_stop" => {
                let provider_index = provider_content_index(value)?;
                let block = self.blocks.remove(&provider_index).ok_or_else(|| {
                    ProviderError::protocol("Anthropic content stop arrived before start")
                })?;
                match block {
                    AnthropicBlock::Text { content_index } => {
                        output.push(LanguageAdapterEvent::TextEnd { content_index });
                    }
                    AnthropicBlock::Reasoning { content_index } => {
                        output.push(LanguageAdapterEvent::ReasoningEnd { content_index });
                    }
                    AnthropicBlock::Tool {
                        content_index, raw, ..
                    } => {
                        output.push(LanguageAdapterEvent::ToolCallEnd {
                            content_index,
                            arguments_raw: if raw.is_empty() {
                                "{}".to_string()
                            } else {
                                raw
                            },
                        });
                    }
                    AnthropicBlock::ProviderTool {
                        content_index,
                        id,
                        name,
                        action,
                    } => {
                        output.push(LanguageAdapterEvent::ProviderToolEnd {
                            content_index,
                            id,
                            name,
                            action,
                            status: "completed".to_string(),
                        });
                    }
                    AnthropicBlock::Ignored => {}
                }
            }
            "message_delta" => {
                self.stop_reason = value
                    .pointer("/delta/stop_reason")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                self.update_usage(value.get("usage"));
            }
            "message_stop" => {
                if !self.blocks.is_empty() {
                    return Err(ProviderError::protocol(
                        "Anthropic message stopped with open content blocks",
                    ));
                }
                if self.usage != Usage::default() {
                    output.push(LanguageAdapterEvent::Usage {
                        usage: self.usage.clone(),
                    });
                }
                output.push(LanguageAdapterEvent::Finish {
                    finish_reason: self.stop_reason.take().map(anthropic_finish_reason),
                });
            }
            "error" => {
                return Err(ProviderError::provider(
                    ErrorPhase::Stream,
                    None,
                    value
                        .pointer("/error/type")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    value
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or("Anthropic stream reported an error"),
                ));
            }
            "ping" => {}
            _ => {
                // Unknown event families are ignored for forward compatibility.
            }
        }
        Ok(output)
    }

    fn allocate(&mut self) -> usize {
        let index = self.next_content_index;
        self.next_content_index += 1;
        index
    }

    fn update_usage(&mut self, usage: Option<&Value>) {
        let Some(usage) = usage else {
            return;
        };
        if let Some(value) = usage.get("input_tokens").and_then(Value::as_u64) {
            self.uncached_input_tokens = Some(value);
        }
        if let Some(value) = usage.get("output_tokens").and_then(Value::as_u64) {
            self.usage.output_tokens = Some(value);
        }
        if let Some(value) = usage.get("cache_read_input_tokens").and_then(Value::as_u64) {
            self.usage.cached_tokens = Some(value);
        }
        if let Some(value) = usage
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64)
        {
            self.usage.cache_write_tokens = Some(value);
        }
        self.usage.input_tokens = if self.uncached_input_tokens.is_some()
            || self.usage.cached_tokens.is_some()
            || self.usage.cache_write_tokens.is_some()
        {
            Some(
                self.uncached_input_tokens
                    .unwrap_or_default()
                    .saturating_add(self.usage.cached_tokens.unwrap_or_default())
                    .saturating_add(self.usage.cache_write_tokens.unwrap_or_default()),
            )
        } else {
            None
        };
        self.usage.total_tokens = match (self.usage.input_tokens, self.usage.output_tokens) {
            (Some(input), Some(output)) => Some(input.saturating_add(output)),
            _ => None,
        };
    }
}

fn provider_content_index(value: &Value) -> AdapterResult<usize> {
    value
        .get("index")
        .and_then(Value::as_u64)
        .and_then(|index| usize::try_from(index).ok())
        .ok_or_else(|| ProviderError::protocol("Anthropic content event omitted index"))
}

fn required_string(value: &Value, key: &str, context: &str) -> AdapterResult<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| ProviderError::protocol(format!("{context} omitted `{key}`")))
}

fn deterministic_json(value: &Value) -> AdapterResult<String> {
    let value = sort_json(value.clone());
    serde_json::to_string(&value).map_err(|error| {
        ProviderError::protocol(format!(
            "structured tool input could not be serialized: {error}"
        ))
    })
}

fn sort_json(value: Value) -> Value {
    match value {
        Value::Object(object) => {
            let sorted = object
                .into_iter()
                .map(|(key, value)| (key, sort_json(value)))
                .collect::<BTreeMap<_, _>>();
            Value::Object(Map::from_iter(sorted))
        }
        Value::Array(values) => Value::Array(values.into_iter().map(sort_json).collect()),
        value => value,
    }
}

fn anthropic_finish_reason(raw: String) -> FinishReason {
    let kind = match raw.as_str() {
        "end_turn" | "stop_sequence" => FinishReasonKind::Stop,
        "max_tokens" => FinishReasonKind::Length,
        "tool_use" => FinishReasonKind::ToolCalls,
        "refusal" => FinishReasonKind::ContentFilter,
        _ => FinishReasonKind::Other,
    };
    FinishReason {
        kind,
        raw: Some(raw),
    }
}

fn allowlisted_anthropic_metadata(value: &Value) -> BTreeMap<String, Value> {
    ["id", "model", "type"]
        .into_iter()
        .filter_map(|key| {
            value
                .get(key)
                .cloned()
                .map(|value| (key.to_string(), value))
        })
        .collect()
}

fn anthropic_citation_source(value: Option<&Value>) -> Option<crate::AssistantSource> {
    let value = value?;
    let url = value.get("url").and_then(Value::as_str)?.to_string();
    Some(crate::AssistantSource::UrlCitation(
        crate::UrlCitationSource {
            url,
            title: value
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            start_index: value
                .get("start_char_index")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok()),
            end_index: value
                .get("end_char_index")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok()),
        },
    ))
}

async fn read_bounded_error(response: reqwest::Response) -> Value {
    const LIMIT: usize = 64 * 1024;
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(next) = stream.next().await {
        let Ok(next) = next else {
            break;
        };
        let remaining = LIMIT.saturating_sub(bytes.len());
        bytes.extend_from_slice(&next[..next.len().min(remaining)]);
        if bytes.len() >= LIMIT {
            break;
        }
    }
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| json!({"error": {"message": String::from_utf8_lossy(&bytes)}}))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{
        AnthropicMessagesAdapter, AnthropicNormalizer, AnthropicSseParser, anthropic_finish_reason,
        anthropic_messages_endpoint,
    };
    use crate::{
        Capability, ErrorKind, ErrorPhase, FinishReason, FinishReasonKind, LanguageAdapterEvent,
        LanguageRequest, LanguageSettings, LanguageTool, Message, ModelDescriptor, ResponseFormat,
        ToolDeclaration,
    };

    fn descriptor() -> ModelDescriptor {
        ModelDescriptor {
            deployment_id: "anthropic".to_string(),
            provider_family: "anthropic".to_string(),
            capability: Capability::Language,
            model_id: "claude-test".to_string(),
            protocol_id: "anthropic_messages".to_string(),
        }
    }

    #[test]
    fn preview_translates_typed_messages_tools_and_warnings() {
        let request = LanguageRequest {
            messages: vec![Message::developer("policy"), Message::user("hello")],
            tools: vec![LanguageTool::from(ToolDeclaration::new(
                "read",
                "read a file",
                json!({"type": "object"}),
            ))],
            settings: LanguageSettings {
                frequency_penalty: Some(0.5),
                ..LanguageSettings::default()
            },
            ..LanguageRequest::default()
        };

        let preview = AnthropicMessagesAdapter::preview(&descriptor(), &request).expect("preview");

        assert_eq!(preview.body["model"], "claude-test");
        assert_eq!(preview.body["system"][0]["text"], "policy");
        assert_eq!(preview.body["messages"][0]["role"], "user");
        assert_eq!(preview.body["tools"][0]["name"], "read");
        assert_eq!(
            preview
                .warnings
                .iter()
                .map(|warning| warning.code.as_str())
                .collect::<Vec<_>>(),
            ["developer_role_folded", "unsupported_frequency_penalty"]
        );
    }

    #[test]
    fn preview_rejects_unsatisfied_structured_output() {
        let request = LanguageRequest {
            messages: vec![Message::user("hello")],
            settings: LanguageSettings {
                response_format: Some(ResponseFormat::JsonObject),
                ..LanguageSettings::default()
            },
            ..LanguageRequest::default()
        };

        let error =
            AnthropicMessagesAdapter::preview(&descriptor(), &request).expect_err("structured");
        assert_eq!(error.kind, ErrorKind::InvalidRequest);
        assert_eq!(error.phase, ErrorPhase::Preflight);
    }

    #[test]
    fn endpoint_normalization_is_stable() {
        assert_eq!(
            anthropic_messages_endpoint("https://api.anthropic.com"),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            anthropic_messages_endpoint("https://proxy.example/v1/"),
            "https://proxy.example/v1/messages"
        );
        assert_eq!(
            anthropic_messages_endpoint("https://proxy.example/v1/messages"),
            "https://proxy.example/v1/messages"
        );
    }

    #[test]
    fn chunked_sse_and_normalizer_preserve_order_usage_and_finish() {
        let payload = concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",",
            "\"model\":\"claude-test\",\"type\":\"message\",",
            "\"usage\":{\"input_tokens\":3,\"cache_read_input_tokens\":5,",
            "\"cache_creation_input_tokens\":7}}}\r\n\r\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,",
            "\"content_block\":{\"type\":\"text\",\"text\":\"Hel\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,",
            "\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},",
            "\"usage\":{\"output_tokens\":2}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        let bytes = payload.as_bytes();
        let mut parser = AnthropicSseParser::default();
        let mut encoded_events = Vec::new();
        for chunk in bytes.chunks(7) {
            encoded_events.extend(parser.push(chunk).expect("SSE chunk"));
        }
        encoded_events.extend(parser.finish().expect("SSE finish"));

        let mut normalizer = AnthropicNormalizer::default();
        let events = encoded_events
            .into_iter()
            .flat_map(|encoded| {
                let value = serde_json::from_str::<Value>(&encoded).expect("event JSON");
                normalizer.ingest(&value).expect("normalized event")
            })
            .collect::<Vec<_>>();

        assert!(matches!(
            &events[0],
            LanguageAdapterEvent::Metadata { metadata }
                if metadata.get("id") == Some(&json!("msg_1"))
        ));
        assert!(matches!(
            &events[1],
            LanguageAdapterEvent::TextStart { content_index: 0 }
        ));
        assert!(matches!(
            &events[2],
            LanguageAdapterEvent::TextDelta { content_index: 0, delta }
                if delta == "Hel"
        ));
        assert!(matches!(
            &events[3],
            LanguageAdapterEvent::TextDelta { content_index: 0, delta }
                if delta == "lo"
        ));
        assert!(matches!(
            &events[4],
            LanguageAdapterEvent::TextEnd { content_index: 0 }
        ));
        assert!(matches!(
            &events[5],
            LanguageAdapterEvent::Usage { usage }
                if usage.input_tokens == Some(15)
                    && usage.output_tokens == Some(2)
                    && usage.total_tokens == Some(17)
                    && usage.cached_tokens == Some(5)
                    && usage.cache_write_tokens == Some(7)
        ));
        assert!(matches!(
            &events[6],
            LanguageAdapterEvent::Finish {
                finish_reason: Some(FinishReason {
                    kind: FinishReasonKind::Stop,
                    ..
                })
            }
        ));
    }

    #[test]
    fn refusal_normalizes_to_content_filter() {
        let finish_reason = anthropic_finish_reason("refusal".to_string());
        assert_eq!(finish_reason.kind, FinishReasonKind::ContentFilter);
        assert_eq!(finish_reason.raw.as_deref(), Some("refusal"));
    }

    #[test]
    fn sse_parser_rejects_invalid_utf8() {
        let mut parser = AnthropicSseParser::default();
        let error = parser
            .push(b"data: \xff\n\n")
            .expect_err("invalid UTF-8 must fail");
        assert_eq!(error.kind, ErrorKind::Protocol);
    }
}
