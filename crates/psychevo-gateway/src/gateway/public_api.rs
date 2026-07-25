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
            .load_session(AgentSessionRef {
                cwd: options.cwd,
                local_session_id,
                native_session_id,
                mcp_servers,
            })
            .await
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
            .resume_session(AgentSessionRef {
                cwd: options.cwd,
                local_session_id: binding.thread_id,
                native_session_id,
                mcp_servers,
            })
            .await?
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
            .fork_session(
                AgentSessionRef {
                    cwd: options.cwd,
                    local_session_id: binding.thread_id,
                    native_session_id,
                    mcp_servers,
                },
                fork_local_session_id,
            )
            .await?
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
            .close_session(AgentSessionRef {
                cwd: options.cwd,
                local_session_id: binding.thread_id,
                native_session_id,
                mcp_servers: Vec::new(),
            })
            .await
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
            .delete_session(AgentSessionRef {
                cwd: options.cwd,
                local_session_id: binding.thread_id,
                native_session_id,
                mcp_servers: Vec::new(),
            })
            .await
    }

    pub(crate) async fn lock_source_mutation(
        &self,
        source_key: &SourceKey,
    ) -> OwnedMutexGuard<()> {
        let lock = self
            .source_mutations
            .lock()
            .expect("gateway source mutation map poisoned")
            .entry(source_key.0.clone())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone();
        lock.lock_owned().await
    }

    pub fn new(state: StateRuntime) -> Self {
        Self::with_backend(state, Arc::new(PsychevoRuntimeBackend))
    }

    pub fn with_backend(state: StateRuntime, backend: Arc<dyn GatewayBackend>) -> Self {
        let supervisor = GatewaySupervisor::default();
        Self {
            state,
            agent_sessions: AgentSessionHost::new(backend),
            event_ingress: GatewayEventIngress::new(supervisor.clone()),
            supervisor,
            active: Arc::new(Mutex::new(HashMap::new())),
            active_aliases: Arc::new(Mutex::new(HashMap::new())),
            process_bindings: Arc::new(Mutex::new(HashMap::new())),
            source_generations: Arc::new(Mutex::new(HashMap::new())),
            source_mutations: Arc::new(Mutex::new(HashMap::new())),
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

    pub(crate) async fn shutdown_application(
        &self,
        force: bool,
    ) -> psychevo_runtime::Result<()> {
        self.supervisor.close_turn_admission();
        self.supervisor.stop_producers();
        self.supervisor.wait_for_producers().await;
        if force {
            self.supervisor.force_cancel_turns();
            self.cancel_active_queue();
        } else {
            self.supervisor.close_turns();
        }
        self.supervisor.wait_for_turns().await;
        self.event_ingress.close();
        if force {
            self.supervisor.force_cancel_infrastructure();
        } else {
            self.supervisor.close_infrastructure();
        }
        self.supervisor.wait_for_infrastructure().await;
        self.agent_sessions.shutdown(force).await
    }

    #[cfg(test)]
    pub(crate) async fn wait_for_gateway_events(&self) {
        self.event_ingress.wait_until_drained().await;
    }

    pub(crate) fn spawn_background<F>(&self, name: impl Into<Arc<str>>, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.supervisor.spawn_producer(name, future);
    }

    pub(crate) fn spawn_accepted_turn<F>(&self, name: impl Into<Arc<str>>, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.supervisor.spawn_turn(name, future);
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
        let (profile, _, _) = resolve_gateway_runtime_profile(&options).await?;
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
        let (profile, _, _) = resolve_gateway_runtime_profile(&options).await?;
        let binding = self
            .state

            .gateway_runtime_binding(&local_session_id)
            .await?
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
            .set_control(
                AgentSessionRef {
                    cwd: options.cwd,
                    local_session_id,
                    native_session_id,
                    mcp_servers,
                },
                control_id,
                value,
            )
            .await?
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
            .run_turn(request, turn_id, session_ready)
            .await
            .map(|output| output.run)
    }

    pub fn owner_id(&self) -> &str {
        self.owner_id.as_str()
    }

    pub async fn resolve_source_thread(
        &self,
        source: &GatewaySource,
    ) -> psychevo_runtime::Result<Option<String>> {
        self.lookup_source_thread(source).await
    }

    pub async fn thread_transcript(
        &self,
        thread_id: &str,
    ) -> psychevo_runtime::Result<Vec<TranscriptEntry>> {
        let summaries = self.state.load_tui_message_summaries(thread_id).await?;
        let mut entries = transcript::project_transcript_entries(thread_id, &summaries);
        let agent_edges = self.state.list_agent_edges_for_parent(thread_id).await?;
        transcript::enrich_agent_blocks_from_edges(&mut entries, &agent_edges);
        let compactions = self
            .state

            .list_valid_session_compactions(thread_id)
            .await?;
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

            .list_gateway_turn_terminals_for_thread(thread_id)
            .await?;
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

    pub fn local_activity_for_selector(
        &self,
        selector: &GatewayThreadSelector,
    ) -> GatewayActivity {
        let selector_keys = self.selector_keys(selector);
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
                activity.running |= state.running && state.active_turn_id.is_some();
                if activity.active_turn_id.is_none() {
                    activity.active_turn_id = state.active_turn_id.clone();
                }
                activity.queued_turns += state.queued.len();
            }
        }
        activity
    }

    pub async fn activity_for_selector(&self, selector: GatewayThreadSelector) -> GatewayActivity {
        let selector_keys = self.selector_keys(&selector);
        let mut activity = self.local_activity_for_selector(&selector);
        let mut durable_keys = Vec::new();
        let mut seen = HashSet::new();
        for key in selector_keys {
            let key = self
                .active_aliases
                .lock()
                .expect("gateway active alias map poisoned")
                .get(&key)
                .cloned()
                .unwrap_or(key);
            if seen.insert(key.clone()) {
                durable_keys.push(key);
            }
        }
        for key in durable_keys {
            if let Ok(Some(record)) = self.durable_activity_for_key(&key).await {
                self.merge_durable_activity(&mut activity, record);
            }
        }
        activity
    }

    pub async fn session_activity_snapshot(
        &self,
    ) -> psychevo_runtime::Result<BTreeMap<String, GatewayActivity>> {
        let mut snapshot = {
            let active = self.active.lock().expect("gateway active map poisoned");
            let aliases = self
                .active_aliases
                .lock()
                .expect("gateway active alias map poisoned");
            let mut snapshot = BTreeMap::new();
            for (key, state) in active.iter() {
                if let Some(thread_id) = key.strip_prefix("thread:") {
                    merge_in_memory_activity(
                        snapshot.entry(thread_id.to_string()).or_default(),
                        state,
                    );
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
            snapshot
        };
        for record in self.state.active_gateway_activities().await? {
            let Some(thread_id) = record.thread_id.clone() else {
                continue;
            };
            self.merge_durable_activity(snapshot.entry(thread_id).or_default(), record);
        }
        Ok(snapshot)
    }

    async fn durable_activity_for_key(
        &self,
        key: &str,
    ) -> psychevo_runtime::Result<Option<GatewayActivityRecord>> {
        if let Some(thread_id) = key.strip_prefix("thread:") {
            return self
                .state

                .active_gateway_activity_for_thread(thread_id)
                .await;
        }
        if let Some(source_key) = key.strip_prefix("source:") {
            return self
                .state

                .active_gateway_activity_for_source(source_key)
                .await;
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

    /// Accepts one caller turn through the complete Thread Application policy
    /// boundary. Gateway supervision owns the accepted work independently of
    /// the returned completion handle.
    pub async fn start_turn(
        &self,
        mut caller: ThreadCallerContext,
        mut intent: ThreadTurnIntent,
    ) -> psychevo_runtime::Result<AcceptedTurn> {
        let admission = self
            .supervisor
            .acquire_activity_admission()
            .map_err(|error| Error::Message(error.to_string()))?;
        let turn_id = intent
            .turn_id
            .take()
            .unwrap_or_else(|| Uuid::now_v7().to_string());
        let client_turn_id = intent
            .client_turn_id
            .take()
            .filter(|value| !value.trim().is_empty());
        let explicit_thread = intent
            .thread_id
            .as_deref()
            .is_some_and(|thread_id| !thread_id.trim().is_empty());
        let thread_id = self
            .materialize_accepted_thread(&caller, &intent)
            .await?;
        intent.thread_id = Some(thread_id.clone());
        intent.policy.continue_latest = false;
        gateway_profile_mark(
            "gateway_start_turn_entered",
            Some(&turn_id),
            Some(&thread_id),
            GatewayProfileFields {
                runtime_source: Some(&caller.runtime_source),
                ..GatewayProfileFields::default()
            },
        );
        if let Some(client_turn_id) = client_turn_id.as_deref() {
            self.state
                .record_gateway_turn_start_receipt(&thread_id, client_turn_id, &turn_id)
                .await?;
        }
        if caller.continue_sources.is_empty() {
            caller.continue_sources.push(caller.runtime_source.clone());
        }
        let request = intent.into_queue_request(caller, self.state.clone(), explicit_thread);
        let (completion_tx, completion_rx) = oneshot::channel();
        let gateway = self.clone();
        let supervised_turn_id = turn_id.clone();
        self.supervisor.spawn_permitted_activity(
            format!("turn:{turn_id}"),
            admission,
            async move {
                let result = gateway
                    .send_turn_with_id(request, supervised_turn_id)
                    .await;
                let _ = completion_tx.send(result);
            },
        );
        Ok(AcceptedTurn {
            receipt: AcceptedTurnReceipt {
                accepted: true,
                thread_id,
                turn_id,
                client_turn_id,
            },
            completion: AcceptedTurnCompletion {
                receiver: completion_rx,
            },
        })
    }

    async fn materialize_accepted_thread(
        &self,
        caller: &ThreadCallerContext,
        intent: &ThreadTurnIntent,
    ) -> psychevo_runtime::Result<String> {
        if let Some(thread_id) = intent
            .thread_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            self.thread_cwd(thread_id).await?;
            return Ok(thread_id.to_string());
        }
        if !intent.reset_source_binding && let Some(source) = intent.source.as_ref() {
            if let Some(thread_id) = self.active_thread_for_source(source) {
                return Ok(thread_id);
            }
            if let Some(thread_id) = self.lookup_source_thread(source).await? {
                return Ok(thread_id);
            }
        }
        let cwd = psychevo_runtime::paths::canonicalize_cwd(&caller.cwd)?;
        if intent.policy.continue_latest {
            let continue_sources = if caller.continue_sources.is_empty() {
                vec![caller.runtime_source.as_str()]
            } else {
                caller
                    .continue_sources
                    .iter()
                    .map(String::as_str)
                    .collect()
            };
            if let Some(thread_id) = self
                .state
                .latest_session_for_cwd_with_sources(&cwd, &continue_sources)
                .await?
            {
                return Ok(thread_id);
            }
        }
        self.state
            .create_session_with_metadata(
                &cwd,
                &caller.runtime_source,
                "pending",
                "pending",
                intent.lineage.clone(),
            )
            .await
    }

    #[cfg(test)]
    pub(crate) async fn send_turn(
        &self,
        mut request: SendTurnRequest,
    ) -> psychevo_runtime::Result<GatewayTurnResult> {
        request.explicit_thread = request.thread_id.is_some();
        let turn_id = Uuid::now_v7().to_string();
        self.send_turn_with_id(request, turn_id).await
    }

    pub(crate) async fn send_turn_with_id(
        &self,
        request: SendTurnRequest,
        turn_id: String,
    ) -> psychevo_runtime::Result<GatewayTurnResult> {
        let queue_key = self.queue_key_for_request(&request).await?;
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

        gateway_profile_mark(
            if queued.is_some() {
                "gateway_turn_queued"
            } else {
                "gateway_turn_admitted"
            },
            Some(&turn_id),
            request
                .as_ref()
                .and_then(|request| request.thread_id.as_deref()),
            GatewayProfileFields {
                queue_depth: queued
                    .as_ref()
                    .map(|(_, _, _, queue_position, _)| *queue_position),
                ..GatewayProfileFields::default()
            },
        );

        if let Some((receiver, event_sink, thread_id, queue_position, active_activity_id)) = queued
        {
            if let Some(active_activity_id) = active_activity_id {
                let _ = self
                    .state

                    .set_gateway_activity_queued_turns(&active_activity_id, queue_position)
                    .await;
            }
            if let Some(event_sink) = event_sink {
                let _ = event_sink.emit(GatewayEvent::TurnQueued {
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
        let _admission = self
            .supervisor
            .acquire_activity_admission()
            .map_err(|error| Error::Message(error.to_string()))?;
        let queue_key = self.queue_key_for_shell_request(&request).await?;
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
                    ShellStartState::Queued {
                        receiver,
                        active_activity_id,
                        queue_position,
                    }
                }
            } else {
                state.running = true;
                ShellStartState::Standalone
            }
        };

        match active {
            ShellStartState::Queued {
                receiver,
                active_activity_id,
                queue_position,
            } => {
                if let Some(active_activity_id) = active_activity_id {
                    let _ = self
                        .state
                        .set_gateway_activity_queued_turns(&active_activity_id, queue_position)
                        .await;
                }
                receiver
                    .await
                    .map_err(|_| Error::Message("gateway shell queue closed".to_string()))?
            }
            ShellStartState::Auxiliary(inject_into) => {
                let (responder, receiver) = oneshot::channel();
                let gateway = self.clone();
                self.spawn_accepted_turn(format!("auxiliary-shell:{shell_id}"), async move {
                    let result = gateway
                        .run_shell_auxiliary(
                            request.take().expect("gateway shell request missing"),
                            shell_id,
                            inject_into,
                        )
                        .await;
                    let _ = responder.send(result);
                });
                receiver
                    .await
                    .map_err(|_| Error::Message("gateway auxiliary shell cancelled".to_string()))?
            }
            ShellStartState::Standalone => {
                let (responder, receiver) = oneshot::channel();
                let gateway = self.clone();
                self.spawn_accepted_turn(format!("shell:{shell_id}"), async move {
                    let result = gateway
                        .run_shell_now(
                            &queue_key,
                            request.take().expect("gateway shell request missing"),
                            shell_id,
                        )
                        .await;
                    gateway.finish_activity_and_spawn_next(queue_key);
                    let _ = responder.send(result);
                });
                receiver
                    .await
                    .map_err(|_| Error::Message("gateway shell cancelled".to_string()))?
            }
        }
    }

    pub async fn enqueue_compact_session(
        &self,
        request: SendCompactRequest,
    ) -> psychevo_runtime::Result<
        BoxFuture<'static, psychevo_runtime::Result<psychevo_runtime::compaction::CompactionResult>>,
    > {
        let admission = self
            .supervisor
            .acquire_activity_admission()
            .map_err(|error| Error::Message(error.to_string()))?;
        let queue_key = self.queue_key_for_compact_request(&request).await?;
        let compact_id = Uuid::now_v7().to_string();
        let event_sink = request.event_sink.clone();
        let event_thread_id = self.compact_event_thread_id(&request).await;
        let (responder, receiver) = oneshot::channel();
        let mut pending = Some(Box::new(PendingQueuedCompact {
            _admission: admission,
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
            self.emit_activity_changed_for_thread(event_sink, event_thread_id)
                .await;
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
    ) -> psychevo_runtime::Result<psychevo_runtime::compaction::CompactionResult> {
        self.enqueue_compact_session(request).await?.await
    }

    pub async fn steer_turn(
        &self,
        selector: GatewayThreadSelector,
        expected_turn_id: Option<&str>,
        message: psychevo_agent_core::Message,
    ) -> Option<psychevo_agent_core::PendingInputId> {
        if self.expected_turn_is_terminal(expected_turn_id).await {
            return None;
        }
        if !self.agent_supports_steer_for_selector(&selector).await {
            return None;
        }
        self.control_for_selector(&selector, expected_turn_id)
            .and_then(|control| control.steer_user_message(message))
    }

    async fn agent_supports_steer_for_selector(
        &self,
        selector: &GatewayThreadSelector,
    ) -> bool {
        let thread_id = match selector {
            GatewayThreadSelector::ThreadId { thread_id } => Some(thread_id.clone()),
            GatewayThreadSelector::Source { source_key } => {
                match self.state.gateway_source_lane(&source_key.0).await {
                    Ok(lane) => lane.and_then(|lane| lane.thread_id),
                    Err(_) => return false,
                }
            }
        };
        let Some(thread_id) = thread_id else {
            return true;
        };
        match self.state.gateway_runtime_binding(&thread_id).await {
            Ok(Some(binding)) => binding.backend_kind.as_deref() == Some("native"),
            Ok(None) => true,
            Err(_) => false,
        }
    }

    pub async fn steer_foreign_turn(
        &self,
        selector: GatewayThreadSelector,
        expected_turn_id: Option<&str>,
        message: psychevo_agent_core::Message,
    ) -> bool {
        if self.expected_turn_is_terminal(expected_turn_id).await {
            return false;
        }
        if !self.agent_supports_steer_for_selector(&selector).await {
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
        .await
    }

    async fn expected_turn_is_terminal(&self, expected_turn_id: Option<&str>) -> bool {
        let Some(turn_id) = expected_turn_id else {
            return false;
        };
        self.state
            .gateway_turn_terminal(turn_id)
            .await
            .map(|terminal| terminal.is_some())
            .unwrap_or(true)
    }

    pub fn cancel_steer(
        &self,
        selector: GatewayThreadSelector,
        expected_turn_id: Option<&str>,
        input_id: psychevo_agent_core::PendingInputId,
    ) -> bool {
        self.control_for_selector(&selector, expected_turn_id)
            .is_some_and(|control| control.cancel_pending_user_message(input_id))
    }

    pub fn update_steer(
        &self,
        selector: GatewayThreadSelector,
        expected_turn_id: Option<&str>,
        input_id: psychevo_agent_core::PendingInputId,
        message: psychevo_agent_core::Message,
    ) -> bool {
        self.control_for_selector(&selector, expected_turn_id)
            .is_some_and(|control| control.update_pending_user_message(input_id, message))
    }

    pub async fn interrupt_turn(&self, selector: GatewayThreadSelector) -> bool {
        if let Some(control) = self.control_for_selector(&selector, None) {
            control.abort();
            true
        } else {
            self.enqueue_foreign_control_command(&selector, "interrupt", json!({}))
                .await
        }
    }

    pub async fn submit_clarify(
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
            .await
    }

    pub async fn submit_permission(
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
                "filesystemScope": decision.filesystem_scope,
            }),
        )
        .await
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
