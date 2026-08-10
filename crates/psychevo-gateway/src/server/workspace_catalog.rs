use std::path::PathBuf;

use psychevo::{GatewayNavigationState, Workspace, WorkspaceUpdate};
use psychevo_gateway_protocol as wire;

use super::auth_input::authorize_thread;
use super::binding::{AuthContext, WebState};

pub(super) async fn update(
    state: &WebState,
    params: wire::thread_command_turn::WorkspaceCatalogUpdateParams,
) -> psychevo::Result<wire::thread_command_turn::WorkspaceCatalogUpdateResult> {
    let workspace_id = params.workspace_id.clone();
    let workspace = state
        .inner
        .framework
        .update_workspace(WorkspaceUpdate {
            workspace_id: params.workspace_id,
            expected_revision: params.expected_revision,
            name: params.name,
            roots: params.roots.into_iter().map(PathBuf::from).collect(),
        })
        .await?;
    super::scope_session::invalidate_browser_workspace_preview_roots(state, &workspace_id);
    Ok(wire::thread_command_turn::WorkspaceCatalogUpdateResult {
        workspace: workspace_view(workspace),
    })
}

pub(super) async fn navigation(
    state: &WebState,
) -> psychevo::Result<wire::thread_command_turn::NavigationStateView> {
    state
        .inner
        .durability
        .navigation()
        .await
        .map(navigation_view)
}

pub(super) async fn set_thread_pinned(
    state: &WebState,
    auth: &AuthContext,
    params: wire::thread_command_turn::ThreadPinSetParams,
) -> psychevo::Result<wire::thread_command_turn::NavigationStateView> {
    authorize_thread(state, auth, &params.thread_id).await?;
    state
        .inner
        .durability
        .set_thread_pinned(&params.thread_id, params.pinned)
        .await
        .map(navigation_view)
}

pub(super) async fn set_workspace_pinned(
    state: &WebState,
    params: wire::thread_command_turn::WorkspacePinSetParams,
) -> psychevo::Result<wire::thread_command_turn::NavigationStateView> {
    state
        .inner
        .durability
        .set_workspace_pinned(&params.workspace_id, params.pinned)
        .await
        .map(navigation_view)
}

fn workspace_view(workspace: Workspace) -> wire::thread_command_turn::WorkspaceView {
    wire::thread_command_turn::WorkspaceView {
        id: workspace.id,
        name: workspace.name,
        roots: workspace.roots,
        revision: workspace.revision,
    }
}

fn navigation_view(
    navigation: GatewayNavigationState,
) -> wire::thread_command_turn::NavigationStateView {
    wire::thread_command_turn::NavigationStateView {
        revision: navigation.revision,
        pinned_thread_ids: navigation.pinned_thread_ids,
        pinned_workspace_ids: navigation.pinned_workspace_ids,
    }
}
