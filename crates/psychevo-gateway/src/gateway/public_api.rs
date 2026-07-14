impl Gateway {
    pub(crate) async fn discover_agent_sessions(
        &self,
        profile: RuntimeProfileConfig,
        peer: ResolvedPeerTurn,
        cwd: PathBuf,
        cursor: Option<String>,
    ) -> psychevo_runtime::Result<acp_peer::AcpSessionListPage> {
        self.agent_sessions
            .discover(
                CapturedAgentSessionTarget::invocation(
                    format!("session-discovery:{}", Uuid::now_v7()),
                    profile,
                    Some(peer),
                ),
                AgentSessionDiscoveryQuery {
                    cwd_filter: Some(cwd),
                    cursor,
                },
            )
            .await
    }

    pub(crate) async fn load_imported_agent_session(
        &self,
        profile: RuntimeProfileConfig,
        peer: ResolvedPeerTurn,
        options: RunOptions,
        local_session_id: String,
        native_session_id: String,
    ) -> psychevo_runtime::Result<acp_peer::AcpSessionLoadOutput> {
        let mcp_servers = acp_peer::resolve_peer_mcp_server_handoffs(&peer, &options)?;
        self.agent_sessions
            .attach(CapturedAgentSessionTarget::invocation(
                format!("session-import:{local_session_id}"),
                profile,
                Some(peer),
            ))?
            .transact(AgentSessionCommand::LoadSession(AgentSessionRef {
                cwd: options.cwd,
                local_session_id,
                native_session_id,
                mcp_servers,
            }))
            .await
            .and_then(AgentSessionResponse::into_loaded)
    }

    pub(crate) async fn release_imported_agent_session(
        &self,
        local_session_id: String,
        native_session_id: String,
    ) -> psychevo_runtime::Result<()> {
        self.agent_sessions
            .release_acp_session(local_session_id, native_session_id)
            .await
    }

    pub(crate) async fn resume_bound_agent_session(
        &self,
        binding: GatewayRuntimeBindingRecord,
        profile: RuntimeProfileConfig,
        peer: ResolvedPeerTurn,
        options: RunOptions,
    ) -> psychevo_runtime::Result<acp_peer::AcpSessionSnapshot> {
        let native_session_id = binding.native_session_id.clone().ok_or_else(|| {
            agent_session_configuration_error(format!(
                "Agent binding for thread `{}` has no native session id.",
                binding.thread_id
            ))
        })?;
        let mcp_servers = acp_peer::resolve_peer_mcp_server_handoffs(&peer, &options)?;
        self.agent_sessions
            .attach(CapturedAgentSessionTarget::bound(
                &binding,
                profile,
                Some(peer),
            )?)?
            .transact(AgentSessionCommand::ResumeSession(AgentSessionRef {
                cwd: options.cwd,
                local_session_id: binding.thread_id,
                native_session_id,
                mcp_servers,
            }))
            .await
            .and_then(AgentSessionResponse::into_resumed)?
            .into_acp()
    }

    pub(crate) async fn fork_bound_agent_session(
        &self,
        binding: GatewayRuntimeBindingRecord,
        profile: RuntimeProfileConfig,
        peer: ResolvedPeerTurn,
        options: RunOptions,
        fork_local_session_id: String,
    ) -> psychevo_runtime::Result<acp_peer::AcpSessionSnapshot> {
        let native_session_id = binding.native_session_id.clone().ok_or_else(|| {
            agent_session_configuration_error(format!(
                "Agent binding for thread `{}` has no native session id.",
                binding.thread_id
            ))
        })?;
        let mcp_servers = acp_peer::resolve_peer_mcp_server_handoffs(&peer, &options)?;
        self.agent_sessions
            .attach(CapturedAgentSessionTarget::bound(
                &binding,
                profile,
                Some(peer),
            )?)?
            .transact(AgentSessionCommand::ForkSession {
                source: AgentSessionRef {
                    cwd: options.cwd,
                    local_session_id: binding.thread_id,
                    native_session_id,
                    mcp_servers,
                },
                fork_local_session_id,
            })
            .await
            .and_then(AgentSessionResponse::into_forked)?
            .into_acp()
    }

    pub(crate) async fn close_bound_agent_session(
        &self,
        binding: GatewayRuntimeBindingRecord,
        profile: RuntimeProfileConfig,
        peer: ResolvedPeerTurn,
        options: RunOptions,
    ) -> psychevo_runtime::Result<()> {
        let native_session_id = binding.native_session_id.clone().ok_or_else(|| {
            agent_session_configuration_error(format!(
                "Agent binding for thread `{}` has no native session id.",
                binding.thread_id
            ))
        })?;
        self.agent_sessions
            .attach(CapturedAgentSessionTarget::bound(
                &binding,
                profile,
                Some(peer),
            )?)?
            .transact(AgentSessionCommand::CloseSession(AgentSessionRef {
                cwd: options.cwd,
                local_session_id: binding.thread_id,
                native_session_id,
                mcp_servers: Vec::new(),
            }))
            .await
            .and_then(AgentSessionResponse::into_closed)
    }

    pub(crate) async fn delete_bound_agent_session(
        &self,
        binding: GatewayRuntimeBindingRecord,
        profile: RuntimeProfileConfig,
        peer: ResolvedPeerTurn,
        options: RunOptions,
    ) -> psychevo_runtime::Result<()> {
        let native_session_id = binding.native_session_id.clone().ok_or_else(|| {
            agent_session_configuration_error(format!(
                "Agent binding for thread `{}` has no native session id.",
                binding.thread_id
            ))
        })?;
        self.agent_sessions
            .attach(CapturedAgentSessionTarget::bound(
                &binding,
                profile,
                Some(peer),
            )?)?
            .transact(AgentSessionCommand::DeleteSession(AgentSessionRef {
                cwd: options.cwd,
                local_session_id: binding.thread_id,
                native_session_id,
                mcp_servers: Vec::new(),
            }))
            .await
            .and_then(AgentSessionResponse::into_deleted)
    }

    pub fn new(state: StateRuntime) -> Self {
        Self::with_backend(state, Arc::new(PsychevoRuntimeBackend))
    }

    pub fn with_backend(state: StateRuntime, backend: Arc<dyn GatewayBackend>) -> Self {
        Self {
            state,
            agent_sessions: AgentSessionHost::new(backend),
            active: Arc::new(Mutex::new(HashMap::new())),
            active_aliases: Arc::new(Mutex::new(HashMap::new())),
            process_bindings: Arc::new(Mutex::new(HashMap::new())),
            source_generations: Arc::new(Mutex::new(HashMap::new())),
            live_snapshots: Arc::new(Mutex::new(HashMap::new())),
            pending_permissions: Arc::new(Mutex::new(HashMap::new())),
            owner_id: Arc::new(format!("gateway:{}:{}", std::process::id(), Uuid::now_v7())),
        }
    }

    pub fn state(&self) -> &StateRuntime {
        &self.state
    }

    pub async fn shutdown_runtimes(&self, force: bool) -> psychevo_runtime::Result<()> {
        self.agent_sessions.shutdown(force).await
    }

    pub(crate) async fn inspect_cached_bound_agent_session(
        &self,
        local_session_id: String,
        native_session_id: String,
    ) -> psychevo_runtime::Result<Option<acp_peer::AcpSessionSnapshot>> {
        self.agent_sessions
            .inspect_cached_acp_session(local_session_id, native_session_id)
            .await
    }

    pub(crate) async fn prepare_agent_session(
        &self,
        peer: ResolvedPeerTurn,
        options: RunOptions,
        source_key: String,
        target_id: String,
        agent_ref: Option<String>,
    ) -> psychevo_runtime::Result<acp_peer::AcpSessionSnapshot> {
        let mcp_servers = acp_peer::resolve_peer_mcp_server_handoffs(&peer, &options)?;
        let (profile, _, _) = resolve_gateway_runtime_profile(&options)?;
        self.agent_sessions
            .prepare(
                CapturedAgentSessionTarget::invocation(
                    format!("draft:{source_key}"),
                    profile,
                    Some(peer),
                ),
                source_key,
                target_id,
                agent_ref,
                options.cwd,
                mcp_servers,
            )
            .await
    }

    pub(crate) async fn inspect_prepared_agent_session(
        &self,
        source_key: &str,
        target_id: &str,
    ) -> psychevo_runtime::Result<Option<acp_peer::AcpSessionSnapshot>> {
        self.agent_sessions
            .inspect_prepared(source_key, target_id)
            .await
    }

    pub(crate) async fn set_prepared_agent_session_control(
        &self,
        source_key: &str,
        target_id: &str,
        control_id: String,
        value: Value,
    ) -> psychevo_runtime::Result<Option<acp_peer::AcpSessionSnapshot>> {
        self.agent_sessions
            .set_prepared_control(source_key, target_id, control_id, value)
            .await
    }

    pub(crate) async fn release_prepared_agent_session(
        &self,
        source_key: &str,
    ) -> psychevo_runtime::Result<bool> {
        self.agent_sessions.release_prepared(source_key).await
    }

    pub(crate) async fn set_bound_agent_session_control(
        &self,
        peer: ResolvedPeerTurn,
        options: RunOptions,
        local_session_id: String,
        native_session_id: String,
        control_id: String,
        value: Value,
    ) -> psychevo_runtime::Result<acp_peer::AcpSessionSnapshot> {
        let mcp_servers = acp_peer::resolve_peer_mcp_server_handoffs(&peer, &options)?;
        let (profile, _, _) = resolve_gateway_runtime_profile(&options)?;
        let binding = self
            .state
            .store()
            .gateway_runtime_binding(&local_session_id)?
            .ok_or_else(|| {
                agent_session_configuration_error(format!(
                    "Agent binding not found for thread `{local_session_id}`."
                ))
            })?;
        if binding.native_session_id.as_deref() != Some(native_session_id.as_str()) {
            return Err(agent_session_configuration_error(format!(
                "Agent binding for thread `{local_session_id}` does not own native session `{native_session_id}`."
            )));
        }
        self.agent_sessions
            .attach(CapturedAgentSessionTarget::bound(
                &binding,
                profile,
                Some(peer),
            )?)?
            .transact(AgentSessionCommand::SetControl {
                session: AgentSessionRef {
                    cwd: options.cwd,
                    local_session_id,
                    native_session_id,
                    mcp_servers,
                },
                control_id,
                value,
            })
            .await
            .and_then(AgentSessionResponse::into_control)?
            .into_acp()
    }

    pub(crate) async fn probe_acp_backend_authentication(
        &self,
        peer: ResolvedPeerTurn,
        cwd: PathBuf,
    ) -> psychevo_runtime::Result<acp_peer::AcpAuthDoctorStatus> {
        self.agent_sessions
            .probe_acp_authentication(peer, cwd)
            .await
    }

    pub(crate) async fn probe_acp_backend_protocol_compatibility(
        &self,
        peer: ResolvedPeerTurn,
        cwd: PathBuf,
    ) -> psychevo_runtime::Result<acp_peer::AcpProtocolDoctorStatus> {
        self.agent_sessions
            .probe_acp_protocol_compatibility(peer, cwd)
            .await
    }

    pub(crate) async fn run_internal_agent_turn(
        &self,
        binding: Option<GatewayRuntimeBindingRecord>,
        profile: RuntimeProfileConfig,
        peer: Option<ResolvedPeerTurn>,
        request: BackendTurnRequest,
        turn_id: String,
        session_ready: Option<acp_peer::AcpSessionReadyCallback>,
    ) -> psychevo_runtime::Result<RunResult> {
        let target = match binding.as_ref() {
            Some(binding) => CapturedAgentSessionTarget::bound(binding, profile, peer)?,
            None => CapturedAgentSessionTarget::invocation(turn_id.clone(), profile, peer),
        };
        self.agent_sessions
            .attach(target)?
            .transact(AgentSessionCommand::SubmitTurn {
                request: Box::new(request),
                turn_id,
                session_ready,
            })
            .await
            .and_then(AgentSessionResponse::into_turn)
            .map(|output| output.run)
    }

    pub fn owner_id(&self) -> &str {
        self.owner_id.as_str()
    }

    pub fn resolve_source_thread(
        &self,
        source: &GatewaySource,
    ) -> psychevo_runtime::Result<Option<String>> {
        self.lookup_source_thread(source)
    }

    pub fn thread_transcript(
        &self,
        thread_id: &str,
    ) -> psychevo_runtime::Result<Vec<TranscriptEntry>> {
        let summaries = self.state.store().load_tui_message_summaries(thread_id)?;
        let mut entries = transcript::project_transcript_entries(thread_id, &summaries);
        let agent_edges = self.state.store().list_agent_edges_for_parent(thread_id)?;
        transcript::enrich_agent_blocks_from_edges(&mut entries, &agent_edges);
        let compactions = self
            .state
            .store()
            .list_valid_session_compactions(thread_id)?;
        let mut synthetic_entries = compactions
            .iter()
            .zip(transcript::project_compaction_entries(
                thread_id,
                &compactions,
            ))
            .map(|(record, entry)| (record.created_after_session_seq, entry))
            .collect::<Vec<_>>();
        let terminals = self
            .state
            .store()
            .list_gateway_turn_terminals_for_thread(thread_id)?;
        transcript::reconcile_terminal_bounded_running_blocks(&mut entries, &terminals);
        synthetic_entries.extend(terminals.iter().filter_map(|terminal| {
            transcript::project_turn_terminal_entry(terminal)
                .map(|entry| (transcript::terminal_structural_boundary(terminal), entry))
        }));
        Ok(transcript::merge_entries_at_session_boundaries(
            entries,
            synthetic_entries,
        ))
    }

    pub fn activity_for_selector(&self, selector: GatewayThreadSelector) -> GatewayActivity {
        let selector_keys = self.selector_keys(&selector);
        let active = self.active.lock().expect("gateway active map poisoned");
        let aliases = self
            .active_aliases
            .lock()
            .expect("gateway active alias map poisoned");
        let mut activity = GatewayActivity::default();
        let mut seen = HashSet::new();
        for key in selector_keys {
            let key = aliases.get(&key).cloned().unwrap_or(key);
            if !seen.insert(key.clone()) {
                continue;
            }
            if let Some(state) = active.get(&key) {
                activity.running |= state.running;
                if activity.active_turn_id.is_none() {
                    activity.active_turn_id = state.active_turn_id.clone();
                }
                activity.queued_turns += state.queued.len();
            }
            if let Ok(Some(record)) = self.durable_activity_for_key(&key) {
                self.merge_durable_activity(&mut activity, record);
            }
        }
        activity
    }

    pub fn session_activity_snapshot(
        &self,
    ) -> psychevo_runtime::Result<BTreeMap<String, GatewayActivity>> {
        let active = self.active.lock().expect("gateway active map poisoned");
        let aliases = self
            .active_aliases
            .lock()
            .expect("gateway active alias map poisoned");
        let mut snapshot = BTreeMap::new();
        for (key, state) in active.iter() {
            if let Some(thread_id) = key.strip_prefix("thread:") {
                merge_in_memory_activity(snapshot.entry(thread_id.to_string()).or_default(), state);
            }
        }
        for (alias, primary) in aliases.iter() {
            let Some(thread_id) = alias.strip_prefix("thread:") else {
                continue;
            };
            let Some(state) = active.get(primary) else {
                continue;
            };
            merge_in_memory_activity(snapshot.entry(thread_id.to_string()).or_default(), state);
        }
        drop(aliases);
        drop(active);
        for record in self.state.store().active_gateway_activities()? {
            let Some(thread_id) = record.thread_id.clone() else {
                continue;
            };
            self.merge_durable_activity(snapshot.entry(thread_id).or_default(), record);
        }
        Ok(snapshot)
    }

    fn durable_activity_for_key(
        &self,
        key: &str,
    ) -> psychevo_runtime::Result<Option<GatewayActivityRecord>> {
        if let Some(thread_id) = key.strip_prefix("thread:") {
            return self
                .state
                .store()
                .active_gateway_activity_for_thread(thread_id);
        }
        if let Some(source_key) = key.strip_prefix("source:") {
            return self
                .state
                .store()
                .active_gateway_activity_for_source(source_key);
        }
        Ok(None)
    }

    fn merge_durable_activity(
        &self,
        activity: &mut GatewayActivity,
        record: GatewayActivityRecord,
    ) {
        let stale = record.status == "running" && record.lease_expires_at_ms < gateway_now_ms();
        if matches!(record.status.as_str(), "running" | "queued") && !stale {
            activity.running = true;
        }
        if stale && activity.takeover_state.is_none() {
            activity.takeover_state = Some("stale".to_string());
        }
        if activity.active_turn_id.is_none() {
            activity.active_turn_id = record.turn_id.clone();
        }
        if record.owner_id == self.owner_id() {
            activity.queued_turns = activity.queued_turns.max(record.queued_turns);
        } else {
            activity.queued_turns += record.queued_turns;
        }
        activity.started_at_ms = match (activity.started_at_ms, Some(record.started_at_ms)) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (None, value) => value,
            (value, None) => value,
        };
        activity.updated_at_ms = match (activity.updated_at_ms, Some(record.updated_at_ms)) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (None, value) => value,
            (value, None) => value,
        };
        if activity.owner_id.is_none() {
            activity.owner_id = Some(record.owner_id);
            activity.owner_surface = record.owner_surface;
            activity.lease_expires_at_ms = Some(record.lease_expires_at_ms);
        }
    }

    /// Executes one caller turn through the complete Thread Application policy
    /// boundary. Surface Adapters provide typed intent; Gateway owns runtime
    /// lowering, queueing, binding, and Agent Adapter selection.
    pub async fn run_turn(
        &self,
        mut request: ThreadTurnRequest,
    ) -> psychevo_runtime::Result<GatewayTurnResult> {
        let turn_id = request
            .turn_id
            .take()
            .unwrap_or_else(|| Uuid::now_v7().to_string());
        let request = request.into_queue_request(self.state.clone());
        self.send_turn_with_id(request, turn_id).await
    }

    #[cfg(test)]
    pub(crate) async fn send_turn(
        &self,
        request: SendTurnRequest,
    ) -> psychevo_runtime::Result<GatewayTurnResult> {
        let turn_id = Uuid::now_v7().to_string();
        self.send_turn_with_id(request, turn_id).await
    }

    pub(crate) async fn send_turn_with_id(
        &self,
        request: SendTurnRequest,
        turn_id: String,
    ) -> psychevo_runtime::Result<GatewayTurnResult> {
        let queue_key = self.queue_key_for_request(&request)?;
        let mut request = Some(request);
        let queued = {
            let mut active = self.active.lock().expect("gateway active map poisoned");
            let state = active.entry(queue_key.clone()).or_default();
            if state.running {
                let (responder, receiver) = oneshot::channel();
                let queue_position = state.queued.len() + 1;
                let queued_request = request.take().expect("gateway request missing");
                let event_sink = queued_request.event_sink.clone();
                let thread_id = queued_request.thread_id.clone();
                let active_activity_id = state.active_turn_id.clone();
                state
                    .queued
                    .push_back(PendingQueuedActivity::Turn(Box::new(PendingQueuedTurn {
                        turn_id: turn_id.clone(),
                        request: queued_request,
                        responder,
                    })));
                Some((
                    receiver,
                    event_sink,
                    thread_id,
                    queue_position,
                    active_activity_id,
                ))
            } else {
                state.running = true;
                None
            }
        };

        if let Some((receiver, event_sink, thread_id, queue_position, active_activity_id)) = queued
        {
            if let Some(active_activity_id) = active_activity_id {
                let _ = self
                    .state
                    .store()
                    .set_gateway_activity_queued_turns(&active_activity_id, queue_position);
            }
            if let Some(event_sink) = event_sink {
                event_sink(GatewayEvent::TurnQueued {
                    thread_id,
                    turn_id,
                    queue_position,
                });
            }
            return receiver
                .await
                .map_err(|_| Error::Message("gateway turn queue closed".to_string()))?;
        }

        let result = self
            .run_turn_now(
                &queue_key,
                request.take().expect("gateway request missing"),
                turn_id,
            )
            .await;
        self.finish_activity_and_spawn_next(queue_key);
        result
    }

    pub async fn send_shell(
        &self,
        request: SendShellRequest,
    ) -> psychevo_runtime::Result<GatewayShellResult> {
        let queue_key = self.queue_key_for_shell_request(&request)?;
        let shell_id = Uuid::now_v7().to_string();
        let mut request = Some(request);
        let active = {
            let mut active = self.active.lock().expect("gateway active map poisoned");
            let state = active.entry(queue_key.clone()).or_default();
            if state.running {
                if state.active_kind == Some(ActiveActivityKind::Turn)
                    && let Some(control) = state.control.clone()
                {
                    ShellStartState::Auxiliary(control)
                } else {
                    let (responder, receiver) = oneshot::channel();
                    let queue_position = state.queued.len() + 1;
                    let active_activity_id = state.active_turn_id.clone();
                    state
                        .queued
                        .push_back(PendingQueuedActivity::Shell(Box::new(PendingQueuedShell {
                            shell_id: shell_id.clone(),
                            request: request.take().expect("gateway shell request missing"),
                            responder,
                        })));
                    if let Some(active_activity_id) = active_activity_id {
                        let _ = self
                            .state
                            .store()
                            .set_gateway_activity_queued_turns(&active_activity_id, queue_position);
                    }
                    ShellStartState::Queued(receiver)
                }
            } else {
                state.running = true;
                ShellStartState::Standalone
            }
        };

        match active {
            ShellStartState::Queued(receiver) => receiver
                .await
                .map_err(|_| Error::Message("gateway shell queue closed".to_string()))?,
            ShellStartState::Auxiliary(inject_into) => {
                self.run_shell_auxiliary(
                    request.take().expect("gateway shell request missing"),
                    shell_id,
                    inject_into,
                )
                .await
            }
            ShellStartState::Standalone => {
                let result = self
                    .run_shell_now(
                        &queue_key,
                        request.take().expect("gateway shell request missing"),
                        shell_id,
                    )
                    .await;
                self.finish_activity_and_spawn_next(queue_key);
                result
            }
        }
    }

    pub fn enqueue_compact_session(
        &self,
        request: SendCompactRequest,
    ) -> psychevo_runtime::Result<
        BoxFuture<'static, psychevo_runtime::Result<psychevo_runtime::CompactionResult>>,
    > {
        let queue_key = self.queue_key_for_compact_request(&request)?;
        let compact_id = Uuid::now_v7().to_string();
        let event_sink = request.event_sink.clone();
        let event_thread_id = self.compact_event_thread_id(&request);
        let (responder, receiver) = oneshot::channel();
        let mut pending = Some(Box::new(PendingQueuedCompact {
            compact_id,
            request,
            responder,
        }));
        let queued = {
            let mut active = self.active.lock().expect("gateway active map poisoned");
            let state = active.entry(queue_key.clone()).or_default();
            if state.running {
                state.queued.push_back(PendingQueuedActivity::Compact(
                    pending.take().expect("gateway compact request missing"),
                ));
                true
            } else {
                state.running = true;
                false
            }
        };

        if queued {
            self.emit_activity_changed_for_thread(event_sink, event_thread_id);
        } else {
            self.spawn_compact_activity(
                queue_key,
                pending.take().expect("gateway compact request missing"),
            );
        }

        Ok(Box::pin(async move {
            receiver
                .await
                .map_err(|_| Error::Message("gateway compact queue closed".to_string()))?
        }))
    }

    pub async fn compact_session(
        &self,
        request: SendCompactRequest,
    ) -> psychevo_runtime::Result<psychevo_runtime::CompactionResult> {
        self.enqueue_compact_session(request)?.await
    }

    pub fn steer_turn(
        &self,
        selector: GatewayThreadSelector,
        expected_turn_id: Option<&str>,
        message: psychevo_runtime::Message,
    ) -> Option<psychevo_runtime::PendingInputId> {
        if self.expected_turn_is_terminal(expected_turn_id) {
            return None;
        }
        if !self.agent_supports_steer_for_selector(&selector) {
            return None;
        }
        self.control_for_selector(&selector, expected_turn_id)
            .and_then(|control| control.steer_user_message(message))
    }

    fn agent_supports_steer_for_selector(&self, selector: &GatewayThreadSelector) -> bool {
        let thread_id = match selector {
            GatewayThreadSelector::ThreadId { thread_id } => Some(thread_id.clone()),
            GatewayThreadSelector::Source { source_key } => {
                match self.state.store().gateway_source_lane(&source_key.0) {
                    Ok(lane) => lane.and_then(|lane| lane.thread_id),
                    Err(_) => return false,
                }
            }
        };
        let Some(thread_id) = thread_id else {
            return true;
        };
        match self.state.store().gateway_runtime_binding(&thread_id) {
            Ok(Some(binding)) => binding.backend_kind.as_deref() == Some("native"),
            Ok(None) => true,
            Err(_) => false,
        }
    }

    pub fn steer_foreign_turn(
        &self,
        selector: GatewayThreadSelector,
        expected_turn_id: Option<&str>,
        message: psychevo_runtime::Message,
    ) -> bool {
        if self.expected_turn_is_terminal(expected_turn_id) {
            return false;
        }
        if !self.agent_supports_steer_for_selector(&selector) {
            return false;
        }
        let Ok(message) = serde_json::to_value(message) else {
            return false;
        };
        self.enqueue_foreign_control_command(
            &selector,
            "steer",
            json!({
                "expectedTurnId": expected_turn_id,
                "message": message,
            }),
        )
    }

    fn expected_turn_is_terminal(&self, expected_turn_id: Option<&str>) -> bool {
        expected_turn_id.is_some_and(|turn_id| {
            self.state
                .store()
                .gateway_turn_terminal(turn_id)
                .map(|terminal| terminal.is_some())
                .unwrap_or(true)
        })
    }

    pub fn cancel_steer(
        &self,
        selector: GatewayThreadSelector,
        expected_turn_id: Option<&str>,
        input_id: psychevo_runtime::PendingInputId,
    ) -> bool {
        self.control_for_selector(&selector, expected_turn_id)
            .is_some_and(|control| control.cancel_pending_user_message(input_id))
    }

    pub fn update_steer(
        &self,
        selector: GatewayThreadSelector,
        expected_turn_id: Option<&str>,
        input_id: psychevo_runtime::PendingInputId,
        message: psychevo_runtime::Message,
    ) -> bool {
        self.control_for_selector(&selector, expected_turn_id)
            .is_some_and(|control| control.update_pending_user_message(input_id, message))
    }

    pub fn interrupt_turn(&self, selector: GatewayThreadSelector) -> bool {
        if let Some(control) = self.control_for_selector(&selector, None) {
            control.abort();
            true
        } else {
            self.enqueue_foreign_control_command(&selector, "interrupt", json!({}))
        }
    }

    pub fn submit_clarify(
        &self,
        selector: GatewayThreadSelector,
        call_id: &str,
        result: ClarifyResult,
    ) -> bool {
        if self
            .control_for_selector(&selector, None)
            .is_some_and(|control| control.submit_clarify_result(call_id, result.clone()))
        {
            return true;
        }
        let payload = match result {
            ClarifyResult::Answered(response) => json!({
                "requestId": call_id,
                "answers": response
                    .answers
                    .into_iter()
                    .map(|answer| answer.answers)
                    .collect::<Vec<_>>(),
            }),
            ClarifyResult::Cancelled => json!({
                "requestId": call_id,
                "cancel": true,
            }),
        };
        self.enqueue_foreign_control_command(&selector, "clarify", payload)
    }

    pub fn submit_permission(
        &self,
        selector: GatewayThreadSelector,
        request_id: &str,
        decision: PermissionApprovalDecision,
    ) -> bool {
        let selector_keys = self.selector_keys_with_active_aliases(&selector);
        let pending = {
            let mut permissions = self
                .pending_permissions
                .lock()
                .expect("gateway pending permission map poisoned");
            match permissions.get(request_id) {
                Some(pending)
                    if pending.selector_key.as_deref().is_some_and(|pending_key| {
                        !selector_keys.iter().any(|key| key == pending_key)
                    }) =>
                {
                    return false;
                }
                Some(_) => permissions.remove(request_id),
                None => None,
            }
        };
        if pending
            .and_then(|pending| pending.responder.send(decision.clone()).ok())
            .is_some()
        {
            return true;
        }
        self.enqueue_foreign_control_command(
            &selector,
            "permission",
            json!({
                "requestId": request_id,
                "decision": permission_decision_label(&decision),
            }),
        )
    }

    pub(crate) fn has_pending_permission_for_selector(
        &self,
        selector: &GatewayThreadSelector,
        request_id: &str,
    ) -> bool {
        let selector_keys = self.selector_keys_with_active_aliases(selector);
        self.pending_permissions
            .lock()
            .expect("gateway pending permission map poisoned")
            .get(request_id)
            .is_some_and(|pending| {
                pending
                    .selector_key
                    .as_deref()
                    .is_none_or(|pending_key| selector_keys.iter().any(|key| key == pending_key))
            })
    }

    pub fn clear_queue(&self, selector: GatewayThreadSelector) -> usize {
        let selector_keys = self.selector_keys(&selector);
        let mut dropped = Vec::new();
        {
            let mut active = self.active.lock().expect("gateway active map poisoned");
            let aliases = self
                .active_aliases
                .lock()
                .expect("gateway active alias map poisoned");
            let mut seen = HashSet::new();
            for key in selector_keys {
                let key = aliases.get(&key).cloned().unwrap_or(key);
                if !seen.insert(key.clone()) {
                    continue;
                }
                if let Some(state) = active.get_mut(&key) {
                    dropped.extend(state.queued.drain(..));
                }
            }
        }
        let count = dropped.len();
        for pending in dropped {
            match pending {
                PendingQueuedActivity::Turn(pending) => {
                    let _ = pending.responder.send(Err(Error::Message(
                        "gateway turn queue cleared".to_string(),
                    )));
                }
                PendingQueuedActivity::Shell(pending) => {
                    let _ = pending.responder.send(Err(Error::Message(
                        "gateway shell queue cleared".to_string(),
                    )));
                }
                PendingQueuedActivity::Compact(pending) => {
                    let _ = pending.responder.send(Err(Error::Message(
                        "gateway compact queue cleared".to_string(),
                    )));
                }
            }
        }
        count
    }
}

fn merge_in_memory_activity(activity: &mut GatewayActivity, state: &ActiveThreadState) {
    activity.running |= state.running;
    if activity.active_turn_id.is_none() {
        activity.active_turn_id = state.active_turn_id.clone();
    }
    activity.queued_turns = activity.queued_turns.max(state.queued.len());
}
