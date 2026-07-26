impl Gateway {
    fn cancel_active_queue(&self) {
        self.active
            .lock()
            .expect("gateway active map poisoned")
            .clear();
        self.active_aliases
            .lock()
            .expect("gateway active alias map poisoned")
            .clear();
    }

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
        if let Some(PendingQueuedActivity::Shell(next)) = next {
            let gateway = self.clone();
            let run_key = queue_key;
            gateway.clone().spawn_accepted_turn(
                format!("queued-shell:{}", next.shell_id),
                async move {
                    let result = gateway
                        .run_shell_now(&run_key, next.request, next.shell_id)
                        .await;
                    gateway.finish_activity_and_spawn_next(run_key);
                    let _ = next.responder.send(result);
                },
            );
        }
    }

    async fn queue_key_for_shell_request(
        &self,
        request: &SendShellRequest,
    ) -> psychevo::Result<String> {
        if let Some(thread_id) = &request.thread_id {
            return Ok(self.primary_queue_key_for_alias(thread_key(thread_id)));
        }
        if let Some(source) = &request.source {
            if let Some(thread_id) = self.lookup_source_thread(source).await? {
                return Ok(self.primary_queue_key_for_alias(thread_key(&thread_id)));
            }
            return Ok(self.primary_queue_key_for_alias(source_key_key(&source.source_key())));
        }
        if let Some(thread_id) = &request.context.session {
            return Ok(self.primary_queue_key_for_alias(thread_key(thread_id)));
        }
        Ok(format!("shell:{}", Uuid::now_v7()))
    }

    async fn lookup_source_thread(
        &self,
        source: &GatewaySource,
    ) -> psychevo::Result<Option<String>> {
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
                .gateway_source_lane(&source.source_key().0)
                .await?
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

    fn register_active(
        &self,
        key: &str,
        activity_id: String,
        control: Option<RunControlHandle>,
        kind: ActiveActivityKind,
    ) {
        let mut active = self.active.lock().expect("gateway active map poisoned");
        let state = active.entry(key.to_string()).or_default();
        state.active_turn_id = Some(activity_id);
        state.control = control;
        state.active_kind = Some(kind);
    }

    fn register_active_thread_alias(&self, key: &str, thread_id: &str) {
        self.register_active_queue_alias(&thread_key(thread_id), key);
    }

    fn register_active_queue_alias(&self, alias: &str, primary: &str) {
        if alias != primary {
            self.active_aliases
                .lock()
                .expect("gateway active alias map poisoned")
                .insert(alias.to_string(), primary.to_string());
        }
    }

    fn primary_queue_key_for_alias(&self, key: String) -> String {
        self.active_aliases
            .lock()
            .expect("gateway active alias map poisoned")
            .get(&key)
            .cloned()
            .unwrap_or(key)
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
                keys
            }
        }
    }
}
