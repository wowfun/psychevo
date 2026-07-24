pub(crate) use std::collections::BTreeMap;
pub(crate) use std::sync::{Arc, Mutex};
pub(crate) use std::time::{Duration, Instant};

pub(crate) use futures::StreamExt;
pub(crate) use psychevo_agent_core::{
    AgentLoopRequest, AssistantBlock, ControlHandle, Message, NoopEventSink, PromptInstruction,
    run_agent_loop, user_text_message,
};
pub(crate) use psychevo_ai::{
    AbortSignal, GenerationProvider, GenerationRequest, ModelTarget, OpenAiChatProvider,
    OpenAiResponsesProvider, Outcome, StreamEvent,
};
pub(crate) use serde_json::{Value, json};
pub(crate) use tokio::time;

pub(crate) use crate::agents::{
    AgentDefinition, AgentDiscoveryOptions, AgentToolContext, agent_catalog_for_prompt,
    agent_catalog_for_selected_policy, agent_mailbox_event_message,
    agent_policy_allows_agent_spawn, agent_project_instructions_enabled, apply_agent_tool_policy,
    apply_runtime_hooks, discover_agents, effective_tool_names, main_agent_metadata,
    narrow_permission_mode_for_agent, resolve_agent_definition, resolve_agents_home,
    session_agent_input_from_metadata, skill_catalog_visible_for_tools,
    spawn_child_agent_background,
};
pub(crate) use crate::compaction::{
    CompactSessionOptions, CompactionReason, compact_session, is_context_overflow_error,
    load_projected_messages,
};
pub(crate) use crate::config::{
    ResolvedRunProvider, load_plugin_policy_config_lenient, load_project_context_instruction_mode,
    load_run_config, resolve_run_provider,
};
pub(crate) use crate::context_usage::{
    ContextRecorder, ContextRecordingProvider, LiveContextProfile, context_counting_metadata,
};
pub(crate) use crate::error::{Error, Result};
pub(crate) use crate::events::PersistenceSink;
pub(crate) use crate::managed_tools::ensure_rg;
pub(crate) use crate::messages::assistant_text;
pub(crate) use crate::paths::canonical_cwd;
pub(crate) use crate::permissions::PermissionRuntime;
pub(crate) use crate::project_instructions::load_project_instructions;
pub(crate) use crate::prompt_assembly::{
    MainPromptPrefixInput, PROMPT_PREFIX_NOTICE_METADATA_KEY, PromptPrefixRecordInput,
    RuntimeTimeContext, assemble_main_prompt_prefix, assembly_from_prefix_record,
    context_evidence_for_request, developer_provider_role, prompt_prefix_record,
    skill_contextual_user_messages, tool_declarations_hash, tool_declarations_hash_with_search,
    turn_prefix_notice_instruction, turn_required_agent_instruction, turn_runtime_time_instruction,
};
pub(crate) use crate::prompt_image::prompt_message_from_inputs_with_options;
pub(crate) use crate::prompt_templates;
pub(crate) use crate::session_trace::SessionTraceSink;
pub(crate) use crate::skills::{
    SelectedSkill, SkillCatalog, SkillDiscoveryOptions, discover_skills, resolve_skills_home,
    select_explicit_skills, select_skills_for_prompt, skill_context_fragments,
    skills_visible_for_prompt_with_tools_and_toolsets,
};
pub(crate) use crate::snapshot::SnapshotStore;
pub(crate) use crate::store::{PromptPrefixRecord, StateRuntime};
pub(crate) use crate::tool_surface::{
    ClarifyToolSurface, ToolSurfaceAssembly, assemble_tool_surface_with_warnings,
};
pub(crate) use crate::tools::{detach_exec_sessions_for_task, interrupt_exec_sessions_for_task};
pub(crate) use crate::types::{
    AgentSpawnOptions, AgentSpawnResult, ApprovalHandler, ModelMetadata,
    PermissionApprovalDecision, PermissionApprovalRequest, PermissionConfig, ReloadContextOptions,
    ReloadContextResult, RunControl, RunOptions, RunResult, RunStreamEvent, RunStreamSink,
    RunWarning, RuntimeTool, SelectedAgent, SmokeControl,
};

#[allow(unused_imports)]
use super::*;

#[path = "run/entrypoints.rs"]
mod entrypoints;
pub(crate) use entrypoints::{
    DEFAULT_AGENT_MAX_TURNS, SESSION_TITLE_MAX_CHARS, TITLE_GENERATION_TIMEOUT_SECS,
};
pub use entrypoints::{
    reload_session_context, run_live, run_live_streaming, run_live_streaming_controlled,
    spawn_agent_background,
};
#[path = "run/execution.rs"]
mod execution;
#[allow(unused_imports)]
pub(crate) use execution::{
    main_agent_input_from_sources, materialize_first_use_empty_session,
    maybe_preflight_compact_session, run_live_internal, selected_agent_for_result,
    selected_skills_for_run, session_model_metadata, should_title_completed_session,
};
#[path = "run/titles.rs"]
mod titles;
pub use titles::fallback_visible_session_title;
#[allow(unused_imports)]
pub(crate) use titles::{
    called_agent_names, clean_generated_session_title, emit_warning_events,
    ensure_new_visible_session_title, fallback_session_title, generate_session_title,
    normalize_session_title, prompt_without_selected_skill_markers, remove_think_blocks,
    selected_skill_title_lines, selected_skills_fallback_title, session_title_request,
    strip_wrapping_title_quotes, truncate_chars, visible_session_source_allows_auto_title,
    warning_event,
};

pub(crate) fn generation_provider(
    base_url: impl Into<String>,
    api_key: impl Into<String>,
    provider: impl Into<String>,
    inference_idle_timeout_secs: u64,
) -> Arc<dyn GenerationProvider> {
    let base_url = base_url.into();
    let api_key = api_key.into();
    let provider = provider.into();
    if crate::config::normalize_provider_id(&provider) == "openai" {
        Arc::new(
            OpenAiResponsesProvider::new(base_url, api_key)
                .with_inference_idle_timeout_secs(inference_idle_timeout_secs),
        )
    } else {
        Arc::new(
            OpenAiChatProvider::new(base_url, api_key, provider)
                .with_inference_idle_timeout_secs(inference_idle_timeout_secs),
        )
    }
}
