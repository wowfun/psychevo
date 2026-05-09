#[test]
fn undo_redo_restore_git_snapshots_and_visible_message_ranges() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&workdir)
            .arg("init")
            .output()
            .expect("git init")
            .status
            .success()
    );
    let file = workdir.join("tracked.txt");
    fs::write(&file, "base\n").expect("base");

    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "tui", "model", "provider", None)
        .expect("session");
    let snapshots = SnapshotStore::new(
        temp.path().join("snapshots"),
        session_id.clone(),
        workdir.clone(),
    );
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

    let options = SessionUndoOptions {
        db_path: db.clone(),
        workdir: workdir.clone(),
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
fn cleanup_reverted_messages_deletes_hidden_range() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&workdir)
            .arg("init")
            .output()
            .expect("git init")
            .status
            .success()
    );
    let file = workdir.join("tracked.txt");
    fs::write(&file, "base\n").expect("base");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "tui", "model", "provider", None)
        .expect("session");
    let snapshots = SnapshotStore::new(
        temp.path().join("snapshots"),
        session_id.clone(),
        workdir.clone(),
    );
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
        db_path: db.clone(),
        workdir,
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
fn undo_redo_error_paths_do_not_mutate_revert_state() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "tui", "model", "provider", None)
        .expect("session");
    let options = SessionUndoOptions {
        db_path: db.clone(),
        workdir: workdir.clone(),
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

