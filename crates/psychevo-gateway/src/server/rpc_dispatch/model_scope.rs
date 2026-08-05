use std::path::PathBuf;

use super::super::auth_input::authorize_thread;
use super::super::binding::{AuthContext, WebState};
use super::super::scope_session::resolve_cwd_filter;

pub(super) async fn resolve_model_state_request_scope(
    state: &WebState,
    auth: &AuthContext,
    cwd: Option<String>,
    thread_id: Option<String>,
) -> psychevo::Result<(PathBuf, Option<String>)> {
    if let Some(thread_id) = thread_id {
        authorize_thread(state, auth, &thread_id).await?;
        let summary = state
            .inner
            .framework
            .thread_summary(&thread_id)
            .await?
            .ok_or_else(|| psychevo::Error::Message(format!("thread not found: {thread_id}")))?;
        return Ok((PathBuf::from(summary.cwd), Some(thread_id)));
    }
    Ok((resolve_cwd_filter(state, auth, cwd)?, None))
}
