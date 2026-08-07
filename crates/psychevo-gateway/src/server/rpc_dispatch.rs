mod model_scope;
mod transport;

use model_scope::resolve_model_state_request_scope;
#[cfg(test)]
pub(super) use transport::{LaunchQuery, prune_expired_launches};
pub(super) use transport::{
    consume_launch, create_launch, managed_identity, managed_shutdown, readyz,
    spawn_gateway_live_event_tailer, ws_handler,
};

use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use futures::future::BoxFuture;
use psychevo::agents::{resolve_agent_definition, resolve_agent_team_definition};
use psychevo::config::{
    McpOAuthCredentialStore, McpServerConfigInput, McpToolPolicyInput,
    clear_mcp_oauth_access_token_with_store, create_local_toolset, load_agent_backend_configs,
    remove_local_toolset, remove_mcp_server, save_mcp_oauth_access_token_with_store,
    set_local_toolset_enabled, set_mcp_server_enabled, set_mcp_server_tool_policy,
    upsert_mcp_server,
};
use psychevo::plugins::{
    PluginInspectOptions, PluginInstallOptions, PluginMarketplaceEntry, PluginScope,
    PluginSourceKind, codex_plugin_set_enabled_value, plugin_import_inspect_value,
    plugin_install_value, plugin_marketplace_add_value, plugin_marketplace_list_value,
    plugin_marketplace_remove_value, plugin_reset_enabled_value, plugin_set_enabled_value,
    plugin_uninstall_value,
};
use psychevo::skills::{
    InstallOptions, ListSkillsOptions, SkillDiscoveryOptions, SkillTarget, discover_skills,
    install_skill, list_skills_value_with_options, remove_installed_skill, set_skill_enabled,
    view_skill_value_selected, write_installed_skill,
};
use psychevo::{ConfigurationQuery, Error, RunMode, config::ConfigScope};
use psychevo_gateway_protocol as wire;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use uuid::Uuid;

use crate::GatewayEventEmitter;
use crate::gateway::activity::SendShellRequest;
use crate::gateway::durable_activity::gateway_activity_view;
use crate::gateway::turn_shell::unavailable_compaction_result;
use crate::journey_profile::{GatewayProfileFields, gateway_profile_mark};
use psychevo_gateway_protocol::events_transcript::GatewayEvent;
use psychevo_gateway_protocol::source::GatewayThreadSelector;

use super::agents::{
    active_profile_config_dir, agent_control_result, agent_list_result, agent_read_result,
    agent_status_result, backend_values_for_scope, delete_agent_definition, delete_backend_config,
    delete_team_definition, discover_gateway_teams, manage_backend_value,
    managed_backend_doctor_value_with_auth, materialize_local_acp_backends, read_agent_definition,
    read_team_definition, set_agent_definition_enabled, set_team_definition_enabled,
    team_list_result, team_read_result, team_status_result, write_agent_definition,
    write_backend_config, write_team_definition,
};
use super::auth_input::authorize_thread;
use super::automations::{
    automation_delete_result, automation_draft_result, automation_list_result,
    automation_run_result, automation_set_enabled_result, automation_write_result,
};
use super::binding::{AuthContext, WebState};
use super::channels::{
    channel_delete_result, channel_doctor_result_live, channel_enable_result,
    channel_list_result_for_scope, channel_show_result, channel_source_list_result,
    channel_update_result, channel_wechat_qr_poll_result, channel_wechat_qr_start_result,
};
use super::commands::{
    command_execute_value, command_list_value, slash_settings_read_value,
    slash_settings_update_value,
};
use super::completion::completion_list_value;
use super::event_delivery::ConnectionSender;
use super::extension_management::{
    extension_app_close_result, extension_app_open_result, extension_list_result,
    extension_read_result, extension_remove_result, extension_set_enabled_result,
};
use super::mcp_oauth_store::{McpOAuthSessionStatus, McpOAuthSessionStore};
use super::rpc_json::{RpcRequest, cwd_source, rpc_notification};
use super::runtime_profiles::{
    delete_runtime_profile, runtime_profile_list_result, runtime_profile_read_result,
    set_runtime_profile_enabled, write_runtime_profile,
};
use super::scope_session::{
    ResolvedScope, bind_source_to_thread, default_resolved_scope, gateway_profile_value,
    reset_source_to_empty, resolve_cwd_filter, resolve_external_file_scope, resolve_optional_scope,
    resolve_required_scope, resolve_workspace_preview_scope, resolved_scope_for_thread,
    shell_execution_intent,
};
use super::session_view::gateway_shell_result_value;
use super::settings_observability::{
    context_read_value, discover_gateway_agents, display_cwd, model_assignment_set_value,
    model_provider_catalog_value, model_provider_save_value, model_settings_value,
    model_state_read_value, model_state_set_value, observability_read_value, settings_read_value,
    update_session_agent_setting, usage_read_value, web_search_settings_update_value,
    web_search_settings_value,
};
use super::thread_application::prewarm_codex_runtime_inventory;
use super::voice::{
    voice_asr_transcribe_value, voice_policy_read_value, voice_policy_update_value,
    voice_tts_synthesize_value,
};
use super::workspace::{
    workspace_create_value, workspace_diff_value, workspace_file_read_value,
    workspace_file_write_value, workspace_files_value, workspace_folder_list_value,
    workspace_git_branches_value, workspace_git_checkout_value,
};
use super::workspace_external::{
    workspace_file_external_actions_value, workspace_file_open_external_value,
};
use super::workspace_preview::{
    workspace_file_preview_open_value, workspace_file_preview_release_value,
};
use super::{
    codex_capability_broker, session_application, session_import_application, thread_application,
    voice,
};

pub(super) fn handle_rpc<T>(
    state: WebState,
    auth: AuthContext,
    out_tx: T,
    request: RpcRequest,
) -> futures::future::BoxFuture<'static, psychevo::Result<Value>>
where
    T: Into<ConnectionSender>,
{
    let out_tx = out_tx.into();
    Box::pin(async move {
        match request.method.as_str() {
            "initialize" => {
                let scope = default_resolved_scope(&state, &auth)?;
                prewarm_codex_runtime_inventory(&state, scope.cwd.clone());
                Ok(json!({
                "server": "psychevo-gateway",
                "version": env!("CARGO_PKG_VERSION"),
                "cwd": scope.cwd,
                "displayCwd": display_cwd(&scope.cwd),
                "scope": scope.to_wire_scope(),
                "source": scope.source,
                "profile": gateway_profile_value(&state),
                "capabilities": {
                    "threads": true,
                    "turns": true,
                    "historyManagement": true,
                    "downloads": true,
                    "media": true,
                    "imageGeneration": true,
                    "automations": true,
                    "settingsWrite": "structured",
                    "workspaceCreate": true,
                    "contextCompaction": true,
                    "memoryResources": "status_only"
                }
                }))
            }
            "thread/draft/open" => {
                let params = request
                    .required_params::<wire::thread_command_turn::ThreadDraftOpenParams>()?;
                Ok(serde_json::to_value(
                    thread_application::open_thread_draft(&state, &auth, params).await?,
                )?)
            }
            "thread/resume" => {
                let params = request.params::<wire::thread_command_turn::ThreadResumeParams>()?;
                Ok(serde_json::to_value(
                    session_application::resume(&state, &auth, params).await?,
                )?)
            }
            "thread/read" => {
                let params =
                    request.required_params::<wire::thread_command_turn::ThreadReadParams>()?;
                Ok(serde_json::to_value(
                    session_application::read(&state, &auth, params).await?,
                )?)
            }
            "thread/trace" => {
                let params =
                    request.required_params::<wire::thread_command_turn::ThreadTraceParams>()?;
                Ok(serde_json::to_value(
                    session_application::trace(&state, &auth, params).await?,
                )?)
            }
            "thread/list" => {
                let params = request.params::<wire::thread_command_turn::ThreadListParams>()?;
                Ok(serde_json::to_value(
                    session_application::list(&state, &auth, params).await?,
                )?)
            }
            "thread/browser" => {
                let params = request.params::<wire::thread_command_turn::ThreadBrowserParams>()?;
                Ok(serde_json::to_value(
                    session_application::browse(&state, &auth, params).await?,
                )?)
            }
            "thread/import/list" => {
                let params = request
                    .required_params::<wire::agents_backend_rpc::ThreadImportListParams>()?;
                Ok(serde_json::to_value(
                    session_import_application::list(&state, &auth, params).await?,
                )?)
            }
            "thread/import" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::ThreadImportParams>()?;
                Ok(serde_json::to_value(
                    session_import_application::import(&state, &auth, params).await?,
                )?)
            }
            "thread/rename" => {
                let params =
                    request.required_params::<wire::thread_command_turn::ThreadRenameParams>()?;
                Ok(serde_json::to_value(
                    session_application::rename(&state, &auth, &out_tx, params).await?,
                )?)
            }
            "thread/archive" => {
                let params =
                    request.required_params::<wire::thread_command_turn::ThreadIdParams>()?;
                Ok(serde_json::to_value(
                    session_application::archive(&state, &auth, params).await?,
                )?)
            }
            "thread/restore" => {
                let params =
                    request.required_params::<wire::thread_command_turn::ThreadIdParams>()?;
                Ok(serde_json::to_value(
                    session_application::restore(&state, &auth, params).await?,
                )?)
            }
            "thread/delete" => {
                let params =
                    request.required_params::<wire::thread_command_turn::ThreadIdParams>()?;
                Ok(serde_json::to_value(
                    session_application::delete(&state, &auth, params).await?,
                )?)
            }
            "thread/context/read" => {
                let params =
                    request.params::<wire::agents_backend_rpc::ThreadContextReadParams>()?;
                Ok(serde_json::to_value(
                    thread_application::inspect_thread(&state, &auth, params).await?,
                )?)
            }
            "thread/draft/prepare" => {
                let params = request
                    .required_params::<wire::agents_backend_rpc::ThreadDraftPrepareParams>()?;
                Ok(serde_json::to_value(
                    thread_application::prepare_thread_draft(&state, &auth, params).await?,
                )?)
            }
            "thread/control/set" => {
                let params = request
                    .required_params::<wire::agents_backend_rpc::ThreadControlSetParams>()?;
                Ok(serde_json::to_value(
                    thread_application::set_thread_control(&state, &auth, params).await?,
                )?)
            }
            "thread/action/run" => {
                let params = request
                    .required_params::<wire::thread_command_turn::ThreadActionRunParams>()?;
                Ok(serde_json::to_value(
                    thread_application::run_thread_action(&state, &auth, out_tx, params).await?,
                )?)
            }
            "thread/interaction/respond" => {
                let params = request
                    .required_params::<wire::thread_command_turn::ThreadInteractionRespondParams>(
                    )?;
                Ok(serde_json::to_value(
                    thread_application::respond_to_thread_interaction(&state, &auth, params)
                        .await?,
                )?)
            }
            "thread/history/read" => {
                let params = request
                    .required_params::<wire::thread_command_turn::ThreadHistoryReadParams>()?;
                Ok(serde_json::to_value(
                    thread_application::read_thread_history(&state, &auth, params).await?,
                )?)
            }
            "thread/history/draft/read" => {
                let params = request
                    .required_params::<wire::thread_command_turn::ThreadHistoryDraftReadParams>()?;
                Ok(serde_json::to_value(
                    thread_application::read_thread_history_draft(&state, &auth, params).await?,
                )?)
            }
            "runtime/profile/list" => {
                let params =
                    request.params::<wire::agents_backend_rpc::RuntimeProfileListParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                state.invalidate_runnable_target_catalog();
                Ok(serde_json::to_value(runtime_profile_list_result(
                    &state, &scope,
                )?)?)
            }
            "runtime/profile/read" => {
                let params = request
                    .required_params::<wire::agents_backend_rpc::RuntimeProfileReadParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                runtime_profile_read_result(&state, &scope, params)
            }
            "runtime/profile/write" => {
                let params = request
                    .required_params::<wire::agents_backend_rpc::RuntimeProfileWriteParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                state.invalidate_runnable_target_catalog_after(write_runtime_profile(
                    &state, &scope, params,
                ))
            }
            "runtime/profile/delete" => {
                let params = request
                    .required_params::<wire::agents_backend_rpc::RuntimeProfileDeleteParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                state.invalidate_runnable_target_catalog_after(delete_runtime_profile(
                    &state, &scope, params,
                ))
            }
            "runtime/profile/setEnabled" => {
                let params = request
                    .required_params::<wire::agents_backend_rpc::RuntimeProfileSetEnabledParams>(
                    )?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                state.invalidate_runnable_target_catalog_after(set_runtime_profile_enabled(
                    &state, &scope, params,
                ))
            }
            "automation/list" => {
                let params = request.params::<wire::automations::AutomationListParams>()?;
                automation_list_result(&state, &auth, params).await
            }
            "automation/draft" => {
                let params =
                    request.required_params::<wire::automations::AutomationDraftParams>()?;
                automation_draft_result(state, &auth, params).await
            }
            "automation/write" => {
                let params =
                    request.required_params::<wire::automations::AutomationWriteParams>()?;
                automation_write_result(&state, &auth, params).await
            }
            "automation/pause" => {
                let params = request.required_params::<wire::automations::AutomationIdParams>()?;
                automation_set_enabled_result(&state, &auth, params, false).await
            }
            "automation/resume" => {
                let params = request.required_params::<wire::automations::AutomationIdParams>()?;
                automation_set_enabled_result(&state, &auth, params, true).await
            }
            "automation/delete" => {
                let params = request.required_params::<wire::automations::AutomationIdParams>()?;
                automation_delete_result(&state, &auth, params).await
            }
            "automation/run" => {
                let params = request.required_params::<wire::automations::AutomationRunParams>()?;
                automation_run_result(state, &auth, params, out_tx).await
            }
            "turn/start" => {
                let params =
                    request.required_params::<wire::thread_command_turn::TurnStartParams>()?;
                Ok(serde_json::to_value(
                    thread_application::start_thread_turn(&state, &auth, out_tx, params).await?,
                )?)
            }
            "voice/asr/transcribe" => {
                let params = request.required_params::<wire::voice::VoiceAsrTranscribeParams>()?;
                voice_asr_transcribe_value(&state, &auth, params).await
            }
            "voice/tts/synthesize" => {
                let params = request.required_params::<wire::voice::VoiceTtsSynthesizeParams>()?;
                voice_tts_synthesize_value(&state, &auth, params).await
            }
            "voice/policy/read" => {
                let params = request.params::<wire::voice::VoicePolicyReadParams>()?;
                voice_policy_read_value(&state, &auth, params).await
            }
            "voice/policy/update" => {
                let params = request.required_params::<wire::voice::VoicePolicyUpdateParams>()?;
                voice_policy_update_value(&state, &auth, params).await
            }
            "thread/realtime/start" => {
                let params = request.required_params::<wire::voice::ThreadRealtimeStartParams>()?;
                Ok(serde_json::to_value(
                    voice::start_realtime(&state, &auth, out_tx, params).await?,
                )?)
            }
            "thread/realtime/appendAudio" => {
                let params =
                    request.required_params::<wire::voice::ThreadRealtimeAppendAudioParams>()?;
                Ok(serde_json::to_value(
                    voice::append_realtime_audio(&state, params).await?,
                )?)
            }
            "thread/realtime/appendText" => {
                let params =
                    request.required_params::<wire::voice::ThreadRealtimeAppendTextParams>()?;
                Ok(serde_json::to_value(
                    voice::append_realtime_text(&state, params).await?,
                )?)
            }
            "thread/realtime/appendSpeech" => {
                let params =
                    request.required_params::<wire::voice::ThreadRealtimeAppendSpeechParams>()?;
                Ok(serde_json::to_value(
                    voice::append_realtime_speech(&state, params).await?,
                )?)
            }
            "thread/realtime/stop" => {
                let params =
                    request.required_params::<wire::voice::ThreadRealtimeSessionParams>()?;
                Ok(serde_json::to_value(voice::stop_realtime(
                    &state, out_tx, params,
                )?)?)
            }
            "thread/realtime/listVoices" => {
                let params =
                    request.required_params::<wire::voice::ThreadRealtimeSessionParams>()?;
                Ok(serde_json::to_value(voice::list_realtime_voices(
                    &state, params,
                )?)?)
            }
            "completion/list" => {
                let params =
                    request.required_params::<wire::thread_command_turn::CompletionListParams>()?;
                let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
                if let Some(thread_id) = &params.thread_id {
                    authorize_thread(&state, &auth, thread_id).await?;
                }
                completion_list_value(&state, &scope, params).await
            }
            "workspace/files" => {
                let params = request
                    .required_params::<wire::settings_workspace_context::WorkspaceFilesParams>()?;
                let scope = resolve_required_scope(&state, &auth, params.scope)?;
                workspace_files_value(&scope)
            }
            "workspace/folders" => {
                let params = request
                    .required_params::<wire::settings_workspace_context::WorkspaceFolderListParams>(
                    )?;
                let scope = resolve_required_scope(&state, &auth, params.scope)?;
                workspace_folder_list_value(&state, &scope, params.path.as_deref())
            }
            "workspace/git/branches" => {
                let params = request.required_params::<wire::settings_workspace_context::WorkspaceGitBranchesParams>()?;
                let scope = resolve_required_scope(&state, &auth, params.scope)?;
                gateway_profile_mark(
                    "workspace_git_branches_started",
                    None,
                    None,
                    GatewayProfileFields {
                        request_method: Some("workspace/git/branches"),
                        runtime_source: Some("web"),
                        ..GatewayProfileFields::default()
                    },
                );
                let result = workspace_git_branches_value(&scope);
                gateway_profile_mark(
                    "workspace_git_branches_completed",
                    None,
                    None,
                    GatewayProfileFields {
                        request_method: Some("workspace/git/branches"),
                        runtime_source: Some("web"),
                        ..GatewayProfileFields::default()
                    },
                );
                result
            }
            "workspace/git/checkout" => {
                let params = request.required_params::<wire::settings_workspace_context::WorkspaceGitCheckoutParams>()?;
                let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
                workspace_git_checkout_value(&scope, params)
            }
            "workspace/file/read" => {
                let params = request
                    .required_params::<wire::settings_workspace_context::WorkspaceFileReadParams>(
                    )?;
                let scope = resolve_required_scope(&state, &auth, params.scope)?;
                workspace_file_read_value(&scope, &params.path)
            }
            "workspace/file/preview/open" => {
                let params = request.required_params::<wire::settings_workspace_context::WorkspaceFilePreviewOpenParams>()?;
                let scope = resolve_workspace_preview_scope(&state, &auth, params.scope)?;
                workspace_file_preview_open_value(&state, &scope, &params.path)
            }
            "workspace/file/preview/release" => {
                let params =
                    request.required_params::<wire::settings_workspace_context::WorkspaceFilePreviewReleaseParams>()?;
                workspace_file_preview_release_value(&state, &params.resource_id)
            }
            "workspace/file/write" => {
                let params = request
                    .required_params::<wire::settings_workspace_context::WorkspaceFileWriteParams>(
                    )?;
                let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
                workspace_file_write_value(&scope, params)
            }
            "workspace/file/externalActions" => {
                let params =
                    request.required_params::<wire::settings_workspace_context::WorkspaceFileExternalActionsParams>()?;
                let scope = resolve_external_file_scope(&state, &auth, params.scope)?;
                workspace_file_external_actions_value(
                    &state.inner.workspace_external,
                    &scope,
                    &params.path,
                )
            }
            "workspace/file/openExternal" => {
                let params = request.required_params::<wire::settings_workspace_context::WorkspaceFileOpenExternalParams>()?;
                let scope = resolve_external_file_scope(&state, &auth, params.scope.clone())?;
                workspace_file_open_external_value(&state.inner.workspace_external, &scope, params)
                    .await
            }
            "workspace/diff" => {
                let params = request
                    .required_params::<wire::settings_workspace_context::WorkspaceDiffParams>()?;
                let scope = resolve_required_scope(&state, &auth, params.scope)?;
                workspace_diff_value(&scope, params.path.as_deref())
            }
            "workspace/changes" => {
                let params = request
                    .required_params::<wire::settings_workspace_context::WorkspaceChangesParams>(
                    )?;
                let scope = resolve_required_scope(&state, &auth, params.scope)?;
                Ok(serde_json::to_value(
                    state.inner.review.changes_for_scope(&scope),
                )?)
            }
            "workspace/change/accept" => {
                let params = request
                    .required_params::<wire::settings_workspace_context::WorkspaceChangeFileParams>(
                    )?;
                let scope = resolve_required_scope(&state, &auth, params.scope)?;
                Ok(serde_json::to_value(state.inner.review.accept(
                    &scope,
                    &params.turn_id,
                    &params.path,
                )?)?)
            }
            "workspace/change/reject" => {
                let params = request
                    .required_params::<wire::settings_workspace_context::WorkspaceChangeFileParams>(
                    )?;
                let scope = resolve_required_scope(&state, &auth, params.scope)?;
                Ok(serde_json::to_value(state.inner.review.reject(
                    &scope,
                    &params.turn_id,
                    &params.path,
                )?)?)
            }
            "workspace/create" => {
                let params = request
                    .required_params::<wire::settings_workspace_context::WorkspaceCreateParams>()?;
                workspace_create_value(&state, &auth, params)
            }
            "context/read" => {
                let params = request
                    .required_params::<wire::settings_workspace_context::ContextReadParams>()?;
                let scope = resolve_required_scope(&state, &auth, params.scope)?;
                if let Some(thread_id) = &params.thread_id {
                    authorize_thread(&state, &auth, thread_id).await?;
                }
                context_read_value(&state, &scope, params.thread_id.as_deref()).await
            }
            "observability/read" => {
                let params = request
                    .required_params::<wire::settings_workspace_context::ObservabilityReadParams>(
                    )?;
                let requested_scope = resolve_required_scope(&state, &auth, params.scope)?;
                let (scope, thread_id) = match params.thread_id {
                    Some(thread_id) => {
                        authorize_thread(&state, &auth, &thread_id).await?;
                        (
                            resolved_scope_for_thread(&state, &thread_id).await?,
                            Some(thread_id),
                        )
                    }
                    None => (requested_scope, None),
                };
                observability_read_value(&state, &scope, thread_id.as_deref()).await
            }
            "usage/read" => {
                let params = request
                    .required_params::<wire::settings_workspace_context::UsageReadParams>()?;
                usage_read_value(&state, params).await
            }
            "source/reset" => {
                let params =
                    request.required_params::<wire::thread_command_turn::SourceResetParams>()?;
                let scope = resolve_required_scope(&state, &auth, params.scope)?;
                state
                    .inner
                    .gateway
                    .release_prepared_agent_session(&scope.source.source_key().0)
                    .await?;
                reset_source_to_empty(&state, &scope).await
            }
            "agent/list" => {
                let params = request.params::<wire::agents_backend_rpc::AgentListParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                state.invalidate_runnable_target_catalog();
                let catalog = discover_gateway_agents(&state, &scope)?;
                Ok(serde_json::to_value(agent_list_result(&catalog))?)
            }
            "agent/read" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::AgentReadParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                if params.target.is_some() {
                    return read_agent_definition(&state, &scope, params);
                }
                let catalog = discover_gateway_agents(&state, &scope)?;
                let agent = resolve_agent_definition(
                    &catalog,
                    &params.name,
                    &scope.cwd,
                    &state.inner.inherited_env,
                )?;
                Ok(serde_json::to_value(agent_read_result(&agent))?)
            }
            "agent/write" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::AgentWriteParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                state.invalidate_runnable_target_catalog_after(write_agent_definition(
                    &state, &scope, params,
                ))
            }
            "agent/setEnabled" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::AgentSetEnabledParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                state.invalidate_runnable_target_catalog_after(set_agent_definition_enabled(
                    &state, &scope, params,
                ))
            }
            "agent/delete" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::AgentDeleteParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                state.invalidate_runnable_target_catalog_after(delete_agent_definition(
                    &state, &scope, params,
                ))
            }
            "agent/status" => {
                let params = request.params::<wire::agents_backend_rpc::AgentStatusParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                if let Some(thread_id) = params.thread_id.as_deref() {
                    authorize_thread(&state, &auth, thread_id).await?;
                }
                let source_thread_id = if params.thread_id.is_some() || params.all.unwrap_or(false)
                {
                    None
                } else {
                    state
                        .inner
                        .gateway
                        .resolve_source_thread(&scope.source)
                        .await?
                };
                let thread_id = params.thread_id.as_deref().or(source_thread_id.as_deref());
                let agent_control = state.inner.runtime.application().agent_control();
                Ok(serde_json::to_value(
                    agent_status_result(&agent_control, thread_id, params.all.unwrap_or(false))
                        .await,
                )?)
            }
            "agent/control" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::AgentControlParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                let _ = scope;
                if let Some(thread_id) = params.thread_id.as_deref() {
                    authorize_thread(&state, &auth, thread_id).await?;
                }
                let agent_control = state.inner.runtime.application().agent_control();
                Ok(serde_json::to_value(
                    agent_control_result(&agent_control, params).await?,
                )?)
            }
            "team/list" => {
                let params = request.params::<wire::agents_backend_rpc::TeamListParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                let agents = discover_gateway_agents(&state, &scope)?;
                let teams = discover_gateway_teams(&state, &scope, &agents)?;
                Ok(serde_json::to_value(team_list_result(&teams))?)
            }
            "team/read" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::TeamReadParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                if params.target.is_some() {
                    return read_team_definition(&state, &scope, params);
                }
                let agents = discover_gateway_agents(&state, &scope)?;
                let teams = discover_gateway_teams(&state, &scope, &agents)?;
                let team = resolve_agent_team_definition(&teams, &params.name)?;
                Ok(serde_json::to_value(team_read_result(&team))?)
            }
            "team/write" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::TeamWriteParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                write_team_definition(&state, &scope, params)
            }
            "team/setEnabled" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::TeamSetEnabledParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                set_team_definition_enabled(&state, &scope, params)
            }
            "team/delete" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::TeamDeleteParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                delete_team_definition(&state, &scope, params)
            }
            "team/status" => {
                let params = request.params::<wire::agents_backend_rpc::TeamStatusParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                if let Some(thread_id) = params.thread_id.as_deref() {
                    authorize_thread(&state, &auth, thread_id).await?;
                }
                let source_thread_id = if params.thread_id.is_some() {
                    None
                } else {
                    state
                        .inner
                        .gateway
                        .resolve_source_thread(&scope.source)
                        .await?
                };
                let thread_id = params.thread_id.as_deref().or(source_thread_id.as_deref());
                let thread = match thread_id {
                    Some(thread_id) => Some(state.inner.framework.resume_thread(thread_id).await?),
                    None => None,
                };
                let agent_control = state.inner.runtime.application().agent_control();
                Ok(serde_json::to_value(
                    team_status_result(thread.as_ref(), &agent_control).await?,
                )?)
            }
            "backend/list" => {
                let params = request.params::<wire::agents_backend_rpc::BackendListParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                materialize_local_acp_backends(&state, &scope)?;
                state.invalidate_runnable_target_catalog();
                let backends = load_agent_backend_configs(
                    &state.inner.home,
                    &scope.cwd,
                    &state.inner.inherited_env,
                )?;
                Ok(serde_json::to_value(
                    wire::agents_backend_rpc::BackendListResult {
                        backends: backend_values_for_scope(&state, &scope, &backends)?,
                    },
                )?)
            }
            "backend/doctor" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::BackendDoctorParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                materialize_local_acp_backends(&state, &scope)?;
                state.invalidate_runnable_target_catalog();
                let backends = load_agent_backend_configs(
                    &state.inner.home,
                    &scope.cwd,
                    &state.inner.inherited_env,
                )?;
                let backend = backends
                    .get(&params.id)
                    .ok_or_else(|| Error::Message(format!("unknown backend: {}", params.id)))?;
                Ok(serde_json::to_value(
                    managed_backend_doctor_value_with_auth(&state, &scope, backend).await?,
                )?)
            }
            "backend/install" | "backend/repair" | "backend/upgrade" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::BackendManageParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                let operation = request.method.strip_prefix("backend/").unwrap_or("install");
                let result = manage_backend_value(&state, &scope, params, operation).await;
                state.invalidate_runnable_target_catalog_after(result)
            }
            "backend/write" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::BackendWriteParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                state.invalidate_runnable_target_catalog_after(write_backend_config(
                    &state, &scope, params,
                ))
            }
            "backend/delete" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::BackendDeleteParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                state.invalidate_runnable_target_catalog_after(delete_backend_config(
                    &state, &scope, params,
                ))
            }
            "plugin/list" => {
                let params = request.params::<wire::agents_backend_rpc::PluginListParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                let mut query = ConfigurationQuery::new(&scope.cwd);
                query.inherited_env = Some(state.inner.inherited_env.clone());
                let configuration = state.inner.framework.configuration(query)?;
                let native = configuration.plugins()?;
                let broker = &state.inner.codex_capability_broker;
                let codex = if broker.is_enabled() {
                    broker.plugin_list(&scope.cwd).await
                } else {
                    Err(psychevo::Error::Message(
                        "Codex plugin authority is disabled in the active profile".to_string(),
                    ))
                };
                let merged = codex_capability_broker::merge_plugin_list(native, codex);
                let merged = codex_capability_broker::apply_codex_policy_views(
                    merged,
                    &state.inner.home,
                    &scope.cwd,
                )?;
                Ok(codex_capability_broker::apply_authority_view(
                    merged,
                    broker.authority_view(),
                ))
            }
            "plugin/read" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::PluginReadParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                if let Some(identity) =
                    codex_capability_broker::CodexPluginIdentity::parse_selector(&params.selector)?
                {
                    let detail = state
                        .inner
                        .codex_capability_broker
                        .plugin_read(&scope.cwd, &identity)
                        .await?;
                    let policy = psychevo::plugins::codex_plugin_policy_value(
                        &state.inner.home,
                        &scope.cwd,
                        &identity.selector(),
                    )?;
                    let trust = state
                        .inner
                        .codex_capability_broker
                        .trust_value(&identity, &detail)?;
                    return Ok(codex_capability_broker::apply_codex_plugin_runtime_state(
                        codex_capability_broker::codex_plugin_read_value(&identity, detail),
                        policy,
                        trust,
                    ));
                }
                let mut query = ConfigurationQuery::new(&scope.cwd);
                query.inherited_env = Some(state.inner.inherited_env.clone());
                state
                    .inner
                    .framework
                    .configuration(query)?
                    .plugin(&params.selector)
            }
            "plugin/doctor" => {
                let params = request.params::<wire::agents_backend_rpc::PluginDoctorParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                if let Some(selector) = params.selector.as_deref()
                    && let Some(identity) =
                        codex_capability_broker::CodexPluginIdentity::parse_selector(selector)?
                {
                    let detail = state
                        .inner
                        .codex_capability_broker
                        .plugin_read(&scope.cwd, &identity)
                        .await?;
                    let apps = state
                        .inner
                        .codex_capability_broker
                        .request("app/list", json!({"threadId":null,"forceRefetch":false}))
                        .await;
                    return Ok(json!({
                        "plugins": [codex_capability_broker::codex_plugin_read_value(&identity, detail)],
                        "apps": match apps {
                            Ok(value) => value,
                            Err(err) => json!({"readiness":"unavailable","reason":err.to_string()}),
                        },
                    }));
                }
                let mut query = ConfigurationQuery::new(&scope.cwd);
                query.inherited_env = Some(state.inner.inherited_env.clone());
                state
                    .inner
                    .framework
                    .configuration(query)?
                    .diagnose_plugins(params.selector.as_deref())
                    .await
            }
            "plugin/import/inspect" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::PluginInspectParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                plugin_import_inspect_value(
                    &state.inner.home,
                    &scope.cwd,
                    PluginInspectOptions {
                        source: params.source,
                        source_kind: parse_plugin_source_kind(params.source_kind.as_deref())?,
                        git_ref: params.git_ref,
                        npm_version: params.npm_version,
                        npm_registry: params.npm_registry,
                    },
                )
            }
            "plugin/install" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::PluginInstallParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                if let Some(identity) =
                    codex_capability_broker::CodexPluginIdentity::parse_selector(&params.source)?
                {
                    let result = state
                        .inner
                        .codex_capability_broker
                        .plugin_install(&scope.cwd, &identity)
                        .await?;
                    state.invalidate_runnable_target_catalog();
                    let mut response = result.as_object().cloned().unwrap_or_default();
                    response.insert(
                        "authority".to_string(),
                        json!({
                            "kind": "codex",
                            "plugin": identity.plugin,
                            "marketplace": identity.marketplace,
                        }),
                    );
                    return Ok(Value::Object(response));
                }
                let result = plugin_install_value(
                    &state.inner.home,
                    &scope.cwd,
                    PluginInstallOptions {
                        source: params.source,
                        source_kind: parse_plugin_source_kind(params.source_kind.as_deref())?,
                        scope: parse_plugin_scope(params.scope_name.as_deref())?,
                        git_ref: params.git_ref,
                        npm_version: params.npm_version,
                        npm_registry: params.npm_registry,
                        force: params.force,
                    },
                );
                state.invalidate_runnable_target_catalog_after(result)
            }
            "plugin/uninstall" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::PluginUninstallParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                if let Some(identity) =
                    codex_capability_broker::CodexPluginIdentity::parse_selector(&params.selector)?
                {
                    let result = state
                        .inner
                        .codex_capability_broker
                        .plugin_uninstall(&scope.cwd, &identity)
                        .await?;
                    state.invalidate_runnable_target_catalog();
                    return Ok(json!({
                        "success": true,
                        "authority": {
                            "kind": "codex",
                            "plugin": identity.plugin,
                            "marketplace": identity.marketplace,
                        },
                        "result": result,
                    }));
                }
                let result = plugin_uninstall_value(
                    &state.inner.home,
                    &scope.cwd,
                    parse_plugin_scope(params.scope_name.as_deref())?,
                    &params.selector,
                );
                state.invalidate_runnable_target_catalog_after(result)
            }
            "plugin/setEnabled" => {
                let params = request
                    .required_params::<wire::agents_backend_rpc::PluginSetEnabledParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                if codex_capability_broker::CodexPluginIdentity::parse_selector(&params.selector)?
                    .is_some()
                {
                    let result = codex_plugin_set_enabled_value(
                        &state.inner.home,
                        &scope.cwd,
                        parse_plugin_scope(params.scope_name.as_deref())?,
                        &params.selector,
                        params.enabled,
                    );
                    if result.is_ok() {
                        state
                            .inner
                            .codex_capability_broker
                            .invalidate_runtime_inventories()
                            .await;
                    }
                    return state.invalidate_runnable_target_catalog_after(result);
                }
                let plugin_scope = parse_plugin_scope(params.scope_name.as_deref())?;
                let result = match params.enabled {
                    Some(enabled) => plugin_set_enabled_value(
                        &state.inner.home,
                        &scope.cwd,
                        plugin_scope,
                        &params.selector,
                        enabled,
                    ),
                    None => plugin_reset_enabled_value(
                        &state.inner.home,
                        &scope.cwd,
                        plugin_scope,
                        &params.selector,
                    ),
                };
                state.invalidate_runnable_target_catalog_after(result)
            }
            "extension/list" => {
                let params = request.params::<wire::agents_backend_rpc::ExtensionListParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                let mut result = extension_list_result(&state.inner.home, &scope.cwd)?;
                for extension in &mut result.extensions {
                    if let Some(reason) = state
                        .inner
                        .extension_app_leases
                        .reason_for(&extension.selector)
                    {
                        extension.sidecar_state = "active".to_string();
                        extension.lease_reason = Some(reason);
                    }
                }
                Ok(serde_json::to_value(result)?)
            }
            "extension/read" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::ExtensionReadParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                let mut result = extension_read_result(
                    &state.inner.home,
                    &scope.cwd,
                    &params.selector,
                    params.scope_name.as_deref(),
                )?;
                if let Some(reason) = state
                    .inner
                    .extension_app_leases
                    .reason_for(&result.extension.selector)
                {
                    result.extension.sidecar_state = "active".to_string();
                    result.extension.lease_reason = Some(reason);
                }
                Ok(serde_json::to_value(result)?)
            }
            "extension/remove" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::ExtensionRemoveParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                state.invalidate_runnable_target_catalog_after(
                    extension_remove_result(
                        &state.inner.extension_app_leases,
                        &state.inner.home,
                        &scope.cwd,
                        &params.selector,
                        params.scope_name.as_deref(),
                    )
                    .and_then(|result| serde_json::to_value(result).map_err(Into::into)),
                )
            }
            "extension/setEnabled" => {
                let params = request
                    .required_params::<wire::agents_backend_rpc::ExtensionSetEnabledParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                state.invalidate_runnable_target_catalog_after(
                    extension_set_enabled_result(
                        &state.inner.extension_app_leases,
                        &state.inner.home,
                        &scope.cwd,
                        &params.selector,
                        params.scope_name.as_deref(),
                        params.enabled,
                    )
                    .and_then(|result| serde_json::to_value(result).map_err(Into::into)),
                )
            }
            "extension/app/open" => {
                let params = request
                    .required_params::<wire::agents_backend_rpc::ExtensionAppOpenParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                Ok(serde_json::to_value(
                    extension_app_open_result(
                        &state.inner.extension_app_leases,
                        super::extension_management::ExtensionAppOpenRequest {
                            owner: out_tx.id(),
                            home: &state.inner.home,
                            cwd: &scope.cwd,
                            inherited_env: &state.inner.inherited_env,
                            selector: &params.selector,
                            scope_name: params.scope_name.as_deref(),
                            app_id: &params.app_id,
                        },
                    )
                    .await?,
                )?)
            }
            "extension/app/close" => {
                let params = request
                    .required_params::<wire::agents_backend_rpc::ExtensionAppCloseParams>()?;
                let _scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                Ok(serde_json::to_value(
                    extension_app_close_result(
                        &state.inner.extension_app_leases,
                        out_tx.id(),
                        &params.lease_id,
                    )
                    .await?,
                )?)
            }
            "plugin/authority/write" => {
                let params = request
                    .required_params::<wire::agents_backend_rpc::PluginAuthorityWriteParams>()?;
                let _scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                state
                    .inner
                    .codex_capability_broker
                    .write_authority(params.enabled, params.binary.as_deref())
                    .await
            }
            "plugin/authority/refresh" => {
                let params =
                    request.params::<wire::agents_backend_rpc::PluginAuthorityRefreshParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                state
                    .inner
                    .codex_capability_broker
                    .refresh_authority(&scope.cwd)
                    .await
            }
            "plugin/authority/setTrust" => {
                let params = request
                    .required_params::<wire::agents_backend_rpc::PluginAuthoritySetTrustParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                let identity =
                    codex_capability_broker::CodexPluginIdentity::parse_selector(&params.selector)?
                        .ok_or_else(|| {
                            psychevo::Error::Message(
                                "plugin/authority/setTrust accepts only Codex authority selectors"
                                    .to_string(),
                            )
                        })?;
                let detail = state
                    .inner
                    .codex_capability_broker
                    .plugin_read(&scope.cwd, &identity)
                    .await?;
                let result = state.inner.codex_capability_broker.set_trust(
                    &identity,
                    &detail,
                    params.trusted,
                );
                if result.is_ok() {
                    state
                        .inner
                        .codex_capability_broker
                        .invalidate_runtime_inventories()
                        .await;
                }
                result
            }
            "plugin/catalog/list" => {
                let params =
                    request.params::<wire::agents_backend_rpc::PluginCatalogListParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                if params.authority.as_deref() == Some("codex") {
                    return state
                        .inner
                        .codex_capability_broker
                        .plugin_list(&scope.cwd)
                        .await;
                }
                plugin_marketplace_list_value(
                    &state.inner.home,
                    &scope.cwd,
                    parse_plugin_scope(params.scope_name.as_deref())?,
                )
            }
            "plugin/catalog/add" => {
                let params = request
                    .required_params::<wire::agents_backend_rpc::PluginCatalogAddParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                if params.authority.as_deref() == Some("codex") {
                    return state
                        .inner
                        .codex_capability_broker
                        .catalog_add(
                            &params.source,
                            params.git_ref.as_deref(),
                            &params.sparse_paths,
                        )
                        .await;
                }
                plugin_marketplace_add_value(
                    &state.inner.home,
                    &scope.cwd,
                    parse_plugin_scope(params.scope_name.as_deref())?,
                    PluginMarketplaceEntry {
                        name: params.name,
                        source: params.source,
                        kind: params.kind,
                        git_ref: params.git_ref,
                        npm_version: params.npm_version,
                        npm_registry: params.npm_registry,
                    },
                )
            }
            "plugin/catalog/remove" => {
                let params = request
                    .required_params::<wire::agents_backend_rpc::PluginCatalogRemoveParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                if params.authority.as_deref() == Some("codex") {
                    return state
                        .inner
                        .codex_capability_broker
                        .catalog_remove(&params.name)
                        .await;
                }
                plugin_marketplace_remove_value(
                    &state.inner.home,
                    &scope.cwd,
                    parse_plugin_scope(params.scope_name.as_deref())?,
                    &params.name,
                )
            }
            "plugin/catalog/upgrade" => {
                let params = request
                    .required_params::<wire::agents_backend_rpc::PluginCatalogUpgradeParams>()?;
                let _scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                if params.authority.as_deref() != Some("codex") {
                    return Err(Error::Message(
                        "catalog upgrade is currently supported only for the Codex authority"
                            .to_string(),
                    ));
                }
                state
                    .inner
                    .codex_capability_broker
                    .catalog_upgrade(
                        Some(&params.name),
                        params.source.as_deref(),
                        params.git_ref.as_deref(),
                        &params.sparse_paths,
                    )
                    .await
            }
            "plugin/connect/start" => {
                let params = request
                    .required_params::<wire::agents_backend_rpc::PluginConnectStartParams>()?;
                let _scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                state
                    .inner
                    .codex_capability_broker
                    .connect_start(
                        &params.selector,
                        &params.component_id,
                        params.kind.as_deref(),
                    )
                    .await
            }
            "plugin/connect/status" => {
                let params = request
                    .required_params::<wire::agents_backend_rpc::PluginConnectStatusParams>()?;
                let _scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                state
                    .inner
                    .codex_capability_broker
                    .connect_status(&params.session_id)
                    .await
            }
            "skill/list" => {
                let params = request.params::<wire::agents_backend_rpc::SkillListParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                let catalog = discover_skills(&SkillDiscoveryOptions {
                    home: state.inner.home.clone(),
                    cwd: scope.cwd,
                    config_path: state.inner.config_path.clone(),
                    env: state.inner.inherited_env.clone(),
                    explicit_inputs: Vec::new(),
                    additional_roots: Vec::new(),
                    no_skills: false,
                })?;
                Ok(list_skills_value_with_options(
                    &catalog,
                    &ListSkillsOptions {
                        include_hidden: true,
                        detail: true,
                        ..ListSkillsOptions::default()
                    },
                ))
            }
            "skill/read" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::SkillReadParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                let catalog = discover_skills(&SkillDiscoveryOptions {
                    home: state.inner.home.clone(),
                    cwd: scope.cwd,
                    config_path: state.inner.config_path.clone(),
                    env: state.inner.inherited_env.clone(),
                    explicit_inputs: Vec::new(),
                    additional_roots: Vec::new(),
                    no_skills: false,
                })?;
                view_skill_value_selected(
                    &catalog,
                    &params.name,
                    params.path.as_deref().map(std::path::Path::new),
                    None,
                )
            }
            "skill/install" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::SkillInstallParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                install_skill(
                    &state.inner.home,
                    &scope.cwd,
                    InstallOptions {
                        source: params.source,
                        target: parse_skill_target(params.target.as_deref())?,
                        name: params.name,
                        all: params.all,
                        force: params.force,
                    },
                )
            }
            "skill/uninstall" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::SkillUninstallParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                remove_installed_skill(
                    &state.inner.home,
                    &scope.cwd,
                    parse_skill_target(params.target.as_deref())?,
                    &params.name,
                    params.path.as_deref().map(std::path::Path::new),
                )
            }
            "skill/setEnabled" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::SkillSetEnabledParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                set_skill_enabled(
                    &state.inner.home,
                    &scope.cwd,
                    parse_skill_target(params.target.as_deref())?,
                    &params.name,
                    params.enabled,
                )
            }
            "skill/write" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::SkillWriteParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                write_installed_skill(
                    &state.inner.home,
                    &scope.cwd,
                    parse_skill_target(params.target.as_deref())?,
                    &params.name,
                    params.path.as_deref().map(std::path::Path::new),
                    &params.raw_markdown,
                )
            }
            "tool/list" => {
                let params = request.params::<wire::agents_backend_rpc::ToolListParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                gateway_configuration(&state, scope.cwd)?.toolsets(ConfigScope::Effective)
            }
            "tool/read" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::ToolReadParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                let value =
                    gateway_configuration(&state, scope.cwd)?.toolsets(ConfigScope::Effective)?;
                let toolsets = value
                    .get("toolsets")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let Some(toolset) = toolsets.into_iter().find(|toolset| {
                    toolset
                        .get("name")
                        .and_then(Value::as_str)
                        .is_some_and(|name| name == params.name)
                }) else {
                    return Err(Error::Config(format!("unknown toolset: {}", params.name)));
                };
                Ok(json!({"toolset": toolset}))
            }
            "tool/setEnabled" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::ToolSetEnabledParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                let config_dir = tool_config_dir(&state, &scope, params.local);
                let mode = parse_tool_mode(&params.mode)?;
                Ok(toolset_mutation_value(set_local_toolset_enabled(
                    config_dir,
                    mode,
                    &params.name,
                    params.enabled,
                )?))
            }
            "tool/create" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::ToolCreateParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                let config_dir = tool_config_dir(&state, &scope, params.local);
                Ok(toolset_mutation_value(create_local_toolset(
                    config_dir,
                    &params.name,
                    params.description,
                    params.tools,
                    params.includes,
                    params.force,
                )?))
            }
            "tool/remove" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::ToolRemoveParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                let config_dir = tool_config_dir(&state, &scope, params.local);
                Ok(toolset_mutation_value(remove_local_toolset(
                    config_dir,
                    &params.name,
                )?))
            }
            "mcp/list" => {
                let params = request.params::<wire::agents_backend_rpc::McpListParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                gateway_configuration(&state, scope.cwd)?.mcp_servers(ConfigScope::Effective)
            }
            "mcp/read" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::McpReadParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                gateway_configuration(&state, scope.cwd)?.mcp_server(&params.name)
            }
            "mcp/upsert" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::McpUpsertParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                upsert_mcp_server(
                    active_profile_config_dir(&state, &scope),
                    mcp_config_input(params),
                )
            }
            "mcp/remove" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::McpNameParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                remove_mcp_server(active_profile_config_dir(&state, &scope), &params.name)
            }
            "mcp/setEnabled" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::McpSetEnabledParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                set_mcp_server_enabled(
                    active_profile_config_dir(&state, &scope),
                    &params.name,
                    params.enabled,
                )
            }
            "mcp/setToolPolicy" => {
                let params = request
                    .required_params::<wire::agents_backend_rpc::McpSetToolPolicyParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                set_mcp_server_tool_policy(
                    active_profile_config_dir(&state, &scope),
                    &params.name,
                    McpToolPolicyInput {
                        enabled_tools: params.enabled_tools,
                        disabled_tools: params.disabled_tools,
                    },
                )
            }
            "mcp/test" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::McpNameParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                gateway_configuration(&state, scope.cwd)?
                    .test_mcp_server(&params.name)
                    .await
            }
            "mcp/oauth/start" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::McpOAuthStartParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                mcp_oauth_start_value(state, scope, params).await
            }
            "mcp/oauth/status" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::McpOAuthStatusParams>()?;
                mcp_oauth_status_value(&state, &params.session_id)
            }
            "mcp/oauth/logout" => {
                let params =
                    request.required_params::<wire::agents_backend_rpc::McpNameParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                mcp_oauth_logout_value(&state, &scope, &params.name)
            }
            "channel/list" => {
                let params = request.params::<wire::channels::ChannelListParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                Ok(serde_json::to_value(channel_list_result_for_scope(
                    &state, &scope,
                )?)?)
            }
            "channel/show" => {
                let params = request.required_params::<wire::channels::ChannelIdParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                Ok(serde_json::to_value(channel_show_result(
                    &state, &scope, &params.id,
                )?)?)
            }
            "channel/enable" => {
                let params = request.required_params::<wire::channels::ChannelEnableParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                Ok(serde_json::to_value(channel_enable_result(
                    &state, &scope, params,
                )?)?)
            }
            "channel/update" => {
                let params = request.required_params::<wire::channels::ChannelUpdateParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                Ok(serde_json::to_value(
                    channel_update_result(&state, &scope, params).await?,
                )?)
            }
            "channel/delete" => {
                let params = request.required_params::<wire::channels::ChannelIdParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                Ok(serde_json::to_value(channel_delete_result(
                    &state, &scope, params,
                )?)?)
            }
            "channel/doctor" => {
                let params = request.params::<wire::channels::ChannelDoctorParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                Ok(serde_json::to_value(
                    channel_doctor_result_live(&state, &scope, params).await?,
                )?)
            }
            "channel/source/list" => {
                let params = request.required_params::<wire::channels::ChannelIdParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                Ok(serde_json::to_value(
                    channel_source_list_result(&state, &scope, params).await?,
                )?)
            }
            "channel/wechat-qr/start" => {
                let params = request.params::<wire::channels::ChannelWechatQrStartParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                Ok(serde_json::to_value(
                    channel_wechat_qr_start_result(&state, &scope, params).await?,
                )?)
            }
            "channel/wechat-qr/poll" => {
                let params =
                    request.required_params::<wire::channels::ChannelWechatQrPollParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                Ok(serde_json::to_value(
                    channel_wechat_qr_poll_result(&state, &scope, params).await?,
                )?)
            }
            "command/list" => {
                let params = request.params::<wire::thread_command_turn::CommandListParams>()?;
                let scope = resolve_optional_scope(&state, &auth, params.scope.clone())?;
                let active_turn = if let Some(thread_id) = params.thread_id.as_deref() {
                    authorize_thread(&state, &auth, thread_id).await?;
                    state.activity(&scope.source, Some(thread_id)).await.running
                } else {
                    state.activity(&scope.source, None).await.running
                };
                command_list_value(&state, &scope, active_turn, params.thread_id.is_some())
            }
            "command/execute" => {
                let params =
                    request.required_params::<wire::thread_command_turn::CommandExecuteParams>()?;
                let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
                if let Some(thread_id) = &params.thread_id {
                    authorize_thread(&state, &auth, thread_id).await?;
                }
                command_execute_value(&state, &scope, params).await
            }
            "slash/settings/read" => {
                let params =
                    request.params::<wire::thread_command_turn::SlashSettingsReadParams>()?;
                let cwd = resolve_cwd_filter(&state, &auth, params.cwd)?;
                let scope = resolve_optional_scope(&state, &auth, None)?;
                slash_settings_read_value(&state, &scope, &cwd)
            }
            "slash/settings/update" => {
                let params = request
                    .required_params::<wire::thread_command_turn::SlashSettingsUpdateParams>()?;
                let cwd = resolve_cwd_filter(&state, &auth, params.cwd.clone())?;
                let scope = resolve_optional_scope(&state, &auth, None)?;
                slash_settings_update_value(&state, &scope, &cwd, params)
            }
            "shell/start" => {
                let params =
                    request.required_params::<wire::thread_command_turn::ShellStartParams>()?;
                let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
                let command = params.command.trim().to_string();
                if command.is_empty() {
                    return Ok(serde_json::to_value(
                        wire::thread_command_turn::ShellStartResult {
                            accepted: false,
                            thread_id: params.thread_id,
                            message: Some(
                                "shell mode: type !<command> to run a local shell command"
                                    .to_string(),
                            ),
                        },
                    )?);
                }
                let thread_id = match params.thread_id.clone() {
                    Some(thread_id) => {
                        authorize_thread(&state, &auth, &thread_id).await?;
                        Some(thread_id)
                    }
                    None => {
                        state
                            .inner
                            .gateway
                            .resolve_source_thread(&scope.source)
                            .await?
                    }
                };
                if state
                    .inner
                    .gateway
                    .resolve_source_thread(&scope.source)
                    .await?
                    .as_deref()
                    != thread_id.as_deref()
                    && let Some(thread_id) = thread_id.as_deref()
                {
                    bind_source_to_thread(&state, &scope, thread_id).await?;
                }
                let event_selector = thread_id
                    .as_ref()
                    .map(GatewayThreadSelector::thread_id)
                    .unwrap_or_else(|| GatewayThreadSelector::source(scope.source.source_key()));
                let event_thread_id = thread_id.clone();
                let event_state = state.clone();
                let event_tx = out_tx.clone();
                let event_sink = GatewayEventEmitter::new(move |event| {
                    let context = event_state
                        .pending_context_for_selector(&event_selector, event_thread_id.as_deref());
                    event_state.publish_gateway_event_for_connection(
                        event,
                        context,
                        None,
                        Some(&event_tx),
                    );
                });
                let execution = shell_execution_intent(&state, &scope);
                let gateway = state.inner.gateway.clone();
                let source = scope.source.clone();
                let bind_source = cwd_source(&scope.cwd);
                let cwd = scope.cwd.clone();
                let result_thread_id = thread_id.clone();
                let supervisor = state.inner.gateway.clone();
                supervisor.spawn_background("shell-result", async move {
                    let result = gateway
                        .send_shell(SendShellRequest {
                            thread_id: result_thread_id.clone(),
                            source: Some(source),
                            bind_source: Some(bind_source),
                            cwd,
                            command,
                            execution,
                            event_sink: Some(event_sink),
                            lineage: Some(json!({"reason": "shell_start"})),
                        })
                        .await;
                    let notification = match result {
                        Ok(result) => {
                            rpc_notification("shell/result", gateway_shell_result_value(result))
                        }
                        Err(err) => rpc_notification(
                            "shell/error",
                            json!({"message": err.to_string(), "threadId": result_thread_id}),
                        ),
                    };
                    let _ = out_tx.send(notification);
                });
                Ok(serde_json::to_value(
                    wire::thread_command_turn::ShellStartResult {
                        accepted: true,
                        thread_id,
                        message: None,
                    },
                )?)
            }
            "terminal/start" => {
                let params =
                    request.required_params::<wire::thread_command_turn::TerminalStartParams>()?;
                let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
                Ok(serde_json::to_value(state.inner.terminals.start(
                    &scope,
                    params,
                    &state.inner.inherited_env,
                    out_tx,
                )?)?)
            }
            "terminal/write" => {
                let params =
                    request.required_params::<wire::thread_command_turn::TerminalWriteParams>()?;
                Ok(serde_json::to_value(
                    state.inner.terminals.write(out_tx.id(), params)?,
                )?)
            }
            "terminal/resize" => {
                let params =
                    request.required_params::<wire::thread_command_turn::TerminalResizeParams>()?;
                Ok(serde_json::to_value(
                    state.inner.terminals.resize(out_tx.id(), params)?,
                )?)
            }
            "terminal/terminate" => {
                let params = request
                    .required_params::<wire::thread_command_turn::TerminalTerminateParams>()?;
                Ok(serde_json::to_value(
                    state.inner.terminals.terminate(params, out_tx)?,
                )?)
            }
            "settings/read" => {
                let params =
                    request.params::<wire::settings_workspace_context::SettingsReadParams>()?;
                let (cwd, thread_id) = if let Some(thread_id) = params.thread_id {
                    authorize_thread(&state, &auth, &thread_id).await?;
                    let summary = state
                        .inner
                        .framework
                        .thread_summary(&thread_id)
                        .await?
                        .ok_or_else(|| Error::Message(format!("thread not found: {thread_id}")))?;
                    (PathBuf::from(summary.cwd), Some(thread_id))
                } else {
                    (resolve_cwd_filter(&state, &auth, params.cwd)?, None)
                };
                settings_read_value(&state, &cwd, thread_id.as_deref()).await
            }
            "web/search/settings/read" => {
                let params = request
                    .params::<wire::settings_workspace_context::WebSearchSettingsReadParams>(
                )?;
                let cwd = resolve_cwd_filter(&state, &auth, params.cwd)?;
                web_search_settings_value(&state, &cwd)
            }
            "web/search/settings/update" => {
                let params = request.required_params::<wire::settings_workspace_context::WebSearchSettingsUpdateParams>()?;
                let scope = resolve_required_scope(&state, &auth, params.scope.clone())?;
                web_search_settings_update_value(&state, &scope.cwd, params)
            }
            "settings/update" => {
                let params = request
                    .required_params::<wire::settings_workspace_context::SettingsUpdateParams>()?;
                let scope = resolve_required_scope(&state, &auth, params.scope)?;
                authorize_thread(&state, &auth, &params.thread_id).await?;
                update_session_agent_setting(
                    &state,
                    &scope,
                    &params.thread_id,
                    params.agent.as_deref(),
                )
                .await?;
                settings_read_value(&state, &scope.cwd, Some(&params.thread_id)).await
            }
            "model/settings/read" => {
                let params = request
                    .params::<wire::settings_workspace_context::ModelSettingsReadParams>()?;
                let cwd = resolve_cwd_filter(&state, &auth, params.cwd)?;
                model_settings_value(&state, &cwd)
            }
            "model/provider/save" => {
                let params = request
                    .required_params::<wire::settings_workspace_context::ModelProviderSaveParams>(
                    )?;
                let cwd = default_resolved_scope(&state, &auth)?.cwd;
                model_provider_save_value(&state, &cwd, params)
            }
            "model/provider/catalog" => {
                let params = request.required_params::<wire::settings_workspace_context::ModelProviderCatalogParams>()?;
                let cwd = resolve_cwd_filter(&state, &auth, params.cwd.clone())?;
                model_provider_catalog_value(&state, &cwd, params).await
            }
            "model/state/read" => {
                let params =
                    request.params::<wire::settings_workspace_context::ModelStateReadParams>()?;
                let (cwd, thread_id) =
                    resolve_model_state_request_scope(&state, &auth, params.cwd, params.thread_id)
                        .await?;
                model_state_read_value(&state, &cwd, thread_id.as_deref()).await
            }
            "model/state/set" => {
                let params = request
                    .required_params::<wire::settings_workspace_context::ModelStateSetParams>()?;
                let (cwd, thread_id) = resolve_model_state_request_scope(
                    &state,
                    &auth,
                    params.cwd.clone(),
                    params.thread_id.clone(),
                )
                .await?;
                model_state_set_value(&state, &cwd, thread_id.as_deref(), params).await
            }
            "model/assignment/set" => {
                let params = request
                    .required_params::<wire::settings_workspace_context::ModelAssignmentSetParams>(
                    )?;
                let cwd = default_resolved_scope(&state, &auth)?.cwd;
                model_assignment_set_value(&state, &cwd, params)
            }
            method => Err(Error::Message(format!("method not found: {method}"))),
        }
    })
}

pub(super) fn runtime_rpc_error(
    code: &str,
    stage: &str,
    retry_class: wire::agents_backend_rpc::RuntimeRetryClassView,
    message: String,
    diagnostic_ref: Option<String>,
) -> Error {
    let view = wire::agents_backend_rpc::RuntimeErrorView {
        code: code.to_string(),
        stage: stage.to_string(),
        retry_class,
        message: message.clone(),
        diagnostic_ref,
    };
    Error::structured(
        message,
        serde_json::to_value(view).expect("runtime error view serializes"),
    )
}

fn gateway_configuration(
    state: &WebState,
    cwd: PathBuf,
) -> psychevo::Result<psychevo::Configuration> {
    let mut query = ConfigurationQuery::new(cwd);
    query.inherited_env = Some(state.inner.inherited_env.clone());
    state
        .inner
        .framework
        .configuration(query)
        .map(|configuration| {
            configuration
                .with_mcp_oauth_credential_store(Arc::clone(&state.inner.mcp_oauth_credentials))
        })
}

fn parse_skill_target(value: Option<&str>) -> psychevo::Result<SkillTarget> {
    match value.unwrap_or("global") {
        "global" | "profile" => Ok(SkillTarget::Global),
        "project" | "local" => Ok(SkillTarget::Project),
        other => Err(Error::Config(format!("unknown skill target: {other}"))),
    }
}

fn parse_plugin_scope(value: Option<&str>) -> psychevo::Result<PluginScope> {
    match value.unwrap_or("global") {
        "global" | "profile" => Ok(PluginScope::Global),
        "local" | "project" => Ok(PluginScope::Local),
        other => Err(Error::Config(format!("unknown plugin scope: {other}"))),
    }
}

fn parse_plugin_source_kind(value: Option<&str>) -> psychevo::Result<Option<PluginSourceKind>> {
    value
        .map(|value| {
            PluginSourceKind::parse(value).ok_or_else(|| {
                Error::Config(format!(
                    "unknown plugin source kind `{value}`; expected local, git, or npm"
                ))
            })
        })
        .transpose()
}

fn parse_tool_mode(value: &str) -> psychevo::Result<RunMode> {
    RunMode::parse(value).ok_or_else(|| Error::Config(format!("unknown tool mode: {value}")))
}

fn tool_config_dir(state: &WebState, scope: &ResolvedScope, local: bool) -> PathBuf {
    if local {
        scope.cwd.join(".psychevo")
    } else {
        active_profile_config_dir(state, scope)
    }
}

fn toolset_mutation_value(result: psychevo::config::ToolsetMutationResult) -> Value {
    json!({
        "success": true,
        "changed": result.changed,
        "name": result.name,
        "path": result.config_path,
    })
}

fn mcp_config_input(params: wire::agents_backend_rpc::McpUpsertParams) -> McpServerConfigInput {
    McpServerConfigInput {
        name: params.name,
        transport: params.transport,
        enabled: params.enabled,
        required: params.required,
        command: params.command,
        args: params.args,
        env: params.env,
        cwd: params.cwd,
        url: params.url,
        headers: params.headers,
        bearer_token_env_var: params.bearer_token_env_var,
        scopes: params.scopes,
        oauth_resource: params.oauth_resource,
        oauth_client_id: params.oauth_client_id,
        enabled_tools: params.enabled_tools,
        disabled_tools: params.disabled_tools,
        supports_parallel_tool_calls: params.supports_parallel_tool_calls,
        startup_timeout_secs: params.startup_timeout_secs,
        tool_timeout_secs: params.tool_timeout_secs,
    }
}

#[derive(Debug, Clone)]
struct McpOAuthMetadata {
    name: String,
    url: String,
    client_id: String,
    scopes: Vec<String>,
    oauth_resource: Option<String>,
    profile_home: PathBuf,
}

async fn mcp_oauth_start_value(
    state: WebState,
    scope: ResolvedScope,
    params: wire::agents_backend_rpc::McpOAuthStartParams,
) -> psychevo::Result<Value> {
    let metadata = mcp_oauth_metadata(&state, &scope, &params.name)?;
    let session_id = Uuid::now_v7().to_string();
    let state_token = Uuid::now_v7().to_string();
    let deadline = state
        .inner
        .mcp_oauth_sessions
        .lock()
        .expect("mcp oauth sessions poisoned")
        .admit(session_id.clone(), Instant::now())?;
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await {
        Ok(listener) => listener,
        Err(err) => {
            state
                .inner
                .mcp_oauth_sessions
                .lock()
                .expect("mcp oauth sessions poisoned")
                .remove(&session_id);
            return Err(err.into());
        }
    };
    let redirect_uri = match listener.local_addr() {
        Ok(addr) => format!("http://{addr}/callback"),
        Err(err) => {
            state
                .inner
                .mcp_oauth_sessions
                .lock()
                .expect("mcp oauth sessions poisoned")
                .remove(&session_id);
            return Err(err.into());
        }
    };
    let authorization_url = match mcp_authorization_url(&metadata, &redirect_uri, &state_token) {
        Ok(url) => url,
        Err(err) => {
            state
                .inner
                .mcp_oauth_sessions
                .lock()
                .expect("mcp oauth sessions poisoned")
                .remove(&session_id);
            return Err(err);
        }
    };
    let sessions = Arc::clone(&state.inner.mcp_oauth_sessions);
    let credentials = Arc::clone(&state.inner.mcp_oauth_credentials);
    state.inner.gateway.spawn_background(
        format!("mcp-oauth-callback:{session_id}"),
        run_mcp_oauth_callback(McpOAuthCallbackTask {
            listener,
            metadata,
            redirect_uri,
            state_token,
            session_id: session_id.clone(),
            deadline,
            sessions,
            credentials,
        }),
    );
    Ok(serde_json::to_value(
        wire::capability_results::McpOAuthStartResult::Pending {
            session_id,
            authorization_url,
        },
    )?)
}

fn mcp_oauth_status_value(state: &WebState, session_id: &str) -> psychevo::Result<Value> {
    let status = state
        .inner
        .mcp_oauth_sessions
        .lock()
        .expect("mcp oauth sessions poisoned")
        .status(session_id, Instant::now());
    let Some(status) = status else {
        return Err(Error::Config(format!(
            "unknown MCP OAuth session: {session_id}"
        )));
    };
    let result = match status {
        McpOAuthSessionStatus::Pending | McpOAuthSessionStatus::Persisting => {
            wire::capability_results::McpOAuthStatusResult::Pending {
                session_id: session_id.to_string(),
            }
        }
        McpOAuthSessionStatus::Succeeded => {
            wire::capability_results::McpOAuthStatusResult::Succeeded {
                session_id: session_id.to_string(),
            }
        }
        McpOAuthSessionStatus::Failed { message } => {
            wire::capability_results::McpOAuthStatusResult::Failed {
                session_id: session_id.to_string(),
                message,
            }
        }
    };
    Ok(serde_json::to_value(result)?)
}

fn mcp_oauth_logout_value(
    state: &WebState,
    scope: &ResolvedScope,
    name: &str,
) -> psychevo::Result<Value> {
    let metadata = mcp_oauth_metadata(state, scope, name)?;
    let removed = clear_mcp_oauth_access_token_with_store(
        state.inner.mcp_oauth_credentials.as_ref(),
        &metadata.profile_home,
        &metadata.name,
        &metadata.url,
    )?;
    Ok(json!({
        "success": true,
        "name": metadata.name,
        "removed": removed,
    }))
}

fn mcp_oauth_metadata(
    state: &WebState,
    scope: &ResolvedScope,
    name: &str,
) -> psychevo::Result<McpOAuthMetadata> {
    let value = gateway_configuration(state, scope.cwd.clone())?.mcp_server(name)?;
    let server = value
        .get("server")
        .ok_or_else(|| Error::Config(format!("unknown MCP server: {name}")))?;
    let transport = server
        .get("transport")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Config(format!("MCP server {name} has no transport")))?;
    if transport.get("kind").and_then(Value::as_str) != Some("streamable_http") {
        return Err(Error::Config(format!(
            "MCP OAuth is only supported for streamable HTTP servers: {name}"
        )));
    }
    let url = transport
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Config(format!("MCP server {name} has no URL")))?
        .to_string();
    let auth = transport
        .get("auth")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Config(format!("MCP server {name} has no OAuth metadata")))?;
    let client_id = auth
        .get("oauthClientId")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Error::Config(format!(
                "MCP server {name} must configure oauth.client_id before OAuth login"
            ))
        })?
        .to_string();
    let scopes = auth
        .get("scopes")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let oauth_resource = auth
        .get("oauthResource")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(McpOAuthMetadata {
        name: name.to_string(),
        url,
        client_id,
        scopes,
        oauth_resource,
        profile_home: active_profile_config_dir(state, scope),
    })
}

fn mcp_authorization_url(
    metadata: &McpOAuthMetadata,
    redirect_uri: &str,
    state_token: &str,
) -> psychevo::Result<String> {
    let base = metadata
        .oauth_resource
        .as_deref()
        .unwrap_or(metadata.url.as_str());
    let mut url = reqwest::Url::parse(&oauth_endpoint(base, "authorize"))
        .map_err(|err| Error::Config(format!("failed to build OAuth authorization URL: {err}")))?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("response_type", "code");
        query.append_pair("client_id", &metadata.client_id);
        query.append_pair("redirect_uri", redirect_uri);
        query.append_pair("state", state_token);
        if !metadata.scopes.is_empty() {
            query.append_pair("scope", &metadata.scopes.join(" "));
        }
        if let Some(resource) = &metadata.oauth_resource {
            query.append_pair("resource", resource);
        }
    }
    Ok(url.to_string())
}

struct McpOAuthCallbackTask {
    listener: TcpListener,
    metadata: McpOAuthMetadata,
    redirect_uri: String,
    state_token: String,
    session_id: String,
    deadline: Instant,
    sessions: Arc<Mutex<McpOAuthSessionStore>>,
    credentials: Arc<dyn McpOAuthCredentialStore>,
}

async fn run_mcp_oauth_callback(task: McpOAuthCallbackTask) {
    let McpOAuthCallbackTask {
        listener,
        metadata,
        redirect_uri,
        state_token,
        session_id,
        deadline,
        sessions,
        credentials,
    } = task;
    let callback = async {
        let (mut stream, _) = listener.accept().await?;
        let mut buffer = vec![0_u8; 8192];
        let size = stream.read(&mut buffer).await?;
        let request = String::from_utf8_lossy(&buffer[..size]);
        let target = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or("/");
        let callback_url = reqwest::Url::parse(&format!("http://localhost{target}"))
            .map_err(|err| Error::Config(format!("OAuth callback parse failed: {err}")))?;
        let pairs = callback_url
            .query_pairs()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect::<BTreeMap<_, _>>();
        if pairs.get("state") != Some(&state_token) {
            write_oauth_callback_response(&mut stream, false).await?;
            return Err(Error::Config("OAuth callback state mismatch".to_string()));
        }
        let Some(code) = pairs.get("code").cloned() else {
            write_oauth_callback_response(&mut stream, false).await?;
            return Err(Error::Config(
                "OAuth callback did not include code".to_string(),
            ));
        };
        let token = match exchange_mcp_oauth_code(&metadata, &redirect_uri, &code).await {
            Ok(token) => token,
            Err(error) => {
                let _ = write_oauth_callback_response(&mut stream, false).await;
                return Err(error);
            }
        };
        Ok::<_, Error>((stream, token))
    };
    let result =
        match tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), callback).await {
            Ok(Ok((mut stream, token))) => {
                let persistence_admitted = sessions
                    .lock()
                    .expect("mcp oauth sessions poisoned")
                    .begin_persistence(&session_id, Instant::now());
                if !persistence_admitted {
                    let _ = write_oauth_callback_response(&mut stream, false).await;
                    Err(Error::Config("MCP OAuth login timed out".to_string()))
                } else {
                    let persisted = save_mcp_oauth_access_token_with_store(
                        credentials.as_ref(),
                        &metadata.profile_home,
                        &metadata.name,
                        &metadata.url,
                        &token,
                    );
                    complete_mcp_oauth_session(&sessions, &session_id, &persisted);
                    let _ = write_oauth_callback_response(&mut stream, persisted.is_ok()).await;
                    return;
                }
            }
            Ok(Err(error)) => Err(error),
            Err(_) => Err(Error::Config("MCP OAuth login timed out".to_string())),
        };
    complete_mcp_oauth_session(&sessions, &session_id, &result);
}

fn complete_mcp_oauth_session(
    sessions: &Mutex<McpOAuthSessionStore>,
    session_id: &str,
    result: &psychevo::Result<()>,
) {
    let status = match result {
        Ok(()) => McpOAuthSessionStatus::Succeeded,
        Err(err) => McpOAuthSessionStatus::Failed {
            message: err.to_string(),
        },
    };
    sessions
        .lock()
        .expect("mcp oauth sessions poisoned")
        .complete(session_id, status, Instant::now());
}

async fn write_oauth_callback_response(
    stream: &mut tokio::net::TcpStream,
    success: bool,
) -> std::io::Result<()> {
    let body = if success {
        "OAuth login finished. You can return to Psychevo."
    } else {
        "OAuth login failed. You can close this page."
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes()).await
}

async fn exchange_mcp_oauth_code(
    metadata: &McpOAuthMetadata,
    redirect_uri: &str,
    code: &str,
) -> psychevo::Result<String> {
    let base = metadata
        .oauth_resource
        .as_deref()
        .unwrap_or(metadata.url.as_str());
    let token_endpoint = oauth_endpoint(base, "token");
    let mut form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
        ("client_id", metadata.client_id.clone()),
    ];
    if !metadata.scopes.is_empty() {
        form.push(("scope", metadata.scopes.join(" ")));
    }
    if let Some(resource) = &metadata.oauth_resource {
        form.push(("resource", resource.clone()));
    }
    let response = reqwest::Client::new()
        .post(token_endpoint)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(form_urlencoded(&form))
        .send()
        .await
        .map_err(|err| Error::Config(format!("OAuth token request failed: {err}")))?;
    let status = response.status();
    let value = response
        .json::<Value>()
        .await
        .map_err(|err| Error::Config(format!("OAuth token response parse failed: {err}")))?;
    if !status.is_success() {
        return Err(Error::Config(format!(
            "OAuth token request failed with HTTP {status}: {value}"
        )));
    }
    value
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|token| !token.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| Error::Config("OAuth token response omitted access_token".to_string()))
}

fn oauth_endpoint(base: &str, suffix: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), suffix)
}

fn form_urlencoded(values: &[(&str, String)]) -> String {
    values
        .iter()
        .map(|(key, value)| format!("{}={}", url_percent_encode(key), url_percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn url_percent_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

pub(super) async fn thread_compact_result_for_thread(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: String,
    instructions: Option<String>,
    runtime_ref: String,
    out_tx: ConnectionSender,
) -> psychevo::Result<wire::thread_command_turn::ThreadCompactionResult> {
    enqueue_thread_compact_result_for_thread(
        state,
        scope,
        thread_id,
        instructions,
        runtime_ref,
        out_tx,
    )
    .await?
    .await
}

pub(super) async fn enqueue_thread_compact_result_for_thread(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: String,
    instructions: Option<String>,
    runtime_ref: String,
    out_tx: ConnectionSender,
) -> psychevo::Result<
    BoxFuture<'static, psychevo::Result<wire::thread_command_turn::ThreadCompactionResult>>,
> {
    let thread = state.inner.framework.resume_thread(&thread_id).await?;
    let binding = state
        .inner
        .gateway
        .framework_agent_binding(&thread_id)
        .await?;
    let non_native_runtime = binding
        .as_ref()
        .is_some_and(|binding| binding.backend_kind == "acp")
        || (binding.is_none() && runtime_ref != "native");
    let mut inherited_env = state.inner.inherited_env.clone();
    inherited_env
        .entry("PSYCHEVO_HOME".to_string())
        .or_insert_with(|| state.inner.home.to_string_lossy().into_owned());
    let request = psychevo::CompactThreadRequest {
        config_path: state.inner.config_path.clone(),
        instructions: instructions
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        force: true,
        inherited_env: Some(inherited_env),
        ..psychevo::CompactThreadRequest::default()
    };
    let source = scope.source.clone();
    let event_selector = GatewayThreadSelector::thread_id(&thread_id);
    let event_thread_id = thread_id.clone();
    let event_state = state.clone();
    let event_sink = GatewayEventEmitter::new(move |event| {
        let context =
            event_state.pending_context_for_selector(&event_selector, Some(&event_thread_id));
        event_state.publish_gateway_event_for_connection(event, context, None, Some(&out_tx));
    });
    let native_compaction: Option<
        BoxFuture<'static, psychevo::Result<psychevo::compaction::CompactionResult>>,
    > = (!non_native_runtime)
        .then(|| Box::pin(async move { thread.compact(request).await }) as BoxFuture<'static, _>);
    let state = state.clone();
    Ok(Box::pin(async move {
        let mut started_activity = state.activity(&source, Some(&thread_id)).await;
        started_activity.running = true;
        let _ = event_sink.emit(GatewayEvent::ActivityChanged {
            thread_id: Some(thread_id.clone()),
            activity: gateway_activity_view(&started_activity),
        });
        let result = if non_native_runtime {
            Ok(unavailable_compaction_result(
                &thread_id,
                psychevo::compaction::CompactionReason::Manual,
                &runtime_ref,
            ))
        } else {
            native_compaction.expect("native compaction future").await
        };
        let completed_activity = state.activity(&source, Some(&thread_id)).await;
        let _ = event_sink.emit(GatewayEvent::ActivityChanged {
            thread_id: Some(thread_id.clone()),
            activity: gateway_activity_view(&completed_activity),
        });
        let response = match result {
            Ok(result) => thread_compact_result(&state, result).await?,
            Err(err) => wire::thread_command_turn::ThreadCompactionResult {
                accepted: false,
                thread_id: Some(thread_id),
                compacted: false,
                reason: "error".to_string(),
                message: err.to_string(),
                checkpoint: None,
                tokens_before: None,
                tokens_after: None,
                summary_provider: None,
                summary_model: None,
                unavailable: false,
                error: Some(err.to_string()),
            },
        };
        Ok(response)
    }))
}

async fn thread_compact_result(
    state: &WebState,
    result: psychevo::compaction::CompactionResult,
) -> psychevo::Result<wire::thread_command_turn::ThreadCompactionResult> {
    let checkpoint = match result.checkpoint_id {
        Some(checkpoint_id) => {
            let thread = state
                .inner
                .framework
                .resume_thread(&result.session_id)
                .await?;
            thread.compaction(checkpoint_id).await?.map(|checkpoint| {
                wire::thread_command_turn::ThreadCompactionCheckpointView {
                    checkpoint_id: checkpoint.checkpoint_id,
                    reason: checkpoint.reason,
                    created_at_ms: checkpoint.created_at_ms,
                    first_kept_session_seq: checkpoint.first_kept_session_seq,
                    tokens_before: checkpoint.tokens_before,
                    tokens_after: checkpoint.tokens_after,
                    summary_provider: Some(checkpoint.summary_provider),
                    summary_model: Some(checkpoint.summary_model),
                    summary: Some(checkpoint.summary),
                }
            })
        }
        None => None,
    };
    let unavailable = result.message.to_ascii_lowercase().contains("unavailable");
    Ok(wire::thread_command_turn::ThreadCompactionResult {
        accepted: true,
        thread_id: Some(result.session_id),
        compacted: result.compacted,
        reason: result.reason,
        message: result.message,
        checkpoint,
        tokens_before: result.tokens_before,
        tokens_after: result.tokens_after,
        summary_provider: result.summary_provider,
        summary_model: result.summary_model,
        unavailable,
        error: None,
    })
}
