#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MotionMode {
    Animated,
    Static,
}

fn tui_motion_mode() -> MotionMode {
    if cfg!(test) {
        MotionMode::Static
    } else {
        MotionMode::Animated
    }
}

fn activity_spinner_frame(elapsed: Duration) -> &'static str {
    match tui_motion_mode() {
        MotionMode::Animated => animated_spinner_frame(elapsed),
        MotionMode::Static => "◦",
    }
}

fn animated_spinner_frame(elapsed: Duration) -> &'static str {
    const FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
    let index = ((elapsed.as_millis() / 120) as usize) % FRAMES.len();
    FRAMES[index]
}
