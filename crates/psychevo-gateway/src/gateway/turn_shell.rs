struct TerminalTurnInput<'a> {
    thread_id: Option<&'a str>,
    turn_id: &'a str,
    status: GatewayTurnStatus,
    outcome: Option<&'a str>,
    error_message: Option<&'a str>,
    error_data: Option<&'a Value>,
    classified_error: Option<&'a GatewayTurnError>,
    first_committed_seq: Option<i64>,
    last_committed_seq: Option<i64>,
    durable_activity: Option<&'a DurableGatewayActivity>,
}

struct AutoCompactionAfterTurn<'a> {
    result: &'a RunResult,
    config_path: Option<PathBuf>,
    model: Option<String>,
    reasoning_effort: Option<String>,
    inherited_env: Option<BTreeMap<String, String>>,
    event_sink: Option<&'a GatewayEventSink>,
    turn_id: &'a str,
}

impl Gateway {
    async fn run_turn_now(
        &self,
        queue_key: &str,
        request: SendTurnRequest,
        turn_id: String,
    ) -> psychevo_runtime::Result<GatewayTurnResult> {
        let thread_hint = request
            .thread_id
            .clone()
            .or_else(|| request.options.session.clone());
        let event_sink = request.event_sink.clone();
        let result = self
            .run_turn_now_inner(queue_key, request, turn_id.clone())
            .await;
        if let Err(error) = &result
            && let Err(terminal_error) = self.ensure_failed_terminal_after_turn_error(
                &turn_id,
                thread_hint.as_deref(),
                event_sink.as_ref(),
                error,
            )
        {
            return Err(Error::Message(format!(
                "{error}; failed to persist the accepted turn terminal: {terminal_error}"
            )));
        }
        if result.is_err()
            && let Err(delivery_error) = self.state.store().finish_gateway_turn_delivery(&turn_id)
        {
            return Err(Error::Message(format!(
                "{}; failed to finalize the accepted turn delivery ledger: {delivery_error}",
                result.as_ref().expect_err("result is an error")
            )));
        }
        result
    }

    async fn run_turn_now_inner(
        &self,
        queue_key: &str,
        request: SendTurnRequest,
        turn_id: String,
    ) -> psychevo_runtime::Result<GatewayTurnResult> {
        let base_event_sink = request.event_sink.clone();
        let queue_source = request.source.clone();
        let alias_source_to_active = request.thread_id.is_none();
        let bind_source = request.bind_source.clone().or_else(|| queue_source.clone());
        let bind_source_generation = bind_source
            .as_ref()
            .map(|source| self.source_generation(source));
        let queue_source_generation = queue_source
            .as_ref()
            .map(|source| self.source_generation(source));
        let source_name = request
            .runtime_source
            .clone()
            .unwrap_or_else(|| "gateway".to_string());
        let continue_sources = if request.continue_sources.is_empty() {
            vec![source_name.clone()]
        } else {
            request.continue_sources.clone()
        };
        let input = request.input.clone();
        let initial_thread_preferences = request.initial_thread_preferences.clone();
        let delivery_input = gateway_delivery_input_parts(&request);
        let delivery_input_json = serde_json::to_string(&delivery_input)?;
        let delivery_input_hash = format!("{:x}", Sha256::digest(delivery_input_json.as_bytes()));
        let mut options = request.options;
        options.state = self.state.clone();
        apply_input_parts(&mut options, &request.input)?;

        let mapped_thread_id = if !request.reset_source_binding
            && let Some(source) = &request.source
        {
            self.lookup_source_thread(source)?
        } else {
            None
        };
        let mut active_thread_id = request
            .thread_id
            .clone()
            .or(mapped_thread_id)
            .or_else(|| options.session.clone());
        if active_thread_id.is_none() {
            options.cwd = psychevo_runtime::canonicalize_cwd(&options.cwd)?;
            if options.continue_latest {
                let continue_source_refs = continue_sources
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>();
                active_thread_id = self
                    .state
                    .store()
                    .latest_session_for_cwd_with_sources(&options.cwd, &continue_source_refs)?;
            }
        }
        if active_thread_id.is_none() {
            active_thread_id = Some(self.state.store().create_session_with_metadata(
                &options.cwd,
                &source_name,
                "pending",
                "pending",
                None,
            )?);
        }
        gateway_profile_mark(
            "gateway_thread_materialized",
            Some(&turn_id),
            active_thread_id.as_deref(),
            GatewayProfileFields {
                runtime_source: Some(&source_name),
                ..GatewayProfileFields::default()
            },
        );
        if let Some(thread_id) = active_thread_id.clone() {
            options.cwd = self.thread_cwd(&thread_id)?;
            options.session = Some(thread_id);
            options.continue_latest = false;
            if options.runtime_ref.is_none()
                && let Some(binding) = self
                    .state
                    .store()
                    .gateway_runtime_binding(options.session.as_deref().expect("active thread"))?
            {
                options.runtime_ref = binding.runtime_ref;
            }
        }
        if options.runtime_ref.is_none()
            && let Some(source) = request.source.as_ref().or(bind_source.as_ref())
            && source.lifetime == GatewaySourceLifetime::Persistent
            && let Some(lane) = self
                .state
                .store()
                .gateway_source_lane(&source.source_key().0)?
        {
            options.runtime_ref = lane.draft_profile_ref;
        }
        let durable_source_key = if request.thread_id.is_some() {
            None
        } else {
            queue_source
                .as_ref()
                .or(bind_source.as_ref())
                .map(|source| source.source_key().0)
        };
        let draft_source_key = queue_source
            .as_ref()
            .or(bind_source.as_ref())
            .map(|source| source.source_key().0);
        let first_committed_seq = active_thread_id
            .as_deref()
            .and_then(|thread_id| {
                self.state
                    .store()
                    .load_tui_message_summaries(thread_id)
                    .ok()
            })
            .and_then(|summaries| summaries.last().map(|summary| summary.session_seq + 1))
            .unwrap_or(1);
        let durable_intent = json!({
            "kind": "turn",
            "threadId": active_thread_id.clone(),
            "sourceKey": durable_source_key.clone(),
            "runtimeSource": source_name.clone(),
            "firstCommittedSeq": first_committed_seq,
            "cwd": options.cwd.to_string_lossy(),
            "input": request.input,
        });
        let durable_activity = Some(self.claim_durable_gateway_activity(
            DurableGatewayActivityClaim {
                activity_id: &turn_id,
                thread_id: active_thread_id.as_deref(),
                source_key: durable_source_key.as_deref(),
                turn_id: Some(&turn_id),
                kind: "turn",
                owner_surface: Some(&source_name),
                queued_turns: 0,
                intent: Some(durable_intent),
            },
        )?);
        let _heartbeat = durable_activity
            .clone()
            .map(|activity| self.spawn_durable_activity_heartbeat(activity));
        let durable_event_sink = self.wrap_gateway_event_sink(
            base_event_sink,
            durable_activity.clone(),
            Some(queue_key.to_string()),
            Some(turn_id.clone()),
        );
        let lifecycle = GatewayTurnLifecycle::new(
            turn_id.clone(),
            active_thread_id.clone(),
            durable_event_sink,
        );
        let event_sink = Some(lifecycle.sink());
        let event_sink_for_completion = event_sink.clone();

        let (control_handle, control) = match request.control {
            Some(control) => (request.control_handle.clone(), Some(control)),
            None if options.clarify_enabled => {
                let (handle, control) = run_control();
                (Some(handle), Some(control))
            }
            None => (None, None),
        };

        self.register_active(
            queue_key,
            turn_id.clone(),
            control_handle,
            ActiveActivityKind::Turn,
        );
        gateway_profile_mark(
            "gateway_event_emitted",
            Some(&turn_id),
            active_thread_id.as_deref(),
            GatewayProfileFields {
                event_type: Some("turnStarted"),
                ..GatewayProfileFields::default()
            },
        );
        lifecycle.start();
        if alias_source_to_active
            && let Some(source) = queue_source.as_ref().or(bind_source.as_ref())
        {
            self.register_active_queue_alias(&source_key_key(&source.source_key()), queue_key);
        }

        let stream = wrap_stream(
            request.stream,
            event_sink,
            turn_id.clone(),
            active_thread_id.clone(),
        );
        let result_lineage = request.lineage.clone();
        let auto_config_path = options.config_path.clone();
        let auto_model = options.model.clone();
        let auto_reasoning_effort = options.reasoning_effort.clone();
        let auto_inherited_env = options.inherited_env.clone();
        // Everything after the durable claim is funneled through one result so configuration,
        // binding, and adapter failures all produce the same single terminal lifecycle.
        let backend_result: psychevo_runtime::Result<(RunResult, GatewayBackendInfo)> = async {
            let bound_target = resolve_bound_gateway_agent_target(
                &options,
                options.runtime_ref.as_deref(),
            )?;
            let (profile_config, profile_revision, profile_fingerprint) = match bound_target.as_ref()
            {
                Some(target) => (
                    target.profile.clone(),
                    target.revision,
                    target.fingerprint.clone(),
                ),
                None => resolve_gateway_runtime_profile(&options)?,
            };
            options.runtime_ref = Some(profile_config.id.clone());
            let existing_binding = match bound_target.as_ref() {
                Some(target) => Some(target.binding.clone()),
                None => self.state.store().gateway_runtime_binding(
                    active_thread_id.as_deref().expect("gateway thread exists"),
                )?,
            };
            let agent_binding = resolve_gateway_agent_binding_snapshot(
                &options,
                &profile_config,
                existing_binding.as_ref(),
                AgentEntrypoint::Peer,
            )?;
            options.agent = agent_binding.agent_ref.clone();
            if options.approval_handler.is_none()
                && let Some(event_sink) = event_sink_for_completion.clone()
            {
                let session_authorization_lifetime = (profile_config.runtime
                    == RuntimeProfileKind::Native)
                    .then_some("psychevo_session");
                options.approval_handler = Some(Arc::new(GatewayApprovalHandler::new(
                    Some(queue_key.to_string()),
                    self.pending_permissions.clone(),
                    event_sink,
                    session_authorization_lifetime,
                )));
            }
            let mut binding = ensure_gateway_runtime_binding(
                &self.state,
                active_thread_id.as_deref().expect("gateway thread exists"),
                &agent_binding,
                &profile_config,
                profile_revision,
                &profile_fingerprint,
            )?;
            if existing_binding.is_none() && !initial_thread_preferences.is_empty() {
                let preferences = initial_thread_preferences
                    .iter()
                    .map(|(control_id, value)| {
                        (control_id.clone(), Value::String(value.clone()))
                    })
                    .collect::<BTreeMap<_, _>>();
                binding = self
                    .state
                    .store()
                    .compare_and_set_gateway_runtime_control_state(
                        &binding.thread_id,
                        binding.binding_revision,
                        binding.control_revision,
                        GatewayRuntimeControlStatePatch {
                            thread_preferences: Some(&preferences),
                            runtime_observed: None,
                        },
                    )?;
            }
            if existing_binding.is_none()
                && profile_config.runtime == RuntimeProfileKind::Acp
                && let Some(source_key) = draft_source_key.as_deref()
                && let Some(native_session_id) = self
                    .agent_sessions
                    .promote_prepared(
                        source_key,
                        binding.agent_ref.as_deref(),
                        &profile_config.id,
                        &profile_fingerprint,
                        &binding.thread_id,
                    )
                    .await?
            {
                self.state.store().attach_gateway_runtime_native_session(
                    &binding.thread_id,
                    binding.binding_revision,
                    &native_session_id,
                )?;
                options.runtime_session_id = Some(native_session_id);
                binding = self
                    .state
                    .store()
                    .gateway_runtime_binding(&binding.thread_id)?
                    .ok_or_else(|| {
                        agent_session_configuration_error(
                            "Promoted ACP draft lost its immutable Thread binding.",
                        )
                    })?;
            }
            self.state
                .store()
                .insert_gateway_turn_delivery(GatewayTurnDeliveryInput {
                    turn_id: &turn_id,
                    thread_id: &binding.thread_id,
                    runtime_ref: &profile_config.id,
                    input_json: &delivery_input_json,
                    input_hash: &delivery_input_hash,
                })?;
            let peer = if let Some(target) = bound_target {
                target.peer
            } else if profile_config.runtime == RuntimeProfileKind::Acp {
                let mut peer_options = options.clone();
                peer_options.runtime_ref = profile_config.backend_ref.clone();
                resolve_peer_turn(&peer_options)?
            } else {
                None
            };
            if peer.is_none()
                && let Some(thread_id) = active_thread_id.as_deref()
            {
                clear_acp_peer_usage_update(&self.state, thread_id)?;
            }
            if peer.is_none() {
                options.external_agent_delegate = Some(Arc::new(GatewayExternalAgentDelegate {
                    gateway: self.clone(),
                    base_options: options.clone(),
                    stream: stream.clone(),
                    event_sink: event_sink_for_completion.clone(),
                }));
            }
            let backend_request = BackendTurnRequest {
                options,
                input,
                runtime_source: source_name,
                continue_sources,
                stream,
                control,
            };
            let attached = self
                .agent_sessions
                .attach(CapturedAgentSessionTarget::bound(
                    &binding,
                    profile_config.clone(),
                    peer,
                )?)?;
            let session_ready = (profile_config.runtime == RuntimeProfileKind::Acp)
                .then(|| acp_session_ready_for_binding(self.state.clone(), binding));
            let output = attached
                .transact(AgentSessionCommand::SubmitTurn {
                    request: Box::new(backend_request),
                    turn_id: turn_id.clone(),
                    session_ready,
                })
                .await?
                .into_turn()?;
            Ok((output.run, output.backend))
        }
        .await;
        let (result, backend_info) = match backend_result {
            Ok(value) => value,
            Err(err) => {
                let thread_id = active_thread_id.clone().or_else(|| {
                    durable_activity.as_ref().and_then(|activity| {
                        self.state
                            .store()
                            .gateway_activity(&activity.activity_id)
                            .ok()
                            .flatten()
                            .and_then(|record| record.thread_id)
                    })
                });
                let error_message = err.to_string();
                let error_data = err.structured_data().cloned();
                let last_committed_seq = thread_id
                    .as_deref()
                    .and_then(|thread_id| {
                        self.state
                            .store()
                            .load_tui_message_summaries(thread_id)
                            .ok()
                    })
                    .and_then(|summaries| {
                        summaries
                            .iter()
                            .rev()
                            .find(|summary| summary.session_seq >= first_committed_seq)
                            .map(|summary| summary.session_seq)
                    })
                    .unwrap_or_else(|| first_committed_seq.saturating_sub(1));
                let turn = self.record_and_project_terminal_turn(TerminalTurnInput {
                    thread_id: thread_id.as_deref(),
                    turn_id: &turn_id,
                    status: GatewayTurnStatus::Failed,
                    outcome: None,
                    error_message: Some(error_message.as_str()),
                    error_data: error_data.as_ref(),
                    classified_error: None,
                    first_committed_seq: Some(first_committed_seq),
                    last_committed_seq: Some(last_committed_seq),
                    durable_activity: durable_activity.as_ref(),
                })?;
                let committed_entries = self.project_terminal_entry_for_turn(&turn_id);
                if let Some(event_sink) = event_sink_for_completion {
                    event_sink(GatewayEvent::TurnCompleted {
                        thread_id,
                        turn_id: turn_id.clone(),
                        turn,
                        committed_entries,
                    });
                }
                self.finish_durable_gateway_activity(durable_activity.as_ref(), "failed");
                return Err(err);
            }
        };
        let mut auto_compaction = None;
        if backend_info.kind == BackendKind::Native
            && backend_info
                .runtime_ref
                .as_deref()
                .is_none_or(|runtime_ref| runtime_ref == "native")
        {
            match self
                .maybe_auto_compact_after_turn(AutoCompactionAfterTurn {
                    result: &result,
                    config_path: auto_config_path,
                    model: auto_model,
                    reasoning_effort: auto_reasoning_effort,
                    inherited_env: auto_inherited_env,
                    event_sink: event_sink_for_completion.as_ref(),
                    turn_id: &turn_id,
                })
                .await
            {
                Ok(result) => auto_compaction = result,
                Err(err) => {
                    if let Some(event_sink) = event_sink_for_completion.as_ref() {
                        event_sink(GatewayEvent::Warning {
                            kind: "compaction_failed".to_string(),
                            message: err.to_string(),
                            source_path: None,
                            suggestion: Some(
                                "Run /compact again after checking the compression model."
                                    .to_string(),
                            ),
                        });
                    }
                }
            }
        }
        let summaries = self
            .state
            .store()
            .load_tui_message_summaries(&result.session_id)?;
        let turn_status = gateway_turn_status_for_outcome(result.outcome);
        let terminal_message = terminal_message_for_result(&result);
        let classified_terminal_error = classified_terminal_error_for_result(&result);
        let last_committed_seq = summaries
            .iter()
            .rev()
            .find(|summary| summary.session_seq >= first_committed_seq)
            .map(|summary| summary.session_seq)
            .unwrap_or_else(|| first_committed_seq.saturating_sub(1));
        let turn = self.record_and_project_terminal_turn(TerminalTurnInput {
            thread_id: Some(&result.session_id),
            turn_id: &turn_id,
            status: turn_status,
            outcome: Some(result.outcome.as_str()),
            error_message: terminal_message.as_deref(),
            error_data: None,
            classified_error: classified_terminal_error.as_ref(),
            first_committed_seq: Some(first_committed_seq),
            last_committed_seq: Some(last_committed_seq),
            durable_activity: durable_activity.as_ref(),
        })?;
        self.state.store().finish_gateway_turn_delivery(&turn_id)?;
        let mut committed_entries = transcript::project_committed_turn_window_entries(
            &result.session_id,
            &summaries,
            transcript::TurnProjectionWindow {
                turn_id: &turn_id,
                first_committed_seq,
            },
        );
        let agent_edges = self
            .state
            .store()
            .list_agent_edges_for_parent(&result.session_id)?;
        transcript::enrich_agent_blocks_from_edges(&mut committed_entries, &agent_edges);
        if let Some(checkpoint_id) = auto_compaction
            .as_ref()
            .filter(|result| result.compacted)
            .and_then(|result| result.checkpoint_id)
            && let Some(record) = self.state.store().session_compaction(checkpoint_id)?
            && record.session_id == result.session_id
        {
            committed_entries.extend(transcript::project_compaction_entries(
                &result.session_id,
                &[record],
            ));
        }
        committed_entries.extend(self.project_terminal_entry_for_turn(&turn_id));
        self.finish_durable_gateway_activity(
            durable_activity.as_ref(),
            durable_activity_status_for_turn(turn_status),
        );
        if let Some(event_sink) = event_sink_for_completion {
            gateway_profile_mark(
                "gateway_turn_completed",
                Some(&turn_id),
                Some(&result.session_id),
                GatewayProfileFields::default(),
            );
            event_sink(GatewayEvent::TurnCompleted {
                thread_id: Some(result.session_id.clone()),
                turn_id: turn_id.clone(),
                turn: turn.clone(),
                committed_entries: committed_entries.clone(),
            });
            if let Ok(Some(summary)) = self.state.store().session_summary(&result.session_id) {
                event_sink(GatewayEvent::TitleChanged {
                    thread_id: result.session_id.clone(),
                    title: summary.title.clone(),
                    display_title: summary.title,
                });
            }
        }
        if let Some(source) = &bind_source {
            self.bind_source_to_result(
                source,
                &result,
                &backend_info,
                result_lineage,
                bind_source_generation,
            )?;
        }
        if let Some(source) = &queue_source
            && bind_source
                .as_ref()
                .is_none_or(|bind_source| bind_source.source_key() != source.source_key())
        {
            self.bind_source_to_result(
                source,
                &result,
                &backend_info,
                None,
                queue_source_generation,
            )?;
        }

        Ok(GatewayTurnResult {
            thread: GatewayThread {
                id: result.session_id.clone(),
                backend: backend_info,
                source_key: bind_source.as_ref().map(GatewaySource::source_key),
                forked_from_thread_id: None,
            },
            turn: GatewayTurn {
                id: turn_id,
                thread_id: Some(result.session_id.clone()),
                status: turn_status,
                outcome: Some(result.outcome.as_str().to_string()),
                error: turn.error.clone(),
                started_at_ms: turn.started_at_ms,
                completed_at_ms: turn.completed_at_ms,
            },
            result,
            committed_entries,
        })
    }

    fn ensure_failed_terminal_after_turn_error(
        &self,
        turn_id: &str,
        thread_hint: Option<&str>,
        event_sink: Option<&GatewayEventSink>,
        error: &Error,
    ) -> psychevo_runtime::Result<()> {
        if let Some(terminal) = self.state.store().gateway_turn_terminal(turn_id)? {
            self.mark_active_turn_terminal(turn_id);
            if let Some(activity) = self.state.store().gateway_activity(turn_id)? {
                self.finish_durable_gateway_activity(
                    Some(&DurableGatewayActivity {
                        activity_id: activity.activity_id,
                        owner_id: activity.owner_id,
                        generation: activity.generation,
                        turn_id: activity.turn_id,
                        kind: activity.kind,
                    }),
                    &terminal.status,
                );
            }
            return Ok(());
        }
        let activity_record = self.state.store().gateway_activity(turn_id)?;
        let thread_id = thread_hint.map(str::to_string).or_else(|| {
            activity_record
                .as_ref()
                .and_then(|record| record.thread_id.clone())
        });
        let Some(thread_id) = thread_id else {
            // Direct callers that failed before materializing a thread were never asynchronously
            // accepted by a user surface, so there is no durable transcript to attach to.
            return Ok(());
        };
        let durable_activity = activity_record.map(|record| DurableGatewayActivity {
            activity_id: record.activity_id,
            owner_id: record.owner_id,
            generation: record.generation,
            turn_id: record.turn_id,
            kind: record.kind,
        });
        let message = error.to_string();
        let error_data = error.structured_data().cloned();
        let turn = self.record_and_project_terminal_turn(TerminalTurnInput {
            thread_id: Some(&thread_id),
            turn_id,
            status: GatewayTurnStatus::Failed,
            outcome: None,
            error_message: Some(&message),
            error_data: error_data.as_ref(),
            classified_error: None,
            first_committed_seq: None,
            last_committed_seq: None,
            durable_activity: durable_activity.as_ref(),
        })?;
        let committed_entries = self.project_terminal_entry_for_turn(turn_id);
        self.finish_durable_gateway_activity(durable_activity.as_ref(), "failed");
        if let Some(event_sink) = event_sink {
            event_sink(GatewayEvent::TurnCompleted {
                thread_id: Some(thread_id),
                turn_id: turn_id.to_string(),
                turn,
                committed_entries,
            });
        }
        Ok(())
    }

    fn record_and_project_terminal_turn(
        &self,
        input: TerminalTurnInput<'_>,
    ) -> psychevo_runtime::Result<GatewayTurn> {
        let completed_at_ms = gateway_now_ms();
        let started_at_ms = input
            .durable_activity
            .and_then(|activity| persisted_gateway_activity(&self.state, activity))
            .map(|record| record.started_at_ms);
        let turn = GatewayTurn {
            id: input.turn_id.to_string(),
            thread_id: input.thread_id.map(str::to_string),
            status: input.status,
            outcome: input.outcome.map(str::to_string),
            error: input.classified_error.cloned().or_else(|| {
                input
                    .error_message
                    .filter(|message| !message.trim().is_empty())
                    .map(|message| gateway_turn_error(message, input.error_data))
            }),
            started_at_ms,
            completed_at_ms: Some(completed_at_ms),
        };
        if let Some(thread_id) = input.thread_id {
            self.state
                .store()
                .upsert_gateway_turn_terminal(GatewayTurnTerminalInput {
                    turn_id: input.turn_id,
                    thread_id,
                    status: gateway_turn_status_name(input.status),
                    outcome: input.outcome,
                    error_message: input.error_message,
                    started_at_ms,
                    completed_at_ms,
                    metadata: Some(json!({
                        "source": "gateway",
                        "error": turn.error.clone(),
                        "firstCommittedSeq": input.first_committed_seq,
                        "lastCommittedSeq": input.last_committed_seq,
                    })),
                })?;
        }
        self.mark_active_turn_terminal(input.turn_id);
        Ok(turn)
    }

    fn project_terminal_entry_for_turn(&self, turn_id: &str) -> Vec<TranscriptEntry> {
        self.state
            .store()
            .gateway_turn_terminal(turn_id)
            .ok()
            .flatten()
            .map(|terminal| transcript::project_turn_terminal_entries(&[terminal]))
            .unwrap_or_default()
    }

    async fn maybe_auto_compact_after_turn(
        &self,
        input: AutoCompactionAfterTurn<'_>,
    ) -> psychevo_runtime::Result<Option<psychevo_runtime::CompactionResult>> {
        let AutoCompactionAfterTurn {
            result,
            config_path,
            model,
            reasoning_effort,
            inherited_env,
            event_sink,
            turn_id,
        } = input;
        let Some(snapshot) = result.context_snapshot.as_ref() else {
            return Ok(None);
        };
        let check = psychevo_runtime::AutoCompactionCheckOptions {
            state: self.state.clone(),
            cwd: result.cwd.clone(),
            session: result.session_id.clone(),
            config_path: config_path.clone(),
            model: model.clone(),
            reasoning_effort: reasoning_effort.clone(),
            inherited_env: inherited_env.clone(),
        };
        if !psychevo_runtime::auto_compaction_due_for_snapshot(&check, snapshot)? {
            return Ok(None);
        }
        let started_at_ms = gateway_now_ms();
        if let Some(event_sink) = event_sink {
            event_sink(GatewayEvent::EntryStarted {
                turn_id: turn_id.to_string(),
                entry: transcript::transient_compaction_entry(
                    &result.session_id,
                    turn_id,
                    TranscriptBlockStatus::Running,
                    started_at_ms,
                    started_at_ms,
                ),
            });
        }
        let result = psychevo_runtime::compact_session(psychevo_runtime::CompactSessionOptions {
            state: self.state.clone(),
            cwd: result.cwd.clone(),
            session: result.session_id.clone(),
            config_path,
            model,
            reasoning_effort,
            inherited_env,
            reason: psychevo_runtime::CompactionReason::AutoThreshold,
            instructions: None,
            force: false,
        })
        .await;
        let completed_at_ms = gateway_now_ms();
        if let Some(event_sink) = event_sink {
            event_sink(GatewayEvent::EntryCompleted {
                turn_id: turn_id.to_string(),
                entry: transcript::transient_compaction_entry(
                    &check.session,
                    turn_id,
                    if result.is_ok() {
                        TranscriptBlockStatus::Completed
                    } else {
                        TranscriptBlockStatus::Failed
                    },
                    started_at_ms,
                    completed_at_ms,
                ),
            });
        }
        Ok(Some(result?))
    }

    async fn run_compact_now(
        &self,
        queue_key: &str,
        request: SendCompactRequest,
        compact_id: String,
    ) -> psychevo_runtime::Result<psychevo_runtime::CompactionResult> {
        let thread_id = match request.thread_id.clone() {
            Some(thread_id) => thread_id,
            None => {
                let source = request.source.as_ref().ok_or_else(|| {
                    Error::Message("no thread is bound for compaction".to_string())
                })?;
                self.lookup_source_thread(source)?.ok_or_else(|| {
                    Error::Message("no thread is bound for compaction".to_string())
                })?
            }
        };
        let bound_profile = resolve_bound_gateway_runtime_profile(&self.state, &thread_id, None)?;
        if let Some(bound) = bound_profile.as_ref()
            && let Some(requested) = request
                .runtime_ref
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty() && *value != "native")
            && requested != bound.profile.id
        {
            return Err(agent_session_error(
                "immutable_binding",
                AgentErrorStage::Binding,
                "user_action",
                "not_delivered",
                format!(
                    "Thread `{thread_id}` is bound to Runtime Profile `{}`; compaction cannot use `{requested}`.",
                    bound.profile.id
                ),
                Some(format!("agent-binding:{thread_id}")),
            ));
        }
        let legacy_non_native_runtime = if bound_profile.is_none() {
            self.non_native_compaction_runtime(&request, &thread_id)?
        } else {
            None
        };
        let cwd = self
            .thread_cwd(&thread_id)
            .unwrap_or_else(|_| request.cwd.clone());
        let source_key = request.source.as_ref().map(|source| source.source_key().0);
        let durable_activity = Some(self.claim_durable_gateway_activity(
            DurableGatewayActivityClaim {
                activity_id: &compact_id,
                thread_id: Some(&thread_id),
                source_key: source_key.as_deref(),
                turn_id: Some(&compact_id),
                kind: "compaction",
                owner_surface: Some("gateway.compaction"),
                queued_turns: 0,
                intent: Some(json!({
                    "kind": "compaction",
                    "threadId": thread_id,
                    "sourceKey": source_key,
                    "cwd": cwd.to_string_lossy(),
                    "reason": request.reason.as_str(),
                })),
            },
        )?);
        let _heartbeat = durable_activity
            .clone()
            .map(|activity| self.spawn_durable_activity_heartbeat(activity));
        self.register_active(queue_key, compact_id, None, ActiveActivityKind::Compact);
        if request.thread_id.is_none()
            && let Some(source) = &request.source
        {
            self.register_active_queue_alias(&source_key_key(&source.source_key()), queue_key);
        }
        if let Some(event_sink) = request.event_sink.as_ref() {
            event_sink(GatewayEvent::ActivityChanged {
                thread_id: Some(thread_id.clone()),
                activity: gateway_activity_view(
                    &self.activity_for_selector(GatewayThreadSelector::thread_id(&thread_id)),
                ),
            });
        }

        let result = match bound_profile {
            Some(bound) if bound.profile.runtime == RuntimeProfileKind::Acp => Ok(
                unavailable_compaction_result(&thread_id, request.reason, &bound.profile.id),
            ),
            None if legacy_non_native_runtime.is_some() => Ok(unavailable_compaction_result(
                &thread_id,
                request.reason,
                legacy_non_native_runtime
                    .as_deref()
                    .expect("checked legacy runtime identity"),
            )),
            _ => {
                psychevo_runtime::compact_session(psychevo_runtime::CompactSessionOptions {
                    state: self.state.clone(),
                    cwd,
                    session: thread_id,
                    config_path: request.config_path,
                    model: request.model,
                    reasoning_effort: request.reasoning_effort,
                    inherited_env: request.inherited_env,
                    reason: request.reason,
                    instructions: request.instructions,
                    force: request.force,
                })
                .await
            }
        };
        self.finish_durable_gateway_activity(
            durable_activity.as_ref(),
            if result.is_ok() {
                "completed"
            } else {
                "failed"
            },
        );
        result
    }

    async fn run_shell_now(
        &self,
        queue_key: &str,
        request: SendShellRequest,
        shell_id: String,
    ) -> psychevo_runtime::Result<GatewayShellResult> {
        let (control_handle, control) = run_control();
        self.register_active(
            queue_key,
            shell_id.clone(),
            Some(control_handle),
            ActiveActivityKind::Shell,
        );
        if request.thread_id.is_none()
            && let Some(source) = &request.source
        {
            self.register_active_queue_alias(&source_key_key(&source.source_key()), queue_key);
        }
        self.run_shell_with_control(request, shell_id, control, None, Some(queue_key))
            .await
    }

    async fn run_shell_auxiliary(
        &self,
        request: SendShellRequest,
        shell_id: String,
        inject_into: RunControlHandle,
    ) -> psychevo_runtime::Result<GatewayShellResult> {
        let (_control_handle, control) = run_control();
        self.run_shell_with_control(request, shell_id, control, Some(inject_into), None)
            .await
    }

    async fn run_shell_with_control(
        &self,
        mut request: SendShellRequest,
        shell_id: String,
        control: RunControl,
        inject_into: Option<RunControlHandle>,
        queue_key: Option<&str>,
    ) -> psychevo_runtime::Result<GatewayShellResult> {
        let queue_source = request.source.clone();
        let bind_source = request.bind_source.clone().or_else(|| queue_source.clone());
        let bind_source_generation = bind_source
            .as_ref()
            .map(|source| self.source_generation(source));
        let queue_source_generation = queue_source
            .as_ref()
            .map(|source| self.source_generation(source));
        let mut context = request.context;
        context.state = self.state.clone();
        let explicit_thread_or_session = request.thread_id.is_some() || context.session.is_some();
        let active_thread_id = request
            .thread_id
            .clone()
            .or_else(|| context.session.clone())
            .or_else(|| {
                request
                    .source
                    .as_ref()
                    .and_then(|source| self.lookup_source_thread(source).ok().flatten())
            });
        if let Some(thread_id) = active_thread_id.clone() {
            let cwd = self.thread_cwd(&thread_id)?;
            request.cwd = cwd;
            context.session = Some(thread_id);
            context.continue_latest = false;
        }
        let durable_source_key = if explicit_thread_or_session {
            None
        } else {
            queue_source
                .as_ref()
                .or(bind_source.as_ref())
                .map(|source| source.source_key().0)
        };
        let first_committed_seq = active_thread_id
            .as_deref()
            .and_then(|thread_id| {
                self.state
                    .store()
                    .load_tui_message_summaries(thread_id)
                    .ok()
            })
            .and_then(|summaries| summaries.last().map(|summary| summary.session_seq + 1))
            .unwrap_or(1);
        let durable_intent = json!({
            "kind": "shell",
            "threadId": active_thread_id.clone(),
            "sourceKey": durable_source_key.clone(),
            "runtimeSource": context.source.clone(),
            "firstCommittedSeq": first_committed_seq,
            "cwd": request.cwd.to_string_lossy(),
            "command": request.command.clone(),
        });
        let durable_activity = if inject_into.is_none() {
            Some(
                self.claim_durable_gateway_activity(DurableGatewayActivityClaim {
                    activity_id: &shell_id,
                    thread_id: active_thread_id.as_deref(),
                    source_key: durable_source_key.as_deref(),
                    turn_id: Some(&shell_id),
                    kind: "shell",
                    owner_surface: Some(&context.source),
                    queued_turns: 0,
                    intent: Some(durable_intent),
                })?,
            )
        } else {
            None
        };
        let _heartbeat = durable_activity
            .clone()
            .map(|activity| self.spawn_durable_activity_heartbeat(activity));
        let event_sink = self.wrap_gateway_event_sink(
            request.event_sink.clone(),
            durable_activity.clone(),
            queue_key.map(str::to_string),
            Some(shell_id.clone()),
        );
        let event_sink_for_completion = event_sink.clone();
        let shell_event_id = shell_id.clone();
        let stream = wrap_stream(
            request.stream,
            event_sink,
            shell_id,
            active_thread_id.clone(),
        );
        let stream = stream.unwrap_or_else(|| Arc::new(|_| {}));
        let result = run_user_shell_command_streaming_controlled(
            UserShellOptions {
                cwd: request.cwd,
                command: request.command,
                context: Some(context),
                inject_into,
            },
            stream,
            control,
        )
        .await?;
        let session_id = result
            .session_id
            .clone()
            .or(active_thread_id)
            .ok_or_else(|| Error::Message("shell command did not resolve a session".to_string()))?;
        let backend = GatewayBackendInfo {
            kind: BackendKind::Native,
            runtime_ref: Some("native".to_string()),
            native_id: Some(session_id.clone()),
        };
        if let Some(source) = &bind_source
            && bind_source_generation
                .is_none_or(|generation| self.source_generation(source) == generation)
        {
            self.bind_source_thread(source, &session_id, &backend, request.lineage)?;
        }
        if let Some(source) = &queue_source
            && bind_source
                .as_ref()
                .is_none_or(|bind_source| bind_source.source_key() != source.source_key())
            && queue_source_generation
                .is_none_or(|generation| self.source_generation(source) == generation)
        {
            self.bind_source_thread(source, &session_id, &backend, None)?;
        }
        let summaries = self.state.store().load_tui_message_summaries(&session_id)?;
        let committed_entries = transcript::project_committed_turn_window_entries(
            &session_id,
            &summaries,
            transcript::TurnProjectionWindow {
                turn_id: &shell_event_id,
                first_committed_seq,
            },
        );
        if let Some(event_sink) = event_sink_for_completion {
            for entry in committed_entries.clone() {
                event_sink(GatewayEvent::EntryUpdated {
                    turn_id: shell_event_id.clone(),
                    entry,
                });
            }
        }
        self.finish_durable_gateway_activity(durable_activity.as_ref(), "completed");
        Ok(GatewayShellResult {
            thread: GatewayThread {
                id: session_id,
                backend,
                source_key: bind_source.as_ref().map(GatewaySource::source_key),
                forked_from_thread_id: None,
            },
            result,
            committed_entries,
        })
    }

    fn thread_cwd(&self, thread_id: &str) -> psychevo_runtime::Result<PathBuf> {
        let summary = self
            .state
            .store()
            .session_summary(thread_id)?
            .ok_or_else(|| Error::Message(format!("session not found: {thread_id}")))?;
        Ok(PathBuf::from(summary.cwd))
    }
}

fn gateway_delivery_input_parts(request: &SendTurnRequest) -> Vec<GatewayInputPart> {
    if !request.input.is_empty() {
        return request.input.clone();
    }
    let mut input = Vec::new();
    if !request.options.prompt.is_empty() {
        input.push(GatewayInputPart::Text {
            text: request.options.prompt.clone(),
        });
    }
    input.extend(request.options.image_inputs.iter().cloned().map(|image| {
        GatewayInputPart::Image {
            input: match image {
                ImageInput::LocalPath(path) => GatewayImageInput::LocalPath {
                    path: path.display().to_string(),
                },
                ImageInput::ImageUrl(url) => GatewayImageInput::Url { url },
            },
        }
    }));
    input
}

fn persisted_gateway_activity(
    state: &StateRuntime,
    activity: &DurableGatewayActivity,
) -> Option<GatewayActivityRecord> {
    state
        .store()
        .gateway_activity(&activity.activity_id)
        .ok()
        .flatten()
}

fn gateway_turn_status_for_outcome(outcome: Outcome) -> GatewayTurnStatus {
    match outcome {
        Outcome::Normal => GatewayTurnStatus::Completed,
        Outcome::Failed => GatewayTurnStatus::Failed,
        Outcome::Stopped | Outcome::Aborted => GatewayTurnStatus::Interrupted,
    }
}

fn gateway_turn_status_name(status: GatewayTurnStatus) -> &'static str {
    match status {
        GatewayTurnStatus::Queued => "queued",
        GatewayTurnStatus::Running => "running",
        GatewayTurnStatus::Completed => "completed",
        GatewayTurnStatus::Failed => "failed",
        GatewayTurnStatus::Interrupted => "interrupted",
    }
}

fn durable_activity_status_for_turn(status: GatewayTurnStatus) -> &'static str {
    match status {
        GatewayTurnStatus::Failed => "failed",
        GatewayTurnStatus::Interrupted => "interrupted",
        _ => "completed",
    }
}

fn terminal_message_for_result(result: &RunResult) -> Option<String> {
    if result.outcome == Outcome::Normal {
        return None;
    }
    result
        .terminal_error
        .as_ref()
        .map(|error| error.message.clone())
        .or_else(|| {
            result
                .terminal_reason
                .as_ref()
                .map(|reason| format!("{reason:?}"))
        })
        .or_else(|| match result.outcome {
            Outcome::Failed => Some("The turn failed.".to_string()),
            Outcome::Stopped | Outcome::Aborted => Some("The turn was interrupted.".to_string()),
            Outcome::Normal => None,
        })
}

fn classified_terminal_error_for_result(result: &RunResult) -> Option<GatewayTurnError> {
    result.terminal_error.as_ref().map(|error| AgentErrorView {
        message: error.message.clone(),
        code: Some(error.code.clone()),
        stage: Some(error.stage.clone()),
        retry_class: Some(error.retry_class.clone()),
        delivery: AgentDeliveryStatusView::Unknown,
        recovery_action: None,
        diagnostic_ref: Some(error.diagnostic_ref.clone()),
    })
}

fn gateway_turn_error(message: &str, data: Option<&Value>) -> GatewayTurnError {
    let mut error = agent_error_view(message, data);
    // Adapter metadata is classification evidence, never public copy. The
    // caller supplies the already-sanitized product message.
    error.stage = error.stage.or_else(|| data.map(|_| "prompt".to_string()));
    error.retry_class = error
        .retry_class
        .or_else(|| data.map(|_| "never".to_string()));
    error
}
