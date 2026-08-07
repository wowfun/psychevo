use std::sync::Arc;

use anyhow::{Result, anyhow};
use futures::future::BoxFuture;
use psychevo_channel_adapters::feishu_lark::{
    FeishuLarkDomain, FeishuLarkLongConnectionAdapter, FeishuLarkLongConnectionConfig,
};
use psychevo_channel_adapters::im::ImAdapter;
use psychevo_channel_sidecar::{ChannelAdapterFactory, run};
use psychevo_extension_protocol::ChannelDescriptor;
use serde_json::Value;

struct FeishuLarkFactory;

impl ChannelAdapterFactory for FeishuLarkFactory {
    fn descriptors(&self) -> Vec<ChannelDescriptor> {
        ["feishu", "lark"]
            .into_iter()
            .map(|channel| ChannelDescriptor {
                channel: channel.to_string(),
                domains: vec![channel.to_string()],
                delivery_capabilities: vec!["push".to_string(), "text".to_string()],
            })
            .collect()
    }

    fn build(
        &self,
        connection_id: String,
        channel: String,
        configuration: Value,
    ) -> BoxFuture<'static, Result<Arc<dyn ImAdapter>>> {
        Box::pin(async move {
            let domain = FeishuLarkDomain::parse(&channel)
                .ok_or_else(|| anyhow!("unsupported Feishu/Lark Channel alias `{channel}`"))?;
            let app_secret = optional_string(&configuration, "appSecret")
                .or_else(|| optional_string(&configuration, "credential"))
                .ok_or_else(|| anyhow!("Feishu/Lark configuration requires `appSecret`"))?;
            let adapter =
                FeishuLarkLongConnectionAdapter::connect(FeishuLarkLongConnectionConfig {
                    connection_id: Some(connection_id),
                    app_id: string(&configuration, "appId")?,
                    app_secret,
                    domain,
                    base_url: optional_string(&configuration, "baseUrl"),
                })
                .await?;
            Ok(Arc::new(adapter) as Arc<dyn ImAdapter>)
        })
    }
}

fn string(value: &Value, key: &str) -> Result<String> {
    optional_string(value, key).ok_or_else(|| anyhow!("Feishu/Lark configuration requires `{key}`"))
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
    run("psychevo.channel.feishu-lark", Arc::new(FeishuLarkFactory)).await
}
