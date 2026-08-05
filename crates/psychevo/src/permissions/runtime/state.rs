use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use psychevo_ai::AbortSignal;
use serde_json::Value;

use crate::types::{
    ExecPolicyDecision, FilesystemApprovalRequest, PermissionAccess, PermissionApprovalOutcome,
    PermissionConfig, PermissionMode,
};

#[derive(Clone)]
pub(crate) struct PermissionRuntime {
    pub(super) inner: Arc<PermissionRuntimeInner>,
}

pub(super) struct PermissionRuntimeInner {
    pub(super) cwd: PathBuf,
    pub(super) project_config_dir: PathBuf,
    pub(super) protected_config_paths: Vec<PathBuf>,
    pub(super) mode: PermissionMode,
    pub(super) config: PermissionConfig,
    pub(super) sandbox_policy: crate::sandbox::SandboxPolicy,
    pub(super) sandbox_grants: crate::sandbox::SandboxWriteGrants,
    pub(super) session_grants: Mutex<HashSet<String>>,
    pub(super) pending_approvals: Mutex<VecDeque<String>>,
    pub(super) approval_events: Mutex<Vec<ApprovalLifecycleEvent>>,
    pub(super) approval_handler: Option<Arc<dyn crate::types::ApprovalHandler>>,
    pub(super) smart_approval_handler: Option<Arc<dyn crate::types::ApprovalHandler>>,
    pub(super) hook_runtime: Option<crate::hooks::HookRuntime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ApprovalLifecycleEvent {
    Requested {
        tool_call_id: String,
        tool_name: String,
    },
    Resolved {
        tool_call_id: String,
        outcome: PermissionApprovalOutcome,
    },
    Aborted {
        tool_call_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PermissionDecision {
    Allow,
    Ask {
        reason: String,
        matched_rule: Option<String>,
        suggested_rule: Option<String>,
        allow_always: bool,
        session_key: String,
        persistent_grants: Vec<PersistentPermissionGrant>,
    },
    Deny {
        reason: String,
        matched_rule: Option<String>,
    },
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PermissionDecisionView {
    Allow,
    Ask { reason: String },
    Deny { reason: String },
}

#[cfg(test)]
impl From<PermissionDecision> for PermissionDecisionView {
    fn from(decision: PermissionDecision) -> Self {
        match decision {
            PermissionDecision::Allow => Self::Allow,
            PermissionDecision::Ask { reason, .. } => Self::Ask { reason },
            PermissionDecision::Deny { reason, .. } => Self::Deny { reason },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PersistentPermissionGrant {
    Filesystem {
        path: String,
        access: PermissionAccess,
    },
    Network {
        host: String,
        access: PermissionAccess,
    },
    WebSearch {
        query: String,
        access: PermissionAccess,
    },
    Exec {
        prefix: Vec<String>,
        decision: ExecPolicyDecision,
    },
    Skill {
        key: String,
        access: PermissionAccess,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SandboxWriteGrantRequest {
    pub(super) paths: Vec<PathBuf>,
    pub(super) reason: String,
}

pub(super) struct ApprovalDecisionRequest<'a> {
    pub(super) tool_call_id: &'a str,
    pub(super) tool_name: &'a str,
    pub(super) args: &'a Value,
    pub(super) reason: &'a str,
    pub(super) matched_rule: Option<&'a str>,
    pub(super) suggested_rule: Option<String>,
    pub(super) allow_always: bool,
    pub(super) filesystem: Option<FilesystemApprovalRequest>,
    pub(super) mcp_startup: Option<crate::types::McpStartupApprovalRequest>,
    pub(super) abort: Option<AbortSignal>,
}

pub(super) struct PendingApprovalGuard {
    pub(super) runtime: PermissionRuntime,
    pub(super) tool_call_id: String,
    pub(super) finished: bool,
}

impl PendingApprovalGuard {
    pub(super) fn finish(&mut self, outcome: PermissionApprovalOutcome) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.runtime
            .finish_pending_approval(&self.tool_call_id, Some(outcome));
    }
}

impl Drop for PendingApprovalGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.runtime
                .finish_pending_approval(&self.tool_call_id, None);
        }
    }
}
