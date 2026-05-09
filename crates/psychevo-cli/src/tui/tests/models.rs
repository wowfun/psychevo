#[tokio::test]
async fn model_command_opens_searchable_bottom_picker() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");

    let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel else {
        panic!("expected model panel");
    };
    assert_eq!(panel.rows.len(), 5);
    assert_eq!(panel.rows[0].label, "Add provider");
    assert_eq!(panel.rows[1].label, "All providers");
    assert_eq!(panel.rows[2].label, "mock");
    panel.set_query_char('o');
    panel.set_query_char('t');
    panel.set_query_char('h');
    let filtered = panel.filtered_indices();
    assert_eq!(
        filtered
            .iter()
            .map(|index| panel.rows[*index].label.as_str())
            .collect::<Vec<_>>(),
        vec!["Add provider", "All providers", "mock", "mock/other-model"]
    );
}

#[tokio::test]
async fn model_fetch_all_adds_fetched_rows_and_preserves_query() {
    let temp = tempdir().expect("temp");
    let server = TuiCatalogServer::new(r#"{"data":[{"id":"remote-model"},{"id":"mock-model"}]}"#);
    let config_path = temp.path().join("fetch-config.jsonc");
    fs::write(
        &config_path,
        format!(
            r#"{{
              "model": "mock/mock-model",
              "provider": {{
                "mock": {{
                  "options": {{
                    "base_url": "{}",
                    "api_key_env": "TEST_PROVIDER_KEY"
                  }},
                  "models": {{
                    "mock-model": {{}}
                  }}
                }}
              }}
            }}"#,
            server.base_url
        ),
    )
    .expect("config");
    let mut app = test_app(&temp);
    app.env_map
        .insert("TEST_PROVIDER_KEY".to_string(), "test-key".to_string());
    app.config_path = Some(config_path);
    app.current_model = Some("mock/mock-model".to_string());
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    {
        let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel else {
            panic!("expected model panel");
        };
        for ch in "remote".chars() {
            panel.set_query_char(ch);
        }
        panel.select_value_key("fetch:all");
    }
    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("fetch");
    drain_catalog_until_idle(&mut app, &mut ui).await;

    let Some(BottomPanel::Models(panel)) = &ui.bottom_panel else {
        panic!("expected model panel");
    };
    assert_eq!(panel.query, "remote");
    assert_eq!(
        panel
            .filtered_indices()
            .iter()
            .map(|index| panel.rows[*index].label.as_str())
            .collect::<Vec<_>>(),
        vec!["Add provider", "All providers", "mock", "mock/remote-model"]
    );
    let request = server
        .requests
        .lock()
        .expect("requests")
        .first()
        .cloned()
        .expect("request");
    assert!(request.starts_with("GET /v1/models HTTP/1.1"));
    assert!(
        request
            .to_lowercase()
            .contains("authorization: bearer test-key")
    );
}

#[tokio::test]
async fn model_fetch_missing_credentials_stays_in_panel() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let config_path = temp.path().join("missing-config.jsonc");
    fs::write(
        &config_path,
        r#"{
              "model": "mock/mock-model",
              "provider": {
                "mock": {
                  "options": {
                    "base_url": "http://api.example/v1",
                    "api_key_env": "TEST_PROVIDER_KEY"
                  },
                  "models": {
                    "mock-model": {}
                  }
                }
              }
            }"#,
    )
    .expect("config");
    app.config_path = Some(config_path);
    app.current_model = Some("mock/mock-model".to_string());
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");

    let Some(BottomPanel::Models(panel)) = &ui.bottom_panel else {
        panic!("expected model panel");
    };
    let provider = panel
        .rows
        .iter()
        .find(|row| row.label == "mock")
        .expect("provider");
    assert_eq!(
        provider.description.as_deref(),
        Some("missing TEST_PROVIDER_KEY")
    );
    assert!(matches!(
        provider.value,
        BottomSelectionValue::ProviderInfo(ref provider) if provider == "mock"
    ));
}

#[tokio::test]
async fn model_add_provider_saves_global_config_fetches_and_selects_model() {
    let temp = tempdir().expect("temp");
    let server = TuiCatalogServer::new(r#"{"data":[{"id":"remote-model"}]}"#);
    let mut app = test_app(&temp);
    app.env_map.insert(
        "PSYCHEVO_HOME".to_string(),
        app.home.to_string_lossy().to_string(),
    );
    fs::write(app.home.join("config.jsonc"), "{}\n").expect("config");
    app.current_model = None;
    app.selected_model = None;
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    {
        let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel else {
            panic!("expected model panel");
        };
        panel.select_value_key("provider:add");
    }
    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("add provider");

    let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel else {
        panic!("expected provider wizard");
    };
    panel.label = "Xiaomi Token Plan CN".to_string();
    panel.provider_id = "xiaomi-token-plan-cn".to_string();
    panel.provider_id_touched = true;
    panel.base_url = server.base_url.clone();
    panel.api_key = "test-key".to_string();
    app.refresh_provider_wizard_env_state(&mut ui);
    app.save_provider_wizard(&mut ui).expect("save provider");
    drain_catalog_until_idle(&mut app, &mut ui).await;

    let config = fs::read_to_string(app.home.join("config.jsonc")).expect("config");
    assert!(config.contains(r#""xiaomi-token-plan-cn""#));
    assert!(config.contains(r#""label": "Xiaomi Token Plan CN""#));
    assert!(config.contains(r#""api_key_env": "XIAOMI_TOKEN_PLAN_CN_API_KEY""#));
    assert!(!config.contains("test-key"));
    let env = fs::read_to_string(app.home.join(".env")).expect("env");
    assert_eq!(env, "XIAOMI_TOKEN_PLAN_CN_API_KEY=test-key\n");
    let request = server
        .requests
        .lock()
        .expect("requests")
        .first()
        .cloned()
        .expect("request");
    assert!(request.starts_with("GET /v1/models HTTP/1.1"));
    assert!(
        request
            .to_lowercase()
            .contains("authorization: bearer test-key")
    );

    let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel else {
        panic!("expected model panel");
    };
    assert_eq!(panel.notice.as_deref(), Some("provider saved; fetching models"));
    panel.select_value_key("model:xiaomi-token-plan-cn/remote-model");
    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("select model");
    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("select variant");

    assert_eq!(
        app.current_model.as_deref(),
        Some("xiaomi-token-plan-cn/remote-model")
    );
    assert_eq!(
        app.state.model_for(&app.workdir_key).as_deref(),
        Some("xiaomi-token-plan-cn/remote-model")
    );
    let config = fs::read_to_string(app.home.join("config.jsonc")).expect("config");
    assert!(!config.contains(r#""model": "xiaomi-token-plan-cn/remote-model""#));
}

#[tokio::test]
async fn model_add_provider_wizard_generates_id_and_reports_validation_errors() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.env_map.insert(
        "PSYCHEVO_HOME".to_string(),
        app.home.to_string_lossy().to_string(),
    );
    fs::write(app.home.join("config.jsonc"), "{}\n").expect("config");
    let mut ui = FullscreenUi::new(&app);

    ui.bottom_panel = Some(BottomPanel::ProviderWizard(app.provider_wizard_panel()));
    for ch in "Xiaomi Token Plan CN".chars() {
        app.handle_provider_wizard_key(
            &mut ui,
            KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .expect("type label");
    }
    let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel else {
        panic!("expected provider wizard");
    };
    assert_eq!(panel.provider_id, "xiaomi-token-plan-cn");
    panel.provider_id = "mimo".to_string();
    panel.provider_id_touched = true;
    panel.base_url = "https://token-plan-cn.xiaomimimo.com/v1".to_string();
    panel.api_key = "test-key".to_string();

    app.save_provider_wizard(&mut ui).expect("save provider");

    let Some(BottomPanel::ProviderWizard(panel)) = &ui.bottom_panel else {
        panic!("expected provider wizard");
    };
    assert!(
        panel
            .notice
            .as_deref()
            .is_some_and(|notice| notice.contains("collides"))
    );
    let config = fs::read_to_string(app.home.join("config.jsonc")).expect("config");
    assert!(!config.contains(r#""mimo""#));
}

#[tokio::test]
async fn fetched_model_selection_uses_provider_default_and_only_persists_tui_state() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    app.sync_model_catalog_providers().expect("providers");
    let state = app
        .model_catalog
        .providers
        .get_mut("mock")
        .expect("mock provider");
    state.status = ModelCatalogStatus::Fetched;
    state.fetched = vec![ModelCatalogEntry {
        id: "remote-model".to_string(),
        context_limit: None,
    }];
    let config_before =
        fs::read_to_string(app.config_path.as_ref().expect("config")).expect("config before");
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    {
        let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel else {
            panic!("expected model panel");
        };
        panel.select_value_key("model:mock/remote-model");
    }
    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("select fetched");
    let Some(BottomPanel::Variants { panel, .. }) = &ui.bottom_panel else {
        panic!("expected variant panel");
    };
    assert_eq!(
        panel.rows[panel.filtered_indices()[panel.selected]]
            .description
            .as_deref(),
        Some("use provider default")
    );

    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("select variant");

    assert_eq!(app.current_model.as_deref(), Some("mock/remote-model"));
    assert_eq!(
        app.state.model_for(&app.workdir_key).as_deref(),
        Some("mock/remote-model")
    );
    assert!(
        app.state
            .recent_models
            .contains(&"mock/remote-model".to_string())
    );
    assert_eq!(
        fs::read_to_string(app.config_path.as_ref().expect("config")).expect("config after"),
        config_before
    );
}

#[test]
fn model_picker_initial_focus_prefers_model_rows_before_fetch_rows() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let panel = app.model_selection_panel().expect("panel");
    assert_eq!(
        panel.rows[panel.filtered_indices()[panel.selected]].label,
        "mock/mock-model"
    );

    app.current_model = None;
    app.selected_model = None;
    let panel = app.model_selection_panel().expect("panel");
    assert_eq!(
        panel.rows[panel.filtered_indices()[panel.selected]].label,
        "mock/mock-model"
    );

    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let config_path = temp.path().join("empty-model-config.jsonc");
    fs::write(
        &config_path,
        r#"{
              "provider": {
                "mock": {
                  "options": { "base_url": "http://127.0.0.1:9" },
                  "models": {}
                }
              }
            }"#,
    )
    .expect("config");
    app.config_path = Some(config_path);
    app.current_model = None;
    app.selected_model = None;
    let panel = app.model_selection_panel().expect("panel");
    assert_eq!(
        panel.rows[panel.filtered_indices()[panel.selected]].label,
        "All providers"
    );
}

#[tokio::test]
async fn model_picker_up_down_wraps_between_first_and_last_rows() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Home, KeyModifiers::NONE))
        .expect("first row");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
        .expect("wrap up");
    let Some(BottomPanel::Models(panel)) = &ui.bottom_panel else {
        panic!("expected model panel");
    };
    assert_eq!(
        panel.rows[panel.filtered_indices()[panel.selected]].label,
        "mock/other-model"
    );

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("wrap down");
    let Some(BottomPanel::Models(panel)) = &ui.bottom_panel else {
        panic!("expected model panel");
    };
    assert_eq!(
        panel.rows[panel.filtered_indices()[panel.selected]].label,
        "Add provider"
    );
}

#[tokio::test]
async fn model_fetch_failure_preserves_old_fetched_cache() {
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
    ui.bottom_panel = Some(BottomPanel::Models(
        app.model_selection_panel().expect("panel"),
    ));

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
async fn model_fetch_cancel_preserves_old_fetched_cache() {
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

async fn drain_catalog_until_idle(app: &mut TuiApp, ui: &mut FullscreenUi<'_>) {
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
async fn model_selection_opens_variant_panel() {
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
async fn model_variant_panel_up_down_wraps_between_first_and_last_rows() {
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
async fn model_config_default_clears_variant_override() {
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
    assert_eq!(app.current_variant, None);
    assert_eq!(
        app.state.model_for(&app.workdir_key).as_deref(),
        Some("mock/other-model")
    );
    assert_eq!(app.state.variant_for(&app.workdir_key), None);
    assert!(ui.bottom_panel.is_none());
}

#[tokio::test]
async fn model_explicit_variant_persists_override() {
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
        app.state.variant_for(&app.workdir_key).as_deref(),
        Some("xhigh")
    );
}

#[tokio::test]
async fn model_variant_escape_returns_to_model_then_closes() {
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
