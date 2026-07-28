use std::collections::{BTreeMap, VecDeque};

use futures::{StreamExt, stream};
use serde_json::{Value, json};

use crate::{
    AdapterCall, AdapterFuture, AdapterResult, AdapterStream, AssistantContent, ErrorKind,
    ErrorPhase, FinishReason, FinishReasonKind, GenerationProvider, GenerationRequest,
    GenerationStream, GenerationTool, HostedWebSearchTool, LanguageAdapter, LanguageAdapterEvent,
    LanguageRequest, LanguageTool, MediaInput, Message, ModelDescriptor, ModelTarget, Outcome,
    ProviderError, RequestPreview, StreamEvent, TextContent, ToolChoice, Usage, UserContent,
    Warning,
};
use crate::{
    OpenAiChatProvider, OpenAiResponsesProvider, allowlisted_provider_metadata,
    model_metadata_disables_image_input, normalize_usage, openai_chat_request_body,
    openai_responses_request_body,
};

const OPENAI_PREVIEW_BASE_URL: &str = "https://api.openai.com/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenAiLanguageProtocol {
    Chat,
    Responses,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OpenAiChatAdapter;

impl OpenAiChatAdapter {
    pub fn endpoint(base_url: &str) -> String {
        crate::openai_chat_completions_endpoint(base_url)
    }

    pub fn preview(
        descriptor: &ModelDescriptor,
        request: &LanguageRequest,
    ) -> AdapterResult<RequestPreview> {
        preview_with_protocol(descriptor, request, OpenAiLanguageProtocol::Chat)
    }
}

impl LanguageAdapter for OpenAiChatAdapter {
    fn stream(
        &self,
        call: AdapterCall<LanguageRequest>,
    ) -> AdapterFuture<'_, AdapterStream<LanguageAdapterEvent>> {
        Box::pin(async move {
            let (request, warnings) = legacy_request(
                &call.context.model,
                call.context.profile.as_ref(),
                call.request,
                OpenAiLanguageProtocol::Chat,
            )?;
            let api_key = call
                .context
                .credentials
                .get("api_key")
                .map(|value| value.expose_secret().to_string())
                .unwrap_or_default();
            let provider = OpenAiChatProvider {
                client: call.context.client,
                base_url: call.context.endpoint,
                api_key,
                provider_name: call.context.model.provider_family,
                inference_idle_timeout: None,
                headers: call.context.headers,
                allow_image_text_fallback: false,
            };
            let stream = provider
                .stream(request, call.context.abort)
                .await
                .map_err(|error| crate::sdk_error::legacy_error(error, ErrorPhase::Dispatch))?;
            Ok(adapt_legacy_stream(stream, warnings))
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OpenAiResponsesAdapter;

impl OpenAiResponsesAdapter {
    pub fn endpoint(base_url: &str) -> String {
        crate::responses_endpoint(base_url)
    }

    pub fn preview(
        descriptor: &ModelDescriptor,
        request: &LanguageRequest,
    ) -> AdapterResult<RequestPreview> {
        preview_with_protocol(descriptor, request, OpenAiLanguageProtocol::Responses)
    }
}

impl LanguageAdapter for OpenAiResponsesAdapter {
    fn stream(
        &self,
        call: AdapterCall<LanguageRequest>,
    ) -> AdapterFuture<'_, AdapterStream<LanguageAdapterEvent>> {
        Box::pin(async move {
            let (request, warnings) = legacy_request(
                &call.context.model,
                call.context.profile.as_ref(),
                call.request,
                OpenAiLanguageProtocol::Responses,
            )?;
            let api_key = call
                .context
                .credentials
                .get("api_key")
                .map(|value| value.expose_secret().to_string())
                .unwrap_or_default();
            let provider = OpenAiResponsesProvider {
                client: call.context.client,
                base_url: call.context.endpoint,
                api_key,
                inference_idle_timeout: None,
                headers: call.context.headers,
            };
            let stream = provider
                .stream(request, call.context.abort)
                .await
                .map_err(|error| crate::sdk_error::legacy_error(error, ErrorPhase::Dispatch))?;
            Ok(adapt_legacy_stream(stream, warnings))
        })
    }
}

pub fn preview_request(
    descriptor: &ModelDescriptor,
    request: &LanguageRequest,
) -> AdapterResult<RequestPreview> {
    match descriptor.protocol_id.as_str() {
        "openai_chat" => OpenAiChatAdapter::preview(descriptor, request),
        "openai_responses" => OpenAiResponsesAdapter::preview(descriptor, request),
        protocol => Err(ProviderError::configuration(format!(
            "request preview is not available for protocol `{protocol}`"
        ))),
    }
}

fn preview_with_protocol(
    descriptor: &ModelDescriptor,
    request: &LanguageRequest,
    protocol: OpenAiLanguageProtocol,
) -> AdapterResult<RequestPreview> {
    let (request, warnings) = legacy_request(descriptor, None, request.clone(), protocol)?;
    let body = match protocol {
        OpenAiLanguageProtocol::Chat => openai_chat_request_body(&request, OPENAI_PREVIEW_BASE_URL),
        OpenAiLanguageProtocol::Responses => {
            openai_responses_request_body(&request, OPENAI_PREVIEW_BASE_URL)
        }
    };
    Ok(RequestPreview { body, warnings })
}

fn legacy_request(
    descriptor: &ModelDescriptor,
    profile: Option<&crate::ModelProfile>,
    request: LanguageRequest,
    protocol: OpenAiLanguageProtocol,
) -> AdapterResult<(GenerationRequest, Vec<Warning>)> {
    validate_semantic_requirements(&request, protocol)?;
    let has_images = request.messages.iter().any(message_has_image);
    let mut metadata = request
        .extensions
        .get("psychevo")
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    if has_images
        && (profile
            .and_then(|profile| profile.capabilities.get("image_input"))
            .is_some_and(|supported| !supported)
            || model_metadata_disables_image_input(&metadata))
    {
        return Err(ProviderError::invalid_request(
            "the selected model profile does not support image input",
        ));
    }
    metadata["_psychevo_ai_settings"] =
        serde_json::to_value(&request.settings).map_err(|error| {
            ProviderError::new(
                ErrorKind::InvalidRequest,
                ErrorPhase::Preflight,
                format!("language settings could not be encoded: {error}"),
            )
        })?;
    if let Some(effort) = request.settings.reasoning_effort.as_deref() {
        metadata["reasoning_effort"] = Value::String(effort.to_string());
    }
    if protocol == OpenAiLanguageProtocol::Responses {
        ensure_developer_role_capability(&mut metadata);
    }

    let mut warnings = setting_warnings(&request, protocol);
    if protocol == OpenAiLanguageProtocol::Chat
        && request
            .messages
            .iter()
            .any(|message| matches!(message, Message::Developer { .. }))
        && !profile
            .and_then(|profile| profile.capabilities.get("developer_role"))
            .copied()
            .unwrap_or_else(|| crate::capability_is_true(&metadata, "developer_role"))
    {
        warnings.push(Warning::new(
            "developer_role_folded",
            "developer messages were folded into system messages",
        ));
    }

    let messages = request
        .messages
        .iter()
        .map(message_to_legacy)
        .collect::<AdapterResult<Vec<_>>>()?;
    let tools = request
        .tools
        .into_iter()
        .map(|tool| tool_to_legacy(tool, protocol))
        .collect::<AdapterResult<Vec<_>>>()?;
    Ok((
        GenerationRequest {
            model: ModelTarget {
                provider: descriptor.provider_family.clone(),
                model: descriptor.model_id.clone(),
            },
            messages,
            tools,
            metadata,
        },
        warnings,
    ))
}

fn validate_semantic_requirements(
    request: &LanguageRequest,
    protocol: OpenAiLanguageProtocol,
) -> AdapterResult<()> {
    if protocol == OpenAiLanguageProtocol::Chat
        && request
            .tools
            .iter()
            .any(|tool| matches!(tool, LanguageTool::WebSearch(_)))
    {
        return Err(ProviderError::invalid_request(
            "OpenAI Chat does not support the hosted web-search tool",
        ));
    }
    if request
        .tools
        .iter()
        .any(|tool| matches!(tool, LanguageTool::Extension { .. }))
    {
        return Err(ProviderError::invalid_request(
            "this OpenAI Adapter does not support extension tools",
        ));
    }
    if matches!(
        request.settings.tool_choice,
        Some(ToolChoice::Required | ToolChoice::Tool { .. })
    ) && !request
        .tools
        .iter()
        .any(|tool| matches!(tool, LanguageTool::Function { .. }))
    {
        return Err(ProviderError::invalid_request(
            "required tool choice needs at least one function declaration",
        ));
    }
    Ok(())
}

fn setting_warnings(request: &LanguageRequest, protocol: OpenAiLanguageProtocol) -> Vec<Warning> {
    if protocol != OpenAiLanguageProtocol::Responses {
        return Vec::new();
    }
    let mut warnings = Vec::new();
    if request.settings.frequency_penalty.is_some() {
        warnings.push(Warning::new(
            "unsupported_frequency_penalty",
            "OpenAI Responses omitted frequency_penalty",
        ));
    }
    if request.settings.presence_penalty.is_some() {
        warnings.push(Warning::new(
            "unsupported_presence_penalty",
            "OpenAI Responses omitted presence_penalty",
        ));
    }
    if !request.settings.stop_sequences.is_empty() {
        warnings.push(Warning::new(
            "unsupported_stop_sequences",
            "OpenAI Responses omitted stop_sequences",
        ));
    }
    if request.settings.seed.is_some() {
        warnings.push(Warning::new(
            "unsupported_seed",
            "OpenAI Responses omitted seed",
        ));
    }
    warnings
}

fn ensure_developer_role_capability(metadata: &mut Value) {
    let Some(object) = metadata.as_object_mut() else {
        return;
    };
    let model_metadata = object.entry("model_metadata").or_insert_with(|| json!({}));
    let Some(model_metadata) = model_metadata.as_object_mut() else {
        return;
    };
    let capabilities = model_metadata
        .entry("capabilities")
        .or_insert_with(|| json!({}));
    if let Some(capabilities) = capabilities.as_object_mut() {
        capabilities
            .entry("developer_role")
            .or_insert(Value::Bool(true));
    }
}

fn message_has_image(message: &Message) -> bool {
    matches!(
        message,
        Message::User { content, .. }
            if content.iter().any(|content| matches!(content, UserContent::Image(_)))
    )
}

fn message_to_legacy(message: &Message) -> AdapterResult<Value> {
    match message {
        Message::System {
            content,
            extensions,
        } => Ok(with_message_metadata(
            json!({"role": "system", "content": joined_text(content)}),
            extensions,
        )),
        Message::Developer {
            content,
            extensions,
        } => Ok(with_message_metadata(
            json!({"role": "developer", "content": joined_text(content)}),
            extensions,
        )),
        Message::User {
            content,
            extensions,
        } => {
            let content = content
                .iter()
                .map(user_content_to_legacy)
                .collect::<AdapterResult<Vec<_>>>()?;
            Ok(with_message_metadata(
                json!({"role": "user", "content": content}),
                extensions,
            ))
        }
        Message::Assistant { message } => {
            let content = message
                .content
                .iter()
                .map(assistant_content_to_legacy)
                .collect::<AdapterResult<Vec<_>>>()?;
            Ok(with_message_metadata(
                json!({"role": "assistant", "content": content}),
                &message.extensions,
            ))
        }
        Message::Tool {
            tool_call_id,
            tool_name,
            content,
            is_error,
            extensions,
        } => Ok(with_message_metadata(
            json!({
                "role": "tool_result",
                "tool_call_id": tool_call_id,
                "tool_name": tool_name,
                "content": joined_text(content),
                "is_error": is_error,
            }),
            extensions,
        )),
    }
}

fn user_content_to_legacy(content: &UserContent) -> AdapterResult<Value> {
    match content {
        UserContent::Text(content) => Ok(json!({"text": content.text})),
        UserContent::Image(content) => match &content.source {
            MediaInput::Inline { media } => Ok(json!({
                "type": "image_url",
                "url": format!(
                    "data:{};base64,{}",
                    media.mime_type(),
                    media.base64().map_err(|error| ProviderError::invalid_request(error.to_string()))?
                ),
            })),
            MediaInput::Url { url, .. } => Ok(json!({
                "type": "image_url",
                "url": url,
            })),
        },
        UserContent::Extension { namespace, .. } => Err(ProviderError::invalid_request(format!(
            "unsupported user-content extension `{namespace}`"
        ))),
    }
}

fn assistant_content_to_legacy(content: &AssistantContent) -> AdapterResult<Value> {
    match content {
        AssistantContent::Text(content) => Ok(json!({
            "type": "text",
            "text": content.text,
        })),
        AssistantContent::Reasoning {
            text,
            provider_evidence,
        } => Ok(json!({
            "type": "reasoning",
            "text": text,
            "provider_evidence": provider_evidence,
        })),
        AssistantContent::ToolCall(call) => Ok(json!({
            "type": "tool_call",
            "id": call.id,
            "name": call.name,
            "arguments": call.arguments,
            "arguments_json": call.arguments_raw,
            "arguments_error": call.argument_error,
        })),
        AssistantContent::ProviderTool(tool) => Ok(json!({
            "type": "provider_tool",
            "id": tool.id,
            "name": tool.name,
            "action": tool.action,
            "status": tool.status,
        })),
        AssistantContent::Source { source } => Ok(json!({
            "type": "source",
            "source": source,
        })),
        AssistantContent::Extension { namespace, .. } => Err(ProviderError::invalid_request(
            format!("unsupported assistant-content extension `{namespace}`"),
        )),
    }
}

fn joined_text(content: &[TextContent]) -> String {
    content
        .iter()
        .map(|content| content.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn with_message_metadata(mut value: Value, extensions: &BTreeMap<String, Value>) -> Value {
    if let Some(metadata) = extensions.get("psychevo") {
        value["metadata"] = metadata.clone();
    } else if !extensions.is_empty() {
        value["metadata"] = json!({"extensions": extensions});
    }
    value
}

fn tool_to_legacy(
    tool: LanguageTool,
    protocol: OpenAiLanguageProtocol,
) -> AdapterResult<GenerationTool> {
    match tool {
        LanguageTool::Function { declaration } => Ok(GenerationTool::Function { declaration }),
        LanguageTool::WebSearch(tool) if protocol == OpenAiLanguageProtocol::Responses => {
            let mut config = tool
                .extensions
                .get("openai")
                .cloned()
                .filter(Value::is_object)
                .unwrap_or_else(|| json!({}));
            if !tool.allowed_domains.is_empty() {
                config["filters"]["allowed_domains"] = json!(tool.allowed_domains);
            }
            if !tool.blocked_domains.is_empty() {
                config["filters"]["blocked_domains"] = json!(tool.blocked_domains);
            }
            if let Some(size) = tool.search_context_size {
                config["search_context_size"] = json!(size);
            }
            if let Some(location) = tool.user_location {
                config["user_location"] = location;
            }
            Ok(GenerationTool::WebSearch(HostedWebSearchTool { config }))
        }
        LanguageTool::WebSearch(_) => Err(ProviderError::invalid_request(
            "hosted web search requires OpenAI Responses",
        )),
        LanguageTool::Extension { namespace, .. } => Err(ProviderError::invalid_request(format!(
            "unsupported tool extension `{namespace}`"
        ))),
    }
}

fn adapt_legacy_stream(
    stream: GenerationStream,
    warnings: Vec<Warning>,
) -> AdapterStream<LanguageAdapterEvent> {
    let state = LegacyStreamState {
        stream,
        normalizer: LegacyNormalizer::default(),
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
            match state.stream.next().await {
                Some(Ok(event)) => match state.normalizer.ingest(event) {
                    Ok(events) => state.pending.extend(events.into_iter().map(Ok)),
                    Err(error) => {
                        return Some((Err(error), state));
                    }
                },
                Some(Err(error)) => {
                    state.done = true;
                    return Some((
                        Err(crate::sdk_error::legacy_error(error, ErrorPhase::Stream)),
                        state,
                    ));
                }
                None => {
                    return None;
                }
            }
        }
    }))
}

struct LegacyStreamState {
    stream: GenerationStream,
    normalizer: LegacyNormalizer,
    pending: VecDeque<AdapterResult<LanguageAdapterEvent>>,
    done: bool,
}

#[derive(Default)]
struct LegacyNormalizer {
    next_content_index: usize,
    text: Option<usize>,
    reasoning: Option<usize>,
    tools: BTreeMap<(usize, usize), LegacyTool>,
    provider_tools: BTreeMap<String, usize>,
}

struct LegacyTool {
    content_index: usize,
    arguments: String,
}

impl LegacyNormalizer {
    fn ingest(&mut self, event: StreamEvent) -> AdapterResult<Vec<LanguageAdapterEvent>> {
        let mut output = Vec::new();
        match event {
            StreamEvent::TextDelta { text } => {
                let index = match self.text {
                    Some(index) => index,
                    None => {
                        let index = self.allocate();
                        self.text = Some(index);
                        output.push(LanguageAdapterEvent::TextStart {
                            content_index: index,
                        });
                        index
                    }
                };
                output.push(LanguageAdapterEvent::TextDelta {
                    content_index: index,
                    delta: text,
                });
            }
            StreamEvent::ReasoningDelta {
                text,
                reasoning_content,
            } => {
                let index = self.reasoning_index(&mut output);
                output.push(LanguageAdapterEvent::ReasoningDelta {
                    content_index: index,
                    delta: text,
                    provider_evidence: reasoning_content
                        .map(|value| json!({"reasoning_content": value})),
                });
            }
            StreamEvent::ReasoningDetails { details } => {
                let index = self.reasoning_index(&mut output);
                output.push(LanguageAdapterEvent::ReasoningDelta {
                    content_index: index,
                    delta: String::new(),
                    provider_evidence: Some(json!({
                        "reasoning_details": details,
                    })),
                });
            }
            StreamEvent::ToolCallStart {
                content_index,
                call_index,
                id,
                name,
            } => {
                let key = (content_index, call_index);
                if self.tools.contains_key(&key) {
                    return Err(ProviderError::protocol("duplicate tool-call start"));
                }
                let canonical = self.allocate();
                self.tools.insert(
                    key,
                    LegacyTool {
                        content_index: canonical,
                        arguments: String::new(),
                    },
                );
                output.push(LanguageAdapterEvent::ToolCallStart {
                    content_index: canonical,
                    id,
                    name,
                });
            }
            StreamEvent::ToolCallDelta {
                content_index,
                call_index,
                arguments_delta,
                ..
            } => {
                let tool = self
                    .tools
                    .get_mut(&(content_index, call_index))
                    .ok_or_else(|| {
                        ProviderError::protocol("tool-call delta arrived before start")
                    })?;
                tool.arguments.push_str(&arguments_delta);
                output.push(LanguageAdapterEvent::ToolCallArgumentsDelta {
                    content_index: tool.content_index,
                    delta: arguments_delta,
                });
            }
            StreamEvent::ToolCallEnd {
                content_index,
                call_index,
            } => {
                let tool = self
                    .tools
                    .remove(&(content_index, call_index))
                    .ok_or_else(|| ProviderError::protocol("tool-call end arrived before start"))?;
                output.push(LanguageAdapterEvent::ToolCallEnd {
                    content_index: tool.content_index,
                    arguments_raw: tool.arguments,
                });
            }
            StreamEvent::ProviderToolStart { id, name, action } => {
                let index = self.allocate();
                if self.provider_tools.insert(id.clone(), index).is_some() {
                    return Err(ProviderError::protocol("duplicate provider-tool start"));
                }
                output.push(LanguageAdapterEvent::ProviderToolStart {
                    content_index: index,
                    id,
                    name,
                    action,
                });
            }
            StreamEvent::ProviderToolEnd {
                id,
                name,
                action,
                status,
            } => {
                let index = self.provider_tools.remove(&id).ok_or_else(|| {
                    ProviderError::protocol("provider-tool end arrived before start")
                })?;
                output.push(LanguageAdapterEvent::ProviderToolEnd {
                    content_index: index,
                    id,
                    name,
                    action,
                    status,
                });
            }
            StreamEvent::Source { source } => {
                let index = self.allocate();
                output.push(LanguageAdapterEvent::Source {
                    content_index: index,
                    source,
                });
            }
            StreamEvent::Usage { usage } => {
                if let Some(usage) = typed_usage(&usage) {
                    output.push(LanguageAdapterEvent::Usage { usage });
                }
            }
            StreamEvent::Metadata { metadata } => {
                if let Some(metadata) = typed_metadata(&metadata) {
                    output.push(LanguageAdapterEvent::Metadata { metadata });
                }
            }
            StreamEvent::Done {
                outcome,
                finish_reason,
            } => {
                if let Some(index) = self.reasoning.take() {
                    output.push(LanguageAdapterEvent::ReasoningEnd {
                        content_index: index,
                    });
                }
                if let Some(index) = self.text.take() {
                    output.push(LanguageAdapterEvent::TextEnd {
                        content_index: index,
                    });
                }
                for (_, tool) in std::mem::take(&mut self.tools) {
                    output.push(LanguageAdapterEvent::ToolCallEnd {
                        content_index: tool.content_index,
                        arguments_raw: tool.arguments,
                    });
                }
                if !self.provider_tools.is_empty() {
                    return Err(ProviderError::protocol(
                        "provider stream finished with an open provider tool",
                    ));
                }
                match outcome {
                    Outcome::Normal | Outcome::Stopped => {
                        output.push(LanguageAdapterEvent::Finish {
                            finish_reason: normalize_finish_reason(outcome, finish_reason),
                        });
                    }
                    Outcome::Failed => {
                        return Err(ProviderError::provider(
                            ErrorPhase::Stream,
                            None,
                            None,
                            "provider reported a failed generation outcome",
                        ));
                    }
                    Outcome::Aborted => {
                        return Err(ProviderError::aborted(ErrorPhase::Stream));
                    }
                }
            }
        }
        Ok(output)
    }

    fn allocate(&mut self) -> usize {
        let index = self.next_content_index;
        self.next_content_index += 1;
        index
    }

    fn reasoning_index(&mut self, output: &mut Vec<LanguageAdapterEvent>) -> usize {
        match self.reasoning {
            Some(index) => index,
            None => {
                let index = self.allocate();
                self.reasoning = Some(index);
                output.push(LanguageAdapterEvent::ReasoningStart {
                    content_index: index,
                });
                index
            }
        }
    }
}

fn typed_usage(value: &Value) -> Option<Usage> {
    let value = normalize_usage(value)?;
    Some(Usage {
        input_tokens: value.get("input_tokens").and_then(Value::as_u64),
        output_tokens: value.get("output_tokens").and_then(Value::as_u64),
        total_tokens: value.get("total_tokens").and_then(Value::as_u64),
        reasoning_tokens: value.get("reasoning_tokens").and_then(Value::as_u64),
        cached_tokens: value.get("cached_tokens").and_then(Value::as_u64),
        cache_write_tokens: value.get("cache_write_tokens").and_then(Value::as_u64),
        provider_reported_cost: None,
        provider_metadata: BTreeMap::new(),
    })
}

fn typed_metadata(value: &Value) -> Option<BTreeMap<String, Value>> {
    allowlisted_provider_metadata(value)?
        .as_object()
        .cloned()
        .map(|object| object.into_iter().collect())
}

fn normalize_finish_reason(outcome: Outcome, raw: Option<String>) -> Option<FinishReason> {
    if raw.is_none() && outcome == Outcome::Normal {
        return None;
    }
    let kind = if outcome == Outcome::Stopped {
        FinishReasonKind::Length
    } else {
        match raw.as_deref() {
            Some("stop" | "completed") => FinishReasonKind::Stop,
            Some("length" | "max_tokens" | "max_output_tokens") => FinishReasonKind::Length,
            Some("tool_calls" | "tool_call") => FinishReasonKind::ToolCalls,
            Some("content_filter") => FinishReasonKind::ContentFilter,
            _ => FinishReasonKind::Other,
        }
    };
    Some(FinishReason { kind, raw })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_filter_normalizes_to_content_filter() {
        let finish_reason =
            normalize_finish_reason(Outcome::Normal, Some("content_filter".to_string()))
                .expect("finish reason");
        assert_eq!(finish_reason.kind, FinishReasonKind::ContentFilter);
        assert_eq!(finish_reason.raw.as_deref(), Some("content_filter"));
    }
}
