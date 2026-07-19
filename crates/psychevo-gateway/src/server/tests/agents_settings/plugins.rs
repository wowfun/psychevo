#[tokio::test]
async fn plugin_read_rpcs_return_manifest_metadata_without_mutation() {
    let (temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(state.inner.home.join("config.toml"), "# config\n").expect("config");
    let source = temp.path().join("display-plugin");
    let manifest_dir = source.join(".codex-plugin");
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
    assert_eq!(list["count"], 2);
    assert!(
        list["plugins"]
            .as_array()
            .expect("plugins")
            .iter()
            .any(|plugin| {
                plugin["name"] == "Browser" && plugin["source_id"] == "builtin:browser"
            })
    );
    assert!(
        list["plugins"]
            .as_array()
            .expect("plugins")
            .iter()
            .any(|plugin| { plugin["name"] == "display-plugin" })
    );

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

#[cfg(unix)]
#[tokio::test]
async fn codex_plugin_rpcs_preserve_authority_and_delegate_catalog_mutation() {
    use std::os::unix::fs::PermissionsExt;

    let bootstrap = tempfile::tempdir().expect("bootstrap");
    let script = bootstrap.path().join("fake-codex.py");
    let log = bootstrap.path().join("calls.log");
    std::fs::write(
        &script,
        format!(
            r#"#!/usr/bin/env python3
import json, os, sys
LOG = {log}
for line in sys.stdin:
    msg = json.loads(line)
    method = msg.get("method")
    if method == "initialize":
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"result":{{"codexHome":os.environ["CODEX_HOME"],"platformFamily":"unix","platformOs":"linux","userAgent":"gateway-fixture/0.144.1"}}}}), flush=True)
    elif method == "initialized":
        pass
    elif msg.get("params") is None:
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"error":{{"code":-32602,"message":"invalid params"}}}}), flush=True)
    elif method == "plugin/list":
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"result":{{"marketplaces":[{{"name":"openai","path":None,"plugins":[{{"id":"review@openai","name":"review","installed":False,"enabled":False,"interface":{{"shortDescription":"Review"}}}}]}}],"marketplaceLoadErrors":[],"featuredPluginIds":[]}}}}), flush=True)
    elif method == "plugin/installed":
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"result":{{"marketplaces":[],"marketplaceLoadErrors":[]}}}}), flush=True)
    elif method == "plugin/read":
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"result":{{"plugin":{{"marketplaceName":"openai","summary":{{"id":"review@openai","name":"review","installed":False,"enabled":False}},"description":"Review plugin","skills":[],"hooks":[],"apps":[{{"id":"review-app"}}],"mcpServers":[]}}}}}}), flush=True)
    elif method == "plugin/install":
        with open(LOG, "a", encoding="utf-8") as handle:
            handle.write("install\n")
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"result":{{"authPolicy":"ON_USE","appsNeedingAuth":[]}}}}), flush=True)
    elif method == "plugin/uninstall":
        with open(LOG, "a", encoding="utf-8") as handle:
            handle.write("uninstall\n")
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"result":{{}}}}), flush=True)
    elif method == "app/list":
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"result":{{"data":[],"nextCursor":None}}}}), flush=True)
"#,
            log = serde_json::to_string(&log).expect("log json"),
        ),
    )
    .expect("script");
    let mut permissions = std::fs::metadata(&script).expect("metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script, permissions).expect("chmod");
    let (_temp, state) = web_state_with_env(BTreeMap::from([
        (
            "PSYCHEVO_CODEX_BIN".to_string(),
            script.display().to_string(),
        ),
        (
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_default(),
        ),
    ]));
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(state.inner.home.join("config.toml"), "# config\n").expect("config");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    capability_rpc(
        &state,
        "plugin/authority/write",
        json!({"scope":scope.clone(),"enabled":true,"binary":script}),
    )
    .await
    .expect("enable Codex authority");

    let list = capability_rpc(&state, "plugin/list", json!({"scope":scope.clone()}))
        .await
        .expect("plugin/list");
    let codex = list["plugins"]
        .as_array()
        .expect("plugins")
        .iter()
        .find(|plugin| plugin["selector"] == "codex:review@openai")
        .unwrap_or_else(|| panic!("Codex plugin missing from {list}"));
    assert_eq!(codex["authority"]["kind"], "codex");
    assert_eq!(codex["canonical_id"], "review@openai");

    let read = capability_rpc(
        &state,
        "plugin/read",
        json!({"scope":scope.clone(),"selector":"codex:review@openai"}),
    )
    .await
    .expect("plugin/read");
    assert_eq!(read["plugin"]["authority"]["marketplace"], "openai");
    assert_eq!(
        read["plugin"]["component_statuses"][0]["executionOwner"],
        "codex_broker"
    );

    let installed = capability_rpc(
        &state,
        "plugin/install",
        json!({"scope":scope.clone(),"source":"codex:review@openai"}),
    )
    .await
    .expect("plugin/install");
    assert_eq!(installed["success"], true);
    assert_eq!(installed["authority"]["kind"], "codex");
    assert!(installed.get("result").is_none());
    let doctor = capability_rpc(
        &state,
        "plugin/doctor",
        json!({"scope":scope.clone(),"selector":"codex:review@openai"}),
    )
    .await
    .expect("plugin/doctor");
    assert_eq!(doctor["apps"]["data"], json!([]));
    let removed = capability_rpc(
        &state,
        "plugin/uninstall",
        json!({"scope":scope,"selector":"codex:review@openai"}),
    )
    .await
    .expect("plugin/uninstall");
    assert_eq!(removed["authority"]["kind"], "codex");
    assert_eq!(
        std::fs::read_to_string(&log).expect("calls"),
        "install\nuninstall\n"
    );
    assert!(!state.inner.home.join("plugins/records").exists());
    state.inner.codex_capability_broker.stop().await;
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
    assert!(
        !state
            .inner
            .cwd
            .join(".psychevo/skills/review-flow/SKILL.md")
            .exists()
    );
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
    let project_path = state.inner.cwd.join(".psychevo/skills/same-skill/SKILL.md");
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
    assert!(
        refused
            .to_string()
            .contains("not removable from project scope")
    );

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
    let project_path = state.inner.cwd.join(".psychevo/skills/same-skill/SKILL.md");
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
    assert!(
        std::fs::read_to_string(&project_path)
            .expect("project skill")
            .contains("project updated body")
    );

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
    assert!(
        std::fs::read_to_string(&profile_path)
            .expect("profile skill")
            .contains("profile updated body")
    );

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
    assert!(
        std::fs::read_to_string(&project_path)
            .expect("project skill unchanged")
            .contains("project updated body")
    );
}

#[tokio::test]
async fn capability_skill_write_rejects_configured_external_skill() {
    let (temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::create_dir_all(state.inner.home.join("skills")).expect("profile skills root");
    let external = temp.path().join("external-skills");
    write_capability_package_skill(
        &external,
        "external-skill",
        "External skill",
        "external body",
    );
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
    assert!(
        listed["skills"]
            .as_array()
            .expect("skills")
            .iter()
            .any(|skill| { skill["name"] == "external-skill" && skill["source"] == "config" })
    );

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
    assert!(
        rejected
            .to_string()
            .contains("not writable from global scope")
    );
}

#[tokio::test]
async fn capability_plugin_rpcs_project_builtin_browser_plugin() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(state.inner.home.join("config.toml"), "# config\n").expect("config");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();

    let list = capability_rpc(&state, "plugin/list", json!({ "scope": scope.clone() }))
        .await
        .expect("plugin/list");
    let browser = list["plugins"]
        .as_array()
        .expect("plugins")
        .iter()
        .find(|plugin| plugin["source_id"] == "builtin:browser")
        .expect("browser plugin");
    assert_eq!(browser["name"], "Browser");
    assert_eq!(browser["source_kind"], "built_in");
    assert_eq!(browser["selector"], "builtin:browser");
    assert_eq!(browser["scope_name"], "profile");
    assert_eq!(browser["removable"], false);
    assert_eq!(browser["package_mutable"], false);
    assert_eq!(browser["enablement_mutable"], true);
    assert_eq!(browser["enabled"], true);
    assert_eq!(browser["status"], "Installed");
    assert_eq!(
        browser["contributions"]["annotation"][0],
        "workspace_comment_context_xml"
    );

    let read = capability_rpc(
        &state,
        "plugin/read",
        json!({
            "scope": scope.clone(),
            "selector": "builtin:browser"
        }),
    )
    .await
    .expect("plugin/read");
    assert_eq!(read["manifest"]["interface"]["displayName"], "Browser");
    assert_eq!(read["plugin"]["status"], "Installed");
    assert_eq!(read["inspection"]["status"], "Installed");

    let disabled = capability_rpc(
        &state,
        "plugin/setEnabled",
        json!({
            "scope": scope.clone(),
            "selector": "builtin:browser",
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
    let browser = list["plugins"]
        .as_array()
        .expect("plugins")
        .iter()
        .find(|plugin| plugin["source_id"] == "builtin:browser")
        .expect("browser plugin");
    assert_eq!(browser["enabled"], false);
    assert_eq!(browser["status"], "Disabled");

    let read = capability_rpc(
        &state,
        "plugin/read",
        json!({
            "scope": scope.clone(),
            "selector": "builtin:browser"
        }),
    )
    .await
    .expect("plugin/read disabled browser");
    assert_eq!(read["plugin"]["status"], "Disabled");
    assert_eq!(read["inspection"]["status"], "Disabled");

    let doctor = capability_rpc(
        &state,
        "plugin/doctor",
        json!({
            "scope": scope.clone(),
            "selector": "builtin:browser"
        }),
    )
    .await
    .expect("plugin/doctor");
    assert_eq!(doctor["plugins"][0]["plugin"]["name"], "Browser");
    assert_eq!(doctor["plugins"][0]["plugin"]["status"], "Disabled");
    assert_eq!(doctor["plugins"][0]["inspection"]["status"], "Disabled");
    assert_eq!(doctor["plugins"][0]["worker"]["status"], "not_applicable");

    let rejected = capability_rpc(
        &state,
        "plugin/uninstall",
        json!({
            "scope": scope,
            "selector": "builtin:browser",
            "scopeName": "profile"
        }),
    )
    .await
    .expect_err("built-in Browser is not removable");
    assert!(
        rejected
            .to_string()
            .contains("built-in plugin `builtin:browser` cannot be uninstalled")
    );
}

#[tokio::test]
async fn capability_plugin_builtin_browser_projects_effective_project_policy_scope() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        "[builtin_plugins.browser]\nenabled = true\n",
    )
    .expect("profile config");
    let project_config_dir = state.inner.cwd.join(".psychevo");
    std::fs::create_dir_all(&project_config_dir).expect("project config dir");
    std::fs::write(
        project_config_dir.join("config.toml"),
        "[builtin_plugins.browser]\nenabled = false\n",
    )
    .expect("project config");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();

    let list = capability_rpc(&state, "plugin/list", json!({ "scope": scope.clone() }))
        .await
        .expect("plugin/list");
    let browser = list["plugins"]
        .as_array()
        .expect("plugins")
        .iter()
        .find(|plugin| plugin["selector"] == "builtin:browser")
        .expect("built-in browser");
    assert_eq!(browser["enabled"], false);
    assert_eq!(browser["status"], "Disabled");
    assert_eq!(browser["scope_name"], "project");

    capability_rpc(
        &state,
        "plugin/setEnabled",
        json!({
            "scope": scope.clone(),
            "selector": "builtin:browser",
            "scopeName": browser["scope_name"],
            "enabled": true
        }),
    )
    .await
    .expect("enable built-in browser in effective scope");

    let read = capability_rpc(
        &state,
        "plugin/read",
        json!({
            "scope": scope,
            "selector": "builtin:browser"
        }),
    )
    .await
    .expect("plugin/read");
    assert_eq!(read["plugin"]["enabled"], true);
    assert_eq!(read["plugin"]["status"], "Installed");
    assert_eq!(read["plugin"]["scope_name"], "project");
}

#[tokio::test]
async fn capability_plugin_builtin_browser_selector_and_policy_do_not_capture_browser_package() {
    let (temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(state.inner.home.join("config.toml"), "# config\n").expect("config");
    let source = temp.path().join("browser-package");
    let manifest_dir = source.join(".codex-plugin");
    std::fs::create_dir_all(&manifest_dir).expect("manifest dir");
    std::fs::write(
        manifest_dir.join("plugin.json"),
        r#"{
          "name": "browser",
          "version": "1.0.0",
          "description": "ordinary browser package"
        }"#,
    )
    .expect("manifest");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();

    capability_rpc(
        &state,
        "plugin/install",
        json!({
            "scope": scope.clone(),
            "source": source,
            "scopeName": "profile"
        }),
    )
    .await
    .expect("install browser package");

    let package = capability_rpc(
        &state,
        "plugin/read",
        json!({
            "scope": scope.clone(),
            "selector": "browser"
        }),
    )
    .await
    .expect("read ordinary browser package");
    assert_eq!(package["plugin"]["name"], "browser");
    assert_eq!(package["plugin"]["source_kind"], "local");

    let legacy_alias = capability_rpc(
        &state,
        "plugin/read",
        json!({
            "scope": scope.clone(),
            "selector": "Browser"
        }),
    )
    .await
    .expect_err("Browser alias must not select the built-in plugin");
    assert!(
        legacy_alias
            .to_string()
            .contains("plugin not found: Browser")
    );

    capability_rpc(
        &state,
        "plugin/setEnabled",
        json!({
            "scope": scope.clone(),
            "selector": "browser",
            "scopeName": "profile",
            "enabled": true
        }),
    )
    .await
    .expect("enable ordinary browser package");

    capability_rpc(
        &state,
        "plugin/setEnabled",
        json!({
            "scope": scope.clone(),
            "selector": "builtin:browser",
            "scopeName": "profile",
            "enabled": false
        }),
    )
    .await
    .expect("disable built-in browser");

    let list = capability_rpc(&state, "plugin/list", json!({ "scope": scope.clone() }))
        .await
        .expect("plugin/list");
    let plugins = list["plugins"].as_array().expect("plugins");
    let builtin = plugins
        .iter()
        .find(|plugin| plugin["source_id"] == "builtin:browser")
        .expect("built-in browser");
    let package = plugins
        .iter()
        .find(|plugin| plugin["name"] == "browser" && plugin["source_kind"] == "local")
        .expect("ordinary browser package");
    assert_eq!(builtin["enabled"], false);
    assert_eq!(package["enabled"], true);

    let removed = capability_rpc(
        &state,
        "plugin/uninstall",
        json!({
            "scope": scope.clone(),
            "selector": "browser",
            "scopeName": "profile"
        }),
    )
    .await
    .expect("uninstall ordinary browser package");
    assert_eq!(removed["plugin"], "browser");

    let builtin = capability_rpc(
        &state,
        "plugin/read",
        json!({
            "scope": scope.clone(),
            "selector": "builtin:browser"
        }),
    )
    .await
    .expect("built-in browser remains after ordinary package uninstall");
    assert_eq!(builtin["plugin"]["status"], "Disabled");

    let removed_package = capability_rpc(
        &state,
        "plugin/read",
        json!({
            "scope": scope,
            "selector": "browser"
        }),
    )
    .await
    .expect_err("ordinary browser package was removed");
    assert!(
        removed_package
            .to_string()
            .contains("plugin not found: browser")
    );
}

#[tokio::test]
async fn capability_plugin_rpcs_project_selector_and_mutate_at_package_scope() {
    let (temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(state.inner.home.join("config.toml"), "# config\n").expect("config");
    let source = temp.path().join("managed-plugin");
    let manifest_dir = source.join(".codex-plugin");
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
            "scopeName": "project"
        }),
    )
    .await
    .expect("plugin/install");
    assert_eq!(installed["plugin"]["name"], "managed-plugin");

    let list = capability_rpc(&state, "plugin/list", json!({ "scope": scope.clone() }))
        .await
        .expect("plugin/list");
    let managed = list["plugins"]
        .as_array()
        .expect("plugins")
        .iter()
        .find(|plugin| plugin["name"] == "managed-plugin")
        .expect("managed plugin");
    let selector = managed["selector"]
        .as_str()
        .expect("canonical selector")
        .to_string();
    assert!(
        selector.starts_with("project:managed-plugin@"),
        "{selector}"
    );
    assert_eq!(managed["scope_name"], "project");
    assert_eq!(managed["enablement_scope_name"], "project");
    assert_eq!(managed["removable"], true);
    assert_eq!(managed["package_mutable"], true);
    assert_eq!(managed["enablement_mutable"], true);

    let read = capability_rpc(
        &state,
        "plugin/read",
        json!({
            "scope": scope.clone(),
            "selector": selector.clone()
        }),
    )
    .await
    .expect("plugin/read");
    assert_eq!(read["plugin"]["selector"], selector);
    assert_eq!(read["plugin"]["scope_name"], "project");

    let doctor = capability_rpc(
        &state,
        "plugin/doctor",
        json!({
            "scope": scope.clone(),
            "selector": selector.clone()
        }),
    )
    .await
    .expect("plugin/doctor");
    assert_eq!(doctor["plugins"][0]["plugin"]["selector"], selector);
    assert_eq!(doctor["plugins"][0]["plugin"]["scope_name"], "project");

    let enabled = capability_rpc(
        &state,
        "plugin/setEnabled",
        json!({
            "scope": scope.clone(),
            "selector": selector.clone(),
            "scopeName": "project",
            "enabled": true
        }),
    )
    .await
    .expect("plugin/setEnabled");
    assert_eq!(enabled["enabled"], true);

    let list = capability_rpc(&state, "plugin/list", json!({ "scope": scope.clone() }))
        .await
        .expect("plugin/list");
    let managed = list["plugins"]
        .as_array()
        .expect("plugins")
        .iter()
        .find(|plugin| plugin["name"] == "managed-plugin")
        .expect("managed plugin");
    assert_eq!(managed["enabled"], true);

    let removed = capability_rpc(
        &state,
        "plugin/uninstall",
        json!({
            "scope": scope,
            "selector": selector,
            "scopeName": "project"
        }),
    )
    .await
    .expect("plugin/uninstall");
    assert_eq!(removed["success"], true);
}

#[tokio::test]
async fn capability_plugin_scoped_selectors_disambiguate_duplicate_installations() {
    let (temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(state.inner.home.join("config.toml"), "# config\n").expect("config");
    let source = temp.path().join("dual-scope-plugin");
    let manifest_dir = source.join(".codex-plugin");
    std::fs::create_dir_all(&manifest_dir).expect("manifest dir");
    std::fs::write(
        manifest_dir.join("plugin.json"),
        r#"{
          "name": "dual-scope",
          "version": "1.0.0",
          "description": "same source in profile and project"
        }"#,
    )
    .expect("manifest");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();

    for scope_name in ["profile", "project"] {
        capability_rpc(
            &state,
            "plugin/install",
            json!({
                "scope": scope.clone(),
                "source": source.clone(),
                "scopeName": scope_name
            }),
        )
        .await
        .unwrap_or_else(|err| panic!("install {scope_name}: {err}"));
    }

    let list = capability_rpc(&state, "plugin/list", json!({ "scope": scope.clone() }))
        .await
        .expect("plugin/list");
    let rows = list["plugins"]
        .as_array()
        .expect("plugins")
        .iter()
        .filter(|plugin| plugin["name"] == "dual-scope")
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2);
    let profile_selector = rows
        .iter()
        .find(|plugin| plugin["scope_name"] == "profile")
        .and_then(|plugin| plugin["selector"].as_str())
        .expect("profile selector")
        .to_string();
    let project_selector = rows
        .iter()
        .find(|plugin| plugin["scope_name"] == "project")
        .and_then(|plugin| plugin["selector"].as_str())
        .expect("project selector")
        .to_string();
    assert!(profile_selector.starts_with("profile:dual-scope@"));
    assert!(project_selector.starts_with("project:dual-scope@"));
    assert_ne!(profile_selector, project_selector);
    assert_eq!(
        profile_selector.strip_prefix("profile:"),
        project_selector.strip_prefix("project:")
    );
    assert_eq!(rows[0]["enablement_scope_name"], rows[0]["scope_name"]);
    assert_eq!(rows[1]["enablement_scope_name"], rows[1]["scope_name"]);

    let trust_scope_mismatch = capability_rpc(
        &state,
        "plugin/setTrust",
        json!({
            "scope": scope.clone(),
            "selector": profile_selector.clone(),
            "scopeName": "project",
            "trusted": true
        }),
    )
    .await
    .expect_err("package trust scope must match installation scope");
    assert!(
        trust_scope_mismatch
            .to_string()
            .contains("installed in profile scope, not project scope")
    );

    let unscoped = profile_selector
        .strip_prefix("profile:")
        .expect("profile prefix");
    let ambiguous = capability_rpc(
        &state,
        "plugin/read",
        json!({ "scope": scope.clone(), "selector": unscoped }),
    )
    .await
    .expect_err("unscoped duplicate selector is ambiguous");
    assert!(
        ambiguous
            .to_string()
            .contains("use profile:name@source or project:name@source")
    );

    for (selector, expected_scope) in [
        (&profile_selector, "profile"),
        (&project_selector, "project"),
    ] {
        let read = capability_rpc(
            &state,
            "plugin/read",
            json!({ "scope": scope.clone(), "selector": selector }),
        )
        .await
        .unwrap_or_else(|err| panic!("read {expected_scope}: {err}"));
        assert_eq!(read["plugin"]["scope_name"], expected_scope);

        let doctor = capability_rpc(
            &state,
            "plugin/doctor",
            json!({ "scope": scope.clone(), "selector": selector }),
        )
        .await
        .unwrap_or_else(|err| panic!("doctor {expected_scope}: {err}"));
        assert_eq!(doctor["plugins"][0]["plugin"]["scope_name"], expected_scope);
    }

    capability_rpc(
        &state,
        "plugin/setEnabled",
        json!({
            "scope": scope.clone(),
            "selector": profile_selector,
            "scopeName": "profile",
            "enabled": false
        }),
    )
    .await
    .expect("disable profile installation");
    capability_rpc(
        &state,
        "plugin/setEnabled",
        json!({
            "scope": scope.clone(),
            "selector": project_selector.clone(),
            "scopeName": "project",
            "enabled": true
        }),
    )
    .await
    .expect("enable project installation");

    let list = capability_rpc(&state, "plugin/list", json!({ "scope": scope.clone() }))
        .await
        .expect("plugin/list after enablement");
    let rows = list["plugins"]
        .as_array()
        .expect("plugins")
        .iter()
        .filter(|plugin| plugin["name"] == "dual-scope")
        .collect::<Vec<_>>();
    let profile = rows
        .iter()
        .find(|plugin| plugin["scope_name"] == "profile")
        .expect("profile row");
    let project = rows
        .iter()
        .find(|plugin| plugin["scope_name"] == "project")
        .expect("project row");
    assert_eq!(profile["enabled"], false);
    assert_eq!(project["enabled"], true);

    capability_rpc(
        &state,
        "plugin/setEnabled",
        json!({
            "scope": scope.clone(),
            "selector": profile_selector.clone(),
            "scopeName": "project",
            "enabled": true
        }),
    )
    .await
    .expect("project policy overrides profile installation");
    let list = capability_rpc(&state, "plugin/list", json!({ "scope": scope.clone() }))
        .await
        .expect("plugin/list after project override");
    let profile = list["plugins"]
        .as_array()
        .expect("plugins")
        .iter()
        .find(|plugin| plugin["selector"] == profile_selector)
        .expect("profile row after project override");
    assert_eq!(profile["scope_name"], "profile");
    assert_eq!(profile["enablement_scope_name"], "project");
    assert_eq!(profile["enabled"], true);

    capability_rpc(
        &state,
        "plugin/uninstall",
        json!({
            "scope": scope.clone(),
            "selector": project_selector,
            "scopeName": "project"
        }),
    )
    .await
    .expect("uninstall project installation");
    let remaining = capability_rpc(
        &state,
        "plugin/read",
        json!({ "scope": scope, "selector": profile_selector }),
    )
    .await
    .expect("profile installation remains");
    assert_eq!(remaining["plugin"]["scope_name"], "profile");
    assert_eq!(remaining["plugin"]["enablement_scope_name"], "project");
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
    assert_eq!(
        upserted["server"]["config"]["bearer_token_env_var"],
        "DOCS_MCP_TOKEN"
    );
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

#[tokio::test]
async fn codex_authority_is_default_off_and_policy_is_profile_dominant() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(state.inner.home.join("config.toml"), "# config\n").expect("config");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();

    let list = capability_rpc(&state, "plugin/list", json!({"scope":scope.clone()}))
        .await
        .expect("plugin/list");
    let codex = list["authorities"]
        .as_array()
        .expect("authorities")
        .iter()
        .find(|authority| authority["kind"] == "codex")
        .expect("Codex authority");
    assert_eq!(codex["runtime"], "disabled");
    assert_eq!(codex["privateHome"], json!(state.inner.home.join("codex")));

    let profile = capability_rpc(
        &state,
        "plugin/setEnabled",
        json!({
            "scope": scope.clone(),
            "selector": "codex:review@openai",
            "scopeName": "profile",
            "enabled": true
        }),
    )
    .await
    .expect("profile allow");
    assert_eq!(profile["policy"]["profileEnabled"], true);
    assert_eq!(profile["policy"]["effectiveEnabled"], true);

    let project_enable = capability_rpc(
        &state,
        "plugin/setEnabled",
        json!({
            "scope": scope.clone(),
            "selector": "codex:review@openai",
            "scopeName": "project",
            "enabled": true
        }),
    )
    .await
    .expect_err("project enable rejected");
    assert!(project_enable.to_string().contains("cannot enable"));

    let project_disable = capability_rpc(
        &state,
        "plugin/setEnabled",
        json!({
            "scope": scope.clone(),
            "selector": "codex:review@openai",
            "scopeName": "project",
            "enabled": false
        }),
    )
    .await
    .expect("project disable");
    assert_eq!(project_disable["policy"]["projectOverride"], false);
    assert_eq!(project_disable["policy"]["effectiveEnabled"], false);

    let reset = capability_rpc(
        &state,
        "plugin/setEnabled",
        json!({
            "scope": scope,
            "selector": "codex:review@openai",
            "scopeName": "project",
            "enabled": null
        }),
    )
    .await
    .expect("project reset");
    assert_eq!(reset["policy"]["projectOverride"], Value::Null);
    assert_eq!(reset["policy"]["effectiveEnabled"], true);
}

#[tokio::test]
async fn codex_authority_write_is_profile_scoped_and_preserves_private_home() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(state.inner.home.join("config.toml"), "# config\n").expect("config");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();

    let value = capability_rpc(
        &state,
        "plugin/authority/write",
        json!({
            "scope": scope,
            "enabled": false,
            "binary": "/opt/reviewed-codex"
        }),
    )
    .await
    .expect("authority write");

    assert_eq!(value["authority"]["runtime"], "disabled");
    assert_eq!(
        value["authority"]["privateHome"],
        json!(state.inner.home.join("codex"))
    );
    let config =
        std::fs::read_to_string(state.inner.home.join("config.toml")).expect("profile config");
    assert!(config.contains("[codex_plugins]"));
    assert!(config.contains("binary = \"/opt/reviewed-codex\""));
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
