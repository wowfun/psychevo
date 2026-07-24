use super::*;
use serde::de::DeserializeOwned;

pub(super) async fn resume(
    state: &WebState,
    auth: &AuthContext,
    params: wire::ThreadResumeParams,
) -> psychevo_runtime::Result<wire::ThreadSnapshot> {
    let (thread_id, scope) = match params.thread_id {
        Some(thread_id) => {
            authorize_thread(state, auth, &thread_id)?;
            let scope = resolved_scope_for_thread(state, &thread_id)?;
            bind_source_to_thread(state, &scope, &thread_id)?;
            grant_browser_session_scope(state, auth, &scope);
            (Some(thread_id), scope)
        }
        None => {
            let scope = resolve_optional_scope(state, auth, params.scope)?;
            let thread_id = state.inner.gateway.resolve_source_thread(&scope.source)?;
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
) -> psychevo_runtime::Result<wire::ThreadSnapshot> {
    authorize_thread(state, auth, &params.thread_id)?;
    let scope = resolved_scope_for_thread(state, &params.thread_id)?;
    decode_result(
        thread_snapshot_live(state, &scope, Some(&params.thread_id)).await?,
        "thread/read",
    )
}

pub(super) async fn trace(
    state: &WebState,
    auth: &AuthContext,
    params: wire::ThreadTraceParams,
) -> psychevo_runtime::Result<wire::ThreadTraceResult> {
    authorize_thread(state, auth, &params.thread_id)?;
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

pub(super) fn list(
    state: &WebState,
    auth: &AuthContext,
    params: wire::ThreadListParams,
) -> psychevo_runtime::Result<wire::ThreadListResult> {
    let limit = params.limit.unwrap_or(50).clamp(1, 200);
    let cwd = resolve_session_cwd_filter(state, auth, params.cwd)?;
    let cwd = cwd.map(|cwd| cwd.to_string_lossy().into_owned());
    let activity_snapshot = state.inner.gateway.session_activity_snapshot()?;
    let sessions = state.inner.state.list_human_session_projections(
        cwd.as_deref(),
        params.archived.unwrap_or(false),
        limit,
    )?;
    let sessions = sessions
        .into_iter()
        .map(|projection| {
            let activity = activity_snapshot
                .get(&projection.summary.id)
                .cloned()
                .unwrap_or_default();
            decode_result(session_summary_value(projection, activity), "thread/list")
        })
        .collect::<psychevo_runtime::Result<Vec<_>>>()?;
    Ok(wire::ThreadListResult { sessions })
}

pub(super) fn browse(
    state: &WebState,
    auth: &AuthContext,
    params: wire::ThreadBrowserParams,
) -> psychevo_runtime::Result<wire::ThreadBrowserResult> {
    let requested_cwd = params
        .cwd
        .clone()
        .or_else(|| params.cursor.as_ref().map(|cursor| cursor.cwd.clone()));
    let cwd = resolve_session_cwd_filter(state, auth, requested_cwd)?;
    decode_result(thread_browser_value(state, params, cwd)?, "thread/browser")
}

pub(super) fn rename(
    state: &WebState,
    auth: &AuthContext,
    out_tx: &ConnectionSender,
    params: wire::ThreadRenameParams,
) -> psychevo_runtime::Result<wire::ThreadMutationResult> {
    authorize_thread(state, auth, &params.thread_id)?;
    state
        .inner
        .state
        .set_session_title(&params.thread_id, &params.title)?;
    let session: wire::SessionSummaryView = decode_result(
        session_summary_by_id(state, &params.thread_id)?,
        "thread/rename",
    )?;
    let event = GatewayEvent::TitleChanged {
        thread_id: params.thread_id.clone(),
        title: session.title.clone(),
        display_title: session.display_title.clone(),
    };
    if let Ok(event_value) = serde_json::to_value(&event) {
        let _ = state.inner.state.append_gateway_live_event(
            None,
            None,
            Some(&params.thread_id),
            None,
            &event_value,
        );
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
) -> psychevo_runtime::Result<wire::ThreadMutationResult> {
    authorize_thread(state, auth, &params.thread_id)?;
    guard_session_mutation(state, auth, &params.thread_id)?;
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
) -> psychevo_runtime::Result<wire::ThreadMutationResult> {
    authorize_thread(state, auth, &params.thread_id)?;
    guard_session_mutation(state, auth, &params.thread_id)?;
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
) -> psychevo_runtime::Result<wire::ThreadDeleteResult> {
    authorize_thread(state, auth, &params.thread_id)?;
    guard_session_mutation(state, auth, &params.thread_id)?;
    let scope = default_resolved_scope(state, auth)?;
    let deleting_current = state
        .inner
        .gateway
        .resolve_source_thread(&scope.source)?
        .as_deref()
        == Some(params.thread_id.as_str());
    session_import_application::delete_thread(state, &params.thread_id).await?;
    if deleting_current {
        state.inner.gateway.clear_source_binding(&scope.source)?;
    }
    Ok(wire::ThreadDeleteResult {
        deleted: true,
        thread_id: params.thread_id,
    })
}

fn decode_result<T>(value: Value, method: &str) -> psychevo_runtime::Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_value(value)
        .map_err(|error| Error::Message(format!("invalid {method} result projection: {error}")))
}
