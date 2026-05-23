#[test]
fn latest_run_session_filters_source_and_workdir() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let other_workdir = canonical_workdir(&temp.path().join("other")).expect("other");
    let store = SqliteStore::open(&db).expect("store");
    let smoke = store.create_session(&workdir).expect("smoke");
    let other = store
        .create_session_with_metadata(&other_workdir, "run", "model", "provider", None)
        .expect("other");
    let first = store
        .create_session_with_metadata(&workdir, "run", "model", "provider", None)
        .expect("first");
    let second = store
        .create_session_with_metadata(&workdir, "run", "model", "provider", None)
        .expect("second");
    store
        .append_message(&first, &user_message("real activity", 1))
        .expect("activity");

    let latest = latest_run_session_for_workdir(&db, &workdir)
        .expect("latest")
        .expect("session");
    assert_eq!(latest, first);
    assert_ne!(latest, second);
    assert_ne!(latest, smoke);
    assert_ne!(latest, other);
}

#[test]
fn session_title_setter_normalizes_and_bounds_title() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "tui", "model", "provider", None)
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
async fn new_tui_session_title_uses_model_generated_title_without_messages() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "tui", "model", "provider", None)
        .expect("session");
    let provider = Arc::new(FakeProvider::new(vec![vec![
        RawStreamEvent::Text("  \"Investigate TUI Copy\"  \nextra".to_string()),
        RawStreamEvent::Done(Outcome::Normal),
    ]]));
    let resolved = resolved_title_provider();

    ensure_new_tui_session_title(
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
async fn new_tui_session_title_falls_back_when_model_title_fails() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "tui", "model", "provider", None)
        .expect("session");
    let provider = Arc::new(FakeProvider::new(Vec::new()));
    let resolved = resolved_title_provider();

    ensure_new_tui_session_title(
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
async fn new_tui_session_title_fallback_uses_selected_skill_for_marker_prompt() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "tui", "model", "provider", None)
        .expect("session");
    let provider = Arc::new(FakeProvider::new(Vec::new()));
    let resolved = resolved_title_provider();
    let (catalog, selected) = title_skill_catalog(temp.path());

    ensure_new_tui_session_title(
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
fn session_title_request_includes_selected_skill_context() {
    let temp = tempdir().expect("temp");
    let (catalog, selected) = title_skill_catalog(temp.path());

    let request = crate::run::session_title_request("$x-daily", &selected, &catalog);

    assert!(request.contains("Selected skills:"));
    assert!(request.contains("- x-daily: Fetch X/Twitter posts and write a daily report"));
    assert!(request.contains("do not title the literal `$skill-name` marker"));
}

#[test]
fn session_title_fallback_removes_selected_skill_markers() {
    let selected = vec![crate::skills::SelectedSkill {
        name: "reviewer".to_string(),
        path: PathBuf::from("/tmp/reviewer/SKILL.md"),
    }];

    assert_eq!(
        crate::run::fallback_session_title("$reviewer inspect sidebar", &selected),
        "inspect sidebar"
    );
}

fn title_skill_catalog(root: &std::path::Path) -> (crate::skills::SkillCatalog, Vec<crate::skills::SelectedSkill>) {
    let path = root.join("x-daily").join("SKILL.md");
    let skill = crate::skills::Skill {
        name: "x-daily".to_string(),
        description: "Fetch X/Twitter posts and write a daily report".to_string(),
        file_path: path.clone(),
        base_dir: root.join("x-daily"),
        source: crate::skills::SkillSource::Project,
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
        supported_on_current_platform: true,
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

fn resolved_title_provider() -> ResolvedRunProvider {
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

fn user_message(text: &str, timestamp_ms: i64) -> Message {
    Message::User {
        content: vec![psychevo_agent_core::UserContentBlock::text(text)],
        timestamp_ms,
    }
}

fn assistant_message(text: &str, timestamp_ms: i64) -> Message {
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
