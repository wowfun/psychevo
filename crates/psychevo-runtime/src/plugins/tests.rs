use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use psychevo_agent_core::ToolBinding;
use psychevo_ai::AbortSignal;
use serde_json::{Value, json};

use tempfile::tempdir;

use super::*;

fn write_plugin(root: &Path, manifest: &str) {
    fs::create_dir_all(root.join(".psychevo-plugin")).expect("manifest dir");
    fs::write(root.join(".psychevo-plugin/plugin.json"), manifest).expect("manifest");
}

fn write_worker(root: &Path, script: &str) -> PathBuf {
    let worker = root.join("worker.py");
    fs::write(&worker, script).expect("worker");
    let mut perms = fs::metadata(&worker).expect("metadata").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&worker, perms).expect("chmod");
    worker
}

#[test]
fn manifest_selects_native_before_compat_and_rejects_path_escape() {
    let temp = tempdir().expect("temp");
    let root = temp.path().join("plugin");
    write_plugin(
        &root,
        r#"{
              "name": "cleanup",
              "version": "1.0.0",
              "description": "cleanup",
              "skills": ["./skills", "../escape"]
            }"#,
    );
    fs::create_dir_all(root.join(".codex-plugin")).expect("codex");
    fs::write(root.join(".codex-plugin/plugin.json"), "{}").expect("codex manifest");
    fs::create_dir_all(root.join("skills")).expect("skills");

    let manifest = load_plugin_manifest(&root, true).expect("manifest");

    assert_eq!(manifest.kind, PluginManifestKind::Psychevo);
    assert_eq!(manifest.skill_roots.len(), 1);
    assert_eq!(manifest.ignored_manifest_paths.len(), 1);
    assert!(manifest.diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("must start with ./")
            || diagnostic.message.contains("must not contain ..")
    }));
}

#[test]
fn malformed_preferred_manifest_does_not_fall_back_to_compat() {
    let temp = tempdir().expect("temp");
    let root = temp.path().join("plugin");
    fs::create_dir_all(root.join(".psychevo-plugin")).expect("native");
    fs::write(root.join(".psychevo-plugin/plugin.json"), "{").expect("native manifest");
    fs::create_dir_all(root.join(".codex-plugin")).expect("codex");
    fs::write(
        root.join(".codex-plugin/plugin.json"),
        r#"{"name":"codex-plugin","version":"1.0.0","description":"codex"}"#,
    )
    .expect("codex manifest");

    let err = load_plugin_manifest(&root, true).expect_err("malformed preferred manifest");

    assert!(err.to_string().contains(".psychevo-plugin/plugin.json"));
}

#[test]
fn manifest_loads_path_and_default_hook_files_additively() {
    let temp = tempdir().expect("temp");
    let root = temp.path().join("plugin");
    write_plugin(
        &root,
        r#"{
              "name": "hooked",
              "version": "1.0.0",
              "description": "hooked",
              "hooks": ["./extra/hooks.json"]
            }"#,
    );
    fs::create_dir_all(root.join("extra")).expect("extra");
    fs::write(
            root.join("extra/hooks.json"),
            r#"{"hooks":{"PreToolUse":[{"matcher":"Write","hooks":[{"type":"command","command":"echo path"}]}]}}"#,
        )
        .expect("path hooks");
    fs::create_dir_all(root.join("hooks")).expect("hooks");
    fs::write(
            root.join("hooks/hooks.json"),
            r#"{"hooks":{"PreToolUse":[{"matcher":"Edit","hooks":[{"type":"command","command":"echo default"}]}]}}"#,
        )
        .expect("default hooks");
    let manifest = load_plugin_manifest(&root, true).expect("manifest");

    let hooks = manifest.hooks.expect("hooks");
    assert_eq!(
        hooks
            .get("PreToolUse")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2)
    );
    assert!(manifest.manifest_resources.contains("hooks"));
}

#[test]
fn manifest_ignores_top_level_runtime_without_psychevo_namespace() {
    let temp = tempdir().expect("temp");
    let root = temp.path().join("plugin");
    write_plugin(
        &root,
        r#"{
              "name": "compat-data",
              "version": "1.0.0",
              "description": "compat data",
              "runtime": {"worker": {"command": "./worker.py"}}
            }"#,
    );

    let manifest = load_plugin_manifest(&root, true).expect("manifest");

    assert!(manifest.worker.is_none());
    assert!(manifest.ignored_fields.contains("runtime"));
}

#[cfg(unix)]
#[test]
fn manifest_rejects_symlink_escape() {
    let temp = tempdir().expect("temp");
    let root = temp.path().join("plugin");
    let outside = temp.path().join("outside");
    fs::create_dir_all(&outside).expect("outside");
    write_plugin(
        &root,
        r#"{
              "name": "escape",
              "version": "1.0.0",
              "description": "escape",
              "skills": ["./linked"]
            }"#,
    );
    std::os::unix::fs::symlink(&outside, root.join("linked")).expect("symlink");

    let manifest = load_plugin_manifest(&root, true).expect("manifest");

    assert!(manifest.skill_roots.is_empty());
    assert!(
        manifest
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("escapes plugin root"))
    );
}

#[test]
fn install_record_preserves_scope_source_and_data_root() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let source = temp.path().join("source");
    write_plugin(
        &source,
        r#"{
              "name": "cleanup",
              "version": "1.0.0",
              "description": "cleanup",
              "skills": ["./skills"]
            }"#,
    );
    fs::create_dir_all(source.join("skills")).expect("skills");

    let record = install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: source.display().to_string(),
            scope: PluginScope::Local,
            git_ref: None,
            force: false,
        },
    )
    .expect("install");

    assert_eq!(record.name, "cleanup");
    assert_eq!(record.scope, PluginScope::Local);
    assert!(
        record
            .package_root
            .starts_with(cwd.join(".psychevo/plugins/cache"))
    );
    assert!(
        record
            .data_root
            .starts_with(cwd.join(".psychevo/plugins/data"))
    );
}

#[cfg(unix)]
#[test]
fn install_rejects_package_symlink_without_copying_escape_content() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let outside = temp.path().join("outside");
    fs::create_dir_all(&outside).expect("outside");
    fs::write(outside.join("secret.txt"), "do not copy").expect("outside file");
    let source = temp.path().join("source");
    write_plugin(
        &source,
        r#"{
              "name": "cleanup",
              "version": "1.0.0",
              "description": "cleanup"
            }"#,
    );
    std::os::unix::fs::symlink(&outside, source.join("linked-outside")).expect("symlink");

    let err = install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: source.display().to_string(),
            scope: PluginScope::Global,
            git_ref: None,
            force: false,
        },
    )
    .expect_err("symlink rejected");

    assert!(err.to_string().contains("unsupported symlink"));
    let records = PluginStore::new(&home, &cwd, PluginScope::Global)
        .expect("store")
        .records()
        .expect("records");
    assert!(records.is_empty());
    let copied_secret = home
        .join("plugins/cache")
        .read_dir()
        .ok()
        .into_iter()
        .flatten()
        .any(|entry| {
            entry
                .expect("entry")
                .path()
                .join("linked-outside/secret.txt")
                .exists()
        });
    assert!(!copied_secret);
}

#[test]
fn selector_conflict_requires_source_qualified_name() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let source_a = temp.path().join("source-a");
    let source_b = temp.path().join("source-b");
    for source in [&source_a, &source_b] {
        write_plugin(
            source,
            r#"{
                  "name": "cleanup",
                  "version": "1.0.0",
                  "description": "cleanup"
                }"#,
        );
    }
    let record_a = install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: source_a.display().to_string(),
            scope: PluginScope::Global,
            git_ref: None,
            force: false,
        },
    )
    .expect("install a");
    install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: source_b.display().to_string(),
            scope: PluginScope::Global,
            git_ref: None,
            force: false,
        },
    )
    .expect("install b");

    let ambiguous = plugin_set_enabled_value(&home, &cwd, PluginScope::Global, "cleanup", true)
        .expect_err("ambiguous selector");
    assert!(ambiguous.to_string().contains("ambiguous; use name@source"));

    let value = plugin_set_enabled_value(
        &home,
        &cwd,
        PluginScope::Global,
        &format!("cleanup@{}", record_a.source_slug),
        true,
    )
    .expect("source-qualified enable");
    assert_eq!(value["enabled"], true);
    assert_eq!(value["source"], record_a.source_slug);
}

#[test]
fn local_policy_can_target_profile_installed_plugin() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let source = temp.path().join("source");
    write_plugin(
        &source,
        r#"{
              "name": "cleanup",
              "version": "1.0.0",
              "description": "cleanup",
              "skills": ["./skills"]
            }"#,
    );
    fs::create_dir_all(source.join("skills")).expect("skills");
    install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: source.display().to_string(),
            scope: PluginScope::Global,
            git_ref: None,
            force: false,
        },
    )
    .expect("profile install");

    let value = plugin_set_enabled_value(&home, &cwd, PluginScope::Local, "cleanup", true)
        .expect("local enable");

    assert_eq!(value["scope"], "local");
    assert_eq!(value["enabled"], true);
    let local_config = fs::read_to_string(cwd.join(".psychevo/config.toml")).expect("local config");
    assert!(local_config.contains("[plugins.cleanup]"));
    assert!(local_config.contains("enabled = true"));
    assert!(!home.join("config.toml").exists());
}

#[test]
fn compatibility_manifest_install_allows_missing_description() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let source = temp.path().join("source");
    fs::create_dir_all(source.join(".codex-plugin")).expect("codex dir");
    fs::write(
        source.join(".codex-plugin/plugin.json"),
        r#"{
              "name": "compat-cleanup",
              "version": "1.0.0"
            }"#,
    )
    .expect("codex manifest");

    let record = install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: source.display().to_string(),
            scope: PluginScope::Global,
            git_ref: None,
            force: false,
        },
    )
    .expect("compat install");

    assert_eq!(record.name, "compat-cleanup");
    assert_eq!(record.description, "");
    assert_eq!(record.manifest_kind, PluginManifestKind::Codex);
}

#[test]
fn marketplace_rejects_unsupported_kind() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");

    let err = plugin_marketplace_add_value(
        &home,
        &cwd,
        PluginScope::Global,
        PluginMarketplaceEntry {
            name: "cleanup".to_string(),
            source: "file:///tmp/cleanup".to_string(),
            kind: "archive".to_string(),
            git_ref: None,
        },
    )
    .expect_err("unsupported kind");

    assert!(err.to_string().contains("expected local or git"));
}

#[test]
fn worker_tool_executes_through_binding() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let source = temp.path().join("source");
    write_plugin(
        &source,
        r#"{
              "name": "cleanup",
              "version": "1.0.0",
              "description": "cleanup",
              "psychevo": {"runtime": {"worker": {"command": "./worker.py"}}}
            }"#,
    );
    write_worker(
        &source,
        r#"#!/usr/bin/env python3
import json, sys
for line in sys.stdin:
    req=json.loads(line)
    mid=req.get("method")
    if mid=="initialize":
        result={"ok": True}
    elif mid=="contributions/list":
        result={"tools":[{"name":"cleanup_status","description":"status","parameters":{"type":"object","properties":{}}}]}
    elif mid=="tools/call":
        result={"json":{"status":"ok","plugin":req["params"]["name"]},"content":"ok"}
    else:
        result={}
    print(json.dumps({"jsonrpc":"2.0","id":req.get("id"),"result":result}), flush=True)
"#,
    );
    let record = install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: source.display().to_string(),
            scope: PluginScope::Global,
            git_ref: None,
            force: false,
        },
    )
    .expect("install");
    let manifest = load_plugin_manifest(&record.package_root, true).expect("manifest");
    let spec = manifest.worker.clone().expect("worker");
    let tools = worker_tools(&record, &manifest, &spec, &BTreeMap::new()).expect("tools");
    assert_eq!(tools[0].name, "cleanup_status");
    let output = call_worker_tool(
        &record,
        &spec,
        &BTreeMap::new(),
        "cleanup_status",
        "call_1",
        json!({}),
    )
    .expect("call");
    assert!(!output.is_error);
    assert_eq!(output.json["status"], "ok");
}

#[test]
fn worker_contribution_discovery_receives_effective_env() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let source = temp.path().join("source");
    write_plugin(
        &source,
        r#"{
              "name": "cleanup",
              "version": "1.0.0",
              "description": "cleanup",
              "psychevo": {"runtime": {"worker": {"command": "./worker.py"}}}
            }"#,
    );
    write_worker(
        &source,
        r#"#!/usr/bin/env python3
import json, os, sys
for line in sys.stdin:
    req=json.loads(line)
    method=req.get("method")
    if method=="initialize":
        result={"ok": True}
    elif method=="contributions/list":
        if os.environ.get("PLUGIN_DISCOVERY_TOKEN") == "ok":
            result={"tools":[{"name":"env_tool","description":"env","parameters":{"type":"object","properties":{}}}]}
        else:
            result={"tools":[]}
    else:
        result={}
    print(json.dumps({"jsonrpc":"2.0","id":req.get("id"),"result":result}), flush=True)
"#,
    );
    let record = install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: source.display().to_string(),
            scope: PluginScope::Global,
            git_ref: None,
            force: false,
        },
    )
    .expect("install");
    let manifest = load_plugin_manifest(&record.package_root, true).expect("manifest");
    let spec = manifest.worker.clone().expect("worker");
    let mut env = BTreeMap::new();
    env.insert("PLUGIN_DISCOVERY_TOKEN".to_string(), "ok".to_string());

    let tools = worker_tools(&record, &manifest, &spec, &env).expect("tools");

    assert_eq!(tools[0].name, "env_tool");
}

#[test]
fn worker_contribution_discovery_times_out() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let source = temp.path().join("source");
    write_plugin(
        &source,
        r#"{
              "name": "cleanup",
              "version": "1.0.0",
              "description": "cleanup",
              "psychevo": {"runtime": {"worker": {"command": "./worker.py"}}}
            }"#,
    );
    write_worker(
        &source,
        r#"#!/usr/bin/env python3
import json, sys, time
for line in sys.stdin:
    req=json.loads(line)
    method=req.get("method")
    if method=="initialize":
        result={"ok": True}
        print(json.dumps({"jsonrpc":"2.0","id":req.get("id"),"result":result}), flush=True)
    elif method=="contributions/list":
        time.sleep(30)
    else:
        print(json.dumps({"jsonrpc":"2.0","id":req.get("id"),"result":{}}), flush=True)
"#,
    );
    let record = install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: source.display().to_string(),
            scope: PluginScope::Global,
            git_ref: None,
            force: false,
        },
    )
    .expect("install");
    let manifest = load_plugin_manifest(&record.package_root, true).expect("manifest");
    let spec = manifest.worker.clone().expect("worker");

    let err = worker_tools(&record, &manifest, &spec, &BTreeMap::new()).expect_err("timeout");

    assert!(err.contains("timed out waiting for contributions/list response"));
}

#[tokio::test]
async fn worker_tool_call_timeout_returns_tool_error() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let source = temp.path().join("source");
    write_plugin(
        &source,
        r#"{
              "name": "cleanup",
              "version": "1.0.0",
              "description": "cleanup",
              "psychevo": {"runtime": {"worker": {"command": "./worker.py"}}}
            }"#,
    );
    write_worker(
        &source,
        r#"#!/usr/bin/env python3
import json, sys, time
for line in sys.stdin:
    req=json.loads(line)
    method=req.get("method")
    if method=="initialize":
        result={"ok": True}
        print(json.dumps({"jsonrpc":"2.0","id":req.get("id"),"result":result}), flush=True)
    elif method=="tools/call":
        time.sleep(30)
    else:
        print(json.dumps({"jsonrpc":"2.0","id":req.get("id"),"result":{}}), flush=True)
"#,
    );
    let record = install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: source.display().to_string(),
            scope: PluginScope::Global,
            git_ref: None,
            force: false,
        },
    )
    .expect("install");
    let manifest = load_plugin_manifest(&record.package_root, true).expect("manifest");
    let spec = manifest.worker.clone().expect("worker");

    let tool = PluginWorkerTool {
        record,
        spec,
        descriptor: WorkerToolDescriptor {
            name: "cleanup_status".to_string(),
            description: "status".to_string(),
            parameters: json!({"type": "object", "properties": {}}),
        },
        env: BTreeMap::new(),
    };
    let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
    let output = tool
        .execute("call_1".to_string(), json!({}), AbortSignal::new(abort_rx))
        .await;

    assert!(output.is_error);
    assert!(
        output.json["error"]
            .as_str()
            .unwrap_or_default()
            .contains("timed out waiting for tools/call response")
    );
}

#[test]
fn install_from_local_git_source_materializes_record() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).expect("repo");
    write_plugin(
        &repo,
        r#"{
              "name": "git-plugin",
              "version": "1.0.0",
              "description": "from git"
            }"#,
    );
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .arg("init")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("git init")
            .success()
    );
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .arg("add")
            .arg(".")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("git add")
            .success()
    );
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
            .arg("commit")
            .arg("-m")
            .arg("plugin")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("git commit")
            .success()
    );

    let record = install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: format!("file://{}", repo.display()),
            scope: PluginScope::Global,
            git_ref: None,
            force: false,
        },
    )
    .expect("install git");

    assert_eq!(record.name, "git-plugin");
    assert!(record.source_id.starts_with("git:file://"));
    assert!(
        record
            .package_root
            .join(".psychevo-plugin/plugin.json")
            .exists()
    );
}
