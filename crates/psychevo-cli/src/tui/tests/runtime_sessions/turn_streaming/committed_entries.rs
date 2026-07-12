#[test]
pub(crate) fn gateway_yielded_exec_entry_keeps_original_command_title() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);

    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_tool_entry(
            "live:turn-1:tool:call_exec",
            "runtime.stream",
            None,
            TranscriptBlockStatus::Running,
            "exec_command",
            Some(serde_json::json!({"cmd": "python fetch.py"})),
            Some(serde_json::json!({"session_id": 7, "exit_code": null, "output": "live\n"})),
        ),
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.tool_name.as_deref() == Some("exec_command"))
        .expect("exec row");
    assert_eq!(row.title, "exec_command python fetch.py");
    assert_eq!(row.kind, TranscriptKind::Ran);
    assert_eq!(row.text, "live\n");
}

#[test]
pub(crate) fn committed_turn_entries_replace_live_overlay_and_optimistic_prompt() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);
    ui.loaded_session_message_count = 2;

    let optimistic_start = ui.transcript.len();
    ui.push_user_with_images("$hackernews-daily".to_string(), &[]);
    ui.mark_optimistic_rows_from(optimistic_start);
    ui.bind_unbound_optimistic_rows_to_turn("turn-1");
    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_test_entry(
            "live:turn-1:reasoning:0",
            TranscriptBlockKind::Reasoning,
            TranscriptBlockStatus::Running,
            Some("Thinking"),
            "live thinking",
        ),
    );
    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_tool_entry(
            "live:turn-1:tool:call_exec",
            "runtime.stream",
            None,
            TranscriptBlockStatus::Running,
            "exec_command",
            Some(serde_json::json!({"cmd": "python fetch.py"})),
            Some(serde_json::json!({"session_id": 7, "exit_code": null, "output": "live\n"})),
        ),
    );
    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_test_entry(
            "live:turn-1:assistant:0",
            TranscriptBlockKind::Text,
            TranscriptBlockStatus::Running,
            None,
            "live answer",
        ),
    );

    app.apply_committed_turn_entries(
        &mut ui,
        Some("session-1"),
        "turn-1",
        vec![
            durable_text_entry(3, TranscriptEntryRole::User, "$hackernews-daily"),
            durable_assistant_entry(
                4,
                vec![
                    durable_block(
                        "message:4:block:0",
                        TranscriptBlockKind::Reasoning,
                        TranscriptBlockStatus::Completed,
                        Some("Thinking"),
                        Some("durable thinking"),
                        None,
                    ),
                    durable_tool_block(
                        "tool:call_exec",
                        TranscriptBlockStatus::Completed,
                        "exec_command",
                        serde_json::json!({"cmd": "python fetch.py"}),
                        serde_json::json!({"session_id": null, "exit_code": 0, "output": "done\n"}),
                    ),
                    durable_block(
                        "message:4:block:2",
                        TranscriptBlockKind::Text,
                        TranscriptBlockStatus::Completed,
                        None,
                        Some("durable answer"),
                        None,
                    ),
                ],
            ),
        ],
    );

    assert!(
        ui.transcript.iter().all(|row| !matches!(
            row.transcript_source.as_deref(),
            Some("runtime.stream" | "tui.optimistic")
        )),
        "{:?}",
        ui.transcript
    );
    assert_eq!(ui.loaded_session_message_count, 4);
    assert_eq!(
        ui.transcript
            .iter()
            .filter(|row| row.kind == TranscriptKind::Prompt && row.text == "$hackernews-daily")
            .count(),
        1,
        "{:?}",
        ui.transcript
    );
    assert_eq!(
        ui.transcript
            .iter()
            .filter(|row| row.kind == TranscriptKind::Thinking && row.text == "durable thinking")
            .count(),
        1,
        "{:?}",
        ui.transcript
    );
    assert!(
        ui.transcript
            .iter()
            .find(|row| row.kind == TranscriptKind::Thinking && row.text == "durable thinking")
            .is_some_and(|row| row.details_collapsed),
        "{:?}",
        ui.transcript
    );
    assert_eq!(
        ui.transcript
            .iter()
            .filter(|row| row.kind == TranscriptKind::Ran
                && row.tool_name.as_deref() == Some("exec_command"))
            .count(),
        1,
        "{:?}",
        ui.transcript
    );
    assert_eq!(
        ui.transcript
            .iter()
            .filter(|row| row.kind == TranscriptKind::Answer && row.text == "durable answer")
            .count(),
        1,
        "{:?}",
        ui.transcript
    );
}

#[tokio::test]
pub(crate) async fn committed_turn_entries_keep_turn_start_notice_before_answer_footer() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);

    let optimistic_start = ui.transcript.len();
    ui.push_user_with_images("$hackernews-daily".to_string(), &[]);
    ui.mark_optimistic_rows_from(optimistic_start);
    ui.bind_unbound_optimistic_rows_to_turn("turn-1");
    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_test_entry(
            "live:turn-1:reasoning:0",
            TranscriptBlockKind::Reasoning,
            TranscriptBlockStatus::Running,
            Some("Thinking"),
            "live thinking",
        ),
    );

    let (_tx, rx) = mpsc::unbounded_channel();
    let (control, _) = run_control();
    let task = tokio::spawn(async { std::future::pending().await });
    ui.running = Some(RunningTurn {
        session_id: Some("session-1".to_string()),
        control,
        selector: None,
        turn_id: Some("turn-1".to_string()),
        events: RunningTurnEvents::Gateway(rx),
        task: RunningTask::Agent(task),
    });
    app.apply_gateway_event(
        &mut ui,
        Some("session-1"),
        GatewayEvent::TurnStarted {
            thread_id: Some("session-1".to_string()),
            turn_id: "turn-1".to_string(),
            selected_skills: vec![psychevo_gateway::GatewaySelectedSkill {
                name: "hackernews-daily".to_string(),
                path: "/tmp/hackernews-daily/SKILL.md".to_string(),
            }],
        },
    );
    app.handle_fullscreen_command(&mut ui, SlashCommand::Status)
        .await
        .expect("status");
    if let Some(running) = ui.running.as_ref() {
        running.task.abort();
    }

    app.apply_committed_turn_entries(
        &mut ui,
        Some("session-1"),
        "turn-1",
        vec![
            durable_text_entry(1, TranscriptEntryRole::User, "$hackernews-daily"),
            durable_assistant_entry(
                2,
                vec![durable_block(
                    "message:2:block:0",
                    TranscriptBlockKind::Text,
                    TranscriptBlockStatus::Completed,
                    None,
                    Some("committed answer"),
                    Some(serde_json::json!({
                        "provider": "mock",
                        "model": "mock-model",
                        "finish_reason": "stop",
                        "outcome": "normal",
                        "metadata": {"elapsed_ms": 2_000}
                    })),
                )],
            ),
        ],
    );

    let prompt = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Prompt && row.text == "$hackernews-daily")
        .expect("committed prompt");
    let answer = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Answer && row.text == "committed answer")
        .expect("committed answer");
    let status = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Command && row.title == "/status")
        .expect("status command row");
    let skill_status = ui
        .transcript
        .iter()
        .position(|row| {
            row.kind == TranscriptKind::Status && row.text == "skill loaded: hackernews-daily"
        })
        .expect("skill status row");
    let footer = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Meta && row.text.contains("mock/mock-model"))
        .expect("turn footer");
    assert!(prompt < answer, "{:?}", ui.transcript);
    assert!(prompt < skill_status, "{:?}", ui.transcript);
    assert!(skill_status < answer, "{:?}", ui.transcript);
    assert!(answer < footer, "{:?}", ui.transcript);
    assert!(footer < status, "{:?}", ui.transcript);
    assert!(answer < status, "{:?}", ui.transcript);
}

#[test]
pub(crate) fn committed_turn_entries_remove_live_meta_without_removing_committed_footer() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);
    ui.loaded_session_message_count = 2;

    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        durable_assistant_entry(
            2,
            vec![durable_block(
                "message:2:block:0",
                TranscriptBlockKind::Text,
                TranscriptBlockStatus::Completed,
                None,
                Some("previous answer"),
                Some(serde_json::json!({
                    "provider": "mock",
                    "model": "mock-model",
                    "finish_reason": "stop",
                    "outcome": "normal",
                    "metadata": {"elapsed_ms": 2_000}
                })),
            )],
        ),
    );

    let optimistic_start = ui.transcript.len();
    ui.push_user_with_images("你有哪些技能".to_string(), &[]);
    ui.mark_optimistic_rows_from(optimistic_start);
    ui.bind_unbound_optimistic_rows_to_turn("turn-2");

    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        live_text_entry_with_turn(
            "live:turn-2:assistant",
            "turn-2",
            TranscriptBlockStatus::Completed,
            "live answer",
        ),
    );
    assert!(
        ui.transcript.iter().any(|row| {
            row.kind == TranscriptKind::Meta
                && row.text.contains("mock/mock-model")
                && row.transcript_source.as_deref() == Some("runtime.stream")
        }),
        "{:?}",
        ui.transcript
    );

    let mut committed_user = durable_text_entry(3, TranscriptEntryRole::User, "你有哪些技能");
    committed_user.turn_id = Some("turn-2".to_string());
    let mut committed_assistant = durable_assistant_entry(
        4,
        vec![durable_block(
            "message:4:block:0",
            TranscriptBlockKind::Text,
            TranscriptBlockStatus::Completed,
            None,
            Some("committed answer"),
            Some(serde_json::json!({
                "provider": "mock",
                "model": "mock-model",
                "finish_reason": "stop",
                "outcome": "normal",
                "metadata": {"elapsed_ms": 3_000}
            })),
        )],
    );
    committed_assistant.turn_id = Some("turn-2".to_string());

    app.apply_committed_turn_entries(
        &mut ui,
        Some("session-1"),
        "turn-2",
        vec![committed_user, committed_assistant],
    );

    assert!(
        ui.transcript.iter().all(|row| !matches!(
            row.transcript_source.as_deref(),
            Some("runtime.stream" | "tui.optimistic")
        )),
        "{:?}",
        ui.transcript
    );
    assert_eq!(
        ui.transcript
            .iter()
            .filter(|row| row.kind == TranscriptKind::Prompt && row.text == "你有哪些技能")
            .count(),
        1,
        "{:?}",
        ui.transcript
    );
    let committed_meta_entries = ui
        .transcript
        .iter()
        .filter(|row| {
            row.kind == TranscriptKind::Meta
                && row.transcript_source.as_deref() == Some("runtime.message")
        })
        .filter_map(|row| row.transcript_entry_id.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(
        committed_meta_entries,
        vec!["message:2", "message:4"],
        "{:?}",
        ui.transcript
    );
}

#[test]
pub(crate) fn committed_footer_consumes_turn_failures_before_completion_fallback() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);

    for index in 1..=2 {
        app.apply_gateway_transcript_entry(
            &mut ui,
            Some("session-1"),
            failed_live_tool_entry(
                &format!("live:turn-1:tool:call_write_{index}"),
                "turn-1",
                &format!("call_write_{index}"),
            ),
        );
    }
    assert_eq!(ui.turn_failures, 2);

    let mut committed_assistant = durable_assistant_entry(
        1,
        vec![durable_block(
            "message:1:block:0",
            TranscriptBlockKind::Text,
            TranscriptBlockStatus::Completed,
            None,
            Some("committed answer"),
            Some(serde_json::json!({
                "provider": "xiaomi-token-plan",
                "model": "mimo-v2.5-pro",
                "finish_reason": "stop",
                "outcome": "normal",
                "metadata": {
                    "elapsed_ms": 26_000,
                    "reasoning_effort": "high"
                }
            })),
        )],
    );
    committed_assistant.turn_id = Some("turn-1".to_string());

    app.apply_committed_turn_entries(
        &mut ui,
        Some("session-1"),
        "turn-1",
        vec![committed_assistant],
    );
    ui.update_turn_meta(false, true, true, true);

    let meta_rows = ui
        .transcript
        .iter()
        .filter(|row| row.kind == TranscriptKind::Meta)
        .collect::<Vec<_>>();
    assert_eq!(meta_rows.len(), 1, "{:?}", ui.transcript);
    let meta = meta_rows[0];
    assert!(
        meta.text.contains("xiaomi-token-plan/mimo-v2.5-pro high"),
        "{}",
        meta.text
    );
    assert!(meta.text.contains("26s"), "{}", meta.text);
    assert!(meta.text.contains("2 failures"), "{}", meta.text);
    assert_eq!(meta.transcript_source.as_deref(), Some("runtime.message"));
    assert_eq!(meta.transcript_entry_id.as_deref(), Some("message:1"));
    assert!(
        ui.transcript.iter().all(|row| {
            !(row.kind == TranscriptKind::Meta
                && row.transcript_source.as_deref() == Some("runtime.stream"))
        }),
        "{:?}",
        ui.transcript
    );
}

#[test]
pub(crate) fn committed_turn_entries_skip_already_loaded_message_sequences() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);
    ui.loaded_session_message_count = 2;

    app.apply_committed_turn_entries(
        &mut ui,
        Some("session-1"),
        "turn-1",
        vec![
            durable_text_entry(1, TranscriptEntryRole::User, "old prompt"),
            durable_text_entry(3, TranscriptEntryRole::User, "new prompt"),
        ],
    );

    assert!(
        ui.transcript.iter().all(|row| row.text != "old prompt"),
        "{:?}",
        ui.transcript
    );
    assert!(
        ui.transcript.iter().any(|row| row.text == "new prompt"),
        "{:?}",
        ui.transcript
    );
    assert_eq!(ui.loaded_session_message_count, 3);
}

#[test]
pub(crate) fn committed_reasoning_entry_uses_middle_fold_preview() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);
    let long = numbered_lines(1, 12);

    app.apply_committed_turn_entries(
        &mut ui,
        Some("session-1"),
        "turn-1",
        vec![durable_assistant_entry(
            1,
            vec![durable_block(
                "message:1:block:0",
                TranscriptBlockKind::Reasoning,
                TranscriptBlockStatus::Completed,
                Some("Thinking"),
                Some(&long),
                None,
            )],
        )],
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Thinking)
        .expect("thinking row");
    assert!(!row.expanded);
    assert_eq!(row.full_text.as_deref(), Some(long.as_str()));
    assert!(row.text.contains("line 1"), "{}", row.text);
    assert!(row.text.contains("line 2"), "{}", row.text);
    assert!(row.text.contains("... 6 more lines"), "{}", row.text);
    assert!(row.text.contains("line 9"), "{}", row.text);
    assert!(row.text.contains("line 12"), "{}", row.text);
    assert!(!row.text.contains("line 8"), "{}", row.text);
}

fn gateway_test_entry(
    id: &str,
    kind: TranscriptBlockKind,
    status: TranscriptBlockStatus,
    title: Option<&str>,
    text: &str,
) -> TranscriptEntry {
    TranscriptEntry {
        id: id.to_string(),
        thread_id: "session-1".to_string(),
        turn_id: Some("turn-1".to_string()),
        message_seq: None,
        role: TranscriptEntryRole::Assistant,
        status,
        source: "runtime.stream".to_string(),
        blocks: vec![TranscriptBlock {
            id: format!("{id}:block"),
            kind,
            status,
            order: 0,
            phase_ordinal: None,
            source: "runtime.stream".to_string(),
            title: title.map(str::to_string),
            preview: Some(text.to_string()),
            detail: Some(text.to_string()),
            body: Some(text.to_string()),
            artifact_ids: Vec::new(),
            metadata: if title == Some("Preamble") {
                Some(serde_json::json!({"projection": "assistant_preamble"}))
            } else {
                None
            },
            result: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        }],
        metadata: None,
        usage: None,
        accounting: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}

fn numbered_lines(start: usize, end: usize) -> String {
    (start..=end)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn gateway_tool_entry(
    id: &str,
    source: &str,
    message_seq: Option<i64>,
    status: TranscriptBlockStatus,
    tool_name: &str,
    args: Option<serde_json::Value>,
    result: Option<serde_json::Value>,
) -> TranscriptEntry {
    let mut metadata = serde_json::Map::new();
    metadata.insert("projection".to_string(), serde_json::json!("tool"));
    metadata.insert("tool_name".to_string(), serde_json::json!(tool_name));
    metadata.insert("tool_call_id".to_string(), serde_json::json!("call_exec"));
    if let Some(args) = args {
        metadata.insert("args".to_string(), args);
    }
    if let Some(result) = result {
        metadata.insert("result".to_string(), result);
    }
    metadata.insert("outcome".to_string(), serde_json::json!("normal"));
    TranscriptEntry {
        id: id.to_string(),
        thread_id: "session-1".to_string(),
        turn_id: Some("turn-1".to_string()),
        message_seq,
        role: TranscriptEntryRole::Assistant,
        status,
        source: source.to_string(),
        blocks: vec![TranscriptBlock {
            id: format!("{id}:block"),
            kind: TranscriptBlockKind::Shell,
            status,
            order: 0,
            phase_ordinal: None,
            source: source.to_string(),
            title: Some(tool_name.to_string()),
            preview: None,
            detail: None,
            body: None,
            artifact_ids: Vec::new(),
            metadata: Some(serde_json::Value::Object(metadata)),
            result: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        }],
        metadata: None,
        usage: None,
        accounting: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}

fn failed_live_tool_entry(id: &str, turn_id: &str, tool_call_id: &str) -> TranscriptEntry {
    let mut metadata = serde_json::Map::new();
    metadata.insert("projection".to_string(), serde_json::json!("tool"));
    metadata.insert("tool_name".to_string(), serde_json::json!("write"));
    metadata.insert("tool_call_id".to_string(), serde_json::json!(tool_call_id));
    metadata.insert(
        "args".to_string(),
        serde_json::json!({"path": format!("/tmp/{tool_call_id}.txt")}),
    );
    metadata.insert(
        "result".to_string(),
        serde_json::json!({"error": "denied by sandbox policy"}),
    );
    metadata.insert("outcome".to_string(), serde_json::json!("failed"));
    TranscriptEntry {
        id: id.to_string(),
        thread_id: "session-1".to_string(),
        turn_id: Some(turn_id.to_string()),
        message_seq: None,
        role: TranscriptEntryRole::Assistant,
        status: TranscriptBlockStatus::Failed,
        source: "runtime.stream".to_string(),
        blocks: vec![TranscriptBlock {
            id: format!("{id}:block"),
            kind: TranscriptBlockKind::Tool,
            status: TranscriptBlockStatus::Failed,
            order: 0,
            phase_ordinal: None,
            source: "runtime.stream".to_string(),
            title: Some("write".to_string()),
            preview: None,
            detail: None,
            body: None,
            artifact_ids: Vec::new(),
            metadata: Some(serde_json::Value::Object(metadata)),
            result: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        }],
        metadata: None,
        usage: None,
        accounting: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}

fn live_text_entry_with_turn(
    id: &str,
    turn_id: &str,
    status: TranscriptBlockStatus,
    text: &str,
) -> TranscriptEntry {
    TranscriptEntry {
        id: id.to_string(),
        thread_id: "session-1".to_string(),
        turn_id: Some(turn_id.to_string()),
        message_seq: None,
        role: TranscriptEntryRole::Assistant,
        status,
        source: "runtime.stream".to_string(),
        blocks: vec![TranscriptBlock {
            id: format!("{id}:block"),
            kind: TranscriptBlockKind::Text,
            status,
            order: 0,
            phase_ordinal: None,
            source: "runtime.stream".to_string(),
            title: None,
            preview: Some(text.to_string()),
            detail: Some(text.to_string()),
            body: Some(text.to_string()),
            artifact_ids: Vec::new(),
            metadata: Some(serde_json::json!({
                "provider": "mock",
                "model": "mock-model",
                "finish_reason": "stop",
                "outcome": "normal",
                "metadata": {"elapsed_ms": 1_000}
            })),
            result: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        }],
        metadata: None,
        usage: None,
        accounting: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}

fn durable_text_entry(seq: i64, role: TranscriptEntryRole, text: &str) -> TranscriptEntry {
    TranscriptEntry {
        id: format!("message:{seq}"),
        thread_id: "session-1".to_string(),
        turn_id: Some("turn-1".to_string()),
        message_seq: Some(seq),
        role,
        status: TranscriptBlockStatus::Completed,
        source: "runtime.message".to_string(),
        blocks: vec![durable_block(
            &format!("message:{seq}:block:0"),
            TranscriptBlockKind::Text,
            TranscriptBlockStatus::Completed,
            None,
            Some(text),
            None,
        )],
        metadata: None,
        usage: None,
        accounting: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}

fn durable_assistant_entry(seq: i64, blocks: Vec<TranscriptBlock>) -> TranscriptEntry {
    TranscriptEntry {
        id: format!("message:{seq}"),
        thread_id: "session-1".to_string(),
        turn_id: Some("turn-1".to_string()),
        message_seq: Some(seq),
        role: TranscriptEntryRole::Assistant,
        status: TranscriptBlockStatus::Completed,
        source: "runtime.message".to_string(),
        blocks,
        metadata: None,
        usage: None,
        accounting: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}

fn durable_tool_block(
    id: &str,
    status: TranscriptBlockStatus,
    tool_name: &str,
    args: serde_json::Value,
    result: serde_json::Value,
) -> TranscriptBlock {
    let mut metadata = serde_json::Map::new();
    metadata.insert("projection".to_string(), serde_json::json!("tool"));
    metadata.insert("tool_name".to_string(), serde_json::json!(tool_name));
    metadata.insert("tool_call_id".to_string(), serde_json::json!("call_exec"));
    metadata.insert("args".to_string(), args);
    metadata.insert("result".to_string(), result);
    metadata.insert("outcome".to_string(), serde_json::json!("normal"));
    durable_block(
        id,
        TranscriptBlockKind::Shell,
        status,
        Some(tool_name),
        None,
        Some(serde_json::Value::Object(metadata)),
    )
}

fn durable_block(
    id: &str,
    kind: TranscriptBlockKind,
    status: TranscriptBlockStatus,
    title: Option<&str>,
    body: Option<&str>,
    metadata: Option<serde_json::Value>,
) -> TranscriptBlock {
    TranscriptBlock {
        id: id.to_string(),
        kind,
        status,
        order: 0,
        phase_ordinal: None,
        source: "runtime.message".to_string(),
        title: title.map(str::to_string),
        preview: body.map(str::to_string),
        detail: body.map(str::to_string),
        body: body.map(str::to_string),
        artifact_ids: Vec::new(),
        metadata,
        result: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}
