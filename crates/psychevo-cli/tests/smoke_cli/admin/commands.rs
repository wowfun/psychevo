#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn admin_cmd(test_home: &Path, psychevo_home: &Path, cwd: &Path) -> Command {
    let mut command = pevo_cmd(test_home);
    command.env("PSYCHEVO_HOME", psychevo_home).current_dir(cwd);
    command
}

pub(crate) fn run_with_stdin(mut command: Command, input: &str) -> std::process::Output {
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

struct MockJsonServer {
    base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl MockJsonServer {
    fn start(responses: Vec<Value>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let base_url = format!("http://{}", listener.local_addr().expect("addr"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_thread = Arc::clone(&requests);
        thread::spawn(move || {
            let mut responses = VecDeque::from(responses);
            while let Some(body) = responses.pop_front() {
                let (mut stream, _) = listener.accept().expect("accept");
                let request = read_http_request(&mut stream);
                requests_for_thread.lock().expect("requests").push(request);
                let body = body.to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).expect("response");
            }
        });
        Self { base_url, requests }
    }
}

#[test]
pub(crate) fn cli_rejects_obsolete_plural_skills_command() {
    let temp = tempdir().expect("temp");
    let output = pevo_cmd(temp.path())
        .args(["skills", "list"])
        .output()
        .expect("pevo skills");
    assert!(!output.status.success());
}

#[test]
pub(crate) fn cli_config_provider_and_auth_write_scoped_env_without_leaking_secret() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&cwd).expect("cwd");
    init_skill_home(temp.path(), &psychevo_home);

    let add_local = admin_cmd(temp.path(), &psychevo_home, &cwd)
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

    let local_config = std::fs::read_to_string(cwd.join(".psychevo/config.toml")).expect("config");
    assert!(local_config.contains("MOCK_LOCAL_KEY"));
    assert!(!local_config.contains("secret-key"));

    let mut set_cmd = admin_cmd(temp.path(), &psychevo_home, &cwd);
    set_cmd.args(["auth", "set", "mock-local", "--api-key-stdin", "--json"]);
    let set = run_with_stdin(set_cmd, "secret-key\n");
    assert!(
        set.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&set.stderr)
    );
    assert!(!String::from_utf8_lossy(&set.stdout).contains("secret-key"));
    assert!(!String::from_utf8_lossy(&set.stderr).contains("secret-key"));
    let local_env = std::fs::read_to_string(cwd.join(".psychevo/.env")).expect("env");
    assert_eq!(local_env, "MOCK_LOCAL_KEY=secret-key\n");

    let status = admin_cmd(temp.path(), &psychevo_home, &cwd)
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

    let mut add_global_cmd = admin_cmd(temp.path(), &psychevo_home, &cwd);
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
        "-g",
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
pub(crate) fn cli_gateway_setup_channels_are_secret_free_and_old_command_is_removed() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&cwd).expect("cwd");
    init_skill_home(temp.path(), &psychevo_home);

    let old = admin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["channel", "list"])
        .output()
        .expect("old channel command");
    assert!(!old.status.success());

    let mut setup_cmd = admin_cmd(temp.path(), &psychevo_home, &cwd);
    setup_cmd.args([
        "gateway",
        "setup",
        "--channel",
        "telegram",
        "--id",
        "release",
        "--label",
        "Release Bot",
        "--allow-user",
        "12345",
        "--enable",
        "--credential-stdin",
        "--json",
    ]);
    let setup = run_with_stdin(setup_cmd, "telegram-secret\n");
    assert!(
        setup.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&setup.stderr)
    );
    assert!(!String::from_utf8_lossy(&setup.stdout).contains("telegram-secret"));
    assert!(!String::from_utf8_lossy(&setup.stderr).contains("telegram-secret"));
    let value: Value = serde_json::from_slice(&setup.stdout).expect("json");
    assert_eq!(value["channel"]["id"], "release");
    assert_eq!(value["channel"]["channel"], "telegram");
    assert_eq!(value["channel"]["credential_env"], "TELEGRAM_BOT_TOKEN");
    assert_eq!(value["channel"]["wrote_credential"], true);
    assert_eq!(value["enabled"]["enabled"], true);
    assert_eq!(value["summary"]["configured"], 1);
    assert_eq!(value["summary"]["enabled"], 1);

    let config = std::fs::read_to_string(psychevo_home.join("config.toml")).expect("config");
    assert!(config.contains("[[channels.connections]]"));
    assert!(config.contains("channel = \"telegram\""));
    assert!(!config.contains("platform = \"telegram\""));
    assert!(config.contains("Release Bot"));
    assert!(config.contains("TELEGRAM_BOT_TOKEN"));
    assert!(!config.contains("telegram-secret"));
    let env = std::fs::read_to_string(psychevo_home.join(".env")).expect("env");
    assert!(env.contains("TELEGRAM_BOT_TOKEN=telegram-secret"));

    let status = admin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["gateway", "status", "--json"])
        .output()
        .expect("gateway status");
    assert!(
        status.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    let value: Value = serde_json::from_slice(&status.stdout).expect("json");
    assert_eq!(value["channels"]["configured"], 1);
    assert_eq!(value["channels"]["enabled"], 1);
    assert_eq!(value["channels"]["ready"], 1);
    assert_eq!(value["channels"]["blocked"], 0);
    assert!(!String::from_utf8_lossy(&status.stdout).contains("telegram-secret"));
}

#[test]
pub(crate) fn cli_gateway_setup_wechat_qr_writes_env_backed_config_without_leaking_secret() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&cwd).expect("cwd");
    init_skill_home(temp.path(), &psychevo_home);
    let server = MockJsonServer::start(vec![
        serde_json::json!({
            "qrcode": "qr-token",
            "qrcode_img_content": "https://qr.example/wechat"
        }),
        serde_json::json!({
            "status": "confirmed",
            "ilink_bot_id": "wx-account",
            "bot_token": "wechat-secret",
            "baseurl": "http://ilink.example",
            "ilink_user_id": "wx-user"
        }),
    ]);

    let setup = admin_cmd(temp.path(), &psychevo_home, &cwd)
        .args([
            "gateway",
            "setup",
            "--channel",
            "wechat",
            "--qr",
            "--ilink-base-url",
            &server.base_url,
            "--enable",
            "--json",
        ])
        .output()
        .expect("wechat qr setup");
    assert!(
        setup.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&setup.stderr)
    );
    assert!(!String::from_utf8_lossy(&setup.stdout).contains("wechat-secret"));
    assert!(!String::from_utf8_lossy(&setup.stderr).contains("wechat-secret"));
    let value: Value = serde_json::from_slice(&setup.stdout).expect("json");
    assert_eq!(value["channel"]["channel"], "wechat");
    assert_eq!(value["channel"]["credential_env"], "WECHAT_BOT_TOKEN");
    assert_eq!(value["channel"]["account_env"], "WECHAT_ACCOUNT_ID");
    assert_eq!(value["channel"]["base_url_env"], "WECHAT_ILINK_BASE_URL");
    assert_eq!(value["enabled"]["enabled"], true);
    assert_eq!(value["doctor"]["channels"][0]["runtime_status"], "ready");

    let config = std::fs::read_to_string(psychevo_home.join("config.toml")).expect("config");
    assert!(config.contains("channel = \"wechat\""));
    assert!(config.contains("credential_env = \"WECHAT_BOT_TOKEN\""));
    assert!(config.contains("account_env = \"WECHAT_ACCOUNT_ID\""));
    assert!(config.contains("base_url_env = \"WECHAT_ILINK_BASE_URL\""));
    assert!(config.contains("allow_users = [\"wx-user\"]"));
    assert!(!config.contains("wechat-secret"));
    let env = std::fs::read_to_string(psychevo_home.join(".env")).expect("env");
    assert!(env.contains("WECHAT_BOT_TOKEN=wechat-secret"));
    assert!(env.contains("WECHAT_ACCOUNT_ID=wx-account"));
    assert!(env.contains("WECHAT_ILINK_BASE_URL=http://ilink.example"));

    let requests = server.requests.lock().expect("requests");
    assert!(requests[0].starts_with("GET /ilink/bot/get_bot_qrcode?bot_type=3 HTTP/1.1"));
    assert!(requests[1].starts_with("GET /ilink/bot/get_qrcode_status?qrcode=qr-token HTTP/1.1"));
}

#[test]
pub(crate) fn cli_config_permissions_lists_and_removes_project_local_rules() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(cwd.join(".psychevo")).expect("cwd");
    init_skill_home(temp.path(), &psychevo_home);
    std::fs::write(
        cwd.join(".psychevo/config.toml"),
        r#"# project-local policy

default_permissions = "local"

[permissions.local]
extends = ":workspace"

[permissions.local.filesystem]
".env" = "deny"

[[exec_policy.rules]]
prefix = ["npm", "test"]
decision = "allow"

[[exec_policy.rules]]
prefix = ["cargo", "publish"]
decision = "prompt"
"#,
    )
    .expect("config");

    let listed = admin_cmd(temp.path(), &psychevo_home, &cwd)
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
    assert_eq!(value["permissions"]["default_permissions"], "local");
    assert_eq!(
        value["permissions"]["profiles"]["local"]["filesystem"][".env"],
        "deny"
    );
    assert_eq!(
        value["permissions"]["exec_policy"]["rules"][0]["prefix"],
        serde_json::json!(["npm", "test"])
    );
    assert_eq!(
        value["permissions"]["exec_policy"]["rules"][1]["decision"],
        "prompt"
    );

    let removed = admin_cmd(temp.path(), &psychevo_home, &cwd)
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
    assert!(!removed.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&removed.stdout),
        String::from_utf8_lossy(&removed.stderr)
    );
    assert!(
        combined.contains("[[exec_policy.rules]]"),
        "output: {combined}"
    );
    let config = std::fs::read_to_string(cwd.join(".psychevo/config.toml")).expect("config");
    assert!(config.contains("\"npm\""));
    assert!(config.contains("\"cargo\""));
}

#[test]
pub(crate) fn cli_tool_commands_list_and_toggle_profile_toolsets() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&cwd).expect("cwd");
    init_skill_home(temp.path(), &psychevo_home);

    let listed = admin_cmd(temp.path(), &psychevo_home, &cwd)
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

    let disabled = admin_cmd(temp.path(), &psychevo_home, &cwd)
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

    let listed = admin_cmd(temp.path(), &psychevo_home, &cwd)
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

    let created = admin_cmd(temp.path(), &psychevo_home, &cwd)
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

    let enabled = admin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["tool", "enable", "docs", "--mode", "plan", "--json"])
        .output()
        .expect("tool enable");
    assert!(
        enabled.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&enabled.stderr)
    );
    let listed = admin_cmd(temp.path(), &psychevo_home, &cwd)
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

    let removed = admin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["tool", "remove", "docs", "--json"])
        .output()
        .expect("tool remove");
    assert!(
        removed.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&removed.stderr)
    );
    let config = std::fs::read_to_string(psychevo_home.join("config.toml")).expect("config");
    assert!(!config.contains("[toolsets.docs]"));

    let local_created = admin_cmd(temp.path(), &psychevo_home, &cwd)
        .args([
            "tool",
            "create",
            "localdocs",
            "--tool",
            "web_fetch",
            "--local",
            "--json",
        ])
        .output()
        .expect("tool create local");
    assert!(
        local_created.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&local_created.stderr)
    );
    let local_config =
        std::fs::read_to_string(cwd.join(".psychevo/config.toml")).expect("local config");
    assert!(local_config.contains("[toolsets.localdocs]"));

    let removed_global_flag = admin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["tool", "disable", "web", "--global"])
        .output()
        .expect("tool global rejected");
    assert!(!removed_global_flag.status.success());
    let stderr = String::from_utf8_lossy(&removed_global_flag.stderr);
    assert!(
        stderr.contains("unexpected argument '--global'"),
        "stderr: {stderr}"
    );
}

#[test]
pub(crate) fn cli_session_commands_manage_active_and_archived_sessions() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&cwd).expect("cwd");
    init_skill_home(temp.path(), &psychevo_home);
    let canonical = cwd.canonicalize().expect("canonical");
    let db = psychevo_home.join("state.db");
    let conn = Connection::open(&db).expect("db");
    insert_session(&conn, "older", &canonical, "run", 1_000, 1_000);
    insert_session(&conn, "newer", &canonical, "tui", 2_000, 2_000);

    let list = admin_cmd(temp.path(), &psychevo_home, &cwd)
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

    let rename = admin_cmd(temp.path(), &psychevo_home, &cwd)
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

    let archive = admin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["session", "archive", "latest", "--json"])
        .output()
        .expect("session archive");
    assert!(archive.status.success());

    let archived = admin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["session", "list", "--archived", "--json"])
        .output()
        .expect("session archived list");
    let value: Value = serde_json::from_slice(&archived.stdout).expect("json");
    assert_eq!(value["sessions"][0]["id"], "newer");

    let restore = admin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["session", "restore", "newer", "--json"])
        .output()
        .expect("session restore");
    assert!(restore.status.success());
}

#[test]
pub(crate) fn cli_session_export_and_share_emit_local_artifacts() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&cwd).expect("cwd");
    init_skill_home(temp.path(), &psychevo_home);
    let canonical = cwd.canonicalize().expect("canonical");
    let db = psychevo_home.join("state.db");
    let conn = Connection::open(&db).expect("db");
    insert_session(&conn, "exported-session", &canonical, "tui", 1_000, 2_000);
    set_export_fixture_session_metadata(&conn, "exported-session");
    insert_export_fixture_messages(&conn, "exported-session");

    let default_export = admin_cmd(temp.path(), &psychevo_home, &cwd)
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

    let reasoning_export = admin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["session", "export", "latest", "--include", "reasoning"])
        .output()
        .expect("session export reasoning");
    assert!(reasoning_export.status.success());
    let stdout = String::from_utf8(reasoning_export.stdout).expect("stdout");
    assert!(stdout.contains("private plan"));
    assert!(!stdout.contains("provider_evidence"));

    let full_inputs_export = admin_cmd(temp.path(), &psychevo_home, &cwd)
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

    let json_export = admin_cmd(temp.path(), &psychevo_home, &cwd)
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

    let json_header = admin_cmd(temp.path(), &psychevo_home, &cwd)
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
    assert_eq!(
        value["header"]["options"]["include"],
        serde_json::json!(["header"])
    );
    assert_eq!(
        value["header"]["prompt_prefix"]["prefix_hash"],
        "fixture-prefix-hash"
    );
    assert_eq!(
        value["header"]["prompt_prefix"]["metadata"]["effective_tools"],
        serde_json::json!([
            "read",
            "spawn_agent",
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
    assert!(
        value["header"]["prompt_prefix"]["slots"][1]
            .get("content")
            .is_none()
    );
    assert!(value.get("prompt_prefix").is_none());
    assert!(value.get("messages").is_none());

    let markdown_header = admin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["session", "export", "latest", "--include", "header"])
        .output()
        .expect("session export markdown header");
    assert!(markdown_header.status.success());
    let stdout = String::from_utf8(markdown_header.stdout).expect("stdout");
    assert!(stdout.contains("### Prompt Prefix"));
    assert!(!stdout.contains("\n## Prompt Prefix"));
    assert!(stdout.contains("agent_catalog"));
    assert!(!stdout.contains("Available agents"));

    let json_last_request = admin_cmd(temp.path(), &psychevo_home, &cwd)
        .args([
            "session", "export", "latest", "--format", "json", "-i", "lpr",
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
    assert!(
        message_texts
            .iter()
            .any(|text| text.contains("hidden AGENTS local"))
    );
    assert!(
        last["body"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .any(|tool| tool["function"]["name"] == "read")
    );
    assert!(
        !last["body"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .any(|tool| tool["function"]["name"] == "write")
    );
    assert!(
        last["body"]["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .any(|message| message["role"] == "tool" && message["content"] == "file body")
    );

    let markdown_last_request = admin_cmd(temp.path(), &psychevo_home, &cwd)
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
    let stale_last_request = admin_cmd(temp.path(), &psychevo_home, &cwd)
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
    assert!(warnings.iter().any(|warning| {
        warning
            .as_str()
            .is_some_and(|text| text.contains("does not match") && text.contains("approximate"))
    }));

    let old_reasoning_export = admin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["session", "export", "latest", "--with-reasoning"])
        .output()
        .expect("session export old reasoning");
    assert!(!old_reasoning_export.status.success());

    let old_full_inputs_export = admin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["session", "export", "latest", "--full-inputs"])
        .output()
        .expect("session export old full inputs");
    assert!(!old_full_inputs_export.status.success());

    let raw_requests_export = admin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["session", "export", "latest", "--raw-requests"])
        .output()
        .expect("session export raw requests");
    assert!(!raw_requests_export.status.success());

    let share_last_request = admin_cmd(temp.path(), &psychevo_home, &cwd)
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

    let json_full_inputs = admin_cmd(temp.path(), &psychevo_home, &cwd)
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

    let share = admin_cmd(temp.path(), &psychevo_home, &cwd)
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
pub(crate) fn cli_model_list_current_and_fetch_use_local_config_and_explicit_fetch_only() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&cwd).expect("cwd");
    init_skill_home(temp.path(), &psychevo_home);
    let server = CatalogJsonServer::start(r#"{"data":[{"id":"mock-a"},{"id":"mock-b"}]}"#);
    std::fs::write(psychevo_home.join(".env"), "MOCK_API_KEY=test-key\n").expect("env");
    std::fs::write(
        psychevo_home.join("config.toml"),
        format!(
            r#"model = "mock/mock-a"

[provider.mock]
name = "Mock"
api = "{}/v1"

[provider.mock.models."mock-a"]
"#,
            server.base_url
        ),
    )
    .expect("config");

    let list = admin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["model", "list", "--json"])
        .output()
        .expect("model list");
    assert!(list.status.success());
    let value: Value = serde_json::from_slice(&list.stdout).expect("json");
    assert_eq!(value["models"][0]["provider"], "mock");
    assert_eq!(server.requests.lock().expect("requests").len(), 0);

    let current = admin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["model", "current", "--json"])
        .output()
        .expect("model current");
    assert!(current.status.success());
    let value: Value = serde_json::from_slice(&current.stdout).expect("json");
    assert_eq!(value["model"]["model"], "mock-a");
    assert_eq!(server.requests.lock().expect("requests").len(), 0);

    let set_local = admin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["model", "set", "mock/mock-b", "--json"])
        .output()
        .expect("model set local");
    assert!(
        set_local.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&set_local.stderr)
    );
    let value: Value = serde_json::from_slice(&set_local.stdout).expect("json");
    assert_eq!(value["scope"], "local");
    assert_eq!(value["model"], "mock/mock-b");
    let local_config =
        std::fs::read_to_string(cwd.join(".psychevo/config.toml")).expect("local config");
    assert!(local_config.contains("model = \"mock/mock-b\""));
    assert_eq!(server.requests.lock().expect("requests").len(), 0);

    let set_global = admin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["model", "set", "-g", "mock/mock-a", "--json"])
        .output()
        .expect("model set global");
    assert!(
        set_global.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&set_global.stderr)
    );
    let value: Value = serde_json::from_slice(&set_global.stdout).expect("json");
    assert_eq!(value["scope"], "global");
    let global_config = std::fs::read_to_string(psychevo_home.join("config.toml")).expect("config");
    assert!(global_config.contains("model = \"mock/mock-a\""));

    let invalid = admin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["model", "set", "mock-a", "--json"])
        .output()
        .expect("model set invalid");
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stdout).contains("provider/model"));

    let fetch = admin_cmd(temp.path(), &psychevo_home, &cwd)
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
    let cache = std::fs::read_to_string(psychevo_home.join("cache/provider_models_cache.json"))
        .expect("provider models cache");
    assert!(cache.contains("mock-a"));
    assert!(!cache.contains("test-key"));
}
