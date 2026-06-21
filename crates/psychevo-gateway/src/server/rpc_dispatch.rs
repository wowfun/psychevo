async fn readyz() -> impl IntoResponse {
    Json(wire::ReadyzResult {
        ok: true,
        server: "psychevo-gateway".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LaunchQuery {
    open_token: String,
}

async fn ws_handler(
    State(state): State<WebState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let Some(auth) = state.auth_from_headers(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    ws.on_upgrade(move |socket| handle_socket(socket, state, auth))
}

async fn create_launch(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(params): Json<wire::CreateLaunchParams>,
) -> impl IntoResponse {
    if !state
        .auth_from_headers(&headers)
        .is_some_and(|auth| auth.is_bearer())
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let workdir = match canonicalize_workdir(Path::new(&params.workdir)) {
        Ok(workdir) => workdir,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": {"message": err.to_string()}})),
            )
                .into_response();
        }
    };
    let source = source_from_input(
        params.source,
        &workdir,
        wire::GatewaySourceLifetime::Persistent,
    );
    let launch_id = Uuid::now_v7().to_string();
    let open_token = Uuid::now_v7().to_string();
    let expires_at_ms = now_ms() + 30_000;
    state
        .inner
        .launches
        .lock()
        .expect("web launches poisoned")
        .insert(
            launch_id.clone(),
            LaunchEntry {
                open_token: open_token.clone(),
                expires_at_ms,
                workdir,
                source,
            },
        );
    let host = headers
        .get("host")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("127.0.0.1");
    let open_url = format!("http://{host}/_gateway/launch/{launch_id}?openToken={open_token}");
    Json(wire::CreateLaunchResult {
        launch_id,
        expires_at_ms,
        open_url,
    })
    .into_response()
}

async fn consume_launch(
    State(state): State<WebState>,
    AxumPath(launch_id): AxumPath<String>,
    Query(query): Query<LaunchQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let entry = {
        let mut launches = state.inner.launches.lock().expect("web launches poisoned");
        let Some(entry) = launches.remove(&launch_id) else {
            if state.auth_from_headers(&headers).is_some() {
                return shell_redirect().into_response();
            }
            return launch_expired_page(StatusCode::NOT_FOUND).into_response();
        };
        entry
    };
    if entry.expires_at_ms < now_ms() || entry.open_token != query.open_token {
        if state.auth_from_headers(&headers).is_some() {
            return shell_redirect().into_response();
        }
        return launch_expired_page(StatusCode::UNAUTHORIZED).into_response();
    }
    let session_id = Uuid::now_v7().to_string();
    state
        .inner
        .browser_sessions
        .lock()
        .expect("web browser sessions poisoned")
        .insert(
            session_id.clone(),
            BrowserSession {
                workdir: entry.workdir,
                source: entry.source,
            },
        );
    let mut response = shell_redirect();
    let secure = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|proto| proto == "https");
    let cookie = if secure {
        format!("psychevo_gateway_session={session_id}; Path=/; HttpOnly; SameSite=Lax; Secure")
    } else {
        format!("psychevo_gateway_session={session_id}; Path=/; HttpOnly; SameSite=Lax")
    };
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(SET_COOKIE, value);
    }
    response.into_response()
}

fn shell_redirect() -> Response<Body> {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::SEE_OTHER;
    response
        .headers_mut()
        .insert(LOCATION, HeaderValue::from_static("/"));
    response
}

async fn handle_socket(socket: WebSocket, state: WebState, auth: AuthContext) {
    let (mut sender, mut receiver) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
    let relay = tokio::spawn(spawn_gateway_live_event_relay(
        state.clone(),
        out_tx.clone(),
    ));
    let writer = tokio::spawn(async move {
        while let Some(message) = out_rx.recv().await {
            if sender.send(WsMessage::Text(message.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(message) = receiver.next().await {
        let Ok(message) = message else {
            break;
        };
        match message {
            WsMessage::Text(text) => {
                let response = handle_rpc_text(&state, &auth, out_tx.clone(), text.as_str()).await;
                if let Some(response) = response {
                    let _ = out_tx.send(response);
                }
            }
            WsMessage::Close(_) => break,
            _ => {}
        }
    }
    relay.abort();
    drop(out_tx);
    let _ = relay.await;
    let _ = writer.await;
}

async fn spawn_gateway_live_event_relay(state: WebState, out_tx: mpsc::UnboundedSender<String>) {
    let mut last_seq = state
        .inner
        .state
        .store()
        .latest_gateway_live_event_seq()
        .unwrap_or_default();
    let mut snapshot_revisions: HashMap<String, i64> = HashMap::new();
    let mut last_cleanup_ms = gateway_now_ms();
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    loop {
        tick.tick().await;
        let events = match state
            .inner
            .state
            .store()
            .list_gateway_live_events_after(last_seq, 100)
        {
            Ok(events) => events,
            Err(_) => continue,
        };
        for record in events {
            last_seq = last_seq.max(record.seq);
            if record.owner_id.as_deref() == Some(state.inner.gateway.owner_id()) {
                continue;
            }
            let Ok(event) = serde_json::from_value::<GatewayEvent>(record.event.clone()) else {
                continue;
            };
            let context = state.pending_context_for_live_event(&record);
            state.record_event_with_context(&event, context.clone());
            let display_event = state.event_with_pending_context(event, &context);
            if out_tx
                .send(rpc_notification("gateway/event", json!(display_event)))
                .is_err()
            {
                return;
            }
        }
        let now = gateway_now_ms();
        let snapshots = match state.inner.state.store().list_gateway_live_snapshots(1000) {
            Ok(snapshots) => snapshots,
            Err(_) => continue,
        };
        for snapshot in snapshots {
            if snapshot.owner_id.as_deref() == Some(state.inner.gateway.owner_id()) {
                continue;
            }
            if snapshot_revisions
                .get(&snapshot.snapshot_key)
                .is_some_and(|revision| *revision >= snapshot.revision)
            {
                continue;
            }
            if let Some(activity_id) = snapshot.activity_id.as_deref() {
                let Ok(Some(activity)) = state.inner.state.store().gateway_activity(activity_id)
                else {
                    continue;
                };
                if !matches!(activity.status.as_str(), "running" | "queued")
                    || activity.lease_expires_at_ms < now
                {
                    continue;
                }
            }
            let Ok(event) = serde_json::from_value::<GatewayEvent>(snapshot.event.clone()) else {
                continue;
            };
            snapshot_revisions.insert(snapshot.snapshot_key, snapshot.revision);
            state.record_event_with_context(&event, PendingInteractionContext::default());
            if out_tx
                .send(rpc_notification("gateway/event", json!(event)))
                .is_err()
            {
                return;
            }
        }
        if now.saturating_sub(last_cleanup_ms) > 60_000 {
            let _ = state
                .inner
                .state
                .store()
                .cleanup_gateway_live_events_before(now - 10 * 60_000);
            let _ = state
                .inner
                .state
                .store()
                .cleanup_gateway_live_snapshots_before(now - 10 * 60_000);
            last_cleanup_ms = now;
        }
    }
}

async fn handle_rpc_text(
    state: &WebState,
    auth: &AuthContext,
    out_tx: mpsc::UnboundedSender<String>,
    text: &str,
) -> Option<String> {
    let request = match serde_json::from_str::<RpcRequest>(text) {
        Ok(request) => request,
        Err(err) => {
            return Some(rpc_error(
                Value::Null,
                -32700,
                format!("invalid json: {err}"),
            ));
        }
    };
    if request.jsonrpc != wire::JSONRPC_VERSION {
        return Some(rpc_error(
            request.id.clone().unwrap_or(Value::Null),
            -32600,
            "invalid JSON-RPC version".to_string(),
        ));
    }
    let id = request.id.clone()?;
    let result = handle_rpc(state.clone(), auth.clone(), out_tx, request).await;
    Some(match result {
        Ok(value) => rpc_result(id, value),
        Err(err) => rpc_error(id, -32000, err.to_string()),
    })
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
            Ok(json!({
            "server": "psychevo-gateway",
            "version": env!("CARGO_PKG_VERSION"),
            "cwd": scope.workdir,
            "scope": scope.to_wire_scope(),
            "source": scope.source,
            "profile": gateway_profile_value(&state),
            "capabilities": {
                "threads": true,
                "turns": true,
                "historyManagement": true,
                "downloads": true,
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
            let workdir = resolve_session_workdir_filter(&state, &auth, params.workdir)?;
            let store = state.inner.state.store();
            let sessions = if params.archived.unwrap_or(false) {
                match workdir.as_ref() {
                    Some(workdir) => {
                        store.list_archived_sessions_for_workdir_with_sources(workdir, &[])?
                    }
                    None => store.list_archived_sessions_with_sources(&[])?,
                }
            } else {
                match workdir.as_ref() {
                    Some(workdir) => store.list_sessions_for_workdir_with_sources(workdir, &[])?,
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
            let requested_workdir = params
                .workdir
                .clone()
                .or_else(|| params.cursor.as_ref().map(|cursor| cursor.workdir.clone()));
            let workdir = resolve_session_workdir_filter(&state, &auth, requested_workdir)?;
            thread_browser_value(&state, params, workdir)
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

            let mut options = state.run_options(scope.workdir.clone(), params.thread_id.clone());
            options.runtime_ref = Some(runtime_ref.to_string());
            options.runtime_session_id = params.runtime_session_id.clone();
            let peer = crate::resolve_peer_turn(&options)?
                .ok_or_else(|| Error::Message(format!("unknown ACP runtime: {runtime_ref}")))?;
            let runtime_options = crate::acp_peer::read_acp_peer_runtime_options(
                peer,
                scope.workdir.clone(),
                params.runtime_session_id.clone(),
            )
            .await?;
            Ok(serde_json::to_value(wire::RuntimeOptionsResult {
                runtime_ref: runtime_ref.to_string(),
                runtime_session_id: runtime_options.native_session_id,
                options: runtime_options.options,
            })?)
        }
        "turn/start" => {
            let params = request.required_params::<wire::TurnStartParams>()?;
            let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
            let input = params.input_parts()?;
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
            let mut options = state.run_options(scope.workdir.clone(), thread_id.clone());
            options.model = params.model;
            options.reasoning_effort = params.reasoning_effort;
            options.runtime_ref = params.runtime_ref.clone();
            options.runtime_session_id = params.runtime_session_id.clone();
            options.runtime_options = params.runtime_options.clone();
            if let Some(mode) = params.mode.as_deref() {
                options.mode = RunMode::parse(mode)
                    .ok_or_else(|| Error::Message(format!("unknown mode: {mode}")))?;
            }
            if let Some(permission_mode) = params.permission_mode.as_deref() {
                options.permission_mode =
                    Some(PermissionMode::parse(permission_mode).ok_or_else(|| {
                        Error::Message(format!("unknown permission mode: {permission_mode}"))
                    })?);
            }
            options.agent = params.agent_name.clone();
            apply_mentions_to_run_options(&mut options, &params.mentions)?;
            let source = scope.source.clone();
            let event_selector = thread_id
                .as_ref()
                .map(GatewayThreadSelector::thread_id)
                .unwrap_or_else(|| GatewayThreadSelector::source(scope.source.source_key()));
            let event_thread_id = thread_id.clone();
            let event_state = state.clone();
            let review_workdir = scope.workdir.clone();
            let event_tx = out_tx.clone();
            let event_sink: GatewayEventSink = Arc::new(move |event| {
                let context = event_state
                    .pending_context_for_selector(&event_selector, event_thread_id.as_deref());
                event_state.record_event_with_context(&event, context.clone());
                event_state.record_review_event(&event, &review_workdir);
                let display_event = event_state.event_with_pending_context(event, &context);
                let _ = event_tx.send(rpc_notification("gateway/event", json!(display_event)));
            });
            let gateway = state.inner.gateway.clone();
            let bind_source = workdir_source(&scope.workdir);
            let requested_thread_id = thread_id.clone();
            tokio::spawn(async move {
                let result = gateway
                    .send_turn(crate::SendTurnRequest {
                        thread_id,
                        source: Some(source),
                        bind_source: Some(bind_source),
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
                &scope.workdir,
                &state.inner.inherited_env,
            )?;
            Ok(serde_json::to_value(agent_read_result(&agent))?)
        }
        "agent/write" => {
            let params = request.required_params::<wire::AgentWriteParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            write_project_agent_definition(&scope.workdir, params)
        }
        "agent/delete" => {
            let params = request.required_params::<wire::AgentDeleteParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            delete_project_agent_definition(&scope.workdir, &params.name)
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
                &scope.workdir,
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
                &scope.workdir,
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
        "channel/doctor" => {
            let params = request.params::<wire::ChannelDoctorParams>()?;
            let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
            Ok(serde_json::to_value(
                channel_doctor_result_live(&state, &scope, params).await?,
            )?)
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
            let bind_source = workdir_source(&scope.workdir);
            let workdir = scope.workdir.clone();
            let result_thread_id = thread_id.clone();
            tokio::spawn(async move {
                let result = gateway
                    .send_shell(SendShellRequest {
                        thread_id: result_thread_id.clone(),
                        source: Some(source),
                        bind_source: Some(bind_source),
                        workdir,
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
            let (workdir, thread_id) = if let Some(thread_id) = params.thread_id {
                authorize_thread(&state, &auth, &thread_id)?;
                let summary = state
                    .inner
                    .state
                    .store()
                    .session_summary(&thread_id)?
                    .ok_or_else(|| Error::Message(format!("session not found: {thread_id}")))?;
                (PathBuf::from(summary.workdir), Some(thread_id))
            } else {
                (resolve_workdir_filter(&state, &auth, params.workdir)?, None)
            };
            settings_read_value(&state, &workdir, thread_id.as_deref())
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
            settings_read_value(&state, &scope.workdir, Some(&params.thread_id))
        }
        method => Err(Error::Message(format!("method not found: {method}"))),
    }
}
