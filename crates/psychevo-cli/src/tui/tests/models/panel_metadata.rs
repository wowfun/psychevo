#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

#[tokio::test]
pub(crate) async fn model_command_opens_searchable_bottom_picker() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");

    let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel else {
        panic!("expected model panel");
    };
    assert_eq!(panel.tab, ModelTab::Models);
    assert_eq!(panel.models.rows.len(), 5);
    assert_eq!(panel.models.rows[0].label, "Add provider");
    assert_eq!(panel.models.rows[1].label, "All providers");
    assert_eq!(panel.models.rows[2].label, "mock");
    panel.models.set_query_char('o');
    panel.models.set_query_char('t');
    panel.models.set_query_char('h');
    let filtered = panel.models.filtered_indices();
    assert_eq!(
        filtered
            .iter()
            .map(|index| panel.models.rows[*index].label.as_str())
            .collect::<Vec<_>>(),
        vec!["Add provider", "All providers", "mock", "mock/other-model"]
    );
}

#[tokio::test]
pub(crate) async fn model_tabs_switch_and_preserve_query_selection_and_scroll() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    {
        let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel else {
            panic!("expected model panel");
        };
        panel.models.set_query_char('o');
        panel.models.select_value_key("model:mock/other-model");
    }
    let selected_before = ui
        .bottom_panel
        .as_ref()
        .expect("panel")
        .selection()
        .selected_key();

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .expect("tab");
    {
        let Some(BottomPanel::Models(panel)) = &ui.bottom_panel else {
            panic!("expected model panel");
        };
        assert_eq!(panel.tab, ModelTab::Info);
        assert_eq!(panel.models.query, "o");
        assert_eq!(panel.models.selected_key(), selected_before);
    }

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("scroll info");
    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("enter info");
    {
        let Some(BottomPanel::Models(panel)) = &ui.bottom_panel else {
            panic!("expected model panel");
        };
        assert_eq!(panel.tab, ModelTab::Info);
        assert_eq!(panel.info_scroll, 1);
    }

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
        .expect("right");
    {
        let Some(BottomPanel::Models(panel)) = &ui.bottom_panel else {
            panic!("expected model panel");
        };
        assert_eq!(panel.tab, ModelTab::Models);
        assert_eq!(panel.models.query, "o");
        assert_eq!(panel.models.selected_key(), selected_before);
        assert_eq!(panel.info_scroll, 1);
    }

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
        .expect("left");
    let Some(BottomPanel::Models(panel)) = &ui.bottom_panel else {
        panic!("expected model panel");
    };
    assert_eq!(panel.tab, ModelTab::Info);
    assert_eq!(panel.models.query, "o");
    assert_eq!(panel.models.selected_key(), selected_before);
    assert_eq!(panel.info_scroll, 1);
}

#[test]
pub(crate) fn model_info_tab_renders_selected_model_details() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    let mut panel = ModelPanel::new(app.model_selection_panel().expect("model panel"));
    panel.tab = ModelTab::Info;
    ui.bottom_panel = Some(BottomPanel::Models(panel));

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 120, 24);
    let text = buffer_text(&buffer);

    assert!(text.contains("Model   Models    Info"));
    assert!(text.contains("model: mock/mock-model"));
    assert!(text.contains("provider: mock (mock)"));
    assert!(text.contains("source: current  local  metadata config"));
    assert!(text.contains("context 128,000  input 120,000  output 16,000"));
    assert!(text.contains("reasoning  tools  structured output"));
    assert!(text.contains("input: text, image"));
    assert!(text.contains("output: text"));
    assert!(text.contains("standard: in/out $1.500/$2.500/M"));
    assert!(text.contains("cache: read/write $0.150/$0.750/M"));
    assert!(text.contains("over-200k: in/out $3.000/$5.000/M"));
    assert!(text.contains("source: config"));
}

#[test]
pub(crate) fn model_info_tab_renders_cached_xiaomi_omni_capabilities() {
    let temp = tempdir().expect("temp");
    let config_path = temp.path().join("xiaomi-omni-config.toml");
    fs::write(
        &config_path,
        r#"
[provider."xiaomi-token-plan"]
label = "Xiaomi Token Plan"

[provider."xiaomi-token-plan".options]
base_url = "https://token-plan-cn.xiaomimimo.com/v1"
api_key_env = "XIAOMI_KEY"

[provider."xiaomi-token-plan".models."mimo-v2-omni"]
"#,
    )
    .expect("config");
    let mut app = test_app(&temp);
    app.env_map.insert(
        "PSYCHEVO_HOME".to_string(),
        app.home.to_string_lossy().to_string(),
    );
    app.env_map
        .insert("XIAOMI_KEY".to_string(), "test-key".to_string());
    fs::write(
        app.home.join("models_dev_cache.json"),
        r#"
        {
          "xiaomi-token-plan-cn": {
            "api": "https://token-plan-cn.xiaomimimo.com/v1",
            "models": {
              "mimo-v2-omni": {
                "id": "mimo-v2-omni",
                "reasoning": true,
                "tool_call": true,
                "temperature": true,
                "attachment": true,
                "interleaved": { "field": "reasoning_content" },
                "limit": { "context": 262144, "output": 131072 },
                "modalities": {
                  "input": ["text", "image", "audio", "video", "pdf"],
                  "output": ["text"]
                }
              },
              "unused-model": {
                "id": "unused-model",
                "limit": { "context": 999999 }
              }
            }
          }
        }
        "#,
    )
    .expect("cache");
    app.config_path = Some(config_path);
    app.current_model = Some("xiaomi-token-plan/mimo-v2-omni".to_string());
    app.refresh_selected_model();
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    let mut panel = ModelPanel::new(app.model_selection_panel().expect("model panel"));
    panel.tab = ModelTab::Info;
    ui.bottom_panel = Some(BottomPanel::Models(panel));

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 140, 24);
    let text = buffer_text(&buffer);

    assert!(text.contains("source: current  local  metadata models.dev"));
    assert!(text.contains("limits: context 262,144  output 131,072"));
    assert!(text.contains("capabilities: reasoning  tools  temperature  attachments  interleaved"));
    assert!(text.contains("modalities: input: text, image, audio, video, pdf  output: text"));
}

#[tokio::test]
pub(crate) async fn model_ctrl_r_refreshes_metadata_cache_and_preserves_info_state() {
    let temp = tempdir().expect("temp");
    let server = TuiCatalogServer::new(
        r#"
        {
          "xiaomi-token-plan-cn": {
            "api": "https://token-plan-cn.xiaomimimo.com/v1",
            "models": {
              "mimo-v2-omni": {
                "id": "mimo-v2-omni",
                "reasoning": true,
                "tool_call": true,
                "temperature": true,
                "attachment": true,
                "interleaved": { "field": "reasoning_content" },
                "cost": { "input": 0, "output": 0 },
                "limit": { "context": 262144, "output": 131072 },
                "modalities": {
                  "input": ["text", "image", "audio", "video", "pdf"],
                  "output": ["text"]
                }
              }
            }
          }
        }
        "#,
    );
    let config_path = temp.path().join("xiaomi-omni-config.toml");
    fs::write(
        &config_path,
        r#"
[provider."xiaomi-token-plan"]
label = "Xiaomi Token Plan"

[provider."xiaomi-token-plan".options]
base_url = "https://token-plan-cn.xiaomimimo.com/v1"
api_key_env = "XIAOMI_KEY"

[provider."xiaomi-token-plan".models."mimo-v2-omni"]
"#,
    )
    .expect("config");
    let mut app = test_app(&temp);
    app.env_map.insert(
        "PSYCHEVO_HOME".to_string(),
        app.home.to_string_lossy().to_string(),
    );
    app.env_map
        .insert("PSYCHEVO_MODELS_DEV_URL".to_string(), server.base_url);
    app.env_map
        .insert("XIAOMI_KEY".to_string(), "test-key".to_string());
    app.config_path = Some(config_path);
    app.current_model = Some("xiaomi-token-plan/mimo-v2-omni".to_string());
    app.model_state
        .set_model("/old", "xiaomi-token-plan/unused-model", None);
    app.refresh_selected_model();
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    {
        let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel else {
            panic!("expected model panel");
        };
        panel.tab = ModelTab::Info;
        panel.info_scroll = 1;
        panel.models.set_query_char('o');
        panel
            .models
            .select_value_key("model:xiaomi-token-plan/mimo-v2-omni");
    }

    app.handle_bottom_panel_key(
        &mut ui,
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
    )
    .expect("refresh");
    {
        let Some(BottomPanel::Models(panel)) = &ui.bottom_panel else {
            panic!("expected model panel");
        };
        assert_eq!(panel.models.notice.as_deref(), Some("refreshing metadata"));
    }
    drain_metadata_refresh_until_idle(&mut app, &mut ui).await;

    let Some(BottomPanel::Models(panel)) = &ui.bottom_panel else {
        panic!("expected model panel");
    };
    assert_eq!(panel.tab, ModelTab::Info);
    assert_eq!(panel.info_scroll, 1);
    assert_eq!(panel.models.query, "o");
    assert_eq!(panel.models.notice.as_deref(), Some("metadata refreshed"));
    assert!(app.home.join("models_dev_cache.json").is_file());
    let cache = fs::read_to_string(app.home.join("models_dev_cache.json")).expect("cache");
    assert!(cache.contains("mimo-v2-omni"), "{cache}");
    assert!(!cache.contains("unused-model"), "{cache}");

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 140, 24);
    let text = buffer_text(&buffer);
    assert!(text.contains("metadata models.dev"), "{text}");
    assert!(
        text.contains("capabilities: reasoning  tools  temperature  attachments  interleaved"),
        "{text}"
    );
    assert!(
        text.contains("modalities: input: text, image, audio, video, pdf  output: text"),
        "{text}"
    );
    assert!(text.contains("pricing: standard: free"), "{text}");
}

#[tokio::test]
pub(crate) async fn startup_warmup_fetches_missing_metadata_cache_silently() {
    let temp = tempdir().expect("temp");
    let server = TuiCatalogServer::new(r#"{"mock":{"models":{"model":{"id":"model"}}}}"#);
    let mut app = test_app(&temp);
    app.env_map.insert(
        "PSYCHEVO_HOME".to_string(),
        app.home.to_string_lossy().to_string(),
    );
    app.env_map
        .insert("PSYCHEVO_MODELS_DEV_URL".to_string(), server.base_url);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);

    app.start_missing_model_metadata_cache_warmup();
    assert!(app.model_catalog.metadata_refreshing());
    drain_metadata_refresh_until_idle(&mut app, &mut ui).await;

    assert!(app.home.join("models_dev_cache.json").is_file());
    let text = ui
        .transcript
        .iter()
        .map(|row| row.text.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!text.contains("metadata warmup failed"), "{text}");
}

pub(crate) async fn drain_metadata_refresh_until_idle(app: &mut TuiApp, ui: &mut FullscreenUi<'_>) {
    for _ in 0..200 {
        app.drain_fullscreen_events(ui).await.expect("drain events");
        if !app.model_catalog.metadata_refreshing() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("metadata refresh did not finish");
}

#[test]
pub(crate) fn model_info_tab_omits_unknown_and_shows_false_capabilities() {
    let mut model = ConfiguredModel {
        provider: "mock".to_string(),
        provider_label: "Mock".to_string(),
        model: "false-caps".to_string(),
        reasoning_effort: Some("low".to_string()),
        context_limit: None,
        metadata: Default::default(),
    };
    model.metadata.capabilities.reasoning = Some(false);
    model.metadata.capabilities.tool_call = Some(false);
    model.metadata.capabilities.temperature = Some(false);
    model.metadata.capabilities.attachment = Some(false);
    model.metadata.capabilities.structured_output = Some(false);
    model.metadata.capabilities.interleaved = Some(Value::Bool(false));
    let row = BottomSelectionRow {
        label: "mock/false-caps".to_string(),
        description: None,
        detail: None,
        group: None,
        search_text: "mock false caps".to_string(),
        is_current: false,
        is_default: false,
        style: BottomRowStyle::Normal,
        footer: None,
        value: BottomSelectionValue::Model {
            model: Box::new(model.clone()),
            source: ModelRowSource::Local,
        },
    };

    let text = model_info_lines(&model, ModelRowSource::Local, &row)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(text.contains("source: local  default low"));
    assert!(text.contains(
        "no reasoning  no tools  no temperature  no attachments  no structured output  no interleaved"
    ));
    assert!(!text.contains("Limits"));
    assert!(!text.contains("Modalities"));
    assert!(!text.contains("Pricing"));
}

#[test]
pub(crate) fn model_info_tab_action_row_shows_empty_state() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    let mut ui = fixture_ui(&app, FixtureKind::Idle);
    let mut panel = ModelPanel::new(app.model_selection_panel().expect("model panel"));
    panel.models.select_value_key("provider:add");
    panel.tab = ModelTab::Info;
    ui.bottom_panel = Some(BottomPanel::Models(panel));

    let buffer = draw_fullscreen_for_test(&app, &mut ui, 120, 24);
    let text = buffer_text(&buffer);

    assert!(text.contains("Select a model row to view metadata."));
}

#[tokio::test]
pub(crate) async fn model_fetch_all_adds_fetched_rows_and_preserves_query() {
    let temp = tempdir().expect("temp");
    let server = TuiCatalogServer::new(r#"{"data":[{"id":"remote-model"},{"id":"mock-model"}]}"#);
    let config_path = temp.path().join("fetch-config.toml");
    fs::write(
        &config_path,
        format!(
            r#"model = "mock/mock-model"

[provider.mock.options]
base_url = "{}"
api_key_env = "TEST_PROVIDER_KEY"

[provider.mock.models."mock-model"]
"#,
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
            panel.models.set_query_char(ch);
        }
        panel.models.select_value_key("fetch:all");
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
    assert_eq!(panel.models.query, "remote");
    assert_eq!(
        panel
            .models
            .filtered_indices()
            .iter()
            .map(|index| panel.models.rows[*index].label.as_str())
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
pub(crate) async fn model_fetch_missing_credentials_stays_in_panel() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let config_path = temp.path().join("missing-config.toml");
    fs::write(
        &config_path,
        r#"model = "mock/mock-model"

[provider.mock.options]
base_url = "http://api.example/v1"
api_key_env = "TEST_PROVIDER_KEY"

[provider.mock.models."mock-model"]
"#,
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
        .models
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
pub(crate) async fn model_add_provider_opens_builtin_preset_picker() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.env_map.insert(
        "PSYCHEVO_HOME".to_string(),
        app.home.to_string_lossy().to_string(),
    );
    fs::write(app.home.join("config.toml"), "\n").expect("config");
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    {
        let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel else {
            panic!("expected model panel");
        };
        panel.models.select_value_key("provider:add");
    }
    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("add provider");

    let Some(BottomPanel::ProviderPresets(panel)) = &ui.bottom_panel else {
        panic!("expected provider preset panel");
    };
    assert_eq!(
        panel
            .rows
            .iter()
            .map(|row| row.label.as_str())
            .collect::<Vec<_>>(),
        vec![
            "DeepSeek",
            "Z.AI / GLM",
            "Xiaomi Token Plan",
            "OpenCode Zen",
            "Custom OpenAI-compatible"
        ]
    );
}

#[tokio::test]
pub(crate) async fn model_add_provider_rejects_psychevo_config_override() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    let config_path = temp.path().join("config.toml");
    fs::write(&config_path, "\n").expect("config");
    app.config_path = Some(config_path);
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    {
        let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel else {
            panic!("expected model panel");
        };
        panel.models.select_value_key("provider:add");
    }
    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("add provider");

    let Some(BottomPanel::Models(panel)) = &ui.bottom_panel else {
        panic!("expected model panel");
    };
    assert_eq!(
        panel.models.notice.as_deref(),
        Some("cannot add provider while PSYCHEVO_CONFIG is active")
    );
}

#[tokio::test]
pub(crate) async fn model_add_builtin_deepseek_saves_fetches_and_hides_secret() {
    let temp = tempdir().expect("temp");
    let server = TuiCatalogServer::new(r#"{"data":[{"id":"remote-model"}]}"#);
    let mut app = test_app(&temp);
    app.env_map.insert(
        "PSYCHEVO_HOME".to_string(),
        app.home.to_string_lossy().to_string(),
    );
    fs::write(app.home.join("config.toml"), "\n").expect("config");
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
        panel.models.select_value_key("provider:add");
    }
    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("add provider");
    {
        let Some(BottomPanel::ProviderPresets(panel)) = &mut ui.bottom_panel else {
            panic!("expected preset panel");
        };
        panel.select_value_key("provider:preset:deepseek");
    }
    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("select deepseek");

    let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel else {
        panic!("expected provider wizard");
    };
    assert!(!panel.is_custom());
    assert_eq!(panel.provider_id, "deepseek");
    assert_eq!(panel.api_key_env, "DEEPSEEK_API_KEY");
    panel.base_url = server.base_url.clone();
    panel.api_key = "test-key".to_string();
    app.refresh_provider_wizard_env_state(&mut ui);
    app.save_provider_wizard(&mut ui).expect("save provider");
    drain_catalog_until_idle(&mut app, &mut ui).await;

    let config = fs::read_to_string(app.home.join("config.toml")).expect("config");
    assert!(config.contains("[provider.deepseek]"));
    assert!(config.contains("label = \"DeepSeek\""));
    assert!(config.contains(&format!("base_url = \"{}\"", server.base_url)));
    assert!(config.contains("api_key_env = \"DEEPSEEK_API_KEY\""));
    assert!(!config.contains("test-key"));
    let env = fs::read_to_string(app.home.join(".env")).expect("env");
    assert_eq!(env, "DEEPSEEK_API_KEY=test-key\n");
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
    assert_eq!(
        panel.models.notice.as_deref(),
        Some("provider saved; fetching models")
    );
    assert!(
        panel
            .models
            .rows
            .iter()
            .any(|row| row.label == "deepseek/remote-model")
    );
}

#[test]
pub(crate) fn model_add_builtin_zai_and_xiaomi_offer_base_url_shortcuts() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);

    let zai_default = app.provider_wizard_panel_for_preset(ProviderSetupPresetId::Zai, Some(0));
    assert_eq!(zai_default.base_url, "https://api.z.ai/api/paas/v4");
    let zai_coding = app.provider_wizard_panel_for_preset(ProviderSetupPresetId::Zai, Some(1));
    assert_eq!(zai_coding.base_url, "https://api.z.ai/api/coding/paas/v4");

    let xiaomi_default =
        app.provider_wizard_panel_for_preset(ProviderSetupPresetId::XiaomiTokenPlan, None);
    assert_eq!(
        xiaomi_default.base_url,
        "https://token-plan-cn.xiaomimimo.com/v1"
    );
    let xiaomi_sgp =
        app.provider_wizard_panel_for_preset(ProviderSetupPresetId::XiaomiTokenPlan, Some(1));
    assert_eq!(
        xiaomi_sgp.base_url,
        "https://token-plan-sgp.xiaomimimo.com/v1"
    );
    let xiaomi_ams =
        app.provider_wizard_panel_for_preset(ProviderSetupPresetId::XiaomiTokenPlan, Some(2));
    assert_eq!(
        xiaomi_ams.base_url,
        "https://token-plan-ams.xiaomimimo.com/v1"
    );
}

#[test]
pub(crate) fn model_add_builtin_xiaomi_writes_canonical_provider_options() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.env_map.insert(
        "PSYCHEVO_HOME".to_string(),
        app.home.to_string_lossy().to_string(),
    );
    fs::write(app.home.join("config.toml"), "\n").expect("config");
    let mut panel =
        app.provider_wizard_panel_for_preset(ProviderSetupPresetId::XiaomiTokenPlan, Some(1));
    panel.api_key = "test-key".to_string();

    let provider_id = app
        .save_provider_wizard_panel(&panel)
        .expect("save provider");

    assert_eq!(provider_id, "xiaomi-token-plan");
    let config = fs::read_to_string(app.home.join("config.toml")).expect("config");
    assert!(config.contains("[provider.xiaomi-token-plan]"));
    assert!(config.contains("label = \"Xiaomi Token Plan\""));
    assert!(config.contains("base_url = \"https://token-plan-sgp.xiaomimimo.com/v1\""));
    assert!(config.contains("api_key_env = \"XIAOMI_TOKEN_PLAN_API_KEY\""));
    assert!(!config.contains("test-key"));
    let env = fs::read_to_string(app.home.join(".env")).expect("env");
    assert_eq!(env, "XIAOMI_TOKEN_PLAN_API_KEY=test-key\n");
}

#[test]
pub(crate) fn model_add_provider_reuses_existing_builtin_env_key() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    fs::write(app.home.join(".env"), "DEEPSEEK_API_KEY=existing\n").expect("env");

    let panel = app.provider_wizard_panel_for_preset(ProviderSetupPresetId::DeepSeek, Some(0));

    assert!(panel.api_key_env_present);
    assert_eq!(panel.api_key_env, "DEEPSEEK_API_KEY");
    assert!(!panel.active_fields().contains(&ProviderWizardField::ApiKey));
    assert!(panel.api_key.is_empty());
}

#[tokio::test]
pub(crate) async fn model_add_provider_saves_global_config_fetches_and_selects_model() {
    let temp = tempdir().expect("temp");
    let server = TuiCatalogServer::new(r#"{"data":[{"id":"remote-model"}]}"#);
    let mut app = test_app(&temp);
    app.env_map.insert(
        "PSYCHEVO_HOME".to_string(),
        app.home.to_string_lossy().to_string(),
    );
    fs::write(app.home.join("config.toml"), "\n").expect("config");
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
        panel.models.select_value_key("provider:add");
    }
    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("add provider");

    {
        let Some(BottomPanel::ProviderPresets(panel)) = &mut ui.bottom_panel else {
            panic!("expected provider preset panel");
        };
        panel.select_value_key("provider:preset:custom");
    }
    let selected = ui
        .bottom_panel
        .as_ref()
        .and_then(BottomPanel::selected_value);
    app.apply_bottom_panel_selection(&mut ui, selected)
        .expect("select custom provider");

    let Some(BottomPanel::ProviderWizard(panel)) = &mut ui.bottom_panel else {
        panic!("expected provider wizard");
    };
    panel.label = "Xiaomi Token Plan CN".to_string();
    panel.provider_id = "xiaomi-token-plan-cn".to_string();
    panel.provider_id_touched = true;
    panel.base_url = server.base_url.clone();
    panel.api_key_env = "XIAOMI_TOKEN_PLAN_CN_API_KEY".to_string();
    panel.api_key_env_touched = true;
    panel.api_key = "test-key".to_string();
    app.refresh_provider_wizard_env_state(&mut ui);
    app.save_provider_wizard(&mut ui).expect("save provider");
    drain_catalog_until_idle(&mut app, &mut ui).await;

    let config = fs::read_to_string(app.home.join("config.toml")).expect("config");
    assert!(config.contains("[provider.xiaomi-token-plan-cn]"));
    assert!(config.contains("label = \"Xiaomi Token Plan CN\""));
    assert!(config.contains("api_key_env = \"XIAOMI_TOKEN_PLAN_CN_API_KEY\""));
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
    assert_eq!(
        panel.models.notice.as_deref(),
        Some("provider saved; fetching models")
    );
    panel
        .models
        .select_value_key("model:xiaomi-token-plan-cn/remote-model");
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
    assert_eq!(app.current_variant, None);
    assert_eq!(
        app.model_state.model_for(&app.workdir_key).as_deref(),
        Some("xiaomi-token-plan-cn/remote-model")
    );
    let selected_model = app.selected_model.as_ref().expect("selected model");
    assert_eq!(selected_model.provider, "xiaomi-token-plan-cn");
    assert_eq!(selected_model.model, "remote-model");
    let config = fs::read_to_string(app.home.join("config.toml")).expect("config");
    assert!(!config.contains("model = \"xiaomi-token-plan-cn/remote-model\""));
    let local_config_path = app.workdir.join(".psychevo/config.toml");
    if local_config_path.exists() {
        let local_config = fs::read_to_string(local_config_path).expect("local config");
        assert!(!local_config.contains("model = \"xiaomi-token-plan-cn/remote-model\""));
    }
}

#[tokio::test]
pub(crate) async fn model_add_provider_wizard_generates_id_and_reports_validation_errors() {
    let temp = tempdir().expect("temp");
    let mut app = test_app(&temp);
    app.env_map.insert(
        "PSYCHEVO_HOME".to_string(),
        app.home.to_string_lossy().to_string(),
    );
    fs::write(app.home.join("config.toml"), "\n").expect("config");
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
    panel.api_key_env = "MIMO_API_KEY".to_string();
    panel.api_key_env_touched = true;
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
    let config = fs::read_to_string(app.home.join("config.toml")).expect("config");
    assert!(!config.contains("mimo"));
}

#[tokio::test]
pub(crate) async fn fetched_model_selection_writes_local_default_model() {
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
        metadata: Default::default(),
    }];
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShow)
        .await
        .expect("model");
    {
        let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel else {
            panic!("expected model panel");
        };
        panel.models.select_value_key("model:mock/remote-model");
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
    assert_eq!(app.current_variant, None);
    assert_eq!(
        app.model_state.model_for(&app.workdir_key).as_deref(),
        Some("mock/remote-model")
    );
    assert!(
        app.model_state
            .recent_model_values()
            .contains(&"mock/remote-model".to_string())
    );
    let local_config_path = app.workdir.join(".psychevo/config.toml");
    if local_config_path.exists() {
        let local_config = fs::read_to_string(local_config_path).expect("local config");
        assert!(!local_config.contains("model = \"mock/remote-model\""));
    }
}

#[tokio::test]
pub(crate) async fn model_global_picker_writes_global_default_model() {
    let temp = tempdir().expect("temp");
    let mut app = test_app_with_models(&temp);
    fs::copy(
        app.config_path.as_ref().expect("config"),
        app.home.join("config.toml"),
    )
    .expect("global config");
    let mut ui = FullscreenUi::new(&app);

    app.handle_fullscreen_command(&mut ui, SlashCommand::ModelShowScoped { global: true })
        .await
        .expect("model");
    {
        let Some(BottomPanel::Models(panel)) = &mut ui.bottom_panel else {
            panic!("expected model panel");
        };
        assert!(panel.global);
        panel.models.select_value_key("model:mock/other-model");
    }
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

    let global_config = fs::read_to_string(app.home.join("config.toml")).expect("global config");
    assert!(global_config.contains("[model]"));
    assert!(global_config.contains("id = \"mock/other-model\""));
    assert!(global_config.contains("reasoning_effort = \"high\""));
    let local_config =
        fs::read_to_string(app.workdir.join(".psychevo/config.toml")).expect("local config");
    assert!(local_config.contains("model = \"mock/mock-model\""));
}

#[test]
pub(crate) fn model_picker_initial_focus_prefers_model_rows_before_fetch_rows() {
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
    let config_path = temp.path().join("empty-model-config.toml");
    fs::write(
        &config_path,
        r#"
[provider.mock.options]
base_url = "http://127.0.0.1:9"

[provider.mock.models]
"#,
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
pub(crate) async fn model_picker_up_down_wraps_between_first_and_last_rows() {
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
        panel.models.rows[panel.models.filtered_indices()[panel.models.selected]].label,
        "mock/other-model"
    );

    app.handle_bottom_panel_key(&mut ui, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .expect("wrap down");
    let Some(BottomPanel::Models(panel)) = &ui.bottom_panel else {
        panic!("expected model panel");
    };
    assert_eq!(
        panel.models.rows[panel.models.filtered_indices()[panel.models.selected]].label,
        "Add provider"
    );
}
