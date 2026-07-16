include!("rpc_dispatch/transport.rs");

fn prewarm_codex_runtime_inventory(state: &WebState, cwd: PathBuf) {
    let warm_state = state.clone();
    tokio::spawn(async move {
        let _ = warm_state
            .inner
            .codex_capability_broker
            .prepare_runtime_inventory(&cwd)
            .await;
    });
}

async fn handle_rpc(
    state: WebState,
    auth: AuthContext,
    out_tx: mpsc::UnboundedSender<String>,
    request: RpcRequest,
) -> psychevo_runtime::Result<Value> {
    match request.method.as_str() {
        "initialize" => {
            let scope = default_resolved_scope(&state, &auth)?;
            prewarm_codex_runtime_inventory(&state, scope.cwd.clone());
            Ok(json!({
            "server": "psychevo-gateway",
            "version": env!("CARGO_PKG_VERSION"),
            "cwd": scope.cwd,
            "scope": scope.to_wire_scope(),
            "source": scope.source,
            "profile": gateway_profile_value(&state),
            "capabilities": {
                "threads": true,
                "turns": true,
                "historyManagement": true,
                "downloads": true,
                "media": true,
                "imageGeneration": true,
                "automations": true,
                "settingsWrite": "structured",
                "workspaceCreate": true,
                "contextCompaction": true,
                "memoryResources": "status_only"
            }
            }))
        }
        "thread/start" => {
            let params = request.required_params::<wire::ThreadStartParams>()?;
            let scope = resolve_start_scope(&state, &auth, params.scope.clone())?;
            state
                .inner
                .gateway
                .release_prepared_agent_session(&scope.source.source_key().0)
                .await?;
            state.inner.gateway.clear_source_binding(&scope.source)?;
            prewarm_codex_runtime_inventory(&state, scope.cwd.clone());
            let snapshot_scope = detached_draft_scope(&scope, &auth);
            update_browser_session_scope(&state, &auth, &snapshot_scope);
            thread_snapshot(&state, &snapshot_scope, None)
        }
        "thread/resume" => {
            let params = request.params::<wire::ThreadResumeParams>()?;
            let (thread_id, scope) = match params.thread_id {
                Some(thread_id) => {
                    authorize_thread(&state, &auth, &thread_id)?;
                    let scope = resolved_scope_for_thread(&state, &thread_id)?;
                    bind_source_to_thread(&state, &scope, &thread_id)?;
                    update_browser_session_scope(&state, &auth, &scope);
                    (Some(thread_id), scope)
                }
                None => {
                    let scope = resolve_optional_scope(&state, &auth, params.scope)?;
                    let thread_id = state.inner.gateway.resolve_source_thread(&scope.source)?;
                    (thread_id, scope)
                }
            };
            thread_snapshot_live(&state, &scope, thread_id.as_deref()).await
        }
        "thread/read" => {
            let params = request.required_params::<wire::ThreadReadParams>()?;
            authorize_thread(&state, &auth, &params.thread_id)?;
            let scope = resolved_scope_for_thread(&state, &params.thread_id)?;
            thread_snapshot_live(&state, &scope, Some(&params.thread_id)).await
        }
        "thread/trace" => {
            let params = request.required_params::<wire::ThreadTraceParams>()?;
            authorize_thread(&state, &auth, &params.thread_id)?;
            let runtime_state = state.inner.state.clone();
            let result = tokio::task::spawn_blocking(move || {
                runtime_state.read_session_trace(
                    &params.thread_id,
                    SessionTraceReadOptions {
                        after_seq: params.after_seq,
                        limit: params.limit,
                    },
                )
            })
            .await
            .map_err(|err| Error::Message(format!("thread trace read task failed: {err}")))?;
            Ok(serde_json::to_value(result)?)
        }
        "thread/list" => {
            let params = request.params::<wire::ThreadListParams>()?;
            let limit = params.limit.unwrap_or(50).clamp(1, 200);
            let cwd = resolve_session_cwd_filter(&state, &auth, params.cwd)?;
            let cwd = cwd.map(|cwd| cwd.to_string_lossy().into_owned());
            let activity_snapshot = state.inner.gateway.session_activity_snapshot()?;
            let sessions = state.inner.state.store().list_human_session_projections(
                cwd.as_deref(),
                params.archived.unwrap_or(false),
                limit,
            )?;
            Ok(json!({
                "sessions": sessions
                    .into_iter()
                    .map(|projection| {
                        let activity = activity_snapshot
                            .get(&projection.summary.id)
                            .cloned()
                            .unwrap_or_default();
                        session_summary_value(projection, activity)
                    })
                    .collect::<Vec<_>>(),
            }))
        }
        "thread/browser" => {
            let params = request.params::<wire::ThreadBrowserParams>()?;
            let requested_cwd = params
                .cwd
                .clone()
                .or_else(|| params.cursor.as_ref().map(|cursor| cursor.cwd.clone()));
            let cwd = resolve_session_cwd_filter(&state, &auth, requested_cwd)?;
            thread_browser_value(&state, params, cwd)
        }
        "thread/import/list" => {
            let params = request.required_params::<wire::ThreadImportListParams>()?;
            let scope = resolve_optional_scope(&state, &auth, Some(params.scope.clone()))?;
            Ok(serde_json::to_value(
                list_importable_agent_sessions(&state, &scope, params).await?,
            )?)
        }
        "thread/import" => {
            let params = request.required_params::<wire::ThreadImportParams>()?;
            let scope = resolve_optional_scope(&state, &auth, Some(params.scope.clone()))?;
            Ok(serde_json::to_value(
                import_agent_session(&state, &scope, params).await?,
            )?)
        }
        "thread/rename" => {
            let params = request.required_params::<wire::ThreadRenameParams>()?;
            authorize_thread(&state, &auth, &params.thread_id)?;
            state
                .inner
                .state
                .store()
                .set_session_title(&params.thread_id, &params.title)?;
            let session = session_summary_by_id(&state, &params.thread_id)?;
            let event = GatewayEvent::TitleChanged {
                thread_id: params.thread_id.clone(),
                title: session
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                display_title: session
                    .get("displayTitle")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            };
            if let Ok(event_value) = serde_json::to_value(&event) {
                let _ = state.inner.state.store().append_gateway_live_event(
                    None,
                    None,
                    Some(&params.thread_id),
                    None,
                    &event_value,
                );
            }
            let _ = out_tx.send(rpc_notification("gateway/event", json!(event)));
            Ok(json!({"session": session}))
        }
        "thread/archive" => {
            let params = request.required_params::<wire::ThreadIdParams>()?;
            authorize_thread(&state, &auth, &params.thread_id)?;
            guard_session_mutation(&state, &auth, &params.thread_id)?;
            let session = archive_thread(&state, &params.thread_id).await?;
            Ok(json!({"session": session}))
        }
        "thread/restore" => {
            let params = request.required_params::<wire::ThreadIdParams>()?;
            authorize_thread(&state, &auth, &params.thread_id)?;
            guard_session_mutation(&state, &auth, &params.thread_id)?;
            let session = restore_thread(&state, &params.thread_id).await?;
            Ok(json!({"session": session}))
        }
        "thread/delete" => {
            let params = request.required_params::<wire::ThreadIdParams>()?;
            authorize_thread(&state, &auth, &params.thread_id)?;
            guard_session_mutation(&state, &auth, &params.thread_id)?;
            let scope = default_resolved_scope(&state, &auth)?;
            let deleting_current = state
                .inner
                .gateway
                .resolve_source_thread(&scope.source)?
                .as_deref()
                == Some(params.thread_id.as_str());
            delete_thread(&state, &params.thread_id).await?;
            if deleting_current {
                state.inner.gateway.clear_source_binding(&scope.source)?;
            }
            Ok(json!({"deleted": true, "threadId": params.thread_id}))
        }
        "thread/context/read" => {
            let params = request.params::<wire::ThreadContextReadParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            if let Some(thread_id) = params.thread_id.as_deref() {
                authorize_thread(&state, &auth, thread_id)?;
            }
            Ok(serde_json::to_value(
                thread_context_read_result_live(&state, &scope, params).await?,
            )?)
        }
        "thread/draft/prepare" => {
            let params = request.required_params::<wire::ThreadDraftPrepareParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            Ok(serde_json::to_value(
                thread_draft_prepare_result(&state, &scope, params).await?,
            )?)
        }
        "thread/control/set" => {
            let params = request.required_params::<wire::ThreadControlSetParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            if let Some(thread_id) = params.thread_id.as_deref() {
                authorize_thread(&state, &auth, thread_id)?;
            }
            Ok(serde_json::to_value(
                thread_control_set_result(&state, &scope, params).await?,
            )?)
        }
        "thread/action/run" => {
            let params = request.required_params::<wire::ThreadActionRunParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            Ok(serde_json::to_value(
                thread_action_run_result(&state, &auth, &scope, params, out_tx).await?,
            )?)
        }
        "thread/interaction/respond" => {
            let params = request.required_params::<wire::ThreadInteractionRespondParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            Ok(serde_json::to_value(thread_interaction_respond_result(
                &state, &auth, &scope, params,
            )?)?)
        }
        "thread/history/read" => {
            let params = request.required_params::<wire::ThreadHistoryReadParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            Ok(serde_json::to_value(
                thread_history_read_result(&state, &auth, &scope, params).await?,
            )?)
        }
        "thread/history/draft/read" => {
            let params = request.required_params::<wire::ThreadHistoryDraftReadParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            Ok(serde_json::to_value(
                thread_history_draft_read_result(&state, &auth, &scope, params).await?,
            )?)
        }
        "runtime/profile/list" => {
            let params = request.params::<wire::RuntimeProfileListParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            Ok(serde_json::to_value(runtime_profile_list_result(
                &state, &scope,
            )?)?)
        }
        "runtime/profile/read" => {
            let params = request.required_params::<wire::RuntimeProfileReadParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            runtime_profile_read_result(&state, &scope, params)
        }
        "runtime/profile/write" => {
            let params = request.required_params::<wire::RuntimeProfileWriteParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            write_runtime_profile(&state, &scope, params)
        }
        "runtime/profile/delete" => {
            let params = request.required_params::<wire::RuntimeProfileDeleteParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            delete_runtime_profile(&state, &scope, params)
        }
        "runtime/profile/setEnabled" => {
            let params = request.required_params::<wire::RuntimeProfileSetEnabledParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            set_runtime_profile_enabled(&state, &scope, params)
        }
        "automation/list" => {
            let params = request.params::<wire::AutomationListParams>()?;
            automation_list_result(&state, &auth, params)
        }
        "automation/draft" => {
            let params = request.required_params::<wire::AutomationDraftParams>()?;
            automation_draft_result(state, &auth, params).await
        }
        "automation/write" => {
            let params = request.required_params::<wire::AutomationWriteParams>()?;
            automation_write_result(&state, &auth, params)
        }
        "automation/pause" => {
            let params = request.required_params::<wire::AutomationIdParams>()?;
            automation_set_enabled_result(&state, &auth, params, false)
        }
        "automation/resume" => {
            let params = request.required_params::<wire::AutomationIdParams>()?;
            automation_set_enabled_result(&state, &auth, params, true)
        }
        "automation/delete" => {
            let params = request.required_params::<wire::AutomationIdParams>()?;
            automation_delete_result(&state, &auth, params)
        }
        "automation/run" => {
            let params = request.required_params::<wire::AutomationRunParams>()?;
            automation_run_result(state, &auth, params, out_tx)
        }
        "turn/start" => {
            let params = request.required_params::<wire::TurnStartParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            let input = params.input_parts()?;
            let requested_thread_id = match params.thread_id.clone() {
                Some(thread_id) => {
                    authorize_thread(&state, &auth, &thread_id)?;
                    Some(thread_id)
                }
                None => None,
            };
            let existing_binding = requested_thread_id
                .as_deref()
                .map(|thread_id| state.inner.state.store().gateway_runtime_binding(thread_id))
                .transpose()?
                .flatten();
            let validated_target = params
                .target
                .as_ref()
                .map(|target| validate_turn_runnable_target(&state, &scope, target))
                .transpose()?;
            if let (Some(binding), Some(target)) =
                (existing_binding.as_ref(), validated_target.as_ref())
            {
                if binding.runtime_ref.as_deref() != Some(target.runtime_profile_ref.as_str()) {
                    return Err(agent_session_error(
                        "immutable_binding",
                        AgentErrorStage::Binding,
                        "user_action",
                        "not_delivered",
                        format!(
                            "Thread is bound to Runtime Profile `{bound}`; start a new thread to use `{}`.",
                            target.runtime_profile_ref,
                            bound = binding.runtime_ref.as_deref().unwrap_or("unresolved"),
                        ),
                        requested_thread_id
                            .as_ref()
                            .map(|thread_id| format!("agent-binding:{thread_id}")),
                    ));
                }
                if binding.agent_ref != target.agent_ref {
                    return Err(agent_session_error(
                        "immutable_binding",
                        AgentErrorStage::Binding,
                        "user_action",
                        "not_delivered",
                        format!(
                            "Thread is bound to Agent target `{}`; start a new thread to use `{}`.",
                            binding.agent_ref.as_deref().unwrap_or("Default Agent"),
                            target.agent_ref.as_deref().unwrap_or("Default Agent"),
                        ),
                        requested_thread_id
                            .as_ref()
                            .map(|thread_id| format!("agent-binding:{thread_id}")),
                    ));
                }
            }
            let runtime_profile_ref = match (
                existing_binding
                    .as_ref()
                    .and_then(|binding| binding.runtime_ref.as_deref()),
                validated_target.as_ref(),
            ) {
                (Some(bound), _) => bound.to_string(),
                (None, Some(target)) => target.runtime_profile_ref.clone(),
                (None, _) => {
                    return Err(agent_session_error(
                        "target_required",
                        AgentErrorStage::Binding,
                        "user_action",
                        "not_delivered",
                        "An unbound turn requires `target.runtimeProfileRef`.",
                        None,
                    ));
                }
            };
            if existing_binding.is_none() {
                ensure_turn_runtime_profile_supported(
                    &state,
                    &scope,
                    Some(runtime_profile_ref.as_str()),
                )?;
            }
            let turn_context = validate_turn_revisions(
                &state,
                &scope,
                requested_thread_id.clone(),
                params.target.clone(),
                params.expected_context_revision.as_deref(),
                params.expected_control_revision.as_deref(),
            )
            .await?;
            validate_turn_admission(
                &turn_context,
                &input,
                &params.mentions,
                &params.turn_overrides,
            )?;
            let mut control_values = BTreeMap::new();
            apply_thread_control_precedence(
                &state,
                &scope,
                requested_thread_id.as_deref(),
                &mut control_values,
            )?;
            let initial_thread_preferences = source_draft_control_values(&turn_context)?;
            control_values.extend(initial_thread_preferences.clone());
            let response_backend_kind = validated_target
                .as_ref()
                .map(|target| target.backend_kind)
                .map(Ok)
                .unwrap_or_else(|| {
                    turn_context
                        .binding
                        .as_ref()
                        .map(|binding| match binding.backend_kind.as_str() {
                            "native" => Ok(wire::BackendKind::Native),
                            "acp" => Ok(wire::BackendKind::Acp),
                            _ => Err(agent_session_error(
                                "bound_backend_kind_invalid",
                                AgentErrorStage::Binding,
                                "never",
                                "not_delivered",
                                "The captured Thread binding has an invalid backend kind.",
                                Some(format!("agent-binding:{}", binding.thread_id)),
                            )),
                        })
                        .unwrap_or_else(|| {
                            runtime_backend_kind(&state, &scope, &runtime_profile_ref)
                        })
                })?;
            let requested_side_conversation_thread = requested_thread_id
                .as_deref()
                .map(|thread_id| {
                    state
                        .inner
                        .state
                        .store()
                        .session_summary(thread_id)?
                        .map(|summary| side_conversation_session_source(&summary.source))
                        .ok_or_else(|| Error::Message(format!("session not found: {thread_id}")))
                })
                .transpose()?
                .unwrap_or(false);
            let thread_id = if requested_side_conversation_thread {
                requested_thread_id
            } else {
                ensure_turn_start_thread(&state, &scope, requested_thread_id)?
            };
            let source = (!requested_side_conversation_thread).then(|| scope.source.clone());
            let event_selector = thread_id
                .as_ref()
                .map(GatewayThreadSelector::thread_id)
                .unwrap_or_else(|| GatewayThreadSelector::source(scope.source.source_key()));
            let event_thread_id = thread_id.clone();
            let event_state = state.clone();
            let review_cwd = scope.cwd.clone();
            let event_tx = out_tx.clone();
            let event_sink: GatewayEventSink = Arc::new(move |event| {
                let context = event_state
                    .pending_context_for_selector(&event_selector, event_thread_id.as_deref());
                event_state.record_event_with_context(&event, context.clone());
                event_state.record_review_event(&event, &review_cwd);
                let display_event = event_state.event_with_pending_context(event, &context);
                let _ = event_tx.send(rpc_notification("gateway/event", json!(display_event)));
            });
            let bind_source = (!requested_side_conversation_thread).then(|| cwd_source(&scope.cwd));
            let response_thread_id = thread_id.clone();
            let notification_thread_id = thread_id.clone();
            let requested_turn_id = Uuid::now_v7().to_string();
            let response_turn_id = requested_turn_id.clone();
            let turn_state = state.clone();
            let turn_scope = scope.clone();
            tokio::spawn(async move {
                let result = run_routed_thread_turn(
                    &turn_state,
                    &turn_scope,
                    RoutedThreadTurn {
                        thread_id,
                        context: turn_context,
                        control_values,
                        initial_thread_preferences,
                        input,
                        mentions: params.mentions,
                        turn_overrides: params.turn_overrides,
                        runtime_source: "web".to_string(),
                        continue_sources: vec![
                            "run".to_string(),
                            "tui".to_string(),
                            "web".to_string(),
                        ],
                        event_sink: Some(event_sink),
                        lineage: None,
                        source,
                        bind_source,
                        turn_id: Some(requested_turn_id.clone()),
                    },
                )
                .await;
                let notification = match result {
                    Ok(result) => {
                        rpc_notification("turn/result", gateway_turn_result_value(result))
                    }
                    Err(err) => rpc_notification(
                        "turn/error",
                        serde_json::to_value(wire::TurnErrorPayload {
                            error: agent_error_view(err.to_string(), err.structured_data()),
                            thread_id: notification_thread_id,
                            turn_id: Some(requested_turn_id),
                        })
                        .unwrap_or_else(|serialization_error| {
                            json!({
                                "error": {
                                    "message": format!("Turn failed; error projection failed: {serialization_error}"),
                                    "delivery": "unknown"
                                },
                                "threadId": Value::Null
                            })
                        }),
                    ),
                };
                let _ = out_tx.send(notification);
            });
            let response_thread_id = response_thread_id.ok_or_else(|| {
                agent_session_error(
                    "thread_creation_failed",
                    AgentErrorStage::Binding,
                    "retry",
                    "not_delivered",
                    "Gateway accepted turn preparation without creating a public Thread.",
                    None,
                )
            })?;
            Ok(serde_json::to_value(wire::TurnStartResult {
                accepted: true,
                thread_id: response_thread_id.clone(),
                turn_id: response_turn_id,
                thread: wire::GatewayThread {
                    id: response_thread_id,
                    backend: wire::GatewayBackendInfo {
                        kind: response_backend_kind,
                        runtime_ref: Some(runtime_profile_ref),
                        native_id: None,
                    },
                    source_key: Some(scope.source.source_key()),
                    forked_from_thread_id: None,
                },
            })?)
        }
        "voice/asr/transcribe" => {
            let params = request.required_params::<wire::VoiceAsrTranscribeParams>()?;
            voice_asr_transcribe_value(&state, &auth, params).await
        }
        "voice/tts/synthesize" => {
            let params = request.required_params::<wire::VoiceTtsSynthesizeParams>()?;
            voice_tts_synthesize_value(&state, &auth, params).await
        }
        "voice/policy/read" => {
            let params = request.params::<wire::VoicePolicyReadParams>()?;
            voice_policy_read_value(&state, &auth, params)
        }
        "voice/policy/update" => {
            let params = request.required_params::<wire::VoicePolicyUpdateParams>()?;
            voice_policy_update_value(&state, &auth, params)
        }
        "thread/realtime/start" => {
            let params = request.required_params::<wire::ThreadRealtimeStartParams>()?;
            voice_realtime_start_value(&state, &auth, out_tx, params).await
        }
        "thread/realtime/appendAudio" => {
            let params = request.required_params::<wire::ThreadRealtimeAppendAudioParams>()?;
            voice_realtime_append_audio_value(&state, params)
        }
        "thread/realtime/appendText" => {
            let params = request.required_params::<wire::ThreadRealtimeAppendTextParams>()?;
            voice_realtime_append_text_value(&state, params)
        }
        "thread/realtime/appendSpeech" => {
            let params = request.required_params::<wire::ThreadRealtimeAppendSpeechParams>()?;
            voice_realtime_append_speech_value(&state, params)
        }
        "thread/realtime/stop" => {
            let params = request.required_params::<wire::ThreadRealtimeSessionParams>()?;
            voice_realtime_stop_value(&state, out_tx, params)
        }
        "thread/realtime/listVoices" => {
            let params = request.required_params::<wire::ThreadRealtimeSessionParams>()?;
            voice_realtime_list_voices_value(&state, params)
        }
        "completion/list" => {
            let params = request.required_params::<wire::CompletionListParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            if let Some(thread_id) = &params.thread_id {
                authorize_thread(&state, &auth, thread_id)?;
            }
            completion_list_value(&state, &scope, params)
        }
        "workspace/files" => {
            let params = request.required_params::<wire::WorkspaceFilesParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            workspace_files_value(&scope)
        }
        "workspace/folders" => {
            let params = request.required_params::<wire::WorkspaceFolderListParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            workspace_folder_list_value(&state, &scope, params.path.as_deref())
        }
        "workspace/git/branches" => {
            let params = request.required_params::<wire::WorkspaceGitBranchesParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            workspace_git_branches_value(&scope)
        }
        "workspace/git/checkout" => {
            let params = request.required_params::<wire::WorkspaceGitCheckoutParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            workspace_git_checkout_value(&scope, params)
        }
        "workspace/file/read" => {
            let params = request.required_params::<wire::WorkspaceFileReadParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            workspace_file_read_value(&scope, &params.path)
        }
        "workspace/file/write" => {
            let params = request.required_params::<wire::WorkspaceFileWriteParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            workspace_file_write_value(&scope, params)
        }
        "workspace/diff" => {
            let params = request.required_params::<wire::WorkspaceDiffParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            workspace_diff_value(&scope, params.path.as_deref())
        }
        "workspace/changes" => {
            let params = request.required_params::<wire::WorkspaceChangesParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            Ok(serde_json::to_value(
                state.inner.review.changes_for_scope(&scope),
            )?)
        }
        "workspace/change/accept" => {
            let params = request.required_params::<wire::WorkspaceChangeFileParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            Ok(serde_json::to_value(state.inner.review.accept(
                &scope,
                &params.turn_id,
                &params.path,
            )?)?)
        }
        "workspace/change/reject" => {
            let params = request.required_params::<wire::WorkspaceChangeFileParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            Ok(serde_json::to_value(state.inner.review.reject(
                &scope,
                &params.turn_id,
                &params.path,
            )?)?)
        }
        "workspace/create" => {
            let params = request.required_params::<wire::WorkspaceCreateParams>()?;
            workspace_create_value(&state, &auth, params)
        }
        "context/read" => {
            let params = request.required_params::<wire::ContextReadParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            if let Some(thread_id) = &params.thread_id {
                authorize_thread(&state, &auth, thread_id)?;
            }
            context_read_value(&state, &scope, params.thread_id.as_deref())
        }
        "observability/read" => {
            let params = request.required_params::<wire::ObservabilityReadParams>()?;
            let requested_scope = resolve_required_scope(&state, &auth, params.scope)?;
            let (scope, thread_id) = match params.thread_id {
                Some(thread_id) => {
                    authorize_thread(&state, &auth, &thread_id)?;
                    (
                        resolved_scope_for_thread(&state, &thread_id)?,
                        Some(thread_id),
                    )
                }
                None => (requested_scope, None),
            };
            observability_read_value(&state, &scope, thread_id.as_deref())
        }
        "usage/read" => {
            let params = request.required_params::<wire::UsageReadParams>()?;
            usage_read_value(&state, params)
        }
        "source/reset" => {
            let params = request.required_params::<wire::SourceResetParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            state
                .inner
                .gateway
                .release_prepared_agent_session(&scope.source.source_key().0)
                .await?;
            reset_source_to_empty(&state, &scope)
        }
        "agent/list" => {
            let params = request.params::<wire::AgentListParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let catalog = discover_gateway_agents(&state, &scope)?;
            Ok(serde_json::to_value(agent_list_result(&catalog))?)
        }
        "agent/read" => {
            let params = request.required_params::<wire::AgentReadParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            if params.target.is_some() {
                return read_agent_definition(&state, &scope, params);
            }
            let catalog = discover_gateway_agents(&state, &scope)?;
            let agent = resolve_agent_definition(
                &catalog,
                &params.name,
                &scope.cwd,
                &state.inner.inherited_env,
            )?;
            Ok(serde_json::to_value(agent_read_result(&agent))?)
        }
        "agent/write" => {
            let params = request.required_params::<wire::AgentWriteParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            write_agent_definition(&state, &scope, params)
        }
        "agent/setEnabled" => {
            let params = request.required_params::<wire::AgentSetEnabledParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            set_agent_definition_enabled(&state, &scope, params)
        }
        "agent/delete" => {
            let params = request.required_params::<wire::AgentDeleteParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            delete_agent_definition(&state, &scope, params)
        }
        "agent/status" => {
            let params = request.params::<wire::AgentStatusParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            if let Some(thread_id) = params.thread_id.as_deref() {
                authorize_thread(&state, &auth, thread_id)?;
            }
            let source_thread_id = if params.thread_id.is_some() || params.all.unwrap_or(false) {
                None
            } else {
                state.inner.gateway.resolve_source_thread(&scope.source)?
            };
            let thread_id = params.thread_id.as_deref().or(source_thread_id.as_deref());
            Ok(serde_json::to_value(agent_status_result(
                Some(state.inner.state.store()),
                thread_id,
                params.all.unwrap_or(false),
            ))?)
        }
        "agent/control" => {
            let params = request.required_params::<wire::AgentControlParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let _ = scope;
            Ok(serde_json::to_value(agent_control_result(
                state.inner.state.store(),
                params,
            )?)?)
        }
        "team/list" => {
            let params = request.params::<wire::TeamListParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let agents = discover_gateway_agents(&state, &scope)?;
            let teams = discover_gateway_teams(&state, &scope, &agents)?;
            Ok(serde_json::to_value(team_list_result(&teams))?)
        }
        "team/read" => {
            let params = request.required_params::<wire::TeamReadParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            if params.target.is_some() {
                return read_team_definition(&state, &scope, params);
            }
            let agents = discover_gateway_agents(&state, &scope)?;
            let teams = discover_gateway_teams(&state, &scope, &agents)?;
            let team = resolve_agent_team_definition(&teams, &params.name)?;
            Ok(serde_json::to_value(team_read_result(&team))?)
        }
        "team/write" => {
            let params = request.required_params::<wire::TeamWriteParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            write_team_definition(&state, &scope, params)
        }
        "team/setEnabled" => {
            let params = request.required_params::<wire::TeamSetEnabledParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            set_team_definition_enabled(&state, &scope, params)
        }
        "team/delete" => {
            let params = request.required_params::<wire::TeamDeleteParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            delete_team_definition(&state, &scope, params)
        }
        "team/status" => {
            let params = request.params::<wire::TeamStatusParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            if let Some(thread_id) = params.thread_id.as_deref() {
                authorize_thread(&state, &auth, thread_id)?;
            }
            let source_thread_id = if params.thread_id.is_some() {
                None
            } else {
                state.inner.gateway.resolve_source_thread(&scope.source)?
            };
            let thread_id = params.thread_id.as_deref().or(source_thread_id.as_deref());
            Ok(serde_json::to_value(team_status_result(
                state.inner.state.store(),
                thread_id,
            )?)?)
        }
        "backend/list" => {
            let params = request.params::<wire::BackendListParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            materialize_local_acp_backends(&state, &scope)?;
            let backends = load_agent_backend_configs(
                &state.inner.home,
                &scope.cwd,
                &state.inner.inherited_env,
            )?;
            Ok(serde_json::to_value(wire::BackendListResult {
                backends: backend_values_for_scope(&state, &scope, &backends)?,
            })?)
        }
        "backend/doctor" => {
            let params = request.required_params::<wire::BackendDoctorParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            materialize_local_acp_backends(&state, &scope)?;
            let backends = load_agent_backend_configs(
                &state.inner.home,
                &scope.cwd,
                &state.inner.inherited_env,
            )?;
            let backend = backends
                .get(&params.id)
                .ok_or_else(|| Error::Message(format!("unknown backend: {}", params.id)))?;
            Ok(serde_json::to_value(
                managed_backend_doctor_value_with_auth(&state, &scope, backend).await?,
            )?)
        }
        "backend/install" | "backend/repair" | "backend/upgrade" => {
            let params = request.required_params::<wire::BackendManageParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let operation = request.method.strip_prefix("backend/").unwrap_or("install");
            manage_backend_value(&state, &scope, params, operation).await
        }
        "backend/write" => {
            let params = request.required_params::<wire::BackendWriteParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            write_backend_config(&state, &scope, params)
        }
        "backend/delete" => {
            let params = request.required_params::<wire::BackendDeleteParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            delete_backend_config(&state, &scope, params)
        }
        "plugin/list" => {
            let params = request.params::<wire::PluginListParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let options = plugin_runtime_options(&state, scope.cwd.clone());
            let native = plugin_list_value(&options)?;
            let codex = state
                .inner
                .codex_capability_broker
                .plugin_list(&scope.cwd)
                .await;
            Ok(codex_capability_broker::merge_plugin_list(native, codex))
        }
        "plugin/read" => {
            let params = request.required_params::<wire::PluginReadParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            if let Some(identity) =
                codex_capability_broker::CodexPluginIdentity::parse_selector(&params.selector)?
            {
                let detail = state
                    .inner
                    .codex_capability_broker
                    .plugin_read(&scope.cwd, &identity)
                    .await?;
                return Ok(codex_capability_broker::codex_plugin_read_value(
                    &identity, detail,
                ));
            }
            let options = plugin_runtime_options(&state, scope.cwd);
            plugin_view_value(&options, &params.selector)
        }
        "plugin/doctor" => {
            let params = request.params::<wire::PluginDoctorParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            if let Some(selector) = params.selector.as_deref()
                && let Some(identity) =
                    codex_capability_broker::CodexPluginIdentity::parse_selector(selector)?
            {
                let detail = state
                    .inner
                    .codex_capability_broker
                    .plugin_read(&scope.cwd, &identity)
                    .await?;
                let apps = state
                    .inner
                    .codex_capability_broker
                    .request("app/list", json!({"threadId":null,"forceRefetch":false}))
                    .await;
                return Ok(json!({
                    "plugins": [codex_capability_broker::codex_plugin_read_value(&identity, detail)],
                    "apps": match apps {
                        Ok(value) => value,
                        Err(err) => json!({"readiness":"unavailable","reason":err.to_string()}),
                    },
                }));
            }
            let options = plugin_runtime_options(&state, scope.cwd);
            plugin_doctor_value(&options, params.selector.as_deref())
        }
        "plugin/import/inspect" => {
            let params = request.required_params::<wire::PluginInspectParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            plugin_import_inspect_value(
                &state.inner.home,
                &scope.cwd,
                PluginInspectOptions {
                    source: params.source,
                    source_kind: parse_plugin_source_kind(params.source_kind.as_deref())?,
                    git_ref: params.git_ref,
                    npm_version: params.npm_version,
                    npm_registry: params.npm_registry,
                    adapter_mode: parse_plugin_adapter_mode(params.adapter_mode.as_deref())?,
                },
            )
        }
        "plugin/install" => {
            let params = request.required_params::<wire::PluginInstallParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            if let Some(identity) =
                codex_capability_broker::CodexPluginIdentity::parse_selector(&params.source)?
            {
                let result = state
                    .inner
                    .codex_capability_broker
                    .plugin_install(&scope.cwd, &identity)
                    .await?;
                return Ok(json!({
                    "success": true,
                    "authority": {
                        "kind": "codex",
                        "plugin": identity.plugin,
                        "marketplace": identity.marketplace,
                    },
                    "result": result,
                }));
            }
            plugin_install_value(
                &state.inner.home,
                &scope.cwd,
                PluginInstallOptions {
                    source: params.source,
                    source_kind: parse_plugin_source_kind(params.source_kind.as_deref())?,
                    scope: parse_plugin_scope(params.scope_name.as_deref())?,
                    git_ref: params.git_ref,
                    npm_version: params.npm_version,
                    npm_registry: params.npm_registry,
                    adapter_mode: parse_plugin_adapter_mode(params.adapter_mode.as_deref())?,
                    force: params.force,
                },
            )
        }
        "plugin/uninstall" => {
            let params = request.required_params::<wire::PluginUninstallParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            if let Some(identity) =
                codex_capability_broker::CodexPluginIdentity::parse_selector(&params.selector)?
            {
                let result = state
                    .inner
                    .codex_capability_broker
                    .plugin_uninstall(&scope.cwd, &identity)
                    .await?;
                return Ok(json!({
                    "success": true,
                    "authority": {
                        "kind": "codex",
                        "plugin": identity.plugin,
                        "marketplace": identity.marketplace,
                    },
                    "result": result,
                }));
            }
            plugin_uninstall_value(
                &state.inner.home,
                &scope.cwd,
                parse_plugin_scope(params.scope_name.as_deref())?,
                &params.selector,
            )
        }
        "plugin/setEnabled" => {
            let params = request.required_params::<wire::PluginSetEnabledParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            plugin_set_enabled_value(
                &state.inner.home,
                &scope.cwd,
                parse_plugin_scope(params.scope_name.as_deref())?,
                &params.selector,
                params.enabled,
            )
        }
        "plugin/setTrust" => {
            let params = request.required_params::<wire::PluginSetTrustParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            plugin_set_trust_value(
                &state.inner.home,
                &scope.cwd,
                parse_plugin_scope(params.scope_name.as_deref())?,
                &params.selector,
                params.trusted,
            )
        }
        "plugin/catalog/list" => {
            let params = request.params::<wire::PluginCatalogListParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            plugin_marketplace_list_value(
                &state.inner.home,
                &scope.cwd,
                parse_plugin_scope(params.scope_name.as_deref())?,
            )
        }
        "plugin/catalog/add" => {
            let params = request.required_params::<wire::PluginCatalogAddParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            plugin_marketplace_add_value(
                &state.inner.home,
                &scope.cwd,
                parse_plugin_scope(params.scope_name.as_deref())?,
                PluginMarketplaceEntry {
                    name: params.name,
                    source: params.source,
                    kind: params.kind,
                    git_ref: params.git_ref,
                    npm_version: params.npm_version,
                    npm_registry: params.npm_registry,
                    adapter_mode: parse_plugin_adapter_mode(params.adapter_mode.as_deref())?,
                },
            )
        }
        "plugin/catalog/remove" => {
            let params = request.required_params::<wire::PluginCatalogRemoveParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            plugin_marketplace_remove_value(
                &state.inner.home,
                &scope.cwd,
                parse_plugin_scope(params.scope_name.as_deref())?,
                &params.name,
            )
        }
        "skill/list" => {
            let params = request.params::<wire::SkillListParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let catalog = discover_skills(&SkillDiscoveryOptions {
                home: state.inner.home.clone(),
                cwd: scope.cwd,
                config_path: state.inner.config_path.clone(),
                env: state.inner.inherited_env.clone(),
                explicit_inputs: Vec::new(),
                additional_roots: Vec::new(),
                no_skills: false,
            })?;
            Ok(list_skills_value_with_options(
                &catalog,
                &ListSkillsOptions {
                    include_hidden: true,
                    detail: true,
                    ..ListSkillsOptions::default()
                },
            ))
        }
        "skill/read" => {
            let params = request.required_params::<wire::SkillReadParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let catalog = discover_skills(&SkillDiscoveryOptions {
                home: state.inner.home.clone(),
                cwd: scope.cwd,
                config_path: state.inner.config_path.clone(),
                env: state.inner.inherited_env.clone(),
                explicit_inputs: Vec::new(),
                additional_roots: Vec::new(),
                no_skills: false,
            })?;
            view_skill_value_selected(
                &catalog,
                &params.name,
                params.path.as_deref().map(std::path::Path::new),
                None,
            )
        }
        "skill/install" => {
            let params = request.required_params::<wire::SkillInstallParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            install_skill(
                &state.inner.home,
                &scope.cwd,
                InstallOptions {
                    source: params.source,
                    target: parse_skill_target(params.target.as_deref())?,
                    name: params.name,
                    all: params.all,
                    force: params.force,
                },
            )
        }
        "skill/uninstall" => {
            let params = request.required_params::<wire::SkillUninstallParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            remove_installed_skill(
                &state.inner.home,
                &scope.cwd,
                parse_skill_target(params.target.as_deref())?,
                &params.name,
                params.path.as_deref().map(std::path::Path::new),
            )
        }
        "skill/setEnabled" => {
            let params = request.required_params::<wire::SkillSetEnabledParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            set_skill_enabled(
                &state.inner.home,
                &scope.cwd,
                parse_skill_target(params.target.as_deref())?,
                &params.name,
                params.enabled,
            )
        }
        "skill/write" => {
            let params = request.required_params::<wire::SkillWriteParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            write_installed_skill(
                &state.inner.home,
                &scope.cwd,
                parse_skill_target(params.target.as_deref())?,
                &params.name,
                params.path.as_deref().map(std::path::Path::new),
                &params.raw_markdown,
            )
        }
        "tool/list" => {
            let params = request.params::<wire::ToolListParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let options = plugin_runtime_options(&state, scope.cwd);
            toolsets_value(&options, ConfigScope::Effective)
        }
        "tool/read" => {
            let params = request.required_params::<wire::ToolReadParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let options = plugin_runtime_options(&state, scope.cwd);
            let value = toolsets_value(&options, ConfigScope::Effective)?;
            let toolsets = value
                .get("toolsets")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let Some(toolset) = toolsets.into_iter().find(|toolset| {
                toolset
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| name == params.name)
            }) else {
                return Err(Error::Config(format!("unknown toolset: {}", params.name)));
            };
            Ok(json!({"toolset": toolset}))
        }
        "tool/setEnabled" => {
            let params = request.required_params::<wire::ToolSetEnabledParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let config_dir = tool_config_dir(&state, &scope, params.local);
            let mode = parse_tool_mode(&params.mode)?;
            Ok(toolset_mutation_value(set_local_toolset_enabled(
                config_dir,
                mode,
                &params.name,
                params.enabled,
            )?))
        }
        "tool/create" => {
            let params = request.required_params::<wire::ToolCreateParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let config_dir = tool_config_dir(&state, &scope, params.local);
            Ok(toolset_mutation_value(create_local_toolset(
                config_dir,
                &params.name,
                params.description,
                params.tools,
                params.includes,
                params.force,
            )?))
        }
        "tool/remove" => {
            let params = request.required_params::<wire::ToolRemoveParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let config_dir = tool_config_dir(&state, &scope, params.local);
            Ok(toolset_mutation_value(remove_local_toolset(
                config_dir,
                &params.name,
            )?))
        }
        "mcp/list" => {
            let params = request.params::<wire::McpListParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let options = plugin_runtime_options(&state, scope.cwd);
            mcp_servers_value(&options, ConfigScope::Effective)
        }
        "mcp/read" => {
            let params = request.required_params::<wire::McpReadParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let options = plugin_runtime_options(&state, scope.cwd);
            mcp_server_value(&options, &params.name)
        }
        "mcp/upsert" => {
            let params = request.required_params::<wire::McpUpsertParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            upsert_mcp_server(
                active_profile_config_dir(&state, &scope),
                mcp_config_input(params),
            )
        }
        "mcp/remove" => {
            let params = request.required_params::<wire::McpNameParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            remove_mcp_server(active_profile_config_dir(&state, &scope), &params.name)
        }
        "mcp/setEnabled" => {
            let params = request.required_params::<wire::McpSetEnabledParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            set_mcp_server_enabled(
                active_profile_config_dir(&state, &scope),
                &params.name,
                params.enabled,
            )
        }
        "mcp/setToolPolicy" => {
            let params = request.required_params::<wire::McpSetToolPolicyParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            set_mcp_server_tool_policy(
                active_profile_config_dir(&state, &scope),
                &params.name,
                McpToolPolicyInput {
                    enabled_tools: params.enabled_tools,
                    disabled_tools: params.disabled_tools,
                },
            )
        }
        "mcp/test" => {
            let params = request.required_params::<wire::McpNameParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let options = plugin_runtime_options(&state, scope.cwd);
            mcp_test_server_value(&options, &params.name).await
        }
        "mcp/oauth/start" => {
            let params = request.required_params::<wire::McpOAuthStartParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            mcp_oauth_start_value(state, scope, params).await
        }
        "mcp/oauth/status" => {
            let params = request.required_params::<wire::McpOAuthStatusParams>()?;
            mcp_oauth_status_value(&state, &params.session_id)
        }
        "mcp/oauth/logout" => {
            let params = request.required_params::<wire::McpNameParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            mcp_oauth_logout_value(&state, &scope, &params.name)
        }
        "channel/list" => {
            let params = request.params::<wire::ChannelListParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            Ok(serde_json::to_value(channel_list_result_for_scope(
                &state, &scope,
            )?)?)
        }
        "channel/show" => {
            let params = request.required_params::<wire::ChannelIdParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            Ok(serde_json::to_value(channel_show_result(
                &state, &scope, &params.id,
            )?)?)
        }
        "channel/enable" => {
            let params = request.required_params::<wire::ChannelEnableParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            Ok(serde_json::to_value(channel_enable_result(
                &state, &scope, params,
            )?)?)
        }
        "channel/update" => {
            let params = request.required_params::<wire::ChannelUpdateParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            Ok(serde_json::to_value(channel_update_result(
                &state, &scope, params,
            )?)?)
        }
        "channel/delete" => {
            let params = request.required_params::<wire::ChannelIdParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            Ok(serde_json::to_value(channel_delete_result(
                &state, &scope, params,
            )?)?)
        }
        "channel/doctor" => {
            let params = request.params::<wire::ChannelDoctorParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            Ok(serde_json::to_value(
                channel_doctor_result_live(&state, &scope, params).await?,
            )?)
        }
        "channel/source/list" => {
            let params = request.required_params::<wire::ChannelIdParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            Ok(serde_json::to_value(channel_source_list_result(
                &state, &scope, params,
            )?)?)
        }
        "channel/wechat-qr/start" => {
            let params = request.params::<wire::ChannelWechatQrStartParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            Ok(serde_json::to_value(
                channel_wechat_qr_start_result(&state, &scope, params).await?,
            )?)
        }
        "channel/wechat-qr/poll" => {
            let params = request.required_params::<wire::ChannelWechatQrPollParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            Ok(serde_json::to_value(
                channel_wechat_qr_poll_result(&state, &scope, params).await?,
            )?)
        }
        "command/list" => {
            let params = request.params::<wire::CommandListParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let active_turn = if let Some(thread_id) = params.thread_id.as_deref() {
                authorize_thread(&state, &auth, thread_id)?;
                state.activity(&scope.source, Some(thread_id)).running
            } else {
                state.activity(&scope.source, None).running
            };
            command_list_value(&state, &scope, active_turn, params.thread_id.is_some())
        }
        "command/execute" => {
            let params = request.required_params::<wire::CommandExecuteParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            if let Some(thread_id) = &params.thread_id {
                authorize_thread(&state, &auth, thread_id)?;
            }
            command_execute_value(&state, &scope, params).await
        }
        "slash/settings/read" => {
            let params = request.params::<wire::SlashSettingsReadParams>()?;
            let cwd = resolve_cwd_filter(&state, &auth, params.cwd)?;
            let scope = resolve_optional_scope(&state, &auth, None)?;
            slash_settings_read_value(&state, &scope, &cwd)
        }
        "slash/settings/update" => {
            let params = request.required_params::<wire::SlashSettingsUpdateParams>()?;
            let cwd = resolve_cwd_filter(&state, &auth, params.cwd.clone())?;
            let scope = resolve_optional_scope(&state, &auth, None)?;
            slash_settings_update_value(&state, &scope, &cwd, params)
        }
        "shell/start" => {
            let params = request.required_params::<wire::ShellStartParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            let command = params.command.trim().to_string();
            if command.is_empty() {
                return Ok(serde_json::to_value(wire::ShellStartResult {
                    accepted: false,
                    thread_id: params.thread_id,
                    message: Some(
                        "shell mode: type !<command> to run a local shell command".to_string(),
                    ),
                })?);
            }
            let thread_id = match params.thread_id.clone() {
                Some(thread_id) => {
                    authorize_thread(&state, &auth, &thread_id)?;
                    Some(thread_id)
                }
                None => state.inner.gateway.resolve_source_thread(&scope.source)?,
            };
            if state
                .inner
                .gateway
                .resolve_source_thread(&scope.source)?
                .as_deref()
                != thread_id.as_deref()
                && let Some(thread_id) = thread_id.as_deref()
            {
                bind_source_to_thread(&state, &scope, thread_id)?;
            }
            let event_selector = thread_id
                .as_ref()
                .map(GatewayThreadSelector::thread_id)
                .unwrap_or_else(|| GatewayThreadSelector::source(scope.source.source_key()));
            let event_thread_id = thread_id.clone();
            let event_state = state.clone();
            let event_tx = out_tx.clone();
            let event_sink: GatewayEventSink = Arc::new(move |event| {
                let context = event_state
                    .pending_context_for_selector(&event_selector, event_thread_id.as_deref());
                event_state.record_event_with_context(&event, context.clone());
                let display_event = event_state.event_with_pending_context(event, &context);
                let _ = event_tx.send(rpc_notification("gateway/event", json!(display_event)));
            });
            let context = user_shell_context_options(&state, &scope, thread_id.clone());
            let gateway = state.inner.gateway.clone();
            let source = scope.source.clone();
            let bind_source = cwd_source(&scope.cwd);
            let cwd = scope.cwd.clone();
            let result_thread_id = thread_id.clone();
            tokio::spawn(async move {
                let result = gateway
                    .send_shell(SendShellRequest {
                        thread_id: result_thread_id.clone(),
                        source: Some(source),
                        bind_source: Some(bind_source),
                        cwd,
                        command,
                        context,
                        stream: None,
                        event_sink: Some(event_sink),
                        lineage: Some(json!({"reason": "shell_start"})),
                    })
                    .await;
                let notification = match result {
                    Ok(result) => {
                        rpc_notification("shell/result", gateway_shell_result_value(result))
                    }
                    Err(err) => rpc_notification(
                        "shell/error",
                        json!({"message": err.to_string(), "threadId": result_thread_id}),
                    ),
                };
                let _ = out_tx.send(notification);
            });
            Ok(serde_json::to_value(wire::ShellStartResult {
                accepted: true,
                thread_id,
                message: None,
            })?)
        }
        "terminal/start" => {
            let params = request.required_params::<wire::TerminalStartParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            Ok(serde_json::to_value(state.inner.terminals.start(
                &scope,
                params,
                &state.inner.inherited_env,
                out_tx,
            )?)?)
        }
        "terminal/write" => {
            let params = request.required_params::<wire::TerminalWriteParams>()?;
            Ok(serde_json::to_value(state.inner.terminals.write(params)?)?)
        }
        "terminal/resize" => {
            let params = request.required_params::<wire::TerminalResizeParams>()?;
            Ok(serde_json::to_value(state.inner.terminals.resize(params)?)?)
        }
        "terminal/terminate" => {
            let params = request.required_params::<wire::TerminalTerminateParams>()?;
            Ok(serde_json::to_value(
                state.inner.terminals.terminate(params, out_tx)?,
            )?)
        }
        "settings/read" => {
            let params = request.params::<wire::SettingsReadParams>()?;
            let (cwd, thread_id) = if let Some(thread_id) = params.thread_id {
                authorize_thread(&state, &auth, &thread_id)?;
                let summary = state
                    .inner
                    .state
                    .store()
                    .session_summary(&thread_id)?
                    .ok_or_else(|| Error::Message(format!("session not found: {thread_id}")))?;
                (PathBuf::from(summary.cwd), Some(thread_id))
            } else {
                (resolve_cwd_filter(&state, &auth, params.cwd)?, None)
            };
            settings_read_value(&state, &cwd, thread_id.as_deref())
        }
        "web/search/settings/read" => {
            let params = request.params::<wire::WebSearchSettingsReadParams>()?;
            let cwd = resolve_cwd_filter(&state, &auth, params.cwd)?;
            web_search_settings_value(&state, &cwd)
        }
        "web/search/settings/update" => {
            let params = request.required_params::<wire::WebSearchSettingsUpdateParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            web_search_settings_update_value(&state, &scope.cwd, params)
        }
        "settings/update" => {
            let params = request.required_params::<wire::SettingsUpdateParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope)?;
            authorize_thread(&state, &auth, &params.thread_id)?;
            update_session_agent_setting(
                &state,
                &scope,
                &params.thread_id,
                params.agent.as_deref(),
            )?;
            settings_read_value(&state, &scope.cwd, Some(&params.thread_id))
        }
        "model/settings/read" => {
            let params = request.params::<wire::ModelSettingsReadParams>()?;
            let cwd = resolve_cwd_filter(&state, &auth, params.cwd)?;
            model_settings_value(&state, &cwd)
        }
        "model/provider/save" => {
            let params = request.required_params::<wire::ModelProviderSaveParams>()?;
            let cwd = default_resolved_scope(&state, &auth)?.cwd;
            model_provider_save_value(&state, &cwd, params)
        }
        "model/provider/catalog" => {
            let params = request.required_params::<wire::ModelProviderCatalogParams>()?;
            let cwd = resolve_cwd_filter(&state, &auth, params.cwd.clone())?;
            model_provider_catalog_value(&state, &cwd, params).await
        }
        "model/state/read" => {
            let params = request.params::<wire::ModelStateReadParams>()?;
            let (cwd, thread_id) =
                resolve_model_state_request_scope(&state, &auth, params.cwd, params.thread_id)?;
            model_state_read_value(&state, &cwd, thread_id.as_deref())
        }
        "model/state/set" => {
            let params = request.required_params::<wire::ModelStateSetParams>()?;
            let (cwd, thread_id) = resolve_model_state_request_scope(
                &state,
                &auth,
                params.cwd.clone(),
                params.thread_id.clone(),
            )?;
            model_state_set_value(&state, &cwd, thread_id.as_deref(), params)
        }
        "model/assignment/set" => {
            let params = request.required_params::<wire::ModelAssignmentSetParams>()?;
            let cwd = default_resolved_scope(&state, &auth)?.cwd;
            model_assignment_set_value(&state, &cwd, params)
        }
        method => Err(Error::Message(format!("method not found: {method}"))),
    }
}

fn runtime_rpc_error(
    code: &str,
    stage: &str,
    retry_class: wire::RuntimeRetryClassView,
    message: String,
    diagnostic_ref: Option<String>,
) -> Error {
    let view = wire::RuntimeErrorView {
        code: code.to_string(),
        stage: stage.to_string(),
        retry_class,
        message: message.clone(),
        diagnostic_ref,
    };
    Error::structured(
        message,
        serde_json::to_value(view).expect("runtime error view serializes"),
    )
}

fn plugin_runtime_options(state: &WebState, cwd: PathBuf) -> RunOptions {
    let mut options = state.run_options(cwd, None);
    let mut inherited_env = options.inherited_env.take().unwrap_or_default();
    inherited_env.insert(
        "PSYCHEVO_HOME".to_string(),
        state.inner.home.display().to_string(),
    );
    options.inherited_env = Some(inherited_env);
    options
}

fn parse_skill_target(value: Option<&str>) -> psychevo_runtime::Result<SkillTarget> {
    match value.unwrap_or("global") {
        "global" | "profile" => Ok(SkillTarget::Global),
        "project" | "local" => Ok(SkillTarget::Project),
        other => Err(Error::Config(format!("unknown skill target: {other}"))),
    }
}

fn parse_plugin_scope(value: Option<&str>) -> psychevo_runtime::Result<PluginScope> {
    match value.unwrap_or("global") {
        "global" | "profile" => Ok(PluginScope::Global),
        "local" | "project" => Ok(PluginScope::Local),
        other => Err(Error::Config(format!("unknown plugin scope: {other}"))),
    }
}

fn parse_plugin_source_kind(
    value: Option<&str>,
) -> psychevo_runtime::Result<Option<PluginSourceKind>> {
    value
        .map(|value| {
            PluginSourceKind::parse(value).ok_or_else(|| {
                Error::Config(format!(
                    "unknown plugin source kind `{value}`; expected local, git, or npm"
                ))
            })
        })
        .transpose()
}

fn parse_plugin_adapter_mode(
    value: Option<&str>,
) -> psychevo_runtime::Result<Option<PluginAdapterMode>> {
    value
        .map(|value| {
            PluginAdapterMode::parse(value).ok_or_else(|| {
                Error::Config(format!(
                    "unknown plugin adapter mode `{value}`; expected adapter_host, manifest_only, or disabled"
                ))
            })
        })
        .transpose()
}

fn parse_tool_mode(value: &str) -> psychevo_runtime::Result<RunMode> {
    RunMode::parse(value).ok_or_else(|| Error::Config(format!("unknown tool mode: {value}")))
}

fn tool_config_dir(state: &WebState, scope: &ResolvedScope, local: bool) -> PathBuf {
    if local {
        scope.cwd.join(".psychevo")
    } else {
        active_profile_config_dir(state, scope)
    }
}

fn toolset_mutation_value(result: psychevo_runtime::ToolsetMutationResult) -> Value {
    json!({
        "success": true,
        "changed": result.changed,
        "name": result.name,
        "path": result.config_path,
    })
}

fn mcp_config_input(params: wire::McpUpsertParams) -> McpServerConfigInput {
    McpServerConfigInput {
        name: params.name,
        transport: params.transport,
        enabled: params.enabled,
        required: params.required,
        command: params.command,
        args: params.args,
        env: params.env,
        cwd: params.cwd,
        url: params.url,
        headers: params.headers,
        bearer_token_env_var: params.bearer_token_env_var,
        scopes: params.scopes,
        oauth_resource: params.oauth_resource,
        oauth_client_id: params.oauth_client_id,
        enabled_tools: params.enabled_tools,
        disabled_tools: params.disabled_tools,
        supports_parallel_tool_calls: params.supports_parallel_tool_calls,
        startup_timeout_secs: params.startup_timeout_secs,
        tool_timeout_secs: params.tool_timeout_secs,
    }
}

#[derive(Debug, Clone)]
struct McpOAuthMetadata {
    name: String,
    url: String,
    client_id: String,
    scopes: Vec<String>,
    oauth_resource: Option<String>,
    profile_home: PathBuf,
}

async fn mcp_oauth_start_value(
    state: WebState,
    scope: ResolvedScope,
    params: wire::McpOAuthStartParams,
) -> psychevo_runtime::Result<Value> {
    let metadata = mcp_oauth_metadata(&state, &scope, &params.name)?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let redirect_uri = format!("http://{}/callback", listener.local_addr()?);
    let session_id = Uuid::now_v7().to_string();
    let state_token = Uuid::now_v7().to_string();
    let authorization_url = mcp_authorization_url(&metadata, &redirect_uri, &state_token)?;
    let status = Arc::new(Mutex::new(McpOAuthSessionStatus::Pending));
    state
        .inner
        .mcp_oauth_sessions
        .lock()
        .expect("mcp oauth sessions poisoned")
        .insert(
            session_id.clone(),
            McpOAuthSession {
                status: Arc::clone(&status),
            },
        );
    tokio::spawn(run_mcp_oauth_callback(
        listener,
        metadata,
        redirect_uri,
        state_token,
        status,
    ));
    Ok(json!({
        "sessionId": session_id,
        "authorizationUrl": authorization_url,
        "status": "pending",
    }))
}

fn mcp_oauth_status_value(state: &WebState, session_id: &str) -> psychevo_runtime::Result<Value> {
    let sessions = state
        .inner
        .mcp_oauth_sessions
        .lock()
        .expect("mcp oauth sessions poisoned");
    let Some(session) = sessions.get(session_id) else {
        return Err(Error::Config(format!(
            "unknown MCP OAuth session: {session_id}"
        )));
    };
    let status = session.status.lock().expect("mcp oauth session poisoned");
    Ok(match &*status {
        McpOAuthSessionStatus::Pending => json!({
            "sessionId": session_id,
            "status": "pending",
        }),
        McpOAuthSessionStatus::Succeeded => json!({
            "sessionId": session_id,
            "status": "succeeded",
        }),
        McpOAuthSessionStatus::Failed(error) => json!({
            "sessionId": session_id,
            "status": "failed",
            "error": error,
        }),
    })
}

fn mcp_oauth_logout_value(
    state: &WebState,
    scope: &ResolvedScope,
    name: &str,
) -> psychevo_runtime::Result<Value> {
    let metadata = mcp_oauth_metadata(state, scope, name)?;
    let removed =
        clear_mcp_oauth_access_token(&metadata.profile_home, &metadata.name, &metadata.url)?;
    Ok(json!({
        "success": true,
        "name": metadata.name,
        "removed": removed,
    }))
}

fn mcp_oauth_metadata(
    state: &WebState,
    scope: &ResolvedScope,
    name: &str,
) -> psychevo_runtime::Result<McpOAuthMetadata> {
    let options = plugin_runtime_options(state, scope.cwd.clone());
    let value = mcp_server_value(&options, name)?;
    let server = value
        .get("server")
        .ok_or_else(|| Error::Config(format!("unknown MCP server: {name}")))?;
    let transport = server
        .get("transport")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Config(format!("MCP server {name} has no transport")))?;
    if transport.get("kind").and_then(Value::as_str) != Some("streamable_http") {
        return Err(Error::Config(format!(
            "MCP OAuth is only supported for streamable HTTP servers: {name}"
        )));
    }
    let url = transport
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Config(format!("MCP server {name} has no URL")))?
        .to_string();
    let auth = transport
        .get("auth")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Config(format!("MCP server {name} has no OAuth metadata")))?;
    let client_id = auth
        .get("oauthClientId")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Error::Config(format!(
                "MCP server {name} must configure oauth.client_id before OAuth login"
            ))
        })?
        .to_string();
    let scopes = auth
        .get("scopes")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let oauth_resource = auth
        .get("oauthResource")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(McpOAuthMetadata {
        name: name.to_string(),
        url,
        client_id,
        scopes,
        oauth_resource,
        profile_home: active_profile_config_dir(state, scope),
    })
}

fn mcp_authorization_url(
    metadata: &McpOAuthMetadata,
    redirect_uri: &str,
    state_token: &str,
) -> psychevo_runtime::Result<String> {
    let base = metadata
        .oauth_resource
        .as_deref()
        .unwrap_or(metadata.url.as_str());
    let mut url = reqwest::Url::parse(&oauth_endpoint(base, "authorize"))
        .map_err(|err| Error::Config(format!("failed to build OAuth authorization URL: {err}")))?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("response_type", "code");
        query.append_pair("client_id", &metadata.client_id);
        query.append_pair("redirect_uri", redirect_uri);
        query.append_pair("state", state_token);
        if !metadata.scopes.is_empty() {
            query.append_pair("scope", &metadata.scopes.join(" "));
        }
        if let Some(resource) = &metadata.oauth_resource {
            query.append_pair("resource", resource);
        }
    }
    Ok(url.to_string())
}

async fn run_mcp_oauth_callback(
    listener: TcpListener,
    metadata: McpOAuthMetadata,
    redirect_uri: String,
    state_token: String,
    status: Arc<Mutex<McpOAuthSessionStatus>>,
) {
    let result = async {
        let (mut stream, _) = listener.accept().await?;
        let mut buffer = vec![0_u8; 8192];
        let size = stream.read(&mut buffer).await?;
        let request = String::from_utf8_lossy(&buffer[..size]);
        let target = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or("/");
        let callback_url = reqwest::Url::parse(&format!("http://localhost{target}"))
            .map_err(|err| Error::Config(format!("OAuth callback parse failed: {err}")))?;
        let pairs = callback_url
            .query_pairs()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect::<BTreeMap<_, _>>();
        if pairs.get("state") != Some(&state_token) {
            write_oauth_callback_response(&mut stream, false).await?;
            return Err(Error::Config("OAuth callback state mismatch".to_string()));
        }
        let Some(code) = pairs.get("code").cloned() else {
            write_oauth_callback_response(&mut stream, false).await?;
            return Err(Error::Config(
                "OAuth callback did not include code".to_string(),
            ));
        };
        write_oauth_callback_response(&mut stream, true).await?;
        let token = exchange_mcp_oauth_code(&metadata, &redirect_uri, &code).await?;
        save_mcp_oauth_access_token(
            &metadata.profile_home,
            &metadata.name,
            &metadata.url,
            &token,
        )?;
        Ok::<(), Error>(())
    }
    .await;
    let mut status = status.lock().expect("mcp oauth session poisoned");
    *status = match result {
        Ok(()) => McpOAuthSessionStatus::Succeeded,
        Err(err) => McpOAuthSessionStatus::Failed(err.to_string()),
    };
}

async fn write_oauth_callback_response(
    stream: &mut tokio::net::TcpStream,
    success: bool,
) -> std::io::Result<()> {
    let body = if success {
        "OAuth login finished. You can return to Psychevo."
    } else {
        "OAuth login failed. You can close this page."
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes()).await
}

async fn exchange_mcp_oauth_code(
    metadata: &McpOAuthMetadata,
    redirect_uri: &str,
    code: &str,
) -> psychevo_runtime::Result<String> {
    let base = metadata
        .oauth_resource
        .as_deref()
        .unwrap_or(metadata.url.as_str());
    let token_endpoint = oauth_endpoint(base, "token");
    let mut form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
        ("client_id", metadata.client_id.clone()),
    ];
    if !metadata.scopes.is_empty() {
        form.push(("scope", metadata.scopes.join(" ")));
    }
    if let Some(resource) = &metadata.oauth_resource {
        form.push(("resource", resource.clone()));
    }
    let response = reqwest::Client::new()
        .post(token_endpoint)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(form_urlencoded(&form))
        .send()
        .await
        .map_err(|err| Error::Config(format!("OAuth token request failed: {err}")))?;
    let status = response.status();
    let value = response
        .json::<Value>()
        .await
        .map_err(|err| Error::Config(format!("OAuth token response parse failed: {err}")))?;
    if !status.is_success() {
        return Err(Error::Config(format!(
            "OAuth token request failed with HTTP {status}: {value}"
        )));
    }
    value
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|token| !token.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| Error::Config("OAuth token response omitted access_token".to_string()))
}

fn oauth_endpoint(base: &str, suffix: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), suffix)
}

fn form_urlencoded(values: &[(&str, String)]) -> String {
    values
        .iter()
        .map(|(key, value)| format!("{}={}", url_percent_encode(key), url_percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn url_percent_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

async fn thread_compact_result_for_thread(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: String,
    instructions: Option<String>,
    runtime_ref: String,
    out_tx: mpsc::UnboundedSender<String>,
) -> psychevo_runtime::Result<wire::ThreadCompactionResult> {
    let options = state.run_options(scope.cwd.clone(), Some(thread_id.clone()));
    let event_selector = GatewayThreadSelector::thread_id(&thread_id);
    let event_thread_id = thread_id.clone();
    let event_state = state.clone();
    let event_tx = out_tx.clone();
    let event_sink: GatewayEventSink = Arc::new(move |event| {
        let context =
            event_state.pending_context_for_selector(&event_selector, Some(&event_thread_id));
        event_state.record_event_with_context(&event, context.clone());
        let display_event = event_state.event_with_pending_context(event, &context);
        let _ = event_tx.send(rpc_notification("gateway/event", json!(display_event)));
    });
    let result = state
        .inner
        .gateway
        .compact_session(SendCompactRequest {
            thread_id: Some(thread_id.clone()),
            source: Some(scope.source.clone()),
            runtime_ref: Some(runtime_ref),
            cwd: scope.cwd.clone(),
            config_path: options.config_path,
            model: options.model,
            reasoning_effort: options.reasoning_effort,
            instructions: instructions
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            force: true,
            reason: psychevo_runtime::CompactionReason::Manual,
            inherited_env: options.inherited_env,
            event_sink: Some(event_sink),
        })
        .await;
    let response = match result {
        Ok(result) => thread_compact_result(state, result)?,
        Err(err) => wire::ThreadCompactionResult {
            accepted: false,
            thread_id: Some(thread_id),
            compacted: false,
            reason: "error".to_string(),
            message: err.to_string(),
            checkpoint: None,
            tokens_before: None,
            tokens_after: None,
            summary_provider: None,
            summary_model: None,
            unavailable: false,
            error: Some(err.to_string()),
        },
    };
    Ok(response)
}

fn thread_compact_result(
    state: &WebState,
    result: psychevo_runtime::CompactionResult,
) -> psychevo_runtime::Result<wire::ThreadCompactionResult> {
    let checkpoint = match result.checkpoint_id {
        Some(checkpoint_id) => state
            .inner
            .state
            .store()
            .session_compaction(checkpoint_id)?
            .map(|record| wire::ThreadCompactionCheckpointView {
                checkpoint_id: record.id,
                reason: record.reason,
                created_at_ms: record.created_at_ms,
                first_kept_session_seq: record.first_kept_session_seq,
                tokens_before: record.tokens_before,
                tokens_after: record.tokens_after,
                summary_provider: Some(record.summary_provider),
                summary_model: Some(record.summary_model),
                summary: Some(record.summary_text),
            }),
        None => None,
    };
    let unavailable = result.message.to_ascii_lowercase().contains("unavailable");
    Ok(wire::ThreadCompactionResult {
        accepted: true,
        thread_id: Some(result.session_id),
        compacted: result.compacted,
        reason: result.reason,
        message: result.message,
        checkpoint,
        tokens_before: result.tokens_before,
        tokens_after: result.tokens_after,
        summary_provider: result.summary_provider,
        summary_model: result.summary_model,
        unavailable,
        error: None,
    })
}

include!("rpc_dispatch/model_scope.rs");
