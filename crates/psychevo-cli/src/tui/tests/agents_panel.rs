#[tokio::test]
async fn agents_command_opens_running_tab_and_available_shows_shadowed_definitions() {
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
        Some("Active project editable  depth 0")
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
        Some("Shadowed built-in read-only  depth 0")
    );
    assert!(!shadowed_general.is_default);
}

#[test]
fn available_agent_actions_expose_editable_psychevo_controls_and_run_prompt() {
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
fn available_agent_use_as_main_is_session_scoped_and_clearable() {
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
}

#[test]
fn main_agent_identity_renders_in_separator_not_status_line() {
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
fn child_session_identity_separator_uses_child_agent_default() {
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
fn session_switch_restores_each_sessions_main_agent() {
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
async fn running_turn_blocks_main_agent_switching() {
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
        rx,
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
fn available_agents_surface_definition_diagnostics() {
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
fn agent_editor_writes_max_spawn_depth_frontmatter() {
    let mut panel = AgentEditorPanel::create();
    panel.name = "translate".to_string();
    panel.description = "Translate messages".to_string();
    panel.instructions = "Translate carefully.".to_string();
    panel.max_spawn_depth = "1".to_string();

    let markdown = agent_editor_markdown(&panel);

    assert!(markdown.contains("maxSpawnDepth: 1"));
}

#[test]
fn child_status_line_keeps_compact_parent_hint() {
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
fn hidden_agent_notifications_do_not_render_as_parent_rows() {
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
fn visible_agent_notifications_keep_one_clickable_parent_status_row() {
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

#[tokio::test]
async fn foreground_agent_tool_result_is_single_claude_style_inspection_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_name": "Agent",
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
            "type": "agent_session_start",
            "tool_call_id": "agent-1",
            "agent_id": "agent-run-1",
            "agent_name": "translate",
            "agent_description": "Translate user message to Chinese",
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
            "tool_name": "Agent",
            "tool_call_id": "agent-1",
            "outcome": "normal",
            "elapsed_ms": 1000,
            "result": {
                "id": "agent-run-1",
                "agent_name": "translate",
                "agent_description": "Translate user message to Chinese",
                "task_name": "translate-019e33e0",
                "task": "Translate the following message to Chinese: hello",
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
    assert_eq!(row.title, "translate(Translate user message to Chinese)");
    assert_eq!(row.text, "Done (0 tool uses · 14.5k tokens)");
    assert_eq!(row.tool_name.as_deref(), Some("Agent"));
    assert_eq!(
        row.agent_target.as_deref(),
        Some("019e33e0-a38c-72a0-8320-9d13124af9d8")
    );
    let full = row.full_text.as_deref().expect("expanded agent details");
    assert!(full.contains("Prompt:\nTranslate the following message"));
    assert!(full.contains("Response:\n你好"));

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 20);
    let text = buffer_text(&buffer);
    assert!(
        text.contains("translate(Translate user message to Chinese)"),
        "{text}"
    );
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
fn agent_session_start_reuses_partial_agent_placeholder_row() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "Agent",
            "arguments_json": "{\"name\":\"general\"}",
            "content_index": 0,
            "call_index": 0
        }),
        false,
    );
    assert_eq!(ui.transcript.len(), 1);
    assert_eq!(ui.transcript[0].title, "general");
    assert!(ui.transcript[0].agent_target.is_none());

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_name": "Agent",
            "tool_call_id": "agent-1",
            "args": {
                "name": "general",
                "prompt": "Fetch the failed articles and comments."
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
            "agent_description": "General-purpose subagent for focused coding tasks.",
            "child_session_id": "019e33e0-a38c-72a0-8320-9d13124af9d8"
        }),
        false,
    );

    assert_eq!(ui.transcript.len(), 1);
    let row = &ui.transcript[0];
    assert_eq!(
        row.title,
        "general(General-purpose subagent for focused coding tasks.)"
    );
    assert_eq!(
        row.agent_target.as_deref(),
        Some("019e33e0-a38c-72a0-8320-9d13124af9d8")
    );
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 120, 12);
    let text = buffer_text(&buffer);
    assert!(text.contains("Open"), "{text}");
}

#[test]
fn loading_parent_history_restores_running_agent_open_from_edge() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    write_tui_agent(
        &app,
        "general",
        "General-purpose subagent for focused coding tasks.",
    );
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("parent session");
    let child = store
        .create_child_session_with_metadata(
            &parent,
            &app.workdir,
            "agent",
            "mock-model",
            "mock",
            None,
        )
        .expect("child session");
    store
        .upsert_agent_edge(
            &parent,
            &child,
            psychevo_runtime::AgentEdgeStatus::Open,
            Some(serde_json::json!({
                "agent": {
                    "id": "agent-run-1",
                    "task_name": "general-task",
                    "name": "general",
                    "task": "Fetch the failed articles and comments."
                }
            })),
        )
        .expect("agent edge");
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_tui_message(
        &conn,
        &parent,
        1,
        "user",
        1,
        serde_json::json!({
            "role": "user",
            "content": [{"type": "text", "text": "Fetch failed articles"}],
            "timestamp_ms": 1
        }),
    );
    insert_tui_message(
        &conn,
        &parent,
        2,
        "assistant",
        2,
        serde_json::json!({
            "role": "assistant",
            "content": [{
                "type": "tool_call",
                "id": "agent-1",
                "name": "Agent",
                "arguments": {
                    "name": "general",
                    "prompt": "Fetch the failed articles and comments."
                },
                "arguments_json": "{\"name\":\"general\",\"prompt\":\"Fetch the failed articles and comments.\"}",
                "content_index": 0,
                "call_index": 0
            }],
            "timestamp_ms": 2,
            "finish_reason": "tool_calls",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
    );
    app.current_session = Some(parent);
    let mut ui = FullscreenUi::new(&app);

    app.load_current_session_history(&mut ui)
        .expect("load parent history");

    let agent_rows = ui
        .transcript
        .iter()
        .filter(|row| row.tool_name.as_deref() == Some("Agent"))
        .collect::<Vec<_>>();
    assert_eq!(agent_rows.len(), 1);
    let row = agent_rows[0];
    assert_eq!(
        row.title,
        "general(General-purpose subagent for focused coding tasks.)"
    );
    assert_eq!(row.agent_target.as_deref(), Some(child.as_str()));
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 120, 12);
    let text = buffer_text(&buffer);
    assert!(text.contains("Open"), "{text}");
}

#[tokio::test]
async fn running_agent_row_enter_opens_child_session_before_parent_turn_finishes() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("parent session");
    let child = store
        .create_child_session_with_metadata(
            &parent,
            &app.workdir,
            "agent",
            "mock-model",
            "mock",
            None,
        )
        .expect("child session");
    store
        .upsert_agent_edge(
            &parent,
            &child,
            psychevo_runtime::AgentEdgeStatus::Open,
            Some(serde_json::json!({
                "agent": {
                    "id": "agent-run-1",
                    "task_name": "translate-1",
                    "name": "translate",
                    "task": "translate text"
                }
            })),
        )
        .expect("agent edge");
    let child_prompt_ms = wall_now_ms() - 12_500;
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    insert_tui_message(
        &conn,
        &child,
        1,
        "user",
        child_prompt_ms,
        serde_json::json!({
            "role": "user",
            "content": [{"type": "text", "text": "translate text"}],
            "timestamp_ms": child_prompt_ms
        }),
    );
    app.current_session = Some(parent.clone());
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Status,
        "translate(Translate user message to Chinese)",
        "running",
    );
    row.tool_name = Some("Agent".to_string());
    row.agent_target = Some(child.clone());
    row.tool_started = Some(Instant::now());
    ui.transcript.push(row);
    ui.focus = FocusMode::Transcript;
    ui.ensure_selection();

    let (_tx, rx) = mpsc::unbounded_channel();
    let result = psychevo_runtime::RunResult {
        session_id: parent.clone(),
        ..finished_run_result(&app)
    };
    let task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        Ok(result)
    });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        session_id: None,
        control,
        rx,
        task: RunningTask::Agent(task),
    });
    ui.start_assistant();
    ui.visible_turn_started = Some(
        Instant::now()
            .checked_sub(Duration::from_secs(140))
            .expect("instant"),
    );

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("open running child session");

    assert_eq!(app.current_session.as_deref(), Some(child.as_str()));
    assert!(ui.running.is_none());
    assert_eq!(ui.auxiliary_agent_tasks.len(), 1);
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 12);
    let text = buffer_text(&buffer);
    assert!(text.contains("s · Esc"), "{text}");
    assert!(!text.contains("2m20s · Esc"), "{text}");
    assert!(
        ui.transcript
            .iter()
            .all(|row| row.text != "finish the current turn before opening an agent session")
    );
}

#[tokio::test]
async fn esc_interrupts_running_child_session_after_open() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("parent session");
    let child = store
        .create_child_session_with_metadata(
            &parent,
            &app.workdir,
            "agent",
            "mock-model",
            "mock",
            None,
        )
        .expect("child session");
    store
        .upsert_agent_edge(
            &parent,
            &child,
            psychevo_runtime::AgentEdgeStatus::Open,
            None,
        )
        .expect("agent edge");
    app.current_session = Some(parent.clone());
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Status,
        "translate(Translate user message to Chinese)",
        "running",
    );
    row.tool_name = Some("Agent".to_string());
    row.agent_target = Some(child.clone());
    row.tool_started = Some(Instant::now());
    ui.transcript.push(row);
    ui.focus = FocusMode::Transcript;
    ui.ensure_selection();

    let (_tx, rx) = mpsc::unbounded_channel();
    let result = psychevo_runtime::RunResult {
        session_id: parent.clone(),
        ..finished_run_result(&app)
    };
    let task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        Ok(result)
    });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        session_id: None,
        control,
        rx,
        task: RunningTask::Agent(task),
    });

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("open running child session");
    assert_eq!(app.current_session.as_deref(), Some(child.as_str()));
    assert!(ui.running.is_none());
    assert_eq!(ui.auxiliary_agent_tasks.len(), 1);

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("interrupt child session");

    assert!(ui.interrupt_requested);
    for agent in &ui.auxiliary_agent_tasks {
        agent.task.abort();
    }
}

#[tokio::test]
async fn esc_interrupts_running_child_from_parent_session_after_return() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("parent session");
    let child = store
        .create_child_session_with_metadata(
            &parent,
            &app.workdir,
            "agent",
            "mock-model",
            "mock",
            None,
        )
        .expect("child session");
    store
        .upsert_agent_edge(
            &parent,
            &child,
            psychevo_runtime::AgentEdgeStatus::Open,
            None,
        )
        .expect("agent edge");
    let conn = rusqlite::Connection::open(&app.db_path).expect("conn");
    let parent_prompt_ms = wall_now_ms() - 22_500;
    insert_tui_message(
        &conn,
        &parent,
        1,
        "user",
        parent_prompt_ms,
        serde_json::json!({
            "role": "user",
            "content": [{"type": "text", "text": "delegate this"}],
            "timestamp_ms": parent_prompt_ms
        }),
    );
    insert_tui_message(
        &conn,
        &parent,
        2,
        "assistant",
        parent_prompt_ms + 1_000,
        serde_json::json!({
            "role": "assistant",
            "content": [{
                "type": "tool_call",
                "id": "call_agent",
                "name": "Agent",
                "arguments": {
                    "agent": "translate",
                    "task": "translate text"
                },
                "arguments_json": "{\"agent\":\"translate\",\"task\":\"translate text\"}",
                "arguments_error": null,
                "content_index": 0,
                "call_index": 0
            }],
            "timestamp_ms": parent_prompt_ms + 1_000,
            "finish_reason": "tool_calls",
            "outcome": "normal",
            "model": "mock-model",
            "provider": "mock"
        }),
    );
    let child_prompt_ms = wall_now_ms() - 6_500;
    insert_tui_message(
        &conn,
        &child,
        1,
        "user",
        child_prompt_ms,
        serde_json::json!({
            "role": "user",
            "content": [{"type": "text", "text": "translate text"}],
            "timestamp_ms": child_prompt_ms
        }),
    );
    app.current_session = Some(parent.clone());
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Status,
        "translate(Translate user message to Chinese)",
        "running",
    );
    row.tool_name = Some("Agent".to_string());
    row.agent_target = Some(child.clone());
    row.tool_started = Some(Instant::now());
    ui.transcript.push(row);
    ui.focus = FocusMode::Transcript;
    ui.ensure_selection();

    let (_tx, rx) = mpsc::unbounded_channel();
    let result = psychevo_runtime::RunResult {
        session_id: parent.clone(),
        ..finished_run_result(&app)
    };
    let task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        Ok(result)
    });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        session_id: None,
        control,
        rx,
        task: RunningTask::Agent(task),
    });

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("open running child session");
    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('p'), KeyModifiers::ALT),
    )
    .await
    .expect("return to parent");
    assert_eq!(app.current_session.as_deref(), Some(parent.as_str()));
    assert!(ui.running.is_none());
    assert_eq!(ui.auxiliary_agent_tasks.len(), 1);
    let buffer = draw_fullscreen_for_test(&app, &mut ui, 100, 12);
    let text = buffer_text(&buffer);
    assert!(text.contains("22s · Esc"), "{text}");
    assert!(!text.contains("6s · Esc"), "{text}");

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("interrupt child from parent session");

    assert!(ui.interrupt_requested);
    for agent in &ui.auxiliary_agent_tasks {
        agent.task.abort();
    }
}

#[tokio::test]
async fn agent_row_click_toggles_and_open_action_enters_child_session() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("parent session");
    let child = store
        .create_child_session_with_metadata(
            &parent,
            &app.workdir,
            "agent",
            "mock-model",
            "mock",
            None,
        )
        .expect("child session");
    store
        .upsert_agent_edge(
            &parent,
            &child,
            psychevo_runtime::AgentEdgeStatus::Open,
            None,
        )
        .expect("agent edge");
    app.current_session = Some(parent.clone());
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Status,
        "translate(Translate user message to Chinese)",
        "Running (0 tool uses)",
    );
    row.tool_name = Some("Agent".to_string());
    row.agent_target = Some(child.clone());
    row.full_text = Some("Running (0 tool uses)\nPrompt:\ntranslate text".to_string());
    let row_id = row.id;
    ui.transcript.push(row);
    draw_fullscreen_for_test(&app, &mut ui, 100, 14);
    let open_area = transcript_test_target_area(&ui, TranscriptHitTarget::AgentOpen(row_id));
    let row_area = transcript_test_target_area(&ui, TranscriptHitTarget::Row(row_id));
    assert_eq!(
        ui.transcript_hit(open_area.x + 1, open_area.y),
        Some(TranscriptHitTarget::AgentOpen(row_id))
    );
    assert_eq!(
        ui.transcript_hit(row_area.x + 1, row_area.y),
        Some(TranscriptHitTarget::Row(row_id))
    );
    let after_open = open_area
        .x
        .saturating_add(open_area.width)
        .saturating_add(1);
    if after_open < row_area.x.saturating_add(row_area.width) {
        assert_eq!(
            ui.transcript_hit(after_open, open_area.y),
            Some(TranscriptHitTarget::Row(row_id))
        );
    }

    click_transcript_test_target(&mut app, &mut ui, TranscriptHitTarget::Row(row_id)).await;
    assert!(ui.transcript[0].expanded);
    assert_eq!(app.current_session.as_deref(), Some(parent.as_str()));

    draw_fullscreen_for_test(&app, &mut ui, 100, 14);
    click_transcript_test_target(&mut app, &mut ui, TranscriptHitTarget::AgentOpen(row_id)).await;
    assert_eq!(app.current_session.as_deref(), Some(child.as_str()));

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('P'), KeyModifiers::ALT),
    )
    .await
    .expect("return to parent");
    assert_eq!(app.current_session.as_deref(), Some(parent.as_str()));
}

#[tokio::test]
async fn running_child_session_receives_scoped_stream_after_open() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("parent session");
    let child = store
        .create_child_session_with_metadata(
            &parent,
            &app.workdir,
            "agent",
            "mock-model",
            "mock",
            None,
        )
        .expect("child session");
    store
        .upsert_agent_edge(
            &parent,
            &child,
            psychevo_runtime::AgentEdgeStatus::Open,
            None,
        )
        .expect("agent edge");
    app.current_session = Some(parent.clone());
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Status,
        "translate(Translate user message to Chinese)",
        "Running (0 tool uses)",
    );
    row.tool_name = Some("Agent".to_string());
    row.agent_target = Some(child.clone());
    row.tool_started = Some(Instant::now());
    ui.transcript.push(row);
    ui.focus = FocusMode::Transcript;
    ui.ensure_selection();

    let (tx, rx) = mpsc::unbounded_channel();
    let result = psychevo_runtime::RunResult {
        session_id: parent.clone(),
        ..finished_run_result(&app)
    };
    let task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(result)
    });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        session_id: None,
        control,
        rx,
        task: RunningTask::Agent(task),
    });

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("open child");
    tx.send(RunStreamEvent::scoped(
        child.clone(),
        RunStreamEvent::ReasoningDelta {
            text: "child is working".to_string(),
        },
    ))
    .expect("send scoped child event");
    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("drain child stream");

    assert_eq!(app.current_session.as_deref(), Some(child.as_str()));
    assert!(
        ui.transcript.iter().any(
            |row| row.kind == TranscriptKind::Thinking && row.text.contains("child is working")
        )
    );
}

#[tokio::test]
async fn opening_running_agent_child_replays_scoped_live_backlog() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let store = SqliteStore::open(&app.db_path).expect("store");
    let parent = store
        .create_session_with_metadata(&app.workdir, "tui", "mock-model", "mock", None)
        .expect("parent session");
    let child = store
        .create_child_session_with_metadata(
            &parent,
            &app.workdir,
            "agent",
            "mock-model",
            "mock",
            None,
        )
        .expect("child session");
    store
        .upsert_agent_edge(
            &parent,
            &child,
            psychevo_runtime::AgentEdgeStatus::Open,
            None,
        )
        .expect("agent edge");
    app.current_session = Some(parent.clone());
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Status,
        "translate(Translate user message to Chinese)",
        "Running (0 tool uses)",
    );
    row.tool_name = Some("Agent".to_string());
    row.agent_target = Some(child.clone());
    row.tool_started = Some(Instant::now());
    ui.transcript.push(row);
    ui.focus = FocusMode::Transcript;
    ui.ensure_selection();

    let (tx, rx) = mpsc::unbounded_channel();
    let result = psychevo_runtime::RunResult {
        session_id: parent.clone(),
        ..finished_run_result(&app)
    };
    let task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(result)
    });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        session_id: None,
        control,
        rx,
        task: RunningTask::Agent(task),
    });
    tx.send(RunStreamEvent::scoped(
        child.clone(),
        RunStreamEvent::ReasoningDelta {
            text: "child started before inspection".to_string(),
        },
    ))
    .expect("send scoped child event");

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("drain parent preview");
    assert_eq!(app.current_session.as_deref(), Some(parent.as_str()));
    assert!(
        ui.transcript[0]
            .text
            .contains("child started before inspection")
    );

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("open child");

    assert_eq!(app.current_session.as_deref(), Some(child.as_str()));
    assert!(ui.transcript.iter().any(|row| {
        row.kind == TranscriptKind::Thinking && row.text.contains("child started before inspection")
    }));
}

#[tokio::test]
async fn scoped_child_stream_updates_parent_agent_tail_without_child_rows() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let parent = "parent-session".to_string();
    let child = "child-session".to_string();
    app.current_session = Some(parent.clone());
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Status,
        "translate(Translate user message to Chinese)",
        "Running (0 tool uses)",
    );
    row.tool_name = Some("Agent".to_string());
    row.agent_target = Some(child.clone());
    row.tool_started = Some(Instant::now());
    ui.transcript.push(row);

    let (tx, rx) = mpsc::unbounded_channel();
    let result = psychevo_runtime::RunResult {
        session_id: parent.clone(),
        ..finished_run_result(&app)
    };
    let task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(result)
    });
    let (control, _) = run_control();
    ui.running = Some(RunningTurn {
        session_id: None,
        control,
        rx,
        task: RunningTask::Agent(task),
    });
    tx.send(RunStreamEvent::scoped(
        child.clone(),
        RunStreamEvent::Event(serde_json::json!({
            "type": "tool_execution_end",
            "tool_name": "read",
            "tool_call_id": "read-1",
            "args": {"path": "src/lib.rs"},
            "result": {"path": "src/lib.rs", "content": "ok"},
            "outcome": "normal"
        })),
    ))
    .expect("send child tool event");
    tx.send(RunStreamEvent::scoped(
        child.clone(),
        RunStreamEvent::Event(serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "translated"}]
            },
            "usage": {"total_tokens": 15}
        })),
    ))
    .expect("send child message event");

    app.drain_fullscreen_events(&mut ui)
        .await
        .expect("drain parent preview");

    assert_eq!(ui.transcript.len(), 1);
    assert!(
        ui.transcript[0]
            .text
            .contains("Running (1 tool use · 15 tokens)")
    );
    assert!(ui.transcript[0].text.contains("Explored src/lib.rs"));
    assert!(ui.transcript[0].text.contains("Response: translated"));
}

#[test]
fn parent_agent_preview_coalesces_streamed_reasoning_chunks() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let child = "child-session".to_string();
    let mut ui = FullscreenUi::new(&app);
    let mut row = TranscriptRow::with_title(
        TranscriptKind::Status,
        "translate(Translate user message to Chinese)",
        "Running (0 tool uses)",
    );
    row.tool_name = Some("Agent".to_string());
    row.agent_target = Some(child.clone());
    row.tool_started = Some(Instant::now());
    ui.transcript.push(row);

    for text in [
        "Hmm, this",
        " looks",
        " like",
        " a",
        " streamed",
        " reasoning",
        " preview",
        " chunk",
    ] {
        assert!(ui.apply_agent_child_preview_event(
            &child,
            &RunStreamEvent::ReasoningDelta {
                text: text.to_string(),
            },
        ));
    }

    let text = &ui.transcript[0].text;
    assert!(
        text.contains("Thinking: Hmm, this looks like a streamed reasoning preview chunk"),
        "{text}"
    );
    assert_eq!(text.matches("Thinking:").count(), 1, "{text}");
    assert!(!text.contains("more lines"), "{text}");
}

#[tokio::test]
async fn ctrl_t_focuses_transcript_and_space_toggles_expandable_row() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::LongThinkingMarkdownBottom);
    let thinking_index = ui
        .transcript
        .iter()
        .position(|row| row.kind == TranscriptKind::Thinking)
        .expect("thinking row");
    assert!(!ui.transcript[thinking_index].expanded);

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL),
    )
    .await
    .expect("focus transcript");
    assert_eq!(ui.focus, FocusMode::Transcript);
    assert!(ui.selected_target.is_some());

    app.handle_fullscreen_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
    )
    .await
    .expect("toggle thinking");
    assert!(ui.transcript[thinking_index].expanded);

    app.handle_fullscreen_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("return composer");
    assert_eq!(ui.focus, FocusMode::Composer);
}

fn write_home_tui_agent(app: &TuiApp, name: &str, description: &str) {
    let dir = app.home.join("agents");
    std::fs::create_dir_all(&dir).expect("home agent dir");
    std::fs::write(
        dir.join(format!("{name}.md")),
        format!("---\ndescription: {description:?}\n---\n\nUse this agent.\n"),
    )
    .expect("home agent");
}

async fn click_transcript_test_target(
    app: &mut TuiApp,
    ui: &mut FullscreenUi<'_>,
    target: TranscriptHitTarget,
) {
    let area = transcript_test_target_area(ui, target);
    app.handle_fullscreen_mouse(
        ui,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x + 1,
            row: area.y,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("mouse down");
    app.handle_fullscreen_mouse(
        ui,
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: area.x + 1,
            row: area.y,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await
    .expect("mouse up");
}

fn transcript_test_target_area(ui: &FullscreenUi<'_>, target: TranscriptHitTarget) -> Rect {
    ui.last_entry_areas
        .iter()
        .find_map(|(entry_target, area)| (*entry_target == target).then_some(*area))
        .expect("target area")
}
