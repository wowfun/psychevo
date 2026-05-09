#[test]
fn tui_snapshot_wide_idle_minimal_chrome() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::Idle);
    assert_tui_snapshot("wide_idle_minimal_chrome", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_wide_optional_sidebar() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    ui.sidebar_forced = true;
    ui.sidebar_hidden = false;
    assert_tui_snapshot("wide_optional_sidebar", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_narrow_idle_composer_without_sidebar() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::Idle);
    assert_tui_snapshot("narrow_idle_composer_without_sidebar", 80, 20, &app, ui);
}

#[test]
fn tui_snapshot_slash_menu_prefix_filtering() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    ui.textarea = textarea_with_text("/mo");
    assert_tui_snapshot("slash_menu_prefix_filtering", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_file_completion_popup() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    ui.textarea = textarea_with_text("review @src");
    ui.file_search.popup = Some(FileSearchPopupState {
        query: "src".to_string(),
        matches: vec![
            FileSearchMatch {
                path: "src".to_string(),
                kind: FileSearchMatchKind::Directory,
            },
            FileSearchMatch {
                path: "src/main.rs".to_string(),
                kind: FileSearchMatchKind::File,
            },
            FileSearchMatch {
                path: "crates/psychevo-cli/src/tui/mod.rs".to_string(),
                kind: FileSearchMatchKind::File,
            },
        ],
        selected: 1,
        waiting: false,
    });
    assert_tui_snapshot("file_completion_popup", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_model_bottom_panel() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    ui.bottom_panel = Some(BottomPanel::Models(
        app.model_selection_panel().expect("model panel"),
    ));
    assert_tui_snapshot("model_bottom_panel", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_variant_bottom_panel() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let models = app.model_selection_panel().expect("model panel");
    let (other, source) = models
        .rows
        .iter()
        .find_map(|row| match &row.value {
            BottomSelectionValue::Model { model, source } if model.model == "other-model" => {
                Some((model.clone(), *source))
            }
            _ => None,
        })
        .expect("other model");
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    ui.bottom_panel = Some(app.variant_panel(other, source, models));
    assert_tui_snapshot("variant_bottom_panel", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_session_bottom_panel() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    ui.bottom_panel = Some(BottomPanel::Sessions(stable_session_bottom_panel()));
    assert_tui_snapshot("session_bottom_panel", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_archived_session_action_bottom_panel() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    let mut panel = stable_archived_session_bottom_panel();
    panel.arm_action_mode();
    ui.bottom_panel = Some(BottomPanel::Sessions(panel));
    assert_tui_snapshot("archived_session_action_bottom_panel", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_running_turn_with_visible_thinking() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::RunningThinking);
    assert_tui_snapshot("running_turn_with_visible_thinking", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_completed_ledger_collapsed_tool_output() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::CollapsedTool);
    assert_tui_snapshot("completed_ledger_collapsed_tool_output", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_expanded_long_tool_output() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::ExpandedTool);
    assert_tui_snapshot("expanded_long_tool_output", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_debug_meta_with_usage_metadata() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.debug = true;
    let ui = fixture_ui(&app, FixtureKind::DebugMeta);
    assert_tui_snapshot("debug_meta_with_usage_metadata", 120, 24, &app, ui);
}

#[test]
fn tui_snapshot_failure_tool_error_turn_meta() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let ui = fixture_ui(&app, FixtureKind::FailureMeta);
    assert_tui_snapshot("failure_tool_error_turn_meta", 120, 24, &app, ui);
}

