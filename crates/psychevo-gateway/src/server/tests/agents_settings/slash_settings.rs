#[tokio::test]
async fn slash_settings_read_update_writes_profile_config_and_preserves_project_overrides() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::create_dir_all(state.inner.cwd.join(".psychevo")).expect("project config dir");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"
[provider.keep]
label = "Keep Provider"
"#,
    )
    .expect("global config");
    let project_config = r#"
[tui.slash_aliases]
"/status" = ["/project"]
"#;
    std::fs::write(
        state.inner.cwd.join(".psychevo/config.toml"),
        project_config,
    )
    .expect("project config");
    let (tx, _rx) = mpsc::unbounded_channel();

    let before = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("before")),
            method: "slash/settings/read".to_string(),
            params: Some(json!({
                "scope": "global",
                "cwd": state.inner.cwd.display().to_string()
            })),
        },
    )
    .await
    .expect("slash/settings/read");
    assert_eq!(before["aliases"], json!([]));

    let updated = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("update")),
            method: "slash/settings/update".to_string(),
            params: Some(json!({
                "scope": "global",
                "cwd": state.inner.cwd.display().to_string(),
                "leaderKey": "ctrl+x",
                "leaderTimeoutMs": 1500,
                "aliases": [
                    { "alias": "/st", "target": "/status" }
                ],
                "keybinds": [
                    { "shortcut": "<leader>s", "target": "/status" }
                ]
            })),
        },
    )
    .await
    .expect("slash/settings/update");
    assert_eq!(updated["leaderKey"], "ctrl+x");
    assert_eq!(updated["leaderTimeoutMs"], 1500);
    assert_eq!(updated["aliases"][0]["alias"], "/st");
    assert_eq!(updated["aliases"][0]["target"], "/status");
    assert_eq!(updated["aliases"][0]["targetSummary"], "show local status");
    assert_eq!(updated["keybinds"][0]["shortcut"], "<leader>s");

    let global_config =
        std::fs::read_to_string(state.inner.home.join("config.toml")).expect("global config");
    let parsed: toml::Value = toml::from_str(&global_config).expect("global toml");
    assert_eq!(
        parsed["provider"]["keep"]["label"].as_str(),
        Some("Keep Provider")
    );
    assert_eq!(
        parsed["tui"]["slash_aliases"]["/status"][0].as_str(),
        Some("/st")
    );
    assert_eq!(
        parsed["tui"]["slash_keybinds"]["/status"][0].as_str(),
        Some("<leader>s")
    );
    assert_eq!(
        std::fs::read_to_string(state.inner.cwd.join(".psychevo/config.toml"))
            .expect("project config"),
        project_config
    );
}

#[tokio::test]
async fn slash_settings_update_rejects_invalid_rows_and_conflicts() {
    let (_temp, state) = web_state();
    let (tx, _rx) = mpsc::unbounded_channel();
    let cases = [
        (
            "invalid-alias",
            json!({
                "scope": "global",
                "aliases": [{ "alias": "st", "target": "/status" }]
            }),
            "slash alias",
        ),
        (
            "invalid-target",
            json!({
                "scope": "global",
                "aliases": [{ "alias": "/st", "target": "/made-up" }]
            }),
            "dynamic skill or bundle",
        ),
        (
            "invalid-export-target",
            json!({
                "scope": "global",
                "aliases": [{ "alias": "/expr", "target": "/export --bad" }]
            }),
            "usage: /export",
        ),
        (
            "built-in-alias",
            json!({
                "scope": "global",
                "aliases": [{ "alias": "/status", "target": "/usage" }]
            }),
            "conflicts with built-in",
        ),
        (
            "duplicate-alias",
            json!({
                "scope": "global",
                "aliases": [
                    { "alias": "/st", "target": "/status" },
                    { "alias": "/st", "target": "/usage" }
                ]
            }),
            "duplicate slash alias",
        ),
        (
            "duplicate-shortcut",
            json!({
                "scope": "global",
                "keybinds": [
                    { "shortcut": "<leader>s", "target": "/status" },
                    { "shortcut": "<leader>s", "target": "/usage" }
                ]
            }),
            "duplicate slash shortcut",
        ),
        (
            "fixed-key-conflict",
            json!({
                "scope": "global",
                "keybinds": [
                    { "shortcut": "ctrl+c", "target": "/status" }
                ]
            }),
            "conflicts with fixed key",
        ),
    ];

    for (id, params, expected) in cases {
        let err = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(id)),
                method: "slash/settings/update".to_string(),
                params: Some(params),
            },
        )
        .await
        .expect_err(id);
        assert!(
            err.to_string().contains(expected),
            "{id}: expected {expected:?}, got {err}"
        );
    }
}
