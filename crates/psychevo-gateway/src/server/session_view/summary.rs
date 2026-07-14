fn session_summary_value(
    projection: SessionListProjection,
    activity: GatewayActivity,
) -> Value {
    let lifecycle = session_lifecycle_value(&projection);
    let summary = projection.summary;
    let display_title = summary
        .title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .or_else(|| projection.first_user_text.as_deref().map(compact_display_text))
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| short_thread_id(&summary.id));
    let project = session_project_value(&summary.cwd);
    let forked_from_thread_id = projection
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("forkedFromThreadId"))
        .and_then(Value::as_str);
    json!({
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
        "activity": activity,
        "title": summary.title,
        "displayTitle": display_title,
        "lifecycle": lifecycle,
        "forkedFromThreadId": forked_from_thread_id,
    })
}

fn session_lifecycle_value(projection: &SessionListProjection) -> Value {
    if projection.runtime_backend_kind.as_deref() == Some("native") {
        let staged = projection
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.get("revert").is_some());
        let side = projection
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get(psychevo_runtime::SIDE_CONVERSATION_METADATA_KEY))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let eligible = projection.summary.parent_session_id.is_none()
            && matches!(projection.summary.source.as_str(), "web" | "tui")
            && !side
            && !staged;
        let unavailable_reason = (!eligible).then_some({
            if staged {
                "Run, restore, or redo the staged history state before forking."
            } else {
                "Only root Workbench and TUI Native Threads can be forked."
            }
        });
        return json!({
            "targetLabel": "Psychevo (Native)",
            "actions": [
                {
                    "id": "fork",
                    "enabled": eligible,
                    "unavailableReason": unavailable_reason
                },
                {"id": "delete", "enabled": true}
            ]
        });
    }
    if projection.runtime_backend_kind.as_deref() != Some("acp") {
        return json!({
            "targetLabel": "Psychevo",
            "actions": [
                {
                    "id": "fork",
                    "enabled": false,
                    "unavailableReason": "Fork requires a resolved Native or ACP binding."
                },
                {"id": "delete", "enabled": true}
            ]
        });
    }
    let lifecycle = projection
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("agentSessionLifecycle"));
    let session_projection = projection
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(ACP_PEER_METADATA_KEY))
        .and_then(|peer| peer.get("sessionProjection"));
    let pending_delete = projection
        .metadata
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
        .or(projection.runtime_ref.as_deref());
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
    json!({
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
    })
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
