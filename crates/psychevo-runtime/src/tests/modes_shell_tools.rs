#[test]
fn run_mode_tool_names_enforce_plan_read_only_surface() {
    assert_eq!(RunMode::Build.as_str(), "default");
    assert_eq!(RunMode::parse("default"), Some(RunMode::Build));
    assert_eq!(RunMode::parse("build"), None);
    assert_eq!(
        tool_names_for_mode(RunMode::Plan),
        vec!["read", "list", "search"]
    );
    assert_eq!(
        tool_names_for_mode(RunMode::Build),
        vec!["read", "write", "edit", "bash"]
    );
}

fn valid_clarify_args() -> Value {
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
async fn clarify_tool_validates_schema_and_is_sequential() {
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
async fn clarify_tool_round_trips_answer_and_rejects_late_response() {
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
        captured.lock().expect("captured stream lock").iter().any(
            |event| matches!(
                event,
                RunStreamEvent::ClarifyRequest(request)
                    if request.call_id == "call_clarify"
                        && request.questions[0].question == "Which mode should we use?"
            ),
        )
    })
    .await;
    assert!(request_seen, "clarify request event was not emitted");

    assert!(handle.submit_clarify_result(
        "call_clarify",
        ClarifyResult::Answered(ClarifyResponse {
            answers: vec![
                ClarifyAnswer {
                    answers: vec!["Careful".to_string(), "user_note: include tests".to_string()],
                },
            ],
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
    assert!(!handle.submit_clarify_result(
        "call_clarify",
        ClarifyResult::Cancelled
    ));
    assert!(
        captured.lock().expect("captured stream lock").iter().any(
            |event| matches!(
                event,
                RunStreamEvent::ClarifyResolved(resolved)
                    if resolved.call_id == "call_clarify"
                        && resolved.reason == ClarifyResolvedReason::Answered
            ),
        )
    );
}

#[tokio::test]
async fn clarify_tool_cancel_and_timeout_emit_resolution() {
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
    assert_eq!(
        output.json["error"],
        "clarify was cancelled by the user"
    );
    assert!(
        captured.lock().expect("captured stream lock").iter().any(
            |event| matches!(
                event,
                RunStreamEvent::ClarifyResolved(resolved)
                    if resolved.reason == ClarifyResolvedReason::Cancelled
            ),
        )
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
    assert!(!timeout_handle.submit_clarify_result(
        "call_clarify",
        ClarifyResult::Cancelled
    ));
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
async fn clarify_tool_returns_model_errors_for_invalid_or_unavailable_requests() {
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

    let unavailable = crate::tools::clarify_tool_impl(
        valid_clarify_args(),
        None,
        None,
    )
    .await;
    assert!(unavailable.is_error);
    assert_eq!(
        unavailable.json["error"],
        "clarify is not available in this execution context"
    );
}

async fn wait_until(
    timeout: Duration,
    condition: impl Fn() -> bool,
) -> bool {
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
async fn user_shell_streams_bash_events_without_provider_config() {
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
                && value["args"]["command"] == "printf 'shell ok\\n'"
    ));
    assert!(matches!(
        &events[1],
        RunStreamEvent::Event(value)
            if value["type"] == "tool_execution_end"
                && value["source"] == "user_shell"
                && value["outcome"] == "normal"
                && value["result"]["output"] == "shell ok\n"
    ));
}

#[tokio::test]
async fn user_shell_abort_returns_aborted_result() {
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
async fn user_shell_context_persists_user_xml_record() {
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
async fn user_shell_context_missing_config_rejects_before_execution() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let marker = workdir.join("marker");
    let context = UserShellContextOptions {
        db_path: temp.path().join("state.db"),
        session: None,
        continue_latest: true,
        source: "tui".to_string(),
        continue_sources: vec!["run".to_string(), "tui".to_string()],
        config_path: None,
        model: None,
        reasoning_effort: None,
        mode: RunMode::Build,
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
async fn user_shell_context_records_bounded_truncated_output() {
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
    assert_eq!(result.result["truncated"], true);
    let context_text = result.context_text.expect("context text");
    assert!(context_text.contains("Truncated: true"));
    assert!(context_text.len() < 60_000);
}

#[cfg(unix)]
#[tokio::test]
async fn bash_abort_kills_background_child_process_group() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let marker = workdir.join("bg.pid");
    let command = format!("sleep 60 & echo $! > {}; wait", shell_quote_path(&marker));
    let (handle, receivers) = psychevo_agent_core::ControlHandle::new();
    let task = tokio::spawn(crate::tools::run_bash_command(
        workdir,
        command,
        60,
        receivers.abort_signal(),
    ));

    let pid = wait_for_pid_file(&marker).await;
    assert!(process_exists(pid), "background child did not start");
    handle.abort();

    let (result, is_error) = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("abort should settle")
        .expect("bash task should join")
        .expect("bash result");
    assert!(is_error);
    assert_eq!(result["error"], "aborted");
    assert!(
        wait_for_process_exit(pid, Duration::from_secs(5)).await,
        "background child pid {pid} survived abort"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn bash_timeout_kills_background_child_process_group() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let marker = workdir.join("bg.pid");
    let command = format!("sleep 60 & echo $! > {}; wait", shell_quote_path(&marker));
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

    let (result, is_error) =
        crate::tools::run_bash_command(workdir, command, 1, receivers.abort_signal())
            .await
            .expect("bash result");

    assert!(is_error);
    assert_eq!(result["error"], "command timed out after 1 seconds");
    let pid = read_pid_file(&marker);
    assert!(
        wait_for_process_exit(pid, Duration::from_secs(5)).await,
        "background child pid {pid} survived timeout"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn bash_output_collection_does_not_wait_for_open_descendant_pipes() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let marker = workdir.join("detached.pid");
    let command = format!(
        "sh -c 'trap \"\" HUP; sleep 30' & echo $! > {}; echo done",
        shell_quote_path(&marker)
    );
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();
    let started = Instant::now();

    let (result, is_error) = tokio::time::timeout(
        Duration::from_secs(5),
        crate::tools::run_bash_command(workdir, command, 60, receivers.abort_signal()),
    )
    .await
    .expect("open descendant pipes should not hang")
    .expect("bash result");

    let pid = read_pid_file(&marker);
    let _ = unsafe { libc::kill(pid, libc::SIGKILL) };
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "output drain should have a bounded deadline"
    );
    assert!(!is_error);
    assert!(result["error"].is_null());
    assert_eq!(result["output"], "done\n");
}

#[tokio::test]
async fn bash_stdin_is_closed_for_prompt_style_commands() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let (_handle, receivers) = psychevo_agent_core::ControlHandle::new();

    let (result, is_error) = crate::tools::run_bash_command(
        workdir,
        "if read line; then printf 'read:%s\\n' \"$line\"; else printf 'stdin closed\\n'; fi"
            .to_string(),
        5,
        receivers.abort_signal(),
    )
    .await
    .expect("bash result");

    assert!(!is_error);
    assert!(result["error"].is_null());
    assert_eq!(result["output"], "stdin closed\n");
}

fn configured_user_shell_context(
    temp: &tempfile::TempDir,
    _workdir: &std::path::Path,
) -> UserShellContextOptions {
    let home = home_dir(temp);
    fs::create_dir_all(&home).expect("home");
    fs::write(
        home.join("config.jsonc"),
        r#"
        {
          "model": "lmstudio/test-model",
          "provider": {
            "lmstudio": {
              "models": { "test-model": {} }
            }
          }
        }
        "#,
    )
    .expect("config");
    UserShellContextOptions {
        db_path: temp.path().join("state.db"),
        session: None,
        continue_latest: true,
        source: "tui".to_string(),
        continue_sources: vec!["run".to_string(), "tui".to_string()],
        config_path: None,
        model: None,
        reasoning_effort: None,
        mode: RunMode::Build,
        inherited_env: Some(BTreeMap::from([
            (
                "HOME".to_string(),
                temp.path().to_string_lossy().to_string(),
            ),
            (
                "PSYCHEVO_HOME".to_string(),
                home.to_string_lossy().to_string(),
            ),
        ])),
    }
}

#[cfg(unix)]
fn shell_quote_path(path: &std::path::Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\"'\"'"))
}

#[cfg(unix)]
async fn wait_for_pid_file(path: &std::path::Path) -> i32 {
    let started = Instant::now();
    loop {
        if path.exists() {
            return read_pid_file(path);
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "timed out waiting for pid file {}",
            path.display()
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[cfg(unix)]
fn read_pid_file(path: &std::path::Path) -> i32 {
    fs::read_to_string(path)
        .expect("pid file")
        .trim()
        .parse()
        .expect("pid")
}

#[cfg(unix)]
fn process_exists(pid: i32) -> bool {
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(unix)]
async fn wait_for_process_exit(pid: i32, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if !process_exists(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

#[test]
fn plan_list_and_search_tools_are_read_only_and_bounded() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(workdir.join("src")).expect("dirs");
    fs::write(workdir.join("src/lib.rs"), "alpha\nneedle one\n").expect("file");
    fs::write(workdir.join("README.md"), "needle two\n").expect("file");
    let tool = WorkdirTool::new(workdir.canonicalize().expect("canonical"));

    let listed = list_tool_impl(tool.clone(), json!({"path":".","limit":1})).expect("list");
    assert_eq!(listed["entries"].as_array().expect("entries").len(), 1);
    assert_eq!(listed["truncated"], true);

    let searched =
        search_tool_impl(tool, json!({"query":"needle","path":".","limit":10})).expect("search");
    let matches = searched["matches"].as_array().expect("matches");
    assert_eq!(matches.len(), 2);
    assert!(
        matches
            .iter()
            .all(|entry| entry["line"].as_str().unwrap().contains("needle"))
    );
}
