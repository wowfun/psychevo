impl Gateway {
    async fn run_turn_now(
        &self,
        queue_key: &str,
        request: SendTurnRequest,
        turn_id: String,
    ) -> psychevo_runtime::Result<GatewayTurnResult> {
        let event_sink = request.event_sink.clone().map(|event_sink| {
            let gateway = self.clone();
            let queue_key = queue_key.to_string();
            Arc::new(move |event: GatewayEvent| {
                if let GatewayEvent::TurnStarted {
                    thread_id: Some(thread_id),
                    ..
                } = &event
                {
                    gateway.register_active_thread_alias(&queue_key, thread_id);
                }
                event_sink(event);
            }) as GatewayEventSink
        });
        let event_sink_for_completion = event_sink.clone();
        let queue_source = request.source.clone();
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
        let active_thread_id = request.thread_id.clone().or(mapped_thread_id);
        if let Some(thread_id) = active_thread_id.clone() {
            options.session = Some(thread_id);
            options.continue_latest = false;
        }
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

        let stream = wrap_stream(
            request.stream,
            event_sink,
            turn_id.clone(),
            active_thread_id.clone(),
        );
        let result_lineage = request.lineage.clone();
        let source_name = request
            .runtime_source
            .unwrap_or_else(|| "gateway".to_string());
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
        let (result, backend_info) = match peer {
            Some(peer) => {
                let result =
                    acp_peer::run_acp_peer_turn(peer, backend_request, turn_id.clone()).await?;
                (
                    result.run,
                    GatewayBackendInfo {
                        kind: BackendKind::PeerAgent,
                        native_id: Some(result.native_session_id),
                    },
                )
            }
            None => {
                let result = self.backend.run_turn(backend_request).await?;
                (
                    result,
                    GatewayBackendInfo {
                        kind: self.backend.kind(),
                        native_id: None,
                    },
                )
            }
        };
        let backend_info = GatewayBackendInfo {
            native_id: backend_info
                .native_id
                .or_else(|| Some(result.session_id.clone())),
            ..backend_info
        };

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
        let summaries = self
            .state
            .store()
            .load_tui_message_summaries(&result.session_id)?;
        let committed_entries = transcript::project_committed_turn_entries(
            &result.session_id,
            &summaries,
            first_committed_seq,
        );
        if let Some(event_sink) = event_sink_for_completion {
            event_sink(GatewayEvent::TurnCompleted {
                thread_id: Some(result.session_id.clone()),
                turn_id: turn_id.clone(),
                outcome: Some(result.outcome.as_str().to_string()),
                committed_entries: committed_entries.clone(),
            });
        }

        Ok(GatewayTurnResult {
            thread: GatewayThread {
                id: result.session_id.clone(),
                backend: backend_info,
                source_key: bind_source.as_ref().map(GatewaySource::source_key),
            },
            turn: GatewayTurn {
                id: turn_id,
                thread_id: result.session_id.clone(),
                status: GatewayTurnStatus::Completed,
            },
            result,
            committed_entries,
        })
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
        self.run_shell_with_control(request, shell_id, control, None)
            .await
    }

    async fn run_shell_auxiliary(
        &self,
        request: SendShellRequest,
        shell_id: String,
        inject_into: RunControlHandle,
    ) -> psychevo_runtime::Result<GatewayShellResult> {
        let (_control_handle, control) = run_control();
        self.run_shell_with_control(request, shell_id, control, Some(inject_into))
            .await
    }

    async fn run_shell_with_control(
        &self,
        request: SendShellRequest,
        shell_id: String,
        control: RunControl,
        inject_into: Option<RunControlHandle>,
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
            context.session = Some(thread_id);
            context.continue_latest = false;
        }
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
        let event_sink_for_completion = request.event_sink.clone();
        let shell_event_id = shell_id.clone();
        let stream = wrap_stream(
            request.stream,
            request.event_sink,
            shell_id,
            active_thread_id.clone(),
        );
        let stream = stream.unwrap_or_else(|| Arc::new(|_| {}));
        let result = run_user_shell_command_streaming_controlled(
            UserShellOptions {
                workdir: request.workdir,
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
        let committed_entries = transcript::project_committed_turn_entries(
            &session_id,
            &summaries,
            first_committed_seq,
        );
        if let Some(event_sink) = event_sink_for_completion {
            for entry in committed_entries.clone() {
                event_sink(GatewayEvent::EntryUpdated {
                    turn_id: shell_event_id.clone(),
                    entry,
                });
            }
        }
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

}
