mod accounting;
mod config;
mod context;
mod context_usage;
mod error;
mod events;
mod messages;
mod paths;
mod run;
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

pub use config::{
    configured_models, create_global_custom_provider, custom_provider_api_key_env,
    fetch_model_catalog, model_catalog_endpoint, model_catalog_providers,
    refresh_model_metadata_cache, selected_configured_model,
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
pub use run::{run_live, run_live_streaming, run_live_streaming_controlled};
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
pub use store::SqliteStore;
pub use tools::tool_names_for_mode;
pub use types::{
    ConfiguredModel, CustomProviderInput, CustomProviderResult, ModelCatalogEntry,
    ModelCatalogProvider, ModelMetadataCacheTarget, RunControl, RunControlHandle, RunMode,
    RunOptions, RunResult, RunStreamEvent, RunStreamSink, SanitizedMessageSummary,
    SessionRedoResult, SessionSummary, SessionUndoOptions, SessionUndoResult, SmokeControl,
    SmokeOptions, SmokeResult, StatsOptions, TuiMessageSummary, UserShellOptions, UserShellResult,
    run_control,
};
pub use undo::{redo_session, undo_session};
pub use user_shell::run_user_shell_command_streaming_controlled;
