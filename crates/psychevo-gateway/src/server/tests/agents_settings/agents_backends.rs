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

#[test]
fn backend_doctor_resolves_windows_pathext_command_shim() {
    let temp = tempfile::tempdir().expect("temp");
    let bin = temp.path().join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let shim = bin.join("opencode.cmd");
    std::fs::write(&shim, "@echo off\n").expect("shim");
    let backend = AgentBackendConfig {
        id: "opencode".to_string(),
        kind: psychevo_runtime::AgentBackendKind::Acp,
        enabled: true,
        label: "OpenCode".to_string(),
        description: None,
        command: Some("opencode".to_string()),
        args: vec!["acp".to_string()],
        env: BTreeMap::new(),
        cwd: "invocation".to_string(),
        entrypoints: [AgentEntrypoint::Peer].into_iter().collect(),
        client_capabilities: std::collections::BTreeSet::new(),
        mcp_servers: std::collections::BTreeSet::new(),
    };
    let env = BTreeMap::from([
        ("PATH".to_string(), bin.display().to_string()),
        ("PATHEXT".to_string(), ".CMD".to_string()),
    ]);

    let result = super::agents::backend_doctor_value_for_platform(
        &backend,
        &env,
        temp.path(),
        HostPlatform::Windows,
    )
    .expect("doctor");
    let command = result
        .checks
        .iter()
        .find(|check| check.name == "command")
        .expect("command check");

    assert!(command.ok);
    assert_eq!(command.message, "command resolved");
    assert_eq!(command.path.as_deref(), Some(shim.to_string_lossy().as_ref()));
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
