use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

include!("protocol/source.rs");
include!("protocol/events_transcript.rs");
include!("protocol/thread_command_turn.rs");
include!("protocol/automations.rs");
include!("protocol/channels.rs");
include!("protocol/voice.rs");
include!("protocol/settings_workspace_context.rs");
include!("protocol/agents_backend_rpc.rs");
include!("protocol/codegen.rs");

#[cfg(test)]
mod thread_application_contract_tests {
    use super::*;

    #[test]
    fn thread_context_and_control_requests_are_closed_typed_methods() {
        let context: ClientRequest = serde_json::from_value(serde_json::json!({
            "method": "thread/context/read",
            "params": {
                "threadId": "thread-1",
                "target": {
                    "agentRef": "reviewer",
                    "runtimeProfileRef": "codex"
                },
                "scope": {
                    "cwd": "/tmp/workspace",
                    "source": { "kind": "web", "rawId": "thread-test" }
                }
            }
        }))
        .expect("Thread Context request");
        assert!(matches!(
            context,
            ClientRequest::ThreadContextRead(ThreadContextReadParams {
                thread_id: Some(ref thread_id),
                target: Some(RunnableTargetInput {
                    agent_ref: Some(ref agent_ref),
                    ref runtime_profile_ref,
                }),
                ..
            }) if thread_id == "thread-1" && agent_ref == "reviewer" && runtime_profile_ref == "codex"
        ));

        let opaque_revision = "5db92a55f2f24d87";
        let control: ClientRequest = serde_json::from_value(serde_json::json!({
            "method": "thread/control/set",
            "params": {
                "threadId": "thread-1",
                "targetId": "target:opaque-reviewer-codex",
                "controlId": "mode",
                "value": "review",
                "expectedCapabilityRevision": opaque_revision,
                "expectedBindingRevision": 7,
                "expectedContextRevision": "11",
                "expectedControlRevision": "13"
            }
        }))
        .expect("Thread control request");
        assert!(matches!(
            control,
            ClientRequest::ThreadControlSet(ThreadControlSetParams {
                thread_id: Some(ref thread_id),
                target_id,
                expected_capability_revision,
                expected_binding_revision: 7,
                expected_context_revision,
                expected_control_revision,
                ..
            }) if thread_id == "thread-1"
                && target_id == "target:opaque-reviewer-codex"
                && expected_capability_revision == opaque_revision
                && expected_context_revision == "11"
                && expected_control_revision == "13"
        ));

        let numeric_revision = serde_json::from_value::<ClientRequest>(serde_json::json!({
            "method": "thread/control/set",
            "params": {
                "targetId": "target:opaque-reviewer-codex",
                "controlId": "mode",
                "value": "review",
                "expectedCapabilityRevision": 9007199254740993_u64,
                "expectedBindingRevision": 7,
                "expectedContextRevision": "11",
                "expectedControlRevision": "13"
            }
        }));
        assert!(
            numeric_revision.is_err(),
            "JSON numbers must not enter the opaque-string capability revision path"
        );
    }

    #[test]
    fn action_interaction_and_history_requests_are_closed_typed_methods() {
        let action: ClientRequest = serde_json::from_value(serde_json::json!({
            "method": "thread/action/run",
            "params": {
                "scope": {
                    "cwd": "/tmp/workspace",
                    "source": { "kind": "web", "rawId": "thread-test" }
                },
                "threadId": "thread-1",
                "action": {
                    "kind": "steer",
                    "expectedTurnId": "turn-1",
                    "text": "Use the smaller patch."
                }
            }
        }))
        .expect("typed Thread action");
        assert!(matches!(
            action,
            ClientRequest::ThreadActionRun(ThreadActionRunParams {
                action: ThreadActionInput::Steer { ref expected_turn_id, ref text },
                ..
            }) if expected_turn_id == "turn-1" && text == "Use the smaller patch."
        ));

        let interaction: ClientRequest = serde_json::from_value(serde_json::json!({
            "method": "thread/interaction/respond",
            "params": {
                "scope": {
                    "cwd": "/tmp/workspace",
                    "source": { "kind": "web", "rawId": "thread-test" }
                },
                "threadId": "thread-1",
                "interactionId": "permission-1",
                "response": { "kind": "permission", "decision": "allowOnce" }
            }
        }))
        .expect("typed Thread interaction response");
        assert!(matches!(
            interaction,
            ClientRequest::ThreadInteractionRespond(ThreadInteractionRespondParams {
                response: ThreadInteractionResponse::Permission {
                    decision: PermissionDecision::AllowOnce
                },
                ..
            })
        ));

        let history: ClientRequest = serde_json::from_value(serde_json::json!({
            "method": "thread/history/read",
            "params": {
                "scope": {
                    "cwd": "/tmp/workspace",
                    "source": { "kind": "web", "rawId": "thread-test" }
                },
                "threadId": "thread-1",
                "cursor": "message:7",
                "limit": 25
            }
        }))
        .expect("typed Thread history read");
        assert!(matches!(
            history,
            ClientRequest::ThreadHistoryRead(ThreadHistoryReadParams {
                ref thread_id,
                ref cursor,
                limit: Some(25),
                ..
            }) if thread_id == "thread-1" && cursor.as_deref() == Some("message:7")
        ));

        let editable: ClientRequest = serde_json::from_value(serde_json::json!({
            "method": "thread/history/draft/read",
            "params": {
                "scope": {
                    "cwd": "/tmp/workspace",
                    "source": { "kind": "web", "rawId": "thread-test" }
                },
                "threadId": "thread-1",
                "messageId": "message:7"
            }
        }))
        .expect("typed editable draft read");
        assert!(matches!(
            editable,
            ClientRequest::ThreadHistoryDraftRead(ThreadHistoryDraftReadParams {
                ref thread_id,
                ref message_id,
                ..
            }) if thread_id == "thread-1" && message_id == "message:7"
        ));

        let open_command = serde_json::from_value::<ClientRequest>(serde_json::json!({
            "method": "thread/action/run",
            "params": {
                "scope": {
                    "cwd": "/tmp/workspace",
                    "source": { "kind": "web", "rawId": "thread-test" }
                },
                "threadId": "thread-1",
                "action": { "kind": "adapterCommand", "operation": "anything" }
            }
        }));
        assert!(
            open_command.is_err(),
            "open adapter commands must fail closed"
        );
    }

    #[test]
    fn turn_errors_preserve_unknown_delivery_as_structured_data() {
        let notification = ServerNotification::TurnError(TurnErrorPayload {
            error: AgentErrorView {
                message: "The ACP connection closed after dispatch.".to_string(),
                code: Some("unknown_delivery".to_string()),
                stage: Some("delivery".to_string()),
                retry_class: Some("reconcile".to_string()),
                delivery: AgentDeliveryStatusView::Unknown,
                recovery_action: Some("thread/history/read".to_string()),
                diagnostic_ref: Some("turn:7".to_string()),
            },
            thread_id: Some("thread-1".to_string()),
            turn_id: Some("turn-7".to_string()),
        });
        let value = serde_json::to_value(notification).expect("turn/error notification");
        assert_eq!(value["params"]["error"]["delivery"], "unknown");
        assert_eq!(
            value["params"]["error"]["recoveryAction"],
            "thread/history/read"
        );
        assert!(value["params"]["error"].get("message").is_some());
        let decoded = serde_json::from_value::<ServerNotification>(value)
            .expect("structured turn/error remains decodable");
        assert!(matches!(
            decoded,
            ServerNotification::TurnError(TurnErrorPayload {
                error: AgentErrorView {
                    delivery: AgentDeliveryStatusView::Unknown,
                    ..
                },
                ..
            })
        ));
    }

    #[test]
    fn retired_runtime_application_methods_are_not_client_requests() {
        for method in [
            "runtime/options",
            "runtime/context/read",
            "runtime/control/set",
            "runtime/auth/action",
            "runtime/goal/read",
            "runtime/goal/set",
            "runtime/goal/clear",
            "runtime/account/rateLimits/read",
            "runtime/snapshot",
            "runtime/health/check",
            "runtime/session/list",
            "runtime/session/read",
            "runtime/session/attach",
            "runtime/session/resume",
            "runtime/session/archive",
            "runtime/session/unarchive",
            "runtime/session/delete",
            "runtime/session/rename",
            "runtime/session/fork",
            "runtime/session/revert",
            "runtime/session/unrevert",
            "permission/respond",
            "clarify/respond",
        ] {
            let request = serde_json::from_value::<ClientRequest>(serde_json::json!({
                "method": method,
                "params": {}
            }));
            assert!(
                request.is_err(),
                "{method} must be absent from ClientRequest"
            );
        }
    }

    #[test]
    fn agent_session_import_and_fork_are_closed_typed_methods() {
        let list: ClientRequest = serde_json::from_value(serde_json::json!({
            "method": "thread/import/list",
            "params": {
                "scope": {
                    "cwd": "/tmp/workspace",
                    "source": { "kind": "web", "rawId": "import-test" }
                },
                "cursors": { "opencode": "cursor:opaque" }
            }
        }))
        .expect("typed Agent session import list");
        assert!(matches!(
            list,
            ClientRequest::ThreadImportList(ThreadImportListParams { ref cursors, .. })
                if cursors.get("opencode").map(String::as_str) == Some("cursor:opaque")
        ));

        let import: ClientRequest = serde_json::from_value(serde_json::json!({
            "method": "thread/import",
            "params": {
                "scope": {
                    "cwd": "/tmp/workspace",
                    "source": { "kind": "web", "rawId": "import-test" }
                },
                "candidateId": "candidate:opaque",
                "targetId": "target:opaque"
            }
        }))
        .expect("typed Agent session import");
        assert!(matches!(
            import,
            ClientRequest::ThreadImport(ThreadImportParams {
                ref candidate_id,
                ref target_id,
                ..
            }) if candidate_id == "candidate:opaque" && target_id == "target:opaque"
        ));

        let fork: ThreadActionInput =
            serde_json::from_value(serde_json::json!({"kind": "fork"})).expect("typed fork action");
        assert_eq!(fork.kind(), ThreadActionKind::Fork);
        let value = serde_json::to_value(fork).expect("serialize typed fork action");
        assert!(value.get("nativeSessionId").is_none());

        let edit: ThreadActionInput = serde_json::from_value(serde_json::json!({
            "kind": "revertConversation",
            "messageId": "message:7",
            "draft": {
                "parts": [
                    {"type": "text", "text": "updated"},
                    {"type": "image", "input": {"kind": "url", "url": "data:image/png;base64,AA=="}}
                ]
            }
        }))
        .expect("typed conversation edit action");
        assert_eq!(edit.kind(), ThreadActionKind::RevertConversation);
    }

    #[test]
    fn profile_and_thread_control_revisions_remain_decimal_strings() {
        let above_js_safe = "9007199254740993";
        let profile: RuntimeProfileView = serde_json::from_value(serde_json::json!({
            "id": "codex",
            "runtime": "acp",
            "enabled": true,
            "label": "Codex",
            "generated": true,
            "backendRef": "codex",
            "profileRevision": "18446744073709551615",
            "capabilityRevision": above_js_safe,
            "health": { "status": "unchecked", "summary": "Configured" }
        }))
        .expect("large decimal-string Profile revisions");
        let value = serde_json::to_value(profile).expect("serialize Profile revisions");
        assert_eq!(value["profileRevision"], "18446744073709551615");
        assert_eq!(value["capabilityRevision"], above_js_safe);
        assert!(value["capabilityRevision"].is_string());
    }

    #[test]
    fn backend_kind_and_structured_error_are_implementation_neutral() {
        assert_eq!(BackendKind::Native.as_str(), "native");
        assert_eq!(BackendKind::Acp.as_str(), "acp");
        let error = RuntimeErrorView {
            code: "process_exit".to_string(),
            stage: "transport".to_string(),
            retry_class: RuntimeRetryClassView::Reconnect,
            message: "The Agent session stopped.".to_string(),
            diagnostic_ref: Some("agent-session:7".to_string()),
        };
        let value = serde_json::to_value(error).expect("structured error");
        assert_eq!(value["retryClass"], "reconnect");
        assert_eq!(value["diagnosticRef"], "agent-session:7");
    }
}
