use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, ExitCode, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow};
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event as CrosstermEvent, KeyCode, KeyEvent,
    KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use psychevo_ai::Outcome;
use psychevo_runtime::{
    ConfiguredModel, ContextFormatOptions, ContextOptions, ContextSnapshot, CustomProviderInput,
    ImageInput, ModelCatalogEntry, ModelCatalogProvider, ModelMetadataCacheTarget,
    PromptAttachmentDisplay, PromptDisplayMetadata, RunControlHandle, RunMode, RunOptions,
    RunStreamEvent, RunStreamSink, SessionSummary, SessionUndoOptions, SkillCatalog,
    SkillDiscoveryOptions, SqliteStore, StatsOptions, TUI_DISPLAY_METADATA_KEY, TuiMessageSummary,
    UserShellOptions, canonicalize_workdir, configured_models, context_snapshot,
    create_global_custom_provider, custom_provider_api_key_env, discover_skills,
    fetch_model_catalog, format_context_snapshot_text_with_options, format_context_total_value,
    format_context_total_value_parts, model_catalog_providers,
    model_metadata_explicitly_disallows_image_input, normalize_context_bar_width,
    prompt_starts_with_supported_image_path, redo_session, refresh_model_metadata_cache,
    resolve_image_source, run_control, run_live_streaming, run_live_streaming_controlled,
    run_user_shell_command_streaming_controlled, selected_configured_model, undo_session,
    usage_stats,
};
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui_textarea::{CursorMove, TextArea, WrapMode};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

mod plain;
mod slash;
mod state;

#[cfg(test)]
mod tests;

use self::plain::{
    TuiRenderer, assistant_text_from_event, format_session_line, format_tool_summary,
};
use self::slash::{
    SlashCommand, SlashMenuItem, VARIANTS, base_slash_menu_items, format_slash_help,
    parse_slash_command, slash_help_sections, slash_menu_items_from, slash_prefix_menu_items_from,
    validate_model_spec, validate_variant,
};
use self::state::TuiState;
use crate::args::TuiArgs;
use crate::env::{
    ensure_home_initialized, env_path, env_value, inherited_env, resolve_explicit_path,
    resolve_psychevo_home, resolve_state_db,
};

const TUI_SESSION_SOURCES: &[&str] = &["run", "tui"];
const USER_SHELL_HELP: &str = "shell mode: type !<command> to run a local shell command";
const FILE_POPUP_MAX_ROWS: usize = 8;

pub(crate) async fn run_tui_command(args: &TuiArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = std::env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    ensure_home_initialized(&home)?;
    let config_path = env_path("PSYCHEVO_CONFIG", &env_map, &cwd)?;
    let db_path = resolve_state_db(&env_map, &home, &cwd)?;
    let workdir = match &args.dir {
        Some(dir) => resolve_explicit_path(dir, &env_map, &cwd)?,
        None => cwd,
    };
    let workdir = canonicalize_workdir(&workdir)?;
    let workdir_key = workdir.to_string_lossy().to_string();
    let state_path = home.join("tui-state.json");
    let state = TuiState::load(&state_path)?;
    let current_model = args.model.clone().or_else(|| state.model_for(&workdir_key));
    let current_variant = args
        .variant
        .map(|variant| variant.as_str().to_string())
        .or_else(|| state.variant_for(&workdir_key));
    let current_mode = state
        .mode_for(&workdir_key)
        .and_then(|value| RunMode::parse(&value))
        .unwrap_or_default();
    let thinking_visible = state.thinking_visible;
    let raw_visible = state.raw_visible;
    let current_session = if let Some(session) = &args.session {
        Some(session.clone())
    } else if args.new_session {
        None
    } else {
        SqliteStore::open(&db_path)?
            .latest_session_for_workdir_with_sources(&workdir, TUI_SESSION_SOURCES)?
    };

    let color = io::stdout().is_terminal() && env_value("NO_COLOR", &env_map).is_none();
    let (clipboard_result_tx, clipboard_result_rx) = std::sync::mpsc::channel();
    let mut app = TuiApp {
        env_map,
        home,
        state_path,
        state,
        db_path,
        config_path,
        workdir,
        workdir_key,
        current_session,
        current_session_title: None,
        force_new_once: args.new_session,
        current_model,
        current_variant,
        selected_model: None,
        current_mode,
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
    };
    app.start_missing_model_metadata_cache_warmup();
    app.refresh_selected_model();
    app.refresh_current_session_title()?;
    app.run(args.message.join(" ")).await
}

// Kept as same-module includes for the first behavior-preserving split.
include!("app/state.rs");
include!("app/loop.rs");
include!("app/bottom_panel.rs");
include!("app/commands.rs");
include!("app/events.rs");
include!("app/status.rs");
include!("app/panels.rs");
include!("app/session_state.rs");
include!("support/running.rs");
include!("support/file_search.rs");
include!("support/skill_search.rs");
include!("support/model_catalog.rs");
include!("ui/types.rs");
include!("support/terminal_probe.rs");
include!("support/theme.rs");
include!("support/motion.rs");
include!("support/markdown_render.rs");
include!("ui/fullscreen.rs");
include!("support/history.rs");
include!("support/selection.rs");
include!("support/input.rs");
include!("support/evidence.rs");
include!("support/sidebar.rs");
include!("support/composer.rs");
include!("render/transcript.rs");
include!("render/surfaces.rs");
include!("render/helpers.rs");
include!("support/clipboard.rs");
include!("support/formatting.rs");
include!("support/turn_printer.rs");
