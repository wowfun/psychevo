use std::collections::BTreeMap;

use psychevo_runtime::{RunStreamEvent, RunWarning};
use serde_json::{Value, json};

use crate::protocol::{
    GatewayEvent, GatewaySelectedSkill, GatewayTurn, GatewayTurnError, GatewayTurnStatus,
    TranscriptBlock, TranscriptBlockKind, TranscriptBlockStatus, TranscriptEntry,
    TranscriptEntryRole,
};

#[derive(Debug, Default)]
pub struct GatewayLiveProjector {
    thread_id: Option<String>,
    active_turn_id: Option<String>,
    assistant_segment: usize,
    stream_seq: u64,
    entries: BTreeMap<usize, LiveEntryState>,
    tool_owners: BTreeMap<String, usize>,
    tool_aliases: BTreeMap<String, String>,
    tool_positions: BTreeMap<String, String>,
    tool_args: BTreeMap<String, Value>,
    exec_sessions: BTreeMap<u64, LiveExecState>,
    child_projectors: BTreeMap<String, GatewayLiveProjector>,
}

#[derive(Debug, Clone)]
struct LiveEntryState {
    segment: usize,
    started: bool,
    created_at_ms: i64,
    updated_at_ms: i64,
    next_placeholder_order: i64,
    blocks: BTreeMap<String, TranscriptBlock>,
}

#[derive(Debug, Clone)]
struct LiveExecState {
    tool_call_id: String,
    segment: usize,
    metadata: Value,
    output: String,
}

struct AssistantContentProjection<'a> {
    turn_id: &'a str,
    event_value: &'a Value,
    content_block: &'a Value,
    index: usize,
    segment: usize,
    status: TranscriptBlockStatus,
    is_tool_call_turn: bool,
}

struct LiveToolBlockUpdate<'a> {
    turn_id: &'a str,
    segment: usize,
    tool_call_id: &'a str,
    tool_name: &'a str,
    status: TranscriptBlockStatus,
    body: Option<String>,
    metadata: Value,
    completed: bool,
}

struct LiveToolBlockBuild<'a> {
    turn_id: &'a str,
    segment: usize,
    tool_call_id: &'a str,
    tool_name: &'a str,
    status: TranscriptBlockStatus,
    body: Option<String>,
    metadata: Value,
    order: Option<i64>,
}

include!("projection/live_projector.rs");
include!("projection/live_projector_agents.rs");
include!("projection/live_projector_assistant.rs");
include!("projection/live_projector_state.rs");
include!("projection/live_projector_tools.rs");
include!("projection/live_helpers.rs");
include!("projection/runtime_events.rs");
include!("projection/tool_helpers.rs");

#[cfg(test)]
mod tests {
    use super::*;

    include!("projection/tests/runtime_projection.rs");
}
