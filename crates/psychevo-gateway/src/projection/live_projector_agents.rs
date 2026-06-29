impl GatewayLiveProjector {
    fn project_agent_session_start(
        &mut self,
        turn_id: &str,
        value: &Value,
    ) -> Option<GatewayEvent> {
        let child_session_id = value
            .get("child_thread_id")
            .or_else(|| value.get("child_session_id"))
            .or_else(|| value.get("session_id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|child_session_id| !child_session_id.is_empty())?;
        let raw_tool_call_id = value
            .get("tool_call_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|tool_call_id| !tool_call_id.is_empty())
            .unwrap_or(child_session_id);
        let tool_call_id = self.strict_agent_tool_call_id(turn_id, raw_tool_call_id, value);
        let segment = self.tool_owner_segment(&tool_call_id);
        let mut metadata = agent_session_start_metadata(value, &tool_call_id, child_session_id);
        if metadata.get("args").is_none_or(Value::is_null)
            && let Some(args) = self.tool_args.get(&tool_call_id)
        {
            set_metadata_field(&mut metadata, "args", args.clone());
        }
        self.enrich_agent_metadata_from_existing(turn_id, segment, &tool_call_id, &mut metadata);
        enrich_agent_metadata_from_fields(&mut metadata);
        let body = agent_task_summary(&metadata);
        let block = self.live_tool_block_from_metadata(LiveToolBlockBuild {
            turn_id,
            segment,
            tool_call_id: &tool_call_id,
            tool_name: "spawn_agent",
            status: TranscriptBlockStatus::Running,
            body,
            metadata,
            order: None,
        });
        self.upsert_block(segment, block);
        Some(self.emit_entry_event(turn_id, segment, false, false))
    }

    fn enrich_agent_metadata_from_existing(
        &self,
        turn_id: &str,
        segment: usize,
        tool_call_id: &str,
        metadata: &mut Value,
    ) {
        let Some(existing) = self
            .entries
            .get(&segment)
            .and_then(|state| state.blocks.get(&live_tool_block_id(turn_id, tool_call_id)))
            .and_then(|block| block.metadata.as_ref())
        else {
            return;
        };
        copy_agent_metadata_field_if_missing(metadata, existing, "agent_id");
        copy_agent_metadata_field_if_missing(metadata, existing, "agent_name");
        copy_agent_metadata_field_if_missing(metadata, existing, "agent_description");
        copy_agent_metadata_field_if_missing(metadata, existing, "agent_type");
        copy_agent_metadata_field_if_missing(metadata, existing, "agent_path");
        copy_agent_metadata_field_if_missing(metadata, existing, "task_name");
        copy_agent_metadata_field_if_missing(metadata, existing, "message");
        copy_agent_metadata_field_if_missing(metadata, existing, "task");
        copy_agent_metadata_field_if_missing(metadata, existing, "prompt");
        copy_agent_metadata_field_if_missing(metadata, existing, "parent_thread_id");
        copy_agent_metadata_field_if_missing(metadata, existing, "parent_session_id");
        copy_agent_metadata_field_if_missing(metadata, existing, "child_thread_id");
        copy_agent_metadata_field_if_missing(metadata, existing, "child_session_id");
        copy_agent_metadata_field_if_missing(metadata, existing, "session_id");
        if metadata.get("args").is_none_or(Value::is_null)
            && let Some(args) = existing.get("args").filter(|args| !args.is_null())
        {
            set_metadata_field(metadata, "args", args.clone());
        }
    }
}

fn agent_session_start_metadata(
    value: &Value,
    tool_call_id: &str,
    child_session_id: &str,
) -> Value {
    let mut metadata = json!({
        "projection": "tool",
        "tool_name": "spawn_agent",
        "tool_call_id": tool_call_id,
        "type": "agent_session_start",
        "outcome": "normal",
        "child_thread_id": child_session_id,
        "child_session_id": child_session_id,
    });
    set_metadata_result_field(&mut metadata, "child_thread_id", json!(child_session_id));
    set_metadata_result_field(&mut metadata, "child_session_id", json!(child_session_id));
    set_metadata_result_field(&mut metadata, "session_id", json!(child_session_id));
    set_metadata_result_field(&mut metadata, "status", json!("running"));
    for key in [
        "agent_id",
        "agent_name",
        "agent_description",
        "agent_type",
        "agent_path",
        "task_name",
        "message",
        "parent_thread_id",
        "parent_session_id",
        "child_thread_id",
        "session_id",
        "background",
        "status",
    ] {
        if let Some(field) = value.get(key).filter(|field| !field.is_null()) {
            set_metadata_field(&mut metadata, key, field.clone());
            set_metadata_result_field(&mut metadata, key, field.clone());
        }
    }
    metadata
}

fn enrich_agent_metadata_from_fields(metadata: &mut Value) {
    if let Some(child_session_id) = agent_child_session_id(metadata).map(ToString::to_string) {
        set_metadata_field(
            metadata,
            "child_thread_id",
            json!(child_session_id.clone()),
        );
        set_metadata_result_field(
            metadata,
            "child_thread_id",
            json!(child_session_id.clone()),
        );
        set_metadata_field(
            metadata,
            "child_session_id",
            json!(child_session_id.clone()),
        );
        set_metadata_result_field(
            metadata,
            "child_session_id",
            json!(child_session_id.clone()),
        );
        set_metadata_result_field(metadata, "session_id", json!(child_session_id));
    }
    if let Some(agent_name) = agent_metadata_string(metadata, "agent_name")
        .or_else(|| agent_metadata_string(metadata, "agent_type"))
        .or_else(|| agent_metadata_string(metadata, "name"))
        .map(ToString::to_string)
    {
        set_metadata_field(metadata, "agent_name", json!(agent_name.clone()));
        set_metadata_field(metadata, "agent_type", json!(agent_name.clone()));
        set_metadata_result_field(metadata, "agent_name", json!(agent_name.clone()));
        set_metadata_result_field(metadata, "agent_type", json!(agent_name));
    }
    if let Some(task_name) = agent_metadata_string(metadata, "task_name").map(ToString::to_string) {
        set_metadata_field(metadata, "task_name", json!(task_name.clone()));
        set_metadata_result_field(metadata, "task_name", json!(task_name));
    }
    if let Some(task) = agent_metadata_string(metadata, "message")
        .or_else(|| agent_metadata_string(metadata, "task"))
        .or_else(|| agent_metadata_string(metadata, "prompt"))
        .map(ToString::to_string)
    {
        set_metadata_field(metadata, "message", json!(task.clone()));
        set_metadata_result_field(metadata, "message", json!(task.clone()));
        set_metadata_field(metadata, "task", json!(task.clone()));
        set_metadata_result_field(metadata, "task", json!(task));
    } else if let Some(prompt) = metadata
        .get("args")
        .and_then(|args| args.get("message").or_else(|| args.get("prompt")))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
        .map(ToString::to_string)
    {
        set_metadata_field(metadata, "message", json!(prompt.clone()));
        set_metadata_result_field(metadata, "message", json!(prompt.clone()));
        set_metadata_field(metadata, "task", json!(prompt.clone()));
        set_metadata_result_field(metadata, "task", json!(prompt));
    }
    if let Some(parent_session_id) =
        agent_metadata_string(metadata, "parent_thread_id")
            .or_else(|| agent_metadata_string(metadata, "parent_session_id"))
            .map(ToString::to_string)
    {
        set_metadata_field(metadata, "parent_thread_id", json!(parent_session_id.clone()));
        set_metadata_result_field(metadata, "parent_thread_id", json!(parent_session_id.clone()));
        set_metadata_result_field(metadata, "parent_session_id", json!(parent_session_id));
    }
}

fn copy_agent_metadata_field_if_missing(target: &mut Value, source: &Value, key: &str) {
    if agent_metadata_string(target, key).is_some() {
        return;
    }
    if let Some(value) = source
        .get(key)
        .filter(|value| !value.is_null())
        .cloned()
        .or_else(|| {
            source
                .get("result")
                .and_then(|result| result.get(key))
                .filter(|value| !value.is_null())
                .cloned()
        })
    {
        set_metadata_field(target, key, value.clone());
        set_metadata_result_field(target, key, value);
    }
}

fn agent_child_session_id(metadata: &Value) -> Option<&str> {
    agent_metadata_string(metadata, "child_thread_id")
        .or_else(|| agent_metadata_string(metadata, "child_session_id"))
        .or_else(|| agent_metadata_string(metadata, "session_id"))
}

fn agent_metadata_string<'a>(metadata: &'a Value, key: &str) -> Option<&'a str> {
    metadata
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            metadata
                .get("result")
                .and_then(|result| result.get(key))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            metadata
                .get("args")
                .and_then(|args| args.get(key))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
}

fn agent_task_summary(metadata: &Value) -> Option<String> {
    agent_metadata_string(metadata, "task_name")
        .or_else(|| agent_metadata_string(metadata, "task"))
        .or_else(|| agent_metadata_string(metadata, "prompt"))
        .or_else(|| agent_metadata_string(metadata, "agent_description"))
        .map(|value| compact_text(value, 240))
}
