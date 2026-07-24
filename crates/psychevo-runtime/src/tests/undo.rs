#[allow(unused_imports)]
pub(crate) use super::*;
#[test]
pub(crate) fn undo_redo_restore_git_snapshots_and_visible_message_ranges() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&cwd)
            .arg("init")
            .output()
            .expect("git init")
            .status
            .success()
    );
    let file = cwd.join("tracked.txt");
    fs::write(&file, "base\n").expect("base");

    let store = StateRuntime::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .expect("session");
    let snapshots = SnapshotStore::new(temp.path().join("snapshots"), cwd.clone());
    let before_first = snapshots
        .track()
        .expect("track first")
        .expect("first snapshot");
    assert!(snapshots.git_dir().expect("git dir").join("HEAD").exists());
    assert!(!temp.path().join("snapshots").join("sessions").exists());
    store
        .append_message_with_undo_snapshot(
            &session_id,
            &user_message("first prompt", 1),
            Some(before_first),
        )
        .expect("user first");
    fs::write(&file, "after first\n").expect("after first");
    store
        .append_message(&session_id, &assistant_message("first answer", 2))
        .expect("assistant first");
    let before_second = snapshots
        .track()
        .expect("track second")
        .expect("second snapshot");
    store
        .append_message_with_undo_snapshot(
            &session_id,
            &user_message("second prompt", 3),
            Some(before_second),
        )
        .expect("user second");
    fs::write(&file, "after second\n").expect("after second");
    store
        .append_message(&session_id, &assistant_message("second answer", 4))
        .expect("assistant second");

    let options = SessionUndoOptions {
        state: StateRuntime::open(&db).expect("state runtime"),
        cwd: cwd.clone(),
        snapshot_root: temp.path().join("snapshots"),
        session_id: session_id.clone(),
    };
    let undo = undo_session(options.clone()).expect("undo latest");
    assert_eq!(undo.prompt, "second prompt");
    assert_eq!(fs::read_to_string(&file).expect("file"), "after first\n");
    assert_eq!(
        store
            .load_tui_message_summaries(&session_id)
            .expect("visible")
            .len(),
        2
    );

    let undo = undo_session(options.clone()).expect("undo previous");
    assert_eq!(undo.prompt, "first prompt");
    assert_eq!(fs::read_to_string(&file).expect("file"), "base\n");
    assert_eq!(
        store
            .load_tui_message_summaries(&session_id)
            .expect("visible")
            .len(),
        0
    );

    let redo = redo_session(options.clone()).expect("redo first");
    assert!(!redo.complete);
    assert_eq!(fs::read_to_string(&file).expect("file"), "after first\n");
    assert_eq!(
        store
            .load_tui_message_summaries(&session_id)
            .expect("visible")
            .len(),
        2
    );

    let redo = redo_session(options).expect("redo complete");
    assert!(redo.complete);
    assert_eq!(fs::read_to_string(&file).expect("file"), "after second\n");
    assert_eq!(
        store
            .load_tui_message_summaries(&session_id)
            .expect("visible")
            .len(),
        4
    );
    assert!(
        store
            .session_revert_state(&session_id)
            .expect("revert state")
            .is_none()
    );
}

#[test]
pub(crate) fn cleanup_reverted_messages_deletes_hidden_range() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&cwd)
            .arg("init")
            .output()
            .expect("git init")
            .status
            .success()
    );
    let file = cwd.join("tracked.txt");
    fs::write(&file, "base\n").expect("base");
    let store = StateRuntime::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .expect("session");
    let snapshots = SnapshotStore::new(temp.path().join("snapshots"), cwd.clone());
    let before_first = snapshots
        .track()
        .expect("track first")
        .expect("first snapshot");
    store
        .append_message_with_undo_snapshot(
            &session_id,
            &user_message("first prompt", 1),
            Some(before_first),
        )
        .expect("user first");
    fs::write(&file, "after first\n").expect("after first");
    store
        .append_message(&session_id, &assistant_message("first answer", 2))
        .expect("assistant first");
    let before_second = snapshots
        .track()
        .expect("track second")
        .expect("second snapshot");
    store
        .append_message_with_undo_snapshot(
            &session_id,
            &user_message("second prompt", 3),
            Some(before_second),
        )
        .expect("user second");
    fs::write(&file, "after second\n").expect("after second");
    store
        .append_message(&session_id, &assistant_message("second answer", 4))
        .expect("assistant second");

    undo_session(SessionUndoOptions {
        state: StateRuntime::open(&db).expect("state runtime"),
        cwd,
        snapshot_root: temp.path().join("snapshots"),
        session_id: session_id.clone(),
    })
    .expect("undo");

    let removed = store
        .cleanup_reverted_messages(&session_id)
        .expect("cleanup");
    assert_eq!(removed, 2);
    assert_eq!(store.load_messages(&session_id).expect("messages").len(), 2);
    let summary = store
        .session_summary(&session_id)
        .expect("summary")
        .expect("session");
    assert_eq!(summary.message_count, 2);
    assert!(
        store
            .session_revert_state(&session_id)
            .expect("revert state")
            .is_none()
    );
}

#[test]
pub(crate) fn undo_redo_error_paths_do_not_mutate_revert_state() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let store = StateRuntime::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .expect("session");
    let options = SessionUndoOptions {
        state: StateRuntime::open(&db).expect("state runtime"),
        cwd: cwd.clone(),
        snapshot_root: temp.path().join("snapshots"),
        session_id: session_id.clone(),
    };

    let err = undo_session(options.clone()).expect_err("nothing to undo");
    assert!(err.to_string().contains("nothing to undo"));
    let err = redo_session(options.clone()).expect_err("nothing to redo");
    assert!(err.to_string().contains("nothing to redo"));

    store
        .append_message(&session_id, &user_message("no snapshot", 1))
        .expect("user");
    let err = undo_session(options).expect_err("missing snapshot");
    assert!(err.to_string().contains("undo snapshot is unavailable"));
    assert!(
        store
            .session_revert_state(&session_id)
            .expect("revert state")
            .is_none()
    );
    assert_eq!(store.load_messages(&session_id).expect("messages").len(), 1);
}

#[test]
pub(crate) fn conversation_edit_is_restart_safe_and_never_restores_workspace_snapshots() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = temp.path().join("work");
    fs::create_dir_all(&cwd).expect("cwd");
    let file = cwd.join("tracked.txt");
    fs::write(&file, "workspace stays current\n").expect("workspace");
    let store = StateRuntime::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .expect("session");
    store
        .append_message_with_undo_snapshot(
            &session_id,
            &user_message("first prompt", 1),
            Some("unused-snapshot-one".to_string()),
        )
        .expect("first");
    let boundary = 2;
    store
        .append_message_with_undo_snapshot(
            &session_id,
            &user_message("second prompt", 2),
            Some("unused-snapshot-two".to_string()),
        )
        .expect("second");
    let staged = SessionRevertState::conversation_edit(
        boundary,
        format!("message:{boundary}"),
        vec![ConversationDraftPart::Text {
            text: "edited prompt".to_string(),
        }],
    );
    store
        .set_session_revert_state(&session_id, staged.clone())
        .expect("stage conversation edit");

    assert_eq!(
        fs::read_to_string(&file).expect("workspace"),
        "workspace stays current\n"
    );
    assert_eq!(
        store
            .load_tui_message_summaries(&session_id)
            .expect("visible")
            .len(),
        1
    );
    drop(store);
    let restarted = StateRuntime::open(&db).expect("restart");
    assert_eq!(
        restarted.session_revert_state(&session_id).expect("revert"),
        Some(staged)
    );

    let options = SessionUndoOptions {
        state: StateRuntime::open(&db).expect("runtime"),
        cwd: cwd.clone(),
        snapshot_root: temp.path().join("snapshots"),
        session_id: session_id.clone(),
    };
    assert!(
        undo_session(options.clone())
            .expect_err("conversation edit blocks undo")
            .to_string()
            .contains("staged conversation edit")
    );
    assert!(
        redo_session(options)
            .expect_err("conversation edit blocks redo")
            .to_string()
            .contains("staged conversation edit")
    );
    assert_eq!(
        fs::read_to_string(&file).expect("workspace"),
        "workspace stays current\n"
    );
    assert_eq!(
        restarted
            .cleanup_reverted_messages(&session_id)
            .expect("accepted replacement cleanup"),
        1
    );
    assert_eq!(
        restarted
            .load_messages(&session_id)
            .expect("messages")
            .len(),
        1
    );
}

#[test]
pub(crate) fn legacy_revert_metadata_parses_as_workspace_undo() {
    let temp = tempdir().expect("temp");
    let store = StateRuntime::open(temp.path().join("state.db")).expect("store");
    let session_id = store
        .create_session_with_metadata(temp.path(), "tui", "model", "provider", None)
        .expect("session");
    store
        .set_session_metadata_field(
            &session_id,
            crate::store::SESSION_REVERT_METADATA_KEY,
            Some(json!({"start_seq": 7, "original_snapshot": "legacy-snapshot"})),
        )
        .expect("legacy metadata");
    assert_eq!(
        store
            .session_revert_state(&session_id)
            .expect("revert")
            .and_then(|revert| revert.original_snapshot().map(str::to_string)),
        Some("legacy-snapshot".to_string())
    );
}
