use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use crate::session_trace::{
    SessionTraceReadOptions, SessionTraceReadResult, read_session_trace, remove_session_trace_dir,
};

pub(crate) const SQLITE_SCHEMA_VERSION: i64 = 34;
pub(crate) const MIN_SUPPORTED_SQLITE_SCHEMA_VERSION: i64 = 33;
pub(crate) const SESSION_REVERT_METADATA_KEY: &str = "revert";
pub(crate) const MESSAGE_UNDO_METADATA_KEY: &str = "undo";
pub(crate) const MESSAGE_PRE_SNAPSHOT_KEY: &str = "pre_snapshot";

fn invalid_persisted_domain_value(table: &str, field: &str, value: &str) -> crate::Error {
    crate::Error::structured(
        "Persisted domain value is invalid.",
        serde_json::json!({
            "kind": "invalid_persisted_domain_value",
            "table": table,
            "field": field,
            "value": value,
        }),
    )
}

// Domain records live with explicit bounded-context ownership.
#[path = "store/records/undo.rs"]
mod undo_records;
pub use undo_records::{
    ConversationDraftPart, NativeSessionForkInput, SessionRevertKind, SessionRevertState,
    UndoTarget,
};
#[path = "store/records/context_evidence.rs"]
mod context_evidence_records;
pub use context_evidence_records::{ContextEvidenceInput, ContextEvidenceRecord};
#[path = "store/records/session_snapshots.rs"]
mod session_snapshots_records;
pub(crate) use session_snapshots_records::WorkspaceSessionSnapshotInput;
pub use session_snapshots_records::{
    ChildSessionRuntimeBindingSnapshotInput, ChildSessionSnapshotInput,
};
#[path = "store/records/gateway_bindings.rs"]
mod gateway_bindings_records;
pub use gateway_bindings_records::{
    GatewaySourceBindingInput, GatewaySourceBindingRecord, GatewaySourceLaneInput,
    GatewaySourceLaneRecord,
};
#[path = "store/records/turn_delivery.rs"]
mod turn_delivery_records;
pub(crate) use turn_delivery_records::{
    ExistingFrameworkThreadTurnInput, NewFrameworkThreadTurnInput,
};
pub use turn_delivery_records::{
    GatewayChannelOutboxInput, GatewayChannelOutboxRecord, GatewayChannelOutboxStatus,
    GatewayTurnDeliveryInput, GatewayTurnDeliveryRecord, GatewayTurnDeliveryStatus,
};
#[path = "store/records/runtime_bindings.rs"]
mod runtime_bindings_records;
pub(crate) use runtime_bindings_records::{
    AgentThreadImportCommit, AgentThreadImportCommitInput, AgentThreadImportMessageInput,
};
pub use runtime_bindings_records::{
    GatewayRuntimeBindingInput, GatewayRuntimeBindingOwnership, GatewayRuntimeBindingRecord,
    GatewayRuntimeBindingStatus, GatewayRuntimeControlStatePatch,
};
#[path = "store/records/gateway_activity.rs"]
mod gateway_activity_records;
pub(crate) use gateway_activity_records::GatewayTurnStartReceiptRecord;
pub use gateway_activity_records::{
    GatewayActivityClaimInput, GatewayActivityKind, GatewayActivityRecord, GatewayActivityState,
    GatewayActivityTerminalStatus,
};
#[path = "store/records/session_browser.rs"]
mod session_browser_records;
pub(crate) use session_browser_records::{
    SessionBrowserRequest, SessionBrowserWorkspaceProjection, SessionListCursor,
    SessionListProjection, SessionListProjectionPage, SessionSummaryPage,
};
#[path = "store/records/gateway_live.rs"]
mod gateway_live_records;
pub use gateway_live_records::{
    GatewayLiveEventCommit, GatewayLiveEventRecord, GatewayLiveSnapshotInput,
    GatewayLiveSnapshotPage, GatewayLiveSnapshotRecord,
};
#[path = "store/records/gateway_control.rs"]
mod gateway_control_records;
pub use gateway_control_records::{
    GatewayControlCommandInput, GatewayControlCommandKind, GatewayControlCommandRecord,
    GatewayControlCommandStatus,
};
#[path = "store/records/turn_terminal.rs"]
mod turn_terminal_records;
pub use turn_terminal_records::{GatewayTurnTerminalInput, GatewayTurnTerminalRecord};
#[path = "store/records/framework_interactions.rs"]
mod framework_interactions_records;
pub(crate) use framework_interactions_records::{
    FrameworkInteractionRecord, FrameworkInteractionStatus,
};
#[path = "store/records/automations.rs"]
mod automations_records;
pub use automations_records::{
    AutomationRunFinishInput, AutomationRunRecord, AutomationRunRecoveryCandidate,
    AutomationRunStatus, AutomationRunTerminalStatus, AutomationTaskInput, AutomationTaskKind,
    AutomationTaskRecord,
};
#[path = "store/records/prompt_prefix.rs"]
mod prompt_prefix_records;
pub use prompt_prefix_records::{PromptPrefixRecord, PromptPrefixSlotRecord};
#[path = "store/records/agent_mailbox.rs"]
mod agent_mailbox_records;
pub use agent_mailbox_records::{AgentMailboxEventInput, AgentMailboxEventRecord};
#[path = "store/records/compactions.rs"]
mod compactions_records;
pub use compactions_records::{SessionCompactionInput, SessionCompactionRecord};
#[path = "store/records/messages.rs"]
mod messages_records;
pub use messages_records::SessionMessageRecord;

#[derive(Clone)]
pub struct StateRuntime {
    pub(crate) inner: Arc<StateRuntimeInner>,
}

pub(crate) struct StateRuntimeInner {
    pub(crate) db_path: PathBuf,
    pub(crate) pool: sqlx::SqlitePool,
    pub(crate) connection_limit: u32,
    pub(crate) in_flight_operations: AtomicU64,
    pub(crate) completed_operations: AtomicU64,
    pub(crate) failed_operations: AtomicU64,
    pub(crate) busy_operations: AtomicU64,
    pub(crate) acquire_latency_micros: AtomicU64,
    pub(crate) execute_latency_micros: AtomicU64,
    pub(crate) filesystem_grants: Mutex<HashMap<String, crate::sandbox::SandboxWriteGrants>>,
    #[cfg(test)]
    pub(crate) fail_next_framework_terminal: AtomicU64,
    #[cfg(test)]
    pub(crate) fail_next_agent_terminal: AtomicU64,
    #[cfg(test)]
    pub(crate) fail_next_agent_thread_import_commit: AtomicU64,
    #[cfg(test)]
    pub(crate) gateway_turn_acceptance_barrier:
        Mutex<Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>>,
    #[cfg(test)]
    pub(crate) native_history_fork_barrier:
        Mutex<Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>>,
    #[cfg(test)]
    pub(crate) state_close_barrier:
        Mutex<Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StateRuntimeDiagnostics {
    pub connection_limit: u32,
    pub pool_size: u32,
    pub pool_idle: usize,
    pub in_flight_operations: u64,
    pub completed_operations: u64,
    pub failed_operations: u64,
    pub busy_operations: u64,
    pub acquire_latency_micros: u64,
    pub execute_latency_micros: u64,
}

impl StateRuntime {
    pub fn db_path(&self) -> &Path {
        &self.inner.db_path
    }

    pub fn diagnostics(&self) -> StateRuntimeDiagnostics {
        let completed_operations = self
            .inner
            .completed_operations
            .load(std::sync::atomic::Ordering::Relaxed);
        let failed_operations = self
            .inner
            .failed_operations
            .load(std::sync::atomic::Ordering::Relaxed);
        StateRuntimeDiagnostics {
            connection_limit: self.inner.connection_limit,
            pool_size: self.inner.pool.size(),
            pool_idle: self.inner.pool.num_idle(),
            in_flight_operations: self
                .inner
                .in_flight_operations
                .load(std::sync::atomic::Ordering::Relaxed),
            completed_operations,
            failed_operations,
            busy_operations: self
                .inner
                .busy_operations
                .load(std::sync::atomic::Ordering::Relaxed),
            acquire_latency_micros: self
                .inner
                .acquire_latency_micros
                .load(std::sync::atomic::Ordering::Relaxed),
            execute_latency_micros: self
                .inner
                .execute_latency_micros
                .load(std::sync::atomic::Ordering::Relaxed),
        }
    }

    pub fn read_session_trace(
        &self,
        session_id: &str,
        options: SessionTraceReadOptions,
    ) -> SessionTraceReadResult {
        read_session_trace(self.db_path(), session_id, options)
    }

    pub(crate) fn filesystem_grants(&self, session_id: &str) -> crate::sandbox::SandboxWriteGrants {
        let mut grants = self
            .inner
            .filesystem_grants
            .lock()
            .expect("filesystem grant map poisoned");
        grants.entry(session_id.to_string()).or_default().clone()
    }

    pub(crate) fn filesystem_grants_with_turn_scopes(
        &self,
        session_id: &str,
        turn_owner_session_id: &str,
    ) -> crate::sandbox::SandboxWriteGrants {
        let local = self.filesystem_grants(session_id);
        let turn_owner = self.filesystem_grants(turn_owner_session_id);
        local.with_turn_scopes_from(&turn_owner)
    }

    pub(crate) fn turn_filesystem_grant_guard(
        &self,
        session_id: impl Into<String>,
    ) -> TurnFilesystemGrantGuard {
        TurnFilesystemGrantGuard {
            state: self.clone(),
            session_id: session_id.into(),
        }
    }

    fn clear_turn_filesystem_grants(&self, session_id: &str) {
        if let Ok(grants) = self.inner.filesystem_grants.lock()
            && let Some(grants) = grants.get(session_id)
        {
            grants.clear_turn_scopes();
        }
    }

    pub(crate) fn clear_session_filesystem_grants(&self, session_id: &str) {
        if let Ok(mut grants) = self.inner.filesystem_grants.lock()
            && let Some(grants) = grants.remove(session_id)
        {
            grants.clear_session_scopes();
        }
    }

    pub(crate) fn remove_session_trace(&self, session_id: &str) {
        let _ = remove_session_trace_dir(self.db_path(), session_id);
    }
}

pub(crate) const DEFAULT_STATE_CONNECTION_LIMIT: u32 = 5;

pub(crate) struct TurnFilesystemGrantGuard {
    state: StateRuntime,
    session_id: String,
}

impl Drop for TurnFilesystemGrantGuard {
    fn drop(&mut self) {
        self.state.clear_turn_filesystem_grants(&self.session_id);
    }
}

impl fmt::Debug for StateRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StateRuntime")
            .field("db_path", &self.inner.db_path)
            .finish_non_exhaustive()
    }
}

// Store internals are split by schema, session, message, undo, and row-helper concerns.
#[path = "store/agents.rs"]
pub(crate) mod store_agents;
#[path = "store/context_evidence.rs"]
pub(crate) mod store_context_evidence;
#[path = "store/history_fork.rs"]
pub(crate) mod store_history_fork;
#[path = "store/messages.rs"]
pub(crate) mod store_messages;
#[path = "store/prompt_prefix.rs"]
pub(crate) mod store_prompt_prefix;
#[path = "store/schema.rs"]
pub(crate) mod store_schema;
#[path = "store/sessions.rs"]
pub(crate) mod store_sessions;
#[path = "store/undo_state.rs"]
pub(crate) mod store_undo_state;
#[cfg(test)]
pub(crate) use store_agents::{
    AgentCoordinationRunStatus, AgentMissionRunInput, AgentTeamRunInput,
};
pub(crate) use store_agents::{
    AgentEdgeRecord, AgentEdgeStatus, AgentMissionRunRecord, AgentTeamRunRecord,
};
#[path = "store/agent_mailbox.rs"]
pub(crate) mod store_agent_mailbox;
#[path = "store/agent_thread_import.rs"]
pub(crate) mod store_agent_thread_import;
#[path = "store/automations.rs"]
pub(crate) mod store_automations;
#[path = "store/compactions.rs"]
pub(crate) mod store_compactions;
#[path = "store/framework_interactions.rs"]
pub(crate) mod store_framework_interactions;
#[path = "store/gateway_activity.rs"]
pub(crate) mod store_gateway_activity;
#[path = "store/gateway_bindings.rs"]
pub(crate) mod store_gateway_bindings;
#[path = "store/gateway_control.rs"]
pub(crate) mod store_gateway_control;
#[path = "store/gateway_live_state.rs"]
pub(crate) mod store_gateway_live_state;
#[path = "store/lifecycle.rs"]
pub(crate) mod store_lifecycle;
#[path = "store/message_fields.rs"]
pub(crate) mod store_message_fields;
#[path = "store/metadata.rs"]
pub(crate) mod store_metadata;
#[path = "store/runtime_bindings.rs"]
pub(crate) mod store_runtime_bindings;
#[path = "store/sqlx_runtime.rs"]
pub(crate) mod store_sqlx_runtime;
#[path = "store/turn_delivery.rs"]
pub(crate) mod store_turn_delivery;
#[path = "store/undo_helpers.rs"]
pub(crate) mod store_undo_helpers;
#[path = "store/workspaces.rs"]
pub(crate) mod store_workspaces;
pub(crate) use store_workspaces::{
    GatewayNavigationRecord, ThreadWorkspaceRecord, WorkspaceRecord,
};

#[cfg(test)]
mod state_runtime_tests {
    use super::*;
    use crate::types::{FilesystemApprovalLifetime, FilesystemApprovalScope};

    #[tokio::test]
    async fn filesystem_grants_follow_turn_and_session_lifecycles() {
        let temp = tempfile::tempdir().expect("temp");
        let turn_root = temp.path().join("turn");
        let session_root = temp.path().join("session");
        std::fs::create_dir_all(&turn_root).expect("turn root");
        std::fs::create_dir_all(&session_root).expect("session root");
        let state = StateRuntime::open(temp.path().join("state.db"))
            .await
            .expect("state");
        let grants = state.filesystem_grants("session-1");
        let turn_guard = state.turn_filesystem_grant_guard("session-1");
        grants
            .grant_scope(&FilesystemApprovalScope {
                directory: turn_root.display().to_string(),
                lifetime: FilesystemApprovalLifetime::Turn,
            })
            .expect("turn grant");
        grants
            .grant_scope(&FilesystemApprovalScope {
                directory: session_root.display().to_string(),
                lifetime: FilesystemApprovalLifetime::Session,
            })
            .expect("session grant");

        drop(turn_guard);

        assert_eq!(
            grants.scoped_roots(),
            vec![crate::host_paths::normalized_native_path(
                &session_root.canonicalize().unwrap()
            )]
        );
        state.clear_session_filesystem_grants("session-1");
        assert!(grants.scoped_roots().is_empty());
    }

    #[tokio::test]
    async fn delegated_grants_share_only_the_parent_turn_scope() {
        let temp = tempfile::tempdir().expect("temp");
        let turn_root = temp.path().join("turn");
        let parent_session_root = temp.path().join("parent-session");
        let child_session_root = temp.path().join("child-session");
        for root in [&turn_root, &parent_session_root, &child_session_root] {
            std::fs::create_dir_all(root).expect("grant root");
        }
        let state = StateRuntime::open(":memory:").await.expect("state");
        let parent = state.filesystem_grants("parent");
        parent
            .grant_scope(&FilesystemApprovalScope {
                directory: turn_root.display().to_string(),
                lifetime: FilesystemApprovalLifetime::Turn,
            })
            .expect("parent turn grant");
        parent
            .grant_scope(&FilesystemApprovalScope {
                directory: parent_session_root.display().to_string(),
                lifetime: FilesystemApprovalLifetime::Session,
            })
            .expect("parent session grant");
        let child = state.filesystem_grants_with_turn_scopes("child", "parent");
        child
            .grant_scope(&FilesystemApprovalScope {
                directory: child_session_root.display().to_string(),
                lifetime: FilesystemApprovalLifetime::Session,
            })
            .expect("child session grant");

        let child_roots = child.scoped_roots();
        assert!(
            child_roots.contains(&crate::host_paths::normalized_native_path(
                &turn_root.canonicalize().expect("turn identity")
            ))
        );
        assert!(
            child_roots.contains(&crate::host_paths::normalized_native_path(
                &child_session_root.canonicalize().expect("child identity")
            ))
        );
        assert!(
            !child_roots.contains(&crate::host_paths::normalized_native_path(
                &parent_session_root.canonicalize().expect("parent identity")
            ))
        );

        state.clear_turn_filesystem_grants("parent");
        assert_eq!(
            child.scoped_roots(),
            vec![crate::host_paths::normalized_native_path(
                &child_session_root.canonicalize().expect("child identity")
            )]
        );
    }
}
