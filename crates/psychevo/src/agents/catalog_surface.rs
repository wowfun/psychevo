mod discovery;
mod views;

#[cfg(test)]
pub(crate) use discovery::stop_agent_id_with_grace;
pub use discovery::{
    AgentControl, AgentRun, AgentRunRecord, discover_agents, format_agents_for_prompt,
    list_agents_value, resolve_agent_definition, view_agent_value, view_agent_value_with_catalog,
};
pub(crate) use discovery::{
    AgentToolContext, RawAgentFrontmatter, agent_catalog_for_prompt,
    agent_catalog_for_selected_policy, agent_policy_allows_agent_spawn,
    agent_project_instructions_enabled, agent_status_model_value, agent_status_value, agent_tools,
    apply_agent_tool_policy, apply_hook_runtime, apply_runtime_hooks, build_hook_runtime,
    close_agent_id, effective_run_mode, effective_tool_names, format_selected_agent_instruction,
    narrow_permission_mode_for_agent, skill_catalog_visible_for_tools, wait_agent_mailbox,
};
pub use views::{
    AgentBackendConfig, AgentBackendKind, AgentBackendRef, AgentCatalog, AgentContribution,
    AgentDefinition, AgentDiagnostic, AgentDiscoveryOptions, AgentEntrypoint, AgentInvocationRole,
    AgentMailboxWaitOutcome, AgentPermissionMode, AgentRunStatus, AgentSource, AgentToolPolicy,
    MAX_AGENT_SPAWN_DEPTH_CAP, agent_source_display_label,
};
pub(crate) use views::{
    MAX_AGENT_NAME_LEN, SUBAGENT_DEFAULT_MAX_TURNS, SUBAGENT_TASK_LABEL_MAX_CHARS,
    default_peer_agent_entrypoints, default_peer_client_capabilities, default_subagent_entrypoints,
};
