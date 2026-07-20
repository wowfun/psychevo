#[tokio::test]
pub(crate) async fn pending_write_tool_input_defers_later_completion_events() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(RunStreamEvent::value(serde_json::json!({
        "type": "message_update",
        "message": {
            "role": "assistant",
            "content": [{
                "type": "text",
                "text": "Now I have all the data needed. Let me write the complete report."
            }],
            "timestamp_ms": 2,
            "outcome": "normal"
        }
    })))
    .expect("send text");
    tx.send(RunStreamEvent::value(serde_json::json!({
        "type": "tool_call_pending",
        "tool_call_id": "call_write_report",
        "tool_name": "write",
        "arguments_json": "",
        "content_index": 1,
        "call_index": 0
    })))
    .expect("send pending");
    tx.send(RunStreamEvent::value(serde_json::json!({
        "type": "tool_execution_start",
        "tool_call_id": "call_write_report",
        "tool_name": "write",
        "args": {
            "path": "/tmp/hackernews-hot-05-39.md",
            "content": "report body"
        }
    })))
    .expect("send start");
    tx.send(RunStreamEvent::value(serde_json::json!({
        "type": "tool_execution_end",
        "tool_call_id": "call_write_report",
        "tool_name": "write",
        "result": {
            "path": "feeds/2026-05-10/hackernews-hot-05-39.md",
            "bytes_written": 24968,
            "error": null
        },
        "outcome": "normal",
        "elapsed_ms": 0
    })))
    .expect("send end");
    drop(tx);

    let result = finished_run_result(&app);
    let task = tokio::spawn(async move { Ok(result) });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        session_id: None,
        control,
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::Runtime(rx),
        task: RunningTask::Agent(task),
    });
    while !ui.running.as_ref().expect("running").task.is_finished() {
        tokio::task::yield_now().await;
    }

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("first drain");
    assert!(ui.transcript.iter().any(|row| row.title == "write"));
    assert_eq!(ui.deferred_stream_events.len(), 2);
    assert!(ui.running.is_some());

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("second drain");
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.title == "write /tmp/hackernews-hot-05-39.md")
    );
    assert_eq!(ui.deferred_stream_events.len(), 1);

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("third drain");
    assert!(ui.running.is_none());
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.title == "write feeds/2026-05-10/hackernews-hot-05-39.md")
    );
}

#[test]
pub(crate) fn fullscreen_write_preview_opens_once_and_preserves_manual_collapse() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "write",
            "arguments_json": r#"{"path":"report.md","content":"first"#,
            "content_index": 0,
            "call_index": 0
        }),
        false,
    );
    let idx = ui
        .transcript
        .iter()
        .position(|row| row.tool_name.as_deref() == Some("write"))
        .expect("write row");
    let row = &ui.transcript[idx];
    assert_eq!(row.title, "write report.md");
    assert_eq!(row.write_preview_phase.as_deref(), Some("generating"));
    assert!(row.expandable_text().contains("Generating · 5 bytes · 1 line"));
    assert!(row.expandable_text().contains("first"));
    assert!(!row.details_collapsed);

    toggle_transcript_row_details(&mut ui.transcript[idx]);
    assert!(ui.transcript[idx].details_collapsed);
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_call",
                    "id": "call-write",
                    "name": "write",
                    "arguments": {"path": "report.md", "content": "first second"},
                    "arguments_json": "{\"path\":\"report.md\",\"content\":\"first second\"}",
                    "content_index": 0,
                    "call_index": 0
                }]
            }
        }),
        false,
    );

    let row = &ui.transcript[idx];
    assert!(row.details_collapsed);
    assert!(row.expandable_text().contains("first second"));
}

#[test]
pub(crate) fn fullscreen_write_preview_transitions_to_writing_and_collapses_once_on_success() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let pending = serde_json::json!({
        "type": "tool_call_pending",
        "tool_call_id": "call-write",
        "tool_name": "write",
        "arguments_json": r#"{"path":"report.md","content":"complete body"#,
        "content_index": 0,
        "call_index": 0
    });
    ui.apply_value_event(&pending, false);
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "call-write",
            "tool_name": "write",
            "args": {"path": "report.md", "content": "complete body"}
        }),
        false,
    );
    let idx = *ui
        .tool_rows
        .get(&tool_id_key("call-write"))
        .expect("write row");
    assert_eq!(
        ui.transcript[idx].write_preview_phase.as_deref(),
        Some("writing")
    );
    assert!(ui.transcript[idx].expandable_text().contains("Writing"));

    let completed = serde_json::json!({
        "type": "tool_execution_end",
        "tool_call_id": "call-write",
        "tool_name": "write",
        "args": {"path": "report.md", "content": "complete body"},
        "result": {"path": "report.md", "bytes_written": 13},
        "outcome": "normal"
    });
    ui.apply_value_event(&completed, false);
    assert!(ui.transcript[idx].write_argument_preview.is_none());
    assert!(ui.transcript[idx].details_collapsed);
    assert!(!ui.transcript[idx].expandable_text().contains("complete body"));

    toggle_transcript_row_details(&mut ui.transcript[idx]);
    assert!(!ui.transcript[idx].details_collapsed);
    ui.apply_value_event(&completed, false);
    assert!(!ui.transcript[idx].details_collapsed);

    for late in [
        serde_json::json!({
            "type": "tool_call_pending",
            "tool_call_id": "call-write",
            "tool_name": "write",
            "arguments_json": r#"{"path":"report.md","content":"late preview"#,
            "content_index": 0,
            "call_index": 0
        }),
        serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "call-write",
            "tool_name": "write",
            "args": {"path": "report.md", "content": "late preview"}
        }),
    ] {
        ui.apply_value_event(&late, false);
        assert!(ui.transcript[idx].write_argument_preview.is_none());
        assert!(ui.transcript[idx].tool_started.is_none());
        assert!(!ui.transcript[idx].expandable_text().contains("late preview"));
    }
}

#[test]
pub(crate) fn fullscreen_failed_write_retains_preview_and_failure_reason() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_call_id": "call-write",
            "tool_name": "write",
            "arguments_json": r#"{"content":"unfinished body","path":"report.md"}"#,
            "content_index": 0,
            "call_index": 0
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call-write",
            "tool_name": "write",
            "args": {"path": "report.md", "content": "unfinished body"},
            "result": {"error": "permission denied"},
            "outcome": "failed"
        }),
        false,
    );

    let idx = *ui
        .tool_rows
        .get(&tool_id_key("call-write"))
        .expect("write row");
    let row = &ui.transcript[idx];
    assert!(row.failed);
    assert!(!row.details_collapsed);
    assert_eq!(row.write_preview_phase.as_deref(), Some("failed"));
    assert!(row.expandable_text().contains("unfinished body"));
    assert!(row.expandable_text().contains("permission denied"));
}

#[test]
pub(crate) fn fullscreen_consumes_gateway_write_preview_metadata() {
    fn entry(status: TranscriptBlockStatus, phase: &str, text: &str, body: &str) -> TranscriptEntry {
        TranscriptEntry {
            id: "live:turn-write:assistant:0".to_string(),
            thread_id: "session-1".to_string(),
            turn_id: Some("turn-write".to_string()),
            message_seq: None,
            role: TranscriptEntryRole::Assistant,
            status,
            source: "runtime.stream".to_string(),
            blocks: vec![TranscriptBlock {
                id: "live:turn-write:tool:call-write".to_string(),
                kind: TranscriptBlockKind::File,
                status,
                order: 0,
                phase_ordinal: None,
                source: "runtime.stream".to_string(),
                title: Some("write report.md".to_string()),
                body: (!body.is_empty()).then(|| body.to_string()),
                preview: None,
                detail: None,
                artifact_ids: Vec::new(),
                metadata: Some(serde_json::json!({
                    "projection": "tool",
                    "tool_name": "write",
                    "tool_call_id": "call-write",
                    "outcome": if phase == "failed" { "failed" } else { "normal" },
                    "result": {"error": body},
                    "write_argument_preview": {
                        "phase": phase,
                        "path": "report.md",
                        "text": text,
                        "bytes_seen": text.len(),
                        "lines_seen": 1,
                        "omitted_bytes": 0,
                        "truncated": false
                    }
                })),
                result: None,
                created_at_ms: 1,
                updated_at_ms: 2,
            }],
            metadata: None,
            usage: None,
            accounting: None,
            created_at_ms: 1,
            updated_at_ms: 2,
        }
    }

    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);
    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        entry(
            TranscriptBlockStatus::Running,
            "generating",
            "unfinished body",
            "",
        ),
    );
    let idx = ui
        .transcript
        .iter()
        .position(|row| row.tool_call_id.as_deref() == Some("call-write"))
        .expect("write row");
    assert!(!ui.transcript[idx].details_collapsed);
    assert!(
        ui.transcript[idx].expandable_text().contains("unfinished body"),
        "{:#?}",
        ui.transcript[idx]
    );

    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        entry(
            TranscriptBlockStatus::Failed,
            "failed",
            "unfinished body",
            "permission denied",
        ),
    );
    let row = &ui.transcript[idx];
    assert!(row.failed);
    assert_eq!(row.write_preview_phase.as_deref(), Some("failed"));
    assert!(row.expandable_text().contains("unfinished body"));
    assert!(row.expandable_text().contains("permission denied"));
}

#[tokio::test]
pub(crate) async fn typed_gateway_final_answer_restores_turn_meta_after_task_completion() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("typed-session".to_string());
    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(GatewayEvent::TurnStarted {
        thread_id: Some("typed-session".to_string()),
        turn_id: "turn-1".to_string(),
        selected_skills: Vec::new(),
    })
    .expect("send turn start");
    tx.send(GatewayEvent::EntryCompleted {
        turn_id: "turn-1".to_string(),
        entry: TranscriptEntry {
            id: "live:turn-1:assistant".to_string(),
            thread_id: "typed-session".to_string(),
            turn_id: Some("turn-1".to_string()),
            message_seq: None,
            role: TranscriptEntryRole::Assistant,
            status: TranscriptBlockStatus::Completed,
            source: "runtime.stream".to_string(),
            blocks: vec![TranscriptBlock {
                id: "live:turn-1:assistant:text".to_string(),
                kind: TranscriptBlockKind::Text,
                status: TranscriptBlockStatus::Completed,
                order: 0,
                phase_ordinal: None,
                source: "runtime.stream".to_string(),
                title: None,
                body: Some("All done.".to_string()),
                preview: Some("All done.".to_string()),
                detail: Some("All done.".to_string()),
                artifact_ids: Vec::new(),
                metadata: Some(serde_json::json!({
                    "provider": "mock",
                    "model": "mock-model",
                    "finish_reason": "stop",
                    "outcome": "normal",
                    "metadata": {"elapsed_ms": 2_000},
                    "usage": {"input_tokens": 12},
                    "accounting": {"estimated_cost_nanodollars": 10}
                })),
                result: None,
                created_at_ms: 1,
                updated_at_ms: 2,
            }],
            metadata: None,
            usage: None,
            accounting: None,
            created_at_ms: 1,
            updated_at_ms: 2,
        },
    })
    .expect("send answer");
    tx.send(GatewayEvent::TurnCompleted {
        thread_id: Some("typed-session".to_string()),
        turn_id: "turn-1".to_string(),
        turn: GatewayTurn {
            id: "turn-1".to_string(),
            thread_id: Some("typed-session".to_string()),
            status: GatewayTurnStatus::Completed,
            outcome: Some("normal".to_string()),
            error: None,
            started_at_ms: None,
            completed_at_ms: Some(2),
        },
        committed_entries: Vec::new(),
    })
    .expect("send turn complete");
    drop(tx);

    let task = tokio::spawn(async move {
        Ok(psychevo_runtime::RunResult {
            session_id: "typed-session".to_string(),
            outcome: Outcome::Normal,
            terminal_reason: None,
            final_answer: "All done.".to_string(),
            db_path: temp.path().join("state.db"),
            cwd: temp.path().to_path_buf(),
            provider: "mock".to_string(),
            model: "mock-model".to_string(),
            base_url: "http://127.0.0.1".to_string(),
            api_key_env: None,
            reasoning_effort: None,
            context_limit: None,
            tool_failures: 0,
            selected_agent: None,
            selected_skills: Vec::new(),
            context_snapshot: None,
            terminal_error: None,
            events: Vec::new(),
            warnings: Vec::new(),
        })
    });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        session_id: None,
        control,
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::Gateway(rx),
        task: RunningTask::Agent(task),
    });
    while !ui.running.as_ref().expect("running").task.is_finished() {
        tokio::task::yield_now().await;
    }

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("drain typed gateway turn");

    assert!(ui.running.is_none());
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer && row.text == "All done.")
    );
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Meta
            && row.text.contains("mock/mock-model")
            && row.text.contains("2s")
    }));
}

#[tokio::test]
pub(crate) async fn opening_child_replays_and_continues_its_gateway_stream_without_thread_leaks() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.cwd, "tui", "mock-model", "mock", None)
        .expect("parent session");
    let child = store
        .create_child_session_with_metadata(&parent, &app.cwd, "agent", "mock-model", "mock", None)
        .expect("child session");
    store
        .upsert_agent_edge(
            &parent,
            &child,
            psychevo_runtime::AgentEdgeStatus::Open,
            Some(serde_json::json!({
                "agent": {
                    "id": "agent-run-1",
                    "name": "opencode",
                    "task": "stream the delegated task"
                }
            })),
        )
        .expect("agent edge");
    app.current_session = Some(parent.clone());
    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(GatewayEvent::EntryUpdated {
        turn_id: "child-turn".to_string(),
        entry: gateway_text_entry_for_thread(
            &child,
            "child-thinking",
            TranscriptBlockKind::Reasoning,
            "child thinking before open",
        ),
    })
    .expect("send child thinking");
    tx.send(GatewayEvent::EntryUpdated {
        turn_id: "child-turn".to_string(),
        entry: gateway_text_entry_for_thread(
            &child,
            "child-answer",
            TranscriptBlockKind::Text,
            "child answer before open",
        ),
    })
    .expect("send child answer");
    tx.send(GatewayEvent::EntryUpdated {
        turn_id: "parent-turn".to_string(),
        entry: gateway_text_entry_for_thread(
            &parent,
            "parent-answer",
            TranscriptBlockKind::Text,
            "parent only",
        ),
    })
    .expect("send parent answer");
    tx.send(GatewayEvent::EntryUpdated {
        turn_id: "other-turn".to_string(),
        entry: gateway_text_entry_for_thread(
            "other-thread",
            "other-answer",
            TranscriptBlockKind::Text,
            "other only",
        ),
    })
    .expect("send other answer");

    let result = psychevo_runtime::RunResult {
        session_id: parent.clone(),
        ..finished_run_result(&app)
    };
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _ = done_rx.await;
        Ok(result)
    });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        session_id: Some(parent.clone()),
        control,
        selector: None,
        turn_id: Some("parent-turn".to_string()),
        events: RunningTurnEvents::Gateway(rx),
        task: RunningTask::Agent(task),
    });

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("drain while parent is visible");
    assert!(
        ui.transcript.iter().any(|row| row.text == "parent only"),
        "{:?}",
        ui.transcript
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| !row.text.contains("child ") && row.text != "other only"),
        "{:?}",
        ui.transcript
    );

    app.open_agent_target_session(&mut ui, &child)
        .expect("open child session");

    assert_eq!(app.current_session.as_deref(), Some(child.as_str()));
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Thinking && row.text == "child thinking before open"
    }));
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Answer && row.text == "child answer before open"
    }));
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.text != "parent only" && row.text != "other only"),
        "{:?}",
        ui.transcript
    );

    tx.send(GatewayEvent::EntryUpdated {
        turn_id: "parent-turn".to_string(),
        entry: gateway_text_entry_for_thread(
            &parent,
            "parent-answer-later",
            TranscriptBlockKind::Text,
            "parent later",
        ),
    })
    .expect("send later parent answer");
    tx.send(GatewayEvent::EntryUpdated {
        turn_id: "other-turn".to_string(),
        entry: gateway_text_entry_for_thread(
            "other-thread",
            "other-answer-later",
            TranscriptBlockKind::Text,
            "other later",
        ),
    })
    .expect("send later other answer");
    tx.send(GatewayEvent::EntryUpdated {
        turn_id: "child-turn".to_string(),
        entry: gateway_text_entry_for_thread(
            &child,
            "child-answer",
            TranscriptBlockKind::Text,
            "child answer after open",
        ),
    })
    .expect("send later child answer");

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("drain while child is visible");

    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Answer && row.text == "child answer after open"
    }));
    assert!(
        ui.transcript.iter().all(|row| {
            row.text != "parent only"
                && row.text != "parent later"
                && row.text != "other only"
                && row.text != "other later"
        }),
        "{:?}",
        ui.transcript
    );

    app.open_session_direct(&mut ui, &parent)
        .expect("return to parent session");
    assert!(
        ui.transcript.iter().any(|row| row.text == "parent later"),
        "{:?}",
        ui.transcript
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| !row.text.contains("child ") && row.text != "other later"),
        "{:?}",
        ui.transcript
    );

    app.open_agent_target_session(&mut ui, &child)
        .expect("reopen child session");
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Answer && row.text == "child answer after open"
    }), "{:?}", ui.transcript);
    assert!(
        ui.transcript.iter().all(|row| {
            row.text != "parent only"
                && row.text != "parent later"
                && row.text != "other only"
                && row.text != "other later"
        }),
        "{:?}",
        ui.transcript
    );

    app.open_session_direct(&mut ui, &parent)
        .expect("return to parent before child completion");
    app.apply_gateway_event(
        &mut ui,
        Some(&parent),
        GatewayEvent::TurnCompleted {
            thread_id: Some(child.clone()),
            turn_id: "child-turn".to_string(),
            turn: GatewayTurn {
                id: "child-turn".to_string(),
                thread_id: Some(child.clone()),
                status: GatewayTurnStatus::Completed,
                outcome: Some("normal".to_string()),
                error: None,
                started_at_ms: None,
                completed_at_ms: Some(3),
            },
            committed_entries: Vec::new(),
        },
    );
    assert!(
        !ui.session_live_event_backlog.contains_key(&child),
        "completed child backlog should be cleared: {:?}",
        ui.session_live_event_backlog.keys().collect::<Vec<_>>()
    );

    let _ = done_tx.send(());
}

#[test]
pub(crate) fn stale_gateway_running_exec_does_not_reactivate_completed_exec_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("typed-session".to_string());
    let mut ui = FullscreenUi::new(&app);

    let completed = gateway_exec_entry_for_test(
        "typed-session",
        "turn-1",
        "live:turn-1:tool:call_exec",
        TranscriptBlockStatus::Completed,
        serde_json::json!({
            "session_id": 7,
            "exit_code": 0,
            "output": "done\n"
        }),
    );
    let _ = app.apply_gateway_event(
        &mut ui,
        Some("typed-session"),
        GatewayEvent::EntryUpdated {
            turn_id: "turn-1".to_string(),
            entry: completed,
        },
    );
    let completed_rows = ui
        .transcript
        .iter()
        .filter(|row| row.tool_call_id.as_deref() == Some("call_exec"))
        .collect::<Vec<_>>();
    assert_eq!(completed_rows.len(), 1, "{:?}", ui.transcript);
    assert!(completed_rows[0].tool_started.is_none());

    let stale_running = gateway_exec_entry_for_test(
        "typed-session",
        "turn-1",
        "live:turn-1:tool:call_exec:stale",
        TranscriptBlockStatus::Running,
        serde_json::json!({
            "session_id": 7,
            "exit_code": null,
            "output": "still running\n"
        }),
    );
    assert!(!app.apply_gateway_event(
        &mut ui,
        Some("typed-session"),
        GatewayEvent::EntryUpdated {
            turn_id: "turn-1".to_string(),
            entry: stale_running,
        },
    ));

    let rows = ui
        .transcript
        .iter()
        .filter(|row| row.tool_call_id.as_deref() == Some("call_exec"))
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1, "{:?}", ui.transcript);
    assert!(rows[0].tool_started.is_none(), "{:?}", rows[0]);
    assert!(!ui.tool_rows.contains_key(&tool_id_key("call_exec")));
    assert!(!ui.exec_session_rows.contains_key(&7));
}

#[test]
pub(crate) fn multi_message_turn_preserves_answer_rows_across_tool_cycles() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "run_start",
            "provider": "mock",
            "model": "mock-model",
            "mode": "default"
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "First visible answer before tools."}],
                "timestamp_ms": 1,
                "finish_reason": "tool_calls",
                "outcome": "normal"
            }
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "call_read_fixture",
            "tool_name": "read",
            "args": {"path": "fixture.txt"}
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_read_fixture",
            "tool_name": "read",
            "args": {"path": "fixture.txt"},
            "result": {"path": "fixture.txt", "content": "fixture content"},
            "outcome": "normal"
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "Second visible answer after tools."}],
                "timestamp_ms": 2,
                "finish_reason": "stop",
                "outcome": "normal"
            }
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "Second visible answer after tools."}],
                "timestamp_ms": 2,
                "finish_reason": "stop",
                "outcome": "normal"
            }
        }),
        false,
    );

    let answers = ui
        .transcript
        .iter()
        .filter(|row| row.kind == TranscriptKind::Answer)
        .map(|row| row.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(answers, vec!["Second visible answer after tools."]);
    let first_preamble = ui
        .transcript
        .iter()
        .position(|row| {
            row.kind == TranscriptKind::Thinking
                && row.title == "Thinking"
                && row.text == "First visible answer before tools."
        })
        .expect("first preamble");
    let tool = ui
        .transcript
        .iter()
        .position(|row| row.title == "read fixture.txt")
        .expect("tool row");
    let second_answer = ui
        .transcript
        .iter()
        .position(|row| row.text == "Second visible answer after tools.")
        .expect("second answer");
    assert!(first_preamble < tool);
    assert!(tool < second_answer);
}

#[tokio::test]
pub(crate) async fn fullscreen_agent_end_releases_turn_before_auxiliary_task_finishes() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(RunStreamEvent::value(serde_json::json!({
        "type": "run_start",
        "session_id": "streamed-session",
        "provider": "mock",
        "model": "mock-model",
        "mode": "default"
    })))
    .expect("send run start");
    tx.send(RunStreamEvent::value(serde_json::json!({
        "type": "message_end",
        "message": {
            "role": "assistant",
            "content": [{"type": "text", "text": "hi"}],
            "timestamp_ms": 1,
            "finish_reason": "stop",
            "outcome": "normal",
            "provider": "mock",
            "model": "mock-model"
        }
    })))
    .expect("send answer");
    tx.send(RunStreamEvent::value(serde_json::json!({
        "type": "agent_end",
        "outcome": "normal",
        "messages": []
    })))
    .expect("send agent end");

    let result = psychevo_runtime::RunResult {
        session_id: "streamed-session".to_string(),
        outcome: Outcome::Normal,
        terminal_reason: None,
        final_answer: "hi".to_string(),
        db_path: app.db_path.clone(),
        cwd: app.cwd.clone(),
        provider: "mock".to_string(),
        model: "mock-model".to_string(),
        base_url: "http://127.0.0.1".to_string(),
        api_key_env: Some("TEST_PROVIDER_KEY".to_string()),
        reasoning_effort: None,
        context_limit: None,
        tool_failures: 0,
        selected_agent: None,
        selected_skills: Vec::new(),
        context_snapshot: None,
        terminal_error: None,
        events: Vec::new(),
        warnings: Vec::new(),
    };
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _ = done_rx.await;
        Ok(result)
    });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        session_id: None,
        control,
        selector: None,
        turn_id: None,
        events: RunningTurnEvents::Runtime(rx),
        task: RunningTask::Agent(task),
    });

    app.drain_fullscreen_events(&mut ui).await.expect("drain");

    assert!(ui.running.is_none());
    assert_eq!(app.current_session.as_deref(), Some("streamed-session"));
    assert_eq!(ui.auxiliary_agent_tasks.len(), 1);
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer && row.text == "hi")
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.text != "a turn is already running")
    );
    ui.running_elapsed_override = Some(Duration::from_secs(15));
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 12);
    let text = buffer_text(&buffer);
    assert!(!text.contains("15s · Esc"), "{text}");
    assert!(!text.contains("Esc"), "{text}");
    let _ = done_tx.send(());
}

fn gateway_exec_entry_for_test(
    session_id: &str,
    turn_id: &str,
    block_id: &str,
    status: TranscriptBlockStatus,
    result: serde_json::Value,
) -> TranscriptEntry {
    TranscriptEntry {
        id: format!("entry-{block_id}"),
        thread_id: session_id.to_string(),
        turn_id: Some(turn_id.to_string()),
        message_seq: None,
        role: TranscriptEntryRole::Assistant,
        status,
        source: "runtime.stream".to_string(),
        blocks: vec![TranscriptBlock {
            id: block_id.to_string(),
            kind: TranscriptBlockKind::Shell,
            status,
            order: 0,
            phase_ordinal: None,
            source: "runtime.stream".to_string(),
            title: Some("exec_command python fetch.py".to_string()),
            body: result
                .get("output")
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string),
            preview: None,
            detail: None,
            artifact_ids: Vec::new(),
            metadata: Some(serde_json::json!({
                "projection": "tool",
                "tool_name": "exec_command",
                "tool_call_id": "call_exec",
                "args": {"cmd": "python fetch.py"},
                "result": result,
            })),
            result: None,
            created_at_ms: 1,
            updated_at_ms: 2,
        }],
        metadata: None,
        usage: None,
        accounting: None,
        created_at_ms: 1,
        updated_at_ms: 2,
    }
}

fn gateway_text_entry_for_thread(
    thread_id: &str,
    id: &str,
    kind: TranscriptBlockKind,
    text: &str,
) -> TranscriptEntry {
    let mut entry = gateway_test_entry(
        id,
        kind,
        TranscriptBlockStatus::Running,
        None,
        text,
    );
    entry.thread_id = thread_id.to_string();
    entry.turn_id = Some(format!("{thread_id}-turn"));
    entry
}

#[tokio::test]
pub(crate) async fn visible_live_auxiliary_turn_defers_terminal_message_meta() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let session_id = "visible-live-session".to_string();
    app.current_session = Some(session_id.clone());
    let mut ui = FullscreenUi::new(&app);
    attach_background_agent_running(&mut ui, &session_id);
    ui.running_elapsed_override = Some(Duration::from_secs(11));

    ui.start_assistant();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "run_start",
            "session_id": session_id,
            "provider": "xiaomi-token-plan",
            "model": "mimo-v2-omni",
            "mode": "default"
        }),
        false,
    );
    assert!(
        ui.status_running_elapsed(app.current_session.as_deref())
            .is_some()
    );

    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "final answer has arrived"}],
                "timestamp_ms": 2,
                "finish_reason": "stop",
                "outcome": "normal",
                "model": "mimo-v2-omni",
                "provider": "xiaomi-token-plan"
            },
            "metadata": {
                "elapsed_ms": 171_000,
                "reasoning_effort": "high"
            }
        }),
        false,
    );

    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer && row.text == "final answer has arrived")
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta),
        "{:?}",
        ui.transcript
    );

    for agent in &ui.auxiliary_agent_tasks {
        agent.task.abort();
    }
}

#[tokio::test]
pub(crate) async fn live_session_history_reload_defers_latest_terminal_meta() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(
            &app.cwd,
            "tui",
            "mimo-v2-omni",
            "xiaomi-token-plan",
            None,
        )
        .expect("session");
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        1,
        "user",
        "prompt",
        serde_json::json!({
            "role": "user",
            "content": [{"text": "prompt"}],
            "timestamp_ms": 1
        }),
        None,
    );
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        2,
        "assistant",
        "final answer has arrived",
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "final answer has arrived"}],
            "timestamp_ms": 2,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mimo-v2-omni",
            "provider": "xiaomi-token-plan"
        }),
        Some(serde_json::json!({
            "elapsed_ms": 171_000,
            "reasoning_effort": "high"
        })),
    );
    app.current_session = Some(session_id.clone());
    let mut ui = FullscreenUi::new(&app);
    attach_background_agent_running(&mut ui, &session_id);

    app.load_current_session_history(&mut ui)
        .expect("load history");

    assert!(
        ui.status_running_elapsed(app.current_session.as_deref())
            .is_some()
    );
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer && row.text == "final answer has arrived")
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Meta),
        "{:?}",
        ui.transcript
    );

    for agent in &ui.auxiliary_agent_tasks {
        agent.task.abort();
    }
}

#[test]
pub(crate) fn typed_gateway_preamble_completion_updates_existing_answer_row_by_item_id() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);

    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_test_entry(
            "live:turn-1:assistant:0",
            TranscriptBlockKind::Text,
            TranscriptBlockStatus::Running,
            None,
            "好的，开始执行 X 日报流程。先运行 fetch.py 抓取数据。",
        ),
    );
    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_test_entry(
            "live:turn-1:assistant:0",
            TranscriptBlockKind::Reasoning,
            TranscriptBlockStatus::Completed,
            Some("Preamble"),
            "好的，开始执行 X 日报流程。先运行 fetch.py 抓取数据。",
        ),
    );

    assert_eq!(
        ui.transcript
            .iter()
            .filter(|row| row.kind == TranscriptKind::Answer)
            .count(),
        0,
        "{:?}",
        ui.transcript
    );
    let thinking_rows = ui
        .transcript
        .iter()
        .filter(|row| row.kind == TranscriptKind::Thinking)
        .collect::<Vec<_>>();
    assert_eq!(thinking_rows.len(), 1, "{:?}", ui.transcript);
    assert_eq!(thinking_rows[0].title, "Thinking");
    assert_eq!(
        thinking_rows[0].text,
        "好的，开始执行 X 日报流程。先运行 fetch.py 抓取数据。"
    );
}

#[test]
pub(crate) fn typed_gateway_reasoning_update_uses_middle_fold_preview() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);
    let long = numbered_lines(1, 12);

    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_test_entry(
            "live:turn-1:reasoning:0",
            TranscriptBlockKind::Reasoning,
            TranscriptBlockStatus::Running,
            Some("Thinking"),
            &long,
        ),
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Thinking)
        .expect("thinking row");
    assert!(!row.expanded);
    assert!(!row.details_collapsed);
    assert_eq!(row.full_text.as_deref(), Some(long.as_str()));
    assert!(row.text.contains("line 1"), "{}", row.text);
    assert!(row.text.contains("line 2"), "{}", row.text);
    assert!(row.text.contains("... 6 more lines"), "{}", row.text);
    assert!(row.text.contains("line 9"), "{}", row.text);
    assert!(row.text.contains("line 12"), "{}", row.text);
    assert!(!row.text.contains("line 8"), "{}", row.text);
}

#[test]
pub(crate) fn typed_gateway_reasoning_collapses_once_for_every_terminal_status() {
    for status in [
        TranscriptBlockStatus::Completed,
        TranscriptBlockStatus::Failed,
        TranscriptBlockStatus::Cancelled,
    ] {
        let temp = tempdir().expect("temp");
        let mut app = test_app(&temp);
        app.current_session = Some("session-1".to_string());
        let mut ui = FullscreenUi::new(&app);

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
        let idx = ui
            .transcript
            .iter()
            .position(|row| row.kind == TranscriptKind::Thinking)
            .expect("thinking row");
        assert!(!ui.transcript[idx].details_collapsed, "{status:?}");

        toggle_transcript_row_details(&mut ui.transcript[idx]);
        app.apply_gateway_transcript_entry(
            &mut ui,
            Some("session-1"),
            gateway_test_entry(
                "live:turn-1:reasoning:0",
                TranscriptBlockKind::Reasoning,
                TranscriptBlockStatus::Running,
                Some("Thinking"),
                "live thinking continues",
            ),
        );
        assert!(ui.transcript[idx].details_collapsed, "{status:?}");
        toggle_transcript_row_details(&mut ui.transcript[idx]);

        app.apply_gateway_transcript_entry(
            &mut ui,
            Some("session-1"),
            gateway_test_entry(
                "live:turn-1:reasoning:0",
                TranscriptBlockKind::Reasoning,
                status,
                Some("Thinking"),
                "terminal thinking",
            ),
        );
        assert!(ui.transcript[idx].details_collapsed, "{status:?}");

        toggle_transcript_row_details(&mut ui.transcript[idx]);
        assert!(!ui.transcript[idx].details_collapsed, "{status:?}");
        app.apply_gateway_transcript_entry(
            &mut ui,
            Some("session-1"),
            gateway_test_entry(
                "live:turn-1:reasoning:0",
                TranscriptBlockKind::Reasoning,
                status,
                Some("Thinking"),
                "terminal thinking remains",
            ),
        );
        assert!(!ui.transcript[idx].details_collapsed, "{status:?}");
    }
}

#[test]
pub(crate) fn streaming_thinking_preview_tail_updates_while_collapsed() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_stream_event(
        RunStreamEvent::ReasoningDelta {
            text: numbered_lines(1, 8),
        },
        true,
        false,
    );
    let idx = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Thinking)
        .expect("thinking row");
    assert!(ui.transcript[idx].text.contains("line 8"));

    ui.apply_stream_event(
        RunStreamEvent::ReasoningDelta {
            text: format!("\n{}", numbered_lines(9, 12)),
        },
        true,
        false,
    );

    let row = &ui.transcript[idx];
    assert!(!row.expanded);
    assert!(!row.details_collapsed);
    assert!(row.text.contains("line 9"), "{}", row.text);
    assert!(row.text.contains("line 12"), "{}", row.text);
    assert!(!row.text.contains("line 8"), "{}", row.text);

    ui.apply_stream_event(RunStreamEvent::ReasoningEnd, true, false);
    let row = &mut ui.transcript[idx];
    assert!(row.details_collapsed);

    toggle_transcript_row_details(row);
    assert!(!row.details_collapsed);
    ui.apply_stream_event(RunStreamEvent::ReasoningEnd, true, false);
    assert!(!ui.transcript[idx].details_collapsed);
}

#[test]
pub(crate) fn typed_gateway_terminal_preamble_keeps_preview_behind_collapsed_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);
    let long = numbered_lines(1, 12);

    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_test_entry(
            "live:turn-1:assistant:0",
            TranscriptBlockKind::Reasoning,
            TranscriptBlockStatus::Completed,
            Some("Preamble"),
            &long,
        ),
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.kind == TranscriptKind::Thinking)
        .expect("thinking row");
    assert_eq!(row.title, "Thinking");
    assert!(!row.expanded);
    assert!(row.details_collapsed);
    assert_eq!(row.full_text.as_deref(), Some(long.as_str()));
    assert!(row.text.contains("... 6 more lines"), "{}", row.text);
    assert!(row.text.contains("line 12"), "{}", row.text);
    assert!(!row.text.contains("line 8"), "{}", row.text);
}

#[test]
pub(crate) fn typed_gateway_agent_session_start_makes_running_row_openable() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("parent-session".to_string());
    let mut ui = FullscreenUi::new(&app);

    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("parent-session"),
        TranscriptEntry {
            id: "live:turn-1:assistant:0".to_string(),
            thread_id: "parent-session".to_string(),
            turn_id: Some("turn-1".to_string()),
            message_seq: None,
            role: TranscriptEntryRole::Assistant,
            status: TranscriptBlockStatus::Running,
            source: "runtime.stream".to_string(),
            blocks: vec![TranscriptBlock {
                id: "live:turn-1:tool:call_agent_translate".to_string(),
                kind: TranscriptBlockKind::Agent,
                status: TranscriptBlockStatus::Running,
                order: 0,
                phase_ordinal: None,
                source: "runtime.stream".to_string(),
                title: Some("translate(Translate user message to Chinese)".to_string()),
                body: Some("Translate user message to Chinese".to_string()),
                preview: Some("Translate user message to Chinese".to_string()),
                detail: Some("Translate user message to Chinese".to_string()),
                artifact_ids: Vec::new(),
                metadata: Some(serde_json::json!({
                    "projection": "tool",
                    "tool_name": "spawn_agent",
                    "tool_call_id": "call_agent_translate",
                    "type": "agent_session_start",
                    "agent_name": "translate",
                    "task_name": "Translate user message to Chinese",
                    "child_session_id": "child-session",
                    "result": {
                        "child_session_id": "child-session",
                        "session_id": "child-session",
                        "status": "running"
                    }
                })),
                result: None,
                created_at_ms: 1,
                updated_at_ms: 2,
            }],
            metadata: None,
            usage: None,
            accounting: None,
            created_at_ms: 1,
            updated_at_ms: 2,
        },
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.tool_name.as_deref() == Some("spawn_agent"))
        .expect("agent row");
    assert_eq!(row.agent_target.as_deref(), Some("child-session"));
    assert_eq!(row.tool_call_id.as_deref(), Some("call_agent_translate"));
    assert_eq!(row.text, "Running (0 tool uses)");
}

#[test]
pub(crate) fn typed_gateway_background_agent_handoff_reuses_row_for_session_start() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("parent-session".to_string());
    let mut ui = FullscreenUi::new(&app);

    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("parent-session"),
        TranscriptEntry {
            id: "live:turn-1:assistant:0".to_string(),
            thread_id: "parent-session".to_string(),
            turn_id: Some("turn-1".to_string()),
            message_seq: None,
            role: TranscriptEntryRole::Assistant,
            status: TranscriptBlockStatus::Running,
            source: "runtime.stream".to_string(),
            blocks: vec![TranscriptBlock {
                id: "live:turn-1:tool:call_agent_translate".to_string(),
                kind: TranscriptBlockKind::Agent,
                status: TranscriptBlockStatus::Running,
                order: 0,
                phase_ordinal: None,
                source: "runtime.stream".to_string(),
                title: Some("translate(translate-en-to-zh)".to_string()),
                body: None,
                preview: None,
                detail: None,
                artifact_ids: Vec::new(),
                metadata: Some(serde_json::json!({
                    "projection": "tool",
                    "tool_name": "spawn_agent",
                    "tool_call_id": "call_agent_translate",
                    "type": "tool_execution_end",
                    "result": {
                        "agent_name": "translate",
                        "agent_description": "Detect the source language automatically.",
                        "task_name": "translate-en-to-zh",
                        "task": "Please translate this English text into Chinese.",
                        "status": "running",
                        "session_id": "child-session",
                        "child_session_id": "child-session"
                    }
                })),
                result: None,
                created_at_ms: 1,
                updated_at_ms: 2,
            }],
            metadata: None,
            usage: None,
            accounting: None,
            created_at_ms: 1,
            updated_at_ms: 2,
        },
    );

    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("parent-session"),
        TranscriptEntry {
            id: "live:turn-1:assistant:0".to_string(),
            thread_id: "parent-session".to_string(),
            turn_id: Some("turn-1".to_string()),
            message_seq: None,
            role: TranscriptEntryRole::Assistant,
            status: TranscriptBlockStatus::Running,
            source: "runtime.stream".to_string(),
            blocks: vec![TranscriptBlock {
                id: "live:turn-1:tool:call_agent_translate".to_string(),
                kind: TranscriptBlockKind::Agent,
                status: TranscriptBlockStatus::Running,
                order: 0,
                phase_ordinal: None,
                source: "runtime.stream".to_string(),
                title: Some("translate(translate-en-to-zh)".to_string()),
                body: Some("translate-en-to-zh".to_string()),
                preview: Some("translate-en-to-zh".to_string()),
                detail: Some("translate-en-to-zh".to_string()),
                artifact_ids: Vec::new(),
                metadata: Some(serde_json::json!({
                    "projection": "tool",
                    "tool_name": "spawn_agent",
                    "tool_call_id": "call_agent_translate",
                    "type": "agent_session_start",
                    "agent_name": "translate",
                    "agent_description": "Detect the source language automatically.",
                    "task_name": "translate-en-to-zh",
                    "child_session_id": "child-session",
                    "result": {
                        "child_session_id": "child-session",
                        "session_id": "child-session",
                        "status": "running"
                    }
                })),
                result: None,
                created_at_ms: 1,
                updated_at_ms: 3,
            }],
            metadata: None,
            usage: None,
            accounting: None,
            created_at_ms: 1,
            updated_at_ms: 3,
        },
    );

    let rows = ui
        .transcript
        .iter()
        .filter(|row| row.tool_name.as_deref() == Some("spawn_agent"))
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1, "{:#?}", ui.transcript);
    assert_eq!(rows[0].agent_target.as_deref(), Some("child-session"));
    assert_eq!(
        rows[0].tool_call_id.as_deref(),
        Some("call_agent_translate")
    );
    assert_eq!(rows[0].title, "translate(translate-en-to-zh)");
    assert!(rows[0].tool_started.is_none(), "{rows:#?}");
    assert_eq!(rows[0].text, "Started in background");

    ui.turn_outcome = Some(Outcome::Normal);
    ui.finish_turn();
    let row = ui
        .transcript
        .iter()
        .find(|row| row.tool_name.as_deref() == Some("spawn_agent"))
        .expect("agent row");
    assert!(!row.interrupted, "{row:#?}");
    assert_eq!(row.text, "Started in background");
}

#[test]
pub(crate) fn typed_gateway_reasoning_completion_is_idempotent_by_item_id() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);

    let running = gateway_test_entry(
        "live:turn-1:reasoning:0",
        TranscriptBlockKind::Reasoning,
        TranscriptBlockStatus::Running,
        Some("Thinking"),
        "The command is still running.",
    );
    let completed = gateway_test_entry(
        "live:turn-1:reasoning:0",
        TranscriptBlockKind::Reasoning,
        TranscriptBlockStatus::Completed,
        Some("Thinking"),
        "The command is still running.",
    );
    app.apply_gateway_transcript_entry(&mut ui, Some("session-1"), running);
    app.apply_gateway_transcript_entry(&mut ui, Some("session-1"), completed.clone());
    app.apply_gateway_transcript_entry(&mut ui, Some("session-1"), completed);

    let thinking_rows = ui
        .transcript
        .iter()
        .filter(|row| row.kind == TranscriptKind::Thinking)
        .collect::<Vec<_>>();
    assert_eq!(thinking_rows.len(), 1, "{:?}", ui.transcript);
    assert_eq!(thinking_rows[0].text, "The command is still running.");
    assert!(thinking_rows[0].tool_started.is_none());
}

#[test]
pub(crate) fn typed_gateway_final_answer_does_not_enter_thinking_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.current_session = Some("session-1".to_string());
    let mut ui = FullscreenUi::new(&app);

    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_test_entry(
            "live:turn-1:reasoning:0",
            TranscriptBlockKind::Reasoning,
            TranscriptBlockStatus::Completed,
            Some("Thinking"),
            "Now I can write the report.",
        ),
    );
    app.apply_gateway_transcript_entry(
        &mut ui,
        Some("session-1"),
        gateway_test_entry(
            "live:turn-1:assistant:0",
            TranscriptBlockKind::Text,
            TranscriptBlockStatus::Completed,
            None,
            "日报完成。",
        ),
    );

    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer && row.text == "日报完成。"),
        "{:?}",
        ui.transcript
    );
    assert!(
        ui.transcript
            .iter()
            .filter(|row| row.kind == TranscriptKind::Thinking)
            .all(|row| !row.text.contains("日报完成")),
        "{:?}",
        ui.transcript
    );
}
