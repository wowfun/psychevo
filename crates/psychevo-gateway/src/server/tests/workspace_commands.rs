#[test]
fn workspace_path_identity_normalizes_verbatim_drive_and_unc_paths() {
    assert_eq!(
        workspace::normalized_workspace_path_identity(Path::new(
            r"\\?\C:\Users\Ada\project\index.html",
        )),
        PathBuf::from(r"C:\Users\Ada\project\index.html")
    );
    assert_eq!(
        workspace::normalized_workspace_path_identity(Path::new(
            r"\\?\UNC\server\share\project\index.html",
        )),
        PathBuf::from(r"\\server\share\project\index.html")
    );
}

#[test]
fn workspace_drive_roots_follow_the_windows_logical_drive_mask() {
    let roots = workspace::windows_drive_roots_from_mask((1 << 2) | (1 << 3) | (1 << 25));

    assert_eq!(
        roots
            .iter()
            .map(|root| (root.name.as_str(), root.path.as_str()))
            .collect::<Vec<_>>(),
        vec![("C:", "C:\\"), ("D:", "D:\\"), ("Z:", "Z:\\")]
    );
}

#[tokio::test]
async fn workspace_external_actions_rpc_classifies_regular_files_without_launching_apps() {
    let (_temp, state) = web_state();
    std::fs::write(state.inner.cwd.join("index.html"), "<main>Hello</main>\n")
        .expect("html fixture");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("external-actions")),
            method: "workspace/file/externalActions".to_string(),
            params: Some(json!({ "scope": scope, "path": "index.html" })),
        },
    )
    .await
    .expect("workspace/file/externalActions");

    assert_eq!(result["path"], "index.html");
    assert_eq!(result["category"], "webpage");
    assert_eq!(result["textLike"], true);
    assert_eq!(result["preferredAction"], "systemDefault");
    assert_eq!(
        result["availableActions"]
            .as_array()
            .and_then(|actions| actions.last()),
        Some(&json!("reveal"))
    );
}

#[tokio::test]
async fn browser_external_file_rpcs_reject_a_scope_outside_the_current_session() {
    let (temp, state) = web_state();
    std::fs::write(state.inner.cwd.join("README.md"), "workspace\n").expect("workspace file");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&outside).expect("outside workspace");
    std::fs::write(outside.join("README.md"), "outside\n").expect("outside file");
    let session_id = "browser-external-scope".to_string();
    state
        .inner
        .browser_sessions
        .lock()
        .expect("browser sessions")
        .insert(
            session_id.clone(),
            BrowserSession::with_external_action_grant(
                state.inner.cwd.clone(),
                state.inner.source.clone(),
            ),
        );
    let outside_scope = wire::GatewayRequestScope {
        cwd: outside.to_string_lossy().to_string(),
        source: wire::GatewaySourceInput {
            kind: "web".to_string(),
            raw_id: Some("outside".to_string()),
            lifetime: None,
            raw_identity: None,
            visible_name: None,
        },
    };
    let auth = AuthContext::Browser { session_id };
    let (tx, _rx) = mpsc::unbounded_channel();

    for (method, params) in [
        (
            "workspace/file/externalActions",
            json!({ "scope": outside_scope.clone(), "path": "README.md" }),
        ),
        (
            "workspace/file/openExternal",
            json!({
                "scope": outside_scope.clone(),
                "path": "README.md",
                "action": "systemDefault"
            }),
        ),
    ] {
        let error = handle_rpc(
            state.clone(),
            auth.clone(),
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(method)),
                method: method.to_string(),
                params: Some(params),
            },
        )
        .await
        .expect_err("browser scope mismatch must be rejected before launch");
        assert!(error.to_string().contains("not authorized"));
    }
}

#[tokio::test]
async fn browser_workspace_external_actions_reject_two_step_ungranted_draft_scope_pivot() {
    let (temp, state) = web_state();
    let arbitrary = temp.path().join("arbitrary-workspace");
    std::fs::create_dir_all(&arbitrary).expect("arbitrary workspace");
    std::fs::write(arbitrary.join("README.md"), "ungranted\n").expect("arbitrary file");
    let arbitrary = canonicalize_cwd(&arbitrary).expect("canonical arbitrary workspace");
    let browser_session_id = "browser-external-pivot".to_string();
    state
        .inner
        .browser_sessions
        .lock()
        .expect("browser sessions")
        .insert(
            browser_session_id.clone(),
            BrowserSession::with_external_action_grant(
                state.inner.cwd.clone(),
                state.inner.source.clone(),
            ),
        );
    let auth = AuthContext::Browser {
        session_id: browser_session_id.clone(),
    };
    let scope = ResolvedScope {
        cwd: arbitrary.clone(),
        source: cwd_source(&arbitrary),
    }
    .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    handle_rpc(
        state.clone(),
        auth.clone(),
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("pivot")),
            method: "thread/draft/open".to_string(),
            params: Some(json!({
                "origin": scope.clone(),
                "targetIntent": { "kind": "default" }
            })),
        },
    )
    .await
    .expect("draft scope pivot updates navigation");
    let session = state
        .inner
        .browser_sessions
        .lock()
        .expect("browser sessions")
        .get(&browser_session_id)
        .cloned()
        .expect("browser session");
    assert_eq!(session.cwd, arbitrary);
    assert!(!session.external_action_grants.contains(&arbitrary));

    let error = handle_rpc(
        state,
        auth,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("external-after-pivot")),
            method: "workspace/file/externalActions".to_string(),
            params: Some(json!({ "scope": scope, "path": "README.md" })),
        },
    )
    .await
    .expect_err("navigation-only draft cwd must remain ungranted");
    assert!(error.to_string().contains("no external-action grant"));
}

#[tokio::test]
async fn browser_source_default_resume_does_not_grant_a_caller_paired_cwd() {
    let (temp, state) = web_state();
    let thread_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(
            &state.inner.cwd,
            "web",
            "fake-model",
            "fake-provider",
            None,
        )
        .expect("stored thread");
    let trusted_scope = ResolvedScope {
        cwd: state.inner.cwd.clone(),
        source: state.inner.source.clone(),
    };
    bind_source_to_thread(&state, &trusted_scope, &thread_id).expect("source binding");
    let arbitrary = temp.path().join("paired-cwd");
    std::fs::create_dir_all(&arbitrary).expect("paired cwd");
    let arbitrary = canonicalize_cwd(&arbitrary).expect("canonical paired cwd");
    let mut caller_scope = trusted_scope.to_wire_scope();
    caller_scope.cwd = arbitrary.display().to_string();
    let browser_session_id = "browser-source-resume-grant".to_string();
    state
        .inner
        .browser_sessions
        .lock()
        .expect("browser sessions")
        .insert(
            browser_session_id.clone(),
            BrowserSession::with_external_action_grant(
                state.inner.cwd.clone(),
                state.inner.source.clone(),
            ),
        );
    let (tx, _rx) = mpsc::unbounded_channel();

    handle_rpc(
        state.clone(),
        AuthContext::Browser {
            session_id: browser_session_id.clone(),
        },
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("source-default-resume")),
            method: "thread/resume".to_string(),
            params: Some(json!({ "scope": caller_scope })),
        },
    )
    .await
    .expect("source-default resume");

    let session = state
        .inner
        .browser_sessions
        .lock()
        .expect("browser sessions")
        .get(&browser_session_id)
        .cloned()
        .expect("browser session");
    assert_eq!(session.cwd, state.inner.cwd);
    assert!(!session.external_action_grants.contains(&arbitrary));
}

#[tokio::test]
async fn workspace_external_actions_reject_directories_and_symlink_escapes() {
    let (temp, state) = web_state();
    std::fs::create_dir_all(state.inner.cwd.join("folder")).expect("folder");
    let outside = temp.path().join("outside.txt");
    std::fs::write(&outside, "secret\n").expect("outside file");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, state.inner.cwd.join("escape.txt")).expect("symlink");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let mut paths = vec!["folder"];
    #[cfg(unix)]
    paths.push("escape.txt");
    for path in paths {
        let error = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(path)),
                method: "workspace/file/externalActions".to_string(),
                params: Some(json!({ "scope": scope.clone(), "path": path })),
            },
        )
        .await
        .expect_err("non-regular or escaped path must be rejected");
        assert!(
            error.to_string().contains("regular file")
                || error.to_string().contains("outside the workspace")
        );
    }
}

#[tokio::test]
async fn workspace_folder_rpc_browses_host_folders_without_a_workspace_root_boundary() {
    let (temp, state) = web_state();
    let root = temp.path().join("workspaces");
    let alpha = root.join("alpha");
    let nested = alpha.join("nested");
    std::fs::create_dir_all(&nested).expect("workspace folders");
    std::fs::create_dir_all(root.join(".local")).expect("normally hidden folder");
    std::fs::write(alpha.join("README.md"), "ignored\n").expect("file");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let root_result = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("folders-1")),
            method: "workspace/folders".to_string(),
            params: Some(json!({ "scope": scope.clone(), "path": root })),
        },
    )
    .await
    .expect("workspace/folders root");
    assert_eq!(
        root_result["current"].as_str(),
        Some(root.to_string_lossy().as_ref())
    );
    assert_eq!(
        root_result["parent"].as_str(),
        Some(temp.path().to_string_lossy().as_ref())
    );
    assert_eq!(root_result["roots"][0]["path"], json!("/"));
    let root_folders = root_result["folders"].as_array().expect("folder array");
    assert!(root_folders.iter().any(|folder| folder["name"] == ".local"));
    assert!(root_folders.iter().any(|folder| folder["name"] == "alpha"));

    let nested_result = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("folders-2")),
            method: "workspace/folders".to_string(),
            params: Some(json!({ "scope": scope.clone(), "path": alpha })),
        },
    )
    .await
    .expect("workspace/folders nested");
    assert_eq!(
        nested_result["parent"].as_str(),
        Some(root.to_string_lossy().as_ref())
    );
    assert_eq!(
        nested_result["folders"][0]["path"].as_str(),
        Some(nested.to_string_lossy().as_ref())
    );

    let outside_result = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("folders-3")),
            method: "workspace/folders".to_string(),
            params: Some(json!({ "scope": scope, "path": temp.path() })),
        },
    )
    .await
    .expect("workspace/folders outside the configured workspace root");
    assert_eq!(
        outside_result["current"].as_str(),
        Some(temp.path().to_string_lossy().as_ref())
    );
}

#[tokio::test]
async fn workspace_git_branch_rpcs_list_switch_and_create_local_branches() {
    let (_temp, state) = web_state();
    git(&state.inner.cwd, ["init", "-b", "main"]);
    git(
        &state.inner.cwd,
        ["config", "user.email", "test@example.com"],
    );
    git(&state.inner.cwd, ["config", "user.name", "Test User"]);
    std::fs::write(state.inner.cwd.join("README.md"), "workspace\n").expect("readme");
    git(&state.inner.cwd, ["add", "."]);
    git(&state.inner.cwd, ["commit", "-m", "initial"]);
    git(&state.inner.cwd, ["branch", "feature/existing"]);
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let listed = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "workspace/git/branches".to_string(),
            params: Some(json!({ "scope": scope.clone() })),
        },
    )
    .await
    .expect("workspace/git/branches");
    assert_eq!(listed["current"].as_str(), Some("main"));
    assert_eq!(
        listed["branches"].as_array().expect("branches"),
        &vec![json!("feature/existing"), json!("main")]
    );

    let switched = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "workspace/git/checkout".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "branch": "feature/existing",
                "create": false
            })),
        },
    )
    .await
    .expect("workspace/git/checkout existing");
    assert_eq!(switched["current"].as_str(), Some("feature/existing"));

    let created = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("3")),
            method: "workspace/git/checkout".to_string(),
            params: Some(json!({
                "scope": scope,
                "branch": "feature/new",
                "create": true
            })),
        },
    )
    .await
    .expect("workspace/git/checkout create");
    assert_eq!(created["current"].as_str(), Some("feature/new"));
    assert!(
        created["branches"]
            .as_array()
            .expect("branches")
            .contains(&json!("feature/new"))
    );
}

#[tokio::test]
async fn workspace_file_rpcs_are_scoped_to_current_project_tree() {
    let (_temp, state) = web_state();
    let src = state.inner.cwd.join("src");
    std::fs::create_dir_all(&src).expect("src");
    std::fs::write(src.join("main.rs"), "fn main() {}\n").expect("main");
    for skipped in [".git", ".local", "target", "node_modules"] {
        let dir = state.inner.cwd.join(skipped);
        std::fs::create_dir_all(&dir).expect("skipped dir");
        std::fs::write(dir.join("hidden.txt"), skipped).expect("hidden");
    }
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "workspace/files".to_string(),
            params: Some(json!({ "scope": scope.clone() })),
        },
    )
    .await
    .expect("workspace/files");

    let paths = result["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .filter_map(|entry| entry["path"].as_str())
        .collect::<Vec<_>>();
    assert!(paths.contains(&"src"));
    assert!(paths.contains(&"src/main.rs"));
    assert!(
        paths.iter().all(|path| !path.starts_with(".git")
            && !path.starts_with(".local")
            && !path.starts_with("target")
            && !path.starts_with("node_modules")),
        "{paths:?}"
    );

    let read = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "workspace/file/read".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "path": "src/main.rs"
            })),
        },
    )
    .await
    .expect("workspace/file/read");
    assert_eq!(read["path"].as_str(), Some("src/main.rs"));
    assert_eq!(read["content"].as_str(), Some("fn main() {}\n"));

    let written = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("3")),
            method: "workspace/file/write".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "path": "src/main.rs",
                "content": "fn main() { println!(\"updated\"); }\n",
                "expectedRevision": read["revision"],
                "force": false
            })),
        },
    )
    .await
    .expect("workspace/file/write existing file");
    assert_eq!(written["path"].as_str(), Some("src/main.rs"));
    assert_eq!(
        std::fs::read_to_string(src.join("main.rs")).expect("updated main"),
        "fn main() { println!(\"updated\"); }\n"
    );

    let err = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("4")),
            method: "workspace/file/read".to_string(),
            params: Some(json!({
                "scope": scope,
                "path": "/etc/passwd"
            })),
        },
    )
    .await
    .expect_err("absolute path should be rejected");
    assert_eq!(err.to_string(), "workspace path must be relative");
}

#[tokio::test]
async fn workspace_file_write_creates_a_new_file_in_an_existing_parent() {
    let (_temp, state) = web_state();
    std::fs::create_dir_all(state.inner.cwd.join("generated")).expect("generated dir");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let written = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "workspace/file/write".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "path": "generated/result.html",
                "content": "<!doctype html><title>Result</title>\n",
                "expectedRevision": "missing",
                "force": false
            })),
        },
    )
    .await
    .expect("workspace/file/write");
    assert_eq!(written["path"].as_str(), Some("generated/result.html"));

    let read = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "workspace/file/read".to_string(),
            params: Some(json!({
                "scope": scope,
                "path": "generated/result.html"
            })),
        },
    )
    .await
    .expect("workspace/file/read");
    assert_eq!(
        read["content"].as_str(),
        Some("<!doctype html><title>Result</title>\n")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn workspace_file_read_and_write_reject_symlink_escapes() {
    let (temp, state) = web_state();
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&outside).expect("outside dir");
    std::fs::write(outside.join("secret.txt"), "outside\n").expect("outside file");
    std::os::unix::fs::symlink(&outside, state.inner.cwd.join("escape"))
        .expect("workspace symlink");
    let dangling_target = outside.join("created-through-symlink.txt");
    std::os::unix::fs::symlink(&dangling_target, state.inner.cwd.join("dangling.txt"))
        .expect("dangling workspace symlink");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    for (id, method, path) in [
        ("1", "workspace/file/read", "escape/secret.txt"),
        ("2", "workspace/file/write", "escape/secret.txt"),
        ("3", "workspace/file/write", "escape/new.txt"),
    ] {
        let params = if method == "workspace/file/read" {
            json!({
                "scope": scope.clone(),
                "path": path
            })
        } else {
            json!({
                "scope": scope.clone(),
                "path": path,
                "content": "blocked\n",
                "expectedRevision": "missing",
                "force": true
            })
        };
        let err = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!(id)),
                method: method.to_string(),
                params: Some(params),
            },
        )
        .await
        .expect_err("symlink escape should be rejected");
        assert_eq!(err.to_string(), "workspace path is outside the workspace");
    }
    assert!(!outside.join("new.txt").exists());

    let err = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("4")),
            method: "workspace/file/write".to_string(),
            params: Some(json!({
                "scope": scope,
                "path": "dangling.txt",
                "content": "blocked\n",
                "expectedRevision": "missing",
                "force": true
            })),
        },
    )
    .await
    .expect_err("dangling final symlink should be rejected");
    assert!(!err.to_string().is_empty());
    assert!(!dangling_target.exists());
}

#[tokio::test]
async fn workspace_diff_rpc_returns_selected_file_diff_preview() {
    let (_temp, state) = web_state();
    git(&state.inner.cwd, ["init"]);
    git(
        &state.inner.cwd,
        ["config", "user.email", "test@example.com"],
    );
    git(&state.inner.cwd, ["config", "user.name", "Test User"]);
    let src = state.inner.cwd.join("src");
    std::fs::create_dir_all(&src).expect("src");
    std::fs::write(src.join("main.rs"), "fn main() {}\n").expect("main");
    git(&state.inner.cwd, ["add", "."]);
    git(&state.inner.cwd, ["commit", "-m", "initial"]);
    std::fs::write(src.join("main.rs"), "fn main() {}\nfn changed() {}\n").expect("main");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "workspace/diff".to_string(),
            params: Some(json!({
                "scope": scope,
                "path": "src/main.rs"
            })),
        },
    )
    .await
    .expect("workspace/diff");

    assert_eq!(result["selectedPath"].as_str(), Some("src/main.rs"));
    assert_eq!(result["files"].as_array().expect("files").len(), 1);
    assert_eq!(result["files"][0]["path"].as_str(), Some("src/main.rs"));
    assert_eq!(result["files"][0]["status"].as_str(), Some("modified"));
    assert!(
        result["unifiedDiff"].as_str().is_some_and(|diff| diff
            .contains("diff --git a/src/main.rs b/src/main.rs")
            && diff.contains("+fn changed() {}")),
        "{result:#}"
    );
}

#[tokio::test]
async fn workspace_file_write_rejects_revision_conflicts_and_allows_force() {
    let (_temp, state) = web_state();
    let src = state.inner.cwd.join("src");
    std::fs::create_dir_all(&src).expect("src");
    std::fs::write(src.join("main.rs"), "fn main() {}\n").expect("main");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let read = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "workspace/file/read".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "path": "src/main.rs"
            })),
        },
    )
    .await
    .expect("workspace/file/read");
    assert_eq!(read["editable"], true, "{read:#}");
    let revision = read["revision"].as_str().expect("revision").to_string();

    std::fs::write(src.join("main.rs"), "fn external() {}\n").expect("external");
    let err = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "workspace/file/write".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "path": "src/main.rs",
                "content": "fn gui() {}\n",
                "expectedRevision": revision,
                "force": false
            })),
        },
    )
    .await
    .expect_err("revision conflict");
    assert_eq!(err.to_string(), "workspace file changed on disk");

    let written = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("3")),
            method: "workspace/file/write".to_string(),
            params: Some(json!({
                "scope": scope,
                "path": "src/main.rs",
                "content": "fn gui() {}\n",
                "expectedRevision": "stale",
                "force": true
            })),
        },
    )
    .await
    .expect("force write");
    assert_eq!(written["path"].as_str(), Some("src/main.rs"));
    assert_eq!(
        std::fs::read_to_string(src.join("main.rs")).expect("main"),
        "fn gui() {}\n"
    );
}

#[tokio::test]
async fn workspace_change_reject_restores_pre_turn_dirty_content() {
    let (_temp, state) = web_state();
    git(&state.inner.cwd, ["init"]);
    git(
        &state.inner.cwd,
        ["config", "user.email", "test@example.com"],
    );
    git(&state.inner.cwd, ["config", "user.name", "Test User"]);
    let path = state.inner.cwd.join("notes.txt");
    std::fs::write(&path, "base\n").expect("base");
    git(&state.inner.cwd, ["add", "."]);
    git(&state.inner.cwd, ["commit", "-m", "initial"]);
    std::fs::write(&path, "user dirty\n").expect("dirty");

    state.record_review_event(
        &GatewayEvent::TurnStarted {
            thread_id: Some("thread-1".to_string()),
            turn_id: "turn-1".to_string(),
            selected_skills: Vec::new(),
        },
        &state.inner.cwd,
    );
    state.inner.review.observe_mutation(
        "turn-1",
        &state.inner.cwd,
        psychevo_runtime::WorkspaceMutation::ExactUtf8 {
            path: "notes.txt".to_string(),
            before: Some("user dirty\n".to_string()),
            after: Some("agent changed\n".to_string()),
        },
    );
    std::fs::write(&path, "agent changed\n").expect("agent");
    std::fs::write(state.inner.cwd.join("unobserved.txt"), "outside observer\n")
        .expect("unobserved");
    state.record_review_event(
        &GatewayEvent::TurnCompleted {
            thread_id: Some("thread-1".to_string()),
            turn_id: "turn-1".to_string(),
            turn: GatewayTurn {
                id: "turn-1".to_string(),
                thread_id: Some("thread-1".to_string()),
                status: GatewayTurnStatus::Completed,
                outcome: Some("normal".to_string()),
                error: None,
                started_at_ms: Some(1),
                completed_at_ms: Some(2),
            },
            committed_entries: Vec::new(),
        },
        &state.inner.cwd,
    );

    let review_scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("review scope");
    let changes = state.inner.review.changes_for_scope(&review_scope);
    assert_eq!(changes.groups.len(), 1);
    assert_eq!(changes.groups[0].files.len(), 1);
    assert_eq!(changes.groups[0].files[0].path, "notes.txt");

    let scope = review_scope.to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();
    let rejected = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "workspace/change/reject".to_string(),
            params: Some(json!({
                "scope": scope,
                "turnId": "turn-1",
                "path": "notes.txt"
            })),
        },
    )
    .await
    .expect("reject");

    assert_eq!(rejected["accepted"], true, "{rejected:#}");
    assert_eq!(
        std::fs::read_to_string(&path).expect("notes"),
        "user dirty\n"
    );
    assert_eq!(
        rejected["changes"]["groups"][0]["files"][0]["reviewStatus"].as_str(),
        Some("rejected"),
        "{rejected:#}"
    );
}

#[test]
fn workspace_review_records_patch_paths_and_opaque_invalidations() {
    let (_temp, state) = web_state();
    let cwd = &state.inner.cwd;
    std::fs::write(cwd.join("update.txt"), "before update\n").expect("update baseline");
    std::fs::write(cwd.join("delete.txt"), "before delete\n").expect("delete baseline");
    std::fs::write(cwd.join("move-from.txt"), "before move\n").expect("move baseline");
    state
        .inner
        .review
        .begin_turn("turn-patch", Some("thread-patch".to_string()), cwd);
    for mutation in [
        psychevo_runtime::WorkspaceMutation::ExactUtf8 {
            path: "add.txt".to_string(),
            before: None,
            after: Some("added\n".to_string()),
        },
        psychevo_runtime::WorkspaceMutation::ExactUtf8 {
            path: "update.txt".to_string(),
            before: Some("before update\n".to_string()),
            after: Some("after update\n".to_string()),
        },
        psychevo_runtime::WorkspaceMutation::ExactUtf8 {
            path: "delete.txt".to_string(),
            before: Some("before delete\n".to_string()),
            after: None,
        },
        psychevo_runtime::WorkspaceMutation::ExactUtf8 {
            path: "move-from.txt".to_string(),
            before: Some("before move\n".to_string()),
            after: None,
        },
        psychevo_runtime::WorkspaceMutation::ExactUtf8 {
            path: "move-to.txt".to_string(),
            before: None,
            after: Some("before move\n".to_string()),
        },
        psychevo_runtime::WorkspaceMutation::Opaque {
            source: "exec_command".to_string(),
        },
        psychevo_runtime::WorkspaceMutation::Opaque {
            source: "acp.edit".to_string(),
        },
    ] {
        state
            .inner
            .review
            .observe_mutation("turn-patch", cwd, mutation);
    }

    std::fs::write(cwd.join("add.txt"), "added\n").expect("add");
    std::fs::write(cwd.join("update.txt"), "after update\n").expect("update");
    std::fs::remove_file(cwd.join("delete.txt")).expect("delete");
    std::fs::rename(cwd.join("move-from.txt"), cwd.join("move-to.txt")).expect("move");
    state.inner.review.complete_turn("turn-patch");

    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let changes = state.inner.review.changes_for_scope(&scope);
    assert_eq!(changes.groups.len(), 1);
    assert_eq!(
        changes.groups[0]
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        vec![
            "add.txt",
            "delete.txt",
            "move-from.txt",
            "move-to.txt",
            "update.txt"
        ]
    );
    assert_eq!(
        changes.groups[0]
            .invalidations
            .iter()
            .map(|invalidation| invalidation.source.as_str())
            .collect::<Vec<_>>(),
        vec!["exec_command", "acp.edit"]
    );
}

#[tokio::test]
async fn completion_list_ranks_dollar_prefix_matches_first() {
    let (_temp, state) = web_state();
    write_project_skill(&state, "x-daily", "Fetch X daily posts.");
    write_project_skill(&state, "explore", "Explore code and X references.");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "completion/list".to_string(),
            params: Some(json!({
                "scope": scope,
                "text": "$x",
                "cursor": 2
            })),
        },
    )
    .await
    .expect("completion/list");

    let labels = result["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(labels.first().copied(), Some("$x-daily"));
    assert!(labels.contains(&"$explore"), "{labels:?}");
    let first = result["items"]
        .as_array()
        .expect("items")
        .first()
        .expect("first item");
    assert_eq!(first["group"], "skills");
    assert_eq!(first["groupLabel"], "Skills");
    assert_eq!(first["scopeLabel"], "Project");
}

#[tokio::test]
async fn command_execute_opens_web_utility_panels() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    for (command, panel) in [
        ("/status", "status"),
        ("/usage", "status"),
        ("/context", "status"),
        ("/help", "commands"),
        ("/commands", "commands"),
        ("/sessions", "history"),
    ] {
        let result = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            tx.clone(),
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("1")),
                method: "command/execute".to_string(),
                params: Some(json!({
                    "scope": scope,
                    "command": command,
                    "threadId": null
                })),
            },
        )
        .await
        .expect("command/execute");

        assert_eq!(result["accepted"], true, "{command}: {result:?}");
        assert_eq!(result["known"], true, "{command}: {result:?}");
        assert_eq!(result["action"]["type"], "showPanel");
        assert_eq!(result["action"]["panel"], panel);
        assert!(result["presentationKind"].as_str().is_some());
        assert!(result["feedbackAnchor"].as_str().is_some());
    }

    let result = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "command/execute".to_string(),
            params: Some(json!({
                "scope": scope,
                "command": "/agents",
                "threadId": null
            })),
        },
    )
    .await
    .expect("command/execute");

    assert_eq!(result["accepted"], false, "{result:?}");
    assert_eq!(result["known"], true, "{result:?}");
    assert!(result["action"].is_null(), "{result:?}");
    assert_eq!(
        result["message"],
        "/agents is managed by the Workbench agent selector and Settings Agents."
    );
}

#[tokio::test]
async fn command_execute_queue_preserves_original_slash_display_text() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "command/execute".to_string(),
            params: Some(json!({
                "scope": scope,
                "command": "/queue hello",
                "threadId": null
            })),
        },
    )
    .await
    .expect("command/execute");

    if result["accepted"] != true {
        panic!("unexpected compact result: {result:#}");
    }
    assert_eq!(result["known"], true);
    assert_eq!(result["presentationKind"], "control");
    assert_eq!(result["feedbackAnchor"], "composer");
    assert_eq!(result["action"]["type"], "queuePrompt");
    assert_eq!(result["action"]["text"], "hello");
    assert_eq!(result["action"]["displayText"], "/queue hello");
}

#[tokio::test]
async fn command_execute_compact_returns_native_compaction_action() {
    let (_temp, state) = web_state();
    let session_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&state.inner.cwd, "web", "fake-model", "fake", None)
        .expect("session");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "command/execute".to_string(),
            params: Some(json!({
                "scope": scope,
                "command": "/compact keep decisions",
                "threadId": session_id
            })),
        },
    )
    .await
    .expect("command/execute");

    assert_eq!(result["accepted"], true, "{result:#}");
    assert_eq!(result["known"], true);
    assert_eq!(result["action"]["type"], "threadCompactStart");
    assert_eq!(result["action"]["instructions"], "keep decisions");
}

#[tokio::test]
async fn thread_action_compact_returns_structured_noop_without_prompt_turn() {
    let (_temp, state) = web_state();
    let session_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&state.inner.cwd, "web", "fake-model", "fake", None)
        .expect("session");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let profile = generated_runtime_profiles()
        .into_iter()
        .find(|profile| profile.id == "native")
        .expect("Native profile");
    let profile_json = serde_json::to_string(&profile).expect("profile snapshot");
    let profile_fingerprint = crate::runtime_profile_config_fingerprint(&profile);
    let profile_revision = crate::runtime_profile_config_revision(&profile_fingerprint).to_string();
    let agent_fingerprint = crate::gateway_agent_definition_fingerprint("null");
    let cwd = state.inner.cwd.display().to_string();
    state
        .inner
        .state
        .store()
        .create_gateway_runtime_binding(psychevo_runtime::GatewayRuntimeBindingInput {
            thread_id: &session_id,
            agent_ref: None,
            agent_fingerprint: &agent_fingerprint,
            agent_definition_json: "null",
            runtime_ref: "native",
            backend_kind: "native",
            native_kind: "native",
            native_session_id: Some(&session_id),
            cwd: &cwd,
            profile_fingerprint: &profile_fingerprint,
            profile_revision: &profile_revision,
            profile_config_json: &profile_json,
            adapter_kind: "native",
            adapter_revision: "test",
            ownership: GatewayRuntimeBindingOwnership::ReadWrite,
            parent_thread_id: None,
        })
        .expect("binding");
    let scope = scope.to_wire_scope();
    let (tx, mut rx) = mpsc::unbounded_channel();

    let result = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "thread/action/run".to_string(),
            params: Some(json!({
                "scope": scope,
                "threadId": session_id,
                "action": {
                    "kind": "compact",
                    "instructions": "keep decisions"
                }
            })),
        },
    )
    .await
    .expect("thread/action/run compact");

    assert_eq!(result["kind"], "compact");
    assert_eq!(result["threadId"], session_id);
    assert_eq!(result["result"]["accepted"], true);
    assert_eq!(result["result"]["compacted"], false);
    assert_eq!(result["result"]["reason"], "manual");
    assert_eq!(
        result["result"]["message"],
        "not enough messages to compact"
    );
    assert!(result["result"]["checkpoint"].is_null());

    let mut activity_running_states = Vec::new();
    while let Ok(message) = rx.try_recv() {
        let value: serde_json::Value =
            serde_json::from_str(&message).expect("gateway notification json");
        if value["method"] == "gateway/event"
            && value["params"]["type"] == "activityChanged"
            && value["params"]["threadId"] == session_id
        {
            activity_running_states.push(value["params"]["activity"]["running"].clone());
        }
    }
    assert_eq!(
        activity_running_states,
        vec![json!(true), json!(false)],
        "activity notifications should bracket compact execution"
    );
}

#[tokio::test]
async fn thread_action_compact_ignores_legacy_source_runtime_evidence_without_binding() {
    let (_temp, state) = web_state();
    let session_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&state.inner.cwd, "web", "fake-model", "fake", None)
        .expect("session");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    state
        .inner
        .state
        .store()
        .upsert_gateway_source_binding(psychevo_runtime::GatewaySourceBindingInput {
            source_key: "legacy:test-lane",
            source_kind: "legacy",
            raw_identity: json!({"lane": "test-lane"}),
            visible_name: Some("Legacy test lane"),
            thread_id: &session_id,
            backend_kind: "acp",
            backend_native_id: Some("retired-native-session"),
            lineage: Some(json!({"runtimeRef": "codex"})),
        })
        .expect("legacy source-row evidence");
    assert!(
        state
            .inner
            .state
            .store()
            .gateway_runtime_binding(&session_id)
            .expect("runtime binding lookup")
            .is_none(),
        "the Thread remains unbound"
    );

    let (tx, _rx) = mpsc::unbounded_channel();
    let result = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "thread/action/run".to_string(),
            params: Some(json!({
                "scope": scope.to_wire_scope(),
                "threadId": session_id,
                "action": { "kind": "compact" }
            })),
        },
    )
    .await
    .expect("thread/action/run compact");

    assert_eq!(result["kind"], "compact");
    assert_eq!(result["result"]["accepted"], true);
    assert_eq!(result["result"]["compacted"], false);
    assert_eq!(result["result"]["reason"], "manual");
    assert_eq!(
        result["result"]["message"],
        "not enough messages to compact"
    );
}

#[tokio::test]
async fn thread_transcript_projects_compaction_checkpoint_divider() {
    let (_temp, state) = web_state();
    let session_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&state.inner.cwd, "web", "fake-model", "fake", None)
        .expect("session");
    let store = state.inner.state.store();
    store
        .append_message(&session_id, &runtime_user_message("first task", 1))
        .expect("user message");
    store
        .append_message(&session_id, &runtime_assistant_message("done", 2))
        .expect("assistant message");
    let record = store
        .append_session_compaction(psychevo_runtime::SessionCompactionInput {
            session_id: session_id.clone(),
            reason: "manual".to_string(),
            summary_text: "Keep the decision trail.".to_string(),
            first_kept_session_seq: 2,
            created_after_session_seq: 2,
            tokens_before: Some(120),
            tokens_after: Some(42),
            summary_provider: "fake".to_string(),
            summary_model: "fake-model".to_string(),
            instructions: Some("keep decisions".to_string()),
            metadata: Some(json!({"test": true})),
        })
        .expect("compaction");

    let entries = state
        .inner
        .gateway
        .thread_transcript(&session_id)
        .expect("transcript");
    let divider = entries
        .iter()
        .find(|entry| entry.id == format!("compaction:{}", record.id))
        .expect("compaction divider");
    assert_eq!(divider.role, TranscriptEntryRole::Diagnostic);
    assert_eq!(divider.blocks[0].kind, TranscriptBlockKind::Compaction);
    assert_eq!(
        divider.blocks[0].title.as_deref(),
        Some("Session compacted")
    );
    assert_eq!(
        divider.blocks[0].detail.as_deref(),
        Some("Keep the decision trail.")
    );
    assert_eq!(
        divider.blocks[0]
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("checkpoint_id"))
            .and_then(Value::as_i64),
        Some(record.id)
    );
}

#[tokio::test]
async fn command_execute_mission_records_team_metadata_and_returns_thread() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("seed")),
            method: "team/write".to_string(),
            params: Some(json!({
                "scope": scope.clone(),
                "name": "ship",
                "target": "project",
                "description": "Ship changes",
                "leader": "general",
                "members": [{"id": "researcher", "agent": "general"}],
                "instructions": "Coordinate shipping."
            })),
        },
    )
    .await
    .expect("team/write");

    let result = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "command/execute".to_string(),
            params: Some(json!({
                "scope": scope,
                "command": "/mission --team ship implement feature",
                "threadId": null
            })),
        },
    )
    .await
    .expect("command/execute mission");

    assert_eq!(result["accepted"], true);
    assert_eq!(result["action"]["type"], "submitPrompt");
    assert_eq!(
        result["action"]["displayText"],
        "/mission --team ship implement feature"
    );
    assert!(
        result["action"]["text"]
            .as_str()
            .expect("mission prompt")
            .contains("Team template: ship")
    );
    let thread_id = result["action"]["threadId"]
        .as_str()
        .expect("thread id")
        .to_string();
    let team = state
        .inner
        .state
        .store()
        .find_active_agent_team_run(&thread_id)
        .expect("team")
        .expect("active team");
    let mission = state
        .inner
        .state
        .store()
        .find_active_agent_mission_run(&thread_id)
        .expect("mission")
        .expect("active mission");
    assert_eq!(team.team_name, "ship");
    assert_eq!(mission.goal, "implement feature");
    assert_eq!(mission.team_run_id.as_deref(), Some(team.id.as_str()));
}

#[tokio::test]
async fn command_execute_btw_creates_side_chat_session() {
    let (_temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let parent_session = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&state.inner.cwd, "web", "fake-model", "fake-provider", None)
        .expect("parent session");
    state
        .inner
        .state
        .store()
        .append_message(&parent_session, &runtime_user_message("parent prompt", 1))
        .expect("parent message");
    let (tx, _rx) = mpsc::unbounded_channel();

    let no_thread = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "command/execute".to_string(),
            params: Some(json!({
                "scope": scope,
                "command": "/btw",
                "threadId": null
            })),
        },
    )
    .await
    .expect("command/execute no thread");
    assert_eq!(no_thread["accepted"], false, "{no_thread:#}");
    assert_eq!(no_thread["known"], true, "{no_thread:#}");
    assert_eq!(
        no_thread["message"],
        "'/btw' is unavailable until the current conversation has started. Send a message first, then try /btw again."
    );

    let unbound = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("unbound")),
            method: "command/execute".to_string(),
            params: Some(json!({
                "scope": scope,
                "command": "/btw explain this",
                "threadId": parent_session.clone()
            })),
        },
    )
    .await
    .expect("command/execute unbound btw");
    assert_eq!(unbound["accepted"], false, "{unbound:#}");
    assert_eq!(
        unbound["message"],
        "Select an Agent target before starting a side chat."
    );

    let parent_binding = bind_native_runtime_to_thread(&state, &parent_session);

    let result = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "command/execute".to_string(),
            params: Some(json!({
                "scope": scope,
                "command": "/btw explain this",
                "threadId": parent_session.clone()
            })),
        },
    )
    .await
    .expect("command/execute btw");

    assert_eq!(result["accepted"], true, "{result:#}");
    assert_eq!(result["known"], true, "{result:#}");
    assert_eq!(result["action"]["type"], "sideConversationStart");
    assert_eq!(result["action"]["parentThreadId"], parent_session);
    assert_eq!(result["action"]["prompt"], "explain this");
    assert_eq!(result["action"]["title"], "Side chat");
    let side_thread_id = result["action"]["threadId"]
        .as_str()
        .expect("side thread id");
    assert_ne!(side_thread_id, parent_session);

    let side_summary = state
        .inner
        .state
        .store()
        .session_summary(side_thread_id)
        .expect("summary")
        .expect("side chat");
    assert_eq!(
        side_summary.parent_session_id.as_deref(),
        Some(parent_session.as_str())
    );
    assert_eq!(side_summary.source, "web-side-conversation");
    assert_eq!(side_summary.model, "fake-model");
    assert_eq!(side_summary.provider, "fake-provider");
    let side_metadata = state
        .inner
        .state
        .store()
        .session_metadata(side_thread_id)
        .expect("metadata")
        .expect("metadata value");
    assert_eq!(
        side_metadata["side_conversation"]["parent_session_id"].as_str(),
        Some(parent_session.as_str())
    );
    assert_eq!(
        side_metadata["side_conversation"]["ephemeral"].as_bool(),
        Some(true)
    );
    let side_binding = state
        .inner
        .state
        .store()
        .gateway_runtime_binding(side_thread_id)
        .expect("side binding")
        .expect("resolved side binding");
    assert_eq!(side_binding.status, GatewayRuntimeBindingStatus::Resolved);
    assert_eq!(side_binding.agent_ref, parent_binding.agent_ref);
    assert_eq!(side_binding.runtime_ref, parent_binding.runtime_ref);
    assert_eq!(
        side_binding.profile_fingerprint,
        parent_binding.profile_fingerprint
    );
    assert_eq!(side_binding.native_session_id, None);
    assert_eq!(side_binding.parent_thread_id, None);
}

#[tokio::test]
async fn command_execute_btw_snapshots_live_effective_acp_controls() {
    let (_temp, state) = web_state();
    let resolved_scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let parent_session = state
        .inner
        .state
        .store()
        .create_session_with_metadata(
            &state.inner.cwd,
            "web",
            "stale-summary-model",
            "fake-provider",
            None,
        )
        .expect("parent session");
    bind_persisted_acp_runtime_to_thread(&state, &parent_session);
    let (tx, _rx) = mpsc::unbounded_channel();

    let parent_context = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("parent-context")),
            method: "thread/context/read".to_string(),
            params: Some(json!({
                "scope": resolved_scope.to_wire_scope(),
                "threadId": parent_session,
                "target": null
            })),
        },
    )
    .await
    .expect("parent Thread Context");
    let effective_controls = parent_context["controls"]
        .as_array()
        .expect("parent controls")
        .iter()
        .filter_map(|control| {
            Some((
                control["id"].as_str()?.to_string(),
                control["effectiveValue"].clone(),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(effective_controls["model"], json!("live-acp-model"));
    assert_eq!(effective_controls["mode"], json!("plan"));

    let result = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("btw")),
            method: "command/execute".to_string(),
            params: Some(json!({
                "scope": resolved_scope.to_wire_scope(),
                "command": "/btw",
                "threadId": parent_session
            })),
        },
    )
    .await
    .expect("command/execute btw");
    let side_thread_id = result["action"]["threadId"]
        .as_str()
        .expect("side thread id");
    let side_binding = state
        .inner
        .state
        .store()
        .gateway_runtime_binding(side_thread_id)
        .expect("side binding")
        .expect("resolved side binding");

    assert_eq!(side_binding.thread_preferences, effective_controls);
    assert!(side_binding.runtime_observed.is_empty());
    assert_eq!(side_binding.native_session_id, None);
}

#[tokio::test]
async fn side_chat_turn_does_not_rebind_current_source_and_can_be_deleted() {
    let backend = Arc::new(AutomationFakeBackend::default());
    let (_temp, state) = web_state_with_automation_backend(backend);
    let resolved_scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
    let scope = resolved_scope.to_wire_scope();
    let parent_session = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&state.inner.cwd, "web", "fake-model", "fake-provider", None)
        .expect("parent session");
    state
        .inner
        .state
        .store()
        .append_message(&parent_session, &runtime_user_message("parent prompt", 1))
        .expect("parent message");
    bind_source_to_thread(&state, &resolved_scope, &parent_session).expect("bind parent source");
    let parent_binding = bind_native_runtime_to_thread(&state, &parent_session);
    let mut parent_preferences = BTreeMap::new();
    parent_preferences.insert("mode".to_string(), json!("plan"));
    let mut parent_observed = BTreeMap::new();
    parent_observed.insert("model".to_string(), json!("fake-model"));
    state
        .inner
        .state
        .store()
        .compare_and_set_gateway_runtime_control_state(
            &parent_session,
            parent_binding.binding_revision,
            parent_binding.control_revision,
            GatewayRuntimeControlStatePatch {
                thread_preferences: Some(&parent_preferences),
                runtime_observed: Some(&parent_observed),
            },
        )
        .expect("parent control state");
    let (tx, mut rx) = mpsc::unbounded_channel();

    let result = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "command/execute".to_string(),
            params: Some(json!({
                "scope": scope,
                "command": "/btw explain this",
                "threadId": parent_session.clone()
            })),
        },
    )
    .await
    .expect("command/execute btw");
    let side_thread_id = result["action"]["threadId"]
        .as_str()
        .expect("side thread id")
        .to_string();
    let context = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("side-context")),
            method: "thread/context/read".to_string(),
            params: Some(json!({
                "scope": resolved_scope.to_wire_scope(),
                "threadId": side_thread_id,
                "target": null
            })),
        },
    )
    .await
    .expect("side Thread Context");
    assert_eq!(context["selectionState"], "bound", "{context:#}");
    assert_eq!(context["runtimeProfileRef"], "native", "{context:#}");
    assert_eq!(context["binding"]["threadId"], side_thread_id);
    assert_eq!(context["sendability"]["allowed"], true, "{context:#}");
    let side_binding = state
        .inner
        .state
        .store()
        .gateway_runtime_binding(&side_thread_id)
        .expect("side binding")
        .expect("resolved side binding");
    assert_eq!(side_binding.thread_preferences["mode"], json!("plan"));
    assert_eq!(
        side_binding.thread_preferences["model"],
        json!("fake-model")
    );
    assert!(side_binding.runtime_observed.is_empty());
    assert_eq!(side_binding.native_session_id, None);

    let accepted = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "turn/start".to_string(),
            params: Some(json!({
                "clientTurnId": "client-side-thread-follow-up",
                "scope": resolved_scope.to_wire_scope(),
                "threadId": side_thread_id.clone(),
                "input": [{"type": "text", "text": "explain this"}],
                "mentions": [],
                "turnOverrides": {"model": "fake-model"},
                "expectedContextRevision": context["contextRevision"],
                "expectedControlRevision": context["controlRevision"]
            })),
        },
    )
    .await
    .expect("turn/start");
    assert_eq!(accepted["accepted"], true);

    let turn_terminal = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while let Some(message) = rx.recv().await {
            if message.contains("\"type\":\"turnCompleted\"") {
                return message;
            }
        }
        panic!("turnCompleted notification channel closed");
    })
    .await
    .expect("turnCompleted notification");
    assert!(turn_terminal.contains(&side_thread_id), "{turn_terminal}");
    assert_eq!(
        state
            .inner
            .gateway
            .resolve_source_thread(&resolved_scope.source)
            .expect("source binding")
            .as_deref(),
        Some(parent_session.as_str())
    );

    let deleted = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("3")),
            method: "thread/delete".to_string(),
            params: Some(json!({ "threadId": side_thread_id.clone() })),
        },
    )
    .await
    .expect("delete side thread");
    assert_eq!(deleted["deleted"], true);
    assert!(
        state
            .inner
            .state
            .store()
            .session_summary(&side_thread_id)
            .expect("side summary")
            .is_none()
    );
}

fn bind_native_runtime_to_thread(state: &WebState, thread_id: &str) -> GatewayRuntimeBindingRecord {
    let profile = generated_runtime_profiles()
        .into_iter()
        .find(|profile| profile.id == "native")
        .expect("Native profile");
    let profile_json = serde_json::to_string(&profile).expect("profile snapshot");
    let profile_fingerprint = crate::runtime_profile_config_fingerprint(&profile);
    let profile_revision = crate::runtime_profile_config_revision(&profile_fingerprint).to_string();
    let agent_fingerprint = crate::gateway_agent_definition_fingerprint("null");
    let cwd = state.inner.cwd.display().to_string();
    state
        .inner
        .state
        .store()
        .create_gateway_runtime_binding(psychevo_runtime::GatewayRuntimeBindingInput {
            thread_id,
            agent_ref: None,
            agent_fingerprint: &agent_fingerprint,
            agent_definition_json: "null",
            runtime_ref: "native",
            backend_kind: "native",
            native_kind: "native",
            native_session_id: Some(thread_id),
            cwd: &cwd,
            profile_fingerprint: &profile_fingerprint,
            profile_revision: &profile_revision,
            profile_config_json: &profile_json,
            adapter_kind: "native",
            adapter_revision: "test",
            ownership: GatewayRuntimeBindingOwnership::ReadWrite,
            parent_thread_id: None,
        })
        .expect("native runtime binding")
}

fn bind_persisted_acp_runtime_to_thread(
    state: &WebState,
    thread_id: &str,
) -> GatewayRuntimeBindingRecord {
    std::fs::create_dir_all(&state.inner.home).expect("profile home");
    let executable = std::env::current_exe().expect("test executable");
    std::fs::write(
        state.inner.home.join("config.toml"),
        format!(
            r#"[agents.backends.ephemeral]
kind = "acp"
label = "Ephemeral"
command = {}
entrypoints = ["peer"]

[runtime_profiles.ephemeral]
runtime = "acp"
enabled = true
label = "Ephemeral ACP"
backend_ref = "ephemeral"
default_model = "profile-default-model"
default_mode = "default"
"#,
            serde_json::to_string(&executable.to_string_lossy()).expect("test executable path")
        ),
    )
    .expect("ACP profile config");
    let profile = RuntimeProfileConfig {
        id: "ephemeral".to_string(),
        runtime: RuntimeProfileKind::Acp,
        enabled: true,
        label: "Ephemeral ACP".to_string(),
        backend_ref: Some("ephemeral".to_string()),
        default_model: Some("profile-default-model".to_string()),
        default_mode: Some("default".to_string()),
        default_agent: None,
        approval_mode: None,
        sandbox: None,
        workspace_roots: Vec::new(),
        options: Value::Null,
    };
    let profile_json = serde_json::to_string(&profile).expect("profile snapshot");
    let profile_fingerprint = crate::runtime_profile_config_fingerprint(&profile);
    let profile_revision = crate::runtime_profile_config_revision(&profile_fingerprint).to_string();
    let agent_json = r#"{"name":"ephemeral","instructions":"captured"}"#;
    let agent_fingerprint = crate::gateway_agent_definition_fingerprint(agent_json);
    let cwd = state.inner.cwd.display().to_string();
    let binding = state
        .inner
        .state
        .store()
        .create_gateway_runtime_binding(psychevo_runtime::GatewayRuntimeBindingInput {
            thread_id,
            agent_ref: Some("ephemeral"),
            agent_fingerprint: &agent_fingerprint,
            agent_definition_json: agent_json,
            runtime_ref: "ephemeral",
            backend_kind: "acp",
            native_kind: "acp",
            native_session_id: Some("ephemeral-native-1"),
            cwd: &cwd,
            profile_fingerprint: &profile_fingerprint,
            profile_revision: &profile_revision,
            profile_config_json: &profile_json,
            adapter_kind: "acp",
            adapter_revision: "test",
            ownership: GatewayRuntimeBindingOwnership::ReadWrite,
            parent_thread_id: None,
        })
        .expect("ACP runtime binding");
    let persisted_projection = crate::acp_peer::AcpSessionSnapshot {
        native_session_id: "ephemeral-native-1".to_string(),
        agent: Some(crate::acp_peer::AcpAgentIdentitySnapshot {
            name: "ephemeral-test".to_string(),
            title: Some("Ephemeral".to_string()),
            version: "1.0.0".to_string(),
        }),
        capabilities: crate::acp_peer::AcpNegotiatedCapabilitiesSnapshot {
            prompt_input: crate::acp_peer::AcpPromptInputCapabilitiesSnapshot {
                text: true,
                image: false,
                audio: false,
                resource: false,
                resource_link: false,
                embedded_context: true,
            },
            session: crate::acp_peer::AcpSessionLifecycleCapabilitiesSnapshot {
                load: true,
                list: false,
                delete: false,
                fork: false,
                resume: true,
                close: false,
                additional_directories: false,
            },
            auth_logout: false,
            auth_methods: Vec::new(),
            providers: false,
            mcp_http: false,
            mcp_sse: false,
            mcp_acp: false,
        },
        options: vec![wire::RuntimeConfigOptionView {
            id: "model".to_string(),
            name: "Model".to_string(),
            description: None,
            category: Some("model".to_string()),
            option_type: "select".to_string(),
            current_value: Some("live-acp-model".to_string()),
            values: vec![wire::RuntimeConfigOptionValueView {
                value: "live-acp-model".to_string(),
                name: "Live ACP Model".to_string(),
                description: None,
                group: None,
            }],
        }],
        available_commands: Vec::new(),
        available_modes: vec![crate::acp_peer::AcpSessionModeSnapshot {
            id: "plan".to_string(),
            name: "Plan".to_string(),
            description: None,
        }],
        current_mode_id: Some("plan".to_string()),
        legacy_models: None,
        history: crate::acp_peer::AcpHistorySnapshot {
            owner: crate::acp_peer::AcpHistoryOwnerSnapshot::Agent,
            resumable: true,
            load_supported: true,
            resume_supported: true,
            loaded_from_agent: true,
            replay_complete: true,
            replay_update_count: 0,
            live_update_count: 0,
        },
        session_info: crate::acp_peer::AcpSessionInfoSnapshot::default(),
        generation: 1,
        session_epoch: 1,
        control_revision: "live-controls".to_string(),
        projection_revision: "live-projection".to_string(),
    };
    state
        .inner
        .state
        .store()
        .set_session_metadata_field(
            thread_id,
            ACP_PEER_METADATA_KEY,
            Some(json!({
                "agentName": "ephemeral",
                "backendId": "ephemeral",
                "backendKind": "acp",
                "nativeSessionId": "ephemeral-native-1",
                "sessionProjection": persisted_projection,
            })),
        )
        .expect("persist ACP projection");
    binding
}

#[tokio::test]
async fn command_execute_undo_redo_restores_session_snapshot() {
    let (_temp, state) = web_state();
    git(&state.inner.cwd, ["init"]);
    let file = state.inner.cwd.join("tracked.txt");
    std::fs::write(&file, "base\n").expect("base");
    let session_id = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&state.inner.cwd, "web", "fake-model", "fake-provider", None)
        .expect("session");
    let snapshot_root = state.inner.home.join("snapshots");
    let before_first = track_snapshot(&snapshot_root, &state.inner.cwd);
    state
        .inner
        .state
        .store()
        .append_message_with_undo_snapshot(
            &session_id,
            &runtime_user_message("first prompt", 1),
            Some(before_first),
        )
        .expect("first user");
    std::fs::write(&file, "after first\n").expect("after first");
    state
        .inner
        .state
        .store()
        .append_message(&session_id, &runtime_assistant_message("first answer", 2))
        .expect("first assistant");
    let before_second = track_snapshot(&snapshot_root, &state.inner.cwd);
    state
        .inner
        .state
        .store()
        .append_message_with_undo_snapshot(
            &session_id,
            &runtime_user_message("second prompt", 3),
            Some(before_second),
        )
        .expect("second user");
    std::fs::write(&file, "after second\n").expect("after second");
    state
        .inner
        .state
        .store()
        .append_message(&session_id, &runtime_assistant_message("second answer", 4))
        .expect("second assistant");
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let undo = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "command/execute".to_string(),
            params: Some(json!({
                "scope": scope,
                "command": "/undo",
                "threadId": session_id
            })),
        },
    )
    .await
    .expect("command/execute undo");

    assert_eq!(undo["accepted"], true, "{undo:#}");
    assert_eq!(undo["known"], true, "{undo:#}");
    assert_eq!(undo["action"]["type"], "sessionUndo");
    assert_eq!(undo["action"]["threadId"], session_id);
    assert_eq!(undo["action"]["prompt"], "second prompt");
    assert_eq!(undo["action"]["revertedMessages"], 2);
    assert_eq!(
        std::fs::read_to_string(&file).expect("file"),
        "after first\n"
    );
    assert_eq!(
        state
            .inner
            .state
            .store()
            .load_tui_message_summaries(&session_id)
            .expect("visible")
            .len(),
        2
    );

    let redo = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "command/execute".to_string(),
            params: Some(json!({
                "scope": scope,
                "command": "/redo",
                "threadId": session_id
            })),
        },
    )
    .await
    .expect("command/execute redo");

    assert_eq!(redo["accepted"], true, "{redo:#}");
    assert_eq!(redo["known"], true, "{redo:#}");
    assert_eq!(redo["action"]["type"], "sessionRedo");
    assert_eq!(redo["action"]["threadId"], session_id);
    assert_eq!(redo["action"]["restoredMessages"], 2);
    assert_eq!(redo["action"]["complete"], true);
    assert_eq!(
        std::fs::read_to_string(&file).expect("file"),
        "after second\n"
    );
    assert_eq!(
        state
            .inner
            .state
            .store()
            .load_tui_message_summaries(&session_id)
            .expect("visible")
            .len(),
        4
    );
}

#[tokio::test]
async fn command_execute_undo_redo_bounded_without_matching_session() {
    let (temp, state) = web_state();
    let scope = default_resolved_scope(&state, &AuthContext::Bearer)
        .expect("scope")
        .to_wire_scope();
    let (tx, _rx) = mpsc::unbounded_channel();

    let no_thread = handle_rpc(
        state.clone(),
        AuthContext::Bearer,
        tx.clone(),
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("1")),
            method: "command/execute".to_string(),
            params: Some(json!({
                "scope": scope,
                "command": "/undo",
                "threadId": null
            })),
        },
    )
    .await
    .expect("command/execute no thread");
    assert_eq!(no_thread["accepted"], false, "{no_thread:#}");
    assert_eq!(no_thread["known"], true, "{no_thread:#}");
    assert!(no_thread["action"].is_null(), "{no_thread:#}");
    assert_eq!(no_thread["message"], "no current session to undo");

    let other_cwd = temp.path().join("other");
    std::fs::create_dir_all(&other_cwd).expect("other cwd");
    let other_session = state
        .inner
        .state
        .store()
        .create_session_with_metadata(&other_cwd, "web", "fake-model", "fake-provider", None)
        .expect("other session");
    let cross_cwd = handle_rpc(
        state,
        AuthContext::Bearer,
        tx,
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!("2")),
            method: "command/execute".to_string(),
            params: Some(json!({
                "scope": scope,
                "command": "/redo",
                "threadId": other_session
            })),
        },
    )
    .await
    .expect("command/execute cross cwd");
    assert_eq!(cross_cwd["accepted"], false, "{cross_cwd:#}");
    assert_eq!(cross_cwd["known"], true, "{cross_cwd:#}");
    assert!(cross_cwd["action"].is_null(), "{cross_cwd:#}");
    assert!(
        cross_cwd["message"]
            .as_str()
            .is_some_and(|message| message.contains("does not belong")),
        "{cross_cwd:#}"
    );
}
