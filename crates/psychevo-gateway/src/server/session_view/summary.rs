fn session_summary_value(
    state: &WebState,
    summary: SessionSummary,
) -> psychevo_runtime::Result<Value> {
    let activity = state
        .inner
        .gateway
        .activity_for_selector(GatewayThreadSelector::thread_id(&summary.id));
    let entries = state.inner.gateway.thread_transcript(&summary.id)?;
    let preview = session_preview(&entries);
    let display_title = summary
        .title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .or_else(|| preview.clone())
        .unwrap_or_else(|| short_thread_id(&summary.id));
    let project = session_project_value(&summary.cwd);
    let lifecycle = session_lifecycle_value(state, &summary.id)?;
    Ok(json!({
        "id": summary.id,
        "cwd": summary.cwd,
        "project": project,
        "model": summary.model,
        "provider": summary.provider,
        "startedAtMs": summary.started_at_ms,
        "updatedAtMs": summary.updated_at_ms,
        "endedAtMs": summary.ended_at_ms,
        "endReason": summary.end_reason,
        "archivedAtMs": summary.archived_at_ms,
        "messageCount": summary.message_count,
        "toolCallCount": summary.tool_call_count,
        "visibleEntryCount": entries.len(),
        "activity": activity,
        "title": summary.title,
        "displayTitle": display_title,
        "preview": preview,
        "lifecycle": lifecycle,
    }))
}

fn session_lifecycle_value(state: &WebState, thread_id: &str) -> psychevo_runtime::Result<Value> {
    let binding = state
        .inner
        .state
        .store()
        .gateway_runtime_binding(thread_id)?;
    if binding
        .as_ref()
        .is_none_or(|binding| binding.backend_kind.as_deref() != Some("acp"))
    {
        return Ok(json!({
            "targetLabel": "Psychevo (Native)",
            "actions": [
                {
                    "id": "fork",
                    "enabled": false,
                    "unavailableReason": "This Agent does not expose session fork."
                },
                {"id": "delete", "enabled": true}
            ]
        }));
    }
    let metadata = state.inner.state.store().session_metadata(thread_id)?;
    let lifecycle = metadata
        .as_ref()
        .and_then(|metadata| metadata.get("agentSessionLifecycle"));
    let session_projection = metadata
        .as_ref()
        .and_then(|metadata| metadata.get(ACP_PEER_METADATA_KEY))
        .and_then(|peer| peer.get("sessionProjection"));
    let pending_delete = metadata
        .as_ref()
        .and_then(|metadata| metadata.get("agentSessionDeleteIntent"))
        .is_some();
    let target_label = lifecycle
        .and_then(|value| value.get("targetLabel"))
        .and_then(Value::as_str)
        .or_else(|| {
            session_projection
                .and_then(|projection| projection.get("agent"))
                .and_then(|agent| agent.get("title").or_else(|| agent.get("name")))
                .and_then(Value::as_str)
        })
        .or_else(|| binding.as_ref().and_then(|binding| binding.runtime_ref.as_deref()));
    let fork = lifecycle
        .and_then(|value| value.get("fork"))
        .and_then(Value::as_bool)
        .or_else(|| {
            session_projection
                .and_then(|projection| projection.pointer("/capabilities/session/fork"))
                .and_then(Value::as_bool)
        })
        .unwrap_or(false);
    let delete = lifecycle
        .and_then(|value| value.get("delete"))
        .and_then(Value::as_bool)
        .or_else(|| {
            session_projection
                .and_then(|projection| projection.pointer("/capabilities/session/delete"))
                .and_then(Value::as_bool)
        })
        .unwrap_or(false)
        && !pending_delete;
    Ok(json!({
        "targetLabel": target_label,
        "actions": [
            {
                "id": "fork",
                "enabled": fork,
                "unavailableReason": (!fork).then_some(
                    "This ACP Agent did not advertise session fork."
                )
            },
            {
                "id": "delete",
                "enabled": delete,
                "unavailableReason": (!delete).then_some(if pending_delete {
                    "Remote deletion is pending reconciliation."
                } else {
                    "This ACP Agent did not advertise persistent session deletion."
                })
            }
        ]
    }))
}

fn session_project_value(cwd: &str) -> Value {
    let path = PathBuf::from(cwd);
    json!({
        "cwd": cwd,
        "label": project_label(&path),
        "displayPath": display_cwd(&path),
    })
}

fn project_label(cwd: &Path) -> String {
    cwd
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("cwd")
        .to_string()
}

fn session_preview(entries: &[TranscriptEntry]) -> Option<String> {
    entries
        .iter()
        .find(|entry| entry.role == TranscriptEntryRole::User)
        .and_then(entry_preview)
        .or_else(|| entries.iter().find_map(entry_preview))
}

fn entry_preview(entry: &TranscriptEntry) -> Option<String> {
    entry
        .blocks
        .iter()
        .filter_map(|block| block.preview.as_deref().or(block.body.as_deref()))
        .map(compact_display_text)
        .find(|text| !text.is_empty())
}

fn compact_display_text(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_CHARS: usize = 120;
    if collapsed.chars().count() <= MAX_CHARS {
        return collapsed;
    }
    let mut out = collapsed.chars().take(MAX_CHARS - 1).collect::<String>();
    out.push('…');
    out
}

fn short_thread_id(id: &str) -> String {
    id.chars().take(8).collect()
}

fn gateway_turn_result_value(result: GatewayTurnResult) -> Value {
    json!({
        "thread": result.thread,
        "turn": result.turn,
        "result": {
            "sessionId": result.result.session_id,
            "outcome": result.result.outcome.as_str(),
            "finalAnswer": result.result.final_answer,
            "toolFailures": result.result.tool_failures,
            "provider": result.result.provider,
            "model": result.result.model,
        },
        "committedEntries": result.committed_entries,
    })
}

fn gateway_shell_result_value(result: GatewayShellResult) -> Value {
    json!({
        "thread": result.thread,
        "command": result.result.command,
        "outcome": result.result.outcome.as_str(),
        "toolFailures": result.result.tool_failures,
        "committedEntries": result.committed_entries,
    })
}
