const GATEWAY_ACTIVITY_LEASE_MS: i64 = 30_000;
const GATEWAY_ACTIVITY_HEARTBEAT_MS: i64 = 5_000;
const GATEWAY_CONTROL_POLL_MS: u64 = 500;

#[derive(Clone, Debug)]
struct DurableGatewayActivity {
    activity_id: String,
    owner_id: String,
    generation: i64,
    turn_id: Option<String>,
}

struct DurableGatewayActivityClaim<'a> {
    activity_id: &'a str,
    thread_id: Option<&'a str>,
    source_key: Option<&'a str>,
    turn_id: Option<&'a str>,
    kind: &'a str,
    owner_surface: Option<&'a str>,
    queued_turns: usize,
    intent: Option<Value>,
}

impl Gateway {
    fn claim_durable_gateway_activity(
        &self,
        claim: DurableGatewayActivityClaim<'_>,
    ) -> psychevo_runtime::Result<DurableGatewayActivity> {
        let record = self.state.store().claim_gateway_activity(
            GatewayActivityClaimInput {
                activity_id: claim.activity_id,
                thread_id: claim.thread_id,
                source_key: claim.source_key,
                turn_id: claim.turn_id,
                kind: claim.kind,
                owner_id: self.owner_id(),
                owner_surface: claim.owner_surface,
                lease_expires_at_ms: gateway_now_ms() + GATEWAY_ACTIVITY_LEASE_MS,
                queued_turns: claim.queued_turns,
                superseded_activity_id: None,
                intent: claim.intent,
            },
        )?;
        Ok(DurableGatewayActivity {
            activity_id: record.activity_id,
            owner_id: record.owner_id,
            generation: record.generation,
            turn_id: record.turn_id,
        })
    }

    fn spawn_durable_activity_heartbeat(
        &self,
        activity: DurableGatewayActivity,
    ) -> oneshot::Sender<()> {
        let (stop_tx, mut stop_rx) = oneshot::channel::<()>();
        let gateway = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(GATEWAY_CONTROL_POLL_MS));
            let mut last_heartbeat_ms = 0;
            loop {
                tokio::select! {
                    _ = &mut stop_rx => break,
                    _ = tick.tick() => {
                        let now = gateway_now_ms();
                        if now.saturating_sub(last_heartbeat_ms) >= GATEWAY_ACTIVITY_HEARTBEAT_MS {
                            let lease_expires_at_ms = now + GATEWAY_ACTIVITY_LEASE_MS;
                            let _ = gateway.state.store().heartbeat_gateway_activity(
                                &activity.activity_id,
                                &activity.owner_id,
                                activity.generation,
                                lease_expires_at_ms,
                            );
                            last_heartbeat_ms = now;
                        }
                        gateway.apply_pending_gateway_control_commands();
                    }
                }
            }
        });
        stop_tx
    }

    fn wrap_gateway_event_sink(
        &self,
        event_sink: Option<GatewayEventSink>,
        activity: Option<DurableGatewayActivity>,
        queue_key: Option<String>,
        default_turn_id: Option<String>,
    ) -> Option<GatewayEventSink> {
        if event_sink.is_none() && activity.is_none() {
            return None;
        }
        let gateway = self.clone();
        Some(Arc::new(move |event: GatewayEvent| {
            if let GatewayEvent::TurnStarted {
                thread_id: Some(thread_id),
                ..
            } = &event
                && let Some(queue_key) = queue_key.as_deref()
            {
                gateway.register_active_thread_alias(queue_key, thread_id);
            }
            if let Some(activity) = activity.as_ref() {
                gateway.persist_gateway_event(activity, &event, default_turn_id.as_deref());
            }
            if let Some(event_sink) = event_sink.as_ref() {
                event_sink(event);
            }
        }) as GatewayEventSink)
    }

    fn persist_gateway_event(
        &self,
        activity: &DurableGatewayActivity,
        event: &GatewayEvent,
        default_turn_id: Option<&str>,
    ) {
        if let GatewayEvent::TurnStarted {
            thread_id: Some(thread_id),
            ..
        } = event
        {
            let _ = self.state.store().update_gateway_activity_thread(
                &activity.activity_id,
                &activity.owner_id,
                activity.generation,
                thread_id,
                gateway_now_ms() + GATEWAY_ACTIVITY_LEASE_MS,
            );
        }
        if let Ok(event_value) = serde_json::to_value(event) {
            let _ = self.state.store().append_gateway_live_event(
                Some(&activity.activity_id),
                Some(&activity.owner_id),
                gateway_event_thread_id(event).as_deref(),
                gateway_event_turn_id(event).or(default_turn_id).or(activity.turn_id.as_deref()),
                &event_value,
            );
        }
    }

    fn finish_durable_gateway_activity(
        &self,
        activity: Option<&DurableGatewayActivity>,
        status: &str,
    ) {
        if let Some(activity) = activity {
            let _ = self.state.store().finish_gateway_activity(
                &activity.activity_id,
                &activity.owner_id,
                activity.generation,
                status,
            );
        }
    }

    pub fn takeover_turn(
        &self,
        selector: GatewayThreadSelector,
    ) -> psychevo_runtime::Result<(bool, GatewayActivity)> {
        let now = gateway_now_ms();
        for key in self.selector_keys(&selector) {
            let Some(record) = self.durable_activity_for_key(&key)? else {
                continue;
            };
            if record.owner_id == self.owner_id() {
                return Ok((false, self.activity_for_selector(selector)));
            }
            if record.status != "running" && record.status != "queued" {
                continue;
            }
            if record.lease_expires_at_ms < now {
                let superseded_by_activity_id = Uuid::now_v7().to_string();
                let accepted = self
                    .state
                    .store()
                    .supersede_stale_gateway_activity(
                        &record.activity_id,
                        &superseded_by_activity_id,
                    )?;
                let mut activity = self.activity_for_selector(selector);
                if accepted {
                    activity.takeover_state = Some("takenOver".to_string());
                    self.append_gateway_activity_changed(
                        record.thread_id,
                        &activity,
                        Some(&record.activity_id),
                    );
                }
                return Ok((accepted, activity));
            }
            let accepted = self.enqueue_foreign_control_command(
                &selector,
                "takeover",
                json!({
                    "activityId": record.activity_id,
                    "requestedOwnerId": self.owner_id(),
                }),
            );
            let mut activity = self.activity_for_selector(selector);
            if accepted {
                activity.takeover_state = Some("requested".to_string());
            }
            return Ok((accepted, activity));
        }
        Ok((false, self.activity_for_selector(selector)))
    }

    fn append_gateway_activity_changed(
        &self,
        thread_id: Option<String>,
        activity: &GatewayActivity,
        activity_id: Option<&str>,
    ) {
        let event = GatewayEvent::ActivityChanged {
            thread_id: thread_id.clone(),
            activity: gateway_activity_view(activity),
        };
        if let Ok(event_value) = serde_json::to_value(event) {
            let _ = self.state.store().append_gateway_live_event(
                activity_id,
                Some(self.owner_id()),
                thread_id.as_deref(),
                activity.active_turn_id.as_deref(),
                &event_value,
            );
        }
    }

    fn enqueue_foreign_control_command(
        &self,
        selector: &GatewayThreadSelector,
        command_kind: &str,
        payload: Value,
    ) -> bool {
        let now = gateway_now_ms();
        for key in self.selector_keys(selector) {
            let Ok(Some(record)) = self.durable_activity_for_key(&key) else {
                continue;
            };
            if record.owner_id == self.owner_id()
                || record.status != "running"
                || record.lease_expires_at_ms < now
            {
                continue;
            }
            return self
                .state
                .store()
                .enqueue_gateway_control_command(GatewayControlCommandInput {
                    activity_id: &record.activity_id,
                    owner_id: &record.owner_id,
                    command_kind,
                    payload,
                })
                .is_ok();
        }
        false
    }

    fn apply_pending_gateway_control_commands(&self) {
        let Ok(commands) = self
            .state
            .store()
            .pending_gateway_control_commands(self.owner_id(), 50)
        else {
            return;
        };
        for command in commands {
            let applied = match command.command_kind.as_str() {
                "interrupt" => self
                    .control_for_activity_id(&command.activity_id)
                    .map(|control| {
                        control.abort();
                        true
                    })
                    .unwrap_or(false),
                "takeover" => self.apply_takeover_control_command(&command.activity_id),
                "steer" => self.apply_steer_control_command(&command.activity_id, &command.payload),
                "permission" => self.apply_permission_control_command(&command.payload),
                "clarify" => self.apply_clarify_control_command(&command.payload),
                _ => false,
            };
            let store = self.state.store();
            let _ = if applied {
                store.mark_gateway_control_command_applied(command.id)
            } else {
                store.mark_gateway_control_command_failed(command.id, "no matching active control")
            };
        }
    }

    fn apply_takeover_control_command(&self, activity_id: &str) -> bool {
        let control = self.control_for_activity_id(activity_id);
        if let Some(control) = control.as_ref() {
            control.abort();
        }
        let released = self
            .state
            .store()
            .gateway_activity(activity_id)
            .ok()
            .flatten()
            .and_then(|record| {
                self.state
                    .store()
                    .finish_gateway_activity(
                        activity_id,
                        self.owner_id(),
                        record.generation,
                        "released",
                    )
                    .ok()
            })
            .unwrap_or(false);
        released || control.is_some()
    }

    fn apply_steer_control_command(&self, activity_id: &str, payload: &Value) -> bool {
        if let Some(expected_turn_id) = payload.get("expectedTurnId").and_then(Value::as_str)
            && expected_turn_id != activity_id
        {
            return false;
        }
        let Some(message_value) = payload.get("message").cloned() else {
            return false;
        };
        let Ok(message) = serde_json::from_value(message_value) else {
            return false;
        };
        self.control_for_activity_id(activity_id)
            .and_then(|control| control.steer_user_message(message))
            .is_some()
    }

    fn apply_permission_control_command(&self, payload: &Value) -> bool {
        let Some(request_id) = payload.get("requestId").and_then(Value::as_str) else {
            return false;
        };
        let Some(decision) = payload
            .get("decision")
            .and_then(Value::as_str)
            .and_then(permission_decision_from_label)
        else {
            return false;
        };
        self.submit_permission_by_request_id(request_id, decision)
    }

    fn apply_clarify_control_command(&self, payload: &Value) -> bool {
        let Some(request_id) = payload.get("requestId").and_then(Value::as_str) else {
            return false;
        };
        let result = if payload
            .get("cancel")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            ClarifyResult::Cancelled
        } else {
            let answers = payload
                .get("answers")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .map(|row| {
                            let answers = row
                                .as_array()
                                .map(|values| {
                                    values
                                        .iter()
                                        .filter_map(Value::as_str)
                                        .map(str::to_string)
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();
                            ClarifyAnswer { answers }
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            ClarifyResult::Answered(ClarifyResponse { answers })
        };
        self.control_for_activity_id(request_id)
            .and_then(|control| {
                if control.submit_clarify_result(request_id, result.clone()) {
                    Some(())
                } else {
                    None
                }
            })
            .is_some()
            || self
                .active_control_handles()
                .into_iter()
                .any(|control| control.submit_clarify_result(request_id, result.clone()))
    }

    fn control_for_activity_id(&self, activity_id: &str) -> Option<RunControlHandle> {
        let active = self.active.lock().expect("gateway active map poisoned");
        active.values().find_map(|state| {
            if state.active_turn_id.as_deref() == Some(activity_id) {
                state.control.clone()
            } else {
                None
            }
        })
    }

    fn active_control_handles(&self) -> Vec<RunControlHandle> {
        self.active
            .lock()
            .expect("gateway active map poisoned")
            .values()
            .filter_map(|state| state.control.clone())
            .collect()
    }

    fn submit_permission_by_request_id(
        &self,
        request_id: &str,
        decision: PermissionApprovalDecision,
    ) -> bool {
        self.pending_permissions
            .lock()
            .expect("gateway pending permission map poisoned")
            .remove(request_id)
            .and_then(|pending| pending.responder.send(decision).ok())
            .is_some()
    }
}

fn gateway_activity_view(activity: &GatewayActivity) -> GatewayActivityView {
    GatewayActivityView {
        running: activity.running,
        active_turn_id: activity.active_turn_id.clone(),
        queued_turns: activity.queued_turns,
        started_at_ms: activity.started_at_ms,
        updated_at_ms: activity.updated_at_ms,
        owner_id: activity.owner_id.clone(),
        owner_surface: activity.owner_surface.clone(),
        lease_expires_at_ms: activity.lease_expires_at_ms,
        takeover_state: activity.takeover_state.clone(),
    }
}

fn gateway_event_thread_id(event: &GatewayEvent) -> Option<String> {
    match event {
        GatewayEvent::TurnStarted { thread_id, .. }
        | GatewayEvent::TurnQueued { thread_id, .. }
        | GatewayEvent::ActivityChanged { thread_id, .. } => thread_id.clone(),
        GatewayEvent::TurnCompleted {
            thread_id, turn, ..
        } => thread_id.clone().or_else(|| turn.thread_id.clone()),
        GatewayEvent::EntryStarted { entry, .. }
        | GatewayEvent::EntryUpdated { entry, .. }
        | GatewayEvent::EntryCompleted { entry, .. } => {
            if entry.thread_id.is_empty() {
                None
            } else {
                Some(entry.thread_id.clone())
            }
        }
        GatewayEvent::TitleChanged { thread_id, .. } => Some(thread_id.clone()),
        _ => None,
    }
}

fn gateway_event_turn_id(event: &GatewayEvent) -> Option<&str> {
    match event {
        GatewayEvent::TurnStarted { turn_id, .. }
        | GatewayEvent::TurnQueued { turn_id, .. }
        | GatewayEvent::TurnCompleted { turn_id, .. }
        | GatewayEvent::EntryStarted { turn_id, .. }
        | GatewayEvent::EntryUpdated { turn_id, .. }
        | GatewayEvent::EntryCompleted { turn_id, .. }
        | GatewayEvent::EntryDelta { turn_id, .. } => Some(turn_id.as_str()),
        _ => None,
    }
}

fn permission_decision_label(decision: &PermissionApprovalDecision) -> &'static str {
    match decision.outcome {
        PermissionApprovalOutcome::AllowOnce => "allow_once",
        PermissionApprovalOutcome::AllowSession => "allow_session",
        PermissionApprovalOutcome::AllowAlways => "allow_always",
        PermissionApprovalOutcome::Deny => "deny",
    }
}

fn permission_decision_from_label(label: &str) -> Option<PermissionApprovalDecision> {
    match label {
        "allow_once" => Some(PermissionApprovalDecision::allow_once()),
        "allow_session" => Some(PermissionApprovalDecision::allow_session()),
        "allow_always" => Some(PermissionApprovalDecision::allow_always()),
        "deny" => Some(PermissionApprovalDecision::deny()),
        _ => None,
    }
}
