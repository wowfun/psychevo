#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn plugin_cmd(test_home: &Path, psychevo_home: &Path, cwd: &Path) -> Command {
    let mut command = pevo_cmd(test_home);
    command.env("PSYCHEVO_HOME", psychevo_home).current_dir(cwd);
    command
}

pub(crate) fn write_cli_plugin(root: &Path) {
    std::fs::create_dir_all(root.join(".psychevo-plugin")).expect("manifest dir");
    std::fs::create_dir_all(root.join("skills/cleanup")).expect("skill dir");
    std::fs::write(
        root.join(".psychevo-plugin/plugin.json"),
        r#"{
          "name": "disk-cleanup",
          "version": "1.0.0",
          "description": "Track and clean temporary files",
          "skills": ["./skills"],
          "psychevo": {"runtime": {"worker": {"command": "./worker.py"}}}
        }"#,
    )
    .expect("manifest");
    std::fs::write(
        root.join("skills/cleanup/SKILL.md"),
        "---\nname: cleanup\ndescription: \"Clean temporary files\"\n---\n\nUse cleanup_status before cleanup.\n",
    )
    .expect("skill");
    std::fs::write(
        root.join("worker.py"),
        r#"#!/usr/bin/env python3
import json, sys
for line in sys.stdin:
    req=json.loads(line)
    method=req.get("method")
    if method=="initialize":
        result={"ok": True}
    elif method=="contributions/list":
        result={"tools":[{"name":"cleanup_status","description":"Report cleanup status","parameters":{"type":"object","properties":{}}}]}
    elif method=="tools/call":
        result={"json":{"status":"ok"},"content":"cleanup ok"}
    else:
        result={}
    print(json.dumps({"jsonrpc":"2.0","id":req.get("id"),"result":result}), flush=True)
"#,
    )
    .expect("worker");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let worker = root.join("worker.py");
        let mut permissions = std::fs::metadata(&worker).expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(worker, permissions).expect("chmod");
    }
}

fn write_display_plugin(root: &Path) {
    std::fs::create_dir_all(root.join(".psychevo-plugin")).expect("manifest dir");
    std::fs::create_dir_all(root.join("assets")).expect("assets");
    std::fs::write(root.join("assets/icon.png"), "icon").expect("icon");
    std::fs::write(
        root.join(".psychevo-plugin/plugin.json"),
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

fn sse_tool_call(call_id: &str, tool_name: &str, arguments: &str) -> String {
    format!(
        "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":{},\"function\":{{\"name\":{},\"arguments\":{}}}}}]}},\"finish_reason\":\"tool_calls\"}}]}}\n\n\
         data: [DONE]\n\n",
        serde_json::to_string(call_id).expect("call id"),
        serde_json::to_string(tool_name).expect("tool name"),
        serde_json::to_string(arguments).expect("arguments")
    )
}

#[test]
pub(crate) fn cli_plugin_view_human_output_includes_interface_summary() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    let source = temp.path().join("display-plugin-source");
    std::fs::create_dir_all(&cwd).expect("cwd");
    init_skill_home(temp.path(), &psychevo_home);
    write_display_plugin(&source);

    let install = plugin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["plugin", "install", source.to_str().expect("source")])
        .output()
        .expect("pevo plugin install");
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

#[test]
pub(crate) fn cli_plugin_install_enable_list_and_doctor_json() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    let source = temp.path().join("plugin-source");
    std::fs::create_dir_all(&cwd).expect("cwd");
    init_skill_home(temp.path(), &psychevo_home);
    write_cli_plugin(&source);

    let install = plugin_cmd(temp.path(), &psychevo_home, &cwd)
        .args([
            "plugin",
            "install",
            source.to_str().expect("source"),
            "--json",
        ])
        .output()
        .expect("pevo plugin install");
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
    assert!(config.contains("[plugins.disk-cleanup]"));
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
    assert_eq!(listed["count"], 1);
    assert_eq!(listed["plugins"][0]["enabled"], true);

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
    assert_eq!(doctor["plugins"][0]["worker"]["status"], "ok");
    assert_eq!(
        doctor["plugins"][0]["worker"]["tools"][0]["name"],
        "cleanup_status"
    );
}

#[test]
pub(crate) fn cli_plugin_local_enable_targets_profile_installed_plugin() {
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    let source = temp.path().join("plugin-source");
    std::fs::create_dir_all(&cwd).expect("cwd");
    init_skill_home(temp.path(), &psychevo_home);
    write_cli_plugin(&source);

    let install = plugin_cmd(temp.path(), &psychevo_home, &cwd)
        .args([
            "plugin",
            "install",
            source.to_str().expect("source"),
            "--json",
        ])
        .output()
        .expect("pevo plugin install");
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
    assert!(!home_config.contains("[plugins.disk-cleanup]"));
    let local_config =
        std::fs::read_to_string(cwd.join(".psychevo/config.toml")).expect("local config");
    assert!(local_config.contains("[plugins.disk-cleanup]"));
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
    assert_eq!(listed["count"], 1);
    assert_eq!(listed["plugins"][0]["enabled"], true);
    assert!(
        listed["plugins"][0]["manifest_resources"]
            .as_array()
            .expect("manifest resources")
            .contains(&json!("skills"))
    );
    assert_eq!(listed["plugins"][0]["psychevo_extensions"][0], "runtime");
}

#[test]
pub(crate) fn cli_run_can_search_and_execute_enabled_plugin_worker_tool_by_default() {
    let server = MockSseServer::start(vec![
        sse_tool_call(
            "call_search",
            "tool_search",
            r#"{"query":"cleanup_status"}"#,
        ),
        sse_tool_call("call_cleanup", "cleanup_status", "{}"),
        sse_text("cleanup checked"),
    ]);
    let temp = tempdir().expect("temp");
    let psychevo_home = temp.path().join("psychevo-home");
    let cwd = temp.path().join("work");
    let source = temp.path().join("plugin-source");
    let db = temp.path().join("state.db");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&psychevo_home).expect("home");
    seed_managed_rg(&psychevo_home);
    write_cli_plugin(&source);
    std::fs::write(
        psychevo_home.join("config.toml"),
        format!(
            r#"model = "mock/mock-model"

[provider.mock]
api = "{}"

[plugins.disk-cleanup]
enabled = true
"#,
            server.base_url
        ),
    )
    .expect("config");
    std::fs::write(psychevo_home.join(".env"), "MOCK_API_KEY=test-key\n").expect("env");

    let install = plugin_cmd(temp.path(), &psychevo_home, &cwd)
        .args(["plugin", "install", source.to_str().expect("source")])
        .output()
        .expect("pevo plugin install");
    assert!(
        install.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&install.stderr)
    );

    let output = pevo_cmd(temp.path())
        .env("PSYCHEVO_HOME", &psychevo_home)
        .env("PSYCHEVO_DB", &db)
        .env("MOCK_API_KEY", "test-key")
        .current_dir(&cwd)
        .args(["run", "-f", "json", "check cleanup status"])
        .output()
        .expect("pevo run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("cleanup checked"));

    let conn = Connection::open(db).expect("db");
    let plugin_tool_results: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE role = 'tool_result' AND tool_name = 'cleanup_status' AND outcome = 'normal'",
            [],
            |row| row.get(0),
        )
        .expect("plugin tool results");
    assert_eq!(plugin_tool_results, 1);
    let tool_search_results: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE role = 'tool_result' AND tool_name = 'tool_search' AND outcome = 'normal'",
            [],
            |row| row.get(0),
        )
        .expect("tool search results");
    assert_eq!(tool_search_results, 1);
}
