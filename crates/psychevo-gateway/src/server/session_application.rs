use psychevo::{Error, HumanThreadListQuery};
use psychevo_gateway_protocol as wire;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::gateway::activity::GatewayActivity;
use psychevo_gateway_protocol::events_transcript::GatewayEvent;

use super::auth_input::authorize_thread;
use super::binding::{AuthContext, PendingInteractionContext, WebState};
use super::event_delivery::ConnectionSender;
use super::scope_session::{
    bind_source_to_thread, default_resolved_scope, grant_browser_session_scope,
    resolve_optional_scope, resolve_session_cwd_filter, resolved_scope_for_thread,
};
use super::session_import_application;
use super::session_view::{
    guard_session_mutation, session_summary_by_id, session_summary_value, thread_browser_value,
    thread_snapshot_live,
};

pub(super) async fn resume(
    state: &WebState,
    auth: &AuthContext,
    params: wire::thread_command_turn::ThreadResumeParams,
) -> psychevo::Result<wire::events_transcript::ThreadSnapshot> {
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
    params: wire::thread_command_turn::ThreadReadParams,
) -> psychevo::Result<wire::events_transcript::ThreadSnapshot> {
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
    params: wire::thread_command_turn::ThreadTraceParams,
) -> psychevo::Result<wire::thread_command_turn::ThreadTraceResult> {
    authorize_thread(state, auth, &params.thread_id).await?;
    let result = state
        .inner
        .framework
        .resume_thread(&params.thread_id)
        .await?
        .trace(params.after_seq, params.limit)
        .await?;
    Ok(wire::thread_command_turn::ThreadTraceResult {
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
    params: wire::thread_command_turn::ThreadListParams,
) -> psychevo::Result<wire::thread_command_turn::ThreadListResult> {
    let limit = params.limit.unwrap_or(50).clamp(1, 200);
    let cwd = resolve_session_cwd_filter(state, auth, params.cwd)?;
    let archived = params.archived.unwrap_or(false);
    let (framework_revision, activity_snapshot) = state.session_activity_snapshot().await?;
    let page = state
        .inner
        .framework
        .list_human_threads(HumanThreadListQuery {
            cwd,
            archived,
            cursor: params.cursor,
            limit,
        })
        .await?;
    let sessions = page
        .threads
        .into_iter()
        .map(|presentation| {
            let activity = activity_snapshot
                .get(&presentation.summary.id)
                .cloned()
                .unwrap_or_else(|| GatewayActivity {
                    framework_revision: Some(framework_revision.clone()),
                    ..GatewayActivity::default()
                });
            decode_result(session_summary_value(presentation, activity), "thread/list")
        })
        .collect::<psychevo::Result<Vec<_>>>()?;
    Ok(wire::thread_command_turn::ThreadListResult {
        sessions,
        next_cursor: page.next_cursor,
    })
}

pub(super) async fn browse(
    state: &WebState,
    auth: &AuthContext,
    params: wire::thread_command_turn::ThreadBrowserParams,
) -> psychevo::Result<wire::thread_command_turn::ThreadBrowserResult> {
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
    params: wire::thread_command_turn::ThreadRenameParams,
) -> psychevo::Result<wire::thread_command_turn::ThreadMutationResult> {
    authorize_thread(state, auth, &params.thread_id).await?;
    let thread = state
        .inner
        .framework
        .resume_thread(&params.thread_id)
        .await?;
    thread.set_title(&params.title).await?;
    let session: wire::events_transcript::SessionSummaryView = decode_result(
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
            .durability
            .append_gateway_live_event(
                None,
                None,
                Some(&params.thread_id),
                None,
                None,
                &event_value,
            )
            .await;
    }
    state.publish_gateway_event_for_connection(
        event,
        PendingInteractionContext::default(),
        None,
        Some(out_tx),
    );
    Ok(wire::thread_command_turn::ThreadMutationResult { session })
}

pub(super) async fn archive(
    state: &WebState,
    auth: &AuthContext,
    params: wire::thread_command_turn::ThreadIdParams,
) -> psychevo::Result<wire::thread_command_turn::ThreadMutationResult> {
    authorize_thread(state, auth, &params.thread_id).await?;
    guard_session_mutation(state, auth, &params.thread_id).await?;
    let session = decode_result(
        session_import_application::archive_thread(state, &params.thread_id).await?,
        "thread/archive",
    )?;
    Ok(wire::thread_command_turn::ThreadMutationResult { session })
}

pub(super) async fn restore(
    state: &WebState,
    auth: &AuthContext,
    params: wire::thread_command_turn::ThreadIdParams,
) -> psychevo::Result<wire::thread_command_turn::ThreadMutationResult> {
    authorize_thread(state, auth, &params.thread_id).await?;
    guard_session_mutation(state, auth, &params.thread_id).await?;
    let session = decode_result(
        session_import_application::restore_thread(state, &params.thread_id).await?,
        "thread/restore",
    )?;
    Ok(wire::thread_command_turn::ThreadMutationResult { session })
}

pub(super) async fn delete(
    state: &WebState,
    auth: &AuthContext,
    params: wire::thread_command_turn::ThreadIdParams,
) -> psychevo::Result<wire::thread_command_turn::ThreadDeleteResult> {
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
    Ok(wire::thread_command_turn::ThreadDeleteResult {
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
