    #[tokio::test]
    async fn workspace_file_rpcs_are_scoped_to_current_project_tree() {
        let (_temp, state) = web_state();
        let src = state.inner.workdir.join("src");
        std::fs::create_dir_all(&src).expect("src");
        std::fs::write(src.join("main.rs"), "fn main() {}\n").expect("main");
        for skipped in [".git", ".local", "target", "node_modules"] {
            let dir = state.inner.workdir.join(skipped);
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

        let err = handle_rpc(
            state,
            AuthContext::Bearer,
            tx,
            RpcRequest {
                jsonrpc: wire::JSONRPC_VERSION.to_string(),
                id: Some(json!("3")),
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
    async fn workspace_diff_rpc_returns_selected_file_diff_preview() {
        let (_temp, state) = web_state();
        git(&state.inner.workdir, ["init"]);
        git(
            &state.inner.workdir,
            ["config", "user.email", "test@example.com"],
        );
        git(&state.inner.workdir, ["config", "user.name", "Test User"]);
        let src = state.inner.workdir.join("src");
        std::fs::create_dir_all(&src).expect("src");
        std::fs::write(src.join("main.rs"), "fn main() {}\n").expect("main");
        git(&state.inner.workdir, ["add", "."]);
        git(&state.inner.workdir, ["commit", "-m", "initial"]);
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
        let src = state.inner.workdir.join("src");
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
        git(&state.inner.workdir, ["init"]);
        git(
            &state.inner.workdir,
            ["config", "user.email", "test@example.com"],
        );
        git(&state.inner.workdir, ["config", "user.name", "Test User"]);
        let path = state.inner.workdir.join("notes.txt");
        std::fs::write(&path, "base\n").expect("base");
        git(&state.inner.workdir, ["add", "."]);
        git(&state.inner.workdir, ["commit", "-m", "initial"]);
        std::fs::write(&path, "user dirty\n").expect("dirty");

        state
            .inner
            .review
            .begin_turn("turn-1", Some("thread-1".to_string()), &state.inner.workdir);
        std::fs::write(&path, "agent changed\n").expect("agent");
        state.inner.review.complete_turn("turn-1");

        let scope = default_resolved_scope(&state, &AuthContext::Bearer)
            .expect("scope")
            .to_wire_scope();
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

        assert_eq!(result["accepted"], true);
        assert_eq!(result["known"], true);
        assert_eq!(result["presentationKind"], "control");
        assert_eq!(result["feedbackAnchor"], "composer");
        assert_eq!(result["action"]["type"], "queuePrompt");
        assert_eq!(result["action"]["text"], "hello");
        assert_eq!(result["action"]["displayText"], "/queue hello");
    }

    #[tokio::test]
    async fn command_execute_undo_redo_restores_session_snapshot() {
        let (_temp, state) = web_state();
        git(&state.inner.workdir, ["init"]);
        let file = state.inner.workdir.join("tracked.txt");
        std::fs::write(&file, "base\n").expect("base");
        let session_id = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &state.inner.workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("session");
        let snapshot_root = state.inner.home.join("snapshots");
        let before_first = track_snapshot(&snapshot_root, &session_id, &state.inner.workdir);
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
        let before_second = track_snapshot(&snapshot_root, &session_id, &state.inner.workdir);
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

        let other_workdir = temp.path().join("other");
        std::fs::create_dir_all(&other_workdir).expect("other workdir");
        let other_session = state
            .inner
            .state
            .store()
            .create_session_with_metadata(
                &other_workdir,
                "web",
                "fake-model",
                "fake-provider",
                None,
            )
            .expect("other session");
        let cross_workdir = handle_rpc(
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
        .expect("command/execute cross workdir");
        assert_eq!(cross_workdir["accepted"], false, "{cross_workdir:#}");
        assert_eq!(cross_workdir["known"], true, "{cross_workdir:#}");
        assert!(cross_workdir["action"].is_null(), "{cross_workdir:#}");
        assert!(
            cross_workdir["message"]
                .as_str()
                .is_some_and(|message| message.contains("does not belong")),
            "{cross_workdir:#}"
        );
    }
