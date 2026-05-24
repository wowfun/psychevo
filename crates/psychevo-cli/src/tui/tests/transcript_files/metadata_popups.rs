#[allow(unused_imports)]
pub(crate) use super::*;

#[test]
pub(crate) fn streaming_tool_calls_keep_partial_arguments_as_null() {
    let event = serde_json::json!({
        "type": "message_update",
        "message": {
            "role": "assistant",
            "content": [{
                "type": "tool_call",
                "id": "",
                "name": "write",
                "arguments": null,
                "arguments_json": "{\"path\":\"report.md\"",
                "arguments_error": "EOF while parsing",
                "content_index": 0,
                "call_index": 0
            }]
        }
    });

    let calls = streaming_tool_calls_from_event(&event);

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, None);
    assert_eq!(calls[0].position_key, "pos:0:0");
    assert!(calls[0].args.is_null());
}

#[test]
pub(crate) fn turn_meta_omits_tokens_and_uses_prefixless_debug_parts() {
    let usage = serde_json::json!({
        "input_tokens": 2,
        "output_tokens": 3,
        "total_tokens": 5
    });
    let default = turn_meta_text(TurnMetaProjection {
        mode: "default",
        provider: "provider",
        model: "model",
        started: None,
        usage: Some(&usage),
        metadata: None,
        accounting: None,
        failures: 0,
        interrupted: false,
        debug: false,
    });
    assert_eq!(default, "provider/model");
    let metadata = serde_json::json!({"provider_response_id":"resp"});
    let debug = turn_meta_text(TurnMetaProjection {
        mode: "plan",
        provider: "provider",
        model: "model",
        started: None,
        usage: Some(&usage),
        metadata: Some(&metadata),
        accounting: None,
        failures: 0,
        interrupted: false,
        debug: true,
    });
    assert!(debug.contains("usage 2 input"));
    assert!(debug.contains("3 output"));
    assert!(debug.contains("metadata response resp"));
    assert!(debug.ends_with("plan"));
    assert!(!debug.contains('='));
}

#[test]
pub(crate) fn turn_meta_omits_accounting_cost() {
    let accounting = serde_json::json!({
        "estimated_cost_nanodollars": 42_000,
        "pricing_source": "catalog"
    });
    let meta = turn_meta_text(TurnMetaProjection {
        mode: "default",
        provider: "provider",
        model: "model",
        started: None,
        usage: None,
        metadata: None,
        accounting: Some(&accounting),
        failures: 0,
        interrupted: false,
        debug: false,
    });

    assert_eq!(meta, "provider/model");
    assert!(!meta.contains("cost"));
}

#[test]
pub(crate) fn turn_meta_prefers_completed_elapsed_metadata() {
    let metadata = serde_json::json!({"elapsed_ms": 120});
    let stale_started = Instant::now()
        .checked_sub(Duration::from_secs(5))
        .expect("instant");

    let meta = turn_meta_text(TurnMetaProjection {
        mode: "default",
        provider: "provider",
        model: "model",
        started: Some(stale_started),
        usage: None,
        metadata: Some(&metadata),
        accounting: None,
        failures: 0,
        interrupted: false,
        debug: true,
    });

    assert!(meta.contains("0s"));
    assert!(!meta.contains("5."));
    assert!(!meta.contains("metadata elapsed"));
}

#[test]
pub(crate) fn turn_meta_formats_persisted_elapsed_minutes() {
    let metadata = serde_json::json!({"elapsed_ms": 65_000});

    let meta = turn_meta_text(TurnMetaProjection {
        mode: "default",
        provider: "provider",
        model: "model",
        started: None,
        usage: None,
        metadata: Some(&metadata),
        accounting: None,
        failures: 0,
        interrupted: false,
        debug: false,
    });

    assert_eq!(meta, "provider/model  1m05s");
}

#[test]
pub(crate) fn turn_meta_places_variant_after_model_and_filters_debug_duplicate() {
    let metadata = serde_json::json!({
        "elapsed_ms": 120,
        "reasoning_effort": "high",
        "provider_response_id": "resp"
    });
    let usage = serde_json::json!({"input_tokens": 2});

    let meta = turn_meta_text(TurnMetaProjection {
        mode: "plan",
        provider: "provider",
        model: "model",
        started: None,
        usage: Some(&usage),
        metadata: Some(&metadata),
        accounting: None,
        failures: 1,
        interrupted: false,
        debug: true,
    });

    assert_eq!(
        meta,
        "provider/model high  0s  1 failure  usage 2 input  metadata response resp  plan"
    );
}

#[test]
pub(crate) fn slash_completion_completes_command_prefixes() {
    assert_eq!(slash_completion("/he").as_deref(), Some("/help"));
    assert_eq!(slash_completion("/ren").as_deref(), Some("/rename"));
    assert_eq!(slash_completion("/rn"), None);
    assert_eq!(slash_completion("/mo").as_deref(), Some("/mode"));
    assert_eq!(slash_completion("/model"), None);
    assert_eq!(slash_completion("hello"), None);
    assert_eq!(slash_completion("/he\nthere"), None);
}

#[test]
pub(crate) fn file_token_detection_covers_boundaries_and_unicode() {
    let cases = vec![
        ("@", 0, 1, Some("")),
        ("@file.txt", 0, 4, Some("file.txt")),
        ("hello @world test", 0, 8, Some("world")),
        (
            "@icons/icon@2x.png",
            0,
            "@icons/icon@2x.png".chars().count(),
            Some("icons/icon@2x.png"),
        ),
        (
            "test　@İstanbul",
            0,
            "test　@İstanbul".chars().count(),
            Some("İstanbul"),
        ),
        ("foo@bar", 0, "foo@bar".chars().count(), None),
        ("@ hello", 0, 2, None),
        (
            "first @one\nsecond @two",
            1,
            "second @two".chars().count(),
            Some("two"),
        ),
    ];

    for (input, row, col, expected) in cases {
        let textarea = textarea_with_lines_and_cursor(
            input.split('\n').map(ToString::to_string).collect(),
            row,
            col,
        );
        let actual = current_file_token(&textarea).map(|token| token.query);
        assert_eq!(
            actual.as_deref(),
            expected,
            "input={input:?} row={row} col={col}"
        );
    }
}

#[test]
pub(crate) fn file_token_replacement_quotes_paths_with_spaces() {
    let mut textarea = textarea_with_text("open @src");
    assert!(replace_current_file_token(&mut textarea, "src/main.rs"));
    assert_eq!(textarea_text(&textarea), "open src/main.rs ");

    let mut textarea = textarea_with_text("open @docs");
    assert!(replace_current_file_token(
        &mut textarea,
        "docs/reference notes.md"
    ));
    assert_eq!(
        textarea_text(&textarea),
        "open \"docs/reference notes.md\" "
    );
}

#[test]
pub(crate) fn file_search_returns_workdir_relative_paths_and_respects_gitignore() {
    let temp = tempdir().expect("temp");
    let root = temp.path();
    fs::create_dir_all(root.join("src")).expect("src dir");
    fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("main");
    fs::write(root.join(".hidden.rs"), "hidden\n").expect("hidden");
    fs::write(root.join("ignored.txt"), "ignored\n").expect("ignored");
    fs::write(root.join(".gitignore"), "ignored.txt\n").expect("gitignore");
    fs::create_dir_all(root.join(".git/objects")).expect("git dir");
    fs::write(root.join(".git/config"), "private\n").expect("git config");
    let cancel = AtomicBool::new(false);

    let src_matches = search_workdir_files(root, "src", &cancel);
    assert_eq!(
        src_matches.first(),
        Some(&FileSearchMatch {
            path: "src".to_string(),
            kind: FileSearchMatchKind::Directory,
        })
    );
    assert!(src_matches.iter().any(|entry| entry.path == "src/main.rs"));

    let ignored_matches = search_workdir_files(root, "ignored", &cancel);
    assert!(ignored_matches.is_empty(), "{ignored_matches:#?}");

    let hidden_matches = search_workdir_files(root, "hidden", &cancel);
    assert_eq!(hidden_matches.len(), 1);
    assert_eq!(hidden_matches[0].path, ".hidden.rs");

    let git_matches = search_workdir_files(root, "config", &cancel);
    assert!(
        git_matches
            .iter()
            .all(|entry| !entry.path.starts_with(".git/")),
        "{git_matches:#?}"
    );
}

#[test]
pub(crate) fn stale_file_search_results_are_ignored() {
    let mut state = FileSearchState::new();
    state.generation = 2;
    state.popup = Some(FileSearchPopupState {
        query: "new".to_string(),
        matches: Vec::new(),
        selected: 0,
        waiting: true,
    });
    state
        .tx
        .send(FileSearchResult {
            generation: 1,
            query: "old".to_string(),
            matches: vec![FileSearchMatch {
                path: "old.rs".to_string(),
                kind: FileSearchMatchKind::File,
            }],
        })
        .expect("send stale");
    state
        .tx
        .send(FileSearchResult {
            generation: 2,
            query: "new".to_string(),
            matches: vec![FileSearchMatch {
                path: "new.rs".to_string(),
                kind: FileSearchMatchKind::File,
            }],
        })
        .expect("send current");

    state.drain_results();

    let popup = state.popup.expect("popup");
    assert_eq!(
        popup.matches,
        vec![FileSearchMatch {
            path: "new.rs".to_string(),
            kind: FileSearchMatchKind::File,
        }]
    );
    assert!(!popup.waiting);
}

#[test]
pub(crate) fn bottom_panel_row_right_aligns_detail_with_wide_title() {
    let row = BottomSelectionRow {
        label: "当前模式询问".to_string(),
        description: Some("deepseek/deepseek-v4-pro  messages=2".to_string()),
        detail: Some("08:50".to_string()),
        group: None,
        search_text: String::new(),
        is_current: false,
        is_default: false,
        style: BottomRowStyle::Normal,
        footer: None,
        value: BottomSelectionValue::Session("session-a".to_string()),
    };

    let width = 54;
    let text = line_text(&bottom_panel_row(
        &row,
        false,
        width,
        false,
        Duration::default(),
    ));

    assert!(text.ends_with("08:50"));
    assert_eq!(UnicodeWidthStr::width(text.as_str()), usize::from(width));
}
