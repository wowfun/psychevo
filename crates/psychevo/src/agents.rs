use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, LazyLock, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use psychevo_agent_core::{
    AgentLoopRequest, AssistantBlock, ControlHandle, Message, ToolBinding, ToolDisplaySpec,
    ToolExecutionMode, ToolOutput, user_text_message,
};
use psychevo_ai::{AbortSignal, GenerationProvider, Outcome};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::compaction::{
    CompactSessionOptions, CompactionReason, compact_session, load_projected_messages,
};
use crate::config::{CustomToolsetConfig, LspConfig, ToolSelectionConfig};
use crate::context_usage::ContextRecorder;
use crate::error::{Error, Result};
use crate::events::PersistenceSink;
use crate::messages::assistant_text;
use crate::permissions::PermissionRuntime;
use crate::prompt_assembly::{
    PromptPrefixRecordInput, RuntimeTimeContext, assemble_child_prompt_prefix,
    context_evidence_for_request, prompt_prefix_record, tool_declarations_hash_with_search,
    turn_runtime_time_instruction,
};
use crate::prompt_templates;
use crate::skills::resolve_skills_home;
use crate::state::StateRuntime;
use crate::store::{
    AgentEdgeRecord, AgentEdgeStatus, AgentMailboxEventInput, AgentMailboxEventRecord,
    AgentMissionRunRecord, AgentTeamRunRecord,
};
use crate::tool_surface::{
    ClarifyToolSurface, ToolSurfaceAssembly, assemble_tool_surface_with_warnings,
};
use crate::types::{
    ApprovalHandler, ApprovalMode, ExternalAgentDelegateRequest, ModelMetadata, PermissionConfig,
    PermissionMode, ProjectContextInstructionMode, RunMode, RunStreamEvent, RunStreamSink,
    RunWarning, RuntimeTool, SelectedAgent, SessionSummary, SmokeControl,
};

#[path = "agents/catalog_surface.rs"]
mod catalog_surface;
pub use catalog_surface::{
    AgentBackendConfig, AgentBackendKind, AgentBackendRef, AgentCatalog, AgentContribution,
    AgentControl, AgentDefinition, AgentDiagnostic, AgentDiscoveryOptions, AgentEntrypoint,
    AgentInvocationRole, AgentPermissionMode, AgentRun, AgentRunRecord, AgentRunStatus,
    AgentSource, AgentToolPolicy, MAX_AGENT_SPAWN_DEPTH_CAP, agent_source_display_label,
    agent_spawn_paused, agent_status_records, agent_status_value, close_agent_id, discover_agents,
    format_agents_for_prompt, list_agents_value, resolve_agent_definition, set_agent_spawn_paused,
    stop_agent_id_with_grace, view_agent_value, view_agent_value_with_catalog, wait_agent_id,
    wait_agent_mailbox,
};
pub(crate) use catalog_surface::{
    AgentToolContext, agent_catalog_for_prompt, agent_catalog_for_selected_policy,
    agent_policy_allows_agent_spawn, agent_project_instructions_enabled, agent_tools,
    apply_agent_tool_policy, apply_hook_runtime, apply_runtime_hooks, build_hook_runtime,
    default_peer_agent_entrypoints, default_peer_client_capabilities, effective_tool_names,
    format_selected_agent_instruction, narrow_permission_mode_for_agent,
    skill_catalog_visible_for_tools,
};
#[path = "agents/main_agent.rs"]
mod main_agent;
pub use main_agent::{
    LoadedMainAgent, SESSION_MAIN_AGENT_METADATA_KEY, main_agent_default_metadata,
    main_agent_from_session_metadata, main_agent_metadata, session_agent_input_from_metadata,
    session_base_agent_name_from_metadata, session_main_agent_explicit_default,
};
#[path = "agents/lifecycle.rs"]
mod lifecycle;
pub use lifecycle::{resume_agent_id, send_agent_message};
#[path = "agents/definition_policy.rs"]
mod definition_policy;
pub use definition_policy::{parse_agent_definition_text, valid_agent_name};
#[path = "agents/child_runs.rs"]
mod child_runs;
pub(crate) use child_runs::spawn_child_agent_background;
#[path = "agents/mailbox_tools.rs"]
mod mailbox_tools;
pub(crate) use mailbox_tools::{agent_mailbox_event_message, resolve_agents_home};
#[path = "agents/teams.rs"]
mod teams;
pub(crate) use teams::active_agent_team_context_for_session;
pub use teams::{
    AgentTeamCatalog, AgentTeamDefinition, AgentTeamMember, AgentTeamSource,
    DEFAULT_TEAM_PARALLEL_AGENTS, MAX_TEAM_PARALLEL_AGENTS_CAP, discover_agent_teams,
    discover_agent_teams_with_catalog, parse_agent_team_definition_text,
    resolve_agent_team_definition,
};
#[cfg(test)]
#[path = "agents/test_support.rs"]
mod test_support;
