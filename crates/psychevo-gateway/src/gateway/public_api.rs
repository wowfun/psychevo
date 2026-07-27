pub(crate) struct BoundedTranscriptPage {
    pub(crate) entries: Vec<TranscriptEntry>,
    pub(crate) next_cursor: Option<String>,
}

impl Gateway {
    pub(crate) async fn discover_agent_sessions(
        &self,
        profile: RuntimeProfileConfig,
        peer: ResolvedPeerTurn,
        cwd: PathBuf,
        cursor: Option<String>,
    ) -> psychevo::Result<acp_peer::AcpSessionListPage> {
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
    ) -> psychevo::Result<acp_peer::AcpSessionLoadOutput> {
        let mcp_servers = acp_peer::resolve_peer_mcp_server_handoffs(&peer, &options).await?;
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
    ) -> psychevo::Result<()> {
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
    ) -> psychevo::Result<acp_peer::AcpSessionSnapshot> {
        let native_session_id = binding.native_session_id.clone().ok_or_else(|| {
            agent_session_configuration_error(format!(
                "Agent binding for thread `{}` has no native session id.",
                binding.thread_id
            ))
        })?;
        let mcp_servers = acp_peer::resolve_peer_mcp_server_handoffs(&peer, &options).await?;
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
    ) -> psychevo::Result<acp_peer::AcpSessionSnapshot> {
        let native_session_id = binding.native_session_id.clone().ok_or_else(|| {
            agent_session_configuration_error(format!(
                "Agent binding for thread `{}` has no native session id.",
                binding.thread_id
            ))
        })?;
        let mcp_servers = acp_peer::resolve_peer_mcp_server_handoffs(&peer, &options).await?;
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
    ) -> psychevo::Result<()> {
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
    ) -> psychevo::Result<()> {
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
            owner_id: Arc::new(format!("gateway:{}:{}", std::process::id(), Uuid::now_v7())),
            framework_application: Arc::new(OnceLock::new()),
        }
    }

    pub fn state(&self) -> &StateRuntime {
        &self.state
    }

    pub(crate) fn attach_framework_application(
        &self,
        application: Application,
    ) -> psychevo::Result<()> {
        self.framework_application.set(application).map_err(|_| {
            Error::Message("Gateway Framework Application is already attached".to_string())
        })
    }

    pub(crate) fn framework_client(&self) -> psychevo::Result<psychevo::Client> {
        self.framework_application
            .get()
            .map(Application::client)
            .ok_or_else(|| {
                Error::Message("Gateway Framework Application is not attached".to_string())
            })
    }

    pub async fn shutdown_runtimes(&self, force: bool) -> psychevo::Result<()> {
        self.agent_sessions.shutdown(force).await
    }

    pub(crate) async fn shutdown_application(
        &self,
        force: bool,
    ) -> psychevo::Result<()> {
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
    ) -> psychevo::Result<Option<acp_peer::AcpSessionSnapshot>> {
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
    ) -> psychevo::Result<acp_peer::AcpSessionSnapshot> {
        let mcp_servers = acp_peer::resolve_peer_mcp_server_handoffs(&peer, &options).await?;
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
    ) -> psychevo::Result<Option<acp_peer::AcpSessionSnapshot>> {
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
    ) -> psychevo::Result<Option<acp_peer::AcpSessionSnapshot>> {
        self.agent_sessions
            .set_prepared_control(source_key, target_id, control_id, value)
            .await
    }

    pub(crate) async fn release_prepared_agent_session(
        &self,
        source_key: &str,
    ) -> psychevo::Result<bool> {
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
    ) -> psychevo::Result<acp_peer::AcpSessionSnapshot> {
        let mcp_servers = acp_peer::resolve_peer_mcp_server_handoffs(&peer, &options).await?;
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
    ) -> psychevo::Result<acp_peer::AcpAuthDoctorStatus> {
        self.agent_sessions
            .probe_acp_authentication(peer, cwd)
            .await
    }

    pub(crate) async fn probe_acp_backend_protocol_compatibility(
        &self,
        peer: ResolvedPeerTurn,
        cwd: PathBuf,
    ) -> psychevo::Result<acp_peer::AcpProtocolDoctorStatus> {
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
    ) -> psychevo::Result<RunResult> {
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
    ) -> psychevo::Result<Option<String>> {
        self.lookup_source_thread(source).await
    }

    pub async fn thread_transcript(
        &self,
        thread_id: &str,
    ) -> psychevo::Result<Vec<TranscriptEntry>> {
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

    pub(crate) async fn thread_transcript_page(
        &self,
        thread_id: &str,
        before_session_seq: Option<i64>,
        limit: usize,
    ) -> psychevo::Result<BoundedTranscriptPage> {
        let fetch_limit = limit.saturating_mul(4).saturating_add(8);
        let summaries = self
            .state
            .load_tui_message_summaries_before(
                thread_id,
                before_session_seq,
                fetch_limit,
            )
            .await?;
        let has_older_messages = summaries.len() == fetch_limit;
        let lower_boundary = summaries
            .first()
            .map(|summary| summary.session_seq)
            .unwrap_or(i64::MIN);
        let mut entries = transcript::project_transcript_entries(thread_id, &summaries);
        let agent_edges = self
            .state
            .list_agent_edges_for_parent_candidates(
                thread_id,
                &transcript::agent_edge_lookup_candidates(&entries),
            )
            .await?;
        transcript::enrich_agent_blocks_from_edges(&mut entries, &agent_edges);

        let compactions = self
            .state
            .list_valid_session_compactions_between(
                thread_id,
                lower_boundary,
                before_session_seq,
                fetch_limit,
            )
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
            .list_gateway_turn_terminals_for_thread_window(
                thread_id,
                lower_boundary,
                before_session_seq,
                fetch_limit,
            )
            .await?;
        transcript::reconcile_terminal_bounded_running_blocks(&mut entries, &terminals);
        synthetic_entries.extend(terminals.iter().filter_map(|terminal| {
            let boundary = transcript::terminal_structural_boundary(terminal);
            (boundary >= lower_boundary
                && before_session_seq.is_none_or(|upper_boundary| boundary < upper_boundary))
                .then(|| transcript::project_turn_terminal_entry(terminal).map(|entry| (boundary, entry)))
                .flatten()
        }));
        let mut entries =
            transcript::merge_entries_at_session_boundaries(entries, synthetic_entries);
        let projected_overflow = entries.len() > limit;
        if projected_overflow {
            let drain = entries.len() - limit;
            entries.drain(..drain);
        }
        let next_cursor = (has_older_messages || projected_overflow)
            .then(|| {
                entries
                    .iter()
                    .filter_map(|entry| entry.message_seq)
                    .min()
                    .map(|session_seq| format!("message:{session_seq}"))
            })
            .flatten();
        Ok(BoundedTranscriptPage {
            entries,
            next_cursor,
        })
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
                if state.running
                    && let (Some(activity_id), Some(kind)) =
                        (state.active_turn_id.as_ref(), state.active_kind)
                {
                    let provenance = match kind {
                        ActiveActivityKind::Shell => ThreadActivityView::GatewayLocal {
                            operation: GatewayLocalOperationView::Shell,
                            activity_id: activity_id.clone(),
                        },
                    };
                    if !activity.activities.contains(&provenance)
                    {
                        activity.activities.push(provenance);
                    }
                }
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
    ) -> psychevo::Result<BTreeMap<String, GatewayActivity>> {
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
    ) -> psychevo::Result<Option<GatewayActivityRecord>> {
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
            if record.kind == "turn"
                && activity.active_turn_id.is_none()
                && let Some(turn_id) = record.turn_id.clone()
            {
                activity.active_turn_id = Some(turn_id);
            }
            let provenance = if record.owner_id != self.owner_id() {
                Some(ThreadActivityView::Foreign {
                    owner_id: record.owner_id.clone(),
                    activity_id: record.activity_id.clone(),
                    owner_surface: record.owner_surface.clone(),
                })
            } else {
                match record.kind.as_str() {
                    "shell" => Some(ThreadActivityView::GatewayLocal {
                        operation: GatewayLocalOperationView::Shell,
                        activity_id: record.activity_id.clone(),
                    }),
                    _ => None,
                }
            };
            if let Some(provenance) = provenance
                && !activity.activities.contains(&provenance)
            {
                activity.activities.push(provenance);
            }
        }
        if stale && activity.takeover_state.is_none() {
            activity.takeover_state = Some("stale".to_string());
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

    pub async fn send_shell(
        &self,
        request: SendShellRequest,
    ) -> psychevo::Result<GatewayShellResult> {
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

    pub async fn submit_clarify(
        &self,
        selector: GatewayThreadSelector,
        call_id: &str,
        result: ClarifyResult,
    ) -> bool {
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

    pub async fn steer_foreign_turn(
        &self,
        selector: GatewayThreadSelector,
        expected_turn_id: Option<&str>,
        message: psychevo::__agent_core::Message,
    ) -> bool {
        if self.expected_turn_is_terminal(expected_turn_id).await
            || !self.agent_supports_steer_for_selector(&selector).await
        {
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

    pub async fn interrupt_turn(&self, selector: GatewayThreadSelector) -> bool {
        self.enqueue_foreign_control_command(&selector, "interrupt", json!({}))
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

    pub async fn submit_permission(
        &self,
        selector: GatewayThreadSelector,
        request_id: &str,
        decision: PermissionApprovalDecision,
    ) -> bool {
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
        for PendingQueuedActivity::Shell(pending) in dropped {
            let _ = pending.responder.send(Err(Error::Message(
                "gateway shell queue cleared".to_string(),
            )));
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
