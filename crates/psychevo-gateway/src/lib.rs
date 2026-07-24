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
use psychevo_ai::{AbortSignal, Outcome};
use psychevo_runtime::state::{
    GatewayActivityClaimInput, GatewayActivityRecord, GatewayControlCommandInput,
    GatewayLiveSnapshotInput, GatewayRuntimeBindingRecord, GatewayRuntimeBindingStatus,
    GatewayRuntimeControlStatePatch, GatewaySourceLaneInput, GatewayTurnDeliveryInput,
    GatewayTurnTerminalInput, StateRuntime,
};
#[cfg(test)]
use psychevo_runtime::state::{GatewayRuntimeBindingInput, GatewayRuntimeBindingOwnership};
use psychevo_runtime::{
    Error, agents::AgentDiscoveryOptions, agents::AgentEntrypoint, agents::discover_agents,
    agents::resolve_agent_definition, config::RuntimeProfileConfig, config::RuntimeProfileKind,
    config::load_agent_backend_configs, run::run_live, run::run_live_streaming,
    run::run_live_streaming_controlled, skills::resolve_skills_home, types::ApprovalHandler,
    types::ClarifyAnswer, types::ClarifyResponse, types::ClarifyResult,
    types::ExternalAgentDelegate, types::ExternalAgentDelegateRequest,
    types::ExternalAgentDelegateResult, types::ImageInput, types::PermissionApprovalDecision,
    types::PermissionApprovalOutcome, types::PermissionApprovalRequest, types::PermissionMode,
    types::PromptDisplayMetadata, types::RunControl, types::RunControlHandle, types::RunMode,
    types::RunOptions, types::RunResult, types::RunStreamEvent, types::RunStreamSink,
    types::StoredEditableInputEnvelope, types::StoredEditableInputPart,
    types::UserShellContextOptions, types::UserShellOptions, types::UserShellResult,
    types::WorkspaceMutationSink, types::run_control,
    user_shell::run_user_shell_command_streaming_controlled,
};
use serde_json::{Value, json};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard, oneshot};
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

pub type GatewayEventSink = Arc<dyn Fn(GatewayEvent) + Send + Sync>;

pub(crate) const ACP_PEER_METADATA_KEY: &str = "peer_agent";

fn gateway_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

#[path = "gateway/agent_session_binding.rs"]
mod agent_session_binding;
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

include!("gateway/state.rs");

include!("gateway/agent_session.rs");
include!("gateway/public_api.rs");
include!("gateway/source_bindings.rs");
include!("gateway/turn_lifecycle.rs");
include!("gateway/turn_shell.rs");
include!("gateway/active_queue.rs");
include!("gateway/durable_activity.rs");

include!("gateway/peer_runtime.rs");
include!("gateway/activity_permission.rs");
include!("gateway/backend_delegate.rs");
include!("gateway/stream_input.rs");

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
