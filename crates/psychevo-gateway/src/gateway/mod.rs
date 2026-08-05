use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use psychevo::application::GatewayDurability;
use tokio::sync::Mutex as AsyncMutex;

use self::activity::ActiveQueueState;
use self::agent_session::AgentSessionHost;
use self::durable_activity::ShellActivityRuntime;
use self::event_ingress::GatewayEventIngress;
pub use self::event_ingress::{GatewayEventCommitEvidence, GatewayEventIngressDiagnostics};
use self::supervisor::GatewaySupervisor;
use psychevo_gateway_protocol::events_transcript::GatewayEvent;

pub const DEFAULT_GATEWAY_EVENT_INGRESS_CAPACITY: usize = 512;
pub const DEFAULT_GATEWAY_SHELL_ACTIVITY_LIMIT: usize = 64;
pub const DEFAULT_GATEWAY_SHELL_QUEUE_LIMIT: usize = 32;
pub const DEFAULT_GATEWAY_DATABASE_CONNECTION_LIMIT: u32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatewayLimits {
    pub event_ingress_capacity: usize,
    pub shell_activity_limit: usize,
    pub shell_queue_limit: usize,
    pub application: psychevo::ApplicationLimits,
    pub database_connection_limit: u32,
}

impl Default for GatewayLimits {
    fn default() -> Self {
        Self {
            event_ingress_capacity: DEFAULT_GATEWAY_EVENT_INGRESS_CAPACITY,
            shell_activity_limit: DEFAULT_GATEWAY_SHELL_ACTIVITY_LIMIT,
            shell_queue_limit: DEFAULT_GATEWAY_SHELL_QUEUE_LIMIT,
            application: psychevo::ApplicationLimits::default(),
            database_connection_limit: DEFAULT_GATEWAY_DATABASE_CONNECTION_LIMIT,
        }
    }
}

impl GatewayLimits {
    pub(crate) fn validate(self) -> psychevo::Result<Self> {
        if self.event_ingress_capacity == 0 {
            return Err(psychevo::Error::Message(
                "Gateway event ingress capacity must be greater than zero".to_string(),
            ));
        }
        if self.shell_activity_limit == 0 {
            return Err(psychevo::Error::Message(
                "Gateway Shell activity limit must be greater than zero".to_string(),
            ));
        }
        if self.shell_queue_limit == 0 {
            return Err(psychevo::Error::Message(
                "Gateway per-lane Shell queue limit must be greater than zero".to_string(),
            ));
        }
        if self.database_connection_limit == 0 {
            return Err(psychevo::Error::Message(
                "Gateway database connection limit must be greater than zero".to_string(),
            ));
        }
        if self.application.max_operations == 0 || self.application.max_thread_operations == 0 {
            return Err(psychevo::Error::Message(
                "Gateway Application operation limits must be greater than zero".to_string(),
            ));
        }
        if self.application.max_thread_operations > self.application.max_operations {
            return Err(psychevo::Error::Message(
                "Gateway per-Thread operation limit cannot exceed the total limit".to_string(),
            ));
        }
        Ok(self)
    }
}

#[cfg(test)]
mod limit_tests {
    use super::GatewayLimits;

    #[test]
    fn every_composition_limit_is_validated_at_the_gateway_boundary() {
        let invalid = [
            GatewayLimits {
                event_ingress_capacity: 0,
                ..GatewayLimits::default()
            },
            GatewayLimits {
                shell_activity_limit: 0,
                ..GatewayLimits::default()
            },
            GatewayLimits {
                shell_queue_limit: 0,
                ..GatewayLimits::default()
            },
            GatewayLimits {
                database_connection_limit: 0,
                ..GatewayLimits::default()
            },
            GatewayLimits {
                application: psychevo::ApplicationLimits {
                    max_operations: 0,
                    max_thread_operations: 0,
                },
                ..GatewayLimits::default()
            },
            GatewayLimits {
                application: psychevo::ApplicationLimits {
                    max_operations: 1,
                    max_thread_operations: 2,
                },
                ..GatewayLimits::default()
            },
        ];
        for limits in invalid {
            assert!(limits.validate().is_err(), "accepted {limits:?}");
        }
    }
}

mod active_queue;
#[path = "activity_permission.rs"]
pub mod activity;
pub(crate) mod agent_session;
pub(crate) mod agent_session_binding;
pub(crate) mod durable_activity;
mod event_ingress;
#[path = "../framework_adapter.rs"]
pub mod framework_adapter;
pub mod live_projection;
pub(crate) mod peer_runtime;
pub(crate) mod public_api;
#[path = "turn_results.rs"]
pub mod results;
mod source_bindings;
mod stream_input;
mod supervisor;
pub(crate) mod turn_shell;

#[derive(Clone, Debug)]
struct PendingGatewayLiveSnapshot {
    snapshot_key: String,
    activity_id: Option<String>,
    owner_id: Option<String>,
    thread_id: Option<String>,
    turn_id: Option<String>,
    event_kind: String,
    event: GatewayEvent,
    dirty: bool,
}

#[derive(Clone)]
pub struct Gateway {
    durability: GatewayDurability,
    agent_sessions: AgentSessionHost,
    framework_client: psychevo::Client,
    event_ingress: GatewayEventIngress,
    supervisor: GatewaySupervisor,
    active_queue: Arc<Mutex<ActiveQueueState>>,
    process_bindings: Arc<Mutex<HashMap<String, String>>>,
    source_generations: Arc<Mutex<HashMap<String, u64>>>,
    source_mutations: Arc<Mutex<HashMap<String, Arc<AsyncMutex<()>>>>>,
    live_snapshots: Arc<Mutex<HashMap<String, PendingGatewayLiveSnapshot>>>,
    shell_activity_runtime: Arc<ShellActivityRuntime>,
    shell_queue_limit: usize,
    owner_id: Arc<String>,
}

impl fmt::Debug for Gateway {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Gateway")
            .field("durability", &self.durability)
            .field("agent_sessions", &self.agent_sessions)
            .field("event_ingress", &self.event_ingress)
            .field("shell_queue_limit", &self.shell_queue_limit)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
