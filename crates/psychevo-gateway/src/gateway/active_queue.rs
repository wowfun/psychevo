impl Gateway {
    fn finish_activity_and_spawn_next(&self, queue_key: String) {
        let next = {
            let mut active = self.active.lock().expect("gateway active map poisoned");
            let Some(state) = active.get_mut(&queue_key) else {
                return;
            };
            state.control = None;
            state.active_turn_id = None;
            state.active_kind = None;
            if let Some(next) = state.queued.pop_front() {
                state.running = true;
                Some(next)
            } else {
                active.remove(&queue_key);
                self.active_aliases
                    .lock()
                    .expect("gateway active alias map poisoned")
                    .retain(|_, primary| primary != &queue_key);
                None
            }
        };
        if let Some(next) = next {
            let gateway = self.clone();
            let run_key = queue_key.clone();
            match next {
                PendingQueuedActivity::Turn(next) => {
                    tokio::spawn(async move {
                        let result = gateway
                            .run_turn_now(&run_key, next.request, next.turn_id)
                            .await;
                        let _ = next.responder.send(result);
                        gateway.finish_activity_and_spawn_next(run_key);
                    });
                }
                PendingQueuedActivity::Shell(next) => {
                    tokio::spawn(async move {
                        let result = gateway
                            .run_shell_now(&run_key, next.request, next.shell_id)
                            .await;
                        let _ = next.responder.send(result);
                        gateway.finish_activity_and_spawn_next(run_key);
                    });
                }
                PendingQueuedActivity::Compact(next) => {
                    gateway.spawn_compact_activity(run_key, next);
                }
            }
        }
    }

    fn spawn_compact_activity(&self, run_key: String, next: Box<PendingQueuedCompact>) {
        let gateway = self.clone();
        tokio::spawn(async move {
            let event_sink = next.request.event_sink.clone();
            let event_thread_id = gateway.compact_event_thread_id(&next.request);
            let result = gateway
                .run_compact_now(&run_key, next.request, next.compact_id)
                .await;
            let _ = next.responder.send(result);
            gateway.finish_activity_and_spawn_next(run_key);
            gateway.emit_activity_changed_for_thread(event_sink, event_thread_id);
        });
    }

    fn compact_event_thread_id(&self, request: &SendCompactRequest) -> Option<String> {
        request.thread_id.clone().or_else(|| {
            request
                .source
                .as_ref()
                .and_then(|source| self.lookup_source_thread(source).ok().flatten())
        })
    }

    fn non_native_compaction_runtime(
        &self,
        request: &SendCompactRequest,
        thread_id: &str,
    ) -> psychevo_runtime::Result<Option<String>> {
        if let Some(binding) = self.state.gateway_runtime_binding(thread_id)? {
            if binding.status == GatewayRuntimeBindingStatus::Resolved {
                return Ok(binding
                    .runtime_ref
                    .filter(|runtime_ref| runtime_ref != "native"));
            }
            return Ok(Some(
                binding
                    .runtime_ref
                    .unwrap_or_else(|| "unresolved".to_string()),
            ));
        }

        let summary = self
            .state

            .session_summary(thread_id)?
            .ok_or_else(|| Error::Message(format!("session not found: {thread_id}")))?;
        if summary.source == "peer_agent" {
            let runtime_ref = self
                .state

                .session_metadata(thread_id)?
                .as_ref()
                .and_then(|metadata| {
                    metadata
                        .get("runtimeRef")
                        .or_else(|| metadata.get("runtime_ref"))
                })
                .and_then(Value::as_str)
                .unwrap_or("peer_agent")
                .to_string();
            return Ok(Some(runtime_ref));
        }

        Ok(request
            .runtime_ref
            .as_deref()
            .map(str::trim)
            .filter(|runtime_ref| !runtime_ref.is_empty() && *runtime_ref != "native")
            .map(ToString::to_string))
    }

    fn emit_activity_changed_for_thread(
        &self,
        event_sink: Option<GatewayEventSink>,
        thread_id: Option<String>,
    ) {
        let (Some(event_sink), Some(thread_id)) = (event_sink, thread_id) else {
            return;
        };
        event_sink(GatewayEvent::ActivityChanged {
            thread_id: Some(thread_id.clone()),
            activity: gateway_activity_view(
                &self.activity_for_selector(GatewayThreadSelector::thread_id(&thread_id)),
            ),
        });
    }

    fn queue_key_for_request(&self, request: &SendTurnRequest) -> psychevo_runtime::Result<String> {
        if let Some(thread_id) = &request.thread_id {
            return Ok(self.primary_queue_key_for_alias(thread_key(thread_id)));
        }
        if let Some(source) = &request.source {
            if !request.reset_source_binding
                && let Some(thread_id) = self.lookup_source_thread(source)?
            {
                return Ok(self.primary_queue_key_for_alias(thread_key(&thread_id)));
            }
            return Ok(self.primary_queue_key_for_alias(source_key_key(&source.source_key())));
        }
        if let Some(thread_id) = &request.options.session {
            return Ok(self.primary_queue_key_for_alias(thread_key(thread_id)));
        }
        Ok(format!("invocation:{}", Uuid::now_v7()))
    }

    fn queue_key_for_shell_request(
        &self,
        request: &SendShellRequest,
    ) -> psychevo_runtime::Result<String> {
        if let Some(thread_id) = &request.thread_id {
            return Ok(self.primary_queue_key_for_alias(thread_key(thread_id)));
        }
        if let Some(source) = &request.source {
            if let Some(thread_id) = self.lookup_source_thread(source)? {
                return Ok(self.primary_queue_key_for_alias(thread_key(&thread_id)));
            }
            return Ok(self.primary_queue_key_for_alias(source_key_key(&source.source_key())));
        }
        if let Some(thread_id) = &request.context.session {
            return Ok(self.primary_queue_key_for_alias(thread_key(thread_id)));
        }
        Ok(format!("shell:{}", Uuid::now_v7()))
    }

    fn queue_key_for_compact_request(
        &self,
        request: &SendCompactRequest,
    ) -> psychevo_runtime::Result<String> {
        if let Some(thread_id) = &request.thread_id {
            return Ok(self.primary_queue_key_for_alias(thread_key(thread_id)));
        }
        if let Some(source) = &request.source {
            if let Some(thread_id) = self.lookup_source_thread(source)? {
                return Ok(self.primary_queue_key_for_alias(thread_key(&thread_id)));
            }
            return Ok(self.primary_queue_key_for_alias(source_key_key(&source.source_key())));
        }
        Ok(format!("compact:{}", Uuid::now_v7()))
    }

    fn lookup_source_thread(
        &self,
        source: &GatewaySource,
    ) -> psychevo_runtime::Result<Option<String>> {
        match source.lifetime {
            GatewaySourceLifetime::Invocation => Ok(None),
            GatewaySourceLifetime::Process => Ok(self
                .process_bindings
                .lock()
                .expect("gateway process binding map poisoned")
                .get(&source.source_key().0)
                .cloned()),
            GatewaySourceLifetime::Persistent => Ok(self
                .state

                .gateway_source_lane(&source.source_key().0)?
                .and_then(|lane| lane.thread_id)),
        }
    }

    fn source_generation(&self, source: &GatewaySource) -> u64 {
        let key = source.source_key();
        self.source_generations
            .lock()
            .expect("gateway source generation map poisoned")
            .get(&key.0)
            .copied()
            .unwrap_or(0)
    }

    fn bump_source_generation_key(&self, source_key: &SourceKey) {
        let mut generations = self
            .source_generations
            .lock()
            .expect("gateway source generation map poisoned");
        let generation = generations.entry(source_key.0.clone()).or_default();
        *generation = generation.saturating_add(1);
    }

    fn bind_source_to_result(
        &self,
        source: &GatewaySource,
        result: &RunResult,
        backend: &GatewayBackendInfo,
        lineage: Option<Value>,
        expected_generation: Option<u64>,
    ) -> psychevo_runtime::Result<()> {
        let source_key = source.source_key();
        if let Some(expected_generation) = expected_generation
            && self.source_generation(source) != expected_generation
        {
            return Ok(());
        }
        match source.lifetime {
            GatewaySourceLifetime::Invocation => {}
            GatewaySourceLifetime::Process => {
                self.process_bindings
                    .lock()
                    .expect("gateway process binding map poisoned")
                    .insert(source_key.0.clone(), result.session_id.clone());
            }
            GatewaySourceLifetime::Persistent => {
                self.state

                    .upsert_gateway_source_lane(GatewaySourceLaneInput {
                        source_key: &source_key.0,
                        source_kind: &source.kind,
                        raw_identity: source.raw_identity.clone().unwrap_or(Value::Null),
                        visible_name: source.visible_name.as_deref(),
                        thread_id: Some(&result.session_id),
                        draft_agent_ref: None,
                        draft_profile_ref: None,
                        draft_control_values: &Default::default(),
                        lineage: lineage_with_runtime_ref(lineage, backend.runtime_ref.as_deref()),
                    })?;
            }
        }
        if source.lifetime != GatewaySourceLifetime::Invocation {
            self.bump_source_generation_key(&source_key);
        }
        Ok(())
    }

    fn register_active(
        &self,
        key: &str,
        turn_id: String,
        control: Option<RunControlHandle>,
        kind: ActiveActivityKind,
    ) {
        let mut active = self.active.lock().expect("gateway active map poisoned");
        let state = active.entry(key.to_string()).or_default();
        state.active_turn_id = Some(turn_id);
        state.control = control;
        state.active_kind = Some(kind);
    }

    fn mark_active_turn_terminal(&self, turn_id: &str) {
        let mut active = self.active.lock().expect("gateway active map poisoned");
        for state in active.values_mut() {
            if state.active_turn_id.as_deref() == Some(turn_id) {
                state.control = None;
                state.active_turn_id = None;
                state.active_kind = None;
            }
        }
    }

    fn register_active_thread_alias(&self, key: &str, thread_id: &str) {
        let alias = thread_key(thread_id);
        self.register_active_queue_alias(&alias, key);
    }

    fn register_active_queue_alias(&self, alias: &str, primary: &str) {
        if alias == primary {
            return;
        }
        self.active_aliases
            .lock()
            .expect("gateway active alias map poisoned")
            .insert(alias.to_string(), primary.to_string());
    }

    fn primary_queue_key_for_alias(&self, key: String) -> String {
        self.active_aliases
            .lock()
            .expect("gateway active alias map poisoned")
            .get(&key)
            .cloned()
            .unwrap_or(key)
    }

    fn control_for_selector(
        &self,
        selector: &GatewayThreadSelector,
        expected_turn_id: Option<&str>,
    ) -> Option<RunControlHandle> {
        let selector_keys = self.selector_keys_with_active_aliases(selector);
        let active = self.active.lock().expect("gateway active map poisoned");
        let mut seen = HashSet::new();
        for key in selector_keys {
            if !seen.insert(key.clone()) {
                continue;
            }
            if let Some(state) = active.get(&key) {
                if expected_turn_id
                    .is_some_and(|expected| state.active_turn_id.as_deref() != Some(expected))
                {
                    continue;
                }
                if let Some(control) = &state.control {
                    return Some(control.clone());
                }
            }
        }
        None
    }

    fn selector_keys_with_active_aliases(&self, selector: &GatewayThreadSelector) -> Vec<String> {
        let selector_keys = self.selector_keys(selector);
        let aliases = self
            .active_aliases
            .lock()
            .expect("gateway active alias map poisoned");
        let mut keys = Vec::new();
        let mut seen = HashSet::new();
        for key in selector_keys {
            if seen.insert(key.clone()) {
                keys.push(key.clone());
            }
            if let Some(primary) = aliases.get(&key)
                && seen.insert(primary.clone())
            {
                keys.push(primary.clone());
            }
        }
        keys
    }

    fn selector_keys(&self, selector: &GatewayThreadSelector) -> Vec<String> {
        match selector {
            GatewayThreadSelector::ThreadId { thread_id } => vec![thread_key(thread_id)],
            GatewayThreadSelector::Source { source_key } => {
                let mut keys = vec![source_key_key(source_key)];
                if let Some(thread_id) = self
                    .process_bindings
                    .lock()
                    .expect("gateway process binding map poisoned")
                    .get(&source_key.0)
                    .cloned()
                {
                    keys.push(thread_key(&thread_id));
                }
                if let Ok(Some(lane)) = self.state.gateway_source_lane(&source_key.0)
                    && let Some(thread_id) = lane.thread_id
                {
                    keys.push(thread_key(&thread_id));
                }
                keys
            }
        }
    }
}

fn unavailable_compaction_result(
    thread_id: &str,
    reason: psychevo_runtime::compaction::CompactionReason,
    runtime_ref: &str,
) -> psychevo_runtime::compaction::CompactionResult {
    psychevo_runtime::compaction::CompactionResult {
        session_id: thread_id.to_string(),
        compacted: false,
        reason: reason.as_str().to_string(),
        message: format!(
            "Context compaction is unavailable for runtime profile `{runtime_ref}` until its adapter owns native compaction."
        ),
        checkpoint_id: None,
        first_kept_session_seq: None,
        tokens_before: None,
        tokens_after: None,
        summary: None,
        summary_provider: None,
        summary_model: None,
    }
}
