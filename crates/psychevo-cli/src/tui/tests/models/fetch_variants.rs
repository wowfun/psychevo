#[allow(unused_imports)]
pub(crate) use super::*;

#[tokio::test]
pub(crate) async fn model_catalog_sync_hydrates_persistent_provider_cache() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    app.sync_model_catalog_providers().expect("providers");
    let provider = app
        .model_catalog
        .providers
        .get("mock")
        .expect("mock provider")
        .provider
        .clone();
    psychevo_runtime::write_cached_model_catalog(
        &app.home,
        &provider,
        &[ModelCatalogEntry {
            id: "cached-remote".to_string(),
            context_limit: None,
            metadata: Default::default(),
        }],
    )
    .expect("write cache");

    let mut fresh = test_app_with_models(&temp);
    fresh
        .sync_model_catalog_providers()
        .expect("fresh providers");
    let state = fresh
        .model_catalog
        .providers
        .get("mock")
        .expect("mock provider");

    assert_eq!(state.status, ModelCatalogStatus::Fetched);
    assert_eq!(state.fetched[0].id, "cached-remote");
}

#[tokio::test]
pub(crate) async fn model_catalog_fetch_writes_persistent_provider_cache() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    app.sync_model_catalog_providers().expect("providers");
    let server = OneShotCatalogServer::new(r#"{"data":[{"id":"remote-live"}]}"#);
    {
        let state = app
            .model_catalog
            .providers
            .get_mut("mock")
            .expect("mock provider");
        state.provider.base_url = server.base_url.clone();
    }
    let provider = app
        .model_catalog
        .providers
        .get("mock")
        .expect("mock provider")
        .provider
        .clone();
    let mut ui = FullscreenUi::new(&app);

    app.start_model_catalog_fetch_task("mock");
    drain_catalog_until_idle(&mut app, &mut ui).await;

    let cached = read_cached_model_catalog(&app.home, &provider).expect("cached models");
    assert_eq!(cached[0].id, "remote-live");
}

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

struct OneShotCatalogServer {
    base_url: String,
}

impl OneShotCatalogServer {
    fn new(body: &'static str) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind catalog server");
        let addr = listener.local_addr().expect("catalog addr");
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0_u8; 4096];
                let _ = std::io::Read::read(&mut stream, &mut buffer);
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = std::io::Write::write_all(&mut stream, response.as_bytes());
            }
        });
        Self {
            base_url: format!("http://{addr}/v1"),
        }
    }
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
pub(crate) async fn model_config_default_saves_composer_model_state() {
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

    assert_eq!(app.current_model.as_deref(), Some("mock/other-model"));
    assert_eq!(app.current_variant.as_deref(), Some("high"));
    assert_eq!(
        app.model_state.model_for(&app.cwd_key).as_deref(),
        Some("mock/other-model")
    );
    assert_eq!(
        app.model_state
            .reasoning_effort_for(&app.cwd_key)
            .as_deref(),
        Some("high")
    );
    let local_config =
        fs::read_to_string(app.cwd.join(".psychevo/config.toml")).expect("local config");
    assert!(!local_config.contains("id = \"mock/other-model\""));
    assert!(ui.bottom_panel.is_none());
}

#[tokio::test]
pub(crate) async fn model_explicit_variant_saves_composer_reasoning_effort() {
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

    assert_eq!(app.current_model.as_deref(), Some("mock/other-model"));
    assert_eq!(app.current_variant.as_deref(), Some("xhigh"));
    assert_eq!(
        app.model_state.model_for(&app.cwd_key).as_deref(),
        Some("mock/other-model")
    );
    assert_eq!(
        app.model_state
            .reasoning_effort_for(&app.cwd_key)
            .as_deref(),
        Some("xhigh")
    );
    let local_config =
        fs::read_to_string(app.cwd.join(".psychevo/config.toml")).expect("local config");
    assert!(!local_config.contains("id = \"mock/other-model\""));
    assert!(!local_config.contains("reasoning_effort = \"xhigh\""));
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
