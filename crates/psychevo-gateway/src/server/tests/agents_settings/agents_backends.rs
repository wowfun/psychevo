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
async fn runtime_profile_rpc_lists_generated_profiles_and_writes_overrides() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    let (tx, _rx) = mpsc::unbounded_channel();

    let profiles = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("runtime-1")),
            method: "runtime/profile/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("runtime/profile/list");
    let ids = profiles["profiles"]
        .as_array()
        .expect("profiles")
        .iter()
        .map(|profile| profile["id"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert!(ids.contains(&"native"));
    assert!(ids.contains(&"codex"));
    assert!(ids.contains(&"opencode"));
    assert_eq!(
        profiles["profiles"]
            .as_array()
            .expect("profiles")
            .iter()
            .find(|profile| profile["id"] == "native")
            .expect("native")["generated"],
        true
    );

    let write = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("runtime-2")),
            method: "runtime/profile/write".to_string(),
            params: Some(json!({
                "id": "codex",
                "target": "profile",
                "runtime": "codex",
                "enabled": false,
                "label": "Codex local",
                "command": "codex",
                "args": ["app-server", "--stdio"],
                "defaultMode": "auto-review"
            })),
        },
    )
    .await
    .expect("runtime/profile/write");
    assert_eq!(write["profile"]["id"], "codex");
    assert_eq!(write["profile"]["generated"], false);
    assert_eq!(write["profile"]["enabled"], false);
    assert_eq!(write["profile"]["defaultMode"], "auto-review");

    let health = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("runtime-3")),
            method: "runtime/health/check".to_string(),
            params: Some(json!({ "runtimeRef": "codex" })),
        },
    )
    .await
    .expect("runtime/health/check");
    assert_eq!(health["health"]["status"], "disabled");
    assert!(health["health"]["checkedAtMs"].as_i64().is_some());

    let options = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("runtime-4")),
            method: "runtime/options".to_string(),
            params: Some(json!({
                "scope": default_resolved_scope(&state, &AuthContext::Bearer)
                    .expect("scope")
                    .to_wire_scope(),
                "runtimeRef": "codex"
            })),
        },
    )
    .await
    .expect("runtime/options");
    assert_eq!(options["runtimeRef"], "codex");
    assert_eq!(options["options"][0]["id"], "mode");
}

#[tokio::test]
async fn turn_start_rejects_direct_runtime_profile_without_adapter_worker() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let (tx, _rx) = mpsc::unbounded_channel();

    let err = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("runtime-turn")),
            method: "turn/start".to_string(),
            params: Some(json!({
                "scope": scope.to_wire_scope(),
                "runtimeRef": "opencode",
                "input": [{"type": "text", "text": "use direct opencode"}]
            })),
        },
    )
    .await
    .expect_err("direct runtime turn should fail before native execution");
    assert!(
        err.to_string()
            .contains("runtime profile `opencode` uses a direct opencode runtime"),
        "{err}"
    );
    assert_eq!(
        state
            .inner
            .gateway
            .resolve_source_thread(&scope.source)
            .expect("source binding"),
        None
    );
}

#[tokio::test]
async fn runtime_profile_guard_prefers_same_named_acp_peer_backend() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"[agents.backends.opencode]
kind = "acp"
command = "opencode"
args = ["acp"]
entrypoints = ["peer", "subagent"]
"#,
    )
    .expect("config");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");

    ensure_turn_runtime_profile_supported(&state, &scope, Some("opencode"))
        .expect("ACP peer backend should take precedence over generated direct profile");
}

#[tokio::test]
async fn backend_list_auto_creates_detected_local_acp_backends() {
    let bin = tempfile::tempdir().expect("bin tempdir");
    write_command_shim(&bin.path().join("opencode"));
    write_command_shim(&bin.path().join("hermes"));
    let (_temp, state) = web_state_with_env(BTreeMap::from([(
        "PATH".to_string(),
        bin.path().display().to_string(),
    )]));
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
    let records = backends["backends"].as_array().expect("backends");
    let opencode = records
        .iter()
        .find(|backend| backend["id"] == "opencode")
        .expect("opencode backend");
    assert_eq!(opencode["command"], "opencode");
    assert_eq!(opencode["args"], json!(["acp"]));
    assert_eq!(opencode["sourceTargets"], json!(["profile"]));
    assert_eq!(opencode["entrypoints"], json!(["peer", "subagent"]));
    let hermes = records
        .iter()
        .find(|backend| backend["id"] == "hermes")
        .expect("hermes backend");
    assert_eq!(hermes["command"], "hermes");
    assert_eq!(hermes["args"], json!(["acp"]));
    assert_eq!(hermes["sourceTargets"], json!(["profile"]));

    let config = std::fs::read_to_string(state.inner.home.join("config.toml")).expect("config");
    assert!(config.contains("[agents.backends.opencode]"));
    assert!(config.contains("[agents.backends.hermes]"));
    assert!(config.contains("command = \"opencode\""));
    assert!(config.contains("command = \"hermes\""));

    let agents = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "agent/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("agent/list");
    let agent_records = agents["agents"].as_array().expect("agents");
    assert!(agent_records.iter().any(|agent| agent["name"] == "opencode"));
    assert!(agent_records.iter().any(|agent| agent["name"] == "hermes"));
}

#[tokio::test]
async fn backend_list_does_not_auto_create_over_existing_effective_backend() {
    let bin = tempfile::tempdir().expect("bin tempdir");
    write_command_shim(&bin.path().join("hermes"));
    let (_temp, state) = web_state_with_env(BTreeMap::from([(
        "PATH".to_string(),
        bin.path().display().to_string(),
    )]));
    let project_config = state.inner.cwd.join(".psychevo").join("config.toml");
    std::fs::create_dir_all(project_config.parent().expect("project config parent"))
        .expect("project config dir");
    std::fs::write(
        &project_config,
        r#"[agents.backends.hermes]
kind = "acp"
label = "Project Hermes"
command = "custom-hermes"
args = ["serve-acp"]
"#,
    )
    .expect("project config");
    let (tx, _rx) = mpsc::unbounded_channel();

    let backends = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "backend/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("backend/list");
    let hermes = backends["backends"]
        .as_array()
        .expect("backends")
        .iter()
        .find(|backend| backend["id"] == "hermes")
        .expect("hermes backend");
    assert_eq!(hermes["label"], "Project Hermes");
    assert_eq!(hermes["command"], "custom-hermes");
    assert_eq!(hermes["args"], json!(["serve-acp"]));
    assert_eq!(hermes["sourceTargets"], json!(["project"]));
    assert!(!state.inner.home.join("config.toml").exists());
}

fn write_command_shim(path: &Path) {
    std::fs::write(path, "#!/bin/sh\n").expect("command shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path).expect("command shim metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("command shim permissions");
    }
}

#[tokio::test]
async fn agent_rpc_manages_project_profile_disabled_and_raw_definitions() {
    let (_temp, state) = web_state();
    let project_agents = state.inner.cwd.join(".psychevo/agents");
    let profile_agents = state.inner.home.join("agents");
    std::fs::create_dir_all(&project_agents).expect("project agents");
    std::fs::create_dir_all(&profile_agents).expect("profile agents");
    std::fs::write(
        project_agents.join("review.md"),
        "---\ndescription: Project review\nenabled: false\nmodel: keep-model\n---\nProject body.\n",
    )
    .expect("project agent");
    std::fs::write(
        profile_agents.join("review.md"),
        "---\ndescription: Profile review\n---\nProfile body.\n",
    )
    .expect("profile agent");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let list = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "agent/list".to_string(),
            params: Some(json!({ "scope": scope.clone() })),
        },
    )
    .await
    .expect("agent/list");
    let active = list["agents"]
        .as_array()
        .expect("agents")
        .iter()
        .find(|agent| agent["name"] == "review")
        .expect("active review");
    assert_eq!(active["description"], "Profile review");
    assert_eq!(active["enabled"], true);
    assert_eq!(active["target"], "profile");
    let disabled = list["disabledAgents"]
        .as_array()
        .expect("disabled agents")
        .iter()
        .find(|agent| agent["name"] == "review")
        .expect("disabled project review");
    assert_eq!(disabled["target"], "project");
    assert_eq!(disabled["mutable"], true);

    let read_project = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "agent/read".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "review",
                "target": "project"
            })),
        },
    )
    .await
    .expect("agent/read project");
    assert_eq!(read_project["agent"]["enabled"], false);
    assert!(read_project["rawMarkdown"]
        .as_str()
        .expect("raw")
        .contains("model: keep-model"));

    let write_project = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("3")),
            method: "agent/write".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "review",
                "target": "project",
                "description": "Project review updated",
                "enabled": true,
                "instructions": "Updated body.",
                "entrypoints": ["subagent"],
                "tools": ["read"],
                "mcpServers": ["repo"]
            })),
        },
    )
    .await
    .expect("agent/write project");
    assert_eq!(write_project["agent"]["enabled"], true);
    assert_eq!(write_project["agent"]["target"], "project");
    let text = std::fs::read_to_string(project_agents.join("review.md")).expect("project text");
    assert!(text.contains("model: keep-model"));
    assert!(text.contains("enabled: true"));
    assert!(text.contains("mcpServers"));

    let list = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("4")),
            method: "agent/list".to_string(),
            params: Some(json!({ "scope": scope.clone() })),
        },
    )
    .await
    .expect("agent/list after enable");
    let active = list["agents"]
        .as_array()
        .expect("agents")
        .iter()
        .find(|agent| agent["name"] == "review")
        .expect("project active review");
    assert_eq!(active["description"], "Project review updated");
    assert_eq!(active["target"], "project");
    assert!(list["shadowedAgents"]
        .as_array()
        .expect("shadowed")
        .iter()
        .any(|agent| agent["name"] == "review" && agent["target"] == "profile"));

    let invalid_raw = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("5")),
            method: "agent/write".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "raw-agent",
                "target": "profile",
                "description": "Ignored",
                "rawMarkdown": "---\nname: other\ndescription: Other\n---\nOther.\n"
            })),
        },
    )
    .await;
    assert!(invalid_raw.is_err());

    let raw = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("6")),
            method: "agent/write".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "raw-agent",
                "target": "profile",
                "description": "Ignored",
                "rawMarkdown": "---\nname: raw-agent\ndescription: Raw agent\nenabled: false\n---\nRaw body.\n"
            })),
        },
    )
    .await
    .expect("raw write");
    assert_eq!(raw["agent"]["enabled"], false);
    assert_eq!(raw["target"], "profile");

    let enabled = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("7")),
            method: "agent/setEnabled".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "raw-agent",
                "target": "profile",
                "enabled": true
            })),
        },
    )
    .await
    .expect("set enabled");
    assert_eq!(enabled["agent"]["enabled"], true);

    let deleted = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("8")),
            method: "agent/delete".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "raw-agent",
                "target": "profile"
            })),
        },
    )
    .await
    .expect("delete raw");
    assert_eq!(deleted["deleted"], true);
}

#[tokio::test]
async fn team_rpc_round_trips_project_definition() {
    let (_temp, state) = web_state();
    let (tx, _rx) = mpsc::unbounded_channel();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();

    let written = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "team/write".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "ship",
                "target": "project",
                "description": "Ship changes",
                "enabled": true,
                "leader": "general",
                "maxParallelAgents": 8,
                "members": [
                    {"id": "researcher", "agent": "general", "role": "research"},
                    {"id": "tester", "agent": "general", "maxTurns": 2}
                ],
                "instructions": "Coordinate the release."
            })),
        },
    )
    .await
    .expect("team/write");
    assert_eq!(written["team"]["name"], "ship");
    assert_eq!(written["team"]["maxParallelAgents"], 4);

    let list = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "team/list".to_string(),
            params: Some(json!({ "scope": scope.clone() })),
        },
    )
    .await
    .expect("team/list");
    assert!(list["teams"]
        .as_array()
        .expect("teams")
        .iter()
        .any(|team| team["name"] == "ship" && team["target"] == "project"));

    let read = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("3")),
            method: "team/read".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "ship",
                "target": "project"
            })),
        },
    )
    .await
    .expect("team/read");
    assert_eq!(read["team"]["leader"], "general");
    assert!(read["rawMarkdown"]
        .as_str()
        .expect("raw")
        .contains("members"));

    let disabled = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("4")),
            method: "team/setEnabled".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "ship",
                "target": "project",
                "enabled": false
            })),
        },
    )
    .await
    .expect("team/setEnabled");
    assert_eq!(disabled["team"]["enabled"], false);

    let deleted = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("5")),
            method: "team/delete".to_string(),
            params: Some(json!({
                "scope": scope,
                "name": "ship",
                "target": "project"
            })),
        },
    )
    .await
    .expect("team/delete");
    assert_eq!(deleted["deleted"], true);
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
