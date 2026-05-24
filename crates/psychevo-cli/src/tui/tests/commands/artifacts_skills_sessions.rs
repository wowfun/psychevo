#[allow(unused_imports)]
pub(crate) use super::*;

#[tokio::test]
pub(crate) async fn pending_preview_queue_edit_confirm_and_escape() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    attach_pending_agent_running(&mut ui);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Queue("next turn".to_string()))
        .await
        .expect("queue");
    let sequence = queued_prompt_sequence(&ui);
    let _ = draw_fullscreen_for_test(&app, &mut ui, 80, 14);
    let edit_area = ui
        .last_pending_input_action_areas
        .iter()
        .find_map(|(target, action, area)| {
            (*target == PendingInputRef::Queue(sequence) && *action == PendingInputAction::Edit)
                .then_some(*area)
        })
        .expect("queue edit area");
    app.handle_fullscreen_mouse(
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: edit_area.x,
            row: edit_area.y,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("click edit");
    assert_eq!(
        ui.pending_input_edit.as_ref().map(|edit| edit.target),
        Some(PendingInputRef::Queue(sequence))
    );
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 80, 14);
    let text = buffer_text(&buffer);
    assert!(text.contains("editing queue"));
    assert!(text.contains("Enter confirm"));
    assert!(text.contains("Esc cancel"));

    ui.pending_input_edit.as_mut().expect("edit").textarea = textarea_with_text("edited next");
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("confirm edit");
    assert!(ui.pending_input_edit.is_none());
    match ui.queued_inputs.front().expect("queued prompt") {
        QueuedInput::Prompt {
            prompt,
            display_prompt,
            ..
        } => {
            assert_eq!(prompt, "edited next");
            assert_eq!(display_prompt, "edited next");
        }
        other => panic!("unexpected queued input: {other:?}"),
    }

    assert!(ui.start_pending_input_edit(PendingInputRef::Queue(sequence)));
    ui.pending_input_edit.as_mut().expect("edit").textarea = textarea_with_text("discarded draft");
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("cancel edit");
    assert!(ui.pending_input_edit.is_none());
    match ui.queued_inputs.front().expect("queued prompt") {
        QueuedInput::Prompt { display_prompt, .. } => {
            assert_eq!(display_prompt, "edited next");
        }
        other => panic!("unexpected queued input: {other:?}"),
    }
}

#[tokio::test]
pub(crate) async fn pending_preview_steer_edit_updates_before_drain() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    attach_pending_agent_running(&mut ui);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Steer("nudge now".to_string()))
        .await
        .expect("steer");
    let id = ui.pending_steers[0].id;
    assert!(ui.start_pending_input_edit(PendingInputRef::Steer(id)));
    ui.pending_input_edit.as_mut().expect("edit").textarea = textarea_with_text("edited nudge");
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("confirm edit");

    assert!(ui.pending_input_edit.is_none());
    assert_eq!(ui.pending_steers.len(), 1);
    assert_eq!(ui.pending_steers[0].id, id);
    assert_eq!(ui.pending_steers[0].display_prompt, "edited nudge");
    assert_eq!(
        ui.ephemeral_status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("pending input updated")
    );
}

#[tokio::test]
pub(crate) async fn pending_preview_late_confirm_resubmits_as_new_input() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    attach_pending_agent_running(&mut ui);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Steer("nudge now".to_string()))
        .await
        .expect("steer");
    let old_id = ui.pending_steers[0].id;
    assert!(ui.start_pending_input_edit(PendingInputRef::Steer(old_id)));
    ui.pending_input_edit.as_mut().expect("edit").textarea = textarea_with_text("late nudge");
    assert!(
        ui.running
            .as_ref()
            .expect("running")
            .control
            .cancel_pending_user_message(old_id)
    );
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("confirm late steer edit");
    assert!(ui.pending_input_edit.is_none());
    assert_eq!(ui.pending_steers.len(), 1);
    assert_ne!(ui.pending_steers[0].id, old_id);
    assert_eq!(ui.pending_steers[0].display_prompt, "late nudge");

    let sequence = ui.next_pending_input_sequence();
    ui.queued_inputs.push_back(QueuedInput::Prompt {
        session_id: app.current_session.clone(),
        prompt: "queued prompt".to_string(),
        display_prompt: "queued prompt".to_string(),
        images: Vec::new(),
        sequence,
    });
    assert!(ui.start_pending_input_edit(PendingInputRef::Queue(sequence)));
    ui.pending_input_edit.as_mut().expect("edit").textarea =
        textarea_with_text("late queued prompt");
    ui.queued_inputs.clear();
    ui.pending_steers.clear();
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("confirm late queue edit");
    assert!(ui.queued_inputs.is_empty());
    assert_eq!(ui.pending_steers.len(), 1);
    assert_eq!(ui.pending_steers[0].display_prompt, "late queued prompt");
}

#[tokio::test]
pub(crate) async fn pending_preview_undo_removes_steer_and_queue() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    attach_pending_agent_running(&mut ui);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Steer("nudge now".to_string()))
        .await
        .expect("steer");
    app.handle_fullscreen_command(&mut ui, SlashCommand::Queue("next turn".to_string()))
        .await
        .expect("queue");
    let steer_id = ui.pending_steers[0].id;
    let sequence = queued_prompt_sequence(&ui);

    app.handle_pending_input_action(
        &mut ui,
        PendingInputRef::Queue(sequence),
        PendingInputAction::Undo,
    )
    .expect("undo queue");
    assert!(ui.queued_inputs.is_empty());
    assert_eq!(ui.pending_steers.len(), 1);

    app.handle_pending_input_action(
        &mut ui,
        PendingInputRef::Steer(steer_id),
        PendingInputAction::Undo,
    )
    .expect("undo steer");
    assert!(ui.pending_steers.is_empty());
    assert!(!ui.has_pending_input_preview());
}

#[tokio::test]
pub(crate) async fn pending_cancel_clears_unsent_steer_and_queue() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    attach_pending_agent_running(&mut ui);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Steer("nudge now".to_string()))
        .await
        .expect("steer");
    app.handle_fullscreen_command(&mut ui, SlashCommand::Queue("next turn".to_string()))
        .await
        .expect("queue");
    app.handle_fullscreen_command(&mut ui, SlashCommand::PendingCancel)
        .await
        .expect("pending cancel");

    assert!(ui.pending_steers.is_empty());
    assert!(ui.queued_inputs.is_empty());
    assert!(ui.pending_input_edit.is_none());
    assert!(!ui.has_pending_input_preview());
    assert!(
        ui.transcript
            .iter()
            .all(|row| { !(row.kind == TranscriptKind::Status && row.title == "Pending steer") })
    );
    assert_eq!(
        ui.ephemeral_status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("pending input canceled: 2")
    );
}

pub(crate) fn queued_prompt_sequence(ui: &FullscreenUi<'_>) -> u64 {
    match ui.queued_inputs.front().expect("queued prompt") {
        QueuedInput::Prompt { sequence, .. } => *sequence,
        other => panic!("unexpected queued input: {other:?}"),
    }
}

#[tokio::test]
pub(crate) async fn explicit_steer_errors_when_idle() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::Steer("too soon".to_string()))
        .await
        .expect("steer");

    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/steer too soon"
            && row.failed
            && row.text.contains("/steer requires a running agent turn")
    }));
}

#[tokio::test]
pub(crate) async fn fullscreen_export_and_share_write_artifacts() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(
            &app.workdir,
            "tui",
            "mock-model",
            "mock",
            Some(serde_json::json!({
                "base_url": "https://example.test/v1",
                "mode": "default",
                "model_metadata": {
                    "capabilities": {
                        "tool_call": true
                    }
                }
            })),
        )
        .expect("session");
    app.current_session = Some(session_id.clone());
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_tui_message(
        &conn,
        &session_id,
        1,
        "user",
        1,
        serde_json::json!({
            "role": "user",
            "content": [{"text": "export this prompt"}],
            "timestamp_ms": 1
        }),
    );
    insert_tui_message(
        &conn,
        &session_id,
        2,
        "assistant",
        2,
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "exported answer"}],
            "timestamp_ms": 2,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
    );
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(
        &mut ui,
        SlashCommand::Export(crate::tui::slash::TuiExportOptions {
            path: Some("exports/session.md".to_string()),
            format: SessionExportFormat::Markdown,
            include: psychevo_runtime::SessionExportIncludeSet::default_for(
                SessionArtifactKind::Export,
            ),
        }),
    )
    .await
    .expect("export");

    let export_path = app.workdir.join("exports/session.md");
    let content = fs::read_to_string(&export_path).expect("export content");
    assert!(content.contains("export this prompt"));
    assert!(content.contains("exported answer"));
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/export exports/session.md"
            && row.text.contains("exported:")
            && row.text.contains("exports/session.md")
    }));

    app.handle_fullscreen_command(
        &mut ui,
        SlashCommand::Export(crate::tui::slash::TuiExportOptions {
            path: Some("exports/session.json".to_string()),
            format: SessionExportFormat::Json,
            include: psychevo_runtime::SessionExportIncludeSet::parse(
                "last-provider-request",
                SessionArtifactKind::Export,
            )
            .unwrap(),
        }),
    )
    .await
    .expect("export last request json");

    let last_export_path = app.workdir.join("exports/session.json");
    let content = fs::read_to_string(&last_export_path).expect("last request export content");
    let value: Value = serde_json::from_str(&content).expect("last request export json");
    assert_eq!(
        value["last_provider_request"]["body"]["messages"][1]["content"],
        "export this prompt"
    );
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title
                == "/export exports/session.json --format json --include last-provider-request"
            && row.text.contains("exported:")
            && row.text.contains("exports/session.json")
    }));

    app.handle_fullscreen_command(
        &mut ui,
        SlashCommand::Share(crate::tui::slash::TuiShareOptions {
            path: Some("share.md".to_string()),
            include: psychevo_runtime::SessionExportIncludeSet::default_for(
                SessionArtifactKind::Share,
            ),
        }),
    )
    .await
    .expect("share");

    let share_path = app.workdir.join("share.md");
    let content = fs::read_to_string(&share_path).expect("share content");
    assert!(content.contains("exported answer"));
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/share share.md"
            && row.text.contains("share:")
            && row.text.contains("share.md")
    }));
}

#[tokio::test]
pub(crate) async fn fullscreen_skills_command_lists_dynamic_entries_and_submits_dynamic_slash() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    fs::create_dir_all(app.workdir.join(".git")).expect("git marker");
    let skill_dir = app.home.join("skills").join("helper");
    fs::create_dir_all(&skill_dir).expect("skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: helper\ndescription: Helps with focused edits\n---\n\nFollow the helper workflow.\n",
    )
    .expect("skill");

    let matches = app.slash_menu_items("/helper");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].command, "/helper");

    let mut ui = FullscreenUi::new(&app);
    app.handle_fullscreen_command(&mut ui, SlashCommand::Skills(None))
        .await
        .expect("skills dashboard");
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/skills"
            && row.text.contains("/skills search <query>")
            && row.text.contains("/skills reload")
    }));

    app.handle_fullscreen_command(&mut ui, SlashCommand::Skills(Some("list".to_string())))
        .await
        .expect("skills");
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/skills list"
            && row.text.contains("helper: Helps with focused edits")
    }));
    app.handle_fullscreen_command(
        &mut ui,
        SlashCommand::Skills(Some("inspect helper".to_string())),
    )
    .await
    .expect("skills inspect");
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/skills inspect helper"
            && row.text.contains("name: helper")
            && row.text.contains("readiness:")
    }));
    app.handle_fullscreen_command(
        &mut ui,
        SlashCommand::Skills(Some("audit helper".to_string())),
    )
    .await
    .expect("skills audit");
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/skills audit helper"
            && row.text.contains("helper: Safe")
    }));
    app.handle_fullscreen_command(&mut ui, SlashCommand::Skills(Some("reload".to_string())))
        .await
        .expect("skills reload");
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/skills reload"
            && row.text.contains("reloaded skills: 1 skills")
    }));

    let local_source = temp.path().join("local-skill");
    fs::create_dir_all(&local_source).expect("local skill source");
    fs::write(
        local_source.join("SKILL.md"),
        "---\nname: local-helper\ndescription: Local helper\n---\n\nLocal helper body.\n",
    )
    .expect("local skill");
    let global_source = temp.path().join("global-skill");
    fs::create_dir_all(&global_source).expect("global skill source");
    fs::write(
        global_source.join("SKILL.md"),
        "---\nname: global-helper\ndescription: Global helper\n---\n\nGlobal helper body.\n",
    )
    .expect("global skill");

    app.current_permission_mode = PermissionMode::BypassPermissions;
    app.handle_fullscreen_command(
        &mut ui,
        SlashCommand::Skills(Some(format!("install {}", local_source.display()))),
    )
    .await
    .expect("skills local install");
    assert!(
        app.workdir
            .join(".psychevo/skills/local-helper/SKILL.md")
            .exists()
    );
    assert!(!app.home.join("skills/local-helper/SKILL.md").exists());

    app.handle_fullscreen_command(
        &mut ui,
        SlashCommand::Skills(Some(format!("install -g {}", global_source.display()))),
    )
    .await
    .expect("skills global install");
    assert!(app.home.join("skills/global-helper/SKILL.md").exists());

    app.handle_fullscreen_command(
        &mut ui,
        SlashCommand::Skills(Some("config enable --scope global helper".to_string())),
    )
    .await
    .expect("skills legacy scope");
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/skills config enable --scope global helper"
            && row.text.contains("use --local or -g/--global")
    }));

    app.current_mode = RunMode::Plan;
    app.handle_fullscreen_command(
        &mut ui,
        SlashCommand::Skills(Some("install ./helper".to_string())),
    )
    .await
    .expect("skills install");
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/skills install ./helper"
            && row.text.contains("unavailable in plan mode")
    }));

    app.handle_fullscreen_command(
        &mut ui,
        SlashCommand::SkillInvoke {
            name: "helper".to_string(),
            args: "apply it to src/lib.rs".to_string(),
        },
    )
    .await
    .expect("skill invoke");
    assert!(ui.running.is_some());
    assert_eq!(textarea_text(&ui.textarea), "");
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Prompt && row.text == "/helper apply it to src/lib.rs"
    }));
    if let Some(running) = ui.running.take() {
        running.control.abort();
        if let RunningTask::Agent(task) = running.task {
            let _ = task.await;
        }
    }
}

#[tokio::test]
pub(crate) async fn enter_on_dynamic_slash_menu_item_submits_without_skill_marker_rewrite() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    fs::create_dir_all(app.workdir.join(".git")).expect("git marker");
    let skill_dir = app.home.join("skills").join("helper");
    fs::create_dir_all(&skill_dir).expect("skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: helper\ndescription: Helps with focused edits\n---\n\nFollow the helper workflow.\n",
    )
    .expect("skill");
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/helper");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("enter");

    assert_eq!(ui.history.last().map(String::as_str), Some("/helper"));
    assert_eq!(
        textarea_text(&ui.textarea),
        "",
        "dynamic slash selection must not leave a $skill marker in the composer"
    );
    assert!(ui.running.is_some());
    assert!(
        ui.transcript
            .iter()
            .any(|row| { row.kind == TranscriptKind::Prompt && row.text == "/helper" })
    );
    assert!(
        ui.transcript
            .iter()
            .all(|row| !(row.kind == TranscriptKind::Prompt && row.text.starts_with("$helper")))
    );
    if let Some(running) = ui.running.take() {
        running.control.abort();
        if let RunningTask::Agent(task) = running.task {
            let _ = task.await;
        }
    }
}

pub(crate) fn test_context_snapshot() -> ContextSnapshot {
    let mut categories = BTreeMap::new();
    categories.insert(
        "base_policy".to_string(),
        ContextCategory {
            label: "Base policy".to_string(),
            tokens: 10,
            estimated: true,
            status: "estimated".to_string(),
            percent: Some(10.0),
            details: Value::Null,
        },
    );
    categories.insert(
        "history".to_string(),
        ContextCategory {
            label: "History".to_string(),
            tokens: 40,
            estimated: true,
            status: "estimated".to_string(),
            percent: Some(40.0),
            details: serde_json::json!({
                "roles": {"user": {"count": 1, "tokens": 40}},
            }),
        },
    );
    categories.insert(
        "free_space".to_string(),
        ContextCategory {
            label: "Free space".to_string(),
            tokens: 50,
            estimated: true,
            status: "derived".to_string(),
            percent: Some(50.0),
            details: Value::Null,
        },
    );
    ContextSnapshot {
        event_type: "context_snapshot".to_string(),
        scope: ContextScope::LastProviderRequest,
        status: "estimated".to_string(),
        session_id: Some("session".to_string()),
        provider: "mock".to_string(),
        model: "model".to_string(),
        mode: Some("default".to_string()),
        context_limit: Some(100),
        tokenizer: ContextTokenizer {
            encoding: "o200k_base".to_string(),
            source: "fallback".to_string(),
            fallback: true,
        },
        total: ContextTotal {
            tokens: 50,
            estimated_tokens: 50,
            estimated: true,
            source: "estimate".to_string(),
            percent: Some(50.0),
        },
        categories,
        advice: Vec::new(),
    }
}

#[tokio::test]
pub(crate) async fn fullscreen_undo_restores_prompt_and_redo_restores_transcript() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&app.workdir)
            .arg("init")
            .output()
            .expect("git init")
            .status
            .success()
    );
    let file = app.workdir.join("tracked.txt");
    fs::write(&file, "base\n").expect("base");
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session_id = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("session");
    app.current_session = Some(session_id.clone());

    let before_first = test_track_snapshot(&app, &session_id);
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        1,
        "user",
        "first prompt",
        serde_json::json!({
            "role": "user",
            "content": [{"text": "first prompt"}],
            "timestamp_ms": 1
        }),
        Some(serde_json::json!({"undo": {"pre_snapshot": before_first}})),
    );
    fs::write(&file, "after first\n").expect("after first");
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        2,
        "assistant",
        "first answer",
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "first answer"}],
            "timestamp_ms": 2,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
        None,
    );
    let before_second = test_track_snapshot(&app, &session_id);
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        3,
        "user",
        "second prompt",
        serde_json::json!({
            "role": "user",
            "content": [{"text": "second prompt"}],
            "timestamp_ms": 3
        }),
        Some(serde_json::json!({"undo": {"pre_snapshot": before_second}})),
    );
    fs::write(&file, "after second\n").expect("after second");
    insert_tui_message_with_metadata(
        &app.db_path,
        &session_id,
        4,
        "assistant",
        "second answer",
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "second answer"}],
            "timestamp_ms": 4,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
        None,
    );

    let mut ui = FullscreenUi::new(&app);
    app.load_current_session_history(&mut ui)
        .expect("load history");
    ui.textarea = textarea_with_text("/undo");
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("undo");

    assert_eq!(textarea_text(&ui.textarea), "second prompt");
    assert_eq!(fs::read_to_string(&file).expect("file"), "after first\n");
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer && row.text == "first answer")
    );
    assert!(ui.transcript.iter().all(|row| row.text != "second answer"));
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/undo"
            && row.text.contains("prompt restored")
    }));

    ui.textarea = textarea_with_text("/redo");
    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("redo");

    assert_eq!(textarea_text(&ui.textarea), "");
    assert_eq!(fs::read_to_string(&file).expect("file"), "after second\n");
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.kind == TranscriptKind::Answer && row.text == "second answer")
    );
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command && row.title == "/redo" && row.text.contains("redone")
    }));
}

#[tokio::test]
pub(crate) async fn fullscreen_new_command_resets_session_without_status_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    ui.push_user("previous prompt".to_string());

    app.handle_fullscreen_command(&mut ui, SlashCommand::New)
        .await
        .expect("new");

    assert_eq!(app.current_session, None);
    assert_eq!(app.current_session_title, None);
    assert!(app.force_new_once);
    assert!(ui.transcript.is_empty());
    assert!(ui.terminal_clear_requested);
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.kind != TranscriptKind::Status)
    );
}

#[tokio::test]
pub(crate) async fn fullscreen_reload_context_points_to_refresh() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ReloadContextDeprecated)
        .await
        .expect("reload deprecated");

    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Command
            && row.title == "/reload-context"
            && row.failed
            && row.text.contains("use /refresh")
    }));
}
