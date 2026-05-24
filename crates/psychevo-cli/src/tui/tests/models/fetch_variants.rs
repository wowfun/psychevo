#[allow(unused_imports)]
pub(crate) use super::*;

#[tokio::test]
pub(crate) async fn model_fetch_failure_preserves_old_fetched_cache() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    app.sync_model_catalog_providers().expect("providers");
    let state = app
        .model_catalog
        .providers
        .get_mut("mock")
        .expect("mock provider");
    state.status = ModelCatalogStatus::Fetching;
    state.fetched = vec![ModelCatalogEntry {
        id: "old-remote".to_string(),
        context_limit: None,
        metadata: Default::default(),
    }];
    app.model_catalog.tasks.insert(
        "mock".to_string(),
        tokio::spawn(async {
            ModelCatalogFetchResult {
                provider: "mock".to_string(),
                result: Err("network down".to_string()),
            }
        }),
    );
    let mut ui = FullscreenUi::new(&app);
    ui.bottom_panel = Some(BottomPanel::Models(ModelPanel::new(
        app.model_selection_panel().expect("panel"),
    )));

    drain_catalog_until_idle(&mut app, &mut ui).await;

    let state = app
        .model_catalog
        .providers
        .get("mock")
        .expect("mock provider");
    assert_eq!(
        state.status,
        ModelCatalogStatus::Failed("network down".to_string())
    );
    assert_eq!(state.fetched[0].id, "old-remote");
}

#[tokio::test]
pub(crate) async fn model_fetch_cancel_preserves_old_fetched_cache() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    app.sync_model_catalog_providers().expect("providers");
    let state = app
        .model_catalog
        .providers
        .get_mut("mock")
        .expect("mock provider");
    state.status = ModelCatalogStatus::Fetching;
    state.fetched = vec![ModelCatalogEntry {
        id: "old-remote".to_string(),
        context_limit: None,
        metadata: Default::default(),
    }];
    app.model_catalog.tasks.insert(
        "mock".to_string(),
        tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            ModelCatalogFetchResult {
                provider: "mock".to_string(),
                result: Ok(Vec::new()),
            }
        }),
    );

    app.model_catalog.abort_unfinished();

    let state = app
        .model_catalog
        .providers
        .get("mock")
        .expect("mock provider");
    assert!(app.model_catalog.tasks.is_empty());
    assert_eq!(state.status, ModelCatalogStatus::Fetched);
    assert_eq!(state.fetched[0].id, "old-remote");
}

pub(crate) async fn drain_catalog_until_idle(app: &mut TuiApp, ui: &mut FullscreenUi<'_>) {
    for _ in 0..50 {
        app.drain_model_catalog_fetches(ui)
            .await
            .expect("drain catalog");
        if app.model_catalog.tasks.is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("catalog fetch did not finish");
}

#[tokio::test]
pub(crate) async fn model_selection_opens_variant_panel() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("down");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("select model");

    let Some(BottomPanel::Variants { panel, .. }) = &ui.bottom_panel else {
        panic!("expected variant panel");
    };
    assert!(panel.title.contains("mock/other-model"));
    assert_eq!(
        panel.rows[panel.filtered_indices()[panel.selected]].label,
        "Config default"
    );
}

#[tokio::test]
pub(crate) async fn model_variant_panel_up_down_wraps_between_first_and_last_rows() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("down to other model");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("select model");

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
        .expect("wrap up");
    let Some(BottomPanel::Variants { panel, .. }) = &ui.bottom_panel else {
        panic!("expected variant panel");
    };
    assert_eq!(
        panel.rows[panel.filtered_indices()[panel.selected]].label,
        "max"
    );

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("wrap down");
    let Some(BottomPanel::Variants { panel, .. }) = &ui.bottom_panel else {
        panic!("expected variant panel");
    };
    assert_eq!(
        panel.rows[panel.filtered_indices()[panel.selected]].label,
        "Config default"
    );
}

#[tokio::test]
pub(crate) async fn model_config_default_clears_variant_override() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    app.current_variant = Some("xhigh".to_string());
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("down");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("select model");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("select config default");

    assert_eq!(app.current_model, None);
    assert_eq!(app.current_variant, None);
    assert_eq!(app.state.model_for(&app.workdir_key), None);
    assert_eq!(app.state.variant_for(&app.workdir_key), None);
    let local_config =
        fs::read_to_string(app.workdir.join(".psychevo/config.toml")).expect("local config");
    assert!(local_config.contains("[model]"));
    assert!(local_config.contains("id = \"mock/other-model\""));
    assert!(local_config.contains("reasoning_effort = \"high\""));
    assert!(ui.bottom_panel.is_none());
}

#[tokio::test]
pub(crate) async fn model_explicit_variant_writes_reasoning_effort_without_variant_override() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("down");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("select model");
    for ch in "xhigh".chars() {
        app.handle_bottom_panel_key(
            &mut ui,
            KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .expect("query");
    }
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("select variant");

    assert_eq!(app.current_model, None);
    assert_eq!(app.current_variant, None);
    assert_eq!(app.state.variant_for(&app.workdir_key), None);
    let local_config =
        fs::read_to_string(app.workdir.join(".psychevo/config.toml")).expect("local config");
    assert!(local_config.contains("[model]"));
    assert!(local_config.contains("id = \"mock/other-model\""));
    assert!(local_config.contains("reasoning_effort = \"xhigh\""));
}

#[tokio::test]
pub(crate) async fn model_variant_escape_returns_to_model_then_closes() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("down");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("select model");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .expect("back");
    assert!(matches!(ui.bottom_panel, Some(BottomPanel::Models(_))));

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .expect("close");
    assert!(ui.bottom_panel.is_none());
}
