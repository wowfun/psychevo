pub mod app_server;
pub mod composition;
pub mod history_editing;
pub mod im;
pub mod server;

mod acp_peer;
pub mod gateway;
mod journey_profile;
mod managed_acp;
mod projection;
#[cfg(test)]
mod test_support;
mod transcript;

use std::fmt;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
use futures::future::BoxFuture;
pub use projection::{gateway_event_from_run_stream, gateway_event_from_turn_event};
use psychevo_gateway_protocol::events_transcript::GatewayEvent;
pub use server::{BoundGatewayWebServer, GatewayWebServerConfig, bind_gateway_web_server};

#[derive(Clone)]
pub struct GatewayEventEmitter {
    emit: Arc<dyn Fn(GatewayEvent) -> Result<(), GatewayEventEmitError> + Send + Sync>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatewayEventEmitError {
    message: Arc<str>,
    overload: Option<GatewayEventIngressOverload>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatewayEventIngressOldest {
    pub age_ms: u64,
    pub activity_id: String,
    pub turn_id: Option<String>,
    pub event_kind: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatewayEventIngressOverload {
    pub occupancy: usize,
    pub limit: usize,
    pub retryable: bool,
    pub oldest: Option<GatewayEventIngressOldest>,
}

impl GatewayEventEmitError {
    fn new(message: impl Into<Arc<str>>) -> Self {
        Self {
            message: message.into(),
            overload: None,
        }
    }

    fn overloaded(overload: GatewayEventIngressOverload) -> Self {
        Self {
            message: Arc::from(format!(
                "Gateway event durability ingress is full ({}/{} outstanding); retry after capacity becomes available.",
                overload.occupancy, overload.limit
            )),
            overload: Some(overload),
        }
    }

    pub fn overload(&self) -> Option<&GatewayEventIngressOverload> {
        self.overload.as_ref()
    }
}

impl fmt::Display for GatewayEventEmitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for GatewayEventEmitError {}

impl fmt::Debug for GatewayEventEmitter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GatewayEventEmitter(..)")
    }
}

impl GatewayEventEmitter {
    pub fn new(emit: impl Fn(GatewayEvent) + Send + Sync + 'static) -> Self {
        Self::try_new(move |event| {
            emit(event);
            Ok(())
        })
    }

    fn try_new(
        emit: impl Fn(GatewayEvent) -> Result<(), GatewayEventEmitError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            emit: Arc::new(emit),
        }
    }

    pub fn emit(&self, event: GatewayEvent) -> Result<(), GatewayEventEmitError> {
        (self.emit)(event)
    }
}

pub(crate) const ACP_PEER_METADATA_KEY: &str = "peer_agent";

#[cfg(test)]
type FrameworkNativeTestExecutor = Arc<
    dyn Fn(
            psychevo::AgentTurnInvocation,
        ) -> BoxFuture<'static, psychevo::Result<psychevo::TurnResult>>
        + Send
        + Sync,
>;

fn gateway_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}
