use std::path::{Path, PathBuf};

use psychevo::{
    HumanThreadSummary, ThreadLifecycleActionPresentation, ThreadLifecyclePresentation,
    ThreadPresentationBackend,
};
use serde_json::{Value, json};

use crate::gateway::activity::GatewayActivity;
use crate::gateway::results::{GatewayShellResult, shell_outcome_wire_value};

use super::super::settings_observability::display_cwd;

pub(in super::super) fn session_summary_value(
    presentation: HumanThreadSummary,
    activity: GatewayActivity,
) -> Value {
    let lifecycle = session_lifecycle_value(presentation.backend, presentation.lifecycle);
    let summary = presentation.summary;
    let project = session_project_value(&summary.cwd);
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
        "displayTitle": presentation.display_title,
        "lifecycle": lifecycle,
        "forkedFromThreadId": summary.forked_from_thread_id,
    })
}

fn session_lifecycle_value(
    backend: ThreadPresentationBackend,
    lifecycle: ThreadLifecyclePresentation,
) -> Value {
    json!({
        "targetLabel": lifecycle.target_label,
        "actions": [
            session_lifecycle_action_value("fork", lifecycle.fork, true),
            session_lifecycle_action_value(
                "delete",
                lifecycle.delete,
                backend == ThreadPresentationBackend::Acp,
            ),
        ]
    })
}

fn session_lifecycle_action_value(
    id: &'static str,
    action: ThreadLifecycleActionPresentation,
    include_unavailable_reason: bool,
) -> Value {
    if include_unavailable_reason || action.unavailable_reason.is_some() {
        json!({
            "id": id,
            "enabled": action.enabled,
            "unavailableReason": action.unavailable_reason,
        })
    } else {
        json!({
            "id": id,
            "enabled": action.enabled,
        })
    }
}

pub(super) fn session_project_value(cwd: &str) -> Value {
    let path = PathBuf::from(cwd);
    json!({
        "cwd": cwd,
        "label": project_label(&path),
        "displayPath": display_cwd(&path),
    })
}

fn project_label(cwd: &Path) -> String {
    cwd.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("cwd")
        .to_string()
}

pub(in super::super) fn gateway_shell_result_value(result: GatewayShellResult) -> Value {
    json!({
        "thread": result.thread,
        "command": result.result.command,
        "outcome": shell_outcome_wire_value(result.result.outcome),
        "toolFailures": result.result.tool_failures,
        "committedEntries": result.committed_entries,
    })
}

#[cfg(test)]
mod shell_result_tests {
    use psychevo::{
        ShellCommandOutcome, ThreadLifecycleActionPresentation, ThreadLifecyclePresentation,
        ThreadPresentationBackend,
    };
    use serde_json::Value;

    use crate::gateway::results::shell_outcome_wire_value;

    use super::session_lifecycle_value;

    #[test]
    fn typed_shell_outcomes_preserve_the_existing_wire_values() {
        assert_eq!(
            shell_outcome_wire_value(ShellCommandOutcome::Completed),
            "normal"
        );
        assert_eq!(
            shell_outcome_wire_value(ShellCommandOutcome::Failed),
            "failed"
        );
        assert_eq!(
            shell_outcome_wire_value(ShellCommandOutcome::Interrupted),
            "aborted"
        );
    }

    #[test]
    fn typed_lifecycle_keeps_the_existing_action_wire_shape() {
        let action = || ThreadLifecycleActionPresentation {
            enabled: true,
            unavailable_reason: None,
        };
        let native = session_lifecycle_value(
            ThreadPresentationBackend::Native,
            ThreadLifecyclePresentation {
                target_label: Some("Psychevo (Native)".to_string()),
                fork: action(),
                delete: action(),
            },
        );
        assert_eq!(native["actions"][0]["unavailableReason"], Value::Null);
        assert!(native["actions"][1].get("unavailableReason").is_none());

        let acp = session_lifecycle_value(
            ThreadPresentationBackend::Acp,
            ThreadLifecyclePresentation {
                target_label: Some("Agent".to_string()),
                fork: action(),
                delete: action(),
            },
        );
        assert_eq!(acp["actions"][1]["unavailableReason"], Value::Null);
    }
}
