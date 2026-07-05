#[tokio::test]
async fn completion_list_returns_cwd_files() {
    let (_temp, state) = web_state();
    let src = state.inner.cwd.join("src");
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
    let item = result["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|item| item["label"] == "@src/main.rs")
        .expect("main.rs completion");
    assert_eq!(item["group"], "files");
    assert_eq!(item["groupLabel"], "Files");
    assert!(item["scopeLabel"].is_null());
}

#[tokio::test]
async fn completion_list_returns_agent_mentions_for_at_prefix() {
    let (_temp, state) = web_state();
    write_agent_definition(
        &state.inner.cwd.join(".psychevo/agents"),
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
    assert_eq!(item["group"], "agents");
    assert_eq!(item["groupLabel"], "Agents");
    assert_eq!(item["scopeLabel"], "Project");
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
