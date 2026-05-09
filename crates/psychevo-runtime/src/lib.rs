mod config;
mod context;
mod error;
mod events;
mod messages;
mod paths;
mod run;
mod session_lookup;
mod skills;
mod smoke;
mod snapshot;
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
    selected_configured_model,
};
pub use context::prune_context;
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
pub use store::SqliteStore;
pub use tools::tool_names_for_mode;
pub use types::{
    ConfiguredModel, CustomProviderInput, CustomProviderResult, ModelCatalogEntry,
    ModelCatalogProvider, RunControl, RunControlHandle, RunMode, RunOptions, RunResult,
    RunStreamEvent, RunStreamSink, SanitizedMessageSummary, SessionRedoResult, SessionSummary,
    SessionUndoOptions, SessionUndoResult, SmokeControl, SmokeOptions, SmokeResult,
    TuiMessageSummary, UserShellOptions, UserShellResult, run_control,
};
pub use undo::{redo_session, undo_session};
pub use user_shell::run_user_shell_command_streaming_controlled;
