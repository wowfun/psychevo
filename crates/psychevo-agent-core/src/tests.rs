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

#[derive(Clone, Default)]
struct RequestCaptureProvider {
    requests: Arc<Mutex<Vec<GenerationRequest>>>,
}

impl GenerationProvider for RequestCaptureProvider {
    fn stream(
        &self,
        request: GenerationRequest,
        _abort: AbortSignal,
    ) -> BoxFuture<'static, psychevo_ai::Result<psychevo_ai::GenerationStream>> {
        self.requests.lock().expect("requests").push(request);
        Box::pin(async {
            let output: psychevo_ai::GenerationStream =
                Box::pin(stream::iter([Ok(StreamEvent::Done {
                    outcome: Outcome::Normal,
                    finish_reason: Some("stop".to_string()),
                })]));
            Ok(output)
        })
    }
}

fn request() -> AgentLoopRequest {
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
        max_turns: 1,
    }
}

#[tokio::test]
async fn prefix_contextual_user_messages_are_inserted_before_history() {
    let provider = RequestCaptureProvider::default();
    let requests = Arc::clone(&provider.requests);
    let (_, control) = ControlHandle::new();
    let completion = run_agent_loop(
        Arc::new(provider),
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
    let messages = &requests[0].messages;
    assert_eq!(messages.len(), 3);
    assert_eq!(
        messages[0]["metadata"]["provider_group"],
        "project_instructions"
    );
    assert_eq!(
        messages[0]["metadata"]["context_category"],
        "project_context"
    );
    assert_eq!(messages[0]["content"].as_array().expect("blocks").len(), 2);
    assert_eq!(messages[0]["content"][0]["text"], "root rules");
    assert_eq!(messages[0]["content"][1]["text"], "local rules");
    assert_eq!(messages[1]["content"][0]["text"], "previous");
    assert_eq!(messages[2]["content"][0]["text"], "accepted prompt");
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

#[test]
fn user_message_deserializes_text_blocks_and_serializes_local_images() {
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
async fn tool_call_pending_is_emitted_before_message_end() {
    let provider = Arc::new(StaticProvider {
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
    stream_assistant(
        provider,
        &request(),
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
}

#[tokio::test]
async fn tool_output_can_separate_event_json_from_model_content() {
    #[derive(Clone)]
    struct SequencedProvider {
        responses: Arc<Mutex<Vec<Vec<StreamEvent>>>>,
    }

    impl GenerationProvider for SequencedProvider {
        fn stream(
            &self,
            _request: GenerationRequest,
            _abort: AbortSignal,
        ) -> BoxFuture<'static, psychevo_ai::Result<psychevo_ai::GenerationStream>> {
            let events = self
                .responses
                .lock()
                .expect("responses")
                .remove(0)
                .into_iter()
                .map(Ok);
            Box::pin(async move {
                let output: psychevo_ai::GenerationStream = Box::pin(stream::iter(events));
                Ok(output)
            })
        }
    }

    struct SplitOutputTool;

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

    let provider = Arc::new(SequencedProvider {
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
    assert!(
        events.iter().any(|event| {
            matches!(event, AgentEvent::ReasoningEnd { text } if text == "secret")
        })
    );
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
