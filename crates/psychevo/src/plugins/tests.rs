use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tempfile::tempdir;

use crate::config::{PluginPolicyConfig, PluginPolicyEntry};
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

fn native_npm_available() -> bool {
    let env = std::env::vars().collect::<BTreeMap<_, _>>();
    let cwd = std::env::current_dir().expect("current dir");
    let Some(npm) = crate::host_paths::resolve_executable_path(
        "npm",
        &cwd,
        &crate::host_paths::ExecutableResolveOptions {
            platform: crate::host_paths::HostPlatform::current(),
            env: &env,
        },
    ) else {
        return false;
    };
    #[cfg(not(unix))]
    let _ = &npm;
    #[cfg(unix)]
    if npm
        .parent()
        .is_some_and(|parent| parent.join("node.exe").is_file())
    {
        return false;
    }
    Command::new("npm")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
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
fn plugin_packages_reject_extension_manifests_and_executable_runtime_overlays() {
    let temp = tempdir().expect("temp");
    let extension_root = temp.path().join("extension");
    write_plugin(&extension_root, r#"{"name":"not-a-plugin"}"#);
    fs::write(
        extension_root.join("psychevo.extension.json"),
        r#"{"schemaVersion":1}"#,
    )
    .expect("Extension marker");
    let error = load_plugin_manifest(&extension_root, true).expect_err("Extension is not Plugin");
    assert!(error.to_string().contains("pevo install"));

    let plugin_root = temp.path().join("plugin");
    write_plugin(&plugin_root, r#"{"name":"declarative-only"}"#);
    fs::write(
        plugin_root.join("psychevo.plugin.json"),
        r#"{"runtime":{"worker":{"command":"./worker"}}}"#,
    )
    .expect("legacy runtime overlay");
    let manifest = load_plugin_manifest(&plugin_root, true).expect("inspect invalid overlay");
    assert!(manifest.psychevo_extensions.is_empty());
    assert!(manifest.diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == "invalid"
            && diagnostic
                .message
                .contains("belongs to an executable Extension manifest")
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
fn inspect_hermes_metadata_does_not_execute_register_module() {
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
        },
    )
    .expect("inspect");

    assert_eq!(value["inspection"]["framework"], "hermes");
    assert_eq!(value["inspection"]["support"], "inspection_only");
    assert_eq!(value["inspection"]["declared_lanes"][0], "tools");
    assert!(!temp.path().join("executed").exists());
}

#[test]
fn install_foreign_source_is_rejected_without_persistence() {
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

    let error = install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: root.display().to_string(),
            source_kind: Some(PluginSourceKind::Local),
            scope: PluginScope::Global,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            force: false,
        },
    )
    .expect_err("foreign install must be rejected");

    assert!(error.to_string().contains("inspection-only"));
    assert!(error.to_string().contains("ACP Agent runtime profile"));
    assert!(
        !home.exists(),
        "inspection-only install must not create state"
    );
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
    if !native_npm_available() {
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
fn resetting_local_policy_restores_profile_inheritance() {
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
            force: false,
        },
    )
    .expect("profile install");
    plugin_set_enabled_value(&home, &cwd, PluginScope::Global, "cleanup", true)
        .expect("profile allow");
    plugin_set_enabled_value(&home, &cwd, PluginScope::Local, "cleanup", false)
        .expect("project deny");

    let reset = plugin_reset_enabled_value(&home, &cwd, PluginScope::Local, "cleanup")
        .expect("reset project override");

    assert!(reset["enabled"].is_null());
    let local_config =
        fs::read_to_string(cwd.join(".psychevo/config.toml")).expect("local config after reset");
    assert!(!local_config.contains("cleanup"));
    let profile_config =
        fs::read_to_string(home.join("config.toml")).expect("profile config remains authoritative");
    assert!(profile_config.contains("enabled = true"));
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

#[tokio::test]
async fn enabled_plugin_contributions_materialize_mcp_servers_and_toolsets() {
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

    let assembly = load_enabled_plugin_contributions(&home, &cwd, &BTreeMap::new(), &policy).await;

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
        },
    )
    .expect_err("unsupported kind");

    assert!(err.to_string().contains("expected local, git, or npm"));
}

#[test]
fn local_marketplace_add_and_plugin_install_preserve_marketplace_identity_disabled() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    let marketplace = temp.path().join("marketplace");
    let plugin = marketplace.join("plugins/review");
    fs::create_dir_all(marketplace.join(".agents/plugins")).expect("catalog dir");
    fs::create_dir_all(&cwd).expect("cwd");
    write_plugin(
        &plugin,
        r#"{
          "name": "review",
          "version": "1.0.0",
          "description": "Review changes"
        }"#,
    );
    fs::write(
        marketplace.join(".agents/plugins/marketplace.json"),
        r#"{
          "name": "personal",
          "plugins": [{
            "name": "review",
            "source": {"source": "local", "path": "./plugins/review"}
          }]
        }"#,
    )
    .expect("marketplace manifest");

    let added = plugin_marketplace_add_value(
        &home,
        &cwd,
        PluginScope::Global,
        PluginMarketplaceEntry {
            name: String::new(),
            source: marketplace.display().to_string(),
            kind: "local".to_string(),
            git_ref: None,
            npm_version: None,
            npm_registry: None,
        },
    )
    .expect("add marketplace");
    assert_eq!(added["marketplace"]["name"], "personal");
    assert_eq!(added["already_added"], false);

    let installed =
        plugin_marketplace_install_value(&home, &cwd, PluginScope::Global, "review", "personal")
            .expect("install marketplace Plugin");
    assert_eq!(installed["plugin"]["marketplace"], "personal");
    assert_eq!(installed["enabled"], false);

    let records = super::records::all_records(&home, &cwd).expect("records");
    let record =
        super::records::select_record(&records, "review@personal").expect("marketplace selector");
    assert_eq!(record.marketplace.as_deref(), Some("personal"));
    assert_eq!(
        super::records::canonical_record_selector(record),
        "profile:review@personal"
    );

    let error = plugin_marketplace_remove_value(&home, &cwd, PluginScope::Global, "personal")
        .expect_err("installed Plugin keeps marketplace identity available");
    assert!(error.to_string().contains("review"));
    assert!(error.to_string().contains("remove them first"));

    plugin_uninstall_value(&home, &cwd, PluginScope::Global, "review@personal")
        .expect("remove marketplace Plugin");
    let removed = plugin_marketplace_remove_value(&home, &cwd, PluginScope::Global, "personal")
        .expect("remove now-unused marketplace");
    assert_eq!(removed["removed"], true);
}

#[test]
fn marketplace_rejects_plugin_paths_that_escape_its_root() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    let marketplace = temp.path().join("marketplace");
    fs::create_dir_all(marketplace.join(".agents/plugins")).expect("catalog dir");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::write(
        marketplace.join(".agents/plugins/marketplace.json"),
        r#"{
          "name": "unsafe",
          "plugins": [{"name": "escape", "source": "../outside"}]
        }"#,
    )
    .expect("marketplace manifest");

    let error = plugin_marketplace_add_value(
        &home,
        &cwd,
        PluginScope::Global,
        PluginMarketplaceEntry {
            name: "unsafe".to_string(),
            source: marketplace.display().to_string(),
            kind: "local".to_string(),
            git_ref: None,
            npm_version: None,
            npm_registry: None,
        },
    )
    .expect_err("escaping Plugin source");
    assert!(error.to_string().contains("without .."));
}

#[test]
fn git_marketplace_upgrade_replaces_validated_snapshot() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    let repo = temp.path().join("marketplace-repo");
    fs::create_dir_all(repo.join(".agents/plugins")).expect("catalog dir");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::write(
        repo.join(".agents/plugins/marketplace.json"),
        r#"{"name":"remote-fixture","plugins":[]}"#,
    )
    .expect("marketplace");
    commit_fixture_repo(&repo, "initial");

    let added = plugin_marketplace_add_value(
        &home,
        &cwd,
        PluginScope::Global,
        PluginMarketplaceEntry {
            name: String::new(),
            source: repo.display().to_string(),
            kind: "git".to_string(),
            git_ref: None,
            npm_version: None,
            npm_registry: None,
        },
    )
    .expect("add Git marketplace");
    let snapshot = PathBuf::from(added["root"].as_str().expect("snapshot root"));
    assert_eq!(
        serde_json::from_str::<Value>(
            &fs::read_to_string(snapshot.join(".agents/plugins/marketplace.json"))
                .expect("snapshot manifest")
        )
        .expect("snapshot json")["plugins"]
            .as_array()
            .expect("plugins")
            .len(),
        0
    );

    fs::create_dir_all(repo.join("plugins/review/.codex-plugin")).expect("plugin dir");
    fs::write(
        repo.join("plugins/review/.codex-plugin/plugin.json"),
        r#"{"name":"review","version":"1.0.0"}"#,
    )
    .expect("plugin manifest");
    fs::write(
        repo.join(".agents/plugins/marketplace.json"),
        r#"{"name":"remote-fixture","plugins":[{"name":"review","source":"./plugins/review"}]}"#,
    )
    .expect("updated marketplace");
    commit_fixture_repo(&repo, "add review");

    let upgraded =
        plugin_marketplace_upgrade_value(&home, &cwd, PluginScope::Global, Some("remote-fixture"))
            .expect("upgrade marketplace");
    assert_eq!(upgraded["marketplaces"][0]["plugin_count"], 1);
    assert!(
        snapshot
            .join("plugins/review/.codex-plugin/plugin.json")
            .is_file()
    );
}

fn commit_fixture_repo(repo: &Path, message: &str) {
    if !repo.join(".git").is_dir() {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(repo)
                .arg("init")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("git init")
                .success()
        );
    }
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["add", "."])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("git add")
            .success()
    );
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(repo)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                message,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("git commit")
            .success()
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

    let repo_uri = crate::host_paths::path_ref_for_native_path(&repo).uri;
    #[cfg(unix)]
    let git_source = format!(
        "file://private-user:private-secret@localhost{}",
        repo_uri.strip_prefix("file://").expect("file URI")
    );
    #[cfg(windows)]
    let git_source = repo_uri;
    let record = install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: git_source,
            source_kind: None,
            scope: PluginScope::Global,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            force: false,
        },
    )
    .expect("install git");

    assert_eq!(record.name, "git-plugin");
    assert!(record.source_id.starts_with("git:file://"));
    assert!(!record.package_root.join(".git").exists());
    assert!(
        record
            .package_root
            .join(".codex-plugin/plugin.json")
            .exists()
    );

    let commit = String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("git rev-parse")
            .stdout,
    )
    .expect("commit utf8")
    .trim()
    .to_string();
    assert_eq!(record.resolved_revision.as_deref(), Some(commit.as_str()));
    let serialized = serde_json::to_string(&record).expect("serialize record");
    assert!(!serialized.contains("private-user"));
    assert!(!serialized.contains("private-secret"));
    assert!(
        !record
            .package_root
            .display()
            .to_string()
            .contains("private-")
    );
    let ref_record = install_plugin(
        &temp.path().join("home-ref"),
        &cwd,
        PluginInstallOptions {
            source: format!("file://{}", repo.display()),
            source_kind: Some(PluginSourceKind::Git),
            scope: PluginScope::Global,
            git_ref: Some(commit.clone()),
            npm_version: None,
            npm_registry: None,
            force: false,
        },
    )
    .expect("install depth-1 git ref");
    assert!(ref_record.source_id.ends_with(&format!("#{commit}")));
    assert_eq!(
        ref_record.resolved_revision.as_deref(),
        Some(commit.as_str())
    );
    assert!(!ref_record.package_root.join(".git").exists());
}

#[tokio::test]
async fn activating_a_legacy_git_record_removes_only_top_level_metadata() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let source = temp.path().join("source");
    write_plugin(
        &source,
        r#"{
              "name": "legacy-git",
              "version": "1.0.0",
              "description": "legacy"
            }"#,
    );
    let mut record = install_plugin(
        &home,
        &cwd,
        PluginInstallOptions {
            source: source.display().to_string(),
            source_kind: Some(PluginSourceKind::Local),
            scope: PluginScope::Global,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            force: false,
        },
    )
    .expect("install");
    record.source_kind = PluginSourceKind::Git;
    let top_level_git = record.package_root.join(".git");
    let nested_git = record.package_root.join("fixture/.git");
    fs::create_dir_all(&top_level_git).expect("legacy top-level metadata");
    fs::write(top_level_git.join("config"), "credential = private").expect("legacy config");
    fs::create_dir_all(&nested_git).expect("nested fixture metadata");
    fs::write(nested_git.join("keep"), "fixture").expect("nested fixture");
    PluginStore::new(&home, &cwd, PluginScope::Global)
        .expect("store")
        .write_record(&record)
        .expect("rewrite record");
    let mut policy = PluginPolicyConfig::default();
    policy.plugins.insert(
        "legacy-git".to_string(),
        PluginPolicyEntry {
            enabled: Some(true),
        },
    );

    let first = load_enabled_plugin_contributions(&home, &cwd, &BTreeMap::new(), &policy).await;
    let second = load_enabled_plugin_contributions(&home, &cwd, &BTreeMap::new(), &policy).await;

    assert!(first.warnings.is_empty(), "{:?}", first.warnings);
    assert!(second.warnings.is_empty(), "{:?}", second.warnings);
    assert!(!top_level_git.exists());
    assert!(nested_git.join("keep").is_file());
}
