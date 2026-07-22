#[test]
fn gateway_turns_expose_model_facing_automation_tool() {
    let (_temp, state) = web_state();

    let options = state.run_options(state.inner.cwd.clone(), Some("thread-1".to_string()));

    assert!(
        options
            .runtime_tools
            .iter()
            .any(|tool| tool.name() == "automation")
    );
}

#[test]
fn automation_declaration_is_concise_and_product_facing() {
    let (_temp, state) = web_state();
    let (description, parameters) = automations::automation_tool_declaration_for_test(
        state.clone(),
        state.inner.cwd.clone(),
        Some("thread-1".to_string()),
    );

    assert_eq!(
        description,
        "Create and manage recurring or one-shot automations."
    );
    let mut missing = Vec::new();
    collect_missing_automation_schema_descriptions(
        &parameters,
        "automation".to_string(),
        &mut missing,
    );
    assert!(
        missing.is_empty(),
        "automation has schema properties without descriptions: {missing:?}"
    );

    let declaration_text = format!(
        "{description}\n{}",
        serde_json::to_string(&parameters).expect("automation parameters serialize")
    )
    .to_ascii_lowercase();
    for implementation_term in [
        "gateway-supplied",
        "harness",
        "model-visible",
        "adapter",
        "persistence",
        "permission_mode",
        "sandbox_override",
    ] {
        assert!(
            !declaration_text.contains(implementation_term),
            "automation exposes implementation term {implementation_term:?}: {declaration_text}"
        );
    }
    assert!(
        parameters["properties"]["target"]["description"]
            .as_str()
            .is_some_and(|value| value.contains("current conversation"))
    );
    assert!(
        parameters["properties"]["execution"]["properties"]["policy"]["description"]
            .as_str()
            .is_some_and(|value| value.contains("requests confirmation"))
    );
}

fn collect_missing_automation_schema_descriptions(
    value: &Value,
    path: String,
    missing: &mut Vec<String>,
) {
    if let Some(properties) = value.get("properties").and_then(Value::as_object) {
        for (name, property) in properties {
            let property_path = format!("{path}.{name}");
            if !property
                .get("description")
                .and_then(Value::as_str)
                .is_some_and(|description| !description.trim().is_empty())
            {
                missing.push(property_path.clone());
            }
            collect_missing_automation_schema_descriptions(property, property_path, missing);
        }
    }
    if let Some(items) = value.get("items") {
        collect_missing_automation_schema_descriptions(items, format!("{path}[]"), missing);
    }
    for keyword in ["oneOf", "anyOf", "allOf"] {
        if let Some(alternatives) = value.get(keyword).and_then(Value::as_array) {
            for (index, alternative) in alternatives.iter().enumerate() {
                collect_missing_automation_schema_descriptions(
                    alternative,
                    format!("{path}.{keyword}[{index}]"),
                    missing,
                );
            }
        }
    }
}

#[test]
fn automation_tool_create_defaults_to_current_thread() {
    let (_temp, state) = web_state();
    let thread_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&state.inner.cwd, "web", "model", "provider", None)
        .expect("session");
    let value = automations::automation_tool_execute_for_test(
        state.clone(),
        state.inner.cwd.clone(),
        Some(thread_id.clone()),
        json!({
            "action": "create",
            "title": "Thread check",
            "prompt": "Continue this thread later.",
            "schedule": { "kind": "interval", "everyMinutes": 30 }
        }),
    )
    .expect("automation tool");

    assert_eq!(value["success"], true);
    assert_eq!(value["action"], "create");
    assert_eq!(value["automation"]["kind"], "threadHeartbeat");
    assert_eq!(value["automation"]["targetThreadId"], thread_id);
}

#[tokio::test]
async fn turn_start_first_prompt_materializes_current_thread_for_automation_tool() {
    let backend = Arc::new(AutomationFakeBackend::default());
    *backend.model_tool_args.lock().expect("tool args") = Some(json!({
        "action": "create",
        "target": "currentThread",
        "title": "First turn thread tip",
        "prompt": "Send one useful software engineering tip.",
        "schedule": { "kind": "interval", "everyMinutes": 1 }
    }));
    let (_temp, state) = web_state_with_automation_backend(backend.clone());
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    assert!(
        state
            .inner
            .gateway
            .resolve_source_thread(&state.inner.source)
            .expect("source")
            .is_none()
    );
    let (tx, mut rx) = mpsc::unbounded_channel();
    let context = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("turn-context")),
            method: "thread/context/read".to_string(),
            params: Some(json!({
                "scope": scope,
                "target": {"agentRef": null, "runtimeProfileRef": "native"}
            })),
        },
    )
    .await
    .expect("prospective Thread Context");

    let accepted = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "turn/start".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": null,
                "target": {"agentRef": null, "runtimeProfileRef": "native"},
                "input": [{"type": "text", "text": "每分钟发一条你认为最有价值的软件工程 tip"}],
                "turnOverrides": {"model": "fake-model"},
                "expectedContextRevision": context["contextRevision"],
                "expectedControlRevision": context["controlRevision"]
            })),
        },
    )
    .await
    .expect("turn/start");
    assert_eq!(accepted["accepted"], true);
    let accepted_thread_id = accepted["threadId"]
        .as_str()
        .expect("accepted turn thread id")
        .to_string();

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if !backend
                .model_tool_results
                .lock()
                .expect("tool results")
                .is_empty()
                || !backend
                    .model_tool_errors
                    .lock()
                    .expect("tool errors")
                    .is_empty()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("model-facing automation tool call");

    let errors = backend
        .model_tool_errors
        .lock()
        .expect("tool errors")
        .clone();
    assert_eq!(errors, Vec::<String>::new());
    let tool_results = backend
        .model_tool_results
        .lock()
        .expect("tool results")
        .clone();
    assert_eq!(tool_results.len(), 1);
    assert_eq!(tool_results[0]["automation"]["kind"], "threadHeartbeat");
    let target_thread_id = tool_results[0]["automation"]["targetThreadId"]
        .as_str()
        .expect("target thread")
        .to_string();
    assert_eq!(accepted_thread_id, target_thread_id);

    let runs = backend.runs.lock().expect("runs").clone();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].session.as_deref(), Some(target_thread_id.as_str()));
    assert!(
        runs[0]
            .runtime_tools
            .iter()
            .any(|name| name == "automation")
    );
    assert_eq!(
        state
            .inner
            .gateway
            .resolve_source_thread(&state.inner.source)
            .expect("source binding")
            .as_deref(),
        Some(target_thread_id.as_str())
    );

    let mut saw_terminal = false;
    while let Ok(message) = rx.try_recv() {
        saw_terminal |= message.contains("\"type\":\"turnCompleted\"");
        assert!(!message.contains("current thread is not available"));
    }
    assert!(saw_terminal);
}

#[tokio::test]
async fn draft_open_remains_empty_without_creating_session() {
    let backend = Arc::new(AutomationFakeBackend::default());
    let (_temp, state) = web_state_with_automation_backend(backend);
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let opened = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: "thread/draft/open".to_string(),
            params: Some(json!({ "origin": scope, "targetIntent": { "kind": "default" } })),
        },
    )
    .await
    .expect("thread/draft/open");

    let snapshot = &opened["snapshot"];
    assert!(snapshot.get("thread").is_some_and(Value::is_null));
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
async fn automation_tool_manages_project_lifecycle_actions() {
    let backend = Arc::new(AutomationFakeBackend::default());
    let (_temp, state) = web_state_with_automation_backend(backend.clone());
    let cwd = state.inner.cwd.clone();

    let created = automations::automation_tool_execute_for_test(
        state.clone(),
        cwd.clone(),
        None,
        json!({
            "action": "create",
            "target": "project",
            "title": "Repo lifecycle",
            "prompt": "Check the repository.",
            "schedule": { "kind": "interval", "everyMinutes": 30 }
        }),
    )
    .expect("create");
    let automation_id = created["automation"]["id"]
        .as_str()
        .expect("automation id")
        .to_string();
    assert_eq!(created["action"], "create");
    assert_eq!(created["automation"]["kind"], "project");

    let listed = automations::automation_tool_execute_for_test(
        state.clone(),
        cwd.clone(),
        None,
        json!({ "action": "list" }),
    )
    .expect("list");
    assert_eq!(listed["action"], "list");
    assert_eq!(listed["automations"][0]["id"], automation_id);

    let updated = automations::automation_tool_execute_for_test(
        state.clone(),
        cwd.clone(),
        None,
        json!({
            "action": "update",
            "automationId": automation_id,
            "title": "Updated lifecycle"
        }),
    )
    .expect("update");
    assert_eq!(updated["action"], "update");
    assert_eq!(updated["automation"]["title"], "Updated lifecycle");

    let paused = automations::automation_tool_execute_for_test(
        state.clone(),
        cwd.clone(),
        None,
        json!({ "action": "pause", "automationId": automation_id }),
    )
    .expect("pause");
    assert_eq!(paused["action"], "pause");
    assert_eq!(paused["automation"]["enabled"], false);
    assert!(paused["automation"]["nextRunAtMs"].is_null());

    let resumed = automations::automation_tool_execute_for_test(
        state.clone(),
        cwd.clone(),
        None,
        json!({ "action": "resume", "automationId": automation_id }),
    )
    .expect("resume");
    assert_eq!(resumed["action"], "resume");
    assert_eq!(resumed["automation"]["enabled"], true);
    assert!(resumed["automation"]["nextRunAtMs"].is_number());

    let run = automations::automation_tool_execute_for_test(
        state.clone(),
        cwd.clone(),
        None,
        json!({ "action": "run", "automationId": automation_id }),
    )
    .expect("run");
    assert_eq!(run["action"], "run");
    assert_eq!(run["accepted"], true);

    tokio::time::timeout(Duration::from_secs(2), backend.notify.notified())
        .await
        .expect("fake backend run");
    let backend_runs = backend.runs.lock().expect("runs").clone();
    assert_eq!(backend_runs.len(), 1);
    assert!(backend_runs[0].runtime_tools.is_empty());

    let removed = automations::automation_tool_execute_for_test(
        state.clone(),
        cwd.clone(),
        None,
        json!({ "action": "remove", "automationId": automation_id }),
    )
    .expect("remove");
    assert_eq!(removed["action"], "remove");
    assert_eq!(removed["deleted"], true);

    let listed = automations::automation_tool_execute_for_test(
        state,
        cwd,
        None,
        json!({ "action": "list" }),
    )
    .expect("list empty");
    assert!(
        listed["automations"]
            .as_array()
            .expect("automations")
            .is_empty()
    );
}
