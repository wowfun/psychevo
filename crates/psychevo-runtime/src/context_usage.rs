pub(crate) use std::collections::BTreeMap;
pub(crate) use std::path::PathBuf;
pub(crate) use std::sync::{Arc, Mutex};

pub(crate) use futures::future::BoxFuture;
pub(crate) use psychevo_agent_core::{Message, PromptInstruction};
pub(crate) use psychevo_ai::{
    AbortSignal, GenerationProvider, GenerationRequest, GenerationStream, ModelTarget,
    OpenAiChatTokenCount, count_openai_chat_request,
};
pub(crate) use serde::{Deserialize, Serialize};
pub(crate) use serde_json::{Value, json};

pub(crate) use crate::accounting::{EffectiveUsageTotal, effective_usage_total};
pub(crate) use crate::compaction::load_projected_messages;
pub(crate) use crate::config::{load_project_context_instruction_mode, selected_configured_model};
pub(crate) use crate::error::{Error, Result};
pub(crate) use crate::paths::canonical_cwd;
pub(crate) use crate::project_instructions::load_project_instructions;
pub(crate) use crate::prompt_assembly::runtime_environment_prompt;
pub(crate) use crate::skills::{
    SkillDiscoveryOptions, discover_skills, format_skills_for_prompt, resolve_skills_home,
    skills_visible_for_prompt_with_tools,
};
pub(crate) use crate::state_runtime::StateRuntime;
pub(crate) use crate::tool_surface::tool_declarations;
pub(crate) use crate::tools::{coding_core_tools_for_mode, mode_instruction, skill_tools_for_mode};
pub(crate) use crate::types::{RunMode, RunOptions};

#[path = "context_usage/snapshot.rs"]
mod snapshot;
pub use snapshot::{
    CONTEXT_BAR_MAX_CELLS, CONTEXT_BAR_MIN_CELLS, ContextAdvice, ContextCategory,
    ContextFormatOptions, ContextOptions, ContextScope, ContextSnapshot, ContextTokenizer,
    ContextTotal, context_snapshot, format_context_snapshot_text,
    format_context_snapshot_text_with_options, format_context_total_value,
    format_context_total_value_parts,
};
pub(crate) use snapshot::{
    ContextRecorder, ContextRecordingProvider, LiveContextProfile, context_counting_metadata,
};
#[path = "context_usage/presentation.rs"]
mod presentation;
pub use presentation::normalize_context_bar_width;
