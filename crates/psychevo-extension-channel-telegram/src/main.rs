use std::sync::Arc;

use anyhow::{Result, anyhow};
use futures::future::BoxFuture;
use psychevo_channel_adapters::im::ImAdapter;
use psychevo_channel_adapters::telegram::{TelegramPollingAdapter, TelegramPollingConfig};
use psychevo_channel_sidecar::{ChannelAdapterFactory, run};
use psychevo_extension_protocol::ChannelDescriptor;
use serde_json::Value;

struct TelegramFactory;

impl ChannelAdapterFactory for TelegramFactory {
    fn descriptors(&self) -> Vec<ChannelDescriptor> {
        vec![ChannelDescriptor {
            channel: "telegram".to_string(),
            domains: Vec::new(),
            delivery_capabilities: vec!["poll".to_string(), "text".to_string()],
        }]
    }

    fn build(
        &self,
        connection_id: String,
        channel: String,
        configuration: Value,
    ) -> BoxFuture<'static, Result<Arc<dyn ImAdapter>>> {
        Box::pin(async move {
            if channel != "telegram" {
                return Err(anyhow!("unsupported Telegram Channel alias `{channel}`"));
            }
            let token = string(&configuration, "credential")?;
            let api_base = optional_string(&configuration, "baseUrl")
                .unwrap_or_else(|| "https://api.telegram.org".to_string());
            Ok(Arc::new(TelegramPollingAdapter::new(TelegramPollingConfig {
                connection_id: Some(connection_id),
                token,
                api_base,
                timeout_secs: 25,
            })?) as Arc<dyn ImAdapter>)
        })
    }
}

fn string(value: &Value, key: &str) -> Result<String> {
    optional_string(value, key).ok_or_else(|| anyhow!("Telegram configuration requires `{key}`"))
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
    run("psychevo.channel.telegram", Arc::new(TelegramFactory)).await
}
