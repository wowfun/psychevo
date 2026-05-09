#[test]
fn resolves_unique_and_ambiguous_session_prefixes() {
    let sessions = vec![summary("abcdef"), summary("abc999"), summary("def000")];
    assert_eq!(
        resolve_session_ref_from_summaries(&sessions, "def").unwrap(),
        "def000"
    );
    assert!(resolve_session_ref_from_summaries(&sessions, "abc").is_err());
    assert_eq!(
        resolve_session_ref_from_summaries(&sessions, "latest").unwrap(),
        "abcdef"
    );
}

#[test]
fn turn_printer_hides_reasoning_by_default() {
    let mut printer = TurnPrinter::new(TuiRenderer::new(false), false, false);
    let mut output = Vec::new();
    printer
        .render_event(
            &RunStreamEvent::ReasoningDelta {
                text: "private".to_string(),
            },
            &mut output,
        )
        .expect("delta");
    printer
        .render_event(&RunStreamEvent::ReasoningEnd, &mut output)
        .expect("end");

    let output = String::from_utf8(output).expect("utf8");
    assert!(output.is_empty());
    assert!(!output.contains("private"));
}

#[test]
fn turn_printer_shows_reasoning_when_enabled() {
    let mut printer = TurnPrinter::new(TuiRenderer::new(false), true, false);
    let mut output = Vec::new();
    printer
        .render_event(
            &RunStreamEvent::ReasoningDelta {
                text: "visible thinking".to_string(),
            },
            &mut output,
        )
        .expect("delta");
    printer
        .render_event(&RunStreamEvent::ReasoningEnd, &mut output)
        .expect("end");

    let output = String::from_utf8(output).expect("utf8");
    assert!(output.contains("Thinking:"));
    assert!(output.contains("visible thinking"));
}

#[test]
fn turn_printer_preserves_bash_command_title_until_tool_end() {
    let mut printer = TurnPrinter::new(TuiRenderer::new(false), false, false);
    let mut output = Vec::new();
    printer
        .render_event(
            &RunStreamEvent::Event(serde_json::json!({
                "type": "tool_execution_start",
                "tool_call_id": "call_bash",
                "tool_name": "bash",
                "args": {"command": "cargo test -p psychevo-cli\ncargo fmt"}
            })),
            &mut output,
        )
        .expect("start");
    printer
        .render_event(
            &RunStreamEvent::Event(serde_json::json!({
                "type": "tool_execution_end",
                "tool_call_id": "call_bash",
                "tool_name": "bash",
                "result": {"output": "ok", "exit_code": 0},
                "outcome": "normal"
            })),
            &mut output,
        )
        .expect("end");

    let output = String::from_utf8(output).expect("utf8");
    assert!(output.contains("Running cargo test -p psychevo-cli: running"));
    assert!(output.contains("Ran cargo test -p psychevo-cli:"));
    assert!(!output.contains("Ran command"));
}

#[test]
fn turn_printer_announces_streaming_tool_preparation_once() {
    let mut printer = TurnPrinter::new(TuiRenderer::new(false), false, false);
    let mut output = Vec::new();
    printer
        .render_event(
            &RunStreamEvent::Event(serde_json::json!({
                "type": "message_update",
                "message": {
                    "role": "assistant",
                    "content": [{
                        "type": "tool_call",
                        "id": "call_write",
                        "name": "write",
                        "arguments": null,
                        "arguments_json": "{\"path\":\"report.md\"",
                        "arguments_error": "EOF while parsing",
                        "content_index": 0,
                        "call_index": 0
                    }]
                }
            })),
            &mut output,
        )
        .expect("pending");
    printer
        .render_event(
            &RunStreamEvent::Event(serde_json::json!({
                "type": "message_update",
                "message": {
                    "role": "assistant",
                    "content": [{
                        "type": "tool_call",
                        "id": "call_write",
                        "name": "write",
                        "arguments": {"path": "report.md", "content": "body"},
                        "arguments_json": "{\"path\":\"report.md\",\"content\":\"body\"}",
                        "arguments_error": null,
                        "content_index": 0,
                        "call_index": 0
                    }]
                }
            })),
            &mut output,
        )
        .expect("pending update");
    printer
        .render_event(
            &RunStreamEvent::Event(serde_json::json!({
                "type": "tool_execution_start",
                "tool_call_id": "call_write",
                "tool_name": "write",
                "args": {"path": "report.md", "content": "body"}
            })),
            &mut output,
        )
        .expect("start");
    printer
        .render_event(
            &RunStreamEvent::Event(serde_json::json!({
                "type": "tool_execution_end",
                "tool_call_id": "call_write",
                "tool_name": "write",
                "result": {"path": "report.md", "bytes_written": 4},
                "outcome": "normal",
                "elapsed_ms": 1_000
            })),
            &mut output,
        )
        .expect("end");

    let output = String::from_utf8(output).expect("utf8");
    assert_eq!(output.matches("Changing files: preparing").count(), 1);
    assert!(!output.contains("Changing report.md: running"));
    assert!(output.contains("Changed report.md 1s:"));
}

#[test]
fn turn_printer_scopes_reused_tool_positions_across_messages() {
    let mut printer = TurnPrinter::new(TuiRenderer::new(false), false, false);
    let mut output = Vec::new();
    printer
        .render_event(
            &RunStreamEvent::Event(serde_json::json!({
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [{
                        "type": "tool_call",
                        "id": "",
                        "name": "bash",
                        "arguments": {"command": "echo one"},
                        "arguments_json": "{\"command\":\"echo one\"}",
                        "arguments_error": null,
                        "content_index": 0,
                        "call_index": 0
                    }]
                }
            })),
            &mut output,
        )
        .expect("first");
    printer
        .render_event(
            &RunStreamEvent::Event(serde_json::json!({
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [{
                        "type": "tool_call",
                        "id": "",
                        "name": "write",
                        "arguments": {"path": "report.md", "content": "body"},
                        "arguments_json": "{\"path\":\"report.md\",\"content\":\"body\"}",
                        "arguments_error": null,
                        "content_index": 0,
                        "call_index": 0
                    }]
                }
            })),
            &mut output,
        )
        .expect("second");

    let output = String::from_utf8(output).expect("utf8");
    assert!(output.contains("Running echo one: preparing"));
    assert!(output.contains("Changing report.md: preparing"));
}
