use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use psychevo_agent_core::{ToolBinding, ToolRouter, ToolSearchOptions};
use psychevo_ai::AbortSignal;
use serde_json::{Value, json};

use tempfile::tempdir;

use crate::RunMode;
use crate::config::{PluginPolicyConfig, PluginPolicyEntry, ToolSelectionConfig};
use crate::contribution_projection::ContributionStatus;
use crate::sandbox::{SandboxPolicy, SandboxWriteGrants};
use crate::tool_surface::{
    ClarifyToolSurface, ToolSurfaceAssembly, assemble_tool_surface_with_warnings,
};
use crate::types::McpTransportInput;

use super::*;

fn write_plugin(root: &Path, manifest: &str) {
    let mut document: Value = serde_json::from_str(manifest).expect("manifest json");
    let overlay = document
        .as_object_mut()
        .and_then(|object| object.remove("psychevo"));
    fs::create_dir_all(root.join(".codex-plugin")).expect("manifest dir");
    fs::write(
        root.join(".codex-plugin/plugin.json"),
        serde_json::to_vec_pretty(&document).expect("manifest encode"),
    )
    .expect("manifest");
    if let Some(overlay) = overlay {
        fs::write(
            root.join("psychevo.plugin.json"),
            serde_json::to_vec_pretty(&overlay).expect("overlay encode"),
        )
        .expect("overlay");
    }
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
fn manifest_selects_codex_before_claude_and_rejects_path_escape() {
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
    fs::create_dir_all(root.join(".claude-plugin")).expect("claude");
    fs::write(root.join(".claude-plugin/plugin.json"), "{}").expect("claude manifest");
    fs::create_dir_all(root.join("skills")).expect("skills");

    let manifest = load_plugin_manifest(&root, true).expect("manifest");

    assert_eq!(manifest.kind, PluginManifestKind::Codex);
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
    fs::create_dir_all(root.join(".codex-plugin")).expect("codex");
    fs::write(root.join(".codex-plugin/plugin.json"), "{").expect("codex manifest");
    fs::create_dir_all(root.join(".claude-plugin")).expect("claude");
    fs::write(
        root.join(".claude-plugin/plugin.json"),
        r#"{"name":"claude-plugin","version":"1.0.0","description":"claude"}"#,
    )
    .expect("claude manifest");

    let err = load_plugin_manifest(&root, true).expect_err("malformed preferred manifest");

    assert!(err.to_string().contains(".codex-plugin/plugin.json"));
}

#[test]
fn manifest_uses_explicit_hooks_before_default_hook_file() {
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
        Some(1)
    );
    let matcher = hooks["PreToolUse"][0]["matcher"].as_str();
    assert_eq!(matcher, Some("Write"));
    assert!(manifest.manifest_resources.contains("hooks"));
}

#[test]
fn manifest_loads_default_hook_file_when_hooks_field_is_absent() {
    let temp = tempdir().expect("temp");
    let root = temp.path().join("plugin");
    write_plugin(
        &root,
        r#"{
              "name": "hooked",
              "version": "1.0.0",
              "description": "hooked"
            }"#,
    );
    fs::create_dir_all(root.join("hooks")).expect("hooks");
    fs::write(
            root.join("hooks/hooks.json"),
            r#"{"hooks":{"PreToolUse":[{"matcher":"Edit","hooks":[{"type":"command","command":"echo default"}]}]}}"#,
        )
        .expect("default hooks");

    let manifest = load_plugin_manifest(&root, true).expect("manifest");

    let hooks = manifest.hooks.expect("hooks");
    assert_eq!(hooks["PreToolUse"][0]["matcher"], "Edit");
    assert!(manifest.manifest_resources.contains("hooks"));
}

#[test]
fn manifest_parses_codex_interface_metadata_with_path_safety() {
    let temp = tempdir().expect("temp");
    let root = temp.path().join("plugin");
    fs::create_dir_all(root.join("assets")).expect("assets");
    for file in ["icon.png", "logo.png", "logo-dark.png", "screen.png"] {
        fs::write(root.join("assets").join(file), "asset").expect("asset");
    }
    write_plugin(
        &root,
        r##"{
              "name": "display-plugin",
              "version": "1.0.0",
              "description": "display plugin",
              "interface": {
                "displayName": "Display Plugin",
                "shortDescription": 7,
                "longDescription": "A longer description",
                "developerName": "Psychevo",
                "category": "productivity",
                "capabilities": ["tools", "hooks"],
                "websiteURL": "https://example.test",
                "privacyPolicyUrl": "https://example.test/privacy",
                "termsOfServiceURL": "https://example.test/terms",
                "brandColor": "#336699",
                "composerIcon": "./assets/icon.png",
                "logo": "./assets/logo.png",
                "logoDark": "./assets/logo-dark.png",
                "screenshots": ["./assets/screen.png", "./../escape.png", 4]
              }
            }"##,
    );

    let manifest = load_plugin_manifest(&root, true).expect("manifest");
    let interface = manifest.interface.expect("interface");

    assert_eq!(interface.display_name.as_deref(), Some("Display Plugin"));
    assert_eq!(interface.short_description, None);
    assert_eq!(interface.category.as_deref(), Some("productivity"));
    assert_eq!(interface.capabilities, vec!["tools", "hooks"]);
    assert_eq!(
        interface.website_url.as_deref(),
        Some("https://example.test")
    );
    assert!(
        interface
            .composer_icon
            .is_some_and(|path| path.starts_with(&root))
    );
    assert!(interface.logo.is_some_and(|path| path.starts_with(&root)));
    assert!(
        interface
            .logo_dark
            .is_some_and(|path| path.starts_with(&root))
    );
    assert_eq!(interface.screenshots.len(), 1);
    assert!(manifest.manifest_resources.contains("interface"));
    assert!(manifest.diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == "invalid"
            && diagnostic
                .message
                .contains("interface.shortDescription must be a string")
    }));
    assert!(manifest.diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == "invalid" && diagnostic.message.contains("must not contain ..")
    }));
    assert!(manifest.diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == "invalid"
            && diagnostic
                .message
                .contains("interface.screenshots must contain string paths")
    }));
}

#[test]
fn codex_profile_conformance_preserves_defaults_raw_fields_and_prompt_rules() {
    let temp = tempdir().expect("temp");
    let root = temp.path().join("fallback-name");
    fs::create_dir_all(root.join("skills/review")).expect("skills");
    fs::create_dir_all(root.join("hooks")).expect("hooks");
    fs::write(
        root.join("hooks/hooks.json"),
        r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"prompt","prompt":"context"}]}]}}"#,
    )
    .expect("hooks");
    fs::write(root.join(".mcp.json"), r#"{"mcpServers":{}}"#).expect("mcp");
    fs::write(root.join(".app.json"), r#"{"apps":[]}"#).expect("apps");
    write_plugin(
        &root,
        r#"{
          "keywords": ["review", "portable"],
          "futureField": {"preserved": true},
          "interface": {
            "defaultPrompt": [
              "  Summarize   this change  ",
              "Find risks",
              "Suggest tests",
              "ignored fourth prompt"
            ]
          }
        }"#,
    );

    let manifest = load_plugin_manifest(&root, true).expect("manifest");

    assert_eq!(manifest.name, "fallback-name");
    assert_eq!(manifest.version, None);
    assert_eq!(manifest.keywords, vec!["review", "portable"]);
    assert_eq!(manifest.skill_roots, vec![root.join("skills")]);
    assert!(manifest.hooks.is_some());
    assert!(manifest.manifest_resources.contains("mcpServers"));
    assert_eq!(manifest.app_resource, Some(root.join(".app.json")));
    assert_eq!(
        manifest
            .interface
            .as_ref()
            .expect("interface")
            .default_prompt,
        vec!["Summarize this change", "Find risks", "Suggest tests"]
    );
    assert_eq!(manifest.raw_manifest["futureField"]["preserved"], true);
    assert!(manifest.ignored_fields.contains("futureField"));
    let apps = manifest
        .component_statuses
        .iter()
        .find(|status| status.component == PluginComponentKind::Apps)
        .expect("apps status");
    assert_eq!(apps.highest_level, PluginCompatibilityLevel::Delegate);
    assert_eq!(apps.execution_owner, PluginExecutionOwner::CodexBroker);
}

#[test]
fn codex_profile_conformance_accepts_inline_hook_object_arrays() {
    let temp = tempdir().expect("temp");
    let root = temp.path().join("plugin");
    write_plugin(
        &root,
        r#"{
          "name":"inline-hooks",
          "hooks":[
            {"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"echo pre"}]}]}},
            {"hooks":{"SessionStart":[{"hooks":[{"type":"prompt","prompt":"context"}]}]}}
          ]
        }"#,
    );

    let manifest = load_plugin_manifest(&root, true).expect("manifest");
    let hooks = manifest.hooks.expect("inline hooks");

    assert_eq!(hooks["PreToolUse"].as_array().map(Vec::len), Some(1));
    assert_eq!(hooks["SessionStart"].as_array().map(Vec::len), Some(1));
}

#[test]
fn companion_overlay_with_shared_component_fails_closed_as_one_unit() {
    let temp = tempdir().expect("temp");
    let root = temp.path().join("plugin");
    write_plugin(&root, r#"{"name":"overlay"}"#);
    fs::write(
        root.join("psychevo.plugin.json"),
        r#"{
          "skills":["./other-skills"],
          "runtime":{"worker":{"command":"./worker.py"}}
        }"#,
    )
    .expect("overlay");

    let manifest = load_plugin_manifest(&root, true).expect("base remains inspectable");

    assert!(manifest.raw_overlay.is_some());
    assert!(manifest.worker.is_none());
    assert!(manifest.psychevo_extensions.is_empty());
    assert!(manifest.diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == "invalid"
            && diagnostic
                .message
                .contains("duplicates a shared Codex component")
            && diagnostic
                .message
                .contains("no overlay fields were projected")
    }));
}

#[test]
fn hermes_plugin_yaml_is_diagnostic_only() {
    let temp = tempdir().expect("temp");
    let root = temp.path().join("plugin");
    fs::create_dir_all(&root).expect("root");
    fs::write(
        root.join("plugin.yaml"),
        "name: hermes\nversion: 1.0.0\nmain: index.js\n",
    )
    .expect("hermes");

    let err = load_plugin_manifest(&root, true).expect_err("dynamic hermes unsupported");

    assert!(
        err.to_string()
            .contains("dynamic register(ctx) plugins are unsupported")
    );

    write_plugin(
        &root,
        r#"{
              "name": "native",
              "version": "1.0.0",
              "description": "native"
            }"#,
    );
    let manifest = load_plugin_manifest(&root, true).expect("native manifest");
    assert!(
        manifest
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.message.contains("Hermes plugin.yaml is ignored") })
    );
}

#[test]
fn inspect_hermes_adapter_descriptor_does_not_execute_register_module() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let root = temp.path().join("hermes-plugin");
    fs::create_dir_all(&root).expect("root");
    fs::write(
        root.join("plugin.yaml"),
        "name: hermes-cleanup\nversion: 1.2.0\ndescription: cleanup\nprovides_tools:\n  - cleanup\n",
    )
    .expect("plugin yaml");
    fs::write(
        root.join("__init__.py"),
        format!(
            "from pathlib import Path\nPath({:?}).write_text('executed')\n",
            temp.path().join("executed")
        ),
    )
    .expect("module");

    let value = plugin_import_inspect_value(
        &home,
        &cwd,
        PluginInspectOptions {
            source: root.display().to_string(),
            source_kind: Some(PluginSourceKind::Local),
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            adapter_mode: Some(PluginAdapterMode::AdapterHost),
        },
    )
    .expect("inspect");

    assert_eq!(value["inspection"]["framework"], "hermes");
    assert_eq!(value["inspection"]["status"], "Needs trust");
    assert_eq!(value["inspection"]["target_lanes"][0], "tools");
    assert!(!temp.path().join("executed").exists());
}

#[test]
fn install_foreign_adapter_records_fingerprint_and_trust() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let root = temp.path().join("hermes-plugin");
    fs::create_dir_all(&root).expect("root");
    fs::write(
        root.join("plugin.yaml"),
        "name: hermes-cleanup\nversion: 1.2.0\ndescription: cleanup\nprovides_hooks:\n  - pre_tool_call\n",
    )
    .expect("plugin yaml");

    let record = install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: root.display().to_string(),
            source_kind: Some(PluginSourceKind::Local),
            scope: PluginScope::Global,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            adapter_mode: Some(PluginAdapterMode::AdapterHost),
            force: false,
        },
    )
    .expect("install");

    assert_eq!(record.manifest_kind, PluginManifestKind::Hermes);
    assert_eq!(record.adapter_mode, PluginAdapterMode::AdapterHost);
    assert!(!record.package_fingerprint.is_empty());

    let value = plugin_set_trust_value(&home, &cwd, PluginScope::Global, "hermes-cleanup", true)
        .expect("trust");

    assert_eq!(value["trust"]["status"], "trusted");
    assert_eq!(value["trust"]["fingerprint"], record.package_fingerprint);
}

#[test]
fn inspect_opencode_rejects_entrypoint_outside_package_root() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let root = temp.path().join("opencode-plugin");
    fs::create_dir_all(&root).expect("root");
    fs::write(
        root.join("package.json"),
        r#"{
          "name": "opencode-bad",
          "version": "0.1.0",
          "exports": {"./server": "../outside.js"}
        }"#,
    )
    .expect("package");

    let value = plugin_import_inspect_value(
        &home,
        &cwd,
        PluginInspectOptions {
            source: root.display().to_string(),
            source_kind: Some(PluginSourceKind::Local),
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            adapter_mode: None,
        },
    )
    .expect("inspect");

    assert_eq!(value["success"], false);
    assert_eq!(value["inspection"]["framework"], "opencode");
    assert!(
        value["inspection"]["diagnostics"][0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("escapes package root"))
    );
}

#[test]
fn install_npm_source_uses_local_pack_fixture() {
    if Command::new("npm")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_err()
    {
        return;
    }
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let source = temp.path().join("npm-plugin");
    fs::create_dir_all(source.join(".codex-plugin")).expect("codex");
    fs::write(
        source.join("package.json"),
        r#"{"name":"npm-codex-plugin","version":"1.2.3","files":[".codex-plugin"]}"#,
    )
    .expect("package");
    fs::write(
        source.join(".codex-plugin/plugin.json"),
        r#"{"name":"npm-codex-plugin","version":"1.2.3","description":"npm plugin"}"#,
    )
    .expect("manifest");

    let record = install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: source.display().to_string(),
            source_kind: Some(PluginSourceKind::Npm),
            scope: PluginScope::Global,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            adapter_mode: None,
            force: false,
        },
    )
    .expect("npm install");

    assert_eq!(record.source_kind, PluginSourceKind::Npm);
    assert_eq!(record.name, "npm-codex-plugin");
    assert!(record.source_id.starts_with("npm:npm-codex-plugin@1.2.3"));
}

#[test]
fn manifest_parses_mcp_servers_object_without_discarding_valid_siblings() {
    let temp = tempdir().expect("temp");
    let root = temp.path().join("plugin");
    write_plugin(
        &root,
        r#"{
              "name": "mcp-plugin",
              "version": "1.0.0",
              "description": "mcp plugin",
              "mcpServers": {
                "stdio": {
                  "command": "node",
                  "args": ["server.js"],
                  "env": {"TOKEN": "x"},
                  "cwd": ".",
                  "enabledTools": ["search"],
                  "disabledTools": ["delete"],
                  "supportsParallelToolCalls": true,
                  "startupTimeoutSecs": 2,
                  "toolTimeoutSecs": 5
                },
                "http": {"type": "http", "url": "https://example.test/mcp", "headers": {"Authorization": "Bearer x"}},
                "future": {"type": "sse", "url": "https://example.test/sse"},
                "bad": {"command": 7}
              }
            }"#,
    );

    let manifest = load_plugin_manifest(&root, true).expect("manifest");

    assert_eq!(manifest.mcp_servers.len(), 3);
    assert!(manifest.manifest_resources.contains("mcpServers"));
    let stdio = manifest
        .mcp_servers
        .iter()
        .find(|server| server.name == "stdio")
        .expect("stdio");
    match &stdio.transport {
        McpTransportInput::Stdio {
            command,
            args,
            env,
            cwd,
        } => {
            assert_eq!(command, &PathBuf::from("node"));
            assert_eq!(args, &vec!["server.js".to_string()]);
            assert_eq!(env.get("TOKEN").map(String::as_str), Some("x"));
            assert!(cwd.as_ref().is_some_and(|cwd| cwd.starts_with(&root)));
        }
        other => panic!("unexpected stdio transport: {other:?}"),
    }
    assert_eq!(
        stdio.policy.enabled_tools.as_deref(),
        Some(&["search".to_string()][..])
    );
    assert_eq!(stdio.policy.disabled_tools, vec!["delete".to_string()]);
    assert!(stdio.policy.supports_parallel_tool_calls);
    assert_eq!(stdio.policy.startup_timeout_secs, Some(2));
    assert_eq!(stdio.policy.tool_timeout_secs, Some(5));
    let http = manifest
        .mcp_servers
        .iter()
        .find(|server| server.name == "http")
        .expect("http");
    assert!(matches!(
        &http.transport,
        McpTransportInput::StreamableHttp { url, headers, .. }
            if url == "https://example.test/mcp"
                && headers.get("Authorization").map(String::as_str) == Some("Bearer x")
    ));
    let unsupported = manifest
        .mcp_servers
        .iter()
        .find(|server| server.name == "future")
        .expect("future");
    assert!(matches!(
        &unsupported.transport,
        McpTransportInput::Unsupported { kind } if kind == "sse"
    ));
    assert!(manifest.diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == "invalid" && diagnostic.message.contains("mcpServers.bad.command")
    }));
}

#[test]
fn manifest_uses_explicit_mcp_servers_path_before_default_mcp_json() {
    let temp = tempdir().expect("temp");
    let root = temp.path().join("plugin");
    write_plugin(
        &root,
        r#"{
              "name": "mcp-plugin",
              "version": "1.0.0",
              "description": "mcp plugin"
            }"#,
    );
    fs::create_dir_all(root.join("bin")).expect("bin");
    fs::write(
        root.join(".mcp.json"),
        r#"{"mcpServers":{"default":{"command":"./bin/default-server"}}}"#,
    )
    .expect("default mcp");

    let manifest = load_plugin_manifest(&root, true).expect("default manifest");
    assert_eq!(manifest.mcp_servers.len(), 1);
    assert_eq!(manifest.mcp_servers[0].name, "default");

    write_plugin(
        &root,
        r#"{
              "name": "mcp-plugin",
              "version": "1.0.0",
              "description": "mcp plugin",
              "mcpServers": "./mcp.json"
            }"#,
    );
    fs::write(
        root.join("mcp.json"),
        r#"{"from-path":{"url":"https://example.test/path"}}"#,
    )
    .expect("path mcp");

    let manifest = load_plugin_manifest(&root, true).expect("path manifest");
    assert_eq!(manifest.mcp_servers.len(), 1);
    assert_eq!(manifest.mcp_servers[0].name, "from-path");
    assert!(matches!(
        &manifest.mcp_servers[0].transport,
        McpTransportInput::StreamableHttp { url, .. } if url == "https://example.test/path"
    ));
}

#[test]
fn manifest_parses_psychevo_toolsets_and_reports_invalid_siblings() {
    let temp = tempdir().expect("temp");
    let root = temp.path().join("plugin");
    write_plugin(
        &root,
        r#"{
              "name": "toolset-plugin",
              "version": "1.0.0",
              "description": "toolset plugin",
              "psychevo": {
                "toolsets": {
                  "review-pack": {"description": "review tools", "tools": ["review"], "includes": ["coding-core"]},
                  "bad name": {"tools": ["x"]},
                  "bad-tools": {"tools": [1]}
                }
              }
            }"#,
    );

    let manifest = load_plugin_manifest(&root, true).expect("manifest");

    let review_pack = manifest.toolsets.get("review-pack").expect("review-pack");
    assert_eq!(review_pack.description.as_deref(), Some("review tools"));
    assert_eq!(review_pack.tools, vec!["review".to_string()]);
    assert_eq!(review_pack.includes, vec!["coding-core".to_string()]);
    assert!(!manifest.toolsets.contains_key("bad name"));
    assert!(!manifest.toolsets.contains_key("bad-tools"));
    assert!(manifest.psychevo_extensions.contains("toolsets"));
    assert!(manifest.diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == "invalid" && diagnostic.message.contains("invalid toolset name")
    }));
    assert!(manifest.diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == "invalid" && diagnostic.message.contains("tools must contain strings")
    }));
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
            source_kind: None,
            scope: PluginScope::Local,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            adapter_mode: None,
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
            source_kind: None,
            scope: PluginScope::Global,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            adapter_mode: None,
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
            source_kind: None,
            scope: PluginScope::Global,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            adapter_mode: None,
            force: false,
        },
    )
    .expect("install a");
    install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: source_b.display().to_string(),
            source_kind: None,
            scope: PluginScope::Global,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            adapter_mode: None,
            force: false,
        },
    )
    .expect("install b");

    let ambiguous = plugin_set_enabled_value(&home, &cwd, PluginScope::Global, "cleanup", true)
        .expect_err("ambiguous selector");
    assert!(
        ambiguous
            .to_string()
            .contains("use profile:name@source or project:name@source")
    );

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
    let record = install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: source.display().to_string(),
            source_kind: None,
            scope: PluginScope::Global,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            adapter_mode: None,
            force: false,
        },
    )
    .expect("profile install");

    let value = plugin_set_enabled_value(&home, &cwd, PluginScope::Local, "cleanup", true)
        .expect("local enable");

    assert_eq!(value["scope"], "local");
    assert_eq!(value["enabled"], true);
    let local_config = fs::read_to_string(cwd.join(".psychevo/config.toml")).expect("local config");
    assert!(local_config.contains(&format!(
        "[plugins.\"profile:cleanup@{}\"]",
        record.source_slug
    )));
    assert!(local_config.contains("enabled = true"));
    assert!(!home.join("config.toml").exists());
}

#[test]
fn scoped_selectors_and_policy_keys_distinguish_duplicate_installations() {
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
          "description": "cleanup"
        }"#,
    );
    let profile = install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: source.display().to_string(),
            source_kind: None,
            scope: PluginScope::Global,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            adapter_mode: None,
            force: false,
        },
    )
    .expect("profile install");
    let project = install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: source.display().to_string(),
            source_kind: None,
            scope: PluginScope::Local,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            adapter_mode: None,
            force: false,
        },
    )
    .expect("project install");
    assert_eq!(profile.source_slug, project.source_slug);
    let unscoped = format!("cleanup@{}", profile.source_slug);
    let profile_selector = format!("profile:{unscoped}");
    let project_selector = format!("project:{unscoped}");

    let ambiguous = plugin_set_enabled_value(&home, &cwd, PluginScope::Local, &unscoped, true)
        .expect_err("unscoped selector is ambiguous across installation scopes");
    assert!(ambiguous.to_string().contains("ambiguous"));

    plugin_set_enabled_value(&home, &cwd, PluginScope::Global, &profile_selector, false)
        .expect("disable profile installation");
    plugin_set_enabled_value(&home, &cwd, PluginScope::Local, &project_selector, true)
        .expect("enable project installation");

    let profile_config = fs::read_to_string(home.join("config.toml")).expect("profile config");
    let project_config =
        fs::read_to_string(cwd.join(".psychevo/config.toml")).expect("project config");
    assert!(profile_config.contains(&format!("[plugins.\"{profile_selector}\"]")));
    assert!(project_config.contains(&format!("[plugins.\"{project_selector}\"]")));

    plugin_uninstall_value(&home, &cwd, PluginScope::Local, &project_selector)
        .expect("uninstall project installation");
    plugin_set_enabled_value(&home, &cwd, PluginScope::Global, &profile_selector, true)
        .expect("profile installation remains selectable");
}

#[test]
fn enabled_plugin_contributions_materialize_mcp_servers_toolsets_and_projection() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let source = temp.path().join("source");
    write_plugin(
        &source,
        r#"{
              "name": "contributor",
              "version": "1.0.0",
              "description": "contributor",
              "mcpServers": {
                "stdio": {"command": "node", "args": ["server.js"]}
              },
              "psychevo": {
                "toolsets": {
                  "contrib-tools": {"tools": ["mcp__stdio__review"]}
                }
              }
            }"#,
    );
    install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: source.display().to_string(),
            source_kind: None,
            scope: PluginScope::Global,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            adapter_mode: None,
            force: false,
        },
    )
    .expect("install");
    let mut policy = PluginPolicyConfig::default();
    policy.plugins.insert(
        "contributor".to_string(),
        PluginPolicyEntry {
            enabled: Some(true),
        },
    );

    let assembly = load_enabled_plugin_contributions(&home, &cwd, &BTreeMap::new(), &policy);

    assert_eq!(assembly.mcp_servers.len(), 1);
    assert_eq!(assembly.mcp_servers[0].name, "stdio");
    assert_eq!(
        assembly.mcp_servers[0].source_kind.as_deref(),
        Some("plugin")
    );
    assert!(
        assembly.mcp_servers[0]
            .source_id
            .as_deref()
            .is_some_and(|source| source.starts_with("plugin:contributor@"))
    );
    assert_eq!(assembly.toolsets.len(), 1);
    assert_eq!(assembly.toolsets[0].name, "contrib-tools");
    assert!(assembly.projection.facts().iter().any(|fact| {
        fact.status == ContributionStatus::Accepted
            && fact.declaration_family == "mcp_server"
            && fact.effect_target == "mcp:stdio"
    }));
    assert!(assembly.projection.facts().iter().any(|fact| {
        fact.status == ContributionStatus::Accepted
            && fact.declaration_family == "toolset"
            && fact.effect_target == "toolset:contrib-tools"
    }));
}

#[test]
fn enabled_plugin_worker_tools_enter_tool_surface_as_searchable_plugin_tools() {
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
    method=req.get("method")
    if method=="initialize":
        result={"ok": True}
    elif method=="contributions/list":
        result={"tools":[{"name":"cleanup_status","description":"status","parameters":{"type":"object","properties":{}}}]}
    else:
        result={}
    print(json.dumps({"jsonrpc":"2.0","id":req.get("id"),"result":result}), flush=True)
"#,
    );
    install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: source.display().to_string(),
            source_kind: None,
            scope: PluginScope::Global,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            adapter_mode: None,
            force: false,
        },
    )
    .expect("install");
    let mut policy = PluginPolicyConfig::default();
    policy.plugins.insert(
        "cleanup".to_string(),
        PluginPolicyEntry {
            enabled: Some(true),
        },
    );

    let assembly = load_enabled_plugin_contributions(&home, &cwd, &BTreeMap::new(), &policy);

    assert_eq!(assembly.runtime_tools.len(), 1);
    assert_eq!(assembly.runtime_tools[0].name(), "cleanup_status");
    assert_eq!(assembly.runtime_tools[0].source_kind(), Some("plugin"));
    assert!(
        assembly.runtime_tools[0]
            .source_id()
            .is_some_and(|source| source.starts_with("plugin:cleanup@"))
    );

    let surface = assemble_tool_surface_with_warnings(ToolSurfaceAssembly {
        cwd,
        task_id: "test".to_string(),
        mode: RunMode::Default,
        lsp: Default::default(),
        allow_login_shell: false,
        stream_events: None,
        env: BTreeMap::new(),
        path_prefixes: Vec::new(),
        sandbox_policy: SandboxPolicy::disabled(),
        sandbox_grants: SandboxWriteGrants::default(),
        home: None,
        image_input_enabled: true,
        image_generation: None,
        web_search: Default::default(),
        tool_selection: ToolSelectionConfig::default(),
        custom_toolsets: BTreeMap::new(),
        contributed_toolsets: assembly.toolsets,
        clarify: ClarifyToolSurface::Disabled,
        skills: None,
        extension_tools: assembly.runtime_tools,
        agents: None,
    });
    let declarations = ToolRouter::from_tools(surface.tools)
        .with_tool_search(ToolSearchOptions::enabled())
        .declarations();
    let names = declarations
        .into_iter()
        .map(|declaration| declaration.name)
        .collect::<Vec<_>>();

    assert!(names.contains(&"tool_search".to_string()));
    assert!(!names.contains(&"cleanup_status".to_string()));
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
              "name": "compat-cleanup"
            }"#,
    )
    .expect("codex manifest");

    let record = install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: source.display().to_string(),
            source_kind: None,
            scope: PluginScope::Global,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            adapter_mode: None,
            force: false,
        },
    )
    .expect("compat install");

    assert_eq!(record.name, "compat-cleanup");
    assert_eq!(record.version, "local");
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
            npm_version: None,
            npm_registry: None,
            adapter_mode: None,
        },
    )
    .expect_err("unsupported kind");

    assert!(err.to_string().contains("expected local, git, or npm"));
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
            source_kind: None,
            scope: PluginScope::Global,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            adapter_mode: None,
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
            source_kind: None,
            scope: PluginScope::Global,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            adapter_mode: None,
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
            source_kind: None,
            scope: PluginScope::Global,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            adapter_mode: None,
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
            source_kind: None,
            scope: PluginScope::Global,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            adapter_mode: None,
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
            source_kind: None,
            scope: PluginScope::Global,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            adapter_mode: None,
            force: false,
        },
    )
    .expect("install git");

    assert_eq!(record.name, "git-plugin");
    assert!(record.source_id.starts_with("git:file://"));
    assert!(
        record
            .package_root
            .join(".codex-plugin/plugin.json")
            .exists()
    );
}
