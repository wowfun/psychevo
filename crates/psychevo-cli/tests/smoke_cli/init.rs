#[allow(unused_imports)]
pub(crate) use super::*;
#[test]
pub(crate) fn cli_help_lists_aligned_command_descriptions() {
    let temp = tempdir().expect("temp");
    let output = pevo_cmd(temp.path())
        .arg("--help")
        .output()
        .expect("pevo help");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("init"));
    assert!(stdout.contains("Create or repair the active Psychevo profile home"));
    assert!(stdout.contains("--profile <NAME>"));
    assert!(stdout.contains("--cd <DIR>"));
    assert!(stdout.contains("profile"));
    assert!(stdout.contains("List, inspect, create, switch, and manage local profiles"));
    assert!(stdout.contains("Run one coding-agent turn"));
    assert!(stdout.contains("Open the fullscreen terminal UI"));
    assert!(stdout.contains("Open or manage the managed local Web UI"));
    assert!(stdout.contains("Open the native Desktop app from a source checkout"));
    assert!(stdout.contains("Run local deterministic diagnostics"));
    assert!(stdout.contains("Inspect local context-window usage for a session"));
}

#[test]
pub(crate) fn cli_help_describes_representative_commands_and_flags() {
    let temp = tempdir().expect("temp");
    assert_help_contains(
        temp.path(),
        &["run", "--help"],
        &[
            "Run one coding-agent turn through the configured provider",
            "--dir <DIR>",
            "--skill <NAME_OR_PATH>",
            "Disable default and configured skill discovery",
            "NDJSON machine output",
        ],
    );
    assert_help_contains(
        temp.path(),
        &["tui", "--help"],
        &[
            "Open the fullscreen terminal UI",
            "--cd <DIR>",
            "--new",
            "leading ! runs a local shell escape",
        ],
    );
    assert_help_contains(
        temp.path(),
        &["web", "--help"],
        &[
            "start, stop, and restart the managed Web server",
            "--cd <DIR>",
            "--no-browser",
            "--print-url",
            "start",
            "restart",
        ],
    );
    assert_help_contains(
        temp.path(),
        &["desktop", "--help"],
        &[
            "Open the native Desktop app from a Psychevo source checkout",
            "--cd <DIR>",
        ],
    );
    assert_help_contains(
        temp.path(),
        &["doctor", "--help"],
        &["Run local diagnostics", "--json", "--live"],
    );
    assert_help_contains(
        temp.path(),
        &["setup", "--help"],
        &["setup wizard", "--dry-run"],
    );
    assert_help_contains(
        temp.path(),
        &["session", "--help"],
        &[
            "List, inspect, rename, archive, restore, export, or share local sessions",
            "export",
            "share",
        ],
    );
    assert_help_contains(
        temp.path(),
        &["session", "export", "--help"],
        &[
            "without contacting providers",
            "last-provider-request",
            "hidden prompts",
            "--include <LIST>",
        ],
    );
    assert_help_contains(
        temp.path(),
        &["skill", "--help"],
        &[
            "List discoverable skills",
            "Install one or more skills from a source path",
        ],
    );
    assert_help_contains(
        temp.path(),
        &["profile", "create", "--help"],
        &[
            "Create a named Psychevo profile",
            "--clone",
            "--clone-from <NAME>",
            "--alias",
        ],
    );
    assert_help_contains(
        temp.path(),
        &["model", "fetch", "--help"],
        &[
            "provider /models endpoints",
            "contacts providers",
            "[PROVIDER]",
        ],
    );
    assert_help_contains(
        temp.path(),
        &["config", "provider", "add", "--help"],
        &[
            "OpenAI-compatible provider",
            "--base-url <URL>",
            "API key from stdin",
            "selected .env",
        ],
    );
    assert_help_contains(
        temp.path(),
        &["auth", "set", "--help"],
        &[
            "Read a provider API key from stdin",
            "Raw API keys are never accepted",
            "--api-key-stdin",
        ],
    );
}

#[test]
pub(crate) fn cli_default_command_rejects_non_tty_without_consuming_stdin() {
    let temp = tempdir().expect("temp");
    let output = pevo_cmd(temp.path())
        .stdin(Stdio::piped())
        .output()
        .expect("pevo default");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("requires an interactive terminal"),
        "{stderr}"
    );
    assert!(stderr.contains("pevo run <prompt>"), "{stderr}");
}

#[test]
pub(crate) fn cli_setup_rejects_non_tty_without_prompting() {
    let temp = tempdir().expect("temp");
    let output = pevo_cmd(temp.path())
        .arg("setup")
        .output()
        .expect("pevo setup");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("pevo setup is interactive and requires a terminal"),
        "{stderr}"
    );
    assert!(stderr.contains("pevo auth setup"), "{stderr}");
}

#[test]
pub(crate) fn cli_doctor_json_reports_local_web_asset_status() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    let dist = temp.path().join("dist");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&dist).expect("dist");
    std::fs::write(dist.join("index.html"), "<html></html>").expect("index");

    let init = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .arg("init")
        .output()
        .expect("pevo init");
    assert!(
        init.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let output = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .env("PSYCHEVO_WEB_DIST", &dist)
        .current_dir(&cwd)
        .args(["doctor", "--json"])
        .output()
        .expect("pevo doctor");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("doctor json");
    assert_eq!(value["live"]["enabled"], false);
    assert_eq!(value["webAssets"]["ok"], true);
    assert_eq!(value["webAssets"]["source"], "env");
    assert_eq!(value["webAssets"]["path"], dist.display().to_string());
}

#[test]
pub(crate) fn cli_web_opens_root_cd_with_json_output() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    let dist = temp.path().join("dist");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&dist).expect("dist");
    std::fs::write(dist.join("index.html"), "<html></html>").expect("index");

    let init = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .arg("init")
        .output()
        .expect("pevo init");
    assert!(
        init.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let output = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .env("PSYCHEVO_WEB_DIST", &dist)
        .current_dir(temp.path())
        .args(["-C", "work", "web", "--no-browser", "--print-url"])
        .output()
        .expect("pevo web");
    let stop = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .current_dir(temp.path())
        .args(["gateway", "stop"])
        .output()
        .expect("pevo gateway stop");
    assert!(
        stop.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&stop.stderr)
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("web json");
    assert_eq!(value["ok"], true);
    assert!(
        value["instanceId"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert_eq!(value["openedBrowser"], false);
    assert_eq!(value["cwd"], cwd.display().to_string());
    assert!(
        value["openUrl"]
            .as_str()
            .unwrap_or("")
            .starts_with("http://"),
        "{value}"
    );
    assert_eq!(value["openUrlOneTime"], true);
    assert!(
        value["openUrlExpiresAtMs"].as_i64().unwrap_or_default() > 0,
        "{value}"
    );
}

#[test]
pub(crate) fn cli_web_replaces_free_lease_stale_state_instead_of_reusing_dead_url() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    let dist = temp.path().join("dist");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&dist).expect("dist");
    std::fs::write(dist.join("index.html"), "<html></html>").expect("index");
    assert!(
        pevo_cmd(temp.path())
            .env("PSYCHEVO_HOME", &psychevo_home)
            .arg("init")
            .status()
            .expect("init")
            .success()
    );
    let gateway = psychevo_home.join("gateway");
    std::fs::create_dir_all(&gateway).expect("gateway");
    std::fs::write(gateway.join("token"), "stale-token").expect("token");
    std::fs::write(
        gateway.join("server.json"),
        serde_json::to_vec(&json!({
            "instanceId": "stale-instance",
            "pid": u32::MAX,
            "baseUrl": "http://127.0.0.1:1",
            "readyzUrl": "http://127.0.0.1:1/readyz",
            "startedAtMs": 1,
            "version": "0.1.0",
            "executablePath": null,
            "executableModifiedMs": null,
            "executableSize": null,
            "executableInode": null,
            "staticDir": null
        }))
        .expect("state json"),
    )
    .expect("state");

    let output = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .env("PSYCHEVO_WEB_DIST", &dist)
        .current_dir(&cwd)
        .args(["web", "start", "--bind", "127.0.0.1:0"])
        .output()
        .expect("start");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("start json");
    assert_ne!(value["instanceId"], "stale-instance");
    assert_ne!(value["baseUrl"], "http://127.0.0.1:1");

    let stop = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .args(["gateway", "stop"])
        .output()
        .expect("stop");
    assert!(
        stop.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&stop.stderr)
    );
}

#[test]
pub(crate) fn cli_stop_fails_closed_when_lease_owner_cannot_match_recorded_process() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    assert!(
        pevo_cmd(temp.path())
            .env("PSYCHEVO_HOME", &psychevo_home)
            .arg("init")
            .status()
            .expect("init")
            .success()
    );
    let gateway = psychevo_home.join("gateway");
    std::fs::create_dir_all(&gateway).expect("gateway");
    let lease = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(gateway.join("instance.lock"))
        .expect("lease file");
    lease.lock().expect("hold lease");
    std::fs::write(gateway.join("token"), "preserve-token").expect("token");
    let state = serde_json::to_vec(&json!({
        "instanceId": "not-this-process",
        "pid": std::process::id(),
        "baseUrl": "http://127.0.0.1:1",
        "readyzUrl": "http://127.0.0.1:1/readyz",
        "startedAtMs": 1,
        "version": "0.1.0",
        "executablePath": null,
        "executableModifiedMs": null,
        "executableSize": null,
        "executableInode": null,
        "staticDir": null
    }))
    .expect("state json");
    std::fs::write(gateway.join("server.json"), &state).expect("state");

    let output = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .args(["gateway", "stop"])
        .output()
        .expect("stop");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("ownership cannot be proven"));
    assert_eq!(
        std::fs::read(gateway.join("server.json")).expect("preserved state"),
        state
    );
    assert_eq!(
        std::fs::read_to_string(gateway.join("token")).expect("preserved token"),
        "preserve-token"
    );
    drop(lease);
}

#[test]
pub(crate) fn concurrent_cli_web_calls_reuse_one_managed_instance() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    let dist = temp.path().join("dist");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&dist).expect("dist");
    std::fs::write(dist.join("index.html"), "<html></html>").expect("index");
    assert!(
        pevo_cmd(temp.path())
            .env("PSYCHEVO_HOME", &psychevo_home)
            .arg("init")
            .status()
            .expect("init")
            .success()
    );

    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let barrier = Arc::clone(&barrier);
        let test_home = temp.path().to_path_buf();
        let psychevo_home = psychevo_home.clone();
        let cwd = cwd.clone();
        let dist = dist.clone();
        workers.push(thread::spawn(move || {
            barrier.wait();
            pevo_cmd(&test_home)
                .env("PSYCHEVO_HOME", &psychevo_home)
                .env("PSYCHEVO_WEB_DIST", &dist)
                .current_dir(&cwd)
                .args(["web", "--no-browser"])
                .output()
                .expect("web")
        }));
    }
    barrier.wait();
    let outputs = workers
        .into_iter()
        .map(|worker| worker.join().expect("worker"))
        .collect::<Vec<_>>();
    for output in &outputs {
        assert!(
            output.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let first: Value = serde_json::from_slice(&outputs[0].stdout).expect("first json");
    let second: Value = serde_json::from_slice(&outputs[1].stdout).expect("second json");
    assert_eq!(first["pid"], second["pid"]);
    assert_eq!(first["instanceId"], second["instanceId"]);
    assert_eq!(first["baseUrl"], second["baseUrl"]);

    let stop = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .args(["gateway", "stop"])
        .output()
        .expect("stop");
    assert!(
        stop.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&stop.stderr)
    );
}

#[test]
pub(crate) fn cli_web_start_failure_surfaces_current_managed_log_output() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    let dist = temp.path().join("dist");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&dist).expect("dist");
    std::fs::write(dist.join("index.html"), "<html></html>").expect("index");

    let init = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .arg("init")
        .output()
        .expect("pevo init");
    assert!(
        init.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let conn = Connection::open(psychevo_home.join("state.db")).expect("state db");
    conn.pragma_update(None, "user_version", 99)
        .expect("unsupported schema version");
    drop(conn);
    let gateway_dir = psychevo_home.join("gateway");
    let log_path = gateway_dir.join("server.log");
    std::fs::create_dir_all(&gateway_dir).expect("gateway dir");
    std::fs::write(&log_path, "old launch sentinel\n").expect("old server log");

    let output = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .env("PSYCHEVO_WEB_DIST", &dist)
        .current_dir(&cwd)
        .args(["web", "start", "--bind", "127.0.0.1:0"])
        .output()
        .expect("pevo web start");

    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(
        stderr.contains("managed gateway did not become ready"),
        "{stderr}"
    );
    assert!(
        stderr.contains("state database schema version 99 is not supported"),
        "{stderr}"
    );
    assert!(stderr.contains(&log_path.display().to_string()), "{stderr}");
    assert!(!stderr.contains("old launch sentinel"), "{stderr}");
    assert!(!gateway_dir.join("server.json").exists());
    assert!(!gateway_dir.join("token").exists());
}

#[test]
pub(crate) fn cli_web_lifecycle_aliases_stop_and_restart_managed_server() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    let dist = temp.path().join("dist");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&dist).expect("dist");
    std::fs::write(dist.join("index.html"), "<html></html>").expect("index");

    let init = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .arg("init")
        .output()
        .expect("pevo init");
    assert!(
        init.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let initial_stop = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .current_dir(&cwd)
        .args(["web", "stop"])
        .output()
        .expect("pevo web stop");
    assert!(
        initial_stop.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&initial_stop.stderr)
    );
    let initial_stop_json: Value =
        serde_json::from_slice(&initial_stop.stdout).expect("initial stop json");
    assert_eq!(initial_stop_json["ok"], true);
    assert_eq!(initial_stop_json["stopped"], false);

    let restart = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .env("PSYCHEVO_WEB_DIST", &dist)
        .current_dir(&cwd)
        .args(["web", "restart"])
        .output()
        .expect("pevo web restart");
    assert!(
        restart.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&restart.stderr)
    );
    let restart_json: Value = serde_json::from_slice(&restart.stdout).expect("restart json");
    assert_eq!(restart_json["ok"], true);
    assert_eq!(restart_json["running"], true);
    assert_eq!(restart_json["restarted"], true);
    assert!(
        restart_json["baseUrl"]
            .as_str()
            .unwrap_or("")
            .starts_with("http://"),
        "{restart_json}"
    );

    let final_stop = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .current_dir(&cwd)
        .args(["web", "stop"])
        .output()
        .expect("pevo web stop after restart");
    assert!(
        final_stop.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&final_stop.stderr)
    );
    let final_stop_json: Value =
        serde_json::from_slice(&final_stop.stdout).expect("final stop json");
    assert_eq!(final_stop_json["ok"], true);
    assert_eq!(final_stop_json["stopped"], true);
    assert!(!psychevo_home.join("gateway/server.json").exists());

    let idempotent_stop = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .current_dir(&cwd)
        .args(["web", "stop"])
        .output()
        .expect("pevo web idempotent stop");
    assert!(
        idempotent_stop.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&idempotent_stop.stderr)
    );
    let idempotent_stop_json: Value =
        serde_json::from_slice(&idempotent_stop.stdout).expect("idempotent stop json");
    assert_eq!(idempotent_stop_json["ok"], true);
    assert_eq!(idempotent_stop_json["stopped"], false);
}

#[test]
pub(crate) fn cli_init_reset_state_stops_managed_gateway_before_recreating_state() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    let dist = temp.path().join("dist");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&dist).expect("dist");
    std::fs::write(dist.join("index.html"), "<html></html>").expect("index");

    let init = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .arg("init")
        .output()
        .expect("pevo init");
    assert!(
        init.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let first_web = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .env("PSYCHEVO_WEB_DIST", &dist)
        .current_dir(&cwd)
        .args(["web", "--no-browser", "--print-url"])
        .output()
        .expect("pevo web");
    assert!(
        first_web.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first_web.stderr)
    );
    let first: Value = serde_json::from_slice(&first_web.stdout).expect("first web json");
    let first_pid = first["pid"].as_u64().expect("first pid");
    let gateway_dir = psychevo_home.join("gateway");
    assert!(gateway_dir.join("server.json").exists());
    assert!(gateway_dir.join("token").exists());

    let reset = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .args(["init", "--reset-state"])
        .output()
        .expect("pevo init reset");
    assert!(
        reset.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&reset.stderr)
    );
    assert!(psychevo_home.join("state.db").exists());
    assert!(!gateway_dir.join("server.json").exists());
    assert!(!gateway_dir.join("token").exists());

    let backup_root = psychevo_home.join("backups");
    let backups = std::fs::read_dir(&backup_root)
        .expect("backups")
        .collect::<std::io::Result<Vec<_>>>()
        .expect("backup entries");
    assert_eq!(backups.len(), 1);
    assert!(backups[0].path().join("state.db").exists());

    let second_web = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .env("PSYCHEVO_WEB_DIST", &dist)
        .current_dir(&cwd)
        .args(["web", "--no-browser", "--print-url"])
        .output()
        .expect("pevo web after reset");
    let stop = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .current_dir(&cwd)
        .args(["gateway", "stop"])
        .output()
        .expect("pevo gateway stop");
    assert!(
        stop.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&stop.stderr)
    );
    assert!(
        second_web.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&second_web.stderr)
    );
    let second: Value = serde_json::from_slice(&second_web.stdout).expect("second web json");
    let second_pid = second["pid"].as_u64().expect("second pid");
    assert_ne!(first_pid, second_pid);
}

pub(crate) fn assert_help_contains(test_home: &Path, args: &[&str], expected: &[&str]) {
    let output = pevo_cmd(test_home).args(args).output().expect("pevo help");
    assert!(
        output.status.success(),
        "args: {args:?}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    for needle in expected {
        assert!(
            stdout.contains(needle),
            "args: {args:?}\nmissing: {needle}\nstdout:\n{stdout}"
        );
    }
}

#[test]
pub(crate) fn cli_init_creates_home_tree_and_is_idempotent() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let output = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &home)
        .arg("init")
        .output()
        .expect("pevo init");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains(&format!("home: {}", home.display())));
    assert!(home.join("config.toml").exists());
    assert!(home.join(".env").exists());
    assert!(home.join("state.db").exists());
    assert!(home.join("sessions").is_dir());
    assert!(home.join("logs").is_dir());
    assert!(home.join("cache").is_dir());

    let config = std::fs::read_to_string(home.join("config.toml")).expect("config");
    assert_starter_config_template(&config);
    let env_template = std::fs::read_to_string(home.join(".env")).expect("env");
    assert!(env_template.contains("DEEPSEEK_API_KEY=sk-..."));
    assert!(!stdout.contains("sk-"));

    let conn = Connection::open(home.join("state.db")).expect("db");
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version");
    assert_eq!(user_version, 28);

    std::fs::write(home.join("config.toml"), "custom config").expect("custom config");
    std::fs::write(home.join(".env"), "CUSTOM=1\n").expect("custom env");
    let rerun = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &home)
        .arg("init")
        .output()
        .expect("pevo init rerun");
    assert!(rerun.status.success());
    assert_eq!(
        std::fs::read_to_string(home.join("config.toml")).expect("config"),
        "custom config"
    );
    assert_eq!(
        std::fs::read_to_string(home.join(".env")).expect("env"),
        "CUSTOM=1\n"
    );
}

#[test]
pub(crate) fn cli_init_reset_state_backs_up_existing_sqlite_files() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let init = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &home)
        .arg("init")
        .output()
        .expect("pevo init");
    assert!(init.status.success());
    std::fs::write(home.join("state.db-wal"), "wal").expect("wal");
    std::fs::write(home.join("state.db-shm"), "shm").expect("shm");

    let reset = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &home)
        .args(["init", "--reset-state"])
        .output()
        .expect("pevo init reset");
    assert!(
        reset.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&reset.stderr)
    );
    let stdout = String::from_utf8(reset.stdout).expect("stdout");
    assert!(stdout.contains("state_backup:"));
    assert!(home.join("state.db").exists());
    assert!(!home.join("state.db-wal").exists());
    assert!(!home.join("state.db-shm").exists());

    let backup_root = home.join("backups");
    let backups = std::fs::read_dir(&backup_root)
        .expect("backups")
        .collect::<std::io::Result<Vec<_>>>()
        .expect("backup entries");
    assert_eq!(backups.len(), 1);
    let backup = backups[0].path();
    assert!(backup.join("state.db").exists());
    assert_eq!(
        std::fs::read_to_string(backup.join("state.db-wal")).expect("wal"),
        "wal"
    );
    assert_eq!(
        std::fs::read_to_string(backup.join("state.db-shm")).expect("shm"),
        "shm"
    );

    let conn = Connection::open(home.join("state.db")).expect("db");
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version");
    assert_eq!(user_version, 28);
}
