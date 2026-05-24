#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

#[test]
pub(crate) fn transcript_auto_follow_tracks_wrapped_streaming_content() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.last_transcript_width = 32;
    ui.last_transcript_height = 4;
    for index in 0..6 {
        ui.transcript.push(TranscriptRow::with_title(
            TranscriptKind::Answer,
            "",
            format!("prior answer {index}"),
        ));
    }
    ui.scroll_to_bottom();
    let initial_bottom = ui.scroll;

    ui.apply_stream_event(
        RunStreamEvent::Event(serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": "streaming answer ".repeat(80)
                }]
            }
        })),
        true,
        false,
    );
    ui.follow_transcript_if_needed();

    assert!(ui.scroll > initial_bottom);
    assert_eq!(ui.scroll, ui.max_transcript_scroll());

    ui.scroll_transcript(-2);
    assert!(!ui.auto_follow_transcript);
    let manual_scroll = ui.scroll;
    ui.apply_stream_event(
        RunStreamEvent::Event(serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": "streaming answer ".repeat(120)
                }]
            }
        })),
        true,
        false,
    );
    ui.follow_transcript_if_needed();
    assert_eq!(ui.scroll, manual_scroll);

    ui.scroll_transcript(10_000);
    assert!(ui.auto_follow_transcript);
    assert_eq!(ui.scroll, ui.max_transcript_scroll());
}

#[test]
pub(crate) fn yielded_exec_session_stays_active_and_merges_live_poll_output() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_stream_event(
        RunStreamEvent::Event(serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "call_exec",
            "tool_name": "exec_command",
            "args": {"cmd": "printf start; sleep 1; printf done"},
            "started_at_ms": 1
        })),
        true,
        false,
    );
    ui.apply_stream_event(
        RunStreamEvent::Event(serde_json::json!({
            "type": "exec_session_yielded",
            "session_id": 42,
            "tool_call_id": "call_exec",
            "cmd": "printf start; sleep 1; printf done",
            "workdir": app.workdir.display().to_string(),
            "started_at_ms": 1
        })),
        true,
        false,
    );
    ui.apply_stream_event(
        RunStreamEvent::Event(serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_exec",
            "tool_name": "exec_command",
            "result": {
                "chunk_id": 0,
                "wall_time_seconds": 0.25,
                "exit_code": null,
                "session_id": 42,
                "original_token_count": 1,
                "output": "start"
            },
            "outcome": "normal",
            "elapsed_ms": 250
        })),
        true,
        false,
    );

    let exec_idx = ui.exec_session_rows[&42];
    assert!(active_tool_row(&ui.transcript[exec_idx]));
    assert_eq!(ui.transcript[exec_idx].text, "start");

    ui.apply_stream_event(
        RunStreamEvent::Event(serde_json::json!({
            "type": "exec_session_output_delta",
            "session_id": 42,
            "tool_call_id": "call_exec",
            "seq": 1,
            "output": "done"
        })),
        true,
        false,
    );
    let before_poll_rows = ui.transcript.len();
    ui.apply_stream_event(
        RunStreamEvent::Event(serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "call_poll",
            "tool_name": "write_stdin",
            "args": {"session_id": 42, "chars": ""}
        })),
        true,
        false,
    );
    ui.apply_stream_event(
        RunStreamEvent::Event(serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_poll",
            "tool_name": "write_stdin",
            "result": {
                "chunk_id": 1,
                "wall_time_seconds": 5.0,
                "exit_code": null,
                "session_id": 42,
                "original_token_count": 1,
                "output": "poll-output-should-not-render"
            },
            "outcome": "normal",
            "elapsed_ms": 5000
        })),
        true,
        false,
    );

    assert_eq!(ui.transcript.len(), before_poll_rows);
    assert_eq!(ui.transcript[exec_idx].text, "startdone");
    assert!(
        ui.transcript
            .iter()
            .all(|row| !row.title.contains("write_stdin"))
    );

    ui.apply_stream_event(
        RunStreamEvent::Event(serde_json::json!({
            "type": "exec_session_finished",
            "session_id": 42,
            "tool_call_id": "call_exec",
            "exit_code": 0,
            "elapsed_ms": 1250,
            "interrupted": false
        })),
        true,
        false,
    );
    assert!(!active_tool_row(&ui.transcript[exec_idx]));
    assert_eq!(
        ui.transcript[exec_idx].tool_elapsed,
        Some(Duration::from_millis(1250))
    );
}

#[test]
pub(crate) fn streaming_empty_write_stdin_poll_placeholder_is_hidden() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(TranscriptKind::Ran, "exec_command long fetch", "");
    row.tool_name = Some("exec_command".to_string());
    row.tool_call_id = Some("call_exec".to_string());
    row.tool_started = Some(Instant::now());
    ui.transcript.push(row);
    ui.exec_session_rows.insert(0, 0);

    ui.apply_stream_event(
        RunStreamEvent::Event(serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_call",
                    "id": "call_poll",
                    "name": "write_stdin",
                    "arguments_json": "{\"session_id\":0,\"yield_time_ms\":30000}",
                    "content_index": 0,
                    "call_index": 0
                }]
            }
        })),
        true,
        false,
    );
    ui.apply_stream_event(
        RunStreamEvent::Event(serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "call_poll",
            "tool_name": "write_stdin",
            "args": {"session_id": 0, "yield_time_ms": 30000}
        })),
        true,
        false,
    );
    ui.apply_stream_event(
        RunStreamEvent::Event(serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_poll",
            "tool_name": "write_stdin",
            "result": {
                "chunk_id": 1,
                "wall_time_seconds": 30.0,
                "exit_code": null,
                "session_id": 0,
                "original_token_count": 0,
                "output": ""
            },
            "outcome": "normal",
            "elapsed_ms": 30000
        })),
        true,
        false,
    );
    ui.finish_turn();

    assert_eq!(ui.transcript.len(), 1);
    assert_eq!(ui.transcript[0].tool_name.as_deref(), Some("exec_command"));
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.tool_name.as_deref() != Some("write_stdin"))
    );
}
