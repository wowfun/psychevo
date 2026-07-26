use super::*;
use futures::future::BoxFuture;
use psychevo::__agent_core::{ToolBinding, ToolExecutionMode, ToolOutput};
use psychevo::__ai::AbortSignal;

const AUTOMATION_RUN_HISTORY_LIMIT: usize = 5;
const AUTOMATION_DUE_LIMIT: usize = 10;
const AUTOMATION_SCHEDULER_TICK_MS: u64 = 30_000;
const AUTOMATION_STALE_RUN_RECOVERY_MS: i64 = 5 * 60 * 1000;
const AUTOMATION_STALE_RUN_RECOVERY_LIMIT: usize = 50;
const AUTOMATION_STALE_RUN_RECOVERY_ERROR: &str =
    "automation run recovery: stale running claim expired without an active gateway activity";

pub(super) fn reconcile(state: WebState) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    let gateway = state.inner.gateway.clone();
    gateway.spawn_background("automation-scheduler", async move {
        if let Err(err) = recover_stale_automation_runs(&state).await {
            eprintln!("automation stale-run recovery failed: {err}");
        }
        let mut tick = tokio::time::interval(Duration::from_millis(AUTOMATION_SCHEDULER_TICK_MS));
        loop {
            tick.tick().await;
            if let Err(err) = run_due_automations_once(state.clone()).await {
                eprintln!("automation scheduler failed: {err}");
            }
        }
    });
}

pub(super) fn automation_runtime_tools(
    state: WebState,
    cwd: PathBuf,
    current_thread_id: Option<String>,
) -> Vec<psychevo::__product::runtime::RuntimeTool> {
    vec![psychevo::__product::runtime::RuntimeTool::new(Arc::new(
        AutomationTool {
            state,
            cwd,
            current_thread_id,
        },
    ))]
}

include!("automations/tool.rs");
include!("automations/rpc.rs");
include!("automations/runner.rs");
include!("automations/support.rs");
include!("automations/draft.rs");
