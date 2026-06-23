    #[tokio::test]
    async fn agent_and_backend_rpc_list_generated_peer_backend() {
        let (_temp, state) = web_state();
        std::fs::create_dir_all(&state.inner.home).expect("home");
        std::fs::write(
            state.inner.home.join("config.toml"),
            r#"[agents.backends.cursor]
kind = "acp"
description = "Cursor ACP coding agent."
command = "cursor-agent"
"#,
        )
        .expect("config");
        let (tx, _rx) = mpsc::unbounded_channel();

        let backends = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "backend/list".to_string(),
                params: None,
            },
        )
        .await
        .expect("backend/list");
        assert_eq!(backends["backends"][0]["id"], "cursor");
        assert_eq!(backends["backends"][0]["sourceTargets"], json!(["profile"]));

        let write = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("2")),
                method: "backend/write".to_string(),
                params: Some(json!({
                    "id": "opencode",
                    "target": "project",
                    "enabled": true,
                    "label": "OpenCode",
                    "description": "OpenCode ACP coding agent.",
                    "command": "opencode",
                    "args": ["acp"],
                    "entrypoints": ["peer", "subagent"],
                    "clientCapabilities": ["fs.read", "fs.write", "terminal"]
                })),
            },
        )
        .await
        .expect("backend/write");
        assert_eq!(write["backend"]["id"], "opencode");
        assert_eq!(write["backend"]["sourceTargets"], json!(["project"]));

        let backends = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("3")),
                method: "backend/list".to_string(),
                params: None,
            },
        )
        .await
        .expect("backend/list after write");
        let opencode_backend = backends["backends"]
            .as_array()
            .expect("backends")
            .iter()
            .find(|backend| backend["id"] == "opencode")
            .expect("opencode backend");
        assert_eq!(opencode_backend["sourceTargets"], json!(["project"]));
        assert_eq!(opencode_backend["args"], json!(["acp"]));

        let minimal = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("4")),
                method: "backend/write".to_string(),
                params: Some(json!({
                    "id": "minimal-acp",
                    "target": "profile",
                    "enabled": true,
                    "command": "minimal-agent",
                    "args": ["acp"],
                    "entrypoints": ["peer", "subagent"],
                    "clientCapabilities": ["fs.read", "fs.write", "terminal"]
                })),
            },
        )
        .await
        .expect("backend/write minimal");
        assert_eq!(minimal["backend"]["label"], "minimal-acp");
        assert_eq!(minimal["backend"]["description"], Value::Null);
        assert_eq!(minimal["backend"]["diagnostics"], json!([]));

        let agents = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("5")),
                method: "agent/list".to_string(),
                params: None,
            },
        )
        .await
        .expect("agent/list");
        let cursor = agents["agents"]
            .as_array()
            .expect("agents")
            .iter()
            .find(|agent| agent["name"] == "cursor")
            .expect("cursor agent");
        assert_eq!(cursor["generated"], true);
        assert_eq!(cursor["backend"]["ref"], "cursor");
        assert!(agents.get("shadowedAgents").is_some());
        let opencode = agents["agents"]
            .as_array()
            .expect("agents")
            .iter()
            .find(|agent| agent["name"] == "opencode")
            .expect("opencode agent");
        assert_eq!(opencode["backend"]["ref"], "opencode");
        let minimal_agent = agents["agents"]
            .as_array()
            .expect("agents")
            .iter()
            .find(|agent| agent["name"] == "minimal-acp")
            .expect("minimal agent");
        assert_eq!(minimal_agent["description"], "minimal-acp");
        assert_eq!(minimal_agent["backend"]["ref"], "minimal-acp");

        let status = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("6")),
                method: "agent/status".to_string(),
                params: None,
            },
        )
        .await
        .expect("agent/status");
        assert!(status.get("control").is_some());
        assert!(status.get("agents").is_some());

        let delete = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("7")),
                method: "backend/delete".to_string(),
                params: Some(json!({
                    "id": "opencode",
                    "target": "project"
                })),
            },
        )
        .await
        .expect("backend/delete");
        assert_eq!(delete["deleted"], true);
    }

    #[tokio::test]
    async fn backend_profile_write_uses_explicit_config_when_set() {
        let temp = tempfile::tempdir().expect("tempdir");
        let explicit_config = temp.path().join("explicit").join("config.toml");
        let (_state_temp, state) = web_state_with_env(BTreeMap::from([(
            "PSYCHEVO_CONFIG".to_string(),
            explicit_config.to_string_lossy().to_string(),
        )]));
        let (tx, _rx) = mpsc::unbounded_channel();

        let write = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "backend/write".to_string(),
                params: Some(json!({
                    "id": "minimal-acp",
                    "target": "profile",
                    "command": "minimal-agent",
                    "entrypoints": ["peer", "subagent"],
                    "clientCapabilities": ["fs.read", "fs.write", "terminal"]
                })),
            },
        )
        .await
        .expect("backend/write");
        assert_eq!(write["backend"]["id"], "minimal-acp");
        assert_eq!(write["backend"]["label"], "minimal-acp");
        assert_eq!(write["backend"]["description"], Value::Null);

        let backends = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("2")),
                method: "backend/list".to_string(),
                params: None,
            },
        )
        .await
        .expect("backend/list");
        assert!(backends["backends"].as_array().is_some_and(|backends| {
            backends
                .iter()
                .any(|backend| backend["id"] == "minimal-acp")
        }));
    }

    #[tokio::test]
    async fn completion_list_returns_workdir_files() {
        let (_temp, state) = web_state();
        let src = state.inner.workdir.join("src");
        std::fs::create_dir_all(&src).expect("src");
        std::fs::write(src.join("main.rs"), "fn main() {}\n").expect("main");
        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();

        let result = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "completion/list".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "text": "@src/ma",
                    "cursor": 7
                })),
            },
        )
        .await
        .expect("completion/list");

        let labels = result["items"]
            .as_array()
            .expect("items")
            .iter()
            .filter_map(|item| item["label"].as_str())
            .collect::<Vec<_>>();
        assert!(labels.contains(&"@src/main.rs"));
    }

    #[tokio::test]
    async fn completion_list_returns_agent_mentions_for_at_prefix() {
        let (_temp, state) = web_state();
        write_agent_definition(
            &state.inner.workdir.join(".psychevo/agents"),
            "review",
            "Review the current task.",
        );
        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
        let (tx, _rx) = mpsc::unbounded_channel();

        let result = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "completion/list".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "text": "@rev",
                    "cursor": 4
                })),
            },
        )
        .await
        .expect("completion/list");

        let items = result["items"].as_array().expect("items");
        let item = items
            .iter()
            .find(|item| item["label"] == "@review")
            .expect("review agent completion");
        assert_eq!(item["sigil"], "@");
        assert_eq!(item["kind"], "agent");
        assert_eq!(item["target"]["kind"], "agent");
        assert_eq!(item["target"]["name"], "review");
        assert!(
            item["target"]["entrypoints"]
                .as_array()
                .expect("entrypoints")
                .iter()
                .any(|entrypoint| entrypoint.as_str() == Some("subagent"))
        );
    }

    #[tokio::test]
    async fn settings_read_returns_workbench_project_and_controls() {
        let (_temp, state) = web_state();
        let (tx, _rx) = mpsc::unbounded_channel();

        let result = handle_rpc(
            state.clone(),
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

        let workdir = state.inner.workdir.display().to_string();
        assert_eq!(result["project"]["path"].as_str(), Some(workdir.as_str()));
        assert!(
            result["project"]["displayPath"]
                .as_str()
                .is_some_and(|path| path.ends_with("/work") || path == "work"),
            "{result:#}"
        );
        assert_eq!(result["controls"]["permissionMode"], "default");
        assert_eq!(result["controls"]["mode"], "default");
        assert_eq!(result["controls"]["agent"], Value::Null);
        assert_eq!(result["controls"]["model"], Value::Null);
        assert_eq!(result["controls"]["modelStatus"], "unconfigured", "{result:#}");
        assert_eq!(result["controls"]["modelError"], Value::Null);
        assert_eq!(result["controls"]["variant"], "none");
        assert!(
            result["controls"]["variantOptions"]
                .as_array()
                .expect("variant options")
                .iter()
                .any(|value| value.as_str() == Some("medium"))
        );
    }

    #[tokio::test]
    async fn model_state_rpc_saves_workdir_selection_and_controls_recent_models() {
        let (_temp, state) = web_state();
        std::fs::create_dir_all(&state.inner.home).expect("home");
        let (tx, _rx) = mpsc::unbounded_channel();
        let workdir = state.inner.workdir.display().to_string();

        let saved = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("model-state-set")),
                method: "model/state/set".to_string(),
                params: Some(json!({
                    "workdir": workdir,
                    "model": "mock/model-a",
                    "reasoningEffort": "high"
                })),
            },
        )
        .await
        .expect("model/state/set");

        assert_eq!(saved["model"], "mock/model-a");
        assert_eq!(saved["reasoningEffort"], "high");
        assert_eq!(saved["recentModels"], json!(["mock/model-a"]));

        let read = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("model-state-read")),
                method: "model/state/read".to_string(),
                params: Some(json!({ "workdir": state.inner.workdir.display().to_string() })),
            },
        )
        .await
        .expect("model/state/read");
        assert_eq!(read["model"], "mock/model-a");
        assert_eq!(read["reasoningEffort"], "high");

        let settings = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("settings-read")),
                method: "settings/read".to_string(),
                params: Some(json!({ "workdir": state.inner.workdir.display().to_string() })),
            },
        )
        .await
        .expect("settings/read");
        assert_eq!(settings["controls"]["model"], "mock/model-a");
        assert_eq!(settings["controls"]["variant"], "high");
        assert_eq!(settings["controls"]["recentModels"], json!(["mock/model-a"]));

        let model_state = ModelState::load(&ModelState::path_for_home(&state.inner.home))
            .expect("model state");
        assert_eq!(
            model_state
                .model_for(state.inner.workdir.to_string_lossy().as_ref())
                .as_deref(),
            Some("mock/model-a")
        );
    }

    #[tokio::test]
    async fn model_state_rpc_with_thread_updates_session_model_metadata() {
        let (_temp, state) = web_state();
        std::fs::create_dir_all(&state.inner.home).expect("home");
        let session_id = state
            .inner
            .state
            .store()
            .create_session_with_metadata(&state.inner.workdir, "web", "old-model", "old", None)
            .expect("session");
        let (tx, _rx) = mpsc::unbounded_channel();

        let saved = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("model-state-thread-set")),
                method: "model/state/set".to_string(),
                params: Some(json!({
                    "threadId": session_id,
                    "model": "mock/model-b",
                    "reasoningEffort": "low"
                })),
            },
        )
        .await
        .expect("model/state/set");
        assert_eq!(saved["threadId"], session_id);
        assert_eq!(saved["model"], "mock/model-b");
        assert_eq!(saved["reasoningEffort"], "low");

        let summary = state
            .inner
            .state
            .store()
            .session_summary(&session_id)
            .expect("summary")
            .expect("session");
        assert_eq!(summary.provider, "mock");
        assert_eq!(summary.model, "model-b");
        let metadata = state
            .inner
            .state
            .store()
            .session_metadata(&session_id)
            .expect("metadata")
            .expect("metadata");
        assert_eq!(
            metadata[SESSION_COMPOSER_MODEL_METADATA_KEY]["reasoningEffort"],
            "low"
        );

        let settings = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("settings-read-thread")),
                method: "settings/read".to_string(),
                params: Some(json!({ "threadId": session_id })),
            },
        )
        .await
        .expect("settings/read");
        assert_eq!(settings["controls"]["model"], "mock/model-b");
        assert_eq!(settings["controls"]["variant"], "low");
    }

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
workdir = "/tmp/project"
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
            .create_session_with_metadata(&state.inner.workdir, "channel", "model", "provider", None)
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
                    "workdir": "",
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
        assert_eq!(updated["channel"]["workdir"], Value::Null);
        assert_eq!(updated["channel"]["model"], Value::Null);
        assert_eq!(updated["channel"]["permissionMode"], Value::Null);
        assert_eq!(updated["channel"]["requireMention"], false);
        assert_eq!(updated["channel"]["credential"]["env"], "TELEGRAM_BOT_TOKEN");
        assert_eq!(updated["channel"]["allowlist"]["users"], json!(["alice", "bob"]));
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

        let same_workdir_thread = state
            .inner
            .state
            .store()
            .create_session_with_metadata(&state.inner.workdir, "channel", "model", "provider", None)
            .expect("same workdir thread");
        state
            .inner
            .state
            .store()
            .upsert_gateway_source_binding(psychevo_runtime::GatewaySourceBindingInput {
                source_key: "im.telegram:same-workdir-lane",
                source_kind: "im.telegram",
                raw_identity: json!({
                    "connectionId": "release",
                    "chatId": "same-workdir-lane",
                }),
                visible_name: Some("Same workdir lane"),
                thread_id: &same_workdir_thread,
                backend_kind: "psychevo",
                backend_native_id: Some(&same_workdir_thread),
                lineage: None,
            })
            .expect("same workdir binding");
        handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("same-workdir")),
                method: "channel/update".to_string(),
                params: Some(json!({
                    "scope": scope.clone(),
                    "id": "release",
                    "workdir": ""
                })),
            },
        )
        .await
        .expect("same workdir channel/update");
        assert_eq!(
            state
                .inner
                .state
                .store()
                .gateway_source_binding("im.telegram:same-workdir-lane")
                .expect("same workdir binding lookup")
                .expect("same workdir binding")
                .thread_id,
            same_workdir_thread
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
            .create_session_with_metadata(&state.inner.workdir, "channel", "model", "provider", None)
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
        assert_eq!(source["workdir"], state.inner.workdir.display().to_string());
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
        assert_eq!(
            started["qrImage"],
            "data:image/png;base64,wechat-qr-image"
        );
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
        async fn models() -> Json<Value> {
            Json(json!({
                "data": [
                    { "id": "beta" },
                    { "id": "alpha-free" }
                ]
            }))
        }

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

        let settings = handle_rpc(
            state.clone(),
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

        let model_settings = handle_rpc(
            state,
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
    }

    #[tokio::test]
    async fn model_settings_global_scope_ignores_project_model_override() {
        let (_temp, state) = web_state();
        std::fs::create_dir_all(&state.inner.home).expect("home");
        std::fs::create_dir_all(state.inner.workdir.join(".psychevo")).expect("project config dir");
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
            state.inner.workdir.join(".psychevo/config.toml"),
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
                    "workdir": state.inner.workdir.display().to_string()
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
        let project_config = std::fs::read_to_string(
            state.inner.workdir.join(".psychevo/config.toml"),
        )
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
                    "workdir": state.inner.workdir.display().to_string()
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
                params: Some(json!({ "workdir": model_settings["workdir"] })),
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
        assert!(started["qrSvg"].as_str().is_some_and(|value| value.contains("<svg")));
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
        assert_eq!(
            selected_detail["providerLabel"].as_str(),
            Some("DeepSeek")
        );
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
                &state.inner.workdir,
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
            &state.inner.workdir.join(".psychevo/agents"),
            "translate",
            "Translate user messages",
        );
        let session = state
            .inner
            .state
            .store()
            .create_session_with_metadata(&state.inner.workdir, "web", "model", "provider", None)
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
        assert!(!state.inner.workdir.join(".psychevo/config.toml").exists());

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
        let project_agents = state.inner.workdir.join(".psychevo/agents");
        let home_agents = state.inner.home.join("agents");
        write_agent_definition(&project_agents, "review", "Project review");
        let shadowed = write_agent_definition(&home_agents, "review", "Global review");
        let session = state
            .inner
            .state
            .store()
            .create_session_with_metadata(&state.inner.workdir, "web", "model", "provider", None)
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
