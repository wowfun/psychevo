#![allow(clippy::module_inception)]

pub(crate) mod accounting;
pub(crate) mod agents;
pub mod automations;
pub mod command_registry;
pub(crate) mod compaction;
pub(crate) mod config;
pub(crate) mod context;
pub(crate) mod context_usage;
pub(crate) mod contribution_projection;
pub(crate) mod error;
pub(crate) mod events;
pub mod extensions;
pub mod hooks;
mod host_paths;
pub mod host_process;
pub(crate) mod managed_tools;
pub(crate) mod mcp;
pub mod media;
pub(crate) mod messages;
pub mod model_state;
pub(crate) mod paths;
pub(crate) mod permissions;
pub mod plugins;
pub mod process_env;
pub(crate) mod project_instructions;
pub(crate) mod prompt_assembly;
pub(crate) mod prompt_image;
pub(crate) mod prompt_templates;
pub(crate) mod run;
pub(crate) mod sandbox;
pub(crate) mod session_export;
pub(crate) mod session_lookup;
pub(crate) mod session_trace;
pub(crate) mod skills;
pub(crate) mod snapshot;
pub(crate) mod state_runtime;
pub(crate) mod stats;
pub(crate) mod store;
pub(crate) mod thread_lineage;
pub(crate) mod tool_surface;
pub(crate) mod tools;
pub(crate) mod types;
pub(crate) mod undo;
pub(crate) mod user_shell;
pub mod workspace_diff;

#[cfg(test)]
pub(crate) mod tests;

pub use agents::{
    AgentBackendConfig, AgentBackendKind, AgentBackendRef, AgentCatalog, AgentContribution,
    AgentControl, AgentDefinition, AgentDiagnostic, AgentDiscoveryOptions, AgentEntrypoint,
    AgentInvocationRole, AgentPermissionMode, AgentRun, AgentRunRecord, AgentRunStatus,
    AgentSource, AgentTeamCatalog, AgentTeamDefinition, AgentTeamMember, AgentTeamSource,
    AgentToolPolicy, DEFAULT_TEAM_PARALLEL_AGENTS, LoadedMainAgent, MAX_AGENT_SPAWN_DEPTH_CAP,
    MAX_TEAM_PARALLEL_AGENTS_CAP, SESSION_MAIN_AGENT_METADATA_KEY, agent_source_display_label,
    agent_spawn_paused, agent_status_records, agent_status_value, close_agent_id,
    discover_agent_teams, discover_agent_teams_with_catalog, discover_agents, list_agents_value,
    main_agent_default_metadata, main_agent_from_session_metadata, main_agent_metadata,
    parse_agent_definition_text, parse_agent_team_definition_text, resolve_agent_definition,
    resolve_agent_team_definition, resume_agent_id, send_agent_message,
    session_agent_input_from_metadata, session_base_agent_name_from_metadata,
    session_main_agent_explicit_default, set_agent_spawn_paused, stop_agent_id_with_grace,
    valid_agent_name, view_agent_value, view_agent_value_with_catalog, wait_agent_id,
    wait_agent_mailbox,
};
pub use automations::{AutomationSchedule, latest_due_at_ms, next_run_at_ms};
pub use compaction::{
    AutoCompactionCheckOptions, CompactSessionOptions, CompactionReason, CompactionResult,
    auto_compaction_due_for_snapshot, compact_session,
};
pub use config::{
    ChannelRuntimeConnection, ChannelSetupInput, ChannelUpdateInput, DEFAULT_WORKSPACE_NAME,
    DEFAULT_WORKSPACE_ROOT, McpServerConfigInput, McpToolPolicyInput, REASONING_EFFORT_VALUES,
    ResolvedVoiceAsrConfig, ResolvedVoiceRealtimeConfig, ResolvedVoiceTtsConfig,
    RuntimeProfileConfig, RuntimeProfileKind, ToolsetMutationResult,
    append_local_permission_allow_rule, append_local_permission_rule, auth_status_value,
    channel_doctor_value, channel_list_value, channel_runtime_connections, channel_show_value,
    channel_summary_value, clear_mcp_oauth_access_token, config_provider_list_value,
    config_show_value, configured_models, create_global_custom_provider, create_local_toolset,
    create_scoped_custom_provider, custom_provider_api_key_env, delete_channel_connection,
    fetch_and_cache_model_catalog, fetch_model_catalog, image_generation_config_value,
    load_agent_backend_configs, load_mcp_oauth_access_token, load_runtime_profile_configs,
    mcp_oauth_keyring_account, mcp_server_value, mcp_servers_value, model_catalog_endpoint,
    model_catalog_entry_is_free, model_catalog_provider, model_catalog_providers,
    normalize_provider_id, permission_rules_value, provider_models_cache_path_for_home,
    read_cached_model_catalog, refresh_model_metadata_cache, remove_config_value,
    remove_local_permission_rule, remove_local_toolset, remove_mcp_server,
    resolve_default_workspace_cwd, resolve_image_generation_config, resolve_voice_asr_config,
    resolve_voice_realtime_config, resolve_voice_tts_config, resolve_workspace_root,
    save_mcp_oauth_access_token, selected_configured_model, set_auxiliary_model,
    set_auxiliary_model_with_reasoning, set_channel_enabled, set_config_value, set_default_model,
    set_default_model_with_reasoning, set_local_toolset_enabled, set_mcp_server_enabled,
    set_mcp_server_tool_policy, set_provider_api_key, set_provider_model_config,
    setup_channel_connection, toolsets_value, update_channel_connection,
    update_global_web_search_settings, upsert_channel_connection, upsert_mcp_server,
    validate_runtime_profile_backend_ref, voice_config_value, web_search_settings_value,
    write_cached_model_catalog,
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
pub use extensions::{
    ApprovalReviewContributor, CapabilityRootLocation, ConfigContributor, ContextContributor,
    ExtensionData, ExtensionDataInit, ExtensionDataScope, ExtensionRegistry,
    ExtensionRegistryBuilder, McpServerContributor, SelectedCapabilityRoot,
    ThreadLifecycleContributor, TokenUsageContributor, ToolContributor, ToolLifecycleContributor,
    TurnInputContributor, TurnItemContributor, TurnLifecycleContributor,
    resolve_mcp_server_handoffs,
};
pub use host_paths::{
    ExecutableResolveOptions, GitBashRuntime, HostPlatform, PSYCHEVO_GIT_BASH_PATH,
    display_path_for_native_path, normalized_native_path, resolve_executable_path,
    resolve_input_path, shell_is_git_bash,
};
pub use mcp::mcp_test_server_value;
pub use media::{
    GeneratedImageArtifact, ImageMimeKind, MAX_IMAGE_SOURCE_BYTES, ResolvedImageSource,
    media_agent_visible_source, media_artifact_path, media_display_url, read_media_artifact,
    resolve_explicit_image_source, validate_media_artifact_id, write_generated_image_artifact,
};
pub use model_state::{
    MODEL_STATE_FILE, MODEL_STATE_RECENT_LIMIT, MODEL_STATE_VERSION, ModelCwdState,
    ModelRecentEntry, ModelState, SESSION_COMPOSER_MODEL_METADATA_KEY, normalize_reasoning_effort,
};
pub use paths::{canonicalize_cwd, workspace_snapshot_id};
pub use plugins::{
    LoadedPluginManifest, PluginAdapterMode, PluginDiagnostic, PluginInspectOptions,
    PluginInstallOptions, PluginInstallRecord, PluginInterfaceMetadata, PluginManifestKind,
    PluginMarketplaceEntry, PluginScope, PluginSourceKind, PluginWorkerSpec, install_plugin,
    load_plugin_manifest, plugin_doctor_value, plugin_import_inspect_value, plugin_install_value,
    plugin_list_value, plugin_marketplace_add_value, plugin_marketplace_list_value,
    plugin_marketplace_remove_value, plugin_set_enabled_value, plugin_set_trust_value,
    plugin_uninstall_value, plugin_view_value,
};
pub use process_env::{
    ProcessEnvOptions, apply_process_env, apply_pty_process_env, apply_tokio_process_env,
    combined_path_value, decode_process_output, decode_process_output_for_platform,
    effective_process_env, env_value_case_insensitive, executable_path_candidates,
    prefixed_path_overlay, terminate_pty_child_tree, terminate_std_child_process_group,
    terminate_std_child_tree, terminate_tokio_child_tree, tokio_host_process_command,
    windows_pathext_extensions, windows_utf8_default_env,
};
pub use prompt_image::{
    extract_image_sources_from_prompt, model_metadata_explicitly_disallows_image_input,
    prompt_message_from_inputs_with_options, prompt_starts_with_supported_image_path,
    resolve_image_source, split_image_source_argument,
};
pub use prompt_templates::side_conversation_boundary_prompt;
pub use psychevo_agent_core::{
    AssistantBlock, Message, PendingInputId, ProviderToolBlock, TerminalReason, ToolCallBlock,
    ToolDisplayBodyPolicy, ToolDisplayCategory, ToolDisplaySpec, UserContentBlock,
};
pub use psychevo_ai::AbortSignal;
pub use psychevo_ai::Outcome;
pub use psychevo_ai::{VoiceAudioFormat, VoiceRealtimeTransport};
pub use run::{
    fallback_visible_session_title, reload_session_context, run_live, run_live_streaming,
    run_live_streaming_controlled, spawn_agent_background,
};
pub use sandbox::{sandbox_status_text, sandbox_status_value};
pub use session_export::{
    SessionArtifactKind, SessionExportArtifact, SessionExportFormat, SessionExportInclude,
    SessionExportIncludeSet, SessionExportOptions, SessionExportWriteResult,
    default_session_export_filename, render_session_export, write_session_export,
};
pub use session_lookup::{latest_run_session_for_cwd, session_exists};
pub use session_trace::{
    SESSION_TRACE_DEFAULT_LIMIT, SESSION_TRACE_MAX_LIMIT, SessionTraceReadOptions,
    SessionTraceReadResult, read_session_trace, session_trace_path,
};
pub use skills::{
    InstallOptions, ListSkillsOptions, SaveSkillBundleOptions, ScanResult, ScanVerdict,
    SelectedSkill, SkillBundle, SkillCatalog, SkillDiagnostic, SkillDiscoveryOptions,
    SkillSettings, SkillSource, SkillTarget, create_skill, delete_skill_bundle, discover_skills,
    discover_skills_with_settings, edit_skill, expand_skill_prompt, install_skill,
    list_skill_bundles, list_skills_value, list_skills_value_with_options, load_skill_settings,
    patch_skill, remove_installed_skill, remove_skill, remove_skill_file, resolve_skills_home,
    save_skill_bundle, scan_skill_path, select_explicit_skills, select_skills_for_prompt,
    set_skill_config_value, set_skill_enabled, skill_context_messages, skill_source_display_label,
    target_skills_dir, view_skill_value, view_skill_value_selected, write_installed_skill,
    write_skill_file,
};
pub use state_runtime::StateRuntime;
pub use stats::{session_usage_summary, usage_read, usage_stats};
pub use store::{
    AgentEdgeRecord, AgentEdgeStatus, AgentMissionRunInput, AgentMissionRunRecord,
    AgentTeamRunInput, AgentTeamRunRecord,
};
pub use store::{
    AutomationRunFinishInput, AutomationRunRecord, AutomationTaskInput, AutomationTaskRecord,
    ChildSessionSnapshotInput, ContextEvidenceInput, ContextEvidenceRecord, ConversationDraftPart,
    GatewayActivityClaimInput, GatewayActivityRecord, GatewayChannelOutboxInput,
    GatewayChannelOutboxRecord, GatewayControlCommandInput, GatewayControlCommandRecord,
    GatewayLiveEventRecord, GatewayLiveSnapshotInput, GatewayLiveSnapshotRecord,
    GatewayRuntimeBindingInput, GatewayRuntimeBindingOwnership, GatewayRuntimeBindingRecord,
    GatewayRuntimeBindingStatus, GatewayRuntimeControlStatePatch, GatewaySourceBindingInput,
    GatewaySourceBindingRecord, GatewaySourceLaneInput, GatewaySourceLaneRecord,
    GatewayTurnDeliveryInput, GatewayTurnDeliveryRecord, GatewayTurnTerminalInput,
    GatewayTurnTerminalRecord, NativeSessionForkInput, SessionBrowserRequest,
    SessionBrowserWorkspaceProjection, SessionCompactionInput, SessionCompactionRecord,
    SessionListProjection, SessionMessageRecord, SessionRevertKind, SessionRevertState,
    SqliteStore,
};
pub use thread_lineage::{
    SIDE_CONVERSATION_METADATA_KEY, SIDE_CONVERSATION_SESSION_SOURCES, SIDE_INHERITED_METADATA_KEY,
    TUI_SIDE_CONVERSATION_SESSION_SOURCE, WEB_SIDE_CONVERSATION_SESSION_SOURCE,
    side_conversation_session_source, side_inherited_metadata_hidden,
};
pub use tools::tool_names_for_mode;
pub use types::{
    AgentSpawnOptions, AgentSpawnResult, ApprovalHandler, ApprovalMode, ClarifyAnswer,
    ClarifyInteractionOutcome, ClarifyQuestion, ClarifyQuestionOption, ClarifyRequestEvent,
    ClarifyResolvedEvent, ClarifyResolvedReason, ClarifyResponse, ClarifyResult, ConfigScope,
    ConfiguredModel, CustomProviderInput, CustomProviderResult, EDITABLE_INPUT_METADATA_KEY,
    ExternalAgentDelegate, ExternalAgentDelegateRequest, ExternalAgentDelegateResult, ImageInput,
    McpServerInput, McpServerPolicy, McpTransportInput, ModelCatalogEntry, ModelCatalogProvider,
    ModelMetadataCacheTarget, PermissionApprovalDecision, PermissionApprovalOutcome,
    PermissionApprovalRequest, PermissionConfig, PermissionMode, ProjectContextInstructionMode,
    PromptAttachmentDisplay, PromptDisplayMetadata, ReloadContextOptions, ReloadContextResult,
    ResolvedMcpServerInput, RunControl, RunControlHandle, RunMode, RunOptions, RunResult,
    RunSandboxMode, RunSandboxOverride, RunStreamEvent, RunStreamSink, RunTerminalError,
    RunWarning, RuntimeTool, SanitizedMessageSummary, ScopedCustomProviderInput, SelectedAgent,
    SessionExportMessageSummary, SessionRedoResult, SessionSummary, SessionUndoOptions,
    SessionUndoResult, SessionUsageOptions, SessionUsageSummary, SmokeControl, StatsOptions,
    StoredEditableInputEnvelope, StoredEditableInputPart, TUI_DISPLAY_METADATA_KEY,
    TuiMessageSummary, USER_SHELL_METADATA_KEY, UsageActivity, UsageActivityDay, UsageReadOptions,
    UsageReadResult, UsageWindowSummary, UserShellContextOptions, UserShellOptions,
    UserShellResult, run_control,
};
pub use undo::{redo_session, undo_session};
pub use user_shell::run_user_shell_command_streaming_controlled;
pub use workspace_diff::{
    WORKSPACE_DIFF_MAX_BYTES, WORKSPACE_DIFF_MAX_LINES, WorkspaceDiff, WorkspaceDiffFile,
    WorkspaceDiffFileStatus, WorkspaceDiffTruncation, collect_workspace_diff,
    collect_workspace_diff_with_caps,
};
