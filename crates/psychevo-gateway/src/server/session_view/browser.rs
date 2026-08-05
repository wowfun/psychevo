use std::collections::BTreeSet;
use std::path::PathBuf;

use psychevo::HumanThreadBrowserQuery;
use psychevo_gateway_protocol as wire;
use serde_json::{Value, json};

use crate::gateway::activity::GatewayActivity;
use crate::gateway_now_ms;

use super::super::binding::WebState;
use super::summary::{session_project_value, session_summary_value};

pub(in super::super) async fn thread_browser_value(
    state: &WebState,
    params: wire::thread_command_turn::ThreadBrowserParams,
    cwd: Option<PathBuf>,
) -> psychevo::Result<Value> {
    let limit = params.limit.unwrap_or(20).clamp(1, 50);
    let recent_days = params.recent_days.unwrap_or(7).clamp(1, 365);
    let recent_since_ms = gateway_now_ms().saturating_sub(recent_days * 86_400_000);
    let include_ids = params
        .include_session_ids
        .iter()
        .filter(|id| !id.trim().is_empty())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let (framework_revision, activity_snapshot) = state.session_activity_snapshot().await?;
    let active_ids = activity_snapshot
        .iter()
        .filter(|(_, activity)| activity.running || activity.takeover_state.is_some())
        .map(|(thread_id, _)| thread_id.clone())
        .collect::<Vec<_>>();
    let cursor_cwd = params.cursor.as_ref().map(|cursor| cursor.cwd.clone());
    let cursor_offset = params
        .cursor
        .as_ref()
        .map(|cursor| cursor.offset)
        .unwrap_or(0);
    let workspaces = state
        .inner
        .framework
        .browse_human_threads(HumanThreadBrowserQuery {
            cwd,
            archived: params.archived.unwrap_or(false),
            cursor_cwd,
            cursor_offset,
            limit,
            recent_since_ms,
            include_thread_ids: include_ids,
            active_thread_ids: active_ids,
        })
        .await?;

    let workspaces = workspaces
        .into_iter()
        .map(|workspace| {
            let cwd = workspace.cwd;
            let sessions = workspace
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
                    session_summary_value(presentation, activity)
                })
                .collect::<Vec<_>>();
            let next_cursor = workspace.next_offset.map(|offset| {
                json!({
                    "cwd": cwd,
                    "offset": offset,
                })
            });
            json!({
                "cwd": cwd,
                "project": session_project_value(&cwd),
                "sessions": sessions,
                "hiddenCount": workspace.hidden_count,
                "nextCursor": next_cursor,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({ "workspaces": workspaces }))
}
