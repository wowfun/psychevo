use super::paths::wechat_context_store_path;
use super::*;

pub(super) async fn build_channel_gateway(
    state: &WebState,
    connection: &ChannelRuntimeConnection,
) -> psychevo_runtime::Result<ChannelGateway> {
    let adapter: Arc<dyn crate::im::ImAdapter> = match connection.channel.as_str() {
        "wechat" => Arc::new(WechatIlinkAdapter::new(WechatIlinkConfig {
            connection_id: Some(connection.id.clone()),
            token: connection.credential.clone().unwrap_or_default(),
            account_id: connection.account_id.clone().unwrap_or_default(),
            base_url: connection
                .base_url
                .clone()
                .unwrap_or_else(|| WECHAT_ILINK_BASE_URL.to_string()),
            timeout_secs: 35,
            context_store_path: Some(wechat_context_store_path(&state.inner.home, &connection.id)),
        })?),
        "telegram" => Arc::new(TelegramPollingAdapter::new(TelegramPollingConfig {
            connection_id: Some(connection.id.clone()),
            token: connection.credential.clone().unwrap_or_default(),
            api_base: connection
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.telegram.org".to_string()),
            timeout_secs: 25,
        })?),
        "feishu" | "lark" => {
            let domain = FeishuLarkDomain::parse(connection.channel.as_str()).ok_or_else(|| {
                Error::Message(format!("unsupported channel `{}`", connection.channel))
            })?;
            Arc::new(
                FeishuLarkLongConnectionAdapter::connect(FeishuLarkLongConnectionConfig {
                    connection_id: Some(connection.id.clone()),
                    app_id: connection.app_id.clone().unwrap_or_default(),
                    app_secret: connection
                        .app_secret
                        .clone()
                        .or_else(|| connection.credential.clone())
                        .unwrap_or_default(),
                    domain,
                    base_url: connection.base_url.clone(),
                })
                .await?,
            )
        }
        other => {
            return Err(Error::Message(format!(
                "unsupported channel adapter `{other}`"
            )));
        }
    };
    Ok(ChannelGateway::new(vec![ChannelAdapterBinding::new(
        connection.id.clone(),
        adapter,
        ChannelAllowlist::new(
            connection.allow_users.clone(),
            connection.allow_groups.clone(),
        ),
    )]))
}
