use pretty_assertions::assert_eq;
use psychevo_extension_protocol::{
    ChannelAttachment, ChannelIdentity, ChannelInboundMessage, ChannelPollResult,
    ChannelStartParams, CommandEffect, CommandRunParams, ContributionDescriptors, ExtensionSurface,
    HostCapabilities, InitializeParams, InitializeResult, PROTOCOL_VERSION, WechatQrPollResult,
};
use serde_json::json;

#[test]
fn initialize_contract_round_trips_with_the_versioned_protocol() {
    let params = InitializeParams {
        protocol: PROTOCOL_VERSION.to_owned(),
        extension_id: "example.echo".to_owned(),
        extension_version: "1.2.3".to_owned(),
        scope: "profile".to_owned(),
        package_root: "/immutable/example.echo".into(),
        data_root: "/data/example.echo".into(),
        capabilities: HostCapabilities {
            structured_displays: true,
            mcp_apps: false,
            channels: false,
        },
    };

    let value = serde_json::to_value(&params).expect("serialize initialize params");
    assert_eq!(value["protocol"], PROTOCOL_VERSION);
    assert_eq!(
        serde_json::from_value::<InitializeParams>(value).expect("deserialize initialize params"),
        params
    );

    let result = InitializeResult {
        protocol: PROTOCOL_VERSION.to_owned(),
        extension_id: "example.echo".to_owned(),
        capabilities: ContributionDescriptors::default(),
    };
    assert_eq!(
        serde_json::from_value::<InitializeResult>(
            serde_json::to_value(&result).expect("serialize initialize result")
        )
        .expect("deserialize initialize result"),
        result
    );
}

#[test]
fn wechat_qr_status_variants_require_only_their_own_fields() {
    assert_eq!(
        serde_json::from_value::<WechatQrPollResult>(json!({
            "status": "confirmed",
            "accountId": "wx-account",
            "token": "secret",
            "baseUrl": "https://ilink.example"
        }))
        .expect("confirmed response has no message"),
        WechatQrPollResult::Confirmed {
            account_id: "wx-account".to_owned(),
            token: "secret".to_owned(),
            base_url: "https://ilink.example".to_owned(),
            user_id: None,
        }
    );
    assert_eq!(
        serde_json::from_value::<WechatQrPollResult>(json!({
            "status": "scaned_but_redirect",
            "message": "follow redirect",
            "baseUrl": "https://redirect.example"
        }))
        .expect("redirect response"),
        WechatQrPollResult::ScannedRedirect {
            message: "follow redirect".to_owned(),
            base_url: "https://redirect.example".to_owned(),
        }
    );
}

#[test]
fn command_run_keeps_literal_argv_and_surface_facts() {
    let params = CommandRunParams {
        command: "echo".to_owned(),
        args: vec!["a b".to_owned(), "$(touch nope)".to_owned()],
        cwd: "/workspace".into(),
        surface: ExtensionSurface::Cli,
        interactive: false,
        terminal: false,
        host_capabilities: HostCapabilities::default(),
    };

    assert_eq!(
        serde_json::to_value(params).expect("serialize command"),
        json!({
            "command": "echo",
            "args": ["a b", "$(touch nope)"],
            "cwd": "/workspace",
            "surface": "cli",
            "interactive": false,
            "terminal": false,
            "hostCapabilities": {
                "structuredDisplays": false,
                "mcpApps": false,
                "channels": false
            }
        })
    );
}

#[test]
fn unknown_command_effect_fails_closed() {
    let error = serde_json::from_value::<CommandEffect>(json!({
        "type": "execute_host_shell",
        "command": "danger"
    }))
    .expect_err("unknown effects must not gain authority");

    assert!(error.to_string().contains("unknown variant"));
}

#[test]
fn channel_contract_round_trips_identity_attachments_and_opaque_configuration() {
    let start = ChannelStartParams {
        connection_id: "ops".to_owned(),
        channel: "wechat".to_owned(),
        configuration: json!({
            "credential": "secret",
            "futureTransportOption": { "enabled": true }
        }),
    };
    assert_eq!(
        serde_json::to_value(&start).expect("serialize start"),
        json!({
            "connectionId": "ops",
            "channel": "wechat",
            "configuration": {
                "credential": "secret",
                "futureTransportOption": { "enabled": true }
            }
        })
    );

    let poll = ChannelPollResult {
        messages: vec![ChannelInboundMessage {
            identity: ChannelIdentity {
                connection_id: Some("ops".to_owned()),
                platform: "wechat".to_owned(),
                domain: None,
                workspace_id: None,
                chat_type: Some("direct".to_owned()),
                chat_id: "chat-1".to_owned(),
                thread_id: None,
                user_id: Some("user-1".to_owned()),
                operator_id: None,
                reply_to: Some("message-0".to_owned()),
            },
            message_id: "message-1".to_owned(),
            text: "hello".to_owned(),
            attachments: vec![ChannelAttachment::File {
                filename: Some("notes.txt".to_owned()),
                mime_type: Some("text/plain".to_owned()),
                size_bytes: Some(5),
                text: Some("hello".to_owned()),
            }],
            task_key: Some("task-1".to_owned()),
        }],
    };
    assert_eq!(
        serde_json::from_value::<ChannelPollResult>(
            serde_json::to_value(&poll).expect("serialize poll")
        )
        .expect("deserialize poll"),
        poll
    );
}
