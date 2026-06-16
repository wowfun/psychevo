#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

#[tokio::test]
pub(crate) async fn agents_command_opens_running_tab_and_available_shows_shadowed_definitions() {
    psychevo_runtime::set_agent_spawn_paused(false);
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    write_tui_agent(
        &app,
        "general",
        "Project general agent used by the panel test",
    );
    write_home_tui_agent(&app, "translator", "Global translator agent");
    let mut ui = FullscreenUi::new(&app);
    ui.textarea = textarea_with_text("/agents");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("open agents");

    let Some(BottomPanel::Agents(panel)) = &ui.bottom_panel else {
        panic!("agents panel");
    };
    assert_eq!(panel.tab, AgentTab::Running);
    assert!(matches!(
        panel.running.rows.first().map(|row| &row.value),
        Some(BottomSelectionValue::AgentSpawningToggle)
    ));
    assert!(
        panel
            .running
            .rows
            .iter()
            .any(|row| row.label == "No running subagents")
    );
    assert_eq!(
        panel.running.footer,
        "P pause/resume  Esc close  Tab available"
    );

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('P'), KeyModifiers::NONE),
    )
    .await
    .expect("pause spawning");
    let Some(BottomPanel::Agents(panel)) = &ui.bottom_panel else {
        panic!("agents panel");
    };
    assert_eq!(panel.running.rows[0].label, "Resume spawning");
    assert_eq!(
        panel.running.notice.as_deref(),
        Some("new agent spawns paused")
    );
    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('P'), KeyModifiers::NONE),
    )
    .await
    .expect("resume spawning");
    assert!(!psychevo_runtime::agent_spawn_paused());

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .await
        .expect("tab available");

    let Some(BottomPanel::Agents(panel)) = &ui.bottom_panel else {
        panic!("agents panel");
    };
    assert_eq!(panel.tab, AgentTab::Available);
    let active_general = panel
        .available
        .rows
        .iter()
        .find(|row| {
            row.label == "general"
                && matches!(
                    &row.value,
                    BottomSelectionValue::AgentAvailable {
                        source: AgentSource::Project,
                        shadowed: false,
                        ..
                    }
                )
        })
        .expect("active project general");
    assert_eq!(
        active_general.detail.as_deref(),
        Some("Active project editable  subagent  depth 0")
    );
    assert!(!active_general.is_default);

    let shadowed_general = panel
        .available
        .rows
        .iter()
        .find(|row| {
            row.label == "general"
                && matches!(
                    &row.value,
                    BottomSelectionValue::AgentAvailable {
                        source: AgentSource::BuiltIn,
                        shadowed: true,
                        ..
                    }
                )
        })
        .expect("shadowed built-in general");
    assert_eq!(
        shadowed_general.detail.as_deref(),
        Some("Shadowed built-in read-only  subagent  depth 0")
    );
    assert!(!shadowed_general.is_default);
}

#[test]
pub(crate) fn available_agent_actions_expose_editable_psychevo_controls_and_run_prompt() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    write_tui_agent(&app, "general", "Project general agent");
    let mut ui = FullscreenUi::new(&app);
    ui.bottom_panel = Some(BottomPanel::Agents(app.agent_panel()));
    if let Some(BottomPanel::Agents(panel)) = &mut ui.bottom_panel {
        panel.tab = AgentTab::Available;
        let index = panel
            .available
            .rows
            .iter()
            .position(|row| {
                row.label == "general"
                    && matches!(
                        &row.value,
                        BottomSelectionValue::AgentAvailable {
                            source: AgentSource::Project,
                            shadowed: false,
                            ..
                        }
                    )
            })
            .expect("general row");
        panel.available.set_selected(index);
    }

    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("open actions");

    let Some(BottomPanel::AgentActions(panel)) = &ui.bottom_panel else {
        panic!("agent actions");
    };
    let labels = panel
        .rows
        .iter()
        .map(|row| row.label.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        labels,
        vec!["Use as main", "Run", "View", "Update", "Delete"]
    );
    assert!(panel.rows.iter().all(|row| !row.is_default));

    if let Some(BottomPanel::AgentActions(panel)) = &mut ui.bottom_panel {
        let index = panel
            .rows
            .iter()
            .position(|row| row.label == "Run")
            .expect("run action");
        panel.set_selected(index);
    }
    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("open run prompt");

    let Some(BottomPanel::AgentRunPrompt(panel)) = &ui.bottom_panel else {
        panic!("run prompt");
    };
    assert_eq!(panel.agent_name, "general");
    assert!(panel.prompt.is_empty());
}

#[test]
pub(crate) fn available_agent_use_as_main_is_session_scoped_and_clearable() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    write_tui_agent(&app, "translate", "Translate user messages");
    let store = SqliteStore::open(&app.db_path).expect("store");
    let session = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("session");
    app.current_session = Some(session.clone());
    let mut ui = FullscreenUi::new(&app);
    ui.bottom_panel = Some(BottomPanel::Agents(app.agent_panel()));
    if let Some(BottomPanel::Agents(panel)) = &mut ui.bottom_panel {
        panel.tab = AgentTab::Available;
        let index = panel
            .available
            .rows
            .iter()
            .position(|row| row.label == "translate")
            .expect("translate row");
        panel.available.set_selected(index);
    }

    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("open actions");
    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("use as main");

    assert_eq!(app.current_agent.as_deref(), Some("translate"));
    assert!(!app.current_agent_explicit_default);
    assert_eq!(
        app.run_options("next".to_string()).agent.as_deref(),
        Some("translate")
    );
    let metadata = store
        .session_metadata(&session)
        .expect("metadata")
        .expect("metadata value");
    assert_eq!(metadata["main_agent"]["mode"], "agent");
    assert_eq!(metadata["main_agent"]["name"], "translate");
    assert!(app.status_text().contains("agent: translate"));
    let status = bottom_status_context_for_width(&app, &ui, 80).expect("status");
    assert!(!status.contains("translate"), "{status}");
    assert_eq!(app.session_identity_label().as_deref(), Some("translate"));
    assert!(ui.bottom_panel.is_none());
    assert!(!app.workdir.join(".psychevo/config.toml").exists());

    let panel = app.agent_panel();
    let translate = panel
        .available
        .rows
        .iter()
        .find(|row| row.label == "translate")
        .expect("translate row");
    assert!(
        translate
            .detail
            .as_deref()
            .unwrap_or_default()
            .starts_with("Current main"),
        "{translate:?}"
    );
    ui.bottom_panel = Some(BottomPanel::Agents(app.agent_panel()));
    let Some(BottomPanel::Agents(panel)) = &mut ui.bottom_panel else {
        panic!("agents panel");
    };
    panel.tab = AgentTab::Available;
    panel.available.select_value_key("agent:main-default");
    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("use default main");

    assert_eq!(app.current_agent, None);
    assert!(app.current_agent_explicit_default);
    assert_eq!(app.session_identity_label(), None);
    assert!(ui.bottom_panel.is_none());
    let metadata = store
        .session_metadata(&session)
        .expect("metadata")
        .expect("metadata value");
    assert_eq!(metadata["main_agent"]["mode"], "default");
    assert!(!app.status_text().contains("agent: translate"));
    assert!(!app.workdir.join(".psychevo/config.toml").exists());
}

#[test]
pub(crate) fn main_agent_identity_renders_in_separator_not_status_line() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    write_tui_agent(&app, "translate", "Translate user messages");
    app.current_agent = Some("translate".to_string());
    let mut ui = FullscreenUi::new(&app);
    ui.sidebar.branch = "main".to_string();
    ui.sidebar_tokens = Some(2_800);
    ui.sidebar_context_limit = Some(1_000_000);

    let status = bottom_status_context_for_width(&app, &ui, 100).expect("status");
    assert_eq!(status, "2.8k/1.0M (0.3%) · ~/work · main");

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 12);
    let text = buffer_text(&buffer);
    assert!(text.contains(" translate "), "{text}");

    app.current_agent = None;
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 12);
    let text = buffer_text(&buffer);
    assert!(!text.contains(" translate "), "{text}");
}

#[test]
pub(crate) fn child_session_identity_separator_uses_child_agent_default() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    write_tui_agent(&app, "translate", "Translate user messages");
    write_tui_agent(&app, "review", "Review code changes");
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("parent");
    let child = store
        .create_child_session_with_metadata(
            &parent,
            &app.workdir,
            "agent",
            "mock-model",
            "mock",
            Some(serde_json::json!({
                "agent": {
                    "name": "translate",
                    "parent_session_id": parent,
                    "role": "child"
                }
            })),
        )
        .expect("child");
    app.current_session = Some(child.clone());
    app.refresh_current_session_agent()
        .expect("child agent restore");
    assert!(app.current_agent_explicit_default);
    assert_eq!(app.session_identity_label().as_deref(), Some("translate"));

    store
        .set_session_metadata_field(
            &child,
            SESSION_MAIN_AGENT_METADATA_KEY,
            Some(main_agent_metadata(
                "review",
                "review",
                AgentSource::Project,
                Some(&app.workdir.join(".psychevo/agents/review.md")),
            )),
        )
        .expect("child review main");
    app.refresh_current_session_agent()
        .expect("child review restore");
    assert_eq!(app.session_identity_label().as_deref(), Some("review"));

    store
        .set_session_metadata_field(
            &child,
            SESSION_MAIN_AGENT_METADATA_KEY,
            Some(main_agent_default_metadata()),
        )
        .expect("child default main");
    app.refresh_current_session_agent()
        .expect("child default restore");
    assert_eq!(app.current_agent.as_deref(), Some("translate"));
    assert!(app.current_agent_explicit_default);
    assert_eq!(app.session_identity_label().as_deref(), Some("translate"));

    let mut ui = FullscreenUi::new(&app);
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 12);
    let text = buffer_text(&buffer);
    assert!(text.contains(" translate "), "{text}");
}

#[test]
pub(crate) fn session_switch_restores_each_sessions_main_agent() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.startup_agent = Some("general".to_string());
    write_tui_agent(&app, "translate", "Translate user messages");
    let store = SqliteStore::open(&app.db_path).expect("store");
    let first = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("first");
    let second = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("second");
    store
        .set_session_metadata_field(
            &first,
            SESSION_MAIN_AGENT_METADATA_KEY,
            Some(main_agent_metadata(
                "translate",
                "translate",
                AgentSource::Project,
                Some(&app.workdir.join(".psychevo/agents/translate.md")),
            )),
        )
        .expect("first main");
    store
        .set_session_metadata_field(
            &second,
            SESSION_MAIN_AGENT_METADATA_KEY,
            Some(main_agent_default_metadata()),
        )
        .expect("second default");

    app.current_session = Some(first);
    app.refresh_current_session_agent().expect("first restore");
    assert_eq!(app.current_agent.as_deref(), Some("translate"));

    app.switch_session_no_print(&second).expect("switch second");
    assert_eq!(app.current_agent, None);
    assert!(app.current_agent_explicit_default);

    let third = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("third");
    app.switch_session_no_print(&third).expect("switch third");
    assert_eq!(app.current_agent.as_deref(), Some("general"));
    assert!(!app.current_agent_explicit_default);
}

#[tokio::test]
pub(crate) async fn running_turn_blocks_main_agent_switching() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    write_tui_agent(&app, "translate", "Translate user messages");
    let session = SqliteStore::open(&app.db_path)
        .expect("store")
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("session");
    app.current_session = Some(session);
    let mut ui = FullscreenUi::new(&app);
    ui.bottom_panel = Some(BottomPanel::Agents(app.agent_panel()));
    if let Some(BottomPanel::Agents(panel)) = &mut ui.bottom_panel {
        panel.tab = AgentTab::Available;
        let index = panel
            .available
            .rows
            .iter()
            .position(|row| row.label == "translate")
            .expect("translate row");
        panel.available.set_selected(index);
    }
    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("open actions");

    let (_tx, rx) = mpsc::unbounded_channel();
    let result = finished_run_result(&app);
    let task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
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
    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("blocked use as main");

    assert_eq!(app.current_agent, None);
    let Some(BottomPanel::AgentActions(panel)) = &ui.bottom_panel else {
        panic!("agent actions");
    };
    assert_eq!(
        panel.notice.as_deref(),
        Some("finish the current turn before switching main agent")
    );
    if let Some(running) = ui.running.take() {
        running.task.abort();
    }
}

#[test]
pub(crate) fn available_agents_surface_definition_diagnostics() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let dir = app.workdir.join(".psychevo/agents");
    std::fs::create_dir_all(&dir).expect("agent dir");
    std::fs::write(dir.join("broken.md"), "---\ndescription: broken\n").expect("broken agent");

    let panel = app.agent_panel();
    let diagnostic = panel
        .available
        .rows
        .iter()
        .find(|row| matches!(row.value, BottomSelectionValue::AgentDiagnostic(_)))
        .expect("diagnostic row");

    assert_eq!(diagnostic.group.as_deref(), Some("Diagnostics"));
    assert!(
        diagnostic
            .description
            .as_deref()
            .unwrap_or_default()
            .contains("failed to load agent")
    );
}

#[test]
pub(crate) fn agent_editor_writes_max_spawn_depth_frontmatter() {
    let mut panel = AgentEditorPanel::create();
    panel.name = "translate".to_string();
    panel.description = "Translate messages".to_string();
    panel.instructions = "Translate carefully.".to_string();
    panel.max_spawn_depth = "1".to_string();

    let markdown = agent_editor_markdown(&panel);

    assert!(markdown.contains("maxSpawnDepth: 1"));
}

#[test]
pub(crate) fn child_status_line_keeps_compact_parent_hint() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("parent");
    let child = store
        .create_child_session_with_metadata(
            &parent,
            &app.workdir,
            "agent",
            "mock-model",
            "mock",
            None,
        )
        .expect("child");
    store
        .upsert_agent_edge(
            &parent,
            &child,
            psychevo_runtime::AgentEdgeStatus::Open,
            None,
        )
        .expect("edge");
    app.current_session = Some(child);
    let ui = FullscreenUi::new(&app);

    let status = bottom_status_context_for_width(&app, &ui, 52).expect("status");

    assert!(status.contains("parent "), "{status}");
    assert!(status.contains("Alt+P"), "{status}");
}

#[test]
pub(crate) fn hidden_agent_notifications_do_not_render_as_parent_rows() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let message = serde_json::json!({
        "role": "user",
        "content": [{"type": "text", "text": "Agent `translate` completed."}]
    });
    let metadata = serde_json::json!({
        "agent_notification": {
            "type": "agent_completed",
            "agent_name": "translate",
            "child_session_id": "019e33e0-a38c-72a0-8320-9d13124af9d8",
            "summary": "translated text",
            "hidden": true
        }
    });

    ui.push_history_message(&message, None, Some(&metadata));

    assert!(ui.transcript.is_empty());
}

#[test]
pub(crate) fn visible_agent_notifications_keep_one_clickable_parent_status_row() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let message = serde_json::json!({
        "role": "user",
        "content": [{"type": "text", "text": "Agent `translate` completed."}]
    });
    let metadata = serde_json::json!({
        "agent_notification": {
            "type": "agent_completed",
            "agent_name": "translate",
            "child_session_id": "019e33e0-a38c-72a0-8320-9d13124af9d8",
            "outcome": "normal",
            "summary": "translated text",
            "hidden": false
        }
    });

    ui.push_history_message(&message, None, Some(&metadata));

    assert_eq!(ui.transcript.len(), 1);
    assert_eq!(ui.transcript[0].title, "Agent");
    assert_eq!(
        ui.transcript[0].agent_target.as_deref(),
        Some("019e33e0-a38c-72a0-8320-9d13124af9d8")
    );
    assert!(ui.transcript[0].text.contains("translated text"));
}

#[test]
pub(crate) fn history_agent_edge_reconcile_does_not_make_failed_rows_openable() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let mut failed_zh =
        TranscriptRow::with_title(TranscriptKind::Ran, "translate(zh-to-en)", "Failed");
    failed_zh.tool_name = Some("spawn_agent".to_string());
    failed_zh.failed = true;
    let mut failed_en =
        TranscriptRow::with_title(TranscriptKind::Ran, "translate(en-to-zh)", "Failed");
    failed_en.tool_name = Some("spawn_agent".to_string());
    failed_en.failed = true;
    let mut success_zh =
        TranscriptRow::with_title(TranscriptKind::Ran, "translate(zh-to-en)", "Started");
    success_zh.tool_name = Some("spawn_agent".to_string());
    let mut success_en =
        TranscriptRow::with_title(TranscriptKind::Ran, "translate(en-to-zh)", "Started");
    success_en.tool_name = Some("spawn_agent".to_string());
    ui.transcript = vec![failed_zh, failed_en, success_zh, success_en];
    let edges = vec![
        psychevo_runtime::AgentEdgeRecord {
            parent_session_id: "parent".to_string(),
            child_session_id: "child-zh".to_string(),
            status: psychevo_runtime::AgentEdgeStatus::Closed,
            created_at_ms: 1,
            updated_at_ms: 2,
            metadata: Some(serde_json::json!({
                "agent": {
                    "id": "agent-zh",
                    "name": "translate",
                    "task_name": "zh-to-en"
                }
            })),
        },
        psychevo_runtime::AgentEdgeRecord {
            parent_session_id: "parent".to_string(),
            child_session_id: "child-en".to_string(),
            status: psychevo_runtime::AgentEdgeStatus::Closed,
            created_at_ms: 1,
            updated_at_ms: 2,
            metadata: Some(serde_json::json!({
                "agent": {
                    "id": "agent-en",
                    "name": "translate",
                    "task_name": "en-to-zh"
                }
            })),
        },
    ];

    ui.reconcile_history_agent_rows(&edges, None);

    assert!(ui.transcript[0].agent_target.is_none());
    assert!(ui.transcript[1].agent_target.is_none());
    assert_eq!(ui.transcript[2].agent_target.as_deref(), Some("child-zh"));
    assert_eq!(ui.transcript[3].agent_target.as_deref(), Some("child-en"));
}

#[tokio::test]
pub(crate) async fn foreground_agent_tool_result_is_single_claude_style_inspection_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_name": "spawn_agent",
            "tool_call_id": "agent-1",
            "args": {
                "agent_type": "translate",
                "task_name": "translate_to_chinese",
                "message": "Translate the following message to Chinese: hello"
            }
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "agent_session_start",
            "tool_call_id": "agent-1",
            "agent_id": "agent-run-1",
            "agent_name": "translate",
            "agent_type": "translate",
            "task_name": "translate_to_chinese",
            "agent_description": "Translate user message to Chinese",
            "message": "Translate the following message to Chinese: hello",
            "child_session_id": "019e33e0-a38c-72a0-8320-9d13124af9d8"
        }),
        false,
    );
    assert_eq!(
        ui.transcript[0].agent_target.as_deref(),
        Some("019e33e0-a38c-72a0-8320-9d13124af9d8")
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_name": "spawn_agent",
            "tool_call_id": "agent-1",
            "outcome": "normal",
            "elapsed_ms": 1000,
            "result": {
                "id": "agent-run-1",
                "agent_name": "translate",
                "agent_type": "translate",
                "agent_description": "Translate user message to Chinese",
                "task_name": "translate_to_chinese",
                "message": "Translate the following message to Chinese: hello",
                "status": "completed",
                "background": false,
                "child_session_id": "019e33e0-a38c-72a0-8320-9d13124af9d8",
                "outcome": "normal",
                "final_answer": "你好",
                "child_session": {"tool_call_count": 0, "latest_total_tokens": 14500}
            }
        }),
        false,
    );
    let hidden_notification = serde_json::json!({
        "agent_notification": {
            "type": "agent_completed",
            "agent_name": "translate",
            "child_session_id": "019e33e0-a38c-72a0-8320-9d13124af9d8",
            "summary": "你好",
            "hidden": true
        }
    });
    ui.push_history_message(
        &serde_json::json!({
            "role": "user",
            "content": [{"type": "text", "text": "Agent `translate` completed."}]
        }),
        None,
        Some(&hidden_notification),
    );

    assert_eq!(ui.transcript.len(), 1);
    let row = &ui.transcript[0];
    assert_eq!(row.title, "translate(translate_to_chinese)");
    assert_eq!(row.text, "Done (0 tool uses · 14.5k tokens)");
    assert_eq!(row.tool_name.as_deref(), Some("spawn_agent"));
    assert_eq!(
        row.agent_target.as_deref(),
        Some("019e33e0-a38c-72a0-8320-9d13124af9d8")
    );
    let full = row.full_text.as_deref().expect("expanded agent details");
    assert!(full.contains("Prompt:\nTranslate the following message"));
    assert!(full.contains("Response:\n你好"));

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 20);
    let text = buffer_text(&buffer);
    assert!(text.contains("translate(translate_to_chinese)"), "{text}");
    assert!(text.contains("Done (0 tool uses · 14.5k tokens)"), "{text}");

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL),
    )
    .await
    .expect("focus transcript");
    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
    )
    .await
    .expect("expand agent row");
    assert!(ui.transcript[0].expanded);
}

#[test]
pub(crate) fn agent_session_start_reuses_position_bound_partial_agent_placeholder_row() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "arguments_json": "{\"agent_type\":\"general\",\"task_name\":\"fetch_failed_articles\",\"message\":\"Fetch the failed articles and comments.\"}",
            "content_index": 0,
            "call_index": 0
        }),
        false,
    );
    assert_eq!(ui.transcript.len(), 1);
    assert_eq!(ui.transcript[0].title, "general(fetch_failed_articles)");
    assert!(ui.transcript[0].agent_target.is_none());

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_name": "spawn_agent",
            "tool_call_id": "agent-1",
            "content_index": 0,
            "call_index": 0,
            "args": {
                "agent_type": "general",
                "task_name": "fetch_failed_articles",
                "message": "Fetch the failed articles and comments."
            }
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "agent_session_start",
            "tool_call_id": "agent-1",
            "agent_id": "agent-run-1",
            "agent_name": "general",
            "agent_type": "general",
            "task_name": "fetch_failed_articles",
            "agent_description": "General-purpose subagent for focused coding tasks.",
            "message": "Fetch the failed articles and comments.",
            "child_session_id": "019e33e0-a38c-72a0-8320-9d13124af9d8"
        }),
        false,
    );

    assert_eq!(ui.transcript.len(), 1);
    let row = &ui.transcript[0];
    assert_eq!(row.title, "general(fetch_failed_articles)");
    assert_eq!(
        row.agent_target.as_deref(),
        Some("019e33e0-a38c-72a0-8320-9d13124af9d8")
    );
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 120, 12);
    let text = buffer_text(&buffer);
    assert!(text.contains("Open"), "{text}");
}

#[test]
pub(crate) fn running_agent_tool_row_keeps_original_prompt_detail() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_name": "spawn_agent",
            "tool_call_id": "agent-1",
            "args": {
                "agent_type": "translate",
                "task_name": "translate_to_chinese",
                "message": "Translate the following message to Chinese: hello"
            }
        }),
        false,
    );

    assert_eq!(ui.transcript.len(), 1);
    let row = &ui.transcript[0];
    assert_eq!(row.tool_name.as_deref(), Some("spawn_agent"));
    assert_eq!(row.text, "Running (0 tool uses)");
    let full = row.full_text.as_deref().expect("running agent detail");
    assert!(full.contains("Prompt:\nTranslate the following message to Chinese: hello"));
}

#[test]
pub(crate) fn completed_agent_tool_row_uses_cached_prompt_when_result_omits_task() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_name": "spawn_agent",
            "tool_call_id": "agent-1",
            "args": {
                "agent_type": "translate",
                "task_name": "Translate user message to Chinese",
                "prompt": "Translate the following message to Chinese: hello"
            }
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_name": "spawn_agent",
            "tool_call_id": "agent-1",
            "outcome": "normal",
            "elapsed_ms": 1000,
            "result": {
                "agent_name": "translate",
                "status": "completed",
                "child_session_id": "child-thread",
                "summary": "你好"
            }
        }),
        false,
    );

    let row = &ui.transcript[0];
    assert_eq!(row.tool_name.as_deref(), Some("spawn_agent"));
    assert_eq!(row.agent_target.as_deref(), Some("child-thread"));
    let full = row.full_text.as_deref().expect("completed agent detail");
    assert!(full.contains("Prompt:\nTranslate the following message to Chinese: hello"));
    assert!(full.contains("Response:\n你好"));
}
