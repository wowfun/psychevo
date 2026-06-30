#[tokio::test]
async fn settings_read_and_channel_rpc_expose_secret_free_channels() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"[[channels.connections]]
id = "release"
channel = "telegram"
label = "Release Bot"
transport = "polling"
enabled = false
credential_env = "TELEGRAM_BOT_TOKEN"
allow_users = ["12345"]
"#,
    )
    .expect("config");
    std::fs::write(
        state.inner.home.join(".env"),
        "TELEGRAM_BOT_TOKEN=telegram-secret\n",
    )
    .expect("env");
    let (tx, _rx) = mpsc::unbounded_channel();

    let settings = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "settings/read".to_string(),
            params: None,
        },
    )
    .await
    .expect("settings/read");
    assert_eq!(settings["channels"]["channels"][0]["id"], "release");
    assert_eq!(settings["channels"]["channels"][0]["channel"], "telegram");
    assert_eq!(
        settings["channels"]["channels"][0]["credential"]["status"],
        "present"
    );
    assert_eq!(
        settings["channels"]["channels"][0]["runtimeStatus"],
        "disabled"
    );
    assert!(!settings.to_string().contains("telegram-secret"));

    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let enabled = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "channel/enable".to_string(),
            params: Some(json!({
                "scope": scope,
                "id": "release",
                "enabled": true
            })),
        },
    )
    .await
    .expect("channel/enable");
    assert_eq!(enabled["channel"]["enabled"], true);
    assert_eq!(enabled["channel"]["runtimeStatus"], "ready");
    assert!(!enabled.to_string().contains("telegram-secret"));

    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let doctor = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("3")),
            method: "channel/doctor".to_string(),
            params: Some(json!({
                "scope": scope,
                "id": "release",
                "live": false
            })),
        },
    )
    .await
    .expect("channel/doctor");
    assert_eq!(doctor["channels"][0]["runtimeStatus"], "ready");
    assert_eq!(doctor["channels"][0]["checks"][0]["status"], "ok");
    assert_eq!(doctor["channels"][0]["checks"][1]["status"], "ok");
    assert!(!doctor.to_string().contains("telegram-secret"));
}

#[tokio::test]
async fn channel_update_and_delete_rpc_refresh_settings_without_exposing_secrets() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"[[channels.connections]]
id = "release"
channel = "telegram"
label = "Release Bot"
transport = "polling"
enabled = false
cwd = "/tmp/project"
model = "provider/model"
permission_mode = "dontAsk"
credential_env = "CUSTOM_TELEGRAM_TOKEN"
allow_users = ["12345"]
"#,
    )
    .expect("config");
    std::fs::write(
        state.inner.home.join(".env"),
        "CUSTOM_TELEGRAM_TOKEN=old-secret\nTELEGRAM_BOT_TOKEN=telegram-secret\n",
    )
    .expect("env");
    let (tx, _rx) = mpsc::unbounded_channel();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let bound_thread = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&state.inner.cwd, "channel", "model", "provider", None)
        .expect("bound thread");
    state
        .inner
        .state
        .store()
        .upsert_gateway_source_binding(psychevo_runtime::GatewaySourceBindingInput {
            source_key: "im.telegram:release-lane",
            source_kind: "im.telegram",
            raw_identity: json!({
                "connectionId": "release",
                "chatId": "release-lane",
            }),
            visible_name: Some("Release lane"),
            thread_id: &bound_thread,
            backend_kind: "psychevo",
            backend_native_id: Some(&bound_thread),
            lineage: None,
        })
        .expect("source binding");

    let updated = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "channel/update".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "id": "release",
                "label": "Ops Bot",
                "enabled": true,
                "cwd": "",
                "model": "",
                "permissionMode": "default",
                "requireMention": false,
                "credentialEnv": "",
                "allowUsers": ["alice", "alice", "bob"],
                "allowGroups": ["team", "team"]
            })),
        },
    )
    .await
    .expect("channel/update");
    assert_eq!(updated["channel"]["label"], "Ops Bot");
    assert_eq!(updated["channel"]["enabled"], true);
    assert_eq!(updated["channel"]["cwd"], Value::Null);
    assert_eq!(updated["channel"]["model"], Value::Null);
    assert_eq!(updated["channel"]["permissionMode"], Value::Null);
    assert_eq!(updated["channel"]["requireMention"], false);
    assert_eq!(
        updated["channel"]["credential"]["env"],
        "TELEGRAM_BOT_TOKEN"
    );
    assert_eq!(
        updated["channel"]["allowlist"]["users"],
        json!(["alice", "bob"])
    );
    assert_eq!(updated["channel"]["allowlist"]["groups"], json!(["team"]));
    assert!(!updated.to_string().contains("telegram-secret"));
    assert!(!updated.to_string().contains("old-secret"));
    assert!(
        state
            .inner
            .state
            .store()
            .gateway_source_binding("im.telegram:release-lane")
            .expect("rotated binding lookup")
            .is_none()
    );
    let bound_summary = state
        .inner
        .state
        .store()
        .session_summary(&bound_thread)
        .expect("bound summary")
        .expect("bound session");
    assert_eq!(
        bound_summary.end_reason.as_deref(),
        Some("channel_workspace_changed")
    );
    assert!(bound_summary.archived_at_ms.is_some());

    let same_cwd_thread = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&state.inner.cwd, "channel", "model", "provider", None)
        .expect("same cwd thread");
    state
        .inner
        .state
        .store()
        .upsert_gateway_source_binding(psychevo_runtime::GatewaySourceBindingInput {
            source_key: "im.telegram:same-cwd-lane",
            source_kind: "im.telegram",
            raw_identity: json!({
                "connectionId": "release",
                "chatId": "same-cwd-lane",
            }),
            visible_name: Some("Same cwd lane"),
            thread_id: &same_cwd_thread,
            backend_kind: "psychevo",
            backend_native_id: Some(&same_cwd_thread),
            lineage: None,
        })
        .expect("same cwd binding");
    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("same-cwd")),
            method: "channel/update".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "id": "release",
                "cwd": ""
            })),
        },
    )
    .await
    .expect("same cwd channel/update");
    assert_eq!(
        state
            .inner
            .state
            .store()
            .gateway_source_binding("im.telegram:same-cwd-lane")
            .expect("same cwd binding lookup")
            .expect("same cwd binding")
            .thread_id,
        same_cwd_thread
    );

    let settings = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "settings/read".to_string(),
            params: None,
        },
    )
    .await
    .expect("settings/read");
    assert_eq!(settings["channels"]["channels"][0]["label"], "Ops Bot");
    assert_eq!(
        settings["channels"]["channels"][0]["credential"]["status"],
        "present"
    );
    assert!(!settings.to_string().contains("telegram-secret"));

    let source_list_thread = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&state.inner.cwd, "channel", "model", "provider", None)
        .expect("source list thread");
    state
        .inner
        .state
        .store()
        .upsert_gateway_source_binding(psychevo_runtime::GatewaySourceBindingInput {
            source_key: "im.telegram:source-hash",
            source_kind: "im.telegram",
            raw_identity: json!({
                "connectionId": "release",
                "platform": "telegram",
                "domain": "telegram",
                "chatType": "dm",
                "chatId": "raw-chat-123456",
                "userId": "raw-user-654321"
            }),
            visible_name: Some("raw-chat-123456/raw-user-654321"),
            thread_id: &source_list_thread,
            backend_kind: "psychevo",
            backend_native_id: Some(&source_list_thread),
            lineage: None,
        })
        .expect("source list binding");
    let sources = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("sources")),
            method: "channel/source/list".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "id": "release"
            })),
        },
    )
    .await
    .expect("channel/source/list");
    let source = sources["sources"]
        .as_array()
        .expect("sources array")
        .iter()
        .find(|source| source["sourceKey"] == "im.telegram:source-hash")
        .expect("source view");
    assert_eq!(source["threadId"], source_list_thread);
    assert_eq!(
        source["cwd"].as_str(),
        Some(state.inner.cwd.display().to_string().as_str())
    );
    assert_eq!(source["activityStatus"], "idle");
    assert!(source["chatLabel"].as_str().unwrap().contains("..."));
    assert!(source["userLabel"].as_str().unwrap().contains("..."));
    let source_json = source.to_string();
    assert!(!source_json.contains("raw-chat-123456"));
    assert!(!source_json.contains("raw-user-654321"));

    let env = std::fs::read_to_string(state.inner.home.join(".env")).expect("env");
    assert!(env.contains("CUSTOM_TELEGRAM_TOKEN=old-secret"));
    assert!(env.contains("TELEGRAM_BOT_TOKEN=telegram-secret"));

    let deleted = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("3")),
            method: "channel/delete".to_string(),
            params: Some(json!({
                "scope": scope,
                "id": "release"
            })),
        },
    )
    .await
    .expect("channel/delete");
    assert_eq!(deleted["channels"], json!([]));
    assert!(!deleted.to_string().contains("telegram-secret"));

    let settings_after_delete = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("4")),
            method: "settings/read".to_string(),
            params: None,
        },
    )
    .await
    .expect("settings/read after delete");
    assert_eq!(settings_after_delete["channels"]["channels"], json!([]));
    let env = std::fs::read_to_string(state.inner.home.join(".env")).expect("env");
    assert!(env.contains("CUSTOM_TELEGRAM_TOKEN=old-secret"));
    assert!(env.contains("TELEGRAM_BOT_TOKEN=telegram-secret"));
}

#[tokio::test]
async fn channel_wechat_qr_rpc_connects_and_writes_secret_free_config() {
    async fn qr_code() -> Json<Value> {
        Json(json!({
            "qrcode": "qr-token",
            "qrcode_img_content": "data:image/png;base64,wechat-qr-image"
        }))
    }

    async fn qr_status() -> Json<Value> {
        Json(json!({
            "status": "confirmed",
            "ilink_bot_id": "wx-account",
            "bot_token": "wechat-secret",
            "ilink_user_id": "wx-user"
        }))
    }

    async fn get_updates() -> Json<Value> {
        Json(json!({
            "ret": 0,
            "errcode": 0,
            "msgs": [],
            "get_updates_buf": "healthy"
        }))
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let base_url = format!("http://{}", listener.local_addr().expect("addr"));
    let router = Router::new()
        .route("/ilink/bot/get_bot_qrcode", get(qr_code))
        .route("/ilink/bot/get_qrcode_status", get(qr_status))
        .route("/ilink/bot/getupdates", post(get_updates));
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
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
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
    let session_id = started["sessionId"].as_str().expect("session id");
    assert_eq!(started["qrImage"], "data:image/png;base64,wechat-qr-image");
    assert!(started["qrSvg"].is_null());
    assert!(!started.to_string().contains("wechat-secret"));

    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let polled = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "channel/wechat-qr/poll".to_string(),
            params: Some(json!({
                "scope": scope,
                "sessionId": session_id,
                "enable": true
            })),
        },
    )
    .await
    .expect("channel/wechat-qr/poll");
    assert_eq!(polled["done"], true);
    assert_eq!(polled["status"], "qr_login_pending");
    assert_eq!(polled["channel"]["channel"], "wechat");
    assert_eq!(polled["channel"]["runtimeStatus"], "ready");
    assert_eq!(polled["channel"]["runner"]["reason"], "qr_login_pending");
    assert!(!polled.to_string().contains("wechat-secret"));

    let config = std::fs::read_to_string(state.inner.home.join("config.toml")).expect("config");
    assert!(config.contains("channel = \"wechat\""));
    assert!(config.contains("account_env = \"WECHAT_ACCOUNT_ID\""));
    assert!(config.contains("base_url_env = \"WECHAT_ILINK_BASE_URL\""));
    assert!(config.contains("allow_users = [\"wx-user\"]"));
    assert!(!config.contains("wechat-secret"));
    let env = std::fs::read_to_string(state.inner.home.join(".env")).expect("env");
    assert!(env.contains("WECHAT_BOT_TOKEN=wechat-secret"));
    assert!(env.contains("WECHAT_ACCOUNT_ID=wx-account"));
    assert!(env.contains(&format!("WECHAT_ILINK_BASE_URL={base_url}")));
}

#[tokio::test]
async fn channel_wechat_qr_rpc_persists_confirmed_token_without_health_gate() {
    async fn qr_code() -> Json<Value> {
        Json(json!({
            "qrcode": "qr-token",
            "qrcode_img_content": "data:image/png;base64,wechat-qr-image"
        }))
    }

    async fn qr_status() -> Json<Value> {
        Json(json!({
            "status": "confirmed",
            "ilink_bot_id": "wx-account",
            "bot_token": "dead-wechat-secret",
            "ilink_user_id": "wx-user"
        }))
    }

    async fn get_updates() -> Json<Value> {
        Json(json!({
            "errcode": -14,
            "errmsg": "session timeout"
        }))
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let base_url = format!("http://{}", listener.local_addr().expect("addr"));
    let router = Router::new()
        .route("/ilink/bot/get_bot_qrcode", get(qr_code))
        .route("/ilink/bot/get_qrcode_status", get(qr_status))
        .route("/ilink/bot/getupdates", post(get_updates));
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve");
    });

    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"[[channels.connections]]
id = "wechat"
channel = "wechat"
label = "WeChat"
transport = "polling"
enabled = true
credential_env = "WECHAT_BOT_TOKEN"
account_env = "WECHAT_ACCOUNT_ID"
base_url_env = "WECHAT_ILINK_BASE_URL"
allow_users = ["existing-user"]
"#,
    )
    .expect("config");
    std::fs::write(
        state.inner.home.join(".env"),
        "WECHAT_BOT_TOKEN=working-secret\nWECHAT_ACCOUNT_ID=old-account\n",
    )
    .expect("env");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let started = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
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
    let session_id = started["sessionId"].as_str().expect("session id");

    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let polled = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "channel/wechat-qr/poll".to_string(),
            params: Some(json!({
                "scope": scope,
                "sessionId": session_id,
                "enable": true
            })),
        },
    )
    .await
    .expect("channel/wechat-qr/poll");
    assert_eq!(polled["done"], true);
    assert_eq!(polled["status"], "qr_login_pending");
    assert_eq!(polled["channel"]["runner"]["reason"], "qr_login_pending");
    assert!(!polled.to_string().contains("dead-wechat-secret"));

    let config = std::fs::read_to_string(state.inner.home.join("config.toml")).expect("config");
    assert!(config.contains("allow_users = [\"wx-user\"]"));
    let env = std::fs::read_to_string(state.inner.home.join(".env")).expect("env");
    assert!(env.contains("WECHAT_BOT_TOKEN=dead-wechat-secret"));
    assert!(env.contains("WECHAT_ACCOUNT_ID=wx-account"));
    assert!(!env.contains("working-secret"));
}
