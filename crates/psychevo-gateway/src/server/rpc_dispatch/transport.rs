async fn readyz() -> impl IntoResponse {
    Json(wire::ReadyzResult {
        ok: true,
        server: "psychevo-gateway".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

async fn managed_identity(State(state): State<WebState>, headers: HeaderMap) -> impl IntoResponse {
    if !state
        .auth_from_headers(&headers)
        .is_some_and(|auth| auth.is_bearer())
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(instance_id) = state.inner.managed_instance_id.as_deref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    Json(json!({
        "ok": true,
        "instanceId": instance_id,
        "pid": std::process::id(),
        "version": env!("CARGO_PKG_VERSION"),
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagedShutdownParams {
    instance_id: String,
}

async fn managed_shutdown(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(params): Json<ManagedShutdownParams>,
) -> impl IntoResponse {
    if !state
        .auth_from_headers(&headers)
        .is_some_and(|auth| auth.is_bearer())
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(instance_id) = state.inner.managed_instance_id.as_deref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if params.instance_id != instance_id {
        return (
            StatusCode::CONFLICT,
            Json(json!({"ok": false, "error": "managed instance mismatch"})),
        )
            .into_response();
    }
    let Some(shutdown) = state.inner.managed_shutdown_tx.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let _ = shutdown.send(true);
    Json(json!({"ok": true, "instanceId": instance_id})).into_response()
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
            BrowserSession::with_external_action_grant(entry.cwd, entry.source),
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
    let permits = Arc::new(Semaphore::new(RPC_IN_FLIGHT_LIMIT));
    let mut requests = JoinSet::new();
    let mut pending = std::collections::VecDeque::new();

    loop {
        let input = if pending.is_empty() {
            SocketInput::Message(next_socket_message(&mut receiver, &mut requests).await)
        } else {
            next_socket_message_or_permit(&mut receiver, &mut requests, permits.clone()).await
        };
        match input {
            SocketInput::Message(Some(Ok(WsMessage::Text(text)))) => {
                let text = text.to_string();
                if pending.len() < RPC_PENDING_LIMIT {
                    pending.push_back(text);
                } else if let Some(response) = rpc_capacity_error(&text) {
                    let _ = out_tx.send(response);
                };
            }
            SocketInput::Permit(Some(permit)) => {
                let Some(text) = pending.pop_front() else {
                    continue;
                };
                let request_state = state.clone();
                let request_auth = auth.clone();
                let request_out_tx = out_tx.clone();
                let response_out_tx = out_tx.clone();
                spawn_bounded_rpc_response(
                    &mut requests,
                    permit,
                    response_out_tx,
                    async move {
                        handle_rpc_text(
                            &request_state,
                            &request_auth,
                            request_out_tx,
                            &text,
                        )
                        .await
                    },
                );
            }
            SocketInput::Message(Some(Ok(WsMessage::Close(_))))
            | SocketInput::Message(Some(Err(_)))
            | SocketInput::Message(None)
            | SocketInput::Permit(None) => break,
            _ => {}
        }
    }
    requests.abort_all();
    while requests.join_next().await.is_some() {}
    relay.abort();
    drop(out_tx);
    let _ = relay.await;
    let _ = writer.await;
}

enum SocketInput<T> {
    Message(Option<T>),
    Permit(Option<OwnedSemaphorePermit>),
}

async fn next_socket_message_or_permit<S>(
    receiver: &mut S,
    requests: &mut JoinSet<()>,
    permits: Arc<Semaphore>,
) -> SocketInput<S::Item>
where
    S: futures::Stream + Unpin,
{
    loop {
        tokio::select! {
            completed = requests.join_next(), if !requests.is_empty() => {
                let _ = completed;
            }
            message = receiver.next() => return SocketInput::Message(message),
            permit = permits.clone().acquire_owned() => return SocketInput::Permit(permit.ok()),
        }
    }
}

fn rpc_capacity_error(text: &str) -> Option<String> {
    let request = serde_json::from_str::<RpcRequest>(text).ok()?;
    let id = request.id?;
    Some(rpc_error(
        id,
        -32001,
        "too many queued requests on this connection".to_string(),
    ))
}

async fn next_socket_message<S>(receiver: &mut S, requests: &mut JoinSet<()>) -> Option<S::Item>
where
    S: futures::Stream + Unpin,
{
    loop {
        tokio::select! {
            completed = requests.join_next(), if !requests.is_empty() => {
                let _ = completed;
            }
            message = receiver.next() => return message,
        }
    }
}

fn spawn_bounded_rpc_response<F>(
    requests: &mut JoinSet<()>,
    permit: OwnedSemaphorePermit,
    out_tx: mpsc::UnboundedSender<String>,
    response: F,
) where
    F: Future<Output = Option<String>> + Send + 'static,
{
    requests.spawn(async move {
        let _permit = permit;
        if let Some(response) = response.await {
            let _ = out_tx.send(response);
        }
    });
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

#[cfg(test)]
mod transport_tests {
    use super::*;
    use tokio::sync::{Notify, Semaphore};
    use tokio::task::JoinSet;

    #[tokio::test]
    async fn held_request_does_not_block_an_independent_response() {
        let permits = Arc::new(Semaphore::new(RPC_IN_FLIGHT_LIMIT));
        let (out_tx, mut out_rx) = mpsc::unbounded_channel();
        let held = Arc::new(Notify::new());
        let mut requests = JoinSet::new();

        let first_permit = permits.clone().acquire_owned().await.expect("first permit");
        let first_held = held.clone();
        spawn_bounded_rpc_response(&mut requests, first_permit, out_tx.clone(), async move {
            first_held.notified().await;
            Some("first".to_string())
        });
        let second_permit = permits.clone().acquire_owned().await.expect("second permit");
        spawn_bounded_rpc_response(&mut requests, second_permit, out_tx.clone(), async {
            Some("second".to_string())
        });

        assert_eq!(out_rx.recv().await.as_deref(), Some("second"));
        held.notify_one();
        assert_eq!(out_rx.recv().await.as_deref(), Some("first"));
        while requests.join_next().await.is_some() {}
        assert_eq!(permits.available_permits(), RPC_IN_FLIGHT_LIMIT);
    }

    #[tokio::test]
    async fn in_flight_limit_is_fixed_and_abort_releases_every_permit() {
        let permits = Arc::new(Semaphore::new(RPC_IN_FLIGHT_LIMIT));
        let (out_tx, _out_rx) = mpsc::unbounded_channel();
        let mut requests = JoinSet::new();
        for _ in 0..RPC_IN_FLIGHT_LIMIT {
            let permit = permits.clone().acquire_owned().await.expect("bounded permit");
            spawn_bounded_rpc_response(&mut requests, permit, out_tx.clone(), async {
                std::future::pending::<Option<String>>().await
            });
        }

        assert!(permits.clone().try_acquire_owned().is_err());
        requests.abort_all();
        while requests.join_next().await.is_some() {}
        assert_eq!(permits.available_permits(), RPC_IN_FLIGHT_LIMIT);
    }

    #[tokio::test]
    async fn completed_requests_are_reaped_before_the_next_socket_message() {
        let mut requests = JoinSet::new();
        for _ in 0..128 {
            requests.spawn(async {});
        }
        let mut messages = Box::pin(futures::stream::once(async {
            tokio::time::sleep(Duration::from_millis(25)).await;
            "next"
        }));

        assert_eq!(
            next_socket_message(&mut messages, &mut requests).await,
            Some("next")
        );
        assert_eq!(requests.len(), 0);
    }

    #[tokio::test]
    async fn saturated_permit_wait_still_observes_socket_disconnect() {
        let permits = Arc::new(Semaphore::new(1));
        let held_permit = permits.clone().acquire_owned().await.expect("held permit");
        let mut requests = JoinSet::new();
        let mut messages = futures::stream::iter(["close"]);

        let input = tokio::time::timeout(
            Duration::from_millis(100),
            next_socket_message_or_permit(&mut messages, &mut requests, permits),
        )
        .await
        .expect("disconnect must not wait for a request permit");

        assert!(matches!(input, SocketInput::Message(Some("close"))));
        drop(held_permit);
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
use std::future::Future;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinSet;

const RPC_IN_FLIGHT_LIMIT: usize = 32;
const RPC_PENDING_LIMIT: usize = 32;
