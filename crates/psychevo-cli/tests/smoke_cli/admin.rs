fn admin_cmd(test_home: &Path, psychevo_home: &Path, workdir: &Path) -> Command {
    let mut command = pevo_cmd(test_home);
    command
        .env("PSYCHEVO_HOME", psychevo_home)
        .current_dir(workdir);
    command
}

fn run_with_stdin(mut command: Command, input: &str) -> std::process::Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn command");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("command output")
}

#[test]
fn cli_rejects_obsolete_plural_skills_command() {
    let temp = tempdir().expect("temp");
    let output = pevo_cmd(temp.path())
        .args(["skills", "list"])
        .output()
        .expect("pevo skills");
    assert!(!output.status.success());
}

#[test]
fn cli_config_provider_and_auth_write_scoped_env_without_leaking_secret() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(&workdir).expect("workdir");
    init_skill_home(temp.path(), &psychevo_home);

    let add_local = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args([
            "config",
            "provider",
            "add",
            "--id",
            "mock-local",
            "--label",
            "Mock Local",
            "--base-url",
            "http://127.0.0.1:7777/v1",
            "--api-key-env",
            "MOCK_LOCAL_KEY",
            "--local",
            "--json",
        ])
        .output()
        .expect("provider add local");
    assert!(
        add_local.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&add_local.stderr)
    );
    let value: Value = serde_json::from_slice(&add_local.stdout).expect("json");
    assert_eq!(value["scope"], "local");
    assert_eq!(value["api_key_env"], "MOCK_LOCAL_KEY");
    assert_eq!(value["wrote_api_key"], false);

    let local_config =
        std::fs::read_to_string(workdir.join(".psychevo/config.toml")).expect("config");
    assert!(local_config.contains("MOCK_LOCAL_KEY"));
    assert!(!local_config.contains("secret-key"));

    let mut set_cmd = admin_cmd(temp.path(), &psychevo_home, &workdir);
    set_cmd.args([
        "auth",
        "set",
        "mock-local",
        "--api-key-stdin",
        "--local",
        "--json",
    ]);
    let set = run_with_stdin(set_cmd, "secret-key\n");
    assert!(
        set.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&set.stderr)
    );
    assert!(!String::from_utf8_lossy(&set.stdout).contains("secret-key"));
    assert!(!String::from_utf8_lossy(&set.stderr).contains("secret-key"));
    let local_env = std::fs::read_to_string(workdir.join(".psychevo/.env")).expect("env");
    assert_eq!(local_env, "MOCK_LOCAL_KEY=secret-key\n");

    let status = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["auth", "status", "mock-local", "--json"])
        .output()
        .expect("auth status");
    assert!(
        status.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    let value: Value = serde_json::from_slice(&status.stdout).expect("json");
    assert_eq!(value["providers"][0]["status"], "present");
    assert!(!String::from_utf8_lossy(&status.stdout).contains("secret-key"));

    let mut add_global_cmd = admin_cmd(temp.path(), &psychevo_home, &workdir);
    add_global_cmd.args([
        "config",
        "provider",
        "add",
        "--id",
        "mock-global",
        "--label",
        "Mock Global",
        "--base-url",
        "http://127.0.0.1:8888/v1",
        "--api-key-stdin",
        "--json",
    ]);
    let add_global = run_with_stdin(add_global_cmd, "global-secret\n");
    assert!(
        add_global.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&add_global.stderr)
    );
    assert!(psychevo_home.join("config.toml").exists());
    assert!(
        std::fs::read_to_string(psychevo_home.join(".env"))
            .expect("global env")
            .contains("MOCK_GLOBAL_API_KEY=global-secret")
    );
    assert!(!String::from_utf8_lossy(&add_global.stdout).contains("global-secret"));
}

#[test]
fn cli_config_permissions_lists_and_removes_project_local_rules() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(workdir.join(".psychevo")).expect("workdir");
    init_skill_home(temp.path(), &psychevo_home);
    std::fs::write(
        workdir.join(".psychevo/config.toml"),
        r#"# project-local policy

[permissions]
allow = ["ExecCommand(npm test *)"]
ask = ["ExecCommand(cargo publish *)"]
deny = ["Write(.env)"]
"#,
    )
    .expect("config");

    let listed = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["config", "permissions", "list", "--json"])
        .output()
        .expect("permissions list");
    assert!(
        listed.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let value: Value = serde_json::from_slice(&listed.stdout).expect("json");
    assert_eq!(value["scope"], "local");
    assert_eq!(value["permissions"]["allow"][0], "ExecCommand(npm test *)");
    assert_eq!(value["permissions"]["ask"][0], "ExecCommand(cargo publish *)");
    assert_eq!(value["permissions"]["deny"][0], "Write(.env)");

    let removed = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args([
            "config",
            "permissions",
            "remove",
            "--kind",
            "allow",
            "--rule",
            "ExecCommand(npm test *)",
            "--json",
        ])
        .output()
        .expect("permissions remove");
    assert!(
        removed.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&removed.stderr)
    );
    let value: Value = serde_json::from_slice(&removed.stdout).expect("json");
    assert_eq!(value["changed"], true);
    let config = std::fs::read_to_string(workdir.join(".psychevo/config.toml")).expect("config");
    assert!(!config.contains("ExecCommand(npm test *)"));
    assert!(config.contains("ExecCommand(cargo publish *)"));
}

#[test]
fn cli_tool_commands_list_and_toggle_project_toolsets() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(&workdir).expect("workdir");
    init_skill_home(temp.path(), &psychevo_home);

    let listed = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["tool", "list", "--json"])
        .output()
        .expect("tool list");
    assert!(
        listed.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let value: Value = serde_json::from_slice(&listed.stdout).expect("json");
    assert_eq!(value["toolsets"][0]["name"], "coding-core");
    assert!(
        value["modes"]["plan"]["effective_tools"]
            .as_array()
            .expect("plan tools")
            .iter()
            .any(|tool| tool.as_str() == Some("web_fetch"))
    );

    let disabled = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["tool", "disable", "web", "--mode", "plan", "--json"])
        .output()
        .expect("tool disable");
    assert!(
        disabled.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&disabled.stderr)
    );
    let value: Value = serde_json::from_slice(&disabled.stdout).expect("json");
    assert_eq!(value["changed"], true);

    let listed = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["tool", "list", "--json"])
        .output()
        .expect("tool list");
    let value: Value = serde_json::from_slice(&listed.stdout).expect("json");
    assert!(
        !value["modes"]["plan"]["effective_tools"]
            .as_array()
            .expect("plan tools")
            .iter()
            .any(|tool| tool.as_str() == Some("web_fetch"))
    );

    let created = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args([
            "tool",
            "create",
            "docs",
            "--tool",
            "web_fetch",
            "--description",
            "Docs URLs",
            "--json",
        ])
        .output()
        .expect("tool create");
    assert!(
        created.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&created.stderr)
    );

    let enabled = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["tool", "enable", "docs", "--mode", "plan", "--json"])
        .output()
        .expect("tool enable");
    assert!(
        enabled.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&enabled.stderr)
    );
    let listed = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["tool", "list", "--json"])
        .output()
        .expect("tool list");
    let value: Value = serde_json::from_slice(&listed.stdout).expect("json");
    assert!(
        value["modes"]["plan"]["effective_tools"]
            .as_array()
            .expect("plan tools")
            .iter()
            .any(|tool| tool.as_str() == Some("web_fetch"))
    );

    let removed = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["tool", "remove", "docs", "--json"])
        .output()
        .expect("tool remove");
    assert!(
        removed.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&removed.stderr)
    );
    let config = std::fs::read_to_string(workdir.join(".psychevo/config.toml")).expect("config");
    assert!(!config.contains("[toolsets.docs]"));
}

#[test]
fn cli_session_commands_manage_active_and_archived_sessions() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(&workdir).expect("workdir");
    init_skill_home(temp.path(), &psychevo_home);
    let canonical = workdir.canonicalize().expect("canonical");
    let db = psychevo_home.join("state.db");
    let conn = Connection::open(&db).expect("db");
    insert_session(&conn, "older", &canonical, "run", 1_000, 1_000);
    insert_session(&conn, "newer", &canonical, "tui", 2_000, 2_000);

    let list = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["session", "list", "--json"])
        .output()
        .expect("session list");
    assert!(
        list.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&list.stderr)
    );
    let value: Value = serde_json::from_slice(&list.stdout).expect("json");
    assert_eq!(value["sessions"][0]["id"], "newer");

    let rename = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["session", "rename", "latest", "New", "Title", "--json"])
        .output()
        .expect("session rename");
    assert!(
        rename.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&rename.stderr)
    );
    let value: Value = serde_json::from_slice(&rename.stdout).expect("json");
    assert_eq!(value["session"]["id"], "newer");
    assert_eq!(value["session"]["title"], "New Title");

    let archive = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["session", "archive", "latest", "--json"])
        .output()
        .expect("session archive");
    assert!(archive.status.success());

    let archived = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["session", "list", "--archived", "--json"])
        .output()
        .expect("session archived list");
    let value: Value = serde_json::from_slice(&archived.stdout).expect("json");
    assert_eq!(value["sessions"][0]["id"], "newer");

    let restore = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["session", "restore", "newer", "--json"])
        .output()
        .expect("session restore");
    assert!(restore.status.success());
}

#[test]
fn cli_session_export_and_share_emit_local_artifacts() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(&workdir).expect("workdir");
    init_skill_home(temp.path(), &psychevo_home);
    let canonical = workdir.canonicalize().expect("canonical");
    let db = psychevo_home.join("state.db");
    let conn = Connection::open(&db).expect("db");
    insert_session(&conn, "exported-session", &canonical, "tui", 1_000, 2_000);
    set_export_fixture_session_metadata(&conn, "exported-session");
    insert_export_fixture_messages(&conn, "exported-session");

    let default_export = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["session", "export", "latest"])
        .output()
        .expect("session export");
    assert!(
        default_export.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&default_export.stderr)
    );
    let stdout = String::from_utf8(default_export.stdout).expect("stdout");
    assert!(!stdout.contains("# Psychevo Session Export"));
    assert!(stdout.contains("visible answer"));
    assert!(stdout.contains("Tool call: `read`"));
    assert!(stdout.contains("file body"));
    assert!(!stdout.contains("private plan"));
    assert!(!stdout.contains("hidden AGENTS"));

    let reasoning_export = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["session", "export", "latest", "--include", "reasoning"])
        .output()
        .expect("session export reasoning");
    assert!(reasoning_export.status.success());
    let stdout = String::from_utf8(reasoning_export.stdout).expect("stdout");
    assert!(stdout.contains("private plan"));
    assert!(!stdout.contains("provider_evidence"));

    let full_inputs_export = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args([
            "session",
            "export",
            "latest",
            "--include",
            "provider-input-evidence",
        ])
        .output()
        .expect("session export full inputs");
    assert!(full_inputs_export.status.success());
    let stdout = String::from_utf8(full_inputs_export.stdout).expect("stdout");
    assert!(stdout.contains("Provider Input Evidence"));
    assert!(stdout.contains("hidden AGENTS"));

    let json_export = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["session", "export", "latest", "--format", "json"])
        .output()
        .expect("session export json");
    assert!(json_export.status.success());
    let value: Value = serde_json::from_slice(&json_export.stdout).expect("json");
    assert!(value.get("header").is_none());
    assert!(value.get("prompt_prefix").is_none());
    assert_eq!(value["messages"][0]["session_seq"], 1);
    assert!(value.get("provider_input_evidence").is_none());
    assert!(value.get("last_provider_request").is_none());

    let json_header = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args([
            "session",
            "export",
            "latest",
            "--format",
            "json",
            "--include",
            "header",
        ])
        .output()
        .expect("session export json header");
    assert!(json_header.status.success());
    let value: Value = serde_json::from_slice(&json_header.stdout).expect("json");
    assert_eq!(value["header"]["session"]["id"], "exported-session");
    assert_eq!(value["header"]["options"]["format"].as_str(), Some("json"));
    assert_eq!(value["header"]["options"]["include"], serde_json::json!(["header"]));
    assert_eq!(value["header"]["prompt_prefix"]["prefix_hash"], "fixture-prefix-hash");
    assert_eq!(
        value["header"]["prompt_prefix"]["metadata"]["effective_tools"],
        serde_json::json!([
            "read",
            "Agent",
            "list_agents",
            "wait_agent",
            "send_message",
            "close_agent",
            "resume_agent",
            "list_skills",
            "view_skill"
        ])
    );
    assert_eq!(
        value["header"]["prompt_prefix"]["metadata"]["project_instructions_role"],
        "system"
    );
    assert_eq!(
        value["header"]["prompt_prefix"]["slots"][1]["slot"],
        "agent_catalog"
    );
    assert!(value["header"]["prompt_prefix"]["slots"][1].get("content").is_none());
    assert!(value.get("prompt_prefix").is_none());
    assert!(value.get("messages").is_none());

    let markdown_header = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["session", "export", "latest", "--include", "header"])
        .output()
        .expect("session export markdown header");
    assert!(markdown_header.status.success());
    let stdout = String::from_utf8(markdown_header.stdout).expect("stdout");
    assert!(stdout.contains("### Prompt Prefix"));
    assert!(!stdout.contains("\n## Prompt Prefix"));
    assert!(stdout.contains("agent_catalog"));
    assert!(!stdout.contains("Available agents"));

    let json_last_request = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args([
            "session",
            "export",
            "latest",
            "--format",
            "json",
            "-i",
            "lpr",
        ])
        .output()
        .expect("session export json last request");
    assert!(
        json_last_request.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&json_last_request.stderr)
    );
    let value: Value = serde_json::from_slice(&json_last_request.stdout).expect("json");
    assert!(value.get("header").is_none());
    assert!(value.get("messages").is_none());
    let last = &value["last_provider_request"];
    assert_eq!(last["prompt_session_seq"], 1);
    assert_eq!(last["assistant_session_seq"], 4);
    assert!(last.get("request_index").is_none());
    assert_eq!(last["provider"], "provider");
    assert_eq!(last["model"], "model");
    assert_eq!(last["base_url"], "https://example.test/v1");
    assert_eq!(last["endpoint"], "https://example.test/v1/chat/completions");
    assert_eq!(last["reconstructed"], true);
    assert_eq!(last["body"]["model"], "model");
    let provider_messages = last["body"]["messages"].as_array().expect("messages");
    let message_texts = provider_messages
        .iter()
        .filter_map(|message| message["content"].as_str())
        .collect::<Vec<_>>();
    let agent_catalog_index = message_texts
        .iter()
        .position(|text| text.contains("Available agents"))
        .expect("agent catalog");
    let skill_index = message_texts
        .iter()
        .position(|text| text.contains("Available skills"))
        .expect("skill index");
    let project_context_index = message_texts
        .iter()
        .position(|text| text.contains("hidden AGENTS root"))
        .expect("project context");
    let selected_skill_index = message_texts
        .iter()
        .position(|text| text.contains("<name>reviewer</name>"))
        .expect("selected skill");
    let prompt_index = message_texts
        .iter()
        .position(|text| *text == "hello export")
        .expect("prompt");
    assert!(agent_catalog_index < skill_index);
    assert!(skill_index < project_context_index);
    assert!(project_context_index < selected_skill_index);
    assert!(selected_skill_index < prompt_index);
    assert!(message_texts
        .iter()
        .any(|text| text.contains("hidden AGENTS local")));
    assert!(last["body"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .any(|tool| tool["function"]["name"] == "read"));
    assert!(!last["body"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .any(|tool| tool["function"]["name"] == "write"));
    assert!(last["body"]["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .any(|message| message["role"] == "tool" && message["content"] == "file body"));

    let markdown_last_request = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args([
            "session",
            "export",
            "latest",
            "--include",
            "last-provider-request",
        ])
        .output()
        .expect("session export markdown last request");
    assert!(markdown_last_request.status.success());
    let stdout = String::from_utf8(markdown_last_request.stdout).expect("stdout");
    assert!(stdout.contains("## Reconstructed Last Provider Request"));
    assert!(stdout.contains("Available agents"));
    assert!(stdout.contains("Available skills"));
    assert!(stdout.contains("hidden AGENTS"));
    assert_eq!(
        stdout
            .matches("## Reconstructed Last Provider Request")
            .count(),
        1
    );

    conn.execute(
        "UPDATE session_prompt_prefixes SET prefix_hash = 'newer-prefix-hash' WHERE session_id = ?1",
        rusqlite::params!["exported-session"],
    )
    .expect("stale prefix hash");
    let stale_last_request = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args([
            "session",
            "export",
            "latest",
            "-f",
            "json",
            "--include",
            "last-provider-request",
        ])
        .output()
        .expect("stale session export json last request");
    assert!(stale_last_request.status.success());
    let value: Value = serde_json::from_slice(&stale_last_request.stdout).expect("json");
    let warnings = value["last_provider_request"]["warnings"]
        .as_array()
        .expect("warnings");
    assert!(warnings.iter().any(|warning| warning
        .as_str()
        .is_some_and(|text| text.contains("does not match") && text.contains("approximate"))));

    let old_reasoning_export = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["session", "export", "latest", "--with-reasoning"])
        .output()
        .expect("session export old reasoning");
    assert!(!old_reasoning_export.status.success());

    let old_full_inputs_export = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["session", "export", "latest", "--full-inputs"])
        .output()
        .expect("session export old full inputs");
    assert!(!old_full_inputs_export.status.success());

    let raw_requests_export = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["session", "export", "latest", "--raw-requests"])
        .output()
        .expect("session export raw requests");
    assert!(!raw_requests_export.status.success());

    let share_last_request = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args([
            "session",
            "share",
            "latest",
            "--include",
            "last-provider-request",
        ])
        .output()
        .expect("session share last request");
    assert!(!share_last_request.status.success());

    let json_full_inputs = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args([
            "session",
            "export",
            "latest",
            "--format",
            "json",
            "--include",
            "provider-input-evidence",
        ])
        .output()
        .expect("session export json full inputs");
    assert!(json_full_inputs.status.success());
    let value: Value = serde_json::from_slice(&json_full_inputs.stdout).expect("json");
    assert!(value.get("messages").is_none());
    assert_eq!(
        value["provider_input_evidence"][0]["items"][0]["content_text"],
        "# AGENTS.md instructions for /repo\n\n<INSTRUCTIONS>\nhidden AGENTS root\n</INSTRUCTIONS>"
    );
    assert_eq!(
        value["provider_input_evidence"][0]["items"][0]["provider_group"],
        "project_instructions"
    );

    let share = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["session", "share", "latest", "--json"])
        .output()
        .expect("session share");
    assert!(
        share.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&share.stderr)
    );
    let value: Value = serde_json::from_slice(&share.stdout).expect("json");
    assert_eq!(value["action"], "share");
    let path = PathBuf::from(value["path"].as_str().expect("path"));
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("psychevo-share-exported-sess.md")
    );
    let content = std::fs::read_to_string(path).expect("share artifact");
    assert!(content.contains("visible answer"));
}

#[test]
fn cli_model_list_current_and_fetch_use_local_config_and_explicit_fetch_only() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(&workdir).expect("workdir");
    init_skill_home(temp.path(), &psychevo_home);
    let server = CatalogJsonServer::start(r#"{"data":[{"id":"mock-a"},{"id":"mock-b"}]}"#);
    std::fs::write(psychevo_home.join(".env"), "MOCK_KEY=test-key\n").expect("env");
    std::fs::write(
        psychevo_home.join("config.toml"),
        format!(
            r#"model = "mock/mock-a"

[provider.mock]
label = "Mock"

[provider.mock.options]
base_url = "{}/v1"
api_key_env = "MOCK_KEY"

[provider.mock.models."mock-a"]
"#,
            server.base_url
        ),
    )
    .expect("config");

    let list = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["model", "list", "--json"])
        .output()
        .expect("model list");
    assert!(list.status.success());
    let value: Value = serde_json::from_slice(&list.stdout).expect("json");
    assert_eq!(value["models"][0]["provider"], "mock");
    assert_eq!(server.requests.lock().expect("requests").len(), 0);

    let current = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["model", "current", "--json"])
        .output()
        .expect("model current");
    assert!(current.status.success());
    let value: Value = serde_json::from_slice(&current.stdout).expect("json");
    assert_eq!(value["model"]["model"], "mock-a");
    assert_eq!(server.requests.lock().expect("requests").len(), 0);

    let fetch = admin_cmd(temp.path(), &psychevo_home, &workdir)
        .args(["model", "fetch", "mock", "--json"])
        .output()
        .expect("model fetch");
    assert!(
        fetch.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&fetch.stderr)
    );
    let value: Value = serde_json::from_slice(&fetch.stdout).expect("json");
    assert_eq!(value["providers"][0]["models"][0]["id"], "mock-a");
    let requests = server.requests.lock().expect("requests");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /v1/models HTTP/1.1"));
}

fn insert_export_fixture_messages(conn: &Connection, session_id: &str) {
    let prompt_prefix_hash = "fixture-prefix-hash";
    let user = serde_json::json!({
        "role": "user",
        "content": [{"text": "hello export"}],
        "timestamp_ms": 1_100
    });
    let assistant = serde_json::json!({
        "role": "assistant",
        "content": [
            {
                "type": "reasoning",
                "text": "private plan",
                "provider_evidence": {"secret": true}
            },
            {"type": "text", "text": "visible answer"},
            {
                "type": "tool_call",
                "id": "call_read",
                "name": "read",
                "arguments": {"path": "fixture.txt"},
                "arguments_json": "{\"path\":\"fixture.txt\"}",
                "arguments_error": null,
                "content_index": 1,
                "call_index": 0
            }
        ],
        "timestamp_ms": 1_200,
        "finish_reason": "tool_calls",
        "outcome": "normal",
        "model": "model",
        "provider": "provider"
    });
    let tool = serde_json::json!({
        "role": "tool_result",
        "tool_call_id": "call_read",
        "tool_name": "read",
        "content": "file body",
        "is_error": false,
        "timestamp_ms": 1_300
    });
    let assistant_final = serde_json::json!({
        "role": "assistant",
        "content": [
            {"type": "text", "text": "final answer after tool"}
        ],
        "timestamp_ms": 1_400,
        "finish_reason": "stop",
        "outcome": "normal",
        "model": "model",
        "provider": "provider"
    });
    for (seq, role, message, content_text, tool_call_id, tool_name, tool_calls_json) in [
        (1, "user", user, Some("hello export"), None, None, None),
        (
            2,
            "assistant",
            assistant,
            Some("visible answer"),
            None,
            None,
            Some(r#"[{"id":"call_read","name":"read"}]"#),
        ),
        (
            3,
            "tool_result",
            tool,
            Some("file body"),
            Some("call_read"),
            Some("read"),
            None,
        ),
        (
            4,
            "assistant",
            assistant_final,
            Some("final answer after tool"),
            None,
            None,
            None,
        ),
    ] {
        conn.execute(
            r#"
            INSERT INTO messages (
                session_id, session_seq, role, timestamp_ms, message_json,
                content_text, tool_call_id, tool_name, tool_calls_json
            ) VALUES (?1, ?2, ?3, ?2, ?4, ?5, ?6, ?7, ?8)
            "#,
            rusqlite::params![
                session_id,
                seq,
                role,
                message.to_string(),
                content_text,
                tool_call_id,
                tool_name,
                tool_calls_json
            ],
        )
        .expect("insert message");
    }
    let prompt_prefix_metadata = serde_json::json!({
        "prompt_prefix": {
            "hash": prompt_prefix_hash,
            "version": 1,
            "created_at_ms": 1_050,
            "provider": "provider",
            "model": "model",
            "tool_declarations_hash": "fixture-tools-hash",
            "invalidation_reason": "new_session",
            "effective_tools": [
                "read",
                "Agent",
                "list_agents",
                "wait_agent",
                "send_message",
                "close_agent",
                "resume_agent",
                "list_skills",
                "view_skill"
            ],
            "agent_catalog_visible": true,
            "visible_agents": ["translate"],
            "skill_catalog_visible": true,
            "project_instructions_visible": true,
            "project_instructions_role": "system"
        }
    });
    conn.execute(
        "UPDATE messages SET metadata_json = ?1 WHERE session_id = ?2 AND role = 'user'",
        rusqlite::params![prompt_prefix_metadata.to_string(), session_id],
    )
    .expect("set user prompt prefix metadata");
    insert_export_fixture_prompt_prefix(conn, session_id, prompt_prefix_hash);
    for (context_seq, source_kind, source_name, source_path, provider_group, block_index, context_kind, content_text, metadata_json) in [
        (
            1,
            "project_instruction",
            "AGENTS.md",
            "/repo/AGENTS.md",
            "project_instructions",
            0,
            "project_instruction",
            "# AGENTS.md instructions for /repo\n\n<INSTRUCTIONS>\nhidden AGENTS root\n</INSTRUCTIONS>",
            r#"{"included_bytes":18,"directory":"/repo"}"#,
        ),
        (
            2,
            "project_instruction",
            "AGENTS.local.md",
            "/repo/AGENTS.local.md",
            "project_instructions",
            1,
            "project_instruction",
            "# AGENTS.md instructions for /repo\n\n<INSTRUCTIONS>\nhidden AGENTS local\n</INSTRUCTIONS>",
            r#"{"included_bytes":19,"directory":"/repo"}"#,
        ),
        (
            3,
            "selected_skill",
            "reviewer",
            "/skills/reviewer/SKILL.md",
            "selected_skill:0:reviewer",
            0,
            "selected_skill",
            "<skill>\n<name>reviewer</name>\nskill body\n</skill>",
            r#"{"base_dir":"/skills/reviewer"}"#,
        ),
    ] {
        conn.execute(
            r#"
            INSERT INTO context_evidence (
                session_id, prompt_session_seq, context_seq, role, source_kind,
                source_name, source_path, provider_group, provider_block_index,
                context_kind, timestamp_ms, content_text, metadata_json
            ) VALUES (?1, 1, ?2, 'user', ?3, ?4, ?5, ?6, ?7, ?8, 1100, ?9, ?10)
            "#,
            rusqlite::params![
                session_id,
                context_seq,
                source_kind,
                source_name,
                source_path,
                provider_group,
                block_index,
                context_kind,
                content_text,
                metadata_json
            ],
        )
        .expect("insert context evidence");
    }
}

fn insert_export_fixture_prompt_prefix(conn: &Connection, session_id: &str, prefix_hash: &str) {
    let slots = serde_json::json!([
        {
            "slot": "base/mode",
            "tier": "base",
            "semantic_role": "base_policy",
            "provider_role": "system",
            "order": 0,
            "content": "Runtime mode: default from prefix snapshot.",
            "content_hash": "base-hash",
            "source_kind": "runtime",
            "source_name": "mode",
            "source_path": null
        },
        {
            "slot": "agent_catalog",
            "tier": "prefix",
            "semantic_role": "developer_prompt",
            "provider_role": "system",
            "order": 1,
            "content": "Available agents:\n- translate: Translate between Chinese and English.",
            "content_hash": "agent-catalog-hash",
            "source_kind": "agent_catalog",
            "source_name": "active_agents",
            "source_path": null
        },
        {
            "slot": "skill_index",
            "tier": "prefix",
            "semantic_role": "developer_prompt",
            "provider_role": "system",
            "order": 2,
            "content": "Available skills:\n- reviewer: Review text carefully.",
            "content_hash": "skill-index-hash",
            "source_kind": "skill_catalog",
            "source_name": "active_skills",
            "source_path": null
        },
        {
            "slot": "project_context:0",
            "tier": "prefix",
            "semantic_role": "developer_prompt",
            "provider_role": "system",
            "order": 3,
            "content": "Project instructions below are policy context, not user task content.\n\n# AGENTS.md instructions for /repo\n\n<INSTRUCTIONS>\nhidden AGENTS root\n</INSTRUCTIONS>",
            "content_hash": "project-root-hash",
            "source_kind": "project_instruction",
            "source_name": "AGENTS.md",
            "source_path": "/repo/AGENTS.md"
        },
        {
            "slot": "project_context:1",
            "tier": "prefix",
            "semantic_role": "developer_prompt",
            "provider_role": "system",
            "order": 4,
            "content": "Project instructions below are policy context, not user task content.\n\n# AGENTS.md instructions for /repo\n\n<INSTRUCTIONS>\nhidden AGENTS local\n</INSTRUCTIONS>",
            "content_hash": "project-local-hash",
            "source_kind": "project_instruction",
            "source_name": "AGENTS.local.md",
            "source_path": "/repo/AGENTS.local.md"
        }
    ]);
    let metadata = serde_json::json!({
        "mode": "default",
        "selected_agent": null,
        "agents_enabled": true,
        "effective_tools": [
            "read",
            "Agent",
            "list_agents",
            "wait_agent",
            "send_message",
            "close_agent",
            "resume_agent",
            "list_skills",
            "view_skill"
        ],
        "agent_catalog_visible": true,
        "visible_agents": ["translate"],
        "skill_catalog_visible": true,
        "project_instructions_visible": true,
        "project_instructions_role": "system"
    });
    conn.execute(
        r#"
        INSERT INTO session_prompt_prefixes (
            session_id, version, created_at_ms, provider, model,
            prefix_hash, tool_declarations_hash, invalidation_reason,
            slots_json, metadata_json
        ) VALUES (?1, 1, 1050, 'provider', 'model', ?2, 'fixture-tools-hash',
            'new_session', ?3, ?4)
        "#,
        rusqlite::params![session_id, prefix_hash, slots.to_string(), metadata.to_string()],
    )
    .expect("insert prompt prefix");
}

fn set_export_fixture_session_metadata(conn: &Connection, session_id: &str) {
    let metadata = serde_json::json!({
        "base_url": "https://example.test/v1",
        "mode": "default",
        "reasoning_effort": "medium",
        "model_metadata": {
            "capabilities": {
                "reasoning": true,
                "tool_call": true
            }
        }
    });
    conn.execute(
        "UPDATE sessions SET metadata_json = ?1 WHERE id = ?2",
        rusqlite::params![metadata.to_string(), session_id],
    )
    .expect("set session metadata");
}

fn insert_session(
    conn: &Connection,
    id: &str,
    workdir: &Path,
    source: &str,
    started_at_ms: i64,
    updated_at_ms: i64,
) {
    conn.execute(
        r#"
        INSERT INTO sessions (
            id, source, parent_session_id, workdir, model, provider,
            started_at_ms, updated_at_ms, ended_at_ms, end_reason, archived_at_ms,
            message_count, tool_call_count, title, metadata_json
        ) VALUES (?1, ?2, NULL, ?3, 'model', 'provider',
            ?4, ?5, NULL, NULL, NULL, 0, 0, NULL, NULL)
        "#,
        rusqlite::params![
            id,
            source,
            workdir.to_string_lossy(),
            started_at_ms,
            updated_at_ms
        ],
    )
    .expect("insert session");
}

struct CatalogJsonServer {
    base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl CatalogJsonServer {
    fn start(body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let base_url = format!("http://{}", listener.local_addr().expect("addr"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_thread = Arc::clone(&requests);
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let request = read_http_request(&mut stream);
            requests_for_thread.lock().expect("requests").push(request);
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).expect("response");
        });
        Self { base_url, requests }
    }
}
