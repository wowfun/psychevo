#[tokio::test]
async fn model_settings_rpc_saves_zen_no_auth_and_auxiliary_assignment() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(state.inner.home.join("config.toml"), "# config\n").expect("config");
    let (tx, _rx) = mpsc::unbounded_channel();

    let saved = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("model-save")),
            method: "model/provider/save".to_string(),
            params: Some(json!({
                "scope": "global",
                "providerId": "zen",
                "label": "OpenCode Zen",
                "baseUrl": "https://opencode.ai/zen/v1",
                "apiKeyEnv": null,
                "apiKey": null,
                "noAuth": true
            })),
        },
    )
    .await
    .expect("model/provider/save");
    let zen = saved["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|provider| provider["id"] == "opencode-zen")
        .expect("zen provider");
    assert_eq!(zen["configured"], true);
    assert_eq!(zen["noAuth"], true);
    assert_eq!(zen["credentialStatus"], "notRequired");

    let assignment = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("model-assignment")),
            method: "model/assignment/set".to_string(),
            params: Some(json!({
                "scope": "global",
                "target": "auxiliary",
                "task": "title_generation",
                "provider": "opencode",
                "model": "mimo-v2.5-free",
                "reasoningEffort": "high"
            })),
        },
    )
    .await
    .expect("model/assignment/set");
    assert_eq!(assignment["ok"], true);
    assert_eq!(assignment["provider"], "opencode-zen");
    assert_eq!(assignment["reasoningEffort"], "high");

    let config = std::fs::read_to_string(state.inner.home.join("config.toml")).expect("config");
    assert!(config.contains("[provider.opencode-zen.options]"));
    assert!(config.contains("no_auth = true"));
    assert!(!config.contains("api_key_env"));
    assert!(config.contains("[auxiliary.title_generation]"));
    assert!(config.contains("provider = \"opencode-zen\""));
    assert!(config.contains("id = \"mimo-v2.5-free\""));
    assert!(config.contains("reasoning_effort = \"high\""));

    let read = handle_rpc(
        state,
        AuthContext::Bearer,
        mpsc::unbounded_channel().0,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("model-settings-read")),
            method: "model/settings/read".to_string(),
            params: Some(json!({ "scope": "global" })),
        },
    )
    .await
    .expect("model/settings/read");
    let title = read["auxiliary"]
        .as_array()
        .expect("auxiliary")
        .iter()
        .find(|value| value["task"].as_str() == Some("title_generation"))
        .expect("title generation assignment");
    assert_eq!(title["reasoningEffort"], "high");
}

#[tokio::test]
async fn model_provider_catalog_rpc_fetches_fake_catalog() {
    let request_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let request_count_for_route = Arc::clone(&request_count);
    let models = move || {
        let request_count = Arc::clone(&request_count_for_route);
        async move {
            request_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Json(json!({
                "data": [
                    { "id": "beta" },
                    { "id": "alpha-free" }
                ]
            }))
        }
    };

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let base_url = format!("http://{}/v1", listener.local_addr().expect("addr"));
    let router = Router::new().route("/v1/models", get(models));
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve");
    });

    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        format!(
            r#"
[provider.localmodels]
label = "Local Models"

[provider.localmodels.options]
base_url = "{base_url}"
no_auth = true
"#
        ),
    )
    .expect("config");
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("catalog")),
            method: "model/provider/catalog".to_string(),
            params: Some(json!({
                "scope": "global",
                "providerId": "localmodels",
                "refresh": true
            })),
        },
    )
    .await
    .expect("model/provider/catalog");

    assert_eq!(result["providerId"], "localmodels");
    assert_eq!(result["models"][0]["value"], "localmodels/alpha-free");
    assert_eq!(result["models"][1]["value"], "localmodels/beta");
    assert_eq!(request_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    let cache_path = psychevo_runtime::provider_models_cache_path_for_home(&state.inner.home);
    let cache = std::fs::read_to_string(cache_path).expect("provider cache");
    assert!(cache.contains("alpha-free"));
    assert!(!cache.contains("gateway-mock-key"));

    let fresh_state = WebState::new(GatewayWebServerConfig::new(
        Gateway::new(state.inner.state.clone()),
        state.inner.home.clone(),
        state.inner.cwd.clone(),
        state.inner.config_path.clone(),
        state.inner.inherited_env.clone(),
        state
            .inner
            .static_dir
            .clone()
            .unwrap_or_else(|| state.inner.home.join("static")),
    ));

    let settings = handle_rpc(
        fresh_state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("settings")),
            method: "settings/read".to_string(),
            params: None,
        },
    )
    .await
    .expect("settings/read");
    assert!(
        settings["controls"]["modelDetails"]
            .as_array()
            .expect("model details")
            .iter()
            .any(|value| value["value"].as_str() == Some("localmodels/alpha-free"))
    );
    assert_eq!(request_count.load(std::sync::atomic::Ordering::SeqCst), 1);

    let model_settings = handle_rpc(
        fresh_state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("model-settings")),
            method: "model/settings/read".to_string(),
            params: Some(json!({ "scope": "global" })),
        },
    )
    .await
    .expect("model/settings/read");
    assert!(
        model_settings["modelOptions"]
            .as_array()
            .expect("model options")
            .iter()
            .any(|value| value["value"].as_str() == Some("localmodels/beta"))
    );
    assert_eq!(request_count.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn model_settings_global_scope_ignores_project_model_override() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::create_dir_all(state.inner.cwd.join(".psychevo")).expect("project config dir");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"
[model]
id = "opencode-zen/big-pickle"
reasoning_effort = "high"

[provider.opencode-zen.options]
base_url = "https://opencode.ai/zen/v1"
no_auth = true

[provider.xiaomi-token-plan]
label = "Xiaomi Token Plan"

[provider.xiaomi-token-plan.options]
base_url = "https://token-plan-cn.xiaomimimo.com/v1"
no_auth = true
"#,
    )
    .expect("global config");
    std::fs::write(
        state.inner.cwd.join(".psychevo/config.toml"),
        r#"
[model]
id = "xiaomi-token-plan/mimo-v2.5-pro"
reasoning_effort = "high"
"#,
    )
    .expect("project config");
    let (tx, _rx) = mpsc::unbounded_channel();

    let model_settings = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("model-settings")),
            method: "model/settings/read".to_string(),
            params: Some(json!({
                "scope": "global",
                "cwd": state.inner.cwd.display().to_string()
            })),
        },
    )
    .await
    .expect("model/settings/read");
    assert_eq!(model_settings["defaultModel"], "opencode-zen/big-pickle");
    assert_eq!(model_settings["defaultReasoningEffort"], "high");

    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("assignment")),
            method: "model/assignment/set".to_string(),
            params: Some(json!({
                "scope": "global",
                "target": "default",
                "provider": "opencode-zen",
                "model": "claude-haiku-4-5",
                "reasoningEffort": "low"
            })),
        },
    )
    .await
    .expect("model/assignment/set");
    let global_config =
        std::fs::read_to_string(state.inner.home.join("config.toml")).expect("global config");
    let project_config = std::fs::read_to_string(state.inner.cwd.join(".psychevo/config.toml"))
        .expect("project config");
    assert!(global_config.contains("id = \"opencode-zen/claude-haiku-4-5\""));
    assert!(project_config.contains("id = \"xiaomi-token-plan/mimo-v2.5-pro\""));

    let model_settings = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("model-settings-after-save")),
            method: "model/settings/read".to_string(),
            params: Some(json!({
                "scope": "global",
                "cwd": state.inner.cwd.display().to_string()
            })),
        },
    )
    .await
    .expect("model/settings/read after save");
    assert_eq!(
        model_settings["defaultModel"],
        "opencode-zen/claude-haiku-4-5"
    );
    assert_eq!(model_settings["defaultReasoningEffort"], "low");

    let settings = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("settings")),
            method: "settings/read".to_string(),
            params: Some(json!({ "cwd": model_settings["cwd"].as_str().expect("cwd") })),
        },
    )
    .await
    .expect("settings/read");
    assert_eq!(
        settings["controls"]["model"],
        "xiaomi-token-plan/mimo-v2.5-pro"
    );
    assert_eq!(settings["controls"]["variant"], "none");
}
