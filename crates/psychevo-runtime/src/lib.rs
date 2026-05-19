mod accounting;
mod agents;
mod config;
mod context;
mod context_usage;
mod error;
mod events;
mod messages;
mod paths;
mod permissions;
mod project_instructions;
mod prompt_assembly;
mod prompt_image;
mod run;
mod session_export;
mod session_lookup;
mod skills;
mod smoke;
mod snapshot;
mod stats;
mod store;
mod tools;
mod types;
mod undo;
mod user_shell;

#[cfg(test)]
mod tests;

pub use agents::{
    AgentCatalog, AgentControl, AgentDefinition, AgentDiagnostic, AgentDiscoveryOptions,
    AgentInvocationRole, AgentPermissionMode, AgentRun, AgentRunRecord, AgentRunStatus,
    AgentSource, AgentToolPolicy, MAX_AGENT_SPAWN_DEPTH_CAP, agent_spawn_paused,
    agent_status_value, close_agent_id, discover_agents, list_agents_value,
    resolve_agent_definition, resume_agent_id, send_agent_message, set_agent_spawn_paused,
    stop_agent_id_with_grace, view_agent_value, view_agent_value_with_catalog, wait_agent_id,
    wait_agent_mailbox,
};
pub use config::{
    append_local_permission_allow_rule, append_local_permission_rule, auth_status_value,
    config_provider_list_value, config_show_value, configured_models,
    create_global_custom_provider, create_scoped_custom_provider, custom_provider_api_key_env,
    fetch_model_catalog, model_catalog_endpoint, model_catalog_providers, permission_rules_value,
    refresh_model_metadata_cache, remove_local_permission_rule, selected_configured_model,
    set_provider_api_key,
};
pub use context::prune_context;
pub use context_usage::{
    CONTEXT_BAR_MAX_CELLS, CONTEXT_BAR_MIN_CELLS, ContextAdvice, ContextCategory,
    ContextFormatOptions, ContextOptions, ContextScope, ContextSnapshot, ContextTokenizer,
    ContextTotal, context_snapshot, format_context_snapshot_text,
    format_context_snapshot_text_with_options, format_context_total_value,
    format_context_total_value_parts, normalize_context_bar_width,
};
pub use error::{Error, Result};
pub use paths::canonicalize_workdir;
pub use prompt_image::{
    extract_image_sources_from_prompt, model_metadata_explicitly_disallows_image_input,
    prompt_starts_with_supported_image_path, resolve_image_source, split_image_source_argument,
};
pub use psychevo_agent_core::TerminalReason;
pub use run::{
    reload_session_context, run_live, run_live_streaming, run_live_streaming_controlled,
    spawn_agent_background,
};
pub use session_export::{
    SessionArtifactKind, SessionExportArtifact, SessionExportFormat, SessionExportInclude,
    SessionExportIncludeSet, SessionExportOptions, SessionExportWriteResult,
    default_session_export_filename, render_session_export, write_session_export,
};
pub use session_lookup::{latest_run_session_for_workdir, session_exists};
pub use skills::{
    InstallOptions, ScanResult, ScanVerdict, SelectedSkill, SkillCatalog, SkillDiagnostic,
    SkillDiscoveryOptions, SkillSettings, SkillSource, SkillTarget, create_skill, discover_skills,
    discover_skills_with_settings, expand_skill_prompt, install_skill, list_skills_value,
    load_skill_settings, patch_skill, remove_skill, resolve_skills_home, scan_skill_path,
    select_explicit_skills, select_skills_for_prompt, set_skill_enabled, skill_context_messages,
    target_skills_dir, view_skill_value,
};
pub use smoke::run_smoke;
pub use stats::usage_stats;
pub use store::{AgentEdgeRecord, AgentEdgeStatus};
pub use store::{ContextEvidenceInput, ContextEvidenceRecord, SqliteStore};
pub use tools::tool_names_for_mode;
pub use types::{
    AgentSpawnOptions, AgentSpawnResult, ApprovalHandler, ApprovalMode, ConfigScope,
    ConfiguredModel, CustomProviderInput, CustomProviderResult, ImageInput, ModelCatalogEntry,
    ModelCatalogProvider, ModelMetadataCacheTarget, PermissionApprovalDecision,
    PermissionApprovalOutcome, PermissionApprovalRequest, PermissionConfig, PermissionMode,
    PromptAttachmentDisplay, PromptDisplayMetadata, ReloadContextOptions, ReloadContextResult,
    RunControl, RunControlHandle, RunMode, RunOptions, RunResult, RunStreamEvent, RunStreamSink,
    RunWarning, SanitizedMessageSummary, ScopedCustomProviderInput, SelectedAgent,
    SessionExportMessageSummary, SessionRedoResult, SessionSummary, SessionUndoOptions,
    SessionUndoResult, SmokeControl, SmokeOptions, SmokeResult, StatsOptions,
    TUI_DISPLAY_METADATA_KEY, TuiMessageSummary, USER_SHELL_METADATA_KEY, UserShellContextOptions,
    UserShellOptions, UserShellResult, run_control,
};
pub use undo::{redo_session, undo_session};
pub use user_shell::run_user_shell_command_streaming_controlled;
