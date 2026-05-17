fn activity_spinner_frame(elapsed: Duration) -> &'static str {
    animated_spinner_frame(elapsed)
}

fn animated_spinner_frame(elapsed: Duration) -> &'static str {
    const FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
    let index = ((elapsed.as_millis() / 120) as usize) % FRAMES.len();
    FRAMES[index]
}
