#[allow(unused_imports)]
pub(crate) use super::*;

use crate::config::config_parse::parse_run_config;

#[test]
pub(crate) fn parse_channels_config_defaults_and_channel_constraints() {
    let config = parse_run_config(json!({
        "channels": {
            "connections": [
                {
                    "id": "release_bot",
                    "channel": "telegram",
                    "label": "Release Bot",
                    "allow_users": ["12345"]
                },
                {
                    "id": "wechat",
                    "channel": "wechat",
                    "allow_users": ["wx_user"]
                },
                {
                    "id": "feishu_ops",
                    "channel": "feishu",
                    "allow_groups": ["oc_abc"],
                    "credential_env": "FEISHU_APP_SECRET",
                    "app_id_env": "FEISHU_APP_ID"
                }
            ]
        }
    }))
    .expect("channels config");

    let telegram = &config.channels.connections[0];
    assert_eq!(telegram.platform.as_str(), "telegram");
    assert_eq!(telegram.transport.as_str(), "polling");
    assert_eq!(telegram.label, "Release Bot");
    assert!(!telegram.enabled);
    assert!(telegram.require_mention);
    assert_eq!(
        telegram.credential_env.as_deref(),
        Some("TELEGRAM_BOT_TOKEN")
    );
    assert_eq!(telegram.allow_users, ["12345"]);

    let wechat = &config.channels.connections[1];
    assert_eq!(wechat.platform.as_str(), "wechat");
    assert_eq!(wechat.account_env.as_deref(), Some("WECHAT_ACCOUNT_ID"));
    assert_eq!(
        wechat.base_url_env.as_deref(),
        Some("WECHAT_ILINK_BASE_URL")
    );

    let feishu = &config.channels.connections[2];
    assert_eq!(feishu.platform.as_str(), "feishu");
    assert_eq!(feishu.transport.as_str(), "long_connection");
    assert_eq!(feishu.domain.as_deref(), Some("feishu"));
    assert_eq!(feishu.app_id_env.as_deref(), Some("FEISHU_APP_ID"));
    assert_eq!(feishu.allow_groups, ["oc_abc"]);
}

#[test]
pub(crate) fn parse_channels_config_rejects_duplicate_ids_and_invalid_transport() {
    let duplicate = parse_run_config(json!({
        "channels": {
            "connections": [
                {"id": "release", "channel": "telegram"},
                {"id": "release", "channel": "wechat"}
            ]
        }
    }))
    .expect_err("duplicate id");
    assert!(
        duplicate
            .to_string()
            .contains("duplicate channel connection id"),
        "{duplicate:#}"
    );

    let invalid_transport = parse_run_config(json!({
        "channels": {
            "connections": [
                {"id": "release", "channel": "telegram", "transport": "long_connection"}
            ]
        }
    }))
    .expect_err("invalid transport");
    assert!(
        invalid_transport
            .to_string()
            .contains("is not supported for telegram"),
        "{invalid_transport:#}"
    );
}

#[test]
pub(crate) fn parse_channels_config_rejects_old_platform_field() {
    let err = parse_run_config(json!({
        "channels": {
            "connections": [
                {"id": "release", "platform": "telegram"}
            ]
        }
    }))
    .expect_err("old platform field");
    assert!(
        err.to_string()
            .contains("use channels.connections[0].channel"),
        "{err:#}"
    );
}

#[test]
pub(crate) fn upsert_channel_connection_updates_wechat_credentials_and_preserves_runtime_fields() {
    let temp = tempdir().expect("temp");
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    write_config(
        config_dir.join("config.toml"),
        r#"[[channels.connections]]
id = "wechat"
channel = "wechat"
label = "Personal WeChat"
transport = "polling"
enabled = true
workdir = "/tmp/project"
model = "provider/model"
credential_env = "WECHAT_BOT_TOKEN"
account_env = "WECHAT_ACCOUNT_ID"
base_url_env = "WECHAT_ILINK_BASE_URL"
allow_users = ["existing-user"]
"#,
    )
    .expect("config");
    fs::write(
        config_dir.join(".env"),
        "WECHAT_BOT_TOKEN=old-token\nWECHAT_ACCOUNT_ID=old-account\nWECHAT_ILINK_BASE_URL=http://old.example\n",
    )
    .expect("env");

    let duplicate = crate::setup_channel_connection(crate::ChannelSetupInput {
        config_dir: config_dir.clone(),
        id: "wechat".to_string(),
        channel: "wechat".to_string(),
        label: None,
        credential_env: Some("WECHAT_BOT_TOKEN".to_string()),
        credential: Some("new-token".to_string()),
        account_env: Some("WECHAT_ACCOUNT_ID".to_string()),
        account_id: Some("new-account".to_string()),
        base_url_env: Some("WECHAT_ILINK_BASE_URL".to_string()),
        base_url: Some("http://new.example".to_string()),
        allow_users: Vec::new(),
        allow_groups: Vec::new(),
    })
    .expect_err("duplicate setup still fails");
    assert!(duplicate.to_string().contains("already exists"));

    crate::upsert_channel_connection(crate::ChannelSetupInput {
        config_dir: config_dir.clone(),
        id: "wechat".to_string(),
        channel: "wechat".to_string(),
        label: None,
        credential_env: Some("WECHAT_BOT_TOKEN".to_string()),
        credential: Some("new-token".to_string()),
        account_env: Some("WECHAT_ACCOUNT_ID".to_string()),
        account_id: Some("new-account".to_string()),
        base_url_env: Some("WECHAT_ILINK_BASE_URL".to_string()),
        base_url: Some("http://new.example".to_string()),
        allow_users: Vec::new(),
        allow_groups: Vec::new(),
    })
    .expect("upsert");

    let config = fs::read_to_string(config_dir.join("config.toml")).expect("config");
    assert!(config.contains("label = \"Personal WeChat\""));
    assert!(config.contains("workdir = \"/tmp/project\""));
    assert!(config.contains("model = \"provider/model\""));
    assert!(config.contains("allow_users = [\"existing-user\"]"));
    let env = fs::read_to_string(config_dir.join(".env")).expect("env");
    assert!(env.contains("WECHAT_BOT_TOKEN=new-token"));
    assert!(env.contains("WECHAT_ACCOUNT_ID=new-account"));
    assert!(env.contains("WECHAT_ILINK_BASE_URL=http://new.example"));
    assert!(!env.contains("old-token"));
}
