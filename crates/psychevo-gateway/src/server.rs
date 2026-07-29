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
use futures::{SinkExt, StreamExt, future::BoxFuture};
use psychevo::__agent_core::{Message as RuntimeMessage, UserContentBlock};
use psychevo::__product::commands::{
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
use psychevo::__product::persistence::{
    AgentMissionRunInput, AgentTeamRunInput, AutomationRunFinishInput, AutomationRunRecord,
    AutomationTaskInput, AutomationTaskRecord, ChildSessionSnapshotInput, ConversationDraftPart,
    GatewayRuntimeBindingOwnership, GatewayRuntimeBindingRecord, GatewayRuntimeBindingStatus,
    GatewayRuntimeControlStatePatch, GatewaySourceLaneInput, SessionListProjection,
    SessionRevertKind, StateRuntime,
};
use psychevo::{
    __product::capabilities::AgentBackendConfig, __product::capabilities::AgentCatalog,
    __product::capabilities::AgentControl, __product::capabilities::AgentDefinition,
    __product::capabilities::AgentDiagnostic, __product::capabilities::AgentDiscoveryOptions,
    __product::capabilities::AgentEntrypoint, __product::capabilities::AgentRunRecord,
    __product::capabilities::AgentSource, __product::capabilities::AgentTeamCatalog,
    __product::capabilities::AgentTeamDefinition, __product::capabilities::AgentTeamMember,
    __product::capabilities::AgentTeamSource, __product::capabilities::InstallOptions,
    __product::capabilities::ListSkillsOptions, __product::capabilities::LoadedMainAgent,
    __product::capabilities::MAX_AGENT_SPAWN_DEPTH_CAP,
    __product::capabilities::MAX_TEAM_PARALLEL_AGENTS_CAP,
    __product::capabilities::PluginInspectOptions, __product::capabilities::PluginInstallOptions,
    __product::capabilities::PluginMarketplaceEntry, __product::capabilities::PluginScope,
    __product::capabilities::PluginSourceKind,
    __product::capabilities::SESSION_MAIN_AGENT_METADATA_KEY,
    __product::capabilities::SkillDiscoveryOptions, __product::capabilities::SkillTarget,
    __product::capabilities::codex_plugin_set_enabled_value,
    __product::capabilities::discover_agent_teams_with_catalog,
    __product::capabilities::discover_agents, __product::capabilities::discover_skills,
    __product::capabilities::install_skill, __product::capabilities::list_skill_bundles,
    __product::capabilities::list_skills_value_with_options,
    __product::capabilities::main_agent_default_metadata,
    __product::capabilities::main_agent_from_session_metadata,
    __product::capabilities::main_agent_metadata,
    __product::capabilities::parse_agent_definition_text,
    __product::capabilities::parse_agent_team_definition_text,
    __product::capabilities::plugin_doctor_value,
    __product::capabilities::plugin_import_inspect_value,
    __product::capabilities::plugin_install_value, __product::capabilities::plugin_list_value,
    __product::capabilities::plugin_marketplace_add_value,
    __product::capabilities::plugin_marketplace_list_value,
    __product::capabilities::plugin_marketplace_remove_value,
    __product::capabilities::plugin_reset_enabled_value,
    __product::capabilities::plugin_set_enabled_value,
    __product::capabilities::plugin_uninstall_value, __product::capabilities::plugin_view_value,
    __product::capabilities::remove_installed_skill,
    __product::capabilities::resolve_agent_definition,
    __product::capabilities::resolve_agent_team_definition,
    __product::capabilities::set_skill_enabled, __product::capabilities::valid_agent_name,
    __product::capabilities::view_skill_value_selected,
    __product::capabilities::write_installed_skill, __product::configuration::McpServerConfigInput,
    __product::configuration::McpToolPolicyInput,
    __product::configuration::REASONING_EFFORT_VALUES,
    __product::configuration::RuntimeProfileConfig, __product::configuration::RuntimeProfileKind,
    __product::configuration::auth_status_value,
    __product::configuration::clear_mcp_oauth_access_token,
    __product::configuration::config_show_value, __product::configuration::configured_models,
    __product::configuration::create_local_toolset,
    __product::configuration::fetch_and_cache_model_catalog,
    __product::configuration::generated_runtime_profile_id_for_backend,
    __product::configuration::image_generation_config_value,
    __product::configuration::load_agent_backend_configs,
    __product::configuration::load_runtime_profile_configs,
    __product::configuration::mcp_server_value, __product::configuration::mcp_servers_value,
    __product::configuration::model_catalog_entry_is_free,
    __product::configuration::model_catalog_provider,
    __product::configuration::model_catalog_providers,
    __product::configuration::normalize_provider_id,
    __product::configuration::read_cached_model_catalog,
    __product::configuration::remove_config_value, __product::configuration::remove_local_toolset,
    __product::configuration::remove_mcp_server,
    __product::configuration::resolve_voice_asr_config,
    __product::configuration::resolve_voice_realtime_config,
    __product::configuration::resolve_voice_tts_config,
    __product::configuration::save_mcp_oauth_access_token,
    __product::configuration::selected_configured_model,
    __product::configuration::set_auxiliary_model_with_reasoning,
    __product::configuration::set_channel_enabled, __product::configuration::set_config_value,
    __product::configuration::set_default_model_with_reasoning,
    __product::configuration::set_local_toolset_enabled,
    __product::configuration::set_mcp_server_enabled,
    __product::configuration::set_mcp_server_tool_policy,
    __product::configuration::set_provider_api_key,
    __product::configuration::set_provider_model_config, __product::configuration::toolsets_value,
    __product::configuration::upsert_mcp_server, __product::configuration::voice_config_value,
    __product::integrations::mcp_test_server_value, __product::platform::ExecutableResolveOptions,
    __product::platform::HostPlatform, __product::platform::canonicalize_cwd,
    __product::platform::normalized_native_path, __product::platform::resolve_executable_path,
    __product::presentation::side_conversation_boundary_prompt, __product::runtime::ClarifyAnswer,
    __product::runtime::ClarifyResponse, __product::runtime::ClarifyResult,
    __product::runtime::ConfigScope, __product::runtime::ModelCatalogEntry,
    __product::runtime::ModelCatalogProvider, __product::runtime::ModelState,
    __product::runtime::PermissionApprovalDecision, __product::runtime::PermissionMode,
    __product::runtime::RunMode, __product::runtime::RunOptions,
    __product::runtime::RunSandboxOverride,
    __product::runtime::SESSION_COMPOSER_MODEL_METADATA_KEY,
    __product::runtime::SessionUndoOptions, __product::runtime::SessionUsageOptions,
    __product::runtime::UsageReadOptions, __product::runtime::UserShellContextOptions,
    __product::runtime::WorkspaceMutationSink, __product::runtime::normalize_reasoning_effort,
    __product::sessions::AutomationSchedule, __product::sessions::SIDE_CONVERSATION_METADATA_KEY,
    __product::sessions::SIDE_INHERITED_METADATA_KEY, __product::sessions::SessionArtifactKind,
    __product::sessions::SessionExportFormat, __product::sessions::SessionExportIncludeSet,
    __product::sessions::SessionExportOptions, __product::sessions::SessionTraceReadOptions,
    __product::sessions::WEB_SIDE_CONVERSATION_SESSION_SOURCE,
    __product::sessions::latest_due_at_ms, __product::sessions::next_run_at_ms,
    __product::sessions::redo_session, __product::sessions::render_session_export,
    __product::sessions::side_conversation_session_source, __product::sessions::undo_session,
    __product::usage::ContextOptions, __product::usage::context_snapshot,
    __product::usage::format_context_total_value,
    __product::usage::format_context_total_value_parts, __product::usage::session_usage_summary,
    __product::usage::usage_read, Application, Client as FrameworkClient, Error,
};
use psychevo_gateway_protocol as wire;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    ACP_PEER_METADATA_KEY, BackendKind, Gateway, GatewayActionKind, GatewayActionOutcome,
    GatewayActivity, GatewayAgentSessionAdapter, GatewayBackendInfo, GatewayEvent,
    GatewayEventEmitter, GatewayInputPart, GatewayProfileFields, GatewayShellResult, GatewaySource,
    GatewaySourceLifetime, GatewayThread, GatewayThreadSelector, GatewayTurnResult,
    PendingActionView, PermissionDecision, SendShellRequest, SourceKey, TranscriptBlock,
    TranscriptBlockKind, TranscriptBlockStatus, TranscriptEntry, TranscriptEntryRole,
    gateway_activity_view, gateway_now_ms, gateway_profile_mark, gateway_turn_status_for_outcome,
    transcript, unavailable_compaction_result,
};
#[cfg(test)]
use crate::{GatewayTurn, GatewayTurnError, GatewayTurnStatus};

mod agents;
mod automations;
mod browser_session_store;
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
use browser_session_store::{BrowserSessionStore, browser_session_cookie};
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
    enqueue_routed_compact_action as enqueue_routed_thread_compact_action,
    framework_gateway_turn_result, pending_interactions as thread_pending_interactions,
    prewarm_codex_runtime_inventory,
    respond_to_routed_interaction as thread_routed_interaction_respond,
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
