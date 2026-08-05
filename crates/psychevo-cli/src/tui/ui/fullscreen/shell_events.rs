use std::time::{Duration, Instant};

use crate::tui::{
    FullscreenUi, PresentedShellEvent, ShellCommandEvent, ShellCommandOutcome, ToolDisplaySpec,
    TranscriptKind, TranscriptRow, instant_from_wall_timestamp_ms, tool_result_output_text,
    user_shell_title,
};

impl<'a> FullscreenUi<'a> {
    pub(crate) fn apply_shell_event(&mut self, event: PresentedShellEvent) -> bool {
        let key = shell_presentation_key(event.presentation_id);
        match event.event {
            ShellCommandEvent::Started {
                command,
                started_at_ms,
                ..
            } => {
                let title = user_shell_title(command.lines().find(|line| !line.trim().is_empty()));
                let idx = self.tool_rows.get(&key).copied().unwrap_or_else(|| {
                    self.insert_evidence_row(TranscriptRow::with_title(
                        TranscriptKind::Ran,
                        title.clone(),
                        "running",
                    ))
                });
                let row = &mut self.transcript[idx];
                row.kind = TranscriptKind::Ran;
                row.title = title;
                row.text = "running".to_string();
                row.full_text = None;
                row.failed = false;
                row.interrupted = false;
                row.user_shell = true;
                row.tool_name = Some("exec_command".to_string());
                row.tool_started = Some(
                    instant_from_wall_timestamp_ms(started_at_ms).unwrap_or_else(Instant::now),
                );
                row.tool_elapsed = None;
                self.tool_rows.insert(key, idx);
                self.remove_turn_meta();
                true
            }
            ShellCommandEvent::Completed {
                output,
                outcome,
                elapsed_ms,
                ..
            } => {
                let idx = self.tool_rows.get(&key).copied().unwrap_or_else(|| {
                    let mut row = TranscriptRow::with_title(TranscriptKind::Ran, "!", "running");
                    row.tool_name = Some("exec_command".to_string());
                    row.user_shell = true;
                    self.insert_evidence_row(row)
                });
                let interrupted = outcome == ShellCommandOutcome::Interrupted;
                let failed = outcome == ShellCommandOutcome::Failed;
                let row = &mut self.transcript[idx];
                row.kind = TranscriptKind::Ran;
                row.failed = failed;
                row.interrupted = interrupted;
                row.user_shell = true;
                row.tool_name = Some("exec_command".to_string());
                let runtime_elapsed = Duration::from_millis(elapsed_ms);
                row.tool_elapsed = Some(
                    row.tool_started
                        .map(|started| runtime_elapsed.max(started.elapsed()))
                        .unwrap_or(runtime_elapsed),
                );
                row.tool_started = None;
                if interrupted {
                    row.text = "interrupted".to_string();
                    row.full_text = None;
                } else {
                    let display = ToolDisplaySpec::for_name("exec_command");
                    let (text, full_text) = tool_result_output_text(
                        "exec_command",
                        shell_outcome_label(outcome),
                        &output,
                        &display,
                    );
                    row.text = text;
                    row.full_text = full_text;
                }
                self.tool_rows.remove(&key);
                false
            }
            ShellCommandEvent::Warning { message, .. } => {
                self.push_status(format!("warning: {message}"));
                false
            }
        }
    }
}

pub(crate) fn shell_outcome_label(outcome: ShellCommandOutcome) -> &'static str {
    match outcome {
        ShellCommandOutcome::Completed => "normal",
        ShellCommandOutcome::Failed => "failed",
        ShellCommandOutcome::Interrupted => "aborted",
    }
}

fn shell_presentation_key(presentation_id: u64) -> String {
    format!("user-shell:{presentation_id}")
}
