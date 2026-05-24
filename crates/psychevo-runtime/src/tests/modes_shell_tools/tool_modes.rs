#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

#[test]
pub(crate) fn run_mode_tool_names_enforce_plan_read_only_surface() {
    assert_eq!(RunMode::Default.as_str(), "default");
    assert_eq!(RunMode::parse("default"), Some(RunMode::Default));
    assert_eq!(RunMode::parse("build"), None);
    let plan = tool_names_for_mode(RunMode::Plan);
    let default = tool_names_for_mode(RunMode::Default);
    assert_eq!(
        plan,
        vec!["read", "exec_command", "write_stdin", "web_fetch"]
    );
    assert_eq!(
        default,
        vec![
            "read",
            "write",
            "edit",
            "exec_command",
            "write_stdin",
            "web_fetch"
        ]
    );
    assert!(plan.iter().all(|name| default.contains(name)));
    assert!(!plan.contains(&"list"));
    assert!(!plan.contains(&"search"));
    assert!(!default.contains(&"list"));
    assert!(!default.contains(&"search"));
}

#[cfg(unix)]
#[tokio::test]
pub(crate) async fn exec_command_prepends_managed_tool_path() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    let tools_dir = temp.path().join("tools");
    fs::create_dir_all(&workdir).expect("workdir");
    fs::create_dir_all(&tools_dir).expect("tools");
    let rg = tools_dir.join("rg");
    fs::write(&rg, "#!/bin/sh\nprintf 'managed-rg\\n'\n").expect("fake rg");
    let mut permissions = fs::metadata(&rg).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&rg, permissions).expect("chmod");
    let tools = crate::tools::coding_core_tools_for_mode_with_context(
        &workdir,
        RunMode::Default,
        crate::tools::ToolRuntimeContext {
            task_id: "exec-path-test".to_string(),
            lsp: crate::config::LspConfig::default(),
            allow_login_shell: false,
            stream_events: None,
            path_prefixes: vec![tools_dir],
        },
    );
    let exec = tools
        .iter()
        .find(|tool| tool.name() == "exec_command")
        .expect("exec_command");
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

    let result = exec
        .execute(
            "call_exec_path".to_string(),
            json!({"cmd": "rg --version", "yield_time_ms": 250}),
            receivers.abort_signal(),
        )
        .await;

    assert!(!result.is_error, "{:?}", result.json);
    assert!(
        result.json["output"]
            .as_str()
            .unwrap_or_default()
            .contains("managed-rg"),
        "{:?}",
        result.json
    );
}

#[test]
pub(crate) fn exec_command_provider_schema_replaces_bash() {
    let temp = tempdir().expect("temp");
    let tools = crate::tools::coding_core_tools(temp.path());
    let names = tools.iter().map(|tool| tool.name()).collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "read",
            "write",
            "edit",
            "exec_command",
            "write_stdin",
            "web_fetch"
        ]
    );

    let exec = tools
        .iter()
        .find(|tool| tool.name() == "exec_command")
        .expect("exec_command");
    assert!(
        exec.description().contains("yield_time_ms"),
        "{}",
        exec.description()
    );
    let exec_params = exec.parameters();
    assert_eq!(exec_params["required"], json!(["cmd"]));
    assert!(exec_params["properties"]["cmd"]["description"].is_string());
    assert_eq!(exec_params["properties"]["tty"]["default"], false);
    assert_eq!(
        exec_params["properties"]["yield_time_ms"]["default"],
        10_000
    );
    assert_eq!(exec_params["properties"]["yield_time_ms"]["minimum"], 250);
    assert_eq!(
        exec_params["properties"]["yield_time_ms"]["maximum"],
        30_000
    );

    let stdin = tools
        .iter()
        .find(|tool| tool.name() == "write_stdin")
        .expect("write_stdin");
    let stdin_params = stdin.parameters();
    assert_eq!(stdin_params["required"], json!(["session_id"]));
    assert_eq!(stdin_params["properties"]["chars"]["default"], "");

    let web_fetch = tools
        .iter()
        .find(|tool| tool.name() == "web_fetch")
        .expect("web_fetch");
    assert!(
        web_fetch.description().contains("not a web search tool"),
        "{}",
        web_fetch.description()
    );
}

#[test]
pub(crate) fn toolset_config_controls_effective_core_tools() {
    let mut selection = crate::config::ToolSelectionConfig::default();
    selection.modes.insert(
        "plan".to_string(),
        crate::config::ToolModeConfig {
            enabled_toolsets: None,
            disabled_toolsets: vec!["web".to_string()],
        },
    );
    let names = crate::tools::effective_tool_names_for_mode_with_config(
        RunMode::Plan,
        &selection,
        &BTreeMap::new(),
    );
    assert_eq!(names, vec!["read", "exec_command", "write_stdin"]);

    let mut custom = BTreeMap::new();
    custom.insert(
        "writer".to_string(),
        crate::config::CustomToolsetConfig {
            description: None,
            tools: vec!["write".to_string(), "web_fetch".to_string()],
            includes: Vec::new(),
        },
    );
    let mut selection = crate::config::ToolSelectionConfig::default();
    selection.modes.insert(
        "plan".to_string(),
        crate::config::ToolModeConfig {
            enabled_toolsets: Some(vec!["writer".to_string()]),
            disabled_toolsets: Vec::new(),
        },
    );
    let names =
        crate::tools::effective_tool_names_for_mode_with_config(RunMode::Plan, &selection, &custom);
    assert_eq!(names, vec!["web_fetch"]);
}

#[test]
pub(crate) fn core_plan_and_clarify_tool_schemas_describe_parameters() {
    let temp = tempdir().expect("temp");
    let mut tools = crate::tools::coding_core_tools_for_mode(temp.path(), RunMode::Plan);
    tools.extend(crate::tools::coding_core_tools(temp.path()));
    tools.push(crate::tools::clarify_tool(None, None));

    for tool in tools {
        assert_schema_property_descriptions(tool.name(), &tool.parameters());
    }
}

pub(crate) fn valid_clarify_args() -> Value {
    json!({
        "questions": [{
            "question": "Which mode should we use?",
            "options": [
                {"label": "Fast", "description": "Prioritize speed"},
                {"label": "Careful", "description": "Prioritize review"}
            ]
        }]
    })
}

#[tokio::test]
pub(crate) async fn clarify_tool_validates_schema_and_is_sequential() {
    assert_eq!(
        crate::tools::clarify_tool(None, None).execution_mode(),
        psychevo_agent_core::ToolExecutionMode::Sequential
    );

    for (args, expected) in [
        (
            json!({"questions": []}),
            "clarify requires one to three questions",
        ),
        (
            json!({
                "questions": [
                    {
                        "question": "Which mode?",
                        "options": [
                            {"label": "Fast", "description": "Prioritize speed"},
                            {"label": "Careful", "description": "Prioritize review"}
                        ]
                    },
                    {
                        "question": "Which mode?",
                        "options": [
                            {"label": "Fast", "description": "Prioritize speed"},
                            {"label": "Careful", "description": "Prioritize review"}
                        ]
                    },
                    {
                        "question": "Which mode?",
                        "options": [
                            {"label": "Fast", "description": "Prioritize speed"},
                            {"label": "Careful", "description": "Prioritize review"}
                        ]
                    },
                    {
                        "question": "Which mode?",
                        "options": [
                            {"label": "Fast", "description": "Prioritize speed"},
                            {"label": "Careful", "description": "Prioritize review"}
                        ]
                    }
                ]
            }),
            "one to three questions",
        ),
        (
            json!({
                "questions": [{
                    "id": "old_id",
                    "header": "Old",
                    "question": "Which mode?",
                    "options": [
                        {"label": "Fast", "description": "Prioritize speed"},
                        {"label": "Careful", "description": "Prioritize review"}
                    ]
                }]
            }),
            "unknown field",
        ),
        (
            json!({
                "questions": [{
                    "question": "Which mode?",
                    "options": [
                        {"label": "Fast", "description": "Prioritize speed"}
                    ]
                }]
            }),
            "two to three options",
        ),
        (
            json!({
                "questions": [{
                    "question": "Which mode?",
                    "options": [
                        {"label": "", "description": "Prioritize speed"},
                        {"label": "Careful", "description": "Prioritize review"}
                    ]
                }]
            }),
            "non-empty label and description",
        ),
        (
            json!({
                "questions": [{
                    "question": "Which mode?",
                    "secret": true,
                    "options": [
                        {"label": "Fast", "description": "Prioritize speed"},
                        {"label": "Careful", "description": "Prioritize review"}
                    ]
                }]
            }),
            "unknown field",
        ),
    ] {
        let output = crate::tools::clarify_tool_impl(args, None, None).await;
        assert!(output.is_error);
        let error = output.json["error"].as_str().expect("error");
        assert!(
            error.contains(expected),
            "expected {expected:?} in {error:?}"
        );
    }
}

#[tokio::test]
pub(crate) async fn clarify_tool_round_trips_answer_and_rejects_late_response() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_for_stream = Arc::clone(&captured);
    let stream: RunStreamSink = Arc::new(move |event| {
        captured_for_stream
            .lock()
            .expect("captured stream lock")
            .push(event);
    });
    let (handle, _control) = run_control();
    let fut = crate::tools::clarify_tool_impl(
        valid_clarify_args(),
        Some(handle.clarify.clone()),
        Some(stream),
    );
    let task = tokio::spawn(fut);

    let request_seen = wait_until(Duration::from_secs(2), || {
        captured
            .lock()
            .expect("captured stream lock")
            .iter()
            .any(|event| {
                matches!(
                    event,
                    RunStreamEvent::ClarifyRequest(request)
                        if request.call_id == "call_clarify"
                            && request.questions[0].question == "Which mode should we use?"
                )
            })
    })
    .await;
    assert!(request_seen, "clarify request event was not emitted");

    assert!(handle.submit_clarify_result(
        "call_clarify",
        ClarifyResult::Answered(ClarifyResponse {
            answers: vec![ClarifyAnswer {
                answers: vec![
                    "Careful".to_string(),
                    "user_note: include tests".to_string()
                ],
            },],
        }),
    ));
    let output = task.await.expect("clarify task");
    assert!(!output.is_error);
    assert_eq!(
        output.json,
        json!({
            "answers": [
                {
                    "answers": ["Careful", "user_note: include tests"]
                }
            ]
        })
    );
    assert!(!handle.submit_clarify_result("call_clarify", ClarifyResult::Cancelled));
    assert!(
        captured
            .lock()
            .expect("captured stream lock")
            .iter()
            .any(|event| matches!(
                event,
                RunStreamEvent::ClarifyResolved(resolved)
                    if resolved.call_id == "call_clarify"
                        && resolved.reason == ClarifyResolvedReason::Answered
            ),)
    );
}

#[tokio::test]
pub(crate) async fn clarify_tool_cancel_and_timeout_emit_resolution() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_for_stream = Arc::clone(&captured);
    let stream: RunStreamSink = Arc::new(move |event| {
        captured_for_stream
            .lock()
            .expect("captured stream lock")
            .push(event);
    });
    let (handle, _control) = run_control();
    let task = tokio::spawn(crate::tools::clarify_tool_impl(
        valid_clarify_args(),
        Some(handle.clarify.clone()),
        Some(stream),
    ));
    assert!(
        wait_until(Duration::from_secs(2), || captured
            .lock()
            .expect("captured stream lock")
            .iter()
            .any(|event| matches!(event, RunStreamEvent::ClarifyRequest(_))))
        .await
    );

    assert!(handle.submit_clarify_result("call_clarify", ClarifyResult::Cancelled));
    let output = task.await.expect("clarify task");
    assert!(output.is_error);
    assert_eq!(output.json["error"], "clarify was cancelled by the user");
    assert!(
        captured
            .lock()
            .expect("captured stream lock")
            .iter()
            .any(|event| matches!(
                event,
                RunStreamEvent::ClarifyResolved(resolved)
                    if resolved.reason == ClarifyResolvedReason::Cancelled
            ),)
    );

    let timed_out_events = Arc::new(Mutex::new(Vec::new()));
    let timed_out_events_for_stream = Arc::clone(&timed_out_events);
    let timeout_stream: RunStreamSink = Arc::new(move |event| {
        timed_out_events_for_stream
            .lock()
            .expect("timed out stream lock")
            .push(event);
    });
    let (timeout_handle, _control) = run_control();
    let timeout_output = tokio::time::timeout(
        Duration::from_secs(2),
        crate::tools::clarify_tool_impl(
            valid_clarify_args(),
            Some(timeout_handle.clarify.clone()),
            Some(timeout_stream),
        ),
    )
    .await
    .expect("clarify should time out in tests");
    assert!(timeout_output.is_error);
    assert_eq!(
        timeout_output.json["error"],
        "timed out waiting for user input"
    );
    assert!(!timeout_handle.submit_clarify_result("call_clarify", ClarifyResult::Cancelled));
    assert!(
        timed_out_events
            .lock()
            .expect("timed out stream lock")
            .iter()
            .any(|event| matches!(
                event,
                RunStreamEvent::ClarifyResolved(resolved)
                    if resolved.reason == ClarifyResolvedReason::TimedOut
            ))
    );
}

#[tokio::test]
pub(crate) async fn clarify_tool_returns_model_errors_for_invalid_or_unavailable_requests() {
    let invalid = crate::tools::clarify_tool_impl(
        json!({
            "questions": [{
                "id": "TargetMode",
                "header": "Mode",
                "question": "Which mode?",
                "options": [
                    {"label": "Fast", "description": "Prioritize speed"},
                    {"label": "Careful", "description": "Prioritize review"}
                ]
            }]
        }),
        None,
        None,
    )
    .await;
    assert!(invalid.is_error);
    assert!(
        invalid.json["error"]
            .as_str()
            .expect("error")
            .contains("unknown field")
    );

    let unavailable = crate::tools::clarify_tool_impl(valid_clarify_args(), None, None).await;
    assert!(unavailable.is_error);
    assert_eq!(
        unavailable.json["error"],
        "clarify is not available in this execution context"
    );
}

pub(crate) async fn wait_until(timeout: Duration, condition: impl Fn() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    condition()
}

#[tokio::test]
pub(crate) async fn user_shell_streams_exec_command_events_without_provider_config() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_for_stream = Arc::clone(&captured);
    let stream: RunStreamSink = Arc::new(move |event| {
        captured_for_stream
            .lock()
            .expect("captured stream lock")
            .push(event);
    });
    let (_handle, control) = run_control();

    let result = run_user_shell_command_streaming_controlled(
        UserShellOptions {
            workdir: workdir.clone(),
            command: "printf 'shell ok\\n'".to_string(),
            context: None,
            inject_into: None,
        },
        stream,
        control,
    )
    .await
    .expect("user shell");

    assert_eq!(result.outcome, Outcome::Normal);
    assert_eq!(result.tool_failures, 0);
    assert_eq!(result.result["output"], "shell ok\n");
    let events = captured.lock().expect("captured stream lock");
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[0],
        RunStreamEvent::Event(value)
            if value["type"] == "tool_execution_start"
                && value["source"] == "user_shell"
                && value["tool_name"] == "exec_command"
                && value["args"]["cmd"] == "printf 'shell ok\\n'"
    ));
    assert!(matches!(
        &events[1],
        RunStreamEvent::Event(value)
            if value["type"] == "tool_execution_end"
                && value["source"] == "user_shell"
                && value["tool_name"] == "exec_command"
                && value["outcome"] == "normal"
                && value["result"]["output"] == "shell ok\n"
    ));
}

#[tokio::test]
pub(crate) async fn user_shell_abort_returns_aborted_result() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let stream: RunStreamSink = Arc::new(|_| {});
    let (handle, control) = run_control();
    handle.abort();

    let result = run_user_shell_command_streaming_controlled(
        UserShellOptions {
            workdir,
            command: "sleep 5".to_string(),
            context: None,
            inject_into: None,
        },
        stream,
        control,
    )
    .await
    .expect("user shell");

    assert_eq!(result.outcome, Outcome::Aborted);
    assert_eq!(result.tool_failures, 0);
    assert_eq!(result.result["error"], "aborted");
}

#[tokio::test]
pub(crate) async fn user_shell_context_persists_user_xml_record() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let context = configured_user_shell_context(&temp, &workdir);
    let stream: RunStreamSink = Arc::new(|_| {});
    let (_handle, control) = run_control();

    let result = run_user_shell_command_streaming_controlled(
        UserShellOptions {
            workdir: workdir.clone(),
            command: "printf 'context-ok\\n'".to_string(),
            context: Some(context),
            inject_into: None,
        },
        stream,
        control,
    )
    .await
    .expect("user shell");

    assert_eq!(result.outcome, Outcome::Normal);
    let session_id = result.session_id.as_deref().expect("session");
    let context_text = result.context_text.as_deref().expect("context text");
    assert!(context_text.contains("<user_shell_command><command>printf 'context-ok\\n'</command>"));
    assert!(context_text.contains("Exit code: 0"));
    assert!(context_text.contains("Truncated: false"));
    assert!(context_text.contains("Output:\ncontext-ok\n"));

    let store = SqliteStore::open(&temp.path().join("state.db")).expect("store");
    let messages = store.load_messages(session_id).expect("messages");
    assert_eq!(messages.len(), 1);
    match &messages[0] {
        Message::User { content, .. } => {
            assert_eq!(content.len(), 1);
            assert_eq!(content[0].text_value(), Some(context_text));
        }
        other => panic!("expected user shell message, got {other:?}"),
    }
    let summary = store
        .session_summary(session_id)
        .expect("summary")
        .expect("session summary");
    assert_eq!(
        summary.title.as_deref(),
        Some("Shell: printf 'context-ok\\n'")
    );
    let tui = store
        .load_tui_message_summaries(session_id)
        .expect("summaries");
    assert_eq!(
        tui[0]
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get(USER_SHELL_METADATA_KEY))
            .and_then(|metadata| metadata.get("command"))
            .and_then(Value::as_str),
        Some("printf 'context-ok\\n'")
    );

    let mut resume_context = configured_user_shell_context(&temp, &workdir);
    resume_context.session = Some(session_id.to_string());
    let (_handle, control) = run_control();
    let resumed = run_user_shell_command_streaming_controlled(
        UserShellOptions {
            workdir,
            command: "printf 'again\\n'".to_string(),
            context: Some(resume_context),
            inject_into: None,
        },
        Arc::new(|_| {}),
        control,
    )
    .await
    .expect("resumed user shell");
    assert_eq!(resumed.session_id.as_deref(), Some(session_id));
    assert_eq!(store.load_messages(session_id).expect("messages").len(), 2);
}

#[tokio::test]
pub(crate) async fn user_shell_context_missing_config_rejects_before_execution() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let marker = workdir.join("marker");
    let context = UserShellContextOptions {
        state: StateRuntime::open(temp.path().join("state.db")).expect("state runtime"),
        session: None,
        continue_latest: true,
        source: "tui".to_string(),
        continue_sources: vec!["run".to_string(), "tui".to_string()],
        config_path: None,
        model: None,
        reasoning_effort: None,
        mode: RunMode::Default,
        inherited_env: Some(BTreeMap::from([(
            "HOME".to_string(),
            temp.path().to_string_lossy().to_string(),
        )])),
    };
    let stream: RunStreamSink = Arc::new(|_| {});
    let (_handle, control) = run_control();
    let err = run_user_shell_command_streaming_controlled(
        UserShellOptions {
            workdir,
            command: "touch marker".to_string(),
            context: Some(context),
            inject_into: None,
        },
        stream,
        control,
    )
    .await
    .expect_err("missing config");

    assert!(
        err.to_string().contains("config") || err.to_string().contains("PSYCHEVO_HOME"),
        "{err:#}"
    );
    assert!(!marker.exists());
}

#[tokio::test]
pub(crate) async fn user_shell_context_records_bounded_truncated_output() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let context = configured_user_shell_context(&temp, &workdir);
    let stream: RunStreamSink = Arc::new(|_| {});
    let (_handle, control) = run_control();

    let result = run_user_shell_command_streaming_controlled(
        UserShellOptions {
            workdir,
            command: "yes x | head -c 60000".to_string(),
            context: Some(context),
            inject_into: None,
        },
        stream,
        control,
    )
    .await
    .expect("user shell");

    assert_eq!(result.outcome, Outcome::Normal);
    assert!(
        result.result["original_token_count"]
            .as_u64()
            .unwrap_or_default()
            > 10_000
    );
    let context_text = result.context_text.expect("context text");
    assert!(context_text.contains("Truncated: true"));
    assert!(context_text.len() < 60_000);
}

#[cfg(unix)]
#[tokio::test]
pub(crate) async fn exec_command_abort_kills_background_child_process_group() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let marker = workdir.join("bg.pid");
    let command = format!("sleep 60 & echo $! > {}; wait", shell_quote_path(&marker));
    let (handle, receivers) = psychevo_agent_core::ControlHandle::new();
    let task = tokio::spawn(crate::tools::run_exec_command_for_user_shell(
        workdir,
        command,
        receivers.abort_signal(),
    ));

    let pid = wait_for_pid_file(&marker).await;
    assert!(process_exists(pid), "background child did not start");
    handle.abort();

    let result = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("abort should settle")
        .expect("exec task should join");
    assert!(
        result
            .expect_err("abort should fail")
            .to_string()
            .contains("aborted")
    );
    assert!(
        wait_for_process_exit(pid, Duration::from_secs(5)).await,
        "background child pid {pid} survived abort"
    );
}

#[cfg(unix)]
#[tokio::test]
pub(crate) async fn exec_command_yields_long_running_session() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

    let result = crate::tools::exec_command_tool_impl(
        workdir,
        false,
        json!({"cmd": "sleep 1; printf done", "yield_time_ms": 250}),
        receivers.abort_signal(),
    )
    .await
    .expect("exec result");

    assert!(result["session_id"].is_u64(), "{result}");
    assert!(result["exit_code"].is_null(), "{result}");
}

pub(crate) fn run_stream_event_value(event: &crate::types::RunStreamEvent) -> Option<&Value> {
    match event {
        crate::types::RunStreamEvent::Event(value) => Some(value),
        crate::types::RunStreamEvent::Scoped { event, .. } => run_stream_event_value(event),
        _ => None,
    }
}

pub(crate) fn latest_event_type(
    events: &Arc<Mutex<Vec<crate::types::RunStreamEvent>>>,
    event_type: &str,
) -> Option<Value> {
    events
        .lock()
        .expect("events")
        .iter()
        .filter_map(run_stream_event_value)
        .find(|value| value.get("type").and_then(Value::as_str) == Some(event_type))
        .cloned()
}

pub(crate) fn assert_event_type(
    events: &Arc<Mutex<Vec<crate::types::RunStreamEvent>>>,
    event_type: &str,
) {
    assert!(
        latest_event_type(events, event_type).is_some(),
        "missing event {event_type}: {:?}",
        events.lock().expect("events")
    );
}

pub(crate) async fn wait_for_event_type(
    events: &Arc<Mutex<Vec<crate::types::RunStreamEvent>>>,
    event_type: &str,
) -> Value {
    for _ in 0..100 {
        if let Some(value) = latest_event_type(events, event_type) {
            return value;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!(
        "missing event {event_type}: {:?}",
        events.lock().expect("events")
    );
}
