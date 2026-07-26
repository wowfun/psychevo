#![allow(clippy::module_inception)]
#![cfg_attr(not(feature = "internal"), allow(dead_code, unused_imports))]

macro_rules! framework_internal_modules {
    ($visibility:vis) => {
        $visibility mod accounting;
        $visibility mod agents;
        $visibility mod automations;
        $visibility mod command_registry;
        $visibility mod compaction;
        $visibility mod config;
        $visibility mod context;
        $visibility mod context_usage;
        $visibility mod events;
        $visibility mod extensions;
        $visibility mod hooks;
        $visibility mod host_paths;
        $visibility mod host_process;
        $visibility mod mcp;
        $visibility mod media;
        $visibility mod model_state;
        $visibility mod paths;
        $visibility mod plugins;
        $visibility mod process_env;
        $visibility mod prompt_image;
        $visibility mod prompt_templates;
        $visibility mod run;
        $visibility mod sandbox;
        $visibility mod session_export;
        $visibility mod session_lookup;
        $visibility mod session_trace;
        $visibility mod skills;
        #[path = "store.rs"]
        $visibility mod state;
        $visibility mod stats;
        $visibility mod thread_lineage;
        $visibility mod tool_argument_display;
        $visibility mod tool_result_display;
        $visibility mod tools;
        $visibility mod types;
        $visibility mod undo;
        $visibility mod user_shell;
        $visibility mod workspace_diff;
    };
}

#[cfg(feature = "internal")]
framework_internal_modules!(pub);
#[cfg(not(feature = "internal"))]
framework_internal_modules!(pub(crate));

mod application;
pub(crate) mod contribution_projection;
pub(crate) mod error;
pub(crate) mod filesystem_identity;
pub(crate) mod managed_tools;
pub(crate) mod messages;
pub(crate) mod permissions;
pub(crate) mod project_instructions;
pub(crate) mod prompt_assembly;
pub(crate) mod snapshot;
pub(crate) use state as store;
pub(crate) mod tool_surface;

#[cfg(test)]
pub(crate) mod tests;

#[cfg(feature = "internal")]
#[doc(hidden)]
pub use application::AdapterTurnOptions;
pub use application::{
    AgentSessionAdapter, AgentTurnRequest, Application, ApplicationBuilder, Client,
    CompactThreadRequest, ForkThreadRequest, InteractionResponse, ItemStage, PendingInteraction,
    StartThreadRequest, Thread, ThreadItem, ThreadListQuery, ThreadSnapshot, ThreadSummary,
    TurnControl, TurnEvent, TurnEventSender, TurnEventStream, TurnHandle, TurnOutcome, TurnReceipt,
    TurnRequest, TurnResult,
};
pub use compaction::CompactionResult;
pub use context_usage::ContextSnapshot;
pub use error::{Error, Result};
#[doc(hidden)]
pub use psychevo_agent_core as __agent_core;
pub use psychevo_agent_core::ToolBinding as Tool;
#[doc(hidden)]
pub use psychevo_ai as __ai;
pub use psychevo_ai::GenerationProvider as Provider;
pub use skills::SelectedSkill;
pub use types::{
    ApprovalHandler, ApprovalMode, ImageInput, McpServerInput, PermissionMode,
    ProjectContextInstructionMode, PromptDisplayMetadata, RunMode, RunSandboxOverride,
    RunTerminalError, RunWarning, SelectedAgent,
};
