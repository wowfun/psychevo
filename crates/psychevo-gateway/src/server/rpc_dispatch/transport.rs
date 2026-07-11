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
    let cwd = match canonicalize_cwd(Path::new(&params.cwd)) {
        Ok(cwd) => cwd,
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
        &cwd,
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
                cwd,
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
                cwd: entry.cwd,
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
        Err(err) => rpc_error_with_data(
            id,
            -32000,
            err.to_string(),
            err.structured_data().cloned(),
        ),
    })
}
