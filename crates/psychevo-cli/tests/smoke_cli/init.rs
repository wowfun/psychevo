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
    assert!(stdout.contains("Open the managed local Web UI"));
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
            "Open the managed local Web UI",
            "--no-browser",
            "--print-url",
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
pub(crate) fn cli_doctor_json_reports_local_web_asset_status() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let workdir = temp.path().join("work");
    let dist = temp.path().join("dist");
    std::fs::create_dir_all(&workdir).expect("workdir");
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
        .current_dir(&workdir)
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
pub(crate) fn cli_web_opens_current_workdir_with_json_output() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let workdir = temp.path().join("work");
    let dist = temp.path().join("dist");
    std::fs::create_dir_all(&workdir).expect("workdir");
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
        .current_dir(&workdir)
        .args(["web", "--no-browser", "--print-url"])
        .output()
        .expect("pevo web");
    let stop = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .current_dir(&workdir)
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
    assert_eq!(value["workdir"], workdir.display().to_string());
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
    assert!(config.contains("model = \"deepseek/deepseek-chat\""));
    assert!(config.contains("api_key_env = \"DEEPSEEK_API_KEY\""));
    let env_template = std::fs::read_to_string(home.join(".env")).expect("env");
    assert!(env_template.contains("DEEPSEEK_API_KEY=sk-..."));
    assert!(!stdout.contains("sk-"));

    let conn = Connection::open(home.join("state.db")).expect("db");
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version");
    assert_eq!(user_version, 20);

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
    assert_eq!(user_version, 20);
}
