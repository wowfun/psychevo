const GATEWAY_ACTIVITY_LEASE_MS: i64 = 30_000;
const GATEWAY_ACTIVITY_HEARTBEAT_MS: i64 = 5_000;
const GATEWAY_CONTROL_POLL_MS: u64 = 500;
const GATEWAY_LIVE_SNAPSHOT_FLUSH_MS: i64 = 250;

#[derive(Clone, Debug)]
struct DurableGatewayActivity {
    activity_id: String,
    owner_id: String,
    generation: i64,
    turn_id: Option<String>,
    kind: String,
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

#[derive(Default)]
struct PersistGatewayEventResult {
    accepted_thread_id: Option<String>,
}

impl Gateway {
    async fn claim_durable_gateway_activity(
        &self,
        claim: DurableGatewayActivityClaim<'_>,
    ) -> psychevo::Result<DurableGatewayActivity> {
        let record = self
            .state

            .claim_gateway_activity(GatewayActivityClaimInput {
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
            })
            .await?;
        Ok(DurableGatewayActivity {
            activity_id: record.activity_id,
            owner_id: record.owner_id,
            generation: record.generation,
            turn_id: record.turn_id,
            kind: record.kind,
        })
    }

    fn spawn_durable_activity_heartbeat(
        &self,
        activity: DurableGatewayActivity,
    ) -> oneshot::Sender<()> {
        let (stop_tx, mut stop_rx) = oneshot::channel::<()>();
        let gateway = self.clone();
        self.spawn_accepted_turn(
            format!("activity-heartbeat:{}", activity.activity_id),
            async move {
            let mut tick = tokio::time::interval(Duration::from_millis(GATEWAY_CONTROL_POLL_MS));
            let mut last_heartbeat_ms = 0;
            loop {
                tokio::select! {
                    _ = &mut stop_rx => break,
                    _ = tick.tick() => {
                        let now = gateway_now_ms();
                        if now.saturating_sub(last_heartbeat_ms) >= GATEWAY_ACTIVITY_HEARTBEAT_MS {
                            let lease_expires_at_ms = now + GATEWAY_ACTIVITY_LEASE_MS;
                            let _ = gateway.state.heartbeat_gateway_activity(
                                &activity.activity_id,
                                &activity.owner_id,
                                activity.generation,
                                lease_expires_at_ms,
                            ).await;
                            last_heartbeat_ms = now;
                        }
                        gateway.apply_pending_gateway_control_commands().await;
                    }
                }
            }
            },
        );
        stop_tx
    }

    fn wrap_gateway_event_sink(
        &self,
        event_sink: Option<GatewayEventEmitter>,
        activity: Option<DurableGatewayActivity>,
        queue_key: Option<String>,
        default_turn_id: Option<String>,
    ) -> Option<GatewayEventEmitter> {
        if event_sink.is_none() && activity.is_none() {
            return None;
        }
        let immediate_gateway = self.clone();
        let immediate_event_sink = event_sink.clone();
        let immediate_activity = activity.clone();
        let immediate_queue_key = queue_key.clone();
        let immediate_default_turn_id = default_turn_id.clone();
        let waiting_gateway = self.clone();
        Some(GatewayEventEmitter::try_new_with_wait(
            move |event: GatewayEvent| {
            let local_result = immediate_event_sink
                .as_ref()
                .map(|event_sink| event_sink.emit(event.clone()))
                .transpose();
            let durable_result = immediate_activity
                .as_ref()
                .map(|root_activity| {
                    immediate_gateway.event_ingress.submit(
                        immediate_gateway.clone(),
                        GatewayEventEnvelope {
                            root_activity_id: Some(root_activity.activity_id.clone()),
                            activity: root_activity.clone(),
                            completion: None,
                            default_turn_id: immediate_default_turn_id.clone(),
                            event,
                            queue_key: immediate_queue_key.clone(),
                        },
                    )
                })
                .transpose();
            match (local_result, durable_result) {
                (Err(error), _) | (_, Err(error)) => Err(error),
                (Ok(_), Ok(_)) => Ok(()),
            }
        },
        move |event: GatewayEvent| {
            let gateway = waiting_gateway.clone();
            let event_sink = event_sink.clone();
            let activity = activity.clone();
            let queue_key = queue_key.clone();
            let default_turn_id = default_turn_id.clone();
            Box::pin(async move {
                let local_result = event_sink
                    .as_ref()
                    .map(|event_sink| event_sink.emit(event.clone()))
                    .transpose();
                let durable_result = match activity {
                    Some(root_activity) => gateway
                        .event_ingress
                        .submit_wait(
                            gateway.clone(),
                            GatewayEventEnvelope {
                                root_activity_id: Some(root_activity.activity_id.clone()),
                                activity: root_activity,
                                completion: None,
                                default_turn_id,
                                event,
                                queue_key,
                            },
                        )
                        .await,
                    None => Ok(()),
                };
                match (local_result, durable_result) {
                    (Err(error), _) | (_, Err(error)) => Err(error),
                    (Ok(_), Ok(_)) => Ok(()),
                }
            })
        },
        ))
    }

    async fn persist_gateway_event_envelope(&self, envelope: GatewayEventEnvelope) {
        let effective_activity = self
            .activity_for_gateway_event(Some(&envelope.activity), &envelope.event)
            .await
            .unwrap_or_else(|| envelope.activity.clone());
        let event = self
            .attention_event_with_public_provenance(
                envelope.event,
                Some(&effective_activity),
            )
            .await;
        let accepted_thread_id = self
            .persist_gateway_event(
                &effective_activity,
                &event,
                envelope.default_turn_id.as_deref(),
            )
            .await
            .accepted_thread_id;
        if let Some(thread_id) = accepted_thread_id.as_deref()
            && let Some(queue_key) = envelope.queue_key.as_deref()
            && envelope
                .root_activity_id
                .as_deref()
                .is_some_and(|root| root == effective_activity.activity_id)
        {
            self.register_active_thread_alias(queue_key, thread_id);
        }
    }

    async fn activity_for_gateway_event(
        &self,
        root: Option<&DurableGatewayActivity>,
        event: &GatewayEvent,
    ) -> Option<DurableGatewayActivity> {
        let Some(thread_id) = gateway_event_thread_id(event) else {
            return root.cloned();
        };
        if let Some(root) = root
            && self
                .state
                .gateway_activity(&root.activity_id)
                .await
                .ok()
                .flatten()
                .and_then(|record| record.thread_id)
                .as_deref()
                == Some(thread_id.as_str())
        {
            return Some(root.clone());
        }
        let event_turn_id = gateway_event_turn_id(event);
        let matching_turn = if let Some(turn_id) = event_turn_id {
            self.state.gateway_activity(turn_id).await.ok().flatten()
        } else {
            None
        }
            .filter(|record| {
                record.owner_id == self.owner_id()
                    && record.thread_id.as_deref() == Some(thread_id.as_str())
            });
        let matching_thread = self
            .state
            .active_gateway_activity_for_thread(&thread_id)
            .await
            .ok()
            .flatten()
            .filter(|record| {
                record.owner_id == self.owner_id()
                    && event_turn_id
                        .is_none_or(|turn_id| record.turn_id.as_deref() == Some(turn_id))
            });
        matching_turn
            .or(matching_thread)
            .map(|record| DurableGatewayActivity {
                activity_id: record.activity_id,
                owner_id: record.owner_id,
                generation: record.generation,
                turn_id: record.turn_id,
                kind: record.kind,
            })
            .or_else(|| root.cloned())
    }

    async fn attention_event_with_public_provenance(
        &self,
        event: GatewayEvent,
        activity: Option<&DurableGatewayActivity>,
    ) -> GatewayEvent {
        let (mut action, updated) = match event {
            GatewayEvent::ActionRequested { action } => (action, false),
            GatewayEvent::ActionUpdated { action } => (action, true),
            event => return event,
        };
        let activity_record = if let Some(activity) = activity {
            self.state
                .gateway_activity(&activity.activity_id)
                .await
                .ok()
                .flatten()
        } else {
            None
        };
        let activity_thread_id = activity_record
            .as_ref()
            .and_then(|record| record.thread_id.clone());
        let thread_id = action.thread_id.clone().or(activity_thread_id);
        action.thread_id = action.thread_id.or_else(|| thread_id.clone());
        if let Some(record) = activity_record {
            action.activity_id = action.activity_id.or(Some(record.activity_id));
            action.turn_id = action.turn_id.or(record.turn_id);
            action.source_key = action.source_key.or(record.source_key);
            action.owner_id = action.owner_id.or(Some(record.owner_id));
            action.lease_expires_at_ms = action
                .lease_expires_at_ms
                .or(Some(record.lease_expires_at_ms));
        }
        let binding = if let Some(thread_id) = thread_id.as_deref() {
            self.state
                .gateway_runtime_binding(thread_id)
                .await
                .ok()
                .flatten()
        } else {
            None
        };
        let runtime_ref = binding
            .as_ref()
            .and_then(|binding| binding.runtime_ref.clone())
            .unwrap_or_else(|| "native".to_string());
        let runtime_kind = binding
            .as_ref()
            .and_then(|binding| binding.native_kind.clone())
            .unwrap_or_else(|| "native".to_string());
        let profile_label = binding
            .as_ref()
            .and_then(|binding| binding.profile_config_json.as_deref())
            .and_then(|profile| serde_json::from_str::<Value>(profile).ok())
            .and_then(|profile| {
                profile
                    .get("label")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| {
                if runtime_ref == "native" {
                    "Psychevo".to_string()
                } else {
                    runtime_ref.clone()
                }
            });
        let parent_thread_id = binding
            .as_ref()
            .and_then(|binding| binding.parent_thread_id.clone())
            .or_else(|| thread_id.clone());
        let child_thread_id = binding
            .as_ref()
            .and_then(|binding| binding.parent_thread_id.as_ref())
            .and(thread_id);
        let mut payload = action.payload.as_object().cloned().unwrap_or_default();
        payload
            .entry("runtimeRef".to_string())
            .or_insert_with(|| json!(runtime_ref));
        payload
            .entry("runtimeKind".to_string())
            .or_insert_with(|| json!(runtime_kind));
        payload
            .entry("profileLabel".to_string())
            .or_insert_with(|| json!(profile_label));
        payload.entry("origin".to_string()).or_insert_with(|| {
            json!({
                "parentThreadId": parent_thread_id,
                "childThreadId": child_thread_id,
            })
        });
        action.payload = Value::Object(payload);
        if updated {
            GatewayEvent::ActionUpdated { action }
        } else {
            GatewayEvent::ActionRequested { action }
        }
    }

    async fn persist_gateway_event(
        &self,
        activity: &DurableGatewayActivity,
        event: &GatewayEvent,
        default_turn_id: Option<&str>,
    ) -> PersistGatewayEventResult {
        let mut result = PersistGatewayEventResult::default();
        if let GatewayEvent::TurnStarted {
            thread_id: Some(thread_id),
            ..
        } = event
            && self
                .state

                .update_gateway_activity_thread(
                    &activity.activity_id,
                    &activity.owner_id,
                    activity.generation,
                    thread_id,
                    gateway_now_ms() + GATEWAY_ACTIVITY_LEASE_MS,
                )
                .await
                .unwrap_or(false)
        {
            result.accepted_thread_id = Some(thread_id.clone());
        }

        if matches!(event, GatewayEvent::TurnCompleted { .. }) {
            self.flush_gateway_live_snapshots_for_activity(&activity.activity_id)
                .await;
        }

        let Ok(event_value) = serde_json::to_value(event) else {
            return result;
        };
        if should_append_gateway_live_event(activity, event) {
            let _ = self.state.append_gateway_live_event(
                Some(&activity.activity_id),
                Some(&activity.owner_id),
                gateway_event_thread_id(event).as_deref(),
                gateway_event_turn_id(event)
                    .or(default_turn_id)
                    .or(activity.turn_id.as_deref()),
                &event_value,
            )
            .await;
        } else if let Some((event_kind, entry)) = gateway_live_snapshot_entry(event) {
            self.retain_gateway_live_snapshot(
                activity,
                event_kind,
                gateway_event_turn_id(event)
                    .or(default_turn_id)
                    .or(activity.turn_id.as_deref()),
                entry,
                event_value,
            )
            .await;
        }
        result
    }

    async fn retain_gateway_live_snapshot(
        &self,
        activity: &DurableGatewayActivity,
        event_kind: &'static str,
        turn_id: Option<&str>,
        entry: &TranscriptEntry,
        event: Value,
    ) {
        let Some(turn_id) = turn_id else {
            return;
        };
        if entry.id.trim().is_empty() || entry.thread_id.trim().is_empty() {
            return;
        }
        let snapshot_key = format!("{}:{turn_id}:{}", activity.activity_id, entry.id);
        let now = gateway_now_ms();
        let snapshot = {
            let mut pending = self
                .live_snapshots
                .lock()
                .expect("gateway live snapshot map poisoned");
            let snapshot =
                pending
                    .entry(snapshot_key.clone())
                    .or_insert_with(|| PendingGatewayLiveSnapshot {
                        snapshot_key: snapshot_key.clone(),
                        activity_id: Some(activity.activity_id.clone()),
                        owner_id: Some(activity.owner_id.clone()),
                        thread_id: Some(entry.thread_id.clone()),
                        turn_id: Some(turn_id.to_string()),
                        event_kind: event_kind.to_string(),
                        event: Value::Null,
                        last_flush_ms: 0,
                        dirty: false,
                    });
            snapshot.activity_id = Some(activity.activity_id.clone());
            snapshot.owner_id = Some(activity.owner_id.clone());
            snapshot.thread_id = Some(entry.thread_id.clone());
            snapshot.turn_id = Some(turn_id.to_string());
            snapshot.event_kind = event_kind.to_string();
            snapshot.event = event;
            snapshot.dirty = true;
            if snapshot.last_flush_ms == 0
                || now.saturating_sub(snapshot.last_flush_ms) >= GATEWAY_LIVE_SNAPSHOT_FLUSH_MS
            {
                snapshot.last_flush_ms = now;
                snapshot.dirty = false;
                Some(snapshot.clone())
            } else {
                None
            }
        };
        if let Some(snapshot) = snapshot {
            self.flush_gateway_live_snapshot(&snapshot).await;
        }
    }

    async fn flush_gateway_live_snapshots_for_activity(&self, activity_id: &str) {
        let now = gateway_now_ms();
        let snapshots = {
            let mut pending = self
                .live_snapshots
                .lock()
                .expect("gateway live snapshot map poisoned");
            pending
                .values_mut()
                .filter(|snapshot| {
                    snapshot.activity_id.as_deref() == Some(activity_id) && snapshot.dirty
                })
                .map(|snapshot| {
                    snapshot.last_flush_ms = now;
                    snapshot.dirty = false;
                    snapshot.clone()
                })
                .collect::<Vec<_>>()
        };
        for snapshot in snapshots {
            self.flush_gateway_live_snapshot(&snapshot).await;
        }
    }

    async fn flush_gateway_live_snapshot(&self, snapshot: &PendingGatewayLiveSnapshot) {
        let _ = self
            .state

            .upsert_gateway_live_snapshot(GatewayLiveSnapshotInput {
                snapshot_key: &snapshot.snapshot_key,
                activity_id: snapshot.activity_id.as_deref(),
                owner_id: snapshot.owner_id.as_deref(),
                thread_id: snapshot.thread_id.as_deref(),
                turn_id: snapshot.turn_id.as_deref(),
                event_kind: &snapshot.event_kind,
                event: snapshot.event.clone(),
            })
            .await;
    }

    async fn clear_gateway_live_snapshots_for_activity(&self, activity_id: &str) {
        {
            let mut pending = self
                .live_snapshots
                .lock()
                .expect("gateway live snapshot map poisoned");
            pending.retain(|_, snapshot| snapshot.activity_id.as_deref() != Some(activity_id));
        }
        let _ = self
            .state

            .delete_gateway_live_snapshots_for_activity(activity_id)
            .await;
    }

    async fn finish_durable_gateway_activity(
        &self,
        activity: Option<&DurableGatewayActivity>,
        status: &str,
    ) {
        if let Some(activity) = activity {
            let _ = self.state.finish_gateway_activity(
                &activity.activity_id,
                &activity.owner_id,
                activity.generation,
                status,
            )
            .await;
            self.clear_gateway_live_snapshots_for_activity(&activity.activity_id)
                .await;
        }
    }

    pub async fn takeover_turn(
        &self,
        selector: GatewayThreadSelector,
    ) -> psychevo::Result<(bool, GatewayActivity)> {
        let now = gateway_now_ms();
        for key in self.selector_keys(&selector) {
            let Some(record) = self.durable_activity_for_key(&key).await? else {
                continue;
            };
            if record.owner_id == self.owner_id() {
                return Ok((false, self.activity_for_selector(selector).await));
            }
            if record.status != "running" && record.status != "queued" {
                continue;
            }
            if record.lease_expires_at_ms < now {
                let superseded_by_activity_id = Uuid::now_v7().to_string();
                let accepted = self.state.supersede_stale_gateway_activity(
                    &record.activity_id,
                    &superseded_by_activity_id,
                )
                .await?;
                let mut activity = self.activity_for_selector(selector).await;
                if accepted {
                    activity.takeover_state = Some("takenOver".to_string());
                    self.append_gateway_activity_changed(
                        record.thread_id,
                        &activity,
                        Some(&record.activity_id),
                    )
                    .await;
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
            )
            .await;
            let mut activity = self.activity_for_selector(selector).await;
            if accepted {
                activity.takeover_state = Some("requested".to_string());
            }
            return Ok((accepted, activity));
        }
        Ok((false, self.activity_for_selector(selector).await))
    }

    async fn append_gateway_activity_changed(
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
            let _ = self.state.append_gateway_live_event(
                activity_id,
                Some(self.owner_id()),
                thread_id.as_deref(),
                activity.active_turn_id.as_deref(),
                &event_value,
            )
            .await;
        }
    }

    async fn enqueue_foreign_control_command(
        &self,
        selector: &GatewayThreadSelector,
        command_kind: &str,
        payload: Value,
    ) -> bool {
        let now = gateway_now_ms();
        for key in self.selector_keys(selector) {
            let Ok(Some(record)) = self.durable_activity_for_key(&key).await else {
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

                .enqueue_gateway_control_command(GatewayControlCommandInput {
                    activity_id: &record.activity_id,
                    owner_id: &record.owner_id,
                    command_kind,
                    payload,
                })
                .await
                .is_ok();
        }
        false
    }

    pub(crate) async fn enqueue_exact_foreign_control_command(
        &self,
        activity_id: &str,
        owner_id: &str,
        command_kind: &str,
        payload: Value,
    ) -> bool {
        self.state
            .enqueue_gateway_control_command(GatewayControlCommandInput {
                activity_id,
                owner_id,
                command_kind,
                payload,
            })
            .await
            .is_ok()
    }

    pub(crate) fn interrupt_local_activity(&self, activity_id: &str) -> bool {
        self.control_for_activity_id(activity_id)
            .map(|control| {
                control.abort();
                true
            })
            .unwrap_or(false)
    }

    async fn apply_pending_gateway_control_commands(&self) {
        let Ok(commands) = self
            .state

            .pending_gateway_control_commands(self.owner_id(), 50)
            .await
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
                "takeover" => self
                    .apply_takeover_control_command(&command.activity_id)
                    .await,
                "steer" => self.apply_steer_control_command(&command.activity_id, &command.payload),
                "clarify" => self.apply_clarify_control_command(&command.payload),
                _ => false,
            };
            let store = &self.state;
            let _ = if applied {
                store.mark_gateway_control_command_applied(command.id).await
            } else {
                store
                    .mark_gateway_control_command_failed(command.id, "no matching active control")
                    .await
            };
        }
    }

    async fn apply_takeover_control_command(&self, activity_id: &str) -> bool {
        let control = self.control_for_activity_id(activity_id);
        if let Some(control) = control.as_ref() {
            control.abort();
        }
        let released = if let Some(record) =
            self.state.gateway_activity(activity_id).await.ok().flatten()
        {
            self.state
                .finish_gateway_activity(
                    activity_id,
                    self.owner_id(),
                    record.generation,
                    "released",
                )
                .await
                .unwrap_or(false)
        } else {
            false
        };
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

}

fn gateway_activity_view(activity: &GatewayActivity) -> GatewayActivityView {
    GatewayActivityView {
        activities: activity.activities.clone(),
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
        GatewayEvent::ActionRequested { action } | GatewayEvent::ActionUpdated { action } => {
            action.thread_id.clone()
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
        | GatewayEvent::EntryCompleted { turn_id, .. } => Some(turn_id.as_str()),
        GatewayEvent::ActionRequested { action } | GatewayEvent::ActionUpdated { action } => {
            action.turn_id.as_deref()
        }
        _ => None,
    }
}

fn should_append_gateway_live_event(
    activity: &DurableGatewayActivity,
    event: &GatewayEvent,
) -> bool {
    if let GatewayEvent::TurnCompleted {
        committed_entries, ..
    } = event
        && activity.kind == "turn"
        && committed_entries.is_empty()
    {
        return false;
    }
    matches!(
        event,
        GatewayEvent::TurnStarted { .. }
            | GatewayEvent::TurnQueued { .. }
            | GatewayEvent::TurnCompleted { .. }
            | GatewayEvent::ActionRequested { .. }
            | GatewayEvent::ActionUpdated { .. }
            | GatewayEvent::ActionResolved { .. }
            | GatewayEvent::ActionCancelled { .. }
            | GatewayEvent::Warning { .. }
            | GatewayEvent::ActivityChanged { .. }
            | GatewayEvent::TitleChanged { .. }
    )
}

fn gateway_live_snapshot_entry(event: &GatewayEvent) -> Option<(&'static str, &TranscriptEntry)> {
    match event {
        GatewayEvent::EntryStarted { entry, .. } => Some(("entryStarted", entry)),
        GatewayEvent::EntryUpdated { entry, .. } => Some(("entryUpdated", entry)),
        GatewayEvent::EntryCompleted { entry, .. } => Some(("entryCompleted", entry)),
        _ => None,
    }
}

fn permission_decision_label(decision: &PermissionApprovalDecision) -> &'static str {
    match decision.outcome {
        PermissionApprovalOutcome::AllowOnce => "allow_once",
        PermissionApprovalOutcome::AllowTurn => "allow_turn",
        PermissionApprovalOutcome::AllowSession => "allow_session",
        PermissionApprovalOutcome::AllowAlways => "allow_always",
        PermissionApprovalOutcome::Deny => "deny",
    }
}

#[cfg(test)]
mod event_ingress_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use psychevo::__product::persistence::StateRuntime;

    use super::*;

    #[tokio::test]
    async fn local_delivery_precedes_bounded_durability_and_survives_closed_ingress() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let state = StateRuntime::open(temp.path().join("state.db"))
            .await
            .expect("state");
        let thread_id = state
            .create_session_with_metadata(&cwd, "test", "model", "provider", None)
            .await
            .expect("thread");
        let gateway = Gateway::new(state.clone());
        let activity = gateway
            .claim_durable_gateway_activity(DurableGatewayActivityClaim {
                activity_id: "turn-ingress",
                thread_id: Some(&thread_id),
                source_key: None,
                turn_id: Some("turn-ingress"),
                kind: "turn",
                owner_surface: Some("test"),
                queued_turns: 0,
                intent: None,
            })
            .await
            .expect("activity");

        let local_deliveries = Arc::new(AtomicUsize::new(0));
        let local_deliveries_for_sink = local_deliveries.clone();
        let local_sink = GatewayEventEmitter::new(move |_| {
            local_deliveries_for_sink.fetch_add(1, Ordering::SeqCst);
        });
        let sink = gateway
            .wrap_gateway_event_sink(
                Some(local_sink),
                Some(activity),
                Some("thread:test".to_string()),
                Some("turn-ingress".to_string()),
            )
            .expect("wrapped sink");

        sink.emit(GatewayEvent::TurnStarted {
            thread_id: Some(thread_id.clone()),
            turn_id: "turn-ingress".to_string(),
            selected_skills: Vec::new(),
        })
        .expect("accepted ingress event");
        assert_eq!(
            local_deliveries.load(Ordering::SeqCst),
            1,
            "local observers must run before asynchronous durable relay completion"
        );

        gateway.wait_for_gateway_events().await;
        let durable_events = state
            .list_gateway_live_events_after(0, 10)
            .await
            .expect("durable events");
        assert_eq!(durable_events.len(), 1);
        let diagnostics = gateway.event_ingress.diagnostics();
        assert_eq!(diagnostics.accepted, 1);
        assert_eq!(diagnostics.completed, 1);
        assert_eq!(diagnostics.rejected, 0);

        sink.emit_wait(GatewayEvent::TurnCompleted {
            thread_id: Some(thread_id.clone()),
            turn_id: "turn-ingress".to_string(),
            turn: GatewayTurn {
                id: "turn-ingress".to_string(),
                thread_id: Some(thread_id.clone()),
                status: GatewayTurnStatus::Completed,
                outcome: Some("completed".to_string()),
                error: None,
                started_at_ms: Some(1),
                completed_at_ms: Some(2),
            },
            committed_entries: Vec::new(),
        })
        .await
        .expect("terminal durability flush");
        let diagnostics = gateway.event_ingress.diagnostics();
        assert_eq!(diagnostics.accepted, 2);
        assert_eq!(
            diagnostics.completed, 2,
            "emit_wait must return only after its terminal envelope is processed"
        );

        gateway.event_ingress.close();
        let error = sink
            .emit(GatewayEvent::TurnQueued {
                thread_id: Some(thread_id),
                turn_id: "turn-ingress-queued".to_string(),
                queue_position: 1,
            })
            .expect_err("closed durable ingress");
        assert!(error.to_string().contains("ingress is closed"));
        assert_eq!(
            local_deliveries.load(Ordering::SeqCst),
            3,
            "durable relay rejection must not suppress local delivery"
        );
        assert_eq!(gateway.event_ingress.diagnostics().rejected, 1);

        gateway
            .shutdown_application(false)
            .await
            .expect("shutdown gateway");
        state.close().await;
    }
}
