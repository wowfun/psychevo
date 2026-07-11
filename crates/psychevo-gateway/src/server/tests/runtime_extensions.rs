#[derive(Debug, Clone, PartialEq)]
struct TypedCodexExtensionCall {
    profile_id: String,
    profile_command: Option<String>,
    expected_binding_revision: Option<u64>,
    namespace: String,
    operation: String,
    argument: Option<Value>,
}

#[derive(Debug)]
struct TypedCodexExtensionRuntime {
    calls: Arc<Mutex<Vec<TypedCodexExtensionCall>>>,
}

impl psychevo_runtime_host::RuntimeModule for TypedCodexExtensionRuntime {
    fn snapshot(
        &self,
        _query: psychevo_runtime_host::SnapshotQuery,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::RuntimeSnapshot> {
        Box::pin(async {
            Err(psychevo_runtime_host::RuntimeError::new(
                "snapshot_not_used",
                psychevo_runtime_host::RuntimeErrorStage::Discovery,
                psychevo_runtime_host::RetryClass::Never,
                "typed extension test does not probe snapshots",
            ))
        })
    }

    fn execute(
        &self,
        request: psychevo_runtime_host::ExecuteRequest,
        _observer: psychevo_runtime_host::RuntimeObserver,
        _control: psychevo_runtime_host::RuntimeControl,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::ExecuteResult> {
        let calls = Arc::clone(&self.calls);
        Box::pin(async move {
            let psychevo_runtime_host::RuntimeIntent::Extension(extension) = request.intent else {
                return Err(psychevo_runtime_host::RuntimeError::new(
                    "unexpected_intent",
                    psychevo_runtime_host::RuntimeErrorStage::Configuration,
                    psychevo_runtime_host::RetryClass::Never,
                    "typed extension test expected an Extension intent",
                ));
            };
            calls
                .lock()
                .expect("typed extension calls poisoned")
                .push(TypedCodexExtensionCall {
                    profile_id: request.profile.id,
                    profile_command: request.profile.command,
                    expected_binding_revision: request.expected_binding_revision,
                    namespace: extension.namespace.clone(),
                    operation: extension.operation.clone(),
                    argument: extension.argument.clone(),
                });
            let result = match (extension.namespace.as_str(), extension.operation.as_str()) {
                ("codex.goal", "read") => json!({
                    "goal": {
                        "objective": "Read native goal",
                        "status": "active",
                        "tokenBudget": 20000,
                        "tokensUsed": 400,
                        "timeUsedSeconds": 30,
                        "createdAt": 10,
                        "updatedAt": 20,
                        "nativeThreadId": "native-thread-secret"
                    }
                }),
                ("codex.goal", "set") => json!({
                    "goal": {
                        "objective": "Ship evidence",
                        "status": "paused",
                        "tokenBudget": null,
                        "tokensUsed": 500,
                        "timeUsedSeconds": 40,
                        "createdAt": 10,
                        "updatedAt": 30,
                        "nativeThreadId": "native-thread-secret"
                    }
                }),
                ("codex.goal", "clear") => json!({ "cleared": true }),
                ("codex.account", "rateLimits/read") => json!({
                    "rateLimits": {
                        "limitId": "codex",
                        "limitName": "Codex",
                        "primary": {
                            "usedPercent": 25,
                            "windowDurationMins": 300,
                            "resetsAt": 9000
                        },
                        "secondary": null,
                        "credits": {
                            "hasCredits": true,
                            "unlimited": false,
                            "balance": "12.50"
                        },
                        "individualLimit": null,
                        "planType": "pro",
                        "rateLimitReachedType": null
                    },
                    "rateLimitsByLimitId": {},
                    "resetCreditsAvailable": 2,
                    "nativeAccountId": "account-secret"
                }),
                _ => {
                    return Err(psychevo_runtime_host::RuntimeError::new(
                        "unexpected_extension",
                        psychevo_runtime_host::RuntimeErrorStage::Configuration,
                        psychevo_runtime_host::RetryClass::Never,
                        format!(
                            "unexpected extension: {}/{}",
                            extension.namespace, extension.operation
                        ),
                    ));
                }
            };
            Ok(psychevo_runtime_host::ExecuteResult::Extension(result))
        })
    }

    fn shutdown(
        &self,
        _mode: psychevo_runtime_host::ShutdownMode,
    ) -> psychevo_runtime_host::RuntimeFuture<()> {
        Box::pin(async { Ok(()) })
    }
}

fn web_state_with_typed_codex_extensions() -> (
    tempfile::TempDir,
    WebState,
    String,
    Arc<Mutex<Vec<TypedCodexExtensionCall>>>,
) {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let env = BTreeMap::from([
        (
            "HOME".to_string(),
            temp.path().to_string_lossy().to_string(),
        ),
        (
            "PSYCHEVO_HOME".to_string(),
            home.to_string_lossy().to_string(),
        ),
    ]);
    let runtime_state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let calls = Arc::new(Mutex::new(Vec::new()));
    let host = psychevo_runtime_host::RuntimeHost::new();
    host.register(
        psychevo_runtime_host::RuntimeKind::Codex,
        Arc::new(TypedCodexExtensionRuntime {
            calls: Arc::clone(&calls),
        }),
    );
    let gateway = Gateway::with_backend_and_runtime_host(
        runtime_state,
        Arc::new(crate::PsychevoRuntimeBackend),
        host,
    );
    let state = WebState::new(GatewayWebServerConfig::new(
        gateway,
        home,
        cwd.clone(),
        None,
        env,
        temp.path().join("static"),
    ));
    let profile = RuntimeProfileConfig {
        id: "codex".to_string(),
        runtime: RuntimeProfileKind::Codex,
        enabled: true,
        label: "Captured Codex".to_string(),
        backend_ref: None,
        command: Some("captured-codex-command".to_string()),
        args: vec!["app-server".to_string(), "--stdio".to_string()],
        env: BTreeMap::new(),
        default_model: None,
        default_mode: None,
        default_agent: None,
        approval_mode: Some("on-request".to_string()),
        sandbox: Some("workspace-write".to_string()),
        workspace_roots: vec![cwd.display().to_string()],
        options: Value::Null,
    };
    let fingerprint = crate::runtime_profile_config_fingerprint(&profile);
    let revision = crate::runtime_profile_config_revision(&fingerprint).to_string();
    let encoded = serde_json::to_string(&profile).expect("captured profile JSON");
    let thread_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&cwd, "web", "pending", "codex", None)
        .expect("thread");
    state
        .inner
        .state
        .store()
        .create_gateway_runtime_binding(psychevo_runtime::GatewayRuntimeBindingInput {
            thread_id: &thread_id,
            runtime_ref: "codex",
            backend_kind: "runtime",
            native_kind: "codex",
            native_session_id: Some("native-thread-secret"),
            cwd: &cwd.display().to_string(),
            profile_fingerprint: &fingerprint,
            profile_revision: &revision,
            profile_config_json: &encoded,
            adapter_kind: "codex",
            adapter_revision: "test",
            ownership: GatewayRuntimeBindingOwnership::ReadWrite,
            parent_thread_id: None,
        })
        .expect("runtime binding");
    (temp, state, thread_id, calls)
}

async fn typed_runtime_extension_rpc(
    state: &WebState,
    method: &str,
    params: Value,
) -> psychevo_runtime::Result<Value> {
    let (tx, _rx) = mpsc::unbounded_channel();
    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(method)),
            method: method.to_string(),
            params: Some(params),
        },
    )
    .await
}

#[tokio::test]
async fn typed_codex_goal_and_rate_limit_rpcs_use_the_immutable_binding() {
    let (_temp, state, thread_id, calls) = web_state_with_typed_codex_extensions();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let scope_value = serde_json::to_value(scope.to_wire_scope()).expect("scope JSON");

    let read = typed_runtime_extension_rpc(
        &state,
        "runtime/goal/read",
        json!({ "threadId": thread_id, "scope": scope_value }),
    )
    .await
    .expect("goal read");
    assert_eq!(read["runtimeRef"], "codex");
    assert_eq!(read["goal"]["objective"], "Read native goal");
    assert_eq!(read["bindingRevision"], 1);
    assert!(
        !read.to_string().contains("native-thread-secret"),
        "{read:#}"
    );

    let set = typed_runtime_extension_rpc(
        &state,
        "runtime/goal/set",
        json!({
            "threadId": thread_id,
            "scope": scope_value,
            "objective": "  Ship evidence  ",
            "status": "paused",
            "clearTokenBudget": true
        }),
    )
    .await
    .expect("goal set");
    assert_eq!(set["goal"]["objective"], "Ship evidence");
    assert_eq!(set["goal"]["status"], "paused");
    assert_eq!(set["goal"]["tokenBudget"], Value::Null);
    assert!(!set.to_string().contains("native-thread-secret"), "{set:#}");

    let mismatch = typed_runtime_extension_rpc(
        &state,
        "runtime/account/rateLimits/read",
        json!({
            "threadId": thread_id,
            "runtimeRef": "opencode",
            "scope": scope_value
        }),
    )
    .await
    .expect_err("bound runtime mismatch");
    assert!(mismatch.to_string().contains("immutable"), "{mismatch:#}");

    let rate_limits = typed_runtime_extension_rpc(
        &state,
        "runtime/account/rateLimits/read",
        json!({ "threadId": thread_id, "scope": scope_value }),
    )
    .await
    .expect("rate-limit read");
    assert_eq!(
        rate_limits["accountRateLimits"]["rateLimits"]["primary"]["usedPercent"],
        25
    );
    assert_eq!(rate_limits["accountRateLimits"]["resetCreditsAvailable"], 2);
    assert!(!rate_limits.to_string().contains("account-secret"));

    let context = typed_runtime_extension_rpc(
        &state,
        "runtime/context/read",
        json!({ "threadId": thread_id, "scope": scope_value }),
    )
    .await
    .expect("runtime context");
    assert_eq!(context["goal"]["objective"], "Ship evidence");
    assert_eq!(
        context["accountRateLimits"]["rateLimits"]["primary"]["usedPercent"],
        25
    );
    assert!(!context.to_string().contains("native-thread-secret"));
    assert!(!context.to_string().contains("account-secret"));

    let clear = typed_runtime_extension_rpc(
        &state,
        "runtime/goal/clear",
        json!({ "threadId": thread_id, "scope": scope_value }),
    )
    .await
    .expect("goal clear");
    assert_eq!(clear["cleared"], true);
    let context = typed_runtime_extension_rpc(
        &state,
        "runtime/context/read",
        json!({ "threadId": thread_id, "scope": scope_value }),
    )
    .await
    .expect("runtime context after clear");
    assert_eq!(context["goal"], Value::Null);
    assert_eq!(
        context["accountRateLimits"]["rateLimits"]["primary"]["usedPercent"],
        25
    );

    let calls = calls.lock().expect("typed extension calls poisoned");
    assert_eq!(calls.len(), 4, "mismatch must not invoke the adapter");
    assert!(calls.iter().all(|call| {
        call.profile_id == "codex"
            && call.profile_command.as_deref() == Some("captured-codex-command")
            && call.expected_binding_revision == Some(1)
    }));
    assert_eq!(
        calls
            .iter()
            .map(|call| (call.namespace.as_str(), call.operation.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("codex.goal", "read"),
            ("codex.goal", "set"),
            ("codex.account", "rateLimits/read"),
            ("codex.goal", "clear"),
        ]
    );
    let set_argument = calls[1].argument.as_ref().expect("set argument");
    assert_eq!(set_argument["threadId"], thread_id);
    assert_eq!(set_argument["nativeSessionId"], "native-thread-secret");
    assert_eq!(set_argument["objective"], "Ship evidence");
    assert_eq!(set_argument["status"], "paused");
    assert_eq!(set_argument["tokenBudget"], Value::Null);
}

#[tokio::test]
async fn typed_codex_goal_rpc_rejects_invalid_or_adapter_shaped_input_before_execution() {
    let (_temp, state, thread_id, calls) = web_state_with_typed_codex_extensions();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let scope_value = serde_json::to_value(scope.to_wire_scope()).expect("scope JSON");

    let empty = typed_runtime_extension_rpc(
        &state,
        "runtime/goal/set",
        json!({ "threadId": thread_id, "scope": scope_value }),
    )
    .await
    .expect_err("empty goal mutation");
    assert!(
        empty.to_string().contains("requires objective"),
        "{empty:#}"
    );

    let conflicting_budget = typed_runtime_extension_rpc(
        &state,
        "runtime/goal/set",
        json!({
            "threadId": thread_id,
            "scope": scope_value,
            "tokenBudget": 1000,
            "clearTokenBudget": true
        }),
    )
    .await
    .expect_err("conflicting token-budget mutation");
    assert!(
        conflicting_budget
            .to_string()
            .contains("cannot be supplied together"),
        "{conflicting_budget:#}"
    );

    let adapter_shaped = typed_runtime_extension_rpc(
        &state,
        "runtime/goal/read",
        json!({
            "threadId": thread_id,
            "runtimeRef": "codex",
            "nativeSessionId": "native-thread-secret",
            "scope": scope_value
        }),
    )
    .await
    .expect_err("adapter-shaped public input");
    assert!(adapter_shaped.to_string().contains("unknown field"));
    assert!(
        calls
            .lock()
            .expect("typed extension calls poisoned")
            .is_empty()
    );
}
