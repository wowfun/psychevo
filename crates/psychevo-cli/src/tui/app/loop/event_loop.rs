#[path = "event_loop/constants.rs"]
mod constants;
#[path = "event_loop/keyboard.rs"]
mod keyboard;
#[path = "event_loop/mouse.rs"]
mod mouse;
#[path = "event_loop/run_loop.rs"]
mod run_loop;

#[cfg(test)]
pub(crate) use constants::FULLSCREEN_EVENT_POLL_INTERVAL;
pub(crate) use constants::FULLSCREEN_PASSIVE_REDRAW_INTERVAL;
