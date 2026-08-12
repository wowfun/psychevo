pub mod accounting;
pub mod agents;
pub mod automations;
pub mod command_registry;
pub mod compaction;
pub mod config;
mod context;
pub mod context_usage;
mod events;
pub mod extensions;
pub mod hooks;
pub mod host_paths;
pub mod host_process;
pub mod mcp;
pub mod media;
pub mod model_state;
pub mod paths;
pub mod plugins;
pub mod process_env;
mod process_tree;
pub mod prompt_image;
pub mod prompt_templates;
mod run;
pub mod sandbox;
pub mod session_export;
mod session_lookup;
pub mod session_trace;
pub mod skills;
#[path = "store.rs"]
mod state;
pub mod stats;
pub mod thread_lineage;
pub mod tool_argument_display;
pub mod tool_result_display;
mod tools;
mod types;
pub mod undo;
pub mod user_shell;
pub mod workspace_diff;

pub mod application;
pub(crate) mod error;
pub(crate) mod filesystem_identity;
pub(crate) mod managed_tools;
pub(crate) mod messages;
mod panic_evidence;
pub(crate) mod permissions;
pub(crate) mod project_instructions;
pub(crate) mod prompt_assembly;
pub(crate) mod snapshot;
pub(crate) use state as store;
pub(crate) mod tool_surface;

#[cfg(test)]
pub(crate) mod tests;

pub use error::{Error, Result};
