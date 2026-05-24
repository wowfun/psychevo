#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) fn activity_spinner_frame(elapsed: Duration) -> &'static str {
    animated_spinner_frame(elapsed)
}

pub(crate) fn animated_spinner_frame(elapsed: Duration) -> &'static str {
    pub(crate) const FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
    let index = ((elapsed.as_millis() / 120) as usize) % FRAMES.len();
    FRAMES[index]
}
