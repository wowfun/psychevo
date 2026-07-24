const ACP_PROCESS_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const ACP_PROCESS_FORCE_SHUTDOWN_MESSAGE: &str = "ACP process forced shutdown";
const ACP_PROTOCOL_PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const ACP_AUTH_PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const ACP_AUTH_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const ACP_AUTH_STATUS_DETAIL_MAX_CHARS: usize = 1_024;

pub(crate) type AcpSessionReadyCallback =
    Arc<dyn Fn(&str) -> psychevo_runtime::Result<()> + Send + Sync>;

pub(crate) struct AcpSetControlInput {
    pub(crate) peer: ResolvedPeerTurn,
    pub(crate) cwd: PathBuf,
    pub(crate) local_session_id: String,
    pub(crate) native_session_id: String,
    pub(crate) mcp_servers: Vec<psychevo_runtime::types::ResolvedMcpServerInput>,
    pub(crate) control_id: String,
    pub(crate) value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AcpProcessKey {
    backend_fingerprint: String,
    canonical_cwd: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AcpAuthObservationKey {
    launch_fingerprint: String,
    canonical_cwd: PathBuf,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum AcpObservedAuthState {
    #[default]
    Unchecked,
    Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AcpAuthDoctorStatus {
    Authenticated(AcpAuthenticatedKind),
    Required,
    Unchecked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AcpProtocolDoctorStatus {
    Compatible {
        version: u16,
    },
    Incompatible {
        expected_version: u16,
        actual_version: u16,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AcpProcessStartupStatus {
    Starting,
    Protocol(AcpProtocolDoctorStatus),
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AcpAuthenticatedKind {
    ApiKey,
    ChatGpt,
    Gateway,
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, agent_client_protocol::JsonRpcRequest,
)]
#[request(
    method = "authentication/status",
    response = CodexAuthenticationStatusResponse,
    crate = agent_client_protocol
)]
struct CodexAuthenticationStatusRequest {}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, agent_client_protocol::JsonRpcResponse,
)]
#[serde(tag = "type", deny_unknown_fields)]
enum CodexAuthenticationStatusResponse {
    #[serde(rename = "unauthenticated")]
    Unauthenticated,
    #[serde(rename = "api-key")]
    ApiKey,
    #[serde(rename = "chat-gpt")]
    ChatGpt { email: String },
    #[serde(rename = "gateway")]
    Gateway { name: String },
}

#[derive(Clone)]
struct AcpProcessHandle {
    generation: u64,
    command_tx: tokio_mpsc::UnboundedSender<AcpProcessCommand>,
    force_tx: watch::Sender<bool>,
    done_rx: watch::Receiver<bool>,
    startup_rx: watch::Receiver<AcpProcessStartupStatus>,
    auth_observation: Arc<Mutex<AcpObservedAuthState>>,
}

struct AcpProcessPoolInner {
    actors: Mutex<HashMap<AcpProcessKey, AcpProcessHandle>>,
    resident_actors: Mutex<HashMap<String, AcpProcessHandle>>,
    auth_observations: Mutex<HashMap<AcpAuthObservationKey, Arc<Mutex<AcpObservedAuthState>>>>,
    next_generation: AtomicU64,
    idle_timeout: Duration,
}

impl Drop for AcpProcessPoolInner {
    fn drop(&mut self) {
        if let Ok(actors) = self.actors.lock() {
            for actor in actors.values() {
                let _ = actor.force_tx.send(true);
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct AcpProcessPool {
    inner: Arc<AcpProcessPoolInner>,
}

impl fmt::Debug for AcpProcessPool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let process_count = self
            .inner
            .actors
            .lock()
            .map(|actors| actors.len())
            .unwrap_or_default();
        formatter
            .debug_struct("AcpProcessPool")
            .field("process_count", &process_count)
            .field("idle_timeout", &self.inner.idle_timeout)
            .finish_non_exhaustive()
    }
}

impl Default for AcpProcessPool {
    fn default() -> Self {
        Self::new(ACP_PROCESS_IDLE_TIMEOUT)
    }
}

impl AcpProcessPool {
    fn new(idle_timeout: Duration) -> Self {
        Self {
            inner: Arc::new(AcpProcessPoolInner {
                actors: Mutex::new(HashMap::new()),
                resident_actors: Mutex::new(HashMap::new()),
                auth_observations: Mutex::new(HashMap::new()),
                next_generation: AtomicU64::new(1),
                idle_timeout,
            }),
        }
    }

    async fn run_turn(
        &self,
        peer: ResolvedPeerTurn,
        context: AcpPeerTurnContext,
        session_ready: AcpSessionReadyCallback,
    ) -> psychevo_runtime::Result<AcpTurnOutput> {
        let handle = self.actor(&peer, &context.cwd)?;
        self.inner
            .resident_actors
            .lock()
            .map_err(|_| Error::Message("ACP resident actor registry poisoned".to_string()))?
            .insert(context.local_session_id.clone(), handle.clone());
        let delivery = AcpDeliveryMarker::default();
        let (reply_tx, reply_rx) = tokio_oneshot::channel();
        handle
            .command_tx
            .send(AcpProcessCommand::Turn(Box::new(AcpTurnCommand {
                peer,
                context,
                session_ready,
                delivery: delivery.clone(),
                reply: reply_tx,
            })))
            .map_err(|_| acp_process_unavailable_error("ACP process mailbox closed"))?;
        let result = match reply_rx.await {
            Ok(result) => result,
            Err(_) if delivery.was_sent() => Err(acp_unknown_delivery_error(
                "ACP connection ended after the prompt request was dispatched; the turn was not retried",
            )),
            Err(_) => Err(acp_process_unavailable_error(
                "ACP process ended before the prompt request was dispatched",
            )),
        };
        observe_acp_auth_result(&handle.auth_observation, &result, true);
        result
    }

    pub(crate) async fn prepare_session(
        &self,
        peer: ResolvedPeerTurn,
        cwd: PathBuf,
        local_session_id: String,
        mcp_servers: Vec<psychevo_runtime::types::ResolvedMcpServerInput>,
    ) -> psychevo_runtime::Result<AcpSessionSnapshot> {
        let handle = self.actor(&peer, &cwd)?;
        self.inner
            .resident_actors
            .lock()
            .map_err(|_| Error::Message("ACP resident actor registry poisoned".to_string()))?
            .insert(local_session_id.clone(), handle.clone());
        let (reply_tx, reply_rx) = tokio_oneshot::channel();
        handle
            .command_tx
            .send(AcpProcessCommand::Prepare {
                local_session_id,
                cwd,
                mcp_servers,
                reply: reply_tx,
            })
            .map_err(|_| acp_process_unavailable_error("ACP process mailbox closed"))?;
        let result = reply_rx.await.map_err(|_| {
            acp_process_unavailable_error("ACP process ended while preparing a draft session")
        })?;
        observe_acp_auth_result(&handle.auth_observation, &result, false);
        result
    }

    pub(crate) async fn promote_session(
        &self,
        old_local_session_id: String,
        new_local_session_id: String,
        native_session_id: String,
    ) -> psychevo_runtime::Result<()> {
        let handle = self
            .inner
            .resident_actors
            .lock()
            .map_err(|_| Error::Message("ACP resident actor registry poisoned".to_string()))?
            .get(&old_local_session_id)
            .filter(|handle| !*handle.done_rx.borrow())
            .cloned()
            .ok_or_else(|| acp_process_unavailable_error("prepared ACP session is no longer resident"))?;
        let (reply_tx, reply_rx) = tokio_oneshot::channel();
        handle
            .command_tx
            .send(AcpProcessCommand::Promote {
                old_local_session_id: old_local_session_id.clone(),
                new_local_session_id: new_local_session_id.clone(),
                native_session_id,
                reply: reply_tx,
            })
            .map_err(|_| acp_process_unavailable_error("ACP process mailbox closed"))?;
        reply_rx.await.map_err(|_| {
            acp_process_unavailable_error("ACP process ended while promoting a draft session")
        })??;
        let mut residents = self
            .inner
            .resident_actors
            .lock()
            .map_err(|_| Error::Message("ACP resident actor registry poisoned".to_string()))?;
        residents.remove(&old_local_session_id);
        residents.insert(new_local_session_id, handle);
        Ok(())
    }

    pub(crate) async fn inspect(
        &self,
        peer: ResolvedPeerTurn,
        cwd: PathBuf,
        local_session_id: String,
        native_session_id: String,
        mcp_servers: Vec<psychevo_runtime::types::ResolvedMcpServerInput>,
    ) -> psychevo_runtime::Result<AcpSessionSnapshot> {
        let handle = self.actor(&peer, &cwd)?;
        let (reply_tx, reply_rx) = tokio_oneshot::channel();
        handle
            .command_tx
            .send(AcpProcessCommand::Inspect {
                local_session_id,
                native_session_id,
                cwd,
                mcp_servers,
                reply: reply_tx,
            })
            .map_err(|_| acp_process_unavailable_error("ACP process mailbox closed"))?;
        let result = reply_rx.await.map_err(|_| {
            acp_process_unavailable_error("ACP process ended while reading session controls")
        })?;
        observe_acp_auth_result(&handle.auth_observation, &result, false);
        result
    }

    pub(crate) async fn load_session(
        &self,
        peer: ResolvedPeerTurn,
        cwd: PathBuf,
        local_session_id: String,
        native_session_id: String,
        mcp_servers: Vec<psychevo_runtime::types::ResolvedMcpServerInput>,
    ) -> psychevo_runtime::Result<AcpSessionLoadOutput> {
        let handle = self.actor(&peer, &cwd)?;
        let resident_local_session_id = local_session_id.clone();
        let (reply_tx, reply_rx) = tokio_oneshot::channel();
        handle
            .command_tx
            .send(AcpProcessCommand::LoadSession {
                local_session_id,
                native_session_id,
                cwd,
                mcp_servers,
                reply: reply_tx,
            })
            .map_err(|_| acp_process_unavailable_error("ACP process mailbox closed"))?;
        let result = reply_rx.await.map_err(|_| {
            acp_process_unavailable_error("ACP process ended while loading Agent history")
        })?;
        if result.is_ok() {
            self.inner
                .resident_actors
                .lock()
                .map_err(|_| Error::Message("ACP resident actor registry poisoned".to_string()))?
                .insert(resident_local_session_id, handle.clone());
        }
        observe_acp_auth_result(&handle.auth_observation, &result, false);
        result
    }

    pub(crate) async fn inspect_cached(
        &self,
        local_session_id: String,
        native_session_id: String,
    ) -> psychevo_runtime::Result<Option<AcpSessionSnapshot>> {
        let handle = self
            .inner
            .resident_actors
            .lock()
            .map_err(|_| Error::Message("ACP resident actor registry poisoned".to_string()))?
            .get(&local_session_id)
            .filter(|handle| !*handle.done_rx.borrow())
            .cloned();
        let Some(handle) = handle else {
            return Ok(None);
        };
        let (reply_tx, reply_rx) = tokio_oneshot::channel();
        handle
            .command_tx
            .send(AcpProcessCommand::InspectCached {
                local_session_id,
                native_session_id,
                reply: reply_tx,
            })
            .map_err(|_| acp_process_unavailable_error("ACP process mailbox closed"))?;
        reply_rx.await.map_err(|_| {
            acp_process_unavailable_error("ACP process ended while reading its resident session projection")
        })?
    }

    pub(crate) async fn set_control(
        &self,
        input: AcpSetControlInput,
    ) -> psychevo_runtime::Result<AcpSessionSnapshot> {
        let handle = self.actor(&input.peer, &input.cwd)?;
        let AcpSetControlInput {
            peer: _,
            cwd,
            local_session_id,
            native_session_id,
            mcp_servers,
            control_id,
            value,
        } = input;
        let (reply_tx, reply_rx) = tokio_oneshot::channel();
        handle
            .command_tx
            .send(AcpProcessCommand::SetControl {
                local_session_id,
                native_session_id,
                cwd,
                mcp_servers,
                control_id,
                value,
                reply: reply_tx,
            })
            .map_err(|_| acp_process_unavailable_error("ACP process mailbox closed"))?;
        let result = reply_rx.await.map_err(|_| {
            acp_process_unavailable_error("ACP process ended while applying a session control")
        })?;
        observe_acp_auth_result(&handle.auth_observation, &result, false);
        result
    }

    pub(crate) async fn list_sessions(
        &self,
        peer: ResolvedPeerTurn,
        invocation_cwd: PathBuf,
        cwd_filter: Option<PathBuf>,
        cursor: Option<String>,
    ) -> psychevo_runtime::Result<AcpSessionListPage> {
        let handle = self.actor(&peer, &invocation_cwd)?;
        let (reply_tx, reply_rx) = tokio_oneshot::channel();
        handle
            .command_tx
            .send(AcpProcessCommand::ListSessions {
                cwd_filter,
                cursor,
                reply: reply_tx,
            })
            .map_err(|_| acp_process_unavailable_error("ACP process mailbox closed"))?;
        let result = reply_rx.await.map_err(|_| {
            acp_process_unavailable_error("ACP process ended while listing Agent sessions")
        })?;
        observe_acp_auth_result(&handle.auth_observation, &result, false);
        result
    }

    pub(crate) async fn resume_session(
        &self,
        peer: ResolvedPeerTurn,
        cwd: PathBuf,
        session: AcpResidentSessionRef,
        mcp_servers: Vec<psychevo_runtime::types::ResolvedMcpServerInput>,
    ) -> psychevo_runtime::Result<AcpSessionSnapshot> {
        let handle = self.actor(&peer, &cwd)?;
        let local_session_id = session.local_session_id.clone();
        let (reply_tx, reply_rx) = tokio_oneshot::channel();
        handle
            .command_tx
            .send(AcpProcessCommand::ResumeSession {
                session,
                cwd,
                mcp_servers,
                reply: reply_tx,
            })
            .map_err(|_| acp_process_unavailable_error("ACP process mailbox closed"))?;
        let result = reply_rx.await.map_err(|_| {
            acp_process_unavailable_error("ACP process ended while resuming an Agent session")
        })?;
        if result.is_ok() {
            self.inner
                .resident_actors
                .lock()
                .map_err(|_| Error::Message("ACP resident actor registry poisoned".to_string()))?
                .insert(local_session_id, handle.clone());
        }
        observe_acp_auth_result(&handle.auth_observation, &result, false);
        result
    }

    pub(crate) async fn fork_session(
        &self,
        peer: ResolvedPeerTurn,
        cwd: PathBuf,
        source: AcpResidentSessionRef,
        fork_local_session_id: String,
    ) -> psychevo_runtime::Result<AcpSessionSnapshot> {
        let handle = self.actor(&peer, &cwd)?;
        let resident_local_session_id = fork_local_session_id.clone();
        let (reply_tx, reply_rx) = tokio_oneshot::channel();
        handle
            .command_tx
            .send(AcpProcessCommand::ForkSession {
                source,
                fork_local_session_id,
                cwd,
                reply: reply_tx,
            })
            .map_err(|_| acp_process_unavailable_error("ACP process mailbox closed"))?;
        let result = reply_rx.await.map_err(|_| {
            acp_process_unavailable_error("ACP process ended while forking an Agent session")
        })?;
        if result.is_ok() {
            self.inner
                .resident_actors
                .lock()
                .map_err(|_| Error::Message("ACP resident actor registry poisoned".to_string()))?
                .insert(resident_local_session_id, handle.clone());
        }
        observe_acp_auth_result(&handle.auth_observation, &result, false);
        result
    }

    pub(crate) async fn close_session(
        &self,
        peer: ResolvedPeerTurn,
        cwd: PathBuf,
        session: AcpResidentSessionRef,
    ) -> psychevo_runtime::Result<()> {
        let handle = self.actor(&peer, &cwd)?;
        let local_session_id = session.local_session_id.clone();
        let (reply_tx, reply_rx) = tokio_oneshot::channel();
        handle
            .command_tx
            .send(AcpProcessCommand::CloseSession {
                session,
                reply: reply_tx,
            })
            .map_err(|_| acp_process_unavailable_error("ACP process mailbox closed"))?;
        let result = reply_rx.await.map_err(|_| {
            acp_process_unavailable_error("ACP process ended while closing an Agent session")
        })?;
        if result.is_ok() {
            self.inner
                .resident_actors
                .lock()
                .map_err(|_| Error::Message("ACP resident actor registry poisoned".to_string()))?
                .remove(&local_session_id);
        }
        observe_acp_auth_result(&handle.auth_observation, &result, false);
        result
    }

    pub(crate) async fn release_session(
        &self,
        session: AcpResidentSessionRef,
    ) -> psychevo_runtime::Result<()> {
        let local_session_id = session.local_session_id.clone();
        let handle = self
            .inner
            .resident_actors
            .lock()
            .map_err(|_| Error::Message("ACP resident actor registry poisoned".to_string()))?
            .get(&local_session_id)
            .filter(|handle| !*handle.done_rx.borrow())
            .cloned();
        let Some(handle) = handle else {
            return Ok(());
        };
        let (reply_tx, reply_rx) = tokio_oneshot::channel();
        handle
            .command_tx
            .send(AcpProcessCommand::ReleaseSession {
                session,
                reply: reply_tx,
            })
            .map_err(|_| acp_process_unavailable_error("ACP process mailbox closed"))?;
        reply_rx.await.map_err(|_| {
            acp_process_unavailable_error("ACP process ended while releasing a draft session")
        })??;
        self.inner
            .resident_actors
            .lock()
            .map_err(|_| Error::Message("ACP resident actor registry poisoned".to_string()))?
            .remove(&local_session_id);
        Ok(())
    }

    pub(crate) async fn delete_session(
        &self,
        peer: ResolvedPeerTurn,
        cwd: PathBuf,
        native_session_id: String,
        resident: Option<AcpResidentSessionRef>,
    ) -> psychevo_runtime::Result<()> {
        let handle = self.actor(&peer, &cwd)?;
        let resident_local_session_id = resident
            .as_ref()
            .map(|resident| resident.local_session_id.clone());
        let (reply_tx, reply_rx) = tokio_oneshot::channel();
        handle
            .command_tx
            .send(AcpProcessCommand::DeleteSession {
                native_session_id,
                resident,
                reply: reply_tx,
            })
            .map_err(|_| acp_process_unavailable_error("ACP process mailbox closed"))?;
        let result = reply_rx.await.map_err(|_| {
            acp_process_unavailable_error("ACP process ended while deleting an Agent session")
        })?;
        if result.is_ok()
            && let Some(local_session_id) = resident_local_session_id
        {
            self.inner
                .resident_actors
                .lock()
                .map_err(|_| Error::Message("ACP resident actor registry poisoned".to_string()))?
                .remove(&local_session_id);
        }
        observe_acp_auth_result(&handle.auth_observation, &result, false);
        result
    }

    pub(crate) async fn probe_protocol_compatibility(
        &self,
        peer: ResolvedPeerTurn,
        cwd: PathBuf,
    ) -> psychevo_runtime::Result<AcpProtocolDoctorStatus> {
        let handle = self.actor(&peer, &cwd)?;
        let mut startup_rx = handle.startup_rx.clone();
        tokio::time::timeout(ACP_PROTOCOL_PROBE_TIMEOUT, async move {
            loop {
                let status = startup_rx.borrow().clone();
                match status {
                    AcpProcessStartupStatus::Starting => {}
                    AcpProcessStartupStatus::Protocol(status) => return Ok(status),
                    AcpProcessStartupStatus::Failed(message) => {
                        return Err(acp_process_unavailable_error(message));
                    }
                }
                startup_rx.changed().await.map_err(|_| {
                    acp_process_unavailable_error(
                        "ACP process ended before reporting protocol compatibility",
                    )
                })?;
            }
        })
        .await
        .map_err(|_| acp_process_unavailable_error("ACP protocol compatibility probe timed out"))?
    }

    pub(crate) async fn probe_authentication(
        &self,
        peer: ResolvedPeerTurn,
        cwd: PathBuf,
    ) -> psychevo_runtime::Result<AcpAuthDoctorStatus> {
        let handle = self.actor(&peer, &cwd)?;
        let (reply_tx, reply_rx) = tokio_oneshot::channel();
        handle
            .command_tx
            .send(AcpProcessCommand::ProbeAuthentication { reply: reply_tx })
            .map_err(|_| acp_process_unavailable_error("ACP process mailbox closed"))?;
        tokio::time::timeout(ACP_AUTH_PROBE_TIMEOUT, reply_rx)
            .await
            .map_err(|_| {
                acp_process_unavailable_error("ACP authentication status probe timed out")
            })?
            .map_err(|_| {
                acp_process_unavailable_error(
                    "ACP process ended while probing authentication status",
                )
            })?
    }

    pub(crate) async fn shutdown(&self, force: bool) -> psychevo_runtime::Result<()> {
        let actors = {
            let actors = self
                .inner
                .actors
                .lock()
                .map_err(|_| Error::Message("ACP process pool lock poisoned".to_string()))?;
            actors.values().cloned().collect::<Vec<_>>()
        };
        for actor in &actors {
            if force {
                let _ = actor.force_tx.send(true);
            } else {
                let _ = actor.command_tx.send(AcpProcessCommand::Shutdown);
            }
        }
        let waits = actors.into_iter().map(|actor| async move {
            let mut done = actor.done_rx;
            if !*done.borrow() {
                let _ = done.changed().await;
            }
        });
        futures::future::join_all(waits).await;
        Ok(())
    }

    fn actor(
        &self,
        peer: &ResolvedPeerTurn,
        cwd: &Path,
    ) -> psychevo_runtime::Result<AcpProcessHandle> {
        let key = acp_process_key(peer, cwd)?;
        let mut actors = self
            .inner
            .actors
            .lock()
            .map_err(|_| Error::Message("ACP process pool lock poisoned".to_string()))?;
        if let Some(actor) = actors.get(&key)
            && !*actor.done_rx.borrow()
        {
            return Ok(actor.clone());
        }
        actors.remove(&key);

        let generation = self.inner.next_generation.fetch_add(1, Ordering::Relaxed);
        let (command_tx, command_rx) = tokio_mpsc::unbounded_channel();
        let (force_tx, force_rx) = watch::channel(false);
        let (done_tx, done_rx) = watch::channel(false);
        let (startup_tx, startup_rx) = watch::channel(AcpProcessStartupStatus::Starting);
        let auth_key = acp_auth_observation_key(peer, cwd)?;
        let auth_observation = {
            let mut observations = self.inner.auth_observations.lock().map_err(|_| {
                Error::Message("ACP authentication observation lock poisoned".to_string())
            })?;
            Arc::clone(
                observations
                    .entry(auth_key)
                    .or_insert_with(|| Arc::new(Mutex::new(AcpObservedAuthState::Unchecked))),
            )
        };
        let handle = AcpProcessHandle {
            generation,
            command_tx,
            force_tx,
            done_rx,
            startup_rx,
            auth_observation: Arc::clone(&auth_observation),
        };
        actors.insert(key.clone(), handle.clone());
        let weak_pool = Arc::downgrade(&self.inner);
        let idle_timeout = self.inner.idle_timeout;
        let peer = peer.clone();
        let invocation_cwd = key.canonical_cwd.clone();
        tokio::spawn(async move {
            run_acp_process_actor(AcpProcessActorInputs {
                peer,
                invocation_cwd,
                generation,
                idle_timeout,
                command_rx,
                force_rx,
                startup_tx,
                auth_observation,
            })
            .await;
            let _ = done_tx.send(true);
            remove_finished_acp_actor(&weak_pool, &key, generation);
        });
        Ok(handle)
    }
}

fn remove_finished_acp_actor(
    pool: &Weak<AcpProcessPoolInner>,
    key: &AcpProcessKey,
    generation: u64,
) {
    let Some(pool) = pool.upgrade() else {
        return;
    };
    let Ok(mut actors) = pool.actors.lock() else {
        return;
    };
    if actors
        .get(key)
        .is_some_and(|actor| actor.generation == generation)
    {
        actors.remove(key);
    }
    drop(actors);
    if let Ok(mut residents) = pool.resident_actors.lock() {
        residents.retain(|_, actor| actor.generation != generation);
    }
}

fn acp_process_key(peer: &ResolvedPeerTurn, cwd: &Path) -> psychevo_runtime::Result<AcpProcessKey> {
    let canonical_cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let launch = resolve_acp_backend_launch(peer, &canonical_cwd)?;
    let mut digest = sha2::Sha256::new();
    digest.update(serde_json::to_vec(&peer.backend)?);
    digest.update([0]);
    digest.update(serde_json::to_vec(&peer.agent)?);
    digest.update([0]);
    digest.update(
        peer.process_scope_fingerprint
            .as_deref()
            .unwrap_or("uncaptured-profile")
            .as_bytes(),
    );
    digest.update([0]);
    digest.update(serde_json::to_vec(&acp_backend_effective_env(peer))?);
    digest.update([0]);
    digest.update(
        psychevo_runtime::host_paths::normalized_native_path(&launch.program)
            .to_string_lossy()
            .as_bytes(),
    );
    digest.update([0]);
    digest.update(
        psychevo_runtime::host_paths::normalized_native_path(&launch.cwd)
            .to_string_lossy()
            .as_bytes(),
    );
    Ok(AcpProcessKey {
        backend_fingerprint: format!("{:x}", digest.finalize()),
        canonical_cwd,
    })
}

fn acp_auth_observation_key(
    peer: &ResolvedPeerTurn,
    cwd: &Path,
) -> psychevo_runtime::Result<AcpAuthObservationKey> {
    let canonical_cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let launch = resolve_acp_backend_launch(peer, &canonical_cwd)?;
    let mut digest = sha2::Sha256::new();
    // Agent instructions and Runtime Profile presentation do not change the
    // Adapter credential store. Backend launch configuration, inherited auth
    // environment, executable, launch cwd, and workspace do.
    digest.update(serde_json::to_vec(&json!({
        "id": peer.backend.id.as_str(),
        "kind": peer.backend.kind.as_str(),
        "command": peer.backend.command.as_deref(),
        "args": &peer.backend.args,
        "env": &peer.backend.env,
        "cwd": peer.backend.cwd.as_str(),
    }))?);
    digest.update([0]);
    digest.update(serde_json::to_vec(&acp_backend_effective_env(peer))?);
    digest.update([0]);
    digest.update(
        psychevo_runtime::host_paths::normalized_native_path(&launch.program)
            .to_string_lossy()
            .as_bytes(),
    );
    digest.update([0]);
    digest.update(
        psychevo_runtime::host_paths::normalized_native_path(&launch.cwd)
            .to_string_lossy()
            .as_bytes(),
    );
    Ok(AcpAuthObservationKey {
        launch_fingerprint: format!("{:x}", digest.finalize()),
        canonical_cwd,
    })
}

fn observe_acp_auth_result<T>(
    observation: &Arc<Mutex<AcpObservedAuthState>>,
    result: &psychevo_runtime::Result<T>,
    clear_on_success: bool,
) {
    let next = match result {
        Ok(_) if clear_on_success => Some(AcpObservedAuthState::Unchecked),
        Err(error)
            if error
                .structured_data()
                .and_then(|data| data.get("code"))
                .and_then(Value::as_str)
                == Some("acp_auth_required") =>
        {
            Some(AcpObservedAuthState::Required)
        }
        _ => None,
    };
    if let Some(next) = next
        && let Ok(mut state) = observation.lock()
    {
        *state = next;
    }
}

#[derive(Clone, Default)]
struct AcpDeliveryMarker(Arc<std::sync::atomic::AtomicBool>);

impl AcpDeliveryMarker {
    fn mark_sent(&self) {
        self.0.store(true, Ordering::Release);
    }

    fn was_sent(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

struct AcpTurnCommand {
    peer: ResolvedPeerTurn,
    context: AcpPeerTurnContext,
    session_ready: AcpSessionReadyCallback,
    delivery: AcpDeliveryMarker,
    reply: tokio_oneshot::Sender<psychevo_runtime::Result<AcpTurnOutput>>,
}

enum AcpProcessCommand {
    ProbeAuthentication {
        reply: tokio_oneshot::Sender<psychevo_runtime::Result<AcpAuthDoctorStatus>>,
    },
    Turn(Box<AcpTurnCommand>),
    Prepare {
        local_session_id: String,
        cwd: PathBuf,
        mcp_servers: Vec<psychevo_runtime::types::ResolvedMcpServerInput>,
        reply: tokio_oneshot::Sender<psychevo_runtime::Result<AcpSessionSnapshot>>,
    },
    Promote {
        old_local_session_id: String,
        new_local_session_id: String,
        native_session_id: String,
        reply: tokio_oneshot::Sender<psychevo_runtime::Result<()>>,
    },
    Inspect {
        local_session_id: String,
        native_session_id: String,
        cwd: PathBuf,
        mcp_servers: Vec<psychevo_runtime::types::ResolvedMcpServerInput>,
        reply: tokio_oneshot::Sender<psychevo_runtime::Result<AcpSessionSnapshot>>,
    },
    LoadSession {
        local_session_id: String,
        native_session_id: String,
        cwd: PathBuf,
        mcp_servers: Vec<psychevo_runtime::types::ResolvedMcpServerInput>,
        reply: tokio_oneshot::Sender<psychevo_runtime::Result<AcpSessionLoadOutput>>,
    },
    InspectCached {
        local_session_id: String,
        native_session_id: String,
        reply: tokio_oneshot::Sender<psychevo_runtime::Result<Option<AcpSessionSnapshot>>>,
    },
    SetControl {
        local_session_id: String,
        native_session_id: String,
        cwd: PathBuf,
        mcp_servers: Vec<psychevo_runtime::types::ResolvedMcpServerInput>,
        control_id: String,
        value: Value,
        reply: tokio_oneshot::Sender<psychevo_runtime::Result<AcpSessionSnapshot>>,
    },
    ListSessions {
        cwd_filter: Option<PathBuf>,
        cursor: Option<String>,
        reply: tokio_oneshot::Sender<psychevo_runtime::Result<AcpSessionListPage>>,
    },
    ResumeSession {
        session: AcpResidentSessionRef,
        cwd: PathBuf,
        mcp_servers: Vec<psychevo_runtime::types::ResolvedMcpServerInput>,
        reply: tokio_oneshot::Sender<psychevo_runtime::Result<AcpSessionSnapshot>>,
    },
    ForkSession {
        source: AcpResidentSessionRef,
        fork_local_session_id: String,
        cwd: PathBuf,
        reply: tokio_oneshot::Sender<psychevo_runtime::Result<AcpSessionSnapshot>>,
    },
    CloseSession {
        session: AcpResidentSessionRef,
        reply: tokio_oneshot::Sender<psychevo_runtime::Result<()>>,
    },
    ReleaseSession {
        session: AcpResidentSessionRef,
        reply: tokio_oneshot::Sender<psychevo_runtime::Result<()>>,
    },
    DeleteSession {
        native_session_id: String,
        resident: Option<AcpResidentSessionRef>,
        reply: tokio_oneshot::Sender<psychevo_runtime::Result<()>>,
    },
    Shutdown,
}

type AcpSessionLocks = Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>;

fn acp_session_lock(
    session_locks: &AcpSessionLocks,
    local_session_id: &str,
) -> psychevo_runtime::Result<Arc<tokio::sync::Mutex<()>>> {
    let mut locks = session_locks
        .lock()
        .map_err(|_| Error::Message("ACP session lock registry poisoned".to_string()))?;
    Ok(Arc::clone(
        locks
            .entry(local_session_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
    ))
}

fn remove_acp_session_lock(
    session_locks: &AcpSessionLocks,
    local_session_id: &str,
    expected: &Arc<tokio::sync::Mutex<()>>,
) {
    let Ok(mut locks) = session_locks.lock() else {
        return;
    };
    if locks
        .get(local_session_id)
        .is_some_and(|current| Arc::ptr_eq(current, expected))
    {
        locks.remove(local_session_id);
    }
}

struct AcpProtocolObservingTransport<T> {
    inner: T,
    startup_tx: watch::Sender<AcpProcessStartupStatus>,
}

impl<T> AcpProtocolObservingTransport<T> {
    fn new(inner: T, startup_tx: watch::Sender<AcpProcessStartupStatus>) -> Self {
        Self { inner, startup_tx }
    }
}

impl<T, R> ConnectTo<R> for AcpProtocolObservingTransport<T>
where
    T: ConnectTo<R>,
    R: Role,
{
    async fn connect_to(
        self,
        client: impl ConnectTo<R::Counterpart>,
    ) -> Result<(), agent_client_protocol::Error> {
        let (channel, serve_self) =
            <Self as ConnectTo<R>>::into_channel_and_future(self);
        match futures::future::select(Box::pin(client.connect_to(channel)), serve_self).await {
            futures::future::Either::Left((result, _))
            | futures::future::Either::Right((result, _)) => result,
        }
    }

    fn into_channel_and_future(
        self,
    ) -> (
        Channel,
        BoxFuture<'static, Result<(), agent_client_protocol::Error>>,
    ) {
        let (inner_channel, inner_future) = self.inner.into_channel_and_future();
        let Channel {
            rx: mut inner_rx,
            tx: inner_tx,
        } = inner_channel;
        let (incoming_tx, incoming_rx) = mpsc::unbounded();
        let (outgoing_tx, mut outgoing_rx) = mpsc::unbounded();
        let initialize_request_id = Arc::new(Mutex::new(None));
        let proxy_initialize_request_id = Arc::clone(&initialize_request_id);
        let startup_tx = self.startup_tx;
        let proxy_future: BoxFuture<'static, Result<(), agent_client_protocol::Error>> =
            Box::pin(async move {
                loop {
                    tokio::select! {
                        incoming = inner_rx.next() => {
                            let Some(incoming) = incoming else {
                                break;
                            };
                            observe_raw_acp_initialize_response(
                                &incoming,
                                &proxy_initialize_request_id,
                                &startup_tx,
                            );
                            if incoming_tx.unbounded_send(incoming).is_err() {
                                break;
                            }
                        }
                        outgoing = outgoing_rx.next() => {
                            let Some(outgoing) = outgoing else {
                                break;
                            };
                            observe_raw_acp_initialize_request(
                                &outgoing,
                                &initialize_request_id,
                            );
                            if inner_tx.unbounded_send(outgoing).is_err() {
                                break;
                            }
                        }
                    }
                }
                Ok(())
            });
        let future = Box::pin(async move {
            match futures::future::select(inner_future, proxy_future).await {
                futures::future::Either::Left((result, _))
                | futures::future::Either::Right((result, _)) => result,
            }
        });
        (
            Channel {
                rx: incoming_rx,
                tx: outgoing_tx,
            },
            future,
        )
    }
}

fn observe_raw_acp_initialize_request(
    message: &Result<RawJsonRpcMessage, agent_client_protocol::Error>,
    initialize_request_id: &Mutex<Option<agent_client_protocol::schema::v1::RequestId>>,
) {
    let Ok(RawJsonRpcMessage::Request(request)) = message else {
        return;
    };
    if request.method.as_ref() != "initialize" {
        return;
    }
    if let Ok(mut request_id) = initialize_request_id.lock() {
        *request_id = Some(request.id.clone());
    }
}

fn observe_raw_acp_initialize_response(
    message: &Result<RawJsonRpcMessage, agent_client_protocol::Error>,
    initialize_request_id: &Mutex<Option<agent_client_protocol::schema::v1::RequestId>>,
    startup_tx: &watch::Sender<AcpProcessStartupStatus>,
) {
    let Ok(RawJsonRpcMessage::Response(AcpJsonRpcResponse::Result { id, result })) = message else {
        return;
    };
    let matches_initialize = initialize_request_id
        .lock()
        .ok()
        .and_then(|request_id| request_id.clone())
        .is_some_and(|request_id| request_id == *id);
    if !matches_initialize {
        return;
    }
    let Some(actual) = result
        .get("protocolVersion")
        .and_then(|version| serde_json::from_value::<ProtocolVersion>(version.clone()).ok())
    else {
        return;
    };
    if actual != ProtocolVersion::V1 {
        startup_tx.send_replace(AcpProcessStartupStatus::Protocol(
            AcpProtocolDoctorStatus::Incompatible {
                expected_version: ProtocolVersion::V1.as_u16(),
                actual_version: actual.as_u16(),
            },
        ));
    }
}

struct AcpProcessActorInputs {
    peer: ResolvedPeerTurn,
    invocation_cwd: PathBuf,
    generation: u64,
    idle_timeout: Duration,
    command_rx: tokio_mpsc::UnboundedReceiver<AcpProcessCommand>,
    force_rx: watch::Receiver<bool>,
    startup_tx: watch::Sender<AcpProcessStartupStatus>,
    auth_observation: Arc<Mutex<AcpObservedAuthState>>,
}

async fn run_acp_process_actor(inputs: AcpProcessActorInputs) {
    let AcpProcessActorInputs {
        peer,
        invocation_cwd,
        generation,
        idle_timeout,
        command_rx,
        force_rx,
        startup_tx,
        auth_observation,
    } = inputs;
    let Ok((mut command, _)) = acp_backend_command(&peer, &invocation_cwd) else {
        startup_tx.send_replace(AcpProcessStartupStatus::Failed(
            "ACP backend launch configuration could not be resolved".to_string(),
        ));
        return;
    };
    let Ok(mut child) = command.spawn() else {
        startup_tx.send_replace(AcpProcessStartupStatus::Failed(
            "ACP process could not be launched".to_string(),
        ));
        return;
    };
    let Some(stdin) = child.stdin.take() else {
        startup_tx.send_replace(AcpProcessStartupStatus::Failed(
            "ACP process did not expose stdin".to_string(),
        ));
        psychevo_runtime::process_env::terminate_tokio_child_tree(&mut child).await;
        return;
    };
    let Some(stdout) = child.stdout.take() else {
        startup_tx.send_replace(AcpProcessStartupStatus::Failed(
            "ACP process did not expose stdout".to_string(),
        ));
        psychevo_runtime::process_env::terminate_tokio_child_tree(&mut child).await;
        return;
    };
    let transport = AcpProtocolObservingTransport::new(
        ByteStreams::new(stdin.compat_write(), stdout.compat()),
        startup_tx.clone(),
    );
    let contexts = Arc::new(Mutex::new(BTreeMap::<String, Arc<AcpClientContext>>::new()));
    let terminals = AcpTerminalRegistry::default();
    let teardown_contexts = Arc::clone(&contexts);
    let teardown_terminals = terminals.clone();
    let (notification_ingress, notification_rx) = AcpNotificationIngress::channel();
    let peer_for_connection = peer.clone();
    let connection_startup_tx = startup_tx.clone();
    let connection = Client
        .builder()
        .name("psychevo-gateway-acp-peer")
        .on_receive_dispatch(
            {
                let notification_ingress = notification_ingress.clone();
                async move |dispatch: Dispatch, _cx: ConnectionTo<Agent>| match dispatch {
                    Dispatch::Response(result, router) => {
                        // Unlike SentRequest::block_task, this callback is
                        // executed synchronously by the SDK's central dispatch
                        // loop. Queue the fence before forwarding the typed
                        // response to its waiter.
                        notification_ingress
                            .response_barrier(router.id())
                            .map_err(|error| {
                                agent_client_protocol::Error::internal_error()
                                    .data(error.to_string())
                            })?;
                        router.respond_with_result(result)?;
                        Ok(Handled::Yes)
                    }
                    dispatch => Ok(Handled::No {
                        message: dispatch,
                        retry: false,
                    }),
                }
            },
            agent_client_protocol::on_receive_dispatch!(),
        )
        .on_receive_notification(
            {
                let notification_ingress = notification_ingress.clone();
                async move |notification: UntypedMessage, _cx| {
                    let payload = if notification.method == "session/update" {
                        serde_json::from_value::<SessionNotification>(notification.params.clone())
                            .map(|notification| {
                                AcpPeerInboundPayload::Session(Box::new(notification))
                            })
                            .unwrap_or_else(|_| AcpPeerInboundPayload::Unknown {
                                method: notification.method,
                                params: notification.params,
                            })
                    } else {
                        AcpPeerInboundPayload::Unknown {
                            method: notification.method,
                            params: notification.params,
                        }
                    };
                    let _ = notification_ingress.notification(payload);
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            {
                let contexts = Arc::clone(&contexts);
                async move |request: ReadTextFileRequest, responder, cx| {
                    let context = acp_request_context(&contexts, &request.session_id.to_string())?;
                    let cancellation = responder.cancellation();
                    cx.spawn(async move {
                        let response = cancellation
                            .run_until_cancelled(read_text_file(context, request))
                            .await;
                        responder.respond_with_result(response)
                    })?;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let contexts = Arc::clone(&contexts);
                async move |request: WriteTextFileRequest, responder, cx| {
                    let context = acp_request_context(&contexts, &request.session_id.to_string())?;
                    let cancellation = responder.cancellation();
                    cx.spawn(async move {
                        let response = cancellation
                            .run_until_cancelled(write_text_file(context, request))
                            .await;
                        responder.respond_with_result(response)
                    })?;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let contexts = Arc::clone(&contexts);
                async move |request: RequestPermissionRequest, responder, cx| {
                    let context = acp_request_context(&contexts, &request.session_id.to_string())?;
                    let cancellation = responder.cancellation();
                    cx.spawn(async move {
                        let response = cancellation
                            .run_until_cancelled(request_permission(context, request))
                            .await;
                        responder.respond_with_result(response)
                    })?;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let contexts = Arc::clone(&contexts);
                async move |request: CreateElicitationRequest, responder, cx| {
                    let contexts = Arc::clone(&contexts);
                    let cancellation = responder.cancellation();
                    cx.spawn(async move {
                        let response = cancellation
                            .run_until_cancelled(create_elicitation(&contexts, request))
                            .await;
                        responder.respond_with_result(response)
                    })?;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let contexts = Arc::clone(&contexts);
                let terminals = terminals.clone();
                async move |request: CreateTerminalRequest, responder, cx| {
                    let context = acp_request_context(&contexts, &request.session_id.to_string())?;
                    let terminals = terminals.clone();
                    let cancellation = responder.cancellation();
                    cx.spawn(async move {
                        let response = cancellation
                            .run_until_cancelled(create_terminal(terminals, context, request))
                            .await;
                        responder.respond_with_result(response)
                    })?;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let terminals = terminals.clone();
                async move |request: TerminalOutputRequest, responder, cx| {
                    let terminals = terminals.clone();
                    let cancellation = responder.cancellation();
                    cx.spawn(async move {
                        let response = cancellation
                            .run_until_cancelled(terminal_output(terminals, request))
                            .await;
                        responder.respond_with_result(response)
                    })?;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let terminals = terminals.clone();
                async move |request: WaitForTerminalExitRequest, responder, cx| {
                    let terminals = terminals.clone();
                    let cancellation = responder.cancellation();
                    cx.spawn(async move {
                        let response = cancellation
                            .run_until_cancelled(wait_for_terminal_exit(terminals, request))
                            .await;
                        responder.respond_with_result(response)
                    })?;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let terminals = terminals.clone();
                async move |request: KillTerminalRequest, responder, cx| {
                    let terminals = terminals.clone();
                    let cancellation = responder.cancellation();
                    cx.spawn(async move {
                        let response = cancellation
                            .run_until_cancelled(kill_terminal(terminals, request))
                            .await;
                        responder.respond_with_result(response)
                    })?;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let terminals = terminals.clone();
                async move |request: ReleaseTerminalRequest, responder, cx| {
                    let terminals = terminals.clone();
                    let cancellation = responder.cancellation();
                    cx.spawn(async move {
                        let response = cancellation
                            .run_until_cancelled(release_terminal(terminals, request))
                            .await;
                        responder.respond_with_result(response)
                    })?;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(transport, async move |cx| {
            let initialized = match initialize_acp_v1(
                &cx,
                &peer_for_connection,
                "psychevo-gateway-acp-peer",
            )
            .await
            {
                Ok(AcpV1Initialization::Compatible(initialized)) => {
                    connection_startup_tx.send_replace(AcpProcessStartupStatus::Protocol(
                        AcpProtocolDoctorStatus::Compatible {
                            version: initialized.protocol_version.as_u16(),
                        },
                    ));
                    Arc::from(initialized)
                }
                Ok(AcpV1Initialization::Incompatible { expected, actual }) => {
                    connection_startup_tx.send_replace(AcpProcessStartupStatus::Protocol(
                        AcpProtocolDoctorStatus::Incompatible {
                            expected_version: expected.as_u16(),
                            actual_version: actual.as_u16(),
                        },
                    ));
                    return Err(agent_client_protocol::Error::invalid_request().data(format!(
                        "ACP peer `{}` negotiated unsupported protocol version {}; stable v{} is required",
                        peer_for_connection.backend.id, actual, expected
                    )));
                }
                Err(error) => {
                    if matches!(
                        *connection_startup_tx.borrow(),
                        AcpProcessStartupStatus::Starting
                    ) {
                        connection_startup_tx.send_replace(AcpProcessStartupStatus::Failed(
                            format!(
                                "ACP initialize request failed: {}",
                                safe_acp_error(&error)
                            ),
                        ));
                    }
                    return Err(error);
                }
            };
            let sessions: AcpResidentSessions =
                Arc::new(tokio::sync::Mutex::new(BTreeMap::new()));
            let notification_router = AcpNotificationRouter::default();
            let session_locks: AcpSessionLocks = Arc::new(Mutex::new(HashMap::new()));
            let next_session_epoch = Arc::new(AtomicU64::new(1));
            let mut command_rx = command_rx;
            let mut notification_rx = notification_rx;
            let mut force_rx = force_rx;
            let mut tasks = tokio::task::JoinSet::new();
            let mut shutdown_requested = false;
            loop {
                if shutdown_requested && tasks.is_empty() {
                    close_resident_acp_sessions(
                        &cx,
                        &initialized,
                        &contexts,
                        &sessions,
                        &terminals,
                    )
                    .await;
                    break;
                }
                let idle = tokio::time::sleep(idle_timeout);
                tokio::pin!(idle);
                tokio::select! {
                    biased;
                    force = force_rx.changed() => {
                        if force.is_err() || *force_rx.borrow() {
                            break;
                        }
                    }
                    completed = tasks.join_next(), if !tasks.is_empty() => {
                        let _ = completed;
                    }
                    command = command_rx.recv(), if !shutdown_requested => {
                        let Some(command) = command else {
                            shutdown_requested = true;
                            continue;
                        };
                        match command {
                            AcpProcessCommand::ProbeAuthentication { reply } => {
                                let cx = cx.clone();
                                let initialized = Arc::clone(&initialized);
                                let auth_observation = Arc::clone(&auth_observation);
                                let notification_ingress = notification_ingress.clone();
                                tasks.spawn(async move {
                                    let result = probe_acp_authentication_status(
                                        &cx,
                                        &initialized,
                                        &auth_observation,
                                        &notification_ingress,
                                    )
                                    .await;
                                    let _ = reply.send(result);
                                });
                            }
                            AcpProcessCommand::Turn(command) => {
                                let AcpTurnCommand {
                                    peer,
                                    context,
                                    session_ready,
                                    delivery,
                                    reply,
                                } = *command;
                                let session_lock = match acp_session_lock(
                                    &session_locks,
                                    &context.local_session_id,
                                ) {
                                    Ok(lock) => lock,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let mut subscription = match notification_router
                                    .subscribe(context.native_session_id.clone())
                                {
                                    Ok(subscription) => subscription,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let cx = cx.clone();
                                let initialized = Arc::clone(&initialized);
                                let contexts = Arc::clone(&contexts);
                                let sessions = Arc::clone(&sessions);
                                let notification_ingress = notification_ingress.clone();
                                let mut task_force_rx = force_rx.clone();
                                let next_session_epoch = Arc::clone(&next_session_epoch);
                                tasks.spawn(async move {
                                    let _session_guard = session_lock.lock().await;
                                    let result = execute_resident_acp_turn(
                                        &cx,
                                        &initialized,
                                        &peer,
                                        &contexts,
                                        &sessions,
                                        &notification_ingress,
                                        &mut subscription,
                                        &mut task_force_rx,
                                        &next_session_epoch,
                                        generation,
                                        context,
                                        session_ready,
                                        delivery,
                                    ).await;
                                    drain_acp_notification_subscription(
                                        &mut subscription,
                                        &sessions,
                                        generation,
                                    ).await;
                                    let _ = reply.send(result);
                                });
                            }
                            AcpProcessCommand::Prepare { local_session_id, cwd, mcp_servers, reply } => {
                                let session_lock = match acp_session_lock(&session_locks, &local_session_id) {
                                    Ok(lock) => lock,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let mut subscription = match notification_router.subscribe(None) {
                                    Ok(subscription) => subscription,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let cx = cx.clone();
                                let initialized = Arc::clone(&initialized);
                                let peer = peer_for_connection.clone();
                                let contexts = Arc::clone(&contexts);
                                let sessions = Arc::clone(&sessions);
                                let notification_ingress = notification_ingress.clone();
                                let next_session_epoch = Arc::clone(&next_session_epoch);
                                tasks.spawn(async move {
                                    let _session_guard = session_lock.lock().await;
                                    let result = prepare_resident_acp_session(
                                        &cx,
                                        &initialized,
                                        &peer,
                                        &contexts,
                                        &sessions,
                                        &notification_ingress,
                                        &mut subscription,
                                        &next_session_epoch,
                                        generation,
                                        local_session_id,
                                        cwd,
                                        mcp_servers,
                                    ).await;
                                    drain_acp_notification_subscription(
                                        &mut subscription,
                                        &sessions,
                                        generation,
                                    ).await;
                                    let _ = reply.send(result);
                                });
                            }
                            AcpProcessCommand::Promote { old_local_session_id, new_local_session_id, native_session_id, reply } => {
                                let session_lock = match acp_session_lock(&session_locks, &old_local_session_id) {
                                    Ok(lock) => lock,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let sessions = Arc::clone(&sessions);
                                tasks.spawn(async move {
                                    let _session_guard = session_lock.lock().await;
                                    let result = async {
                                        let mut sessions = sessions.lock().await;
                                        if sessions.contains_key(&new_local_session_id) {
                                            return Err(crate::agent_session_error(
                                                "acp_session_promotion_conflict",
                                                crate::AgentErrorStage::Binding,
                                                "never",
                                                "not_delivered",
                                                "ACP draft promotion destination is already resident.",
                                                Some(format!("acp-session:{new_local_session_id}")),
                                            ));
                                        }
                                        let session = sessions.remove(&old_local_session_id).ok_or_else(|| {
                                            acp_process_unavailable_error("prepared ACP session disappeared before promotion")
                                        })?;
                                        if session.native_session_id != native_session_id {
                                            sessions.insert(old_local_session_id, session);
                                            return Err(crate::agent_session_error(
                                                "acp_session_identity_mismatch",
                                                crate::AgentErrorStage::Binding,
                                                "never",
                                                "not_delivered",
                                                "Prepared ACP session identity changed before promotion.",
                                                None,
                                            ));
                                        }
                                        sessions.insert(new_local_session_id, session);
                                        Ok(())
                                    }.await;
                                    let _ = reply.send(result);
                                });
                            }
                            AcpProcessCommand::Inspect { local_session_id, native_session_id, cwd, mcp_servers, reply } => {
                                let session_lock = match acp_session_lock(&session_locks, &local_session_id) {
                                    Ok(lock) => lock,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let mut subscription = match notification_router
                                    .subscribe(Some(native_session_id.clone()))
                                {
                                    Ok(subscription) => subscription,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let cx = cx.clone();
                                let initialized = Arc::clone(&initialized);
                                let peer = peer_for_connection.clone();
                                let contexts = Arc::clone(&contexts);
                                let sessions = Arc::clone(&sessions);
                                let notification_ingress = notification_ingress.clone();
                                let next_session_epoch = Arc::clone(&next_session_epoch);
                                tasks.spawn(async move {
                                    let _session_guard = session_lock.lock().await;
                                    let result = inspect_resident_acp_session(
                                        &cx,
                                        &initialized,
                                        &peer,
                                        &contexts,
                                        &sessions,
                                        &notification_ingress,
                                        &mut subscription,
                                        &next_session_epoch,
                                        generation,
                                        local_session_id,
                                        native_session_id,
                                        cwd,
                                        mcp_servers,
                                    ).await;
                                    drain_acp_notification_subscription(
                                        &mut subscription,
                                        &sessions,
                                        generation,
                                    ).await;
                                    let _ = reply.send(result);
                                });
                            }
                            AcpProcessCommand::LoadSession { local_session_id, native_session_id, cwd, mcp_servers, reply } => {
                                let session_lock = match acp_session_lock(&session_locks, &local_session_id) {
                                    Ok(lock) => lock,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let mut subscription = match notification_router
                                    .subscribe(Some(native_session_id.clone()))
                                {
                                    Ok(subscription) => subscription,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let cx = cx.clone();
                                let initialized = Arc::clone(&initialized);
                                let peer = peer_for_connection.clone();
                                let contexts = Arc::clone(&contexts);
                                let sessions = Arc::clone(&sessions);
                                let notification_ingress = notification_ingress.clone();
                                let next_session_epoch = Arc::clone(&next_session_epoch);
                                tasks.spawn(async move {
                                    let _session_guard = session_lock.lock().await;
                                    let result = load_resident_acp_session(
                                        &cx,
                                        &initialized,
                                        &peer,
                                        &contexts,
                                        &sessions,
                                        &notification_ingress,
                                        &mut subscription,
                                        &next_session_epoch,
                                        generation,
                                        local_session_id,
                                        native_session_id,
                                        cwd,
                                        mcp_servers,
                                    ).await;
                                    drain_acp_notification_subscription(
                                        &mut subscription,
                                        &sessions,
                                        generation,
                                    ).await;
                                    let _ = reply.send(result);
                                });
                            }
                            AcpProcessCommand::InspectCached { local_session_id, native_session_id, reply } => {
                                let sessions = Arc::clone(&sessions);
                                tasks.spawn(async move {
                                    let result = {
                                        let sessions = sessions.lock().await;
                                        sessions.get(&local_session_id).map(|session| {
                                            let expected = AcpResidentSessionRef {
                                                local_session_id: local_session_id.clone(),
                                                native_session_id: native_session_id.clone(),
                                            };
                                            validate_lifecycle_session_identity(session, &expected)
                                                .map(|()| acp_session_snapshot(session, generation))
                                        }).transpose()
                                    };
                                    let _ = reply.send(result);
                                });
                            }
                            AcpProcessCommand::SetControl { local_session_id, native_session_id, cwd, mcp_servers, control_id, value, reply } => {
                                let session_lock = match acp_session_lock(&session_locks, &local_session_id) {
                                    Ok(lock) => lock,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let mut subscription = match notification_router
                                    .subscribe(Some(native_session_id.clone()))
                                {
                                    Ok(subscription) => subscription,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let cx = cx.clone();
                                let initialized = Arc::clone(&initialized);
                                let peer = peer_for_connection.clone();
                                let contexts = Arc::clone(&contexts);
                                let sessions = Arc::clone(&sessions);
                                let notification_ingress = notification_ingress.clone();
                                let next_session_epoch = Arc::clone(&next_session_epoch);
                                tasks.spawn(async move {
                                    let _session_guard = session_lock.lock().await;
                                    let result = set_resident_acp_control(
                                        &cx,
                                        &initialized,
                                        &peer,
                                        &contexts,
                                        &sessions,
                                        &notification_ingress,
                                        &mut subscription,
                                        &next_session_epoch,
                                        generation,
                                        local_session_id,
                                        native_session_id,
                                        cwd,
                                        mcp_servers,
                                        control_id,
                                        value,
                                    ).await;
                                    drain_acp_notification_subscription(
                                        &mut subscription,
                                        &sessions,
                                        generation,
                                    ).await;
                                    let _ = reply.send(result);
                                });
                            }
                            AcpProcessCommand::ListSessions { cwd_filter, cursor, reply } => {
                                if let Err(error) = require_acp_lifecycle_capability(
                                    &initialized,
                                    AcpLifecycleCapability::List,
                                ) {
                                    let _ = reply.send(Err(error));
                                    continue;
                                }
                                let mut subscription = match notification_router.subscribe(None) {
                                    Ok(subscription) => subscription,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let cx = cx.clone();
                                let initialized = Arc::clone(&initialized);
                                let sessions = Arc::clone(&sessions);
                                let notification_ingress = notification_ingress.clone();
                                tasks.spawn(async move {
                                    let result = listed_acp_sessions(
                                        &cx,
                                        &initialized,
                                        &sessions,
                                        &notification_ingress,
                                        &mut subscription,
                                        generation,
                                        AcpListSessionsInput {
                                            cwd: cwd_filter,
                                            cursor,
                                        },
                                    )
                                    .await;
                                    drain_acp_notification_subscription(
                                        &mut subscription,
                                        &sessions,
                                        generation,
                                    )
                                    .await;
                                    let _ = reply.send(result);
                                });
                            }
                            AcpProcessCommand::ResumeSession { session, cwd, mcp_servers, reply } => {
                                if let Err(error) = require_acp_lifecycle_capability(
                                    &initialized,
                                    AcpLifecycleCapability::Resume,
                                ) {
                                    let _ = reply.send(Err(error));
                                    continue;
                                }
                                let session_lock = match acp_session_lock(
                                    &session_locks,
                                    &session.local_session_id,
                                ) {
                                    Ok(lock) => lock,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let mut subscription = match notification_router
                                    .subscribe(Some(session.native_session_id.clone()))
                                {
                                    Ok(subscription) => subscription,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let cx = cx.clone();
                                let initialized = Arc::clone(&initialized);
                                let peer = peer_for_connection.clone();
                                let contexts = Arc::clone(&contexts);
                                let sessions = Arc::clone(&sessions);
                                let notification_ingress = notification_ingress.clone();
                                let next_session_epoch = Arc::clone(&next_session_epoch);
                                tasks.spawn(async move {
                                    let _session_guard = session_lock.lock().await;
                                    let result = resume_resident_acp_session(
                                        &cx,
                                        &initialized,
                                        &peer,
                                        &contexts,
                                        &sessions,
                                        &notification_ingress,
                                        &mut subscription,
                                        &next_session_epoch,
                                        generation,
                                        session,
                                        cwd,
                                        mcp_servers,
                                    )
                                    .await;
                                    drain_acp_notification_subscription(
                                        &mut subscription,
                                        &sessions,
                                        generation,
                                    )
                                    .await;
                                    let _ = reply.send(result);
                                });
                            }
                            AcpProcessCommand::ForkSession { source, fork_local_session_id, cwd, reply } => {
                                if let Err(error) = require_acp_lifecycle_capability(
                                    &initialized,
                                    AcpLifecycleCapability::Fork,
                                ) {
                                    let _ = reply.send(Err(error));
                                    continue;
                                }
                                if source.local_session_id == fork_local_session_id {
                                    let _ = reply.send(Err(acp_lifecycle_error(
                                        "acp_session_fork_identity_conflict",
                                        "ACP session/fork requires a distinct destination public Thread.",
                                    )));
                                    continue;
                                }
                                let source_lock = match acp_session_lock(
                                    &session_locks,
                                    &source.local_session_id,
                                ) {
                                    Ok(lock) => lock,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let fork_lock = match acp_session_lock(
                                    &session_locks,
                                    &fork_local_session_id,
                                ) {
                                    Ok(lock) => lock,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let mut subscription = match notification_router.subscribe(None) {
                                    Ok(subscription) => subscription,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let cx = cx.clone();
                                let initialized = Arc::clone(&initialized);
                                let peer = peer_for_connection.clone();
                                let contexts = Arc::clone(&contexts);
                                let sessions = Arc::clone(&sessions);
                                let notification_ingress = notification_ingress.clone();
                                let next_session_epoch = Arc::clone(&next_session_epoch);
                                let source_first = source.local_session_id < fork_local_session_id;
                                tasks.spawn(async move {
                                    let result = if source_first {
                                        let _source_guard = source_lock.lock().await;
                                        let _fork_guard = fork_lock.lock().await;
                                        fork_resident_acp_session(
                                            &cx,
                                            &initialized,
                                            &peer,
                                            &contexts,
                                            &sessions,
                                            &notification_ingress,
                                            &mut subscription,
                                            &next_session_epoch,
                                            generation,
                                            source,
                                            fork_local_session_id,
                                            cwd,
                                        )
                                        .await
                                    } else {
                                        let _fork_guard = fork_lock.lock().await;
                                        let _source_guard = source_lock.lock().await;
                                        fork_resident_acp_session(
                                            &cx,
                                            &initialized,
                                            &peer,
                                            &contexts,
                                            &sessions,
                                            &notification_ingress,
                                            &mut subscription,
                                            &next_session_epoch,
                                            generation,
                                            source,
                                            fork_local_session_id,
                                            cwd,
                                        )
                                        .await
                                    };
                                    drain_acp_notification_subscription(
                                        &mut subscription,
                                        &sessions,
                                        generation,
                                    )
                                    .await;
                                    let _ = reply.send(result);
                                });
                            }
                            AcpProcessCommand::ReleaseSession { session, reply } => {
                                let session_lock = match acp_session_lock(
                                    &session_locks,
                                    &session.local_session_id,
                                ) {
                                    Ok(lock) => lock,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let contexts = Arc::clone(&contexts);
                                let sessions = Arc::clone(&sessions);
                                let terminals = terminals.clone();
                                let session_locks = Arc::clone(&session_locks);
                                tasks.spawn(async move {
                                    let _session_guard = session_lock.lock().await;
                                    let result = remove_resident_session_resources(
                                        &contexts,
                                        &sessions,
                                        &terminals,
                                        &session,
                                    ).await;
                                    if result.is_ok() {
                                        remove_acp_session_lock(
                                            &session_locks,
                                            &session.local_session_id,
                                            &session_lock,
                                        );
                                    }
                                    let _ = reply.send(result);
                                });
                            }
                            AcpProcessCommand::CloseSession { session, reply } => {
                                if let Err(error) = require_acp_lifecycle_capability(
                                    &initialized,
                                    AcpLifecycleCapability::Close,
                                ) {
                                    let _ = reply.send(Err(error));
                                    continue;
                                }
                                let session_lock = match acp_session_lock(
                                    &session_locks,
                                    &session.local_session_id,
                                ) {
                                    Ok(lock) => lock,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let mut subscription = match notification_router
                                    .subscribe(Some(session.native_session_id.clone()))
                                {
                                    Ok(subscription) => subscription,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let cx = cx.clone();
                                let initialized = Arc::clone(&initialized);
                                let contexts = Arc::clone(&contexts);
                                let sessions = Arc::clone(&sessions);
                                let terminals = terminals.clone();
                                let notification_ingress = notification_ingress.clone();
                                let session_locks = Arc::clone(&session_locks);
                                tasks.spawn(async move {
                                    let result = match validate_resident_session_ref(
                                        &sessions,
                                        &session,
                                    )
                                    .await
                                    .and_then(|()| {
                                        cooperative_acp_session_cancel(
                                            &cx,
                                            &session.native_session_id,
                                        )
                                    }) {
                                        Err(error) => Err(error),
                                        Ok(()) => {
                                            let _session_guard = session_lock.lock().await;
                                            close_resident_acp_session(
                                                &cx,
                                                &initialized,
                                                &contexts,
                                                &sessions,
                                                &terminals,
                                                &notification_ingress,
                                                &mut subscription,
                                                generation,
                                                session.clone(),
                                            )
                                            .await
                                        }
                                    };
                                    drain_acp_notification_subscription(
                                        &mut subscription,
                                        &sessions,
                                        generation,
                                    )
                                    .await;
                                    if result.is_ok() {
                                        remove_acp_session_lock(
                                            &session_locks,
                                            &session.local_session_id,
                                            &session_lock,
                                        );
                                    }
                                    let _ = reply.send(result);
                                });
                            }
                            AcpProcessCommand::DeleteSession { native_session_id, resident, reply } => {
                                if let Err(error) = require_acp_lifecycle_capability(
                                    &initialized,
                                    AcpLifecycleCapability::Delete,
                                ) {
                                    let _ = reply.send(Err(error));
                                    continue;
                                }
                                let lock_id = resident
                                    .as_ref()
                                    .map(|session| session.local_session_id.clone())
                                    .unwrap_or_else(|| format!("native-delete:{native_session_id}"));
                                let session_lock = match acp_session_lock(&session_locks, &lock_id) {
                                    Ok(lock) => lock,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let mut subscription = match notification_router
                                    .subscribe(Some(native_session_id.clone()))
                                {
                                    Ok(subscription) => subscription,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                                let cx = cx.clone();
                                let initialized = Arc::clone(&initialized);
                                let contexts = Arc::clone(&contexts);
                                let sessions = Arc::clone(&sessions);
                                let terminals = terminals.clone();
                                let notification_ingress = notification_ingress.clone();
                                let session_locks = Arc::clone(&session_locks);
                                tasks.spawn(async move {
                                    let result = match validate_delete_session_ref(
                                        &sessions,
                                        &native_session_id,
                                        resident.as_ref(),
                                    )
                                    .await
                                    .and_then(|()| {
                                        cooperative_acp_session_cancel(&cx, &native_session_id)
                                    }) {
                                        Err(error) => Err(error),
                                        Ok(()) => {
                                            let _session_guard = session_lock.lock().await;
                                            delete_acp_session(
                                                &cx,
                                                &initialized,
                                                &contexts,
                                                &sessions,
                                                &terminals,
                                                &notification_ingress,
                                                &mut subscription,
                                                generation,
                                                native_session_id,
                                                resident,
                                            )
                                            .await
                                        }
                                    };
                                    drain_acp_notification_subscription(
                                        &mut subscription,
                                        &sessions,
                                        generation,
                                    )
                                    .await;
                                    if result.is_ok() {
                                        remove_acp_session_lock(
                                            &session_locks,
                                            &lock_id,
                                            &session_lock,
                                        );
                                    }
                                    let _ = reply.send(result);
                                });
                            }
                            AcpProcessCommand::Shutdown => {
                                shutdown_requested = true;
                            }
                        }
                    }
                    notification = notification_rx.next() => {
                        let Some(notification) = notification else { break; };
                        let owned = notification_router.publish(notification.clone());
                        if !owned {
                            reduce_idle_acp_notification(&sessions, generation, notification).await;
                        }
                    }
                    _ = &mut idle, if tasks.is_empty() => {
                        shutdown_requested = true;
                    },
                }
            }
            Ok(())
        });
    tokio::pin!(connection);
    tokio::select! {
        _ = &mut connection => {}
        _ = child.wait() => {}
    }

    if let Ok(mut contexts) = teardown_contexts.lock() {
        contexts.clear();
    }
    let _ = teardown_terminals.terminate_all();

    psychevo_runtime::process_env::terminate_tokio_child_tree(&mut child).await;
    let _ = child.wait().await;
}

async fn probe_acp_authentication_status(
    cx: &ConnectionTo<Agent>,
    initialized: &InitializeResponse,
    observation: &Arc<Mutex<AcpObservedAuthState>>,
    notification_ingress: &AcpNotificationIngress,
) -> psychevo_runtime::Result<AcpAuthDoctorStatus> {
    if reviewed_initialize_capability_pack(initialized) != Some(AcpCapabilityPackKind::Codex) {
        return Ok(match observation.lock() {
            Ok(state) if *state == AcpObservedAuthState::Required => AcpAuthDoctorStatus::Required,
            _ => AcpAuthDoctorStatus::Unchecked,
        });
    }

    let (response, _response_barrier) = tokio::time::timeout(
        ACP_AUTH_REQUEST_TIMEOUT,
        acp_response_with_projection_barrier(
            cx.send_request(CodexAuthenticationStatusRequest {}),
            notification_ingress,
        ),
    )
    .await
    .map_err(|_| {
        acp_process_unavailable_error("Codex ACP authentication/status request timed out")
    })?
    .map_err(|error| {
        acp_agent_not_delivered_error(
            "acp_auth_status_probe_failed",
            "authentication/status",
            &error,
        )
    })?;
    let status = match response {
        CodexAuthenticationStatusResponse::Unauthenticated => AcpAuthDoctorStatus::Required,
        CodexAuthenticationStatusResponse::ApiKey => {
            AcpAuthDoctorStatus::Authenticated(AcpAuthenticatedKind::ApiKey)
        }
        CodexAuthenticationStatusResponse::ChatGpt { email } => {
            validate_acp_auth_status_detail("chat-gpt email", &email)?;
            AcpAuthDoctorStatus::Authenticated(AcpAuthenticatedKind::ChatGpt)
        }
        CodexAuthenticationStatusResponse::Gateway { name } => {
            validate_acp_auth_status_detail("gateway name", &name)?;
            AcpAuthDoctorStatus::Authenticated(AcpAuthenticatedKind::Gateway)
        }
    };
    if let Ok(mut observed) = observation.lock() {
        *observed = if status == AcpAuthDoctorStatus::Required {
            AcpObservedAuthState::Required
        } else {
            AcpObservedAuthState::Unchecked
        };
    }
    Ok(status)
}

fn validate_acp_auth_status_detail(label: &str, value: &str) -> psychevo_runtime::Result<()> {
    if value.chars().count() > ACP_AUTH_STATUS_DETAIL_MAX_CHARS
        || value.chars().any(char::is_control)
    {
        return Err(Error::Message(format!(
            "Codex ACP authentication/status returned an invalid {label}"
        )));
    }
    Ok(())
}

async fn close_resident_acp_sessions(
    cx: &ConnectionTo<Agent>,
    initialized: &InitializeResponse,
    contexts: &Arc<Mutex<BTreeMap<String, Arc<AcpClientContext>>>>,
    sessions: &AcpResidentSessions,
    terminals: &AcpTerminalRegistry,
) {
    let close_supported = initialized
        .agent_capabilities
        .session_capabilities
        .close
        .is_some();
    let native_session_ids = sessions
        .lock()
        .await
        .values()
        .map(|session| session.native_session_id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for native_session_id in &native_session_ids {
        let _ = cx.send_notification(CancelNotification::new(native_session_id.clone()));
        if close_supported {
            let close = cx
                .send_request(CloseSessionRequest::new(native_session_id.clone()))
                .block_task();
            let _ = tokio::time::timeout(Duration::from_secs(1), close).await;
        }
        let _ = terminals.terminate_session(native_session_id);
    }
    sessions.lock().await.clear();
    if let Ok(mut contexts) = contexts.lock() {
        contexts.clear();
    }
    let _ = terminals.terminate_all();
}

fn acp_process_unavailable_error(message: impl Into<String>) -> Error {
    crate::agent_session_error(
        "acp_process_unavailable",
        crate::AgentErrorStage::Delivery,
        "safe",
        "not_delivered",
        message,
        Some("acp-process".to_string()),
    )
}

fn acp_unknown_delivery_error(message: impl Into<String>) -> Error {
    crate::agent_session_error(
        "acp_unknown_delivery",
        crate::AgentErrorStage::Delivery,
        "unknown_delivery",
        "unknown",
        message,
        Some("acp-process".to_string()),
    )
}
