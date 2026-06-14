#[derive(Clone)]
pub struct Gateway {
    state: StateRuntime,
    backend: Arc<dyn GatewayBackend>,
    active: Arc<Mutex<HashMap<String, ActiveThreadState>>>,
    active_aliases: Arc<Mutex<HashMap<String, String>>>,
    process_bindings: Arc<Mutex<HashMap<String, String>>>,
    source_generations: Arc<Mutex<HashMap<String, u64>>>,
    pending_permissions: PendingPermissionMap,
    owner_id: Arc<String>,
}

impl fmt::Debug for Gateway {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Gateway")
            .field("state", &self.state)
            .field("backend", &self.backend)
            .finish_non_exhaustive()
    }
}
