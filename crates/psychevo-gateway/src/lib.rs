pub mod im;
pub mod protocol;
pub mod server;

mod acp_peer;
mod projection;
mod transcript;

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::future::BoxFuture;
use psychevo_runtime::{
    AbortSignal, AgentDiscoveryOptions, AgentEntrypoint, ApprovalHandler, ClarifyAnswer,
    ClarifyResponse, ClarifyResult, Error, ExternalAgentDelegate, ExternalAgentDelegateRequest,
    ExternalAgentDelegateResult, GatewayActivityClaimInput, GatewayActivityRecord,
    GatewayControlCommandInput, GatewaySourceBindingInput, ImageInput, PermissionApprovalDecision,
    PermissionApprovalOutcome, PermissionApprovalRequest, RunControl, RunControlHandle, RunOptions,
    RunResult, RunStreamEvent, RunStreamSink, StateRuntime, UserShellContextOptions,
    UserShellOptions, UserShellResult, discover_agents, load_agent_backend_configs,
    resolve_agent_definition, resolve_skills_home, run_control, run_live, run_live_streaming,
    run_live_streaming_controlled, run_user_shell_command_streaming_controlled,
};
use serde_json::{Value, json};
use tokio::sync::oneshot;
use tokio::time::timeout;
use uuid::Uuid;

use projection::GatewayLiveProjector;
pub use projection::gateway_event_from_run_stream;
pub use protocol::{
    BackendKind, GatewayActivityView, GatewayBackendInfo, GatewayEvent, GatewayImageInput,
    GatewayInputPart, GatewaySelectedSkill, GatewaySource, GatewaySourceLifetime, GatewayThread,
    GatewayThreadSelector, GatewayTurn, GatewayTurnStatus, PermissionDecision, SourceKey,
    TranscriptBlock, TranscriptBlockKind, TranscriptBlockStatus, TranscriptEntry,
    TranscriptEntryRole, TranscriptToolResult,
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

include!("gateway/state.rs");

include!("gateway/public_api.rs");
include!("gateway/source_bindings.rs");
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
    include!("gateway/tests/source_lanes.rs");
    include!("gateway/tests/control_runtime.rs");
    include!("gateway/tests/acp_peer_sessions.rs");
    include!("gateway/tests/acp_peer_streams.rs");
}
