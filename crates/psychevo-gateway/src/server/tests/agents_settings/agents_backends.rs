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
    let cursor = backends["backends"]
        .as_array()
        .expect("backends")
        .iter()
        .find(|backend| backend["id"] == "cursor")
        .expect("cursor backend");
    assert_eq!(cursor["sourceTargets"], json!(["profile"]));

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
async fn runtime_profile_rpc_omits_launch_fields_from_visible_profiles() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"[agents.backends.reviewer]
kind = "acp"
command = "reviewer-agent"
"#,
    )
    .expect("backend config");
    let (tx, _rx) = mpsc::unbounded_channel();

    let profiles = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("profile-list")),
            method: "runtime/profile/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("runtime/profile/list");
    let profiles = profiles["profiles"].as_array().expect("profiles");
    let native = profiles
        .iter()
        .find(|profile| profile["id"] == "native")
        .expect("native profile");
    assert_eq!(native["runtime"], "native");
    for profile in profiles {
        assert!(profile.get("command").is_none());
        assert!(profile.get("args").is_none());
        assert!(profile.get("envKeys").is_none());
    }

    let written = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("profile-write")),
            method: "runtime/profile/write".to_string(),
            params: Some(json!({
                "id": "reviewer",
                "target": "profile",
                "runtime": "acp",
                "enabled": true,
                "label": "Reviewer",
                "backendRef": "reviewer",
                "defaultModel": "model-a"
            })),
        },
    )
    .await
    .expect("runtime/profile/write");
    assert_eq!(written["profile"]["runtime"], "acp");
    assert_eq!(written["profile"]["backendRef"], "reviewer");
    let config_text = std::fs::read_to_string(
        written["path"]
            .as_str()
            .expect("Runtime Profile config path"),
    )
    .expect("Runtime Profile config");
    let config = config_text
        .parse::<toml::Value>()
        .expect("parse Runtime Profile config");
    let profile_config = config
        .get("runtime_profiles")
        .and_then(|profiles| profiles.get("reviewer"))
        .and_then(toml::Value::as_table)
        .expect("reviewer Runtime Profile table");
    for launch_field in ["command", "args", "env"] {
        assert!(
            !profile_config.contains_key(launch_field),
            "{launch_field} leaked into Runtime Profile config: {config_text}"
        );
    }

    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let context = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("thread-context")),
            method: "thread/context/read".to_string(),
            params: Some(json!({
                "target": {"agentRef": "reviewer", "runtimeProfileRef": "reviewer"},
                "scope": scope.to_wire_scope()
            })),
        },
    )
    .await
    .expect("thread/context/read");
    assert_eq!(context["runtimeProfileRef"], "reviewer");
    assert_eq!(context["selectionState"], "prospective");
    assert_eq!(context["controls"][0]["id"], "model");
    assert_eq!(context["controls"][0]["effectiveValue"], "model-a");
}

#[tokio::test]
async fn managed_codex_target_is_runnable_only_after_verified_install() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    let paths = crate::managed_acp::managed_codex_acp_paths(
        &state.inner.home,
        HostPlatform::current(),
    );
    std::fs::write(
        state.inner.home.join("config.toml"),
        format!(
            r#"[agents.backends.codex]
kind = "acp"
command = {:?}
entrypoints = ["peer", "subagent"]
"#,
            paths.executable.display().to_string()
        ),
    )
    .expect("backend config");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let (tx, _rx) = mpsc::unbounded_channel();

    let read_context = |id: &'static str| {
        handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(id)),
                method: "thread/context/read".to_string(),
                params: Some(json!({
                    "scope": scope.to_wire_scope(),
                    "target": {"agentRef": "codex", "runtimeProfileRef": "codex"}
                })),
            },
        )
    };

    let missing = read_context("managed-codex-missing")
        .await
        .expect("missing context");
    assert_eq!(missing["sendability"]["allowed"], false);
    assert_eq!(missing["sendability"]["recoveryAction"], "backend/install");
    assert!(missing["sendability"]["reason"]
        .as_str()
        .unwrap_or_default()
        .contains("not installed"));
    let error = ensure_turn_runtime_profile_supported(&state, &scope, Some("codex"))
        .expect_err("missing managed install must reject turn");
    assert!(error.to_string().contains("backend/install"));

    std::fs::create_dir_all(paths.executable.parent().expect("launcher parent"))
        .expect("managed launcher parent");
    std::fs::write(&paths.executable, "surviving launcher only").expect("stale launcher");
    state.invalidate_runnable_target_catalog();
    let invalid = read_context("managed-codex-invalid")
        .await
        .expect("invalid context");
    assert_eq!(invalid["sendability"]["allowed"], false);
    assert_eq!(invalid["sendability"]["recoveryAction"], "backend/repair");
    assert!(invalid["sendability"]["reason"]
        .as_str()
        .unwrap_or_default()
        .contains("backend/repair"));
    let error = ensure_turn_runtime_profile_supported(&state, &scope, Some("codex"))
        .expect_err("invalid managed install must reject turn");
    assert!(error.to_string().contains("backend/repair"));
}

#[tokio::test]
async fn runtime_profile_registry_uses_public_shortcuts_without_duplicate_acp_rows() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"[agents.backends.opencode]
kind = "acp"
command = "opencode"
args = ["acp"]

[agents.backends.cursor]
kind = "acp"
command = "cursor-agent"
"#,
    )
    .expect("config");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");

    let profiles = runtime_profile_list_result(&state, &scope).expect("profiles");
    let opencode = profiles
        .profiles
        .iter()
        .find(|profile| profile.id == "opencode")
        .expect("public OpenCode ACP profile");
    assert_eq!(opencode.runtime, "acp");
    assert_eq!(opencode.provenance, "ACP");
    assert_eq!(opencode.backend_ref.as_deref(), Some("opencode"));
    assert!(
        profiles
            .profiles
            .iter()
            .all(|profile| profile.id != "acp:opencode"),
        "public shortcuts must suppress duplicate generated ACP rows"
    );
    let cursor = profiles
        .profiles
        .iter()
        .find(|profile| profile.id == "acp:cursor")
        .expect("arbitrary ACP backend profile");
    assert_eq!(cursor.backend_ref.as_deref(), Some("cursor"));
    assert!(
        super::runtime_profiles::resolve_runtime_ref_peer_turn(&state, &scope, "opencode")
            .expect("ACP resolution")
            .is_some()
    );
}

#[tokio::test]
async fn bound_hidden_acp_profile_starts_follow_up_from_captured_target() {
    let host_env = std::env::vars().collect::<BTreeMap<_, _>>();
    let host_cwd = std::env::current_dir().expect("host cwd");
    let python = ["python3", "python"]
        .into_iter()
        .find_map(|command| {
            resolve_executable_path(
                command,
                &host_cwd,
                &ExecutableResolveOptions {
                    platform: HostPlatform::current(),
                    env: &host_env,
                },
            )
        })
        .expect("Python is required by the ACP fixture");
    let (_temp, state) = web_state();
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_acp_lifecycle.py");
    let log = state.inner.cwd.join("bound-hidden-profile.jsonl");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let wire_scope = scope.to_wire_scope();
    let (tx, mut rx) = mpsc::unbounded_channel();

    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("write-hidden-profile-backend")),
            method: "backend/write".to_string(),
            params: Some(json!({
                "id": "opencode",
                "target": "project",
                "command": python,
                "args": [fixture],
                "env": {
                    "ACP_LIFECYCLE_LOG": log,
                    "ACP_LIFECYCLE_MODE": "all"
                },
                "entrypoints": ["peer", "subagent"]
            })),
        },
    )
    .await
    .expect("backend/write");

    let profiles = runtime_profile_list_result(&state, &scope).expect("profiles");
    assert!(profiles.profiles.iter().any(|profile| profile.id == "opencode"));
    assert!(
        profiles
            .profiles
            .iter()
            .all(|profile| profile.id != "acp:opencode")
    );

    let catalog = psychevo_runtime::discover_agents(&psychevo_runtime::AgentDiscoveryOptions {
        home: state.inner.home.clone(),
        cwd: state.inner.cwd.clone(),
        env: state.inner.inherited_env.clone(),
        explicit_inputs: vec!["opencode".to_string()],
        no_agents: false,
    })
    .expect("Agent catalog");
    let agent = catalog
        .agents
        .iter()
        .find(|agent| agent.name == "opencode")
        .expect("generated OpenCode Agent")
        .clone();
    let agent_json = serde_json::to_string(&agent).expect("Agent snapshot");
    let agent_fingerprint = crate::gateway_agent_definition_fingerprint(&agent_json);
    let profile = RuntimeProfileConfig {
        id: "acp:opencode".to_string(),
        runtime: RuntimeProfileKind::Acp,
        enabled: true,
        label: "OpenCode (ACP)".to_string(),
        backend_ref: Some("opencode".to_string()),
        default_model: None,
        default_mode: None,
        default_agent: None,
        approval_mode: None,
        sandbox: None,
        workspace_roots: Vec::new(),
        options: Value::Null,
    };
    let profile_json = serde_json::to_string(&profile).expect("Profile snapshot");
    let profile_fingerprint = crate::runtime_profile_config_fingerprint(&profile);
    let profile_revision = crate::runtime_profile_config_revision(&profile_fingerprint).to_string();
    let parent_thread_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&state.inner.cwd, "web", "model", "provider", None)
        .expect("parent Thread");
    let thread_id = state
        .inner
        .state
        .store()
        .create_child_session_with_metadata(
            &parent_thread_id,
            &state.inner.cwd,
            "peer_agent",
            "opencode",
            "acp:opencode",
            None,
        )
        .expect("child Thread");
    let cwd = state.inner.cwd.display().to_string();
    state
        .inner
        .state
        .store()
        .create_gateway_runtime_binding(psychevo_runtime::GatewayRuntimeBindingInput {
            thread_id: &thread_id,
            agent_ref: Some("opencode"),
            agent_fingerprint: &agent_fingerprint,
            agent_definition_json: &agent_json,
            runtime_ref: "acp:opencode",
            backend_kind: "acp",
            native_kind: "acp",
            native_session_id: Some("bound-native"),
            cwd: &cwd,
            profile_fingerprint: &profile_fingerprint,
            profile_revision: &profile_revision,
            profile_config_json: &profile_json,
            adapter_kind: "acp",
            adapter_revision: "test",
            ownership: GatewayRuntimeBindingOwnership::ReadWrite,
            parent_thread_id: Some(&parent_thread_id),
        })
        .expect("captured child binding");

    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("remove-current-peer-entrypoint")),
            method: "backend/write".to_string(),
            params: Some(json!({
                "id": "opencode",
                "target": "project",
                "command": python,
                "args": [fixture],
                "env": {
                    "ACP_LIFECYCLE_LOG": log,
                    "ACP_LIFECYCLE_MODE": "all"
                },
                "entrypoints": ["subagent"]
            })),
        },
    )
    .await
    .expect("current Agent target no longer admits peer entrypoint");

    let context = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("bound-hidden-context")),
            method: "thread/context/read".to_string(),
            params: Some(json!({
                "scope": wire_scope,
                "threadId": thread_id
            })),
        },
    )
    .await
    .expect("bound Thread Context");
    assert_eq!(context["runtimeProfileRef"], "acp:opencode");
    assert_eq!(context["sendability"]["allowed"], true);

    let accepted = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("bound-hidden-follow-up")),
            method: "turn/start".to_string(),
            params: Some(json!({
                "scope": wire_scope,
                "threadId": thread_id,
                "input": [{"type": "text", "text": "follow up"}],
                "expectedContextRevision": context["contextRevision"],
                "expectedControlRevision": context["controlRevision"]
            })),
        },
    )
    .await
    .expect("bound hidden Profile follow-up is accepted");
    assert_eq!(accepted["accepted"], true);

    let terminal = tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(message) = rx.recv().await {
            if message.contains("\"type\":\"turnCompleted\"") {
                return message;
            }
        }
        String::new()
    })
    .await
    .expect("follow-up terminal");
    assert!(terminal.contains("\"status\":\"completed\""), "{terminal}");
    assert!(!terminal.contains("unknown runtime profile"), "{terminal}");

    let context = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        mpsc::unbounded_channel().0,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("bound-hidden-context-after-turn")),
            method: "thread/context/read".to_string(),
            params: Some(json!({
                "scope": wire_scope,
                "threadId": thread_id
            })),
        },
    )
    .await
    .expect("bound Thread Context after follow-up");
    assert!(context["actions"].as_array().is_some_and(|actions| {
        actions
            .iter()
            .any(|action| action["id"] == "fork" && action["enabled"] == true)
    }));
    let mode = context["controls"]
        .as_array()
        .expect("bound controls")
        .iter()
        .find(|control| control["id"] == "mode")
        .expect("bound mode control");
    let control = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        mpsc::unbounded_channel().0,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("bound-hidden-control")),
            method: "thread/control/set".to_string(),
            params: Some(json!({
                "scope": wire_scope,
                "threadId": thread_id,
                "targetId": context["selectedTargetId"],
                "controlId": "mode",
                "value": "plan",
                "expectedCapabilityRevision": mode["capabilityRevision"],
                "expectedBindingRevision": context["binding"]["bindingRevision"],
                "expectedContextRevision": context["contextRevision"],
                "expectedControlRevision": context["controlRevision"]
            })),
        },
    )
    .await
    .expect("bound hidden Profile control");
    assert_eq!(control["control"]["effectiveValue"], "plan");
    let forked = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        mpsc::unbounded_channel().0,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("bound-hidden-fork")),
            method: "thread/action/run".to_string(),
            params: Some(json!({
                "scope": wire_scope,
                "threadId": thread_id,
                "action": {"kind": "fork"}
            })),
        },
    )
    .await
    .expect("bound hidden Profile fork");
    assert_eq!(forked["kind"], "fork");
    assert_ne!(forked["snapshot"]["thread"]["id"], thread_id);

    let archived = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        mpsc::unbounded_channel().0,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("bound-hidden-archive")),
            method: "thread/archive".to_string(),
            params: Some(json!({"threadId": thread_id})),
        },
    )
    .await
    .expect("bound hidden Profile archive");
    assert_eq!(archived["session"]["id"], thread_id);
    let restored = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        mpsc::unbounded_channel().0,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("bound-hidden-restore")),
            method: "thread/restore".to_string(),
            params: Some(json!({"threadId": thread_id})),
        },
    )
    .await
    .expect("bound hidden Profile restore");
    assert_eq!(restored["session"]["id"], thread_id);
    let deleted = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        mpsc::unbounded_channel().0,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("bound-hidden-delete")),
            method: "thread/delete".to_string(),
            params: Some(json!({"threadId": thread_id})),
        },
    )
    .await
    .expect("bound hidden Profile delete");
    assert_eq!(deleted["deleted"], true);

    state
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown fixture");
}

#[tokio::test]
async fn thread_context_keeps_runtime_modes_out_of_agent_targets() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"[agents.backends.opencode-fixture]
kind = "acp"
label = "OpenCode fixture"
command = "/bin/true"
entrypoints = ["peer", "subagent"]

[runtime_profiles.opencode-fixture]
runtime = "acp"
enabled = true
label = "OpenCode fixture (ACP)"
backend_ref = "opencode-fixture"
default_mode = "build"
default_agent = "build"
"#,
    )
    .expect("config");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let (tx, _rx) = mpsc::unbounded_channel();

    let catalog = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("runtime-mode-target-catalog")),
            method: "thread/context/read".to_string(),
            params: Some(json!({ "scope": scope.to_wire_scope() })),
        },
    )
    .await
    .expect("thread/context/read catalog");
    let targets = catalog["compatibleTargets"].as_array().expect("targets");
    assert!(targets.iter().any(|target| {
        target["runtimeProfileRef"] == "opencode-fixture"
            && target["agentRef"] == "opencode-fixture"
    }));
    assert!(
        targets.iter().all(|target| {
            target["runtimeProfileRef"] != "opencode-fixture" || target["agentRef"] != "build"
        }),
        "OpenCode build is a Session mode, not a Psychevo Agent Definition"
    );
    let selected = runnable_target_for_source(
        &state,
        &scope,
        &scope.source,
        "opencode-fixture",
    )
    .expect("profile default target");
    assert_eq!(selected.agent_ref.as_deref(), Some("opencode-fixture"));

    let context = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("runtime-mode-target-context")),
            method: "thread/context/read".to_string(),
            params: Some(json!({
                "scope": scope.to_wire_scope(),
                "target": {
                    "agentRef": "opencode-fixture",
                    "runtimeProfileRef": "opencode-fixture"
                }
            })),
        },
    )
    .await
    .expect("thread/context/read target");
    let mode_controls = context["controls"]
        .as_array()
        .expect("controls")
        .iter()
        .filter(|control| control["surfaceRole"] == "mode")
        .collect::<Vec<_>>();
    assert_eq!(mode_controls.len(), 1);
    assert_eq!(mode_controls[0]["id"], "mode");
    assert_eq!(mode_controls[0]["effectiveValue"], "build");
}

fn write_runnable_target_catalog_fixture(state: &WebState) {
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"[agents.backends.cursor]
kind = "acp"
label = "Cursor"
command = "cursor-agent"
entrypoints = ["peer", "subagent"]
"#,
    )
    .expect("backend config");
    let agents = state.inner.cwd.join(".psychevo/agents");
    std::fs::create_dir_all(&agents).expect("project agents");
    for (name, definition) in [
        (
            "native-peer",
            "---\ndescription: Native peer\nentrypoints: [peer]\n---\nRun through Native.\n",
        ),
        (
            "native-subagent",
            "---\ndescription: Native child only\nentrypoints: [subagent]\n---\nChild only.\n",
        ),
        (
            "cursor-peer",
            "---\ndescription: Cursor peer\nbackend:\n  ref: cursor\nentrypoints: [peer]\n---\nRun through Cursor.\n",
        ),
        (
            "cursor-subagent",
            "---\ndescription: Cursor child only\nbackend:\n  ref: cursor\nentrypoints: [subagent]\n---\nChild only.\n",
        ),
    ] {
        std::fs::write(agents.join(format!("{name}.md")), definition).expect("Agent Definition");
    }
}

#[tokio::test]
async fn native_thread_context_reports_effective_permission_and_reasoning_values() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"model = "deepseek/deepseek-chat"

[provider.deepseek.models."deepseek-chat"]
reasoning_effort = "high"
"#,
    )
    .expect("model config");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let context = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        mpsc::unbounded_channel().0,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("native-effective-controls")),
            method: "thread/context/read".to_string(),
            params: Some(json!({
                "scope": scope.to_wire_scope(),
                "target": {"agentRef": null, "runtimeProfileRef": "native"}
            })),
        },
    )
    .await
    .expect("thread/context/read");

    let controls = context["controls"].as_array().expect("controls");
    let permission = controls
        .iter()
        .find(|control| control["id"] == "permissionMode")
        .expect("permission control");
    assert_eq!(permission["effectiveValue"], "default");
    assert_eq!(permission["effectiveSource"], "runtimeDefault");
    let reasoning = controls
        .iter()
        .find(|control| control["id"] == "reasoning")
        .expect("reasoning control");
    assert_eq!(reasoning["effectiveValue"], "high");

    let updated = handle_rpc(
        state,
        AuthContext::Bearer,
        mpsc::unbounded_channel().0,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("native-permission-change")),
            method: "thread/control/set".to_string(),
            params: Some(json!({
                "scope": scope.to_wire_scope(),
                "targetId": context["selectedTargetId"],
                "controlId": "permissionMode",
                "value": "dontAsk",
                "expectedCapabilityRevision": permission["capabilityRevision"],
                "expectedBindingRevision": 0,
                "expectedContextRevision": context["contextRevision"],
                "expectedControlRevision": context["controlRevision"]
            })),
        },
    )
    .await
    .expect("thread/control/set");
    assert_eq!(updated["control"]["effectiveValue"], "dontAsk");
}

#[tokio::test]
async fn thread_context_catalog_pairs_agent_definitions_with_runtime_profiles() {
    let (_temp, state) = web_state();
    write_runnable_target_catalog_fixture(&state);
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let (tx, _rx) = mpsc::unbounded_channel();

    let context = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("thread-context-targets")),
            method: "thread/context/read".to_string(),
            params: Some(json!({ "scope": scope.to_wire_scope() })),
        },
    )
    .await
    .expect("thread/context/read");
    let targets = context["compatibleTargets"].as_array().expect("targets");
    let has_pair = |agent_ref: Option<&str>, runtime_profile_ref: &str| {
        targets.iter().any(|target| {
            target["agentRef"].as_str() == agent_ref
                && target["agentRef"].is_null() == agent_ref.is_none()
                && target["runtimeProfileRef"] == runtime_profile_ref
        })
    };

    let default = targets
        .iter()
        .find(|target| target["agentRef"].is_null() && target["runtimeProfileRef"] == "native")
        .expect("explicit default Agent target");
    assert!(
        default["label"]
            .as_str()
            .unwrap_or_default()
            .contains("Psychevo")
    );
    assert!(has_pair(Some("native-peer"), "native"));
    assert!(has_pair(Some("cursor"), "acp:cursor"));
    assert!(has_pair(Some("cursor-peer"), "acp:cursor"));
    assert!(
        has_pair(Some("native-subagent"), "native"),
        "Native top-level Agents accept active local definitions whose default entrypoint is subagent"
    );
    assert!(!has_pair(Some("cursor-subagent"), "acp:cursor"));
    assert!(!has_pair(Some("native-peer"), "acp:cursor"));
    assert!(!has_pair(Some("cursor-peer"), "native"));

    let validated = validate_turn_runnable_target(
        &state,
        &scope,
        &wire::RunnableTargetInput {
            agent_ref: Some(" cursor-peer ".to_string()),
            runtime_profile_ref: " acp:cursor ".to_string(),
        },
    )
    .expect("catalog-backed target validation");
    assert_eq!(validated.agent_ref.as_deref(), Some("cursor-peer"));
    assert_eq!(validated.runtime_profile_ref, "acp:cursor");
}

#[tokio::test]
async fn unbound_control_set_uses_the_exact_prospective_target_once() {
    let (_temp, state) = web_state();
    write_runnable_target_catalog_fixture(&state);
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let wire_scope = scope.to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();
    let context = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("prospective-control-context")),
            method: "thread/context/read".to_string(),
            params: Some(json!({
                "scope": wire_scope.clone(),
                "target": {"agentRef": "native-peer", "runtimeProfileRef": "native"}
            })),
        },
    )
    .await
    .expect("prospective context");
    let mode = context["controls"]
        .as_array()
        .expect("controls")
        .iter()
        .find(|control| control["id"] == "mode")
        .expect("mode control");

    let receipt = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("prospective-control-set")),
            method: "thread/control/set".to_string(),
            params: Some(json!({
                "scope": wire_scope,
                "threadId": null,
                "targetId": context["selectedTargetId"],
                "controlId": "mode",
                "value": "plan",
                "expectedCapabilityRevision": mode["capabilityRevision"],
                "expectedBindingRevision": 0,
                "expectedContextRevision": context["contextRevision"],
                "expectedControlRevision": context["controlRevision"]
            })),
        },
    )
    .await
    .expect("the first control mutation uses the prospective target revisions");

    assert_eq!(receipt["status"], "applied");
    assert_eq!(receipt["context"]["selectedTargetId"], context["selectedTargetId"]);
    assert_eq!(receipt["context"]["selectionState"], "prospective");
    assert_eq!(receipt["control"]["effectiveValue"], "plan");
    assert_eq!(receipt["contextRevision"], context["contextRevision"]);
    assert_ne!(receipt["controlRevision"], context["controlRevision"]);
    let lane = state
        .inner
        .state
        .store()
        .gateway_source_lane(&scope.source.source_key().0)
        .expect("source lane read")
        .expect("source lane persisted");
    assert_eq!(lane.draft_agent_ref.as_deref(), Some("native-peer"));
    assert_eq!(lane.draft_profile_ref.as_deref(), Some("native"));
    assert_eq!(lane.draft_control_values.get("mode").map(String::as_str), Some("plan"));
}

#[tokio::test]
async fn unbound_control_receipt_can_start_turn_with_same_target() {
    let backend = Arc::new(AutomationFakeBackend::default());
    let (_temp, state) = web_state_with_automation_backend(backend.clone());
    write_runnable_target_catalog_fixture(&state);
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let wire_scope = scope.to_wire_scope();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let context = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("turn-control-context")),
            method: "thread/context/read".to_string(),
            params: Some(json!({
                "scope": wire_scope.clone(),
                "target": {"agentRef": "native-peer", "runtimeProfileRef": "native"}
            })),
        },
    )
    .await
    .expect("prospective context");
    let mode = context["controls"]
        .as_array()
        .expect("controls")
        .iter()
        .find(|control| control["id"] == "mode")
        .expect("mode control");
    let receipt = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("turn-control-set")),
            method: "thread/control/set".to_string(),
            params: Some(json!({
                "scope": wire_scope.clone(),
                "threadId": null,
                "targetId": context["selectedTargetId"],
                "controlId": "mode",
                "value": "plan",
                "expectedCapabilityRevision": mode["capabilityRevision"],
                "expectedBindingRevision": 0,
                "expectedContextRevision": context["contextRevision"],
                "expectedControlRevision": context["controlRevision"]
            })),
        },
    )
    .await
    .expect("source draft control receipt");

    let accepted = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("turn-after-control")),
            method: "turn/start".to_string(),
            params: Some(json!({
                "scope": wire_scope,
                "threadId": null,
                "target": {"agentRef": "native-peer", "runtimeProfileRef": "native"},
                "input": [{"type": "text", "text": "use the selected mode"}],
                "turnOverrides": {"model": "fake-model"},
                "expectedContextRevision": receipt["contextRevision"],
                "expectedControlRevision": receipt["controlRevision"]
            })),
        },
    )
    .await
    .expect("a fresh source-draft receipt starts the turn");
    assert_eq!(accepted["accepted"], true);
    let thread_id = accepted["threadId"].as_str().expect("thread id");

    let terminal = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(message) = rx.recv().await {
            if message.contains("\"type\":\"turnCompleted\"") {
                return message;
            }
        }
        String::new()
    })
    .await
    .expect("turn terminal");
    assert!(terminal.contains("\"status\":\"completed\""), "{terminal}");

    let runs = backend.runs.lock().expect("runs").clone();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].mode, RunMode::Plan);
    let binding = state
        .inner
        .state
        .store()
        .gateway_runtime_binding(thread_id)
        .expect("binding read")
        .expect("binding");
    assert_eq!(binding.thread_preferences.get("mode"), Some(&json!("plan")));
}

#[tokio::test]
async fn turn_start_rejects_agent_profile_pairs_missing_from_thread_context_catalog() {
    let (_temp, state) = web_state();
    write_runnable_target_catalog_fixture(&state);
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    for target in [
        json!({"agentRef": "native-peer", "runtimeProfileRef": "acp:cursor"}),
        json!({"agentRef": "cursor-subagent", "runtimeProfileRef": "acp:cursor"}),
        json!({"agentRef": null, "runtimeProfileRef": "acp:cursor"}),
    ] {
        let error = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("invalid-target")),
                method: "turn/start".to_string(),
                params: Some(json!({
                    "scope": scope.clone(),
                    "target": target,
                    "input": [{"type": "text", "text": "must not deliver"}]
                })),
            },
        )
        .await
        .expect_err("incompatible RunnableTarget must reject synchronously");
        assert!(error.to_string().contains("not compatible"), "{error}");
    }
    assert!(
        state
            .inner
            .state
            .store()
            .list_sessions_for_cwd_with_sources(&state.inner.cwd, &[])
            .expect("sessions")
            .is_empty(),
        "target rejection must happen before thread creation or delivery"
    );
}

#[tokio::test]
async fn turn_start_requires_fresh_context_and_control_revisions_before_thread_creation() {
    let (_temp, state) = web_state();
    write_runnable_target_catalog_fixture(&state);
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();
    let context = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("revision-context")),
            method: "thread/context/read".to_string(),
            params: Some(json!({
                "scope": scope,
                "target": {"agentRef": null, "runtimeProfileRef": "native"}
            })),
        },
    )
    .await
    .expect("prospective Thread Context");

    let missing = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("revision-missing")),
            method: "turn/start".to_string(),
            params: Some(json!({
                "scope": scope,
                "target": {"agentRef": null, "runtimeProfileRef": "native"},
                "input": [{"type": "text", "text": "must not deliver"}]
            })),
        },
    )
    .await
    .expect_err("missing revisions reject");
    assert_eq!(
        missing
            .structured_data()
            .and_then(|data| data["code"].as_str()),
        Some("revision_required")
    );

    let stale = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("revision-stale")),
            method: "turn/start".to_string(),
            params: Some(json!({
                "scope": scope,
                "target": {"agentRef": null, "runtimeProfileRef": "native"},
                "input": [{"type": "text", "text": "must not deliver"}],
                "expectedContextRevision": context["contextRevision"],
                "expectedControlRevision": "stale"
            })),
        },
    )
    .await
    .expect_err("stale revision rejects");
    assert_eq!(
        stale
            .structured_data()
            .and_then(|data| data["code"].as_str()),
        Some("stale_revision")
    );
    assert!(
        state
            .inner
            .state
            .store()
            .list_sessions_for_cwd_with_sources(&state.inner.cwd, &[])
            .expect("sessions")
            .is_empty(),
        "revision rejection must precede public Thread creation and delivery"
    );
}

#[tokio::test]
async fn thread_context_projects_immutable_agent_binding_and_turn_rejects_agent_change() {
    let (_temp, state) = web_state();
    write_runnable_target_catalog_fixture(&state);
    let profile = super::runtime_profiles::generated_runtime_profiles()
        .into_iter()
        .find(|profile| profile.id == "native")
        .expect("Native profile");
    let profile_json = serde_json::to_string(&profile).expect("profile snapshot");
    let profile_fingerprint = crate::runtime_profile_config_fingerprint(&profile);
    let profile_revision = crate::runtime_profile_config_revision(&profile_fingerprint).to_string();
    let agent_json = r#"{"name":"native-peer","instructions":"captured"}"#;
    let agent_fingerprint = crate::gateway_agent_definition_fingerprint(agent_json);
    let thread_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&state.inner.cwd, "web", "pending", "pending", None)
        .expect("thread");
    let cwd = state.inner.cwd.display().to_string();
    state
        .inner
        .state
        .store()
        .create_gateway_runtime_binding(psychevo_runtime::GatewayRuntimeBindingInput {
            thread_id: &thread_id,
            agent_ref: Some("native-peer"),
            agent_fingerprint: &agent_fingerprint,
            agent_definition_json: agent_json,
            runtime_ref: "native",
            backend_kind: "native",
            native_kind: "native",
            native_session_id: Some(&thread_id),
            cwd: &cwd,
            profile_fingerprint: &profile_fingerprint,
            profile_revision: &profile_revision,
            profile_config_json: &profile_json,
            adapter_kind: "native",
            adapter_revision: "test",
            ownership: GatewayRuntimeBindingOwnership::ReadWrite,
            parent_thread_id: None,
        })
        .expect("binding");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let context = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("bound-context")),
            method: "thread/context/read".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "threadId": thread_id.clone()
            })),
        },
    )
    .await
    .expect("bound Thread Context");
    assert_eq!(context["binding"]["agentRef"], "native-peer");
    assert_eq!(context["binding"]["agentFingerprint"], agent_fingerprint);
    assert_eq!(context["pendingInteractions"], json!([]));
    let actions = context["actions"].as_array().expect("Native actions");
    assert!(
        actions
            .iter()
            .any(|action| { action["id"] == "compact" && action["enabled"] == true })
    );
    assert!(
        actions
            .iter()
            .any(|action| { action["id"] == "interrupt" && action["enabled"] == false })
    );
    let mode = context["controls"]
        .as_array()
        .expect("Native controls")
        .iter()
        .find(|control| control["id"] == "mode")
        .expect("Native mode control");
    let receipt = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("native-control")),
            method: "thread/control/set".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": thread_id,
                "targetId": context["selectedTargetId"],
                "controlId": "mode",
                "value": "plan",
                "expectedCapabilityRevision": mode["capabilityRevision"],
                "expectedBindingRevision": context["binding"]["bindingRevision"],
                "expectedContextRevision": context["contextRevision"],
                "expectedControlRevision": context["controlRevision"]
            })),
        },
    )
    .await
    .expect("Native control uses the public receipt path");
    assert_eq!(receipt["status"], "applied");
    assert_eq!(receipt["control"]["effectiveValue"], "plan");
    assert_eq!(receipt["control"]["effectiveSource"], "threadPreference");
    let stored = state
        .inner
        .state
        .store()
        .gateway_runtime_binding(&thread_id)
        .expect("binding read")
        .expect("binding");
    assert_eq!(stored.thread_preferences["mode"], "plan");
    assert_eq!(
        stored.control_revision.to_string(),
        receipt["controlRevision"]
    );
    let mut resolved_turn_controls = BTreeMap::new();
    apply_thread_control_precedence(
        &state,
        &default_resolved_scope(&state, &AuthContext::Bearer).expect("scope"),
        Some(&thread_id),
        &mut resolved_turn_controls,
    )
    .expect("resolve Thread preferences");
    assert_eq!(resolved_turn_controls["mode"], "plan");
    resolved_turn_controls.insert("mode".to_string(), "default".to_string());
    assert_eq!(
        resolved_turn_controls["mode"], "default",
        "turnOverride is applied after the sticky Thread preference"
    );

    state
        .inner
        .state
        .store()
        .claim_gateway_activity(psychevo_runtime::GatewayActivityClaimInput {
            activity_id: "foreign-native-turn",
            thread_id: Some(&thread_id),
            source_key: Some(&state.inner.source.source_key().0),
            turn_id: Some("foreign-native-turn"),
            kind: "turn",
            owner_id: "gateway:foreign",
            owner_surface: Some("web"),
            lease_expires_at_ms: gateway_now_ms() + 30_000,
            queued_turns: 0,
            superseded_activity_id: None,
            intent: None,
        })
        .expect("foreign activity");
    let active_context = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("active-context")),
            method: "thread/context/read".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": thread_id
            })),
        },
    )
    .await
    .expect("active Thread Context");
    assert!(active_context["actions"].as_array().expect("actions").iter().any(
        |action| action["id"] == "interrupt" && action["enabled"] == true
    ));
    let interrupted = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("interrupt-action")),
            method: "thread/action/run".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": thread_id,
                "action": {"kind": "interrupt"}
            })),
        },
    )
    .await
    .expect("public interrupt action");
    assert_eq!(interrupted["kind"], "interrupt");
    assert_eq!(interrupted["interrupted"], true);

    let error = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("agent-change")),
            method: "turn/start".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": thread_id,
                "target": {"agentRef": null, "runtimeProfileRef": "native"},
                "input": [{"type": "text", "text": "must not deliver"}]
            })),
        },
    )
    .await
    .expect_err("immutable Agent target must reject");
    assert!(
        error
            .to_string()
            .contains("bound to Agent target `native-peer`")
    );
}

#[tokio::test]
async fn backend_list_auto_creates_detected_local_acp_backends() {
    let bin = tempfile::tempdir().expect("bin tempdir");
    write_command_shim(&bin.path().join("codex"));
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
    let codex = records
        .iter()
        .find(|backend| backend["id"] == "codex")
        .expect("codex backend");
    assert_eq!(codex["sourceTargets"], json!(["profile"]));
    assert!(codex["command"]
        .as_str()
        .is_some_and(|command| command.contains("runtime-adapters")));
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
    assert!(
        agent_records
            .iter()
            .any(|agent| agent["name"] == "opencode")
    );
    assert!(agent_records.iter().any(|agent| agent["name"] == "hermes"));
}

#[tokio::test]
async fn backend_list_hides_undetected_known_local_acp_backends_and_profiles() {
    let bin = tempfile::tempdir().expect("bin tempdir");
    let (_temp, state) = web_state_with_env(BTreeMap::from([(
        "PATH".to_string(),
        bin.path().display().to_string(),
    )]));
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let backends = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("undetected-backends")),
            method: "backend/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("backend/list");
    let ids = backends["backends"]
        .as_array()
        .expect("backends")
        .iter()
        .filter_map(|backend| backend["id"].as_str())
        .collect::<Vec<_>>();
    assert!(!ids.contains(&"codex"));
    assert!(!ids.contains(&"opencode"));
    assert!(!ids.contains(&"hermes"));

    let profiles = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("undetected-profiles")),
            method: "runtime/profile/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("runtime/profile/list");
    let profile_ids = profiles["profiles"]
        .as_array()
        .expect("profiles")
        .iter()
        .filter_map(|profile| profile["id"].as_str())
        .collect::<Vec<_>>();
    assert!(!profile_ids.contains(&"codex"));
    assert!(!profile_ids.contains(&"opencode"));

    let imports = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("undetected-imports")),
            method: "thread/import/list".to_string(),
            params: Some(json!({"scope": scope, "cursors": {}})),
        },
    )
    .await
    .expect("thread/import/list");
    let import_ids = imports["profiles"]
        .as_array()
        .expect("import profiles")
        .iter()
        .filter_map(|profile| profile["runtimeProfileRef"].as_str())
        .collect::<Vec<_>>();
    assert!(!import_ids.contains(&"codex"));
    assert!(!import_ids.contains(&"opencode"));
}

#[tokio::test]
async fn backend_list_retains_existing_known_backend_when_cli_is_absent() {
    let bin = tempfile::tempdir().expect("bin tempdir");
    let (_temp, state) = web_state_with_env(BTreeMap::from([(
        "PATH".to_string(),
        bin.path().display().to_string(),
    )]));
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"[agents.backends.codex]
kind = "acp"
label = "Review Codex"
command = "custom-codex-acp"
args = []
"#,
    )
    .expect("existing backend config");
    let (tx, _rx) = mpsc::unbounded_channel();

    let backends = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("retained-backend")),
            method: "backend/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("backend/list");
    let codex = backends["backends"]
        .as_array()
        .expect("backends")
        .iter()
        .find(|backend| backend["id"] == "codex")
        .expect("retained Codex backend");
    assert_eq!(codex["label"], "Review Codex");
    assert_eq!(codex["command"], "custom-codex-acp");

    let profiles = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("retained-profile")),
            method: "runtime/profile/list".to_string(),
            params: None,
        },
    )
    .await
    .expect("runtime/profile/list");
    assert!(profiles["profiles"]
        .as_array()
        .expect("profiles")
        .iter()
        .any(|profile| profile["id"] == "codex"));
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
    let profile_config = std::fs::read_to_string(state.inner.home.join("config.toml"))
        .unwrap_or_default();
    let profile_config = profile_config
        .parse::<toml::Value>()
        .unwrap_or_else(|_| toml::Value::Table(Default::default()));
    assert!(
        profile_config
            .get("agents")
            .and_then(|agents| agents.get("backends"))
            .and_then(|backends| backends.get("hermes"))
            .is_none(),
        "materialization must not shadow the effective project Hermes backend"
    );
}

fn write_command_shim(path: &Path) {
    std::fs::write(path, "#!/bin/sh\n").expect("command shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)
            .expect("command shim metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("command shim permissions");
    }
}

#[tokio::test]
async fn agent_rpc_manages_project_profile_disabled_and_raw_definitions() {
    let (_temp, state) = web_state();
    let project_agents = state.inner.cwd.join(".psychevo/agents");
    let compatible_agents = state.inner.cwd.join(".agents/agents");
    let profile_agents = state.inner.home.join("agents");
    std::fs::create_dir_all(&project_agents).expect("project agents");
    std::fs::create_dir_all(&compatible_agents).expect("compatible agents");
    std::fs::create_dir_all(&profile_agents).expect("profile agents");
    std::fs::write(
        project_agents.join("review.md"),
        "---\ndescription: Project review\nenabled: false\nmodel: keep-model\n---\nProject body.\n",
    )
    .expect("project agent");
    std::fs::write(
        compatible_agents.join("compat.md"),
        "---\ndescription: Compatible agent\n---\nCompatibility body.\n",
    )
    .expect("compatible agent");
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
    let compatible = list["agents"]
        .as_array()
        .expect("agents")
        .iter()
        .find(|agent| agent["name"] == "compat")
        .expect("compatible agent");
    assert_eq!(compatible["source"], "agents_project");
    assert_eq!(compatible["sourceLabel"], "Project");
    assert_eq!(compatible["target"], Value::Null);
    assert_eq!(compatible["mutable"], false);

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
    assert!(
        read_project["rawMarkdown"]
            .as_str()
            .expect("raw")
            .contains("model: keep-model")
    );

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
                "mcpServers": ["repo"],
                "optionalContributions": ["tools", "mcp"]
            })),
        },
    )
    .await
    .expect("agent/write project");
    assert_eq!(write_project["agent"]["enabled"], true);
    assert_eq!(write_project["agent"]["target"], "project");
    assert_eq!(
        write_project["agent"]["optionalContributions"],
        json!(["tools", "mcp"])
    );
    let text = std::fs::read_to_string(project_agents.join("review.md")).expect("project text");
    assert!(text.contains("model: keep-model"));
    assert!(text.contains("enabled: true"));
    assert!(text.contains("mcpServers"));
    assert!(text.contains("optionalContributions"));

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
    assert_eq!(
        active["contributions"],
        json!(["instructions", "tools", "mcp"])
    );
    assert!(
        list["shadowedAgents"]
            .as_array()
            .expect("shadowed")
            .iter()
            .any(|agent| agent["name"] == "review" && agent["target"] == "profile")
    );

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
async fn team_rpc_round_trips_native_and_acp_members() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"[agents.backends.codex]
kind = "acp"
command = "codex-acp"
"#,
    )
    .expect("Codex ACP backend");
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
            id: Some(json!("team-write")),
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
                    {
                        "id": "tester",
                        "agent": "codex",
                        "runtimeRef": "codex",
                        "runtimeOptions": {"model": "model-a", "mode": "review"},
                        "maxTurns": 2
                    }
                ],
                "instructions": "Coordinate the release."
            })),
        },
    )
    .await
    .expect("team/write");
    assert_eq!(written["team"]["name"], "ship");
    assert_eq!(written["team"]["maxParallelAgents"], 4);
    assert_eq!(written["team"]["members"][1]["agent"], "codex");
    assert_eq!(written["team"]["members"][1]["runtimeRef"], "codex");
    assert!(
        written["team"]["members"][1]["runtimeProfileRevision"]
            .as_str()
            .and_then(|revision| revision.parse::<u64>().ok())
            .is_some_and(|revision| revision > 0)
    );

    let read = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("team-read")),
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
    assert_eq!(read["team"]["members"][1]["runtimeRef"], "codex");
    assert_eq!(
        read["team"]["members"][1]["runtimeProfileRevision"],
        written["team"]["members"][1]["runtimeProfileRevision"]
    );
    assert!(
        read["rawMarkdown"]
            .as_str()
            .expect("raw Team")
            .contains("runtimeProfileRevision")
    );

    let deleted = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("team-delete")),
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

#[tokio::test]
async fn team_write_fails_closed_for_unknown_profiles_pairings_and_overrides() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"[agents.backends.codex]
kind = "acp"
command = "codex-acp"
"#,
    )
    .expect("Codex ACP backend");
    let (tx, _rx) = mpsc::unbounded_channel();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();

    let unknown = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("unknown")),
            method: "team/write".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "unknown-runtime",
                "target": "project",
                "description": "Must not guess",
                "leader": "general",
                "members": [{"id": "reviewer", "agent": "general", "runtimeRef": "missing"}]
            })),
        },
    )
    .await
    .expect_err("unknown Runtime Profile must fail");
    assert!(unknown.to_string().contains("unknown Runtime Profile"));

    let pairing = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("pairing")),
            method: "team/write".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "bad-pairing",
                "target": "project",
                "description": "Backend pairing is exact",
                "leader": "general",
                "members": [{"id": "reviewer", "agent": "general", "runtimeRef": "codex"}]
            })),
        },
    )
    .await
    .expect_err("Native Agent paired with ACP Profile must fail");
    assert!(pairing.to_string().contains("uses ACP backend"));

    for (name, runtime_ref, options, expected) in [
        (
            "unsupported-acp-option",
            "codex",
            json!({"effort": "high"}),
            "ACP runtime option",
        ),
        (
            "safety-override",
            "codex",
            json!({"sandbox": "danger-full-access"}),
            "safety override",
        ),
        (
            "native-option",
            "native",
            json!({"model": "model-a"}),
            "Native Runtime Profile options are not supported",
        ),
    ] {
        let agent = if runtime_ref == "native" {
            "general"
        } else {
            "codex"
        };
        let error = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(name)),
                method: "team/write".to_string(),
                params: Some(json!({
                    "scope": scope.clone(),
                    "name": name,
                    "target": "project",
                    "description": "Validate exact options",
                    "leader": "general",
                    "members": [{
                        "id": "reviewer",
                        "agent": agent,
                        "runtimeRef": runtime_ref,
                        "runtimeOptions": options
                    }]
                })),
            },
        )
        .await
        .expect_err("invalid Team option must fail");
        assert!(
            error.to_string().contains(expected),
            "unexpected error for {name}: {error:?}"
        );
    }
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
    assert_eq!(
        command.path.as_deref(),
        Some(shim.to_string_lossy().as_ref())
    );
}

#[tokio::test]
async fn backend_doctor_reports_generic_auth_unchecked_without_session_or_prompt_probe() {
    let host_env = std::env::vars().collect::<BTreeMap<_, _>>();
    let host_cwd = std::env::current_dir().expect("host cwd");
    let python = ["python3", "python"]
        .into_iter()
        .find_map(|command| {
            resolve_executable_path(
                command,
                &host_cwd,
                &ExecutableResolveOptions {
                    platform: HostPlatform::current(),
                    env: &host_env,
                },
            )
        })
        .expect("Python is required by the ACP fixture");
    let (_temp, state) = web_state();
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_acp_lifecycle.py");
    let log = state.inner.cwd.join("backend-doctor-auth.jsonl");
    let (tx, _rx) = mpsc::unbounded_channel();

    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("write-auth-doctor-fixture")),
            method: "backend/write".to_string(),
            params: Some(json!({
                "id": "auth-doctor-fixture",
                "target": "project",
                "command": python,
                "args": [fixture],
                "env": {
                    "ACP_LIFECYCLE_LOG": log,
                    "ACP_LIFECYCLE_MODE": "all"
                },
                "entrypoints": ["peer", "subagent"]
            })),
        },
    )
    .await
    .expect("backend/write");

    let doctor = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("doctor-auth-fixture")),
            method: "backend/doctor".to_string(),
            params: Some(json!({"id": "auth-doctor-fixture"})),
        },
    )
    .await
    .expect("backend/doctor");
    let protocol = doctor["checks"]
        .as_array()
        .expect("doctor checks")
        .iter()
        .find(|check| check["name"] == "protocol")
        .expect("protocol check");
    assert_eq!(protocol["ok"], true);
    assert!(
        protocol["message"]
            .as_str()
            .unwrap_or_default()
            .contains("stable ACP protocol v1")
    );
    let authentication = doctor["checks"]
        .as_array()
        .expect("doctor checks")
        .iter()
        .find(|check| check["name"] == "authentication")
        .expect("authentication check");
    assert_eq!(authentication["ok"], true);
    assert!(
        authentication["message"]
            .as_str()
            .unwrap_or_default()
            .contains("unchecked")
    );

    state
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown fixture");
    let requests = std::fs::read_to_string(log)
        .expect("fixture log")
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|entry| entry["method"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    assert_eq!(requests, vec!["initialize"]);
}

#[tokio::test]
async fn backend_doctor_reports_protocol_incompatibility_without_auth_or_session_probe() {
    let host_env = std::env::vars().collect::<BTreeMap<_, _>>();
    let host_cwd = std::env::current_dir().expect("host cwd");
    let python = ["python3", "python"]
        .into_iter()
        .find_map(|command| {
            resolve_executable_path(
                command,
                &host_cwd,
                &ExecutableResolveOptions {
                    platform: HostPlatform::current(),
                    env: &host_env,
                },
            )
        })
        .expect("Python is required by the ACP fixture");
    let (_temp, state) = web_state();
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_acp_lifecycle.py");
    let log = state.inner.cwd.join("backend-doctor-protocol.jsonl");
    let (tx, _rx) = mpsc::unbounded_channel();

    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("write-protocol-doctor-fixture")),
            method: "backend/write".to_string(),
            params: Some(json!({
                "id": "protocol-doctor-fixture",
                "target": "project",
                "command": python,
                "args": [fixture],
                "env": {
                    "ACP_LIFECYCLE_LOG": log,
                    "ACP_LIFECYCLE_MODE": "protocol-v2"
                },
                "entrypoints": ["peer", "subagent"]
            })),
        },
    )
    .await
    .expect("backend/write");

    let doctor = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("doctor-protocol-fixture")),
            method: "backend/doctor".to_string(),
            params: Some(json!({"id": "protocol-doctor-fixture"})),
        },
    )
    .await
    .expect("backend/doctor");
    let checks = doctor["checks"].as_array().expect("doctor checks");
    let protocol = checks
        .iter()
        .find(|check| check["name"] == "protocol")
        .expect("protocol check");
    assert_eq!(protocol["ok"], false);
    let protocol_message = protocol["message"].as_str().unwrap_or_default();
    assert!(protocol_message.contains("expected stable ACP v1"));
    assert!(protocol_message.contains("v2"));
    let authentication = checks
        .iter()
        .find(|check| check["name"] == "authentication")
        .expect("authentication check");
    assert_eq!(authentication["ok"], true);
    let authentication_message = authentication["message"].as_str().unwrap_or_default();
    assert!(authentication_message.contains("unchecked"));
    assert!(authentication_message.contains("protocol"));
    assert_eq!(doctor["ok"], false);

    state
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown fixture");
    let requests = std::fs::read_to_string(log)
        .expect("fixture log")
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|entry| entry["method"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    assert_eq!(requests, vec!["initialize"]);
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
async fn thread_draft_prepare_projects_opencode_controls_before_first_prompt() {
    let host_env = std::env::vars().collect::<BTreeMap<_, _>>();
    let host_cwd = std::env::current_dir().expect("host cwd");
    let python = ["python3", "python"]
        .into_iter()
        .find_map(|command| {
            resolve_executable_path(
                command,
                &host_cwd,
                &ExecutableResolveOptions {
                    platform: HostPlatform::current(),
                    env: &host_env,
                },
            )
        })
        .expect("Python is required by the ACP fixture");
    let (_temp, state) = web_state();
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_acp_lifecycle.py");
    let log = state.inner.cwd.join("draft-prepare.jsonl");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("write-opencode-draft-fixture")),
            method: "backend/write".to_string(),
            params: Some(json!({
                "id": "opencode",
                "target": "project",
                "command": python,
                "args": [fixture],
                "env": {
                    "ACP_LIFECYCLE_LOG": log,
                    "ACP_LIFECYCLE_MODE": "all"
                },
                "entrypoints": ["peer", "subagent"]
            })),
        },
    )
    .await
    .expect("backend/write");

    let before = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("context-before-prepare")),
            method: "thread/context/read".to_string(),
            params: Some(json!({"threadId": null, "scope": scope})),
        },
    )
    .await
    .expect("thread/context/read");
    let target_id = before["compatibleTargets"]
        .as_array()
        .expect("targets")
        .iter()
        .find(|target| {
            target["agentRef"] == "opencode" && target["runtimeProfileRef"] == "opencode"
        })
        .and_then(|target| target["targetId"].as_str())
        .expect("OpenCode target")
        .to_string();

    let prepared = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("prepare-opencode")),
            method: "thread/draft/prepare".to_string(),
            params: Some(json!({"targetId": target_id, "scope": scope})),
        },
    )
    .await
    .expect("thread/draft/prepare");
    let controls = prepared["context"]["controls"].as_array().expect("controls");
    let model = controls
        .iter()
        .find(|control| control["surfaceRole"] == "model")
        .expect("model control");
    assert_eq!(model["effectiveValue"], "test/default");
    assert_eq!(model["choices"].as_array().map(Vec::len), Some(2));
    let mode = controls
        .iter()
        .find(|control| control["surfaceRole"] == "mode")
        .expect("mode control");
    assert_eq!(mode["effectiveValue"], "build");
    assert_eq!(
        mode["choices"]
            .as_array()
            .expect("mode choices")
            .iter()
            .map(|choice| choice["value"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec!["build", "plan"]
    );

    let refreshed = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("context-after-delayed-command")),
            method: "thread/context/read".to_string(),
            params: Some(json!({
                "threadId": null,
                "target": {"agentRef": "opencode", "runtimeProfileRef": "opencode"},
                "scope": scope
            })),
        },
    )
    .await
    .expect("context after delayed available_commands_update");
    assert!(refreshed["capabilities"]
        .as_array()
        .expect("refreshed capabilities")
        .iter()
        .any(|capability| capability["id"] == "command:fixture_status"));
    assert_eq!(
        refreshed["contextRevision"], prepared["context"]["contextRevision"],
        "display-only command updates must not invalidate admission freshness"
    );
    assert_eq!(
        refreshed["controlRevision"], prepared["context"]["controlRevision"],
        "display-only command updates must not invalidate control freshness"
    );

    let changed = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("set-draft-model")),
            method: "thread/control/set".to_string(),
            params: Some(json!({
                "threadId": null,
                "targetId": target_id,
                "controlId": "model",
                "value": "test/second",
                "expectedCapabilityRevision": model["capabilityRevision"],
                "expectedBindingRevision": 0,
                "expectedContextRevision": prepared["context"]["contextRevision"],
                "expectedControlRevision": prepared["context"]["controlRevision"],
                "scope": scope
            })),
        },
    )
    .await
    .expect("thread/control/set prepared model");
    assert_eq!(changed["status"], "observed");
    assert_eq!(changed["control"]["effectiveValue"], "test/second");

    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("prepare-opencode-again")),
            method: "thread/draft/prepare".to_string(),
            params: Some(json!({"targetId": target_id, "scope": scope})),
        },
    )
    .await
    .expect("idempotent thread/draft/prepare");
    let session_new_count = std::fs::read_to_string(&log)
        .expect("fixture log")
        .lines()
        .filter(|line| line.contains("\"method\":\"session/new\""))
        .count();
    assert_eq!(session_new_count, 1);

    let accepted = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("turn-with-prepared-session")),
            method: "turn/start".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": null,
                "target": {"agentRef": "opencode", "runtimeProfileRef": "opencode"},
                "input": [{"type": "text", "text": "use the prepared session"}],
                "turnOverrides": {},
                "expectedContextRevision": changed["contextRevision"],
                "expectedControlRevision": changed["controlRevision"]
            })),
        },
    )
    .await
    .expect("turn/start with prepared session");
    assert_eq!(accepted["accepted"], true);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let content = std::fs::read_to_string(&log).unwrap_or_default();
            if content.contains("\"method\":\"session/prompt\"") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("prepared prompt");
    let content = std::fs::read_to_string(log).expect("fixture log after turn");
    assert_eq!(
        content
            .lines()
            .filter(|line| line.contains("\"method\":\"session/new\""))
            .count(),
        1,
        "the first turn must promote the prepared session instead of creating another"
    );
    assert!(content.lines().any(|line| {
        line.contains("\"method\":\"session/prompt\"")
            && line.contains("\"sessionId\":\"draft-native\"")
    }));
}

#[tokio::test]
async fn thread_draft_prepare_failure_remains_blocking_on_the_source_lane() {
    let host_env = std::env::vars().collect::<BTreeMap<_, _>>();
    let host_cwd = std::env::current_dir().expect("host cwd");
    let python = ["python3", "python"]
        .into_iter()
        .find_map(|command| {
            resolve_executable_path(
                command,
                &host_cwd,
                &ExecutableResolveOptions {
                    platform: HostPlatform::current(),
                    env: &host_env,
                },
            )
        })
        .expect("Python is required by the ACP fixture");
    let (_temp, state) = web_state();
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_acp_lifecycle.py");
    let log = state.inner.cwd.join("draft-prepare-failure.jsonl");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("write-failing-draft-fixture")),
            method: "backend/write".to_string(),
            params: Some(json!({
                "id": "opencode",
                "target": "project",
                "command": python,
                "args": [fixture],
                "env": {
                    "ACP_LIFECYCLE_LOG": log,
                    "ACP_LIFECYCLE_MODE": "session-new-error"
                },
                "entrypoints": ["peer", "subagent"]
            })),
        },
    )
    .await
    .expect("backend/write");

    let before = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("context-before-failing-prepare")),
            method: "thread/context/read".to_string(),
            params: Some(json!({"threadId": null, "scope": scope})),
        },
    )
    .await
    .expect("context before failing prepare");
    let targets = before["compatibleTargets"].as_array().expect("targets");
    let failing_target_id = targets
        .iter()
        .find(|target| {
            target["agentRef"] == "opencode"
                && target["runtimeProfileRef"] == "opencode"
        })
        .and_then(|target| target["targetId"].as_str())
        .expect("failing ACP target")
        .to_string();

    let prepared = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("prepare-failing-acp")),
            method: "thread/draft/prepare".to_string(),
            params: Some(json!({"targetId": failing_target_id, "scope": scope})),
        },
    )
    .await
    .expect("bounded prepare failure");
    assert_eq!(prepared["context"]["sendability"]["allowed"], false);
    assert!(prepared["problem"]["message"]
        .as_str()
        .is_some_and(|message| message.contains("fixture session preparation failed")));

    let refreshed = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("context-after-failing-prepare")),
            method: "thread/context/read".to_string(),
            params: Some(json!({"threadId": null, "scope": scope})),
        },
    )
    .await
    .expect("context after failing prepare");
    assert_eq!(refreshed["selectedTargetId"], failing_target_id);
    assert_eq!(refreshed["sendability"]["allowed"], false);
    assert_eq!(
        refreshed["sendability"]["reason"],
        prepared["context"]["sendability"]["reason"]
    );

    state
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown fixture");
}

#[tokio::test]
async fn thread_draft_prepare_projects_and_applies_legacy_acp_models() {
    let host_env = std::env::vars().collect::<BTreeMap<_, _>>();
    let host_cwd = std::env::current_dir().expect("host cwd");
    let python = ["python3", "python"]
        .into_iter()
        .find_map(|command| {
            resolve_executable_path(
                command,
                &host_cwd,
                &ExecutableResolveOptions {
                    platform: HostPlatform::current(),
                    env: &host_env,
                },
            )
        })
        .expect("Python is required by the ACP fixture");
    let (_temp, state) = web_state();
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_acp_lifecycle.py");
    let log = state.inner.cwd.join("legacy-model-draft.jsonl");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("write-legacy-model-fixture")),
            method: "backend/write".to_string(),
            params: Some(json!({
                "id": "legacy-hermes",
                "target": "project",
                "command": python,
                "args": [fixture],
                "env": {
                    "ACP_LIFECYCLE_LOG": log,
                    "ACP_LIFECYCLE_MODE": "legacy-models"
                },
                "entrypoints": ["peer", "subagent"]
            })),
        },
    )
    .await
    .expect("backend/write");

    let before = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("context-before-legacy-prepare")),
            method: "thread/context/read".to_string(),
            params: Some(json!({"threadId": null, "scope": scope})),
        },
    )
    .await
    .expect("thread/context/read");
    let target = before["compatibleTargets"]
        .as_array()
        .expect("targets")
        .iter()
        .find(|target| target["agentRef"] == "legacy-hermes")
        .expect("legacy Hermes target")
        .clone();
    let target_id = target["targetId"].as_str().expect("target id");

    let prepared = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("prepare-legacy-hermes")),
            method: "thread/draft/prepare".to_string(),
            params: Some(json!({"targetId": target_id, "scope": scope})),
        },
    )
    .await
    .expect("thread/draft/prepare");
    let model = prepared["context"]["controls"]
        .as_array()
        .expect("controls")
        .iter()
        .find(|control| control["surfaceRole"] == "model")
        .expect("legacy model control");
    assert_eq!(model["effectiveValue"], "test/default");
    assert_eq!(model["choices"].as_array().map(Vec::len), Some(2));

    let changed = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("set-legacy-model")),
            method: "thread/control/set".to_string(),
            params: Some(json!({
                "threadId": null,
                "targetId": target_id,
                "controlId": "model",
                "value": "test/second",
                "expectedCapabilityRevision": model["capabilityRevision"],
                "expectedBindingRevision": 0,
                "expectedContextRevision": prepared["context"]["contextRevision"],
                "expectedControlRevision": prepared["context"]["controlRevision"],
                "scope": scope
            })),
        },
    )
    .await
    .expect("thread/control/set legacy model");
    assert_eq!(changed["status"], "observed");
    assert_eq!(changed["control"]["effectiveValue"], "test/second");

    let accepted = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("turn-with-legacy-model")),
            method: "turn/start".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": null,
                "target": {
                    "agentRef": target["agentRef"],
                    "runtimeProfileRef": target["runtimeProfileRef"]
                },
                "input": [{"type": "text", "text": "use the legacy model"}],
                "turnOverrides": {},
                "expectedContextRevision": changed["contextRevision"],
                "expectedControlRevision": changed["controlRevision"]
            })),
        },
    )
    .await
    .expect("turn/start with legacy model");
    assert_eq!(accepted["accepted"], true);

    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let content = std::fs::read_to_string(&log).unwrap_or_default();
            if content.contains("\"method\":\"session/prompt\"") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("legacy prompt");
    let entries = std::fs::read_to_string(&log)
        .expect("legacy fixture log")
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect::<Vec<_>>();
    let methods = entries
        .iter()
        .filter(|entry| entry["event"] == "request")
        .filter_map(|entry| entry["method"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        methods.iter().filter(|method| **method == "session/new").count(),
        1,
        "the first turn must reuse the prepared legacy session"
    );
    let prompt_index = methods
        .iter()
        .position(|method| *method == "session/prompt")
        .expect("session/prompt request");
    let model_indices = methods
        .iter()
        .enumerate()
        .filter_map(|(index, method)| (*method == "session/set_model").then_some(index))
        .collect::<Vec<_>>();
    assert!(!model_indices.is_empty());
    assert!(model_indices.iter().all(|index| *index < prompt_index));

    state
        .inner
        .gateway
        .shutdown_runtimes(false)
        .await
        .expect("shutdown legacy model fixture");
}

#[test]
fn acp_session_modes_project_as_typed_mode_control_and_revision_facts() {
    let modes = vec![
        crate::acp_peer::AcpSessionModeSnapshot {
            id: "ask".to_string(),
            name: "Ask".to_string(),
            description: Some("Answer questions".to_string()),
        },
        crate::acp_peer::AcpSessionModeSnapshot {
            id: "plan".to_string(),
            name: "Plan".to_string(),
            description: Some("Plan changes".to_string()),
        },
    ];

    let control = acp_session_mode_control_descriptor(
        &modes,
        Some("plan"),
        "capability-revision".to_string(),
    )
    .expect("mode descriptor");

    assert_eq!(control.id, "mode");
    assert_eq!(control.surface_role, wire::ThreadControlSurfaceRoleView::Mode);
    assert_eq!(control.effective_value, Some(json!("plan")));
    assert_eq!(
        control.effective_source,
        wire::ThreadControlEffectiveSourceView::RuntimeObserved
    );
    assert_eq!(
        control
            .choices
            .iter()
            .map(|choice| choice.value.clone())
            .collect::<Vec<_>>(),
        vec![json!("ask"), json!("plan")]
    );
    assert!(control.channel_safe);

    let before = combined_thread_revision(&["binding-1", "projection-1"]);
    let projection_changed = combined_thread_revision(&["binding-1", "projection-2"]);
    let binding_changed = combined_thread_revision(&["binding-2", "projection-1"]);
    assert_ne!(before, projection_changed);
    assert_ne!(before, binding_changed);
}
