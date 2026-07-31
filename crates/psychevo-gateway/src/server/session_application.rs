use super::*;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use psychevo::__product::persistence::SessionListCursor;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadListCursor {
    cwd: Option<String>,
    archived: bool,
    position: SessionListCursor,
}

pub(super) async fn resume(
    state: &WebState,
    auth: &AuthContext,
    params: wire::ThreadResumeParams,
) -> psychevo::Result<wire::ThreadSnapshot> {
    let (thread_id, scope) = match params.thread_id {
        Some(thread_id) => {
            authorize_thread(state, auth, &thread_id).await?;
            let scope = resolved_scope_for_thread(state, &thread_id).await?;
            bind_source_to_thread(state, &scope, &thread_id).await?;
            grant_browser_session_scope(state, auth, &scope);
            (Some(thread_id), scope)
        }
        None => {
            let scope = resolve_optional_scope(state, auth, params.scope)?;
            let thread_id = state
                .inner
                .gateway
                .resolve_source_thread(&scope.source)
                .await?;
            (thread_id, scope)
        }
    };
    decode_result(
        thread_snapshot_live(state, &scope, thread_id.as_deref()).await?,
        "thread/resume",
    )
}

pub(super) async fn read(
    state: &WebState,
    auth: &AuthContext,
    params: wire::ThreadReadParams,
) -> psychevo::Result<wire::ThreadSnapshot> {
    authorize_thread(state, auth, &params.thread_id).await?;
    let scope = resolved_scope_for_thread(state, &params.thread_id).await?;
    decode_result(
        thread_snapshot_live(state, &scope, Some(&params.thread_id)).await?,
        "thread/read",
    )
}

pub(super) async fn trace(
    state: &WebState,
    auth: &AuthContext,
    params: wire::ThreadTraceParams,
) -> psychevo::Result<wire::ThreadTraceResult> {
    authorize_thread(state, auth, &params.thread_id).await?;
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
    Ok(wire::ThreadTraceResult {
        thread_id: result.thread_id,
        available: result.available,
        events: result.events,
        warnings: result.warnings,
        truncated: result.truncated,
        next_after_seq: result.next_after_seq,
    })
}

pub(super) async fn list(
    state: &WebState,
    auth: &AuthContext,
    params: wire::ThreadListParams,
) -> psychevo::Result<wire::ThreadListResult> {
    let limit = params.limit.unwrap_or(50).clamp(1, 200);
    let cwd = resolve_session_cwd_filter(state, auth, params.cwd)?;
    let cwd = cwd.map(|cwd| cwd.to_string_lossy().into_owned());
    let archived = params.archived.unwrap_or(false);
    let cursor = params
        .cursor
        .as_deref()
        .map(|cursor| decode_thread_list_cursor(cursor, cwd.as_deref(), archived))
        .transpose()?;
    let (framework_revision, activity_snapshot) = state.session_activity_snapshot().await?;
    let page = state
        .inner
        .state
        .list_human_session_projections(cwd.as_deref(), archived, cursor.as_ref(), limit)
        .await?;
    let sessions = page
        .sessions
        .into_iter()
        .map(|projection| {
            let activity = activity_snapshot
                .get(&projection.summary.id)
                .cloned()
                .unwrap_or_else(|| GatewayActivity {
                    framework_revision: Some(framework_revision.clone()),
                    ..GatewayActivity::default()
                });
            decode_result(session_summary_value(projection, activity), "thread/list")
        })
        .collect::<psychevo::Result<Vec<_>>>()?;
    let next_cursor = page
        .next_cursor
        .map(|position| encode_thread_list_cursor(cwd, archived, position))
        .transpose()?;
    Ok(wire::ThreadListResult {
        sessions,
        next_cursor,
    })
}

fn encode_thread_list_cursor(
    cwd: Option<String>,
    archived: bool,
    position: SessionListCursor,
) -> psychevo::Result<String> {
    Ok(
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&ThreadListCursor {
            cwd,
            archived,
            position,
        })?),
    )
}

fn decode_thread_list_cursor(
    encoded: &str,
    cwd: Option<&str>,
    archived: bool,
) -> psychevo::Result<SessionListCursor> {
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| Error::Message("invalid thread list cursor".to_string()))?;
    let cursor = serde_json::from_slice::<ThreadListCursor>(&bytes)
        .map_err(|_| Error::Message("invalid thread list cursor".to_string()))?;
    if cursor.cwd.as_deref() != cwd || cursor.archived != archived {
        return Err(Error::Message(
            "thread list cursor does not match the current filters".to_string(),
        ));
    }
    Ok(cursor.position)
}

pub(super) async fn browse(
    state: &WebState,
    auth: &AuthContext,
    params: wire::ThreadBrowserParams,
) -> psychevo::Result<wire::ThreadBrowserResult> {
    let requested_cwd = params
        .cwd
        .clone()
        .or_else(|| params.cursor.as_ref().map(|cursor| cursor.cwd.clone()));
    let cwd = resolve_session_cwd_filter(state, auth, requested_cwd)?;
    decode_result(
        thread_browser_value(state, params, cwd).await?,
        "thread/browser",
    )
}

pub(super) async fn rename(
    state: &WebState,
    auth: &AuthContext,
    out_tx: &ConnectionSender,
    params: wire::ThreadRenameParams,
) -> psychevo::Result<wire::ThreadMutationResult> {
    authorize_thread(state, auth, &params.thread_id).await?;
    state
        .inner
        .state
        .set_session_title(&params.thread_id, &params.title)
        .await?;
    let session: wire::SessionSummaryView = decode_result(
        session_summary_by_id(state, &params.thread_id).await?,
        "thread/rename",
    )?;
    let event = GatewayEvent::TitleChanged {
        thread_id: params.thread_id.clone(),
        title: session.title.clone(),
        display_title: session.display_title.clone(),
    };
    if let Ok(event_value) = serde_json::to_value(&event) {
        let _ = state
            .inner
            .state
            .append_gateway_live_event(None, None, Some(&params.thread_id), None, &event_value)
            .await;
    }
    state.publish_gateway_event_for_connection(
        event,
        PendingInteractionContext::default(),
        None,
        Some(out_tx),
    );
    Ok(wire::ThreadMutationResult { session })
}

pub(super) async fn archive(
    state: &WebState,
    auth: &AuthContext,
    params: wire::ThreadIdParams,
) -> psychevo::Result<wire::ThreadMutationResult> {
    authorize_thread(state, auth, &params.thread_id).await?;
    guard_session_mutation(state, auth, &params.thread_id).await?;
    let session = decode_result(
        session_import_application::archive_thread(state, &params.thread_id).await?,
        "thread/archive",
    )?;
    Ok(wire::ThreadMutationResult { session })
}

pub(super) async fn restore(
    state: &WebState,
    auth: &AuthContext,
    params: wire::ThreadIdParams,
) -> psychevo::Result<wire::ThreadMutationResult> {
    authorize_thread(state, auth, &params.thread_id).await?;
    guard_session_mutation(state, auth, &params.thread_id).await?;
    let session = decode_result(
        session_import_application::restore_thread(state, &params.thread_id).await?,
        "thread/restore",
    )?;
    Ok(wire::ThreadMutationResult { session })
}

pub(super) async fn delete(
    state: &WebState,
    auth: &AuthContext,
    params: wire::ThreadIdParams,
) -> psychevo::Result<wire::ThreadDeleteResult> {
    authorize_thread(state, auth, &params.thread_id).await?;
    guard_session_mutation(state, auth, &params.thread_id).await?;
    let scope = default_resolved_scope(state, auth)?;
    let deleting_current = state
        .inner
        .gateway
        .resolve_source_thread(&scope.source)
        .await?
        .as_deref()
        == Some(params.thread_id.as_str());
    session_import_application::delete_thread(state, &params.thread_id).await?;
    if deleting_current {
        state
            .inner
            .gateway
            .clear_source_binding(&scope.source)
            .await?;
    }
    Ok(wire::ThreadDeleteResult {
        deleted: true,
        thread_id: params.thread_id,
    })
}

fn decode_result<T>(value: Value, method: &str) -> psychevo::Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_value(value)
        .map_err(|error| Error::Message(format!("invalid {method} result projection: {error}")))
}
