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
pub struct ThreadApplication {
    state: StateRuntime,
    agent_sessions: AgentSessionHost,
    active: Arc<Mutex<HashMap<String, ActiveThreadState>>>,
    active_aliases: Arc<Mutex<HashMap<String, String>>>,
    process_bindings: Arc<Mutex<HashMap<String, String>>>,
    source_generations: Arc<Mutex<HashMap<String, u64>>>,
    source_mutations: Arc<Mutex<HashMap<String, Arc<AsyncMutex<()>>>>>,
    live_snapshots: Arc<Mutex<HashMap<String, PendingGatewayLiveSnapshot>>>,
    pending_permissions: PendingPermissionMap,
    owner_id: Arc<String>,
}

/// Compatibility-free internal application kernel. The public `Gateway` name is
/// an API-facing alias; caller adapters do not own Agent execution policy.
pub type Gateway = ThreadApplication;

impl fmt::Debug for ThreadApplication {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ThreadApplication")
            .field("state", &self.state)
            .field("agent_sessions", &self.agent_sessions)
            .finish_non_exhaustive()
    }
}
