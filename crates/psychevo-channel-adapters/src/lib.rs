pub mod im {
    use futures::future::BoxFuture;

    pub use psychevo_extension_protocol::{
        ChannelAttachment as ImAttachment, ChannelIdentity as ImIdentity,
        ChannelInboundMessage as ImInboundMessage, ChannelOutboundMessage as ImOutboundMessage,
    };

    pub trait ImAdapter: Send + Sync {
        fn platform(&self) -> &str;
        fn poll(&self) -> BoxFuture<'static, anyhow::Result<Vec<ImInboundMessage>>>;
        fn send(&self, message: ImOutboundMessage) -> BoxFuture<'static, anyhow::Result<()>>;
        fn shutdown(&self) -> BoxFuture<'static, anyhow::Result<()>> {
            Box::pin(async { Ok(()) })
        }
    }
}

mod util;

#[cfg(feature = "telegram")]
pub mod telegram;
#[cfg(feature = "telegram")]
pub use telegram::{TelegramPollingAdapter, TelegramPollingConfig};

#[cfg(feature = "wechat")]
pub mod wechat;
#[cfg(feature = "wechat")]
pub use wechat::{WechatIlinkAdapter, WechatIlinkConfig};

#[cfg(feature = "feishu-lark")]
pub mod feishu_lark;
#[cfg(feature = "feishu-lark")]
pub use feishu_lark::{
    FeishuLarkDomain, FeishuLarkLongConnectionAdapter, FeishuLarkLongConnectionConfig,
};

#[cfg(all(
    test,
    any(feature = "telegram", feature = "wechat", feature = "feishu-lark")
))]
mod tests;
