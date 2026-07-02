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
            scope: psychevo_runtime::PluginScope::Global,
            git_ref: None,
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
