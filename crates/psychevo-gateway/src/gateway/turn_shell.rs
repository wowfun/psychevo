impl Gateway {
    async fn run_shell_now(
        &self,
        queue_key: &str,
        request: SendShellRequest,
        shell_id: String,
    ) -> psychevo::Result<GatewayShellResult> {
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
        self.run_shell_with_control(request, shell_id, control, Some(queue_key))
            .await
    }

    async fn run_shell_with_control(
        &self,
        mut request: SendShellRequest,
        shell_id: String,
        control: RunControl,
        queue_key: Option<&str>,
    ) -> psychevo::Result<GatewayShellResult> {
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
        let source_thread_id = if let Some(source) = request.source.as_ref() {
            self.lookup_source_thread(source).await.ok().flatten()
        } else {
            None
        };
        let active_thread_id = request
            .thread_id
            .clone()
            .or_else(|| context.session.clone())
            .or(source_thread_id);
        if let Some(thread_id) = active_thread_id.clone() {
            let cwd = self.thread_cwd(&thread_id).await?;
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
        let first_committed_seq = match active_thread_id.as_deref() {
            Some(thread_id) => self
                .state
                .load_tui_message_summaries(thread_id)
                .await
                .ok()
                .and_then(|summaries| {
                    summaries.last().map(|summary| summary.session_seq + 1)
                })
                .unwrap_or(1),
            None => 1,
        };
        let durable_intent = json!({
            "kind": "shell",
            "threadId": active_thread_id.clone(),
            "sourceKey": durable_source_key.clone(),
            "runtimeSource": context.source.clone(),
            "firstCommittedSeq": first_committed_seq,
            "cwd": request.cwd.to_string_lossy(),
            "command": request.command.clone(),
        });
        let durable_activity = Some(
            self.claim_durable_gateway_activity(DurableGatewayActivityClaim {
                activity_id: &shell_id,
                thread_id: active_thread_id.as_deref(),
                source_key: durable_source_key.as_deref(),
                turn_id: Some(&shell_id),
                kind: "shell",
                owner_surface: Some(&context.source),
                queued_turns: 0,
                intent: Some(durable_intent),
            })
            .await?,
        );
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
                inject_into: None,
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
            self.bind_source_thread(source, &session_id, &backend, request.lineage)
                .await?;
        }
        if let Some(source) = &queue_source
            && bind_source
                .as_ref()
                .is_none_or(|bind_source| bind_source.source_key() != source.source_key())
            && queue_source_generation
                .is_none_or(|generation| self.source_generation(source) == generation)
        {
            self.bind_source_thread(source, &session_id, &backend, None)
                .await?;
        }
        let summaries = self
            .state
            .load_tui_message_summaries(&session_id)
            .await?;
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
                let _ = event_sink.emit(GatewayEvent::EntryUpdated {
                    turn_id: shell_event_id.clone(),
                    entry,
                });
            }
        }
        self.finish_durable_gateway_activity(durable_activity.as_ref(), "completed")
            .await;
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

    async fn thread_cwd(&self, thread_id: &str) -> psychevo::Result<PathBuf> {
        let summary = self
            .state
            .session_summary(thread_id)
            .await?
            .ok_or_else(|| Error::Message(format!("session not found: {thread_id}")))?;
        Ok(PathBuf::from(summary.cwd))
    }
}

fn gateway_turn_status_for_outcome(outcome: Outcome) -> GatewayTurnStatus {
    match outcome {
        Outcome::Normal => GatewayTurnStatus::Completed,
        Outcome::Failed => GatewayTurnStatus::Failed,
        Outcome::Stopped | Outcome::Aborted => GatewayTurnStatus::Interrupted,
    }
}

fn unavailable_compaction_result(
    thread_id: &str,
    reason: psychevo::__product::sessions::CompactionReason,
    runtime_ref: &str,
) -> psychevo::__product::sessions::CompactionResult {
    psychevo::__product::sessions::CompactionResult {
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

fn gateway_turn_error(message: &str, data: Option<&Value>) -> GatewayTurnError {
    let mut error = agent_error_view(message, data);
    error.stage = error.stage.or_else(|| data.map(|_| "prompt".to_string()));
    error.retry_class = error
        .retry_class
        .or_else(|| data.map(|_| "never".to_string()));
    error
}
