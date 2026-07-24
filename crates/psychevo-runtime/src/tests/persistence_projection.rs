#[allow(unused_imports)]
pub(crate) use super::*;

#[test]
fn editable_input_envelope_is_versioned_and_separate_from_display_metadata() {
    let display = PromptDisplayMetadata {
        content_text: "describe image".to_string(),
        attachments: Vec::new(),
        editable_input: Some(StoredEditableInputEnvelope {
            version: 1,
            parts: vec![
                StoredEditableInputPart::Text {
                    text: "describe ".to_string(),
                },
                StoredEditableInputPart::Image {
                    image_block_index: 0,
                },
                StoredEditableInputPart::Text {
                    text: " precisely".to_string(),
                },
            ],
        }),
    };

    let (metadata, content_override) =
        crate::events::prompt_user_metadata(None, Some(&display), None);
    let metadata = metadata.expect("metadata");
    assert_eq!(content_override.as_deref(), Some("describe image"));
    assert_eq!(
        metadata[EDITABLE_INPUT_METADATA_KEY],
        json!({
            "version": 1,
            "parts": [
                {"type": "text", "text": "describe "},
                {"type": "image", "imageBlockIndex": 0},
                {"type": "text", "text": " precisely"}
            ]
        })
    );
    assert!(
        metadata[TUI_DISPLAY_METADATA_KEY]
            .get("editable_input")
            .is_none()
    );
}

#[tokio::test]
pub(crate) async fn persistence_sink_streams_elapsed_metadata_for_assistant_message_end() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = StateRuntime::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .expect("session");
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_for_stream = Arc::clone(&captured);
    let stream: RunStreamSink = Arc::new(move |event| {
        captured_for_stream
            .lock()
            .expect("captured stream lock")
            .push(event);
    });
    let started = Instant::now()
        .checked_sub(Duration::from_millis(1200))
        .expect("instant");
    let sink = PersistenceSink {
        store: store.clone(),
        session_id: session_id.clone(),
        prompt_snapshot: None,
        prompt_snapshot_written: Arc::new(Mutex::new(false)),
        prompt_context_evidence: Arc::new(Vec::new()),
        started,
        tool_elapsed_ms: Arc::new(Mutex::new(BTreeMap::new())),
        current_turn_index: Arc::new(Mutex::new(None)),
        control: SmokeControl::None,
        control_handle: None,
        events: None,
        stream_events: Some(stream),
        trace: None,
        trace_warning_emitted: Arc::new(Mutex::new(false)),
        include_reasoning: false,
        reasoning_effort: None,
        model_metadata: Default::default(),
        prompt_display: None,
        context_recorder: None,
        selected_agent: None,
        prompt_prefix_metadata: None,
    };

    sink.emit(AgentEvent::MessageEnd {
        message: Message::Assistant {
            content: vec![AssistantBlock::Text {
                text: "hi".to_string(),
            }],
            timestamp_ms: 1,
            finish_reason: Some("stop".to_string()),
            outcome: Outcome::Normal,
            model: Some("model".to_string()),
            provider: Some("provider".to_string()),
        },
        usage: None,
        metadata: Some(json!({"provider_response_id": "resp_1"})),
    })
    .await
    .expect("message end");

    let elapsed = match captured.lock().expect("captured stream lock").as_slice() {
        [RunStreamEvent::Event(value)] => value["metadata"]["elapsed_ms"]
            .as_u64()
            .expect("stream elapsed"),
        other => panic!("unexpected stream events: {other:?}"),
    };
    assert!(elapsed >= 1200);
    let summaries = store
        .load_tui_message_summaries(&session_id)
        .expect("summaries");
    assert_eq!(
        summaries[0].metadata.as_ref().unwrap()["provider_response_id"],
        "resp_1"
    );
    assert_eq!(
        summaries[0].metadata.as_ref().unwrap()["elapsed_ms"]
            .as_u64()
            .expect("stored elapsed"),
        elapsed
    );
    assert!(
        summaries[0].metadata.as_ref().unwrap()["reasoning_effort"].is_null(),
        "absent or none reasoning effort must not be stored"
    );
}

#[tokio::test]
pub(crate) async fn persistence_sink_persists_selected_agent_on_assistant_message_end() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = StateRuntime::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .expect("session");
    let sink = PersistenceSink {
        store: store.clone(),
        session_id: session_id.clone(),
        prompt_snapshot: None,
        prompt_snapshot_written: Arc::new(Mutex::new(false)),
        prompt_context_evidence: Arc::new(Vec::new()),
        started: Instant::now(),
        tool_elapsed_ms: Arc::new(Mutex::new(BTreeMap::new())),
        current_turn_index: Arc::new(Mutex::new(None)),
        control: SmokeControl::None,
        control_handle: None,
        events: None,
        stream_events: None,
        trace: None,
        trace_warning_emitted: Arc::new(Mutex::new(false)),
        include_reasoning: false,
        reasoning_effort: None,
        model_metadata: Default::default(),
        prompt_display: None,
        context_recorder: None,
        selected_agent: Some(SelectedAgent {
            name: "translate".to_string(),
            source: "project".to_string(),
            path: Some(cwd.join(".psychevo/agents/translate.md")),
        }),
        prompt_prefix_metadata: None,
    };

    sink.emit(AgentEvent::MessageEnd {
        message: Message::Assistant {
            content: vec![AssistantBlock::Text {
                text: "hi".to_string(),
            }],
            timestamp_ms: 1,
            finish_reason: Some("stop".to_string()),
            outcome: Outcome::Normal,
            model: Some("model".to_string()),
            provider: Some("provider".to_string()),
        },
        usage: None,
        metadata: None,
    })
    .await
    .expect("message end");

    let summaries = store
        .load_tui_message_summaries(&session_id)
        .expect("summaries");
    let metadata = summaries[0].metadata.as_ref().expect("metadata");
    assert_eq!(metadata["selected_agent"]["name"], "translate");
    assert_eq!(metadata["selected_agent"]["source"], "project");
}

#[tokio::test]
pub(crate) async fn persistence_sink_projects_and_persists_terminal_reason() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = StateRuntime::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(
            &cwd,
            "tui",
            "model",
            "provider",
            Some(json!({"provider_label": "Mock"})),
        )
        .expect("session");
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_for_stream = Arc::clone(&captured);
    let stream: RunStreamSink = Arc::new(move |event| {
        captured_for_stream
            .lock()
            .expect("captured stream lock")
            .push(event);
    });
    let sink = PersistenceSink {
        store: store.clone(),
        session_id: session_id.clone(),
        prompt_snapshot: None,
        prompt_snapshot_written: Arc::new(Mutex::new(false)),
        prompt_context_evidence: Arc::new(Vec::new()),
        started: Instant::now(),
        tool_elapsed_ms: Arc::new(Mutex::new(BTreeMap::new())),
        current_turn_index: Arc::new(Mutex::new(None)),
        control: SmokeControl::None,
        control_handle: None,
        events: None,
        stream_events: Some(stream),
        trace: None,
        trace_warning_emitted: Arc::new(Mutex::new(false)),
        include_reasoning: false,
        reasoning_effort: None,
        model_metadata: Default::default(),
        prompt_display: None,
        context_recorder: None,
        selected_agent: None,
        prompt_prefix_metadata: None,
    };

    sink.emit(AgentEvent::AgentEnd {
        outcome: Outcome::Failed,
        messages: Vec::new(),
        terminal_reason: Some(psychevo_agent_core::TerminalReason::MaxTurnsExceeded {
            max_turns: 128,
        }),
    })
    .await
    .expect("agent end");

    let event = match captured.lock().expect("captured stream lock").as_slice() {
        [RunStreamEvent::Event(value)] => value.as_value().clone(),
        other => panic!("unexpected stream events: {other:?}"),
    };
    assert_eq!(event["type"], "agent_end");
    assert_eq!(event["outcome"], "failed");
    assert_eq!(event["terminal_reason"]["type"], "max_turns_exceeded");
    assert_eq!(event["terminal_reason"]["max_turns"], 128);
    assert!(
        event["terminal_message"]
            .as_str()
            .expect("terminal message")
            .contains("model-turn limit (128)")
    );

    let metadata = store
        .session_metadata(&session_id)
        .expect("metadata")
        .expect("metadata");
    assert_eq!(metadata["provider_label"], "Mock");
    assert_eq!(metadata["terminal_reason"]["type"], "max_turns_exceeded");
    assert_eq!(metadata["terminal_reason"]["max_turns"], 128);
    let summary = store
        .session_summary(&session_id)
        .expect("summary")
        .expect("summary");
    assert_eq!(summary.end_reason.as_deref(), Some("failed"));
}

#[tokio::test]
pub(crate) async fn persistence_sink_persists_assistant_reasoning_effort_metadata() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = StateRuntime::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .expect("session");
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_for_stream = Arc::clone(&captured);
    let stream: RunStreamSink = Arc::new(move |event| {
        captured_for_stream
            .lock()
            .expect("captured stream lock")
            .push(event);
    });
    let sink = PersistenceSink {
        store: store.clone(),
        session_id: session_id.clone(),
        prompt_snapshot: None,
        prompt_snapshot_written: Arc::new(Mutex::new(false)),
        prompt_context_evidence: Arc::new(Vec::new()),
        started: Instant::now(),
        tool_elapsed_ms: Arc::new(Mutex::new(BTreeMap::new())),
        current_turn_index: Arc::new(Mutex::new(None)),
        control: SmokeControl::None,
        control_handle: None,
        events: None,
        stream_events: Some(stream),
        trace: None,
        trace_warning_emitted: Arc::new(Mutex::new(false)),
        include_reasoning: false,
        reasoning_effort: Some("high".to_string()),
        model_metadata: Default::default(),
        prompt_display: None,
        context_recorder: None,
        selected_agent: None,
        prompt_prefix_metadata: None,
    };

    sink.emit(AgentEvent::MessageEnd {
        message: Message::Assistant {
            content: vec![AssistantBlock::Text {
                text: "hi".to_string(),
            }],
            timestamp_ms: 1,
            finish_reason: Some("stop".to_string()),
            outcome: Outcome::Normal,
            model: Some("model".to_string()),
            provider: Some("provider".to_string()),
        },
        usage: None,
        metadata: None,
    })
    .await
    .expect("message end");

    match captured.lock().expect("captured stream lock").as_slice() {
        [RunStreamEvent::Event(value)] => {
            assert_eq!(value["metadata"]["reasoning_effort"], "high");
            assert!(value["metadata"]["elapsed_ms"].as_u64().is_some());
        }
        other => panic!("unexpected stream events: {other:?}"),
    }
    let summaries = store
        .load_tui_message_summaries(&session_id)
        .expect("summaries");
    assert_eq!(
        summaries[0].metadata.as_ref().unwrap()["reasoning_effort"],
        "high"
    );
}

#[tokio::test]
pub(crate) async fn persistence_sink_persists_tool_elapsed_metadata() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = StateRuntime::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .expect("session");
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_for_stream = Arc::clone(&captured);
    let stream: RunStreamSink = Arc::new(move |event| {
        captured_for_stream
            .lock()
            .expect("captured stream lock")
            .push(event);
    });
    let sink = PersistenceSink {
        store: store.clone(),
        session_id: session_id.clone(),
        prompt_snapshot: None,
        prompt_snapshot_written: Arc::new(Mutex::new(false)),
        prompt_context_evidence: Arc::new(Vec::new()),
        started: Instant::now(),
        tool_elapsed_ms: Arc::new(Mutex::new(BTreeMap::new())),
        current_turn_index: Arc::new(Mutex::new(None)),
        control: SmokeControl::None,
        control_handle: None,
        events: None,
        stream_events: Some(stream),
        trace: None,
        trace_warning_emitted: Arc::new(Mutex::new(false)),
        include_reasoning: false,
        reasoning_effort: None,
        model_metadata: Default::default(),
        prompt_display: None,
        context_recorder: None,
        selected_agent: None,
        prompt_prefix_metadata: None,
    };

    sink.emit(AgentEvent::ToolExecutionEnd {
        tool_call_id: "call_read".to_string(),
        tool_name: "read".to_string(),
        result: json!({"content":"done"}),
        outcome: Outcome::Normal,
        elapsed_ms: 321,
        display: None,
    })
    .await
    .expect("tool end");
    sink.emit(AgentEvent::MessageEnd {
        message: Message::ToolResult {
            tool_call_id: "call_read".to_string(),
            tool_name: "read".to_string(),
            content: "{\"content\":\"done\"}".to_string(),
            is_error: false,
            timestamp_ms: 2,
        },
        usage: None,
        metadata: None,
    })
    .await
    .expect("tool result");

    let stream_events = captured.lock().expect("captured stream lock");
    assert!(
        stream_events.iter().any(|event| {
            matches!(
                event,
                RunStreamEvent::Event(value)
                    if value["type"] == "tool_execution_end"
                        && value["elapsed_ms"] == 321
            )
        }),
        "tool_execution_end should expose elapsed_ms"
    );
    assert!(
        stream_events.iter().any(|event| {
            matches!(
                event,
                RunStreamEvent::Event(value)
                    if value["type"] == "message_end"
                        && value["metadata"]["elapsed_ms"] == 321
            )
        }),
        "tool_result message_end should expose elapsed metadata"
    );
    drop(stream_events);

    let summaries = store
        .load_tui_message_summaries(&session_id)
        .expect("summaries");
    assert_eq!(
        summaries[0].metadata.as_ref().unwrap()["elapsed_ms"]
            .as_u64()
            .expect("stored elapsed"),
        321
    );
}

#[tokio::test]
pub(crate) async fn messages_preserve_assistant_content_order() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = StateRuntime::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .expect("session");
    let sink = test_persistence_sink(store.clone(), session_id.clone());

    sink.emit(AgentEvent::MessageEnd {
        message: Message::Assistant {
            content: vec![
                AssistantBlock::Reasoning {
                    text: "think first".to_string(),
                    provider_evidence: None,
                },
                AssistantBlock::Text {
                    text: "answer after thinking".to_string(),
                },
                AssistantBlock::ToolCall(psychevo_agent_core::ToolCallBlock {
                    id: "call_exec".to_string(),
                    name: "exec_command".to_string(),
                    arguments: json!({"cmd": "date"}),
                    arguments_json: "{\"cmd\":\"date\"}".to_string(),
                    arguments_error: None,
                    content_index: 0,
                    call_index: 0,
                }),
            ],
            timestamp_ms: 1,
            finish_reason: Some("tool_calls".to_string()),
            outcome: Outcome::Normal,
            model: Some("model".to_string()),
            provider: Some("provider".to_string()),
        },
        usage: None,
        metadata: None,
    })
    .await
    .expect("message end");

    let messages = store.load_messages(&session_id).expect("messages");
    let [
        Message::Assistant {
            content,
            finish_reason,
            outcome,
            model,
            provider,
            ..
        },
    ] = messages.as_slice()
    else {
        panic!("unexpected messages: {messages:?}");
    };
    assert_eq!(finish_reason.as_deref(), Some("tool_calls"));
    assert_eq!(*outcome, Outcome::Normal);
    assert_eq!(model.as_deref(), Some("model"));
    assert_eq!(provider.as_deref(), Some("provider"));
    let [
        AssistantBlock::Reasoning {
            text: reasoning, ..
        },
        AssistantBlock::Text { text: answer },
        AssistantBlock::ToolCall(tool_call),
    ] = content.as_slice()
    else {
        panic!("unexpected assistant content: {content:?}");
    };
    assert_eq!(reasoning, "think first");
    assert_eq!(answer, "answer after thinking");
    assert_eq!(tool_call.id, "call_exec");
    assert_eq!(tool_call.name, "exec_command");
    assert_eq!(tool_call.arguments["cmd"], "date");
}

#[tokio::test]
pub(crate) async fn messages_preserve_tool_call_and_tool_result_facts() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = StateRuntime::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .expect("session");
    let sink = test_persistence_sink(store.clone(), session_id.clone());

    sink.emit(AgentEvent::MessageEnd {
        message: Message::Assistant {
            content: vec![AssistantBlock::ToolCall(
                psychevo_agent_core::ToolCallBlock {
                    id: "call_poll".to_string(),
                    name: "write_stdin".to_string(),
                    arguments: json!({"session_id": 7, "yield_time_ms": 60000}),
                    arguments_json: "{\"session_id\":7,\"yield_time_ms\":60000}".to_string(),
                    arguments_error: None,
                    content_index: 0,
                    call_index: 0,
                },
            )],
            timestamp_ms: 1,
            finish_reason: Some("tool_calls".to_string()),
            outcome: Outcome::Normal,
            model: Some("model".to_string()),
            provider: Some("provider".to_string()),
        },
        usage: None,
        metadata: None,
    })
    .await
    .expect("assistant message");
    sink.emit(AgentEvent::MessageEnd {
        message: Message::ToolResult {
            tool_call_id: "call_poll".to_string(),
            tool_name: "write_stdin".to_string(),
            content: "{\"session_id\":null,\"exit_code\":0,\"output\":\"done\"}".to_string(),
            is_error: false,
            timestamp_ms: 2,
        },
        usage: None,
        metadata: None,
    })
    .await
    .expect("tool result");

    let messages = store.load_messages(&session_id).expect("messages");
    let [
        Message::Assistant { content, .. },
        Message::ToolResult {
            tool_call_id,
            tool_name,
            content: result_content,
            is_error,
            ..
        },
    ] = messages.as_slice()
    else {
        panic!("unexpected messages: {messages:?}");
    };
    let [AssistantBlock::ToolCall(tool_call)] = content.as_slice() else {
        panic!("unexpected assistant content: {content:?}");
    };
    assert_eq!(tool_call.id, "call_poll");
    assert_eq!(tool_call.name, "write_stdin");
    assert_eq!(tool_call.arguments["session_id"], 7);
    assert_eq!(tool_call.arguments["yield_time_ms"], 60000);
    assert_eq!(tool_call_id, "call_poll");
    assert_eq!(tool_name, "write_stdin");
    assert!(!is_error);
    assert_eq!(
        serde_json::from_str::<Value>(result_content).unwrap()["output"],
        "done"
    );
}

#[tokio::test]
pub(crate) async fn messages_preserve_selected_skill_prompt_metadata() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = StateRuntime::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .expect("session");
    let mut sink = test_persistence_sink(store.clone(), session_id.clone());
    sink.prompt_prefix_metadata = Some(json!({
        "selected_skills": [
            {"name": "x-daily", "path": "/tmp/x-daily/SKILL.md"}
        ]
    }));

    sink.emit(AgentEvent::MessageEnd {
        message: user_message("$x-daily", 1),
        usage: None,
        metadata: None,
    })
    .await
    .expect("user message");

    let summaries = store
        .load_tui_message_summaries(&session_id)
        .expect("summaries");
    assert_eq!(summaries.len(), 1);
    assert_eq!(
        summaries[0].metadata.as_ref().unwrap()["prompt_prefix"]["selected_skills"][0]["name"],
        "x-daily"
    );
    assert_eq!(
        summaries[0].metadata.as_ref().unwrap()["prompt_prefix"]["selected_skills"][0]["path"],
        "/tmp/x-daily/SKILL.md"
    );
}

#[tokio::test]
pub(crate) async fn persistence_sink_persists_prompt_context_evidence_once() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = StateRuntime::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .expect("session");
    let sink = PersistenceSink {
        store: store.clone(),
        session_id: session_id.clone(),
        prompt_snapshot: Some("snapshot".to_string()),
        prompt_snapshot_written: Arc::new(Mutex::new(false)),
        prompt_context_evidence: Arc::new(vec![ContextEvidenceInput {
            role: "system".to_string(),
            source_kind: "system_instruction".to_string(),
            source_name: Some("mode".to_string()),
            source_path: None,
            provider_group: Some("system_instructions".to_string()),
            provider_block_index: Some(0),
            context_kind: Some("system_instruction".to_string()),
            content_text: "mode instruction".to_string(),
            metadata: Some(json!({ "instruction_index": 0 })),
        }]),
        started: Instant::now(),
        tool_elapsed_ms: Arc::new(Mutex::new(BTreeMap::new())),
        current_turn_index: Arc::new(Mutex::new(None)),
        control: SmokeControl::None,
        control_handle: None,
        events: None,
        stream_events: None,
        trace: None,
        trace_warning_emitted: Arc::new(Mutex::new(false)),
        include_reasoning: false,
        reasoning_effort: None,
        model_metadata: Default::default(),
        prompt_display: None,
        context_recorder: None,
        selected_agent: None,
        prompt_prefix_metadata: None,
    };

    sink.emit(AgentEvent::MessageEnd {
        message: user_message("first", 1),
        usage: None,
        metadata: None,
    })
    .await
    .expect("first prompt");
    sink.emit(AgentEvent::MessageEnd {
        message: user_message("second", 2),
        usage: None,
        metadata: None,
    })
    .await
    .expect("second prompt");

    assert_eq!(store.load_messages(&session_id).expect("messages").len(), 2);
    let first = store
        .load_context_evidence(&session_id, 1)
        .expect("first evidence");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].source_name.as_deref(), Some("mode"));
    assert_eq!(
        first[0].provider_group.as_deref(),
        Some("system_instructions")
    );
    assert_eq!(first[0].provider_block_index, Some(0));
    assert_eq!(first[0].context_kind.as_deref(), Some("system_instruction"));
    assert_eq!(first[0].content_text, "mode instruction");
    assert!(
        store
            .load_context_evidence(&session_id, 2)
            .expect("second evidence")
            .is_empty()
    );
}

#[test]
pub(crate) fn json_projection_hides_reasoning_unless_included() {
    let message = Message::Assistant {
        content: vec![
            AssistantBlock::Reasoning {
                text: "private".to_string(),
                provider_evidence: Some(json!({
                    "reasoning_details": [{ "type": "thinking" }]
                })),
            },
            AssistantBlock::Text {
                text: "visible".to_string(),
            },
        ],
        timestamp_ms: 1,
        finish_reason: Some("stop".to_string()),
        outcome: Outcome::Normal,
        model: Some("model".to_string()),
        provider: Some("provider".to_string()),
    };
    let event = AgentEvent::MessageEnd {
        message: message.clone(),
        usage: Some(json!({"total_tokens": 2})),
        metadata: Some(json!({"provider_response_id": "resp"})),
    };
    let hidden = project_agent_event(&event, false).expect("hidden");
    let hidden_text = serde_json::to_string(&hidden).expect("hidden json");
    assert!(hidden_text.contains("visible"));
    assert!(!hidden_text.contains("private"));
    assert!(!hidden_text.contains("reasoning_content"));
    assert!(!hidden_text.contains("total_tokens"));

    assert!(project_agent_event(&AgentEvent::ReasoningDelta { text: "x".into() }, false).is_none());
    let shown =
        project_agent_event(&AgentEvent::ReasoningDelta { text: "x".into() }, true).expect("shown");
    assert_eq!(shown, json!({"type":"reasoning_delta","text":"x"}));

    let stream =
        project_run_stream_event(&AgentEvent::ReasoningDelta { text: "x".into() }).expect("stream");
    assert_eq!(
        stream,
        RunStreamEvent::ReasoningDelta {
            text: "x".to_string()
        }
    );
    let streaming_update = project_run_stream_event(&AgentEvent::MessageUpdate {
        message: Message::Assistant {
            content: vec![AssistantBlock::Text {
                text: "streaming visible".to_string(),
            }],
            timestamp_ms: 2,
            finish_reason: None,
            outcome: Outcome::Normal,
            model: Some("model".to_string()),
            provider: Some("provider".to_string()),
        },
    })
    .expect("streaming update");
    match streaming_update {
        RunStreamEvent::Event(value) => {
            assert_eq!(value["type"], "message_update");
            assert_eq!(value["message"]["role"], "assistant");
            assert_eq!(value["message"]["content"][0]["text"], "streaming visible");
        }
        other => panic!("unexpected streaming update: {other:?}"),
    }
    assert!(
        project_agent_event(
            &AgentEvent::ToolCallPending {
                tool_call_id: "call_write".to_string(),
                tool_name: "write".to_string(),
                arguments_json: String::new(),
                content_index: 0,
                call_index: 0,
                display: None,
            },
            false,
        )
        .is_none()
    );
    let pending = project_run_stream_event(&AgentEvent::ToolCallPending {
        tool_call_id: "call_write".to_string(),
        tool_name: "write".to_string(),
        arguments_json: "{\"path\":\"report.md\"".to_string(),
        content_index: 0,
        call_index: 0,
        display: Some(ToolDisplaySpec::for_name("write")),
    })
    .expect("pending");
    match pending {
        RunStreamEvent::Event(value) => {
            assert_eq!(value["type"], "tool_call_pending");
            assert_eq!(value["tool_name"], "write");
            assert_eq!(value["arguments_json"], "{\"path\":\"report.md\"");
            assert_eq!(value["display"]["category"], "update");
        }
        other => panic!("unexpected pending event: {other:?}"),
    }
    let metrics = project_run_stream_event(&event).expect("metrics");
    match metrics {
        RunStreamEvent::Event(value) => {
            assert_eq!(value["usage"]["total_tokens"], 2);
            assert_eq!(value["metadata"]["provider_response_id"], "resp");
            assert!(
                !serde_json::to_string(value.as_value())
                    .unwrap()
                    .contains("private")
            );
        }
        other => panic!("unexpected stream event: {other:?}"),
    }
    let committed_steer = project_run_stream_event(&AgentEvent::MessageEnd {
        message: Message::User {
            content: vec![psychevo_agent_core::UserContentBlock::text("adjust")],
            timestamp_ms: 2,
        },
        usage: None,
        metadata: Some(json!({
            "pending_input": {
                "id": 7,
                "kind": "steer"
            }
        })),
    })
    .expect("committed steer");
    match committed_steer {
        RunStreamEvent::Event(value) => {
            assert_eq!(value["type"], "message_end");
            assert_eq!(value["message"]["role"], "user");
            assert_eq!(value["metadata"]["pending_input"]["id"], 7);
            assert_eq!(value["metadata"]["pending_input"]["kind"], "steer");
        }
        other => panic!("unexpected stream event: {other:?}"),
    }
}

fn test_persistence_sink(store: StateRuntime, session_id: String) -> PersistenceSink {
    PersistenceSink {
        store,
        session_id,
        prompt_snapshot: None,
        prompt_snapshot_written: Arc::new(Mutex::new(false)),
        prompt_context_evidence: Arc::new(Vec::new()),
        started: Instant::now(),
        tool_elapsed_ms: Arc::new(Mutex::new(BTreeMap::new())),
        current_turn_index: Arc::new(Mutex::new(None)),
        control: SmokeControl::None,
        control_handle: None,
        events: None,
        stream_events: None,
        trace: None,
        trace_warning_emitted: Arc::new(Mutex::new(false)),
        include_reasoning: false,
        reasoning_effort: None,
        model_metadata: Default::default(),
        prompt_display: None,
        context_recorder: None,
        selected_agent: None,
        prompt_prefix_metadata: None,
    }
}
