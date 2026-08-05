use psychevo::{ShellCommandOutcome, ShellCommandResult, TurnResult};
use psychevo_gateway_protocol::events_transcript::TranscriptEntry;
use psychevo_gateway_protocol::source::{GatewayThread, GatewayTurn};

#[derive(Debug)]
pub struct GatewayTurnResult {
    pub thread: GatewayThread,
    pub turn: GatewayTurn,
    pub result: TurnResult,
    pub committed_entries: Vec<TranscriptEntry>,
}

#[derive(Debug)]
pub struct GatewayShellResult {
    pub thread: GatewayThread,
    pub result: ShellCommandResult,
    pub committed_entries: Vec<TranscriptEntry>,
}

pub(crate) fn shell_outcome_wire_value(outcome: ShellCommandOutcome) -> &'static str {
    match outcome {
        ShellCommandOutcome::Completed => "normal",
        ShellCommandOutcome::Failed => "failed",
        ShellCommandOutcome::Interrupted => "aborted",
    }
}
