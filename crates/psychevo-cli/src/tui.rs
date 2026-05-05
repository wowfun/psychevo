use std::collections::BTreeMap;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::{Command as StdCommand, ExitCode};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event as CrosstermEvent, KeyCode, KeyEvent,
    KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use psychevo_ai::Outcome;
use psychevo_runtime::{
    ConfiguredModel, RunControlHandle, RunMode, RunOptions, RunStreamEvent, RunStreamSink,
    SessionSummary, SqliteStore, canonicalize_workdir, configured_models, run_control,
    run_live_streaming, run_live_streaming_controlled,
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

use crate::tui_render::{
    TuiRenderer, assistant_text_from_event, format_sanitized_message, format_session_line,
    format_tool_summary,
};
use crate::tui_slash::slash_menu_items;
use crate::tui_slash::{SlashCommand, parse_slash_command, validate_model_spec, validate_variant};
use crate::tui_state::TuiState;
use crate::{
    TuiArgs, ensure_home_initialized, env_path, env_value, inherited_env, resolve_explicit_path,
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
        force_new_once: args.new_session,
        current_model,
        current_variant,
        current_mode,
        thinking_visible,
        renderer: TuiRenderer::new(color),
        debug: args.debug,
        had_error: false,
    };
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
    force_new_once: bool,
    current_model: Option<String>,
    current_variant: Option<String>,
    current_mode: RunMode,
    thinking_visible: bool,
    renderer: TuiRenderer,
    debug: bool,
    had_error: bool,
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
        if !initial_prompt.trim().is_empty() {
            self.start_fullscreen_turn(&mut ui, initial_prompt)?;
        }
        loop {
            self.drain_fullscreen_events(&mut ui).await?;
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
                        ui.handle_mouse(mouse);
                    }
                    _ => {}
                }
            }
        }
        if let Some(running) = ui.running.take() {
            running.control.abort();
            let _ = running.task.await;
        }
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
        if ui.history_search {
            return self.handle_history_search_key(ui, key);
        }
        if ui.focus == FocusMode::Transcript {
            match key.code {
                KeyCode::Esc => {
                    ui.focus = FocusMode::Composer;
                    ui.push_status("composer focus");
                }
                KeyCode::Up => ui.move_selection(-1),
                KeyCode::Down => ui.move_selection(1),
                KeyCode::Enter | KeyCode::Char(' ') => ui.toggle_selected(),
                KeyCode::PageUp => ui.scroll = ui.scroll.saturating_sub(6),
                KeyCode::PageDown => ui.scroll = ui.scroll.saturating_add(6),
                _ => {}
            }
            return Ok(false);
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
                ui.ensure_selection();
                ui.push_status("transcript focus");
            }
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if ui.last_sidebar_visible {
                    ui.sidebar_hidden = true;
                    ui.sidebar_forced = false;
                    ui.push_status("sidebar hidden");
                } else {
                    ui.sidebar_hidden = false;
                    ui.sidebar_forced = true;
                    ui.push_status("sidebar shown");
                }
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
                ui.textarea = new_textarea();
                ui.history.push(line.clone());
                ui.history_index = None;
                if let Some(command) = parse_slash_command(&line)? {
                    return self.handle_fullscreen_command(ui, command).await;
                }
                self.start_fullscreen_turn(ui, line)?;
            }
            KeyCode::Tab => {
                self.cycle_mode(ui, true)?;
            }
            KeyCode::BackTab => {
                self.cycle_mode(ui, false)?;
            }
            KeyCode::Esc => {
                if let Some(running) = &ui.running {
                    running.control.abort();
                    ui.push_error("interrupt requested");
                } else {
                    ui.push_status("idle");
                }
            }
            KeyCode::PageUp => {
                ui.scroll = ui.scroll.saturating_sub(6);
            }
            KeyCode::PageDown => {
                ui.scroll = ui.scroll.saturating_add(6);
            }
            KeyCode::Up if textarea_text(&ui.textarea).is_empty() => {
                ui.recall_history(-1);
            }
            KeyCode::Down if ui.history_index.is_some() => {
                ui.recall_history(1);
            }
            _ => {
                ui.textarea.input(key);
            }
        }
        Ok(false)
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
            SlashCommand::Help => {
                for line in self.help_lines() {
                    ui.push_status(line);
                }
            }
            SlashCommand::Status => {
                for line in self.status_lines() {
                    ui.push_status(line);
                }
            }
            SlashCommand::New => {
                self.current_session = None;
                self.force_new_once = true;
                ui.push_status("new session will start on next prompt");
                ui.refresh_sidebar(self);
            }
            SlashCommand::SessionList => {
                for line in self.session_list_lines()? {
                    ui.push_status(line);
                }
            }
            SlashCommand::SessionShow(reference) => {
                for line in self.session_show_lines(reference.as_deref())? {
                    ui.push_status(line);
                }
            }
            SlashCommand::SessionSwitch(reference) => {
                self.switch_session_no_print(&reference)?;
                ui.push_status(format!(
                    "session: {}",
                    self.current_session.as_deref().unwrap_or("(none)")
                ));
                ui.refresh_sidebar(self);
            }
            SlashCommand::ModelShow => {
                for line in self.model_lines() {
                    ui.push_status(line);
                }
            }
            SlashCommand::Models => {
                for line in self.configured_model_lines()? {
                    ui.push_status(line);
                }
            }
            SlashCommand::ModelSet(model) => {
                self.set_model_no_print(model.clone())?;
                ui.push_status(format!("model: {model}"));
                ui.refresh_sidebar(self);
            }
            SlashCommand::VariantShow => {
                ui.push_status(self.variant_line());
            }
            SlashCommand::VariantSet(variant) => {
                self.set_variant_no_print(variant.clone())?;
                ui.push_status(format!("variant: {variant}"));
                ui.refresh_sidebar(self);
            }
            SlashCommand::ModeShow => {
                ui.push_status(format!("mode: {}", self.current_mode.as_str()));
            }
            SlashCommand::ModeSet(mode) => {
                self.set_mode_no_print(&mode)?;
                ui.push_status(format!("mode: {mode}"));
                ui.refresh_sidebar(self);
            }
            SlashCommand::ThinkingToggle => {
                let enabled = !self.thinking_visible;
                self.set_thinking_no_print(enabled)?;
                ui.push_status(format!("thinking: {}", on_off(enabled)));
                ui.refresh_sidebar(self);
            }
            SlashCommand::ThinkingSet(enabled) => {
                self.set_thinking_no_print(enabled)?;
                ui.push_status(format!("thinking: {}", on_off(enabled)));
                ui.refresh_sidebar(self);
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
            SlashCommand::Help => self.show_help(),
            SlashCommand::Quit => return Ok(true),
            SlashCommand::Status => self.show_status(),
            SlashCommand::New => {
                self.current_session = None;
                self.force_new_once = true;
                println!(
                    "{}",
                    self.renderer
                        .status("new session will start on next prompt")
                );
                Ok(())
            }
            SlashCommand::SessionList => self.show_session_list(),
            SlashCommand::SessionShow(reference) => self.show_session(reference.as_deref()),
            SlashCommand::SessionSwitch(reference) => self.switch_session(&reference),
            SlashCommand::ModelShow => self.show_model(),
            SlashCommand::Models => self.show_models(),
            SlashCommand::ModelSet(model) => self.set_model(model),
            SlashCommand::VariantShow => self.show_variant(),
            SlashCommand::VariantSet(variant) => self.set_variant(variant),
            SlashCommand::ModeShow => self.show_mode(),
            SlashCommand::ModeSet(mode) => self.set_mode(mode),
            SlashCommand::ThinkingToggle => self.toggle_thinking(),
            SlashCommand::ThinkingSet(enabled) => self.set_thinking(enabled),
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
        let mut pending = Vec::new();
        if let Some(running) = &mut ui.running {
            while let Ok(event) = running.rx.try_recv() {
                pending.push(event);
            }
        }
        for event in pending {
            ui.apply_stream_event(event, self.thinking_visible, self.debug);
        }
        if ui
            .running
            .as_ref()
            .is_some_and(|running| running.task.is_finished())
        {
            let mut running = ui.running.take().expect("checked running");
            let result = running.task.await;
            while let Ok(event) = running.rx.try_recv() {
                ui.apply_stream_event(event, self.thinking_visible, self.debug);
            }
            match result {
                Ok(Ok(result)) => {
                    self.current_session = Some(result.session_id.clone());
                    self.force_new_once = false;
                    let success = result.outcome == Outcome::Normal && result.tool_failures == 0;
                    if success {
                        ui.push_success(format!("turn complete: {}", result.outcome.as_str()));
                    } else {
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
        }
        Ok(())
    }

    fn render_fullscreen(&self, frame: &mut Frame<'_>, ui: &mut FullscreenUi<'_>) {
        let area = frame.area();
        let sidebar_allowed = area.width >= 100 || ui.sidebar_forced;
        let sidebar_visible = sidebar_allowed && !ui.sidebar_hidden;
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
        let composer_height = composer_height(&ui.textarea);
        let slash_items = slash_menu_items(&textarea_text(&ui.textarea));
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
                    Constraint::Length(1),
                ])
                .split(main)
        };
        if slash_height == 0 {
            render_transcript(frame, vertical[0], ui);
            render_composer(frame, vertical[1], ui, self.current_mode);
            render_status(frame, vertical[2], self, ui);
            render_footer(frame, vertical[3], ui);
        } else {
            render_transcript(frame, vertical[0], ui);
            render_slash_menu(frame, vertical[1], &slash_items);
            render_composer(frame, vertical[2], ui, self.current_mode);
            render_status(frame, vertical[3], self, ui);
            render_footer(frame, vertical[4], ui);
        }
        if sidebar_visible {
            render_sidebar(frame, horizontal[1], ui);
        }
    }

    fn run_options(&self, prompt: String) -> RunOptions {
        RunOptions {
            db_path: self.db_path.clone(),
            workdir: self.workdir.clone(),
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

    fn show_help(&self) -> Result<()> {
        println!("{}", self.renderer.brand("pevo tui commands"));
        for line in self.help_lines() {
            println!("{line}");
        }
        Ok(())
    }

    fn show_status(&self) -> Result<()> {
        for line in self.status_lines() {
            println!("{line}");
        }
        Ok(())
    }

    fn show_session_list(&self) -> Result<()> {
        for line in self.session_list_lines()? {
            println!("{line}");
        }
        Ok(())
    }

    fn show_session(&self, reference: Option<&str>) -> Result<()> {
        for line in self.session_show_lines(reference)? {
            println!("{line}");
        }
        Ok(())
    }

    fn switch_session(&mut self, reference: &str) -> Result<()> {
        let id = self.switch_session_no_print(reference)?;
        println!("{}", self.renderer.status(&format!("session: {id}")));
        Ok(())
    }

    fn show_model(&self) -> Result<()> {
        for line in self.model_lines() {
            println!("{line}");
        }
        Ok(())
    }

    fn show_models(&self) -> Result<()> {
        for line in self.configured_model_lines()? {
            println!("{line}");
        }
        Ok(())
    }

    fn set_model(&mut self, model: String) -> Result<()> {
        self.set_model_no_print(model.clone())?;
        println!("{}", self.renderer.status(&format!("model: {model}")));
        Ok(())
    }

    fn show_variant(&self) -> Result<()> {
        println!("{}", self.variant_line());
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

    fn show_mode(&self) -> Result<()> {
        println!("mode: {}", self.current_mode.as_str());
        Ok(())
    }

    fn set_mode(&mut self, mode: String) -> Result<()> {
        self.set_mode_no_print(&mode)?;
        println!("{}", self.renderer.status(&format!("mode: {mode}")));
        Ok(())
    }

    fn help_lines(&self) -> Vec<String> {
        vec![
            "Enter submit; Shift/Ctrl/Alt+Enter or Ctrl+J newline; Tab mode; Ctrl+B sidebar; Ctrl+T transcript; Esc interrupt".to_string(),
            "/help".to_string(),
            "/quit /exit /q".to_string(),
            "/status".to_string(),
            "/clear /new".to_string(),
            "/session list".to_string(),
            "/session show [id]".to_string(),
            "/session switch <id|prefix|latest>".to_string(),
            "/model".to_string(),
            "/models".to_string(),
            "/model set <provider/model>".to_string(),
            "/variant".to_string(),
            "/variant set <none|minimal|low|medium|high|xhigh|max>".to_string(),
            "/mode".to_string(),
            "/mode set <plan|build>".to_string(),
            "/thinking [on|off]".to_string(),
            "/undo /compact /export (upcoming)".to_string(),
        ]
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
            format!(
                "model: {}",
                self.current_model.as_deref().unwrap_or("(config default)")
            ),
            self.variant_line(),
            format!("mode: {}", self.current_mode.as_str()),
            format!("thinking: {}", on_off(self.thinking_visible)),
            format!("debug: {}", on_off(self.debug)),
        ]
    }

    fn session_list_lines(&self) -> Result<Vec<String>> {
        let sessions = self.sessions_for_workdir()?;
        if sessions.is_empty() {
            return Ok(vec!["no sessions for this workdir".to_string()]);
        }
        Ok(sessions
            .into_iter()
            .map(|session| {
                format_session_line(
                    &session.id,
                    &session.source,
                    &session.provider,
                    &session.model,
                    session.message_count,
                )
            })
            .collect())
    }

    fn session_show_lines(&self, reference: Option<&str>) -> Result<Vec<String>> {
        let id = match reference {
            Some(value) => self.resolve_session_ref(value)?,
            None => self
                .current_session
                .clone()
                .ok_or_else(|| anyhow!("no current session"))?,
        };
        let store = SqliteStore::open(&self.db_path)?;
        let summary = store
            .session_summary(&id)?
            .ok_or_else(|| anyhow!("session not found: {id}"))?;
        let mut lines = vec![format_session_line(
            &summary.id,
            &summary.source,
            &summary.provider,
            &summary.model,
            summary.message_count,
        )];
        for message in store.load_sanitized_messages(&id)? {
            let value = serde_json::to_value(message)?;
            lines.push(format_sanitized_message(&value));
        }
        Ok(lines)
    }

    fn model_lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "model: {}",
            self.current_model.as_deref().unwrap_or("(config default)")
        )];
        if !self.state.recent_models.is_empty() {
            lines.push(format!("recent: {}", self.state.recent_models.join(", ")));
        }
        lines
    }

    fn configured_model_lines(&self) -> Result<Vec<String>> {
        let models = configured_models(&self.run_options(String::new()))?;
        if models.is_empty() {
            return Ok(vec!["no configured models".to_string()]);
        }
        Ok(models.iter().map(format_configured_model).collect())
    }

    fn variant_line(&self) -> String {
        format!(
            "variant: {}",
            self.current_variant
                .as_deref()
                .unwrap_or("(config/default)")
        )
    }

    fn switch_session_no_print(&mut self, reference: &str) -> Result<String> {
        let id = self.resolve_session_ref(reference)?;
        SqliteStore::open(&self.db_path)?.resume_session(&id)?;
        self.current_session = Some(id.clone());
        self.force_new_once = false;
        Ok(id)
    }

    fn set_model_no_print(&mut self, model: String) -> Result<()> {
        validate_model_spec(&model)?;
        self.current_model = Some(model.clone());
        self.state.set_model(&self.workdir_key, model);
        self.state.save(&self.state_path)?;
        Ok(())
    }

    fn set_variant_no_print(&mut self, variant: String) -> Result<()> {
        validate_variant(&variant)?;
        self.current_variant = Some(variant.clone());
        self.state.set_variant(&self.workdir_key, variant);
        self.state.save(&self.state_path)?;
        Ok(())
    }

    fn set_mode_no_print(&mut self, mode: &str) -> Result<()> {
        let Some(parsed) = RunMode::parse(mode) else {
            return Err(anyhow!("mode must be one of plan, build"));
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

    fn cycle_mode(&mut self, ui: &mut FullscreenUi<'_>, _forward: bool) -> Result<()> {
        let next = match self.current_mode {
            RunMode::Plan => RunMode::Build,
            RunMode::Build => RunMode::Plan,
        };
        self.set_mode_no_print(next.as_str())?;
        ui.push_status(format!("mode: {}", next.as_str()));
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
}

struct RunningTurn {
    control: RunControlHandle,
    rx: mpsc::UnboundedReceiver<RunStreamEvent>,
    task: JoinHandle<psychevo_runtime::Result<psychevo_runtime::RunResult>>,
}

const TUI_CYAN: Color = Color::Cyan;
const TUI_MAGENTA: Color = Color::Magenta;
const TUI_GREEN: Color = Color::Green;
const TUI_RED: Color = Color::Red;
const TUI_DIM: Color = Color::DarkGray;

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
    Success,
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
    turn_started: Option<Instant>,
    turn_provider: String,
    turn_model: String,
    turn_mode: String,
    turn_context_limit: Option<u64>,
    turn_usage: Option<Value>,
    turn_metadata: Option<Value>,
    turn_failures: usize,
    reasoning_hidden_active: bool,
    running: Option<RunningTurn>,
    scroll: u16,
    focus: FocusMode,
    selected_row: Option<usize>,
    last_entry_areas: Vec<(usize, Rect)>,
    sidebar_forced: bool,
    sidebar_hidden: bool,
    last_sidebar_visible: bool,
    sidebar: SidebarSnapshot,
    history: Vec<String>,
    history_index: Option<usize>,
    history_search: bool,
    history_query: String,
    quit_requested: bool,
}

#[derive(Debug, Clone, Default)]
struct SidebarSnapshot {
    session: String,
    source: String,
    workdir: String,
    branch: String,
    model: String,
    variant: String,
    mode: String,
    thinking: String,
    message_count: usize,
    tool_count: usize,
    changed_files: Vec<String>,
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
            turn_started: None,
            turn_provider: String::new(),
            turn_model: String::new(),
            turn_mode: app.current_mode.as_str().to_string(),
            turn_context_limit: None,
            turn_usage: None,
            turn_metadata: None,
            turn_failures: 0,
            reasoning_hidden_active: false,
            running: None,
            scroll: 0,
            focus: FocusMode::Composer,
            selected_row: None,
            last_entry_areas: Vec::new(),
            sidebar_forced: false,
            sidebar_hidden: false,
            last_sidebar_visible: false,
            sidebar: SidebarSnapshot::default(),
            history: Vec::new(),
            history_index: None,
            history_search: false,
            history_query: String::new(),
            quit_requested: false,
        };
        ui.push_status("pevo fullscreen tui");
        ui.refresh_sidebar(app);
        ui
    }

    fn refresh_sidebar(&mut self, app: &TuiApp) {
        let git = git_snapshot(&app.workdir);
        self.sidebar = SidebarSnapshot {
            session: app
                .current_session
                .as_deref()
                .map(short_session)
                .unwrap_or("(none)")
                .to_string(),
            source: "tui".to_string(),
            workdir: tail_compact_path(&app.workdir.display().to_string(), 34),
            branch: git.branch,
            model: app
                .current_model
                .as_deref()
                .unwrap_or("(config default)")
                .to_string(),
            variant: app
                .current_variant
                .as_deref()
                .unwrap_or("(config/default)")
                .to_string(),
            mode: app.current_mode.as_str().to_string(),
            thinking: on_off(app.thinking_visible).to_string(),
            message_count: self
                .transcript
                .iter()
                .filter(|row| matches!(row.kind, TranscriptKind::Prompt | TranscriptKind::Answer))
                .count(),
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

    fn push_user(&mut self, text: String) {
        self.transcript.push(TranscriptRow::with_title(
            TranscriptKind::Prompt,
            "Prompt",
            text,
        ));
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
        self.reasoning_hidden_active = false;
    }

    fn push_status(&mut self, text: impl Into<String>) {
        self.transcript
            .push(TranscriptRow::simple(TranscriptKind::Status, text));
    }

    fn push_success(&mut self, text: impl Into<String>) {
        self.transcript
            .push(TranscriptRow::simple(TranscriptKind::Success, text));
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

    fn apply_stream_event(&mut self, event: RunStreamEvent, thinking_visible: bool, debug: bool) {
        match event {
            RunStreamEvent::ReasoningDelta { text } => {
                if thinking_visible {
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
                } else if !self.reasoning_hidden_active {
                    self.reasoning_hidden_active = true;
                    self.insert_evidence_row(TranscriptRow::with_title(
                        TranscriptKind::Thinking,
                        "Thinking",
                        "hidden",
                    ));
                }
            }
            RunStreamEvent::ReasoningEnd => {
                self.reasoning_hidden_active = false;
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
                    .unwrap_or("build")
                    .to_string();
                self.turn_context_limit = value.get("context_limit").and_then(Value::as_u64);
            }
            "message_update" | "message_end" => {
                if let Some(text) = assistant_text_from_event(value) {
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
                row.title = tool_title(tool, value);
                row.failed = failed;
                let (collapsed, full) = tool_output_text(value);
                row.text = if collapsed.is_empty() {
                    format_tool_summary(value)
                } else {
                    collapsed
                };
                row.full_text = full;
                self.update_turn_meta(debug);
            }
            _ => {}
        }
    }

    fn finish_turn(&mut self) {
        self.assistant_row = None;
        self.reasoning_row = None;
        self.meta_row = None;
        self.tool_rows.clear();
        self.reasoning_hidden_active = false;
        self.focus = FocusMode::Composer;
    }

    fn recall_history(&mut self, direction: isize) {
        if self.history.is_empty() {
            return;
        }
        let next = match self.history_index {
            None if direction < 0 => self.history.len().saturating_sub(1),
            None => return,
            Some(index) if direction < 0 => index.saturating_sub(1),
            Some(index) => {
                if index + 1 >= self.history.len() {
                    self.history_index = None;
                    self.textarea = new_textarea();
                    return;
                }
                index + 1
            }
        };
        self.history_index = Some(next);
        self.textarea = textarea_with_text(&self.history[next]);
    }

    fn update_turn_meta(&mut self, debug: bool) {
        let meta = turn_meta_text(TurnMetaProjection {
            mode: &self.turn_mode,
            provider: &self.turn_provider,
            model: &self.turn_model,
            started: self.turn_started,
            context_limit: self.turn_context_limit,
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
                "Meta",
                String::new(),
            ));
            self.meta_row = Some(idx);
            idx
        });
        self.transcript[idx].text = meta;
    }

    fn ensure_selection(&mut self) {
        if self
            .selected_row
            .is_some_and(|idx| idx < self.transcript.len())
        {
            return;
        }
        self.selected_row = self
            .transcript
            .iter()
            .position(TranscriptRow::is_expandable)
            .or_else(|| self.transcript.len().checked_sub(1));
    }

    fn move_selection(&mut self, direction: isize) {
        if self.transcript.is_empty() {
            self.selected_row = None;
            return;
        }
        self.ensure_selection();
        let current = self.selected_row.unwrap_or(0);
        let next = if direction < 0 {
            current.saturating_sub(1)
        } else {
            (current + 1).min(self.transcript.len().saturating_sub(1))
        };
        self.selected_row = Some(next);
    }

    fn toggle_selected(&mut self) {
        if let Some(index) = self.selected_row
            && let Some(row) = self.transcript.get_mut(index)
            && row.is_expandable()
        {
            row.expanded = !row.expanded;
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        if !matches!(mouse.kind, MouseEventKind::Down(_)) {
            return;
        }
        let Some((index, _)) = self.last_entry_areas.iter().find(|(_, rect)| {
            mouse.column >= rect.x
                && mouse.column < rect.x.saturating_add(rect.width)
                && mouse.row >= rect.y
                && mouse.row < rect.y.saturating_add(rect.height)
        }) else {
            return;
        };
        self.selected_row = Some(*index);
        self.focus = FocusMode::Transcript;
        self.toggle_selected();
    }
}

fn default_title(kind: TranscriptKind) -> &'static str {
    match kind {
        TranscriptKind::Prompt => "Prompt",
        TranscriptKind::Answer => "",
        TranscriptKind::Thinking => "Thinking",
        TranscriptKind::Explored => "Explored",
        TranscriptKind::Ran => "Ran",
        TranscriptKind::Changed => "Changed",
        TranscriptKind::Meta => "Meta",
        TranscriptKind::Status => "Status",
        TranscriptKind::Success => "Ok",
        TranscriptKind::Error => "Error",
    }
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
                .unwrap_or("command")
                .lines()
                .next()
                .unwrap_or("command")
                .trim();
            format!("Ran {command}")
        }
        "write" | "edit" => format!("Changed {}", path_from(args, result).unwrap_or("files")),
        other => format!("Tool {other}"),
    }
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

struct TurnMetaProjection<'a> {
    mode: &'a str,
    provider: &'a str,
    model: &'a str,
    started: Option<Instant>,
    context_limit: Option<u64>,
    usage: Option<&'a Value>,
    metadata: Option<&'a Value>,
    failures: usize,
    debug: bool,
}

fn turn_meta_text(meta: TurnMetaProjection<'_>) -> String {
    let mut parts = Vec::new();
    if !meta.mode.is_empty() {
        parts.push(format!("mode={}", meta.mode));
    }
    if !meta.provider.is_empty() || !meta.model.is_empty() {
        parts.push(format!("{}/{}", meta.provider, meta.model));
    }
    if let Some(started) = meta.started {
        parts.push(format!("elapsed={}ms", started.elapsed().as_millis()));
    }
    if let (Some(total), Some(limit)) =
        (meta.usage.and_then(usage_total_tokens), meta.context_limit)
    {
        let pct = (total as f64 / limit as f64) * 100.0;
        parts.push(format!("tokens={total}/{limit} {pct:.1}%"));
    }
    if meta.failures > 0 {
        parts.push(format!("failures={}", meta.failures));
    }
    if meta.debug {
        if let Some(usage) = meta.usage {
            let mut usage_parts = Vec::new();
            for key in [
                "input_tokens",
                "output_tokens",
                "reasoning_tokens",
                "cached_tokens",
                "total_tokens",
            ] {
                if let Some(value) = usage.get(key).and_then(Value::as_u64) {
                    usage_parts.push(format!("{key}={value}"));
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
                .take(5)
                .map(|(key, value)| format!("{key}={}", compact_value(value)))
                .collect::<Vec<_>>()
                .join(" ");
            parts.push(format!("metadata {summary}"));
        }
    }
    parts.join("  ")
}

fn usage_total_tokens(usage: &Value) -> Option<u64> {
    usage.get("total_tokens").and_then(Value::as_u64)
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
    textarea.set_block(
        Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(TUI_CYAN)),
    );
    textarea.set_wrap_mode(WrapMode::WordOrGlyph);
    textarea.set_cursor_line_style(Style::default());
    textarea.set_placeholder_text("Ask pevo...");
    textarea
}

fn textarea_with_text<'a>(text: &str) -> TextArea<'a> {
    let mut textarea = TextArea::new(text.split('\n').map(ToString::to_string).collect());
    textarea.set_block(
        Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(TUI_CYAN)),
    );
    textarea.set_wrap_mode(WrapMode::WordOrGlyph);
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
    (lines + 1).clamp(3, 8)
}

fn render_transcript(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>) {
    let mut lines = Vec::new();
    let mut areas = Vec::new();
    let mut cursor = 0u16;
    for (index, row) in ui.transcript.iter().enumerate() {
        let row_lines = transcript_lines(row, ui.selected_row == Some(index));
        let height = row_lines.len() as u16;
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
        lines.extend(row_lines);
    }
    ui.last_entry_areas = areas;
    let paragraph = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::BOTTOM))
        .wrap(Wrap { trim: false })
        .scroll((ui.scroll, 0));
    frame.render_widget(paragraph, area);
}

fn transcript_lines(row: &TranscriptRow, selected: bool) -> Vec<Line<'static>> {
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
    out.push(Line::from(""));
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
        TranscriptKind::Thinking | TranscriptKind::Meta => Style::default().fg(TUI_DIM),
        TranscriptKind::Status => Style::default().fg(TUI_CYAN),
        TranscriptKind::Success => Style::default().fg(TUI_GREEN),
        TranscriptKind::Error => Style::default().fg(TUI_RED),
    }
}

fn style_for_body(kind: TranscriptKind, failed: bool) -> Style {
    if failed {
        return Style::default().fg(TUI_RED);
    }
    match kind {
        TranscriptKind::Thinking | TranscriptKind::Meta | TranscriptKind::Status => {
            Style::default().fg(TUI_DIM)
        }
        TranscriptKind::Success => Style::default().fg(TUI_GREEN),
        TranscriptKind::Error => Style::default().fg(TUI_RED),
        _ => Style::default(),
    }
}

fn render_composer(frame: &mut Frame<'_>, area: Rect, ui: &mut FullscreenUi<'_>, mode: RunMode) {
    ui.textarea.set_block(
        Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(TUI_CYAN))
            .style(Style::default().bg(Color::Rgb(18, 18, 22)))
            .title(format!(" {} ", mode.as_str())),
    );
    frame.render_widget(&ui.textarea, area);
}

fn render_slash_menu(frame: &mut Frame<'_>, area: Rect, items: &[crate::tui_slash::SlashMenuItem]) {
    let lines = items
        .iter()
        .map(|item| {
            let marker = if item.upcoming { " upcoming" } else { "" };
            Line::from(vec![
                Span::styled(item.command, Style::default().fg(TUI_CYAN)),
                Span::styled(
                    format!("  {}{marker}", item.description),
                    Style::default().fg(TUI_DIM),
                ),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::LEFT).title(" commands "))
            .style(Style::default().bg(Color::Rgb(16, 16, 20))),
        area,
    );
}

fn render_status(frame: &mut Frame<'_>, area: Rect, app: &TuiApp, ui: &FullscreenUi<'_>) {
    let running = if ui.running.is_some() {
        "running"
    } else {
        "idle"
    };
    let session = app
        .current_session
        .as_deref()
        .map(short_session)
        .unwrap_or("(new)");
    let detail = if area.width < 100 {
        format!(
            "  {running}  {}  {}  {}  thinking={}  Ctrl+T",
            app.current_mode.as_str(),
            app.current_model.as_deref().unwrap_or("config"),
            session,
            on_off(app.thinking_visible)
        )
    } else {
        format!(
            "  {running}  mode={}  model={}  variant={}  session={}  thinking={}  Ctrl+T transcript",
            app.current_mode.as_str(),
            app.current_model.as_deref().unwrap_or("config"),
            app.current_variant.as_deref().unwrap_or("config"),
            session,
            on_off(app.thinking_visible)
        )
    };
    let line = Line::from(vec![
        Span::styled(
            "pevo",
            Style::default()
                .fg(TUI_MAGENTA)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(detail),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, ui: &FullscreenUi<'_>) {
    let text = if ui.history_search {
        format!("history search: {}", ui.history_query)
    } else {
        match ui.focus {
            FocusMode::Composer if area.width < 100 => {
                "Enter submit  Ctrl+J newline  Tab mode  Ctrl+B sidebar  /help".to_string()
            }
            FocusMode::Composer => "Enter submit  Ctrl+J newline  Tab mode  Ctrl+B sidebar  Ctrl+R history  Ctrl+T transcript  /help".to_string(),
            FocusMode::Transcript if area.width < 100 => {
                "Transcript: Up/Down select  Enter/Space expand  Esc composer".to_string()
            }
            FocusMode::Transcript => "Transcript: Up/Down select  Enter/Space expand  Esc composer  PageUp/PageDown scroll".to_string(),
        }
    };
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(TUI_DIM)),
        area,
    );
}

fn render_sidebar(frame: &mut Frame<'_>, area: Rect, ui: &FullscreenUi<'_>) {
    let mut lines = vec![
        sidebar_heading("Session", TUI_MAGENTA),
        Line::from(format!("session: {}", ui.sidebar.session)),
        Line::from(format!("source: {}", ui.sidebar.source)),
        Line::from(""),
        sidebar_heading("Context", TUI_CYAN),
        Line::from(format!("workdir: {}", ui.sidebar.workdir)),
        Line::from(format!("branch: {}", ui.sidebar.branch)),
        Line::from(format!("model: {}", ui.sidebar.model)),
        Line::from(format!("variant: {}", ui.sidebar.variant)),
        Line::from(format!("mode: {}", ui.sidebar.mode)),
        Line::from(format!("thinking: {}", ui.sidebar.thinking)),
        Line::from(format!("messages: {}", ui.sidebar.message_count)),
        Line::from(format!("tools: {}", ui.sidebar.tool_count)),
        Line::from(""),
        sidebar_heading("Modified Files", TUI_CYAN),
    ];
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
    lines.push(Line::from(""));
    lines.push(sidebar_heading("Footer", TUI_CYAN));
    lines.push(Line::from("local facts only"));
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::LEFT))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn sidebar_heading(label: &'static str, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled("▌ ", Style::default().fg(color)),
        Span::styled(
            label,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ])
}

fn short_session(id: &str) -> &str {
    &id[..id.len().min(8)]
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
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
        }
    }

    fn render_event(&mut self, event: &RunStreamEvent, out: &mut impl Write) -> io::Result<()> {
        match event {
            RunStreamEvent::ReasoningDelta { text } => {
                if !self.reasoning_active {
                    self.reasoning_active = true;
                    if self.thinking_enabled {
                        writeln!(out, "Thinking:")?;
                    } else {
                        writeln!(out, "Thinking: hidden")?;
                    }
                }
                if self.thinking_enabled {
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
                    .unwrap_or("build")
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
                    context_limit: self.context_limit,
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
                writeln!(out, "{}: running", tool_title(tool, value))?;
            }
            "tool_execution_end" => {
                let outcome = value
                    .get("outcome")
                    .and_then(Value::as_str)
                    .unwrap_or("normal");
                let summary = format_tool_summary(value);
                let label = match evidence_kind(
                    value
                        .get("tool_name")
                        .and_then(Value::as_str)
                        .unwrap_or("tool"),
                ) {
                    TranscriptKind::Explored => "Explored",
                    TranscriptKind::Ran => "Ran",
                    TranscriptKind::Changed => "Changed",
                    _ => "Tool",
                };
                if outcome == "normal" {
                    writeln!(
                        out,
                        "{}",
                        self.renderer.success(&format!("{label}: {summary}"))
                    )?;
                } else {
                    writeln!(
                        out,
                        "{}",
                        self.renderer.error(&format!("{label}: failed {summary}"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use std::fs;
    use tempfile::tempdir;

    fn summary(id: &str) -> SessionSummary {
        SessionSummary {
            id: id.to_string(),
            source: "tui".to_string(),
            workdir: "/repo".to_string(),
            model: "model".to_string(),
            provider: "provider".to_string(),
            started_at_ms: 1,
            updated_at_ms: 1,
            ended_at_ms: None,
            end_reason: None,
            message_count: 0,
            tool_call_count: 0,
            title: None,
        }
    }

    #[test]
    fn resolves_unique_and_ambiguous_session_prefixes() {
        let sessions = vec![summary("abcdef"), summary("abc999"), summary("def000")];
        assert_eq!(
            resolve_session_ref_from_summaries(&sessions, "def").unwrap(),
            "def000"
        );
        assert!(resolve_session_ref_from_summaries(&sessions, "abc").is_err());
        assert_eq!(
            resolve_session_ref_from_summaries(&sessions, "latest").unwrap(),
            "abcdef"
        );
    }

    #[test]
    fn turn_printer_hides_reasoning_by_default() {
        let mut printer = TurnPrinter::new(TuiRenderer::new(false), false, false);
        let mut output = Vec::new();
        printer
            .render_event(
                &RunStreamEvent::ReasoningDelta {
                    text: "private".to_string(),
                },
                &mut output,
            )
            .expect("delta");
        printer
            .render_event(&RunStreamEvent::ReasoningEnd, &mut output)
            .expect("end");

        let output = String::from_utf8(output).expect("utf8");
        assert!(output.contains("Thinking: hidden"));
        assert!(!output.contains("private"));
    }

    #[test]
    fn turn_printer_shows_reasoning_when_enabled() {
        let mut printer = TurnPrinter::new(TuiRenderer::new(false), true, false);
        let mut output = Vec::new();
        printer
            .render_event(
                &RunStreamEvent::ReasoningDelta {
                    text: "visible thinking".to_string(),
                },
                &mut output,
            )
            .expect("delta");
        printer
            .render_event(&RunStreamEvent::ReasoningEnd, &mut output)
            .expect("end");

        let output = String::from_utf8(output).expect("utf8");
        assert!(output.contains("Thinking:"));
        assert!(output.contains("visible thinking"));
    }

    #[test]
    fn tui_snapshot_wide_idle_composer_with_sidebar() {
        let temp = tempdir().expect("temp");
        let app = test_app(&temp);
        let ui = fixture_ui(&app, FixtureKind::Idle);
        assert_tui_snapshot("wide_idle_composer_with_sidebar", 120, 24, &app, ui);
    }

    #[test]
    fn tui_snapshot_narrow_idle_composer_without_sidebar() {
        let temp = tempdir().expect("temp");
        let app = test_app(&temp);
        let ui = fixture_ui(&app, FixtureKind::Idle);
        assert_tui_snapshot("narrow_idle_composer_without_sidebar", 80, 20, &app, ui);
    }

    #[test]
    fn tui_snapshot_slash_menu_prefix_filtering() {
        let temp = tempdir().expect("temp");
        let app = test_app(&temp);
        let mut ui = fixture_ui(&app, FixtureKind::Idle);
        ui.textarea = textarea_with_text("/mo");
        assert_tui_snapshot("slash_menu_prefix_filtering", 120, 24, &app, ui);
    }

    #[test]
    fn tui_snapshot_running_turn_with_visible_thinking() {
        let temp = tempdir().expect("temp");
        let app = test_app(&temp);
        let ui = fixture_ui(&app, FixtureKind::RunningThinking);
        assert_tui_snapshot("running_turn_with_visible_thinking", 120, 24, &app, ui);
    }

    #[test]
    fn tui_snapshot_completed_ledger_collapsed_tool_output() {
        let temp = tempdir().expect("temp");
        let app = test_app(&temp);
        let ui = fixture_ui(&app, FixtureKind::CollapsedTool);
        assert_tui_snapshot("completed_ledger_collapsed_tool_output", 120, 24, &app, ui);
    }

    #[test]
    fn tui_snapshot_expanded_long_tool_output() {
        let temp = tempdir().expect("temp");
        let app = test_app(&temp);
        let ui = fixture_ui(&app, FixtureKind::ExpandedTool);
        assert_tui_snapshot("expanded_long_tool_output", 120, 24, &app, ui);
    }

    #[test]
    fn tui_snapshot_debug_meta_with_usage_metadata() {
        let temp = tempdir().expect("temp");
        let mut app = test_app(&temp);
        app.debug = true;
        let ui = fixture_ui(&app, FixtureKind::DebugMeta);
        assert_tui_snapshot("debug_meta_with_usage_metadata", 120, 24, &app, ui);
    }

    #[test]
    fn tui_snapshot_failure_tool_error_turn_meta() {
        let temp = tempdir().expect("temp");
        let app = test_app(&temp);
        let ui = fixture_ui(&app, FixtureKind::FailureMeta);
        assert_tui_snapshot("failure_tool_error_turn_meta", 120, 24, &app, ui);
    }

    #[test]
    fn transcript_selection_toggles_expandable_output() {
        let temp = tempdir().expect("temp");
        let app = test_app(&temp);
        let mut ui = FullscreenUi::new(&app);
        let mut row = TranscriptRow::with_title(TranscriptKind::Explored, "Explored log", "a");
        row.full_text = Some("a\nb\nc".to_string());
        ui.transcript.push(row);
        ui.focus = FocusMode::Transcript;
        ui.ensure_selection();
        ui.toggle_selected();
        assert!(ui.transcript[1].expanded);
        ui.toggle_selected();
        assert!(!ui.transcript[1].expanded);
    }

    #[test]
    fn turn_meta_hides_tokens_without_context_limit_unless_debug() {
        let usage = serde_json::json!({
            "input_tokens": 2,
            "output_tokens": 3,
            "total_tokens": 5
        });
        let default = turn_meta_text(TurnMetaProjection {
            mode: "build",
            provider: "provider",
            model: "model",
            started: None,
            context_limit: None,
            usage: Some(&usage),
            metadata: None,
            failures: 0,
            debug: false,
        });
        assert!(!default.contains("tokens="));
        let metadata = serde_json::json!({"provider_response_id":"resp"});
        let debug = turn_meta_text(TurnMetaProjection {
            mode: "build",
            provider: "provider",
            model: "model",
            started: None,
            context_limit: None,
            usage: Some(&usage),
            metadata: Some(&metadata),
            failures: 0,
            debug: true,
        });
        assert!(debug.contains("usage input_tokens=2"));
        assert!(debug.contains("metadata provider_response_id=resp"));
    }

    #[tokio::test]
    async fn fullscreen_drain_keeps_queued_events_after_task_completion() {
        let temp = tempdir().expect("temp");
        let mut app = test_app(&temp);
        let mut ui = FullscreenUi::new(&app);
        let (tx, rx) = mpsc::unbounded_channel();
        tx.send(RunStreamEvent::Event(serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "final answer"}],
                "timestamp_ms": 1,
                "finish_reason": "stop",
                "outcome": "normal"
            }
        })))
        .expect("send answer");
        tx.send(RunStreamEvent::Event(serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "call_read_fixture",
            "tool_name": "read",
            "args": {"path": "fixture.txt"}
        })))
        .expect("send start");
        tx.send(RunStreamEvent::Event(serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_read_fixture",
            "tool_name": "read",
            "args": {"path": "fixture.txt"},
            "result": {"path": "fixture.txt", "content": "fixture content"},
            "outcome": "normal"
        })))
        .expect("send end");
        drop(tx);

        let result = psychevo_runtime::RunResult {
            session_id: "finished-session".to_string(),
            outcome: Outcome::Normal,
            final_answer: "done".to_string(),
            db_path: app.db_path.clone(),
            workdir: app.workdir.clone(),
            provider: "mock".to_string(),
            model: "mock-model".to_string(),
            base_url: "http://127.0.0.1".to_string(),
            api_key_env: Some("TEST_PROVIDER_KEY".to_string()),
            reasoning_effort: None,
            context_limit: None,
            tool_failures: 0,
            events: Vec::new(),
        };
        let task = tokio::spawn(async move { Ok(result) });
        let (control, _) = run_control();
        ui.running = Some(RunningTurn { control, rx, task });
        while !ui.running.as_ref().expect("running").task.is_finished() {
            tokio::task::yield_now().await;
        }

        app.drain_fullscreen_events(&mut ui).await.expect("drain");

        let tool_row = ui
            .transcript
            .iter()
            .find(|row| row.title == "Explored fixture.txt")
            .expect("tool evidence row");
        assert_eq!(tool_row.kind, TranscriptKind::Explored);
        assert_eq!(tool_row.text, "fixture content");
        let tool_index = ui
            .transcript
            .iter()
            .position(|row| row.title == "Explored fixture.txt")
            .expect("tool index");
        let answer_index = ui
            .transcript
            .iter()
            .position(|row| row.kind == TranscriptKind::Answer)
            .expect("answer index");
        assert!(tool_index < answer_index);
        assert!(ui.running.is_none());
    }

    fn test_app(temp: &tempfile::TempDir) -> TuiApp {
        let home = temp.path().join("home");
        let workdir = temp.path().join("work");
        std::fs::create_dir_all(&workdir).expect("workdir");
        let workdir = workdir.canonicalize().expect("canonical");
        TuiApp {
            env_map: BTreeMap::new(),
            home: home.clone(),
            state_path: home.join("tui-state.json"),
            state: TuiState::default(),
            db_path: home.join("state.db"),
            config_path: None,
            workdir: workdir.clone(),
            workdir_key: workdir.display().to_string(),
            current_session: Some("1234567890abcdef".to_string()),
            force_new_once: false,
            current_model: Some("mock/model".to_string()),
            current_variant: Some("high".to_string()),
            current_mode: RunMode::Build,
            thinking_visible: true,
            renderer: TuiRenderer::new(false),
            debug: false,
            had_error: false,
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum FixtureKind {
        Idle,
        RunningThinking,
        CollapsedTool,
        ExpandedTool,
        DebugMeta,
        FailureMeta,
    }

    fn fixture_ui<'a>(app: &TuiApp, kind: FixtureKind) -> FullscreenUi<'a> {
        let mut ui = FullscreenUi::new(app);
        ui.sidebar = stable_sidebar();
        match kind {
            FixtureKind::Idle => {}
            FixtureKind::RunningThinking => {
                ui.transcript.clear();
                ui.push_user("Inspect the CLI rendering path.".to_string());
                ui.start_assistant();
                ui.apply_value_event(
                    &serde_json::json!({
                        "type": "run_start",
                        "provider": "mock",
                        "model": "mock-model",
                        "mode": "build",
                        "context_limit": 64000
                    }),
                    false,
                );
                ui.turn_started = None;
                ui.apply_stream_event(
                    RunStreamEvent::ReasoningDelta {
                        text: "Read the TUI renderer and identify stable evidence blocks."
                            .to_string(),
                    },
                    true,
                    false,
                );
                ui.transcript.push(TranscriptRow::with_title(
                    TranscriptKind::Explored,
                    "Explored crates/psychevo-cli/src/tui.rs",
                    "running",
                ));
            }
            FixtureKind::CollapsedTool | FixtureKind::ExpandedTool => {
                ui.transcript.clear();
                push_completed_turn(&mut ui, kind);
            }
            FixtureKind::DebugMeta => {
                ui.transcript.clear();
                push_completed_turn(&mut ui, kind);
                ui.sidebar_hidden = true;
            }
            FixtureKind::FailureMeta => {
                ui.transcript.clear();
                push_failure_turn(&mut ui);
            }
        }
        ui.sidebar = stable_sidebar();
        ui
    }

    fn stable_sidebar() -> SidebarSnapshot {
        SidebarSnapshot {
            session: "12345678".to_string(),
            source: "tui".to_string(),
            workdir: "/repo/psychevo".to_string(),
            branch: "main".to_string(),
            model: "mock/mock-model".to_string(),
            variant: "high".to_string(),
            mode: "build".to_string(),
            thinking: "on".to_string(),
            message_count: 2,
            tool_count: 1,
            changed_files: vec![
                "M crates/psychevo-cli/src/tui.rs".to_string(),
                "?? specs/210-pevo-tui/testing.md".to_string(),
            ],
        }
    }

    fn push_completed_turn(ui: &mut FullscreenUi<'_>, kind: FixtureKind) {
        ui.push_user("Summarize the TUI snapshot harness.".to_string());
        ui.transcript.push(TranscriptRow::with_title(
            TranscriptKind::Thinking,
            "Thinking",
            "Check layout boundaries, style roles, and expandable evidence.",
        ));
        let mut row = TranscriptRow::with_title(
            TranscriptKind::Explored,
            "Explored crates/psychevo-cli/src/tui.rs",
            long_tool_output()
                .lines()
                .take(collapsed_fixture_lines(kind))
                .collect::<Vec<_>>()
                .join("\n")
                + &format!("\n... {} more lines", 24 - collapsed_fixture_lines(kind)),
        );
        row.full_text = Some(long_tool_output());
        if matches!(kind, FixtureKind::ExpandedTool) {
            row.expanded = true;
            ui.focus = FocusMode::Transcript;
            ui.selected_row = Some(2);
        }
        ui.transcript.push(row);
        ui.transcript.push(TranscriptRow::with_title(
            TranscriptKind::Answer,
            "",
            "The harness snapshots stable buffer text and style roles, then leaves real terminal screenshots as diagnostics.",
        ));
        let debug = matches!(kind, FixtureKind::DebugMeta);
        let usage = if debug {
            serde_json::json!({
                "input_tokens": 120,
                "total_tokens": 177
            })
        } else {
            serde_json::json!({
            "input_tokens": 120,
            "output_tokens": 45,
            "reasoning_tokens": 12,
            "total_tokens": 177
            })
        };
        let metadata = if debug {
            serde_json::json!({
                "provider_response_id": "resp_snapshot"
            })
        } else {
            serde_json::json!({
                "provider_response_id": "resp_snapshot",
                "system_fingerprint": "fp_mock"
            })
        };
        ui.transcript.push(TranscriptRow::with_title(
            TranscriptKind::Meta,
            "Meta",
            turn_meta_text(TurnMetaProjection {
                mode: "build",
                provider: "mock",
                model: "mock-model",
                started: None,
                context_limit: Some(64000),
                usage: Some(&usage),
                metadata: Some(&metadata),
                failures: 0,
                debug,
            }),
        ));
    }

    fn push_failure_turn(ui: &mut FullscreenUi<'_>) {
        ui.push_user("Run a command that fails.".to_string());
        let mut row = TranscriptRow::with_title(
            TranscriptKind::Ran,
            "Ran cargo test -p psychevo-cli",
            "exit_code=101\ncompile error: fixture failure",
        );
        row.failed = true;
        ui.transcript.push(row);
        ui.transcript.push(TranscriptRow::with_title(
            TranscriptKind::Answer,
            "",
            "The run failed before producing a clean validation result.",
        ));
        ui.transcript.push(TranscriptRow::with_title(
            TranscriptKind::Meta,
            "Meta",
            "mode=build  mock/mock-model  failures=1",
        ));
    }

    fn long_tool_output() -> String {
        (1..=24)
            .map(|line| format!("{line:02}: crates/psychevo-cli/src/tui.rs evidence row"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn collapsed_fixture_lines(kind: FixtureKind) -> usize {
        match kind {
            FixtureKind::ExpandedTool => 20,
            _ => 4,
        }
    }

    fn assert_tui_snapshot(
        name: &str,
        width: u16,
        height: u16,
        app: &TuiApp,
        mut ui: FullscreenUi<'_>,
    ) {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| app.render_fullscreen(frame, &mut ui))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        let text = buffer_text(buffer);
        let styles = buffer_style_text(buffer);
        let combined = format!(
            "fixture={name}\nsize={width}x{height}\n\n--- text ---\n{text}\n--- styles ---\n{styles}"
        );
        write_snapshot_diagnostics(name, &text, &styles, &combined);
        insta::with_settings!({ prepend_module_to_snapshot => false }, {
            insta::assert_snapshot!(name, combined);
        });
    }

    fn write_snapshot_diagnostics(name: &str, text: &str, styles: &str, combined: &str) {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/pevo-tui-snapshots")
            .join(name);
        if fs::create_dir_all(&dir).is_err() {
            return;
        }
        let _ = fs::write(dir.join("text.txt"), text);
        let _ = fs::write(dir.join("styles.txt"), styles);
        let _ = fs::write(dir.join("combined.txt"), combined);
        let _ = fs::write(
            dir.join("metadata.json"),
            serde_json::json!({
                "fixture": name,
                "source": "ratatui TestBackend",
                "golden": "insta snapshot"
            })
            .to_string(),
        );
    }

    fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
        let area = *buffer.area();
        let mut text = String::new();
        for y in area.y..area.y + area.height {
            let mut line = String::new();
            for x in area.x..area.x + area.width {
                line.push_str(buffer.cell((x, y)).expect("cell").symbol());
            }
            text.push_str(line.trim_end());
            text.push('\n');
        }
        text
    }

    fn buffer_style_text(buffer: &ratatui::buffer::Buffer) -> String {
        let area = *buffer.area();
        let mut text = String::new();
        for y in area.y..area.y + area.height {
            let mut line = String::new();
            let mut last = None;
            for x in area.x..area.x + area.width {
                let cell = buffer.cell((x, y)).expect("cell");
                if last != Some(cell.fg) {
                    last = Some(cell.fg);
                    line.push_str(style_marker(cell.fg));
                }
                line.push_str(cell.symbol());
            }
            text.push_str(line.trim_end());
            text.push('\n');
        }
        text
    }

    fn style_marker(color: Color) -> &'static str {
        if color == TUI_MAGENTA || color == Color::Magenta {
            "[magenta]"
        } else if color == TUI_CYAN || color == Color::Cyan {
            "[cyan]"
        } else if color == TUI_GREEN || color == Color::Green {
            "[green]"
        } else if color == TUI_RED || color == Color::Red {
            "[red]"
        } else if color == TUI_DIM || color == Color::DarkGray {
            "[dim]"
        } else {
            "[default]"
        }
    }
}
