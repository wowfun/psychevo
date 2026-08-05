use psychevo::AgentRelationship;
use serde_json::{Value, json};

use psychevo_gateway_protocol::events_transcript::{
    TranscriptBlockKind, TranscriptBlockStatus, TranscriptEntry,
};

use super::{ensure_json_object_field, metadata_object};

pub(crate) fn enrich_agent_blocks_from_relationships(
    entries: &mut [TranscriptEntry],
    relationships: &[AgentRelationship],
) {
    let mut used_relationships = vec![false; relationships.len()];
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
            let Some((relationship_index, relationship)) = matching_agent_relationship_for_block(
                &metadata,
                relationships,
                &used_relationships,
            ) else {
                block.metadata = Some(Value::Object(metadata));
                continue;
            };
            used_relationships[relationship_index] = true;
            enrich_agent_metadata_from_relationship(&mut metadata, relationship);
            block.metadata = Some(Value::Object(metadata.clone()));
            if let Some(result) = &mut block.result {
                result.metadata = Some(Value::Object(metadata));
            }
        }
    }
}

pub(crate) fn agent_relationship_lookup_candidates(entries: &[TranscriptEntry]) -> Vec<String> {
    let mut candidates = std::collections::BTreeSet::new();
    for block in entries
        .iter()
        .flat_map(|entry| &entry.blocks)
        .filter(|block| block.kind == TranscriptBlockKind::Agent)
    {
        let Some(metadata) = block.metadata.as_ref().and_then(Value::as_object) else {
            continue;
        };
        insert_candidate(&mut candidates, metadata.get("tool_call_id"));
        for object in ["args", "result"]
            .into_iter()
            .filter_map(|key| metadata.get(key).and_then(Value::as_object))
        {
            for key in [
                "agent_id",
                "id",
                "agent_name",
                "agent_type",
                "task_name",
                "task",
                "message",
            ] {
                insert_candidate(&mut candidates, object.get(key));
            }
        }
    }
    candidates.into_iter().collect()
}

fn insert_candidate(candidates: &mut std::collections::BTreeSet<String>, value: Option<&Value>) {
    if let Some(value) = value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        candidates.insert(value.to_string());
    }
}

pub(super) fn enrich_committed_agent_metadata(metadata: &mut serde_json::Map<String, Value>) {
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

fn matching_agent_relationship_for_block<'a>(
    metadata: &serde_json::Map<String, Value>,
    relationships: &'a [AgentRelationship],
    used_relationships: &[bool],
) -> Option<(usize, &'a AgentRelationship)> {
    let args = metadata.get("args").unwrap_or(&Value::Null);
    let result = metadata.get("result").unwrap_or(&Value::Null);
    let tool_call_id = metadata
        .get("tool_call_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(match_by_tool_call) =
        relationships
            .iter()
            .enumerate()
            .find(|(index, relationship)| {
                !used_relationships[*index]
                    && tool_call_id.is_some_and(|id| {
                        relationship
                            .agent
                            .as_ref()
                            .and_then(|agent| agent.parent_tool_call_id.as_deref())
                            == Some(id)
                    })
            })
    {
        return Some(match_by_tool_call);
    }

    let result_agent_id = result
        .get("agent_id")
        .or_else(|| result.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(match_by_agent_id) =
        relationships
            .iter()
            .enumerate()
            .find(|(index, relationship)| {
                !used_relationships[*index]
                    && result_agent_id.is_some_and(|id| {
                        relationship
                            .agent
                            .as_ref()
                            .and_then(|agent| agent.id.as_deref())
                            == Some(id)
                    })
            })
    {
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

    relationships
        .iter()
        .enumerate()
        .find(|(index, relationship)| {
            let Some(agent) = relationship.agent.as_ref() else {
                return false;
            };
            !used_relationships[*index]
                && agent_name.is_some_and(|name| agent.name.as_deref() == Some(name))
                && (task_label.is_some_and(|label| {
                    agent.task_name.as_deref() == Some(label)
                        || agent.task.as_deref() == Some(label)
                }) || task_prompt.is_some_and(|prompt| agent.task.as_deref() == Some(prompt)))
        })
}

fn enrich_agent_metadata_from_relationship(
    metadata: &mut serde_json::Map<String, Value>,
    relationship: &AgentRelationship,
) {
    let result = ensure_json_object_field(metadata, "result");
    result.insert(
        "child_thread_id".to_string(),
        Value::String(relationship.child_thread_id.clone()),
    );
    result.insert(
        "child_session_id".to_string(),
        Value::String(relationship.child_thread_id.clone()),
    );
    result.insert(
        "session_id".to_string(),
        Value::String(relationship.child_thread_id.clone()),
    );
    result.insert(
        "parent_thread_id".to_string(),
        Value::String(relationship.parent_thread_id.clone()),
    );
    result.insert(
        "parent_session_id".to_string(),
        Value::String(relationship.parent_thread_id.clone()),
    );
    let Some(agent) = relationship.agent.as_ref() else {
        return;
    };
    for (key, value) in [
        ("agent_id", agent.id.as_deref()),
        ("agent_name", agent.name.as_deref()),
        ("task_name", agent.task_name.as_deref()),
        ("task", agent.task.as_deref()),
        ("agent_description", agent.description.as_deref()),
        ("parent_tool_call_id", agent.parent_tool_call_id.as_deref()),
        ("team_run_id", agent.team_run_id.as_deref()),
        ("mission_run_id", agent.mission_run_id.as_deref()),
        ("team_name", agent.team_name.as_deref()),
        ("team_member_id", agent.team_member_id.as_deref()),
        ("runtime_ref", agent.runtime_ref.as_deref()),
        ("role", agent.role.as_deref()),
    ] {
        insert_agent_string_if_missing(result, key, value);
    }
    insert_agent_bool_if_missing(result, "background", agent.background);
    insert_agent_bool_if_missing(result, "fork_context", agent.fork_context);
}

fn insert_agent_string_if_missing(
    result: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<&str>,
) {
    if result.get(key).is_none()
        && let Some(value) = value.map(str::trim).filter(|value| !value.is_empty())
    {
        result.insert(key.to_string(), Value::String(value.to_string()));
    }
}

fn insert_agent_bool_if_missing(
    result: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<bool>,
) {
    if result.get(key).is_none()
        && let Some(value) = value
    {
        result.insert(key.to_string(), Value::Bool(value));
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
