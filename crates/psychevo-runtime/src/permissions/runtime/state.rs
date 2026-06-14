#[derive(Clone)]
pub(crate) struct PermissionRuntime {
    pub(crate) inner: Arc<PermissionRuntimeInner>,
}

pub(crate) struct PermissionRuntimeInner {
    pub(crate) workdir: PathBuf,
    pub(crate) project_config_dir: PathBuf,
    pub(crate) mode: PermissionMode,
    pub(crate) config: PermissionConfig,
    pub(crate) sandbox_policy: crate::sandbox::SandboxPolicy,
    pub(crate) sandbox_grants: crate::sandbox::SandboxWriteGrants,
    pub(crate) session_grants: Mutex<HashSet<String>>,
    pub(crate) pending_approvals: Mutex<VecDeque<String>>,
    pub(crate) approval_events: Mutex<Vec<ApprovalLifecycleEvent>>,
    pub(crate) approval_handler: Option<Arc<dyn crate::types::ApprovalHandler>>,
    pub(crate) smart_approval_handler: Option<Arc<dyn crate::types::ApprovalHandler>>,
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

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PermissionRule {
    pub(crate) raw: String,
    pub(crate) tool: String,
    pub(crate) pattern: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PermissionDecision {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PersistentPermissionGrant {
    Filesystem {
        path: String,
        access: PermissionAccess,
    },
    Network {
        host: String,
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
struct SandboxWriteGrantRequest {
    paths: Vec<PathBuf>,
    reason: String,
}

struct ApprovalDecisionRequest<'a> {
    tool_call_id: &'a str,
    tool_name: &'a str,
    args: &'a Value,
    reason: &'a str,
    matched_rule: Option<&'a str>,
    suggested_rule: Option<String>,
    allow_always: bool,
    abort: Option<AbortSignal>,
}

pub(crate) struct PendingApprovalGuard {
    runtime: PermissionRuntime,
    tool_call_id: String,
    finished: bool,
}

impl PendingApprovalGuard {
    pub(crate) fn finish(&mut self, outcome: PermissionApprovalOutcome) {
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
