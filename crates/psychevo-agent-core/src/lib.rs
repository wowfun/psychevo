use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use futures::StreamExt;
use futures::future::{BoxFuture, join_all};
use psychevo_ai::{
    AbortSignal, GenerationProvider, GenerationRequest, Outcome, StreamEvent, ToolDeclaration,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::sync::watch;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("provider failed: {0}")]
    Provider(#[from] psychevo_ai::Error),
    #[error("event sink failed: {0}")]
    EventSink(String),
    #[error("agent failed: {0}")]
    Agent(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Message {
    User {
        content: Vec<TextBlock>,
        timestamp_ms: i64,
    },
    Assistant {
        content: Vec<AssistantBlock>,
        timestamp_ms: i64,
        finish_reason: Option<String>,
        outcome: Outcome,
        model: Option<String>,
        provider: Option<String>,
    },
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        content: String,
        is_error: bool,
        timestamp_ms: i64,
    },
}

impl Message {
    pub fn role(&self) -> &'static str {
        match self {
            Self::User { .. } => "user",
            Self::Assistant { .. } => "assistant",
            Self::ToolResult { .. } => "tool_result",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextBlock {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssistantBlock {
    Text {
        text: String,
    },
    Reasoning {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_evidence: Option<Value>,
    },
    ToolCall(ToolCallBlock),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCallBlock {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    pub arguments_json: String,
    pub arguments_error: Option<String>,
    pub content_index: usize,
    pub call_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolExecutionMode {
    Parallel,
    Sequential,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolOutput {
    pub json: Value,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn ok(json: Value) -> Self {
        Self {
            json,
            is_error: false,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            json: json!({ "error": message.into() }),
            is_error: true,
        }
    }
}

pub trait ToolBinding: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;
    fn execution_mode(&self) -> ToolExecutionMode;
    fn execute(
        &self,
        tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    AgentStart,
    AgentEnd {
        outcome: Outcome,
        messages: Vec<Message>,
    },
    TurnStart {
        turn_index: usize,
    },
    TurnEnd {
        turn_index: usize,
        outcome: Outcome,
    },
    MessageStart {
        message: Message,
    },
    MessageUpdate {
        message: Message,
    },
    MessageEnd {
        message: Message,
    },
    ReasoningDelta {
        text: String,
    },
    ReasoningEnd {
        text: String,
    },
    ToolExecutionStart {
        tool_call_id: String,
        tool_name: String,
        args: Value,
    },
    ToolExecutionUpdate {
        tool_call_id: String,
        tool_name: String,
        partial_result: Value,
    },
    ToolExecutionEnd {
        tool_call_id: String,
        tool_name: String,
        result: Value,
        outcome: Outcome,
    },
}

pub trait EventSink: Send + Sync {
    fn emit(&self, event: AgentEvent) -> BoxFuture<'static, Result<()>>;
}

#[derive(Clone)]
pub struct ControlHandle {
    stop_tx: watch::Sender<bool>,
    abort_tx: watch::Sender<bool>,
}

pub struct ControlReceivers {
    stop_rx: watch::Receiver<bool>,
    abort_rx: watch::Receiver<bool>,
}

impl ControlHandle {
    pub fn new() -> (Self, ControlReceivers) {
        let (stop_tx, stop_rx) = watch::channel(false);
        let (abort_tx, abort_rx) = watch::channel(false);
        (
            Self { stop_tx, abort_tx },
            ControlReceivers { stop_rx, abort_rx },
        )
    }

    pub fn stop(&self) {
        let _ = self.stop_tx.send(true);
    }

    pub fn abort(&self) {
        let _ = self.abort_tx.send(true);
    }
}

impl ControlReceivers {
    fn stop_requested(&self) -> bool {
        *self.stop_rx.borrow()
    }

    fn abort_requested(&self) -> bool {
        *self.abort_rx.borrow()
    }

    fn abort_signal(&self) -> AbortSignal {
        AbortSignal::new(self.abort_rx.clone())
    }
}

#[derive(Clone)]
pub struct AgentLoopRequest {
    pub model_provider: String,
    pub model: String,
    pub generation_metadata: Value,
    pub previous_messages: Vec<Message>,
    pub prompt_messages: Vec<Message>,
    pub tools: Vec<Arc<dyn ToolBinding>>,
    pub max_turns: usize,
}

#[derive(Debug, Clone)]
pub struct AgentCompletion {
    pub outcome: Outcome,
    pub messages: Vec<Message>,
}

pub async fn run_agent_loop(
    provider: Arc<dyn GenerationProvider>,
    request: AgentLoopRequest,
    sink: Arc<dyn EventSink>,
    control: ControlReceivers,
) -> Result<AgentCompletion> {
    emit(&sink, AgentEvent::AgentStart).await?;

    if control.abort_requested() {
        let completion = AgentCompletion {
            outcome: Outcome::Aborted,
            messages: Vec::new(),
        };
        emit(
            &sink,
            AgentEvent::AgentEnd {
                outcome: completion.outcome,
                messages: completion.messages.clone(),
            },
        )
        .await?;
        return Ok(completion);
    }

    let mut context = request.previous_messages.clone();
    let mut new_messages = Vec::new();
    let mut turn_index = 0usize;

    emit(&sink, AgentEvent::TurnStart { turn_index }).await?;
    for message in request.prompt_messages.iter().cloned() {
        context.push(message.clone());
        new_messages.push(message.clone());
        emit(
            &sink,
            AgentEvent::MessageStart {
                message: message.clone(),
            },
        )
        .await?;
        emit(&sink, AgentEvent::MessageEnd { message }).await?;
    }

    loop {
        if turn_index >= request.max_turns {
            let outcome = Outcome::Failed;
            emit(
                &sink,
                AgentEvent::TurnEnd {
                    turn_index,
                    outcome,
                },
            )
            .await?;
            emit(
                &sink,
                AgentEvent::AgentEnd {
                    outcome,
                    messages: new_messages.clone(),
                },
            )
            .await?;
            return Ok(AgentCompletion {
                outcome,
                messages: new_messages,
            });
        }

        if control.abort_requested() {
            let outcome = Outcome::Aborted;
            emit(
                &sink,
                AgentEvent::TurnEnd {
                    turn_index,
                    outcome,
                },
            )
            .await?;
            emit(
                &sink,
                AgentEvent::AgentEnd {
                    outcome,
                    messages: new_messages.clone(),
                },
            )
            .await?;
            return Ok(AgentCompletion {
                outcome,
                messages: new_messages,
            });
        }

        let assistant = stream_assistant(
            Arc::clone(&provider),
            &request,
            &context,
            Arc::clone(&sink),
            control.abort_signal(),
        )
        .await?;

        let assistant_outcome = assistant_outcome(&assistant);
        context.push(assistant.clone());
        new_messages.push(assistant.clone());

        if assistant_outcome != Outcome::Normal {
            emit(
                &sink,
                AgentEvent::TurnEnd {
                    turn_index,
                    outcome: assistant_outcome,
                },
            )
            .await?;
            emit(
                &sink,
                AgentEvent::AgentEnd {
                    outcome: assistant_outcome,
                    messages: new_messages.clone(),
                },
            )
            .await?;
            return Ok(AgentCompletion {
                outcome: assistant_outcome,
                messages: new_messages,
            });
        }

        let tool_calls = assistant_tool_calls(&assistant);
        if !tool_calls.is_empty() {
            let tool_results = execute_tool_batch(
                &request.tools,
                &tool_calls,
                Arc::clone(&sink),
                control.abort_signal(),
            )
            .await?;
            for result in tool_results {
                context.push(result.clone());
                new_messages.push(result.clone());
                emit(
                    &sink,
                    AgentEvent::MessageStart {
                        message: result.clone(),
                    },
                )
                .await?;
                emit(&sink, AgentEvent::MessageEnd { message: result }).await?;
            }
        }

        let terminal = if control.abort_requested() {
            Some(Outcome::Aborted)
        } else if control.stop_requested() {
            Some(Outcome::Stopped)
        } else if tool_calls.is_empty() {
            Some(Outcome::Normal)
        } else {
            None
        };

        if let Some(outcome) = terminal {
            emit(
                &sink,
                AgentEvent::TurnEnd {
                    turn_index,
                    outcome,
                },
            )
            .await?;
            emit(
                &sink,
                AgentEvent::AgentEnd {
                    outcome,
                    messages: new_messages.clone(),
                },
            )
            .await?;
            return Ok(AgentCompletion {
                outcome,
                messages: new_messages,
            });
        }

        emit(
            &sink,
            AgentEvent::TurnEnd {
                turn_index,
                outcome: Outcome::Normal,
            },
        )
        .await?;
        turn_index += 1;
        emit(&sink, AgentEvent::TurnStart { turn_index }).await?;
    }
}

async fn emit(sink: &Arc<dyn EventSink>, event: AgentEvent) -> Result<()> {
    sink.emit(event)
        .await
        .map_err(|err| Error::EventSink(err.to_string()))
}

async fn stream_assistant(
    provider: Arc<dyn GenerationProvider>,
    request: &AgentLoopRequest,
    context: &[Message],
    sink: Arc<dyn EventSink>,
    abort: AbortSignal,
) -> Result<Message> {
    let generation_request = GenerationRequest {
        model: psychevo_ai::ModelTarget {
            provider: request.model_provider.clone(),
            model: request.model.clone(),
        },
        messages: context
            .iter()
            .map(serde_json::to_value)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|err| Error::Agent(err.to_string()))?,
        tools: request
            .tools
            .iter()
            .map(|tool| ToolDeclaration {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters(),
            })
            .collect(),
        metadata: request.generation_metadata.clone(),
    };

    let mut stream = provider.stream(generation_request, abort).await?;
    let mut raw_text = String::new();
    let mut provider_reasoning = String::new();
    let mut reasoning_details = Vec::new();
    let mut emitted_inline_reasoning_len = 0usize;
    let mut tool_builders: BTreeMap<(usize, usize), ToolCallBuilder> = BTreeMap::new();
    let mut finish_reason = None;
    let mut outcome = Outcome::Normal;
    let timestamp_ms = now_ms();
    let mut assistant = Message::Assistant {
        content: Vec::new(),
        timestamp_ms,
        finish_reason: None,
        outcome,
        model: Some(request.model.clone()),
        provider: Some(request.model_provider.clone()),
    };
    let mut last_visible_assistant = assistant.clone();
    emit(
        &sink,
        AgentEvent::MessageStart {
            message: assistant.clone(),
        },
    )
    .await?;

    while let Some(event) = stream.next().await {
        let mut visible_changed = false;
        match event? {
            StreamEvent::TextDelta { text: delta } => {
                raw_text.push_str(&delta);
                let (_, inline_reasoning) = split_inline_think_blocks(&raw_text, true);
                if inline_reasoning.len() > emitted_inline_reasoning_len {
                    let delta = inline_reasoning[emitted_inline_reasoning_len..].to_string();
                    emitted_inline_reasoning_len = inline_reasoning.len();
                    if !delta.is_empty() {
                        emit(&sink, AgentEvent::ReasoningDelta { text: delta }).await?;
                    }
                }
                visible_changed = true;
            }
            StreamEvent::ReasoningDelta {
                text: delta,
                reasoning_content: _,
            } => {
                provider_reasoning.push_str(&delta);
                emit(&sink, AgentEvent::ReasoningDelta { text: delta }).await?;
            }
            StreamEvent::ReasoningDetails { details } => {
                collect_reasoning_details(&mut reasoning_details, details);
            }
            StreamEvent::ToolCallStart {
                content_index,
                call_index,
                id,
                name,
            } => {
                tool_builders.insert(
                    (content_index, call_index),
                    ToolCallBuilder {
                        id,
                        name,
                        arguments_json: String::new(),
                        content_index,
                        call_index,
                    },
                );
                visible_changed = true;
            }
            StreamEvent::ToolCallDelta {
                content_index,
                call_index,
                id,
                name,
                arguments_delta,
            } => {
                let builder = tool_builders
                    .entry((content_index, call_index))
                    .or_insert_with(|| ToolCallBuilder {
                        id: String::new(),
                        name: String::new(),
                        arguments_json: String::new(),
                        content_index,
                        call_index,
                    });
                if let Some(id) = id {
                    builder.id = id;
                }
                if let Some(name) = name {
                    builder.name = name;
                }
                builder.arguments_json.push_str(&arguments_delta);
                visible_changed = true;
            }
            StreamEvent::ToolCallEnd { .. } => {}
            StreamEvent::Usage { .. } | StreamEvent::Metadata { .. } => {}
            StreamEvent::Done {
                outcome: done_outcome,
                finish_reason: done_reason,
            } => {
                outcome = done_outcome;
                finish_reason = done_reason;
                break;
            }
        }
        let (visible_text, inline_reasoning) = split_inline_think_blocks(&raw_text, true);
        let reasoning = combine_reasoning(&provider_reasoning, &inline_reasoning);
        assistant = build_assistant_message(
            AssistantBuildState {
                text: &visible_text,
                reasoning: &reasoning,
                reasoning_provider_evidence: reasoning_provider_evidence(&reasoning_details),
                tool_builders: &tool_builders,
                timestamp_ms,
                finish_reason: finish_reason.clone(),
                outcome,
            },
            request,
        );
        if visible_changed && visible_assistant_changed(&last_visible_assistant, &assistant) {
            last_visible_assistant = assistant.clone();
            emit(
                &sink,
                AgentEvent::MessageUpdate {
                    message: assistant.clone(),
                },
            )
            .await?;
        }
    }

    let (visible_text, inline_reasoning) = split_inline_think_blocks(&raw_text, false);
    if inline_reasoning.len() > emitted_inline_reasoning_len {
        let delta = inline_reasoning[emitted_inline_reasoning_len..].to_string();
        if !delta.is_empty() {
            emit(&sink, AgentEvent::ReasoningDelta { text: delta }).await?;
        }
    }
    let reasoning = combine_reasoning(&provider_reasoning, &inline_reasoning);
    assistant = build_assistant_message(
        AssistantBuildState {
            text: &visible_text,
            reasoning: &reasoning,
            reasoning_provider_evidence: reasoning_provider_evidence(&reasoning_details),
            tool_builders: &tool_builders,
            timestamp_ms,
            finish_reason,
            outcome,
        },
        request,
    );
    if visible_assistant_changed(&last_visible_assistant, &assistant) {
        emit(
            &sink,
            AgentEvent::MessageUpdate {
                message: assistant.clone(),
            },
        )
        .await?;
    }
    if !reasoning.is_empty() {
        emit(&sink, AgentEvent::ReasoningEnd { text: reasoning }).await?;
    }
    emit(
        &sink,
        AgentEvent::MessageEnd {
            message: assistant.clone(),
        },
    )
    .await?;
    Ok(assistant)
}

struct AssistantBuildState<'a> {
    text: &'a str,
    reasoning: &'a str,
    reasoning_provider_evidence: Option<Value>,
    tool_builders: &'a BTreeMap<(usize, usize), ToolCallBuilder>,
    timestamp_ms: i64,
    finish_reason: Option<String>,
    outcome: Outcome,
}

fn build_assistant_message(state: AssistantBuildState<'_>, request: &AgentLoopRequest) -> Message {
    let mut content = Vec::new();
    if !state.reasoning.is_empty() || state.reasoning_provider_evidence.is_some() {
        content.push(AssistantBlock::Reasoning {
            text: state.reasoning.to_string(),
            provider_evidence: state.reasoning_provider_evidence,
        });
    }
    if !state.text.is_empty() {
        content.push(AssistantBlock::Text {
            text: state.text.to_string(),
        });
    }
    for builder in state.tool_builders.values() {
        let parsed = serde_json::from_str::<Value>(&builder.arguments_json);
        let (arguments, arguments_error) = match parsed {
            Ok(value) => (value, None),
            Err(err) => (Value::Null, Some(err.to_string())),
        };
        content.push(AssistantBlock::ToolCall(ToolCallBlock {
            id: builder.id.clone(),
            name: builder.name.clone(),
            arguments,
            arguments_json: builder.arguments_json.clone(),
            arguments_error,
            content_index: builder.content_index,
            call_index: builder.call_index,
        }));
    }
    Message::Assistant {
        content,
        timestamp_ms: state.timestamp_ms,
        finish_reason: state.finish_reason,
        outcome: state.outcome,
        model: Some(request.model.clone()),
        provider: Some(request.model_provider.clone()),
    }
}

fn split_inline_think_blocks(input: &str, streaming: bool) -> (String, String) {
    let mut visible = String::new();
    let mut reasoning = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative_start) = input[cursor..].find("<think>") {
        let start = cursor + relative_start;
        visible.push_str(&input[cursor..start]);
        let content_start = start + "<think>".len();
        if let Some(relative_end) = input[content_start..].find("</think>") {
            let end = content_start + relative_end;
            let thought = input[content_start..end].trim();
            if !thought.is_empty() {
                reasoning.push(thought.to_string());
            }
            cursor = end + "</think>".len();
        } else {
            if !streaming {
                visible.push_str(&input[start..]);
            }
            return (visible, reasoning.join("\n\n"));
        }
    }
    visible.push_str(&input[cursor..]);
    (visible, reasoning.join("\n\n"))
}

fn combine_reasoning(provider_reasoning: &str, inline_reasoning: &str) -> String {
    match (
        provider_reasoning.trim().is_empty(),
        inline_reasoning.trim().is_empty(),
    ) {
        (true, true) => String::new(),
        (false, true) => provider_reasoning.to_string(),
        (true, false) => inline_reasoning.to_string(),
        (false, false) => format!("{provider_reasoning}\n\n{inline_reasoning}"),
    }
}

fn collect_reasoning_details(details: &mut Vec<Value>, value: Value) {
    match value {
        Value::Array(values) => details.extend(values),
        other => details.push(other),
    }
}

fn reasoning_provider_evidence(details: &[Value]) -> Option<Value> {
    (!details.is_empty()).then(|| json!({ "reasoning_details": details }))
}

fn visible_assistant_changed(previous: &Message, current: &Message) -> bool {
    visible_assistant_blocks(previous) != visible_assistant_blocks(current)
}

fn visible_assistant_blocks(message: &Message) -> Vec<AssistantBlock> {
    let Message::Assistant { content, .. } = message else {
        return Vec::new();
    };
    content
        .iter()
        .filter(|block| !matches!(block, AssistantBlock::Reasoning { .. }))
        .cloned()
        .collect()
}

#[derive(Debug, Clone)]
struct ToolCallBuilder {
    id: String,
    name: String,
    arguments_json: String,
    content_index: usize,
    call_index: usize,
}

fn assistant_outcome(message: &Message) -> Outcome {
    match message {
        Message::Assistant { outcome, .. } => *outcome,
        _ => Outcome::Failed,
    }
}

fn assistant_tool_calls(message: &Message) -> Vec<ToolCallBlock> {
    let Message::Assistant { content, .. } = message else {
        return Vec::new();
    };
    content
        .iter()
        .filter_map(|block| match block {
            AssistantBlock::ToolCall(call) => Some(call.clone()),
            _ => None,
        })
        .collect()
}

async fn execute_tool_batch(
    tools: &[Arc<dyn ToolBinding>],
    tool_calls: &[ToolCallBlock],
    sink: Arc<dyn EventSink>,
    abort: AbortSignal,
) -> Result<Vec<Message>> {
    let has_sequential = tool_calls.iter().any(|call| {
        tools
            .iter()
            .find(|tool| tool.name() == call.name)
            .is_none_or(|tool| tool.execution_mode() == ToolExecutionMode::Sequential)
    });

    for call in tool_calls {
        emit(
            &sink,
            AgentEvent::ToolExecutionStart {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                args: call.arguments.clone(),
            },
        )
        .await?;
    }

    let outputs = if has_sequential {
        let mut outputs = Vec::new();
        for call in tool_calls {
            let output =
                execute_one_tool(tools, call.clone(), Arc::clone(&sink), abort.clone()).await?;
            outputs.push(output);
        }
        outputs
    } else {
        let futures = tool_calls
            .iter()
            .cloned()
            .map(|call| execute_one_tool(tools, call, Arc::clone(&sink), abort.clone()));
        let joined = join_all(futures).await;
        let mut outputs = Vec::new();
        for output in joined {
            outputs.push(output?);
        }
        outputs
    };

    Ok(outputs
        .into_iter()
        .map(|(call, output)| tool_result_message(call, output))
        .collect())
}

async fn execute_one_tool(
    tools: &[Arc<dyn ToolBinding>],
    call: ToolCallBlock,
    sink: Arc<dyn EventSink>,
    abort: AbortSignal,
) -> Result<(ToolCallBlock, ToolOutput)> {
    let output = if let Some(err) = &call.arguments_error {
        ToolOutput::error(format!("invalid tool arguments JSON: {err}"))
    } else if let Some(tool) = tools.iter().find(|tool| tool.name() == call.name) {
        tool.execute(call.id.clone(), call.arguments.clone(), abort)
            .await
    } else {
        ToolOutput::error(format!("tool not found: {}", call.name))
    };
    let outcome = if output.is_error {
        Outcome::Failed
    } else {
        Outcome::Normal
    };
    emit(
        &sink,
        AgentEvent::ToolExecutionEnd {
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            result: output.json.clone(),
            outcome,
        },
    )
    .await?;
    Ok((call, output))
}

fn tool_result_message(call: ToolCallBlock, output: ToolOutput) -> Message {
    Message::ToolResult {
        tool_call_id: call.id,
        tool_name: call.name,
        content: serde_json::to_string(&output.json)
            .unwrap_or_else(|_| "{\"error\":\"invalid result\"}".to_string()),
        is_error: output.is_error,
        timestamp_ms: now_ms(),
    }
}

pub fn user_text_message(text: impl Into<String>) -> Message {
    Message::User {
        content: vec![TextBlock { text: text.into() }],
        timestamp_ms: now_ms(),
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn emit(&self, _event: AgentEvent) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use psychevo_ai::{FakeProvider, RawStreamEvent};
    use std::sync::Mutex;

    #[derive(Default)]
    struct CaptureSink {
        events: Mutex<Vec<AgentEvent>>,
    }

    impl EventSink for CaptureSink {
        fn emit(&self, event: AgentEvent) -> BoxFuture<'static, Result<()>> {
            self.events.lock().expect("events").push(event);
            Box::pin(async { Ok(()) })
        }
    }

    #[derive(Clone)]
    struct StaticProvider {
        events: Vec<StreamEvent>,
    }

    impl GenerationProvider for StaticProvider {
        fn stream(
            &self,
            _request: GenerationRequest,
            _abort: AbortSignal,
        ) -> BoxFuture<'static, psychevo_ai::Result<psychevo_ai::GenerationStream>> {
            let events = self.events.clone().into_iter().map(Ok);
            Box::pin(async move {
                let output: psychevo_ai::GenerationStream = Box::pin(stream::iter(events));
                Ok(output)
            })
        }
    }

    fn request() -> AgentLoopRequest {
        AgentLoopRequest {
            model_provider: "fake".to_string(),
            model: "model".to_string(),
            generation_metadata: json!({}),
            previous_messages: Vec::new(),
            prompt_messages: vec![user_text_message("hello")],
            tools: Vec::new(),
            max_turns: 1,
        }
    }

    #[tokio::test]
    async fn reasoning_only_progress_has_no_visible_message_update() {
        let provider = Arc::new(FakeProvider::new(vec![vec![
            RawStreamEvent::Reasoning("private".to_string()),
            RawStreamEvent::Text("visible".to_string()),
            RawStreamEvent::Done(Outcome::Normal),
        ]]));
        let sink = Arc::new(CaptureSink::default());
        let (_, control) = ControlHandle::new();
        let completion = run_agent_loop(provider, request(), sink.clone(), control)
            .await
            .expect("loop");
        assert_eq!(completion.outcome, Outcome::Normal);

        let events = sink.events.lock().expect("events");
        assert!(events.iter().any(|event| {
            matches!(event, AgentEvent::ReasoningDelta { text } if text == "private")
        }));
        let updates = events
            .iter()
            .filter(|event| matches!(event, AgentEvent::MessageUpdate { .. }))
            .count();
        assert_eq!(updates, 1);
    }

    #[tokio::test]
    async fn usage_and_metadata_do_not_emit_empty_message_updates() {
        let provider = Arc::new(StaticProvider {
            events: vec![
                StreamEvent::Metadata {
                    metadata: json!({"id":"resp"}),
                },
                StreamEvent::Usage {
                    usage: json!({"total_tokens":1}),
                },
                StreamEvent::TextDelta {
                    text: "ok".to_string(),
                },
                StreamEvent::Done {
                    outcome: Outcome::Normal,
                    finish_reason: Some("stop".to_string()),
                },
            ],
        });
        let sink = Arc::new(CaptureSink::default());
        let (_, control) = ControlHandle::new();
        run_agent_loop(provider, request(), sink.clone(), control)
            .await
            .expect("loop");

        let events = sink.events.lock().expect("events");
        let updates = events
            .iter()
            .filter(|event| matches!(event, AgentEvent::MessageUpdate { .. }))
            .count();
        assert_eq!(updates, 1);
    }

    #[tokio::test]
    async fn complete_inline_think_blocks_are_folded_reasoning() {
        let provider = Arc::new(FakeProvider::new(vec![vec![
            RawStreamEvent::Text("visible <think>secret</think> done".to_string()),
            RawStreamEvent::Done(Outcome::Normal),
        ]]));
        let sink = Arc::new(CaptureSink::default());
        let (_, control) = ControlHandle::new();
        let completion = run_agent_loop(provider, request(), sink.clone(), control)
            .await
            .expect("loop");
        let assistant = completion
            .messages
            .iter()
            .find(|message| matches!(message, Message::Assistant { .. }))
            .expect("assistant");
        let Message::Assistant { content, .. } = assistant else {
            unreachable!();
        };
        assert!(content.contains(&AssistantBlock::Reasoning {
            text: "secret".to_string(),
            provider_evidence: None,
        }));
        assert!(content.contains(&AssistantBlock::Text {
            text: "visible  done".to_string()
        }));

        let events = sink.events.lock().expect("events");
        assert!(events.iter().any(|event| {
            matches!(event, AgentEvent::ReasoningEnd { text } if text == "secret")
        }));
    }

    #[tokio::test]
    async fn reasoning_details_attach_to_reasoning_block_evidence() {
        let provider = Arc::new(StaticProvider {
            events: vec![
                StreamEvent::ReasoningDelta {
                    text: "scratch".to_string(),
                    reasoning_content: Some("scratch".to_string()),
                },
                StreamEvent::ReasoningDetails {
                    details: json!([{ "type": "thinking", "text": "opaque" }]),
                },
                StreamEvent::TextDelta {
                    text: "visible".to_string(),
                },
                StreamEvent::Done {
                    outcome: Outcome::Normal,
                    finish_reason: Some("stop".to_string()),
                },
            ],
        });
        let sink = Arc::new(CaptureSink::default());
        let (_, control) = ControlHandle::new();
        let completion = run_agent_loop(provider, request(), sink, control)
            .await
            .expect("loop");
        let assistant = completion
            .messages
            .iter()
            .find(|message| matches!(message, Message::Assistant { .. }))
            .expect("assistant");
        let Message::Assistant { content, .. } = assistant else {
            unreachable!();
        };
        let reasoning = content
            .iter()
            .find_map(|block| match block {
                AssistantBlock::Reasoning {
                    text,
                    provider_evidence,
                } => Some((text, provider_evidence)),
                _ => None,
            })
            .expect("reasoning block");
        assert_eq!(reasoning.0, "scratch");
        assert_eq!(
            reasoning.1.as_ref().expect("evidence")["reasoning_details"][0]["type"],
            "thinking"
        );
    }
}
