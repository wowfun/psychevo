use crate::pevo_cmd;
use crate::smoke_cli_skills::init_skill_home;
use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

pub(crate) fn plugin_cmd(test_home: &Path, psychevo_home: &Path, cwd: &Path) -> Command {
    let mut command = pevo_cmd(test_home);
    command.env("PSYCHEVO_HOME", psychevo_home).current_dir(cwd);
    command
}

pub(crate) fn write_cli_plugin(root: &Path) {
    std::fs::create_dir_all(root.join(".codex-plugin")).expect("manifest dir");
    std::fs::create_dir_all(root.join("skills/cleanup")).expect("skill dir");
    std::fs::write(
        root.join(".codex-plugin/plugin.json"),
        r#"{
          "name": "disk-cleanup",
          "version": "1.0.0",
          "description": "Track and clean temporary files",
          "skills": ["./skills"]
        }"#,
    )
    .expect("manifest");
    std::fs::write(
        root.join("skills/cleanup/SKILL.md"),
        "---\nname: cleanup\ndescription: \"Clean temporary files\"\n---\n\nUse cleanup_status before cleanup.\n",
    )
    .expect("skill");
}

fn write_display_plugin(root: &Path) {
    std::fs::create_dir_all(root.join(".codex-plugin")).expect("manifest dir");
    std::fs::create_dir_all(root.join("assets")).expect("assets");
    std::fs::write(root.join("assets/icon.png"), "icon").expect("icon");
    std::fs::write(
        root.join(".codex-plugin/plugin.json"),
        r#"{
          "name": "display-plugin",
          "version": "1.0.0",
          "description": "Display plugin",
          "interface": {
            "displayName": "Display Plugin",
            "shortDescription": "Adds display metadata.",
            "category": "productivity",
            "capabilities": ["tools", "hooks"],
            "composerIcon": "./assets/icon.png"
          }
        }"#,
    )
    .expect("manifest");
}

fn add_marketplace_plugin(
    test_home: &Path,
    psychevo_home: &Path,
    cwd: &Path,
    source: &Path,
    plugin_name: &str,
    json_output: bool,
) -> std::process::Output {
    std::fs::create_dir_all(source.join(".agents/plugins")).expect("marketplace dir");
    std::fs::write(
        source.join(".agents/plugins/marketplace.json"),
        format!(
            r#"{{
              "name": "test-marketplace",
              "plugins": [{{"name": "{plugin_name}", "source": "."}}]
            }}"#
        ),
    )
    .expect("marketplace manifest");
    let marketplace = plugin_cmd(test_home, psychevo_home, cwd)
        .args([
            "plugin",
            "marketplace",
            "add",
            source.to_str().expect("source"),
            "--json",
        ])
        .output()
        .expect("pevo plugin marketplace add");
    assert!(
        marketplace.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&marketplace.stderr)
    );
    let mut command = plugin_cmd(test_home, psychevo_home, cwd);
    command.args(["plugin", "add", &format!("{plugin_name}@test-marketplace")]);
    if json_output {
        command.arg("--json");
    }
    command.output().expect("pevo plugin add")
}

#[tokio::test]
pub(crate) async fn cli_plugin_view_human_output_includes_interface_summary() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    let source = temp.path().join("display-plugin-source");
    std::fs::create_dir_all(&cwd).expect("cwd");
    init_skill_home(temp.path(), &psychevo_home);
    write_display_plugin(&source);

    let install = add_marketplace_plugin(
        temp.path(),
        &psychevo_home,
        &cwd,
        &source,
        "display-plugin",
        false,
    );
    assert!(
        install.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&install.stderr)
    );

    let view = plugin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["plugin", "view", "display-plugin"])
        .output()
        .expect("pevo plugin view");
    assert!(
        view.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&view.stderr)
    );
    let stdout = String::from_utf8(view.stdout).expect("stdout");
    assert!(stdout.contains("display-plugin 1.0.0 [global]"));
    assert!(stdout.contains("Display: Display Plugin"));
    assert!(stdout.contains("Category: productivity"));
    assert!(stdout.contains("Capabilities: tools, hooks"));
    assert!(stdout.contains("Adds display metadata."));
}

#[tokio::test]
pub(crate) async fn cli_plugin_install_enable_list_and_doctor_json() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    let source = temp.path().join("plugin-source");
    std::fs::create_dir_all(&cwd).expect("cwd");
    init_skill_home(temp.path(), &psychevo_home);
    write_cli_plugin(&source);

    let install = add_marketplace_plugin(
        temp.path(),
        &psychevo_home,
        &cwd,
        &source,
        "disk-cleanup",
        true,
    );
    assert!(
        install.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&install.stderr)
    );
    let installed: Value = serde_json::from_slice(&install.stdout).expect("install json");
    assert_eq!(installed["plugin"]["name"], "disk-cleanup");
    assert_eq!(installed["plugin"]["scope"], "global");

    let enable = plugin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["plugin", "enable", "disk-cleanup", "--json"])
        .output()
        .expect("pevo plugin enable");
    assert!(
        enable.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&enable.stderr)
    );
    let enabled: Value = serde_json::from_slice(&enable.stdout).expect("enable json");
    assert_eq!(enabled["enabled"], true);
    let config = std::fs::read_to_string(psychevo_home.join("config.toml")).expect("config");
    assert!(config.contains("[plugins.\"profile:disk-cleanup@"));
    assert!(config.contains("enabled = true"));

    let list = plugin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["plugin", "list", "--json"])
        .output()
        .expect("pevo plugin list");
    assert!(
        list.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&list.stderr)
    );
    let listed: Value = serde_json::from_slice(&list.stdout).expect("list json");
    assert_eq!(listed["count"], 2);
    let disk_cleanup = listed["plugins"]
        .as_array()
        .expect("plugins")
        .iter()
        .find(|plugin| plugin["name"] == "disk-cleanup")
        .expect("disk-cleanup plugin");
    assert_eq!(disk_cleanup["enabled"], true);

    let doctor = plugin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["plugin", "doctor", "disk-cleanup", "--json"])
        .output()
        .expect("pevo plugin doctor");
    assert!(
        doctor.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&doctor.stderr)
    );
    let doctor: Value = serde_json::from_slice(&doctor.stdout).expect("doctor json");
    assert!(doctor["plugins"][0].get("worker").is_none());
}

#[tokio::test]
pub(crate) async fn cli_plugin_local_enable_targets_profile_installed_plugin() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    let source = temp.path().join("plugin-source");
    std::fs::create_dir_all(&cwd).expect("cwd");
    init_skill_home(temp.path(), &psychevo_home);
    write_cli_plugin(&source);

    let install = add_marketplace_plugin(
        temp.path(),
        &psychevo_home,
        &cwd,
        &source,
        "disk-cleanup",
        true,
    );
    assert!(
        install.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&install.stderr)
    );

    let enable = plugin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["plugin", "enable", "disk-cleanup", "--local", "--json"])
        .output()
        .expect("pevo plugin enable --local");
    assert!(
        enable.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&enable.stderr)
    );
    let enabled: Value = serde_json::from_slice(&enable.stdout).expect("enable json");
    assert_eq!(enabled["scope"], "local");
    assert_eq!(enabled["enabled"], true);

    let home_config = std::fs::read_to_string(psychevo_home.join("config.toml")).expect("config");
    assert!(!home_config.contains("[plugins.\"profile:disk-cleanup@"));
    let local_config =
        std::fs::read_to_string(cwd.join(".psychevo/config.toml")).expect("local config");
    assert!(local_config.contains("[plugins.\"profile:disk-cleanup@"));
    assert!(local_config.contains("enabled = true"));

    let list = plugin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["plugin", "list", "--json"])
        .output()
        .expect("pevo plugin list");
    assert!(
        list.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&list.stderr)
    );
    let listed: Value = serde_json::from_slice(&list.stdout).expect("list json");
    assert_eq!(listed["count"], 2);
    let disk_cleanup = listed["plugins"]
        .as_array()
        .expect("plugins")
        .iter()
        .find(|plugin| plugin["name"] == "disk-cleanup")
        .expect("disk-cleanup plugin");
    assert_eq!(disk_cleanup["enabled"], true);
    assert!(
        disk_cleanup["manifest_resources"]
            .as_array()
            .expect("manifest resources")
            .contains(&json!("skills"))
    );
    assert_eq!(disk_cleanup["psychevo_extensions"], json!([]));
}
