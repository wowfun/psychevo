pub(crate) use std::collections::{BTreeMap, BTreeSet, VecDeque};
pub(crate) use std::io::{self, IsTerminal};
pub(crate) use std::path::PathBuf;
#[cfg(test)]
pub(crate) use std::process::Command as StdCommand;
pub(crate) use std::process::ExitCode;
pub(crate) use std::time::Instant;

pub(crate) use crate::provider_setup::{
    ProviderSetupPresetId, default_provider_setup_api_key_env, is_loopback_base_url,
    looks_like_api_key, provider_setup_preset, provider_setup_presets, validate_api_key_env,
    validate_base_url,
};
pub(crate) use anyhow::{Result, anyhow};
pub(crate) use crossterm::event::{
    self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton,
    MouseEvent, MouseEventKind,
};
#[cfg(test)]
pub(crate) use psychevo::TurnRequest;
pub(crate) use psychevo::application::{
    AgentMissionRegistration, AgentTeamRegistration, ClarifyAnswer, ClarifyQuestion,
    ClarifyRequestEvent, ClarifyResolvedEvent, ClarifyResolvedReason, ClarifyResponse,
    ClarifyResult, HistoryReplayItem, ModelMetadataCacheTarget, Outcome, QueuedSteerId,
    TerminalReason, ToolDisplaySpec,
};
pub(crate) use psychevo::{
    AgentRelationship, AgentRelationshipStatus, AutoCompactionRequest, Client as FrameworkClient,
    CompactThreadRequest, ConfigurationQuery, ConfigureProviderRequest,
    CreateCustomProviderRequest, ImageInput, PermissionMode, PromptAttachmentDisplay,
    RefreshThreadContextResult, RunMode, SetThreadMainAgentSelection, ShellCommandEvent,
    ShellCommandOutcome, SideConversationSurface, StartSideConversationRequest, StartThreadRequest,
    ThreadItem, ThreadListQuery, ThreadMainAgentSelection, ThreadModelSelection, ThreadSummary,
    ThreadUsageSummary, TurnAdmissionCancellation, TurnEvent, TurnOutcome, UsageQuery,
    UserShellDisplay, agents::AgentCatalog, agents::AgentDiscoveryOptions, agents::AgentEntrypoint,
    agents::AgentRunStatus, agents::AgentSource, agents::MAX_AGENT_SPAWN_DEPTH_CAP,
    agents::MAX_TEAM_PARALLEL_AGENTS_CAP, agents::discover_agent_teams_with_catalog,
    agents::discover_agents, agents::resolve_agent_definition,
    agents::resolve_agent_team_definition, application::PermissionApprovalDecision,
    application::PermissionApprovalOutcome, application::PermissionApprovalRequest,
    compaction::CompactionReason, compaction::CompactionResult, config::ConfigScope,
    config::ConfiguredModel, config::custom_provider_api_key_env,
    context_usage::ContextFormatOptions, context_usage::ContextSnapshot,
    context_usage::format_context_snapshot_text_with_options,
    context_usage::format_context_total_value, context_usage::format_context_total_value_parts,
    context_usage::normalize_context_bar_width, model_state::ModelState,
    model_state::normalize_reasoning_effort, paths::canonicalize_cwd,
    prompt_image::model_metadata_explicitly_disallows_image_input,
    prompt_image::prompt_message_from_inputs_with_options, prompt_image::resolve_image_source,
    session_export::SessionArtifactKind, session_export::SessionExportFormat,
    session_export::SessionExportOptions, session_export::SessionExportWriteResult,
    session_export::default_session_export_filename, skills::InstallOptions, skills::SkillBundle,
    skills::SkillCatalog, skills::SkillDiscoveryOptions, skills::SkillTarget,
    skills::discover_skills, skills::install_skill, skills::list_skill_bundles,
    skills::remove_installed_skill, skills::scan_skill_path, skills::set_skill_config_value,
    skills::set_skill_enabled, skills::view_skill_value,
    thread_lineage::TUI_SIDE_CONVERSATION_SESSION_SOURCE,
    tool_argument_display::WriteArgumentPreview,
    tool_argument_display::WriteArgumentPreviewTracker,
    tool_argument_display::write_argument_preview_from_args,
    tool_result_display::decode_persisted_tool_result_for_display, workspace_diff::WorkspaceDiff,
    workspace_diff::collect_workspace_diff,
};
#[cfg(test)]
pub(crate) use psychevo::{ShellCommandResult, TurnResult, config::ModelCatalogEntry};
pub(crate) use psychevo_gateway::composition::GatewayApplication;
pub(crate) use psychevo_gateway::gateway::activity::GatewayActivity;
pub(crate) use psychevo_gateway::gateway::live_projection::{
    ForeignGatewayLiveEvent, GatewayLiveSnapshotObservation,
};
pub(crate) use psychevo_gateway::history_editing::HistoryEditingSurface;
pub(crate) use psychevo_gateway_protocol::events_transcript::{
    GatewayActionKind, GatewayActionOutcome, GatewayEvent, ThreadHistoryEditingKind,
    TranscriptBlock, TranscriptBlockKind, TranscriptBlockStatus, TranscriptEntry,
    TranscriptEntryRole,
};
pub(crate) use psychevo_gateway_protocol::source::{
    GatewayImageInput, GatewaySource, GatewayThreadSelector, GatewayTurnStatus,
};
pub(crate) use psychevo_gateway_protocol::thread_command_turn::{
    ThreadEditableDraft, ThreadEditableDraftFidelity, ThreadEditableInputPart,
};
pub(crate) use ratatui::Frame;
pub(crate) use ratatui::Terminal;
pub(crate) use ratatui::backend::CrosstermBackend;
pub(crate) use ratatui::layout::{Constraint, Direction, Layout, Rect};
pub(crate) use ratatui::style::{Color, Modifier, Style};
pub(crate) use ratatui::text::{Line, Span, Text};
pub(crate) use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};
pub(crate) use ratatui_textarea::{CursorMove, TextArea};
pub(crate) use serde_json::Value;
pub(crate) use tokio::sync::{mpsc, oneshot};
pub(crate) use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub(crate) mod plain;
pub(crate) mod slash;
pub(crate) mod state;

#[cfg(test)]
pub(crate) mod tests;

use self::plain::{TuiRenderer, assistant_text_from_event};
use self::slash::{
    EffectiveSlashConfig, SlashCommand, SlashHelpSections, SlashMenuItem, SlashShortcutMatch,
    TuiSlashParse, VARIANTS, configured_slash_menu_items, format_slash_help_with_config,
    parse_effective_slash_config, parse_slash_command_with_config, parse_tui_slash_with_config,
    slash_help_sections_with_config, slash_menu_items_from, validate_model_spec, validate_variant,
};
use self::state::TuiState;
pub(crate) use crate::args::TuiArgs;
pub(crate) use crate::env::{
    ensure_home_initialized, env_path, env_value, inherited_env, resolve_explicit_path,
    resolve_psychevo_home, resolve_state_db,
};
pub(crate) use psychevo::command_registry::mission_prompt_marker;

pub(crate) const TUI_CONTINUE_SESSION_SOURCES: &[&str] = &["run", "tui"];
pub(crate) const TUI_INTERNAL_SESSION_SOURCES: &[&str] = &[TUI_SIDE_CONVERSATION_SESSION_SOURCE];
pub(crate) const USER_SHELL_HELP: &str = "shell mode: type !<command> to run a local shell command";
pub(crate) const FILE_POPUP_MAX_ROWS: usize = 8;
pub(crate) const COMPLETION_POPUP_MAX_ROWS: usize = FILE_POPUP_MAX_ROWS + 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompletionPopupTarget {
    Agent(usize),
    File(usize),
    Skill(usize),
}

pub(crate) async fn run_tui_command(args: &TuiArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let journey_profile = TuiJourneyProfileProbe::from_env(&env_map)?;
    let cwd = std::env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    ensure_home_initialized(&home)?;
    let config_path = env_path("PSYCHEVO_CONFIG", &env_map, &cwd)?;
    let db_path = resolve_state_db(&env_map, &home, &cwd)?;
    let runtime = GatewayApplication::open(
        home.clone(),
        db_path.clone(),
        config_path.clone(),
        env_map.clone(),
    )
    .await?;
    let cwd = match &args.cd {
        Some(cd) => resolve_explicit_path(cd, &env_map, &cwd)?,
        None => cwd,
    };
    let cwd = canonicalize_cwd(&cwd)?;
    let slash_config = load_effective_tui_slash_config(runtime.client(), &env_map, cwd.clone())?;
    let cwd_key = cwd.to_string_lossy().to_string();
    let state_path = home.join("tui-state.json");
    let state = TuiState::load(&state_path)?;
    let model_state_path = ModelState::path_for_home(&home);
    let model_state = ModelState::load(&model_state_path)?;
    let current_model = args
        .model
        .clone()
        .or_else(|| model_state.model_for(&cwd_key));
    let current_variant = args
        .variant
        .map(|variant| variant.as_str().to_string())
        .or_else(|| model_state.reasoning_effort_for(&cwd_key));
    let current_mode = state
        .mode_for(&cwd_key)
        .and_then(|value| RunMode::parse(&value))
        .unwrap_or_default();
    let current_permission_mode = args
        .permission_mode
        .map(|mode| mode.permission_mode())
        .or_else(|| {
            state
                .permission_mode_for(&cwd_key)
                .and_then(|value| PermissionMode::parse(&value))
        })
        .unwrap_or_default();
    let current_mode = args
        .permission_mode
        .map(|mode| mode.run_mode())
        .unwrap_or(current_mode);
    let startup_agent = (!args.no_agents).then(|| args.agent.clone()).flatten();
    let thinking_visible = state.thinking_visible;
    let raw_visible = state.raw_visible;
    let current_session = if let Some(session) = &args.session {
        Some(session.clone())
    } else if args.new_session {
        None
    } else {
        latest_human_visible_session_id(runtime.client()).await?
    };

    let color = io::stdout().is_terminal() && env_value("NO_COLOR", &env_map).is_none();
    let (clipboard_result_tx, clipboard_result_rx) = std::sync::mpsc::channel();
    let last_gateway_live_event_seq = runtime
        .gateway()
        .latest_live_event_seq()
        .await
        .unwrap_or_default();
    let mut app = TuiApp {
        env_map,
        home,
        state_path,
        state,
        model_state_path,
        model_state,
        runtime,
        db_path,
        config_path,
        cwd,
        cwd_key,
        current_session,
        current_session_title: None,
        current_session_forked_from: None,
        current_agent_breadcrumb: None,
        force_new_once: args.new_session,
        draft_source_raw_id: None,
        current_model,
        current_variant,
        selected_model: None,
        current_mode,
        current_permission_mode,
        startup_agent: startup_agent.clone(),
        current_agent: startup_agent,
        current_agent_explicit_default: false,
        no_agents: args.no_agents,
        no_skills: args.no_skills,
        skill_inputs: args.skill.clone(),
        thinking_visible,
        raw_visible,
        clipboard: default_clipboard_sink(),
        renderer: TuiRenderer::new(color),
        debug: args.debug,
        had_error: false,
        last_context_snapshot: None,
        model_catalog: ModelCatalogCache::default(),
        clipboard_result_tx,
        clipboard_result_rx,
        clipboard_copies_in_flight: 0,
        slash_config,
        side_conversation: None,
        last_live_agent_reload_check: None,
        last_gateway_live_event_seq,
        gateway_live_snapshot_revisions: BTreeMap::new(),
        session_browser_limits: BTreeMap::new(),
        side_cleanup_task: None,
        side_delete_tasks: Vec::new(),
        compaction_task: None,
        diff_task: None,
        journey_profile,
    };
    if args.new_session {
        app.begin_new_session_draft();
    }
    app.start_missing_model_metadata_cache_warmup();
    app.refresh_selected_model();
    app.refresh_current_session_title().await?;
    app.refresh_current_session_agent().await?;
    let run_result = app.run(args.message.join(" ")).await;
    let shutdown_result = app.runtime.shutdown().await;
    let profile_result = app.journey_profile.finish();
    match (run_result, shutdown_result, profile_result) {
        (Err(err), _, _) => Err(err),
        (Ok(_), Err(err), _) => Err(err.into()),
        (Ok(_), Ok(_), Err(err)) => Err(err.into()),
        (Ok(exit_code), Ok(_), Ok(())) => Ok(exit_code),
    }
}

pub(crate) fn load_effective_tui_slash_config(
    framework: &FrameworkClient,
    env_map: &BTreeMap<String, String>,
    cwd: PathBuf,
) -> Result<EffectiveSlashConfig> {
    let mut query = ConfigurationQuery::new(cwd);
    query.inherited_env = Some(env_map.clone());
    let document = framework
        .configuration(query)?
        .config_value(ConfigScope::Effective)?;
    parse_effective_slash_config(&document["value"])
}

// Split into normal Rust modules while preserving the original TUI module surface.
#[path = "app/state.rs"]
pub(crate) mod app_state;
pub(crate) use app_state::{
    CompactionTask, SideCleanupTask, SideConversationState, SideDeleteTask, TuiApp,
};
#[path = "app/loop.rs"]
pub(crate) mod app_loop;
#[cfg(test)]
pub(crate) use app_loop::{
    FULLSCREEN_EVENT_POLL_INTERVAL, TUI_MOUSE_CAPTURE_DISABLE_ANSI, TUI_MOUSE_CAPTURE_ENABLE_ANSI,
    write_fullscreen_enter_commands, write_fullscreen_exit_commands,
};
pub(crate) use app_loop::{
    FULLSCREEN_PASSIVE_REDRAW_INTERVAL, FullscreenEventOutcome, FullscreenTerminalGuard,
};
#[cfg(test)]
pub(crate) use app_loop::{
    ManagedTerminalTitle, mouse_event_needs_redraw, passive_redraw_due,
    schedule_next_passive_redraw,
};
#[path = "app/bottom_panel.rs"]
pub(crate) mod app_bottom_panel;
#[cfg(test)]
pub(crate) use app_bottom_panel::agent_editor_markdown;
pub(crate) use app_bottom_panel::strip_dotenv_quotes;
#[path = "app/side.rs"]
pub(crate) mod app_side;
pub(crate) use app_side::RELOAD_CONTEXT_DEPRECATED_MESSAGE;
#[path = "app/commands.rs"]
pub(crate) mod app_commands;
pub(crate) use app_commands::SubmittedSlashInput;
#[path = "app/events.rs"]
pub(crate) mod app_events;
pub(crate) use app_events::tui_live_event_is_clarify_request;
#[path = "app/panels.rs"]
pub(crate) mod app_panels;
#[path = "app/status.rs"]
pub(crate) mod app_status;
pub(crate) use app_panels::{
    json_array_strings, json_i64, model_capability_tags, model_pricing_label, pluralize_count,
    string_values,
};
#[path = "app/session_state.rs"]
pub(crate) mod app_session_state;
pub(crate) use app_session_state::{latest_human_visible_session_id, session_project_label};
#[path = "support/running.rs"]
pub(crate) mod support_running;
pub(crate) use support_running::{
    AuxiliaryAgentTask, AuxiliaryShellTask, ForeignGatewayActivity, PendingAuxiliaryShellCommand,
    PendingImageAttachment, PendingSteerInput, PresentedShellEvent, QueuedInput, RunningCompletion,
    RunningTask, RunningTurn, RunningTurnControl, RunningTurnEvents, StartedTurn, StartingTurn,
    StartingTurnCleanup, TuiApprovalEvent, TuiApprovalHandler, TuiApprovalRequest, TuiLiveEvent,
    attachment_metadata_text, next_image_placeholder, presented_shell_event_channel,
    prompt_display_metadata, prompt_without_image_placeholders, queued_input_sequence,
    queued_input_session_id, queued_input_text, rebind_queued_input_session,
};
#[path = "support/journey_profile.rs"]
pub(crate) mod support_journey_profile;
pub(crate) use support_journey_profile::{TuiJourneyProfileProbe, TuiProfileFrameObservation};
#[path = "support/file_search.rs"]
pub(crate) mod support_file_search;
pub(crate) use support_file_search::{
    FileSearchMatch, FileSearchMatchKind, FileSearchState, FileToken,
};
#[cfg(test)]
pub(crate) use support_file_search::{FileSearchPopupState, FileSearchResult};
#[path = "support/agent_search.rs"]
pub(crate) mod support_agent_search;
#[cfg(test)]
pub(crate) use support_agent_search::AgentSearchPopupState;
pub(crate) use support_agent_search::{AgentSearchMatch, AgentSearchState, AgentToken};
#[path = "support/skill_search.rs"]
pub(crate) mod support_skill_search;
#[cfg(test)]
pub(crate) use support_skill_search::SkillSearchPopupState;
pub(crate) use support_skill_search::{SkillSearchMatch, SkillSearchState, SkillToken};
#[path = "support/model_catalog.rs"]
pub(crate) mod support_model_catalog;
pub(crate) use support_model_catalog::{
    ClipboardSink, ModelCatalogCache, ModelCatalogFetchResult, ModelCatalogStatus,
    ModelMetadataRefreshTask, ModelProviderCatalogState, TuiSessionDisplaySummary,
    push_model_metadata_target, push_raw_model_metadata_target,
};
#[path = "ui/types.rs"]
pub(crate) mod ui_types;
pub(crate) use ui_types::{
    AgentAction, AgentEditorField, AgentEditorMode, AgentEditorPanel, AgentPanel,
    AgentRunPromptPanel, AgentTab, BottomPanel, BottomRowStyle, BottomSelectionPanel,
    BottomSelectionRow, BottomSelectionValue, ClarifyInputMode, ClarifyPanel, ClarifyQuestionState,
    ComposerHistoryKind, DiffOverlay, FocusMode, FullscreenUi, HelpPanel, HelpTab,
    HistoryMessageAction, HistoryMessageEdit, ModelPanel, ModelRowSource, ModelTab,
    MouseWheelTarget, PendingInputAction, PendingInputEdit, PendingInputEntry, PendingInputKind,
    PendingInputRef, PermissionApprovalChoice, PermissionApprovalPanel, ProviderWizardField,
    ProviderWizardPanel, ScreenLine, SelectableRegion, SelectionState, SessionListView,
    SidebarSnapshot, TranscriptHitTarget, TranscriptKind, TranscriptLayoutBlock,
    TranscriptLayoutBlockKey, TranscriptLayoutCache, TranscriptLayoutRowKey, TranscriptRenderBlock,
    TranscriptRow, UiEphemeralStatus,
};
#[cfg(test)]
pub(crate) use ui_types::{
    TUI_ROLE_ACCENT, TUI_ROLE_DANGER, TUI_ROLE_DIM, TUI_ROLE_IDENTITY, TUI_ROLE_SELECTION_BG,
    TUI_ROLE_SURFACE_BG, TUI_ROLE_THINKING,
};
#[path = "support/terminal_probe.rs"]
pub(crate) mod support_terminal_probe;
#[cfg(test)]
pub(crate) use support_terminal_probe::parse_terminal_default_colors;
#[path = "support/theme.rs"]
pub(crate) mod support_theme;
#[cfg(test)]
pub(crate) use support_theme::{TerminalColorLevel, TerminalProfile};
pub(crate) use support_theme::{TuiTheme, text_selection_style, tui_theme};
#[path = "support/renderable.rs"]
pub(crate) mod support_renderable;
pub(crate) use support_renderable::{
    DisplayRow, DisplayRowTone, TuiRenderable, render_display_rows,
};
#[path = "support/motion.rs"]
pub(crate) mod support_motion;
pub(crate) use support_motion::activity_spinner_frame;
#[path = "support/markdown_render.rs"]
pub(crate) mod support_markdown_render;
pub(crate) use support_markdown_render::{highlight_code_line, render_markdown_lines};
#[path = "support/diff_render.rs"]
pub(crate) mod support_diff_render;
pub(crate) use support_diff_render::{diff_overlay_from_workspace_diff, render_inline_edit_diff};
#[path = "ui/fullscreen.rs"]
pub(crate) mod ui_fullscreen;
pub(crate) use ui_fullscreen::{
    TUI_TURN_START_TRANSCRIPT_SOURCE, agent_child_status_text, append_agent_child_live_fragment,
    apply_agent_child_value_preview, auxiliary_agent_live_for_session, bounded_stdin_display,
    clarify_request_args_value, current_session_matches, exec_result_completed,
    exec_result_running, exec_row_full_text_without_history_marker, exec_session_id_from_args,
    exec_session_id_from_result, refresh_agent_child_preview, selected_skill_names_from_event,
    set_exec_row_text, tool_result_output, with_exec_history_running_marker,
    write_stdin_non_empty_chars,
};
#[path = "support/history.rs"]
pub(crate) mod support_history;
pub(crate) use support_history::{
    HistoryToolCall, agent_notification_display, agent_notification_target,
    assistant_message_has_tool_calls, assistant_message_keeps_tool_calls_active,
    assistant_reasoning_from_message, assistant_text_from_message, default_title,
    history_meta_text, history_tool_calls_from_message, history_tool_started_instant,
    instant_from_wall_timestamp_ms, message_timestamp_ms, metadata_elapsed_duration,
    outcome_from_value, pending_input_id_from_message_end, reasoning_only_message_receives_meta,
    row_visible, tool_started_instant, user_display_from_item, user_text_from_item,
    visible_answer_message_receives_meta, wall_now_ms, wrapped_line_count,
};
#[cfg(test)]
pub(crate) use support_history::{transcript_line_count, visible_transcript_message_count};
#[path = "support/selection.rs"]
pub(crate) mod support_selection;
#[cfg(test)]
pub(crate) use support_selection::screen_cells_from_text;
pub(crate) use support_selection::{
    cell_overlaps_range, ordered_selection, screen_line_from_buffer, selected_text_from_lines,
};
#[path = "support/input.rs"]
pub(crate) mod support_input;
#[cfg(test)]
pub(crate) use support_input::slash_completion;
pub(crate) use support_input::{parse_shell_escape_input, slash_completion_with_items};
#[path = "support/evidence.rs"]
pub(crate) mod support_evidence;
pub(crate) use support_evidence::{
    StreamingToolCall, TurnMetaProjection, active_tool_row, active_tool_title,
    agent_child_latest_tokens, agent_relationship_title, agent_session_start_title,
    agent_target_from_tool_event, assistant_message_stream_event_type,
    background_running_agent_result, clarify_no_answer_result, completed_live_tool_elapsed,
    completed_tool_title_from_active, evidence_kind, evidence_kind_for_value, format_compact_count,
    format_count, format_duration_compact, format_nanodollars, format_tool_summary,
    matching_agent_relationship, model_meta_label, pluralize, running_agent_tool_full_text,
    scoped_tool_position_key, single_line_preview, streaming_tool_calls_from_event,
    tool_event_interrupted, tool_id_key, tool_output_text, tool_position_key,
    tool_result_output_text, tool_title, tool_title_as_invocation, tool_title_for_update,
    turn_meta_text, usage_context_tokens, usage_total_tokens, user_shell_title,
};
#[path = "support/sidebar.rs"]
pub(crate) mod support_sidebar;
pub(crate) use support_sidebar::{
    directory_display_value, format_directory_display_with_home, git_snapshot, home_dir_for_display,
};
#[path = "support/composer.rs"]
pub(crate) mod support_composer;
#[cfg(test)]
pub(crate) use support_composer::search_cwd_files;
#[cfg(test)]
pub(crate) use support_composer::textarea_with_lines_and_cursor;
pub(crate) use support_composer::{
    composer_cursor_from_point, composer_height, composer_marker_width,
    composer_terminal_cursor_position, current_agent_token, current_file_token,
    current_skill_token, fuzzy_subsequence_score, new_textarea, replace_current_agent_token,
    replace_current_file_token, replace_current_skill_token, textarea_text, textarea_with_text,
};
#[path = "render/transcript.rs"]
pub(crate) mod render_transcript;
pub(crate) use render_transcript::{
    DISPLAY_TOKEN_CHUNK_CELLS, DISPLAY_TOKEN_LONG_RUN_FREE_CELLS, ToolRowPhase,
    active_tool_elapsed, answer_lines, append_expandable_evidence_body, collapsed_more_line_count,
    display_token_count, display_token_count_segment, focus_marker_style, foldable_evidence_body,
    foldable_tool_title, interruption_style, is_agent_tool_row, label_style,
    ledger_body_collapse_policy, ledger_title_line, ledger_title_right_text, prompt_lines,
    refresh_transcript_layout, render_active_selection, render_transcript, row_expand_hint,
    style_for_body, suffix_display_tokens, suffix_display_width, thinking_lines,
    toggle_transcript_row_details, tool_display_title, tool_elapsed_label, tool_lines,
    tool_title_detail, transcript_layout_matches_current, transcript_render_blocks,
    transcript_total_height_for_ui, truncate_display_width, user_shell_lines, wrap_command_text,
};
#[cfg(test)]
pub(crate) use render_transcript::{
    LEDGER_BODY_COLLAPSE_HEAD_LINES, LEDGER_BODY_COLLAPSE_TAIL_LINES, LEDGER_BODY_COLLAPSE_TOKENS,
    LEDGER_BODY_COLLAPSE_WIDTH, status_lines, transcript_layout_row_key, transcript_lines,
};
#[path = "render/surfaces.rs"]
pub(crate) mod render_surfaces;
pub(crate) use render_surfaces::{
    bottom_panel_row, model_detail_capabilities, model_detail_modalities, model_detail_pricing,
    model_detail_source, pending_input_preview_height, render_bottom_panel,
    render_completion_popup, render_composer, render_diff_overlay, render_help_panel,
    render_pending_input_preview, render_provider_wizard_panel, render_sidebar, render_slash_menu,
    render_status,
};
#[cfg(test)]
pub(crate) use render_surfaces::{
    bottom_status_context_for_width, bottom_status_session_usage_segments, model_info_lines,
};
#[path = "render/helpers.rs"]
pub(crate) mod render_helpers;
pub(crate) use render_helpers::{
    format_session_date, format_session_time, rect_contains, short_fetch_error, short_session,
    sidebar_heading, truncate_chars,
};
#[path = "support/clipboard.rs"]
pub(crate) mod support_clipboard;
pub(crate) use support_clipboard::default_clipboard_sink;
#[cfg(test)]
pub(crate) use support_clipboard::{
    ClipboardCommand, ClipboardEnvironment, NO_ARGS, base64_encode, copy_text_to_clipboard_with,
    is_probably_wsl_from, local_clipboard_commands_for, osc52_sequence_with_passthrough,
    tmux_clipboard_copy_ready,
};
#[path = "support/formatting.rs"]
pub(crate) mod support_formatting;
#[cfg(test)]
pub(crate) use support_formatting::resolve_session_ref_from_summaries;
pub(crate) use support_formatting::{
    configured_model_display_label, decrement_row_index, format_model_spec, increment_row_index,
    variant_description,
};
#[path = "support/turn_printer.rs"]
pub(crate) mod support_turn_printer;
pub(crate) use support_turn_printer::TurnPrinter;
#[path = "support/turn_event.rs"]
pub(crate) mod support_turn_event;
