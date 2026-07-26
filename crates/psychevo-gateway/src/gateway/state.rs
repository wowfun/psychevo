#[derive(Clone, Debug)]
struct PendingGatewayLiveSnapshot {
    snapshot_key: String,
    activity_id: Option<String>,
    owner_id: Option<String>,
    thread_id: Option<String>,
    turn_id: Option<String>,
    event_kind: String,
    event: Value,
    last_flush_ms: i64,
    dirty: bool,
}

#[derive(Clone)]
pub struct Gateway {
    state: StateRuntime,
    agent_sessions: AgentSessionHost,
    event_ingress: GatewayEventIngress,
    supervisor: GatewaySupervisor,
    active: Arc<Mutex<HashMap<String, ActiveThreadState>>>,
    active_aliases: Arc<Mutex<HashMap<String, String>>>,
    process_bindings: Arc<Mutex<HashMap<String, String>>>,
    source_generations: Arc<Mutex<HashMap<String, u64>>>,
    source_mutations: Arc<Mutex<HashMap<String, Arc<AsyncMutex<()>>>>>,
    live_snapshots: Arc<Mutex<HashMap<String, PendingGatewayLiveSnapshot>>>,
    pending_permissions: PendingPermissionMap,
    owner_id: Arc<String>,
}

impl fmt::Debug for Gateway {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Gateway")
            .field("state", &self.state)
            .field("agent_sessions", &self.agent_sessions)
            .field("event_ingress", &self.event_ingress)
            .finish_non_exhaustive()
    }
}
