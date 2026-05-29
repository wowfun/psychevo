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
        path: None,
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["summary,matrix".to_string()],
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
        path: Some(PathBuf::from("runs/test-coding/fake-pass")),
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["summary,matrix".to_string()],
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
    assert_eq!(scoped_payload["summary"]["total_trials"], 1);

    let explicit_output = temp.path().join("nested").join("explicit.html");
    let explicit = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root.clone()),
        path: Some(run.cells[0].cell_root.clone()),
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["summary".to_string()],
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
        path: Some(run.cells[0].cell_root.clone()),
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["summary".to_string()],
        format: None,
        output: Some(Some(markdown_output)),
    })
    .expect_err("markdown output fails");
    assert!(format!("{err:#}").contains("markdown view output was removed"));

    let external = temp.path().join("external");
    fs::create_dir_all(&external).expect("external");
    let err = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root),
        path: Some(external),
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["summary".to_string()],
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
}
