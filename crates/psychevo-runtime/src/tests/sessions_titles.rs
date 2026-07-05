#[allow(unused_imports)]
pub(crate) use super::*;
#[test]
pub(crate) fn latest_run_session_filters_source_and_cwd() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let other_cwd = canonical_cwd(&temp.path().join("other")).expect("other");
    let store = SqliteStore::open(&db).expect("store");
    let smoke = store.create_session(&cwd).expect("smoke");
    let other = store
        .create_session_with_metadata(&other_cwd, "run", "model", "provider", None)
        .expect("other");
    let first = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
        .expect("first");
    let second = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
        .expect("second");
    store
        .append_message(&first, &user_message("real activity", 1))
        .expect("activity");

    let state = StateRuntime::from_store(db, store.clone());
    let latest = latest_run_session_for_cwd(&state, &cwd)
        .expect("latest")
        .expect("session");
    assert_eq!(latest, first);
    assert_ne!(latest, second);
    assert_ne!(latest, smoke);
    assert_ne!(latest, other);
}

#[test]
pub(crate) fn session_title_setter_normalizes_and_bounds_title() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .expect("session");

    let title = store
        .set_session_title(&session_id, &format!("  hello\n\t{}  ", "x".repeat(120)))
        .expect("title");
    assert_eq!(title.chars().count(), SESSION_TITLE_MAX_CHARS);
    assert!(title.starts_with("hello x"));
    let summary = store
        .session_summary(&session_id)
        .expect("summary")
        .expect("session");
    assert_eq!(summary.title.as_deref(), Some(title.as_str()));
    assert!(store.set_session_title(&session_id, "   ").is_err());
}

#[tokio::test]
pub(crate) async fn new_visible_session_title_uses_model_generated_title_without_messages() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .expect("session");
    let provider = Arc::new(FakeProvider::new(vec![vec![
        RawStreamEvent::Text("  \"Investigate TUI Copy\"  \nextra".to_string()),
        RawStreamEvent::Done(Outcome::Normal),
    ]]));
    let resolved = resolved_title_provider();

    ensure_new_visible_session_title(
        &store,
        &session_id,
        "please inspect copy behavior",
        &[],
        &crate::skills::SkillCatalog::default(),
        provider,
        &resolved,
    )
    .await
    .expect("title");

    let summary = store
        .session_summary(&session_id)
        .expect("summary")
        .expect("session");
    assert_eq!(summary.title.as_deref(), Some("Investigate TUI Copy"));
    assert_eq!(summary.message_count, 0);
    assert_eq!(summary.tool_call_count, 0);
}

#[tokio::test]
pub(crate) async fn new_visible_session_title_falls_back_when_model_title_fails() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .expect("session");
    let provider = Arc::new(FakeProvider::new(Vec::new()));
    let resolved = resolved_title_provider();

    ensure_new_visible_session_title(
        &store,
        &session_id,
        "  inspect\nsidebar   title  behavior  ",
        &[],
        &crate::skills::SkillCatalog::default(),
        provider,
        &resolved,
    )
    .await
    .expect("fallback title");

    let summary = store
        .session_summary(&session_id)
        .expect("summary")
        .expect("session");
    assert_eq!(
        summary.title.as_deref(),
        Some("inspect sidebar title behavior")
    );
}

#[tokio::test]
pub(crate) async fn new_visible_session_title_fallback_uses_selected_skill_for_marker_prompt() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .expect("session");
    let provider = Arc::new(FakeProvider::new(Vec::new()));
    let resolved = resolved_title_provider();
    let (catalog, selected) = title_skill_catalog(temp.path());

    ensure_new_visible_session_title(
        &store,
        &session_id,
        "$x-daily ",
        &selected,
        &catalog,
        provider,
        &resolved,
    )
    .await
    .expect("fallback title");

    let summary = store
        .session_summary(&session_id)
        .expect("summary")
        .expect("session");
    assert_eq!(summary.title.as_deref(), Some("x-daily"));
}

#[test]
pub(crate) fn session_title_request_includes_selected_skill_context() {
    let temp = tempdir().expect("temp");
    let (catalog, selected) = title_skill_catalog(temp.path());

    let request = crate::run::session_title_request("$x-daily", &selected, &catalog);

    assert!(request.contains("Selected skills:"));
    assert!(request.contains("- x-daily: Fetch X/Twitter posts and write a daily report"));
    assert!(request.contains("do not title the literal `$skill-name` marker"));
}

#[tokio::test]
pub(crate) async fn new_visible_session_title_fallback_covers_visible_sources() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let resolved = resolved_title_provider();

    for source in ["web", "run", "automation", "channel/wechat"] {
        let session_id = store
            .create_session_with_metadata(&cwd, source, "model", "provider", None)
            .expect("session");
        ensure_new_visible_session_title(
            &store,
            &session_id,
            "  summarize\nvisible   source  ",
            &[],
            &crate::skills::SkillCatalog::default(),
            Arc::new(FakeProvider::new(Vec::new())),
            &resolved,
        )
        .await
        .expect("fallback title");

        let summary = store
            .session_summary(&session_id)
            .expect("summary")
            .expect("session");
        assert_eq!(summary.title.as_deref(), Some("summarize visible source"));
    }
}

#[tokio::test]
pub(crate) async fn new_visible_session_title_skips_internal_and_child_sessions() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let resolved = resolved_title_provider();
    let internal = store
        .create_session_with_metadata(
            &cwd,
            crate::WEB_SIDE_CONVERSATION_SESSION_SOURCE,
            "model",
            "provider",
            None,
        )
        .expect("internal session");
    let parent = store
        .create_session_with_metadata(&cwd, "web", "model", "provider", None)
        .expect("parent");
    let child = store
        .create_child_session_with_metadata(&parent, &cwd, "web", "model", "provider", None)
        .expect("child");

    for session_id in [&internal, &child] {
        ensure_new_visible_session_title(
            &store,
            session_id,
            "should not persist",
            &[],
            &crate::skills::SkillCatalog::default(),
            Arc::new(FakeProvider::new(vec![vec![
                RawStreamEvent::Text("Generated Title".to_string()),
                RawStreamEvent::Done(Outcome::Normal),
            ]])),
            &resolved,
        )
        .await
        .expect("skip title");
        let summary = store
            .session_summary(session_id)
            .expect("summary")
            .expect("session");
        assert_eq!(summary.title, None);
    }
}

#[tokio::test]
pub(crate) async fn new_visible_session_title_preserves_existing_title() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "web", "model", "provider", None)
        .expect("session");
    store
        .set_session_title(&session_id, "Manual Title")
        .expect("manual title");
    let resolved = resolved_title_provider();

    ensure_new_visible_session_title(
        &store,
        &session_id,
        "replace me",
        &[],
        &crate::skills::SkillCatalog::default(),
        Arc::new(FakeProvider::new(vec![vec![
            RawStreamEvent::Text("Generated Title".to_string()),
            RawStreamEvent::Done(Outcome::Normal),
        ]])),
        &resolved,
    )
    .await
    .expect("preserve title");

    let summary = store
        .session_summary(&session_id)
        .expect("summary")
        .expect("session");
    assert_eq!(summary.title.as_deref(), Some("Manual Title"));
}

#[test]
pub(crate) fn visible_session_source_title_rules_match_history_sources() {
    for source in [
        "web",
        "run",
        "tui",
        "automation",
        "channel/wechat",
        "peer_agent",
    ] {
        assert!(visible_session_source_allows_auto_title(source), "{source}");
    }
    for source in [
        "automation-draft",
        crate::TUI_SIDE_CONVERSATION_SESSION_SOURCE,
        crate::WEB_SIDE_CONVERSATION_SESSION_SOURCE,
    ] {
        assert!(
            !visible_session_source_allows_auto_title(source),
            "{source}"
        );
    }
}

#[test]
pub(crate) fn first_use_empty_visible_session_materializes_model_and_metadata() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "web", "pending", "pending", None)
        .expect("session");
    let metadata = json!({
        "provider_label": "Local Test",
        "cwd": cwd.display().to_string(),
    });

    let materialized = crate::run::materialize_first_use_empty_session(
        &store,
        &session_id,
        "local",
        "test-model",
        metadata.clone(),
    )
    .expect("first use");

    assert!(materialized);
    let summary = store
        .session_summary(&session_id)
        .expect("summary")
        .expect("session");
    assert_eq!(summary.provider, "local");
    assert_eq!(summary.model, "test-model");
    assert_eq!(
        store.session_metadata(&session_id).expect("metadata"),
        Some(metadata)
    );
}

#[test]
pub(crate) fn first_use_empty_visible_session_does_not_rewrite_existing_or_internal_sessions() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = SqliteStore::open(&db).expect("store");
    let non_empty = store
        .create_session_with_metadata(
            &cwd,
            "web",
            "existing-model",
            "existing-provider",
            Some(json!({ "existing": true })),
        )
        .expect("non empty");
    store
        .append_message(&non_empty, &user_message("hello", 1))
        .expect("message");
    let internal = store
        .create_session_with_metadata(
            &cwd,
            crate::WEB_SIDE_CONVERSATION_SESSION_SOURCE,
            "internal-model",
            "internal-provider",
            None,
        )
        .expect("internal");
    let parent = store
        .create_session_with_metadata(&cwd, "web", "parent-model", "parent-provider", None)
        .expect("parent");
    let child = store
        .create_child_session_with_metadata(
            &parent,
            &cwd,
            "web",
            "child-model",
            "child-provider",
            None,
        )
        .expect("child");

    for session_id in [&non_empty, &internal, &child] {
        let materialized = crate::run::materialize_first_use_empty_session(
            &store,
            session_id,
            "replacement-provider",
            "replacement-model",
            json!({ "replacement": true }),
        )
        .expect("skip");
        assert!(!materialized, "{session_id}");
    }

    let non_empty_summary = store
        .session_summary(&non_empty)
        .expect("summary")
        .expect("session");
    assert_eq!(non_empty_summary.provider, "existing-provider");
    assert_eq!(non_empty_summary.model, "existing-model");
    assert_eq!(
        store.session_metadata(&non_empty).expect("metadata"),
        Some(json!({ "existing": true }))
    );
}

#[test]
pub(crate) fn first_use_empty_visible_session_extends_new_session_title_gate() {
    assert!(crate::run::should_title_completed_session(
        false,
        true,
        Outcome::Normal
    ));
    assert!(crate::run::should_title_completed_session(
        true,
        false,
        Outcome::Normal
    ));
    assert!(!crate::run::should_title_completed_session(
        false,
        false,
        Outcome::Normal
    ));
    assert!(!crate::run::should_title_completed_session(
        false,
        true,
        Outcome::Aborted
    ));
}

#[test]
pub(crate) fn session_title_fallback_removes_selected_skill_markers() {
    let selected = vec![crate::skills::SelectedSkill {
        name: "reviewer".to_string(),
        path: PathBuf::from("/tmp/reviewer/SKILL.md"),
    }];

    assert_eq!(
        crate::run::fallback_session_title("$reviewer inspect sidebar", &selected),
        "inspect sidebar"
    );
}

pub(crate) fn title_skill_catalog(
    root: &std::path::Path,
) -> (
    crate::skills::SkillCatalog,
    Vec<crate::skills::SelectedSkill>,
) {
    let path = root.join("x-daily").join("SKILL.md");
    let skill = crate::skills::Skill {
        name: "x-daily".to_string(),
        description: "Fetch X/Twitter posts and write a daily report".to_string(),
        file_path: path.clone(),
        base_dir: root.join("x-daily"),
        source: crate::skills::SkillSource::Project,
        enabled: true,
        disable_model_invocation: false,
        category: None,
        tags: Vec::new(),
        related: Vec::new(),
        platforms: Vec::new(),
        required_environment_variables: Vec::new(),
        required_credential_files: Vec::new(),
        setup_help: None,
        compatibility: None,
        license: None,
        allowed_tools: Vec::new(),
        required_tools: Vec::new(),
        fallback_for_tools: Vec::new(),
        required_toolsets: Vec::new(),
        fallback_for_toolsets: Vec::new(),
        supported_on_current_platform: true,
        collision_group: Vec::new(),
    };
    let selected = vec![crate::skills::SelectedSkill {
        name: skill.name.clone(),
        path: skill.file_path.clone(),
    }];
    (
        crate::skills::SkillCatalog {
            skills: vec![skill],
            diagnostics: Vec::new(),
            collisions: Default::default(),
        },
        selected,
    )
}

pub(crate) fn resolved_title_provider() -> ResolvedRunProvider {
    ResolvedRunProvider {
        provider: "fake".to_string(),
        display_label: "Fake".to_string(),
        model: "model".to_string(),
        base_url: "http://127.0.0.1:9/v1".to_string(),
        api_key_env: None,
        api_key: "test-key".to_string(),
        reasoning_effort: None,
        context_limit: None,
        metadata: Default::default(),
    }
}

pub(crate) fn user_message(text: &str, timestamp_ms: i64) -> Message {
    Message::User {
        content: vec![psychevo_agent_core::UserContentBlock::text(text)],
        timestamp_ms,
    }
}

pub(crate) fn assistant_message(text: &str, timestamp_ms: i64) -> Message {
    Message::Assistant {
        content: vec![AssistantBlock::Text {
            text: text.to_string(),
        }],
        timestamp_ms,
        finish_reason: Some("stop".to_string()),
        outcome: Outcome::Normal,
        model: Some("model".to_string()),
        provider: Some("provider".to_string()),
    }
}
