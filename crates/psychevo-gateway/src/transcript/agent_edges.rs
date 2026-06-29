pub(crate) fn enrich_agent_blocks_from_edges(
    entries: &mut [TranscriptEntry],
    edges: &[AgentEdgeRecord],
) {
    let mut used_edges = BTreeMap::<usize, ()>::new();
    for entry in entries {
        for block in &mut entry.blocks {
            if block.kind != TranscriptBlockKind::Agent
                || block.status == TranscriptBlockStatus::Failed
            {
                continue;
            }
            let mut metadata = metadata_object(block.metadata.take());
            if agent_result_child_session_id(&metadata).is_some() {
                block.metadata = Some(Value::Object(metadata));
                continue;
            }
            let Some((edge_index, edge)) =
                matching_agent_edge_for_block(&metadata, edges, &used_edges)
            else {
                block.metadata = Some(Value::Object(metadata));
                continue;
            };
            used_edges.insert(edge_index, ());
            enrich_agent_metadata_from_edge(&mut metadata, edge);
            block.metadata = Some(Value::Object(metadata.clone()));
            if let Some(result) = &mut block.result {
                result.metadata = Some(Value::Object(metadata));
            }
        }
    }
}

fn enrich_committed_agent_metadata(metadata: &mut serde_json::Map<String, Value>) {
    let args = metadata.get("args").cloned().unwrap_or(Value::Null);
    let result = ensure_json_object_field(metadata, "result");
    for key in [
        "agent_name",
        "agent_type",
        "agent_path",
        "task_name",
        "message",
        "parent_thread_id",
        "parent_session_id",
        "child_thread_id",
        "child_session_id",
        "session_id",
    ] {
        if result.get(key).is_none()
            && let Some(value) = args.get(key).filter(|value| !value.is_null())
        {
            result.insert(key.to_string(), value.clone());
        }
    }
    if result.get("message").is_none()
        && let Some(prompt) = args
            .get("message")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|prompt| !prompt.is_empty())
    {
        result.insert("message".to_string(), json!(prompt));
    }
    if result.get("task").is_none()
        && let Some(prompt) = result
            .get("message")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|prompt| !prompt.is_empty())
    {
        result.insert("task".to_string(), json!(prompt));
    }
    if result.get("child_thread_id").is_none()
        && let Some(child_session_id) = result
            .get("child_session_id")
            .or_else(|| result.get("session_id"))
            .filter(|value| !value.is_null())
            .cloned()
    {
        result.insert("child_thread_id".to_string(), child_session_id);
    }
    if result.get("child_session_id").is_none()
        && let Some(session_id) = result
            .get("session_id")
            .filter(|value| !value.is_null())
            .cloned()
    {
        result.insert("child_session_id".to_string(), session_id);
    }
    if result.get("session_id").is_none()
        && let Some(child_session_id) = result
            .get("child_session_id")
            .filter(|value| !value.is_null())
            .cloned()
    {
        result.insert("session_id".to_string(), child_session_id);
    }
    if result.get("parent_thread_id").is_none()
        && let Some(parent_session_id) = result
            .get("parent_session_id")
            .filter(|value| !value.is_null())
            .cloned()
    {
        result.insert("parent_thread_id".to_string(), parent_session_id);
    }
}

fn matching_agent_edge_for_block<'a>(
    metadata: &serde_json::Map<String, Value>,
    edges: &'a [AgentEdgeRecord],
    used_edges: &BTreeMap<usize, ()>,
) -> Option<(usize, &'a AgentEdgeRecord)> {
    let args = metadata.get("args").unwrap_or(&Value::Null);
    let result = metadata.get("result").unwrap_or(&Value::Null);
    let tool_call_id = metadata
        .get("tool_call_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(match_by_tool_call) = edges.iter().enumerate().find(|(index, edge)| {
        !used_edges.contains_key(index)
            && tool_call_id
                .is_some_and(|id| agent_edge_string(edge, "parent_tool_call_id") == Some(id))
    }) {
        return Some(match_by_tool_call);
    }

    let result_agent_id = result
        .get("agent_id")
        .or_else(|| result.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(match_by_agent_id) = edges.iter().enumerate().find(|(index, edge)| {
        !used_edges.contains_key(index)
            && result_agent_id.is_some_and(|id| agent_edge_string(edge, "id") == Some(id))
    }) {
        return Some(match_by_agent_id);
    }

    let agent_name = result
        .get("agent_name")
        .or_else(|| result.get("agent_type"))
        .or_else(|| args.get("agent_type"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let task_label = result
        .get("task_name")
        .or_else(|| args.get("task_name"))
        .or_else(|| result.get("task"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let task_prompt = result
        .get("message")
        .or_else(|| result.get("task"))
        .or_else(|| args.get("message"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    edges.iter().enumerate().find(|(index, edge)| {
        !used_edges.contains_key(index)
            && agent_name.is_some_and(|name| agent_edge_string(edge, "name") == Some(name))
            && (task_label.is_some_and(|label| {
                agent_edge_string(edge, "task_name") == Some(label)
                    || agent_edge_string(edge, "task") == Some(label)
            }) || task_prompt
                .is_some_and(|prompt| agent_edge_string(edge, "task") == Some(prompt)))
    })
}

fn enrich_agent_metadata_from_edge(
    metadata: &mut serde_json::Map<String, Value>,
    edge: &AgentEdgeRecord,
) {
    let result = ensure_json_object_field(metadata, "result");
    result.insert(
        "child_thread_id".to_string(),
        Value::String(edge.child_session_id.clone()),
    );
    result.insert(
        "child_session_id".to_string(),
        Value::String(edge.child_session_id.clone()),
    );
    result.insert(
        "session_id".to_string(),
        Value::String(edge.child_session_id.clone()),
    );
    result.insert(
        "parent_thread_id".to_string(),
        Value::String(edge.parent_session_id.clone()),
    );
    result.insert(
        "parent_session_id".to_string(),
        Value::String(edge.parent_session_id.clone()),
    );
    if let Some(value) = agent_edge_string(edge, "id")
        && result.get("agent_id").is_none()
    {
        result.insert("agent_id".to_string(), Value::String(value.to_string()));
    }
    for key in [
        "name",
        "agent_type",
        "agent_path",
        "task_name",
        "message",
        "task",
    ] {
        if let Some(value) = agent_edge_string(edge, key) {
            let result_key = if key == "name" { "agent_name" } else { key };
            result
                .entry(result_key.to_string())
                .or_insert_with(|| Value::String(value.to_string()));
        }
    }
}

fn agent_result_child_session_id(metadata: &serde_json::Map<String, Value>) -> Option<&str> {
    metadata
        .get("result")
        .and_then(|result| {
            result
                .get("child_thread_id")
                .or_else(|| result.get("child_session_id"))
                .or_else(|| result.get("session_id"))
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn agent_edge_string<'a>(edge: &'a AgentEdgeRecord, key: &str) -> Option<&'a str> {
    edge.metadata
        .as_ref()?
        .get("agent")?
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}
