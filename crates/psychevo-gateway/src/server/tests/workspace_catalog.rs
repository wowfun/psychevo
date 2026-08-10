use std::sync::{Arc, Mutex};

use psychevo::{StartThreadRequest, WorkspaceUpdate};
use serde_json::json;
use tokio::sync::mpsc;

use crate::server::binding::{AuthContext, BrowserSession};
use crate::server::rpc_dispatch::handle_rpc;
use crate::server::rpc_json::RpcRequest;
use crate::server::runtime_profiles::validate_draft_workspace_roots;
use crate::server::scope_session::default_resolved_scope;
use crate::server::tests::helpers::{
    rpc_test_request, web_state, web_state_with_native_test_executor,
};

#[tokio::test]
async fn pinning_an_implicit_workspace_does_not_stale_its_editor_revision() {
    let (_temp, state) = web_state().await;
    let thread = state
        .inner
        .framework
        .start_thread(StartThreadRequest::new(&state.inner.cwd))
        .await
        .expect("direct Thread");
    let context = state
        .inner
        .framework
        .thread_workspace_context(thread.id())
        .await
        .expect("Workspace context");
    let workspace = state
        .inner
        .framework
        .workspace(&context.workspace_id)
        .await
        .expect("Workspace lookup")
        .expect("Workspace");
    let (tx, _rx) = mpsc::unbounded_channel();

    rpc_test_request(
        &state,
        &tx,
        "workspace/pin/set",
        json!({ "workspaceId": workspace.id, "pinned": true }),
    )
    .await;
    let updated = rpc_test_request(
        &state,
        &tx,
        "workspace/catalog/update",
        json!({
            "workspaceId": workspace.id,
            "expectedRevision": workspace.revision,
            "name": "Pinned workspace",
            "roots": workspace.roots,
        }),
    )
    .await;

    assert_eq!(updated["workspace"]["name"], "Pinned workspace");
}

#[tokio::test]
async fn thread_snapshot_projects_authoritative_direct_roots_not_catalog_membership() {
    let (temp, state) = web_state().await;
    let secondary = temp.path().join("secondary");
    std::fs::create_dir_all(&secondary).expect("secondary root");
    let thread = state
        .inner
        .framework
        .start_thread(StartThreadRequest::new(&state.inner.cwd))
        .await
        .expect("direct Thread");
    let context = state
        .inner
        .framework
        .thread_workspace_context(thread.id())
        .await
        .expect("Workspace context");
    let workspace = state
        .inner
        .framework
        .workspace(&context.workspace_id)
        .await
        .expect("Workspace lookup")
        .expect("Workspace");
    state
        .inner
        .framework
        .update_workspace(WorkspaceUpdate {
            workspace_id: workspace.id,
            expected_revision: workspace.revision,
            name: "Catalog group".to_string(),
            roots: vec![state.inner.cwd.clone(), secondary],
        })
        .await
        .expect("multi-root catalog Workspace");
    let (tx, _rx) = mpsc::unbounded_channel();

    let snapshot = rpc_test_request(
        &state,
        &tx,
        "thread/read",
        json!({ "threadId": thread.id() }),
    )
    .await;

    assert_eq!(snapshot["workspaceRoots"], json!([state.inner.cwd]));
}

#[tokio::test]
async fn draft_workspace_root_fence_rejects_a_catalog_edit() {
    let (temp, state) = web_state().await;
    let primary = state.inner.cwd.clone();
    let secondary = temp.path().join("secondary");
    let replacement = temp.path().join("replacement");
    std::fs::create_dir_all(&secondary).expect("secondary root");
    std::fs::create_dir_all(&replacement).expect("replacement root");
    let thread = state
        .inner
        .framework
        .start_thread(StartThreadRequest::new(&primary))
        .await
        .expect("seed Thread");
    let context = state
        .inner
        .framework
        .thread_workspace_context(thread.id())
        .await
        .expect("Workspace context");
    let current = state
        .inner
        .framework
        .workspace(&context.workspace_id)
        .await
        .expect("Workspace lookup")
        .expect("Workspace");
    let workspace = state
        .inner
        .framework
        .update_workspace(WorkspaceUpdate {
            workspace_id: current.id,
            expected_revision: current.revision,
            name: "Draft roots".to_string(),
            roots: vec![primary.clone(), secondary.clone()],
        })
        .await
        .expect("multi-root Workspace");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");

    validate_draft_workspace_roots(
        &state,
        &scope,
        Some(&workspace.id),
        std::slice::from_ref(&secondary),
    )
    .await
    .expect("current roots");
    state
        .inner
        .framework
        .update_workspace(WorkspaceUpdate {
            workspace_id: workspace.id.clone(),
            expected_revision: workspace.revision,
            name: workspace.name,
            roots: vec![primary, replacement],
        })
        .await
        .expect("replace secondary root");

    let error = validate_draft_workspace_roots(
        &state,
        &scope,
        Some(&workspace.id),
        std::slice::from_ref(&secondary),
    )
    .await
    .expect_err("stale draft roots");
    assert!(error.to_string().contains("Workspace directories changed"));
}

#[tokio::test]
async fn first_workspace_turn_resolves_current_primary_before_native_preparation() {
    let observed = Arc::new(Mutex::new(None));
    let observed_executor = Arc::clone(&observed);
    let executor: crate::FrameworkNativeTestExecutor = Arc::new(move |invocation| {
        *observed_executor.lock().expect("observed invocation") = Some(invocation.thread.clone());
        Box::pin(async move {
            invocation.persistence.confirm_delivery().await?;
            Ok(psychevo::TurnResult {
                thread_id: invocation.receipt.thread_id,
                outcome: psychevo::TurnOutcome::Completed,
                final_answer: String::new(),
                provider: "fixture".to_string(),
                model: "fixture".to_string(),
                reasoning_effort: None,
                tool_failures: 0,
                context_limit: None,
                context_snapshot: None,
                warnings: Vec::new(),
                terminal_reason: None,
                terminal_error: None,
                selected_agent: None,
                selected_skills: Vec::new(),
            })
        })
    });
    let (temp, state) = web_state_with_native_test_executor(executor).await;
    let primary = state.inner.cwd.clone();
    let secondary = temp.path().join("secondary");
    std::fs::create_dir_all(&secondary).expect("secondary");
    let seed = state
        .inner
        .framework
        .start_thread(StartThreadRequest::new(&primary))
        .await
        .expect("seed direct Thread");
    let context = state
        .inner
        .framework
        .thread_workspace_context(seed.id())
        .await
        .expect("seed context");
    let current = state
        .inner
        .framework
        .workspace(&context.workspace_id)
        .await
        .expect("workspace lookup")
        .expect("workspace");
    let workspace = state
        .inner
        .framework
        .update_workspace(WorkspaceUpdate {
            workspace_id: current.id,
            expected_revision: current.revision,
            name: "Multi-root".to_string(),
            roots: vec![primary.clone(), secondary.clone()],
        })
        .await
        .expect("multi-root workspace");
    let (tx, _rx) = mpsc::unbounded_channel();
    let draft = rpc_test_request(
        &state,
        &tx,
        "thread/draft/open",
        json!({
            "origin": {
                "source": state.inner.source,
                "location": { "kind": "workspace", "workspaceId": workspace.id }
            },
            "targetIntent": { "kind": "default" }
        }),
    )
    .await;
    let reordered = state
        .inner
        .framework
        .update_workspace(WorkspaceUpdate {
            workspace_id: workspace.id.clone(),
            expected_revision: workspace.revision,
            name: workspace.name,
            roots: vec![secondary.clone(), primary.clone()],
        })
        .await
        .expect("reorder Workspace primary");
    let current_context = rpc_test_request(
        &state,
        &tx,
        "thread/context/read",
        json!({
            "scope": {
                "cwd": secondary,
                "source": state.inner.source
            },
            "threadId": null,
            "target": { "agentRef": null, "runtimeProfileRef": "native" }
        }),
    )
    .await;

    let accepted = rpc_test_request(
        &state,
        &tx,
        "turn/start",
        json!({
            "clientTurnId": "workspace-current-primary",
            "scope": draft["snapshot"]["scope"],
            "threadId": null,
            "workspaceId": reordered.id,
            "target": { "agentRef": null, "runtimeProfileRef": "native" },
            "input": [{ "type": "text", "text": "use the current Workspace" }],
            "mentions": [],
            "turnOverrides": { "model": "fake-model" },
            "expectedContextRevision": current_context["contextRevision"],
            "expectedControlRevision": current_context["controlRevision"]
        }),
    )
    .await;
    assert_eq!(accepted["accepted"], true);
    let invocation = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if let Some(invocation) = observed.lock().expect("observed invocation").clone() {
                return invocation;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("native invocation");

    assert_eq!(invocation.cwd, secondary.display().to_string());
    assert_eq!(
        invocation.roots,
        vec![
            secondary.display().to_string(),
            primary.display().to_string()
        ]
    );
}

#[tokio::test]
async fn explicit_workspace_admission_ignores_an_inferred_source_thread() {
    let (_temp, state) = web_state().await;
    let existing = state
        .inner
        .framework
        .start_thread(StartThreadRequest::new(&state.inner.cwd))
        .await
        .expect("existing Thread");
    let scope = crate::server::scope_session::ResolvedScope {
        cwd: state.inner.cwd.clone(),
        source: state.inner.source.clone(),
    };
    crate::server::scope_session::bind_source_to_thread(&state, &scope, existing.id())
        .await
        .expect("source binding");

    let (selected, creates_thread) =
        crate::server::scope_session::ensure_turn_start_thread(&state, &scope, None, true)
            .await
            .expect("explicit Workspace admission");

    assert!(creates_thread);
    assert_ne!(selected.as_deref(), Some(existing.id()));
}

#[tokio::test]
async fn explicit_workspace_shell_ignores_an_inferred_source_thread() {
    let (temp, state) = web_state().await;
    std::fs::write(
        state.inner.home.join("config.toml"),
        r#"
model = "lmstudio/test-model"
[provider.lmstudio.models.test-model]
"#,
    )
    .expect("config");
    let existing = state
        .inner
        .framework
        .start_thread(StartThreadRequest::new(&state.inner.cwd))
        .await
        .expect("existing Thread");
    let scope = crate::server::scope_session::ResolvedScope {
        cwd: state.inner.cwd.clone(),
        source: state.inner.source.clone(),
    };
    crate::server::scope_session::bind_source_to_thread(&state, &scope, existing.id())
        .await
        .expect("source binding");
    let other_root = temp.path().join("other");
    std::fs::create_dir_all(&other_root).expect("other root");
    let other_seed = state
        .inner
        .framework
        .start_thread(StartThreadRequest::new(&other_root))
        .await
        .expect("other Workspace seed");
    let other_context = state
        .inner
        .framework
        .thread_workspace_context(other_seed.id())
        .await
        .expect("other Workspace context");
    let (tx, mut rx) = mpsc::unbounded_channel();

    let accepted = rpc_test_request(
        &state,
        &tx,
        "shell/start",
        json!({
            "scope": scope.to_wire_scope(),
            "workspaceId": other_context.workspace_id,
            "threadId": null,
            "command": "printf explicit-workspace-shell"
        }),
    )
    .await;
    assert_eq!(accepted["accepted"], true);
    assert!(accepted["threadId"].is_null());

    let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(message) = rx.recv().await {
            let notification: serde_json::Value =
                serde_json::from_str(&message).expect("notification");
            if notification["method"] == "shell/result" {
                return notification["params"].clone();
            }
            if notification["method"] == "shell/error" {
                panic!("shell failed: {notification}");
            }
        }
        panic!("shell notification channel closed");
    })
    .await
    .expect("shell result");
    let thread_id = result["thread"]["id"].as_str().expect("result Thread");
    let context = state
        .inner
        .framework
        .thread_workspace_context(thread_id)
        .await
        .expect("shell Workspace context");

    assert_ne!(thread_id, existing.id());
    assert_eq!(context.workspace_id, other_context.workspace_id);
    assert_eq!(context.cwd, other_root.display().to_string());
}

#[tokio::test]
async fn catalog_updates_revoke_preview_roots_until_turn_admission_refreshes_them() {
    let executor: crate::FrameworkNativeTestExecutor = Arc::new(|invocation| {
        Box::pin(async move {
            invocation.persistence.confirm_delivery().await?;
            Ok(psychevo::TurnResult {
                thread_id: invocation.receipt.thread_id,
                outcome: psychevo::TurnOutcome::Completed,
                final_answer: String::new(),
                provider: "fixture".to_string(),
                model: "fixture".to_string(),
                reasoning_effort: None,
                tool_failures: 0,
                context_limit: None,
                context_snapshot: None,
                warnings: Vec::new(),
                terminal_reason: None,
                terminal_error: None,
                selected_agent: None,
                selected_skills: Vec::new(),
            })
        })
    });
    let (temp, state) = web_state_with_native_test_executor(executor).await;
    let primary = state.inner.cwd.clone();
    let removed = temp.path().join("removed");
    let added = temp.path().join("added");
    std::fs::create_dir_all(&removed).expect("removed root");
    std::fs::create_dir_all(&added).expect("added root");
    let seed = state
        .inner
        .framework
        .start_thread(StartThreadRequest::new(&primary))
        .await
        .expect("seed Thread");
    let context = state
        .inner
        .framework
        .thread_workspace_context(seed.id())
        .await
        .expect("Workspace context");
    let workspace = state
        .inner
        .framework
        .workspace(&context.workspace_id)
        .await
        .expect("Workspace lookup")
        .expect("Workspace");
    let workspace = state
        .inner
        .framework
        .update_workspace(WorkspaceUpdate {
            workspace_id: workspace.id,
            expected_revision: workspace.revision,
            name: workspace.name,
            roots: vec![primary.clone(), removed.clone()],
        })
        .await
        .expect("initial roots");
    let browser_session_id = "browser-preview-authority".to_string();
    state
        .inner
        .browser_sessions
        .lock()
        .expect("browser sessions")
        .insert(
            browser_session_id.clone(),
            BrowserSession::with_external_action_grant(primary.clone(), state.inner.source.clone()),
        );
    let auth = || AuthContext::Browser {
        session_id: browser_session_id.clone(),
    };
    let (tx, _rx) = mpsc::unbounded_channel();
    let draft = handle_rpc(
        state.clone(),
        auth(),
        tx.clone(),
        RpcRequest {
            jsonrpc: psychevo_gateway_protocol::source::JSONRPC_VERSION.to_string(),
            id: Some(json!("draft")),
            method: "thread/draft/open".to_string(),
            params: Some(json!({
                "origin": {
                    "source": state.inner.source,
                    "location": { "kind": "workspace", "workspaceId": workspace.id }
                },
                "targetIntent": { "kind": "default" }
            })),
        },
    )
    .await
    .expect("draft");
    let updated = handle_rpc(
        state.clone(),
        auth(),
        tx.clone(),
        RpcRequest {
            jsonrpc: psychevo_gateway_protocol::source::JSONRPC_VERSION.to_string(),
            id: Some(json!("update")),
            method: "workspace/catalog/update".to_string(),
            params: Some(json!({
                "workspaceId": workspace.id,
                "expectedRevision": workspace.revision,
                "name": workspace.name,
                "roots": [primary, added]
            })),
        },
    )
    .await
    .expect("update");
    {
        let sessions = state
            .inner
            .browser_sessions
            .lock()
            .expect("browser sessions");
        let roots = &sessions
            .get(&browser_session_id)
            .expect("browser session")
            .workspace_preview_roots;
        assert_eq!(
            roots,
            &std::collections::BTreeSet::from([psychevo::host_paths::normalized_native_path(
                &state.inner.cwd
            )])
        );
    }
    let current_context = handle_rpc(
        state.clone(),
        auth(),
        tx.clone(),
        RpcRequest {
            jsonrpc: psychevo_gateway_protocol::source::JSONRPC_VERSION.to_string(),
            id: Some(json!("context")),
            method: "thread/context/read".to_string(),
            params: Some(json!({
                "scope": draft["snapshot"]["scope"],
                "threadId": null,
                "target": { "agentRef": null, "runtimeProfileRef": "native" }
            })),
        },
    )
    .await
    .expect("context");
    let accepted = handle_rpc(
        state.clone(),
        auth(),
        tx,
        RpcRequest {
            jsonrpc: psychevo_gateway_protocol::source::JSONRPC_VERSION.to_string(),
            id: Some(json!("turn")),
            method: "turn/start".to_string(),
            params: Some(json!({
                "clientTurnId": "refresh-preview-authority",
                "scope": draft["snapshot"]["scope"],
                "threadId": null,
                "workspaceId": updated["workspace"]["id"],
                "target": { "agentRef": null, "runtimeProfileRef": "native" },
                "input": [{ "type": "text", "text": "refresh roots" }],
                "mentions": [],
                "turnOverrides": { "model": "fake-model" },
                "expectedContextRevision": current_context["contextRevision"],
                "expectedControlRevision": current_context["controlRevision"]
            })),
        },
    )
    .await
    .expect("turn");
    assert_eq!(accepted["accepted"], true);
    let sessions = state
        .inner
        .browser_sessions
        .lock()
        .expect("browser sessions");
    let roots = &sessions
        .get(&browser_session_id)
        .expect("browser session")
        .workspace_preview_roots;
    assert!(roots.contains(&psychevo::host_paths::normalized_native_path(&added)));
    assert!(!roots.contains(&psychevo::host_paths::normalized_native_path(&removed)));
}
