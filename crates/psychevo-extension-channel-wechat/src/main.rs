use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use futures::future::BoxFuture;
use psychevo_channel_adapters::im::ImAdapter;
use psychevo_channel_adapters::wechat::{
    WECHAT_ILINK_BASE_URL, WechatIlinkAdapter, WechatIlinkConfig, WechatQrPoll,
    check_wechat_ilink_health, fetch_wechat_qr_code, poll_wechat_qr_code,
    render_wechat_qr_terminal,
};
use psychevo_channel_sidecar::{ChannelAdapterFactory, run};
use psychevo_extension_protocol::{ChannelDescriptor, WechatQrPollResult};
use serde_json::{Value, json};

struct WechatFactory;

impl ChannelAdapterFactory for WechatFactory {
    fn descriptors(&self) -> Vec<ChannelDescriptor> {
        vec![ChannelDescriptor {
            channel: "wechat".to_string(),
            domains: Vec::new(),
            delivery_capabilities: vec![
                "poll".to_string(),
                "text".to_string(),
                "image".to_string(),
                "file".to_string(),
                "qr_setup".to_string(),
            ],
        }]
    }

    fn build(
        &self,
        connection_id: String,
        channel: String,
        configuration: Value,
    ) -> BoxFuture<'static, Result<Arc<dyn ImAdapter>>> {
        Box::pin(async move {
            if channel != "wechat" {
                return Err(anyhow!("unsupported WeChat Channel alias `{channel}`"));
            }
            let data_root = std::env::var_os("PSYCHEVO_EXTENSION_DATA")
                .map(PathBuf::from)
                .ok_or_else(|| anyhow!("PSYCHEVO_EXTENSION_DATA is unavailable"))?;
            let adapter = WechatIlinkAdapter::new(WechatIlinkConfig {
                connection_id: Some(connection_id.clone()),
                token: string(&configuration, "credential")?,
                account_id: string(&configuration, "accountId")?,
                base_url: optional_string(&configuration, "baseUrl")
                    .unwrap_or_else(|| WECHAT_ILINK_BASE_URL.to_string()),
                timeout_secs: 35,
                context_store_path: Some(data_root.join(format!("{connection_id}.context.json"))),
            })?;
            Ok(Arc::new(adapter) as Arc<dyn ImAdapter>)
        })
    }

    fn control(&self, method: String, params: Value) -> BoxFuture<'static, Result<Value>> {
        Box::pin(async move {
            let client = reqwest::Client::new();
            match method.as_str() {
                "channel/wechat/qr/start" => {
                    let base_url = optional_string(&params, "baseUrl")
                        .unwrap_or_else(|| WECHAT_ILINK_BASE_URL.to_string());
                    let qr = fetch_wechat_qr_code(&client, &base_url).await?;
                    let qr_terminal = qr
                        .qr_image
                        .is_none()
                        .then(|| render_wechat_qr_terminal(&qr.qr_url))
                        .transpose()?;
                    Ok(json!({
                        "qrcode": qr.qrcode,
                        "qrUrl": qr.qr_url,
                        "qrImage": qr.qr_image,
                        "qrSvg": qr.qr_svg,
                        "qrTerminal": qr_terminal,
                        "baseUrl": qr.base_url,
                    }))
                }
                "channel/wechat/qr/poll" => {
                    let base_url = optional_string(&params, "baseUrl")
                        .unwrap_or_else(|| WECHAT_ILINK_BASE_URL.to_string());
                    let qrcode = string(&params, "qrcode")?;
                    match poll_wechat_qr_code(&client, &base_url, &qrcode).await? {
                        WechatQrPoll::Waiting {
                            status,
                            message,
                            base_url,
                        } => Ok(serde_json::to_value(match status.as_str() {
                            "scaned" => WechatQrPollResult::Scanned { message, base_url },
                            "scaned_but_redirect" => {
                                WechatQrPollResult::ScannedRedirect { message, base_url }
                            }
                            _ => WechatQrPollResult::Waiting { message, base_url },
                        })?),
                        WechatQrPoll::Expired { message } => {
                            Ok(serde_json::to_value(WechatQrPollResult::Expired {
                                message,
                            })?)
                        }
                        WechatQrPoll::Confirmed {
                            account_id,
                            token,
                            base_url,
                            user_id,
                        } => Ok(serde_json::to_value(WechatQrPollResult::Confirmed {
                            account_id,
                            token,
                            base_url,
                            user_id,
                        })?),
                    }
                }
                "channel/wechat/health" => {
                    let base_url = optional_string(&params, "baseUrl")
                        .unwrap_or_else(|| WECHAT_ILINK_BASE_URL.to_string());
                    let token = string(&params, "credential")?;
                    let health = check_wechat_ilink_health(&client, &base_url, &token, 3).await?;
                    Ok(json!({
                        "ok": health.ok,
                        "reason": health.reason,
                        "errcode": health.errcode,
                        "message": health.message,
                        "msgCount": health.msg_count,
                    }))
                }
                _ => Err(anyhow!(
                    "unsupported WeChat Channel control method `{method}`"
                )),
            }
        })
    }
}

fn string(value: &Value, key: &str) -> Result<String> {
    optional_string(value, key).ok_or_else(|| anyhow!("WeChat configuration requires `{key}`"))
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[tokio::main]
async fn main() -> Result<()> {
    run("psychevo.channel.wechat", Arc::new(WechatFactory)).await
}
