#[tokio::test]
pub(crate) async fn background_completion_records_mailbox_event_without_parent_user_message() {
    let tmp = TempDir::new().expect("tmp");
    let store = SqliteStore::open(&tmp.path().join("state.sqlite")).expect("store");
    let parent = store
        .create_session_with_metadata(tmp.path(), "run", "model", "provider", None)
        .expect("parent");
    let child = store
        .create_child_session_with_metadata(&parent, tmp.path(), "agent", "model", "provider", None)
        .expect("child");
    let record = test_agent_run_record(parent.clone(), Some(child));

    append_parent_agent_mailbox_event(&store, &parent, &record, "normal", "mailbox final")
        .expect("mailbox event");

    assert!(store.load_messages(&parent).expect("messages").is_empty());
    let events = store.load_agent_mailbox_events(&parent).expect("events");
    assert_eq!(events.len(), 1);
    assert!(events[0].content_text.contains("mailbox final"));
    assert!(!events[0].content_text.contains("agent_id"));
    assert!(!events[0].content_text.contains("child_session_id"));
    assert!(
        events[0].payload["content"]
            .as_str()
            .expect("content")
            .contains("<subagent_notification>")
    );
    let metadata = events[0].metadata.as_ref().expect("metadata");
    assert_eq!(metadata["agent_id"], "agent-1");
    assert_eq!(
        metadata["child_session_id"].as_str(),
        events[0].child_session_id.as_deref()
    );
    assert_eq!(
        metadata["model_visible_summary"]["summary"],
        "mailbox final"
    );
}

#[tokio::test]
pub(crate) async fn wait_agent_mailbox_returns_status_without_final_answer() {
    let tmp = TempDir::new().expect("tmp");
    let store = SqliteStore::open(&tmp.path().join("state.sqlite")).expect("store");
    let parent = store
        .create_session_with_metadata(tmp.path(), "run", "model", "provider", None)
        .expect("parent");
    let record = test_agent_run_record(parent.clone(), None);
    append_parent_agent_mailbox_event(&store, &parent, &record, "normal", "mailbox final")
        .expect("mailbox event");

    let value = wait_agent_mailbox(&parent, Duration::from_millis(0), &store)
        .await
        .expect("wait");
    assert_eq!(value["timed_out"], false);
    assert_eq!(value["message"], "Wait completed.");
    assert!(value.get("final_answer").is_none());
    assert!(value.get("statuses").is_none());

    let empty_parent = store
        .create_session_with_metadata(tmp.path(), "run", "model", "provider", None)
        .expect("empty parent");
    let value = wait_agent_mailbox(&empty_parent, Duration::from_millis(0), &store)
        .await
        .expect("timeout");
    assert_eq!(value["timed_out"], true);
}

#[test]
pub(crate) fn subagent_summary_uses_prompt_task_and_direct_child_tokens() {
    let tmp = TempDir::new().expect("tmp");
    let store = SqliteStore::open(&tmp.path().join("state.sqlite")).expect("store");
    let parent = store
        .create_session_with_metadata(tmp.path(), "run", "parent-model", "provider", None)
        .expect("parent");
    let child = store
        .create_child_session_with_metadata(
            &parent,
            tmp.path(),
            "agent",
            "child-model",
            "provider",
            None,
        )
        .expect("child");
    store
        .append_message_with_metrics(
            &child,
            &Message::Assistant {
                content: vec![AssistantBlock::ToolCall(ToolCallBlock {
                    id: "tool-1".to_string(),
                    name: "read".to_string(),
                    arguments: json!({}),
                    arguments_json: "{}".to_string(),
                    arguments_error: None,
                    content_index: 0,
                    call_index: 0,
                })],
                timestamp_ms: now_ms(),
                finish_reason: Some("tool_calls".to_string()),
                outcome: Outcome::Normal,
                model: Some("child-model".to_string()),
                provider: Some("provider".to_string()),
            },
            Some(json!({
                "input_tokens": 10,
                "output_tokens": 5,
                "reasoning_tokens": 2
            })),
            None,
        )
        .expect("assistant tool call");
    store
        .append_message_with_metrics(
            &child,
            &Message::Assistant {
                content: vec![AssistantBlock::Text {
                    text: "done".to_string(),
                }],
                timestamp_ms: now_ms(),
                finish_reason: Some("stop".to_string()),
                outcome: Outcome::Normal,
                model: Some("child-model".to_string()),
                provider: Some("provider".to_string()),
            },
            Some(json!({
                "prompt_tokens": 3,
                "completion_tokens": 7,
                "total_tokens": 15
            })),
            None,
        )
        .expect("assistant final");

    let id = "agent-summary-1".to_string();
    let mut record = test_agent_run_record(parent, Some(child));
    record.id = id.clone();
    record.task_name = Some(default_task_name("worker", &id));
    record.task = "  First line   with spacing  \nsecond line".to_string();
    let value = subagent_summary_value(Some(&store), &record, false);

    assert!(value.get("agent_id").is_none());
    assert_eq!(value["agent_name"], "worker");
    assert_eq!(value["task_name"], default_task_name("worker", &id));
    assert_eq!(value["status"], "completed");
    assert_eq!(value["exit_reason"], "normal");
    assert_eq!(value["summary"], "mailbox final");
    assert_eq!(value["duration_ms"], 1);
    assert_eq!(value["tool_call_count"], 1);
    assert_eq!(value["model"], "child-model");
    assert_eq!(value["tokens"]["input"], 13);
    assert_eq!(value["tokens"]["output"], 12);
    assert_eq!(value["tokens"]["reasoning"], 2);
    assert_eq!(value["tokens"]["total"], 32);
    assert!(!value.to_string().contains("null"));
}

#[tokio::test]
pub(crate) async fn foreground_agent_tool_result_uses_compact_model_summary() {
    let tmp = TempDir::new().expect("tmp");
    let db_path = tmp.path().join("state.sqlite");
    let store = SqliteStore::open(&db_path).expect("store");
    let parent = store
        .create_session_with_metadata(tmp.path(), "run", "model", "provider", None)
        .expect("parent");
    let catalog = AgentCatalog {
        agents: vec![built_in_agent("worker", "Worker", "Work.", None)],
        shadowed_agents: Vec::new(),
        diagnostics: Vec::new(),
    };
    let (_tx, rx) = watch::channel(false);
    let output = spawn_subagent(
        test_agent_tool_context(
            &tmp,
            Arc::new(FakeProvider::new(vec![vec![
                RawStreamEvent::Text("child final".to_string()),
                RawStreamEvent::Done(Outcome::Normal),
            ]])),
            store,
            db_path,
            parent,
            catalog,
        ),
        SpawnAgentArgs {
            agent_type: Some("worker".to_string()),
                        message: "Summarize this task.\nDo not echo metadata.".to_string(),
            task_name: "test_task".to_string(),
            background: Some(false),
            model: None,
            fork_context: false,
            fork_turns: None,
            max_turns: Some(1),
            max_spawn_depth: None,
        },
        "call".to_string(),
        AbortSignal::new(rx),
    )
    .await
    .expect("spawn");

    assert!(output.json.get("child_session_id").is_some());
    assert!(output.json.get("effective_max_spawn_depth").is_some());
    let model_value: Value =
        serde_json::from_str(output.model_content.as_deref().expect("model content"))
            .expect("model json");
    assert_eq!(model_value["agent_name"], "worker");
    assert_eq!(model_value["task_name"], "test_task");
    assert_eq!(model_value["status"], "completed");
    assert_eq!(model_value["summary"], "child final");
    assert!(model_value.get("agent_id").is_none());
    assert!(model_value.get("child_session_id").is_none());
    assert!(model_value.get("effective_max_spawn_depth").is_none());
}

#[tokio::test]
pub(crate) async fn background_agent_tool_result_includes_child_session_identity() {
    let tmp = TempDir::new().expect("tmp");
    let db_path = tmp.path().join("state.sqlite");
    let store = SqliteStore::open(&db_path).expect("store");
    let parent = store
        .create_session_with_metadata(tmp.path(), "run", "model", "provider", None)
        .expect("parent");
    let catalog = AgentCatalog {
        agents: vec![built_in_agent("worker", "Worker", "Work.", None)],
        shadowed_agents: Vec::new(),
        diagnostics: Vec::new(),
    };
    let (_tx, rx) = watch::channel(false);
    let output = spawn_subagent(
        test_agent_tool_context(
            &tmp,
            Arc::new(FakeProvider::new(vec![vec![
                RawStreamEvent::Text("child final".to_string()),
                RawStreamEvent::Done(Outcome::Normal),
            ]])),
            store.clone(),
            db_path,
            parent.clone(),
            catalog,
        ),
        SpawnAgentArgs {
            agent_type: Some("worker".to_string()),
                        message: "Summarize this task.".to_string(),
            task_name: "explicit_task".to_string(),
            background: Some(true),
            model: None,
            fork_context: false,
            fork_turns: None,
            max_turns: Some(1),
            max_spawn_depth: None,
        },
        "call".to_string(),
        AbortSignal::new(rx),
    )
    .await
    .expect("spawn");

    assert_eq!(output.json["status"], "running");
    assert_eq!(output.json["background"], true);
    assert_eq!(output.json["task_name"], "explicit_task");
    let child_session = output.json["child_session_id"]
        .as_str()
        .expect("child session id");
    assert_eq!(output.json["session_id"].as_str(), Some(child_session));
    let edge = store
        .find_agent_edge(child_session)
        .expect("edge")
        .expect("edge");
    assert_eq!(edge.parent_session_id, parent);
    assert_eq!(edge.child_session_id, child_session);
    let metadata = edge.metadata.as_ref().expect("edge metadata");
    assert_eq!(metadata["agent"]["id"], output.json["id"]);
    assert_eq!(metadata["agent"]["task_name"], "explicit_task");

    let model_value: Value =
        serde_json::from_str(output.model_content.as_deref().expect("model content"))
            .expect("model json");
    assert_eq!(model_value["agent_name"], "worker");
    assert_eq!(model_value["task_name"], "explicit_task");
    assert_eq!(model_value["status"], "running");
    assert!(model_value.get("child_session_id").is_none());
    assert!(model_value.get("session_id").is_none());
}

#[tokio::test]
pub(crate) async fn foreground_child_agent_closes_edge_after_completion() {
    let tmp = TempDir::new().expect("tmp");
    let db_path = tmp.path().join("state.sqlite");
    let store = SqliteStore::open(&db_path).expect("store");
    let parent = store
        .create_session_with_metadata(tmp.path(), "run", "model", "provider", None)
        .expect("parent");
    let catalog = AgentCatalog {
        agents: vec![built_in_agent("worker", "Worker", "Work.", None)],
        shadowed_agents: Vec::new(),
        diagnostics: Vec::new(),
    };
    let (_tx, rx) = watch::channel(false);
    let output = spawn_subagent(
        test_agent_tool_context(
            &tmp,
            Arc::new(FakeProvider::new(vec![vec![
                RawStreamEvent::Text("child final".to_string()),
                RawStreamEvent::Done(Outcome::Normal),
            ]])),
            store.clone(),
            db_path,
            parent,
            catalog,
        ),
        SpawnAgentArgs {
            agent_type: Some("worker".to_string()),
                        message: "Summarize this task.".to_string(),
            task_name: "test_task".to_string(),
            background: Some(false),
            model: None,
            fork_context: false,
            fork_turns: None,
            max_turns: Some(1),
            max_spawn_depth: None,
        },
        "call".to_string(),
        AbortSignal::new(rx),
    )
    .await
    .expect("spawn");

    let child_session = output.json["child_session_id"]
        .as_str()
        .expect("child session");
    let edge = store
        .find_agent_edge(child_session)
        .expect("edge")
        .expect("edge");
    assert_eq!(edge.status, AgentEdgeStatus::Closed);
}

#[tokio::test]
pub(crate) async fn parent_abort_interrupts_foreground_child_agent() {
    let tmp = TempDir::new().expect("tmp");
    let db_path = tmp.path().join("state.sqlite");
    let store = SqliteStore::open(&db_path).expect("store");
    let parent = store
        .create_session_with_metadata(tmp.path(), "run", "model", "provider", None)
        .expect("parent");
    let catalog = AgentCatalog {
        agents: vec![built_in_agent("worker", "Worker", "Work.", None)],
        shadowed_agents: Vec::new(),
        diagnostics: Vec::new(),
    };
    let provider = Arc::new(AbortAwareProvider::default());
    let started = Arc::clone(&provider.started);
    let (abort_tx, rx) = watch::channel(false);
    let task = tokio::spawn(spawn_subagent(
        test_agent_tool_context(&tmp, provider, store.clone(), db_path, parent, catalog),
        SpawnAgentArgs {
            agent_type: Some("worker".to_string()),
                        message: "Wait until interrupted.".to_string(),
            task_name: "test_task".to_string(),
            background: Some(false),
            model: None,
            fork_context: false,
            fork_turns: None,
            max_turns: Some(1),
            max_spawn_depth: None,
        },
        "call".to_string(),
        AbortSignal::new(rx),
    ));

    started.notified().await;
    abort_tx.send(true).expect("abort");
    let output = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("child should settle after parent abort")
        .expect("join")
        .expect("spawn");

    assert_eq!(output.json["status"], "interrupted");
    assert_eq!(output.json["outcome"], "aborted");
    let child_session = output.json["child_session_id"]
        .as_str()
        .expect("child session");
    let edge = store
        .find_agent_edge(child_session)
        .expect("edge")
        .expect("edge");
    assert_eq!(edge.status, AgentEdgeStatus::Closed);
}

#[tokio::test]
pub(crate) async fn backend_backed_agent_tool_uses_external_delegate() {
    let tmp = TempDir::new().expect("tmp");
    let db_path = tmp.path().join("state.sqlite");
    let store = SqliteStore::open(&db_path).expect("store");
    let parent = store
        .create_session_with_metadata(tmp.path(), "run", "model", "provider", None)
        .expect("parent");
    let catalog = AgentCatalog {
        agents: vec![backend_backed_agent("opencode", "opencode")],
        shadowed_agents: Vec::new(),
        diagnostics: Vec::new(),
    };
    let delegate = Arc::new(FakeExternalAgentDelegate::default());
    let mut context = test_agent_tool_context(
        &tmp,
        Arc::new(FakeProvider::new(Vec::new())),
        store.clone(),
        db_path,
        parent.clone(),
        catalog,
    );
    context.external_delegate =
        Some(delegate.clone() as Arc<dyn crate::types::ExternalAgentDelegate>);
    let (_tx, rx) = watch::channel(false);
    let output = spawn_subagent(
        context,
        SpawnAgentArgs {
            agent_type: Some("opencode".to_string()),
                        message: "List your tools.".to_string(),
            task_name: "test_task".to_string(),
            background: Some(false),
            model: None,
            fork_context: false,
            fork_turns: None,
            max_turns: None,
            max_spawn_depth: None,
        },
        "call".to_string(),
        AbortSignal::new(rx),
    )
    .await
    .expect("delegate spawn");

    assert!(!output.is_error);
    assert_eq!(output.json["agent_name"], "opencode");
    assert_eq!(output.json["final_answer"], "delegated final");
    let child_session = output.json["child_session_id"]
        .as_str()
        .expect("child session");
    let summary = store
        .session_summary(child_session)
        .expect("summary")
        .expect("child summary");
    assert_eq!(summary.provider, "acp:opencode");
    let edge = store
        .find_agent_edge(child_session)
        .expect("edge")
        .expect("edge");
    assert_eq!(edge.parent_session_id, parent);
    assert_eq!(edge.status, AgentEdgeStatus::Closed);
    let calls = delegate.calls.lock().expect("delegate calls");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].backend_ref, "opencode");
    assert_eq!(calls[0].prompt, "List your tools.");
    assert_eq!(calls[0].child_session_id, child_session);
}

#[tokio::test]
pub(crate) async fn parent_abort_reaches_backend_backed_agent_delegate() {
    let tmp = TempDir::new().expect("tmp");
    let db_path = tmp.path().join("state.sqlite");
    let store = SqliteStore::open(&db_path).expect("store");
    let parent = store
        .create_session_with_metadata(tmp.path(), "run", "model", "provider", None)
        .expect("parent");
    let catalog = AgentCatalog {
        agents: vec![backend_backed_agent("opencode", "opencode")],
        shadowed_agents: Vec::new(),
        diagnostics: Vec::new(),
    };
    let delegate = Arc::new(AbortAwareExternalAgentDelegate::default());
    let started = Arc::clone(&delegate.started);
    let mut context = test_agent_tool_context(
        &tmp,
        Arc::new(FakeProvider::new(Vec::new())),
        store.clone(),
        db_path,
        parent,
        catalog,
    );
    context.external_delegate =
        Some(delegate.clone() as Arc<dyn crate::types::ExternalAgentDelegate>);
    let (abort_tx, rx) = watch::channel(false);
    let task = tokio::spawn(spawn_subagent(
        context,
        SpawnAgentArgs {
            agent_type: Some("opencode".to_string()),
                        message: "Wait until interrupted.".to_string(),
            task_name: "test_task".to_string(),
            background: Some(false),
            model: None,
            fork_context: false,
            fork_turns: None,
            max_turns: None,
            max_spawn_depth: None,
        },
        "call".to_string(),
        AbortSignal::new(rx),
    ));

    started.notified().await;
    abort_tx.send(true).expect("abort");
    let output = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("delegate should settle after parent abort")
        .expect("join")
        .expect("spawn");

    assert_eq!(output.json["status"], "interrupted");
    assert_eq!(output.json["outcome"], "aborted");
    let child_session = output.json["child_session_id"]
        .as_str()
        .expect("child session");
    let edge = store
        .find_agent_edge(child_session)
        .expect("edge")
        .expect("edge");
    assert_eq!(edge.status, AgentEdgeStatus::Closed);
}

#[tokio::test]
pub(crate) async fn backend_backed_agent_tool_without_delegate_returns_unavailable_error() {
    let tmp = TempDir::new().expect("tmp");
    let db_path = tmp.path().join("state.sqlite");
    let store = SqliteStore::open(&db_path).expect("store");
    let parent = store
        .create_session_with_metadata(tmp.path(), "run", "model", "provider", None)
        .expect("parent");
    let catalog = AgentCatalog {
        agents: vec![backend_backed_agent("opencode", "opencode")],
        shadowed_agents: Vec::new(),
        diagnostics: Vec::new(),
    };
    let (_tx, rx) = watch::channel(false);
    let output = spawn_subagent(
        test_agent_tool_context(
            &tmp,
            Arc::new(FakeProvider::new(Vec::new())),
            store.clone(),
            db_path,
            parent,
            catalog,
        ),
        SpawnAgentArgs {
            agent_type: Some("opencode".to_string()),
                        message: "List your tools.".to_string(),
            task_name: "test_task".to_string(),
            background: Some(false),
            model: None,
            fork_context: false,
            fork_turns: None,
            max_turns: None,
            max_spawn_depth: None,
        },
        "call".to_string(),
        AbortSignal::new(rx),
    )
    .await
    .expect("tool output");

    assert!(output.is_error);
    assert!(
        output
            .model_content
            .as_deref()
            .unwrap_or_default()
            .contains("cannot delegate to peer agents")
            || output
                .json
                .to_string()
                .contains("cannot delegate to peer agents")
    );
    assert!(store.list_agent_edges().expect("edges").is_empty());
}
