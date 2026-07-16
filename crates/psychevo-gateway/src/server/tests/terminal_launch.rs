#[tokio::test]
async fn terminal_start_rejects_cwd_outside_workspace() {
    let (temp, state) = web_state();
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&outside).expect("outside dir");
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
            id: Some(json!("1")),
            method: "terminal/start".to_string(),
            params: Some(json!({
                "scope": scope,
                "cwd": outside,
                "cols": 80,
                "rows": 24
            })),
        },
    )
    .await
    .expect_err("outside cwd should be rejected");

    assert!(err.to_string().contains("outside the workspace"), "{err:?}");
}

#[tokio::test]
async fn terminal_rpc_streams_output_and_exit_notifications() {
    let shell = if cfg!(windows) { "cmd.exe" } else { "/bin/sh" };
    let (_temp, state) =
        web_state_with_env(BTreeMap::from([("SHELL".to_string(), shell.to_string())]));
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, mut rx) = mpsc::unbounded_channel();

    let started = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "terminal/start".to_string(),
            params: Some(json!({
                "scope": scope,
                "cwd": null,
                "cols": 80,
                "rows": 24
            })),
        },
    )
    .await
    .expect("terminal/start");
    let terminal_id = started["terminalId"]
        .as_str()
        .expect("terminal id")
        .to_string();

    let resize = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "terminal/resize".to_string(),
            params: Some(json!({
                "terminalId": terminal_id.clone(),
                "cols": 100,
                "rows": 30
            })),
        },
    )
    .await
    .expect("terminal/resize");
    assert_eq!(resize["accepted"], true);

    let command = "printf pevo-terminal-ok\\n\nexit\n";
    let write = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("3")),
            method: "terminal/write".to_string(),
            params: Some(json!({
                "terminalId": terminal_id.clone(),
                "dataBase64": BASE64_STANDARD.encode(command.as_bytes())
            })),
        },
    )
    .await
    .expect("terminal/write");
    assert_eq!(write["accepted"], true);

    let mut output = String::new();
    let mut saw_exit = false;
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(message) = rx.recv().await {
            let notification: Value = serde_json::from_str(&message).expect("notification");
            match notification["method"].as_str() {
                Some("terminal/output") => {
                    let encoded = notification["params"]["dataBase64"]
                        .as_str()
                        .expect("dataBase64");
                    let bytes = BASE64_STANDARD.decode(encoded).expect("base64");
                    output.push_str(&String::from_utf8_lossy(&bytes));
                }
                Some("terminal/exited") => {
                    saw_exit = true;
                }
                _ => {}
            }
            if output.contains("pevo-terminal-ok") && saw_exit {
                break;
            }
        }
    })
    .await
    .expect("terminal notifications");

    assert!(output.contains("pevo-terminal-ok"), "{output:?}");
    assert!(saw_exit);
}

#[tokio::test]
async fn command_list_and_completion_use_web_desktop_presentation_catalog() {
    let (_temp, state) = web_state();
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
            method: "command/list".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": null
            })),
        },
    )
    .await
    .expect("command/list");
    let commands = list["commands"].as_array().expect("commands");
    let names = commands
        .iter()
        .filter_map(|command| command["name"].as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"diff"), "{names:?}");
    assert!(names.contains(&"sessions"), "{names:?}");
    assert!(names.contains(&"undo"), "{names:?}");
    assert!(names.contains(&"redo"), "{names:?}");
    assert!(!names.contains(&"btw"), "{names:?}");
    assert!(!names.contains(&"agents"), "{names:?}");
    assert!(!names.contains(&"model"), "{names:?}");
    assert!(!names.contains(&"tools"), "{names:?}");

    let diff = commands
        .iter()
        .find(|command| command["name"] == "diff")
        .expect("diff command");
    assert_eq!(diff["presentationKind"], "inspect");
    assert_eq!(diff["destination"], "preview");
    assert_eq!(diff["feedbackAnchor"], "trigger");

    let completion = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "completion/list".to_string(),
            params: Some(json!({
                "scope": scope,
                "text": "/",
                "cursor": 1,
                "threadId": null
            })),
        },
    )
    .await
    .expect("completion/list");
    let items = completion["items"].as_array().expect("items");
    assert!(items.iter().any(|item| item["label"] == "/diff"));
    assert!(items.iter().any(|item| item["label"] == "/undo"));
    assert!(items.iter().any(|item| item["label"] == "/redo"));
    assert!(!items.iter().any(|item| item["label"] == "/btw"));
    assert!(!items.iter().any(|item| item["label"] == "/agents"));
    assert!(!items.iter().any(|item| item["label"] == "/model"));
    let diff_completion = items
        .iter()
        .find(|item| item["label"] == "/diff")
        .expect("diff completion");
    assert_eq!(
        diff_completion["detail"].as_str(),
        Some("Preview - show workspace diff")
    );
    assert_eq!(diff_completion["group"], "commands");
    assert_eq!(diff_completion["groupLabel"], "Commands");
    assert!(diff_completion["scopeLabel"].is_null());

    let parent_session = state
        .inner
        .state
        .store()
        .create_session_with_metadata(
            &state.inner.cwd,
            "web",
            "fake-model",
            "fake-provider",
            None,
        )
        .expect("parent session");
    let (tx, _rx) = mpsc::unbounded_channel();
    let list = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("3")),
            method: "command/list".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": parent_session.clone()
            })),
        },
    )
    .await
    .expect("command/list with session");
    let commands = list["commands"].as_array().expect("commands");
    let btw = commands
        .iter()
        .find(|command| command["name"] == "btw")
        .expect("btw command");
    assert_eq!(btw["slash"], "/btw");
    assert_eq!(btw["presentationKind"], "control");

    let completion = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("4")),
            method: "completion/list".to_string(),
            params: Some(json!({
                "scope": scope,
                "text": "/bt",
                "cursor": 3,
                "threadId": parent_session
            })),
        },
    )
    .await
    .expect("completion/list with session");
    assert!(
        completion["items"]
            .as_array()
            .expect("items")
            .iter()
            .any(|item| item["label"] == "/btw"),
        "{completion:#}"
    );
}

#[tokio::test]
async fn command_catalog_completion_and_execute_use_effective_slash_aliases() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"
[tui.slash_aliases]
"/status" = ["/st"]
"/queue base" = ["/qq"]
"/export -i lpr,last-provider-response -f json" = ["/expr"]

[tui.slash_keybinds]
"/status" = "<leader>s"
"#,
    )
    .expect("config");
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
            method: "command/list".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "threadId": null
            })),
        },
    )
    .await
    .expect("command/list");
    let commands = list["commands"].as_array().expect("commands");
    let alias = commands
        .iter()
        .find(|command| command["slash"] == "/st")
        .expect("custom alias command");
    assert_eq!(alias["source"], "custom");
    assert_eq!(alias["summary"], "alias for /status");
    assert_eq!(alias["expandsTo"], "/status");
    assert_eq!(alias["presentationKind"], "inspect");
    assert_eq!(alias["destination"], "status");

    let completion = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "completion/list".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "text": "/st",
                "cursor": 3,
                "threadId": null
            })),
        },
    )
    .await
    .expect("completion/list");
    let items = completion["items"].as_array().expect("items");
    assert_eq!(
        items.first().and_then(|item| item["label"].as_str()),
        Some("/st")
    );
    assert_eq!(
        items.first().and_then(|item| item["insertText"].as_str()),
        Some("/st")
    );

    let result = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("3")),
            method: "command/execute".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "command": "/qq follow up",
                "threadId": null
            })),
        },
    )
    .await
    .expect("command/execute");
    assert_eq!(result["accepted"], true);
    assert_eq!(result["known"], true);
    assert_eq!(result["command"], "/qq follow up");
    assert_eq!(result["action"]["type"], "queuePrompt");
    assert_eq!(result["action"]["text"], "base follow up");
    assert_eq!(result["action"]["displayText"], "/qq follow up");

    let export = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("4")),
            method: "command/execute".to_string(),
            params: Some(json!({
                "scope": scope,
                "command": "/expr",
                "threadId": null
            })),
        },
    )
    .await
    .expect("command/execute export alias");
    assert_eq!(export["accepted"], true);
    assert_eq!(export["known"], true);
    assert_eq!(export["command"], "/expr");
    assert_eq!(export["action"]["type"], "downloadSession");
    assert_eq!(export["action"]["kind"], "export");
    assert_eq!(export["action"]["format"], "json");
    assert_eq!(
        export["action"]["include"],
        json!(["last-provider-request", "last-provider-response"])
    );
}

#[tokio::test]
async fn command_execute_alias_to_unsupported_target_is_not_prompt_passthrough() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(&state.inner.home).expect("home");
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"
[tui.slash_aliases]
"/model" = ["/mm"]
"#,
    )
    .expect("config");
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
            method: "command/execute".to_string(),
            params: Some(json!({
                "scope": scope,
                "command": "/mm",
                "threadId": null
            })),
        },
    )
    .await
    .expect("command/execute");

    assert_eq!(result["accepted"], false);
    assert_eq!(result["known"], true);
    assert!(result["action"].is_null(), "{result:#}");
    assert_eq!(result["presentationKind"], "control");
    assert_eq!(result["feedbackAnchor"], "composer");
    assert!(
        result["message"]
            .as_str()
            .is_some_and(|message| message.contains("Workbench model controls")),
        "{result:#}"
    );
}

#[tokio::test]
async fn command_execute_export_share_invalid_args_return_known_guidance() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    for (id, command) in [("1", "/export --bad"), ("2", "/share -f json")] {
        let result = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(id)),
                method: "command/execute".to_string(),
                params: Some(json!({
                    "scope": scope.clone(),
                    "command": command,
                    "threadId": null
                })),
            },
        )
        .await
        .expect("command/execute invalid export args");

        assert_eq!(result["accepted"], false);
        assert_eq!(result["known"], true);
        assert!(result["action"].is_null(), "{result:#}");
        assert!(
            result["message"]
                .as_str()
                .is_some_and(|message| message.starts_with("usage: /")),
            "{result:#}"
        );
    }
}

#[tokio::test]
async fn command_list_and_execute_include_dynamic_skill_commands() {
    let (_temp, state) = web_state();
    write_project_skill(&state, "x-daily", "Fetch X daily posts.");
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
            method: "command/list".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": null
            })),
        },
    )
    .await
    .expect("command/list");
    let dynamic = list["commands"]
        .as_array()
        .expect("commands")
        .iter()
        .find(|command| command["name"] == "x-daily")
        .expect("dynamic command");
    assert_eq!(dynamic["source"], "dynamic");
    assert_eq!(dynamic["slash"], "/x-daily");
    assert_eq!(dynamic["presentationKind"], "extension");
    assert_eq!(dynamic["destination"], "composer");

    let completion = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "completion/list".to_string(),
            params: Some(json!({
                "scope": scope,
                "text": "/x",
                "cursor": 2,
                "threadId": null
            })),
        },
    )
    .await
    .expect("completion/list");
    let dynamic_completion = completion["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|item| item["label"] == "/x-daily")
        .expect("dynamic completion");
    assert_eq!(
        dynamic_completion["detail"].as_str(),
        Some("Prompt - Fetch X daily posts.")
    );
    assert_eq!(dynamic_completion["group"], "skills");
    assert_eq!(dynamic_completion["groupLabel"], "Skills");
    assert_eq!(dynamic_completion["scopeLabel"], "User");

    let result = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("3")),
            method: "command/execute".to_string(),
            params: Some(json!({
                "scope": scope,
                "command": "/x-daily latest",
                "threadId": null
            })),
        },
    )
    .await
    .expect("command/execute");
    assert_eq!(result["accepted"], true);
    assert_eq!(result["known"], true);
    assert_eq!(result["presentationKind"], "extension");
    assert_eq!(result["feedbackAnchor"], "composer");
    assert_eq!(result["action"]["type"], "submitPrompt");
    assert_eq!(result["action"]["text"], "$x-daily latest");
    assert_eq!(result["action"]["displayText"], "/x-daily latest");
}

#[tokio::test]
async fn command_execute_known_unsupported_returns_guidance_without_passthrough() {
    let (_temp, state) = web_state();
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
            method: "command/execute".to_string(),
            params: Some(json!({
                "scope": scope,
                "command": "/model",
                "threadId": null
            })),
        },
    )
    .await
    .expect("command/execute");

    assert_eq!(result["accepted"], false);
    assert_eq!(result["known"], true);
    assert!(result["action"].is_null(), "{result:#}");
    assert_eq!(result["presentationKind"], "control");
    assert_eq!(result["feedbackAnchor"], "composer");
    assert_eq!(result["alternateAction"]["type"], "openComposerControl");
    assert_eq!(result["alternateAction"]["target"], "model");
    assert!(
        result["message"]
            .as_str()
            .is_some_and(|message| message.contains("Workbench model controls")),
        "{result:#}"
    );
}

#[tokio::test]
async fn command_execute_unknown_slash_returns_prompt_passthrough() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    for command in ["/made-up hello", "/tmp/output.txt"] {
        let result = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "command/execute".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "command": command,
                    "threadId": null
                })),
            },
        )
        .await
        .expect("command/execute");

        assert_eq!(result["accepted"], false);
        assert_eq!(result["known"], false);
        assert_eq!(result["action"]["type"], "passThroughPrompt");
        assert_eq!(result["action"]["text"], command);
        assert!(result["message"].is_null());
        assert!(result["presentationKind"].is_null());
    }
}

#[tokio::test]
async fn shell_start_empty_command_returns_bounded_help_without_spawning() {
    let (_temp, state) = web_state();
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
            method: "shell/start".to_string(),
            params: Some(json!({
                "scope": scope,
                "command": "  ",
                "threadId": null
            })),
        },
    )
    .await
    .expect("shell/start");

    assert_eq!(result["accepted"], false);
    assert_eq!(
        result["message"],
        "shell mode: type !<command> to run a local shell command"
    );
}

#[tokio::test]
async fn turn_start_empty_input_rejects_before_creating_session() {
    let (_temp, state) = web_state();
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
            id: Some(json!("1")),
            method: "turn/start".to_string(),
            params: Some(json!({
                "scope": scope,
                "input": [],
                "threadId": null
            })),
        },
    )
    .await
    .expect_err("empty turn should reject");

    assert_eq!(err.to_string(), "turn/start requires input");
    assert_eq!(
        state
            .inner
            .state
            .store()
            .list_sessions_for_cwd_with_sources(&state.inner.cwd, &[])
            .expect("sessions")
            .len(),
        0
    );
    assert!(
        state
            .inner
            .gateway
            .resolve_source_thread(&state.inner.source)
            .expect("source lookup")
            .is_none()
    );
}

#[tokio::test]
async fn shell_start_first_request_can_be_accepted_without_thread_id() {
    let (_temp, state) = web_state();
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
            method: "shell/start".to_string(),
            params: Some(json!({
                "scope": scope,
                "command": "printf shell-ok",
                "threadId": null
            })),
        },
    )
    .await
    .expect("shell/start");

    assert_eq!(result["accepted"], true);
    assert!(result.get("threadId").is_some_and(Value::is_null));
}

#[tokio::test]
async fn agent_write_rpc_creates_project_backend_ref_shadow() {
    let (_temp, state) = web_state();
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "agent/write".to_string(),
            params: Some(json!({
                "name": "cursor-reviewer",
                "description": "Review with Cursor",
                "backend": {"ref": "cursor"},
                "entrypoints": ["subagent"],
                "instructions": "Return concise findings."
            })),
        },
    )
    .await
    .expect("agent/write");
    let path = result["path"].as_str().expect("path");
    let text = std::fs::read_to_string(path).expect("agent file");
    assert!(text.contains("cursor-reviewer"));
    assert!(text.contains("ref: cursor"));
    assert!(text.contains("subagent"));
}

#[tokio::test]
async fn static_shell_without_browser_session_returns_launch_required_page() {
    let (_temp, state) = web_state_with_static();

    let response = static_asset(
        State(state),
        HeaderMap::new(),
        axum::http::Uri::from_static("/"),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.headers().get("cache-control").and_then(|value| value.to_str().ok()),
        Some("no-store")
    );
    let body = response_text(response).await;
    assert!(body.contains("pevo launch required"), "{body}");
    assert!(body.contains("pevo web --print-url"), "{body}");
    assert!(!body.contains("<title>workbench</title>"), "{body}");
}

#[tokio::test]
async fn static_shell_with_browser_session_serves_workbench_index() {
    let (_temp, state) = web_state_with_static();
    let session_id = "session-test".to_string();
    state
        .inner
        .browser_sessions
        .lock()
        .expect("sessions")
        .insert(
            session_id.clone(),
            BrowserSession {
                cwd: state.inner.cwd.clone(),
                source: state.inner.source.clone(),
            },
        );
    let mut headers = HeaderMap::new();
    headers.insert(
        COOKIE,
        HeaderValue::from_str(&format!("psychevo_gateway_session={session_id}")).expect("cookie"),
    );

    let response = static_asset(State(state.clone()), headers.clone(), axum::http::Uri::from_static("/"))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("cache-control").and_then(|value| value.to_str().ok()),
        Some("no-store")
    );
    let body = response_text(response).await;
    assert!(body.contains("<title>workbench</title>"), "{body}");

    let fallback = static_asset(
        State(state),
        headers,
        axum::http::Uri::from_static("/sessions/thread-1"),
    )
    .await
    .into_response();
    assert_eq!(fallback.status(), StatusCode::OK);
    assert_eq!(
        fallback.headers().get("cache-control").and_then(|value| value.to_str().ok()),
        Some("no-store")
    );
    assert!(response_text(fallback).await.contains("<title>workbench</title>"));
}

#[tokio::test]
async fn fingerprinted_static_asset_is_immutable_cacheable() {
    let (temp, state) = web_state_with_static();
    let assets = temp.path().join("static/assets");
    std::fs::create_dir_all(&assets).expect("assets dir");
    std::fs::write(assets.join("index-C0FFEE42.js"), "export const ready = true;")
        .expect("asset");

    let response = static_asset(
        State(state),
        HeaderMap::new(),
        axum::http::Uri::from_static("/assets/index-C0FFEE42.js"),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("cache-control").and_then(|value| value.to_str().ok()),
        Some("public, max-age=31536000, immutable")
    );
    assert_eq!(
        response.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()),
        Some("text/javascript; charset=utf-8")
    );
}

#[tokio::test]
async fn download_session_honors_export_query_options() {
    let (_temp, state) = web_state_with_static();
    let session_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(
            &state.inner.cwd,
            "web",
            "fake-model",
            "fake-provider",
            None,
        )
        .expect("session");
    state
        .inner
        .state
        .store()
        .append_message(&session_id, &runtime_user_message("hello export", 1))
        .expect("message");
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", state.inner.token)).expect("auth"),
    );

    let response = download_session(
        State(state),
        headers,
        AxumPath((session_id, "export".to_string())),
        Query(DownloadQuery {
            format: Some("json".to_string()),
            include: Some("messages".to_string()),
            filename: Some("exports/review.md".to_string()),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/json; charset=utf-8")
    );
    assert_eq!(
        response
            .headers()
            .get(CONTENT_DISPOSITION)
            .and_then(|value| value.to_str().ok()),
        Some("attachment; filename=\"review.json\"")
    );
    let body = response_text(response).await;
    assert!(body.contains("hello export"), "{body}");
}

#[tokio::test]
async fn download_session_defaults_to_markdown_artifact() {
    let (_temp, state) = web_state_with_static();
    let session_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(
            &state.inner.cwd,
            "web",
            "fake-model",
            "fake-provider",
            None,
        )
        .expect("session");
    state
        .inner
        .state
        .store()
        .append_message(&session_id, &runtime_user_message("hello markdown", 1))
        .expect("message");
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", state.inner.token)).expect("auth"),
    );

    let response = download_session(
        State(state),
        headers,
        AxumPath((session_id.clone(), "export".to_string())),
        Query(DownloadQuery::default()),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/markdown; charset=utf-8")
    );
    assert_eq!(
        response
            .headers()
            .get(CONTENT_DISPOSITION)
            .and_then(|value| value.to_str().ok()),
        Some(format!("attachment; filename=\"export-{session_id}.md\"").as_str())
    );
    let body = response_text(response).await;
    assert!(body.contains("hello markdown"), "{body}");
}

#[tokio::test]
async fn media_artifact_endpoint_requires_auth_and_serves_image_bytes() {
    use base64::Engine as _;

    let (_temp, state) = web_state_with_static();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(psychevo_ai::DEFAULT_FAKE_IMAGE_BASE64)
        .expect("png fixture");
    let artifact = psychevo_runtime::write_generated_image_artifact(
        &state.inner.home,
        &bytes,
        "image/png",
    )
    .expect("artifact");

    let unauthorized = read_media_artifact(
        State(state.clone()),
        HeaderMap::new(),
        AxumPath(artifact.artifact_id.clone()),
    )
    .await
    .into_response();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", state.inner.token)).expect("auth"),
    );
    let response = read_media_artifact(State(state), headers, AxumPath(artifact.artifact_id))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("image/png")
    );
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    assert_eq!(body.as_ref(), bytes.as_slice());
}

#[tokio::test]
async fn consumed_launch_without_browser_session_returns_recovery_page() {
    let (_temp, state) = web_state_with_static();

    let response = consume_launch(
        State(state),
        AxumPath("missing-launch".to_string()),
        Query(LaunchQuery {
            open_token: "used-token".to_string(),
        }),
        HeaderMap::new(),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response_text(response).await;
    assert!(body.contains("pevo launch link expired"), "{body}");
    assert!(body.contains("pevo web --print-url"), "{body}");
}

#[tokio::test]
async fn consumed_launch_with_browser_session_redirects_to_clean_shell() {
    let (_temp, state) = web_state_with_static();
    let session_id = "session-test".to_string();
    state
        .inner
        .browser_sessions
        .lock()
        .expect("sessions")
        .insert(
            session_id.clone(),
            BrowserSession {
                cwd: state.inner.cwd.clone(),
                source: state.inner.source.clone(),
            },
        );
    let mut headers = HeaderMap::new();
    headers.insert(
        COOKIE,
        HeaderValue::from_str(&format!("psychevo_gateway_session={session_id}")).expect("cookie"),
    );

    let response = consume_launch(
        State(state),
        AxumPath("missing-launch".to_string()),
        Query(LaunchQuery {
            open_token: "used-token".to_string(),
        }),
        headers,
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response
            .headers()
            .get(LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some("/")
    );
}
