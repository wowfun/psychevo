impl Gateway {
    pub fn new(state: StateRuntime) -> Self {
        Self::with_backend(state, Arc::new(PsychevoRuntimeBackend))
    }

    pub fn with_backend(state: StateRuntime, backend: Arc<dyn GatewayBackend>) -> Self {
        Self {
            state,
            backend,
            active: Arc::new(Mutex::new(HashMap::new())),
            active_aliases: Arc::new(Mutex::new(HashMap::new())),
            process_bindings: Arc::new(Mutex::new(HashMap::new())),
            source_generations: Arc::new(Mutex::new(HashMap::new())),
            pending_permissions: Arc::new(Mutex::new(HashMap::new())),
            owner_id: Arc::new(format!("gateway:{}:{}", std::process::id(), Uuid::now_v7())),
        }
    }

    pub fn state(&self) -> &StateRuntime {
        &self.state
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
        Ok(transcript::project_transcript_entries(
            thread_id, &summaries,
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

    fn durable_activity_for_key(
        &self,
        key: &str,
    ) -> psychevo_runtime::Result<Option<GatewayActivityRecord>> {
        if let Some(thread_id) = key.strip_prefix("thread:") {
            return self.state.store().active_gateway_activity_for_thread(thread_id);
        }
        if let Some(source_key) = key.strip_prefix("source:") {
            return self.state.store().active_gateway_activity_for_source(source_key);
        }
        Ok(None)
    }

    fn merge_durable_activity(&self, activity: &mut GatewayActivity, record: GatewayActivityRecord) {
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

    pub async fn send_turn(
        &self,
        request: SendTurnRequest,
    ) -> psychevo_runtime::Result<GatewayTurnResult> {
        let queue_key = self.queue_key_for_request(&request)?;
        let turn_id = Uuid::now_v7().to_string();
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

        if let Some((receiver, event_sink, thread_id, queue_position, active_activity_id)) = queued {
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

    pub fn steer_turn(
        &self,
        selector: GatewayThreadSelector,
        expected_turn_id: Option<&str>,
        message: psychevo_runtime::Message,
    ) -> Option<psychevo_runtime::PendingInputId> {
        self.control_for_selector(&selector, expected_turn_id)
            .and_then(|control| control.steer_user_message(message))
    }

    pub fn steer_foreign_turn(
        &self,
        selector: GatewayThreadSelector,
        expected_turn_id: Option<&str>,
        message: psychevo_runtime::Message,
    ) -> bool {
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
        let permissions = self
            .pending_permissions
            .lock()
            .expect("gateway pending permission map poisoned");
        permissions.get(request_id).is_some_and(|pending| {
            pending.selector_key.as_deref().is_none_or(|pending_key| {
                selector_keys.iter().any(|key| key == pending_key)
            })
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
            }
        }
        count
    }

}
