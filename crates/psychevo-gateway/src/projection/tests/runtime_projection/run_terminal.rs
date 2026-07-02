#[test]
fn run_start_projects_selected_skills() {
    let event = gateway_event_from_run_stream(
        "turn-1",
        &RunStreamEvent::value(json!({
            "type": "run_start",
            "session_id": "thread-1",
            "selected_skills": [
                {"name": "reviewer", "path": "/tmp/reviewer/SKILL.md"}
            ]
        })),
    );
    match event.expect("run_start should project a Gateway event") {
        GatewayEvent::TurnStarted {
            thread_id,
            turn_id,
            selected_skills,
        } => {
            assert_eq!(thread_id.as_deref(), Some("thread-1"));
            assert_eq!(turn_id, "turn-1");
            assert_eq!(selected_skills.len(), 1);
            assert_eq!(selected_skills[0].name, "reviewer");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn turn_complete_projects_terminal_turn_status() {
    let event = gateway_event_from_run_stream(
        "turn-1",
        &RunStreamEvent::value(json!({
            "type": "turn_complete",
            "session_id": "thread-1",
            "outcome": "failed",
            "error": "model service failed"
        })),
    );
    match event.expect("turn_complete should project a Gateway event") {
        GatewayEvent::TurnCompleted {
            thread_id,
            turn_id,
            turn,
            committed_entries,
        } => {
            assert_eq!(thread_id.as_deref(), Some("thread-1"));
            assert_eq!(turn_id, "turn-1");
            assert_eq!(turn.thread_id.as_deref(), Some("thread-1"));
            assert_eq!(turn.status, GatewayTurnStatus::Failed);
            assert_eq!(turn.outcome.as_deref(), Some("failed"));
            assert_eq!(
                turn.error.as_ref().map(|error| error.message.as_str()),
                Some("model service failed")
            );
            assert!(committed_entries.is_empty());
        }
        other => panic!("unexpected event: {other:?}"),
    }
}
