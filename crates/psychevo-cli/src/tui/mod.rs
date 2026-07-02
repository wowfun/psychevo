pub(crate) use std::collections::{BTreeMap, BTreeSet, VecDeque};
pub(crate) use std::fs;
pub(crate) use std::io::{self, BufRead, IsTerminal, Write};
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::process::{Command as StdCommand, ExitCode, Stdio};
pub(crate) use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
pub(crate) use std::sync::{Arc, Mutex};
pub(crate) use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) use crate::provider_setup::{
    ProviderSetupPresetId, default_provider_setup_api_key_env, is_loopback_base_url,
    looks_like_api_key, provider_setup_preset, provider_setup_presets, upsert_provider_options,
    validate_api_key_env, validate_base_url,
};
pub(crate) use anyhow::{Result, anyhow};
pub(crate) use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event as CrosstermEvent, KeyCode, KeyEvent,
    KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
pub(crate) use crossterm::execute;
pub(crate) use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
pub(crate) use psychevo_ai::Outcome;
pub(crate) use psychevo_gateway::{
    Gateway, GatewayActionKind, GatewayActionOutcome, GatewayActivity, GatewayEvent,
    GatewayEventSink, GatewaySource, GatewayThreadSelector, GatewayTurnStatus, SendTurnRequest,
    TranscriptBlock, TranscriptBlockKind, TranscriptBlockStatus, TranscriptEntry,
    TranscriptEntryRole,
};
pub(crate) use psychevo_runtime::{
    AgentCatalog, AgentDiscoveryOptions, AgentEdgeRecord, AgentEntrypoint, AgentSource,
    AgentSpawnOptions, AutoCompactionCheckOptions, ChildSessionSnapshotInput, ClarifyAnswer,
    ClarifyQuestion, ClarifyRequestEvent, ClarifyResolvedEvent, ClarifyResolvedReason,
    ClarifyResponse, ClarifyResult, CompactSessionOptions, CompactionReason, CompactionResult,
    ConfigScope, ConfiguredModel, ContextFormatOptions, ContextOptions, ContextSnapshot,
    GatewayLiveEventRecord, GatewayLiveSnapshotRecord, ImageInput, InstallOptions, LoadedMainAgent,
    MAX_AGENT_SPAWN_DEPTH_CAP, ModelCatalogEntry, ModelCatalogProvider, ModelMetadataCacheTarget,
    ModelState, PendingInputId, PermissionApprovalDecision, PermissionApprovalOutcome,
    PermissionApprovalRequest, PermissionMode, PromptAttachmentDisplay, PromptDisplayMetadata,
    ReloadContextOptions, RunControlHandle, RunMode, RunOptions, RunStreamEvent, RunStreamSink,
    SESSION_COMPOSER_MODEL_METADATA_KEY, SESSION_MAIN_AGENT_METADATA_KEY,
    SIDE_CONVERSATION_METADATA_KEY, SIDE_INHERITED_METADATA_KEY, ScopedCustomProviderInput,
    SessionArtifactKind, SessionExportFormat, SessionExportOptions, SessionExportWriteResult,
    SessionSummary, SessionUndoOptions, SessionUsageOptions, SessionUsageSummary, SkillBundle,
    SkillCatalog, SkillDiscoveryOptions, SkillTarget, SqliteStore, StateRuntime, StatsOptions,
    TUI_DISPLAY_METADATA_KEY, TUI_SIDE_CONVERSATION_SESSION_SOURCE, TerminalReason,
    ToolDisplayBodyPolicy, ToolDisplayCategory, ToolDisplaySpec, TuiMessageSummary,
    USER_SHELL_METADATA_KEY, UserShellContextOptions, UserShellOptions, WorkspaceDiff,
    agent_spawn_paused, agent_status_value, auto_compaction_due_for_snapshot, canonicalize_cwd,
    collect_workspace_diff, compact_session, config_show_value, configured_models,
    context_snapshot, create_scoped_custom_provider, custom_provider_api_key_env,
    default_session_export_filename, discover_agents, discover_skills,
    fetch_and_cache_model_catalog, format_context_snapshot_text_with_options,
    format_context_total_value, format_context_total_value_parts, install_skill,
    list_skill_bundles, main_agent_default_metadata, main_agent_from_session_metadata,
    main_agent_metadata, model_catalog_providers, model_metadata_explicitly_disallows_image_input,
    normalize_context_bar_width, normalize_reasoning_effort, permission_rules_value,
    prompt_message_from_inputs_with_options, prompt_starts_with_supported_image_path,
    read_cached_model_catalog, redo_session, refresh_model_metadata_cache, reload_session_context,
    remove_installed_skill, resolve_agent_definition, resolve_image_source, run_control,
    run_user_shell_command_streaming_controlled, scan_skill_path, selected_configured_model,
    session_base_agent_name_from_metadata, session_usage_summary, set_agent_spawn_paused,
    set_default_model_with_reasoning, set_local_toolset_enabled, set_provider_api_key,
    set_skill_config_value, set_skill_enabled, side_conversation_boundary_prompt,
    side_inherited_metadata_hidden, spawn_agent_background, stop_agent_id_with_grace,
    toolsets_value, undo_session, usage_stats, view_skill_value, write_session_export,
};
pub(crate) use ratatui::Frame;
pub(crate) use ratatui::Terminal;
pub(crate) use ratatui::backend::CrosstermBackend;
pub(crate) use ratatui::layout::{Constraint, Direction, Layout, Rect};
pub(crate) use ratatui::style::{Color, Modifier, Style};
pub(crate) use ratatui::text::{Line, Span, Text};
pub(crate) use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};
pub(crate) use ratatui_textarea::{CursorMove, TextArea, WrapMode};
pub(crate) use serde_json::Value;
pub(crate) use tokio::sync::{mpsc, oneshot};
pub(crate) use tokio::task::JoinHandle;
pub(crate) use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub(crate) mod plain;
pub(crate) mod slash;
pub(crate) mod state;

#[cfg(test)]
pub(crate) mod tests;

use self::plain::{TuiRenderer, assistant_text_from_event, format_session_line};
use self::slash::{
    EffectiveSlashConfig, SlashCommand, SlashHelpSections, SlashMenuItem, SlashShortcutMatch,
    TuiSlashParse, VARIANTS, configured_slash_menu_items, format_slash_help_with_config,
    parse_effective_slash_config, parse_slash_command_with_config, parse_tui_slash_with_config,
    slash_help_sections_with_config, slash_menu_items_from, slash_prefix_menu_items_from,
    validate_model_spec, validate_variant,
};
use self::state::TuiState;
pub(crate) use crate::args::TuiArgs;
pub(crate) use crate::env::{
    ensure_home_initialized, env_path, env_value, inherited_env, resolve_explicit_path,
    resolve_psychevo_home, resolve_state_db,
};

pub(crate) const TUI_CONTINUE_SESSION_SOURCES: &[&str] = &["run", "tui"];
pub(crate) const TUI_INTERNAL_SESSION_SOURCES: &[&str] = &[TUI_SIDE_CONVERSATION_SESSION_SOURCE];
pub(crate) const USER_SHELL_HELP: &str = "shell mode: type !<command> to run a local shell command";
pub(crate) const FILE_POPUP_MAX_ROWS: usize = 8;

pub(crate) async fn run_tui_command(args: &TuiArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = std::env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    ensure_home_initialized(&home)?;
    let config_path = env_path("PSYCHEVO_CONFIG", &env_map, &cwd)?;
    let db_path = resolve_state_db(&env_map, &home, &cwd)?;
    let state_runtime = StateRuntime::open(&db_path)?;
    let cwd = match &args.dir {
        Some(dir) => resolve_explicit_path(dir, &env_map, &cwd)?,
        None => cwd,
    };
    let cwd = canonicalize_cwd(&cwd)?;
    let slash_config = load_effective_tui_slash_config(
        &env_map,
        state_runtime.clone(),
        cwd.clone(),
        config_path.clone(),
    )?;
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
        latest_human_visible_session_id(state_runtime.store())?
    };

    let color = io::stdout().is_terminal() && env_value("NO_COLOR", &env_map).is_none();
    let (clipboard_result_tx, clipboard_result_rx) = std::sync::mpsc::channel();
    let gateway = Gateway::new(state_runtime.clone());
    let last_gateway_live_event_seq = state_runtime
        .store()
        .latest_gateway_live_event_seq()
        .unwrap_or_default();
    let mut app = TuiApp {
        env_map,
        home,
        state_path,
        state,
        model_state_path,
        model_state,
        state_runtime,
        gateway,
        db_path,
        config_path,
        cwd,
        cwd_key,
        current_session,
        current_session_title: None,
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
        compaction_task: None,
        diff_task: None,
    };
    if args.new_session {
        app.begin_new_session_draft();
    }
    app.start_missing_model_metadata_cache_warmup();
    app.refresh_selected_model();
    app.refresh_current_session_title()?;
    app.refresh_current_session_agent()?;
    app.run(args.message.join(" ")).await
}

pub(crate) fn load_effective_tui_slash_config(
    env_map: &BTreeMap<String, String>,
    state: StateRuntime,
    cwd: PathBuf,
    config_path: Option<PathBuf>,
) -> Result<EffectiveSlashConfig> {
    let options = RunOptions {
        state,
        cwd,
        snapshot_root: None,
        session: None,
        continue_latest: false,
        prompt: String::new(),
        image_inputs: Vec::new(),
        extract_prompt_image_sources: false,
        prompt_display: None,
        max_context_messages: None,
        config_path,
        project_context_override: None,
        sandbox_override: None,
        model: None,
        reasoning_effort: None,
        runtime_ref: None,
        runtime_session_id: None,
        runtime_options: BTreeMap::new(),
        external_agent_delegate: None,
        include_reasoning: false,
        mode: RunMode::Default,
        permission_mode: None,
        approval_mode: None,
        approval_handler: None,
        clarify_enabled: false,
        inherited_env: Some(env_map.clone()),
        agent: None,
        no_agents: false,
        no_skills: false,
        selected_capability_roots: Vec::new(),
        skill_inputs: Vec::new(),
        mcp_servers: Vec::new(),
        runtime_tools: Vec::new(),
    };
    let document = config_show_value(&options, ConfigScope::Effective)?;
    parse_effective_slash_config(&document["value"])
}

// Split into normal Rust modules while preserving the original TUI module surface.
#[path = "app/state.rs"]
pub(crate) mod app_state;
#[allow(unused_imports)]
use app_state::*;
#[path = "app/loop.rs"]
pub(crate) mod app_loop;
#[allow(unused_imports)]
use app_loop::*;
#[path = "app/bottom_panel.rs"]
pub(crate) mod app_bottom_panel;
#[allow(unused_imports)]
use app_bottom_panel::*;
#[path = "app/side.rs"]
pub(crate) mod app_side;
#[allow(unused_imports)]
use app_side::*;
#[path = "app/commands.rs"]
pub(crate) mod app_commands;
#[allow(unused_imports)]
use app_commands::*;
#[path = "app/events.rs"]
pub(crate) mod app_events;
#[allow(unused_imports)]
use app_events::*;
#[path = "app/status.rs"]
pub(crate) mod app_status;
#[allow(unused_imports)]
use app_status::*;
#[path = "app/panels.rs"]
pub(crate) mod app_panels;
#[allow(unused_imports)]
use app_panels::*;
#[path = "app/session_state.rs"]
pub(crate) mod app_session_state;
#[allow(unused_imports)]
use app_session_state::*;
#[path = "support/running.rs"]
pub(crate) mod support_running;
#[allow(unused_imports)]
use support_running::*;
#[path = "support/file_search.rs"]
pub(crate) mod support_file_search;
#[allow(unused_imports)]
use support_file_search::*;
#[path = "support/agent_search.rs"]
pub(crate) mod support_agent_search;
#[allow(unused_imports)]
use support_agent_search::*;
#[path = "support/skill_search.rs"]
pub(crate) mod support_skill_search;
#[allow(unused_imports)]
use support_skill_search::*;
#[path = "support/model_catalog.rs"]
pub(crate) mod support_model_catalog;
#[allow(unused_imports)]
use support_model_catalog::*;
#[path = "ui/types.rs"]
pub(crate) mod ui_types;
#[allow(unused_imports)]
use ui_types::*;
#[path = "support/terminal_probe.rs"]
pub(crate) mod support_terminal_probe;
#[allow(unused_imports)]
use support_terminal_probe::*;
#[path = "support/theme.rs"]
pub(crate) mod support_theme;
#[allow(unused_imports)]
use support_theme::*;
#[path = "support/renderable.rs"]
pub(crate) mod support_renderable;
#[allow(unused_imports)]
use support_renderable::*;
#[path = "support/motion.rs"]
pub(crate) mod support_motion;
#[allow(unused_imports)]
use support_motion::*;
#[path = "support/markdown_render.rs"]
pub(crate) mod support_markdown_render;
#[allow(unused_imports)]
use support_markdown_render::*;
#[path = "support/diff_render.rs"]
pub(crate) mod support_diff_render;
#[allow(unused_imports)]
use support_diff_render::*;
#[path = "ui/fullscreen.rs"]
pub(crate) mod ui_fullscreen;
#[allow(unused_imports)]
use ui_fullscreen::*;
#[path = "support/history.rs"]
pub(crate) mod support_history;
#[allow(unused_imports)]
use support_history::*;
#[path = "support/selection.rs"]
pub(crate) mod support_selection;
#[allow(unused_imports)]
use support_selection::*;
#[path = "support/input.rs"]
pub(crate) mod support_input;
#[allow(unused_imports)]
use support_input::*;
#[path = "support/evidence.rs"]
pub(crate) mod support_evidence;
#[allow(unused_imports)]
use support_evidence::*;
#[path = "support/sidebar.rs"]
pub(crate) mod support_sidebar;
#[allow(unused_imports)]
use support_sidebar::*;
#[path = "support/composer.rs"]
pub(crate) mod support_composer;
#[allow(unused_imports)]
use support_composer::*;
#[path = "render/transcript.rs"]
pub(crate) mod render_transcript;
#[allow(unused_imports)]
use render_transcript::*;
#[path = "render/surfaces.rs"]
pub(crate) mod render_surfaces;
#[allow(unused_imports)]
use render_surfaces::*;
#[path = "render/helpers.rs"]
pub(crate) mod render_helpers;
#[allow(unused_imports)]
use render_helpers::*;
#[path = "support/clipboard.rs"]
pub(crate) mod support_clipboard;
#[allow(unused_imports)]
use support_clipboard::*;
#[path = "support/formatting.rs"]
pub(crate) mod support_formatting;
#[allow(unused_imports)]
use support_formatting::*;
#[path = "support/turn_printer.rs"]
pub(crate) mod support_turn_printer;
#[allow(unused_imports)]
use support_turn_printer::*;
