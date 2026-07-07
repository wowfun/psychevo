#[tokio::test]
async fn plugin_read_rpcs_return_manifest_metadata_without_mutation() {
    let (temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(state.inner.home.join("config.toml"), "# config\n").expect("config");
    let source = temp.path().join("display-plugin");
    let manifest_dir = source.join(".psychevo-plugin");
    std::fs::create_dir_all(&manifest_dir).expect("manifest dir");
    std::fs::create_dir_all(source.join("assets")).expect("assets");
    std::fs::write(source.join("assets/icon.png"), "icon").expect("icon");
    std::fs::write(
        manifest_dir.join("plugin.json"),
        r#"{
          "name": "display-plugin",
          "version": "1.0.0",
          "description": "display plugin",
          "interface": {
            "displayName": "Display Plugin",
            "shortDescription": "Adds display metadata.",
            "category": "productivity",
            "capabilities": ["tools"],
            "composerIcon": "./assets/icon.png"
          }
        }"#,
    )
    .expect("manifest");
    psychevo_runtime::install_plugin(
        &state.inner.home,
        &state.inner.cwd,
        psychevo_runtime::PluginInstallOptions {
            source: source.display().to_string(),
            source_kind: None,
            scope: psychevo_runtime::PluginScope::Global,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            adapter_mode: None,
            force: false,
        },
    )
    .expect("install");
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
            id: Some(json!("plugin-list")),
            method: "plugin/list".to_string(),
            params: Some(json!({ "scope": scope.clone() })),
        },
    )
    .await
    .expect("plugin/list");
    assert_eq!(list["count"], 1);
    assert_eq!(list["plugins"][0]["name"], "display-plugin");

    let read = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("plugin-read")),
            method: "plugin/read".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "selector": "display-plugin"
            })),
        },
    )
    .await
    .expect("plugin/read");
    assert_eq!(
        read["manifest"]["interface"]["displayName"],
        "Display Plugin"
    );
    assert_eq!(
        read["manifest"]["interface"]["shortDescription"],
        "Adds display metadata."
    );
    assert_eq!(read["manifest"]["interface"]["capabilities"][0], "tools");
    assert!(
        read["manifest"]["interface"]["composerIcon"]
            .as_str()
            .is_some_and(|path| path.contains("assets/icon.png"))
    );

    let doctor = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("plugin-doctor")),
            method: "plugin/doctor".to_string(),
            params: Some(json!({
                "scope": scope,
                "selector": "display-plugin"
            })),
        },
    )
    .await
    .expect("plugin/doctor");
    assert_eq!(
        doctor["plugins"][0]["manifest"]["interface"]["displayName"],
        "Display Plugin"
    );
    assert_eq!(
        std::fs::read_to_string(state.inner.home.join("config.toml")).expect("config"),
        "# config\n"
    );
    assert!(!state.inner.cwd.join(".psychevo/config.toml").exists());
}

#[tokio::test]
async fn capability_skill_rpcs_install_toggle_and_uninstall_project_skill() {
    let (temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    let source = temp.path().join("source-skills");
    write_capability_package_skill(&source, "review-flow", "Review workflow", "fresh body");
    write_capability_package_skill(
        &state.inner.cwd.join(".psychevo").join("skills"),
        "review-flow",
        "Stale workflow",
        "stale body",
    );
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();

    let rejected = capability_rpc(
        &state,
        "skill/install",
        json!({
            "scope": scope.clone(),
            "source": source,
            "name": "review-flow",
            "target": "project"
        }),
    )
    .await
    .expect_err("force is required to overwrite");
    assert!(rejected.to_string().contains("skill already exists"));

    let installed = capability_rpc(
        &state,
        "skill/install",
        json!({
            "scope": scope.clone(),
            "source": source,
            "name": "review-flow",
            "target": "project",
            "force": true
        }),
    )
    .await
    .expect("skill/install");
    assert_eq!(installed["installed"][0]["name"], "review-flow");

    let read = capability_rpc(
        &state,
        "skill/read",
        json!({
            "scope": scope.clone(),
            "name": "review-flow"
        }),
    )
    .await
    .expect("skill/read");
    assert_eq!(read["description"], "Review workflow");
    assert_eq!(read["content"], "fresh body");
    let preview_content = read["preview_content"].as_str().expect("preview content");
    assert!(preview_content.contains("---"));
    assert!(preview_content.contains("description: \"Review workflow\""));
    assert!(preview_content.contains("fresh body"));

    let disabled = capability_rpc(
        &state,
        "skill/setEnabled",
        json!({
            "scope": scope.clone(),
            "name": "review-flow",
            "target": "project",
            "enabled": false
        }),
    )
    .await
    .expect("skill/setEnabled");
    assert_eq!(disabled["enabled"], false);
    let project_config =
        std::fs::read_to_string(state.inner.cwd.join(".psychevo/config.toml")).expect("config");
    assert!(project_config.contains("review-flow"));
    let listed = capability_rpc(
        &state,
        "skill/list",
        json!({
            "scope": scope.clone()
        }),
    )
    .await
    .expect("skill/list");
    let row = listed["skills"]
        .as_array()
        .expect("skills")
        .iter()
        .find(|skill| skill["name"] == "review-flow")
        .expect("review-flow row");
    assert_eq!(row["enabled"], false);
    assert_eq!(row["prompt_visible"], false);
    assert!(row["id"].as_str().expect("id").ends_with("SKILL.md"));

    let project_skill_path = state
        .inner
        .cwd
        .join(".psychevo/skills/review-flow/SKILL.md");
    let removed = capability_rpc(
        &state,
        "skill/uninstall",
        json!({
            "scope": scope,
            "name": "review-flow",
            "path": project_skill_path.clone(),
            "target": "project"
        }),
    )
    .await
    .expect("skill/uninstall");
    assert_eq!(removed["success"], true);
    assert!(!state
        .inner
        .cwd
        .join(".psychevo/skills/review-flow/SKILL.md")
        .exists());
}

#[tokio::test]
async fn capability_skill_read_and_uninstall_accept_path_selector_for_collisions() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    write_capability_package_skill(
        &state.inner.cwd.join(".psychevo").join("skills"),
        "same-skill",
        "Project skill",
        "project body",
    );
    write_capability_package_skill(
        &state.inner.home.join("skills"),
        "same-skill",
        "Profile skill",
        "profile body",
    );
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let project_path = state
        .inner
        .cwd
        .join(".psychevo/skills/same-skill/SKILL.md");
    let profile_path = state.inner.home.join("skills/same-skill/SKILL.md");

    let list = capability_rpc(&state, "skill/list", json!({ "scope": scope.clone() }))
        .await
        .expect("skill/list");
    let rows = list["skills"]
        .as_array()
        .expect("skills")
        .iter()
        .filter(|skill| skill["name"] == "same-skill")
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|skill| skill["prompt_visible"] == false));
    assert!(rows.iter().all(|skill| {
        skill["collision_group"]
            .as_array()
            .is_some_and(|group| group.len() == 2)
    }));

    let ambiguous = capability_rpc(
        &state,
        "skill/read",
        json!({
            "scope": scope.clone(),
            "name": "same-skill"
        }),
    )
    .await
    .expect_err("name-only collision should fail");
    assert!(ambiguous.to_string().contains("ambiguous skill name"));

    let selected = capability_rpc(
        &state,
        "skill/read",
        json!({
            "scope": scope.clone(),
            "name": "same-skill",
            "path": project_path.clone()
        }),
    )
    .await
    .expect("path-selected read");
    assert_eq!(selected["description"], "Project skill");

    let refused = capability_rpc(
        &state,
        "skill/uninstall",
        json!({
            "scope": scope.clone(),
            "name": "same-skill",
            "path": profile_path.clone(),
            "target": "project"
        }),
    )
    .await
    .expect_err("profile path is not project mutable");
    assert!(refused.to_string().contains("not removable from project scope"));

    let removed = capability_rpc(
        &state,
        "skill/uninstall",
        json!({
            "scope": scope,
            "name": "same-skill",
            "path": state.inner.cwd.join(".psychevo/skills/same-skill/SKILL.md"),
            "target": "project"
        }),
    )
    .await
    .expect("path-selected uninstall");
    assert_eq!(removed["success"], true);
    assert!(!project_path.exists());
    assert!(profile_path.exists());
}

#[tokio::test]
async fn capability_skill_write_updates_project_and_profile_skill_markdown() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    write_capability_package_skill(
        &state.inner.cwd.join(".psychevo").join("skills"),
        "same-skill",
        "Project skill",
        "project body",
    );
    write_capability_package_skill(
        &state.inner.home.join("skills"),
        "same-skill",
        "Profile skill",
        "profile body",
    );
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let project_path = state
        .inner
        .cwd
        .join(".psychevo/skills/same-skill/SKILL.md");
    let profile_path = state.inner.home.join("skills/same-skill/SKILL.md");

    let project = capability_rpc(
        &state,
        "skill/write",
        json!({
            "scope": scope.clone(),
            "name": "same-skill",
            "path": project_path.clone(),
            "target": "project",
            "rawMarkdown": "---\nname: same-skill\ndescription: Project updated\n---\n\nproject updated body\n"
        }),
    )
    .await
    .expect("project skill/write");
    assert_eq!(project["written"], true);
    assert_eq!(project["target"], "project");
    assert!(std::fs::read_to_string(&project_path)
        .expect("project skill")
        .contains("project updated body"));

    let profile = capability_rpc(
        &state,
        "skill/write",
        json!({
            "scope": scope.clone(),
            "name": "same-skill",
            "path": profile_path.clone(),
            "target": "profile",
            "rawMarkdown": "---\nname: same-skill\ndescription: Profile updated\n---\n\nprofile updated body\n"
        }),
    )
    .await
    .expect("profile skill/write");
    assert_eq!(profile["written"], true);
    assert_eq!(profile["target"], "global");
    assert!(std::fs::read_to_string(&profile_path)
        .expect("profile skill")
        .contains("profile updated body"));

    let invalid = capability_rpc(
        &state,
        "skill/write",
        json!({
            "scope": scope,
            "name": "same-skill",
            "path": project_path.clone(),
            "target": "project",
            "rawMarkdown": "---\nname: wrong-name\n---\n\nmissing description\n"
        }),
    )
    .await
    .expect_err("invalid markdown is rejected");
    assert!(invalid.to_string().contains("name \"wrong-name\""));
    assert!(std::fs::read_to_string(&project_path)
        .expect("project skill unchanged")
        .contains("project updated body"));
}

#[tokio::test]
async fn capability_skill_write_rejects_configured_external_skill() {
    let (temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::create_dir_all(state.inner.home.join("skills")).expect("profile skills root");
    let external = temp.path().join("external-skills");
    write_capability_package_skill(&external, "external-skill", "External skill", "external body");
    std::fs::write(
        state.inner.home.join("config.toml"),
        format!("[skills]\npaths = [\"{}\"]\n", external.display()),
    )
    .expect("config");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let external_path = external.join("external-skill/SKILL.md");

    let listed = capability_rpc(&state, "skill/list", json!({ "scope": scope.clone() }))
        .await
        .expect("skill/list");
    assert!(listed["skills"].as_array().expect("skills").iter().any(|skill| {
        skill["name"] == "external-skill" && skill["source"] == "config"
    }));

    let rejected = capability_rpc(
        &state,
        "skill/write",
        json!({
            "scope": scope,
            "name": "external-skill",
            "path": external_path,
            "target": "profile",
            "rawMarkdown": "---\nname: external-skill\ndescription: Edited\n---\n\nedited\n"
        }),
    )
    .await
    .expect_err("configured skill is read-only");
    assert!(rejected.to_string().contains("not writable from global scope"));
}

#[tokio::test]
async fn capability_plugin_rpcs_install_toggle_and_uninstall_profile_plugin() {
    let (temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    let source = temp.path().join("managed-plugin");
    let manifest_dir = source.join(".psychevo-plugin");
    std::fs::create_dir_all(&manifest_dir).expect("manifest dir");
    std::fs::write(
        manifest_dir.join("plugin.json"),
        r#"{
          "name": "managed-plugin",
          "version": "1.0.0",
          "description": "managed plugin",
          "interface": {
            "displayName": "Managed Plugin",
            "capabilities": ["tools"]
          }
        }"#,
    )
    .expect("manifest");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();

    let installed = capability_rpc(
        &state,
        "plugin/install",
        json!({
            "scope": scope.clone(),
            "source": source,
            "scopeName": "profile"
        }),
    )
    .await
    .expect("plugin/install");
    assert_eq!(installed["plugin"]["name"], "managed-plugin");

    let disabled = capability_rpc(
        &state,
        "plugin/setEnabled",
        json!({
            "scope": scope.clone(),
            "selector": "managed-plugin",
            "scopeName": "profile",
            "enabled": false
        }),
    )
    .await
    .expect("plugin/setEnabled");
    assert_eq!(disabled["enabled"], false);

    let list = capability_rpc(&state, "plugin/list", json!({ "scope": scope.clone() }))
        .await
        .expect("plugin/list");
    assert_eq!(list["plugins"][0]["name"], "managed-plugin");
    assert_eq!(list["plugins"][0]["enabled"], false);

    let removed = capability_rpc(
        &state,
        "plugin/uninstall",
        json!({
            "scope": scope,
            "selector": "managed-plugin",
            "scopeName": "profile"
        }),
    )
    .await
    .expect("plugin/uninstall");
    assert_eq!(removed["success"], true);
}

#[tokio::test]
async fn plugin_inspect_and_trust_rpcs_return_adapter_state() {
    let (temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    let source = temp.path().join("hermes-plugin");
    std::fs::create_dir_all(&source).expect("source");
    std::fs::write(
        source.join("plugin.yaml"),
        "name: hermes-managed\nversion: 0.2.0\ndescription: hermes\nprovides_tools:\n  - helper\n",
    )
    .expect("manifest");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();

    let inspected = capability_rpc(
        &state,
        "plugin/import/inspect",
        json!({
            "scope": scope.clone(),
            "source": source,
            "sourceKind": "local",
            "adapterMode": "adapter_host"
        }),
    )
    .await
    .expect("plugin/import/inspect");
    assert_eq!(inspected["inspection"]["framework"], "hermes");
    assert_eq!(inspected["inspection"]["status"], "Needs trust");

    let installed = capability_rpc(
        &state,
        "plugin/install",
        json!({
            "scope": scope.clone(),
            "source": source,
            "sourceKind": "local",
            "adapterMode": "adapter_host",
            "scopeName": "profile"
        }),
    )
    .await
    .expect("plugin/install");
    assert_eq!(installed["plugin"]["manifest_kind"], "hermes");

    let trusted = capability_rpc(
        &state,
        "plugin/setTrust",
        json!({
            "scope": scope,
            "selector": "hermes-managed",
            "trusted": true,
            "scopeName": "profile"
        }),
    )
    .await
    .expect("plugin/setTrust");
    assert_eq!(trusted["trust"]["status"], "trusted");
}

#[tokio::test]
async fn capability_tool_and_mcp_rpcs_write_profile_config_without_inline_secrets() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();

    let created = capability_rpc(
        &state,
        "tool/create",
        json!({
            "scope": scope.clone(),
            "name": "review-tools",
            "description": "Review tools",
            "tools": ["read", "unknown_tool"],
            "includes": ["web"]
        }),
    )
    .await
    .expect("tool/create");
    assert_eq!(created["name"], "review-tools");
    let expected_profile_config = state.inner.home.join("config.toml");
    assert_eq!(
        created["path"].as_str(),
        Some(expected_profile_config.to_string_lossy().as_ref())
    );
    assert!(!state.inner.cwd.join(".psychevo/config.toml").exists());

    capability_rpc(
        &state,
        "tool/setEnabled",
        json!({
            "scope": scope.clone(),
            "name": "review-tools",
            "mode": "plan",
            "enabled": false
        }),
    )
    .await
    .expect("tool/setEnabled");
    let tool = capability_rpc(
        &state,
        "tool/read",
        json!({
            "scope": scope.clone(),
            "name": "review-tools"
        }),
    )
    .await
    .expect("tool/read");
    assert_eq!(tool["toolset"]["source"], "custom");
    assert_eq!(tool["toolset"]["unknown_tools"][0], "unknown_tool");

    let upserted = capability_rpc(
        &state,
        "mcp/upsert",
        json!({
            "scope": scope.clone(),
            "name": "docs",
            "transport": "streamable_http",
            "url": "https://mcp.example.test/mcp",
            "headers": { "X-Trace": "trace" },
            "bearerTokenEnvVar": "DOCS_MCP_TOKEN",
            "scopes": ["docs.read"],
            "oauthResource": "https://auth.example.test",
            "oauthClientId": "client-1",
            "enabledTools": ["search"],
            "disabledTools": ["delete"]
        }),
    )
    .await
    .expect("mcp/upsert");
    assert_eq!(upserted["server"]["config"]["bearer_token_env_var"], "DOCS_MCP_TOKEN");
    assert!(upserted.to_string().contains("DOCS_MCP_TOKEN"));
    assert!(!upserted.to_string().contains("Bearer "));

    let server = capability_rpc(
        &state,
        "mcp/read",
        json!({
            "scope": scope.clone(),
            "name": "docs"
        }),
    )
    .await
    .expect("mcp/read");
    assert_eq!(
        server["server"]["transport"]["auth"]["bearerTokenEnvVar"],
        "DOCS_MCP_TOKEN"
    );
    assert_eq!(server["server"]["policy"]["enabledTools"][0], "search");

    capability_rpc(
        &state,
        "mcp/setToolPolicy",
        json!({
            "scope": scope.clone(),
            "name": "docs",
            "enabledTools": ["fetch"],
            "disabledTools": []
        }),
    )
    .await
    .expect("mcp/setToolPolicy");
    let disabled = capability_rpc(
        &state,
        "mcp/setEnabled",
        json!({
            "scope": scope.clone(),
            "name": "docs",
            "enabled": false
        }),
    )
    .await
    .expect("mcp/setEnabled");
    assert_eq!(disabled["server"]["config"]["enabled"], false);

    let inline_token = capability_rpc(
        &state,
        "mcp/upsert",
        json!({
            "scope": scope.clone(),
            "name": "bad",
            "transport": "streamable_http",
            "url": "https://mcp.example.test/mcp",
            "headers": { "Authorization": "Bearer secret" }
        }),
    )
    .await
    .expect_err("inline bearer must be rejected");
    assert!(inline_token.to_string().contains("bearer_token_env_var"));

    let removed_mcp = capability_rpc(
        &state,
        "mcp/remove",
        json!({
            "scope": scope.clone(),
            "name": "docs"
        }),
    )
    .await
    .expect("mcp/remove");
    assert_eq!(removed_mcp["success"], true);
    let removed_tool = capability_rpc(
        &state,
        "tool/remove",
        json!({
            "scope": scope,
            "name": "review-tools"
        }),
    )
    .await
    .expect("tool/remove");
    assert_eq!(removed_tool["success"], true);
}

#[tokio::test]
async fn capability_tool_rpcs_reject_coding_core_configuration() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(state.inner.home.join("config.toml"), "# config\n").expect("config");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();

    let list = capability_rpc(&state, "tool/list", json!({ "scope": scope.clone() }))
        .await
        .expect("tool/list");
    let coding_core = list["toolsets"]
        .as_array()
        .expect("toolsets")
        .iter()
        .find(|toolset| toolset["name"] == "coding-core")
        .expect("coding-core toolset");
    assert_eq!(coding_core["mode_mutable"], false);
    assert_eq!(coding_core["removable"], false);
    let web = list["toolsets"]
        .as_array()
        .expect("toolsets")
        .iter()
        .find(|toolset| toolset["name"] == "web")
        .expect("web toolset");
    assert_eq!(web["mode_mutable"], true);
    assert_eq!(web["removable"], false);

    let read = capability_rpc(
        &state,
        "tool/read",
        json!({
            "scope": scope.clone(),
            "name": "coding-core"
        }),
    )
    .await
    .expect("tool/read");
    assert_eq!(read["toolset"]["mode_mutable"], false);
    assert_eq!(read["toolset"]["removable"], false);

    let err = capability_rpc(
        &state,
        "tool/setEnabled",
        json!({
            "scope": scope.clone(),
            "name": "coding-core",
            "mode": "default",
            "enabled": false
        }),
    )
    .await
    .expect_err("coding-core setEnabled should fail");
    assert!(
        err.to_string()
            .contains("built-in toolset coding-core cannot be configured"),
        "{err}"
    );

    let err = capability_rpc(
        &state,
        "tool/create",
        json!({
            "scope": scope,
            "name": "coding-core",
            "description": "override",
            "tools": ["read"],
            "force": true
        }),
    )
    .await
    .expect_err("coding-core create should fail");
    assert!(
        err.to_string()
            .contains("built-in toolset coding-core cannot be overwritten"),
        "{err}"
    );
    assert_eq!(
        std::fs::read_to_string(state.inner.home.join("config.toml")).expect("config"),
        "# config\n"
    );
    assert!(!state.inner.cwd.join(".psychevo/config.toml").exists());
}

async fn capability_rpc(
    state: &WebState,
    method: &str,
    params: Value,
) -> psychevo_runtime::Result<Value> {
    let (tx, _rx) = mpsc::unbounded_channel();
    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(method)),
            method: method.to_string(),
            params: Some(params),
        },
    )
    .await
}

fn write_capability_package_skill(
    root: &std::path::Path,
    name: &str,
    description: &str,
    body: &str,
) {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).expect("skill dir");
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description:?}\n---\n\n{body}\n"),
    )
    .expect("skill");
}
