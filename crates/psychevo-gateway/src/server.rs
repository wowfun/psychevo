use std::collections::{BTreeMap, HashMap};
use std::io::{Error as IoError, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::Body;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::header::{
    AUTHORIZATION, CONTENT_DISPOSITION, CONTENT_TYPE, COOKIE, LOCATION, SET_COOKIE,
};
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode};
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
#[cfg(test)]
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use futures::{SinkExt, StreamExt};
use psychevo_gateway_protocol as wire;
use psychevo_runtime::command_registry::{
    AvailableSlashCommand, CommandArgumentKind, CommandCapability, CommandPresentation,
    DynamicSlashCommand, SlashCommandAction, SlashCommandEffect, SlashCommandParse,
    SlashCommandSurface, available_slash_commands_for_surface, command_presentation,
    dynamic_slash_command_effect, parse_slash_command_line, skill_prompt_marker,
    slash_invocation_effect,
};
use psychevo_runtime::{
    AgentBackendConfig, AgentCatalog, AgentDefinition, AgentDiagnostic, AgentDiscoveryOptions,
    AgentEntrypoint, AgentRunRecord, ChildSessionSnapshotInput, ClarifyAnswer, ClarifyResponse,
    ClarifyResult, ConfigScope, ContextOptions, Error, LoadedMainAgent, MAX_AGENT_SPAWN_DEPTH_CAP,
    Message as RuntimeMessage, PermissionApprovalDecision, PermissionApprovalOutcome,
    PermissionMode, RunMode, RunOptions, SESSION_MAIN_AGENT_METADATA_KEY,
    SIDE_CONVERSATION_METADATA_KEY, SIDE_CONVERSATION_SESSION_SOURCES, SIDE_INHERITED_METADATA_KEY,
    SessionArtifactKind, SessionExportFormat, SessionExportIncludeSet, SessionExportOptions,
    SessionSummary, SessionTraceReadOptions, SessionUndoOptions, SessionUsageOptions,
    SkillDiscoveryOptions, StateRuntime, UsageReadOptions, UserContentBlock,
    UserShellContextOptions, WEB_SIDE_CONVERSATION_SESSION_SOURCE, agent_spawn_paused,
    agent_status_records, canonicalize_workdir, configured_models, context_snapshot,
    discover_agents, discover_skills, format_context_total_value, format_context_total_value_parts,
    list_skill_bundles, load_agent_backend_configs, main_agent_default_metadata,
    main_agent_from_session_metadata, main_agent_metadata, redo_session, remove_config_value,
    render_session_export, resolve_agent_definition, selected_configured_model,
    session_usage_summary, set_channel_enabled, set_config_value,
    side_conversation_boundary_prompt, undo_session, usage_read, valid_agent_name,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    ACP_PEER_METADATA_KEY, BackendKind, Gateway, GatewayActivity, GatewayBackendInfo, GatewayEvent,
    GatewayEventSink, GatewayInputPart, GatewayShellResult, GatewaySource, GatewaySourceLifetime,
    GatewayThread, GatewayThreadSelector, GatewayTurnResult, PermissionDecision, SendShellRequest,
    SourceKey, TranscriptBlock, TranscriptBlockKind, TranscriptBlockStatus, TranscriptEntry,
    TranscriptEntryRole, gateway_now_ms,
};
#[cfg(test)]
use crate::{GatewayTurn, GatewayTurnError, GatewayTurnStatus};

mod agents;
mod channel_runtime;
mod channels;
mod commands;
mod completion;
mod terminal;
mod workspace;

use agents::{
    agent_list_result, agent_read_result, agent_status_result, backend_doctor_value,
    backend_values_for_scope, delete_backend_config, delete_project_agent_definition,
    write_backend_config, write_project_agent_definition,
};
use channels::{
    channel_delete_result, channel_doctor_result_live, channel_enable_result,
    channel_list_result_for_scope, channel_list_result_for_workdir, channel_show_result,
    channel_source_list_result, channel_update_result, channel_wechat_qr_poll_result,
    channel_wechat_qr_start_result,
};
use commands::{
    command_completion_detail, command_execute_value, command_list_value, compact_prompt_text,
    gateway_command_capabilities, web_desktop_command_visible,
};
#[cfg(test)]
use completion::active_completion_token;
use completion::completion_list_value;
use terminal::TerminalManager;
#[cfg(test)]
use workspace::workspace_dir_name;
use workspace::{
    WorkspaceReviewState, workspace_create_value, workspace_diff_result, workspace_diff_value,
    workspace_file_read_value, workspace_file_write_value, workspace_files_value,
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
    include!("server/tests/terminal_launch.rs");
    include!("server/tests/helpers.rs");
}
