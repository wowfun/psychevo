#![allow(clippy::module_inception)]

pub mod accounting;
pub mod agents;
pub mod automations;
pub mod command_registry;
pub mod compaction;
pub mod config;
pub mod context;
pub mod context_usage;
pub(crate) mod contribution_projection;
pub(crate) mod error;
pub mod events;
pub mod extensions;
pub(crate) mod filesystem_identity;
pub mod hooks;
pub mod host_paths;
pub mod host_process;
pub(crate) mod managed_tools;
pub mod mcp;
pub mod media;
pub(crate) mod messages;
pub mod model_state;
pub mod paths;
pub(crate) mod permissions;
pub mod plugins;
pub mod process_env;
pub(crate) mod project_instructions;
pub(crate) mod prompt_assembly;
pub mod prompt_image;
pub mod prompt_templates;
pub mod run;
pub mod sandbox;
pub mod session_export;
pub mod session_lookup;
pub mod session_trace;
pub mod skills;
pub(crate) mod snapshot;
#[path = "store.rs"]
pub mod state;
pub mod stats;
pub(crate) use state as store;
pub mod thread_lineage;
pub mod tool_argument_display;
pub mod tool_result_display;
pub(crate) mod tool_surface;
pub mod tools;
pub mod types;
pub mod undo;
pub mod user_shell;
pub mod workspace_diff;

#[cfg(test)]
pub(crate) mod tests;

pub use error::{Error, Result};
