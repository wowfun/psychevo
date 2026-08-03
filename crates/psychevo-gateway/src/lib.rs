pub mod app_server;
pub mod history_editing;
pub mod im;
pub mod protocol;
pub mod server;

mod acp_peer;
mod journey_profile;
mod managed_acp;
mod projection;
#[cfg(test)]
mod test_support;
mod transcript;

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::future::BoxFuture;
#[cfg(test)]
use psychevo::__ai::AbortSignal;
use psychevo::__ai::Outcome;
use psychevo::__product::persistence::{
    GatewayActivityClaimInput, GatewayActivityRecord, GatewayControlCommandInput,
    GatewayLiveSnapshotInput, GatewayRuntimeBindingRecord, GatewayRuntimeBindingStatus,
    GatewayRuntimeControlStatePatch, GatewaySourceLaneInput, StateRuntime,
};
#[cfg(test)]
use psychevo::__product::runtime::PermissionApprovalRequest;
use psychevo::{
    __product::capabilities::AgentDiscoveryOptions, __product::capabilities::AgentEntrypoint,
    __product::capabilities::discover_agents, __product::capabilities::resolve_agent_definition,
    __product::capabilities::resolve_skills_home, __product::configuration::RuntimeProfileConfig,
    __product::configuration::RuntimeProfileKind,
    __product::configuration::load_agent_backend_configs,
    __product::presentation::run_user_shell_command_streaming_controlled,
    __product::runtime::ApprovalHandler, __product::runtime::ClarifyAnswer,
    __product::runtime::ClarifyResponse, __product::runtime::ClarifyResult,
    __product::runtime::ExternalAgentDelegate, __product::runtime::ExternalAgentDelegateRequest,
    __product::runtime::ExternalAgentDelegateResult, __product::runtime::ImageInput,
    __product::runtime::PermissionApprovalDecision, __product::runtime::PermissionApprovalOutcome,
    __product::runtime::PermissionMode, __product::runtime::PromptDisplayMetadata,
    __product::runtime::RunControl, __product::runtime::RunControlHandle,
    __product::runtime::RunMode, __product::runtime::RunOptions, __product::runtime::RunResult,
    __product::runtime::RunStreamEvent, __product::runtime::RunStreamSink,
    __product::runtime::StoredEditableInputEnvelope, __product::runtime::StoredEditableInputPart,
    __product::runtime::UserShellContextOptions, __product::runtime::UserShellOptions,
    __product::runtime::UserShellResult, __product::runtime::WorkspaceMutationSink,
    __product::runtime::run_control, __product::runtime::run_live,
    __product::runtime::run_live_streaming, __product::runtime::run_live_streaming_controlled,
    Application, Error,
};
use serde_json::{Value, json};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard, oneshot};
use uuid::Uuid;

use journey_profile::{GatewayProfileFields, gateway_profile_mark};
use projection::GatewayLiveProjector;
pub use projection::gateway_event_from_run_stream;
pub use protocol::{
    AgentDeliveryStatusView, AgentErrorView, BackendKind, FrameworkTurnKind, GatewayActionKind,
    GatewayActionOutcome, GatewayActivityView, GatewayBackendInfo, GatewayEvent, GatewayImageInput,
    GatewayInputPart, GatewayLocalOperationView, GatewaySelectedSkill, GatewaySource,
    GatewaySourceLifetime, GatewayThread, GatewayThreadSelector, GatewayTurn, GatewayTurnError,
    GatewayTurnStatus, PendingActionView, PermissionDecision, SourceKey, ThreadActivityView,
    ThreadEditableDraft, ThreadEditableDraftFidelity, ThreadEditableInputPart,
    ThreadHistoryDraftReadResult, TranscriptBlock, TranscriptBlockKind, TranscriptBlockStatus,
    TranscriptEntry, TranscriptEntryRole, TranscriptToolResult,
};
pub use server::{BoundGatewayWebServer, GatewayWebServerConfig, bind_gateway_web_server};

type GatewayEventWaitEmitter =
    dyn Fn(GatewayEvent) -> BoxFuture<'static, Result<(), GatewayEventEmitError>> + Send + Sync;

#[derive(Clone)]
pub struct GatewayEventEmitter {
    emit: Arc<dyn Fn(GatewayEvent) -> Result<(), GatewayEventEmitError> + Send + Sync>,
    emit_wait: Option<Arc<GatewayEventWaitEmitter>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatewayEventEmitError {
    message: Arc<str>,
}

impl GatewayEventEmitError {
    fn new(message: impl Into<Arc<str>>) -> Self {
        Self {
            message: message.into(),
        }
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
            emit_wait: None,
        }
    }

    fn try_new_with_wait(
        emit: impl Fn(GatewayEvent) -> Result<(), GatewayEventEmitError> + Send + Sync + 'static,
        emit_wait: impl Fn(GatewayEvent) -> BoxFuture<'static, Result<(), GatewayEventEmitError>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            emit: Arc::new(emit),
            emit_wait: Some(Arc::new(emit_wait)),
        }
    }

    pub fn emit(&self, event: GatewayEvent) -> Result<(), GatewayEventEmitError> {
        (self.emit)(event)
    }

    pub async fn emit_wait(&self, event: GatewayEvent) -> Result<(), GatewayEventEmitError> {
        match self.emit_wait.as_ref() {
            Some(emit_wait) => emit_wait(event).await,
            None => self.emit(event),
        }
    }
}

pub(crate) const ACP_PEER_METADATA_KEY: &str = "peer_agent";

fn gateway_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

#[path = "gateway/agent_session_binding.rs"]
mod agent_session_binding;
#[path = "gateway/supervisor.rs"]
mod supervisor;
use supervisor::GatewaySupervisor;
#[path = "gateway/event_ingress.rs"]
mod event_ingress;
pub(crate) use agent_session_binding::{
    BoundGatewayAgentTarget, agent_definition_matches_runtime_profile,
    gateway_agent_definition_fingerprint, generated_gateway_runtime_profiles,
    resolve_bound_gateway_agent_target, runtime_profile_config_fingerprint,
    runtime_profile_config_revision, runtime_session_handle,
};
use agent_session_binding::{
    ensure_gateway_runtime_binding, resolve_captured_bound_peer,
    resolve_gateway_agent_binding_snapshot, resolve_gateway_runtime_profile,
};
use event_ingress::{GatewayEventEnvelope, GatewayEventIngress};

include!("gateway/state.rs");

include!("gateway/agent_session.rs");
include!("gateway/public_api.rs");
include!("gateway/source_bindings.rs");
include!("gateway/turn_shell.rs");
include!("gateway/active_queue.rs");
include!("gateway/durable_activity.rs");

include!("gateway/peer_runtime.rs");
include!("gateway/activity_permission.rs");
include!("gateway/backend_delegate.rs");
include!("gateway/stream_input.rs");
include!("framework_adapter.rs");

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    include!("gateway/tests/support_peer.rs");
    include!("gateway/tests/agent_conformance.rs");
    include!("gateway/tests/source_lanes.rs");
    include!("gateway/tests/control_runtime.rs");
    include!("gateway/tests/acp_peer_sessions.rs");
    include!("gateway/tests/acp_peer_streams.rs");
}
