use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Error as IoError, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{AgentErrorStage, agent_error_view, agent_session_error};
use axum::Router;
use axum::body::Body;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::header::{
    AUTHORIZATION, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE, COOKIE, LOCATION, SET_COOKIE,
};
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode};
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
#[cfg(test)]
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use futures::{SinkExt, StreamExt};
use psychevo_agent_core::{Message as RuntimeMessage, UserContentBlock};
use psychevo_gateway_protocol as wire;
use psychevo_runtime::command_registry::{
    AvailableSlashCommand, CommandArgumentKind, CommandCapability, CommandPresentation,
    DynamicSlashCommand, SharedSlashAlias, SharedSlashConfig, SharedSlashKeybind,
    SlashCommandAction, SlashCommandEffect, SlashCommandParse, SlashCommandSurface,
    available_slash_commands_for_surface, command_presentation, dynamic_slash_command_effect,
    parse_key_chord_display, parse_key_sequence_display, parse_session_export_command_args,
    parse_session_export_format, parse_shared_slash_config, parse_slash_command_line,
    skill_prompt_marker, slash_invocation_effect, split_key_sequence_list,
    split_slash_command_token, validate_configured_alias, validate_configured_slash_target,
    validate_shared_slash_config,
};
use psychevo_runtime::state::{
    AgentMissionRunInput, AgentTeamRunInput, AutomationRunFinishInput, AutomationRunRecord,
    AutomationTaskInput, AutomationTaskRecord, ChildSessionSnapshotInput, ConversationDraftPart,
    GatewayRuntimeBindingOwnership, GatewayRuntimeBindingRecord, GatewayRuntimeBindingStatus,
    GatewayRuntimeControlStatePatch, GatewaySourceLaneInput, SessionListProjection,
    SessionRevertKind, StateRuntime,
};
use psychevo_runtime::{
    Error, agents::AgentBackendConfig, agents::AgentCatalog, agents::AgentDefinition,
    agents::AgentDiagnostic, agents::AgentDiscoveryOptions, agents::AgentEntrypoint,
    agents::AgentRunRecord, agents::AgentSource, agents::AgentTeamCatalog,
    agents::AgentTeamDefinition, agents::AgentTeamMember, agents::AgentTeamSource,
    agents::LoadedMainAgent, agents::MAX_AGENT_SPAWN_DEPTH_CAP,
    agents::MAX_TEAM_PARALLEL_AGENTS_CAP, agents::SESSION_MAIN_AGENT_METADATA_KEY,
    agents::agent_spawn_paused, agents::agent_status_records,
    agents::discover_agent_teams_with_catalog, agents::discover_agents,
    agents::main_agent_default_metadata, agents::main_agent_from_session_metadata,
    agents::main_agent_metadata, agents::parse_agent_definition_text,
    agents::parse_agent_team_definition_text, agents::resolve_agent_definition,
    agents::resolve_agent_team_definition, agents::resume_agent_id, agents::send_agent_message,
    agents::set_agent_spawn_paused, agents::stop_agent_id_with_grace, agents::valid_agent_name,
    automations::AutomationSchedule, automations::latest_due_at_ms, automations::next_run_at_ms,
    config::McpServerConfigInput, config::McpToolPolicyInput, config::REASONING_EFFORT_VALUES,
    config::RuntimeProfileConfig, config::RuntimeProfileKind, config::auth_status_value,
    config::clear_mcp_oauth_access_token, config::config_show_value, config::configured_models,
    config::create_local_toolset, config::fetch_and_cache_model_catalog,
    config::generated_runtime_profile_id_for_backend, config::image_generation_config_value,
    config::load_agent_backend_configs, config::load_runtime_profile_configs,
    config::mcp_server_value, config::mcp_servers_value, config::model_catalog_entry_is_free,
    config::model_catalog_provider, config::model_catalog_providers, config::normalize_provider_id,
    config::read_cached_model_catalog, config::remove_config_value, config::remove_local_toolset,
    config::remove_mcp_server, config::resolve_voice_asr_config,
    config::resolve_voice_realtime_config, config::resolve_voice_tts_config,
    config::save_mcp_oauth_access_token, config::selected_configured_model,
    config::set_auxiliary_model_with_reasoning, config::set_channel_enabled,
    config::set_config_value, config::set_default_model_with_reasoning,
    config::set_local_toolset_enabled, config::set_mcp_server_enabled,
    config::set_mcp_server_tool_policy, config::set_provider_api_key,
    config::set_provider_model_config, config::toolsets_value, config::upsert_mcp_server,
    config::voice_config_value, context_usage::ContextOptions, context_usage::context_snapshot,
    context_usage::format_context_total_value, context_usage::format_context_total_value_parts,
    host_paths::ExecutableResolveOptions, host_paths::HostPlatform,
    host_paths::normalized_native_path, host_paths::resolve_executable_path,
    mcp::mcp_test_server_value, model_state::ModelState,
    model_state::SESSION_COMPOSER_MODEL_METADATA_KEY, model_state::normalize_reasoning_effort,
    paths::canonicalize_cwd, plugins::PluginInspectOptions, plugins::PluginInstallOptions,
    plugins::PluginMarketplaceEntry, plugins::PluginScope, plugins::PluginSourceKind,
    plugins::codex_plugin_set_enabled_value, plugins::plugin_doctor_value,
    plugins::plugin_import_inspect_value, plugins::plugin_install_value,
    plugins::plugin_list_value, plugins::plugin_marketplace_add_value,
    plugins::plugin_marketplace_list_value, plugins::plugin_marketplace_remove_value,
    plugins::plugin_reset_enabled_value, plugins::plugin_set_enabled_value,
    plugins::plugin_uninstall_value, plugins::plugin_view_value,
    prompt_templates::side_conversation_boundary_prompt, session_export::SessionArtifactKind,
    session_export::SessionExportFormat, session_export::SessionExportIncludeSet,
    session_export::SessionExportOptions, session_export::render_session_export,
    session_trace::SessionTraceReadOptions, skills::InstallOptions, skills::ListSkillsOptions,
    skills::SkillDiscoveryOptions, skills::SkillTarget, skills::discover_skills,
    skills::install_skill, skills::list_skill_bundles, skills::list_skills_value_with_options,
    skills::remove_installed_skill, skills::set_skill_enabled, skills::view_skill_value_selected,
    skills::write_installed_skill, stats::session_usage_summary, stats::usage_read,
    thread_lineage::SIDE_CONVERSATION_METADATA_KEY, thread_lineage::SIDE_INHERITED_METADATA_KEY,
    thread_lineage::WEB_SIDE_CONVERSATION_SESSION_SOURCE,
    thread_lineage::side_conversation_session_source, types::ClarifyAnswer, types::ClarifyResponse,
    types::ClarifyResult, types::ConfigScope, types::ModelCatalogEntry,
    types::ModelCatalogProvider, types::PermissionApprovalDecision, types::PermissionMode,
    types::RunMode, types::RunOptions, types::RunSandboxOverride, types::SessionUndoOptions,
    types::SessionUsageOptions, types::UsageReadOptions, types::UserShellContextOptions,
    types::WorkspaceMutationSink, undo::redo_session, undo::undo_session,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    ACP_PEER_METADATA_KEY, BackendKind, Gateway, GatewayActionKind, GatewayActionOutcome,
    GatewayActivity, GatewayBackendInfo, GatewayEvent, GatewayEventSink, GatewayInputPart,
    GatewayProfileFields, GatewayShellResult, GatewaySource, GatewaySourceLifetime, GatewayThread,
    GatewayThreadSelector, GatewayTurnResult, PendingActionView, PermissionDecision,
    SendCompactRequest, SendShellRequest, SourceKey, TranscriptBlock, TranscriptBlockKind,
    TranscriptBlockStatus, TranscriptEntry, TranscriptEntryRole, gateway_now_ms,
    gateway_profile_mark, transcript,
};
#[cfg(test)]
use crate::{GatewayTurn, GatewayTurnError, GatewayTurnStatus};

mod agents;
mod automations;
mod channel_runtime;
mod channels;
mod codex_capability_broker;
mod commands;
mod completion;
mod event_delivery;
mod runtime_profiles;
mod session_application;
mod session_import_application;
mod terminal;
mod thread_application;
mod voice;
mod workspace;
mod workspace_external;
mod workspace_preview;

use agents::{
    active_profile_config_dir, agent_control_result, agent_list_result, agent_read_result,
    agent_status_result, backend_values_for_scope, delete_agent_definition, delete_backend_config,
    delete_team_definition, discover_gateway_teams, manage_backend_value,
    managed_backend_doctor_value_with_auth, materialize_local_acp_backends, read_agent_definition,
    read_team_definition, set_agent_definition_enabled, set_team_definition_enabled,
    team_list_result, team_read_result, team_status_result, write_agent_definition,
    write_backend_config, write_team_definition,
};
use automations::{
    automation_delete_result, automation_draft_result, automation_list_result,
    automation_run_result, automation_set_enabled_result, automation_write_result,
};
use channels::{
    channel_delete_result, channel_doctor_result_live, channel_enable_result,
    channel_list_result_for_cwd, channel_list_result_for_scope, channel_show_result,
    channel_source_list_result, channel_update_result, channel_wechat_qr_poll_result,
    channel_wechat_qr_start_result,
};
use commands::{
    command_execute_value, command_item_completion_detail, command_item_matches,
    command_list_result, command_list_value, slash_settings_read_value,
    slash_settings_update_value,
};
#[cfg(test)]
use completion::active_completion_token;
use completion::completion_list_value;
use event_delivery::{ConnectionSender, GatewayEventHub, OutboxReceive, connection_outbox};
use runtime_profiles::{
    RunnableTargetCatalog, ThreadDraftPrepareWork, apply_thread_control_precedence,
    cached_thread_history_descriptor, delete_runtime_profile,
    ensure_turn_runtime_profile_supported, prepare_draft_source_lane, runnable_target_for_source,
    runnable_target_for_source_profile, runnable_target_input, runtime_backend_kind,
    runtime_profile_list_result, runtime_profile_read_result, selected_context_target_id,
    set_runtime_profile_enabled, thread_context_read_result_for_target_id,
    thread_context_read_result_live, thread_context_read_result_live_with_catalog_and_configured,
    thread_control_override_string_value, thread_control_set_result, thread_draft_prepare_result,
    thread_draft_prepare_result_with_work, validate_and_capture_team_runtime_members,
    validate_turn_runnable_target, write_runtime_profile,
};
#[cfg(test)]
use runtime_profiles::{
    acp_session_mode_control_descriptor, combined_thread_revision, generated_runtime_profiles,
};
use session_import_application::{
    fork_acp_thread, fork_native_thread, reconcile_acknowledged_session_deletes,
    typed_thread_snapshot,
};
use terminal::TerminalManager;
use thread_application::{
    RoutedThreadTurn, action_descriptors as thread_action_descriptors,
    authoritative_history_projection, authoritative_history_view,
    pending_interactions as thread_pending_interactions, prewarm_codex_runtime_inventory,
    respond_to_routed_interaction_for_selector as thread_routed_interaction_respond_for_selector,
    run_routed_action as run_routed_thread_action, run_routed_turn as run_routed_thread_turn,
    source_draft_control_values,
};
use voice::{
    RealtimeSessionState, update_voice_policy_for_source, voice_asr_transcribe_value,
    voice_policy_for_source, voice_policy_read_value, voice_policy_update_value,
    voice_tts_synthesize_value,
};
#[cfg(test)]
use workspace::workspace_dir_name;
use workspace::{
    WorkspaceReviewState, workspace_create_value, workspace_diff_result, workspace_diff_value,
    workspace_file_read_value, workspace_file_write_value, workspace_files_value,
    workspace_folder_list_value, workspace_git_branches_value, workspace_git_checkout_value,
};
use workspace_external::{
    WorkspaceExternalState, workspace_file_external_actions_value,
    workspace_file_open_external_value,
};
use workspace_preview::{
    WorkspacePreviewLeaseStore, configured_workspace_preview_origins,
    workspace_file_preview_open_value, workspace_file_preview_release_value,
    workspace_preview_resource,
};

include!("server/binding.rs");
include!("server/rpc_dispatch.rs");
include!("server/scope_session.rs");
include!("server/settings_observability.rs");
include!("server/auth_input.rs");
include!("server/download_static.rs");
include!("server/session_view.rs");
include!("server/rpc_json.rs");

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use std::collections::BTreeMap;
    use std::ffi::OsStr;

    include!("server/tests/observability.rs");
    include!("server/tests/session_browser.rs");
    include!("server/tests/agents_settings.rs");
    include!("server/tests/workspace_commands.rs");
    include!("server/tests/workspace_preview.rs");
    include!("server/tests/automations.rs");
    include!("server/tests/voice_rpc.rs");
    include!("server/tests/terminal_launch.rs");
    include!("server/tests/session_lifecycle.rs");
    include!("server/tests/history_editing.rs");
    include!("server/tests/managed_lifecycle.rs");
    include!("server/tests/first_token_performance.rs");
    include!("server/tests/draft_open.rs");
    include!("server/tests/helpers.rs");
}
