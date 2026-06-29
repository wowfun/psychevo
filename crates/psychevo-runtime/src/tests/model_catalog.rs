#[allow(unused_imports)]
pub(crate) use super::*;
#[test]
pub(crate) fn model_catalog_endpoint_follows_chat_base_url() {
    assert_eq!(
        model_catalog_endpoint("https://api.example.com/v1"),
        "https://api.example.com/v1/models"
    );
    assert_eq!(
        model_catalog_endpoint("https://api.example.com/v1/"),
        "https://api.example.com/v1/models"
    );
    assert_eq!(
        model_catalog_endpoint("https://api.example.com/v1/chat/completions"),
        "https://api.example.com/v1/models"
    );
    assert_eq!(
        model_catalog_endpoint("https://api.example.com/v1/chat/completions/"),
        "https://api.example.com/v1/models"
    );
}

#[test]
pub(crate) fn model_catalog_providers_resolve_auth_and_no_auth() {
    let temp = tempdir().expect("temp");
    let mut options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    write_config(
        config_dir.join("config.toml"),
        r#"
model = "openai/gpt-4.1"

[provider.openai.options]
base_url = "http://api.example/v1"
api_key_env = "OPENAI_API_KEY"

[provider.openai.models."gpt-4.1"]

[provider.lmstudio.options]
base_url = "http://127.0.0.1:1234/v1"

[provider.lmstudio.models]
"#,
    )
    .expect("config");
    options.inherited_env = Some(BTreeMap::from([(
        "HOME".to_string(),
        temp.path().to_string_lossy().to_string(),
    )]));

    let providers = model_catalog_providers(&options).expect("providers");
    let openai = providers
        .iter()
        .find(|provider| provider.provider == "openai")
        .expect("openai");
    assert_eq!(
        openai.missing_credentials.as_deref(),
        Some("OPENAI_API_KEY")
    );
    assert!(!openai.fetchable());
    let lmstudio = providers
        .iter()
        .find(|provider| provider.provider == "lmstudio")
        .expect("lmstudio");
    assert_eq!(lmstudio.missing_credentials, None);
    assert!(lmstudio.no_auth);
    assert!(lmstudio.fetchable());
}

#[tokio::test]
pub(crate) async fn model_catalog_fetch_parses_models_and_sends_auth() {
    let server = CatalogServer::new(r#"{"data":[{"id":"zeta"},{"id":"alpha"},{"id":""}]}"#);
    let provider = ModelCatalogProvider {
        provider: "openai".to_string(),
        display_label: "OpenAI".to_string(),
        base_url: server.base_url.clone(),
        api_key_env: Some("OPENAI_API_KEY".to_string()),
        missing_credentials: None,
        unavailable_reason: None,
        no_auth: false,
        api_key: Some("secret-key".to_string()),
    };

    let models =
        fetch_model_catalog_with_client(&provider, &reqwest::Client::new(), Duration::from_secs(2))
            .await
            .expect("fetch");

    assert_eq!(
        models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "zeta"]
    );
    assert!(models.iter().all(|model| model.context_limit.is_none()));
    assert!(
        models
            .iter()
            .all(|model| model.metadata.source.as_deref() == Some("provider"))
    );
    let request = server.request();
    assert!(request.starts_with("GET /v1/models HTTP/1.1"));
    assert!(
        request
            .to_lowercase()
            .contains("authorization: bearer secret-key")
    );
    assert!(request.to_lowercase().contains("user-agent: psychevo/"));
}

#[test]
pub(crate) fn provider_model_cache_writes_reads_and_strips_private_data() {
    let temp = tempdir().expect("temp");
    let home = home_dir(&temp);
    let provider = cached_provider("openai", "https://api.example.com/v1", Some("mock-api-key"));
    let mut metadata = ModelMetadata {
        source: Some("provider".to_string()),
        raw: Some(json!({"credential": "mock-api-key", "raw": true})),
        ..Default::default()
    };
    metadata.limits.context = Some(123_000);
    let models = vec![ModelCatalogEntry {
        id: "mock-model".to_string(),
        context_limit: Some(123_000),
        metadata,
    }];

    write_cached_model_catalog(&home, &provider, &models).expect("write cache");

    let cached = read_cached_model_catalog(&home, &provider).expect("cached models");
    assert_eq!(cached.len(), 1);
    assert_eq!(cached[0].id, "mock-model");
    assert_eq!(cached[0].context_limit, Some(123_000));
    assert_eq!(cached[0].metadata.raw, None);
    let text =
        fs::read_to_string(provider_models_cache_path_for_home(&home)).expect("cache content");
    assert!(text.contains("mock-model"));
    assert!(!text.contains("mock-api-key"));
    assert!(!text.contains("\"raw\""));
}

#[test]
pub(crate) fn provider_model_cache_ignores_invalid_json_and_fingerprint_mismatch() {
    let temp = tempdir().expect("temp");
    let home = home_dir(&temp);
    let cache_path = provider_models_cache_path_for_home(&home);
    fs::create_dir_all(cache_path.parent().expect("cache dir")).expect("cache dir");
    fs::write(&cache_path, "{not-json").expect("invalid cache");
    let provider = cached_provider("openai", "https://api.example.com/v1", Some("first-key"));
    assert!(read_cached_model_catalog(&home, &provider).is_none());

    write_cached_model_catalog(
        &home,
        &provider,
        &[ModelCatalogEntry {
            id: "cached".to_string(),
            context_limit: None,
            metadata: ModelMetadata::default(),
        }],
    )
    .expect("write cache");
    let changed_key = cached_provider("openai", "https://api.example.com/v1", Some("second-key"));
    assert!(read_cached_model_catalog(&home, &changed_key).is_none());
    let changed_url = cached_provider("openai", "https://api.example.com/other", Some("first-key"));
    assert!(read_cached_model_catalog(&home, &changed_url).is_none());
}

#[tokio::test]
pub(crate) async fn provider_model_cache_empty_live_result_keeps_old_cache() {
    let temp = tempdir().expect("temp");
    let home = home_dir(&temp);
    let server = CatalogServer::new(r#"{"data":[]}"#);
    let provider = cached_provider("lmstudio", &server.base_url, None);
    write_cached_model_catalog(
        &home,
        &provider,
        &[ModelCatalogEntry {
            id: "old-model".to_string(),
            context_limit: None,
            metadata: ModelMetadata::default(),
        }],
    )
    .expect("write old cache");

    let models = fetch_and_cache_model_catalog(&home, &provider)
        .await
        .expect("fetch empty catalog");

    assert!(models.is_empty());
    let cached = read_cached_model_catalog(&home, &provider).expect("old cache");
    assert_eq!(cached[0].id, "old-model");
}

#[tokio::test]
pub(crate) async fn model_catalog_fetch_omits_auth_for_no_auth_providers() {
    let server = CatalogServer::new(r#"{"data":[]}"#);
    let provider = ModelCatalogProvider {
        provider: "lmstudio".to_string(),
        display_label: "LM Studio".to_string(),
        base_url: server.base_url.clone(),
        api_key_env: None,
        missing_credentials: None,
        unavailable_reason: None,
        no_auth: true,
        api_key: None,
    };

    fetch_model_catalog_with_client(&provider, &reqwest::Client::new(), Duration::from_secs(2))
        .await
        .expect("fetch");

    let request = server.request();
    assert!(request.starts_with("GET /v1/models HTTP/1.1"));
    assert!(!request.to_lowercase().contains("authorization:"));
}

fn cached_provider(provider: &str, base_url: &str, api_key: Option<&str>) -> ModelCatalogProvider {
    ModelCatalogProvider {
        provider: provider.to_string(),
        display_label: provider.to_string(),
        base_url: base_url.to_string(),
        api_key_env: api_key.map(|_| "MOCK_API_KEY".to_string()),
        missing_credentials: None,
        unavailable_reason: None,
        no_auth: api_key.is_none(),
        api_key: api_key.map(str::to_string),
    }
}

#[test]
pub(crate) fn opencode_zen_catalog_provider_allows_no_auth_without_config_entry() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let home = home_dir(&temp);
    fs::create_dir_all(&home).expect("home");
    write_config(home.join("config.toml"), "# initialized\n").expect("config");
    let provider = model_catalog_provider(&options, "zen")
        .expect("provider")
        .expect("zen provider");

    assert_eq!(provider.provider, "opencode-zen");
    assert_eq!(provider.base_url, "https://opencode.ai/zen/v1");
    assert_eq!(provider.missing_credentials, None);
    assert!(provider.no_auth);
    assert!(provider.fetchable());
}

#[test]
pub(crate) fn opencode_zen_free_models_are_classified_from_ids() {
    let free = ModelCatalogEntry {
        id: "mimo-v2.5-free".to_string(),
        context_limit: None,
        metadata: ModelMetadata::default(),
    };
    let stealth = ModelCatalogEntry {
        id: "big-pickle".to_string(),
        context_limit: None,
        metadata: ModelMetadata::default(),
    };
    let paid = ModelCatalogEntry {
        id: "deepseek-v4-pro".to_string(),
        context_limit: None,
        metadata: ModelMetadata::default(),
    };

    assert!(model_catalog_entry_is_free("opencode-zen", &free));
    assert!(model_catalog_entry_is_free("zen", &stealth));
    assert!(!model_catalog_entry_is_free("opencode-zen", &paid));
}

#[tokio::test]
pub(crate) async fn model_catalog_fetch_times_out() {
    let server = CatalogServer::with_delay(r#"{"data":[]}"#, Duration::from_millis(80));
    let provider = ModelCatalogProvider {
        provider: "lmstudio".to_string(),
        display_label: "LM Studio".to_string(),
        base_url: server.base_url.clone(),
        api_key_env: None,
        missing_credentials: None,
        unavailable_reason: None,
        no_auth: true,
        api_key: None,
    };

    let err = fetch_model_catalog_with_client(
        &provider,
        &reqwest::Client::new(),
        Duration::from_millis(5),
    )
    .await
    .expect_err("timeout");

    assert_eq!(err.to_string(), "timeout");
}
