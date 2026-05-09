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
