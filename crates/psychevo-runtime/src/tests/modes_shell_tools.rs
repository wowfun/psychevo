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
