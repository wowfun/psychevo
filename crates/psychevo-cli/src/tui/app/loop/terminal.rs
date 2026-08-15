use crate::tui::{
    BottomPanel, FULLSCREEN_PASSIVE_REDRAW_INTERVAL, FullscreenUi, ModelTab, Result, TuiApp,
    short_session,
};
use crossterm::{
    cursor::{Hide, Show},
    event::{DisableBracketedPaste, EnableBracketedPaste, MouseEventKind},
    execute, queue,
    terminal::{
        BeginSynchronizedUpdate, EndSynchronizedUpdate, EnterAlternateScreen, LeaveAlternateScreen,
        disable_raw_mode, enable_raw_mode,
    },
};
use ratatui::{
    Frame, Terminal,
    backend::{Backend, ClearType, CrosstermBackend, WindowSize},
    buffer::Cell,
    layout::{Position, Size},
};
use std::{
    io::{self, Write},
    time::Instant,
};
pub(crate) const TUI_MOUSE_CAPTURE_ENABLE_ANSI: &str = concat!(
    "\x1b[?1000h",
    "\x1b[?1002h",
    "\x1b[?1015h",
    "\x1b[?1006h",
    "\x1b[?1007h"
);
pub(crate) const TUI_MOUSE_CAPTURE_DISABLE_ANSI: &str = concat!(
    "\x1b[?1007l",
    "\x1b[?1006l",
    "\x1b[?1015l",
    "\x1b[?1002l",
    "\x1b[?1000l"
);
const TUI_TERMINAL_TITLE_PREFIX: &str = "Pevo | ";
const TUI_TERMINAL_TITLE_MAX_CHARS: usize = 240;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SetTerminalTitle<'a>(&'a str);

impl crossterm::Command for SetTerminalTitle<'_> {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b]0;{}\x07", self.0)
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Err(io::Error::other(
            "tried to execute SetTerminalTitle using WinAPI; use ANSI instead",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

#[derive(Debug, Default)]
pub(crate) struct ManagedTerminalTitle {
    last: Option<String>,
}

impl ManagedTerminalTitle {
    pub(crate) fn sync(&mut self, out: &mut impl Write, title: &str) -> io::Result<()> {
        let title = sanitize_terminal_title(title);
        if self.last.as_deref() == Some(title.as_str()) {
            return Ok(());
        }
        execute!(out, SetTerminalTitle(&title))?;
        self.last = Some(title);
        Ok(())
    }

    pub(crate) fn clear(&mut self, out: &mut impl Write) -> io::Result<()> {
        if self.last.is_none() {
            return Ok(());
        }
        execute!(out, SetTerminalTitle(""))?;
        self.last = None;
        Ok(())
    }
}

fn sanitize_terminal_title(title: &str) -> String {
    let mut sanitized = String::new();
    let mut pending_space = false;
    for ch in title.chars() {
        if is_terminal_title_unsafe(ch) {
            continue;
        }
        if ch.is_whitespace() {
            pending_space = !sanitized.is_empty();
            continue;
        }
        if pending_space && sanitized.chars().count() < TUI_TERMINAL_TITLE_MAX_CHARS {
            sanitized.push(' ');
        }
        pending_space = false;
        if sanitized.chars().count() >= TUI_TERMINAL_TITLE_MAX_CHARS {
            break;
        }
        sanitized.push(ch);
    }
    sanitized
}

fn is_terminal_title_unsafe(ch: char) -> bool {
    ch.is_control()
        || matches!(
            ch,
            '\u{061c}'
                | '\u{200b}'..='\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2060}'..='\u{206f}'
                | '\u{feff}'
                | '\u{fff9}'..='\u{fffb}'
        )
}

impl TuiApp {
    pub(crate) fn terminal_tab_title(&self) -> String {
        let label = self
            .current_session_title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
            .map(str::to_owned)
            .or_else(|| {
                self.current_session
                    .as_deref()
                    .map(|session_id| short_session(session_id).to_string())
            })
            .unwrap_or_else(|| "New session".to_string());
        format!("{TUI_TERMINAL_TITLE_PREFIX}{label}")
    }
}

pub(crate) fn fullscreen_has_passive_motion(ui: &FullscreenUi<'_>) -> bool {
    ui.foreground_turn_active()
        || !ui.auxiliary_agent_tasks.is_empty()
        || !ui.auxiliary_shell_tasks.is_empty()
}

pub(crate) fn schedule_next_passive_redraw(now: Instant) -> Instant {
    now.checked_add(FULLSCREEN_PASSIVE_REDRAW_INTERVAL)
        .unwrap_or(now)
}

pub(crate) fn passive_redraw_due(now: Instant, next_due: &mut Instant) -> bool {
    if now < *next_due {
        return false;
    }
    *next_due = schedule_next_passive_redraw(now);
    true
}

pub(crate) trait FullscreenFrameBackend: Backend {
    fn begin_synchronized_update(&mut self) -> std::result::Result<(), Self::Error>;

    fn queue_cursor_visibility(&mut self, visible: bool) -> std::result::Result<(), Self::Error> {
        if visible {
            Backend::show_cursor(self)
        } else {
            Backend::hide_cursor(self)
        }
    }

    fn end_synchronized_update(&mut self) -> std::result::Result<(), Self::Error>;
}

impl<W: Write> FullscreenFrameBackend for CrosstermBackend<W> {
    fn begin_synchronized_update(&mut self) -> io::Result<()> {
        queue!(self, BeginSynchronizedUpdate)
    }

    fn queue_cursor_visibility(&mut self, visible: bool) -> io::Result<()> {
        if visible {
            queue!(self, Show)
        } else {
            queue!(self, Hide)
        }
    }

    fn end_synchronized_update(&mut self) -> io::Result<()> {
        execute!(self, EndSynchronizedUpdate)
    }
}

pub(crate) struct StableCursorBackend<B> {
    inner: B,
    cursor_visible: bool,
    synchronized_update: bool,
    requested_cursor_visibility: Option<bool>,
}

impl<B> StableCursorBackend<B> {
    pub(crate) const fn hidden(inner: B) -> Self {
        Self {
            inner,
            cursor_visible: false,
            synchronized_update: false,
            requested_cursor_visibility: None,
        }
    }

    #[cfg(test)]
    pub(crate) const fn inner(&self) -> &B {
        &self.inner
    }

    #[cfg(test)]
    pub(crate) const fn inner_mut(&mut self) -> &mut B {
        &mut self.inner
    }

    fn apply_cursor_visibility(&mut self, visible: bool) -> Result<(), B::Error>
    where
        B: Backend,
    {
        if self.cursor_visible == visible {
            return Ok(());
        }
        if visible {
            self.inner.show_cursor()?;
        } else {
            self.inner.hide_cursor()?;
        }
        self.cursor_visible = visible;
        Ok(())
    }
}

impl<B: Write> Write for StableCursorBackend<B> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<B: Backend> Backend for StableCursorBackend<B> {
    type Error = B::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        self.inner.draw(content)
    }

    fn append_lines(&mut self, n: u16) -> Result<(), Self::Error> {
        self.inner.append_lines(n)
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        if self.synchronized_update {
            self.requested_cursor_visibility = Some(false);
            return Ok(());
        }
        self.apply_cursor_visibility(false)
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        if self.synchronized_update {
            self.requested_cursor_visibility = Some(true);
            return Ok(());
        }
        self.apply_cursor_visibility(true)
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> Result<Size, Self::Error> {
        self.inner.size()
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }
}

impl<B: FullscreenFrameBackend> FullscreenFrameBackend for StableCursorBackend<B> {
    fn begin_synchronized_update(&mut self) -> Result<(), Self::Error> {
        self.inner.begin_synchronized_update()?;
        self.synchronized_update = true;
        self.requested_cursor_visibility = None;
        Ok(())
    }

    fn end_synchronized_update(&mut self) -> Result<(), Self::Error> {
        let requested_visibility = self.requested_cursor_visibility.take();
        let visibility_result = match requested_visibility {
            Some(visible) if self.cursor_visible != visible => {
                self.inner.queue_cursor_visibility(visible)
            }
            _ => Ok(()),
        };
        let end_result = self.inner.end_synchronized_update();
        self.synchronized_update = false;
        match (visibility_result, end_result) {
            (Err(err), _) => Err(err),
            (Ok(()), Err(err)) => Err(err),
            (Ok(()), Ok(())) => {
                if let Some(visible) = requested_visibility {
                    self.cursor_visible = visible;
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
impl FullscreenFrameBackend for ratatui::backend::TestBackend {
    fn begin_synchronized_update(&mut self) -> std::result::Result<(), Self::Error> {
        Ok(())
    }

    fn end_synchronized_update(&mut self) -> std::result::Result<(), Self::Error> {
        Ok(())
    }
}

pub(crate) fn draw_fullscreen_frame<B, F>(
    terminal: &mut Terminal<B>,
    clear_requested: bool,
    render: F,
) -> std::result::Result<(), B::Error>
where
    B: FullscreenFrameBackend,
    F: FnOnce(&mut Frame<'_>),
{
    terminal.backend_mut().begin_synchronized_update()?;
    let frame_result = (|| {
        if clear_requested {
            terminal.clear()?;
        }
        terminal.draw(render)?;
        Ok(())
    })();
    let end_result = terminal.backend_mut().end_synchronized_update();
    match (frame_result, end_result) {
        (Err(err), _) => Err(err),
        (Ok(()), result) => result,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EnableTuiMouseCapture;

impl crossterm::Command for EnableTuiMouseCapture {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str(TUI_MOUSE_CAPTURE_ENABLE_ANSI)
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Err(io::Error::other(
            "tried to execute EnableTuiMouseCapture using WinAPI; use ANSI instead",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DisableTuiMouseCapture;

impl crossterm::Command for DisableTuiMouseCapture {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str(TUI_MOUSE_CAPTURE_DISABLE_ANSI)
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Err(io::Error::other(
            "tried to execute DisableTuiMouseCapture using WinAPI; use ANSI instead",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

#[derive(Debug)]
pub(crate) struct FullscreenTerminalGuard {
    pub(crate) active: bool,
    terminal_title: ManagedTerminalTitle,
}

impl FullscreenTerminalGuard {
    pub(crate) fn enter(stdout: &mut io::Stdout) -> Result<Self> {
        enable_raw_mode()?;
        if let Err(err) = write_fullscreen_enter_commands(stdout) {
            let _ = restore_fullscreen_terminal_modes();
            return Err(err.into());
        }
        Ok(Self {
            active: true,
            terminal_title: ManagedTerminalTitle::default(),
        })
    }

    pub(crate) fn sync_title(&mut self, out: &mut impl Write, title: &str) {
        let _ = self.terminal_title.sync(out, title);
    }

    pub(crate) fn restore(&mut self) -> Result<()> {
        if self.active {
            let _ = self.terminal_title.clear(&mut io::stdout());
            restore_fullscreen_terminal_modes()?;
            self.active = false;
        }
        Ok(())
    }
}

impl Drop for FullscreenTerminalGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = self.terminal_title.clear(&mut io::stdout());
            let _ = restore_fullscreen_terminal_modes();
            self.active = false;
        }
    }
}

pub(crate) fn write_fullscreen_enter_commands(out: &mut impl Write) -> io::Result<()> {
    execute!(out, EnterAlternateScreen)?;
    execute!(
        out,
        crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
        crossterm::cursor::MoveTo(0, 0)
    )?;
    execute!(out, EnableBracketedPaste)?;
    execute!(out, EnableTuiMouseCapture)?;
    execute!(out, crossterm::cursor::SetCursorStyle::SteadyBar)?;
    execute!(out, crossterm::cursor::Hide)?;
    Ok(())
}

pub(crate) fn write_fullscreen_exit_commands(out: &mut impl Write) -> io::Result<()> {
    let mut first_error = execute!(out, EndSynchronizedUpdate).err();
    if let Err(err) = execute!(out, DisableBracketedPaste) {
        first_error.get_or_insert(err);
    }
    if let Err(err) = execute!(out, DisableTuiMouseCapture) {
        first_error.get_or_insert(err);
    }
    if let Err(err) = execute!(out, LeaveAlternateScreen) {
        first_error.get_or_insert(err);
    }
    if let Err(err) = execute!(out, crossterm::cursor::SetCursorStyle::DefaultUserShape) {
        first_error.get_or_insert(err);
    }
    if let Err(err) = execute!(out, crossterm::cursor::Show) {
        first_error.get_or_insert(err);
    }
    match first_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

pub(crate) fn restore_fullscreen_terminal_modes() -> io::Result<()> {
    let mut stdout = io::stdout();
    let mut first_error = write_fullscreen_exit_commands(&mut stdout).err();
    if let Err(err) = disable_raw_mode() {
        first_error.get_or_insert(err);
    }
    match first_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FullscreenEventOutcome {
    pub(crate) needs_draw: bool,
    pub(crate) should_quit: bool,
}

pub(crate) fn mouse_event_needs_redraw(kind: MouseEventKind) -> bool {
    !matches!(kind, MouseEventKind::Moved)
}

pub(crate) fn scroll_bottom_panel(panel: &mut BottomPanel, amount: isize) {
    match panel {
        BottomPanel::Help(panel) => panel.scroll_by(amount),
        BottomPanel::Models(panel) if panel.tab == ModelTab::Info => panel.scroll_info_by(amount),
        BottomPanel::PermissionApproval(panel) => panel.scroll_by(amount),
        BottomPanel::ProviderWizard(_) => {}
        _ => panel.selection_mut().move_selection(amount),
    }
}

pub(crate) fn normalize_bracketed_paste_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}
