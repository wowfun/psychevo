#[tokio::test]
async fn persistence_sink_streams_elapsed_metadata_for_assistant_message_end() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "tui", "model", "provider", None)
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
        started,
        tool_elapsed_ms: Arc::new(Mutex::new(BTreeMap::new())),
        control: SmokeControl::None,
        control_handle: None,
        events: None,
        stream_events: Some(stream),
        include_reasoning: false,
        reasoning_effort: None,
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
async fn persistence_sink_persists_assistant_reasoning_effort_metadata() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "tui", "model", "provider", None)
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
        started: Instant::now(),
        tool_elapsed_ms: Arc::new(Mutex::new(BTreeMap::new())),
        control: SmokeControl::None,
        control_handle: None,
        events: None,
        stream_events: Some(stream),
        include_reasoning: false,
        reasoning_effort: Some("high".to_string()),
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
async fn persistence_sink_persists_tool_elapsed_metadata() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "tui", "model", "provider", None)
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
        started: Instant::now(),
        tool_elapsed_ms: Arc::new(Mutex::new(BTreeMap::new())),
        control: SmokeControl::None,
        control_handle: None,
        events: None,
        stream_events: Some(stream),
        include_reasoning: false,
        reasoning_effort: None,
    };

    sink.emit(AgentEvent::ToolExecutionEnd {
        tool_call_id: "call_read".to_string(),
        tool_name: "read".to_string(),
        result: json!({"content":"done"}),
        outcome: Outcome::Normal,
        elapsed_ms: 321,
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

#[test]
fn json_projection_hides_reasoning_unless_included() {
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
    let metrics = project_run_stream_event(&event).expect("metrics");
    match metrics {
        RunStreamEvent::Event(value) => {
            assert_eq!(value["usage"]["total_tokens"], 2);
            assert_eq!(value["metadata"]["provider_response_id"], "resp");
            assert!(!serde_json::to_string(&value).unwrap().contains("private"));
        }
        other => panic!("unexpected stream event: {other:?}"),
    }
}
