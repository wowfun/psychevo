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
mod runtime_contract_tests {
    use super::*;

    #[test]
    fn runtime_context_and_revision_methods_are_typed() {
        let context: ClientRequest = serde_json::from_value(serde_json::json!({
            "method": "runtime/context/read",
            "params": {
                "threadId": "thread-1",
                "scope": {
                    "cwd": "/tmp/workspace",
                    "source": { "kind": "web", "rawId": "runtime-test" }
                }
            }
        }))
        .expect("runtime context request");
        assert!(matches!(
            context,
            ClientRequest::RuntimeContextRead(RuntimeContextReadParams {
                thread_id: Some(ref thread_id),
                ..
            }) if thread_id == "thread-1"
        ));

        let read: ClientRequest = serde_json::from_value(serde_json::json!({
            "method": "runtime/session/read",
            "params": {
                "runtimeRef": "opencode",
                "sessionHandle": "rts_opaque_1",
                "cursor": "rtc_opaque_page_2"
            }
        }))
        .expect("runtime paginated read request");
        assert!(matches!(
            read,
            ClientRequest::RuntimeSessionRead(RuntimeSessionReadParams {
                cursor: Some(ref cursor),
                ..
            }) if cursor == "rtc_opaque_page_2"
        ));
        let raw_read_id = serde_json::from_value::<ClientRequest>(serde_json::json!({
            "method": "runtime/session/read",
            "params": {
                "runtimeRef": "opencode",
                "sessionHandle": "rts_opaque_1",
                "messageID": "msg_native_secret"
            }
        }));
        assert!(raw_read_id.is_err(), "raw native read ids must be rejected");

        let attach: ClientRequest = serde_json::from_value(serde_json::json!({
            "method": "runtime/session/attach",
            "params": {
                "runtimeRef": "codex",
                "sessionHandle": "rts_active_opaque"
            }
        }))
        .expect("runtime read-only attach request");
        assert!(matches!(
            attach,
            ClientRequest::RuntimeSessionAttach(RuntimeSessionParams {
                native_session_id: ref session_handle,
                ..
            }) if session_handle == "rts_active_opaque"
        ));
        let raw_attach_id = serde_json::from_value::<ClientRequest>(serde_json::json!({
            "method": "runtime/session/attach",
            "params": {
                "runtimeRef": "codex",
                "sessionHandle": "rts_active_opaque",
                "nativeSessionId": "native-secret"
            }
        }));
        assert!(
            raw_attach_id.is_err(),
            "raw native attach ids must be rejected"
        );

        let revert: ClientRequest = serde_json::from_value(serde_json::json!({
            "method": "runtime/session/revert",
            "params": {
                "runtimeRef": "opencode",
                "sessionHandle": "rts_opaque_1",
                "revisionHandle": "rtr_opaque_1"
            }
        }))
        .expect("runtime revert request");
        assert!(matches!(
            revert,
            ClientRequest::RuntimeSessionRevert(RuntimeSessionRevisionParams {
                runtime_ref,
                native_session_id,
                revision_handle: Some(revision_handle),
                ..
            }) if runtime_ref == "opencode"
                && native_session_id == "rts_opaque_1"
                && revision_handle == "rtr_opaque_1"
        ));

        let raw_native_id = serde_json::from_value::<ClientRequest>(serde_json::json!({
            "method": "runtime/session/revert",
            "params": {
                "runtimeRef": "opencode",
                "sessionHandle": "rts_opaque_1",
                "itemId": "msg_native_secret"
            }
        }));
        assert!(
            raw_native_id.is_err(),
            "raw native item ids must be rejected"
        );
    }

    #[test]
    fn direct_runtime_backend_and_structured_error_serialize_without_native_guessing() {
        assert_eq!(BackendKind::Runtime.as_str(), "runtime");
        let error = RuntimeErrorView {
            code: "process_exit".to_string(),
            stage: "transport".to_string(),
            retry_class: RuntimeRetryClassView::Reconnect,
            message: "The Codex runtime stopped.".to_string(),
            diagnostic_ref: Some("runtime:codex:7".to_string()),
        };
        let value = serde_json::to_value(error).expect("runtime error");
        assert_eq!(value["retryClass"], "reconnect");
        assert_eq!(value["diagnosticRef"], "runtime:codex:7");
    }

    #[test]
    fn codex_goal_and_rate_limit_methods_are_closed_typed_requests() {
        let set: ClientRequest = serde_json::from_value(serde_json::json!({
            "method": "runtime/goal/set",
            "params": {
                "threadId": "thread-1",
                "objective": "Ship evidence",
                "status": "active",
                "tokenBudget": 12000,
                "clearTokenBudget": false
            }
        }))
        .expect("typed goal set request");
        assert!(matches!(
            set,
            ClientRequest::RuntimeGoalSet(RuntimeGoalSetParams {
                thread_id: Some(ref thread_id),
                status: Some(RuntimeGoalStatusView::Active),
                token_budget: Some(12_000),
                ..
            }) if thread_id == "thread-1"
        ));

        let unknown_status = serde_json::from_value::<ClientRequest>(serde_json::json!({
            "method": "runtime/goal/set",
            "params": { "threadId": "thread-1", "status": "native_future_status" }
        }));
        assert!(unknown_status.is_err(), "goal statuses are a closed enum");

        let adapter_shaped_goal = serde_json::from_value::<ClientRequest>(serde_json::json!({
            "method": "runtime/goal/read",
            "params": {
                "threadId": "thread-1",
                "runtimeRef": "codex",
                "nativeSessionId": "native-secret"
            }
        }));
        assert!(
            adapter_shaped_goal.is_err(),
            "goal requests must derive runtime and native identity from binding"
        );

        let rate_limits: ClientRequest = serde_json::from_value(serde_json::json!({
            "method": "runtime/account/rateLimits/read",
            "params": { "runtimeRef": "codex", "threadId": null }
        }))
        .expect("typed account rate-limit request");
        assert!(matches!(
            rate_limits,
            ClientRequest::RuntimeAccountRateLimitsRead(RuntimeAccountRateLimitsReadParams {
                runtime_ref: Some(ref runtime_ref),
                thread_id: None,
                ..
            }) if runtime_ref == "codex"
        ));
    }

    #[test]
    fn runtime_profile_and_capability_revisions_round_trip_above_js_safe_integer() {
        let above_js_safe = "9007199254740993";
        let profile: RuntimeProfileView = serde_json::from_value(serde_json::json!({
            "id": "codex",
            "runtime": "codex",
            "enabled": true,
            "label": "Codex",
            "generated": true,
            "profileRevision": "18446744073709551615",
            "capabilityRevision": above_js_safe,
            "health": { "status": "ready", "summary": "Ready" }
        }))
        .expect("large decimal-string Profile revisions");
        let value = serde_json::to_value(profile).expect("serialize Profile revisions");
        assert_eq!(value["profileRevision"], "18446744073709551615");
        assert_eq!(value["capabilityRevision"], above_js_safe);
        assert!(value["capabilityRevision"].is_string());

        let control: RuntimeControlSetParams = serde_json::from_value(serde_json::json!({
            "runtimeRef": "codex",
            "controlId": "mode",
            "value": "review",
            "expectedCapabilityRevision": above_js_safe,
            "expectedBindingRevision": 7
        }))
        .expect("large expected capability revision");
        assert_eq!(control.expected_capability_revision, above_js_safe);
        assert_eq!(control.expected_binding_revision, 7);

        let numeric = serde_json::from_value::<RuntimeControlSetParams>(serde_json::json!({
            "runtimeRef": "codex",
            "controlId": "mode",
            "value": "review",
            "expectedCapabilityRevision": 9007199254740993_u64,
            "expectedBindingRevision": 7
        }));
        assert!(
            numeric.is_err(),
            "JSON numbers must not enter the u64 revision path"
        );
    }
}
