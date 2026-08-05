use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::{future::BoxFuture, stream};
use psychevo_ai::{
    AbortSignal, AdapterCall, AdapterFuture, AdapterStream, AssistantSource, DeploymentConfig,
    FakeLanguageAdapter, FinishReason, FinishReasonKind, LanguageAdapter, LanguageAdapterEvent,
    LanguageModel, LanguageRequest, Outcome, Provider, Usage,
};
use serde_json::{Value, json};
use tokio::sync::watch;

use crate::Result;
use crate::agent::assistant::InlineThinkParser;
use crate::agent::run_agent_loop;
use crate::agent::stream::stream_assistant;
use crate::agent::tools::execute_tool_batch;
use crate::control::ControlHandle;
use crate::events::{AgentEvent, EventSink};
use crate::request::{AgentLoopRequest, ToolSearchOptions};
use crate::support::{NoopEventSink, user_text_message};
use crate::tool_router::{ToolRouter, ToolRouterError};
use crate::types::{
    AssistantBlock, ContextualUserBlock, ContextualUserMessage, Message, ToolBinding,
    ToolCallBlock, ToolDisplayBodyPolicy, ToolDisplayCategory, ToolDisplaySpec, ToolExecutionMode,
    ToolExposure, ToolOutput, UserContentBlock,
};

#[derive(Debug, Clone)]
enum RawStreamEvent {
    Text(String),
    Reasoning(String),
    Done(Outcome),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StreamEvent {
    TextDelta {
        text: String,
    },
    ReasoningDelta {
        text: String,
        reasoning_content: Option<String>,
    },
    ReasoningDetails {
        details: Value,
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

#[derive(Default)]
pub(crate) struct CaptureSink {
    pub(crate) events: Mutex<Vec<AgentEvent>>,
}

pub(crate) struct AbortOnFirstDeltaSink {
    pub(crate) control: ControlHandle,
    pub(crate) events: Mutex<Vec<AgentEvent>>,
    pub(crate) deltas: AtomicUsize,
}

impl EventSink for AbortOnFirstDeltaSink {
    fn emit(&self, event: AgentEvent) -> BoxFuture<'static, Result<()>> {
        if matches!(event, AgentEvent::AssistantTextDelta { .. })
            && self.deltas.fetch_add(1, Ordering::SeqCst) == 0
        {
            self.control.abort();
        }
        self.events.lock().expect("events").push(event);
        Box::pin(async { Ok(()) })
    }
}

impl EventSink for CaptureSink {
    fn emit(&self, event: AgentEvent) -> BoxFuture<'static, Result<()>> {
        self.events.lock().expect("events").push(event);
        Box::pin(async { Ok(()) })
    }
}

#[derive(Clone)]
pub(crate) struct StaticProvider {
    pub(crate) events: Vec<StreamEvent>,
}

impl LanguageAdapter for StaticProvider {
    fn stream(
        &self,
        _call: AdapterCall<LanguageRequest>,
    ) -> AdapterFuture<'_, AdapterStream<LanguageAdapterEvent>> {
        let events = normalize_test_events(self.events.clone())
            .into_iter()
            .map(Ok);
        Box::pin(async move { Ok(Box::pin(stream::iter(events)) as AdapterStream<_>) })
    }
}

#[derive(Clone, Default)]
pub(crate) struct RequestCaptureProvider {
    pub(crate) requests: Arc<Mutex<Vec<LanguageRequest>>>,
}

impl LanguageAdapter for RequestCaptureProvider {
    fn stream(
        &self,
        call: AdapterCall<LanguageRequest>,
    ) -> AdapterFuture<'_, AdapterStream<LanguageAdapterEvent>> {
        self.requests.lock().expect("requests").push(call.request);
        Box::pin(async {
            Ok(Box::pin(stream::iter([Ok(LanguageAdapterEvent::Finish {
                finish_reason: Some(finish_reason("stop")),
            })])) as AdapterStream<_>)
        })
    }
}

pub(crate) fn test_model(adapter: impl LanguageAdapter) -> LanguageModel {
    Provider::builder(
        DeploymentConfig::new("fake", "fake", "fake://local")
            .with_default_language_protocol("fake"),
    )
    .language_adapter(adapter)
    .build()
    .expect("fake provider")
    .language_model("model")
    .expect("fake language model")
}

fn raw_test_model(scripts: Vec<Vec<RawStreamEvent>>) -> LanguageModel {
    #[derive(Clone)]
    struct RawAdapter {
        scripts: Arc<Mutex<VecDeque<Vec<RawStreamEvent>>>>,
    }

    impl LanguageAdapter for RawAdapter {
        fn stream(
            &self,
            _call: AdapterCall<LanguageRequest>,
        ) -> AdapterFuture<'_, AdapterStream<LanguageAdapterEvent>> {
            let script = self
                .scripts
                .lock()
                .expect("raw scripts")
                .pop_front()
                .expect("raw adapter script");
            let events = script.into_iter().map(raw_stream_event).collect::<Vec<_>>();
            Box::pin(async move {
                Ok(Box::pin(stream::iter(
                    normalize_test_events(events).into_iter().map(Ok),
                )) as AdapterStream<_>)
            })
        }
    }

    test_model(RawAdapter {
        scripts: Arc::new(Mutex::new(scripts.into())),
    })
}

fn raw_stream_event(event: RawStreamEvent) -> StreamEvent {
    match event {
        RawStreamEvent::Text(text) => StreamEvent::TextDelta { text },
        RawStreamEvent::Reasoning(text) => StreamEvent::ReasoningDelta {
            text,
            reasoning_content: None,
        },
        RawStreamEvent::Done(outcome) => StreamEvent::Done {
            outcome,
            finish_reason: None,
        },
    }
}

fn normalize_test_events(events: Vec<StreamEvent>) -> Vec<LanguageAdapterEvent> {
    let mut normalized = Vec::new();
    let mut next_content_index = 0;
    let mut text_index = None;
    let mut reasoning_index = None;
    let mut tool_arguments = BTreeMap::<(usize, usize), (usize, String)>::new();

    let close_text = |normalized: &mut Vec<_>, text_index: &mut Option<usize>| {
        if let Some(content_index) = text_index.take() {
            normalized.push(LanguageAdapterEvent::TextEnd { content_index });
        }
    };
    let close_reasoning = |normalized: &mut Vec<_>, reasoning_index: &mut Option<usize>| {
        if let Some(content_index) = reasoning_index.take() {
            normalized.push(LanguageAdapterEvent::ReasoningEnd { content_index });
        }
    };

    for event in events {
        match event {
            StreamEvent::TextDelta { text } => {
                close_reasoning(&mut normalized, &mut reasoning_index);
                let content_index = *text_index.get_or_insert_with(|| {
                    let content_index = next_content_index;
                    next_content_index += 1;
                    normalized.push(LanguageAdapterEvent::TextStart { content_index });
                    content_index
                });
                normalized.push(LanguageAdapterEvent::TextDelta {
                    content_index,
                    delta: text,
                });
            }
            StreamEvent::ReasoningDelta { text, .. } => {
                close_text(&mut normalized, &mut text_index);
                let content_index = *reasoning_index.get_or_insert_with(|| {
                    let content_index = next_content_index;
                    next_content_index += 1;
                    normalized.push(LanguageAdapterEvent::ReasoningStart { content_index });
                    content_index
                });
                normalized.push(LanguageAdapterEvent::ReasoningDelta {
                    content_index,
                    delta: text,
                    provider_evidence: None,
                });
            }
            StreamEvent::ReasoningDetails { details } => {
                close_text(&mut normalized, &mut text_index);
                let content_index = *reasoning_index.get_or_insert_with(|| {
                    let content_index = next_content_index;
                    next_content_index += 1;
                    normalized.push(LanguageAdapterEvent::ReasoningStart { content_index });
                    content_index
                });
                normalized.push(LanguageAdapterEvent::ReasoningDelta {
                    content_index,
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
                close_text(&mut normalized, &mut text_index);
                close_reasoning(&mut normalized, &mut reasoning_index);
                let sdk_index = next_content_index;
                next_content_index += 1;
                tool_arguments.insert((content_index, call_index), (sdk_index, String::new()));
                normalized.push(LanguageAdapterEvent::ToolCallStart {
                    content_index: sdk_index,
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
                let (sdk_index, arguments) = tool_arguments
                    .get_mut(&(content_index, call_index))
                    .expect("tool-call delta after start");
                arguments.push_str(&arguments_delta);
                normalized.push(LanguageAdapterEvent::ToolCallArgumentsDelta {
                    content_index: *sdk_index,
                    delta: arguments_delta,
                });
            }
            StreamEvent::ToolCallEnd {
                content_index,
                call_index,
            } => {
                let (sdk_index, arguments_raw) = tool_arguments
                    .remove(&(content_index, call_index))
                    .expect("tool-call end after start");
                normalized.push(LanguageAdapterEvent::ToolCallEnd {
                    content_index: sdk_index,
                    arguments_raw,
                });
            }
            StreamEvent::Usage { usage } => {
                normalized.push(LanguageAdapterEvent::Usage {
                    usage: typed_test_usage(&usage),
                });
            }
            StreamEvent::Metadata { metadata } => {
                normalized.push(LanguageAdapterEvent::Metadata {
                    metadata: allowlisted_test_metadata(&metadata),
                });
            }
            StreamEvent::Done {
                outcome: _,
                finish_reason: reason,
            } => {
                close_text(&mut normalized, &mut text_index);
                close_reasoning(&mut normalized, &mut reasoning_index);
                for (_, (content_index, arguments_raw)) in std::mem::take(&mut tool_arguments) {
                    normalized.push(LanguageAdapterEvent::ToolCallEnd {
                        content_index,
                        arguments_raw,
                    });
                }
                normalized.push(LanguageAdapterEvent::Finish {
                    finish_reason: reason.as_deref().map(finish_reason),
                });
            }
        }
    }
    normalized
}

fn typed_test_usage(value: &Value) -> Usage {
    Usage {
        input_tokens: value
            .get("input_tokens")
            .or_else(|| value.get("prompt_tokens"))
            .and_then(Value::as_u64),
        output_tokens: value
            .get("output_tokens")
            .or_else(|| value.get("completion_tokens"))
            .and_then(Value::as_u64),
        total_tokens: value.get("total_tokens").and_then(Value::as_u64),
        ..Usage::default()
    }
}

fn allowlisted_test_metadata(value: &Value) -> BTreeMap<String, Value> {
    let Some(object) = value.as_object() else {
        return BTreeMap::new();
    };
    [
        "provider_response_id",
        "response_id",
        "model",
        "system_fingerprint",
        "service_tier",
        "created",
        "finish_reason",
        "request_id",
    ]
    .into_iter()
    .filter_map(|key| {
        object
            .get(key)
            .filter(|value| {
                matches!(
                    value,
                    Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null
                )
            })
            .cloned()
            .map(|value| (key.to_string(), value))
    })
    .chain(
        (!object.contains_key("provider_response_id"))
            .then(|| object.get("id"))
            .flatten()
            .filter(|value| {
                matches!(
                    value,
                    Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null
                )
            })
            .cloned()
            .map(|value| ("provider_response_id".to_string(), value)),
    )
    .collect()
}

fn finish_reason(raw: &str) -> FinishReason {
    FinishReason {
        kind: match raw {
            "stop" => FinishReasonKind::Stop,
            "length" => FinishReasonKind::Length,
            "tool_calls" => FinishReasonKind::ToolCalls,
            "content_filter" => FinishReasonKind::ContentFilter,
            _ => FinishReasonKind::Other,
        },
        raw: Some(raw.to_string()),
    }
}

pub(crate) fn request() -> AgentLoopRequest {
    AgentLoopRequest {
        model_provider: "fake".to_string(),
        model: "model".to_string(),
        generation_metadata: json!({}),
        prompt_instructions: Vec::new(),
        turn_prompt_instructions: Vec::new(),
        previous_messages: Vec::new(),
        context_messages: Vec::new(),
        prefix_contextual_user_messages: Vec::new(),
        turn_contextual_user_messages: Vec::new(),
        prompt_messages: vec![user_text_message("hello")],
        tools: Vec::new(),
        tool_search: ToolSearchOptions::disabled(),
        max_turns: 1,
    }
}

pub(crate) struct DisplayOnlyTool;

impl ToolBinding for DisplayOnlyTool {
    fn name(&self) -> &str {
        "display_only"
    }

    fn description(&self) -> &str {
        "A test tool with UI-only display metadata."
    }

    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}, "additionalProperties": false})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn display_spec(&self) -> ToolDisplaySpec {
        ToolDisplaySpec {
            category: ToolDisplayCategory::Update,
            title_arg_keys: vec!["target".to_string()],
            title_result_keys: vec!["target".to_string()],
            summary_keys: vec!["status".to_string()],
            body_keys: vec!["content".to_string()],
            body_policy: ToolDisplayBodyPolicy::Summary,
        }
    }

    fn execute(
        &self,
        _tool_call_id: String,
        _args: Value,
        _abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        Box::pin(async { ToolOutput::ok(json!({"status": "ok"})) })
    }
}

pub(crate) struct HiddenTool;

impl ToolBinding for HiddenTool {
    fn name(&self) -> &str {
        "hidden"
    }

    fn description(&self) -> &str {
        "A hidden test tool."
    }

    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}, "additionalProperties": false})
    }

    fn exposure(&self) -> ToolExposure {
        ToolExposure::Hidden
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn execute(
        &self,
        _tool_call_id: String,
        _args: Value,
        _abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        Box::pin(async { ToolOutput::ok(json!({"hidden": true})) })
    }
}

pub(crate) struct DeferredTool;

impl ToolBinding for DeferredTool {
    fn name(&self) -> &str {
        "deferred_lookup"
    }

    fn description(&self) -> &str {
        "Looks up deferred extension data."
    }

    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}, "additionalProperties": false})
    }

    fn search_metadata(&self) -> Vec<String> {
        vec![
            "mcp:repo_tools".to_string(),
            "repo tools/raw_lookup".to_string(),
        ]
    }

    fn exposure(&self) -> ToolExposure {
        ToolExposure::Deferred
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn execute(
        &self,
        _tool_call_id: String,
        _args: Value,
        _abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        Box::pin(async { ToolOutput::ok(json!({"deferred": true})) })
    }
}

pub(crate) struct NamespacedTool;

impl ToolBinding for NamespacedTool {
    fn name(&self) -> &str {
        "mcp__repo__search"
    }

    fn canonical_tool_name(&self) -> psychevo_ai::ToolName {
        psychevo_ai::ToolName::namespaced("mcp__repo", "search")
    }

    fn description(&self) -> &str {
        "Searches a repository MCP server."
    }

    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}, "additionalProperties": false})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        _args: Value,
        _abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        Box::pin(async { ToolOutput::ok(json!({"ok": true})) })
    }
}

pub(crate) struct AliasedTool {
    display_name: &'static str,
    canonical_name: psychevo_ai::ToolName,
}

impl ToolBinding for AliasedTool {
    fn name(&self) -> &str {
        self.display_name
    }

    fn canonical_tool_name(&self) -> psychevo_ai::ToolName {
        self.canonical_name.clone()
    }

    fn description(&self) -> &str {
        "A test tool with independently selectable identities."
    }

    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}, "additionalProperties": false})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        _args: Value,
        _abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        Box::pin(async { ToolOutput::ok(json!({"ok": true})) })
    }
}

#[tokio::test]
pub(crate) async fn tool_display_spec_is_not_model_visible_declaration() {
    let provider = RequestCaptureProvider::default();
    let requests = Arc::clone(&provider.requests);
    let (_, control) = ControlHandle::new();
    let mut request = request();
    request.tools = vec![Arc::new(DisplayOnlyTool)];

    run_agent_loop(
        test_model(provider),
        request,
        Arc::new(NoopEventSink),
        control,
    )
    .await
    .expect("loop");

    let requests = requests.lock().expect("requests");
    let tool = requests[0].tools.first().expect("tool declaration");
    let psychevo_ai::LanguageTool::Function { declaration } = tool else {
        panic!("expected function declaration");
    };
    let value = serde_json::to_value(declaration).expect("tool declaration json");
    assert_eq!(value["name"], "display_only");
    assert!(value.get("display").is_none(), "{value}");
}

#[test]
pub(crate) fn tool_search_activates_deferred_tools_for_later_declarations() {
    let mut router = ToolRouter::from_tools(vec![Arc::new(DeferredTool) as Arc<dyn ToolBinding>])
        .expect("unique tools")
        .with_tool_search(ToolSearchOptions::enabled());

    let initial = router.declarations();
    let search = initial.first().expect("tool_search declaration");
    assert!(search.description.contains("not currently loaded"));
    assert!(search.description.contains("later calls"));
    let search_text = format!(
        "{}\n{}",
        search.description,
        serde_json::to_string(&search.parameters).expect("search parameters")
    )
    .to_ascii_lowercase();
    for implementation_term in ["deferred", "activate", "router", "harness"] {
        assert!(
            !search_text.contains(implementation_term),
            "tool_search exposes {implementation_term:?}: {search_text}"
        );
    }
    let initial_names = initial
        .into_iter()
        .map(|declaration| declaration.name)
        .collect::<Vec<_>>();
    assert_eq!(initial_names, vec!["tool_search"]);

    let output = router.execute_tool_search(&json!({"query": "extension data"}));
    assert!(!output.is_error);
    assert_eq!(output.json["activated"], json!(["deferred_lookup"]));

    let activated_names = router
        .declarations()
        .into_iter()
        .map(|declaration| declaration.name)
        .collect::<Vec<_>>();
    assert_eq!(activated_names, vec!["deferred_lookup"]);
}

#[test]
pub(crate) fn tool_search_matches_source_metadata() {
    let mut router = ToolRouter::from_tools(vec![Arc::new(DeferredTool) as Arc<dyn ToolBinding>])
        .expect("unique tools")
        .with_tool_search(ToolSearchOptions::enabled());

    let output = router.execute_tool_search(&json!({"query": "repo_tools"}));

    assert!(!output.is_error);
    assert_eq!(output.json["activated"], json!(["deferred_lookup"]));
}

#[test]
pub(crate) fn router_declarations_preserve_canonical_tool_identity() {
    let router = ToolRouter::from_tools(vec![Arc::new(NamespacedTool) as Arc<dyn ToolBinding>])
        .expect("unique tools");
    let declarations = router.declarations();

    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].name, "mcp__repo__search");
    assert_eq!(declarations[0].namespace.as_deref(), Some("mcp__repo"));
    assert_eq!(declarations[0].canonical_name.as_deref(), Some("search"));
    assert!(
        router
            .tool_by_canonical_name(&psychevo_ai::ToolName::namespaced("mcp__repo", "search"))
            .is_some()
    );
}

#[test]
pub(crate) fn router_rejects_duplicate_display_names_in_any_input_order() {
    let first = || {
        Arc::new(AliasedTool {
            display_name: "lookup",
            canonical_name: psychevo_ai::ToolName::namespaced("first", "lookup"),
        }) as Arc<dyn ToolBinding>
    };
    let second = || {
        Arc::new(AliasedTool {
            display_name: "lookup",
            canonical_name: psychevo_ai::ToolName::namespaced("second", "lookup"),
        }) as Arc<dyn ToolBinding>
    };

    for tools in [vec![first(), second()], vec![second(), first()]] {
        let Err(error) = ToolRouter::from_tools(tools) else {
            panic!("duplicate display name must fail");
        };
        assert_eq!(
            error,
            ToolRouterError::DuplicateDisplayName("lookup".to_string())
        );
    }
}

#[test]
pub(crate) fn router_rejects_duplicate_canonical_names_in_any_input_order() {
    let first = || {
        Arc::new(AliasedTool {
            display_name: "repo_lookup",
            canonical_name: psychevo_ai::ToolName::namespaced("repo", "lookup"),
        }) as Arc<dyn ToolBinding>
    };
    let second = || {
        Arc::new(AliasedTool {
            display_name: "workspace_lookup",
            canonical_name: psychevo_ai::ToolName::namespaced("repo", "lookup"),
        }) as Arc<dyn ToolBinding>
    };

    for tools in [vec![first(), second()], vec![second(), first()]] {
        let Err(error) = ToolRouter::from_tools(tools) else {
            panic!("duplicate canonical name must fail");
        };
        assert_eq!(
            error,
            ToolRouterError::DuplicateCanonicalName("repo__lookup".to_string())
        );
    }
}

#[test]
pub(crate) fn valid_router_keeps_declarations_and_both_lookups_bijective() {
    let tools = vec![
        Arc::new(AliasedTool {
            display_name: "repo_lookup",
            canonical_name: psychevo_ai::ToolName::namespaced("repo", "lookup"),
        }) as Arc<dyn ToolBinding>,
        Arc::new(AliasedTool {
            display_name: "workspace_read",
            canonical_name: psychevo_ai::ToolName::namespaced("workspace", "read"),
        }) as Arc<dyn ToolBinding>,
    ];

    let router = ToolRouter::from_tools(tools).expect("unique tools");
    let declarations = router.declarations();
    assert_eq!(declarations.len(), 2);
    for declaration in declarations {
        let display = router.tool(&declaration.name).expect("display lookup");
        let canonical = display.canonical_tool_name();
        assert!(Arc::ptr_eq(
            &display,
            &router
                .tool_by_canonical_name(&canonical)
                .expect("canonical lookup")
        ));
    }
}

#[tokio::test]
pub(crate) async fn hidden_tools_are_not_model_callable() {
    let (_abort_tx, abort_rx) = watch::channel(false);
    let tools: Vec<Arc<dyn ToolBinding>> = vec![Arc::new(HiddenTool)];
    let mut router = ToolRouter::from_tools(tools).expect("unique tools");
    let messages = execute_tool_batch(
        &mut router,
        &[ToolCallBlock {
            id: "call-1".to_string(),
            name: "hidden".to_string(),
            arguments: json!({}),
            arguments_json: "{}".to_string(),
            arguments_error: None,
            content_index: 0,
            call_index: 0,
        }],
        Arc::new(CaptureSink::default()),
        AbortSignal::new(abort_rx),
    )
    .await
    .expect("tool execution");

    let Message::ToolResult {
        is_error, content, ..
    } = &messages[0]
    else {
        panic!("tool result");
    };
    assert!(*is_error);
    assert!(content.contains("tool not found: hidden"));
}

struct BatchTrackingTool {
    name: &'static str,
    mode: ToolExecutionMode,
    delay_ms: u64,
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
    log: Arc<Mutex<Vec<String>>>,
}

impl ToolBinding for BatchTrackingTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "track batch ordering and concurrency"
    }

    fn parameters(&self) -> Value {
        json!({})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        self.mode
    }

    fn execute(
        &self,
        tool_call_id: String,
        _args: Value,
        _abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let active = Arc::clone(&self.active);
        let max_active = Arc::clone(&self.max_active);
        let log = Arc::clone(&self.log);
        let delay_ms = self.delay_ms;
        Box::pin(async move {
            log.lock()
                .expect("batch log")
                .push(format!("start:{tool_call_id}"));
            let current = active.fetch_add(1, Ordering::SeqCst) + 1;
            max_active.fetch_max(current, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            active.fetch_sub(1, Ordering::SeqCst);
            log.lock()
                .expect("batch log")
                .push(format!("end:{tool_call_id}"));
            ToolOutput::ok(json!({"id": tool_call_id}))
        })
    }
}

fn batch_call(id: &str, name: &str) -> ToolCallBlock {
    ToolCallBlock {
        id: id.to_string(),
        name: name.to_string(),
        arguments: json!({}),
        arguments_json: "{}".to_string(),
        arguments_error: None,
        content_index: 0,
        call_index: 0,
    }
}

#[tokio::test]
async fn tool_batch_uses_parallel_segments_with_sequential_barriers() {
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let log = Arc::new(Mutex::new(Vec::new()));
    let tool = |name, mode, delay_ms| {
        Arc::new(BatchTrackingTool {
            name,
            mode,
            delay_ms,
            active: Arc::clone(&active),
            max_active: Arc::clone(&max_active),
            log: Arc::clone(&log),
        }) as Arc<dyn ToolBinding>
    };
    let mut router = ToolRouter::from_tools(vec![
        tool("parallel_a", ToolExecutionMode::Parallel, 80),
        tool("parallel_b", ToolExecutionMode::Parallel, 20),
        tool("barrier", ToolExecutionMode::Sequential, 10),
        tool("parallel_c", ToolExecutionMode::Parallel, 5),
    ])
    .expect("router");
    let (_abort_tx, abort_rx) = watch::channel(false);

    let messages = execute_tool_batch(
        &mut router,
        &[
            batch_call("a", "parallel_a"),
            batch_call("b", "parallel_b"),
            batch_call("s", "barrier"),
            batch_call("c", "parallel_c"),
        ],
        Arc::new(NoopEventSink),
        AbortSignal::new(abort_rx),
    )
    .await
    .expect("batch");

    let log = log.lock().expect("batch log");
    let position = |entry: &str| log.iter().position(|item| item == entry).expect(entry);
    assert!(position("start:s") > position("end:a"));
    assert!(position("start:s") > position("end:b"));
    assert!(position("start:c") > position("end:s"));
    assert!(position("end:b") < position("end:a"));
    assert_eq!(max_active.load(Ordering::SeqCst), 2);
    let result_ids = messages
        .iter()
        .filter_map(|message| match message {
            Message::ToolResult { tool_call_id, .. } => Some(tool_call_id.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(result_ids, vec!["a", "b", "s", "c"]);
}

#[tokio::test]
async fn parallel_tool_segment_never_exceeds_eight_in_flight() {
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let tool = Arc::new(BatchTrackingTool {
        name: "parallel",
        mode: ToolExecutionMode::Parallel,
        delay_ms: 50,
        active: Arc::clone(&active),
        max_active: Arc::clone(&max_active),
        log: Arc::new(Mutex::new(Vec::new())),
    }) as Arc<dyn ToolBinding>;
    let mut router = ToolRouter::from_tools(vec![tool]).expect("router");
    let calls = (0..9)
        .map(|index| batch_call(&format!("call-{index}"), "parallel"))
        .collect::<Vec<_>>();
    let (_abort_tx, abort_rx) = watch::channel(false);

    let messages = execute_tool_batch(
        &mut router,
        &calls,
        Arc::new(NoopEventSink),
        AbortSignal::new(abort_rx),
    )
    .await
    .expect("batch");

    assert_eq!(max_active.load(Ordering::SeqCst), 8);
    assert_eq!(messages.len(), 9);
}

#[tokio::test]
pub(crate) async fn prefix_contextual_user_messages_are_inserted_before_history() {
    let provider = RequestCaptureProvider::default();
    let requests = Arc::clone(&provider.requests);
    let (_, control) = ControlHandle::new();
    let completion = run_agent_loop(
        test_model(provider),
        AgentLoopRequest {
            model_provider: "fake".to_string(),
            model: "model".to_string(),
            generation_metadata: json!({}),
            prompt_instructions: Vec::new(),
            turn_prompt_instructions: Vec::new(),
            previous_messages: vec![user_text_message("previous")],
            context_messages: Vec::new(),
            prefix_contextual_user_messages: vec![ContextualUserMessage::new_with_category(
                "project_instructions",
                "project_context",
                vec![
                    ContextualUserBlock::new(
                        "project_instruction",
                        Some("AGENTS.md".to_string()),
                        Some("/repo/AGENTS.md".to_string()),
                        "root rules",
                    ),
                    ContextualUserBlock::new(
                        "project_instruction",
                        Some("AGENTS.local.md".to_string()),
                        Some("/repo/AGENTS.local.md".to_string()),
                        "local rules",
                    ),
                ],
            )],
            turn_contextual_user_messages: Vec::new(),
            prompt_messages: vec![user_text_message("accepted prompt")],
            tools: Vec::new(),
            tool_search: ToolSearchOptions::disabled(),
            max_turns: 1,
        },
        Arc::new(NoopEventSink),
        control,
    )
    .await
    .expect("loop");
    assert_eq!(completion.outcome, Outcome::Normal);
    let Message::User { content, .. } = &completion.messages[0] else {
        panic!("completion user message");
    };
    assert_eq!(content, &[UserContentBlock::text("accepted prompt")]);

    let requests = requests.lock().expect("requests");
    let messages =
        serde_json::to_value(&requests[0].messages).expect("typed language request messages");
    let messages = messages.as_array().expect("message array");
    assert_eq!(messages.len(), 3);
    assert_eq!(
        messages[0]["extensions"]["psychevo"]["provider_group"],
        "project_instructions"
    );
    assert_eq!(
        messages[0]["extensions"]["psychevo"]["context_category"],
        "project_context"
    );
    assert_eq!(messages[0]["content"].as_array().expect("blocks").len(), 2);
    assert_eq!(messages[0]["content"][0]["text"], "root rules");
    assert_eq!(messages[0]["content"][1]["text"], "local rules");
    assert_eq!(messages[1]["content"][0]["text"], "previous");
    assert_eq!(messages[2]["content"][0]["text"], "accepted prompt");
}

#[tokio::test]
pub(crate) async fn reasoning_only_progress_has_no_visible_message_update() {
    let provider = raw_test_model(vec![vec![
        RawStreamEvent::Reasoning("private".to_string()),
        RawStreamEvent::Text("visible".to_string()),
        RawStreamEvent::Done(Outcome::Normal),
    ]]);
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

#[test]
pub(crate) fn user_message_deserializes_text_blocks_and_serializes_local_images() {
    let text_message = serde_json::from_value::<Message>(json!({
        "role": "user",
        "content": [{ "text": "hello" }],
        "timestamp_ms": 1
    }))
    .expect("text user message");

    assert_eq!(
        text_message,
        Message::User {
            content: vec![UserContentBlock::text("hello")],
            timestamp_ms: 1,
        }
    );

    let image_message = Message::User {
        content: vec![
            UserContentBlock::local_image("/tmp/image.avif"),
            UserContentBlock::image_url("https://example.com/image.png"),
        ],
        timestamp_ms: 2,
    };
    let value = serde_json::to_value(image_message).expect("image user message");

    assert_eq!(
        value,
        json!({
            "role": "user",
            "content": [
                { "type": "local_image", "path": "/tmp/image.avif" },
                { "type": "image_url", "url": "https://example.com/image.png" }
            ],
            "timestamp_ms": 2
        })
    );
}

#[test]
pub(crate) fn assistant_source_blocks_round_trip_without_discriminator_collision() {
    let message = Message::Assistant {
        content: vec![
            AssistantBlock::Source {
                source: AssistantSource::UrlCitation(psychevo_ai::UrlCitationSource {
                    url: "https://example.com/source".to_string(),
                    title: "Source".to_string(),
                    start_index: Some(1),
                    end_index: Some(7),
                }),
            },
            AssistantBlock::Source {
                source: AssistantSource::Image(psychevo_ai::ImageSearchSource {
                    image_url: "https://example.com/image.png".to_string(),
                    thumbnail_url: Some("https://example.com/thumb.png".to_string()),
                    source_website_url: "https://example.com".to_string(),
                    caption: Some("Image".to_string()),
                }),
            },
            AssistantBlock::Source {
                source: AssistantSource::Provider {
                    kind: "future_source".to_string(),
                    data: json!({ "id": "source-1" }),
                },
            },
        ],
        timestamp_ms: 3,
        finish_reason: Some("stop".to_string()),
        outcome: Outcome::Normal,
        model: Some("model".to_string()),
        provider: Some("provider".to_string()),
    };

    let value = serde_json::to_value(&message).expect("assistant source message json");
    assert_eq!(
        value["content"],
        json!([
            {
                "type": "source",
                "source": {
                    "type": "url_citation",
                    "url": "https://example.com/source",
                    "title": "Source",
                    "start_index": 1,
                    "end_index": 7
                }
            },
            {
                "type": "source",
                "source": {
                    "type": "image",
                    "image_url": "https://example.com/image.png",
                    "thumbnail_url": "https://example.com/thumb.png",
                    "source_website_url": "https://example.com",
                    "caption": "Image"
                }
            },
            {
                "type": "source",
                "source": {
                    "type": "provider",
                    "kind": "future_source",
                    "data": { "id": "source-1" }
                }
            }
        ])
    );
    let decoded = serde_json::from_value::<Message>(value).expect("assistant source round trip");
    assert_eq!(decoded, message);
}

#[tokio::test]
pub(crate) async fn usage_and_metadata_do_not_emit_empty_message_updates() {
    let provider = test_model(StaticProvider {
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
    assert!(events.iter().any(|event| {
        matches!(
            event,
            AgentEvent::MessageEnd { usage: Some(usage), metadata: Some(metadata), .. }
                if usage["total_tokens"] == 1
                    && metadata["provider_response_id"] == "resp"
        )
    }));
}

#[tokio::test]
pub(crate) async fn text_stream_preserves_linear_delta_content_without_full_message_per_token() {
    let mut provider_events = (0..1_000)
        .map(|_| StreamEvent::TextDelta {
            text: "x".to_string(),
        })
        .collect::<Vec<_>>();
    provider_events.push(StreamEvent::Done {
        outcome: Outcome::Normal,
        finish_reason: Some("stop".to_string()),
    });
    let provider = test_model(StaticProvider {
        events: provider_events,
    });
    let sink = Arc::new(CaptureSink::default());
    let (_, control) = ControlHandle::new();

    let completion = run_agent_loop(provider, request(), sink.clone(), control)
        .await
        .expect("loop");

    let events = sink.events.lock().expect("events");
    let text_deltas = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::AssistantTextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(!text_deltas.is_empty());
    assert_eq!(
        text_deltas.iter().map(|delta| delta.len()).sum::<usize>(),
        1_000
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AgentEvent::MessageUpdate { .. }))
            .count(),
        1
    );
    let Message::Assistant { content, .. } = completion.messages.last().expect("assistant") else {
        panic!("assistant message");
    };
    assert!(matches!(
        content.as_slice(),
        [AssistantBlock::Text { text }] if text.len() == 1_000
    ));
}

#[test]
pub(crate) fn inline_think_parser_handles_split_tags_without_rescanning_history() {
    let mut parser = InlineThinkParser::new();
    let chunks = ["hello <", "think> private", " </thi", "nk> world"];
    let mut visible = String::new();
    let mut reasoning = String::new();
    for chunk in chunks {
        let (visible_delta, reasoning_delta) = parser.push(chunk);
        visible.push_str(&visible_delta);
        reasoning.push_str(&reasoning_delta);
    }
    let (visible_delta, reasoning_delta) = parser.finish();
    visible.push_str(&visible_delta);
    reasoning.push_str(&reasoning_delta);

    assert_eq!(visible, "hello  world");
    assert_eq!(reasoning, "private");
    assert_eq!(parser.visible(), visible);
    assert_eq!(parser.reasoning(), reasoning);
}

#[tokio::test]
pub(crate) async fn tool_call_pending_is_emitted_before_message_end() {
    let provider = test_model(StaticProvider {
        events: vec![
            StreamEvent::ToolCallStart {
                content_index: 0,
                call_index: 0,
                id: "call_write".to_string(),
                name: "write".to_string(),
            },
            StreamEvent::ToolCallDelta {
                content_index: 0,
                call_index: 0,
                id: Some("call_write".to_string()),
                name: Some("write".to_string()),
                arguments_delta: "{\"path\":\"report.md\"".to_string(),
            },
            StreamEvent::Done {
                outcome: Outcome::Normal,
                finish_reason: Some("tool_calls".to_string()),
            },
        ],
    });
    let sink = Arc::new(CaptureSink::default());
    let (_, control) = ControlHandle::new();
    let router = ToolRouter::from_tools(request().tools).expect("unique tools");
    stream_assistant(
        provider,
        &request(),
        &router,
        &[],
        sink.clone(),
        control.abort_signal(),
    )
    .await
    .expect("assistant");

    let events = sink.events.lock().expect("events");
    let pending_index = events
        .iter()
        .position(|event| {
            matches!(
                event,
                AgentEvent::ToolCallPending {
                    tool_call_id,
                    tool_name,
                    arguments_json,
                    ..
                } if tool_call_id == "call_write"
                    && tool_name == "write"
                    && arguments_json.is_empty()
            )
        })
        .expect("pending tool call");
    let message_end_index = events
        .iter()
        .position(|event| matches!(event, AgentEvent::MessageEnd { .. }))
        .expect("message end");
    assert!(pending_index < message_end_index);
    assert!(events.iter().any(|event| {
        matches!(
            event,
            AgentEvent::ToolCallPending { arguments_json, .. }
                if arguments_json == "{\"path\":\"report.md\""
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            AgentEvent::ToolCallPending { display: Some(display), .. }
                if display.category == ToolDisplayCategory::Update
        )
    }));
}

#[tokio::test]
pub(crate) async fn abort_does_not_drain_buffered_generation_deltas() {
    let mut events = (0..2_048)
        .map(|_| StreamEvent::TextDelta {
            text: "x".to_string(),
        })
        .collect::<Vec<_>>();
    events.push(StreamEvent::Done {
        outcome: Outcome::Normal,
        finish_reason: Some("stop".to_string()),
    });
    let provider = test_model(StaticProvider { events });
    let (control, receivers) = ControlHandle::new();
    let sink = Arc::new(AbortOnFirstDeltaSink {
        control,
        events: Mutex::new(Vec::new()),
        deltas: AtomicUsize::new(0),
    });

    let completion = run_agent_loop(provider, request(), sink.clone(), receivers)
        .await
        .expect("aborted loop");

    assert_eq!(completion.outcome, Outcome::Aborted);
    assert_eq!(
        sink.deltas.load(Ordering::SeqCst),
        1,
        "buffered deltas after cancellation must be discarded"
    );
    assert!(
        sink.events
            .lock()
            .expect("events")
            .iter()
            .any(|event| matches!(
                event,
                AgentEvent::AgentEnd {
                    outcome: Outcome::Aborted,
                    ..
                }
            ))
    );
}

#[tokio::test]
pub(crate) async fn content_filter_completion_is_stopped() {
    let provider = test_model(StaticProvider {
        events: vec![
            StreamEvent::TextDelta {
                text: "filtered partial".to_string(),
            },
            StreamEvent::Done {
                outcome: Outcome::Normal,
                finish_reason: Some("content_filter".to_string()),
            },
        ],
    });
    let (_, receivers) = ControlHandle::new();

    let completion = run_agent_loop(
        provider,
        request(),
        Arc::new(CaptureSink::default()),
        receivers,
    )
    .await
    .expect("filtered completion");

    assert_eq!(completion.outcome, Outcome::Stopped);
    assert!(matches!(
        completion.messages.last(),
        Some(Message::Assistant {
            outcome: Outcome::Stopped,
            finish_reason: Some(reason),
            ..
        }) if reason == "content_filter"
    ));
}

#[tokio::test]
pub(crate) async fn tool_output_can_separate_event_json_from_model_content() {
    #[derive(Clone)]
    struct SequencedProvider {
        responses: Arc<Mutex<Vec<Vec<StreamEvent>>>>,
    }

    impl LanguageAdapter for SequencedProvider {
        fn stream(
            &self,
            _call: AdapterCall<LanguageRequest>,
        ) -> AdapterFuture<'_, AdapterStream<LanguageAdapterEvent>> {
            let events = self.responses.lock().expect("responses").remove(0);
            let events = normalize_test_events(events).into_iter().map(Ok);
            Box::pin(async move { Ok(Box::pin(stream::iter(events)) as AdapterStream<_>) })
        }
    }

    pub(crate) struct SplitOutputTool;

    impl ToolBinding for SplitOutputTool {
        fn name(&self) -> &str {
            "split_output"
        }

        fn description(&self) -> &str {
            "Return full event JSON with compact model content."
        }

        fn parameters(&self) -> Value {
            json!({"type": "object", "properties": {}, "additionalProperties": false})
        }

        fn execution_mode(&self) -> ToolExecutionMode {
            ToolExecutionMode::Parallel
        }

        fn execute(
            &self,
            _tool_call_id: String,
            _args: Value,
            _abort: AbortSignal,
        ) -> BoxFuture<'static, ToolOutput> {
            Box::pin(async {
                ToolOutput::ok_with_model_content(
                    json!({
                        "full": {
                            "child_session_id": "child-session",
                            "usage": {"total_tokens": 42}
                        }
                    }),
                    r#"{"summary":"compact"}"#,
                )
            })
        }
    }

    let provider = test_model(SequencedProvider {
        responses: Arc::new(Mutex::new(vec![
            vec![
                StreamEvent::ToolCallStart {
                    content_index: 0,
                    call_index: 0,
                    id: "call_split".to_string(),
                    name: "split_output".to_string(),
                },
                StreamEvent::ToolCallDelta {
                    content_index: 0,
                    call_index: 0,
                    id: Some("call_split".to_string()),
                    name: Some("split_output".to_string()),
                    arguments_delta: "{}".to_string(),
                },
                StreamEvent::ToolCallEnd {
                    content_index: 0,
                    call_index: 0,
                },
                StreamEvent::Done {
                    outcome: Outcome::Normal,
                    finish_reason: Some("tool_calls".to_string()),
                },
            ],
            vec![
                StreamEvent::TextDelta {
                    text: "done".to_string(),
                },
                StreamEvent::Done {
                    outcome: Outcome::Normal,
                    finish_reason: Some("stop".to_string()),
                },
            ],
        ])),
    });
    let sink = Arc::new(CaptureSink::default());
    let (_, control) = ControlHandle::new();
    let completion = run_agent_loop(
        provider,
        AgentLoopRequest {
            tools: vec![Arc::new(SplitOutputTool)],
            tool_search: ToolSearchOptions::disabled(),
            max_turns: 2,
            ..request()
        },
        sink.clone(),
        control,
    )
    .await
    .expect("loop");

    let events = sink.events.lock().expect("events");
    assert!(events.iter().any(|event| {
        matches!(
            event,
            AgentEvent::ToolExecutionEnd { result, .. }
                if result["full"]["child_session_id"] == "child-session"
                    && result["full"]["usage"]["total_tokens"] == 42
        )
    }));
    let tool_content = completion
        .messages
        .iter()
        .find_map(|message| match message {
            Message::ToolResult { content, .. } => Some(content.as_str()),
            _ => None,
        })
        .expect("tool result");
    assert_eq!(tool_content, r#"{"summary":"compact"}"#);
    assert!(!tool_content.contains("child_session_id"));
    assert!(!tool_content.contains("usage"));
}

#[tokio::test]
pub(crate) async fn complete_inline_think_blocks_are_folded_reasoning() {
    let provider = raw_test_model(vec![vec![
        RawStreamEvent::Text("visible <think>secret</think> done".to_string()),
        RawStreamEvent::Done(Outcome::Normal),
    ]]);
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
    assert!(
        events.iter().any(|event| {
            matches!(event, AgentEvent::ReasoningEnd { text } if text == "secret")
        })
    );
}

#[tokio::test]
pub(crate) async fn reasoning_details_attach_to_reasoning_block_evidence() {
    let provider = test_model(StaticProvider {
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

#[tokio::test]
pub(crate) async fn final_assistant_uses_sdk_content_order_and_native_reasoning_evidence() {
    let provider = test_model(FakeLanguageAdapter::new(vec![
        vec![
            Ok(LanguageAdapterEvent::ToolCallStart {
                content_index: 0,
                id: "call-1".to_string(),
                name: "display_only".to_string(),
            }),
            Ok(LanguageAdapterEvent::ToolCallArgumentsDelta {
                content_index: 0,
                delta: "{}".to_string(),
            }),
            Ok(LanguageAdapterEvent::ToolCallEnd {
                content_index: 0,
                arguments_raw: "{}".to_string(),
            }),
            Ok(LanguageAdapterEvent::TextStart { content_index: 1 }),
            Ok(LanguageAdapterEvent::TextDelta {
                content_index: 1,
                delta: "visible".to_string(),
            }),
            Ok(LanguageAdapterEvent::TextEnd { content_index: 1 }),
            Ok(LanguageAdapterEvent::ReasoningStart { content_index: 2 }),
            Ok(LanguageAdapterEvent::ReasoningDelta {
                content_index: 2,
                delta: "private".to_string(),
                provider_evidence: Some(json!({"signature": "signed-thinking"})),
            }),
            Ok(LanguageAdapterEvent::ReasoningEnd { content_index: 2 }),
            Ok(LanguageAdapterEvent::Finish {
                finish_reason: Some(finish_reason("tool_calls")),
            }),
        ],
        vec![
            Ok(LanguageAdapterEvent::TextStart { content_index: 0 }),
            Ok(LanguageAdapterEvent::TextDelta {
                content_index: 0,
                delta: "done".to_string(),
            }),
            Ok(LanguageAdapterEvent::TextEnd { content_index: 0 }),
            Ok(LanguageAdapterEvent::Finish {
                finish_reason: Some(finish_reason("stop")),
            }),
        ],
    ]));
    let (_, control) = ControlHandle::new();
    let completion = run_agent_loop(
        provider,
        AgentLoopRequest {
            tools: vec![Arc::new(DisplayOnlyTool)],
            max_turns: 2,
            ..request()
        },
        Arc::new(CaptureSink::default()),
        control,
    )
    .await
    .expect("loop");
    let content = completion
        .messages
        .iter()
        .find_map(|message| match message {
            Message::Assistant { content, .. }
                if content
                    .iter()
                    .any(|block| matches!(block, AssistantBlock::ToolCall(_))) =>
            {
                Some(content)
            }
            _ => None,
        })
        .expect("assistant with tool call");

    assert!(matches!(content[0], AssistantBlock::ToolCall(_)));
    assert_eq!(
        content[1],
        AssistantBlock::Text {
            text: "visible".to_string(),
        }
    );
    assert_eq!(
        content[2],
        AssistantBlock::Reasoning {
            text: "private".to_string(),
            provider_evidence: Some(json!({"signature": "signed-thinking"})),
        }
    );
}

#[test]
fn control_state_is_retained_after_receivers_are_dropped() {
    let (control, receivers) = ControlHandle::new();
    drop(receivers);

    control.stop();
    control.abort();

    assert!(control.is_stopped());
    assert!(control.is_aborted());
}
