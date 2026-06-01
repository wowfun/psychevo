#[allow(unused_imports)]
use super::*;

#[test]
pub(crate) fn view_all_outputs_full_diagnostics_with_bounded_previews() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    let solver = fixture.join("solver.sh");
    fs::write(
        &solver,
        r#"printf '%s\n' '{"type":"system_prompt","system_prompt":"follow visible policy"}'
printf '%s\n' '{"type":"user_message","message":"complete task"}'
printf '%s\n' '{"type":"assistant_message","message":"I will write status","reasoning":"plan carefully","model":"test-model","usage":{"input_tokens":3,"output_tokens":4,"cache_read_tokens":1,"cost_usd":0.01}}'
printf '%s\n' '{"type":"tool_call","tool_call_id":"call-1","function_name":"write_file","arguments":{"path":"status.txt"}}'
printf '%s\n' '{"type":"tool_result","tool_call_id":"call-1","result":{"diff":"diff --git a/status.txt b/status.txt\n--- a/status.txt\n+++ b/status.txt\n@@ -1 +1 @@\n-pending\n+fixed\n","ok":true}}'
printf fixed > status.txt
"#,
    )
    .expect("solver");
    fs::write(
        fixture.join("eval.toml"),
        format!(
            r#"schema_version = 5
id = "test-coding-eval"
name = "test-coding eval"

[benchmark]
path = "benchmark.toml"

[select]
agents = ["jsonl-solver"]
sets = ["local/rust-swe"]

[[agents]]
id = "jsonl-solver"
kind = "command"
command = {{ command = "sh", args = ["{}"], model = "test-model" }}
"#,
            solver.display()
        ),
    )
    .expect("eval");
    let store_root = init_workspace(temp.path().join("evals"));
    let run = run_evaluation(RunRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        task_set: Some("local/rust-swe".to_string()),
        task: None,
        agent: Some("jsonl-solver".to_string()),
        overwrite: false,
        store_root: Some(store_root.clone()),
        output_root: None,
        include_artifacts: vec!["workspace".to_string()],
    })
    .expect("run");
    let cell_root = &run.cells[0].cell_root;
    assert!(cell_root.join("prompt.md").is_file());
    fs::create_dir_all(cell_root.join("logs")).expect("logs");
    let big_log = format!(
        "visible-start\n{}secret-tail\n",
        "a".repeat(2 * 1024 * 1024)
    );
    fs::write(cell_root.join("logs/big.log"), big_log).expect("big log");
    fs::write(
        cell_root.join("notes.md"),
        "## Human note\nPersistent <script>evil()</script>\n- kept with the cell\n",
    )
    .expect("notes");

    let json = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root.clone()),
        paths: vec![cell_root.clone()],
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["all".to_string()],
        notes: vec![
            "0=Report **context**".to_string(),
            "1=CLI **note** with a=b".to_string(),
        ],
        format: Some(ViewFormat::Json),
        output: None,
    })
    .expect("json view");
    let payload: Value = serde_json::from_str(&json.stdout).expect("json payload");
    assert_eq!(payload["schema_version"], VIEW_SCHEMA_VERSION);
    assert_eq!(
        payload["includes"],
        json!(["core", "comparison", "annotations", "attachments"])
    );
    assert!(payload.get("logs").is_none());
    assert!(payload.get("diff").is_none());
    assert!(payload.get("trials").is_none());
    assert!(payload.get("usage").is_none());
    assert!(payload.get("warnings").is_none());
    assert!(payload.get("artifacts").is_none());
    assert!(payload.get("summary").is_none());
    assert!(payload.get("matrix").is_none());
    assert!(payload.get("leaderboard").is_none());
    assert!(payload.get("default_metric").is_none());
    assert!(json.stdout.contains("\"trial_key\""));
    assert!(json.stdout.contains("\"matrix_cell_key\""));
    assert!(!json.stdout.contains("\"atif\""));
    assert!(payload["trajectory_meta"][0].get("cell_root").is_none());
    assert!(!json.stdout.contains("\"cell_key\""));
    assert_eq!(payload["schema_version"], VIEW_SCHEMA_VERSION);
    assert!(
        !payload["comparison"]["leaderboard"]["entries"]
            .as_array()
            .expect("leaderboard entries")
            .is_empty()
    );
    assert_eq!(
        payload["comparison"]["leaderboard"]["entries"][0]["total_trials"],
        1
    );
    assert_eq!(
        payload["comparison"]["leaderboard"]["entries"][0]["successes"],
        1
    );
    assert!(
        payload["trajectory_meta"][0]["trial_key"]
            .as_str()
            .unwrap()
            .ends_with(":t001")
    );
    assert!(
        payload["comparison"]["matrix"]["cells"][0]["trial_keys"]
            .as_array()
            .unwrap()
            .len()
            == 1
    );
    let artifact_refs = payload["attachments"]["artifacts"][0]["refs"]
        .as_array()
        .expect("artifact refs");
    assert!(artifact_refs.len() > 3);
    assert!(artifact_refs.iter().all(|artifact| {
        !Path::new(artifact["relative_path"].as_str().expect("path string")).is_absolute()
    }));
    assert!(
        payload["attachments"]["artifacts"][0]
            .get("paths")
            .is_none()
    );
    assert!(
        payload["trajectory_meta"][0]["cell_root_relative"]
            .as_str()
            .expect("cell root relative")
            .starts_with("runs/")
    );
    assert_eq!(payload["trajectory_meta"][0]["score_passed"], true);
    assert!(
        payload["trajectory_meta"][0]["score_message"]
            .as_str()
            .is_some()
    );
    assert_eq!(payload["trajectory"][0]["schema_version"], "ATIF-v1.7");
    assert_eq!(
        payload["trajectory"][0]["agent"]["model_name"],
        "test-model"
    );
    assert_eq!(
        payload["trajectory"][0]["trajectory_id"],
        payload["trajectory_meta"][0]["trial_key"]
    );
    assert_eq!(
        payload["trajectory"][0]["final_metrics"]["total_prompt_tokens"],
        3
    );
    assert!(
        !payload["trajectory"][0]["steps"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        payload["trajectory"][0]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step["source"] == "system")
    );
    assert!(
        payload["trajectory"][0]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step["reasoning_content"] == "plan carefully")
    );
    assert!(
        payload["trajectory"][0]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step["tool_calls"][0]["tool_call_id"] == "call-1")
    );
    assert!(
        payload["trajectory"][0]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step["observation"]["results"][0]["source_call_id"] == "call-1")
    );
    assert!(
        payload["trajectory_meta"][0]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|step| !step["timestamp_ms"].is_null())
            .skip(1)
            .any(|step| !step["elapsed_ms"].is_null() && !step["duration_ms"].is_null())
    );
    assert_eq!(
        payload["trajectory_meta"][0]["trial_key"],
        payload["trajectory"][0]["trajectory_id"]
    );
    assert_eq!(
        payload["trajectory_meta"][0]["data_ref"]["relative_path"],
        "trajectory.jsonl"
    );
    assert!(
        payload["trajectory_meta"][0]
            .get("system_exposed")
            .is_none()
    );
    assert!(
        payload["trajectory_meta"][0]
            .get("reasoning_exposed")
            .is_none()
    );
    assert!(
        payload["trajectory"][0]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|step| step.get("extra").is_none() && step.get("llm_call_count").is_none())
    );
    assert_eq!(
        payload["annotations"]["analysis"][0]["status"], "missing",
        "analysis should not run providers"
    );
    let report_notes = payload["annotations"]["report_notes"]
        .as_array()
        .expect("report notes");
    assert_eq!(report_notes.len(), 1);
    assert_eq!(report_notes[0]["label"], "Report note 1");
    assert_eq!(report_notes[0]["markdown"], "Report **context**");
    let notes = payload["annotations"]["notes"].as_array().expect("notes");
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0]["source"], "cell");
    assert_eq!(notes[0]["label"], "notes.md");
    assert_eq!(notes[0]["source_ref"]["kind"], "note");
    assert!(
        notes[0]["markdown"]
            .as_str()
            .expect("cell note")
            .contains("Persistent <script>evil()</script>")
    );
    assert_eq!(notes[1]["source"], "cli");
    assert_eq!(notes[1]["label"], "CLI note 1");
    assert!(
        notes[1]["markdown"]
            .as_str()
            .expect("cli note")
            .contains("a=b")
    );

    let html = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root.clone()),
        paths: vec![cell_root.clone()],
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["all".to_string()],
        notes: vec![
            "0=Report **context**".to_string(),
            "1=CLI **note** with a=b".to_string(),
        ],
        format: Some(ViewFormat::Html),
        output: None,
    })
    .expect("html view");
    assert!(html.stdout.contains("Visible Trial Heatmap"));
    assert!(html.stdout.contains("<h3>Run</h3>"));
    assert!(html.stdout.contains("<h3>Result</h3>"));
    assert!(!html.stdout.contains("<h3>Files</h3>"));
    assert!(html.stdout.contains("agent / model"));
    assert!(html.stdout.contains("test-model"));
    assert!(html.stdout.contains("trajectory_meta"));
    assert!(html.stdout.contains("Report note 1"));
    assert!(html.stdout.contains("Report **context**"));
    assert!(html.stdout.contains("Notes"));
    assert!(html.stdout.contains("CLI **note** with a=b"));
    assert!(html.stdout.contains("renderSelectedNotes(trial.trial_key)"));
    assert!(
        html.stdout
            .contains("renderSelectedAnalysis(trial.trial_key)")
    );
    assert!(!html.stdout.contains("Manual Notes"));
    assert!(!html.stdout.contains("<script>evil()"));
    assert!(!html.stdout.contains("id=\"search\""));
    assert!(!html.stdout.contains("status-filter"));
    assert!(!html.stdout.contains("task-filter"));
    assert!(!html.stdout.contains("metric-select"));
    assert!(
        html.stdout.find("leaderboard").expect("leaderboard")
            < html.stdout.find("matrix-title").expect("matrix")
    );
    assert!(!html.stdout.contains("Basic Information"));
    assert!(!html.stdout.contains("Trajectory Information"));
    assert!(!html.stdout.contains("Final Metrics"));
    assert!(!html.stdout.contains("Outcome Stack"));
    assert!(!html.stdout.contains("Evaluation Metrics"));
    assert!(!html.stdout.contains("<h3>Paths</h3>"));
    assert!(html.stdout.contains("Agent / Model Comparison"));
    assert!(html.stdout.contains("leaderboard-aggregate"));
    assert!(html.stdout.contains("Pass Rate"));
    assert!(html.stdout.contains("hasVisibleVariant"));
    assert!(html.stdout.contains("comparisonColumns(comparisonRows)"));
    assert!(html.stdout.contains("trialColumns(trialRows)"));
    assert!(html.stdout.contains("columns.unshift(variantColumn())"));
    assert!(html.stdout.contains("title=\"${esc(column.label)}\""));
    assert!(
        html.stdout
            .contains("cellTitle: row => notesFullTextForKeys")
    );
    assert!(html.stdout.contains("note-snippet"));
    assert!(!html.stdout.contains("Resolution Rate"));
    assert!(html.stdout.contains("filterable"));
    assert!(html.stdout.contains("sortable"));
    assert!(html.stdout.contains("sort-label"));
    assert!(html.stdout.contains("data-table-sort"));
    assert!(html.stdout.contains("data-table-filter"));
    assert!(html.stdout.contains("multi-filter"));
    assert!(html.stdout.contains("data-filter-value"));
    assert!(html.stdout.contains("renderStepsHeader"));
    assert!(html.stdout.contains("bindStepControls"));
    assert!(html.stdout.contains("data-step-action=\"toggle\""));
    assert!(!html.stdout.contains("data-step-action=\"expand\""));
    assert!(!html.stdout.contains("data-step-action=\"collapse\""));
    assert!(html.stdout.contains("function refreshStepToggleButton()"));
    assert!(html.stdout.contains("rows.every(row => row.open)"));
    assert!(
        html.stdout
            .contains("const shouldOpen = !rows.every(row => row.open)")
    );
    assert!(
        html.stdout
            .contains("button.textContent = allOpen ? \"Collapse all\" : \"Expand all\"")
    );
    assert!(html.stdout.contains("Expand all"));
    assert!(html.stdout.contains("Collapse all"));
    assert!(html.stdout.contains(".steps-head"));
    assert!(html.stdout.contains(".step-toggle-button"));
    assert!(html.stdout.contains("&#9650;"));
    assert!(html.stdout.contains("&#9660;"));
    assert!(html.stdout.contains("missing-metric"));
    assert!(!html.stdout.contains("Evidence Ledger"));
    assert!(!html.stdout.contains("Showing ${filtered.length}"));
    assert!(!html.stdout.contains("table-meta"));
    assert!(html.stdout.contains("data-row-trial"));
    assert!(html.stdout.contains("clickable-row"));
    assert!(html.stdout.contains("selected-row"));
    assert!(
        html.stdout
            .contains("function trialKeyInCell(cell, trialKey)")
    );
    assert!(
        html.stdout
            .contains("function selectedTrialInVisibleCells(visible)")
    );
    assert!(
        html.stdout
            .contains("if (!selectedTrialInVisibleCells(visible))")
    );
    assert!(
        html.stdout
            .contains("trialKeyInCell(cell, state.selectedTrial) ? \"selected\"")
    );
    assert!(
        html.stdout
            .contains("if (!trialKeyInCell(cell, state.selectedTrial))")
    );
    assert!(html.stdout.contains(
        "const cell = visible.find(item => item.representative_trial_key === button.dataset.trial)"
    ));
    assert!(
        !html.stdout.contains(
            "visible.some(cell => cell.representative_trial_key === state.selectedTrial)"
        )
    );
    assert!(
        html.stdout
            .contains("renderTrialSiblingSwitcher(trial.trial_key)")
    );
    assert!(html.stdout.contains("class=\"trial-switcher\""));
    assert!(html.stdout.contains("data-switch-trial"));
    assert!(html.stdout.contains("bindTrialSwitcher"));
    assert!(html.stdout.contains("trialKeys.length <= 1"));
    assert!(html.stdout.contains("hasMultiTrialMatrixCell"));
    assert!(
        html.stdout
            .contains("columns.unshift(trialIdentityColumn())")
    );
    assert!(html.stdout.contains("trial-id-chip"));
    assert!(html.stdout.contains("function averageOptional(values)"));
    assert!(
        html.stdout
            .contains("average_tokens: averageOptional(taskTrials.map")
    );
    assert!(
        html.stdout
            .contains("average_cost_usd: averageOptional(taskTrials.map")
    );
    assert!(
        !html
            .stdout
            .contains("total_tokens: sumOptional(taskTrials.map")
    );
    assert!(
        !html
            .stdout
            .contains("total_cost_usd: sumOptional(taskTrials.map")
    );
    assert!(html.stdout.contains("key: \"average_tokens\""));
    assert!(html.stdout.contains("key: \"average_cost_usd\""));
    assert!(html.stdout.contains("renderSelectedEvidence(trial)"));
    assert!(html.stdout.contains("Score Details"));
    assert!(html.stdout.contains("Usage Breakdown"));
    assert!(html.stdout.contains("Cell Root"));
    assert!(html.stdout.contains("Artifacts"));
    assert!(
        !html
            .stdout
            .contains("{ key: \"cell_root_relative\", label: \"Cell Root\"")
    );
    assert!(!html.stdout.contains("{ key: \"rank\", label: \"Rank\""));
    assert!(
        html.stdout
            .contains("notes.map(note => String(note.markdown || \"\").trim())")
    );
    assert!(!html.stdout.contains("note.label || note.source"));
    assert!(
        html.stdout
            .contains("<article class=\"manual-note\"><div class=\"note-body\">")
    );
    assert!(
        !html
            .stdout
            .contains("renderNotesEvidence(view.notes || [])")
    );
    assert!(
        !html
            .stdout
            .contains("renderAnalysisEvidence(view.analysis || [])")
    );
    assert!(
        !html
            .stdout
            .contains("${renderArtifactsEvidence(view.artifacts || [])}")
    );
    assert!(!html.stdout.contains("Absolute Path"));
    assert!(!html.stdout.contains("<th>Kind</th>"));
    assert!(!html.stdout.contains("<th>MIME</th>"));
    assert!(!html.stdout.contains("<th>Bytes</th>"));
    assert!(!html.stdout.contains("Logs"));
    assert!(!html.stdout.contains("Diff"));
    assert!(!html.stdout.contains("Scoring"));
    assert!(html.stdout.contains("Reasoning"));
    assert!(html.stdout.contains("Tool Calls"));
    assert!(html.stdout.contains("tool success / total"));
    assert!(html.stdout.contains("renderStepRail(step, meta)"));
    assert!(!html.stdout.contains("renderStepMetrics(step, meta)"));
    assert!(!html.stdout.contains("<h4>Metrics</h4>"));
    assert!(html.stdout.contains("renderToolTiming(toolMeta)"));
    assert!(html.stdout.contains("renderToolNameChip(tool, toolMeta)"));
    assert!(html.stdout.contains("toolExecutionText(toolMeta)"));
    assert!(html.stdout.contains("hasMetricValue"));
    assert!(
        html.stdout
            .contains("const fmtMs = value => fmtDurationMs(value);")
    );
    assert!(
        html.stdout
            .contains("const fmtRailMs = value => fmtDurationMs(value);")
    );
    assert!(html.stdout.contains("function fmtDurationMs(value)"));
    assert!(html.stdout.contains("function trialWallDurationMs(trial)"));
    assert!(
        html.stdout
            .contains("trial.finished_at_ms) - Number(trial.started_at_ms)")
    );
    assert!(
        html.stdout
            .contains("[\"wall duration\", fmtMs(trialWallDurationMs(trial))]")
    );
    assert!(html.stdout.contains("seconds.toFixed(1)"));
    assert!(!html.stdout.contains("\"<1s\""));
    assert!(
        !html
            .stdout
            .contains("const fmtMs = value => value === null")
    );
    assert!(html.stdout.contains("step span"));
    assert!(html.stdout.contains("tool exec"));
    assert!(!html.stdout.contains("tool time"));
    assert!(html.stdout.contains("fmtRailMs(meta?.duration_ms)"));
    assert!(html.stdout.contains("fmtRailMs(meta?.elapsed_ms)"));
    assert!(html.stdout.contains("stepToolLabels(step, meta)"));
    assert!(html.stdout.contains("rail-tools"));
    assert!(html.stdout.contains("rail-time"));
    assert!(html.stdout.contains("rail-chip-tools"));
    assert!(html.stdout.contains("rail-chip-tool-list"));
    assert!(html.stdout.contains("rail-chip-step-time"));
    assert!(html.stdout.contains("rail-chip-elapsed-time"));
    assert!(html.stdout.contains("title=\"step span\""));
    assert!(
        html.stdout
            .contains("step ${esc(fmtRailMs(meta?.duration_ms))}")
    );
    assert!(
        html.stdout
            .contains("elapsed ${esc(fmtRailMs(meta?.elapsed_ms))}")
    );
    assert!(!html.stdout.contains("step span / elapsed"));
    assert!(html.stdout.contains("rail-chip-tokens"));
    assert!(
        html.stdout
            .contains("${esc(toolCallRatio(toolCalls, toolErrors))} tools")
    );
    assert!(html.stdout.contains("${esc(text)}"));
    assert!(
        !html
            .stdout
            .contains("tools ${toolCallRatio(toolCalls, toolErrors)}")
    );
    assert!(!html.stdout.contains("duration ${fmtMs(meta.duration_ms)}"));
    assert!(!html.stdout.contains("step ${fmtMs(meta.duration_ms)}"));
    assert!(!html.stdout.contains("elapsed ${fmtMs(meta.elapsed_ms)}"));
    assert!(!html.stdout.contains("tool ${fmtMs(toolExecutionMs)}"));
    assert!(!html.stdout.contains("stepToolExecutionMs(meta)"));
    assert!(
        html.stdout
            .find("if (step.reasoning_content)")
            .expect("reasoning rendered")
            < html
                .stdout
                .find("const message = valuePreview(step.message);")
                .expect("message rendered")
    );
    assert!(html.stdout.contains("(No Message)"));
    assert!(!html.stdout.contains("(Empty Message)"));
    assert!(!html.stdout.contains("meta?.summary ||"));
    assert!(
        !html
            .stdout
            .contains("prompt ${fmtNum(step.metrics?.prompt_tokens)}")
    );
    assert!(
        !html
            .stdout
            .contains("completion ${fmtNum(step.metrics?.completion_tokens)}")
    );
    assert!(
        !html
            .stdout
            .contains("cached ${fmtNum(step.metrics?.cached_tokens)}")
    );
    assert!(
        !html
            .stdout
            .contains("cost ${fmtCost(step.metrics?.cost_usd)}")
    );
    assert!(
        !html
            .stdout
            .contains("fmtNum(stepTokenTotal(step, meta))} tok")
    );
    assert!(html.stdout.contains("Observations"));
    assert!(html.stdout.contains("System Prompt"));
    assert!(html.stdout.contains("follow visible policy"));
    assert!(html.stdout.contains("message-block"));
    assert!(html.stdout.contains("reasoning-block"));
    assert!(!html.stdout.contains("<section class=\"evidence-ledger\""));
    assert!(
        html.stdout
            .contains("<div class=\"step-body\">${renderStepBlocks(step, meta)}</div>")
    );
    assert!(!html.stdout.contains("<aside"));
    assert!(!html.stdout.contains("class=\"inspector\""));
    assert!(
        !html
            .stdout
            .contains("<details class=\"block message-block\"")
    );
    assert!(
        !html
            .stdout
            .contains("<details class=\"block reasoning-block\"")
    );
    assert!(!html.stdout.contains("<details class=\"evidence-ledger\""));
    assert!(!html.stdout.contains("secret-tail"));

    let escape = cell_root.parent().expect("cell parent").join("escape.txt");
    fs::write(&escape, "outside").expect("escape file");
    let err = safe_artifact_path(cell_root, Path::new("../escape.txt"))
        .expect_err("outside path rejected");
    assert!(format!("{err:#}").contains("escapes cell root"));

    fs::remove_file(cell_root.join("prompt.md")).expect("remove prompt");
    let legacy_workspace_prompt = cell_root.join("workspace/.peval/prompt.md");
    if legacy_workspace_prompt.exists() {
        fs::remove_file(legacy_workspace_prompt).expect("remove legacy workspace prompt");
    }
    fs::write(
        cell_root.join("trajectory.jsonl"),
        r#"{"schema_version":8,"sequence":0,"case_id":"case","kind":"assistant_message","message":"assistant","data":{"raw_event":{"type":"assistant_message","message":"no timestamp"}}}
"#,
    )
    .expect("legacy trajectory without timestamp");
    let legacy_prompt_view = build_view(ViewRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root),
        paths: vec![cell_root.clone()],
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec![ViewInclude::Core],
        notes: Vec::new(),
    })
    .expect("legacy prompt view");
    assert!(legacy_prompt_view.trajectory_meta[0].prompt_unavailable);
    assert_eq!(
        legacy_prompt_view.trajectory[0].steps[1].message.as_str(),
        Some("no timestamp")
    );
    assert!(
        legacy_prompt_view.trajectory_meta[0].steps[1]
            .timestamp_ms
            .is_none()
    );
    assert!(
        legacy_prompt_view.trajectory_meta[0].steps[1]
            .elapsed_ms
            .is_none()
    );
    assert!(
        legacy_prompt_view.trajectory_meta[0].steps[1]
            .duration_ms
            .is_none()
    );
}

#[test]
pub(crate) fn view_cli_notes_fail_for_out_of_range_index() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    let store_root = init_workspace(temp.path().join("evals"));
    run_evaluation(RunRequest {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        task_set: Some("local/rust-swe".to_string()),
        task: None,
        agent: Some("fake-pass".to_string()),
        overwrite: false,
        store_root: Some(store_root.clone()),
        output_root: None,
        include_artifacts: Vec::new(),
    })
    .expect("run");

    let err = run_view(ViewArgs {
        config: Some(fixture.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(store_root),
        paths: Vec::new(),
        task_set: Some("local/rust-swe".to_string()),
        agent: Some("fake-pass".to_string()),
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec!["annotations".to_string()],
        notes: vec!["2=missing trial".to_string()],
        format: Some(ViewFormat::Json),
        output: None,
    })
    .expect_err("out of range note fails");
    let message = format!("{err:#}");
    assert!(message.contains("view note index 2 is out of range"));
    assert!(message.contains("1 visible Trials"));
}
