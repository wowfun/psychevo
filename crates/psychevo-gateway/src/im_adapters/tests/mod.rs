use std::sync::{Arc, Mutex as StdMutex};

use axum::extract::State;
use axum::http::HeaderMap as AxumHeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use tokio::net::TcpListener;

use super::*;

#[derive(Clone, Default)]
struct HttpTestState {
    bodies: Arc<StdMutex<Vec<Value>>>,
    headers: Arc<StdMutex<Vec<BTreeMap<String, String>>>>,
}

#[test]
fn telegram_update_maps_text_message() {
    let update = json!({
        "update_id": 91,
        "message": {
            "message_id": 10,
            "message_thread_id": 5,
            "chat": { "id": -1001, "type": "supergroup" },
            "from": { "id": 42 },
            "text": "hello"
        }
    });

    let message = telegram_update_to_message(&update, Some("release")).expect("message");
    assert_eq!(message.identity.connection_id.as_deref(), Some("release"));
    assert_eq!(message.identity.platform, "telegram");
    assert_eq!(message.identity.chat_id, "-1001");
    assert_eq!(message.identity.user_id.as_deref(), Some("42"));
    assert_eq!(message.identity.thread_id.as_deref(), Some("5"));
    assert_eq!(message.message_id, "10");
    assert_eq!(message.text, "hello");
}

#[tokio::test]
async fn telegram_polling_adapter_calls_bot_api_and_advances_offset() {
    async fn get_updates(
        State(state): State<HttpTestState>,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        let mut bodies = state.bodies.lock().expect("bodies");
        bodies.push(body);
        let result = if bodies.len() == 1 {
            vec![json!({
                "update_id": 91,
                "message": {
                    "message_id": 10,
                    "chat": { "id": 123, "type": "private" },
                    "from": { "id": 42 },
                    "text": "hello"
                }
            })]
        } else {
            Vec::new()
        };
        Json(json!({ "ok": true, "result": result }))
    }

    let state = HttpTestState::default();
    let base_url = spawn_router(
        Router::new()
            .route("/{bot}/getUpdates", post(get_updates))
            .with_state(state.clone()),
    )
    .await;
    let adapter = TelegramPollingAdapter::new(TelegramPollingConfig {
        connection_id: Some("telegram".to_string()),
        token: "test-token".to_string(),
        api_base: base_url,
        timeout_secs: 1,
    })
    .expect("adapter");

    let messages = adapter.poll().await.expect("first poll");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].identity.chat_id, "123");
    assert_eq!(messages[0].text, "hello");
    let _ = adapter.poll().await.expect("second poll");

    let bodies = state.bodies.lock().expect("bodies");
    assert_eq!(bodies[0]["timeout"], 1);
    assert_eq!(bodies[0]["allowed_updates"], json!(["message"]));
    assert!(bodies[0].get("offset").is_none_or(Value::is_null));
    assert_eq!(bodies[1]["offset"], 92);
}

#[test]
fn wechat_message_maps_text_and_context_identity() {
    let raw = json!({
        "message_id": "wx_msg_1",
        "from_user_id": "wx_user",
        "to_user_id": "account",
        "item_list": [
            { "type": 1, "text_item": { "text": "ping" } }
        ]
    });

    let message = wechat_message_to_inbound(&raw, Some("wechat"), "account").expect("message");
    assert_eq!(message.identity.connection_id.as_deref(), Some("wechat"));
    assert_eq!(message.identity.platform, "wechat");
    assert_eq!(message.identity.chat_type.as_deref(), Some("dm"));
    assert_eq!(message.identity.chat_id, "wx_user");
    assert_eq!(message.text, "ping");
}

#[test]
fn wechat_message_preserves_media_metadata_when_download_is_not_available() {
    let raw = json!({
        "message_id": "wx_msg_media",
        "from_user_id": "wx_user",
        "to_user_id": "account",
        "item_list": [
            {
                "type": 2,
                "image_item": {
                    "media": {
                        "encrypt_query_param": "encrypted",
                        "aes_key": "key"
                    },
                    "hd_size": 4096
                }
            },
            {
                "type": 4,
                "file_item": {
                    "file_name": "notes.pdf",
                    "len": "12345",
                    "media": {
                        "encrypt_query_param": "encrypted-file",
                        "aes_key": "key"
                    }
                }
            }
        ]
    });

    let message = wechat_message_to_inbound(&raw, Some("wechat"), "account").expect("message");

    assert!(message.text.is_empty());
    assert_eq!(message.attachments.len(), 2);
    match &message.attachments[0] {
        ImAttachment::MediaMetadata {
            media_kind,
            mime_type,
            size_bytes,
            reason,
            ..
        } => {
            assert_eq!(media_kind, "image");
            assert_eq!(mime_type.as_deref(), Some("image/*"));
            assert_eq!(*size_bytes, Some(4096));
            assert!(reason.contains("download is not enabled"));
        }
        other => panic!("expected image metadata, got {other:?}"),
    }
    match &message.attachments[1] {
        ImAttachment::MediaMetadata {
            media_kind,
            filename,
            size_bytes,
            ..
        } => {
            assert_eq!(media_kind, "file");
            assert_eq!(filename.as_deref(), Some("notes.pdf"));
            assert_eq!(*size_bytes, Some(12345));
        }
        other => panic!("expected file metadata, got {other:?}"),
    }
}

#[tokio::test]
async fn wechat_ilink_adapter_posts_getupdates_and_persists_context_token() {
    async fn get_updates(
        State(state): State<HttpTestState>,
        headers: AxumHeaderMap,
        body: String,
    ) -> Json<Value> {
        let parsed: Value = serde_json::from_str(&body).expect("json body");
        state.bodies.lock().expect("bodies").push(parsed);
        state.headers.lock().expect("headers").push(
            headers
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .to_str()
                        .ok()
                        .map(|value| (name.as_str().to_string(), value.to_string()))
                })
                .collect(),
        );
        Json(json!({
            "ret": 0,
            "get_updates_buf": "next",
            "msgs": [{
                "message_id": "wx_msg_1",
                "from_user_id": "wx_user",
                "to_user_id": "account",
                "context_token": "ctx-token",
                "item_list": [
                    { "type": 1, "text_item": { "text": "ping" } }
                ]
            }]
        }))
    }

    let state = HttpTestState::default();
    let base_url = spawn_router(
        Router::new()
            .route("/ilink/bot/getupdates", post(get_updates))
            .with_state(state.clone()),
    )
    .await;
    let temp = tempfile::tempdir().expect("tempdir");
    let context_store_path = temp.path().join("wechat-context.json");
    let adapter = WechatIlinkAdapter::new(WechatIlinkConfig {
        connection_id: Some("wechat".to_string()),
        token: "token".to_string(),
        account_id: "account".to_string(),
        base_url,
        timeout_secs: 1,
        context_store_path: Some(context_store_path.clone()),
    })
    .expect("adapter");

    let messages = adapter.poll().await.expect("poll");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].identity.chat_id, "wx_user");
    assert_eq!(messages[0].text, "ping");

    let bodies = state.bodies.lock().expect("bodies");
    assert_eq!(bodies[0]["get_updates_buf"], "");
    assert_eq!(
        bodies[0]["base_info"]["channel_version"],
        WECHAT_CHANNEL_VERSION
    );
    let headers = state.headers.lock().expect("headers");
    assert_eq!(
        headers[0].get("authorizationtype").map(String::as_str),
        Some("ilink_bot_token")
    );
    assert_eq!(
        headers[0].get("ilink-app-id").map(String::as_str),
        Some(WECHAT_ILINK_APP_ID)
    );
    let persisted = fs::read_to_string(context_store_path).expect("context tokens");
    assert!(persisted.contains("ctx-token"));
}

#[tokio::test]
async fn wechat_ilink_adapter_classifies_session_timeout_as_qr_login_needed() {
    async fn get_updates() -> Json<Value> {
        Json(json!({
            "errcode": -14,
            "errmsg": "session timeout"
        }))
    }

    let base_url =
        spawn_router(Router::new().route("/ilink/bot/getupdates", post(get_updates))).await;
    let adapter = WechatIlinkAdapter::new(WechatIlinkConfig {
        connection_id: Some("wechat".to_string()),
        token: "token".to_string(),
        account_id: "account".to_string(),
        base_url,
        timeout_secs: 1,
        context_store_path: None,
    })
    .expect("adapter");

    let err = adapter.poll().await.expect_err("expired session");
    let message = err.to_string();
    assert!(is_wechat_ilink_session_expired_error(&message));
    assert_eq!(
        wechat_ilink_error_code_from_message(&message),
        Some(WECHAT_SESSION_EXPIRED_ERRCODE)
    );
}

#[tokio::test]
async fn wechat_ilink_health_treats_local_longpoll_timeout_as_empty_poll() {
    async fn get_updates() -> Json<Value> {
        tokio::time::sleep(Duration::from_secs(2)).await;
        Json(json!({ "ret": 0, "errcode": 0, "msgs": [] }))
    }

    let base_url =
        spawn_router(Router::new().route("/ilink/bot/getupdates", post(get_updates))).await;
    let health = check_wechat_ilink_health(&reqwest::Client::new(), &base_url, "token", 1)
        .await
        .expect("health");
    assert!(health.ok);
    assert_eq!(health.reason.as_deref(), Some("polling_empty"));
}

#[test]
fn feishu_event_maps_text_payload() {
    let event: feishu_sdk::event::Event = serde_json::from_value(json!({
        "schema": "2.0",
        "header": {
            "event_id": "evt_1",
            "event_type": "im.message.receive_v1",
            "tenant_key": "tenant"
        },
        "event": {
            "sender": {
                "sender_id": { "open_id": "ou_user" },
                "sender_type": "user",
                "tenant_key": "tenant"
            },
            "message": {
                "message_id": "om_msg",
                "chat_id": "oc_chat",
                "message_type": "text",
                "content": "{\"text\":\"hello from lark\"}"
            }
        }
    }))
    .expect("event");
    let config = FeishuLarkLongConnectionConfig {
        connection_id: Some("lark".to_string()),
        app_id: "cli_test".to_string(),
        app_secret: "secret".to_string(),
        domain: FeishuLarkDomain::Lark,
        base_url: None,
    };

    let message = feishu_event_to_inbound(&event, &config).expect("message");
    assert_eq!(message.identity.connection_id.as_deref(), Some("lark"));
    assert_eq!(message.identity.platform, "lark");
    assert_eq!(message.identity.workspace_id.as_deref(), Some("tenant"));
    assert_eq!(message.identity.chat_id, "oc_chat");
    assert_eq!(message.identity.user_id.as_deref(), Some("ou_user"));
    assert_eq!(message.text, "hello from lark");
}

async fn spawn_router(router: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve");
    });
    format!("http://{addr}")
}
