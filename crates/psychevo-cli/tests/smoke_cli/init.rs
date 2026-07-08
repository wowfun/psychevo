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
            "--new",
            "leading ! runs a local shell escape",
        ],
    );
    assert_help_contains(
        temp.path(),
        &["web", "--help"],
        &[
            "start, stop, and restart the managed Web server",
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
            "--dir <DIR>",
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
pub(crate) fn cli_web_opens_current_cwd_with_json_output() {
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
        .args(["web", "--no-browser", "--print-url"])
        .output()
        .expect("pevo web");
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
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("web json");
    assert_eq!(value["ok"], true);
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
    assert_eq!(user_version, 24);

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
    assert_eq!(user_version, 24);
}
