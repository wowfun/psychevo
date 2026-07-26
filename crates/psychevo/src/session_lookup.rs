#[cfg(test)]
use std::path::Path;

#[cfg(test)]
use crate::error::Result;
#[cfg(test)]
use crate::state::StateRuntime;

#[cfg(test)]
pub async fn latest_run_session_for_cwd(
    state: &StateRuntime,
    cwd: &Path,
) -> Result<Option<String>> {
    state.latest_run_session_for_cwd(cwd).await
}
