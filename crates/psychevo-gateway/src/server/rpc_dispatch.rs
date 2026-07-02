include!("rpc_dispatch/transport.rs");

async fn handle_rpc(
    state: WebState,
    auth: AuthContext,
    out_tx: mpsc::UnboundedSender<String>,
    request: RpcRequest,
) -> psychevo_runtime::Result<Value> {
    match request.method.as_str() {
        "initialize" => {
            let scope = default_resolved_scope(&state, &auth)?;
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
                "automations": true,
                "settingsWrite": "structured",
                "workspaceCreate": true,
                "memoryResources": "status_only"
            }
            }))
        }
        "thread/start" => {
            let params = request.required_params::<wire::ThreadStartParams>()?;
            let scope = resolve_start_scope(&state, &auth, params.scope.clone())?;
            state.inner.gateway.clear_source_binding(&scope.source)?;
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
            thread_snapshot(&state, &scope, thread_id.as_deref())
        }
        "thread/read" => {
            let params = request.required_params::<wire::ThreadReadParams>()?;
            authorize_thread(&state, &auth, &params.thread_id)?;
            let scope = resolved_scope_for_thread(&state, &params.thread_id)?;
            thread_snapshot(&state, &scope, Some(&params.thread_id))
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
            let store = state.inner.state.store();
            let sessions = if params.archived.unwrap_or(false) {
                match cwd.as_ref() {
                    Some(cwd) => {
                        store.list_archived_sessions_for_cwd_with_sources(cwd, &[])?
                    }
                    None => store.list_archived_sessions_with_sources(&[])?,
                }
            } else {
                match cwd.as_ref() {
                    Some(cwd) => store.list_sessions_for_cwd_with_sources(cwd, &[])?,
                    None => store.list_sessions_with_sources(&[])?,
                }
            };
            Ok(json!({
                "sessions": sessions
                    .into_iter()
                    .filter(|session| human_visible_session(&state, session))
                    .take(limit)
                    .map(|session| session_summary_value(&state, session))
                    .collect::<psychevo_runtime::Result<Vec<_>>>()?,
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
            guard_session_mutation(&state, &auth, &params.thread_id, true)?;
            state
                .inner
                .state
                .store()
                .archive_session(&params.thread_id)?;
            Ok(json!({"session": session_summary_by_id(&state, &params.thread_id)?}))
        }
        "thread/restore" => {
            let params = request.required_params::<wire::ThreadIdParams>()?;
            authorize_thread(&state, &auth, &params.thread_id)?;
            guard_session_mutation(&state, &auth, &params.thread_id, true)?;
            state
                .inner
                .state
                .store()
                .restore_session(&params.thread_id)?;
            Ok(json!({"session": session_summary_by_id(&state, &params.thread_id)?}))
        }
        "thread/delete" => {
            let params = request.required_params::<wire::ThreadIdParams>()?;
            authorize_thread(&state, &auth, &params.thread_id)?;
            guard_session_mutation(&state, &auth, &params.thread_id, false)?;
            state.inner.state.delete_session(&params.thread_id)?;
            Ok(json!({"deleted": true, "threadId": params.thread_id}))
        }
        "runtime/options" => {
            let params = request.required_params::<wire::RuntimeOptionsParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            if let Some(thread_id) = params.thread_id.as_deref() {
                authorize_thread(&state, &auth, thread_id)?;
            }
            let runtime_ref = params.runtime_ref.trim();
            if runtime_ref.is_empty() || runtime_ref == "native" {
                return Ok(serde_json::to_value(wire::RuntimeOptionsResult {
                    runtime_ref: "native".to_string(),
                    runtime_session_id: None,
                    options: vec![native_runtime_mode_option()],
                })?);
            }

            let mut options = state.run_options(scope.cwd.clone(), params.thread_id.clone());
            options.runtime_ref = Some(runtime_ref.to_string());
            options.runtime_session_id = params.runtime_session_id.clone();
            let peer = crate::resolve_peer_turn(&options)?
                .ok_or_else(|| Error::Message(format!("unknown ACP runtime: {runtime_ref}")))?;
            let runtime_options = crate::acp_peer::read_acp_peer_runtime_options(
                peer,
                scope.cwd.clone(),
                params.runtime_session_id.clone(),
            )
            .await?;
            Ok(serde_json::to_value(wire::RuntimeOptionsResult {
                runtime_ref: runtime_ref.to_string(),
                runtime_session_id: runtime_options.native_session_id,
                options: runtime_options.options,
            })?)
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
            let mode = params
                .mode
                .as_deref()
                .map(|mode| {
                    RunMode::parse(mode)
                        .ok_or_else(|| Error::Message(format!("unknown mode: {mode}")))
                })
                .transpose()?;
            let permission_mode = params
                .permission_mode
                .as_deref()
                .map(|permission_mode| {
                    PermissionMode::parse(permission_mode).ok_or_else(|| {
                        Error::Message(format!("unknown permission mode: {permission_mode}"))
                    })
                })
                .transpose()?;
            let mut mention_validation = state.run_options(scope.cwd.clone(), None);
            mention_validation.runtime_ref = params.runtime_ref.clone();
            apply_mentions_to_run_options(&mut mention_validation, &params.mentions)?;

            let thread_id = if requested_side_conversation_thread {
                requested_thread_id
            } else {
                ensure_turn_start_thread(&state, &scope, requested_thread_id)?
            };
            let mut options = state.run_options(scope.cwd.clone(), thread_id.clone());
            options.model = params.model;
            options.reasoning_effort = params.reasoning_effort;
            options.runtime_ref = params.runtime_ref.clone();
            options.runtime_session_id = params.runtime_session_id.clone();
            options.runtime_options = params.runtime_options.clone();
            if let Some(mode) = mode {
                options.mode = mode;
            }
            if let Some(permission_mode) = permission_mode {
                options.permission_mode = Some(permission_mode);
            }
            options.agent = params.agent_name.clone();
            apply_mentions_to_run_options(&mut options, &params.mentions)?;
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
            let gateway = state.inner.gateway.clone();
            let bind_source =
                (!requested_side_conversation_thread).then(|| cwd_source(&scope.cwd));
            let requested_thread_id = thread_id.clone();
            tokio::spawn(async move {
                let result = gateway
                    .send_turn(crate::SendTurnRequest {
                        thread_id,
                        source,
                        bind_source,
                        reset_source_binding: false,
                        input,
                        options,
                        runtime_source: Some("web".to_string()),
                        continue_sources: vec![
                            "run".to_string(),
                            "tui".to_string(),
                            "web".to_string(),
                        ],
                        stream: None,
                        event_sink: Some(event_sink.clone()),
                        control_handle: None,
                        control: None,
                        lineage: None,
                    })
                    .await;
                let notification = match result {
                    Ok(result) => {
                        rpc_notification("turn/result", gateway_turn_result_value(result))
                    }
                    Err(err) => rpc_notification(
                        "turn/error",
                        json!({"message": err.to_string(), "threadId": requested_thread_id}),
                    ),
                };
                let _ = out_tx.send(notification);
            });
            Ok(json!({"accepted": true}))
        }
        "turn/steer" => {
            let params = request.required_params::<wire::TurnSteerParams>()?;
            if let Some(thread_id) = &params.thread_id {
                authorize_thread(&state, &auth, thread_id)?;
            }
            let message = RuntimeMessage::User {
                content: vec![UserContentBlock::text(params.text)],
                timestamp_ms: gateway_now_ms(),
            };
            let selector = selector_from_thread_or_default(&state, &auth, params.thread_id)?;
            let accepted = state
                .inner
                .gateway
                .steer_turn(
                    selector.clone(),
                    Some(&params.expected_turn_id),
                    message.clone(),
                )
                .is_some()
                || state.inner.gateway.steer_foreign_turn(
                    selector,
                    Some(&params.expected_turn_id),
                    message,
                );
            Ok(json!({"accepted": accepted}))
        }
        "turn/interrupt" => {
            let params = request.params::<wire::TurnInterruptParams>()?;
            if let Some(thread_id) = &params.thread_id {
                authorize_thread(&state, &auth, thread_id)?;
            }
            let selector = if let Some(thread_id) = params.thread_id {
                GatewayThreadSelector::thread_id(thread_id)
            } else if let Some(source_key) = params.source_key {
                GatewayThreadSelector::source(source_key)
            } else {
                let scope = default_resolved_scope(&state, &auth)?;
                state.selector(&scope.source)
            };
            let interrupted = state.inner.gateway.interrupt_turn(selector.clone());
            let cleared = state.inner.gateway.clear_queue(selector);
            Ok(json!({"interrupted": interrupted, "cleared": cleared}))
        }
        "turn/takeover" => {
            let params = request.params::<wire::TurnTakeoverParams>()?;
            if let Some(thread_id) = &params.thread_id {
                authorize_thread(&state, &auth, thread_id)?;
            }
            let selector = if let Some(thread_id) = params.thread_id {
                GatewayThreadSelector::thread_id(thread_id)
            } else if let Some(source_key) = params.source_key {
                GatewayThreadSelector::source(source_key)
            } else {
                let scope = default_resolved_scope(&state, &auth)?;
                state.selector(&scope.source)
            };
            let (accepted, activity) = state.inner.gateway.takeover_turn(selector)?;
            Ok(json!({"accepted": accepted, "activity": activity}))
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
            reset_source_to_empty(&state, &scope)
        }
        "permission/respond" => {
            let params = request.required_params::<wire::PermissionRespondParams>()?;
            let decision = permission_decision(params.decision);
            let selector = selector_from_interaction_context(
                &state,
                &auth,
                params.thread_id,
                params.source_key,
                params.activity_id,
            )?;
            let accepted =
                state
                    .inner
                    .gateway
                    .submit_permission(selector, &params.request_id, decision);
            if !accepted {
                state.remove_pending_permission(&params.request_id);
            }
            Ok(json!({"accepted": accepted}))
        }
        "clarify/respond" => {
            let params = request.required_params::<wire::ClarifyRespondParams>()?;
            let result = if params.cancel.unwrap_or(false) {
                ClarifyResult::Cancelled
            } else {
                ClarifyResult::Answered(ClarifyResponse {
                    answers: params
                        .answers
                        .unwrap_or_default()
                        .into_iter()
                        .map(|answers| ClarifyAnswer { answers })
                        .collect(),
                })
            };
            let selector = selector_from_interaction_context(
                &state,
                &auth,
                params.thread_id,
                params.source_key,
                params.activity_id,
            )?;
            let accepted = state
                .inner
                .gateway
                .submit_clarify(selector, &params.request_id, result);
            Ok(json!({"accepted": accepted}))
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
            write_project_agent_definition(&scope.cwd, params)
        }
        "agent/delete" => {
            let params = request.required_params::<wire::AgentDeleteParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            delete_project_agent_definition(&scope.cwd, &params.name)
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
        "backend/list" => {
            let params = request.params::<wire::BackendListParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
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
            let backends = load_agent_backend_configs(
                &state.inner.home,
                &scope.cwd,
                &state.inner.inherited_env,
            )?;
            let backend = backends
                .get(&params.id)
                .ok_or_else(|| Error::Message(format!("unknown backend: {}", params.id)))?;
            Ok(serde_json::to_value(backend_doctor_value(
                backend,
                &state.inner.inherited_env,
            )?)?)
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
            let options = plugin_runtime_options(&state, scope.cwd);
            plugin_list_value(&options)
        }
        "plugin/read" => {
            let params = request.required_params::<wire::PluginReadParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let options = plugin_runtime_options(&state, scope.cwd);
            plugin_view_value(&options, &params.selector)
        }
        "plugin/doctor" => {
            let params = request.params::<wire::PluginDoctorParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            let options = plugin_runtime_options(&state, scope.cwd);
            plugin_doctor_value(&options, params.selector.as_deref())
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
            command_execute_value(&state, &scope, params)
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

include!("rpc_dispatch/model_scope.rs");
