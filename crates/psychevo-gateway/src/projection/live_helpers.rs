const DEFAULT_REASONING_ORDER: i64 = 0;
const DEFAULT_TEXT_ORDER: i64 = 100;
const TOOL_PLACEHOLDER_ORDER: i64 = 1000;

impl LiveEntryState {
    fn new(segment: usize) -> Self {
        let now = crate::gateway_now_ms();
        Self {
            segment,
            started: false,
            created_at_ms: now,
            updated_at_ms: now,
            next_placeholder_order: TOOL_PLACEHOLDER_ORDER,
            blocks: BTreeMap::new(),
        }
    }

    fn upsert_block(&mut self, block: TranscriptBlock) {
        let block = self
            .blocks
            .get(&block.id)
            .map(|existing| merge_live_block(existing, block.clone()))
            .unwrap_or(block);
        self.updated_at_ms = block.updated_at_ms;
        self.blocks.insert(block.id.clone(), block);
    }

    fn replace_blocks(&mut self, blocks: BTreeMap<String, TranscriptBlock>) {
        let mut replaced = BTreeMap::new();
        for (id, block) in blocks {
            let block = self
                .blocks
                .get(&id)
                .map(|existing| merge_live_block(existing, block.clone()))
                .unwrap_or(block);
            replaced.insert(id, block);
        }
        self.blocks = replaced;
        self.updated_at_ms = crate::gateway_now_ms();
    }

    fn tool_block_order(&self, tool_call_id: &str) -> Option<i64> {
        self.blocks.values().find_map(|block| {
            block
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("tool_call_id"))
                .and_then(Value::as_str)
                .is_some_and(|candidate| candidate == tool_call_id)
                .then_some(block.order)
        })
    }

    fn to_entry(
        &self,
        turn_id: &str,
        stream_seq: u64,
        authoritative_blocks: bool,
    ) -> TranscriptEntry {
        let mut blocks = self.blocks.values().cloned().collect::<Vec<_>>();
        blocks.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.created_at_ms.cmp(&right.created_at_ms))
                .then_with(|| left.id.cmp(&right.id))
        });
        TranscriptEntry {
            id: live_assistant_entry_id(turn_id, self.segment),
            thread_id: String::new(),
            turn_id: Some(turn_id.to_string()),
            message_seq: None,
            role: TranscriptEntryRole::Assistant,
            status: aggregate_entry_status(&blocks),
            source: "runtime.stream".to_string(),
            blocks,
            metadata: Some(json!({
                "projection": "assistant_segment",
                "liveOrder": self.segment,
                "streamSeq": stream_seq,
                "authoritativeBlocks": authoritative_blocks,
            })),
            usage: None,
            accounting: None,
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
        }
    }
}

fn merge_live_block(existing: &TranscriptBlock, next: TranscriptBlock) -> TranscriptBlock {
    let metadata = merge_live_block_metadata(existing, &next);
    TranscriptBlock {
        id: existing.id.clone(),
        kind: next.kind,
        status: merge_live_block_status(existing.status, next.status),
        order: next.order,
        phase_ordinal: next.phase_ordinal.or(existing.phase_ordinal),
        source: if next.source.trim().is_empty() {
            existing.source.clone()
        } else {
            next.source
        },
        title: next.title.or_else(|| existing.title.clone()),
        body: next.body.or_else(|| existing.body.clone()),
        preview: next.preview.or_else(|| existing.preview.clone()),
        detail: next.detail.or_else(|| existing.detail.clone()),
        artifact_ids: if next.artifact_ids.is_empty() {
            existing.artifact_ids.clone()
        } else {
            next.artifact_ids
        },
        metadata,
        result: next.result.or_else(|| existing.result.clone()),
        created_at_ms: existing.created_at_ms,
        updated_at_ms: next.updated_at_ms,
    }
}

fn merge_live_block_status(
    existing: TranscriptBlockStatus,
    next: TranscriptBlockStatus,
) -> TranscriptBlockStatus {
    if next == TranscriptBlockStatus::Pending && existing != TranscriptBlockStatus::Pending {
        existing
    } else {
        next
    }
}

fn merge_json_metadata(left: Option<Value>, right: Option<Value>) -> Option<Value> {
    match (left, right) {
        (Some(Value::Object(mut left)), Some(Value::Object(right))) => {
            for (key, value) in right {
                left.insert(key, value);
            }
            Some(Value::Object(left))
        }
        (_, Some(right)) => Some(right),
        (Some(left), None) => Some(left),
        (None, None) => None,
    }
}

fn merge_live_block_metadata(
    existing: &TranscriptBlock,
    next: &TranscriptBlock,
) -> Option<Value> {
    if block_is_spawn_agent(existing) || block_is_spawn_agent(next) {
        return merge_agent_block_metadata(existing.metadata.clone(), next.metadata.clone());
    }
    merge_json_metadata(existing.metadata.clone(), next.metadata.clone())
}

fn block_is_spawn_agent(block: &TranscriptBlock) -> bool {
    block
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("tool_name"))
        .and_then(Value::as_str)
        == Some("spawn_agent")
}

fn merge_agent_block_metadata(left: Option<Value>, right: Option<Value>) -> Option<Value> {
    match (left, right) {
        (Some(Value::Object(left)), Some(Value::Object(mut right))) => {
            for key in [
                "projection",
                "tool_name",
                "tool_call_id",
                "parent_thread_id",
                "parent_session_id",
                "child_thread_id",
                "child_session_id",
                "session_id",
                "agent_id",
                "agent_name",
                "agent_type",
                "agent_path",
                "task_name",
                "message",
                "task",
                "prompt",
                "args",
                "arguments",
            ] {
                copy_json_field_if_missing(&mut right, &left, key);
            }
            merge_agent_result_identity(&left, &mut right);
            Some(Value::Object(right))
        }
        (_, Some(right)) => Some(right),
        (Some(left), None) => Some(left),
        (None, None) => None,
    }
}

fn merge_agent_result_identity(
    left: &serde_json::Map<String, Value>,
    right: &mut serde_json::Map<String, Value>,
) {
    let Some(Value::Object(left_result)) = left.get("result") else {
        return;
    };
    let right_result = right
        .entry("result".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !right_result.is_object() {
        *right_result = Value::Object(serde_json::Map::new());
    }
    let Some(right_result) = right_result.as_object_mut() else {
        return;
    };
    for key in [
        "parent_thread_id",
        "parent_session_id",
        "child_thread_id",
        "child_session_id",
        "session_id",
        "agent_id",
        "agent_name",
        "agent_type",
        "agent_path",
        "task_name",
        "message",
        "task",
        "prompt",
    ] {
        copy_json_field_if_missing(right_result, left_result, key);
    }
}

fn copy_json_field_if_missing(
    target: &mut serde_json::Map<String, Value>,
    source: &serde_json::Map<String, Value>,
    key: &str,
) {
    if target.get(key).is_some_and(|value| !value.is_null()) {
        return;
    }
    if let Some(value) = source.get(key).filter(|value| !value.is_null()) {
        target.insert(key.to_string(), value.clone());
    }
}

fn aggregate_entry_status(blocks: &[TranscriptBlock]) -> TranscriptBlockStatus {
    if blocks
        .iter()
        .any(|block| block.status == TranscriptBlockStatus::Failed)
    {
        return TranscriptBlockStatus::Failed;
    }
    if blocks
        .iter()
        .any(|block| block.status == TranscriptBlockStatus::Cancelled)
    {
        return TranscriptBlockStatus::Cancelled;
    }
    if blocks
        .iter()
        .any(|block| block.status == TranscriptBlockStatus::NeedsInput)
    {
        return TranscriptBlockStatus::NeedsInput;
    }
    if blocks.iter().any(|block| {
        matches!(
            block.status,
            TranscriptBlockStatus::Running | TranscriptBlockStatus::Pending
        )
    }) {
        return TranscriptBlockStatus::Running;
    }
    TranscriptBlockStatus::Completed
}

fn live_block(
    id: String,
    kind: TranscriptBlockKind,
    status: TranscriptBlockStatus,
    order: i64,
    title: Option<String>,
    body: Option<String>,
    metadata: Option<Value>,
) -> TranscriptBlock {
    let now = crate::gateway_now_ms();
    TranscriptBlock {
        id,
        kind,
        status,
        order,
        phase_ordinal: None,
        source: "runtime.stream".to_string(),
        title,
        preview: body.as_deref().map(|text| compact_text(text, 240)),
        detail: body.clone(),
        body,
        artifact_ids: Vec::new(),
        metadata,
        result: None,
        created_at_ms: now,
        updated_at_ms: now,
    }
}

fn live_assistant_entry_id(turn_id: &str, segment: usize) -> String {
    format!("live:{turn_id}:assistant:{segment}")
}

fn live_reasoning_block_id(turn_id: &str, segment: usize) -> String {
    format!("live:{turn_id}:assistant:{segment}:reasoning")
}

fn live_text_block_id(turn_id: &str, segment: usize, index: usize) -> String {
    format!("live:{turn_id}:assistant:{segment}:text:{index}")
}

fn live_tool_block_id(turn_id: &str, tool_call_id: &str) -> String {
    format!("live:{turn_id}:tool:{tool_call_id}")
}

fn temporary_tool_call_id_for_value(
    tool_name: &str,
    segment: usize,
    value: &Value,
    event_seq: u64,
) -> String {
    tool_position_key(segment, value)
        .map(|position_key| format!("live-temp:{tool_name}:position:{position_key}"))
        .unwrap_or_else(|| format!("live-temp:{tool_name}:event:{event_seq}"))
}

fn temporary_tool_call_id(tool_call_id: &str) -> bool {
    tool_call_id.starts_with("live-temp:")
}

fn tool_position_key(segment: usize, value: &Value) -> Option<String> {
    let content_index = value
        .get("content_index")
        .or_else(|| value.get("content_array_index"))
        .and_then(Value::as_i64)?;
    let call_index = value
        .get("call_index")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    Some(format!("{segment}:{content_index}:{call_index}"))
}

fn content_block_order(_block: &Value, index: usize, _fallback: i64) -> i64 {
    index as i64
}

fn tool_message_block_metadata(block: &Value, index: usize) -> Option<(String, String, Value)> {
    let tool_name = block
        .get("name")
        .or_else(|| block.get("tool_name"))
        .or_else(|| {
            block
                .get("function")
                .and_then(|function| function.get("name"))
        })
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_string();
    let tool_call_id = block
        .get("id")
        .or_else(|| block.get("tool_call_id"))
        .or_else(|| block.get("call_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|tool_call_id| !tool_call_id.is_empty())
        .map(ToString::to_string)
        .unwrap_or_default();
    let arguments_json = block
        .get("arguments_json")
        .or_else(|| {
            block
                .get("function")
                .and_then(|function| function.get("arguments"))
        })
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let args = block
        .get("arguments")
        .or_else(|| block.get("args"))
        .cloned()
        .or_else(|| {
            arguments_json
                .as_deref()
                .and_then(|raw| serde_json::from_str(raw).ok())
        })
        .unwrap_or(Value::Null);
    let order = content_block_order(block, index, index as i64);
    let mut metadata = json!({
        "projection": "tool",
        "tool_name": tool_name.clone(),
        "tool_call_id": tool_call_id.clone(),
        "content_array_index": index,
        "liveOrder": order,
        "arguments": args.clone(),
        "args": args,
        "outcome": "normal",
    });
    if let Some(arguments_json) = arguments_json {
        set_metadata_field(&mut metadata, "arguments_json", json!(arguments_json));
    }
    for key in ["content_index", "call_index", "arguments_error"] {
        if let Some(value) = block.get(key) {
            set_metadata_field(&mut metadata, key, value.clone());
        }
    }
    Some((tool_call_id, tool_name, metadata))
}

fn event_thread_id(event: &GatewayEvent) -> Option<String> {
    match event {
        GatewayEvent::TurnStarted {
            thread_id: Some(thread_id),
            ..
        }
        | GatewayEvent::TurnQueued {
            thread_id: Some(thread_id),
            ..
        }
        | GatewayEvent::TurnCompleted {
            thread_id: Some(thread_id),
            ..
        } => Some(thread_id.clone()),
        GatewayEvent::EntryStarted { entry, .. }
        | GatewayEvent::EntryUpdated { entry, .. }
        | GatewayEvent::EntryCompleted { entry, .. }
            if !entry.thread_id.is_empty() =>
        {
            Some(entry.thread_id.clone())
        }
        _ => None,
    }
}

fn runtime_message_role(message: Option<&Value>) -> Option<&str> {
    message
        .and_then(|message| message.get("role"))
        .and_then(Value::as_str)
}
