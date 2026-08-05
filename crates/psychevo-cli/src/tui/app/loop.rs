#[path = "loop/event_loop.rs"]
mod event_loop;
#[cfg(test)]
pub(crate) use event_loop::FULLSCREEN_EVENT_POLL_INTERVAL;
pub(crate) use event_loop::FULLSCREEN_PASSIVE_REDRAW_INTERVAL;
#[path = "loop/terminal.rs"]
mod terminal;
pub(crate) use terminal::{
    FullscreenEventOutcome, FullscreenTerminalGuard, fullscreen_has_passive_motion,
    mouse_event_needs_redraw, normalize_bracketed_paste_text, passive_redraw_due,
    schedule_next_passive_redraw, scroll_bottom_panel,
};
#[cfg(test)]
pub(crate) use terminal::{
    ManagedTerminalTitle, TUI_MOUSE_CAPTURE_DISABLE_ANSI, TUI_MOUSE_CAPTURE_ENABLE_ANSI,
    write_fullscreen_enter_commands, write_fullscreen_exit_commands,
};
