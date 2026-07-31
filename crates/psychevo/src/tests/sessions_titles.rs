#[allow(unused_imports)]
pub(crate) use super::*;

fn title_model(text: &str) -> psychevo_ai::LanguageModel {
    psychevo_ai::Fake::with_language(psychevo_ai::FakeLanguageAdapter::text(text))
        .expect("fake title provider")
        .provider()
        .language_model("model")
        .expect("fake title model")
}

fn fake_provider(text: &str) -> psychevo_ai::Provider {
    psychevo_ai::Fake::with_language(psychevo_ai::FakeLanguageAdapter::text(text))
        .expect("fake provider")
        .provider()
}

fn scripted_fake_provider(main_text: &str, title_text: &str) -> psychevo_ai::Provider {
    fn text_script(
        text: &str,
    ) -> Vec<psychevo_ai::AdapterResult<psychevo_ai::LanguageAdapterEvent>> {
        vec![
            Ok(psychevo_ai::LanguageAdapterEvent::TextStart { content_index: 0 }),
            Ok(psychevo_ai::LanguageAdapterEvent::TextDelta {
                content_index: 0,
                delta: text.to_string(),
            }),
            Ok(psychevo_ai::LanguageAdapterEvent::TextEnd { content_index: 0 }),
            Ok(psychevo_ai::LanguageAdapterEvent::Finish {
                finish_reason: None,
            }),
        ]
    }

    psychevo_ai::Fake::with_language(psychevo_ai::FakeLanguageAdapter::new([
        text_script(main_text),
        text_script(title_text),
    ]))
    .expect("scripted fake provider")
    .provider()
}

#[tokio::test]
pub(crate) async fn latest_run_session_filters_source_and_cwd() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let other_cwd = canonical_cwd(&temp.path().join("other")).expect("other");
    let store = StateRuntime::open(&db).await.expect("store");
    let smoke = store.create_session(&cwd).await.expect("smoke");
    let other = store
        .create_session_with_metadata(&other_cwd, "run", "model", "provider", None)
        .await
        .expect("other");
    let first = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
        .await
        .expect("first");
    let second = store
        .create_session_with_metadata(&cwd, "run", "model", "provider", None)
        .await
        .expect("second");
    store
        .append_message(&first, &user_message("real activity", 1))
        .await
        .expect("activity");

    let state = store.clone();
    let latest = latest_run_session_for_cwd(&state, &cwd)
        .await
        .expect("latest")
        .expect("session");
    assert_eq!(latest, first);
    assert_ne!(latest, second);
    assert_ne!(latest, smoke);
    assert_ne!(latest, other);
}

#[tokio::test]
pub(crate) async fn session_title_setter_normalizes_and_bounds_title() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = StateRuntime::open(&db).await.expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .await
        .expect("session");

    let title = store
        .set_session_title(&session_id, &format!("  hello\n\t{}  ", "x".repeat(120)))
        .await
        .expect("title");
    assert_eq!(title.chars().count(), SESSION_TITLE_MAX_CHARS);
    assert!(title.starts_with("hello x"));
    let summary = store
        .session_summary(&session_id)
        .await
        .expect("summary")
        .expect("session");
    assert_eq!(summary.title.as_deref(), Some(title.as_str()));
    assert!(store.set_session_title(&session_id, "   ").await.is_err());
}

#[tokio::test]
pub(crate) async fn new_visible_session_title_uses_model_generated_title_without_messages() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = StateRuntime::open(&db).await.expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .await
        .expect("session");
    let provider = title_model("  \"Investigate TUI Copy\"  \nextra");
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
        .await
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
    let store = StateRuntime::open(&db).await.expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .await
        .expect("session");
    let provider = title_model("");
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
        .await
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
    let store = StateRuntime::open(&db).await.expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "tui", "model", "provider", None)
        .await
        .expect("session");
    let provider = title_model("");
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
        .await
        .expect("summary")
        .expect("session");
    assert_eq!(summary.title.as_deref(), Some("x-daily"));
}

#[tokio::test]
pub(crate) async fn session_title_request_includes_selected_skill_context() {
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
    let store = StateRuntime::open(&db).await.expect("store");
    let resolved = resolved_title_provider();

    for source in ["web", "run", "automation", "channel/wechat"] {
        let session_id = store
            .create_session_with_metadata(&cwd, source, "model", "provider", None)
            .await
            .expect("session");
        ensure_new_visible_session_title(
            &store,
            &session_id,
            "  summarize\nvisible   source  ",
            &[],
            &crate::skills::SkillCatalog::default(),
            title_model(""),
            &resolved,
        )
        .await
        .expect("fallback title");

        let summary = store
            .session_summary(&session_id)
            .await
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
    let store = StateRuntime::open(&db).await.expect("store");
    let resolved = resolved_title_provider();
    let internal = store
        .create_session_with_metadata(
            &cwd,
            crate::thread_lineage::WEB_SIDE_CONVERSATION_SESSION_SOURCE,
            "model",
            "provider",
            None,
        )
        .await
        .expect("internal session");
    let parent = store
        .create_session_with_metadata(&cwd, "web", "model", "provider", None)
        .await
        .expect("parent");
    let child = store
        .create_child_session_with_metadata(&parent, &cwd, "web", "model", "provider", None)
        .await
        .expect("child");

    for session_id in [&internal, &child] {
        ensure_new_visible_session_title(
            &store,
            session_id,
            "should not persist",
            &[],
            &crate::skills::SkillCatalog::default(),
            title_model("Generated Title"),
            &resolved,
        )
        .await
        .expect("skip title");
        let summary = store
            .session_summary(session_id)
            .await
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
    let store = StateRuntime::open(&db).await.expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "web", "model", "provider", None)
        .await
        .expect("session");
    store
        .set_session_title(&session_id, "Manual Title")
        .await
        .expect("manual title");
    let resolved = resolved_title_provider();

    ensure_new_visible_session_title(
        &store,
        &session_id,
        "replace me",
        &[],
        &crate::skills::SkillCatalog::default(),
        title_model("Generated Title"),
        &resolved,
    )
    .await
    .expect("preserve title");

    let summary = store
        .session_summary(&session_id)
        .await
        .expect("summary")
        .expect("session");
    assert_eq!(summary.title.as_deref(), Some("Manual Title"));
}

#[tokio::test]
pub(crate) async fn invalid_auxiliary_title_provider_falls_back_without_blocking_streaming_turn() {
    let temp = tempdir().expect("temp");
    let home = home_dir(&temp);
    let cwd = temp.path().join("work");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&cwd).expect("cwd");

    write_config(
        home.join("config.toml"),
        r#"
model = "invalidprovidermain/main"

[provider.invalidprovidermain]
api = "http://127.0.0.1:1/v1"
no_auth = true

[provider.invalidprovidermain.models.main]

[auxiliary.title_generation]
provider = "missingtitle"
model = "title"
"#,
    )
    .expect("config");

    let mut options = base_options(&temp).await;
    options.cwd = cwd;
    options.model = Some("invalidprovidermain/main".to_string());
    options.no_agents = true;
    options.no_skills = true;
    let (title_event_tx, mut title_event_rx) = tokio::sync::mpsc::unbounded_channel();
    let stream: RunStreamSink = Arc::new(move |event| {
        if let Some(value) = event.legacy_value()
            && value.get("type").and_then(Value::as_str) == Some("session_title_changed")
        {
            let _ = title_event_tx.send(value.clone());
        }
    });
    let (_control_handle, control) = run_control();

    let run_result = tokio::time::timeout(
        Duration::from_secs(2),
        run_live_streaming_controlled_with_provider(
            options,
            "web",
            &["web"],
            stream,
            control,
            fake_provider("Main turn completed."),
        ),
    )
    .await
    .expect("main run timeout");
    let result = run_result.expect("invalid auxiliary title provider must not fail the main turn");
    let title_event = tokio::time::timeout(Duration::from_secs(2), title_event_rx.recv())
        .await
        .expect("fallback title event timeout")
        .expect("fallback title event");

    assert_eq!(result.outcome, Outcome::Normal);
    assert_eq!(result.final_answer, "Main turn completed.");
    assert_eq!(title_event["session_id"], result.session_id);
    assert_eq!(title_event["title"], "hello");
    assert!(result.warnings.iter().any(|warning| {
        warning.kind == "title_generation_failed"
            && warning.message.contains("unknown provider: missingtitle")
    }));
}

#[tokio::test]
pub(crate) async fn web_first_turn_failure_before_generation_publishes_fallback_title() {
    let temp = tempdir().expect("temp");
    let home = home_dir(&temp);
    let cwd = temp.path().join("work");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&cwd).expect("cwd");
    write_config(
        home.join("config.toml"),
        r#"
model = "fixture/main"

[provider.fixture]
api = "http://127.0.0.1:1/v1"
no_auth = true

[provider.fixture.models.main]
"#,
    )
    .expect("config");

    let mut options = base_options(&temp).await;
    options.cwd = cwd;
    options.model = Some("fixture/main".to_string());
    options.no_agents = true;
    options.no_skills = true;
    options.mcp_servers.push(
        crate::types::McpServerInput::new(
            "required-disabled",
            crate::types::McpTransportInput::Unsupported {
                kind: "fixture".to_string(),
            },
        )
        .with_policy(crate::types::McpServerPolicy {
            enabled: false,
            required: true,
            ..Default::default()
        }),
    );
    let state = options.state.clone();
    let (title_event_tx, mut title_event_rx) = tokio::sync::mpsc::unbounded_channel();
    let stream: RunStreamSink = Arc::new(move |event| {
        if let Some(value) = event.legacy_value()
            && value.get("type").and_then(Value::as_str) == Some("session_title_changed")
        {
            let _ = title_event_tx.send(value.clone());
        }
    });
    let (_control_handle, control) = run_control();

    let error = run_live_streaming_controlled_with_provider(
        options,
        "web",
        &["web"],
        stream,
        control,
        fake_provider("main generation must not start"),
    )
    .await
    .expect_err("required disabled MCP must fail before main generation");
    assert!(
        error
            .to_string()
            .contains("required MCP server unavailable"),
        "{error}"
    );

    let title_event = tokio::time::timeout(Duration::from_secs(2), title_event_rx.recv())
        .await
        .expect("fallback title event timeout")
        .expect("fallback title event");
    let session_id = title_event["session_id"]
        .as_str()
        .expect("title session id");
    assert_eq!(title_event["title"], "hello");
    assert_eq!(
        state
            .session_summary(session_id)
            .await
            .expect("summary")
            .and_then(|summary| summary.title),
        Some("hello".to_string())
    );
}

#[tokio::test]
pub(crate) async fn web_first_turn_main_model_construction_failure_publishes_fallback_title() {
    let temp = tempdir().expect("temp");
    let home = home_dir(&temp);
    let cwd = temp.path().join("work");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&cwd).expect("cwd");
    write_config(
        home.join("config.toml"),
        r#"
model = "fixture/main"

[provider.fixture]
api = "http://127.0.0.1:1/v1"
no_auth = true

[provider.fixture.models.main]
"#,
    )
    .expect("config");

    let mut options = base_options(&temp).await;
    options.cwd = cwd;
    options.model = Some("fixture/main".to_string());
    options.no_agents = true;
    options.no_skills = true;
    let state = options.state.clone();
    let (title_event_tx, mut title_event_rx) = tokio::sync::mpsc::unbounded_channel();
    let stream: RunStreamSink = Arc::new(move |event| {
        if let Some(value) = event.legacy_value()
            && value.get("type").and_then(Value::as_str) == Some("session_title_changed")
        {
            let _ = title_event_tx.send(value.clone());
        }
    });
    let (_control_handle, control) = run_control();
    let image_only_provider = psychevo_ai::Provider::builder(psychevo_ai::DeploymentConfig::new(
        "image-only",
        "fixture",
        "fixture://image-only",
    ))
    .image_adapter(psychevo_ai::FakeImageAdapter::default())
    .build()
    .expect("image-only provider");

    let error = run_live_streaming_controlled_with_provider(
        options,
        "web",
        &["web"],
        stream,
        control,
        image_only_provider,
    )
    .await
    .expect_err("main provider without language capability must fail");
    assert!(
        error.to_string().to_ascii_lowercase().contains("language"),
        "{error}"
    );

    let title_event = tokio::time::timeout(Duration::from_secs(2), title_event_rx.recv())
        .await
        .expect("fallback title event timeout")
        .expect("fallback title event");
    let session_id = title_event["session_id"]
        .as_str()
        .expect("title session id");
    assert_eq!(title_event["title"], "hello");
    assert_eq!(
        state
            .session_summary(session_id)
            .await
            .expect("summary")
            .and_then(|summary| summary.title),
        Some("hello".to_string())
    );
}

#[tokio::test]
pub(crate) async fn web_first_turn_abort_before_generation_publishes_fallback_title() {
    let temp = tempdir().expect("temp");
    let home = home_dir(&temp);
    let cwd = temp.path().join("work");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&cwd).expect("cwd");
    write_config(
        home.join("config.toml"),
        r#"
model = "fixture/main"

[provider.fixture]
api = "http://127.0.0.1:1/v1"
no_auth = true

[provider.fixture.models.main]
"#,
    )
    .expect("config");

    let mut options = base_options(&temp).await;
    options.cwd = cwd;
    options.model = Some("fixture/main".to_string());
    options.no_agents = true;
    options.no_skills = true;
    let (title_event_tx, mut title_event_rx) = tokio::sync::mpsc::unbounded_channel();
    let stream: RunStreamSink = Arc::new(move |event| {
        if let Some(value) = event.legacy_value()
            && value.get("type").and_then(Value::as_str) == Some("session_title_changed")
        {
            let _ = title_event_tx.send(value.clone());
        }
    });
    let (control_handle, control) = run_control();
    control_handle.abort();

    let result = run_live_streaming_controlled_with_provider(
        options,
        "web",
        &["web"],
        stream,
        control,
        fake_provider("main generation must not start"),
    )
    .await
    .expect("pre-generation abort");
    assert_eq!(result.outcome, Outcome::Aborted);

    let title_event = tokio::time::timeout(Duration::from_secs(2), title_event_rx.recv())
        .await
        .expect("fallback title event timeout")
        .expect("fallback title event");
    assert_eq!(title_event["session_id"], result.session_id);
    assert_eq!(title_event["title"], "hello");
}

#[tokio::test]
pub(crate) async fn web_title_generation_does_not_preempt_shared_scripted_main_provider() {
    let temp = tempdir().expect("temp");
    let home = home_dir(&temp);
    let cwd = temp.path().join("work");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&cwd).expect("cwd");

    write_config(
        home.join("config.toml"),
        r#"
model = "shared/main"

[provider.shared]
api = "http://127.0.0.1:1/v1"
no_auth = true

[provider.shared.models.main]

[provider.shared.models.title]

[auxiliary.title_generation]
provider = "shared"
model = "title"
"#,
    )
    .expect("config");

    let mut options = base_options(&temp).await;
    options.cwd = cwd;
    options.model = Some("shared/main".to_string());
    options.no_agents = true;
    options.no_skills = true;
    let (title_event_tx, mut title_event_rx) = tokio::sync::mpsc::unbounded_channel();
    let stream: RunStreamSink = Arc::new(move |event| {
        if let Some(value) = event.legacy_value()
            && value.get("type").and_then(Value::as_str) == Some("session_title_changed")
        {
            let _ = title_event_tx.send(value.clone());
        }
    });
    let (_control_handle, control) = run_control();

    let result = tokio::time::timeout(
        Duration::from_secs(2),
        run_live_streaming_controlled_with_provider(
            options,
            "web",
            &["web"],
            stream,
            control,
            scripted_fake_provider("Main turn completed.", "Generated title"),
        ),
    )
    .await
    .expect("main run timeout")
    .expect("main run");
    let title_event = tokio::time::timeout(Duration::from_secs(2), title_event_rx.recv())
        .await
        .expect("title event timeout")
        .expect("title event");

    assert_eq!(result.outcome, Outcome::Normal);
    assert_eq!(result.final_answer, "Main turn completed.");
    assert_eq!(title_event["session_id"], result.session_id);
    assert_eq!(title_event["title"], "Generated title");
}

#[tokio::test]
pub(crate) async fn invalid_auxiliary_title_model_does_not_fail_non_streaming_turn() {
    let temp = tempdir().expect("temp");
    let home = home_dir(&temp);
    let cwd = temp.path().join("work");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&cwd).expect("cwd");

    write_config(
        home.join("config.toml"),
        r#"
model = "invalidmodelmain/main"

[provider.invalidmodelmain]
api = "http://127.0.0.1:1/v1"
no_auth = true

[provider.invalidmodelmain.models.main]

[provider.invalidtitle]
api = "http://127.0.0.1:1/v1"
no_auth = true

[auxiliary.title_generation]
provider = "invalidtitle"
model = "bad\nmodel"
"#,
    )
    .expect("config");

    let mut options = base_options(&temp).await;
    options.cwd = cwd;
    options.model = Some("invalidmodelmain/main".to_string());
    options.no_agents = true;
    options.no_skills = true;
    let state = options.state.clone();

    let run_result = tokio::time::timeout(
        Duration::from_secs(2),
        run_live_internal(
            options,
            "run",
            &["run"],
            None,
            None,
            false,
            Some(fake_provider("Main turn completed.")),
        ),
    )
    .await
    .expect("main run timeout");
    let result = run_result.expect("invalid auxiliary title model must not fail the main turn");

    assert_eq!(result.outcome, Outcome::Normal);
    assert_eq!(result.final_answer, "Main turn completed.");
    assert!(result.warnings.iter().any(|warning| {
        warning.kind == "title_generation_failed"
            && warning
                .message
                .contains("model id must be non-empty, trimmed, and contain no control characters")
    }));
    assert_eq!(
        state
            .session_summary(&result.session_id)
            .await
            .expect("summary")
            .and_then(|summary| summary.title)
            .as_deref(),
        Some("hello")
    );
}

#[tokio::test]
pub(crate) async fn streaming_visible_session_title_is_published_before_main_turn_settles() {
    let temp = tempdir().expect("temp");
    let home = home_dir(&temp);
    let cwd = temp.path().join("work");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&cwd).expect("cwd");

    let main_listener = TcpListener::bind("127.0.0.1:0").expect("bind main");
    let main_address = main_listener.local_addr().expect("main address");
    let title_listener = TcpListener::bind("127.0.0.1:0").expect("bind title");
    let title_address = title_listener.local_addr().expect("title address");
    let (main_started_tx, main_started_rx) = tokio::sync::oneshot::channel();
    let (release_main_progress_tx, release_main_progress_rx) = std::sync::mpsc::channel();
    let (main_progress_tx, main_progress_rx) = tokio::sync::oneshot::channel();
    let (title_started_tx, title_started_rx) = tokio::sync::oneshot::channel();
    let (release_main_tx, release_main_rx) = std::sync::mpsc::channel();
    let main_server = thread::spawn(move || {
        let (mut stream, _) = main_listener.accept().expect("main request");
        let _ = read_http_request(&mut stream);
        main_started_tx.send(()).expect("main started");
        release_main_progress_rx
            .recv()
            .expect("release main progress");
        let response = concat!(
            "HTTP/1.1 200 OK\r\n",
            "content-type: text/event-stream\r\n",
            "connection: close\r\n",
            "\r\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},",
            "\"finish_reason\":null}]}\n\n"
        );
        stream
            .write_all(response.as_bytes())
            .expect("write main progress");
        stream.flush().expect("flush main progress");
        main_progress_tx.send(()).expect("main progress sent");
        release_main_rx.recv().expect("release main");
        let _ = stream.write_all(
            concat!(
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n"
            )
            .as_bytes(),
        );
    });
    let title_server = thread::spawn(move || {
        let (mut stream, _) = title_listener.accept().expect("title request");
        let _ = read_http_request(&mut stream);
        title_started_tx.send(()).expect("title started");
        write_test_sse(&mut stream, "Greeting session");
    });
    write_config(
        home.join("config.toml"),
        &format!(
            r#"
model = "timingmain/main"

[provider.timingmain]
api = "http://{main_address}/v1"

[provider.timingmain.models.main]

[provider.timingtitle]
api = "http://{title_address}/v1"

[provider.timingtitle.models.title]

[auxiliary.title_generation]
provider = "timingtitle"
model = "title"
"#,
        ),
    )
    .expect("config");

    let mut options = base_options(&temp).await;
    options.cwd = cwd;
    options.model = Some("timingmain/main".to_string());
    options.no_agents = true;
    options.no_skills = true;
    let state = options.state.clone();
    let (title_event_tx, mut title_event_rx) = tokio::sync::mpsc::unbounded_channel();
    let stream: RunStreamSink = Arc::new(move |event| {
        if let Some(value) = event.legacy_value()
            && value.get("type").and_then(Value::as_str) == Some("session_title_changed")
        {
            let _ = title_event_tx.send(value.clone());
        }
    });
    let run =
        tokio::spawn(async move { run_live_streaming(options, "web", &["web"], stream).await });

    tokio::time::timeout(Duration::from_secs(2), main_started_rx)
        .await
        .expect("main request timeout")
        .expect("main request started");
    let mut title_started_rx = title_started_rx;
    assert!(
        tokio::time::timeout(Duration::from_millis(150), &mut title_started_rx)
            .await
            .is_err(),
        "title request started before the main provider produced progress"
    );
    release_main_progress_tx
        .send(())
        .expect("release main progress");
    tokio::time::timeout(Duration::from_secs(2), main_progress_rx)
        .await
        .expect("main progress timeout")
        .expect("main progress sent");
    tokio::time::timeout(Duration::from_secs(2), title_started_rx)
        .await
        .expect("title request timeout after main progress")
        .expect("title request started");
    let event = tokio::time::timeout(Duration::from_secs(2), title_event_rx.recv())
        .await
        .expect("title event timeout while main turn was blocked")
        .expect("title event");
    let main_was_blocked = !run.is_finished();
    let session_id = event["session_id"]
        .as_str()
        .expect("title session id")
        .to_string();
    let event_title = event["title"].as_str().expect("title").to_string();

    run.abort();
    release_main_tx.send(()).expect("release main");
    let run_error = tokio::time::timeout(Duration::from_secs(2), run)
        .await
        .expect("main run timeout")
        .expect_err("blocked main run should be cancelled after the timing assertion");
    main_server.join().expect("main server");
    title_server.join().expect("title server");
    assert!(run_error.is_cancelled());
    assert!(
        main_was_blocked,
        "main run settled before the remainder of its provider response was released"
    );
    assert!(!event_title.is_empty());
    assert_eq!(
        state
            .session_summary(&session_id)
            .await
            .expect("summary")
            .and_then(|summary| summary.title),
        Some(event_title)
    );
}

#[tokio::test]
pub(crate) async fn non_web_streaming_run_returns_before_post_main_title_generation_finishes() {
    let temp = tempdir().expect("temp");
    let home = home_dir(&temp);
    let cwd = temp.path().join("work");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&cwd).expect("cwd");

    let main_listener = TcpListener::bind("127.0.0.1:0").expect("bind main");
    let main_address = main_listener.local_addr().expect("main address");
    let title_listener = TcpListener::bind("127.0.0.1:0").expect("bind title");
    let title_address = title_listener.local_addr().expect("title address");
    let (title_started_tx, title_started_rx) = tokio::sync::oneshot::channel();
    let (release_title_tx, release_title_rx) = std::sync::mpsc::channel();
    let main_server = thread::spawn(move || {
        let (mut main_stream, _) = main_listener.accept().expect("main request");
        let _ = read_http_request(&mut main_stream);
        write_test_sse(&mut main_stream, "Hi from the main turn.");
    });
    let title_server = thread::spawn(move || {
        let (mut title_stream, _) = title_listener.accept().expect("title request");
        let _ = read_http_request(&mut title_stream);
        title_started_tx.send(()).expect("title started");
        release_title_rx.recv().expect("release title");
        write_test_sse(&mut title_stream, "Greeting session");
    });
    write_config(
        home.join("config.toml"),
        &format!(
            r#"
model = "nonblockingmain/main"

[provider.nonblockingmain]
api = "http://{main_address}/v1"

[provider.nonblockingmain.models.main]

[provider.nonblockingtitle]
api = "http://{title_address}/v1"

[provider.nonblockingtitle.models.title]

[auxiliary.title_generation]
provider = "nonblockingtitle"
model = "title"
"#,
        ),
    )
    .expect("config");

    let mut options = base_options(&temp).await;
    options.cwd = cwd;
    options.model = Some("nonblockingmain/main".to_string());
    options.no_agents = true;
    options.no_skills = true;
    let state = options.state.clone();
    let (title_event_tx, mut title_event_rx) = tokio::sync::mpsc::unbounded_channel();
    let stream: RunStreamSink = Arc::new(move |event| {
        if let Some(value) = event.legacy_value()
            && value.get("type").and_then(Value::as_str) == Some("session_title_changed")
        {
            let _ = title_event_tx.send(value.clone());
        }
    });
    let run =
        tokio::spawn(async move { run_live_streaming(options, "run", &["run"], stream).await });

    let result = tokio::time::timeout(Duration::from_secs(5), run)
        .await
        .expect("streaming run remained active while title generation was blocked")
        .expect("run task")
        .expect("streaming run");
    tokio::time::timeout(Duration::from_secs(5), title_started_rx)
        .await
        .expect("detached title request timeout")
        .expect("title request started");
    state
        .set_session_title(&result.session_id, "Manual title")
        .await
        .expect("manual title");
    release_title_tx.send(()).expect("release title");
    main_server.join().expect("main server");
    title_server.join().expect("title server");

    let event = tokio::time::timeout(Duration::from_secs(2), title_event_rx.recv())
        .await
        .expect("detached title event timeout")
        .expect("detached title event");
    assert_eq!(event["title"], "Manual title");
    assert_eq!(
        state
            .session_summary(&result.session_id)
            .await
            .expect("summary")
            .and_then(|summary| summary.title)
            .as_deref(),
        Some("Manual title")
    );
}

fn write_test_sse(stream: &mut std::net::TcpStream, content: &str) {
    let body = format!(
        "data: {{\"choices\":[{{\"delta\":{{\"content\":{}}},\"finish_reason\":\"stop\"}}]}}\n\ndata: [DONE]\n\n",
        serde_json::to_string(content).expect("content json")
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .expect("write response");
}

#[tokio::test]
pub(crate) async fn visible_session_source_title_rules_match_history_sources() {
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
        crate::thread_lineage::TUI_SIDE_CONVERSATION_SESSION_SOURCE,
        crate::thread_lineage::WEB_SIDE_CONVERSATION_SESSION_SOURCE,
    ] {
        assert!(
            !visible_session_source_allows_auto_title(source),
            "{source}"
        );
    }
}

#[tokio::test]
pub(crate) async fn first_use_empty_visible_session_materializes_model_and_metadata() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = StateRuntime::open(&db).await.expect("store");
    let session_id = store
        .create_session_with_metadata(&cwd, "web", "pending", "pending", None)
        .await
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
    .await
    .expect("first use");

    assert!(materialized);
    let summary = store
        .session_summary(&session_id)
        .await
        .expect("summary")
        .expect("session");
    assert_eq!(summary.provider, "local");
    assert_eq!(summary.model, "test-model");
    assert_eq!(
        store.session_metadata(&session_id).await.expect("metadata"),
        Some(metadata)
    );
}

#[tokio::test]
pub(crate) async fn first_use_empty_visible_session_does_not_rewrite_existing_or_internal_sessions()
{
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let cwd = canonical_cwd(&temp.path().join("work")).expect("cwd");
    let store = StateRuntime::open(&db).await.expect("store");
    let non_empty = store
        .create_session_with_metadata(
            &cwd,
            "web",
            "existing-model",
            "existing-provider",
            Some(json!({ "existing": true })),
        )
        .await
        .expect("non empty");
    store
        .append_message(&non_empty, &user_message("hello", 1))
        .await
        .expect("message");
    let internal = store
        .create_session_with_metadata(
            &cwd,
            crate::thread_lineage::WEB_SIDE_CONVERSATION_SESSION_SOURCE,
            "internal-model",
            "internal-provider",
            None,
        )
        .await
        .expect("internal");
    let parent = store
        .create_session_with_metadata(&cwd, "web", "parent-model", "parent-provider", None)
        .await
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
        .await
        .expect("child");

    for session_id in [&non_empty, &internal, &child] {
        let materialized = crate::run::materialize_first_use_empty_session(
            &store,
            session_id,
            "replacement-provider",
            "replacement-model",
            json!({ "replacement": true }),
        )
        .await
        .expect("skip");
        assert!(!materialized, "{session_id}");
    }

    let non_empty_summary = store
        .session_summary(&non_empty)
        .await
        .expect("summary")
        .expect("session");
    assert_eq!(non_empty_summary.provider, "existing-provider");
    assert_eq!(non_empty_summary.model, "existing-model");
    assert_eq!(
        store.session_metadata(&non_empty).await.expect("metadata"),
        Some(json!({ "existing": true }))
    );
}

#[tokio::test]
pub(crate) async fn visible_first_turn_title_gate_accepts_created_or_first_use_empty_session() {
    assert!(crate::run::should_title_visible_first_turn(false, true));
    assert!(crate::run::should_title_visible_first_turn(true, false));
    assert!(!crate::run::should_title_visible_first_turn(false, false));
}

#[tokio::test]
pub(crate) async fn session_title_fallback_removes_selected_skill_markers() {
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
        inference_idle_timeout_secs: psychevo_ai::DEFAULT_INFERENCE_IDLE_TIMEOUT_SECS,
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
