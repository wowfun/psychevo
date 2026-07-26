pub mod app_server;
pub mod history_editing;
pub mod im;
pub mod protocol;
pub mod server;

mod acp_peer;
mod journey_profile;
mod managed_acp;
mod projection;
mod transcript;

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::future::BoxFuture;
use psychevo::__ai::{AbortSignal, Outcome};
use psychevo::state::{
    GatewayActivityClaimInput, GatewayActivityRecord, GatewayControlCommandInput,
    GatewayLiveSnapshotInput, GatewayRuntimeBindingRecord, GatewayRuntimeBindingStatus,
    GatewayRuntimeControlStatePatch, GatewaySourceLaneInput, GatewayTurnTerminalInput,
    StateRuntime,
};
#[cfg(test)]
use psychevo::state::{
    GatewayRuntimeBindingInput, GatewayRuntimeBindingOwnership, GatewayTurnDeliveryInput,
};
#[cfg(test)]
use psychevo::types::PermissionApprovalRequest;
use psychevo::{
    Error, agents::AgentDiscoveryOptions, agents::AgentEntrypoint, agents::discover_agents,
    agents::resolve_agent_definition, config::RuntimeProfileConfig, config::RuntimeProfileKind,
    config::load_agent_backend_configs, run::run_live, run::run_live_streaming,
    run::run_live_streaming_controlled, skills::resolve_skills_home, types::ApprovalHandler,
    types::ClarifyAnswer, types::ClarifyResponse, types::ClarifyResult,
    types::ExternalAgentDelegate, types::ExternalAgentDelegateRequest,
    types::ExternalAgentDelegateResult, types::ImageInput, types::PermissionApprovalDecision,
    types::PermissionApprovalOutcome, types::PermissionMode, types::PromptDisplayMetadata,
    types::RunControl, types::RunControlHandle, types::RunMode, types::RunOptions,
    types::RunResult, types::RunStreamEvent, types::RunStreamSink,
    types::StoredEditableInputEnvelope, types::StoredEditableInputPart,
    types::UserShellContextOptions, types::UserShellOptions, types::UserShellResult,
    types::WorkspaceMutationSink, types::run_control,
    user_shell::run_user_shell_command_streaming_controlled,
};
use serde_json::{Value, json};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard, oneshot};
#[cfg(test)]
use tokio::time::timeout;
use uuid::Uuid;

use journey_profile::{GatewayProfileFields, gateway_profile_mark};
use projection::GatewayLiveProjector;
pub use projection::gateway_event_from_run_stream;
pub use protocol::{
    AgentDeliveryStatusView, AgentErrorView, BackendKind, GatewayActionKind, GatewayActionOutcome,
    GatewayActivityView, GatewayBackendInfo, GatewayEvent, GatewayImageInput, GatewayInputPart,
    GatewaySelectedSkill, GatewaySource, GatewaySourceLifetime, GatewayThread,
    GatewayThreadSelector, GatewayTurn, GatewayTurnError, GatewayTurnStatus, PendingActionView,
    PermissionDecision, SourceKey, ThreadEditableDraft, ThreadEditableDraftFidelity,
    ThreadEditableInputPart, ThreadHistoryDraftReadResult, TranscriptBlock, TranscriptBlockKind,
    TranscriptBlockStatus, TranscriptEntry, TranscriptEntryRole, TranscriptToolResult,
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
    ensure_gateway_runtime_binding, resolve_bound_gateway_runtime_profile,
    resolve_gateway_agent_binding_snapshot, resolve_gateway_runtime_profile,
};
use event_ingress::{GatewayEventEnvelope, GatewayEventIngress};

include!("gateway/state.rs");

include!("gateway/agent_session.rs");
include!("gateway/public_api.rs");
include!("gateway/source_bindings.rs");
#[cfg(test)]
include!("gateway/turn_lifecycle.rs");
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
