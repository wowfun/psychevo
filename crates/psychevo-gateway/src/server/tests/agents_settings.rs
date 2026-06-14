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
        assert!(
            result["controls"]["variantOptions"]
                .as_array()
                .expect("variant options")
                .iter()
                .any(|value| value.as_str() == Some("medium"))
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
