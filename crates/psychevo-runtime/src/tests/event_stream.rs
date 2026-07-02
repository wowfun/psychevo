#[allow(unused_imports)]
pub(crate) use super::*;

#[test]
pub(crate) fn run_start_projects_to_session_configured_bootstrap_event() {
    let event = RunStreamEvent::value(json!({
        "type": "run_start",
        "session_id": "session-1",
        "thread_id": "session-1",
        "cwd": "/tmp/project",
        "root": "/tmp/project",
        "provider": "fake-provider",
        "model": "fake-model",
        "permission_profile": {
            "mode": "default",
            "approval_mode": "manual",
        },
        "resume_seed": {
            "requested_session_id": null,
            "continue_latest": false,
            "resolved_session_id": "session-1",
            "created_session": true,
            "source": "startup",
        },
        "selected_capability_roots": [
            {
                "id": "local-tools",
                "location": {
                    "local": {
                        "path": "tools"
                    }
                }
            }
        ],
    }));

    let RunStreamEvent::Event(event) = event else {
        panic!("expected session event");
    };
    assert_eq!(event.kind(), "run_start");
    assert!(matches!(
        event.payload,
        crate::types::SessionEventPayload::SessionConfigured { .. }
    ));
    assert!(event.sequence.is_some());
    assert_eq!(event.session_id.as_deref(), Some("session-1"));
    assert_eq!(event.thread_id.as_deref(), Some("session-1"));
    assert_eq!(event.as_value()["permission_profile"]["mode"], "default");
    assert_eq!(event.as_value()["resume_seed"]["source"], "startup");
    assert_eq!(
        event.as_value()["selected_capability_roots"][0]["id"],
        "local-tools"
    );
}

#[test]
pub(crate) fn blocking_action_events_project_to_typed_session_payloads() {
    let requested = RunStreamEvent::value(json!({
        "type": "action_requested",
        "action_id": "clarify-1",
        "kind": "clarify",
        "payload": {
            "call_id": "clarify-1",
            "questions": []
        }
    }));
    let RunStreamEvent::Event(requested) = requested else {
        panic!("expected session event");
    };
    assert!(matches!(
        requested.payload,
        crate::types::SessionEventPayload::BlockingActionRequested {
            ref action_id,
            kind: crate::types::BlockingActionKind::Clarify,
            ..
        } if action_id == "clarify-1"
    ));

    let resolved = RunStreamEvent::value(json!({
        "type": "action_resolved",
        "action_id": "clarify-1",
        "kind": "clarify",
        "reason": "answered"
    }));
    let RunStreamEvent::Event(resolved) = resolved else {
        panic!("expected session event");
    };
    assert!(matches!(
        resolved.payload,
        crate::types::SessionEventPayload::BlockingActionResolved {
            ref action_id,
            kind: crate::types::BlockingActionKind::Clarify,
            ref reason,
        } if action_id == "clarify-1" && reason == "answered"
    ));
}
