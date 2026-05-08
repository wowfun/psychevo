use std::collections::BTreeMap;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::{Command as StdCommand, ExitCode, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event as CrosstermEvent, KeyCode, KeyEvent,
    KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use psychevo_ai::Outcome;
use psychevo_runtime::{
    ConfiguredModel, ModelCatalogEntry, ModelCatalogProvider, RunControlHandle, RunMode,
    RunOptions, RunStreamEvent, RunStreamSink, SessionSummary, SessionUndoOptions, SqliteStore,
    TuiMessageSummary, canonicalize_workdir, configured_models, fetch_model_catalog,
    model_catalog_providers, redo_session, run_control, run_live_streaming,
    run_live_streaming_controlled, selected_configured_model, undo_session,
};
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
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
use self::slash::slash_menu_items;
use self::slash::{
    SlashCommand, VARIANTS, parse_slash_command, validate_model_spec, validate_variant,
};
use self::state::TuiState;
use crate::args::TuiArgs;
use crate::env::{
    ensure_home_initialized, env_path, env_value, inherited_env, resolve_explicit_path,
    resolve_psychevo_home, resolve_state_db,
};

const TUI_SESSION_SOURCES: &[&str] = &["run", "tui"];

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
    let current_session = if let Some(session) = &args.session {
        Some(session.clone())
    } else if args.new_session {
        None
    } else {
        SqliteStore::open(&db_path)?
            .latest_session_for_workdir_with_sources(&workdir, TUI_SESSION_SOURCES)?
    };

    let color = io::stdout().is_terminal() && env_value("NO_COLOR", &env_map).is_none();
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
        thinking_visible,
        clipboard: default_clipboard_sink(),
        renderer: TuiRenderer::new(color),
        debug: args.debug,
        had_error: false,
        model_catalog: ModelCatalogCache::default(),
    };
    app.refresh_selected_model();
    app.refresh_current_session_title()?;
    app.run(args.message.join(" ")).await
}

struct TuiApp {
    env_map: BTreeMap<String, String>,
    home: PathBuf,
    state_path: PathBuf,
    state: TuiState,
    db_path: PathBuf,
    config_path: Option<PathBuf>,
    workdir: PathBuf,
    workdir_key: String,
    current_session: Option<String>,
    current_session_title: Option<String>,
    force_new_once: bool,
    current_model: Option<String>,
    current_variant: Option<String>,
    selected_model: Option<ConfiguredModel>,
    current_mode: RunMode,
    thinking_visible: bool,
    clipboard: ClipboardSink,
    renderer: TuiRenderer,
    debug: bool,
    had_error: bool,
    model_catalog: ModelCatalogCache,
}

impl TuiApp {
    async fn run(&mut self, initial_prompt: String) -> Result<ExitCode> {
        let interactive = io::stdin().is_terminal() && io::stdout().is_terminal();
        if interactive {
            self.run_fullscreen_loop(initial_prompt).await?;
        } else {
            if !initial_prompt.trim().is_empty() {
                self.submit_prompt(initial_prompt).await?;
            }
            self.run_scripted_loop().await?;
        }

        Ok(if self.had_error {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        })
    }

    async fn run_fullscreen_loop(&mut self, initial_prompt: String) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        let result = self
            .run_fullscreen_loop_inner(&mut terminal, initial_prompt)
            .await;
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            DisableMouseCapture,
            LeaveAlternateScreen
        )?;
        terminal.show_cursor()?;
        result
    }

    async fn run_fullscreen_loop_inner(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        initial_prompt: String,
    ) -> Result<()> {
        let mut ui = FullscreenUi::new(self);
        self.load_current_session_history(&mut ui)?;
        if !initial_prompt.trim().is_empty() {
            self.start_fullscreen_turn(&mut ui, initial_prompt)?;
        }
        loop {
            self.drain_fullscreen_events(&mut ui).await?;
            if ui.take_terminal_clear_request() {
                terminal.clear()?;
            }
            terminal.draw(|frame| self.render_fullscreen(frame, &mut ui))?;
            if ui.quit_requested && ui.running.is_none() {
                break;
            }
            if event::poll(Duration::from_millis(40))? {
                match event::read()? {
                    CrosstermEvent::Key(key) => {
                        if key.kind == KeyEventKind::Release {
                            continue;
                        }
                        if self.handle_fullscreen_key(&mut ui, key).await? {
                            break;
                        }
                    }
                    CrosstermEvent::Mouse(mouse) => {
                        if self.handle_fullscreen_mouse(&mut ui, mouse).await? {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
        if let Some(running) = ui.running.take() {
            running.control.abort();
            let _ = running.task.await;
        }
        self.model_catalog.abort_unfinished();
        Ok(())
    }

    async fn run_scripted_loop(&mut self) -> Result<()> {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if self.handle_line(&line).await? {
                break;
            }
        }
        Ok(())
    }

    async fn handle_fullscreen_key(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        key: KeyEvent,
    ) -> Result<bool> {
        if key.code == KeyCode::Char('c')
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && self.copy_selected_text(ui)?
        {
            return Ok(false);
        }
        if key.code == KeyCode::Esc && ui.selection.anchor.is_some() {
            ui.clear_selection();
            return Ok(false);
        }
        if ui.bottom_panel.is_some() {
            return self.handle_bottom_panel_key(ui, key);
        }
        if ui.history_search {
            return self.handle_history_search_key(ui, key);
        }
        if ui.focus == FocusMode::Transcript {
            match key.code {
                KeyCode::Esc => {
                    ui.focus = FocusMode::Composer;
                }
                KeyCode::Up => ui.move_selection(-1),
                KeyCode::Down => ui.move_selection(1),
                KeyCode::Enter | KeyCode::Char(' ') => ui.toggle_selected(),
                KeyCode::PageUp => ui.scroll_transcript(-6),
                KeyCode::PageDown => ui.scroll_transcript(6),
                _ => {}
            }
            return Ok(false);
        }
        let slash_count = slash_menu_items(&textarea_text(&ui.textarea)).len();
        if slash_count > 0 {
            match key.code {
                KeyCode::Up => {
                    ui.move_slash_menu_selection(-1, slash_count);
                    return Ok(false);
                }
                KeyCode::Down => {
                    ui.move_slash_menu_selection(1, slash_count);
                    return Ok(false);
                }
                KeyCode::Home => {
                    ui.set_slash_menu_selection(0, slash_count);
                    return Ok(false);
                }
                KeyCode::End => {
                    ui.set_slash_menu_selection(slash_count.saturating_sub(1), slash_count);
                    return Ok(false);
                }
                _ => {}
            }
        }
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if ui.running.is_some() {
                    ui.push_status("press Ctrl+C again to quit after the running turn");
                    ui.quit_requested = true;
                    return Ok(false);
                }
                return Ok(true);
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(true);
            }
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ui.history_search = true;
                ui.history_query.clear();
                ui.push_status("history search");
            }
            KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ui.focus = FocusMode::Transcript;
                ui.auto_follow_transcript = false;
                ui.ensure_selection();
            }
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let visible = !ui.sidebar_enabled();
                self.set_sidebar_visible_no_print(visible)?;
                ui.sidebar_forced = visible;
                ui.sidebar_hidden = !visible;
                ui.refresh_sidebar(self);
            }
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ui.textarea.insert_newline();
            }
            KeyCode::Enter if is_newline_key(key) => {
                ui.textarea.insert_newline();
            }
            KeyCode::Enter => {
                let line = textarea_text(&ui.textarea);
                if line.trim().is_empty() {
                    return Ok(false);
                }
                let submitted = selected_slash_menu_command(&line, ui.slash_menu_selected)
                    .map(str::to_string)
                    .unwrap_or_else(|| line.clone());
                ui.textarea = new_textarea();
                ui.slash_menu_selected = 0;
                ui.push_submitted_history(submitted.clone());
                match parse_slash_command(&submitted) {
                    Ok(Some(command)) => return self.handle_fullscreen_command(ui, command).await,
                    Ok(None) => {}
                    Err(err) => {
                        ui.push_error(format!("error: {err:#}"));
                        return Ok(false);
                    }
                }
                self.start_fullscreen_turn(ui, submitted)?;
            }
            KeyCode::BackTab => {
                self.cycle_mode(ui)?;
            }
            KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.cycle_mode(ui)?;
            }
            KeyCode::Tab => {
                ui.complete_slash_command();
            }
            KeyCode::Esc => {
                if let Some(running) = &ui.running {
                    running.control.abort();
                    ui.push_error("interrupt requested");
                }
            }
            KeyCode::PageUp => ui.scroll_transcript(-6),
            KeyCode::PageDown => ui.scroll_transcript(6),
            KeyCode::Up if ui.can_recall_history_previous() => {
                ui.recall_history(-1);
            }
            KeyCode::Down if ui.can_recall_history_next() => {
                ui.recall_history(1);
            }
            _ => {
                ui.clear_history_navigation_for_edit();
                ui.textarea.input(key);
            }
        }
        Ok(false)
    }

    async fn handle_fullscreen_mouse(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        mouse: MouseEvent,
    ) -> Result<bool> {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if let Some(panel) = &mut ui.bottom_panel {
                    panel.selection_mut().move_selection(-3);
                } else {
                    ui.scroll_transcript(-3);
                }
            }
            MouseEventKind::ScrollDown => {
                if let Some(panel) = &mut ui.bottom_panel {
                    panel.selection_mut().move_selection(3);
                } else {
                    ui.scroll_transcript(3);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(index) = ui.bottom_panel_hit(mouse.column, mouse.row) {
                    ui.clear_selection();
                    if let Some(panel) = &mut ui.bottom_panel {
                        panel.selection_mut().set_selected(index);
                    }
                    let selected = ui
                        .bottom_panel
                        .as_ref()
                        .and_then(BottomPanel::selected_value);
                    self.apply_bottom_panel_selection(ui, selected)?;
                } else if let Some(index) = ui.slash_menu_hit(mouse.column, mouse.row) {
                    ui.clear_selection();
                    let line = textarea_text(&ui.textarea);
                    ui.set_slash_menu_selection(index, slash_menu_items(&line).len());
                    if let Some(command) =
                        selected_slash_menu_command(&line, ui.slash_menu_selected)
                    {
                        let submitted = command.to_string();
                        ui.textarea = new_textarea();
                        ui.slash_menu_selected = 0;
                        ui.push_submitted_history(submitted.clone());
                        match parse_slash_command(&submitted) {
                            Ok(Some(command)) => {
                                return self.handle_fullscreen_command(ui, command).await;
                            }
                            Ok(None) => {}
                            Err(err) => {
                                ui.push_error(format!("error: {err:#}"));
                                return Ok(false);
                            }
                        }
                    }
                } else if ui.selectable_hit(mouse.column, mouse.row) {
                    ui.start_selection(mouse.column, mouse.row);
                } else {
                    ui.clear_selection();
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                ui.update_selection(mouse.column, mouse.row);
            }
            MouseEventKind::Up(MouseButton::Left) => {
                ui.update_selection(mouse.column, mouse.row);
                if !self.copy_selected_text(ui)? {
                    ui.clear_selection();
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_bottom_panel_key(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        key: KeyEvent,
    ) -> Result<bool> {
        if ui.bottom_panel.is_none() {
            return Ok(false);
        }
        match key.code {
            KeyCode::Esc => {
                if let Some(BottomPanel::Variants { models, .. }) = ui.bottom_panel.take() {
                    ui.bottom_panel = Some(BottomPanel::Models(*models));
                } else {
                    if matches!(ui.bottom_panel, Some(BottomPanel::Models(_))) {
                        self.model_catalog.abort_unfinished();
                    }
                    ui.bottom_panel = None;
                }
            }
            KeyCode::Enter => {
                let selected = ui
                    .bottom_panel
                    .as_ref()
                    .and_then(BottomPanel::selected_value);
                self.apply_bottom_panel_selection(ui, selected)?;
            }
            KeyCode::Up => {
                if let Some(panel) = &mut ui.bottom_panel {
                    panel.move_selection(-1);
                }
            }
            KeyCode::Down => {
                if let Some(panel) = &mut ui.bottom_panel {
                    panel.move_selection(1);
                }
            }
            KeyCode::PageUp => {
                if let Some(panel) = &mut ui.bottom_panel {
                    panel.selection_mut().move_selection(-8);
                }
            }
            KeyCode::PageDown => {
                if let Some(panel) = &mut ui.bottom_panel {
                    panel.selection_mut().move_selection(8);
                }
            }
            KeyCode::Home => {
                if let Some(panel) = &mut ui.bottom_panel {
                    panel.selection_mut().move_to(0);
                }
            }
            KeyCode::End => {
                if let Some(panel) = &mut ui.bottom_panel {
                    let len = panel.selection().filtered_indices().len();
                    panel.selection_mut().move_to(len.saturating_sub(1));
                }
            }
            KeyCode::Backspace => {
                if let Some(panel) = &mut ui.bottom_panel {
                    panel.selection_mut().backspace_query();
                }
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if let Some(panel) = &mut ui.bottom_panel {
                    panel.selection_mut().set_query_char(c);
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn apply_bottom_panel_selection(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        selected: Option<BottomSelectionValue>,
    ) -> Result<()> {
        match selected {
            Some(BottomSelectionValue::Session(session_id)) => {
                self.switch_session_no_print(&session_id)?;
                ui.bottom_panel = None;
                ui.clear_transcript();
                self.load_current_session_history(ui)?;
                ui.refresh_sidebar(self);
            }
            Some(BottomSelectionValue::FetchAllModels) => {
                self.start_model_catalog_fetch_all(ui)?;
            }
            Some(BottomSelectionValue::FetchProvider(provider)) => {
                self.start_model_catalog_fetch_provider(ui, &provider)?;
            }
            Some(BottomSelectionValue::ProviderInfo(provider)) => {
                let message = if provider == "all" {
                    if self.model_catalog.providers.is_empty() {
                        "no configured providers".to_string()
                    } else if self.model_catalog.any_fetching() {
                        "already fetching".to_string()
                    } else {
                        "no fetchable providers".to_string()
                    }
                } else {
                    self.model_catalog
                        .providers
                        .get(&provider)
                        .map(|state| self.provider_status_text(state))
                        .unwrap_or_else(|| "provider unavailable".to_string())
                };
                ui.set_bottom_panel_notice(message);
            }
            Some(BottomSelectionValue::Model { model, source }) => {
                self.model_catalog.abort_unfinished();
                if let Some(BottomPanel::Models(models)) = ui.bottom_panel.take() {
                    ui.bottom_panel = Some(self.variant_panel(model, source, models));
                }
            }
            Some(BottomSelectionValue::Variant { model, variant }) => {
                self.set_model_and_variant_no_print(model.clone(), variant.clone())?;
                ui.bottom_panel = None;
                ui.push_status(format!(
                    "model: {model}  variant: {}",
                    variant.as_deref().unwrap_or("config default")
                ));
                ui.refresh_sidebar(self);
            }
            None => {}
        }
        Ok(())
    }

    fn start_model_catalog_fetch_all(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        if self.model_catalog.any_fetching() {
            ui.set_bottom_panel_notice("already fetching");
            return Ok(());
        }
        let providers = self
            .model_catalog_provider_order()
            .into_iter()
            .filter(|provider| {
                self.model_catalog
                    .providers
                    .get(provider)
                    .is_some_and(|state| state.provider.fetchable())
            })
            .collect::<Vec<_>>();
        if providers.is_empty() {
            ui.set_bottom_panel_notice(if self.model_catalog.providers.is_empty() {
                "no configured providers"
            } else {
                "no fetchable providers"
            });
            return Ok(());
        }
        for provider in providers {
            self.start_model_catalog_fetch_task(&provider);
        }
        self.rebuild_model_panel(ui, Some("fetch:all".to_string()))?;
        Ok(())
    }

    fn start_model_catalog_fetch_provider(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        provider: &str,
    ) -> Result<()> {
        let Some(state) = self.model_catalog.providers.get(provider) else {
            ui.set_bottom_panel_notice("provider unavailable");
            return Ok(());
        };
        if matches!(state.status, ModelCatalogStatus::Fetching) {
            ui.set_bottom_panel_notice("already fetching");
            return Ok(());
        }
        if !state.provider.fetchable() {
            ui.set_bottom_panel_notice(self.provider_status_text(state));
            return Ok(());
        }
        let key = format!("fetch:provider:{provider}");
        self.start_model_catalog_fetch_task(provider);
        self.rebuild_model_panel(ui, Some(key))?;
        Ok(())
    }

    fn start_model_catalog_fetch_task(&mut self, provider: &str) {
        if self.model_catalog.tasks.contains_key(provider) {
            return;
        }
        let Some(state) = self.model_catalog.providers.get_mut(provider) else {
            return;
        };
        if !state.provider.fetchable() {
            return;
        }
        state.status = ModelCatalogStatus::Fetching;
        let provider_config = state.provider.clone();
        let provider_id = provider_config.provider.clone();
        let task = tokio::spawn(async move {
            let result = fetch_model_catalog(&provider_config)
                .await
                .map_err(|err| short_fetch_error(&err.to_string()));
            ModelCatalogFetchResult {
                provider: provider_id,
                result,
            }
        });
        self.model_catalog.tasks.insert(provider.to_string(), task);
    }

    async fn drain_model_catalog_fetches(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        let finished = self
            .model_catalog
            .tasks
            .iter()
            .filter(|(_, task)| task.is_finished())
            .map(|(provider, _)| provider.clone())
            .collect::<Vec<_>>();
        if finished.is_empty() {
            return Ok(());
        }
        let selected_key = ui
            .bottom_panel
            .as_ref()
            .map(|panel| panel.selection().selected_key());
        for provider in finished {
            let Some(task) = self.model_catalog.tasks.remove(&provider) else {
                continue;
            };
            match task.await {
                Ok(result) => {
                    if let Some(state) = self.model_catalog.providers.get_mut(&result.provider) {
                        match result.result {
                            Ok(models) => {
                                state.fetched = models;
                                state.status = ModelCatalogStatus::Fetched;
                            }
                            Err(error) => {
                                state.status = ModelCatalogStatus::Failed(error);
                            }
                        }
                    }
                }
                Err(err) if err.is_cancelled() => {}
                Err(err) => {
                    if let Some(state) = self.model_catalog.providers.get_mut(&provider) {
                        state.status =
                            ModelCatalogStatus::Failed(short_fetch_error(&err.to_string()));
                    }
                }
            }
        }
        self.rebuild_model_panel(ui, selected_key)?;
        Ok(())
    }

    fn rebuild_model_panel(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        selected_key: Option<String>,
    ) -> Result<()> {
        let Some(BottomPanel::Models(panel)) = ui.bottom_panel.as_ref() else {
            return Ok(());
        };
        let query = panel.query.clone();
        let notice = panel.notice.clone();
        let mut panel = self.model_selection_panel()?;
        panel.query = query;
        panel.notice = notice;
        if let Some(key) = selected_key {
            panel.select_value_key(&key);
        }
        ui.bottom_panel = Some(BottomPanel::Models(panel));
        Ok(())
    }

    fn copy_selected_text(&self, ui: &mut FullscreenUi<'_>) -> Result<bool> {
        let Some(text) = ui.selected_text() else {
            return Ok(false);
        };
        if let Err(err) = (self.clipboard)(&text) {
            ui.push_error(format!(
                "copy failed: {}",
                truncate_chars(&err.to_string(), 240)
            ));
            ui.clear_selection();
            return Ok(true);
        }
        ui.clear_selection();
        Ok(true)
    }

    fn handle_history_search_key(&self, ui: &mut FullscreenUi<'_>, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                ui.history_search = false;
                ui.push_status("history search closed");
            }
            KeyCode::Enter => {
                if let Some(entry) = ui
                    .history
                    .iter()
                    .rev()
                    .find(|entry| entry.contains(&ui.history_query))
                    .cloned()
                {
                    ui.textarea = textarea_with_text(&entry);
                    ui.push_status("history entry selected");
                } else {
                    ui.push_error("no history match");
                }
                ui.history_search = false;
            }
            KeyCode::Backspace => {
                ui.history_query.pop();
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                ui.history_query.push(c);
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_fullscreen_command(
        &mut self,
        ui: &mut FullscreenUi<'_>,
        command: SlashCommand,
    ) -> Result<bool> {
        match command {
            SlashCommand::Quit => return Ok(true),
            SlashCommand::Status => {
                ui.push_status(self.status_text());
            }
            SlashCommand::New => {
                self.current_session = None;
                self.current_session_title = None;
                self.force_new_once = true;
                ui.clear_transcript();
                ui.replace_session_history_prompts(Vec::new());
                ui.refresh_sidebar(self);
            }
            SlashCommand::Sessions => {
                ui.bottom_panel = Some(BottomPanel::Sessions(self.session_selection_panel()?));
            }
            SlashCommand::ModelShow => {
                ui.bottom_panel = Some(BottomPanel::Models(self.model_selection_panel()?));
            }
            SlashCommand::VariantSet(variant) => {
                self.set_variant_no_print(variant.clone())?;
                ui.push_status(format!("variant: {variant}"));
                ui.refresh_sidebar(self);
            }
            SlashCommand::ModeSet(mode) => {
                self.set_mode_no_print(&mode)?;
                ui.refresh_sidebar(self);
            }
            SlashCommand::ThinkingToggle => {
                let enabled = !self.thinking_visible;
                self.set_thinking_no_print(enabled)?;
                ui.set_thinking_visible(enabled);
                ui.refresh_sidebar(self);
            }
            SlashCommand::ThinkingSet(enabled) => {
                self.set_thinking_no_print(enabled)?;
                ui.set_thinking_visible(enabled);
                ui.refresh_sidebar(self);
            }
            SlashCommand::Rename(title) => match self.rename_session_no_print(title) {
                Ok(title) => {
                    ui.push_status(format!("session renamed: {title}"));
                    ui.refresh_sidebar(self);
                }
                Err(err) => ui.push_error(format!("error: {err:#}")),
            },
            SlashCommand::Undo => {
                if let Some(running) = &ui.running {
                    running.control.abort();
                    ui.push_error("interrupt requested; run /undo again after the turn settles");
                } else {
                    match self.undo_session_no_print(ui) {
                        Ok(message) => ui.push_status(message),
                        Err(err) => ui.push_error(format!("error: {err:#}")),
                    }
                }
            }
            SlashCommand::Redo => {
                if let Some(running) = &ui.running {
                    running.control.abort();
                    ui.push_error("interrupt requested; run /redo again after the turn settles");
                } else {
                    match self.redo_session_no_print(ui) {
                        Ok(message) => ui.push_status(message),
                        Err(err) => ui.push_error(format!("error: {err:#}")),
                    }
                }
            }
            SlashCommand::Upcoming(command) => {
                ui.push_status(format!("/{command} upcoming"));
            }
        }
        Ok(false)
    }

    async fn handle_line(&mut self, line: &str) -> Result<bool> {
        match parse_slash_command(line) {
            Ok(Some(command)) => self.handle_command(command).await,
            Ok(None) => {
                if let Err(err) = self.submit_prompt(line.to_string()).await {
                    self.had_error = true;
                    eprintln!("{}", self.renderer.error(&format!("error: {err:#}")));
                }
                Ok(false)
            }
            Err(err) => {
                self.had_error = true;
                eprintln!("{}", self.renderer.error(&format!("error: {err:#}")));
                Ok(false)
            }
        }
    }

    async fn handle_command(&mut self, command: SlashCommand) -> Result<bool> {
        let result = match command {
            SlashCommand::Quit => return Ok(true),
            SlashCommand::Status => self.show_status(),
            SlashCommand::New => {
                self.current_session = None;
                self.current_session_title = None;
                self.force_new_once = true;
                Ok(())
            }
            SlashCommand::Sessions => self.show_session_list(),
            SlashCommand::ModelShow => self.show_model(),
            SlashCommand::VariantSet(variant) => self.set_variant(variant),
            SlashCommand::ModeSet(mode) => self.set_mode(mode),
            SlashCommand::ThinkingToggle => self.toggle_thinking(),
            SlashCommand::ThinkingSet(enabled) => self.set_thinking(enabled),
            SlashCommand::Rename(title) => self.rename_session(title),
            SlashCommand::Undo => self.undo_session_print(),
            SlashCommand::Redo => self.redo_session_print(),
            SlashCommand::Upcoming(command) => {
                println!("{}", self.renderer.status(&format!("/{command} upcoming")));
                Ok(())
            }
        };
        if let Err(err) = result {
            self.had_error = true;
            eprintln!("{}", self.renderer.error(&format!("error: {err:#}")));
        }
        Ok(false)
    }

    async fn submit_prompt(&mut self, prompt: String) -> Result<()> {
        let stdout = Arc::new(Mutex::new(io::stdout()));
        let turn = Arc::new(Mutex::new(TurnPrinter::new(
            self.renderer,
            self.thinking_visible,
            self.debug,
        )));
        {
            let mut stdout = stdout.lock().expect("stdout lock poisoned");
            writeln!(stdout, "Prompt: {prompt}")?;
        }
        let turn_for_sink = Arc::clone(&turn);
        let stdout_for_sink = Arc::clone(&stdout);
        let sink: RunStreamSink = Arc::new(move |event| {
            let mut turn = turn_for_sink.lock().expect("turn lock poisoned");
            let mut stdout = stdout_for_sink.lock().expect("stdout lock poisoned");
            let _ = turn.render_event(&event, &mut *stdout);
        });
        let options = self.run_options(prompt);
        let result = run_live_streaming(options, "tui", TUI_SESSION_SOURCES, sink).await?;
        {
            let mut turn = turn.lock().expect("turn lock poisoned");
            let mut stdout = stdout.lock().expect("stdout lock poisoned");
            turn.finish(&mut *stdout)?;
        }
        self.current_session = Some(result.session_id);
        self.refresh_current_session_title()?;
        self.force_new_once = false;
        let success = result.outcome == Outcome::Normal && result.tool_failures == 0;
        if !success {
            self.had_error = true;
        }
        Ok(())
    }

    fn start_fullscreen_turn(&mut self, ui: &mut FullscreenUi<'_>, prompt: String) -> Result<()> {
        if ui.running.is_some() {
            ui.push_error("a turn is already running");
            return Ok(());
        }
        ui.push_user(prompt.clone());
        let (tx, rx) = mpsc::unbounded_channel();
        let sink: RunStreamSink = Arc::new(move |event| {
            let _ = tx.send(event);
        });
        let (control_handle, control) = run_control();
        let options = self.run_options(prompt);
        let task = tokio::spawn(async move {
            run_live_streaming_controlled(options, "tui", TUI_SESSION_SOURCES, sink, control).await
        });
        ui.scroll_to_bottom();
        ui.running = Some(RunningTurn {
            control: control_handle,
            rx,
            task,
        });
        ui.start_assistant();
        ui.refresh_sidebar(self);
        Ok(())
    }

    async fn drain_fullscreen_events(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        self.drain_model_catalog_fetches(ui).await?;
        let mut pending = Vec::new();
        if let Some(running) = &mut ui.running {
            while let Ok(event) = running.rx.try_recv() {
                pending.push(event);
            }
        }
        let had_pending = !pending.is_empty();
        for event in pending {
            self.apply_fullscreen_stream_event(ui, event);
        }
        if had_pending {
            ui.follow_transcript_if_needed();
            ui.refresh_sidebar(self);
        }
        if ui
            .running
            .as_ref()
            .is_some_and(|running| running.task.is_finished())
        {
            let mut running = ui.running.take().expect("checked running");
            let result = running.task.await;
            while let Ok(event) = running.rx.try_recv() {
                self.apply_fullscreen_stream_event(ui, event);
            }
            ui.follow_transcript_if_needed();
            match result {
                Ok(Ok(result)) => {
                    self.current_session = Some(result.session_id.clone());
                    self.refresh_current_session_title()?;
                    self.force_new_once = false;
                    let success = result.outcome == Outcome::Normal && result.tool_failures == 0;
                    if !success {
                        self.had_error = true;
                        ui.push_error(format!("turn ended: {}", result.outcome.as_str()));
                    }
                }
                Ok(Err(err)) => {
                    self.had_error = true;
                    ui.push_error(format!("error: {err:#}"));
                }
                Err(err) => {
                    self.had_error = true;
                    ui.push_error(format!("task failed: {err}"));
                }
            }
            ui.finish_turn();
            ui.refresh_sidebar(self);
        } else if ui.turn_outcome.is_some() {
            self.finish_streamed_agent_turn(ui);
        }
        Ok(())
    }

    fn apply_fullscreen_stream_event(&mut self, ui: &mut FullscreenUi<'_>, event: RunStreamEvent) {
        if let RunStreamEvent::Event(value) = &event {
            self.observe_fullscreen_value_event(value);
        }
        ui.apply_stream_event(event, self.thinking_visible, self.debug);
    }

    fn observe_fullscreen_value_event(&mut self, value: &Value) {
        if value.get("type").and_then(Value::as_str) != Some("run_start") {
            return;
        }
        let Some(session_id) = value.get("session_id").and_then(Value::as_str) else {
            return;
        };
        if self.current_session.as_deref() != Some(session_id) {
            self.current_session = Some(session_id.to_string());
            self.current_session_title = None;
        }
        self.force_new_once = false;
    }

    fn finish_streamed_agent_turn(&mut self, ui: &mut FullscreenUi<'_>) {
        let outcome = ui.turn_outcome.unwrap_or(Outcome::Normal);
        let _detached = ui.running.take();
        let success = outcome == Outcome::Normal && ui.turn_failures == 0;
        if !success {
            self.had_error = true;
            ui.push_error(format!("turn ended: {}", outcome.as_str()));
        }
        ui.finish_turn();
        ui.refresh_sidebar(self);
    }

    fn render_fullscreen(&self, frame: &mut Frame<'_>, ui: &mut FullscreenUi<'_>) {
        let area = frame.area();
        ui.clear_screen_lines();
        ui.set_thinking_visible(self.thinking_visible);
        let sidebar_visible = ui.sidebar_forced && area.width >= 100 && !ui.sidebar_hidden;
        ui.last_sidebar_visible = sidebar_visible;
        let horizontal = if sidebar_visible {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(40), Constraint::Length(42)])
                .split(area)
        } else {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(40)])
                .split(area)
        };
        let main = horizontal[0];
        if ui.bottom_panel.is_some() {
            ui.last_slash_menu_areas.clear();
            let panel_height = bottom_panel_height(main.height);
            let vertical = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(5),
                    Constraint::Length(panel_height),
                    Constraint::Length(1),
                ])
                .split(main);
            render_transcript(frame, vertical[0], ui);
            if let Some(panel) = &mut ui.bottom_panel {
                render_bottom_panel(frame, vertical[1], panel, &mut ui.last_bottom_panel_areas);
            }
            render_status(frame, vertical[2], self);
            if sidebar_visible {
                render_sidebar(frame, horizontal[1], ui);
            }
            render_active_selection(frame, ui);
            return;
        }
        let composer_height = composer_height(&ui.textarea);
        let slash_items = slash_menu_items(&textarea_text(&ui.textarea));
        ui.clamp_slash_menu_selection(slash_items.len());
        ui.last_bottom_panel_areas.clear();
        let slash_height = if slash_items.is_empty() {
            0
        } else {
            (slash_items.len() as u16 + 2).min(10)
        };
        let vertical = if slash_height == 0 {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(5),
                    Constraint::Length(composer_height),
                    Constraint::Length(1),
                ])
                .split(main)
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(5),
                    Constraint::Length(slash_height),
                    Constraint::Length(composer_height),
                    Constraint::Length(1),
                ])
                .split(main)
        };
        if slash_height == 0 {
            render_transcript(frame, vertical[0], ui);
            render_composer(frame, vertical[1], ui);
            render_status(frame, vertical[2], self);
        } else {
            render_transcript(frame, vertical[0], ui);
            render_slash_menu(
                frame,
                vertical[1],
                &slash_items,
                ui.slash_menu_selected,
                &mut ui.last_slash_menu_areas,
            );
            render_composer(frame, vertical[2], ui);
            render_status(frame, vertical[3], self);
        }
        if sidebar_visible {
            render_sidebar(frame, horizontal[1], ui);
        }
        render_active_selection(frame, ui);
    }

    fn run_options(&self, prompt: String) -> RunOptions {
        RunOptions {
            db_path: self.db_path.clone(),
            workdir: self.workdir.clone(),
            snapshot_root: Some(self.home.join("snapshots")),
            session: self.current_session.clone(),
            continue_latest: self.current_session.is_none() && !self.force_new_once,
            prompt,
            max_context_messages: None,
            config_path: self.config_path.clone(),
            model: self.current_model.clone(),
            reasoning_effort: self.current_variant.clone(),
            include_reasoning: false,
            mode: self.current_mode,
            inherited_env: Some(self.env_map.clone()),
        }
    }

    fn show_status(&self) -> Result<()> {
        println!("{}", self.status_text());
        Ok(())
    }

    fn show_session_list(&self) -> Result<()> {
        for line in self.session_list_lines()? {
            println!("{line}");
        }
        Ok(())
    }

    fn show_model(&self) -> Result<()> {
        for line in self.model_lines()? {
            println!("{line}");
        }
        Ok(())
    }

    fn set_variant(&mut self, variant: String) -> Result<()> {
        self.set_variant_no_print(variant.clone())?;
        println!("{}", self.renderer.status(&format!("variant: {variant}")));
        Ok(())
    }

    fn toggle_thinking(&mut self) -> Result<()> {
        self.set_thinking_no_print(!self.thinking_visible)?;
        self.show_thinking_status();
        Ok(())
    }

    fn set_thinking(&mut self, enabled: bool) -> Result<()> {
        self.set_thinking_no_print(enabled)?;
        self.show_thinking_status();
        Ok(())
    }

    fn show_thinking_status(&self) {
        println!(
            "{}",
            self.renderer
                .status(&format!("thinking: {}", on_off(self.thinking_visible)))
        );
    }

    fn set_mode(&mut self, mode: String) -> Result<()> {
        self.set_mode_no_print(&mode)?;
        println!("{}", self.renderer.status(&format!("mode: {mode}")));
        Ok(())
    }

    fn rename_session(&mut self, title: String) -> Result<()> {
        let title = self.rename_session_no_print(title)?;
        println!(
            "{}",
            self.renderer.status(&format!("session renamed: {title}"))
        );
        Ok(())
    }

    fn undo_session_print(&mut self) -> Result<()> {
        let result = undo_session(self.undo_options()?)?;
        println!(
            "{}",
            self.renderer.status(&format!(
                "undone {} messages; prompt restored",
                result.reverted_messages
            ))
        );
        Ok(())
    }

    fn redo_session_print(&mut self) -> Result<()> {
        let result = redo_session(self.undo_options()?)?;
        let suffix = if result.complete {
            "complete"
        } else {
            "partial"
        };
        println!(
            "{}",
            self.renderer.status(&format!(
                "redone {} messages; {suffix}",
                result.restored_messages
            ))
        );
        Ok(())
    }

    fn status_lines(&self) -> Vec<String> {
        vec![
            format!("workdir: {}", self.workdir.display()),
            format!("home: {}", self.home.display()),
            format!("db: {}", self.db_path.display()),
            format!(
                "session: {}",
                self.current_session.as_deref().unwrap_or("(none)")
            ),
            format!("model: {}", self.model_display_value()),
            self.variant_line(),
            format!("mode: {}", self.current_mode.as_str()),
            format!("thinking: {}", on_off(self.thinking_visible)),
            format!("debug: {}", on_off(self.debug)),
        ]
    }

    fn status_text(&self) -> String {
        self.status_lines().join("\n")
    }

    fn session_list_lines(&self) -> Result<Vec<String>> {
        let sessions = self.tui_sessions_for_workdir()?;
        if sessions.is_empty() {
            return Ok(vec!["no sessions for this workdir".to_string()]);
        }
        Ok(sessions
            .into_iter()
            .map(|session| {
                let summary = &session.summary;
                format_session_line(
                    &summary.id,
                    &summary.source,
                    &summary.provider,
                    &summary.model,
                    session.visible_message_count as i64,
                )
            })
            .collect())
    }

    fn session_selection_panel(&self) -> Result<BottomSelectionPanel> {
        let current_session = self.current_session.as_deref();
        let rows = self
            .tui_sessions_for_workdir()?
            .into_iter()
            .map(|session| {
                let summary = session.summary;
                let title = summary
                    .title
                    .clone()
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| short_session(&summary.id).to_string());
                let provider_model = format!("{}/{}", summary.provider, summary.model);
                let description = Some(format!(
                    "{}  messages={}",
                    provider_model, session.visible_message_count
                ));
                let search_text = format!(
                    "{} {} {} {} {}",
                    summary.id, title, summary.provider, summary.model, summary.source
                );
                BottomSelectionRow {
                    label: title,
                    description,
                    detail: Some(format_session_time(summary.updated_at_ms)),
                    group: Some(format_session_date(summary.updated_at_ms)),
                    search_text,
                    is_current: current_session.is_some_and(|id| id == summary.id),
                    is_default: false,
                    style: BottomRowStyle::Normal,
                    footer: None,
                    value: BottomSelectionValue::Session(summary.id),
                }
            })
            .collect();
        Ok(BottomSelectionPanel::new(
            "Sessions",
            "Search local run and TUI sessions.",
            "No sessions",
            rows,
        ))
    }

    fn model_selection_panel(&mut self) -> Result<BottomSelectionPanel> {
        self.sync_model_catalog_providers()?;
        let current = self.model_display_value();
        let local_models = configured_models(&self.run_options(String::new()))?;
        let mut local_by_provider: BTreeMap<String, Vec<ConfiguredModel>> = BTreeMap::new();
        let mut known_specs = BTreeMap::new();
        for model in local_models {
            known_specs.insert(format_model_spec(&model), ModelRowSource::Local);
            local_by_provider
                .entry(model.provider.clone())
                .or_default()
                .push(model);
        }

        let mut rows = Vec::new();
        let all_fetchable = self.model_catalog.providers.values().any(|state| {
            state.provider.fetchable() && !matches!(state.status, ModelCatalogStatus::Fetching)
        });
        rows.push(BottomSelectionRow {
            label: "All providers".to_string(),
            description: Some(self.all_providers_status()),
            detail: None,
            group: None,
            search_text: "all providers fetch models".to_string(),
            is_current: false,
            is_default: false,
            style: BottomRowStyle::Action,
            footer: Some("Enter fetch  Esc close  Type search".to_string()),
            value: if all_fetchable {
                BottomSelectionValue::FetchAllModels
            } else {
                BottomSelectionValue::ProviderInfo("all".to_string())
            },
        });

        let mut first_model_key = None;
        let mut first_local_key = None;
        let mut current_key = None;
        for provider_id in self.model_catalog_provider_order() {
            let Some(state) = self.model_catalog.providers.get(&provider_id) else {
                continue;
            };
            rows.push(BottomSelectionRow {
                label: state.provider.display_label.clone(),
                description: Some(self.provider_status_text(state)),
                detail: None,
                group: None,
                search_text: format!(
                    "{} {}",
                    state.provider.provider, state.provider.display_label
                ),
                is_current: false,
                is_default: false,
                style: BottomRowStyle::Action,
                footer: Some("Enter fetch  Esc close  Type search".to_string()),
                value: if state.provider.fetchable() {
                    BottomSelectionValue::FetchProvider(state.provider.provider.clone())
                } else {
                    BottomSelectionValue::ProviderInfo(state.provider.provider.clone())
                },
            });

            if let Some(models) = local_by_provider.get_mut(&provider_id) {
                models.sort_by(|left, right| left.model.cmp(&right.model));
                for model in models.iter().cloned() {
                    let key = format!("model:{}", format_model_spec(&model));
                    first_model_key.get_or_insert_with(|| key.clone());
                    first_local_key.get_or_insert_with(|| key.clone());
                    if format_model_spec(&model) == current {
                        current_key = Some(key.clone());
                    }
                    rows.push(self.model_row(model, ModelRowSource::Local, &current));
                }
            }

            for entry in &state.fetched {
                let spec = format!("{}/{}", state.provider.provider, entry.id);
                if known_specs.contains_key(&spec) {
                    continue;
                }
                let model = ConfiguredModel {
                    provider: state.provider.provider.clone(),
                    provider_label: state.provider.display_label.clone(),
                    model: entry.id.clone(),
                    reasoning_effort: None,
                    context_limit: entry.context_limit,
                };
                let key = format!("model:{spec}");
                first_model_key.get_or_insert_with(|| key.clone());
                if spec == current {
                    current_key = Some(key.clone());
                }
                rows.push(self.model_row(model, ModelRowSource::Fetched, &current));
                known_specs.insert(spec, ModelRowSource::Fetched);
            }
        }

        if current != "config"
            && !known_specs.contains_key(&current)
            && let Some((provider, model)) = current.split_once('/')
        {
            let provider_label = self
                .model_catalog
                .providers
                .get(provider)
                .map(|state| state.provider.display_label.clone())
                .unwrap_or_else(|| provider.to_string());
            let model = ConfiguredModel {
                provider: provider.to_string(),
                provider_label,
                model: model.to_string(),
                reasoning_effort: None,
                context_limit: None,
            };
            let key = format!("model:{current}");
            current_key = Some(key.clone());
            first_model_key.get_or_insert(key);
            rows.push(self.model_row(model, ModelRowSource::CurrentOnly, &current));
        }

        let mut panel = BottomSelectionPanel::new("Select Model", "", "No models", rows);
        let initial_key = current_key
            .or(first_local_key)
            .or(first_model_key)
            .unwrap_or_else(|| "fetch:all".to_string());
        panel.select_value_key(&initial_key);
        Ok(panel)
    }

    fn variant_panel(
        &self,
        model: ConfiguredModel,
        source: ModelRowSource,
        models: BottomSelectionPanel,
    ) -> BottomPanel {
        let model_spec = format_model_spec(&model);
        let current_model = self.model_display_value();
        let is_current_model = current_model == model_spec;
        let configured = model
            .reasoning_effort
            .as_deref()
            .map(|variant| format!("configured default: {variant}"))
            .unwrap_or_else(|| match source {
                ModelRowSource::Local => "use provider configuration".to_string(),
                ModelRowSource::Fetched | ModelRowSource::CurrentOnly => {
                    "use provider default".to_string()
                }
            });
        let mut rows = vec![BottomSelectionRow {
            label: "Config default".to_string(),
            description: Some(configured),
            detail: None,
            group: None,
            search_text: "config default provider configuration".to_string(),
            is_current: is_current_model && self.current_variant.is_none(),
            is_default: true,
            style: BottomRowStyle::Normal,
            footer: None,
            value: BottomSelectionValue::Variant {
                model: model_spec.clone(),
                variant: None,
            },
        }];
        rows.extend(VARIANTS.iter().map(|variant| BottomSelectionRow {
            label: (*variant).to_string(),
            description: Some(variant_description(variant).to_string()),
            detail: None,
            group: None,
            search_text: format!("{variant} {}", variant_description(variant)),
            is_current: is_current_model && self.current_variant.as_deref() == Some(*variant),
            is_default: false,
            style: BottomRowStyle::Normal,
            footer: None,
            value: BottomSelectionValue::Variant {
                model: model_spec.clone(),
                variant: Some((*variant).to_string()),
            },
        }));
        let mut panel = BottomSelectionPanel::new(
            &format!("Select Variant for {model_spec}"),
            "Use config default or persist an explicit variant override.",
            "No variants",
            rows,
        );
        panel.footer = "Enter select  Esc back  Type search".to_string();
        if is_current_model
            && let Some(current_variant) = self.current_variant.as_deref()
            && let Some(index) = panel
                .rows
                .iter()
                .position(|row| row.label == current_variant)
        {
            panel.set_selected(index);
        }
        BottomPanel::Variants {
            models: Box::new(models),
            panel,
        }
    }

    fn sync_model_catalog_providers(&mut self) -> Result<()> {
        let providers = model_catalog_providers(&self.run_options(String::new()))?;
        let active = providers
            .iter()
            .map(|provider| provider.provider.clone())
            .collect::<Vec<_>>();
        for provider in providers {
            self.model_catalog
                .providers
                .entry(provider.provider.clone())
                .and_modify(|state| state.provider = provider.clone())
                .or_insert_with(|| ModelProviderCatalogState {
                    provider,
                    status: ModelCatalogStatus::NotFetched,
                    fetched: Vec::new(),
                });
        }
        self.model_catalog
            .providers
            .retain(|provider, _| active.contains(provider));
        Ok(())
    }

    fn model_catalog_provider_order(&self) -> Vec<String> {
        let mut providers = self
            .model_catalog
            .providers
            .values()
            .map(|state| {
                (
                    state.provider.display_label.clone(),
                    state.provider.provider.clone(),
                )
            })
            .collect::<Vec<_>>();
        providers.sort();
        providers
            .into_iter()
            .map(|(_, provider)| provider)
            .collect()
    }

    fn all_providers_status(&self) -> String {
        if self.model_catalog.providers.is_empty() {
            return "no providers".to_string();
        }
        if self.model_catalog.any_fetching() {
            return "fetching".to_string();
        }
        let mut fetchable = 0usize;
        let mut failed = 0usize;
        let mut fetched = 0usize;
        let mut models = 0usize;
        let mut missing = 0usize;
        for state in self.model_catalog.providers.values() {
            if !state.provider.fetchable() {
                missing += 1;
                continue;
            }
            fetchable += 1;
            match &state.status {
                ModelCatalogStatus::Failed(_) => failed += 1,
                ModelCatalogStatus::Fetched => {
                    fetched += 1;
                    models += state.fetched.len();
                }
                ModelCatalogStatus::Fetching | ModelCatalogStatus::NotFetched => {}
            }
        }
        if fetchable == 0 && missing > 0 {
            return "missing credentials".to_string();
        }
        if failed > 0 && fetched > 0 {
            return "partial failed".to_string();
        }
        if failed > 0 {
            return "failed".to_string();
        }
        if fetched > 0 {
            if models == 0 {
                "no models".to_string()
            } else {
                format!("fetched {models} models")
            }
        } else {
            "not fetched".to_string()
        }
    }

    fn provider_status_text(&self, state: &ModelProviderCatalogState) -> String {
        if let Some(missing) = &state.provider.missing_credentials {
            return format!("missing {missing}");
        }
        if let Some(reason) = &state.provider.unavailable_reason {
            return format!("failed: {}", short_fetch_error(reason));
        }
        match &state.status {
            ModelCatalogStatus::NotFetched => "not fetched".to_string(),
            ModelCatalogStatus::Fetching => "fetching".to_string(),
            ModelCatalogStatus::Fetched if state.fetched.is_empty() => "no models".to_string(),
            ModelCatalogStatus::Fetched => format!("fetched {} models", state.fetched.len()),
            ModelCatalogStatus::Failed(error) => format!("failed: {error}"),
        }
    }

    fn model_row(
        &self,
        model: ConfiguredModel,
        source: ModelRowSource,
        current: &str,
    ) -> BottomSelectionRow {
        let model_spec = format_model_spec(&model);
        let mut details = Vec::new();
        if source == ModelRowSource::Fetched {
            details.push("fetched".to_string());
        }
        if source == ModelRowSource::Local
            && let Some(variant) = &model.reasoning_effort
        {
            details.push(format!("default {variant}"));
        }
        if let Some(limit) = model.context_limit {
            details.push(format!("context {}", format_count(limit)));
        }
        let description = if details.is_empty() {
            Some(model.provider_label.clone())
        } else {
            Some(format!("{}  {}", model.provider_label, details.join("  ")))
        };
        let search_text = format!(
            "{} {} {} {} {}",
            model_spec,
            model.provider_label,
            model.reasoning_effort.clone().unwrap_or_default(),
            model.context_limit.unwrap_or_default(),
            if source == ModelRowSource::Fetched {
                "fetched"
            } else {
                ""
            }
        );
        BottomSelectionRow {
            label: model_spec.clone(),
            description,
            detail: None,
            group: None,
            search_text,
            is_current: model_spec == current,
            is_default: self.current_model.is_none() && model_spec == current,
            style: BottomRowStyle::Normal,
            footer: None,
            value: BottomSelectionValue::Model { model, source },
        }
    }

    fn model_lines(&self) -> Result<Vec<String>> {
        let mut lines = vec![format!("model: {}", self.model_display_value())];
        if !self.state.recent_models.is_empty() {
            lines.push(format!("recent: {}", self.state.recent_models.join(", ")));
        }
        lines.push("configured models:".to_string());
        lines.extend(self.configured_model_lines()?);
        Ok(lines)
    }

    fn configured_model_lines(&self) -> Result<Vec<String>> {
        let models = configured_models(&self.run_options(String::new()))?;
        if models.is_empty() {
            return Ok(vec!["no configured models".to_string()]);
        }
        Ok(models.iter().map(format_configured_model).collect())
    }

    fn variant_line(&self) -> String {
        format!("variant: {}", self.variant_display_value())
    }

    fn model_display_value(&self) -> String {
        self.current_model
            .clone()
            .or_else(|| {
                self.selected_model
                    .as_ref()
                    .map(|model| format!("{}/{}", model.provider, model.model))
            })
            .unwrap_or_else(|| "config".to_string())
    }

    fn variant_display_value(&self) -> String {
        self.current_variant
            .clone()
            .or_else(|| {
                self.selected_model
                    .as_ref()
                    .and_then(|model| model.reasoning_effort.clone())
            })
            .unwrap_or_else(|| "default".to_string())
    }

    fn refresh_selected_model(&mut self) {
        self.selected_model = selected_configured_model(&self.run_options(String::new()))
            .ok()
            .flatten();
    }

    fn refresh_current_session_title(&mut self) -> Result<()> {
        self.current_session_title = self
            .current_session
            .as_deref()
            .map(|session_id| SqliteStore::open(&self.db_path)?.session_summary(session_id))
            .transpose()?
            .flatten()
            .and_then(|summary| summary.title)
            .filter(|title| !title.trim().is_empty());
        Ok(())
    }

    fn session_sidebar_title(&self) -> String {
        self.current_session_title
            .clone()
            .or_else(|| {
                self.current_session
                    .as_deref()
                    .map(short_session)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "New session".to_string())
    }

    fn switch_session_no_print(&mut self, reference: &str) -> Result<String> {
        let id = self.resolve_session_ref(reference)?;
        SqliteStore::open(&self.db_path)?.resume_session(&id)?;
        self.current_session = Some(id.clone());
        self.force_new_once = false;
        self.refresh_current_session_title()?;
        Ok(id)
    }

    fn set_model_and_variant_no_print(
        &mut self,
        model: String,
        variant: Option<String>,
    ) -> Result<()> {
        validate_model_spec(&model)?;
        if let Some(variant) = &variant {
            validate_variant(variant)?;
        }
        self.current_model = Some(model.clone());
        self.current_variant = variant.clone();
        self.state.set_model(&self.workdir_key, model);
        if let Some(variant) = variant {
            self.state.set_variant(&self.workdir_key, variant);
        } else {
            self.state.clear_variant(&self.workdir_key);
        }
        self.state.save(&self.state_path)?;
        self.refresh_selected_model();
        Ok(())
    }

    fn set_variant_no_print(&mut self, variant: String) -> Result<()> {
        validate_variant(&variant)?;
        self.current_variant = Some(variant.clone());
        self.state.set_variant(&self.workdir_key, variant);
        self.state.save(&self.state_path)?;
        self.refresh_selected_model();
        Ok(())
    }

    fn set_mode_no_print(&mut self, mode: &str) -> Result<()> {
        let Some(parsed) = RunMode::parse(mode) else {
            return Err(anyhow!("mode must be one of plan, default"));
        };
        self.current_mode = parsed;
        self.state.set_mode(&self.workdir_key, mode.to_string());
        self.state.save(&self.state_path)?;
        Ok(())
    }

    fn set_thinking_no_print(&mut self, enabled: bool) -> Result<()> {
        self.thinking_visible = enabled;
        self.state.set_thinking_visible(enabled);
        self.state.save(&self.state_path)?;
        Ok(())
    }

    fn rename_session_no_print(&mut self, title: String) -> Result<String> {
        let Some(session_id) = self.current_session.as_deref() else {
            return Err(anyhow!("no current session to rename"));
        };
        let title = SqliteStore::open(&self.db_path)?.set_session_title(session_id, &title)?;
        self.current_session_title = Some(title.clone());
        Ok(title)
    }

    fn undo_options(&self) -> Result<SessionUndoOptions> {
        let Some(session_id) = self.current_session.clone() else {
            return Err(anyhow!("no current session to undo"));
        };
        Ok(SessionUndoOptions {
            db_path: self.db_path.clone(),
            workdir: self.workdir.clone(),
            snapshot_root: self.home.join("snapshots"),
            session_id,
        })
    }

    fn undo_session_no_print(&mut self, ui: &mut FullscreenUi<'_>) -> Result<String> {
        let result = undo_session(self.undo_options()?)?;
        ui.clear_transcript();
        self.load_current_session_history(ui)?;
        ui.textarea = textarea_with_text(&result.prompt);
        ui.refresh_sidebar(self);
        Ok(format!(
            "undone {} messages; prompt restored",
            result.reverted_messages
        ))
    }

    fn redo_session_no_print(&mut self, ui: &mut FullscreenUi<'_>) -> Result<String> {
        let result = redo_session(self.undo_options()?)?;
        ui.clear_transcript();
        self.load_current_session_history(ui)?;
        ui.textarea = new_textarea();
        ui.refresh_sidebar(self);
        let suffix = if result.complete {
            "complete"
        } else {
            "partial"
        };
        Ok(format!(
            "redone {} messages; {suffix}",
            result.restored_messages
        ))
    }

    fn set_sidebar_visible_no_print(&mut self, visible: bool) -> Result<()> {
        self.state.set_sidebar_visible(visible);
        self.state.save(&self.state_path)?;
        Ok(())
    }

    fn cycle_mode(&mut self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        let next = match self.current_mode {
            RunMode::Plan => RunMode::Build,
            RunMode::Build => RunMode::Plan,
        };
        self.set_mode_no_print(next.as_str())?;
        ui.refresh_sidebar(self);
        Ok(())
    }

    fn resolve_session_ref(&self, reference: &str) -> Result<String> {
        let sessions = self.sessions_for_workdir()?;
        resolve_session_ref_from_summaries(&sessions, reference)
    }

    fn sessions_for_workdir(&self) -> Result<Vec<SessionSummary>> {
        SqliteStore::open(&self.db_path)?
            .list_sessions_for_workdir_with_sources(&self.workdir, TUI_SESSION_SOURCES)
            .map_err(Into::into)
    }

    fn tui_sessions_for_workdir(&self) -> Result<Vec<TuiSessionDisplaySummary>> {
        let store = SqliteStore::open(&self.db_path)?;
        store
            .list_sessions_for_workdir_with_sources(&self.workdir, TUI_SESSION_SOURCES)?
            .into_iter()
            .map(|summary| {
                let messages = store.load_tui_message_summaries(&summary.id)?;
                Ok(TuiSessionDisplaySummary {
                    summary,
                    visible_message_count: visible_tui_message_count(&messages)?,
                })
            })
            .collect()
    }

    fn load_current_session_history(&self, ui: &mut FullscreenUi<'_>) -> Result<()> {
        let Some(session_id) = self.current_session.as_deref() else {
            ui.replace_session_history_prompts(Vec::new());
            ui.refresh_sidebar(self);
            return Ok(());
        };
        let store = SqliteStore::open(&self.db_path)?;
        let mut history_prompts = Vec::new();
        for summary in store.load_tui_message_summaries(session_id)? {
            let value = serde_json::to_value(summary.message)?;
            if value.get("role").and_then(Value::as_str) == Some("user")
                && let Some(text) = user_text_from_message(&value)
            {
                history_prompts.push(text);
            }
            ui.push_history_message(&value, summary.usage.as_ref(), summary.metadata.as_ref());
        }
        ui.replace_session_history_prompts(history_prompts);
        ui.scroll_to_bottom();
        ui.refresh_sidebar(self);
        Ok(())
    }
}

struct RunningTurn {
    control: RunControlHandle,
    rx: mpsc::UnboundedReceiver<RunStreamEvent>,
    task: JoinHandle<psychevo_runtime::Result<psychevo_runtime::RunResult>>,
}

struct TuiSessionDisplaySummary {
    summary: SessionSummary,
    visible_message_count: usize,
}

type ClipboardSink = Arc<dyn Fn(&str) -> io::Result<()> + Send + Sync>;

#[derive(Default)]
struct ModelCatalogCache {
    providers: BTreeMap<String, ModelProviderCatalogState>,
    tasks: BTreeMap<String, JoinHandle<ModelCatalogFetchResult>>,
}

struct ModelProviderCatalogState {
    provider: ModelCatalogProvider,
    status: ModelCatalogStatus,
    fetched: Vec<ModelCatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ModelCatalogStatus {
    NotFetched,
    Fetching,
    Fetched,
    Failed(String),
}

struct ModelCatalogFetchResult {
    provider: String,
    result: std::result::Result<Vec<ModelCatalogEntry>, String>,
}

impl ModelCatalogCache {
    fn any_fetching(&self) -> bool {
        self.providers
            .values()
            .any(|state| matches!(state.status, ModelCatalogStatus::Fetching))
    }

    fn abort_unfinished(&mut self) {
        for (_, task) in std::mem::take(&mut self.tasks) {
            task.abort();
        }
        for state in self.providers.values_mut() {
            if matches!(state.status, ModelCatalogStatus::Fetching) {
                state.status = if state.fetched.is_empty() {
                    ModelCatalogStatus::NotFetched
                } else {
                    ModelCatalogStatus::Fetched
                };
            }
        }
    }
}

const TUI_CYAN: Color = Color::Cyan;
const TUI_MAGENTA: Color = Color::Magenta;
const TUI_RED: Color = Color::Red;
const TUI_DIM: Color = Color::DarkGray;
const TUI_PAPER: Color = Color::Rgb(216, 205, 184);
const TUI_SURFACE_BG: Color = Color::Rgb(38, 38, 38);
const TUI_SELECTION_BG: Color = Color::Rgb(62, 88, 105);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranscriptKind {
    Prompt,
    Answer,
    Thinking,
    Explored,
    Ran,
    Changed,
    Meta,
    Status,
    Error,
}

#[derive(Debug, Clone)]
struct TranscriptRow {
    kind: TranscriptKind,
    title: String,
    text: String,
    full_text: Option<String>,
    expanded: bool,
    failed: bool,
    tool_call_id: Option<String>,
    tool_started: Option<Instant>,
    tool_elapsed: Option<Duration>,
}

impl TranscriptRow {
    fn simple(kind: TranscriptKind, text: impl Into<String>) -> Self {
        let title = default_title(kind).to_string();
        Self::with_title(kind, title, text)
    }

    fn with_title(kind: TranscriptKind, title: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            text: text.into(),
            full_text: None,
            expanded: false,
            failed: false,
            tool_call_id: None,
            tool_started: None,
            tool_elapsed: None,
        }
    }

    fn expandable_text(&self) -> &str {
        if self.expanded {
            self.full_text.as_deref().unwrap_or(&self.text)
        } else {
            &self.text
        }
    }

    fn is_expandable(&self) -> bool {
        self.full_text
            .as_ref()
            .is_some_and(|full| full != &self.text)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusMode {
    Composer,
    Transcript,
}

struct FullscreenUi<'a> {
    textarea: TextArea<'a>,
    transcript: Vec<TranscriptRow>,
    assistant_row: Option<usize>,
    reasoning_row: Option<usize>,
    meta_row: Option<usize>,
    tool_rows: BTreeMap<String, usize>,
    history_tool_titles: BTreeMap<String, String>,
    turn_started: Option<Instant>,
    turn_provider: String,
    turn_model: String,
    turn_mode: String,
    turn_context_limit: Option<u64>,
    turn_usage: Option<Value>,
    turn_metadata: Option<Value>,
    turn_failures: usize,
    turn_outcome: Option<Outcome>,
    history_prompt_started_ms: Option<i64>,
    thinking_visible: bool,
    running: Option<RunningTurn>,
    scroll: u16,
    last_transcript_height: u16,
    last_transcript_width: u16,
    auto_follow_transcript: bool,
    focus: FocusMode,
    selected_row: Option<usize>,
    last_entry_areas: Vec<(usize, Rect)>,
    sidebar_forced: bool,
    sidebar_hidden: bool,
    last_sidebar_visible: bool,
    sidebar: SidebarSnapshot,
    sidebar_tokens: Option<u64>,
    sidebar_context_limit: Option<u64>,
    history: Vec<String>,
    history_kinds: Vec<ComposerHistoryKind>,
    history_index: Option<usize>,
    history_draft: Option<String>,
    history_search: bool,
    history_query: String,
    slash_menu_selected: usize,
    last_slash_menu_areas: Vec<(usize, Rect)>,
    last_bottom_panel_areas: Vec<(usize, Rect)>,
    bottom_panel: Option<BottomPanel>,
    screen_lines: Vec<ScreenLine>,
    selection: SelectionState,
    terminal_clear_requested: bool,
    quit_requested: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComposerHistoryKind {
    SessionPrompt,
    ProcessCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScreenLine {
    region: SelectableRegion,
    y: u16,
    cells: Vec<ScreenCell>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectableRegion {
    Transcript,
    Sidebar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScreenCell {
    x: u16,
    width: u16,
    text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SelectionState {
    anchor: Option<(u16, u16)>,
    focus: Option<(u16, u16)>,
    region: Option<SelectableRegion>,
}

#[derive(Debug, Clone, Default)]
struct SidebarSnapshot {
    title: String,
    session: String,
    workdir: String,
    branch: String,
    tokens: Option<u64>,
    context_percent: Option<f64>,
    message_count: usize,
    tool_count: usize,
    changed_files: Vec<String>,
}

#[derive(Debug, Clone)]
struct BottomSelectionPanel {
    title: String,
    empty_label: String,
    footer: String,
    notice: Option<String>,
    rows: Vec<BottomSelectionRow>,
    query: String,
    selected: usize,
    scroll: u16,
}

#[derive(Debug, Clone)]
struct BottomSelectionRow {
    label: String,
    description: Option<String>,
    detail: Option<String>,
    group: Option<String>,
    search_text: String,
    is_current: bool,
    is_default: bool,
    style: BottomRowStyle,
    footer: Option<String>,
    value: BottomSelectionValue,
}

#[derive(Debug, Clone)]
enum BottomSelectionValue {
    Session(String),
    FetchAllModels,
    FetchProvider(String),
    ProviderInfo(String),
    Model {
        model: ConfiguredModel,
        source: ModelRowSource,
    },
    Variant {
        model: String,
        variant: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BottomRowStyle {
    Normal,
    Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelRowSource {
    Local,
    Fetched,
    CurrentOnly,
}

#[derive(Debug, Clone)]
enum BottomPanel {
    Sessions(BottomSelectionPanel),
    Models(BottomSelectionPanel),
    Variants {
        models: Box<BottomSelectionPanel>,
        panel: BottomSelectionPanel,
    },
}

impl BottomSelectionPanel {
    fn new(title: &str, _subtitle: &str, empty_label: &str, rows: Vec<BottomSelectionRow>) -> Self {
        Self {
            title: title.to_string(),
            empty_label: empty_label.to_string(),
            footer: "Enter select  Esc close  Type search".to_string(),
            notice: None,
            rows,
            query: String::new(),
            selected: 0,
            scroll: 0,
        }
    }

    fn filtered_indices(&self) -> Vec<usize> {
        let query = self.query.trim().to_lowercase();
        if self
            .rows
            .iter()
            .any(|row| matches!(row.value, BottomSelectionValue::FetchAllModels))
        {
            return self.filtered_model_indices(&query);
        }
        self.rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                if query.is_empty() || row.search_text.to_lowercase().contains(&query) {
                    Some(index)
                } else {
                    None
                }
            })
            .collect()
    }

    fn filtered_model_indices(&self, query: &str) -> Vec<usize> {
        if query.is_empty() {
            return (0..self.rows.len()).collect();
        }
        let mut include = BTreeMap::new();
        let mut provider_rows = BTreeMap::new();
        for (index, row) in self.rows.iter().enumerate() {
            match &row.value {
                BottomSelectionValue::FetchAllModels => {
                    include.insert(index, ());
                }
                BottomSelectionValue::ProviderInfo(provider) if provider == "all" => {
                    include.insert(index, ());
                }
                BottomSelectionValue::FetchProvider(provider)
                | BottomSelectionValue::ProviderInfo(provider) => {
                    provider_rows.insert(provider.clone(), index);
                    if row.search_text.to_lowercase().contains(query)
                        || row.label.to_lowercase().contains(query)
                    {
                        include.insert(index, ());
                        for (model_index, model_row) in self.rows.iter().enumerate() {
                            if let BottomSelectionValue::Model { model, .. } = &model_row.value
                                && &model.provider == provider
                            {
                                include.insert(model_index, ());
                            }
                        }
                    }
                }
                BottomSelectionValue::Model { model, .. } => {
                    if row.search_text.to_lowercase().contains(query)
                        || row.label.to_lowercase().contains(query)
                    {
                        include.insert(index, ());
                        if let Some(provider_index) = provider_rows.get(&model.provider) {
                            include.insert(*provider_index, ());
                        }
                    }
                }
                _ => {}
            }
        }
        include.into_keys().collect()
    }

    fn selected_value(&self) -> Option<BottomSelectionValue> {
        self.filtered_indices()
            .get(self.selected)
            .and_then(|index| self.rows.get(*index))
            .map(|row| row.value.clone())
    }

    fn selected_key(&self) -> String {
        self.selected_value()
            .map(|value| value.key())
            .unwrap_or_else(|| "fetch:all".to_string())
    }

    fn select_value_key(&mut self, key: &str) {
        let filtered = self.filtered_indices();
        if let Some(index) = filtered
            .iter()
            .position(|row_index| self.rows[*row_index].value.key() == key)
        {
            self.selected = index;
            self.ensure_selected_visible(8);
        }
    }

    fn footer_text(&self) -> String {
        self.filtered_indices()
            .get(self.selected)
            .and_then(|index| self.rows.get(*index))
            .and_then(|row| row.footer.clone())
            .unwrap_or_else(|| self.footer.clone())
    }

    fn set_query_char(&mut self, c: char) {
        self.query.push(c);
        self.selected = 0;
        self.scroll = 0;
        self.notice = None;
    }

    fn backspace_query(&mut self) {
        self.query.pop();
        self.selected = 0;
        self.scroll = 0;
        self.notice = None;
    }

    fn move_selection(&mut self, direction: isize) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.selected = 0;
            return;
        }
        let current = self.selected.min(len.saturating_sub(1)) as isize;
        self.selected = (current + direction).rem_euclid(len as isize) as usize;
        self.notice = None;
        self.ensure_selected_visible(8);
    }

    fn move_to(&mut self, index: usize) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.selected = 0;
            self.scroll = 0;
            return;
        }
        self.selected = index.min(len.saturating_sub(1));
        self.notice = None;
        self.ensure_selected_visible(8);
    }

    fn ensure_selected_visible(&mut self, visible_rows: u16) {
        let selected = self.selected as u16;
        if selected < self.scroll {
            self.scroll = selected;
        }
        if selected >= self.scroll.saturating_add(visible_rows) {
            self.scroll = selected.saturating_sub(visible_rows.saturating_sub(1));
        }
        self.clamp_scroll(visible_rows);
    }

    fn clamp_scroll(&mut self, visible_rows: u16) {
        let len = self.filtered_indices().len() as u16;
        let max = len.saturating_sub(visible_rows);
        self.scroll = self.scroll.min(max);
    }

    fn set_selected(&mut self, index: usize) {
        self.selected = index.min(self.filtered_indices().len().saturating_sub(1));
        self.scroll = 0;
        self.notice = None;
    }
}

impl BottomSelectionValue {
    fn key(&self) -> String {
        match self {
            BottomSelectionValue::Session(id) => format!("session:{id}"),
            BottomSelectionValue::FetchAllModels => "fetch:all".to_string(),
            BottomSelectionValue::FetchProvider(provider) => {
                format!("fetch:provider:{provider}")
            }
            BottomSelectionValue::ProviderInfo(provider) => {
                if provider == "all" {
                    "fetch:all".to_string()
                } else {
                    format!("fetch:provider:{provider}")
                }
            }
            BottomSelectionValue::Model { model, .. } => {
                format!("model:{}", format_model_spec(model))
            }
            BottomSelectionValue::Variant { model, variant } => {
                format!(
                    "variant:{model}:{}",
                    variant.as_deref().unwrap_or("default")
                )
            }
        }
    }
}

impl BottomPanel {
    fn selection(&self) -> &BottomSelectionPanel {
        match self {
            BottomPanel::Sessions(panel) | BottomPanel::Models(panel) => panel,
            BottomPanel::Variants { panel, .. } => panel,
        }
    }

    fn selection_mut(&mut self) -> &mut BottomSelectionPanel {
        match self {
            BottomPanel::Sessions(panel) | BottomPanel::Models(panel) => panel,
            BottomPanel::Variants { panel, .. } => panel,
        }
    }

    fn selected_value(&self) -> Option<BottomSelectionValue> {
        self.selection().selected_value()
    }

    fn move_selection(&mut self, direction: isize) {
        self.selection_mut().move_selection(direction);
    }
}

impl<'a> FullscreenUi<'a> {
    fn new(app: &TuiApp) -> Self {
        let mut ui = Self {
            textarea: new_textarea(),
            transcript: Vec::new(),
            assistant_row: None,
            reasoning_row: None,
            meta_row: None,
            tool_rows: BTreeMap::new(),
            history_tool_titles: BTreeMap::new(),
            turn_started: None,
            turn_provider: String::new(),
            turn_model: String::new(),
            turn_mode: app.current_mode.as_str().to_string(),
            turn_context_limit: None,
            turn_usage: None,
            turn_metadata: None,
            turn_failures: 0,
            turn_outcome: None,
            history_prompt_started_ms: None,
            thinking_visible: app.thinking_visible,
            running: None,
            scroll: 0,
            last_transcript_height: 0,
            last_transcript_width: 0,
            auto_follow_transcript: true,
            focus: FocusMode::Composer,
            selected_row: None,
            last_entry_areas: Vec::new(),
            sidebar_forced: app.state.sidebar_visible,
            sidebar_hidden: !app.state.sidebar_visible,
            last_sidebar_visible: false,
            sidebar: SidebarSnapshot::default(),
            sidebar_tokens: None,
            sidebar_context_limit: None,
            history: Vec::new(),
            history_kinds: Vec::new(),
            history_index: None,
            history_draft: None,
            history_search: false,
            history_query: String::new(),
            slash_menu_selected: 0,
            last_slash_menu_areas: Vec::new(),
            last_bottom_panel_areas: Vec::new(),
            bottom_panel: None,
            screen_lines: Vec::new(),
            selection: SelectionState::default(),
            terminal_clear_requested: false,
            quit_requested: false,
        };
        ui.refresh_sidebar(app);
        ui
    }

    fn refresh_sidebar(&mut self, app: &TuiApp) {
        let git = git_snapshot(&app.workdir);
        self.sidebar = SidebarSnapshot {
            title: app.session_sidebar_title(),
            session: app
                .current_session
                .as_deref()
                .map(short_session)
                .unwrap_or("(none)")
                .to_string(),
            workdir: tail_compact_path(&app.workdir.display().to_string(), 30),
            branch: git.branch,
            tokens: self.sidebar_tokens,
            context_percent: self.context_percent(),
            message_count: visible_transcript_message_count(&self.transcript),
            tool_count: self
                .transcript
                .iter()
                .filter(|row| {
                    matches!(
                        row.kind,
                        TranscriptKind::Explored | TranscriptKind::Ran | TranscriptKind::Changed
                    )
                })
                .count(),
            changed_files: git.changed_files,
        };
    }

    fn clear_transcript(&mut self) {
        self.transcript.clear();
        self.assistant_row = None;
        self.reasoning_row = None;
        self.meta_row = None;
        self.tool_rows.clear();
        self.history_tool_titles.clear();
        self.scroll = 0;
        self.last_transcript_height = 0;
        self.last_transcript_width = 0;
        self.auto_follow_transcript = true;
        self.selected_row = None;
        self.last_entry_areas.clear();
        self.selection = SelectionState::default();
        self.terminal_clear_requested = true;
        self.sidebar_tokens = None;
        self.sidebar_context_limit = None;
        self.history_prompt_started_ms = None;
    }

    fn take_terminal_clear_request(&mut self) -> bool {
        std::mem::take(&mut self.terminal_clear_requested)
    }

    fn set_thinking_visible(&mut self, visible: bool) {
        self.thinking_visible = visible;
        if self
            .selected_row
            .and_then(|index| self.transcript.get(index))
            .is_some_and(|row| !row_visible(row, self.thinking_visible))
        {
            self.selected_row = None;
            self.ensure_selection();
        }
        self.clamp_transcript_scroll();
    }

    fn scroll_transcript(&mut self, amount: isize) {
        if amount < 0 {
            self.scroll = self.scroll.saturating_sub(amount.unsigned_abs() as u16);
        } else {
            self.scroll = self.scroll.saturating_add(amount as u16);
        }
        self.clamp_transcript_scroll();
        self.auto_follow_transcript = amount > 0 && self.is_transcript_at_bottom();
    }

    fn clamp_transcript_scroll(&mut self) {
        self.scroll = self.scroll.min(self.max_transcript_scroll());
    }

    fn max_transcript_scroll(&self) -> u16 {
        let total = transcript_line_count(
            &self.transcript,
            self.last_transcript_width,
            self.thinking_visible,
        )
        .min(usize::from(u16::MAX)) as u16;
        total.saturating_sub(self.last_transcript_height)
    }

    fn is_transcript_at_bottom(&self) -> bool {
        self.scroll >= self.max_transcript_scroll()
    }

    fn follow_transcript_if_needed(&mut self) {
        if self.auto_follow_transcript {
            self.scroll_to_bottom();
        } else {
            self.clamp_transcript_scroll();
        }
    }

    fn context_percent(&self) -> Option<f64> {
        let tokens = self.sidebar_tokens?;
        let limit = self.sidebar_context_limit?;
        (limit > 0).then_some((tokens as f64 / limit as f64) * 100.0)
    }

    fn sidebar_enabled(&self) -> bool {
        self.sidebar_forced && !self.sidebar_hidden
    }

    fn clear_screen_lines(&mut self) {
        self.screen_lines.clear();
    }

    #[cfg(test)]
    fn push_screen_line(&mut self, x: u16, y: u16, text: impl Into<String>) {
        let text = text.into();
        self.screen_lines.push(ScreenLine {
            region: SelectableRegion::Transcript,
            y,
            cells: screen_cells_from_text(x, &text),
        });
    }

    fn capture_selectable_rows(
        &mut self,
        buffer: &ratatui::buffer::Buffer,
        area: Rect,
        region: SelectableRegion,
    ) {
        let area = buffer.area().intersection(area);
        if area.is_empty() {
            return;
        }
        for y in area.y..area.y.saturating_add(area.height) {
            if let Some(line) = screen_line_from_buffer(buffer, area.x, y, area.width, region) {
                self.screen_lines.push(line);
            }
        }
    }

    fn selectable_hit(&self, column: u16, row: u16) -> bool {
        self.screen_lines.iter().any(|line| {
            line.y == row
                && line
                    .cells
                    .iter()
                    .any(|cell| column >= cell.x && column < cell.x.saturating_add(cell.width))
        })
    }

    fn selection_region_at(&self, column: u16, row: u16) -> Option<SelectableRegion> {
        self.screen_lines
            .iter()
            .find(|line| {
                line.y == row
                    && line
                        .cells
                        .iter()
                        .any(|cell| column >= cell.x && column < cell.x.saturating_add(cell.width))
            })
            .map(|line| line.region)
    }

    fn start_selection(&mut self, column: u16, row: u16) {
        self.selection.anchor = Some((column, row));
        self.selection.focus = Some((column, row));
        self.selection.region = self.selection_region_at(column, row);
    }

    fn update_selection(&mut self, column: u16, row: u16) {
        if self.selection.anchor.is_some() {
            self.selection.focus = Some((column, row));
        }
    }

    fn clear_selection(&mut self) {
        self.selection = SelectionState::default();
    }

    fn selected_text(&self) -> Option<String> {
        selected_text_from_lines(&self.screen_lines, &self.selection)
    }

    fn push_history_message(
        &mut self,
        message: &Value,
        usage: Option<&Value>,
        metadata: Option<&Value>,
    ) {
        match message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "user" => {
                if let Some(text) = user_text_from_message(message) {
                    self.push_user(text);
                }
                self.history_prompt_started_ms = message_timestamp_ms(message);
            }
            "assistant" => {
                for (tool_call_id, title) in history_tool_titles_from_message(message) {
                    self.history_tool_titles.insert(tool_call_id, title);
                }
                if let Some(reasoning) = assistant_reasoning_from_message(message) {
                    self.transcript.push(TranscriptRow::with_title(
                        TranscriptKind::Thinking,
                        "Thinking",
                        reasoning,
                    ));
                }
                let has_answer = if let Some(text) = assistant_text_from_message(message) {
                    self.transcript.push(TranscriptRow::with_title(
                        TranscriptKind::Answer,
                        "",
                        text,
                    ));
                    true
                } else {
                    false
                };
                if let Some(total) = usage.and_then(usage_total_tokens) {
                    self.sidebar_tokens = Some(total);
                }
                if has_answer
                    && let Some(meta) =
                        history_meta_text(message, usage, metadata, self.history_prompt_started_ms)
                {
                    self.transcript
                        .push(TranscriptRow::with_title(TranscriptKind::Meta, "", meta));
                }
                self.history_prompt_started_ms = None;
            }
            "tool_result" => self.push_history_tool_result(message, metadata),
            _ => {}
        }
    }

    fn push_history_tool_result(&mut self, message: &Value, metadata: Option<&Value>) {
        let tool = message
            .get("tool_name")
            .and_then(Value::as_str)
            .unwrap_or("tool");
        let is_error = message
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let content = message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let tool_call_id = message
            .get("tool_call_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        let result = serde_json::from_str::<Value>(content)
            .unwrap_or_else(|_| serde_json::json!({ "content": content }));
        let value = serde_json::json!({
            "tool_name": tool,
            "result": result,
            "outcome": if is_error { "failed" } else { "normal" }
        });
        let title = self
            .history_tool_titles
            .get(tool_call_id)
            .cloned()
            .unwrap_or_else(|| tool_title(tool, &value));
        let mut row = TranscriptRow::with_title(evidence_kind(tool), title, "");
        row.failed = is_error;
        row.tool_elapsed = metadata_elapsed_duration(metadata);
        let (collapsed, full) = tool_output_text(&value);
        row.text = if collapsed.is_empty() {
            format_tool_summary(&value)
        } else {
            collapsed
        };
        row.full_text = full;
        self.transcript.push(row);
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll = self.max_transcript_scroll();
        self.auto_follow_transcript = true;
    }

    fn complete_slash_command(&mut self) {
        let input = textarea_text(&self.textarea);
        if let Some(completed) = slash_completion(&input) {
            self.textarea = textarea_with_text(&completed);
            self.slash_menu_selected = 0;
        }
    }

    fn clamp_slash_menu_selection(&mut self, len: usize) {
        if len == 0 {
            self.slash_menu_selected = 0;
            self.last_slash_menu_areas.clear();
            return;
        }
        self.slash_menu_selected = self.slash_menu_selected.min(len.saturating_sub(1));
    }

    fn move_slash_menu_selection(&mut self, direction: isize, len: usize) {
        if len == 0 {
            self.slash_menu_selected = 0;
            return;
        }
        let current = self.slash_menu_selected.min(len.saturating_sub(1)) as isize;
        let next = (current + direction).rem_euclid(len as isize) as usize;
        self.slash_menu_selected = next;
    }

    fn set_slash_menu_selection(&mut self, index: usize, len: usize) {
        self.slash_menu_selected = if len == 0 {
            0
        } else {
            index.min(len.saturating_sub(1))
        };
    }

    fn slash_menu_hit(&self, column: u16, row: u16) -> Option<usize> {
        self.last_slash_menu_areas
            .iter()
            .find(|(_, area)| rect_contains(*area, column, row))
            .map(|(index, _)| *index)
    }

    fn bottom_panel_hit(&self, column: u16, row: u16) -> Option<usize> {
        self.last_bottom_panel_areas
            .iter()
            .find(|(_, area)| rect_contains(*area, column, row))
            .map(|(index, _)| *index)
    }

    fn set_bottom_panel_notice(&mut self, text: impl Into<String>) {
        if let Some(panel) = &mut self.bottom_panel {
            panel.selection_mut().notice = Some(text.into());
        }
    }

    fn push_user(&mut self, text: String) {
        self.transcript
            .push(TranscriptRow::with_title(TranscriptKind::Prompt, "", text));
    }

    fn start_assistant(&mut self) {
        self.assistant_row = None;
        self.reasoning_row = None;
        self.meta_row = None;
        self.tool_rows.clear();
        self.turn_started = None;
        self.turn_provider.clear();
        self.turn_model.clear();
        self.turn_context_limit = None;
        self.turn_usage = None;
        self.turn_metadata = None;
        self.turn_failures = 0;
        self.turn_outcome = None;
    }

    fn push_status(&mut self, text: impl Into<String>) {
        self.transcript
            .push(TranscriptRow::simple(TranscriptKind::Status, text));
    }

    fn push_error(&mut self, text: impl Into<String>) {
        self.transcript
            .push(TranscriptRow::simple(TranscriptKind::Error, text));
    }

    fn insert_transcript_row(&mut self, index: usize, row: TranscriptRow) -> usize {
        let index = index.min(self.transcript.len());
        self.transcript.insert(index, row);
        increment_row_index(&mut self.assistant_row, index);
        increment_row_index(&mut self.reasoning_row, index);
        increment_row_index(&mut self.meta_row, index);
        increment_row_index(&mut self.selected_row, index);
        for row_index in self.tool_rows.values_mut() {
            if *row_index >= index {
                *row_index += 1;
            }
        }
        index
    }

    fn insert_evidence_row(&mut self, row: TranscriptRow) -> usize {
        let index = self
            .assistant_row
            .or(self.meta_row)
            .unwrap_or(self.transcript.len());
        self.insert_transcript_row(index, row)
    }

    fn insert_answer_row(&mut self, row: TranscriptRow) -> usize {
        let index = self.meta_row.unwrap_or(self.transcript.len());
        self.insert_transcript_row(index, row)
    }

    fn apply_stream_event(&mut self, event: RunStreamEvent, _thinking_visible: bool, debug: bool) {
        match event {
            RunStreamEvent::ReasoningDelta { text } => {
                let idx = self.reasoning_row.unwrap_or_else(|| {
                    let idx = self.insert_evidence_row(TranscriptRow::with_title(
                        TranscriptKind::Thinking,
                        "Thinking",
                        String::new(),
                    ));
                    self.reasoning_row = Some(idx);
                    idx
                });
                self.transcript[idx].text.push_str(&text);
            }
            RunStreamEvent::ReasoningEnd => {
                self.reasoning_row = None;
            }
            RunStreamEvent::Event(value) => self.apply_value_event(&value, debug),
        }
    }

    fn apply_value_event(&mut self, value: &Value, debug: bool) {
        match value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "run_start" => {
                self.turn_started = Some(Instant::now());
                self.turn_provider = value
                    .get("provider")
                    .and_then(Value::as_str)
                    .unwrap_or("provider")
                    .to_string();
                self.turn_model = value
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("model")
                    .to_string();
                self.turn_mode = value
                    .get("mode")
                    .and_then(Value::as_str)
                    .unwrap_or("default")
                    .to_string();
                self.turn_context_limit = value.get("context_limit").and_then(Value::as_u64);
                self.sidebar_context_limit = self.turn_context_limit;
            }
            "message_update" | "message_end" => {
                if let Some(text) =
                    assistant_text_from_event(value).filter(|text| !text.trim().is_empty())
                {
                    let idx = self.assistant_row.unwrap_or_else(|| {
                        let idx = self.insert_answer_row(TranscriptRow::with_title(
                            TranscriptKind::Answer,
                            "",
                            String::new(),
                        ));
                        self.assistant_row = Some(idx);
                        idx
                    });
                    self.transcript[idx].text = text;
                }
                if value.get("type").and_then(Value::as_str) == Some("message_end") {
                    self.turn_usage = value.get("usage").cloned();
                    self.sidebar_tokens = self.turn_usage.as_ref().and_then(usage_total_tokens);
                    self.turn_metadata = value.get("metadata").cloned();
                    self.update_turn_meta(debug);
                }
            }
            "tool_execution_start" => {
                let tool = value
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                let tool_call_id = value
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let mut row = TranscriptRow::with_title(
                    evidence_kind(tool),
                    tool_title(tool, value),
                    "running",
                );
                row.tool_call_id = (!tool_call_id.is_empty()).then_some(tool_call_id.clone());
                row.tool_started = Some(tool_started_instant(value));
                let idx = self.insert_evidence_row(row);
                if !tool_call_id.is_empty() {
                    self.tool_rows.insert(tool_call_id, idx);
                }
            }
            "tool_execution_end" => {
                let tool = value
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                let tool_call_id = value
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let outcome = value
                    .get("outcome")
                    .and_then(Value::as_str)
                    .unwrap_or("normal");
                let failed = outcome != "normal";
                if failed {
                    self.turn_failures += 1;
                }
                let idx = self
                    .tool_rows
                    .get(tool_call_id)
                    .copied()
                    .unwrap_or_else(|| {
                        self.insert_evidence_row(TranscriptRow::with_title(
                            evidence_kind(tool),
                            tool_title(tool, value),
                            String::new(),
                        ))
                    });
                let row = &mut self.transcript[idx];
                row.kind = evidence_kind(tool);
                row.title = tool_title_for_update(tool, value, &row.title);
                row.failed = failed;
                row.tool_elapsed = metadata_elapsed_duration(Some(value))
                    .or_else(|| row.tool_started.map(|started| started.elapsed()));
                row.tool_started = None;
                let (collapsed, full) = tool_output_text(value);
                row.text = if collapsed.is_empty() {
                    format_tool_summary(value)
                } else {
                    collapsed
                };
                row.full_text = full;
                self.update_turn_meta(debug);
            }
            "agent_end" => {
                self.turn_outcome = outcome_from_value(value);
            }
            _ => {}
        }
    }

    fn finish_turn(&mut self) {
        self.assistant_row = None;
        self.reasoning_row = None;
        self.meta_row = None;
        self.tool_rows.clear();
        self.turn_outcome = None;
        self.focus = FocusMode::Composer;
    }

    fn replace_session_history_prompts(&mut self, prompts: Vec<String>) {
        let process_commands = self
            .history
            .iter()
            .zip(self.history_kinds.iter())
            .filter_map(|(entry, kind)| {
                (*kind == ComposerHistoryKind::ProcessCommand).then_some(entry.clone())
            })
            .collect::<Vec<_>>();
        self.history = prompts;
        self.history_kinds = vec![ComposerHistoryKind::SessionPrompt; self.history.len()];
        for command in process_commands {
            self.history.push(command);
            self.history_kinds.push(ComposerHistoryKind::ProcessCommand);
        }
        self.reset_history_navigation();
    }

    fn push_submitted_history(&mut self, submitted: String) {
        let kind = if submitted.trim_start().starts_with('/') {
            ComposerHistoryKind::ProcessCommand
        } else {
            ComposerHistoryKind::SessionPrompt
        };
        self.history.push(submitted);
        self.history_kinds.push(kind);
        self.reset_history_navigation();
    }

    fn reset_history_navigation(&mut self) {
        self.history_index = None;
        self.history_draft = None;
    }

    fn can_recall_history_previous(&self) -> bool {
        !self.history.is_empty() && self.textarea.cursor().0 == 0
    }

    fn can_recall_history_next(&self) -> bool {
        self.history_index.is_some() && self.textarea.cursor().0 + 1 >= self.textarea.lines().len()
    }

    fn clear_history_navigation_for_edit(&mut self) {
        if self.history_index.is_some() {
            self.history_index = None;
            self.history_draft = None;
        }
    }

    fn recall_history(&mut self, direction: isize) {
        if self.history.is_empty() {
            return;
        }
        if self.history_index.is_none() && direction < 0 {
            self.history_draft = Some(textarea_text(&self.textarea));
        }
        let next = match self.history_index {
            None if direction < 0 => self.history.len().saturating_sub(1),
            None => return,
            Some(index) if direction < 0 => index.saturating_sub(1),
            Some(index) => {
                if index + 1 >= self.history.len() {
                    self.history_index = None;
                    self.textarea = match self.history_draft.take() {
                        Some(draft) if !draft.is_empty() => textarea_with_text(&draft),
                        _ => new_textarea(),
                    };
                    return;
                }
                index + 1
            }
        };
        self.history_index = Some(next);
        self.textarea = textarea_with_text(&self.history[next]);
    }

    fn update_turn_meta(&mut self, debug: bool) {
        if self.assistant_row.is_none() && self.turn_failures == 0 {
            return;
        }
        let meta = turn_meta_text(TurnMetaProjection {
            mode: &self.turn_mode,
            provider: &self.turn_provider,
            model: &self.turn_model,
            started: self.turn_started,
            usage: self.turn_usage.as_ref(),
            metadata: self.turn_metadata.as_ref(),
            failures: self.turn_failures,
            debug,
        });
        if meta.is_empty() {
            return;
        }
        let idx = self.meta_row.unwrap_or_else(|| {
            let idx = self.transcript.len();
            self.transcript.push(TranscriptRow::with_title(
                TranscriptKind::Meta,
                "",
                String::new(),
            ));
            self.meta_row = Some(idx);
            idx
        });
        self.transcript[idx].text = meta;
    }

    fn ensure_selection(&mut self) {
        if self.selected_row.is_some_and(|idx| {
            self.transcript
                .get(idx)
                .is_some_and(|row| row_visible(row, self.thinking_visible))
        }) {
            return;
        }
        self.selected_row = self
            .transcript
            .iter()
            .position(|row| row_visible(row, self.thinking_visible) && row.is_expandable())
            .or_else(|| {
                self.transcript
                    .iter()
                    .rposition(|row| row_visible(row, self.thinking_visible))
            });
    }

    fn move_selection(&mut self, direction: isize) {
        if self.transcript.is_empty() {
            self.selected_row = None;
            return;
        }
        self.auto_follow_transcript = false;
        self.ensure_selection();
        let visible = self
            .transcript
            .iter()
            .enumerate()
            .filter_map(|(index, row)| row_visible(row, self.thinking_visible).then_some(index))
            .collect::<Vec<_>>();
        if visible.is_empty() {
            self.selected_row = None;
            return;
        }
        let current_position = self
            .selected_row
            .and_then(|current| visible.iter().position(|index| *index == current))
            .unwrap_or(0);
        let next_position = if direction < 0 {
            current_position.saturating_sub(direction.unsigned_abs())
        } else {
            current_position
                .saturating_add(direction as usize)
                .min(visible.len().saturating_sub(1))
        };
        self.selected_row = visible.get(next_position).copied();
    }

    fn toggle_selected(&mut self) {
        self.auto_follow_transcript = false;
        if let Some(index) = self.selected_row
            && let Some(row) = self.transcript.get_mut(index)
            && row_visible(row, self.thinking_visible)
            && row.is_expandable()
        {
            row.expanded = !row.expanded;
        }
    }
}

fn default_title(kind: TranscriptKind) -> &'static str {
    match kind {
        TranscriptKind::Prompt => "",
        TranscriptKind::Answer => "",
        TranscriptKind::Thinking => "Thinking",
        TranscriptKind::Explored => "Explored",
        TranscriptKind::Ran => "Ran",
        TranscriptKind::Changed => "Changed",
        TranscriptKind::Meta => "",
        TranscriptKind::Status => "Status",
        TranscriptKind::Error => "Error",
    }
}

fn user_text_from_message(message: &Value) -> Option<String> {
    let text = message
        .get("content")?
        .as_array()?
        .iter()
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

fn visible_tui_message_count(messages: &[TuiMessageSummary]) -> Result<usize> {
    messages.iter().try_fold(0, |count, summary| {
        let message = serde_json::to_value(&summary.message)?;
        Ok(count + visible_message_count_from_value(&message))
    })
}

fn visible_message_count_from_value(message: &Value) -> usize {
    match message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "user" => usize::from(user_text_from_message(message).is_some()),
        "assistant" => usize::from(assistant_text_from_message(message).is_some()),
        _ => 0,
    }
}

fn visible_transcript_message_count(rows: &[TranscriptRow]) -> usize {
    rows.iter()
        .filter(|row| matches!(row.kind, TranscriptKind::Prompt | TranscriptKind::Answer))
        .count()
}

fn assistant_text_from_message(message: &Value) -> Option<String> {
    let text = message
        .get("content")?
        .as_array()?
        .iter()
        .filter_map(|block| {
            (block.get("type").and_then(Value::as_str) == Some("text"))
                .then(|| block.get("text").and_then(Value::as_str))
                .flatten()
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

fn assistant_reasoning_from_message(message: &Value) -> Option<String> {
    let text = message
        .get("content")?
        .as_array()?
        .iter()
        .filter_map(|block| {
            (block.get("type").and_then(Value::as_str) == Some("reasoning"))
                .then(|| block.get("text").and_then(Value::as_str))
                .flatten()
        })
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

fn history_tool_titles_from_message(message: &Value) -> Vec<(String, String)> {
    let Some(content) = message.get("content").and_then(Value::as_array) else {
        return Vec::new();
    };
    content
        .iter()
        .filter_map(|block| {
            if block.get("type").and_then(Value::as_str) != Some("tool_call") {
                return None;
            }
            let id = block.get("id").and_then(Value::as_str)?;
            let name = block.get("name").and_then(Value::as_str)?;
            let args = tool_call_args_from_block(block);
            let value = serde_json::json!({ "args": args });
            Some((id.to_string(), tool_title(name, &value)))
        })
        .collect()
}

fn tool_call_args_from_block(block: &Value) -> Value {
    block
        .get("arguments")
        .cloned()
        .or_else(|| {
            block
                .get("arguments_json")
                .and_then(Value::as_str)
                .and_then(|raw| serde_json::from_str(raw).ok())
        })
        .unwrap_or(Value::Null)
}

fn message_timestamp_ms(message: &Value) -> Option<i64> {
    message.get("timestamp_ms").and_then(Value::as_i64)
}

fn outcome_from_value(value: &Value) -> Option<Outcome> {
    match value.get("outcome").and_then(Value::as_str)? {
        "normal" => Some(Outcome::Normal),
        "stopped" => Some(Outcome::Stopped),
        "failed" => Some(Outcome::Failed),
        "aborted" => Some(Outcome::Aborted),
        _ => None,
    }
}

fn history_meta_text(
    message: &Value,
    _usage: Option<&Value>,
    metadata: Option<&Value>,
    prompt_started_ms: Option<i64>,
) -> Option<String> {
    let provider = message
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or("");
    let model = message.get("model").and_then(Value::as_str).unwrap_or("");
    let mut parts = Vec::new();
    if !provider.is_empty() || !model.is_empty() {
        parts.push(model_meta_label(provider, model, metadata));
    }
    if let Some(elapsed) = history_elapsed_duration(message, metadata, prompt_started_ms) {
        parts.push(format_duration_s(elapsed));
    }
    (!parts.is_empty()).then(|| parts.join("  "))
}

fn history_elapsed_duration(
    message: &Value,
    metadata: Option<&Value>,
    prompt_started_ms: Option<i64>,
) -> Option<Duration> {
    if let Some(elapsed) = metadata_elapsed_duration(metadata) {
        return Some(elapsed);
    }
    let started = prompt_started_ms?;
    let ended = message_timestamp_ms(message)?;
    let elapsed = ended.checked_sub(started)?;
    (elapsed >= 0).then_some(Duration::from_millis(elapsed as u64))
}

fn metadata_elapsed_duration(metadata: Option<&Value>) -> Option<Duration> {
    metadata
        .and_then(|metadata| metadata.get("elapsed_ms"))
        .and_then(Value::as_u64)
        .map(Duration::from_millis)
}

fn metadata_reasoning_effort(metadata: Option<&Value>) -> Option<&str> {
    metadata
        .and_then(|metadata| metadata.get("reasoning_effort"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "none")
}

fn tool_started_instant(value: &Value) -> Instant {
    let now = Instant::now();
    let Some(started_at_ms) = value.get("started_at_ms").and_then(Value::as_i64) else {
        return now;
    };
    let Some(elapsed_ms) = wall_now_ms().checked_sub(started_at_ms) else {
        return now;
    };
    if elapsed_ms <= 0 {
        return now;
    }
    now.checked_sub(Duration::from_millis(elapsed_ms as u64))
        .unwrap_or(now)
}

fn wall_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn row_visible(row: &TranscriptRow, thinking_visible: bool) -> bool {
    thinking_visible || row.kind != TranscriptKind::Thinking
}

fn next_visible_row(
    rows: &[TranscriptRow],
    index: usize,
    thinking_visible: bool,
) -> Option<&TranscriptRow> {
    rows.iter()
        .skip(index + 1)
        .find(|row| row_visible(row, thinking_visible))
}

fn compact_trailing_for(
    rows: &[TranscriptRow],
    index: usize,
    row: &TranscriptRow,
    thinking_visible: bool,
) -> bool {
    next_visible_row(rows, index, thinking_visible)
        .is_some_and(|next| row.kind == TranscriptKind::Answer && next.kind == TranscriptKind::Meta)
}

fn transcript_line_count(rows: &[TranscriptRow], width: u16, thinking_visible: bool) -> usize {
    rows.iter()
        .enumerate()
        .filter(|(_, row)| row_visible(row, thinking_visible))
        .map(|(index, row)| {
            let compact_trailing = compact_trailing_for(rows, index, row, thinking_visible);
            let lines = transcript_lines(row, false, compact_trailing, width);
            wrapped_line_count(&lines, width)
        })
        .sum()
}

fn wrapped_line_count(lines: &[Line<'_>], width: u16) -> usize {
    let width = usize::from(width.max(1));
    lines
        .iter()
        .map(|line| {
            let line_width = line.width();
            if line_width == 0 {
                1
            } else {
                line_width.div_ceil(width)
            }
        })
        .sum()
}

impl ScreenLine {
    #[cfg(test)]
    fn first_x(&self) -> u16 {
        self.cells.first().map(|cell| cell.x).unwrap_or(0)
    }

    #[cfg(test)]
    fn text(&self) -> String {
        self.cells
            .iter()
            .map(|cell| cell.text.as_str())
            .collect::<String>()
    }
}

#[cfg(test)]
fn screen_cells_from_text(start_x: u16, text: &str) -> Vec<ScreenCell> {
    let mut cells: Vec<ScreenCell> = Vec::new();
    let mut x = start_x;
    for ch in text.chars() {
        let width = ch.width().unwrap_or(0) as u16;
        if width == 0 {
            if let Some(cell) = cells.last_mut() {
                cell.text.push(ch);
            }
            continue;
        }
        cells.push(ScreenCell {
            x,
            width,
            text: ch.to_string(),
        });
        x = x.saturating_add(width);
    }
    cells
}

fn screen_line_from_buffer(
    buffer: &ratatui::buffer::Buffer,
    start_x: u16,
    y: u16,
    width: u16,
    region: SelectableRegion,
) -> Option<ScreenLine> {
    let mut cells = Vec::new();
    let right = start_x.saturating_add(width);
    let mut x = start_x;
    while x < right {
        let Some(cell) = buffer.cell((x, y)) else {
            break;
        };
        let symbol = cell.symbol();
        let symbol_width = UnicodeWidthStr::width(symbol).max(1) as u16;
        let width = symbol_width.min(right.saturating_sub(x)).max(1);
        cells.push(ScreenCell {
            x,
            width,
            text: symbol.to_string(),
        });
        x = x.saturating_add(width);
    }
    trim_screen_padding_right(&mut cells);
    (!cells.is_empty()).then_some(ScreenLine { region, y, cells })
}

fn trim_screen_padding_right(cells: &mut Vec<ScreenCell>) {
    while cells.last().is_some_and(|cell| cell.text == " ") {
        cells.pop();
    }
}

fn selected_text_from_lines(lines: &[ScreenLine], selection: &SelectionState) -> Option<String> {
    let anchor = selection.anchor?;
    let focus = selection.focus?;
    if anchor == focus {
        return None;
    }
    let ((start_x, start_y), (end_x, end_y)) = ordered_selection(anchor, focus);
    let mut pieces = Vec::new();
    for y in start_y..=end_y {
        let from = if y == start_y { start_x } else { 0 };
        let to = if y == end_y { end_x } else { u16::MAX };
        let mut segments = lines
            .iter()
            .filter(|line| selection.region.is_none_or(|region| line.region == region))
            .filter(|line| line.y == y)
            .filter_map(|line| selected_segment_from_line(line, from, to))
            .collect::<Vec<_>>();
        segments.sort_by_key(|segment| segment.start_x);
        let mut row_text = String::new();
        let mut cursor = None;
        for segment in segments {
            if let Some(cursor_x) = cursor
                && segment.start_x > cursor_x
            {
                row_text.push_str(&" ".repeat(usize::from(segment.start_x - cursor_x)));
            }
            row_text.push_str(&segment.text);
            cursor = Some(segment.end_x);
        }
        if !row_text.is_empty() {
            pieces.push(row_text);
        }
    }
    (!pieces.is_empty()).then(|| pieces.join("\n"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectedSegment {
    start_x: u16,
    end_x: u16,
    text: String,
}

fn selected_segment_from_line(line: &ScreenLine, from: u16, to: u16) -> Option<SelectedSegment> {
    if to <= from {
        return None;
    }
    let mut text = String::new();
    let mut start_x = None;
    let mut end_x = None;
    let mut cursor = None;
    for cell in &line.cells {
        if !cell_overlaps_range(cell, from, to) {
            continue;
        }
        if start_x.is_none() {
            start_x = Some(cell.x);
        }
        if let Some(cursor_x) = cursor
            && cell.x > cursor_x
        {
            text.push_str(&" ".repeat(usize::from(cell.x - cursor_x)));
        }
        text.push_str(&cell.text);
        let cell_end = cell.x.saturating_add(cell.width);
        end_x = Some(cell_end);
        cursor = Some(cell_end);
    }
    Some(SelectedSegment {
        start_x: start_x?,
        end_x: end_x?,
        text,
    })
}

fn cell_overlaps_range(cell: &ScreenCell, from: u16, to: u16) -> bool {
    let cell_end = cell.x.saturating_add(cell.width);
    to > cell.x && from < cell_end
}

fn ordered_selection(anchor: (u16, u16), focus: (u16, u16)) -> ((u16, u16), (u16, u16)) {
    let (ax, ay) = anchor;
    let (fx, fy) = focus;
    if (ay, ax) <= (fy, fx) {
        ((ax, ay), (fx, fy))
    } else {
        ((fx, fy), (ax, ay))
    }
}

fn slash_completion(input: &str) -> Option<String> {
    if input.contains('\n') {
        return None;
    }
    let leading_len = input.len().saturating_sub(input.trim_start().len());
    let leading = &input[..leading_len];
    let typed = input.trim_start();
    if !typed.starts_with('/') {
        return None;
    }
    let items = slash_menu_items(typed);
    if items.is_empty() {
        return None;
    }
    let commands = items.iter().map(|item| item.command).collect::<Vec<_>>();
    let common = common_prefix(&commands);
    let completed = if common.len() > typed.len() {
        common
    } else if commands.contains(&typed) || commands.len() > 1 {
        return None;
    } else {
        commands[0].to_string()
    };
    (completed != typed).then(|| format!("{leading}{completed}"))
}

fn selected_slash_menu_command(input: &str, selected_index: usize) -> Option<&'static str> {
    if input.contains('\n') {
        return None;
    }
    let typed = input.trim_start();
    slash_menu_items(typed)
        .get(selected_index)
        .map(|item| item.command)
}

fn common_prefix(values: &[&str]) -> String {
    let Some(first) = values.first() else {
        return String::new();
    };
    let mut end = first.len();
    for value in values.iter().skip(1) {
        end = first
            .as_bytes()
            .iter()
            .zip(value.as_bytes())
            .take_while(|(left, right)| left == right)
            .count()
            .min(end);
    }
    first[..end].to_string()
}

fn evidence_kind(tool: &str) -> TranscriptKind {
    match tool {
        "read" | "list" | "search" => TranscriptKind::Explored,
        "bash" => TranscriptKind::Ran,
        "write" | "edit" => TranscriptKind::Changed,
        _ => TranscriptKind::Status,
    }
}

fn tool_title(tool: &str, value: &Value) -> String {
    let args = value.get("args").unwrap_or(&Value::Null);
    let result = value.get("result").unwrap_or(&Value::Null);
    match tool {
        "read" | "list" => format!("Explored {}", path_from(args, result).unwrap_or(".")),
        "search" => {
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .or_else(|| result.get("query").and_then(Value::as_str))
                .unwrap_or("text");
            format!("Explored search {query}")
        }
        "bash" => {
            let command = args
                .get("command")
                .and_then(Value::as_str)
                .and_then(first_non_empty_line)
                .unwrap_or("command");
            format!("Ran {command}")
        }
        "write" | "edit" => format!("Changed {}", path_from(args, result).unwrap_or("files")),
        other => format!("Tool {other}"),
    }
}

fn tool_title_for_update(tool: &str, value: &Value, existing_title: &str) -> String {
    let title = tool_title(tool, value);
    if tool == "bash" && title == "Ran command" && existing_title.starts_with("Ran ") {
        existing_title.to_string()
    } else {
        title
    }
}

fn first_non_empty_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| !line.is_empty())
}

fn path_from<'a>(args: &'a Value, result: &'a Value) -> Option<&'a str> {
    args.get("path")
        .and_then(Value::as_str)
        .or_else(|| result.get("path").and_then(Value::as_str))
}

fn tool_output_text(value: &Value) -> (String, Option<String>) {
    let result = value.get("result").unwrap_or(&Value::Null);
    let full = result
        .get("content")
        .and_then(Value::as_str)
        .or_else(|| result.get("output").and_then(Value::as_str))
        .or_else(|| result.get("diff").and_then(Value::as_str))
        .or_else(|| result.get("error").and_then(Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| format_tool_summary(value));
    collapse_lines(&full, 20)
}

fn collapse_lines(text: &str, max_lines: usize) -> (String, Option<String>) {
    let lines = text.lines().collect::<Vec<_>>();
    if lines.len() <= max_lines {
        return (text.to_string(), None);
    }
    let collapsed = lines
        .iter()
        .take(max_lines)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    (
        format!("{collapsed}\n... {} more lines", lines.len() - max_lines),
        Some(text.to_string()),
    )
}

fn model_label(provider: &str, model: &str) -> String {
    match (provider.is_empty(), model.is_empty()) {
        (false, false) => format!("{provider}/{model}"),
        (false, true) => provider.to_string(),
        (true, false) => model.to_string(),
        (true, true) => String::new(),
    }
}

fn model_meta_label(provider: &str, model: &str, metadata: Option<&Value>) -> String {
    let label = model_label(provider, model);
    match metadata_reasoning_effort(metadata) {
        Some(reasoning_effort) if !label.is_empty() => format!("{label} {reasoning_effort}"),
        Some(reasoning_effort) => reasoning_effort.to_string(),
        None => label,
    }
}

struct TurnMetaProjection<'a> {
    mode: &'a str,
    provider: &'a str,
    model: &'a str,
    started: Option<Instant>,
    usage: Option<&'a Value>,
    metadata: Option<&'a Value>,
    failures: usize,
    debug: bool,
}

fn turn_meta_text(meta: TurnMetaProjection<'_>) -> String {
    let mut parts = Vec::new();
    if !meta.provider.is_empty() || !meta.model.is_empty() {
        parts.push(model_meta_label(meta.provider, meta.model, meta.metadata));
    }
    if let Some(elapsed) = metadata_elapsed_duration(meta.metadata)
        .or_else(|| meta.started.map(|started| started.elapsed()))
    {
        parts.push(format_duration_s(elapsed));
    }
    if meta.failures > 0 {
        let suffix = if meta.failures == 1 {
            "failure"
        } else {
            "failures"
        };
        parts.push(format!("{} {suffix}", meta.failures));
    }
    if meta.debug {
        if let Some(usage) = meta.usage {
            let mut usage_parts = Vec::new();
            for (key, label) in [
                ("input_tokens", "input"),
                ("output_tokens", "output"),
                ("reasoning_tokens", "reasoning"),
                ("cached_tokens", "cached"),
            ] {
                if let Some(value) = usage.get(key).and_then(Value::as_u64) {
                    usage_parts.push(format!("{value} {label}"));
                }
            }
            if !usage_parts.is_empty() {
                parts.push(format!("usage {}", usage_parts.join(" ")));
            }
        }
        if let Some(metadata) = meta.metadata.and_then(Value::as_object)
            && !metadata.is_empty()
        {
            let summary = metadata
                .iter()
                .filter(|(key, _)| !matches!(key.as_str(), "elapsed_ms" | "reasoning_effort"))
                .take(5)
                .map(|(key, value)| format!("{} {}", metadata_label(key), compact_value(value)))
                .collect::<Vec<_>>()
                .join(" ");
            if !summary.is_empty() {
                parts.push(format!("metadata {summary}"));
            }
        }
    }
    if !meta.mode.is_empty() && meta.mode != "default" {
        parts.push(meta.mode.to_string());
    }
    parts.join("  ")
}

fn usage_total_tokens(usage: &Value) -> Option<u64> {
    usage.get("total_tokens").and_then(Value::as_u64)
}

fn format_duration_s(duration: Duration) -> String {
    format!("{:.1}s", duration.as_secs_f64())
}

fn metadata_label(key: &str) -> &str {
    match key {
        "provider_response_id" => "response",
        "system_fingerprint" => "fingerprint",
        "model" => "model",
        other => other,
    }
}

fn format_count(value: u64) -> String {
    let text = value.to_string();
    let mut out = String::new();
    for (index, ch) in text.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn compact_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".to_string()),
    }
}

struct GitSnapshot {
    branch: String,
    changed_files: Vec<String>,
}

fn git_snapshot(workdir: &PathBuf) -> GitSnapshot {
    let branch = StdCommand::new("git")
        .arg("-C")
        .arg(workdir)
        .args(["branch", "--show-current"])
        .output()
        .ok()
        .and_then(|output| {
            output
                .status
                .success()
                .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "(none)".to_string());
    let changed_files = StdCommand::new("git")
        .arg("-C")
        .arg(workdir)
        .args(["status", "--short"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .take(10)
                .map(tail_compact_status_line)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    GitSnapshot {
        branch,
        changed_files,
    }
}

fn tail_compact_status_line(line: &str) -> String {
    let mut parts = line.split_whitespace().collect::<Vec<_>>();
    let Some(path) = parts.pop() else {
        return line.to_string();
    };
    let prefix = line
        .strip_suffix(path)
        .map(str::trim_end)
        .filter(|value| !value.is_empty())
        .unwrap_or("");
    let compact = tail_compact_path(path, 32);
    if prefix.is_empty() {
        compact
    } else {
        format!("{prefix} {compact}")
    }
}

fn tail_compact_path(path: &str, max_chars: usize) -> String {
    if path.chars().count() <= max_chars {
        return path.to_string();
    }
    let tail = path
        .chars()
        .rev()
        .take(max_chars.saturating_sub(3))
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    format!("...{tail}")
}

fn new_textarea<'a>() -> TextArea<'a> {
    let mut textarea = TextArea::default();
    textarea.set_block(Block::default().style(Style::default().bg(TUI_SURFACE_BG)));
    textarea.set_style(Style::default().bg(TUI_SURFACE_BG));
    textarea.set_wrap_mode(WrapMode::WordOrGlyph);
    textarea.set_cursor_line_style(Style::default());
    textarea
}

fn textarea_with_text<'a>(text: &str) -> TextArea<'a> {
    let mut textarea = TextArea::new(text.split('\n').map(ToString::to_string).collect());
    textarea.set_block(Block::default().style(Style::default().bg(TUI_SURFACE_BG)));
    textarea.set_style(Style::default().bg(TUI_SURFACE_BG));
    textarea.set_wrap_mode(WrapMode::WordOrGlyph);
    textarea.set_cursor_line_style(Style::default());
    textarea.move_cursor(CursorMove::Bottom);
    textarea.move_cursor(CursorMove::End);
    textarea
}

fn textarea_text(textarea: &TextArea<'_>) -> String {
    textarea.lines().join("\n")
}

fn is_newline_key(key: KeyEvent) -> bool {
    key.code == KeyCode::Enter
        && key
            .modifiers
            .intersects(KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT)
}

fn composer_height(textarea: &TextArea<'_>) -> u16 {
    let lines = textarea.lines().len() as u16;
    lines.clamp(2, 6)
}

fn transcript_selectable_area(area: Rect) -> Rect {
    Rect {
        height: area.height.saturating_sub(1),
        ..area
    }
}

fn render_active_selection(frame: &mut Frame<'_>, ui: &FullscreenUi<'_>) {
    apply_selection_highlight(frame.buffer_mut(), &ui.screen_lines, &ui.selection);
}

fn apply_selection_highlight(
    buffer: &mut ratatui::buffer::Buffer,
    lines: &[ScreenLine],
    selection: &SelectionState,
) {
    let Some(anchor) = selection.anchor else {
        return;
    };
    let Some(focus) = selection.focus else {
        return;
    };
    if anchor == focus {
        return;
    }
    let ((start_x, start_y), (end_x, end_y)) = ordered_selection(anchor, focus);
    for line in lines {
        if selection.region.is_some_and(|region| line.region != region) {
            continue;
        }
        if line.y < start_y || line.y > end_y {
            continue;
        }
        let from = if line.y == start_y { start_x } else { 0 };
        let to = if line.y == end_y { end_x } else { u16::MAX };
        if to <= from {
            continue;
        }
        for cell in &line.cells {
            if !cell_overlaps_range(cell, from, to) {
                continue;
            }
            let cell_end = cell.x.saturating_add(cell.width);
            for x in cell.x..cell_end {
                if let Some(buffer_cell) = buffer.cell_mut((x, line.y)) {
                    buffer_cell.set_bg(TUI_SELECTION_BG);
                }
            }
        }
    }
}

fn render_transcript(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>) {
    ui.last_transcript_height = area.height;
    ui.last_transcript_width = area.width;
    ui.follow_transcript_if_needed();
    ui.clamp_transcript_scroll();
    let mut lines = Vec::new();
    let mut surface_rows = Vec::new();
    let mut areas = Vec::new();
    let mut cursor = 0u16;
    for (index, row) in ui.transcript.iter().enumerate() {
        if !row_visible(row, ui.thinking_visible) {
            continue;
        }
        let compact_trailing =
            compact_trailing_for(&ui.transcript, index, row, ui.thinking_visible);
        let row_lines = transcript_lines(
            row,
            ui.selected_row == Some(index),
            compact_trailing,
            area.width,
        );
        let height = wrapped_line_count(&row_lines, area.width) as u16;
        if cursor.saturating_add(height) > ui.scroll
            && cursor < ui.scroll.saturating_add(area.height)
        {
            let y = area.y.saturating_add(cursor.saturating_sub(ui.scroll));
            areas.push((
                index,
                Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height: height.min(area.height),
                },
            ));
        }
        cursor = cursor.saturating_add(height);
        let prompt_surface_rows = if row.kind == TranscriptKind::Prompt {
            row_lines
                .len()
                .saturating_sub(usize::from(!compact_trailing))
        } else {
            0
        };
        surface_rows
            .extend((0..row_lines.len()).map(|line_index| line_index < prompt_surface_rows));
        lines.extend(row_lines);
    }
    ui.last_entry_areas = areas;
    let paragraph = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::BOTTOM))
        .wrap(Wrap { trim: false })
        .scroll((ui.scroll, 0));
    frame.render_widget(paragraph, area);
    for (offset, has_surface_bg) in surface_rows
        .iter()
        .skip(ui.scroll as usize)
        .take(area.height as usize)
        .enumerate()
    {
        if !*has_surface_bg {
            continue;
        }
        let y = area.y.saturating_add(offset as u16);
        for x in area.x..area.x.saturating_add(area.width) {
            frame.buffer_mut()[(x, y)].set_bg(TUI_SURFACE_BG);
        }
    }
    ui.capture_selectable_rows(
        frame.buffer_mut(),
        transcript_selectable_area(area),
        SelectableRegion::Transcript,
    );
}

fn transcript_lines(
    row: &TranscriptRow,
    selected: bool,
    compact_trailing: bool,
    width: u16,
) -> Vec<Line<'static>> {
    if row.kind == TranscriptKind::Prompt {
        return prompt_lines(row, selected, compact_trailing, width);
    }
    if row.kind == TranscriptKind::Answer {
        return answer_lines(row, selected, compact_trailing);
    }
    if row.kind == TranscriptKind::Thinking {
        return thinking_lines(row, selected, compact_trailing);
    }
    if matches!(
        row.kind,
        TranscriptKind::Explored | TranscriptKind::Ran | TranscriptKind::Changed
    ) {
        return tool_lines(row, selected, compact_trailing, width);
    }

    let style = label_style(row.kind, row.failed);
    let marker = if selected { ">" } else { "▌" };
    let mut out = Vec::new();
    let title = row.title.trim();
    if !title.is_empty() {
        let suffix = if row.is_expandable() {
            if row.expanded { " [-]" } else { " [+]" }
        } else {
            ""
        };
        out.push(Line::from(vec![
            Span::styled(format!("{marker} "), style),
            Span::styled(
                format!("{title}{suffix}"),
                style.add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    let body_style = style_for_body(row.kind, row.failed);
    for line in row.expandable_text().lines() {
        let mut span = Span::styled(line.to_string(), body_style);
        if row.kind == TranscriptKind::Prompt {
            span = span.style(body_style.bg(Color::Rgb(24, 24, 28)));
        }
        out.push(Line::from(vec![
            Span::styled(format!("{marker} "), style),
            span,
        ]));
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled(marker.to_string(), style)));
    }
    if !compact_trailing {
        out.push(Line::from(""));
    }
    out
}

fn prompt_lines(
    row: &TranscriptRow,
    selected: bool,
    compact_trailing: bool,
    width: u16,
) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let body_style = style_for_body(row.kind, row.failed).bg(TUI_SURFACE_BG);
    for (index, line) in row.expandable_text().lines().enumerate() {
        let first_prefix = if selected {
            "> "
        } else if index == 0 {
            "› "
        } else {
            "  "
        };
        let continuation_prefix = if selected { "> " } else { "  " };
        for (wrapped_index, wrapped) in wrap_prompt_text(line, first_prefix, width)
            .into_iter()
            .enumerate()
        {
            let prefix = if wrapped_index == 0 {
                first_prefix
            } else {
                continuation_prefix
            };
            out.push(prompt_line(prefix, &wrapped, width, body_style));
        }
    }
    if out.is_empty() {
        out.push(prompt_line("› ", "", width, body_style));
    }
    if !compact_trailing {
        out.push(Line::from(""));
    }
    out
}

fn prompt_line(prefix: &str, text: &str, width: u16, style: Style) -> Line<'static> {
    let content_width = UnicodeWidthStr::width(prefix).saturating_add(UnicodeWidthStr::width(text));
    let padding = usize::from(width).saturating_sub(content_width);
    Line::from(vec![
        Span::styled(prefix.to_string(), style.fg(TUI_DIM)),
        Span::styled(text.to_string(), style),
        Span::styled(" ".repeat(padding), style),
    ])
}

fn wrap_prompt_text(text: &str, prefix: &str, width: u16) -> Vec<String> {
    let content_width = usize::from(width)
        .saturating_sub(UnicodeWidthStr::width(prefix))
        .saturating_sub(1)
        .max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    for ch in text.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if current_width > 0 && current_width.saturating_add(ch_width) > content_width {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current.push(ch);
        current_width = current_width.saturating_add(ch_width);
        if current_width >= content_width {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

fn answer_lines(row: &TranscriptRow, selected: bool, compact_trailing: bool) -> Vec<Line<'static>> {
    let body_style = style_for_body(row.kind, row.failed);
    let mut out = Vec::new();
    for line in row.expandable_text().lines() {
        if selected {
            out.push(Line::from(vec![
                Span::styled("> ".to_string(), label_style(row.kind, row.failed)),
                Span::styled(line.to_string(), body_style),
            ]));
        } else {
            out.push(Line::from(Span::styled(line.to_string(), body_style)));
        }
    }
    if out.is_empty() && selected {
        out.push(Line::from(Span::styled(
            ">".to_string(),
            label_style(row.kind, row.failed),
        )));
    }
    if !compact_trailing {
        out.push(Line::from(""));
    }
    out
}

fn thinking_lines(
    row: &TranscriptRow,
    selected: bool,
    compact_trailing: bool,
) -> Vec<Line<'static>> {
    let rail_style = if row.failed {
        Style::default().fg(TUI_RED)
    } else {
        Style::default().fg(TUI_DIM)
    };
    let prefix_style = label_style(row.kind, row.failed);
    let body_style = style_for_body(row.kind, row.failed);
    let marker = if selected { ">" } else { "▌" };
    let mut out = Vec::new();
    for (index, line) in row.expandable_text().lines().enumerate() {
        let mut spans = vec![Span::styled(format!("{marker} "), rail_style)];
        if index == 0 {
            spans.push(Span::styled("Thinking: ".to_string(), prefix_style));
        }
        spans.push(Span::styled(line.to_string(), body_style));
        out.push(Line::from(spans));
    }
    if out.is_empty() {
        out.push(Line::from(vec![
            Span::styled(format!("{marker} "), rail_style),
            Span::styled("Thinking:".to_string(), prefix_style),
        ]));
    }
    if !compact_trailing {
        out.push(Line::from(""));
    }
    out
}

fn tool_lines(
    row: &TranscriptRow,
    selected: bool,
    compact_trailing: bool,
    width: u16,
) -> Vec<Line<'static>> {
    let bullet_style = if row.failed {
        Style::default().fg(TUI_RED)
    } else if selected {
        Style::default().fg(TUI_CYAN)
    } else {
        Style::default().fg(Color::Green)
    };
    let title_style = if row.failed {
        Style::default().fg(TUI_RED).add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };
    let body_style = style_for_body(row.kind, row.failed);
    let marker = if selected { "> " } else { "• " };
    let mut out = Vec::new();
    let title = row.title.trim();
    if !title.is_empty() {
        let suffix = if row.is_expandable() {
            if row.expanded { " [-]" } else { " [+]" }
        } else {
            ""
        };
        let title = format!("{title}{suffix}");
        let elapsed = tool_elapsed_label(row);
        out.push(tool_title_line(
            marker,
            bullet_style,
            &title,
            title_style,
            elapsed.as_deref(),
            width,
        ));
    }
    for (index, line) in row.expandable_text().lines().enumerate() {
        let prefix = if index == 0 { "  └ " } else { "    " };
        out.push(Line::from(vec![
            Span::styled(prefix.to_string(), Style::default().fg(TUI_DIM)),
            Span::styled(line.to_string(), body_style),
        ]));
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled(marker.to_string(), bullet_style)));
    }
    if !compact_trailing {
        out.push(Line::from(""));
    }
    out
}

fn tool_title_line(
    marker: &str,
    marker_style: Style,
    title: &str,
    title_style: Style,
    elapsed: Option<&str>,
    width: u16,
) -> Line<'static> {
    let Some(elapsed) = elapsed.filter(|value| !value.is_empty()) else {
        return Line::from(vec![
            Span::styled(marker.to_string(), marker_style),
            Span::styled(title.to_string(), title_style),
        ]);
    };
    let marker_width = UnicodeWidthStr::width(marker);
    let width = usize::from(width);
    let elapsed = truncate_display_width(elapsed, width.saturating_sub(marker_width));
    let elapsed_width = UnicodeWidthStr::width(elapsed.as_str());
    let separator_width = usize::from(elapsed_width > 0);
    let title_width = width
        .saturating_sub(marker_width)
        .saturating_sub(elapsed_width)
        .saturating_sub(separator_width);
    let title = truncate_display_width(title, title_width);
    let padding = width
        .saturating_sub(marker_width)
        .saturating_sub(UnicodeWidthStr::width(title.as_str()))
        .saturating_sub(elapsed_width);
    Line::from(vec![
        Span::styled(marker.to_string(), marker_style),
        Span::styled(title, title_style),
        Span::raw(" ".repeat(padding)),
        Span::styled(elapsed, Style::default().fg(TUI_DIM)),
    ])
}

fn tool_elapsed_label(row: &TranscriptRow) -> Option<String> {
    row.tool_elapsed
        .or_else(|| row.tool_started.map(|started| started.elapsed()))
        .map(format_duration_s)
}

fn truncate_display_width(value: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(value) <= max_width {
        return value.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    let ellipsis = "…";
    if max_width == 1 {
        return ellipsis.to_string();
    }
    let keep_width = max_width.saturating_sub(UnicodeWidthStr::width(ellipsis));
    let mut out = String::new();
    let mut width = 0usize;
    for ch in value.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if width.saturating_add(ch_width) > keep_width {
            break;
        }
        out.push(ch);
        width = width.saturating_add(ch_width);
    }
    out.push_str(ellipsis);
    out
}

fn label_style(kind: TranscriptKind, failed: bool) -> Style {
    if failed {
        return Style::default().fg(TUI_RED);
    }
    match kind {
        TranscriptKind::Prompt
        | TranscriptKind::Explored
        | TranscriptKind::Ran
        | TranscriptKind::Changed => Style::default().fg(TUI_CYAN),
        TranscriptKind::Answer => Style::default().fg(TUI_MAGENTA),
        TranscriptKind::Thinking => Style::default().fg(TUI_PAPER),
        TranscriptKind::Meta => Style::default().fg(TUI_DIM),
        TranscriptKind::Status => Style::default().fg(TUI_CYAN),
        TranscriptKind::Error => Style::default().fg(TUI_RED),
    }
}

fn style_for_body(kind: TranscriptKind, failed: bool) -> Style {
    if failed {
        return Style::default().fg(TUI_RED);
    }
    match kind {
        TranscriptKind::Thinking => Style::default().fg(TUI_DIM),
        TranscriptKind::Meta | TranscriptKind::Status => Style::default().fg(TUI_DIM),
        TranscriptKind::Error => Style::default().fg(TUI_RED),
        _ => Style::default(),
    }
}

fn render_composer(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>) {
    let surface_style = Style::default().bg(TUI_SURFACE_BG);
    frame.render_widget(Block::default().style(surface_style), area);
    if area.width == 0 || area.height == 0 {
        return;
    }

    let textarea_empty = ui.textarea.is_empty();
    let marker_width = if textarea_empty {
        area.width.min(1)
    } else {
        area.width.min(2)
    };
    frame.render_widget(
        Paragraph::new(Line::from(if textarea_empty {
            vec![Span::styled("›".to_string(), surface_style.fg(TUI_DIM))]
        } else {
            vec![
                Span::styled("›".to_string(), surface_style.fg(TUI_DIM)),
                Span::styled(" ".to_string(), surface_style),
            ]
        }))
        .style(surface_style),
        Rect {
            x: area.x,
            y: area.y,
            width: marker_width,
            height: area.height,
        },
    );

    let input_area = Rect {
        x: area.x.saturating_add(marker_width),
        y: area.y,
        width: area.width.saturating_sub(marker_width),
        height: area.height,
    };
    if input_area.width == 0 || input_area.height == 0 {
        return;
    }

    ui.textarea.set_block(Block::default().style(surface_style));
    ui.textarea.set_style(surface_style);
    ui.textarea.set_placeholder_text("");
    frame.render_widget(&ui.textarea, input_area);

    if textarea_empty && input_area.width > 1 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Ask pevo...".to_string(),
                surface_style.fg(TUI_DIM),
            )))
            .style(surface_style),
            Rect {
                x: input_area.x.saturating_add(1),
                y: input_area.y,
                width: input_area.width.saturating_sub(1),
                height: 1,
            },
        );
    }
}

fn render_slash_menu(
    frame: &mut Frame<'_>,
    area: Rect,
    items: &[self::slash::SlashMenuItem],
    selected_index: usize,
    row_areas: &mut Vec<(usize, Rect)>,
) {
    row_areas.clear();
    let lines = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let marker = if item.upcoming { " upcoming" } else { "" };
            let selected = index == selected_index;
            let prefix = if selected { "> " } else { "  " };
            let row_style = if selected {
                Style::default().bg(Color::Rgb(24, 24, 28))
            } else {
                Style::default()
            };
            Line::from(vec![
                Span::styled(prefix, row_style.fg(TUI_CYAN)),
                Span::styled(item.command, row_style.fg(TUI_CYAN)),
                Span::styled(
                    format!("  {}{marker}", item.description),
                    row_style.fg(TUI_DIM),
                ),
            ])
        })
        .collect::<Vec<_>>();
    for index in 0..items.len() {
        row_areas.push((
            index,
            Rect {
                x: area.x,
                y: area.y.saturating_add(index as u16),
                width: area.width,
                height: 1,
            },
        ));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::LEFT).title(" commands "))
            .style(Style::default().bg(Color::Rgb(16, 16, 20))),
        area,
    );
}

fn render_status(frame: &mut Frame<'_>, area: Rect, app: &TuiApp) {
    let model = app.model_display_value();
    let variant = app.variant_display_value();
    let mut spans = Vec::new();
    spans.push(Span::raw(model));
    spans.push(Span::raw("  "));
    spans.push(Span::styled(variant, Style::default().fg(TUI_MAGENTA)));
    if app.current_mode != RunMode::Build {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            app.current_mode.as_str().to_string(),
            Style::default().fg(TUI_CYAN),
        ));
    }
    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_sidebar(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>) {
    let mut lines = vec![
        Line::from(Span::styled(
            ui.sidebar.title.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("session: {}", ui.sidebar.session)),
        Line::from(""),
        sidebar_heading("Context"),
        Line::from(format!("workdir: {}", ui.sidebar.workdir)),
        Line::from(format!("branch: {}", ui.sidebar.branch)),
    ];
    lines.push(Line::from(format!(
        "messages: {}  tools: {}",
        ui.sidebar.message_count, ui.sidebar.tool_count
    )));
    if let Some(tokens) = ui.sidebar.tokens {
        lines.push(Line::from(format!("tokens: {}", format_count(tokens))));
    }
    if let Some(percent) = ui.sidebar.context_percent {
        lines.push(Line::from(format!("context: {percent:.1}%")));
    }
    lines.push(Line::from(""));
    lines.push(sidebar_heading("Modified Files"));
    if ui.sidebar.changed_files.is_empty() {
        lines.push(Line::from(Span::styled(
            "(clean)",
            Style::default().fg(TUI_DIM),
        )));
    } else {
        for file in &ui.sidebar.changed_files {
            lines.push(Line::from(file.clone()));
        }
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
    ui.capture_selectable_rows(frame.buffer_mut(), area, SelectableRegion::Sidebar);
}

fn render_bottom_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    panel: &mut BottomPanel,
    row_areas: &mut Vec<(usize, Rect)>,
) {
    row_areas.clear();
    frame.render_widget(
        Block::default().style(Style::default().bg(Color::Rgb(18, 18, 22))),
        area,
    );
    let inner = Rect {
        x: area.x.saturating_add(2),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    };
    let selection = panel.selection_mut();
    let reserved = 4 + if selection.notice.is_some() { 1 } else { 0 };
    let visible_rows = inner.height.saturating_sub(reserved).max(1);
    selection.ensure_selected_visible(visible_rows);

    let mut lines = Vec::new();
    let title_width = selection.title.chars().count() as u16;
    let esc_hint = "esc";
    let header_padding = inner
        .width
        .saturating_sub(title_width)
        .saturating_sub(esc_hint.len() as u16) as usize;
    lines.push(Line::from(vec![
        Span::styled(
            selection.title.clone(),
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(header_padding)),
        Span::styled(esc_hint, Style::default().fg(TUI_DIM)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Search ", Style::default().fg(TUI_DIM)),
        Span::styled(selection.query.clone(), Style::default().fg(Color::Gray)),
    ]));
    let mut row_y = inner.y.saturating_add(lines.len() as u16);

    let filtered = selection.filtered_indices();
    if filtered.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            selection.empty_label.clone(),
            Style::default().fg(TUI_DIM),
        )));
    } else {
        let mut last_group: Option<String> = None;
        for (visible_index, row_index) in filtered
            .iter()
            .enumerate()
            .skip(selection.scroll as usize)
            .take(visible_rows as usize)
        {
            let row = &selection.rows[*row_index];
            if row.group != last_group
                && let Some(group) = row.group.clone()
            {
                lines.push(Line::from(Span::styled(
                    group.clone(),
                    Style::default().fg(TUI_CYAN).add_modifier(Modifier::BOLD),
                )));
                row_y = row_y.saturating_add(1);
                last_group = Some(group);
            }
            row_areas.push((
                visible_index,
                Rect {
                    x: inner.x,
                    y: row_y,
                    width: inner.width,
                    height: 1,
                },
            ));
            lines.push(bottom_panel_row(
                row,
                visible_index == selection.selected,
                inner.width,
            ));
            row_y = row_y.saturating_add(1);
        }
    }
    lines.push(Line::from(""));
    if let Some(notice) = &selection.notice {
        lines.push(Line::from(Span::styled(
            notice.clone(),
            Style::default().fg(TUI_DIM),
        )));
    }
    lines.push(Line::from(Span::styled(
        selection.footer_text(),
        Style::default().fg(TUI_DIM),
    )));

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn bottom_panel_row(row: &BottomSelectionRow, selected: bool, width: u16) -> Line<'static> {
    let select_marker = if selected { "›" } else { " " };
    let state_marker = if row.is_current {
        "● "
    } else if row.is_default {
        "◆ "
    } else {
        "  "
    };
    let prefix = format!("{select_marker} {state_marker}{}", row.label);
    let mut left = prefix.clone();
    if let Some(description) = &row.description {
        left.push_str("  ");
        left.push_str(description);
    }
    let detail = row.detail.as_deref().unwrap_or_default();
    let text = if detail.is_empty() {
        truncate_display_width(&left, width as usize)
    } else {
        let width = usize::from(width);
        let detail = truncate_display_width(detail, width);
        let detail_width = UnicodeWidthStr::width(detail.as_str());
        let separator_width = 2.min(width.saturating_sub(detail_width));
        let available = width
            .saturating_sub(detail_width)
            .saturating_sub(separator_width);
        let left = truncate_display_width(&left, available);
        let padding = width
            .saturating_sub(UnicodeWidthStr::width(left.as_str()))
            .saturating_sub(detail_width);
        format!("{left}{}{detail}", " ".repeat(padding))
    };
    let style = if selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Rgb(246, 178, 127))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    if selected || row.style == BottomRowStyle::Normal || !detail.is_empty() {
        return Line::from(Span::styled(text, style));
    }
    let prefix = truncate_display_width(&prefix, width as usize);
    let prefix_width = UnicodeWidthStr::width(prefix.as_str());
    let rest = text
        .chars()
        .skip(prefix.chars().count())
        .collect::<String>();
    let rest = truncate_display_width(&rest, (width as usize).saturating_sub(prefix_width));
    Line::from(vec![
        Span::styled(
            prefix,
            Style::default().fg(TUI_CYAN).add_modifier(Modifier::BOLD),
        ),
        Span::styled(rest, Style::default().fg(TUI_DIM)),
    ])
}

fn bottom_panel_height(height: u16) -> u16 {
    16.min(height.saturating_sub(6)).max(8)
}

fn rect_contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && column < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}

fn sidebar_heading(label: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        label,
        Style::default().add_modifier(Modifier::BOLD),
    ))
}

fn short_session(id: &str) -> &str {
    &id[..id.len().min(8)]
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    format!("{}…", value.chars().take(keep).collect::<String>())
}

fn short_fetch_error(value: &str) -> String {
    let value = value
        .trim()
        .replace(['\r', '\n', '\t'], " ")
        .trim_start_matches("config failed: ")
        .trim_start_matches("HTTP request failed: ")
        .trim_start_matches("error: ")
        .to_string();
    if value == "timeout" {
        return value;
    }
    truncate_chars(&value, 120)
}

fn format_session_date(timestamp_ms: i64) -> String {
    let days = timestamp_ms.div_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

fn format_session_time(timestamp_ms: i64) -> String {
    let millis = timestamp_ms.rem_euclid(86_400_000);
    let minutes = millis / 60_000;
    let hour = minutes / 60;
    let minute = minutes % 60;
    format!("{hour:02}:{minute:02}")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m as u32, d as u32)
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn default_clipboard_sink() -> ClipboardSink {
    Arc::new(copy_text_to_clipboard)
}

fn copy_text_to_clipboard(text: &str) -> io::Result<()> {
    copy_text_to_clipboard_with(
        text,
        local_clipboard_commands(),
        |candidate, text| pipe_to_command(candidate.command, candidate.args, text),
        write_osc52_clipboard,
    )
}

fn copy_text_to_clipboard_with(
    text: &str,
    candidates: Vec<ClipboardCommand>,
    mut local_copy: impl FnMut(ClipboardCommand, &str) -> io::Result<bool>,
    osc52_copy: impl FnOnce(&str) -> io::Result<()>,
) -> io::Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    let mut failures = Vec::new();
    for candidate in candidates {
        match local_copy(candidate, text) {
            Ok(true) => return Ok(()),
            Ok(false) => failures.push(format!("{} unavailable", candidate.command)),
            Err(err) => failures.push(format!("{}: {err}", candidate.command)),
        }
    }
    match osc52_copy(text) {
        Ok(()) => Ok(()),
        Err(err) => {
            failures.push(format!("OSC52: {err}"));
            Err(io::Error::other(format!(
                "clipboard copy failed: {}",
                clipboard_failure_summary(&failures)
            )))
        }
    }
}

fn clipboard_failure_summary(failures: &[String]) -> String {
    if failures.is_empty() {
        return "no clipboard backend succeeded".to_string();
    }
    let summary = failures.join("; ");
    truncate_chars(&summary, 240)
}

fn write_osc52_clipboard(text: &str) -> io::Result<()> {
    let sequence = osc52_sequence(text)?;
    #[cfg(unix)]
    {
        if let Ok(tty) = std::fs::OpenOptions::new().write(true).open("/dev/tty")
            && write_osc52_sequence(tty, &sequence).is_ok()
        {
            return Ok(());
        }
    }
    write_osc52_sequence(io::stdout(), &sequence)
}

fn write_osc52_sequence(mut writer: impl Write, sequence: &str) -> io::Result<()> {
    writer.write_all(sequence.as_bytes())?;
    writer.flush()
}

fn osc52_sequence(text: &str) -> io::Result<String> {
    osc52_sequence_with_passthrough(
        text,
        std::env::var_os("TMUX").is_some() || std::env::var_os("STY").is_some(),
    )
}

fn osc52_sequence_with_passthrough(text: &str, passthrough: bool) -> io::Result<String> {
    const OSC52_MAX_RAW_BYTES: usize = 100_000;
    let raw_bytes = text.len();
    if raw_bytes > OSC52_MAX_RAW_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("OSC52 payload too large ({raw_bytes} bytes; max {OSC52_MAX_RAW_BYTES})"),
        ));
    }
    let encoded = base64_encode(text.as_bytes());
    if passthrough {
        Ok(format!("\x1bPtmux;\x1b\x1b]52;c;{encoded}\x07\x1b\\"))
    } else {
        Ok(format!("\x1b]52;c;{encoded}\x07"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClipboardCommand {
    command: &'static str,
    args: &'static [&'static str],
}

const NO_ARGS: &[&str] = &[];
const POWERSHELL_CLIPBOARD_ARGS: &[&str] = &[
    "-NonInteractive",
    "-NoProfile",
    "-Command",
    "[Console]::InputEncoding = [System.Text.Encoding]::UTF8; $ErrorActionPreference = 'Stop'; $text = [Console]::In.ReadToEnd(); Set-Clipboard -Value $text",
];
const XCLIP_CLIPBOARD_ARGS: &[&str] = &["-selection", "clipboard"];
const XSEL_CLIPBOARD_ARGS: &[&str] = &["--clipboard", "--input"];

fn local_clipboard_commands() -> Vec<ClipboardCommand> {
    local_clipboard_commands_for(
        cfg!(target_os = "macos"),
        cfg!(target_os = "windows"),
        is_probably_wsl(),
        is_wayland_session(),
    )
}

fn is_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE")
            .is_ok_and(|value| value.eq_ignore_ascii_case("wayland"))
}

fn is_probably_wsl() -> bool {
    let proc_version = std::fs::read_to_string("/proc/version").ok();
    let os_release = std::fs::read_to_string("/proc/sys/kernel/osrelease").ok();
    is_probably_wsl_from(
        proc_version.as_deref(),
        os_release.as_deref(),
        std::env::var_os("WSL_DISTRO_NAME").is_some(),
        std::env::var_os("WSL_INTEROP").is_some(),
    )
}

fn is_probably_wsl_from(
    proc_version: Option<&str>,
    os_release: Option<&str>,
    distro_env: bool,
    interop_env: bool,
) -> bool {
    proc_version.is_some_and(contains_wsl_marker)
        || os_release.is_some_and(contains_wsl_marker)
        || distro_env
        || interop_env
}

fn contains_wsl_marker(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("microsoft") || lower.contains("wsl")
}

fn local_clipboard_commands_for(
    macos: bool,
    windows: bool,
    wsl: bool,
    wayland: bool,
) -> Vec<ClipboardCommand> {
    if macos {
        return vec![ClipboardCommand {
            command: "pbcopy",
            args: NO_ARGS,
        }];
    }
    if windows {
        return vec![ClipboardCommand {
            command: "powershell.exe",
            args: POWERSHELL_CLIPBOARD_ARGS,
        }];
    }

    let mut candidates = Vec::new();
    if wsl {
        candidates.push(ClipboardCommand {
            command: "powershell.exe",
            args: POWERSHELL_CLIPBOARD_ARGS,
        });
        candidates.push(ClipboardCommand {
            command: "clip.exe",
            args: NO_ARGS,
        });
    }
    if wayland {
        candidates.push(ClipboardCommand {
            command: "wl-copy",
            args: NO_ARGS,
        });
    }
    candidates.push(ClipboardCommand {
        command: "xclip",
        args: XCLIP_CLIPBOARD_ARGS,
    });
    candidates.push(ClipboardCommand {
        command: "xsel",
        args: XSEL_CLIPBOARD_ARGS,
    });
    candidates
}

fn pipe_to_command(command: &str, args: &[&str], text: &str) -> io::Result<bool> {
    let mut child = match StdCommand::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(text.as_bytes())?;
    }
    drop(child.stdin.take());
    let status = child.wait()?;
    Ok(status.success())
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn increment_row_index(value: &mut Option<usize>, inserted_at: usize) {
    if let Some(index) = value
        && *index >= inserted_at
    {
        *index += 1;
    }
}

fn format_configured_model(model: &ConfiguredModel) -> String {
    let mut parts = vec![format!("{}/{}", model.provider, model.model)];
    if let Some(variant) = &model.reasoning_effort {
        parts.push(format!("variant={variant}"));
    }
    if let Some(limit) = model.context_limit {
        parts.push(format!("context={limit}"));
    }
    parts.join(" ")
}

fn format_model_spec(model: &ConfiguredModel) -> String {
    format!("{}/{}", model.provider, model.model)
}

fn variant_description(variant: &str) -> &'static str {
    match variant {
        "none" => "suppress provider reasoning field",
        "minimal" => "smallest reasoning request",
        "low" => "lighter reasoning",
        "medium" => "balanced reasoning",
        "high" => "deeper reasoning",
        "xhigh" => "extra high reasoning",
        "max" => "maximum configured reasoning",
        _ => "custom reasoning setting",
    }
}

fn resolve_session_ref_from_summaries(
    sessions: &[SessionSummary],
    reference: &str,
) -> Result<String> {
    if reference == "latest" {
        return sessions
            .first()
            .map(|session| session.id.clone())
            .ok_or_else(|| anyhow!("no latest session for this workdir"));
    }
    if let Some(session) = sessions.iter().find(|session| session.id == reference) {
        return Ok(session.id.clone());
    }
    let matches = sessions
        .iter()
        .filter(|session| session.id.starts_with(reference))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [session] => Ok(session.id.clone()),
        [] => Err(anyhow!("session not found: {reference}")),
        _ => Err(anyhow!("ambiguous session prefix: {reference}")),
    }
}

struct TurnPrinter {
    renderer: TuiRenderer,
    last_assistant_text: String,
    reasoning_active: bool,
    thinking_enabled: bool,
    debug: bool,
    run_provider: String,
    run_model: String,
    run_mode: String,
    context_limit: Option<u64>,
    tool_titles: BTreeMap<String, String>,
}

impl TurnPrinter {
    fn new(renderer: TuiRenderer, thinking_enabled: bool, debug: bool) -> Self {
        Self {
            renderer,
            last_assistant_text: String::new(),
            reasoning_active: false,
            thinking_enabled,
            debug,
            run_provider: String::new(),
            run_model: String::new(),
            run_mode: String::new(),
            context_limit: None,
            tool_titles: BTreeMap::new(),
        }
    }

    fn render_event(&mut self, event: &RunStreamEvent, out: &mut impl Write) -> io::Result<()> {
        match event {
            RunStreamEvent::ReasoningDelta { text } => {
                if self.thinking_enabled {
                    if !self.reasoning_active {
                        self.reasoning_active = true;
                        write!(out, "Thinking: ")?;
                    }
                    write!(out, "{}", self.renderer.dim(text))?;
                }
            }
            RunStreamEvent::ReasoningEnd => {
                if self.reasoning_active {
                    self.reasoning_active = false;
                    if self.thinking_enabled {
                        writeln!(out)?;
                    }
                }
            }
            RunStreamEvent::Event(value) => self.render_value_event(value, out)?,
        }
        out.flush()
    }

    fn render_value_event(&mut self, value: &Value, out: &mut impl Write) -> io::Result<()> {
        match value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "run_start" => {
                self.run_provider = value
                    .get("provider")
                    .and_then(Value::as_str)
                    .unwrap_or("provider")
                    .to_string();
                self.run_model = value
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("model")
                    .to_string();
                self.run_mode = value
                    .get("mode")
                    .and_then(Value::as_str)
                    .unwrap_or("default")
                    .to_string();
                self.context_limit = value.get("context_limit").and_then(Value::as_u64);
            }
            "message_update" => {
                if let Some(text) = assistant_text_from_event(value) {
                    self.last_assistant_text = text;
                }
            }
            "message_end" => {
                if let Some(text) = assistant_text_from_event(value) {
                    self.last_assistant_text = text.clone();
                    if !text.trim().is_empty() {
                        writeln!(out, "Answer:\n{text}")?;
                    }
                }
                let meta = turn_meta_text(TurnMetaProjection {
                    mode: &self.run_mode,
                    provider: &self.run_provider,
                    model: &self.run_model,
                    started: None,
                    usage: value.get("usage"),
                    metadata: value.get("metadata"),
                    failures: 0,
                    debug: self.debug,
                });
                if !meta.is_empty() {
                    writeln!(out, "Meta: {meta}")?;
                }
            }
            "tool_execution_start" => {
                let tool = value
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                let title = tool_title(tool, value);
                if let Some(tool_call_id) = value.get("tool_call_id").and_then(Value::as_str) {
                    self.tool_titles
                        .insert(tool_call_id.to_string(), title.clone());
                }
                writeln!(out, "{title}: running")?;
            }
            "tool_execution_end" => {
                let outcome = value
                    .get("outcome")
                    .and_then(Value::as_str)
                    .unwrap_or("normal");
                let summary = format_tool_summary(value);
                let tool = value
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                let existing_title = value
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .and_then(|tool_call_id| self.tool_titles.get(tool_call_id))
                    .map(String::as_str)
                    .unwrap_or("");
                let title = match evidence_kind(tool) {
                    TranscriptKind::Ran => tool_title_for_update(tool, value, existing_title),
                    TranscriptKind::Explored => "Explored".to_string(),
                    TranscriptKind::Changed => "Changed".to_string(),
                    _ => "Tool".to_string(),
                };
                let elapsed = metadata_elapsed_duration(Some(value))
                    .map(|elapsed| format!(" {}", format_duration_s(elapsed)))
                    .unwrap_or_default();
                if outcome == "normal" {
                    writeln!(
                        out,
                        "{}",
                        self.renderer
                            .success(&format!("{title}{elapsed}: {summary}"))
                    )?;
                } else {
                    writeln!(
                        out,
                        "{}",
                        self.renderer
                            .error(&format!("{title}{elapsed}: failed {summary}"))
                    )?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(&mut self, out: &mut impl Write) -> io::Result<()> {
        if self.reasoning_active {
            writeln!(out)?;
            self.reasoning_active = false;
        }
        out.flush()
    }
}
