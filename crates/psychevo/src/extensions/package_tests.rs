use std::fs;
#[cfg(unix)]
use std::fs::File;
use std::io::Write;
use std::path::Path;
#[cfg(unix)]
use std::process::Command;
#[cfg(unix)]
use std::thread;
use std::time::Duration;

use flate2::{Compression, write::GzEncoder};
use pretty_assertions::assert_eq;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use super::{
    ExtensionCommandCatalog, ExtensionScope, ExtensionStore, ReleaseArtifact, ReleaseDescriptor,
    load_extension_manifest,
};

fn write_manifest(root: &Path, manifest: &str) {
    fs::create_dir_all(root).expect("extension root");
    fs::write(root.join("psychevo.extension.json"), manifest).expect("extension manifest");
}

#[test]
fn manifest_loads_static_commands_without_starting_an_executable() {
    let temp = TempDir::new().expect("temp");
    write_manifest(
        temp.path(),
        r#"{
          "schemaVersion": 1,
          "id": "example.echo",
          "version": "local",
          "runtime": {
            "protocol": "psychevo-extension/1",
            "executable": "./echo-sidecar",
            "args": ["--stdio"]
          },
          "contributions": {
            "commands": [{
              "name": "echo",
              "usage": "pevo echo [ARGS]...",
              "summary": "Echo literal arguments",
              "argumentKind": "trailing_args",
              "surfaces": ["cli", "tui"]
            }]
          }
        }"#,
    );

    let manifest = load_extension_manifest(temp.path()).expect("load extension");

    assert_eq!(manifest.id, "example.echo");
    assert_eq!(
        manifest.runtime.executable,
        temp.path().join("echo-sidecar")
    );
    assert_eq!(manifest.contributions.commands[0].name, "echo");
    assert!(manifest.plugin_manifest.is_none());
}

#[test]
fn manifest_requires_mcp_app_resource_origin_and_fallback_evidence() {
    let valid = TempDir::new().expect("temp");
    write_manifest(
        valid.path(),
        r#"{
          "schemaVersion": 1,
          "id": "example.app",
          "version": "local",
          "runtime": {
            "protocol": "psychevo-extension/1",
            "executable": "./sidecar"
          },
          "contributions": {
            "mcpApps": [{
              "id": "dashboard",
              "resourceUri": "ui://example/dashboard.html",
              "fallback": "Dashboard is available in Web and Desktop.",
              "resourceUrl": "https://apps.example.test/dashboard.html",
              "resourceDomains": ["https://apps.example.test"],
              "connectDomains": ["https://api.example.test"],
              "allowedTools": ["example.lookup"]
            }]
          }
        }"#,
    );
    let manifest = load_extension_manifest(valid.path()).expect("valid MCP App descriptor");
    assert_eq!(
        manifest.contributions.mcp_apps[0].allowed_tools,
        ["example.lookup"]
    );

    let invalid = TempDir::new().expect("temp");
    write_manifest(
        invalid.path(),
        r#"{
          "schemaVersion": 1,
          "id": "example.app",
          "version": "local",
          "runtime": {
            "protocol": "psychevo-extension/1",
            "executable": "./sidecar"
          },
          "contributions": {
            "mcpApps": [{
              "id": "dashboard",
              "resourceUri": "ui://example/dashboard.html",
              "fallback": "Use the text fallback.",
              "resourceUrl": "https://other.example.test/dashboard.html",
              "resourceDomains": ["https://apps.example.test"]
            }]
          }
        }"#,
    );
    let error = load_extension_manifest(invalid.path()).expect_err("undeclared resource origin");
    assert!(
        error
            .to_string()
            .contains("is not declared in resourceDomains")
    );
}

#[test]
fn manifest_rejects_unsafe_executable_and_two_plugin_bases() {
    let unsafe_root = TempDir::new().expect("temp");
    write_manifest(
        unsafe_root.path(),
        r#"{
          "schemaVersion": 1,
          "id": "example.unsafe",
          "version": "local",
          "runtime": {
            "protocol": "psychevo-extension/1",
            "executable": "../outside"
          }
        }"#,
    );
    let error = load_extension_manifest(unsafe_root.path()).expect_err("unsafe executable");
    assert!(error.to_string().contains("explicit package-relative path"));

    let duplicate_root = TempDir::new().expect("temp");
    write_manifest(
        duplicate_root.path(),
        r#"{
          "schemaVersion": 1,
          "id": "example.duplicate",
          "version": "local",
          "runtime": {
            "protocol": "psychevo-extension/1",
            "executable": "./sidecar"
          }
        }"#,
    );
    fs::create_dir_all(duplicate_root.path().join(".codex-plugin")).expect("codex dir");
    fs::create_dir_all(duplicate_root.path().join(".claude-plugin")).expect("claude dir");
    fs::write(
        duplicate_root.path().join(".codex-plugin/plugin.json"),
        r#"{"name":"one"}"#,
    )
    .expect("codex plugin");
    fs::write(
        duplicate_root.path().join(".claude-plugin/plugin.json"),
        r#"{"name":"two"}"#,
    )
    .expect("claude plugin");

    let error = load_extension_manifest(duplicate_root.path()).expect_err("two plugin bases");
    assert!(error.to_string().contains("at most one co-root Plugin"));
}

#[test]
fn command_catalog_rejects_builtins_and_duplicate_enabled_extensions() {
    let first = manifest_with_command("example.first", "review");
    let second = manifest_with_command("example.second", "review");
    let builtin = manifest_with_command("example.shadow", "run");

    let error = ExtensionCommandCatalog::build(&[first.clone(), second], &["run", "plugin"])
        .expect_err("duplicate command");
    assert!(error.to_string().contains("example.first"));
    assert!(error.to_string().contains("example.second"));

    let error = ExtensionCommandCatalog::build(&[builtin], &["run", "plugin"])
        .expect_err("builtin conflict");
    assert!(error.to_string().contains("built-in"));

    let catalog =
        ExtensionCommandCatalog::build(&[first], &["run", "plugin"]).expect("unique catalog");
    assert_eq!(catalog.owner("review"), Some("example.first"));
}

#[test]
fn local_install_is_in_place_enabled_and_fingerprint_trusted() {
    let profile = TempDir::new().expect("profile");
    let source = TempDir::new().expect("source");
    write_manifest(
        source.path(),
        r#"{
          "schemaVersion": 1,
          "id": "example.local",
          "version": "local",
          "runtime": {
            "protocol": "psychevo-extension/1",
            "executable": "./sidecar"
          }
        }"#,
    );
    fs::write(source.path().join("sidecar"), b"fixture").expect("sidecar");
    let store = ExtensionStore::new(profile.path(), profile.path().join("workspace"));

    let record = store
        .install_local(source.path(), ExtensionScope::Profile)
        .expect("install local");

    assert_eq!(
        record.package_root,
        source.path().canonicalize().expect("root")
    );
    assert!(record.enabled);
    assert_eq!(record.trusted_fingerprint, record.fingerprint);
    assert!(store.cache_root(ExtensionScope::Profile).is_dir());
    assert!(
        store
            .data_root(ExtensionScope::Profile)
            .join("example.local")
            .is_dir()
    );
}

#[test]
fn channel_resolution_is_static_unique_trusted_and_actionable_when_missing() {
    let profile = TempDir::new().expect("profile");
    let workspace = profile.path().join("workspace");
    let source = TempDir::new().expect("source");
    let duplicate = TempDir::new().expect("duplicate");
    write_manifest(
        source.path(),
        r#"{
          "schemaVersion": 1,
          "id": "psychevo.channel.telegram",
          "version": "local",
          "runtime": {"protocol": "psychevo-extension/1", "executable": "./sidecar"},
          "contributions": {"channels": [{"channel": "telegram", "deliveryCapabilities": ["poll", "text"]}]}
        }"#,
    );
    fs::write(source.path().join("sidecar"), b"not started").expect("sidecar");
    let store = ExtensionStore::new(profile.path(), &workspace);
    store
        .install_local(source.path(), ExtensionScope::Profile)
        .expect("install");

    let (record, manifest) = store
        .resolve_channel_extension("telegram")
        .expect("resolve Channel");
    assert_eq!(record.id, "psychevo.channel.telegram");
    assert_eq!(manifest.contributions.channels[0].channel, "telegram");

    let error = store
        .resolve_channel_extension("wechat")
        .expect_err("missing Channel Extension");
    assert!(
        error
            .to_string()
            .contains("pevo install psychevo.channel.wechat")
    );

    write_manifest(
        duplicate.path(),
        r#"{
          "schemaVersion": 1,
          "id": "example.alternate-telegram",
          "version": "local",
          "runtime": {"protocol": "psychevo-extension/1", "executable": "./sidecar"},
          "contributions": {"channels": [{"channel": "telegram", "deliveryCapabilities": ["poll"]}]}
        }"#,
    );
    fs::write(duplicate.path().join("sidecar"), b"not started").expect("duplicate sidecar");
    store
        .install_local(duplicate.path(), ExtensionScope::Profile)
        .expect("install duplicate Channel owner");
    let error = store
        .resolve_channel_extension("telegram")
        .expect_err("ambiguous Channel Extension");
    assert!(error.to_string().contains("multiple enabled Extensions"));

    store
        .set_enabled("example.alternate-telegram", ExtensionScope::Profile, false)
        .expect("disable duplicate Channel owner");
    fs::write(source.path().join("sidecar"), b"changed after install").expect("tamper sidecar");
    let error = store
        .resolve_channel_extension("telegram")
        .expect_err("changed Channel Extension");
    assert!(error.to_string().contains("changed after installation"));
}

#[test]
#[cfg(unix)]
fn activity_lock_serializes_installation_mutation_across_os_processes() {
    const CHILD_LOCK: &str = "PSYCHEVO_TEST_EXTENSION_CHILD_LOCK";
    const CHILD_READY: &str = "PSYCHEVO_TEST_EXTENSION_CHILD_READY";
    const CHILD_RELEASE: &str = "PSYCHEVO_TEST_EXTENSION_CHILD_RELEASE";

    if let Some(lock_path) = std::env::var_os(CHILD_LOCK) {
        let ready =
            std::path::PathBuf::from(std::env::var_os(CHILD_READY).expect("child ready path"));
        let release =
            std::path::PathBuf::from(std::env::var_os(CHILD_RELEASE).expect("child release path"));
        let lock = File::options()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .expect("open activity lock in child");
        lock.lock_shared()
            .expect("hold shared activity lock in child");
        fs::write(&ready, b"ready").expect("publish child lock readiness");
        for _ in 0..1_000 {
            if release.exists() {
                lock.unlock().expect("release child activity lock");
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("parent did not release child activity lock");
    }

    let profile = TempDir::new().expect("profile");
    let workspace = profile.path().join("workspace");
    let source = TempDir::new().expect("source");
    write_manifest(
        source.path(),
        r#"{
          "schemaVersion": 1,
          "id": "example.process-lock",
          "version": "local",
          "runtime": {"protocol": "psychevo-extension/1", "executable": "./sidecar"}
        }"#,
    );
    fs::write(source.path().join("sidecar"), b"not started").expect("sidecar");
    let store = ExtensionStore::new(profile.path(), &workspace);
    store
        .install_local(source.path(), ExtensionScope::Profile)
        .expect("install Extension before lock test");

    let lock_path = store
        .data_root(ExtensionScope::Profile)
        .join("example.process-lock/.activity.lock");
    let ready = profile.path().join("child.ready");
    let release = profile.path().join("child.release");
    let mut child = Command::new(std::env::current_exe().expect("current test executable"))
        .args([
            "--exact",
            "extensions::package_tests::activity_lock_serializes_installation_mutation_across_os_processes",
            "--nocapture",
        ])
        .env(CHILD_LOCK, &lock_path)
        .env(CHILD_READY, &ready)
        .env(CHILD_RELEASE, &release)
        .spawn()
        .expect("spawn lock-holder test process");
    for _ in 0..1_000 {
        if ready.exists() {
            break;
        }
        assert!(
            child.try_wait().expect("poll child").is_none(),
            "lock-holder child exited early"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert!(ready.exists(), "lock-holder child did not become ready");

    let error = store
        .set_enabled("example.process-lock", ExtensionScope::Profile, false)
        .expect_err("separate process activity lock must block mutation");
    assert!(error.to_string().contains("active sidecar lease"));

    fs::write(&release, b"release").expect("release lock-holder child");
    let status = child.wait().expect("wait for lock-holder child");
    assert!(status.success(), "lock-holder child failed: {status}");
    let record = store
        .set_enabled("example.process-lock", ExtensionScope::Profile, false)
        .expect("mutation after separate process releases activity lock");
    assert!(!record.enabled);
}

#[test]
fn activation_rejects_command_conflicts_before_publishing_records() {
    let profile = TempDir::new().expect("profile");
    let workspace = TempDir::new().expect("workspace");
    let first = TempDir::new().expect("first");
    let duplicate = TempDir::new().expect("duplicate");
    let builtin = TempDir::new().expect("builtin");
    let slash_builtin = TempDir::new().expect("slash builtin");
    write_local_extension_with_command(first.path(), "example.first", "review");
    write_local_extension_with_command(duplicate.path(), "example.duplicate", "review");
    write_local_extension_with_command(builtin.path(), "example.builtin", "run");
    write_local_extension_with_command(slash_builtin.path(), "example.slash", "help");
    let store = ExtensionStore::new(profile.path(), workspace.path());

    store
        .install_local(first.path(), ExtensionScope::Profile)
        .expect("install first command owner");
    let error = store
        .install_local(duplicate.path(), ExtensionScope::Profile)
        .expect_err("reject duplicate command owner");
    assert!(error.to_string().contains("example.first"));
    assert!(error.to_string().contains("example.duplicate"));
    assert!(
        store
            .read_record("example.duplicate", ExtensionScope::Profile)
            .expect("read duplicate record")
            .is_none()
    );

    let error = store
        .install_local(builtin.path(), ExtensionScope::Profile)
        .expect_err("reject builtin command shadowing");
    assert!(error.to_string().contains("built-in pevo command"));
    assert!(
        store
            .read_record("example.builtin", ExtensionScope::Profile)
            .expect("read builtin record")
            .is_none()
    );

    let error = store
        .install_local(slash_builtin.path(), ExtensionScope::Profile)
        .expect_err("reject built-in TUI command shadowing");
    assert!(error.to_string().contains("built-in pevo command"));
}

#[test]
fn removing_project_override_cannot_reveal_a_conflicting_profile_command() {
    let profile = TempDir::new().expect("profile");
    let workspace = TempDir::new().expect("workspace");
    let first = TempDir::new().expect("first");
    let shadowed_profile = TempDir::new().expect("shadowed profile");
    let project_override = TempDir::new().expect("project override");
    write_local_extension_with_command(first.path(), "example.first", "review");
    write_local_extension_with_command(shadowed_profile.path(), "example.shadowed", "review");
    write_local_extension_with_command(
        project_override.path(),
        "example.shadowed",
        "project-review",
    );
    let store = ExtensionStore::new(profile.path(), workspace.path());

    store
        .install_local(first.path(), ExtensionScope::Profile)
        .expect("install first owner");
    store
        .install_local(project_override.path(), ExtensionScope::Local)
        .expect("install project override");
    store
        .install_local(shadowed_profile.path(), ExtensionScope::Profile)
        .expect("install shadowed profile record");

    let error = store
        .remove("example.shadowed", ExtensionScope::Local)
        .expect_err("removal would reveal conflict");
    assert!(error.to_string().contains("conflicts between"));
    assert!(
        store
            .read_record("example.shadowed", ExtensionScope::Local)
            .expect("read retained override")
            .is_some()
    );
}

#[test]
fn remote_archive_verifies_digest_and_publishes_record_last() {
    let profile = TempDir::new().expect("profile");
    let store = ExtensionStore::new(profile.path(), profile.path().join("workspace"));
    let first = extension_archive("example.remote", "1.0.0", b"first");
    let first_digest = format!("{:x}", Sha256::digest(&first));
    let first_descriptor = release_descriptor("example.remote", "1.0.0", &first_digest);

    let first_record = store
        .install_remote_archive_for_test(
            &first_descriptor,
            "https://extensions.example/release.json",
            "x86_64-unknown-linux-gnu",
            &first,
            ExtensionScope::Profile,
        )
        .expect("install first artifact");
    assert_eq!(first_record.version, "1.0.0");
    assert!(first_record.package_root.join("sidecar").is_file());

    let second = extension_archive("example.remote", "2.0.0", b"second");
    let second_descriptor = release_descriptor("example.remote", "2.0.0", &"0".repeat(64));
    let error = store
        .install_remote_archive_for_test(
            &second_descriptor,
            "https://extensions.example/release.json",
            "x86_64-unknown-linux-gnu",
            &second,
            ExtensionScope::Profile,
        )
        .expect_err("digest mismatch");
    assert!(error.to_string().contains("SHA-256 mismatch"));

    let retained = store
        .read_record("example.remote", ExtensionScope::Profile)
        .expect("read retained record")
        .expect("retained record");
    assert_eq!(retained, first_record);
}

#[test]
fn remote_archive_update_preserves_disabled_policy() {
    let profile = TempDir::new().expect("profile");
    let store = ExtensionStore::new(profile.path(), profile.path().join("workspace"));
    let first = extension_archive("example.remote-disabled", "1.0.0", b"first");
    let first_descriptor = release_descriptor(
        "example.remote-disabled",
        "1.0.0",
        &format!("{:x}", Sha256::digest(&first)),
    );
    store
        .install_remote_archive_for_test(
            &first_descriptor,
            "https://extensions.example/release.json",
            "x86_64-unknown-linux-gnu",
            &first,
            ExtensionScope::Profile,
        )
        .expect("install first artifact");
    store
        .set_enabled("example.remote-disabled", ExtensionScope::Profile, false)
        .expect("disable installed Extension");

    let second = extension_archive("example.remote-disabled", "2.0.0", b"second");
    let second_descriptor = release_descriptor(
        "example.remote-disabled",
        "2.0.0",
        &format!("{:x}", Sha256::digest(&second)),
    );
    let updated = store
        .install_remote_archive_for_test(
            &second_descriptor,
            "https://extensions.example/release.json",
            "x86_64-unknown-linux-gnu",
            &second,
            ExtensionScope::Profile,
        )
        .expect("update disabled Extension");

    assert_eq!(updated.version, "2.0.0");
    assert!(!updated.enabled);
    assert_eq!(
        store
            .read_record("example.remote-disabled", ExtensionScope::Profile)
            .expect("read updated record")
            .expect("updated record")
            .enabled,
        false
    );
}

#[tokio::test]
async fn remote_package_client_refuses_every_plain_http_request() {
    let client = super::package::remote_package_client().expect("remote package client");
    let error = tokio::time::timeout(
        Duration::from_millis(250),
        client.get("http://127.0.0.1:9/plaintext-hop").send(),
    )
    .await
    .expect("HTTPS-only rejection must happen before network I/O")
    .expect_err("HTTPS-only client must reject a plaintext redirect hop");
    assert!(
        error.is_builder(),
        "unexpected HTTPS-only rejection: {error}"
    );
    assert_eq!(
        error.url().map(|url| url.scheme()),
        Some("http"),
        "the rejection must identify the plaintext hop"
    );
}

#[test]
fn release_descriptor_requires_the_selected_precompiled_target() {
    let descriptor = release_descriptor("example.remote", "1.0.0", &"a".repeat(64));
    let error = descriptor
        .artifact_for_target("aarch64-apple-darwin")
        .expect_err("missing target");
    assert!(error.to_string().contains("aarch64-apple-darwin"));
    assert!(error.to_string().contains("precompiled artifact"));
}

#[test]
fn project_records_override_profile_and_remove_retains_data() {
    let profile = TempDir::new().expect("profile");
    let workspace = TempDir::new().expect("workspace");
    let profile_source = TempDir::new().expect("profile source");
    let local_source = TempDir::new().expect("local source");
    write_local_extension(profile_source.path(), "example.layered", b"profile");
    write_local_extension(local_source.path(), "example.layered", b"local");
    let store = ExtensionStore::new(profile.path(), workspace.path());
    store
        .install_local(profile_source.path(), ExtensionScope::Profile)
        .expect("profile install");
    let local = store
        .install_local(local_source.path(), ExtensionScope::Local)
        .expect("local install");

    let effective = store.effective_records().expect("effective records");
    assert_eq!(effective.len(), 1);
    assert_eq!(effective[0].package_root, local.package_root);

    let data_root = local.data_root.clone();
    let removed = store
        .remove("example.layered", ExtensionScope::Local)
        .expect("remove local")
        .expect("removed record");
    assert_eq!(removed.package_root, local.package_root);
    assert!(data_root.is_dir());
    assert_eq!(
        store.effective_records().expect("profile fallback")[0].scope,
        ExtensionScope::Profile
    );
}

#[test]
fn extension_policy_toggle_preserves_exact_trust_and_package() {
    let profile = TempDir::new().expect("profile");
    let source = TempDir::new().expect("source");
    write_local_extension(source.path(), "example.toggle", b"sidecar");
    let store = ExtensionStore::new(profile.path(), profile.path().join("workspace"));
    let installed = store
        .install_local(source.path(), ExtensionScope::Profile)
        .expect("install");

    let disabled = store
        .set_enabled("example.toggle", ExtensionScope::Profile, false)
        .expect("disable");
    assert!(!disabled.enabled);
    assert_eq!(disabled.package_root, installed.package_root);
    assert_eq!(disabled.trusted_fingerprint, installed.trusted_fingerprint);

    let enabled = store
        .set_enabled("example.toggle", ExtensionScope::Profile, true)
        .expect("enable");
    assert!(enabled.enabled);
    assert_eq!(enabled.fingerprint, installed.fingerprint);
}

fn manifest_with_command(id: &str, command: &str) -> super::ExtensionManifest {
    let temp = TempDir::new().expect("temp");
    write_manifest(
        temp.path(),
        &format!(
            r#"{{
              "schemaVersion": 1,
              "id": "{id}",
              "version": "local",
              "runtime": {{
                "protocol": "psychevo-extension/1",
                "executable": "./sidecar"
              }},
              "contributions": {{
                "commands": [{{
                  "name": "{command}",
                  "usage": "pevo {command}",
                  "summary": "fixture"
                }}]
              }}
            }}"#
        ),
    );
    load_extension_manifest(temp.path()).expect("fixture manifest")
}

fn release_descriptor(id: &str, version: &str, digest: &str) -> ReleaseDescriptor {
    ReleaseDescriptor {
        schema_version: 1,
        id: id.to_owned(),
        version: version.to_owned(),
        artifacts: [(
            "x86_64-unknown-linux-gnu".to_owned(),
            ReleaseArtifact {
                url: "https://extensions.example/example.tar.gz".to_owned(),
                sha256: digest.to_owned(),
                format: "tar.gz".to_owned(),
                executable: "./sidecar".to_owned(),
                size: None,
            },
        )]
        .into_iter()
        .collect(),
    }
}

fn extension_archive(id: &str, version: &str, sidecar: &[u8]) -> Vec<u8> {
    let output = Vec::new();
    let encoder = GzEncoder::new(output, Compression::fast());
    let mut archive = tar::Builder::new(encoder);
    append_archive_file(
        &mut archive,
        "psychevo.extension.json",
        format!(
            r#"{{
              "schemaVersion": 1,
              "id": "{id}",
              "version": "{version}",
              "runtime": {{
                "protocol": "psychevo-extension/1",
                "executable": "./sidecar"
              }}
            }}"#
        )
        .as_bytes(),
        0o644,
    );
    append_archive_file(&mut archive, "sidecar", sidecar, 0o755);
    let encoder = archive.into_inner().expect("finish tar");
    encoder.finish().expect("finish gzip")
}

fn append_archive_file<W: Write>(
    archive: &mut tar::Builder<W>,
    path: &str,
    bytes: &[u8],
    mode: u32,
) {
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(mode);
    header.set_cksum();
    archive
        .append_data(&mut header, path, bytes)
        .expect("archive file");
}

fn write_local_extension(root: &Path, id: &str, sidecar: &[u8]) {
    write_manifest(
        root,
        &format!(
            r#"{{
              "schemaVersion": 1,
              "id": "{id}",
              "version": "local",
              "runtime": {{
                "protocol": "psychevo-extension/1",
                "executable": "./sidecar"
              }}
            }}"#
        ),
    );
    fs::write(root.join("sidecar"), sidecar).expect("sidecar");
}

fn write_local_extension_with_command(root: &Path, id: &str, command: &str) {
    write_manifest(
        root,
        &format!(
            r#"{{
              "schemaVersion": 1,
              "id": "{id}",
              "version": "local",
              "runtime": {{
                "protocol": "psychevo-extension/1",
                "executable": "./sidecar"
              }},
              "contributions": {{
                "commands": [{{
                  "name": "{command}",
                  "usage": "pevo {command}",
                  "summary": "fixture"
                }}]
              }}
            }}"#
        ),
    );
    fs::write(root.join("sidecar"), b"fixture").expect("sidecar");
}
