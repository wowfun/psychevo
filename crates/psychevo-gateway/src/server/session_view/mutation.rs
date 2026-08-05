use psychevo::Error;
use serde_json::Value;

use super::super::binding::{AuthContext, WebState};
use super::super::scope_session::default_resolved_scope;
use super::summary::session_summary_value;

pub(in super::super) async fn guard_session_mutation(
    state: &WebState,
    auth: &AuthContext,
    session_id: &str,
) -> psychevo::Result<()> {
    let scope = default_resolved_scope(state, auth)?;
    let activity = state.activity(&scope.source, Some(session_id)).await;
    if activity.running {
        return Err(Error::Message(
            "running session cannot be archived, restored, or deleted".to_string(),
        ));
    }
    Ok(())
}

pub(in super::super) async fn session_summary_by_id(
    state: &WebState,
    session_id: &str,
) -> psychevo::Result<Value> {
    let presentation = state
        .inner
        .framework
        .human_thread_summary(session_id)
        .await?
        .ok_or_else(|| Error::Message(format!("session not found: {session_id}")))?;
    let activity = state.activity(&state.inner.source, Some(session_id)).await;
    Ok(session_summary_value(presentation, activity))
}
