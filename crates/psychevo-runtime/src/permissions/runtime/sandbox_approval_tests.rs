#[cfg(test)]
mod sandbox_approval_tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Debug)]
    struct RecordingApprovalHandler {
        decisions: Mutex<VecDeque<crate::types::PermissionApprovalDecision>>,
        requests: Mutex<Vec<PermissionApprovalRequest>>,
    }

    impl RecordingApprovalHandler {
        fn new(decisions: Vec<crate::types::PermissionApprovalDecision>) -> Arc<Self> {
            Arc::new(Self {
                decisions: Mutex::new(decisions.into()),
                requests: Mutex::new(Vec::new()),
            })
        }

        fn requests(&self) -> Vec<PermissionApprovalRequest> {
            self.requests.lock().expect("requests").clone()
        }
    }

    impl crate::types::ApprovalHandler for RecordingApprovalHandler {
        fn timeout_secs(&self) -> u64 {
            0
        }

        fn request_permission(
            &self,
            request: PermissionApprovalRequest,
        ) -> BoxFuture<'static, crate::types::PermissionApprovalDecision> {
            self.requests.lock().expect("requests").push(request);
            let decision = self
                .decisions
                .lock()
                .expect("decisions")
                .pop_front()
                .unwrap_or_else(crate::types::PermissionApprovalDecision::deny);
            Box::pin(async move { decision })
        }
    }

    fn abort_signal() -> AbortSignal {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        AbortSignal::new(rx)
    }

    fn sandbox_policy(
        cwd: &Path,
        mode: crate::sandbox::SandboxMode,
    ) -> crate::sandbox::SandboxPolicy {
        crate::sandbox::SandboxPolicy::from_config(
            &crate::sandbox::SandboxConfig {
                enabled: true,
                mode,
                writable_roots: Vec::new(),
                include_tmp: false,
                include_common_caches: false,
            },
            cwd,
            crate::types::RunMode::Default,
            &BTreeMap::new(),
        )
        .expect("sandbox policy")
    }

    fn tool_context(
        policy: crate::sandbox::SandboxPolicy,
        grants: crate::sandbox::SandboxWriteGrants,
    ) -> crate::tools::ToolRuntimeContext {
        crate::tools::ToolRuntimeContext {
            task_id: "sandbox-approval-test".to_string(),
            lsp: crate::config::LspConfig {
                enabled: false,
                ..Default::default()
            },
            lsp_manager: crate::tools::write_support::default_lsp_manager(),
            allow_login_shell: false,
            stream_events: None,
            env: BTreeMap::new(),
            path_prefixes: Vec::new(),
            sandbox_policy: policy,
            sandbox_grants: grants,
            ..crate::tools::ToolRuntimeContext::default()
        }
    }

    fn permission_runtime(
        cwd: &Path,
        policy: crate::sandbox::SandboxPolicy,
        grants: crate::sandbox::SandboxWriteGrants,
        handler: Arc<RecordingApprovalHandler>,
    ) -> PermissionRuntime {
        permission_runtime_with_config(
            cwd,
            PermissionConfig::default(),
            policy,
            grants,
            handler,
        )
    }

    fn permission_runtime_with_config(
        cwd: &Path,
        config: PermissionConfig,
        policy: crate::sandbox::SandboxPolicy,
        grants: crate::sandbox::SandboxWriteGrants,
        handler: Arc<RecordingApprovalHandler>,
    ) -> PermissionRuntime {
        PermissionRuntime::new(
            cwd.to_path_buf(),
            cwd.join(".psychevo"),
            config,
            PermissionMode::Default,
            ApprovalMode::Manual,
            Some(handler),
            None,
        )
        .with_sandbox(policy, grants)
    }

    fn wrapped_write(
        cwd: &Path,
        policy: crate::sandbox::SandboxPolicy,
        grants: crate::sandbox::SandboxWriteGrants,
        runtime: &PermissionRuntime,
    ) -> Arc<dyn ToolBinding> {
        runtime
            .wrap_tools(vec![Arc::new(crate::tools::WriteTool::new(
                cwd.to_path_buf(),
                tool_context(policy, grants),
            )) as Arc<dyn ToolBinding>])
            .into_iter()
            .next()
            .expect("write tool")
    }

    fn wrapped_edit(
        cwd: &Path,
        policy: crate::sandbox::SandboxPolicy,
        grants: crate::sandbox::SandboxWriteGrants,
        runtime: &PermissionRuntime,
    ) -> Arc<dyn ToolBinding> {
        runtime
            .wrap_tools(vec![Arc::new(crate::tools::EditTool::new(
                cwd.to_path_buf(),
                tool_context(policy, grants),
            )) as Arc<dyn ToolBinding>])
            .into_iter()
            .next()
            .expect("edit tool")
    }

    #[tokio::test]
    async fn allow_once_grants_current_write_call_only() {
        let work = tempfile::tempdir().expect("work");
        let outside = tempfile::tempdir().expect("outside");
        let target = outside.path().join("writer.txt");
        let policy = sandbox_policy(work.path(), crate::sandbox::SandboxMode::WorkspaceWrite);
        let grants = crate::sandbox::SandboxWriteGrants::default();
        let handler = RecordingApprovalHandler::new(vec![
            crate::types::PermissionApprovalDecision::allow_once(),
        ]);
        let runtime =
            permission_runtime(work.path(), policy.clone(), grants.clone(), handler.clone());
        let write = wrapped_write(work.path(), policy, grants, &runtime);

        let first = write
            .execute(
                "call-write-once".to_string(),
                json!({"path": target.display().to_string(), "content": "one\n"}),
                abort_signal(),
            )
            .await;
        assert!(!first.is_error, "{:?}", first.json);
        assert_eq!(std::fs::read_to_string(&target).expect("target"), "one\n");

        let requests = handler.requests();
        assert_eq!(requests.len(), 1);
        assert!(!requests[0].allow_always);
        assert!(requests[0].reason.contains("sandbox approval required"));

        let second = write
            .execute(
                "call-write-second".to_string(),
                json!({"path": target.display().to_string(), "content": "two\n"}),
                abort_signal(),
            )
            .await;
        assert!(second.is_error, "{:?}", second.json);
        assert_eq!(std::fs::read_to_string(&target).expect("target"), "one\n");
    }

    #[tokio::test]
    async fn allow_session_reuses_same_file_key_but_not_sibling() {
        let work = tempfile::tempdir().expect("work");
        let outside = tempfile::tempdir().expect("outside");
        let target = outside.path().join("session.txt");
        let sibling = outside.path().join("sibling.txt");
        let policy = sandbox_policy(work.path(), crate::sandbox::SandboxMode::WorkspaceWrite);
        let grants = crate::sandbox::SandboxWriteGrants::default();
        let handler = RecordingApprovalHandler::new(vec![
            crate::types::PermissionApprovalDecision::allow_session(),
        ]);
        let runtime =
            permission_runtime(work.path(), policy.clone(), grants.clone(), handler.clone());
        let write = wrapped_write(work.path(), policy, grants, &runtime);

        let first = write
            .execute(
                "call-session-1".to_string(),
                json!({"path": target.display().to_string(), "content": "one\n"}),
                abort_signal(),
            )
            .await;
        assert!(!first.is_error, "{:?}", first.json);
        let second = write
            .execute(
                "call-session-2".to_string(),
                json!({"path": target.display().to_string(), "content": "two\n"}),
                abort_signal(),
            )
            .await;
        assert!(!second.is_error, "{:?}", second.json);
        assert_eq!(std::fs::read_to_string(&target).expect("target"), "two\n");
        assert_eq!(handler.requests().len(), 1);

        let sibling_result = write
            .execute(
                "call-session-sibling".to_string(),
                json!({"path": sibling.display().to_string(), "content": "bad\n"}),
                abort_signal(),
            )
            .await;
        assert!(sibling_result.is_error, "{:?}", sibling_result.json);
        assert!(!sibling.exists());
        assert_eq!(handler.requests().len(), 2);
    }

    #[tokio::test]
    async fn read_only_sandbox_denies_without_prompt() {
        let work = tempfile::tempdir().expect("work");
        let target = work.path().join("blocked.txt");
        let policy = sandbox_policy(work.path(), crate::sandbox::SandboxMode::ReadOnly);
        let grants = crate::sandbox::SandboxWriteGrants::default();
        let handler = RecordingApprovalHandler::new(vec![
            crate::types::PermissionApprovalDecision::allow_once(),
        ]);
        let runtime =
            permission_runtime(work.path(), policy.clone(), grants.clone(), handler.clone());
        let write = wrapped_write(work.path(), policy, grants, &runtime);

        let output = write
            .execute(
                "call-read-only".to_string(),
                json!({"path": target.display().to_string(), "content": "no\n"}),
                abort_signal(),
            )
            .await;

        assert!(output.is_error, "{:?}", output.json);
        assert!(
            output.json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("read-only")
        );
        assert!(!target.exists());
        assert!(handler.requests().is_empty());
    }

    #[tokio::test]
    async fn allow_once_grants_current_edit_call() {
        let work = tempfile::tempdir().expect("work");
        let outside = tempfile::tempdir().expect("outside");
        let target = outside.path().join("edit.txt");
        std::fs::write(&target, "alpha\n").expect("seed");
        let policy = sandbox_policy(work.path(), crate::sandbox::SandboxMode::WorkspaceWrite);
        let grants = crate::sandbox::SandboxWriteGrants::default();
        let handler = RecordingApprovalHandler::new(vec![
            crate::types::PermissionApprovalDecision::allow_once(),
        ]);
        let runtime =
            permission_runtime(work.path(), policy.clone(), grants.clone(), handler.clone());
        let edit = wrapped_edit(work.path(), policy, grants, &runtime);

        let output = edit
            .execute(
                "call-edit-once".to_string(),
                json!({
                    "mode": "replace",
                    "path": target.display().to_string(),
                    "old_string": "alpha",
                    "new_string": "beta"
                }),
                abort_signal(),
            )
            .await;

        assert!(!output.is_error, "{:?}", output.json);
        assert_eq!(std::fs::read_to_string(&target).expect("target"), "beta\n");
        let requests = handler.requests();
        assert_eq!(requests.len(), 1);
        assert!(!requests[0].allow_always);
    }

    #[tokio::test]
    async fn sandbox_prompts_even_when_permission_profile_allows_path() {
        let work = tempfile::tempdir().expect("work");
        let outside = tempfile::tempdir().expect("outside");
        let target = outside.path().join("profile-allowed.txt");
        let policy = sandbox_policy(work.path(), crate::sandbox::SandboxMode::WorkspaceWrite);
        let grants = crate::sandbox::SandboxWriteGrants::default();
        let handler = RecordingApprovalHandler::new(vec![
            crate::types::PermissionApprovalDecision::allow_once(),
        ]);
        let config = PermissionConfig {
            default_permissions: ":danger-full-access".to_string(),
            ..PermissionConfig::default()
        };
        let runtime = permission_runtime_with_config(
            work.path(),
            config,
            policy.clone(),
            grants.clone(),
            handler.clone(),
        );
        let write = wrapped_write(work.path(), policy, grants, &runtime);

        let output = write
            .execute(
                "call-profile-allowed".to_string(),
                json!({"path": target.display().to_string(), "content": "ok\n"}),
                abort_signal(),
            )
            .await;

        assert!(!output.is_error, "{:?}", output.json);
        assert_eq!(std::fs::read_to_string(&target).expect("target"), "ok\n");
        let requests = handler.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].reason,
            format!(
                "sandbox approval required: write to {} is outside configured writable roots",
                target.display()
            )
        );
        assert!(!requests[0].allow_always);
    }

    #[tokio::test]
    async fn approval_policy_never_blocks_sandbox_widening_prompt() {
        let work = tempfile::tempdir().expect("work");
        let outside = tempfile::tempdir().expect("outside");
        let target = outside.path().join("blocked-by-never.txt");
        let policy = sandbox_policy(work.path(), crate::sandbox::SandboxMode::WorkspaceWrite);
        let grants = crate::sandbox::SandboxWriteGrants::default();
        let handler = RecordingApprovalHandler::new(vec![
            crate::types::PermissionApprovalDecision::allow_once(),
        ]);
        let config = PermissionConfig {
            default_permissions: ":danger-full-access".to_string(),
            approval_policy: ApprovalPolicy::Never,
            ..PermissionConfig::default()
        };
        let runtime = permission_runtime_with_config(
            work.path(),
            config,
            policy.clone(),
            grants.clone(),
            handler.clone(),
        );
        let write = wrapped_write(work.path(), policy, grants, &runtime);

        let output = write
            .execute(
                "call-never".to_string(),
                json!({"path": target.display().to_string(), "content": "no\n"}),
                abort_signal(),
            )
            .await;

        assert!(output.is_error, "{:?}", output.json);
        assert!(
            output.json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("approval_policy=never")
        );
        assert!(!target.exists());
        assert!(handler.requests().is_empty());
    }
}
