#[tokio::test]
async fn channel_wechat_qr_start_generates_svg_for_url_payload() {
    async fn qr_code() -> Json<Value> {
        Json(json!({
            "qrcode": "qr-token",
            "qrcode_img_content": "https://qr.example/wechat"
        }))
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let base_url = format!("http://{}", listener.local_addr().expect("addr"));
    let router = Router::new().route("/ilink/bot/get_bot_qrcode", get(qr_code));
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve");
    });

    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(state.inner.home.join("config.toml"), "# config\n").expect("config");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let started = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "channel/wechat-qr/start".to_string(),
            params: Some(json!({
                "scope": scope,
                "id": "wechat",
                "label": "WeChat",
                "ilinkBaseUrl": base_url
            })),
        },
    )
    .await
    .expect("channel/wechat-qr/start");
    assert_eq!(started["qrUrl"], "https://qr.example/wechat");
    assert!(started["qrImage"].is_null());
    assert!(
        started["qrSvg"]
            .as_str()
            .is_some_and(|value| value.contains("<svg"))
    );
}

#[tokio::test]
async fn settings_read_exposes_resolved_model_without_variant_override() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"model = "deepseek/deepseek-chat"

[provider.deepseek.models."deepseek-chat"]
reasoning_effort = "medium"

[provider.deepseek.models."deepseek-chat-lite"]
reasoning = false
"#,
    )
    .expect("config");
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "settings/read".to_string(),
            params: None,
        },
    )
    .await
    .expect("settings/read");

    assert_eq!(result["controls"]["model"], "deepseek/deepseek-chat");
    assert_eq!(result["controls"]["modelStatus"], "resolved");
    assert_eq!(result["controls"]["modelError"], Value::Null);
    assert!(
        result["controls"]["modelOptions"]
            .as_array()
            .expect("model options")
            .iter()
            .any(|value| value.as_str() == Some("deepseek/deepseek-chat"))
    );
    let model_details = result["controls"]["modelDetails"]
        .as_array()
        .expect("model details");
    let selected_detail = model_details
        .iter()
        .find(|value| value["value"].as_str() == Some("deepseek/deepseek-chat"))
        .expect("selected model detail");
    assert_eq!(selected_detail["provider"].as_str(), Some("deepseek"));
    assert_eq!(selected_detail["id"].as_str(), Some("deepseek-chat"));
    assert_eq!(selected_detail["providerName"].as_str(), Some("DeepSeek"));
    assert_eq!(selected_detail["reasoningSupported"].as_bool(), None);
    assert!(
        selected_detail["reasoningEfforts"]
            .as_array()
            .expect("reasoning efforts")
            .iter()
            .any(|value| value.as_str() == Some("high"))
    );
    let no_reasoning_detail = model_details
        .iter()
        .find(|value| value["value"].as_str() == Some("deepseek/deepseek-chat-lite"))
        .expect("no reasoning model detail");
    assert_eq!(
        no_reasoning_detail["reasoningSupported"].as_bool(),
        Some(false)
    );
    assert_eq!(
        no_reasoning_detail["reasoningEfforts"]
            .as_array()
            .expect("reasoning efforts"),
        &vec![json!("none")]
    );
    assert_eq!(result["controls"]["variant"], "none");
}

#[tokio::test]
async fn web_search_settings_store_secrets_in_profile_env_and_only_return_status() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(state.inner.home.join("config.toml"), "# config\n").expect("config");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope").to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();
    let result = handle_rpc(state.clone(), AuthContext::Bearer, tx, RpcRequest {
        jsonrpc: wire::JSONRPC_VERSION.to_string(), id: Some(json!("web-search")),
        method: "web/search/settings/update".to_string(), params: Some(json!({
            "scope": scope,
            "search": {
                "execution":"local", "backend":"brave", "externalAccess":"live",
                "contextSize":"medium", "returnTokenBudget":"default", "contentTypes":["text"],
                "allowedDomains":[], "blockedDomains":[], "backgroundStorageAcknowledged":false,
                "location": {"country":"", "region":"", "city":"", "timezone":""},
                "image": {"max_results":3, "caption":true},
                "credentials": {"brave":"missing"}
            },
            "credentialValues": {"BRAVE_SEARCH_API_KEY":"super-secret"}
        })),
    }).await.expect("web search settings update");
    assert_eq!(result["backend"], "brave");
    assert_eq!(result["credentials"]["brave"], "present");
    assert!(!result.to_string().contains("super-secret"));
    assert!(!std::fs::read_to_string(state.inner.home.join("config.toml")).unwrap().contains("super-secret"));
    assert!(std::fs::read_to_string(state.inner.home.join(".env")).unwrap().contains("BRAVE_SEARCH_API_KEY="));
}

#[tokio::test]
async fn settings_read_projects_web_search_with_protocol_field_names() {
    let (_temp, state) = web_state();
    let (tx, _rx) = mpsc::unbounded_channel();
    let result = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("web-search-settings-read")),
            method: "settings/read".to_string(),
            params: None,
        },
    )
    .await
    .expect("settings read");

    let search = &result["webSearch"];
    assert_eq!(search["execution"], "local");
    assert_eq!(search["backend"], "exa");
    assert_eq!(search["externalAccess"], "live");
    assert_eq!(search["contextSize"], "medium");
    assert_eq!(search["returnTokenBudget"], "default");
    assert_eq!(search["backgroundStorageAcknowledged"], false);
    assert!(search.get("external_access").is_none());
}

#[tokio::test]
async fn settings_read_reports_model_resolution_errors_without_failing() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"model = { provider = "deepseek", id = "deepseek-chat", reasoning_effort = "turbo" }

[provider.deepseek.models."deepseek-chat"]
"#,
    )
    .expect("config");
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "settings/read".to_string(),
            params: None,
        },
    )
    .await
    .expect("settings/read");

    assert_eq!(result["controls"]["model"], Value::Null);
    assert_eq!(result["controls"]["modelStatus"], "error");
    assert!(
        result["controls"]["modelError"]
            .as_str()
            .is_some_and(|message| message.contains("reasoning_effort")),
        "{result:#}"
    );
}

#[tokio::test]
async fn settings_read_exposes_session_agent() {
    let (_temp, state) = web_state();
    let session = state
        .inner
        .state
        .store()
        .create_session_with_metadata(
            &state.inner.cwd,
            "web",
            "model",
            "provider",
            Some(json!({
                "main_agent": main_agent_metadata(
                    "translate",
                    "translate",
                    psychevo_runtime::AgentSource::Project,
                    None,
                )
            })),
        )
        .expect("session");
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "settings/read".to_string(),
            params: Some(json!({ "threadId": session })),
        },
    )
    .await
    .expect("settings/read");

    assert_eq!(result["controls"]["agent"].as_str(), Some("translate"));
}

#[tokio::test]
async fn settings_update_persists_session_agent_and_default() {
    let (_temp, state) = web_state();
    write_agent_definition(
        &state.inner.cwd.join(".psychevo/agents"),
        "translate",
        "Translate user messages",
    );
    let session = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&state.inner.cwd, "web", "model", "provider", None)
        .expect("session");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "settings/update".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": session,
                "agent": "translate"
            })),
        },
    )
    .await
    .expect("settings/update");

    assert_eq!(result["controls"]["agent"].as_str(), Some("translate"));
    let metadata = state
        .inner
        .state
        .store()
        .session_metadata(&session)
        .expect("metadata")
        .expect("metadata value");
    assert_eq!(metadata["main_agent"]["mode"], "agent");
    assert_eq!(metadata["main_agent"]["name"], "translate");
    assert!(!state.inner.cwd.join(".psychevo/config.toml").exists());

    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();
    let result = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "settings/update".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": session,
                "agent": null
            })),
        },
    )
    .await
    .expect("settings/update");

    assert_eq!(result["controls"]["agent"], Value::Null);
    let metadata = state
        .inner
        .state
        .store()
        .session_metadata(&session)
        .expect("metadata")
        .expect("metadata value");
    assert_eq!(metadata["main_agent"]["mode"], "default");
}

#[tokio::test]
async fn settings_update_rejects_unknown_or_shadowed_session_agent() {
    let (_temp, state) = web_state();
    let project_agents = state.inner.cwd.join(".psychevo/agents");
    let home_agents = state.inner.home.join("agents");
    write_agent_definition(&project_agents, "review", "Project review");
    let shadowed = write_agent_definition(&home_agents, "review", "Global review");
    let session = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&state.inner.cwd, "web", "model", "provider", None)
        .expect("session");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();

    let (tx, _rx) = mpsc::unbounded_channel();
    let active = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "settings/update".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": session,
                "agent": "review"
            })),
        },
    )
    .await
    .expect("active review is valid");
    assert_eq!(active["controls"]["agent"].as_str(), Some("review"));

    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();
    let err = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "settings/update".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": session,
                "agent": shadowed.display().to_string()
            })),
        },
    )
    .await
    .expect_err("shadowed path");
    assert!(
        err.to_string().contains("shadowed agent definitions"),
        "{err:#}"
    );

    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();
    let err = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("3")),
            method: "settings/update".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": session,
                "agent": "missing"
            })),
        },
    )
    .await
    .expect_err("unknown agent");
    assert!(
        err.to_string().contains("unknown agent: missing"),
        "{err:#}"
    );
}
