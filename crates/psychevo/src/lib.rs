#![allow(clippy::module_inception)]
#![cfg_attr(not(feature = "product"), allow(dead_code, unused_imports))]

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

framework_internal_modules!(pub(crate));

#[cfg(feature = "product")]
/// First-party product assembly facade.
///
/// Raw implementation modules remain private even when this feature is
/// enabled:
///
/// ```compile_fail
/// use psychevo::state::StateRuntime;
/// ```
#[doc(hidden)]
pub mod __product {
    pub mod capabilities {
        pub use crate::agents::*;
        pub use crate::extensions::*;
        pub use crate::hooks::*;
        pub use crate::plugins::*;
        pub use crate::skills::*;
    }

    pub mod commands {
        pub use crate::command_registry::*;
    }

    pub mod configuration {
        pub use crate::config::*;
    }

    pub mod integrations {
        pub use crate::mcp::*;
    }

    pub mod persistence {
        pub use crate::state::*;
    }

    pub mod platform {
        pub use crate::host_paths::*;
        pub use crate::host_process::*;
        pub use crate::media::*;
        pub use crate::paths::*;
        pub use crate::process_env::*;
        pub use crate::sandbox::*;
    }

    pub mod presentation {
        pub use crate::prompt_image::*;
        pub use crate::prompt_templates::*;
        pub use crate::tool_argument_display::*;
        pub use crate::tool_result_display::*;
        pub use crate::user_shell::*;
    }

    pub mod runtime {
        pub use crate::mcp::McpRuntime;
        pub use crate::model_state::*;
        pub use crate::run::*;
        pub use crate::types::*;
    }

    pub mod sessions {
        pub use crate::automations::*;
        pub use crate::compaction::*;
        pub use crate::session_export::*;
        pub use crate::session_trace::*;
        pub use crate::thread_lineage::*;
        pub use crate::undo::*;
        pub use crate::workspace_diff::*;
    }

    pub mod usage {
        pub use crate::accounting::*;
        pub use crate::context_usage::*;
        pub use crate::stats::*;
    }
}

mod application;
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

#[cfg(feature = "product")]
#[doc(hidden)]
pub use application::AdapterTurnOptions;
pub use application::{
    AgentSessionAdapter, AgentTurnRequest, Application, ApplicationBuilder, Client,
    CompactThreadRequest, ForkThreadRequest, HistoryPage, HistoryReader, InteractionResponse,
    ItemStage, PendingInteraction, PendingTerminalFailure, ShutdownAdapterStatus, ShutdownReport,
    StartThreadRequest, Thread, ThreadExecutionContext, ThreadItem, ThreadListPage,
    ThreadListQuery, ThreadSnapshot, ThreadSummary, TurnControl, TurnEvent, TurnEventSender,
    TurnEventStream, TurnHandle, TurnOutcome, TurnReceipt, TurnRequest, TurnResult,
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
