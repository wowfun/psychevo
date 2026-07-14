#[allow(unused_imports)]
use psychevo_runtime::{
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

#[allow(unused_imports)]
use psychevo_runtime::{
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

#[allow(unused_imports)]
use psychevo_runtime::{
    CONTEXT_BAR_MAX_CELLS, CONTEXT_BAR_MIN_CELLS, ContextAdvice, ContextCategory,
    ContextFormatOptions, ContextOptions, ContextScope, ContextSnapshot, ContextTokenizer,
    ContextTotal, context_snapshot, format_context_snapshot_text,
    format_context_snapshot_text_with_options, format_context_total_value,
    format_context_total_value_parts, normalize_context_bar_width,
};

#[allow(unused_imports)]
use psychevo_runtime::{
    fallback_visible_session_title, reload_session_context, run_live, run_live_streaming,
    run_live_streaming_controlled, spawn_agent_background,
};

#[allow(unused_imports)]
use psychevo_runtime::{
    SessionArtifactKind, SessionExportArtifact, SessionExportFormat, SessionExportInclude,
    SessionExportIncludeSet, SessionExportOptions, SessionExportWriteResult,
    default_session_export_filename, render_session_export, write_session_export,
};

#[allow(unused_imports)]
use psychevo_runtime::{
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

#[allow(unused_imports)]
use psychevo_runtime::{
    AgentEdgeRecord, AgentEdgeStatus, AgentMissionRunInput, AgentMissionRunRecord,
    AgentTeamRunInput, AgentTeamRunRecord, AutomationRunFinishInput, AutomationRunRecord,
    AutomationTaskInput, AutomationTaskRecord, ChildSessionSnapshotInput, ContextEvidenceInput,
    ContextEvidenceRecord, GatewayActivityClaimInput, GatewayActivityRecord,
    GatewayChannelOutboxInput, GatewayChannelOutboxRecord, GatewayControlCommandInput,
    GatewayControlCommandRecord, GatewayLiveEventRecord, GatewayLiveSnapshotInput,
    GatewayLiveSnapshotRecord, GatewayRuntimeBindingInput, GatewayRuntimeBindingOwnership,
    GatewayRuntimeBindingRecord, GatewayRuntimeBindingStatus, GatewayRuntimeControlStatePatch,
    GatewaySourceBindingInput, GatewaySourceBindingRecord, GatewaySourceLaneInput,
    GatewaySourceLaneRecord, GatewayTurnDeliveryInput, GatewayTurnDeliveryRecord,
    GatewayTurnTerminalInput, GatewayTurnTerminalRecord, SessionCompactionInput,
    SessionCompactionRecord, SessionMessageRecord, SqliteStore,
};

#[test]
fn affected_facade_exports_remain_available_at_the_crate_root() {}
