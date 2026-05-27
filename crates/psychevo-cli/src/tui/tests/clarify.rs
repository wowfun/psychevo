#[allow(unused_imports)]
pub(crate) use super::*;
#[test]
pub(crate) fn clarify_request_opens_bottom_panel_and_renders_options() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_stream_event(clarify_request_event(), true, false);

    assert!(matches!(ui.bottom_panel, Some(BottomPanel::Clarify(_))));
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 24);
    let text = buffer_text(&buffer);
    assert!(text.contains("Question 1/3 (3 unanswered)"));
    assert!(text.contains("Which mode should we use?"));
    assert!(text.contains("Fast (Recommended)"));
    assert!(text.contains("Other"));
    assert!(text.contains("tab to edit note/custom answer"));
    assert!(!text.contains("HeaderHidden"));
    assert!(!text.contains("clarify"));
    assert!(!text.contains("N note"));
    assert_eq!(
        ui.last_bottom_panel_area.expect("bottom panel area").height,
        10
    );
    assert_eq!(ui.last_bottom_panel_areas.len(), 3);
    assert!(
        ui.transcript
            .iter()
            .all(|row| { row.title != "Asking user" && !row.text.contains("clarify requested") })
    );
}

#[test]
pub(crate) fn clarify_panel_supports_other_text_and_optional_note_modes() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_stream_event(clarify_request_event(), true, false);
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("down");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("down to other");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .expect("open other");
    for ch in "custom".chars() {
        app.handle_bottom_panel_key(
            &mut ui,
            KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .expect("type other");
    }

    assert_eq!(
        ui.bottom_panel.as_ref().and_then(|panel| match panel {
            BottomPanel::Clarify(panel) => Some(panel.mode()),
            _ => None,
        }),
        Some(ClarifyInputMode::Other)
    );
    let mut buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 24);
    let mut text = buffer_text(&buffer);
    assert!(!text.contains("Other answer"));
    assert!(text.contains("answer: custom"));
    assert!(text.contains("custom"));

    for _ in 0..3 {
        app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
            .expect("move other cursor");
    }
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('-'), KeyModifiers::NONE),
    )
    .expect("insert other middle");
    buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 24);
    text = buffer_text(&buffer);
    assert!(text.contains("answer: cus-tom"));

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .expect("back to options");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
        .expect("next question");
    buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 24);
    text = buffer_text(&buffer);
    assert!(text.contains("Question 2/3 (3 unanswered)"));
    assert!(text.contains("How much detail should the answer include?"));
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
        .expect("previous question");
    buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 24);
    text = buffer_text(&buffer);
    assert!(text.contains("Question 1/3 (3 unanswered)"));
    assert!(text.contains("answer: cus-tom"));

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("reopen other");
    assert_eq!(
        ui.bottom_panel.as_ref().and_then(|panel| match panel {
            BottomPanel::Clarify(panel) => Some(panel.mode()),
            _ => None,
        }),
        Some(ClarifyInputMode::Other)
    );
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::End, KeyModifiers::NONE))
        .expect("end other");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
    )
    .expect("edit other");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
    )
    .expect("edit other");
    buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 24);
    text = buffer_text(&buffer);
    assert!(text.contains("answer: cus-tos"));
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .expect("back to options");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
    )
    .expect("select first");
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
    )
    .expect("n is ignored");
    assert_eq!(
        ui.bottom_panel.as_ref().and_then(|panel| match panel {
            BottomPanel::Clarify(panel) => Some(panel.mode()),
            _ => None,
        }),
        Some(ClarifyInputMode::Options)
    );
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .expect("note");
    for ch in "include tests".chars() {
        app.handle_bottom_panel_key(
            &mut ui,
            KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .expect("type note");
    }
    for _ in 0..5 {
        app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
            .expect("move note cursor");
    }
    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('-'), KeyModifiers::NONE),
    )
    .expect("insert note middle");

    assert_eq!(
        ui.bottom_panel.as_ref().and_then(|panel| match panel {
            BottomPanel::Clarify(panel) => Some(panel.mode()),
            _ => None,
        }),
        Some(ClarifyInputMode::Note)
    );
    buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 24);
    text = buffer_text(&buffer);
    assert!(!text.contains("Optional note"));
    assert!(text.contains("note: include -tests"));
    assert!(text.contains("include -tests"));
}

#[tokio::test]
pub(crate) async fn clarify_panel_mouse_click_selects_options_and_opens_other_input() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_stream_event(clarify_request_event(), true, false);
    let _ = draw_fullscreen_for_test(&app, &mut ui, 100, 24);
    let careful_area = ui
        .last_bottom_panel_areas
        .iter()
        .find(|(index, _)| *index == 1)
        .map(|(_, area)| *area)
        .expect("careful row");
    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: careful_area.x.saturating_add(2),
            row: careful_area.y,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("click normal option");

    let panel = match ui.bottom_panel.as_ref().expect("clarify panel") {
        BottomPanel::Clarify(panel) => panel,
        _ => panic!("expected clarify panel"),
    };
    assert_eq!(panel.question_index, 1);
    assert_eq!(panel.mode(), ClarifyInputMode::Options);
    assert_eq!(
        panel.answers[0].as_ref().expect("first answer").answers,
        vec!["Careful".to_string()]
    );
    assert!(
        panel.current_question().is_some_and(
            |question| question.question == "How much detail should the answer include?"
        )
    );

    let other_area = ui
        .last_bottom_panel_areas
        .iter()
        .find(|(index, _)| *index == 2)
        .map(|(_, area)| *area)
        .expect("other row");
    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: other_area.x.saturating_add(2),
            row: other_area.y,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("click other option");

    let panel = match ui.bottom_panel.as_ref().expect("clarify panel") {
        BottomPanel::Clarify(panel) => panel,
        _ => panic!("expected clarify panel"),
    };
    assert_eq!(panel.question_index, 1);
    assert_eq!(panel.selected(), 2);
    assert_eq!(panel.mode(), ClarifyInputMode::Other);
}

#[test]
pub(crate) fn clarify_resolved_restores_previous_bottom_panel() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.bottom_panel = Some(BottomPanel::Stats(BottomSelectionPanel::new(
        "Stats",
        "",
        "No stats",
        Vec::new(),
    )));

    ui.apply_stream_event(clarify_request_event(), true, false);
    assert!(matches!(ui.bottom_panel, Some(BottomPanel::Clarify(_))));
    ui.apply_stream_event(
        RunStreamEvent::ClarifyResolved(ClarifyResolvedEvent {
            call_id: "call_clarify".to_string(),
            reason: psychevo_runtime::ClarifyResolvedReason::TimedOut,
        }),
        true,
        false,
    );

    assert!(matches!(ui.bottom_panel, Some(BottomPanel::Stats(_))));
    assert!(ui.transcript.iter().all(|row| {
        !row.text.contains("clarify timed out") && !row.text.contains("clarify cancelled")
    }));
}

#[test]
pub(crate) fn clarify_request_from_background_session_is_buffered_without_focus_steal() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.apply_owned_fullscreen_stream_event(
        &mut ui,
        Some("fedcba9876543210"),
        clarify_request_event(),
    );

    assert!(ui.bottom_panel.is_none());
    assert!(
        ui.session_live_event_backlog
            .get("fedcba9876543210")
            .is_some_and(|events| events.len() == 1)
    );
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Status
            && row.text.contains("clarify pending in session fedcba98")
    }));
}

#[test]
pub(crate) fn clarify_tool_result_cell_summarizes_questions_and_answers() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();

    let args = clarify_args();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "call_clarify",
            "tool_name": "clarify",
            "args": args.clone()
        }),
        false,
    );
    assert!(ui.transcript.iter().all(|row| row.title != "Asking user"));
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_clarify",
            "tool_name": "clarify",
            "result": {
                "answers": [
                    {
                        "answers": ["Fast (Recommended)", "user_note: include tests"]
                    },
                    {
                        "answers": ["Brief"]
                    },
                    {
                        "answers": ["Markdown"]
                    }
                ]
            },
            "outcome": "normal"
        }),
        false,
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.title == "Questions 3/3 answered")
        .expect("clarify result row");
    assert_eq!(row.kind, TranscriptKind::Status);
    let full_text = row.full_text.as_deref().unwrap_or(&row.text);
    assert!(full_text.contains("Which mode should we use?"));
    assert!(full_text.contains("answer: Fast (Recommended)"));
    assert!(full_text.contains("note: include tests"));
}

#[test]
pub(crate) fn clarify_cancel_result_cell_is_unanswered_without_status_noise() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.start_assistant();
    ui.apply_stream_event(clarify_request_event(), true, false);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_clarify",
            "tool_name": "clarify",
            "result": {"error": "clarify was cancelled by the user"},
            "outcome": "failed"
        }),
        false,
    );

    let row = ui
        .transcript
        .iter()
        .find(|row| row.title == "Questions 0/3 answered")
        .expect("clarify declined row");
    assert_eq!(row.kind, TranscriptKind::Status);
    assert!(!row.failed);
    assert_eq!(row.text.matches("(unanswered)").count(), 3);
    assert!(!row.text.contains("clarify was cancelled"));
    assert!(ui.transcript.iter().all(|row| {
        !row.text.contains("clarify cancelled") && !row.text.contains("clarify timed out")
    }));
}

#[test]
pub(crate) fn clarify_history_tool_result_reuses_tool_call_questions() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let args = clarify_args();
    let assistant = serde_json::json!({
        "role": "assistant",
        "content": [{
            "type": "tool_call",
            "id": "call_clarify",
            "name": "clarify",
            "arguments": args,
            "content_index": 0,
            "call_index": 0
        }],
        "timestamp_ms": 1,
        "finish_reason": "tool_calls",
        "outcome": "normal"
    });
    let result = serde_json::json!({
        "answers": [
            {
                "answers": ["Careful"]
            },
            {
                "answers": ["Brief"]
            },
            {
                "answers": ["Markdown"]
            }
        ]
    });
    let tool_result = serde_json::json!({
        "role": "tool_result",
        "tool_call_id": "call_clarify",
        "tool_name": "clarify",
        "content": serde_json::to_string(&result).expect("result json"),
        "is_error": false,
        "timestamp_ms": 2
    });

    ui.push_history_message(&assistant, None, None);
    ui.push_history_message(&tool_result, None, None);

    let row = ui
        .transcript
        .iter()
        .find(|row| row.title == "Questions 3/3 answered")
        .expect("clarify history result row");
    assert!(row.text.contains("Which mode should we use?"));
    assert!(row.text.contains("answer: Careful"));
    assert!(!row.title.contains("User answered"));
    assert!(!row.text.contains("clarify normal"));
}

#[test]
pub(crate) fn tui_snapshot_clarify_question_panel() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    ui.apply_stream_event(clarify_request_event(), true, false);
    assert_tui_snapshot("clarify_question_panel", 120, 24, &app, ui);
}

#[test]
pub(crate) fn tui_snapshot_clarify_other_inline() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    ui.apply_stream_event(clarify_request_event(), true, false);
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("down");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("down to other");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("open other");
    for ch in "custom path".chars() {
        app.handle_bottom_panel_key(
            &mut ui,
            KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .expect("type other");
    }
    assert_tui_snapshot("clarify_other_inline", 120, 24, &app, ui);
}

#[test]
pub(crate) fn tui_snapshot_clarify_note_inline() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    ui.apply_stream_event(clarify_request_event(), true, false);
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .expect("note");
    for ch in "include tests".chars() {
        app.handle_bottom_panel_key(
            &mut ui,
            KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .expect("type note");
    }
    assert_tui_snapshot("clarify_note_inline", 120, 24, &app, ui);
}

#[test]
pub(crate) fn tui_snapshot_clarify_answered_result() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    apply_clarify_answered_result(&mut ui);
    assert_tui_snapshot("clarify_answered_result", 120, 24, &app, ui);
}

#[test]
pub(crate) fn tui_snapshot_clarify_declined_result() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    apply_clarify_declined_result(&mut ui);
    assert_tui_snapshot("clarify_declined_result", 120, 24, &app, ui);
}

pub(crate) fn apply_clarify_answered_result(ui: &mut FullscreenUi<'_>) {
    let args = clarify_args();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_clarify",
            "tool_name": "clarify",
            "args": args,
            "result": {
                "answers": [
                    {
                        "answers": ["Fast (Recommended)", "user_note: include tests"]
                    },
                    {
                        "answers": ["Brief"]
                    },
                    {
                        "answers": ["Markdown"]
                    }
                ]
            },
            "outcome": "normal"
        }),
        false,
    );
}

pub(crate) fn apply_clarify_declined_result(ui: &mut FullscreenUi<'_>) {
    let args = clarify_args();
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_clarify",
            "tool_name": "clarify",
            "args": args,
            "result": {"error": "clarify was cancelled by the user"},
            "outcome": "failed"
        }),
        false,
    );
}

#[tokio::test]
pub(crate) async fn permission_approval_panel_mouse_click_resolves_each_option() {
    assert_permission_click_outcome(0, PermissionApprovalOutcome::AllowOnce).await;
    assert_permission_click_outcome(1, PermissionApprovalOutcome::AllowSession).await;
    assert_permission_click_outcome(2, PermissionApprovalOutcome::AllowAlways).await;
    assert_permission_click_outcome(3, PermissionApprovalOutcome::Deny).await;
}

async fn assert_permission_click_outcome(index: usize, expected: PermissionApprovalOutcome) {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let (response, decision) = oneshot::channel();
    ui.active_permission_approval = Some(response);
    ui.bottom_panel = Some(BottomPanel::PermissionApproval(
        PermissionApprovalPanel::new(
            app.current_session.clone(),
            PermissionApprovalRequest {
                tool_call_id: "call_fetch".to_string(),
                tool_name: "web_fetch".to_string(),
                summary: "https://example.com/article".to_string(),
                reason: "network access to `example.com` requires approval".to_string(),
                matched_rule: None,
                suggested_rule: Some("WebFetch(https://example.com/*)".to_string()),
                allow_always: true,
                timeout_secs: 0,
            },
            None,
        ),
    ));
    let _ = draw_fullscreen_for_test(&app, &mut ui, 100, 24);
    let area = ui
        .last_bottom_panel_areas
        .iter()
        .find(|(row_index, _)| *row_index == index)
        .map(|(_, area)| *area)
        .expect("approval option row");

    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x.saturating_add(2),
            row: area.y,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("click approval option");

    assert_eq!(decision.await.expect("approval decision").outcome, expected);
    assert!(ui.active_permission_approval.is_none());
    assert!(ui.bottom_panel.is_none());
}

pub(crate) fn clarify_request_event() -> RunStreamEvent {
    RunStreamEvent::ClarifyRequest(ClarifyRequestEvent {
        call_id: "call_clarify".to_string(),
        questions: clarify_questions(),
    })
}

pub(crate) fn clarify_questions() -> Vec<ClarifyQuestion> {
    vec![
        ClarifyQuestion {
            question: "Which mode should we use?".to_string(),
            options: vec![
                psychevo_runtime::ClarifyQuestionOption {
                    label: "Fast (Recommended)".to_string(),
                    description: "Prioritize speed".to_string(),
                },
                psychevo_runtime::ClarifyQuestionOption {
                    label: "Careful".to_string(),
                    description: "Prioritize review".to_string(),
                },
            ],
        },
        ClarifyQuestion {
            question: "How much detail should the answer include?".to_string(),
            options: vec![
                psychevo_runtime::ClarifyQuestionOption {
                    label: "Brief".to_string(),
                    description: "Keep it concise".to_string(),
                },
                psychevo_runtime::ClarifyQuestionOption {
                    label: "Deep".to_string(),
                    description: "Cover tradeoffs".to_string(),
                },
            ],
        },
        ClarifyQuestion {
            question: "Which output format should be used?".to_string(),
            options: vec![
                psychevo_runtime::ClarifyQuestionOption {
                    label: "Markdown".to_string(),
                    description: "Use prose and bullets".to_string(),
                },
                psychevo_runtime::ClarifyQuestionOption {
                    label: "JSON".to_string(),
                    description: "Use structured data".to_string(),
                },
            ],
        },
    ]
}

pub(crate) fn clarify_args() -> serde_json::Value {
    serde_json::json!({
        "questions": clarify_questions()
            .into_iter()
            .map(|question| {
                serde_json::json!({
                    "question": question.question,
                    "options": question.options.into_iter().map(|option| {
                        serde_json::json!({
                            "label": option.label,
                            "description": option.description
                        })
                    }).collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>()
    })
}
