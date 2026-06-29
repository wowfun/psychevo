impl Gateway {
    async fn run_turn_now(
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
        let active_thread_id = request
            .thread_id
            .clone()
            .or(mapped_thread_id)
            .or_else(|| options.session.clone());
        if let Some(thread_id) = active_thread_id.clone() {
            options.cwd = self.thread_cwd(&thread_id)?;
            options.session = Some(thread_id);
            options.continue_latest = false;
        }
        let source_name = request
            .runtime_source
            .clone()
            .unwrap_or_else(|| "gateway".to_string());
        let durable_source_key = if request.thread_id.is_some() {
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
        let event_sink = self.wrap_gateway_event_sink(
            base_event_sink,
            durable_activity.clone(),
            Some(queue_key.to_string()),
            Some(turn_id.clone()),
        );
        let event_sink_for_completion = event_sink.clone();

        if options.approval_handler.is_none()
            && let Some(event_sink) = event_sink.clone()
        {
            options.approval_handler = Some(Arc::new(GatewayApprovalHandler::new(
                Some(queue_key.to_string()),
                self.pending_permissions.clone(),
                event_sink,
            )));
        }

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
        let continue_sources = if request.continue_sources.is_empty() {
            vec![source_name.clone()]
        } else {
            request.continue_sources
        };

        let peer = resolve_peer_turn(&options)?;
        if peer.is_none()
            && let Some(thread_id) = active_thread_id.as_deref()
        {
            clear_acp_peer_usage_update(&self.state, thread_id)?;
        }
        if peer.is_none() {
            options.external_agent_delegate = Some(Arc::new(GatewayExternalAgentDelegate {
                base_options: options.clone(),
                stream: stream.clone(),
            }));
        }
        let backend_request = BackendTurnRequest {
            options,
            runtime_source: source_name,
            continue_sources,
            stream,
            control,
        };
        let backend_result = match peer {
            Some(peer) => acp_peer::run_acp_peer_turn(peer, backend_request, turn_id.clone())
                .await
                .map(|result| {
                    (
                        result.run,
                        GatewayBackendInfo {
                            kind: BackendKind::PeerAgent,
                            native_id: Some(result.native_session_id),
                        },
                    )
                }),
            None => self.backend.run_turn(backend_request).await.map(|result| {
                (
                    result,
                    GatewayBackendInfo {
                        kind: self.backend.kind(),
                        native_id: None,
                    },
                )
            }),
        };
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
                let turn = self.record_and_project_terminal_turn(
                    thread_id.as_deref(),
                    &turn_id,
                    GatewayTurnStatus::Failed,
                    None,
                    Some(error_message.as_str()),
                    durable_activity.as_ref(),
                );
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
        let backend_info = GatewayBackendInfo {
            native_id: backend_info
                .native_id
                .or_else(|| Some(result.session_id.clone())),
            ..backend_info
        };
        let summaries = self
            .state
            .store()
            .load_tui_message_summaries(&result.session_id)?;
        let turn_status = gateway_turn_status_for_outcome(result.outcome);
        let terminal_message = terminal_message_for_result(&result);
        let turn = self.record_and_project_terminal_turn(
            Some(&result.session_id),
            &turn_id,
            turn_status,
            Some(result.outcome.as_str()),
            terminal_message.as_deref(),
            durable_activity.as_ref(),
        );
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
        committed_entries.extend(self.project_terminal_entry_for_turn(&turn_id));
        if let Some(event_sink) = event_sink_for_completion {
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
        self.finish_durable_gateway_activity(
            durable_activity.as_ref(),
            durable_activity_status_for_turn(turn_status),
        );

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
            },
            turn: GatewayTurn {
                id: turn_id,
                thread_id: Some(result.session_id.clone()),
                status: turn_status,
                outcome: Some(result.outcome.as_str().to_string()),
                error: terminal_message.map(|message| GatewayTurnError { message }),
                started_at_ms: turn.started_at_ms,
                completed_at_ms: turn.completed_at_ms,
            },
            result,
            committed_entries,
        })
    }

    fn record_and_project_terminal_turn(
        &self,
        thread_id: Option<&str>,
        turn_id: &str,
        status: GatewayTurnStatus,
        outcome: Option<&str>,
        error_message: Option<&str>,
        durable_activity: Option<&DurableGatewayActivity>,
    ) -> GatewayTurn {
        let completed_at_ms = gateway_now_ms();
        let started_at_ms = durable_activity
            .and_then(|activity| persisted_gateway_activity(&self.state, activity))
            .map(|record| record.started_at_ms);
        let turn = GatewayTurn {
            id: turn_id.to_string(),
            thread_id: thread_id.map(str::to_string),
            status,
            outcome: outcome.map(str::to_string),
            error: error_message
                .filter(|message| !message.trim().is_empty())
                .map(|message| GatewayTurnError {
                    message: message.to_string(),
                }),
            started_at_ms,
            completed_at_ms: Some(completed_at_ms),
        };
        if let Some(thread_id) = thread_id {
            let _ = self
                .state
                .store()
                .upsert_gateway_turn_terminal(GatewayTurnTerminalInput {
                    turn_id,
                    thread_id,
                    status: gateway_turn_status_name(status),
                    outcome,
                    error_message,
                    started_at_ms,
                    completed_at_ms,
                    metadata: Some(json!({
                        "source": "gateway",
                    })),
                });
        }
        turn
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
            kind: BackendKind::Psychevo,
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
        .terminal_reason
        .as_ref()
        .map(|reason| format!("{reason:?}"))
        .or_else(|| match result.outcome {
            Outcome::Failed => Some("The turn failed.".to_string()),
            Outcome::Stopped | Outcome::Aborted => Some("The turn was interrupted.".to_string()),
            Outcome::Normal => None,
        })
}
