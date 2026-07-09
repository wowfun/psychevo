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
            }
        }
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
                .store()
                .gateway_source_binding(&source.source_key().0)?
                .map(|binding| binding.thread_id)),
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
                    .store()
                    .upsert_gateway_source_binding(GatewaySourceBindingInput {
                        source_key: &source_key.0,
                        source_kind: &source.kind,
                        raw_identity: source.raw_identity.clone().unwrap_or(Value::Null),
                        visible_name: source.visible_name.as_deref(),
                        thread_id: &result.session_id,
                        backend_kind: backend.kind.as_str(),
                        backend_native_id: backend.native_id.as_deref(),
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
                if let Ok(Some(binding)) = self.state.store().gateway_source_binding(&source_key.0)
                {
                    keys.push(thread_key(&binding.thread_id));
                }
                keys
            }
        }
    }
}
