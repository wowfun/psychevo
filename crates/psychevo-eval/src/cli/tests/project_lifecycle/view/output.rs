#[allow(unused_imports)]
use super::*;

#[test]
pub(crate) fn view_output_optional_path_defaults_to_mirrored_workspace_views() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    let store_root = init_workspace(temp.path().join("evals"));
    let run = run_evaluation(RunRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        task_set: Some("local/rust-swe".to_string()),
        task: None,
        agent: None,
        overwrite: false,
        store_root: Some(store_root.clone()),
        output_root: None,
        include_artifacts: Vec::new(),
    })
    .expect("run");

    let default_html = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root.clone()),
        paths: Vec::new(),
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["comparison".to_string()],
        notes: Vec::new(),
        format: None,
        output: Some(None),
    })
    .expect("default html output");
    let default_html_path = store_root.join("views/test-coding/index.html");
    assert_eq!(
        default_html.stdout,
        format!("wrote {}\n", default_html_path.display())
    );
    assert!(
        fs::read_to_string(default_html_path)
            .expect("default html")
            .contains("<!doctype html>")
    );

    let scoped_json = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root.clone()),
        paths: vec![PathBuf::from("runs/test-coding/fake-pass")],
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["comparison".to_string()],
        notes: Vec::new(),
        format: Some(ViewFormat::Json),
        output: Some(None),
    })
    .expect("default json output");
    let scoped_json_path = store_root.join("views/test-coding/fake-pass/index.json");
    assert_eq!(
        scoped_json.stdout,
        format!("wrote {}\n", scoped_json_path.display())
    );
    let scoped_payload: Value =
        serde_json::from_str(&fs::read_to_string(scoped_json_path).expect("scoped json"))
            .expect("json payload");
    assert_eq!(scoped_payload["comparison"]["summary"]["total_trials"], 1);

    let multi_json = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root.clone()),
        paths: run
            .cells
            .iter()
            .take(2)
            .map(|cell| cell.cell_root.clone())
            .collect(),
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["comparison".to_string()],
        notes: Vec::new(),
        format: Some(ViewFormat::Json),
        output: Some(None),
    })
    .expect("default multi-path json output");
    assert!(multi_json.stdout.contains("/views/selections/"));
    let multi_path = PathBuf::from(
        multi_json
            .stdout
            .trim()
            .strip_prefix("wrote ")
            .expect("wrote prefix"),
    );
    assert!(multi_path.is_file());
    let multi_payload: Value =
        serde_json::from_str(&fs::read_to_string(multi_path).expect("multi json"))
            .expect("multi payload");
    assert_eq!(
        multi_payload["path_selections"]
            .as_array()
            .expect("paths")
            .len(),
        2
    );

    let explicit_output = temp.path().join("nested").join("explicit.html");
    let explicit = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root.clone()),
        paths: vec![run.cells[0].cell_root.clone()],
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["comparison".to_string()],
        notes: Vec::new(),
        format: None,
        output: Some(Some(explicit_output.clone())),
    })
    .expect("explicit output");
    assert_eq!(
        explicit.stdout,
        format!("wrote {}\n", explicit_output.display())
    );
    assert!(
        fs::read_to_string(explicit_output)
            .expect("explicit html")
            .contains("<!doctype html>")
    );

    let markdown_output = temp.path().join("nested").join("removed.md");
    let err = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root.clone()),
        paths: vec![run.cells[0].cell_root.clone()],
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["comparison".to_string()],
        notes: Vec::new(),
        format: None,
        output: Some(Some(markdown_output)),
    })
    .expect_err("markdown output fails");
    assert!(format!("{err:#}").contains("markdown view output was removed"));

    let external = temp.path().join("external-cell");
    copy_dir(&run.cells[0].cell_root, &external).expect("external cell");
    let err = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root),
        paths: vec![external],
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["comparison".to_string()],
        notes: Vec::new(),
        format: None,
        output: Some(None),
    })
    .expect_err("external default output fails");
    assert!(format!("{err:#}").contains("pass -o PATH"));
}

#[test]
pub(crate) fn view_output_cli_accepts_short_alias_and_optional_value() {
    let view = Cli::try_parse_from(["peval", "view", "-o"]).expect("-o parses");
    let Commands::View(args) = view.command else {
        panic!("expected view command");
    };
    assert_eq!(args.output, Some(None));

    let view = Cli::try_parse_from(["peval", "view", "-o", "out.html"]).expect("-o path parses");
    let Commands::View(args) = view.command else {
        panic!("expected view command");
    };
    assert_eq!(args.output, Some(Some(PathBuf::from("out.html"))));

    let view = Cli::try_parse_from(["peval", "view", "--output"]).expect("--output parses");
    let Commands::View(args) = view.command else {
        panic!("expected view command");
    };
    assert_eq!(args.output, Some(None));

    let view = Cli::try_parse_from(["peval", "view", "-p", "one", "--path", "two"])
        .expect("repeat paths parse");
    let Commands::View(args) = view.command else {
        panic!("expected view command");
    };
    assert_eq!(args.paths, vec![PathBuf::from("one"), PathBuf::from("two")]);

    let view = Cli::try_parse_from([
        "peval",
        "view",
        "--note",
        "0=report = note",
        "--note",
        "1=first trial",
    ])
    .expect("repeat notes parse");
    let Commands::View(args) = view.command else {
        panic!("expected view command");
    };
    assert_eq!(
        parse_view_notes(&args.notes)
            .expect("notes parse")
            .iter()
            .map(|note| (note.index, note.markdown.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "report = note"), (1, "first trial")]
    );

    let err = parse_view_notes(&["missing-equals".to_string()]).expect_err("note fails");
    assert!(format!("{err:#}").contains("INDEX=TEXT"));
    let err = parse_view_notes(&["abc=note".to_string()]).expect_err("note index fails");
    assert!(format!("{err:#}").contains("invalid note index"));
}
