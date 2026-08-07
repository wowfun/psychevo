use std::collections::BTreeMap;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

use psychevo_extension_protocol::{
    ChannelConnectionParams, ChannelIdentity, ChannelOutboundMessage, ChannelSendParams,
    ChannelStartParams, CommandEffect, CommandRunParams, ExtensionSurface, HostCapabilities,
};
use serde_json::json;
use tempfile::TempDir;

use super::{ExtensionHostMode, ExtensionRuntime, ExtensionScope, ExtensionStore};

#[tokio::test]
async fn one_shot_host_is_lazy_preserves_argv_and_shuts_down() {
    let profile = TempDir::new().expect("profile");
    let source = TempDir::new().expect("source");
    write_sidecar_extension(source.path(), "example.echo");
    let store = ExtensionStore::new(profile.path(), source.path());
    let record = store
        .install_local(source.path(), ExtensionScope::Profile)
        .expect("install");
    let manifest = super::load_extension_manifest(source.path()).expect("manifest");
    let marker = record.data_root.join("lifecycle.log");
    let runtime = ExtensionRuntime::new(
        record,
        manifest,
        BTreeMap::new(),
        ExtensionHostMode::OneShot,
    )
    .expect("runtime");

    assert!(!runtime.started().await);
    let lease = runtime.acquire().await.expect("lease");
    assert!(!runtime.started().await);

    let effect = lease
        .command_run(CommandRunParams {
            command: "echo".to_owned(),
            args: vec!["a b".to_owned(), "$(touch nope)".to_owned()],
            cwd: source.path().to_path_buf(),
            surface: ExtensionSurface::Cli,
            interactive: false,
            terminal: false,
            host_capabilities: Default::default(),
        })
        .await
        .expect("command");
    assert_eq!(
        effect,
        CommandEffect::BoundedText {
            text: r#"["a b", "$(touch nope)"]"#.to_owned()
        }
    );
    assert!(runtime.started().await);

    lease.release().await.expect("release");
    assert!(!runtime.started().await);
    assert_eq!(
        fs::read_to_string(marker).expect("lifecycle marker"),
        "initialize\nshutdown\n"
    );
}

#[tokio::test]
async fn leased_host_cancels_and_restarts_the_five_minute_idle_deadline() {
    let profile = TempDir::new().expect("profile");
    let source = TempDir::new().expect("source");
    write_sidecar_extension(source.path(), "example.pooled");
    let store = ExtensionStore::new(profile.path(), source.path());
    let record = store
        .install_local(source.path(), ExtensionScope::Profile)
        .expect("install");
    let manifest = super::load_extension_manifest(source.path()).expect("manifest");
    let runtime = ExtensionRuntime::new(
        record,
        manifest,
        BTreeMap::new(),
        ExtensionHostMode::Leased {
            idle_timeout: Duration::from_secs(300),
        },
    )
    .expect("runtime");

    let first = runtime.acquire().await.expect("first lease");
    first.contributions().await.expect("start sidecar");
    tokio::time::pause();
    first.release().await.expect("release first");
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(299)).await;
    assert!(runtime.started().await);

    let second = runtime.acquire().await.expect("second lease");
    tokio::time::advance(Duration::from_secs(2)).await;
    assert!(runtime.started().await);
    second.release().await.expect("release second");
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(300)).await;
    tokio::time::resume();
    for _ in 0..50 {
        if !runtime.started().await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!runtime.started().await);
}

#[tokio::test]
async fn active_runtime_lease_blocks_independent_store_mutation() {
    let profile = TempDir::new().expect("profile");
    let source = TempDir::new().expect("source");
    write_sidecar_extension(source.path(), "example.locked");
    let store = ExtensionStore::new(profile.path(), source.path());
    let record = store
        .install_local(source.path(), ExtensionScope::Profile)
        .expect("install");
    let manifest = super::load_extension_manifest(source.path()).expect("manifest");
    let runtime = ExtensionRuntime::new(
        record,
        manifest,
        BTreeMap::new(),
        ExtensionHostMode::OneShot,
    )
    .expect("runtime");

    let lease = runtime.acquire().await.expect("lease");
    let error = store
        .set_enabled("example.locked", ExtensionScope::Profile, false)
        .expect_err("active lease must block mutation");
    assert!(
        error.to_string().contains("active sidecar lease"),
        "unexpected error: {error}"
    );

    lease.release().await.expect("release");
    let record = store
        .set_enabled("example.locked", ExtensionScope::Profile, false)
        .expect("mutation after release");
    assert!(!record.enabled);
}

#[tokio::test]
async fn channel_lease_negotiates_capability_and_routes_full_lifecycle() {
    let profile = TempDir::new().expect("profile");
    let source = TempDir::new().expect("source");
    write_sidecar_extension(source.path(), "example.channel");
    let store = ExtensionStore::new(profile.path(), source.path());
    let record = store
        .install_local(source.path(), ExtensionScope::Profile)
        .expect("install");
    let manifest = super::load_extension_manifest(source.path()).expect("manifest");
    let runtime = ExtensionRuntime::with_capabilities(
        record,
        manifest,
        BTreeMap::new(),
        ExtensionHostMode::Leased {
            idle_timeout: Duration::from_secs(300),
        },
        HostCapabilities {
            channels: true,
            ..HostCapabilities::default()
        },
    )
    .expect("runtime");
    let lease = runtime.acquire().await.expect("lease");

    lease
        .channel_start(ChannelStartParams {
            connection_id: "test".to_owned(),
            channel: "test".to_owned(),
            configuration: json!({ "credential": "literal-secret" }),
        })
        .await
        .expect("start");
    let poll = lease
        .channel_poll(ChannelConnectionParams {
            connection_id: "test".to_owned(),
        })
        .await
        .expect("poll");
    assert_eq!(poll.messages.len(), 1);
    assert_eq!(
        poll.messages[0].identity.connection_id.as_deref(),
        Some("test")
    );
    assert_eq!(poll.messages[0].text, "inbound");

    lease
        .channel_send(ChannelSendParams {
            connection_id: "test".to_owned(),
            message: ChannelOutboundMessage {
                identity: ChannelIdentity {
                    connection_id: Some("test".to_owned()),
                    platform: "test".to_owned(),
                    domain: None,
                    workspace_id: None,
                    chat_type: None,
                    chat_id: "chat".to_owned(),
                    thread_id: None,
                    user_id: None,
                    operator_id: None,
                    reply_to: None,
                },
                thread_id: "thread".to_owned(),
                text: "outbound".to_owned(),
            },
        })
        .await
        .expect("send");
    assert_eq!(
        lease
            .channel_control("channel/test/status", json!({ "connectionId": "test" }))
            .await
            .expect("control"),
        json!({ "started": true, "lastSent": "outbound" })
    );
    assert!(
        lease
            .channel_control("unsafe/control", json!({}))
            .await
            .expect_err("non-Channel method")
            .to_string()
            .contains("must start with `channel/`")
    );
    lease
        .channel_stop(ChannelConnectionParams {
            connection_id: "test".to_owned(),
        })
        .await
        .expect("stop");
    lease.release().await.expect("release");
    runtime.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn channel_send_can_complete_while_a_long_poll_is_pending() {
    let profile = TempDir::new().expect("profile");
    let source = TempDir::new().expect("source");
    write_sidecar_extension(source.path(), "example.concurrent-channel");
    let store = ExtensionStore::new(profile.path(), source.path());
    let record = store
        .install_local(source.path(), ExtensionScope::Profile)
        .expect("install");
    let manifest = super::load_extension_manifest(source.path()).expect("manifest");
    let runtime = ExtensionRuntime::with_capabilities(
        record,
        manifest,
        BTreeMap::new(),
        ExtensionHostMode::Leased {
            idle_timeout: Duration::from_secs(300),
        },
        HostCapabilities {
            channels: true,
            ..HostCapabilities::default()
        },
    )
    .expect("runtime");
    let lease = runtime.acquire().await.expect("lease");
    lease
        .channel_start(ChannelStartParams {
            connection_id: "test".to_owned(),
            channel: "test".to_owned(),
            configuration: json!({ "blockPollUntilSend": true }),
        })
        .await
        .expect("start");

    let poll = lease.channel_poll(ChannelConnectionParams {
        connection_id: "test".to_owned(),
    });
    let send = async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        lease
            .channel_send(ChannelSendParams {
                connection_id: "test".to_owned(),
                message: ChannelOutboundMessage {
                    identity: ChannelIdentity {
                        connection_id: Some("test".to_owned()),
                        platform: "test".to_owned(),
                        domain: None,
                        workspace_id: None,
                        chat_type: None,
                        chat_id: "chat".to_owned(),
                        thread_id: None,
                        user_id: None,
                        operator_id: None,
                        reply_to: None,
                    },
                    thread_id: "thread".to_owned(),
                    text: "release poll".to_owned(),
                },
            })
            .await
    };
    let (poll_result, send_result) =
        tokio::time::timeout(Duration::from_secs(1), async { tokio::join!(poll, send) })
            .await
            .expect("send must not queue behind long poll");
    assert_eq!(poll_result.expect("poll").messages.len(), 1);
    send_result.expect("send");

    lease.release().await.expect("release");
    runtime.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn a_call_after_timeout_rebuilds_the_terminated_session() {
    let profile = TempDir::new().expect("profile");
    let source = TempDir::new().expect("source");
    write_sidecar_extension(source.path(), "example.restart-channel");
    let store = ExtensionStore::new(profile.path(), source.path());
    let record = store
        .install_local(source.path(), ExtensionScope::Profile)
        .expect("install");
    let marker = record.data_root.join("lifecycle.log");
    let manifest = super::load_extension_manifest(source.path()).expect("manifest");
    let runtime = ExtensionRuntime::with_capabilities(
        record,
        manifest,
        BTreeMap::new(),
        ExtensionHostMode::Leased {
            idle_timeout: Duration::from_secs(300),
        },
        HostCapabilities {
            channels: true,
            ..HostCapabilities::default()
        },
    )
    .expect("runtime");
    let lease = runtime.acquire().await.expect("lease");
    lease
        .channel_start(ChannelStartParams {
            connection_id: "restartable".to_owned(),
            channel: "test".to_owned(),
            configuration: json!({ "credential": "preserved" }),
        })
        .await
        .expect("start channel before transport failure");

    let error = lease
        .channel_control("channel/test/hang", json!({}))
        .await
        .expect_err("hung call times out");
    assert!(error.to_string().contains("timed out"));
    assert_eq!(
        lease
            .channel_control(
                "channel/test/status",
                json!({ "connectionId": "restartable" }),
            )
            .await
            .expect("fresh session call"),
        json!({ "started": true, "lastSent": null }),
        "the replacement session must restore active Channel connections"
    );

    let lifecycle = fs::read_to_string(marker).expect("lifecycle log");
    assert_eq!(
        lifecycle
            .lines()
            .filter(|line| *line == "initialize")
            .count(),
        2
    );
    lease.release().await.expect("release");
    runtime.shutdown().await.expect("shutdown");
}

fn write_sidecar_extension(root: &Path, id: &str) {
    let executable = if cfg!(windows) {
        "./sidecar.cmd"
    } else {
        "./sidecar.py"
    };
    fs::write(
        root.join("psychevo.extension.json"),
        format!(
            r#"{{
              "schemaVersion": 1,
              "id": "{id}",
              "version": "local",
              "runtime": {{
                "protocol": "psychevo-extension/1",
                "executable": "{executable}"
              }},
              "contributions": {{
                "commands": [{{
                  "name": "echo",
                  "usage": "pevo echo [ARGS]...",
                  "summary": "Echo literal arguments"
                }}],
                "channels": [{{
                  "channel": "test",
                  "deliveryCapabilities": ["poll", "text"]
                }}]
              }}
            }}"#
        ),
    )
    .expect("manifest");
    fs::write(
        root.join("sidecar.py"),
        include_str!("../../tests/fixtures/extension_echo_sidecar.py"),
    )
    .expect("python sidecar");
    #[cfg(unix)]
    {
        let script = root.join("sidecar.py");
        let mut permissions = fs::metadata(&script).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(script, permissions).expect("chmod");
    }
    #[cfg(windows)]
    fs::write(
        root.join("sidecar.cmd"),
        "@echo off\r\npython \"%~dp0sidecar.py\" %*\r\n",
    )
    .expect("windows sidecar");
}
