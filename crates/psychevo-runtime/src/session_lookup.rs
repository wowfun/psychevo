use std::path::Path;

use crate::error::Result;
use crate::state::StateRuntime;

pub async fn session_exists(state: &StateRuntime, session_id: &str) -> Result<bool> {
    Ok(state.session_summary(session_id).await?.is_some())
}

pub async fn latest_run_session_for_cwd(
    state: &StateRuntime,
    cwd: &Path,
) -> Result<Option<String>> {
    state.latest_run_session_for_cwd(cwd).await
}
