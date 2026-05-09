#[test]
fn model_catalog_endpoint_follows_chat_base_url() {
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
fn model_catalog_providers_resolve_auth_and_no_auth() {
    let temp = tempdir().expect("temp");
    let mut options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::write(
        config_dir.join("config.jsonc"),
        r#"
            {
              "model": "openai/gpt-4.1",
              "provider": {
                "openai": {
                  "options": {
                    "base_url": "http://api.example/v1",
                    "api_key_env": "OPENAI_API_KEY"
                  },
                  "models": { "gpt-4.1": {} }
                },
                "lmstudio": {
                  "options": { "base_url": "http://127.0.0.1:1234/v1" },
                  "models": {}
                }
              }
            }
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
async fn model_catalog_fetch_parses_models_and_sends_auth() {
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
        models,
        vec![
            ModelCatalogEntry {
                id: "alpha".to_string(),
                context_limit: None,
            },
            ModelCatalogEntry {
                id: "zeta".to_string(),
                context_limit: None,
            },
        ]
    );
    let request = server.request();
    assert!(request.starts_with("GET /v1/models HTTP/1.1"));
    assert!(
        request
            .to_lowercase()
            .contains("authorization: bearer secret-key")
    );
}

#[tokio::test]
async fn model_catalog_fetch_omits_auth_for_no_auth_providers() {
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

#[tokio::test]
async fn model_catalog_fetch_times_out() {
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

