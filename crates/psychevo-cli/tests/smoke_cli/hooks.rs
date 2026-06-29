#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn hooks_cmd(test_home: &Path, psychevo_home: &Path, cwd: &Path) -> Command {
    let mut command = pevo_cmd(test_home);
    command.env("PSYCHEVO_HOME", psychevo_home).current_dir(cwd);
    command
}

#[test]
pub(crate) fn cli_hooks_list_trust_disable_and_enable_profile_state() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(cwd.join(".psychevo")).expect("project config");
    init_skill_home(temp.path(), &psychevo_home);
    std::fs::write(cwd.join(".psychevo/config.toml"), "\n").expect("config");
    std::fs::write(
        cwd.join(".psychevo/hooks.json"),
        r#"{
          "hooks": {
            "PreToolUse": [
              {
                "matcher": "Bash",
                "hooks": [
                  {"type": "command", "command": "echo hook"}
                ]
              }
            ]
          }
        }"#,
    )
    .expect("hooks");

    let list = hooks_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["hooks", "list", "--json"])
        .output()
        .expect("hooks list");
    assert!(
        list.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&list.stderr)
    );
    let listed: Value = serde_json::from_slice(&list.stdout).expect("list json");
    let hook = &listed["hooks"][0];
    assert_eq!(hook["event"], "PreToolUse");
    assert_eq!(hook["source_kind"], "project");
    assert_eq!(hook["trust_status"], "untrusted");
    let key = hook["key"].as_str().expect("key").to_string();

    let trust = hooks_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["hooks", "trust", &key, "--json"])
        .output()
        .expect("hooks trust");
    assert!(
        trust.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&trust.stderr)
    );
    let trusted: Value = serde_json::from_slice(&trust.stdout).expect("trust json");
    assert_eq!(trusted["hook"], key);

    let disable = hooks_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["hooks", "disable", &key, "--json"])
        .output()
        .expect("hooks disable");
    assert!(
        disable.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&disable.stderr)
    );
    let config = std::fs::read_to_string(psychevo_home.join("config.toml")).expect("config");
    assert!(config.contains("trusted_hash"));
    assert!(config.contains("enabled = false"));

    let enable = hooks_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["hooks", "enable", &key, "--json"])
        .output()
        .expect("hooks enable");
    assert!(
        enable.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&enable.stderr)
    );
    let config = std::fs::read_to_string(psychevo_home.join("config.toml")).expect("config");
    assert!(config.contains("enabled = true"));
}
