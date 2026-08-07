use std::process::Stdio;

use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

struct Sidecar {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Sidecar {
    fn spawn(data_root: &TempDir) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_psychevo-channel-wechat"))
            .env("PSYCHEVO_EXTENSION_DATA", data_root.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn sidecar");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        }
    }

    async fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.stdin
            .write_all(
                format!(
                    "{}\n",
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "method": method,
                        "params": params,
                    })
                )
                .as_bytes(),
            )
            .await
            .expect("write request");
        self.stdin.flush().await.expect("flush request");
        let mut line = String::new();
        self.stdout
            .read_line(&mut line)
            .await
            .expect("read response");
        let response: Value = serde_json::from_str(&line).expect("response JSON");
        assert_eq!(response["id"], id);
        assert!(
            response.get("error").is_none() || response["error"].is_null(),
            "RPC failed: {response}"
        );
        response["result"].clone()
    }

    async fn shutdown(mut self) {
        self.call("shutdown", json!({})).await;
        assert!(self.child.wait().await.expect("wait sidecar").success());
    }
}

#[tokio::test]
async fn sidecar_negotiates_and_routes_wechat_controls_to_http_transport() {
    async fn qr_code() -> Json<Value> {
        Json(json!({
            "qrcode": "qr-token",
            "qrcode_img_content": "data:image/png;base64,wechat-qr-image"
        }))
    }

    async fn qr_status() -> Json<Value> {
        Json(json!({
            "status": "confirmed",
            "ilink_bot_id": "wx-account",
            "bot_token": "wechat-secret",
            "ilink_user_id": "wx-user"
        }))
    }

    async fn get_updates() -> Json<Value> {
        Json(json!({
            "ret": 0,
            "errcode": 0,
            "msgs": [],
            "get_updates_buf": "healthy"
        }))
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let base_url = format!("http://{}", listener.local_addr().expect("addr"));
    let router = Router::new()
        .route("/ilink/bot/get_bot_qrcode", get(qr_code))
        .route("/ilink/bot/get_qrcode_status", get(qr_status))
        .route("/ilink/bot/getupdates", post(get_updates));
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve");
    });

    let data_root = TempDir::new().expect("data root");
    let mut sidecar = Sidecar::spawn(&data_root);
    let initialized = sidecar
        .call(
            "initialize",
            json!({
                "protocol": "psychevo-extension/1",
                "extensionId": "psychevo.channel.wechat",
                "extensionVersion": "0.1.0",
                "scope": "profile",
                "packageRoot": data_root.path(),
                "dataRoot": data_root.path(),
                "capabilities": {
                    "structuredDisplays": false,
                    "mcpApps": false,
                    "channels": true
                }
            }),
        )
        .await;
    assert_eq!(initialized["protocol"], "psychevo-extension/1");
    assert_eq!(
        initialized["capabilities"]["channels"][0]["channel"],
        "wechat"
    );

    let started = sidecar
        .call("channel/wechat/qr/start", json!({ "baseUrl": base_url }))
        .await;
    assert_eq!(started["qrcode"], "qr-token");
    assert_eq!(started["qrImage"], "data:image/png;base64,wechat-qr-image");
    assert_eq!(started["qrTerminal"], Value::Null);

    let polled = sidecar
        .call(
            "channel/wechat/qr/poll",
            json!({ "baseUrl": base_url, "qrcode": "qr-token" }),
        )
        .await;
    assert_eq!(polled["status"], "confirmed");
    assert_eq!(polled["accountId"], "wx-account");
    assert_eq!(polled["token"], "wechat-secret");
    assert_eq!(polled["userId"], "wx-user");
    assert!(
        polled.get("message").is_none(),
        "confirmed uses the protocol variant without a message field"
    );

    let health = sidecar
        .call(
            "channel/wechat/health",
            json!({ "baseUrl": base_url, "credential": "wechat-secret" }),
        )
        .await;
    assert_eq!(health["ok"], true);
    assert_eq!(health["reason"], "polling_empty");
    assert_eq!(health["msgCount"], 0);

    sidecar.shutdown().await;
}
