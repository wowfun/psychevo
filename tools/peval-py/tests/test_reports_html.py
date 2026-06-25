from __future__ import annotations

from peval_py_test_support import *


def compact_css_text(value: str) -> str:
    return re.sub(r"\s+", "", value).replace(";}", "}")


def write_cached_analysis(
    root: Path,
    *,
    eval_slug: str = "default",
    agent_id: str = "agent-a",
    session_id: str = "common_session",
    cell_key: str = "session_t001",
    summary: str = "Cached analysis summary.",
    extra: dict | None = None,
) -> Path:
    path = root / "runs" / eval_slug / agent_id / session_id / cell_key / "analysis.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "schema_version": 1,
        "trial_name": session_id,
        "summary": summary,
        "checks": {},
    }
    if extra:
        payload.update(extra)
    path.write_text(
        json.dumps(payload),
        encoding="utf-8",
    )
    return path


def write_cached_markdown(
    root: Path,
    *,
    eval_slug: str = "default",
    agent_id: str = "agent-a",
    session_id: str = "common_session",
    cell_key: str = "session_t001",
    markdown: str = "## Finding\n\n- Cached markdown report.",
) -> Path:
    path = root / "runs" / eval_slug / agent_id / session_id / cell_key / "analysis.md"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(markdown, encoding="utf-8")
    return path


def write_cached_note(
    root: Path,
    *,
    eval_slug: str = "default",
    agent_id: str = "agent-a",
    session_id: str = "common_session",
    cell_key: str = "session_t001",
    markdown: str = "Manual cell note.",
) -> Path:
    path = root / "runs" / eval_slug / agent_id / session_id / cell_key / "notes.md"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(markdown, encoding="utf-8")
    return path


class PevalPyReportHtmlTests(unittest.TestCase):
    def test_report_json_subset_and_html_safe_embedding(self) -> None:
        records = read_jsonl(str(FIXTURES / "psychevo_session.jsonl"))
        config = ToolConfig(adapter="psychevo", trajectory_id="trial:html")
        result = convert_records(records, config)
        report = build_report(result, config, "psychevo_session.jsonl")
        self.assertEqual(report["schema_version"], 19)
        self.assertEqual(report["includes"], ["core", "annotations"])
        self.assertNotIn("comparison", report)
        self.assertNotIn("scope", report)
        self.assertNotIn("path_selections", report)
        self.assertIn("trajectory", report)
        self.assertIn("trajectory_meta", report)
        self.assertEqual(report["trajectory_meta"][0]["adapter"], "psychevo")
        self.assertEqual(report["trajectory_meta"][0]["status"], "passed")
        self.assertNotIn("usage", report["trajectory"][0]["final_metrics"])
        self.assertIn("usage", report["trajectory"][0]["final_metrics"]["extra"])
        for step_meta in report["trajectory_meta"][0]["steps"]:
            self.assertNotIn("data_preview", step_meta)
        analysis = report["annotations"]["analysis"][0]
        self.assertEqual(analysis["trial_key"], "trial:html")
        self.assertEqual(analysis["status"], "computed")
        auto = analysis["analysis_metrics"]["auto"]
        self.assertNotIn("outcome", auto)
        self.assertNotIn("efficiency", auto)
        self.assertNotIn("tokens_per_turn", json.dumps(auto))
        self.assertNotIn("tools_per_turn", json.dumps(auto))
        self.assertNotIn("tool_calls", auto["tooling"])
        self.assertNotIn("tool_errors", auto["tooling"])
        self.assertNotIn("top_tools_by_count", auto["tooling"])
        self.assertNotIn("top_tools_by_errors", auto["tooling"])
        self.assertNotIn("top_tools_by_duration_ms", auto["tooling"])
        self.assertEqual(auto["tooling"]["tool_error_rate"], 0.0)
        self.assertNotIn("total_tokens", auto.get("cost", {}))
        self.assertNotIn("token_breakdown", auto.get("cost", {}))
        self.assertNotIn("cost_usd", auto.get("cost", {}))
        self.assertEqual(auto["cost"]["cost_per_1k_tokens"], 0.555556)

        html = render_html(report)
        self.assertIn("data-step-action=\"toggle\"", html)
        self.assertIn("<h1>Agent Trajectory Report</h1>", html)
        self.assertNotIn("<p class=\"eyebrow\">agent trajectory</p>", html)
        self.assertNotIn("id=\"report-copy\"", html)
        self.assertNotIn("id=\"score-strip\"", html)
        self.assertNotIn("class=\"metric-card\"", html)
        self.assertIn('"run": "Run"', html)
        self.assertIn('t("run", "Run")', html)
        self.assertIn('t("result", "Result")', html)
        self.assertIn('t("evidence", "Evidence")', html)
        self.assertIn('"usage_breakdown": "Usage Breakdown"', html)
        self.assertIn("wall duration", html)
        self.assertIn("tool success / total", html)
        self.assertNotIn("Computed analysis", html)
        self.assertNotIn("Auto Metrics", html)
        self.assertIn(
            compact_css_text(
                "body{margin:0;background:var(--canvas);color:var(--ink);"
                "font:15px/1.48 var(--sans)}",
            ),
            compact_css_text(html),
        )
        font_sizes = [
            int(value)
            for value in re.findall(r"font(?:-size)?:[^;}]*?(\d+)px", html)
        ]
        self.assertGreaterEqual(min(font_sizes), 12)
        self.assertIn("\\u003cscript", html)
        self.assertNotIn("<script>alert(1)</script>", html)


    def test_multi_session_jsonl_report_comparison_and_notes(self) -> None:
        config = ToolConfig(adapter="opencode")
        first = convert_records(read_jsonl(str(FIXTURES / "common_session.jsonl")), config)
        second = convert_records(read_jsonl(str(FIXTURES / "psychevo_session.jsonl")), config)

        report = build_multi_report(
            [
                ReportSession(
                    conversion=first,
                    input_label="common_session.jsonl",
                    input_path=str(FIXTURES / "common_session.jsonl"),
                    session_hint="common_session",
                ),
                ReportSession(
                    conversion=second,
                    input_label="psychevo_session.jsonl",
                    input_path=str(FIXTURES / "psychevo_session.jsonl"),
                    session_hint="psychevo_session",
                ),
            ],
            config,
            [
                NoteInput(index=0, markdown="Report <script>note</script>"),
                NoteInput(index=2, markdown="Second session note"),
            ],
        )

        self.assertEqual(report["includes"], ["core", "annotations"])
        self.assertEqual(len(report["trajectory"]), 2)
        self.assertEqual(report["trajectory"][0]["session_id"], "common_session")
        self.assertEqual(report["trajectory"][1]["session_id"], "sess-psychevo")
        self.assertNotIn("comparison", report)
        self.assertEqual(report["trajectory_meta"][0]["duration_ms"], 100)
        self.assertEqual(report["trajectory_meta"][0]["wall_duration_ms"], 600)
        self.assertEqual(report["trajectory_meta"][1]["duration_ms"], 321)
        self.assertEqual(report["trajectory_meta"][1]["wall_duration_ms"], 2_000)
        meta_forbidden = {
            "matrix_cell_key",
            "benchmark",
            "cell_root_relative",
            "case_id",
            "task_set_id",
            "task_id",
            "task_family",
            "score_passed",
            "score_details",
        }
        for meta in report["trajectory_meta"]:
            self.assertTrue(meta_forbidden.isdisjoint(meta))
        self.assertEqual(report["annotations"]["report_notes"][0]["markdown"], "Report <script>note</script>")
        self.assertEqual(
            report["annotations"]["notes"][0]["trial_key"],
            report["trajectory_meta"][1]["trial_key"],
        )

        before_html_render = json.loads(json.dumps(report))
        html = render_html(report)
        compact_html = compact_css_text(html)
        self.assertEqual(report, before_html_render)
        self.assertIn("function synthesizedReportRow(trajectory, meta)", html)
        self.assertIn("function reportRows()", html)
        self.assertNotIn("state.view?.comparison?.leaderboard?.entries", html)
        self.assertNotIn("<h3>Summary</h3>", html)
        self.assertNotIn("Session Heatmap", html)
        self.assertNotIn("Session Table", html)
        self.assertIn("report-note-list", html)
        self.assertIn("report-note", html)

        self.assertNotIn("Visible Heatmap", html)
        self.assertNotIn("visible_heatmap", html)
        self.assertNotIn("visible_heatmap_eyebrow", html)
        self.assertNotIn("session-axis", html)
        self.assertNotIn("visible-grid", html)
        self.assertIn("grid-template-columns:minmax(150px,220px) minmax(0,1fr)", html)
        self.assertNotIn("repeat(${Math.max(rows.length, 1)}, minmax(150px, 1fr))", html)
        self.assertNotIn("metric-button", html)
        self.assertIn('label: t("agent", "Agent")', html)
        self.assertIn("agentNameFor(row)", html)
        self.assertIn("metricCellShade(row, column, rows)", html)
        self.assertIn("metric-shade-4", html)
        self.assertIn('key: "session_id", label: t("session", "Session"), width: "180px", filterable: true', html)
        self.assertIn('key: "agent", label: t("agent", "Agent"), width: "120px", filterable: true', html)
        self.assertIn('key: "model", label: t("model", "Model"), width: "150px", filterable: true', html)
        self.assertIn('key: "status", label: t("result", "Result"), width: "104px", filterable: true', html)
        self.assertIn("function renderDataTable", html)
        self.assertIn("function applyDataTableControls", html)
        self.assertIn("function bindDataTableControls", html)
        self.assertIn("function toggleDataTableSort", html)
        self.assertIn("controls.sort = null", html)
        self.assertIn("state.tables[tableId]", html)
        self.assertIn("controls.filters ||= {}", html)
        self.assertIn('bindDataTableControls(target, "leaderboard"', html)
        self.assertIn('bindDataTableControls(target, "timeline"', html)
        self.assertNotIn("state.filters", html)
        self.assertIn("columns.every(column =>", html)
        self.assertIn("selected.includes(filterValue(row, column))", html)
        self.assertIn('return applyDataTableControls("leaderboard", reportRows(), leaderboardColumns(), reportRows())', html)
        self.assertIn("filter-control", html)
        self.assertIn("filter-option", html)
        self.assertIn("table-head-inline", html)
        self.assertIn("filter-icon", html)
        self.assertIn("data-filter-key", html)
        self.assertIn("data-filter-clear", html)
        self.assertIn('label: t("duration", "Active Duration")', html)
        self.assertIn("trial?.wall_duration_ms", html)
        self.assertIn("metric: true, value: row => row.duration_ms", html)
        self.assertIn("metric: true, value: row => row.tokens", html)
        self.assertIn("metric: true, value: row => row.total_tool_calls", html)
        self.assertIn("metric: true, value: row => row.turns", html)
        self.assertNotIn("function rowIdleDurationMs(row)", html)
        self.assertNotIn('key: "idle_duration_ms"', html)
        self.assertNotIn("Idle Duration", html)
        self.assertIn("function rowToolErrorRate(row)", html)
        self.assertIn('value: row => rowToolErrorRate(row)', html)
        self.assertIn('key: "cost_usd"', html)
        self.assertNotIn("metric: true, value: row => row.cost_usd", html)
        self.assertIn('tableId: "leaderboard"', html)
        self.assertIn('tableId: "timeline"', html)
        self.assertIn("Leaderboard", html)
        self.assertNotIn("leaderboard_eyebrow", html)
        self.assertIn("data-table-sort", html)
        self.assertIn("selected-row", html)
        self.assertIn("data-trial-key", html)
        self.assertIn("Trajectory Overview", html)
        self.assertIn("trajectory-overview-title", html)
        self.assertIn("trajectory-node", html)
        self.assertIn("trajectory-node-letter", html)
        self.assertIn("trajectory-node.duration-heat-1", html)
        self.assertIn("trajectory-node.duration-heat-10", html)
        self.assertIn("function trajectoryOverviewTimingModel", html)
        self.assertIn("function overviewStepMeta", html)
        self.assertIn("trajectoryDurationHeatClass(ratio)", html)
        self.assertIn("function trajectoryDurationHeatClass", html)
        self.assertIn('timeTitle("step", stepDuration, durationRatio, "slowest step")', html)
        self.assertIn('if (role === "system") return "S"', html)
        self.assertIn('if (role === "user") return "U"', html)
        self.assertIn('if (role === "agent") return "A"', html)
        self.assertIn('return "?"', html)
        self.assertNotIn("trajectory-node.role-system", html)
        self.assertNotIn("trajectory-node.role-user", html)
        self.assertNotIn("trajectory-node.role-agent", html)
        self.assertNotIn("--step-count", html)
        self.assertIn("renderLeaderboard(rows);", html)
        self.assertIn("renderTrajectoryOverview(rows);", html)
        self.assertIn("function renderTrajectoryOverview(rows = leaderboardRows())", html)
        self.assertIn('id="step-drawer"', html)
        self.assertIn("function renderStepDrawer()", html)
        self.assertIn("data-step-id", html)
        self.assertIn("data-step-drawer-close", html)
        self.assertIn('event.key !== "Escape"', html)
        self.assertIn('document.addEventListener("click"', html)
        self.assertIn('target?.closest?.("#step-drawer")', html)
        self.assertIn('target?.closest?.("[data-step-id]")', html)
        self.assertIn('target?.closest?.("[data-timeline-step-id]")', html)
        self.assertIn('target?.closest?.("[data-timeline-chart]")', html)
        self.assertIn("function setStepDrawerOpen(open)", html)
        self.assertIn('document.body.classList.toggle("step-drawer-open", Boolean(open))', html)
        self.assertIn("renderStep(step, trial, timingStats, { open: true })", html)
        self.assertIn("step-drawer", html)
        self.assertIn("--step-drawer-width:min(760px,44vw)", html)
        self.assertIn("--step-drawer-gap:24px", html)
        self.assertIn(
            compact_css_text(
                ".step-drawer-open .workspace{max-width:calc(100vw - var(--step-drawer-width) - var(--step-drawer-gap));margin-left:0;margin-right:calc(var(--step-drawer-width) + var(--step-drawer-gap))}"
            ),
            compact_html,
        )
        self.assertIn("width:var(--step-drawer-width)", html)
        self.assertIn("height:100vh", html)
        self.assertIn("overflow:auto", html)
        self.assertIn("grid-template-rows:auto minmax(0,1fr)", html)
        self.assertIn(
            compact_css_text(
                ".step-drawer-body{min-height:0;padding:16px;display:grid;gap:12px;align-content:start}"
            ),
            compact_html,
        )
        self.assertIn(
            compact_css_text(
                ".step-drawer .step-body{min-height:0;display:grid;gap:10px;overflow:visible}"
            ),
            compact_html,
        )
        self.assertIn(
            compact_css_text(
                ".step-drawer .block{min-height:0;overflow:visible}"
            ),
            compact_html,
        )
        self.assertIn(
            compact_css_text(
                ".step-drawer .block pre{min-height:0;max-height:min(52vh,420px);overflow:auto}"
            ),
            compact_html,
        )
        self.assertNotIn(
            compact_css_text(
                ".step-drawer .step-body{min-height:0;display:grid;grid-auto-rows:minmax(120px,1fr);overflow:visible}"
            ),
            compact_html,
        )
        self.assertNotIn(
            compact_css_text(
                ".step-drawer .block pre{flex:1 1 auto;min-height:0;max-height:none;overflow:auto}"
            ),
            compact_html,
        )
        self.assertIn("selected trial trajectory", html)
        self.assertIn("note-list", html)
        self.assertIn("note-snippet", html)
        self.assertIn("Second session note", html)
        self.assertIn("Report \\u003cscript", html)
        self.assertNotIn("<script>note</script>", html)

    def test_analysis_metrics_render_structured_html_instead_of_json_strings(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        report = {
            "schema_version": 19,
            "includes": ["core", "annotations"],
            "trajectory": [{"trajectory_id": "trial:metrics", "steps": []}],
            "trajectory_meta": [{"trial_key": "trial:metrics", "status": "passed", "steps": []}],
            "annotations": {
                "analysis": [
                    {
                        "trial_key": "trial:metrics",
                        "status": "cached",
                        "analysis_metrics": {
                            "auto": {
                                "tooling": {
                                    "tool_error_rate": 0.25,
                                    "distinct_tools": 2,
                                },
                                "latency": {
                                    "step_duration_ms": {"min": 100, "q1": 200, "p50": 300, "q3": 1000, "p95": 1500, "max": 2000},
                                    "tool_execution_duration_ms": {"min": 20, "q1": 40, "p50": 80, "q3": 400, "p95": 900, "max": 1200},
                                    "model_duration_ms": {"min": 90, "q1": 180, "p50": 240, "q3": 900, "p95": 1300, "max": 1800},
                                },
                            },
                            "imported_scalar": 7,
                            "imported_rows": [
                                {"name": "quality", "score": 0.9},
                                {"name": "speed", "score": 0.4},
                            ],
                            "imported_nested": {"outer": {"inner": {"value": 1}}},
                        },
                    }
                ]
            },
        }
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = f"""
const vm = require("vm");
const asset = {json.dumps(asset)};
const report = {json.dumps(report)};
const context = {{
  document: {{
    body: {{ classList: {{ toggle() {{}} }} }},
    addEventListener() {{}},
    getElementById: () => null,
    querySelector: () => null,
  }},
  window: {{ addEventListener() {{}} }},
  console,
  JSON,
  Number,
  String,
  Object,
  Math,
  Date,
  Set,
  Array,
  RegExp,
  report,
}};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`
  state.view = report;
  renderSelectedAnalysis("trial:metrics");
`, context);
console.log(result);
"""
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        rendered = node.stdout
        self.assertIn("Tool error rate", rendered)
        self.assertNotIn("Tokens / turn", rendered)
        self.assertNotIn("Tools / turn", rendered)
        self.assertNotIn("Top tools by count", rendered)
        self.assertNotIn("analysis-ranked-bars", rendered)
        self.assertIn("analysis-latency-chart", rendered)
        self.assertIn("analysis-boxplot", rendered)
        self.assertIn("analysis-box-label-p50", rendered)
        self.assertIn("analysis-box-label-max", rendered)
        self.assertNotIn("analysis-dist-values", rendered)
        self.assertIn("Step duration", rendered)
        self.assertIn("Tool execution", rendered)
        self.assertIn("Model duration", rendered)
        self.assertRegex(
            rendered,
            r'(?s)<div class="analysis-boxplot"[^>]*>.*?</div><h6>Step duration</h6>',
        )
        self.assertIn("q1", rendered)
        self.assertIn("q3", rendered)
        self.assertIn("p50", rendered)
        self.assertIn("p95", rendered)
        self.assertIn("Max", rendered)
        self.assertIn("1.5s", rendered)
        self.assertIn("analysis-data-table", rendered)
        self.assertIn("quality", rendered)
        self.assertIn("analysis-json-details", rendered)
        self.assertNotIn('"top_tools_by_count"', rendered)

    def test_step_rail_counts_tool_errors_per_failed_tool_call(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = f"""
const vm = require("vm");
const asset = {json.dumps(asset)};
const context = {{
  document: {{
    body: {{ classList: {{ toggle() {{}} }} }},
    addEventListener() {{}},
    getElementById: () => null,
    querySelector: () => null,
    querySelectorAll: () => [],
  }},
  window: {{ addEventListener() {{}} }},
  console,
  JSON,
  Number,
  String,
  Object,
  Math,
  Date,
  Set,
  Array,
  RegExp,
}};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`
  renderStepRail(
    {{
      step_id: 1,
      source: "agent",
      message: "run tools",
      tool_calls: [
        {{ tool_call_id: "call-test", function_name: "test", arguments: {{}} }},
        {{ tool_call_id: "call-lint", function_name: "lint", arguments: {{}} }},
        {{ tool_call_id: "call-read", function_name: "read", arguments: {{}} }},
      ],
    }},
    {{
      step_id: 1,
      tool_error: true,
      tool_calls: [
        {{ tool_call_id: "call-test", status: "error", title: "test" }},
        {{ tool_call_id: "call-lint", status: "error", title: "lint" }},
        {{ tool_call_id: "call-read", status: "completed", title: "read" }},
      ],
    }},
    "trial:tool-errors",
    {{}}
  );
`, context);
console.log(result);
"""
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        self.assertIn(">1/3 tools<", node.stdout)
        self.assertNotIn(">2/3 tools<", node.stdout)

    def test_step_blocks_sort_mixed_tool_calls_and_observations_by_timestamp(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = f"""
const vm = require("vm");
const asset = {json.dumps(asset)};
const context = {{
  document: {{
    body: {{ classList: {{ toggle() {{}} }} }},
    addEventListener() {{}},
    getElementById: () => null,
    querySelector: () => null,
    querySelectorAll: () => [],
  }},
  window: {{ addEventListener() {{}} }},
  console,
  JSON,
  Number,
  String,
  Object,
  Math,
  Date,
  Set,
  Array,
  RegExp,
}};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`
  renderBlocks(
    {{
      step_id: 1,
      source: "agent",
      message: "I will run tools",
      tool_calls: [
        {{ tool_call_id: "call-late", function_name: "late", arguments: {{ cmd: "late" }} }},
        {{ tool_call_id: "call-early", function_name: "early", arguments: {{ cmd: "early" }} }},
      ],
      observation: {{
        results: [
          {{ source_call_id: "call-late", content: "late result" }},
          {{ source_call_id: "call-early", content: "early result" }},
        ],
      }},
    }},
    {{
      step_id: 1,
      tool_calls: [
        {{ tool_call_id: "call-late", title: "late", timestamp_ms: 1200 }},
        {{ tool_call_id: "call-early", title: "early", timestamp_ms: 1000 }},
      ],
      observations: [
        {{ source_call_id: "call-late", status: "completed", timestamp_ms: 1300 }},
        {{ source_call_id: "call-early", status: "completed", timestamp_ms: 1100 }},
      ],
    }},
    {{}}
  );
`, context);
console.log(result);
"""
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        rendered = node.stdout
        self.assertLess(rendered.index("Message"), rendered.index("ID: call-early"))
        self.assertLess(rendered.index("ID: call-early"), rendered.index("Result for: call-early"))
        self.assertLess(rendered.index("Result for: call-early"), rendered.index("ID: call-late"))
        self.assertLess(rendered.index("ID: call-late"), rendered.index("Result for: call-late"))

    def test_report_reads_cached_analysis_from_peval_runs_workspace(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            analysis_path = write_cached_analysis(
                root,
                extra={
                    "status": "reviewed",
                    "subject": {
                        "session_id": "common_session",
                        "trial_key": "session:t001",
                    },
                    "findings": [
                        {
                            "severity": "high",
                            "title": "Slow step <script>alert(2)</script>",
                            "evidence": ["step #2"],
                            "recommendation": "Inspect cached markdown.",
                        }
                    ],
                    "recommendations": ["Keep the structured analysis."],
                    "limitations": ["No live provider validation."],
                    "commands": ["peval-py view tr -f json"],
                    "metrics": {"review_turns": 3, "auto": {"bad": True}},
                    "confidence": "medium",
                    "unknown_field": "not exposed",
                },
            )
            markdown_path = write_cached_markdown(
                root,
                markdown=(
                    "## Slow step\n\n"
                    "- Check cached markdown.\n\n"
                    "<script>alert(1)</script>"
                ),
            )
            config = ToolConfig(adapter="opencode", workspace_root=str(root))
            report = build_report_from_loaded_inputs(
                LoadedInputs(
                    sessions=[
                        LoadedSession(
                            records=read_jsonl(str(FIXTURES / "common_session.jsonl")),
                            input_label="common_session.jsonl",
                            adapter_id="opencode",
                            session_hint="common_session",
                            agent_name="agent-a",
                        )
                    ],
                    notes=[],
                ),
                config,
            )

            self.assertEqual(report["includes"], ["core", "annotations"])
            analysis = report["annotations"]["analysis"][0]
            self.assertEqual(analysis["trial_key"], report["trajectory_meta"][0]["trial_key"])
            self.assertEqual(analysis["status"], "cached")
            self.assertEqual(analysis["analysis_status"], "reviewed")
            self.assertEqual(analysis["summary"], "Cached analysis summary.")
            self.assertEqual(analysis["subject"]["session_id"], "common_session")
            self.assertEqual(analysis["findings"][0]["severity"], "high")
            self.assertEqual(
                analysis["recommendations"],
                ["Keep the structured analysis."],
            )
            self.assertEqual(analysis["limitations"], ["No live provider validation."])
            self.assertEqual(analysis["commands"], ["peval-py view tr -f json"])
            self.assertEqual(analysis["analysis_metrics"]["review_turns"], 3)
            self.assertNotIn("outcome", analysis["analysis_metrics"]["auto"])
            self.assertNotIn("bad", analysis["analysis_metrics"]["auto"])
            self.assertEqual(analysis["confidence"], "medium")
            self.assertNotIn("unknown_field", analysis)
            self.assertNotIn("checks", analysis)
            self.assertIn("## Slow step", analysis["md_report"])
            self.assertEqual(
                analysis["relative_path"],
                analysis_path.relative_to(root).as_posix(),
            )
            self.assertEqual(
                analysis["relative_paths"],
                {
                    "json": analysis_path.relative_to(root).as_posix(),
                    "md": markdown_path.relative_to(root).as_posix(),
                },
            )
            self.assertNotIn(str(root), json.dumps(analysis))

            html = render_html(report)
            self.assertIn("Analysis", html)
            self.assertIn("Cached analysis summary.", html)
            self.assertIn("Slow step \\u003cscript\\u003ealert(2)\\u003c/script\\u003e", html)
            self.assertIn("Keep the structured analysis.", html)
            self.assertIn("No live provider validation.", html)
            self.assertIn("review_turns", html)
            self.assertIn("## Slow step", html)
            self.assertIn("Check cached markdown.", html)
            self.assertIn("renderMarkdown(analysis.md_report)", html)
            self.assertIn("\\u003cscript", html)
            self.assertNotIn("<script>alert(1)</script>", html)
            self.assertNotIn("<script>alert(2)</script>", html)
            self.assertIn(
                "runs/default/agent-a/common_session/session_t001/analysis.json",
                html,
            )
            self.assertIn(
                "runs/default/agent-a/common_session/session_t001/analysis.md",
                html,
            )

    def test_report_ignores_invalid_analysis_json_typed_fields(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_cached_analysis(
                root,
                extra={
                    "status": ["wrong"],
                    "subject": ["wrong"],
                    "findings": {"wrong": True},
                    "recommendations": "wrong",
                    "limitations": "wrong",
                    "commands": "wrong",
                    "metrics": ["wrong"],
                    "confidence": True,
                    "unknown_field": "not exposed",
                },
            )
            config = ToolConfig(adapter="opencode", workspace_root=str(root))
            report = build_report_from_loaded_inputs(
                LoadedInputs(
                    sessions=[
                        LoadedSession(
                            records=read_jsonl(str(FIXTURES / "common_session.jsonl")),
                            input_label="common_session.jsonl",
                            adapter_id="opencode",
                            session_hint="common_session",
                            agent_name="agent-a",
                        )
                    ],
                    notes=[],
                ),
                config,
            )

            analysis = report["annotations"]["analysis"][0]
            self.assertEqual(analysis["summary"], "Cached analysis summary.")
            for key in [
                "analysis_status",
                "subject",
                "findings",
                "recommendations",
                "limitations",
                "commands",
                "confidence",
                "unknown_field",
            ]:
                self.assertNotIn(key, analysis)
            self.assertEqual(list(analysis["analysis_metrics"]), ["auto"])

    def test_report_reads_cell_notes_separately_from_analysis(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            note_path = write_cached_note(
                root,
                markdown="Cell note with <script>alert(1)</script>.",
            )
            write_cached_markdown(root, markdown="Cached analysis body.")
            config = ToolConfig(adapter="opencode", workspace_root=str(root))
            report = build_report_from_loaded_inputs(
                LoadedInputs(
                    sessions=[
                        LoadedSession(
                            records=read_jsonl(str(FIXTURES / "common_session.jsonl")),
                            input_label="common_session.jsonl",
                            adapter_id="opencode",
                            session_hint="common_session",
                            agent_name="agent-a",
                        )
                    ],
                    notes=["1=CLI note after cell note"],
                ),
                config,
            )

            self.assertEqual(report["includes"], ["core", "annotations"])
            notes = report["annotations"]["notes"]
            self.assertEqual([note["source"] for note in notes], ["cell", "cli"])
            self.assertEqual(notes[0]["label"], "notes.md")
            self.assertEqual(notes[0]["markdown"], "Cell note with <script>alert(1)</script>.")
            self.assertEqual(
                notes[0]["source_ref"],
                {
                    "kind": "note",
                    "label": "notes.md",
                    "relative_path": note_path.relative_to(root).as_posix(),
                },
            )
            self.assertEqual(notes[1]["markdown"], "CLI note after cell note")
            self.assertEqual(
                report["annotations"]["analysis"][0]["md_report"],
                "Cached analysis body.",
            )

            html = render_html(report)
            self.assertIn("notes.md", html)
            self.assertIn("runs/default/agent-a/common_session/session_t001/notes.md", html)
            self.assertIn("Cell note with \\u003cscript", html)
            self.assertNotIn("<script>alert(1)</script>", html)

    def test_report_reads_notes_only_from_exact_trial_cell(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_cached_note(root, agent_id="opencode", cell_key="one")
            write_cached_note(root, agent_id="opencode", cell_key="two")
            session_note = root / "runs" / "default" / "opencode" / "common_session" / "notes.md"
            session_note.parent.mkdir(parents=True, exist_ok=True)
            session_note.write_text("Session note is not a Trial note.", encoding="utf-8")
            config = ToolConfig(adapter="opencode", workspace_root=str(root))
            conversion = convert_records(read_jsonl(str(FIXTURES / "common_session.jsonl")), config)
            report = build_multi_report(
                [
                    ReportSession(
                        conversion=conversion,
                        input_label="common_session.jsonl",
                        session_hint="common_session",
                        adapter_id="opencode",
                    )
                ],
                config,
                [],
            )
            self.assertEqual(report["annotations"]["notes"], [])
            self.assertEqual(report["annotations"]["analysis"][0]["status"], "computed")
            self.assertIn("auto", report["annotations"]["analysis"][0]["analysis_metrics"])

    def test_report_reads_markdown_only_cached_analysis(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            markdown_path = write_cached_markdown(
                root,
                agent_id="opencode",
                markdown="Markdown-only cached report.",
            )
            config = ToolConfig(adapter="opencode", workspace_root=str(root))
            conversion = convert_records(read_jsonl(str(FIXTURES / "common_session.jsonl")), config)
            report = build_multi_report(
                [
                    ReportSession(
                        conversion=conversion,
                        input_label="common_session.jsonl",
                        session_hint="common_session",
                        adapter_id="opencode",
                    )
                ],
                config,
                [],
            )

            analysis = report["annotations"]["analysis"][0]
            self.assertNotIn("summary", analysis)
            self.assertEqual(analysis["md_report"], "Markdown-only cached report.")
            self.assertEqual(
                analysis["relative_path"],
                markdown_path.relative_to(root).as_posix(),
            )
            self.assertEqual(
                analysis["relative_paths"],
                {"md": markdown_path.relative_to(root).as_posix()},
            )

    def test_report_omits_missing_or_non_trial_cell_analysis(self) -> None:
        config = ToolConfig(adapter="opencode")
        conversion = convert_records(read_jsonl(str(FIXTURES / "common_session.jsonl")), config)
        missing = build_multi_report(
            [
                ReportSession(
                    conversion=conversion,
                    input_label="common_session.jsonl",
                    session_hint="common_session",
                    adapter_id="opencode",
                    analysis_agent_id="opencode",
                )
            ],
            config,
            [],
        )
        self.assertEqual(missing["annotations"]["notes"], [])
        self.assertEqual(missing["annotations"]["analysis"][0]["status"], "computed")
        self.assertIn("auto", missing["annotations"]["analysis"][0]["analysis_metrics"])

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_cached_analysis(root, agent_id="opencode", cell_key="one")
            write_cached_analysis(root, agent_id="opencode", cell_key="two")
            session_analysis = root / "runs" / "default" / "opencode" / "common_session" / "analysis.json"
            session_analysis.write_text(json.dumps({"summary": "Session analysis"}), encoding="utf-8")
            non_trial_cell = build_multi_report(
                [
                    ReportSession(
                        conversion=conversion,
                        input_label="common_session.jsonl",
                        session_hint="common_session",
                        adapter_id="opencode",
                    )
                ],
                ToolConfig(adapter="opencode", workspace_root=str(root)),
                [],
            )
            self.assertEqual(non_trial_cell["annotations"]["notes"], [])
            self.assertEqual(
                non_trial_cell["annotations"]["analysis"][0]["status"],
                "computed",
            )
            self.assertIn(
                "auto",
                non_trial_cell["annotations"]["analysis"][0]["analysis_metrics"],
            )

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            bad_path = (
                root
                / "runs"
                / "default"
                / "opencode"
                / "common_session"
                / "session_t001"
                / "analysis.json"
            )
            bad_path.parent.mkdir(parents=True)
            bad_path.write_text("{not json", encoding="utf-8")
            write_cached_markdown(
                root,
                agent_id="opencode",
                cell_key="session_t001",
                markdown="Markdown survives malformed JSON.",
            )
            malformed = build_multi_report(
                [
                    ReportSession(
                        conversion=conversion,
                        input_label="common_session.jsonl",
                        session_hint="common_session",
                        adapter_id="opencode",
                    )
                ],
                ToolConfig(adapter="opencode", workspace_root=str(root)),
                [],
            )
            analysis = malformed["annotations"]["analysis"][0]
            self.assertNotIn("summary", analysis)
            self.assertNotIn("json", analysis["relative_paths"])
            self.assertEqual(analysis["md_report"], "Markdown survives malformed JSON.")

    def test_serve_html_mode_reuses_report_body_with_export_selection_controls(self) -> None:
        config = ToolConfig(adapter="opencode")
        first = convert_records(read_jsonl(str(FIXTURES / "common_session.jsonl")), config)
        second = convert_records(read_jsonl(str(FIXTURES / "psychevo_session.jsonl")), config)
        report = build_multi_report(
            [
                ReportSession(
                    conversion=first,
                    input_label="common_session.jsonl",
                    input_path=str(FIXTURES / "common_session.jsonl"),
                    session_hint="common_session",
                    source_alias="Readable source",
                ),
                ReportSession(
                    conversion=second,
                    input_label="psychevo_session.jsonl",
                    input_path=str(FIXTURES / "psychevo_session.jsonl"),
                    session_hint="psychevo_session",
                ),
            ],
            config,
            [],
        )
        self.assertNotIn("comparison", report)
        self.assertEqual(report["trajectory"][0]["session_id"], "common_session")
        self.assertEqual(report["trajectory_meta"][0]["source_alias"], "Readable source")
        self.assertEqual(report["trajectory_meta"][0]["finished_at_ms"], 1500)

        static_html = render_html(report)
        serve_html = render_serve_html(
            report,
            adapter_defaults={"opencode": "/tmp/opencode.db"},
        )

        self.assertIn('<body class="report-mode">', static_html)
        self.assertNotIn('class="serve-import-panel"', static_html)
        self.assertNotIn('class="source-manager-modal"', static_html)
        self.assertNotIn('<form class="source-form"', static_html)
        self.assertNotIn('type="button" data-db-inspect', static_html)
        self.assertIn("Timeline Waterfall", static_html)
        self.assertIn("Timeline Detail Table", static_html)
        self.assertIn(
            '<script src="https://cdn.jsdelivr.net/npm/echarts@6.0.0/dist/echarts.min.js"></script>',
            static_html,
        )
        self.assertNotIn("/assets/echarts/6.0.0/echarts.min.js", static_html)
        self.assertEqual(
            script_json(static_html, "peval-py-render-options"),
            {"mode": "report", "sources": []},
        )

        serve_options = script_json(serve_html, "peval-py-render-options")
        self.assertEqual(serve_options["mode"], "serve")
        self.assertEqual(len(serve_options["sources"]), 2)
        self.assertEqual(
            serve_options["adapter_defaults"],
            {"opencode": "/tmp/opencode.db"},
        )
        self.assertIn('<body class="serve-mode">', serve_html)
        self.assertIn('/assets/echarts/6.0.0/echarts.min.js', serve_html)
        self.assertIn(
            "this.onerror=null;this.src='https://cdn.jsdelivr.net/npm/echarts@6.0.0/dist/echarts.min.js'",
            serve_html,
        )
        self.assertNotIn('class="serve-import-panel"', serve_html)
        self.assertIn('class="serve-source-toolbar"', serve_html)
        self.assertIn('data-locale-select', serve_html)
        self.assertIn('class="source-manager-modal"', serve_html)
        self.assertIn("width:min(1480px,calc(100vw - 28px));", serve_html)
        self.assertIn('class="adapter-default-db-panel"', serve_html)
        self.assertIn("Adapter default DB", serve_html)
        self.assertIn("data-adapter-default-db-form", serve_html)
        self.assertIn("data-adapter-default-db-select", serve_html)
        self.assertIn("data-adapter-default-db-input", serve_html)
        self.assertIn("data-adapter-default-db-clear", serve_html)
        self.assertIn(
            '<option value="opencode" selected data-default-db="/tmp/opencode.db">opencode</option>',
            serve_html,
        )
        self.assertIn("Upload snapshot", serve_html)
        self.assertIn("report JSON uploads", serve_html)
        self.assertIn("Session / ATIF Path", serve_html)
        self.assertNotIn("<strong>Session / ATIF Path</strong>", serve_html)
        self.assertIn('<textarea name="path"', serve_html)
        self.assertIn('<textarea name="db"', serve_html)
        self.assertIn("Inspect DB", serve_html)
        self.assertIn("data-db-inspect", serve_html)
        self.assertIn("data-db-session-picker", serve_html)
        self.assertIn("data-db-add-selected", serve_html)
        self.assertIn("data-db-select-all", serve_html)
        self.assertEqual(serve_html.count('class="source-adapter-select"'), 4)
        self.assertEqual(serve_html.count('class="source-add-actions"'), 4)
        self.assertIn('name="adapter" aria-label="Adapter"', serve_html)
        self.assertIn('<option value="auto" selected>Auto</option>', serve_html)
        self.assertIn('<option value="opencode"  data-default-db="/tmp/opencode.db">opencode</option>', serve_html)
        self.assertEqual(serve_html.count('name="alias"'), 4)
        self.assertIn('data-source-alias-save', serve_html)
        self.assertNotIn("adapter-choice-group", serve_html)
        self.assertNotIn('type="radio" name="adapter"', serve_html)
        self.assertIn('data-source-action="delete"', serve_html)
        self.assertIn("2 sources", serve_html)
        self.assertIn("common_session.jsonl", serve_html)
        self.assertIn("Timeline Waterfall", serve_html)
        self.assertIn("Timeline Detail Table", serve_html)

        self.assertIn("function renderLeaderboard(rows = leaderboardRows())", serve_html)
        self.assertIn("function renderTrajectoryOverview(rows = leaderboardRows())", serve_html)
        self.assertIn("function renderTrace()", serve_html)
        self.assertIn("function renderStepDrawer()", serve_html)
        self.assertIn("function displayLeaderboardColumns()", serve_html)
        self.assertIn('t("session_alias", "Session Alias")', serve_html)
        self.assertIn('t("last_turn_end", "Last Turn End")', serve_html)
        self.assertIn('key: "finished_at_ms"', serve_html)
        self.assertIn("function sourceColumns()", serve_html)
        self.assertIn("last_turn_finished_at_ms", serve_html)
        self.assertIn("source-table", serve_html)
        self.assertIn('bindDataTableControls(list, "sources"', serve_html)
        self.assertIn("serveMode() ? [selectionColumn(), ...leaderboardColumns()] : leaderboardColumns()", serve_html)
        self.assertIn("data-select-visible", serve_html)
        self.assertIn("data-row-select", serve_html)
        self.assertIn("leaderboard-export", serve_html)
        self.assertIn("function bindServeSourceControls()", serve_html)
        self.assertIn('serveApi("/api/config/locale"', serve_html)
        self.assertIn('serveApi("/api/config/adapter-default-db"', serve_html)
        self.assertIn("adapterDefaults: initialAdapterDefaults()", serve_html)
        self.assertIn("function saveAdapterDefaultDb(form)", serve_html)
        self.assertIn("function updateAdapterDefaultOptions()", serve_html)
        self.assertIn("function bindAdapterDefaultDbControls()", serve_html)
        self.assertIn("function saveSourceAlias(button)", serve_html)
        self.assertIn('serveApi("/api/db-sessions"', serve_html)
        self.assertIn("function inspectDbSessions(form)", serve_html)
        self.assertIn("function addSelectedDbSessions(form)", serve_html)
        self.assertIn("session_ids: sessionIds", serve_html)
        self.assertIn('serveApi("/api/upload"', serve_html)
        self.assertIn('serveApi("/api/sources"', serve_html)
        self.assertIn('serveApi("/api/refresh"', serve_html)
        self.assertIn("data-source-manager-open", serve_html)
        self.assertIn("data-source-list", serve_html)
        self.assertIn("data-source-upload-form", serve_html)
        self.assertIn('t("export", "Export")', serve_html)
        self.assertIn('t("export_table", "Table")', serve_html)
        self.assertIn('data-export-kind="csv"', serve_html)
        self.assertIn('data-export-kind="json"', serve_html)
        self.assertIn('data-export-kind="html"', serve_html)
        self.assertIn("function exportScopeRows()", serve_html)
        self.assertIn(
            "const selected = rows.filter(row => state.rowSelection.has(row.trial_key));",
            serve_html,
        )
        self.assertIn("return selected.length ? selected : rows;", serve_html)
        self.assertIn("state.rowSelection.delete(key)", serve_html)
        self.assertIn("renderComparisonPanels({ trace: false })", serve_html)
        self.assertIn("event.stopPropagation();", serve_html)
        self.assertIn("function reportSubset(rows)", serve_html)
        self.assertIn("function renderAnalysisPaths(analysis)", serve_html)
        self.assertIn("analysis.md_report", serve_html)
        self.assertIn("analysis.relative_paths", serve_html)
        self.assertIn("renderMarkdown(analysis.md_report)", serve_html)
        self.assertIn("function editableNotesSource(trialKey)", serve_html)
        self.assertIn("function saveSelectedNotes(button)", serve_html)
        self.assertIn("data-notes-edit", serve_html)
        self.assertIn("data-notes-save", serve_html)
        self.assertIn("/notes", serve_html)
        self.assertIn(
            "analysis: (original.annotations.analysis || []).filter(item => selectedKeys.has(item.trial_key))",
            serve_html,
        )
        self.assertIn('downloadText("peval-report-v19.json"', serve_html)
        self.assertIn('downloadText("peval-report.html"', serve_html)
        self.assertIn('downloadText("peval-leaderboard-visible.csv"', serve_html)


    def test_html_render_mode_rejects_unknown_mode(self) -> None:
        report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [{"trajectory_id": "trial:mode", "steps": []}],
            "trajectory_meta": [{"trial_key": "trial:mode", "status": "passed", "steps": []}],
        }

        with self.assertRaisesRegex(ValueError, "unsupported HTML render mode"):
            render_html(report, mode="dashboard")


    def test_html_report_locale_localizes_report_chrome_except_steps(self) -> None:
        config = ToolConfig(adapter="opencode")
        first = convert_records(read_jsonl(str(FIXTURES / "common_session.jsonl")), config)
        second = convert_records(read_jsonl(str(FIXTURES / "psychevo_session.jsonl")), config)
        report = build_multi_report(
            [
                ReportSession(
                    conversion=first,
                    input_label="common_session.jsonl",
                    input_path=str(FIXTURES / "common_session.jsonl"),
                    session_hint="common_session",
                ),
                ReportSession(
                    conversion=second,
                    input_label="psychevo_session.jsonl",
                    input_path=str(FIXTURES / "psychevo_session.jsonl"),
                    session_hint="psychevo_session",
                ),
            ],
            config,
            [],
        )

        english_html = render_html(report)
        zh_html = render_html(report, locale="zh-CN")

        self.assertIn('<html lang="en">', english_html)
        self.assertIn("<h1>Agent Trajectory Report</h1>", english_html)
        self.assertIn("Leaderboard", english_html)
        self.assertIn("Trajectory Overview", english_html)
        self.assertIn('"agent": "Agent"', english_html)
        self.assertIn('"filter": "Filter"', english_html)
        self.assertIn('"clear": "Clear"', english_html)
        self.assertIn('"selected_count": "selected"', english_html)
        self.assertIn('"step_details": "Step details"', english_html)
        self.assertIn('"open_step_details": "Open step details"', english_html)
        self.assertIn('"close": "Close"', english_html)
        self.assertNotIn("Agent 轨迹报告", english_html)
        self.assertNotIn("可见热力图", english_html)
        self.assertNotIn("visible_heatmap", english_html)

        self.assertIn('<html lang="zh-CN">', zh_html)
        self.assertIn("<h1>Agent 轨迹报告</h1>", zh_html)
        self.assertIn('"leaderboard": "Leaderboard"', zh_html)
        self.assertIn('"agent": "Agent"', zh_html)
        self.assertIn('"trajectory_overview": "轨迹概览"', zh_html)
        self.assertIn('"filter": "筛选"', zh_html)
        self.assertIn('"clear": "清除"', zh_html)
        self.assertIn('"selected_count": "已选"', zh_html)
        self.assertIn('"step_details": "Step 详情"', zh_html)
        self.assertIn('"open_step_details": "打开 Step 详情"', zh_html)
        self.assertIn('"close": "关闭"', zh_html)
        self.assertNotIn('"visible_heatmap"', zh_html)
        self.assertNotIn("visible_heatmap_eyebrow", zh_html)
        self.assertNotIn("leaderboard_eyebrow", zh_html)
        self.assertIn('"duration": "活跃耗时"', zh_html)
        self.assertIn('"status.passed": "通过"', zh_html)
        self.assertIn('"session": "Session"', zh_html)
        self.assertIn('"result": "Result"', zh_html)
        self.assertIn('"notes": "Notes"', zh_html)
        self.assertNotIn('"agent": "代理"', zh_html)
        self.assertIn('"selected_trial_trajectory": "selected trial trajectory"', zh_html)
        self.assertIn('"run": "Run"', zh_html)
        self.assertIn('"variant": "variant"', zh_html)
        self.assertIn('"evaluator": "evaluator"', zh_html)
        self.assertIn('"reasoning": "reasoning"', zh_html)
        self.assertIn('"reasoning_exposed": "reasoning exposed"', zh_html)
        self.assertIn('"steps_events": "steps/events"', zh_html)
        self.assertIn('"turns": "Turns"', zh_html)
        self.assertIn('"tool_calls": "Tool Calls"', zh_html)
        self.assertIn('"tool_success_total": "tool success / total"', zh_html)
        self.assertIn('"evidence": "Evidence"', zh_html)
        self.assertIn('"cache_read": "cache read"', zh_html)
        self.assertIn('"cache_write": "cache write"', zh_html)
        self.assertIn('"usage_breakdown": "用量明细"', zh_html)
        self.assertNotIn('"session": "会话"', zh_html)
        self.assertNotIn('"result": "结果"', zh_html)
        self.assertNotIn('"notes": "备注"', zh_html)
        self.assertNotIn('"trajectory_overview": "Trajectory Overview"', zh_html)
        self.assertNotIn('"selected_trial_trajectory": "选中的 Trial 轨迹"', zh_html)
        self.assertNotIn('"run": "运行"', zh_html)
        self.assertNotIn('"variant": "变体"', zh_html)
        self.assertNotIn('"evaluator": "评估器"', zh_html)
        self.assertNotIn('"reasoning": "推理"', zh_html)
        self.assertNotIn('"reasoning_exposed": "包含推理"', zh_html)
        self.assertNotIn('"steps_events": "步骤/事件"', zh_html)
        self.assertNotIn('"turns": "轮次"', zh_html)
        self.assertNotIn('"tool_calls": "工具调用"', zh_html)
        self.assertNotIn('"tool_success_total": "工具成功 / 总数"', zh_html)
        self.assertNotIn('"evidence": "证据"', zh_html)
        self.assertNotIn('"cache_read": "缓存读取"', zh_html)
        self.assertNotIn('"cache_write": "缓存写入"', zh_html)
        self.assertNotIn('"leaderboard": "排行榜"', zh_html)
        self.assertNotIn(">排行榜<", zh_html)
        self.assertIn("<h3>Steps (${count})</h3>", zh_html)
        self.assertIn("<h4>Tool Calls</h4>", zh_html)


    def test_html_renders_tool_names_timing_and_nested_observations(self) -> None:
        records = [
            MessageRecord(
                message={
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool_call",
                            "id": "call-exec",
                            "name": "exec_command",
                            "arguments": {"cmd": "true"},
                        }
                    ],
                    "timestamp_ms": 1000,
                },
                usage={"prompt_tokens": 21460},
            ),
            MessageRecord(
                message={
                    "role": "tool_result",
                    "tool_call_id": "call-exec",
                    "tool_name": "exec_command",
                    "content": {"exit_code": 0},
                    "timestamp_ms": 1110,
                },
                metadata={"elapsed_ms": 101},
            ),
        ]
        config = ToolConfig(adapter="psychevo", trajectory_id="trial:tool-html")
        result = convert_records(records, config)
        report = build_report(result, config, "inline")
        html = render_html(report)

        self.assertEqual(len(report["trajectory"][0]["steps"]), 1)
        self.assertIn("exec_command", html)
        self.assertIn("tool exec", html)
        self.assertIn("rail-summary", html)
        self.assertIn("rail-tool-row", html)
        self.assertIn("function stepTimingStats", html)
        self.assertIn("maxStepDurationMs", html)
        self.assertIn("maxToolExecutionMs", html)
        self.assertIn("elapsedMaxMs", html)
        self.assertIn("timeGradientStyle", html)
        self.assertIn("time-gradient", html)
        self.assertIn("--time-pct", html)
        self.assertIn("slowest step", html)
        self.assertIn("slowest tool", html)
        self.assertIn('timeTitle("elapsed", meta?.elapsed_ms, elapsedRatio, "trajectory")', html)
        self.assertIn("function fmtRailTokens", html)
        self.assertIn("fmtRailTokens(tokenInfo.tokens)", html)
        self.assertIn("fmtNum(tokenInfo.tokens)", html)
        self.assertIn("Tool Calls", html)
        self.assertIn("Observations", html)
        self.assertEqual(
            report["trajectory_meta"][0]["steps"][0]["tool_calls"][0][
                "execution_duration_ms"
            ],
            101,
        )


    def test_html_renders_wall_time_timeline_diagnostics_without_new_json_fields(self) -> None:
        report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:timeline",
                    "session_id": "timeline",
                    "agent": {"name": "custom", "model_name": "test-model"},
                    "steps": [
                        {"step_id": 1, "source": "user", "message": "start"},
                        {
                            "step_id": 2,
                            "source": "system",
                            "message": "system context",
                        },
                        {
                            "step_id": 3,
                            "source": "agent",
                            "message": "run slow tool",
                            "tool_calls": [
                                {
                                    "tool_call_id": "call-slow",
                                    "function_name": "exec_command",
                                    "arguments": {"cmd": "sleep 1"},
                                }
                            ],
                            "observation": {
                                "results": [
                                    {
                                        "source_call_id": "call-slow",
                                        "content": "done",
                                    }
                                ]
                            },
                        },
                        {
                            "step_id": 4,
                            "source": "user",
                            "message": "after long retained-session idle",
                        },
                        {
                            "step_id": 5,
                            "source": "user",
                            "message": "long input processing",
                        },
                        {"step_id": 6, "source": "agent", "message": "final"},
                    ],
                    "final_metrics": {},
                }
            ],
            "trajectory_meta": [
                {
                    "trial_key": "trial:timeline",
                    "status": "passed",
                    "started_at_ms": 1_000,
                    "finished_at_ms": 702_300,
                    "wall_duration_ms": 701_300,
                    "duration_ms": 450,
                    "steps": [
                        {
                            "step_id": 1,
                            "timestamp_ms": 1_000,
                            "elapsed_ms": 0,
                            "duration_ms": None,
                            "tool_calls": [],
                            "observations": [],
                        },
                        {
                            "step_id": 2,
                            "timestamp_ms": 1_040,
                            "elapsed_ms": 40,
                            "duration_ms": 80,
                            "tool_calls": [],
                            "observations": [],
                        },
                        {
                            "step_id": 3,
                            "timestamp_ms": 1_100,
                            "elapsed_ms": 100,
                            "duration_ms": 250,
                            "tool_calls": [
                                {
                                    "tool_call_id": "call-slow",
                                    "title": "exec_command",
                                    "timestamp_ms": 1_125,
                                    "execution_duration_ms": 200,
                                    "status": "error",
                                }
                            ],
                            "observations": [
                                {
                                    "source_call_id": "call-slow",
                                    "timestamp_ms": 1_325,
                                    "status": "error",
                                }
                            ],
                        },
                        {
                            "step_id": 4,
                            "timestamp_ms": 702_000,
                            "elapsed_ms": 701_000,
                            "duration_ms": None,
                            "tool_calls": [],
                            "observations": [],
                        },
                        {
                            "step_id": 5,
                            "timestamp_ms": 702_050,
                            "elapsed_ms": 701_050,
                            "duration_ms": 120,
                            "tool_calls": [],
                            "observations": [],
                        },
                        {
                            "step_id": 6,
                            "timestamp_ms": 702_200,
                            "elapsed_ms": 701_200,
                            "duration_ms": 0,
                            "tool_calls": [],
                            "observations": [],
                        },
                    ],
                    "warnings": [],
                }
            ],
        }
        before = json.loads(json.dumps(report))
        html = render_html(report)
        payload = script_json(html, "peval-py-data")
        js = load_asset_text("report.js")
        stage_source = js[
            js.index("function timelineTrace") : js.index("function timelinePushStage")
        ]
        detail_columns_source = js[
            js.index("function timelineDetailColumns") : js.index(
                "function renderTimelineDetailTable"
            )
        ]
        detail_pct_source = js[
            js.index("function timelineActivePctValue") : js.index(
                "function timelineDetailColumns"
            )
        ]
        detail_table_source = js[
            js.index("function renderTimelineDetailTable") : js.index(
                "function renderTimelineStageLabel"
            )
        ]
        chart_option_source = js[
            js.index("function timelineChartOption") : js.index(
                "function timelineYAxisLabelWidth"
            )
        ]

        self.assertEqual(report, before)
        self.assertEqual(payload, before)
        self.assertEqual(payload["schema_version"], 19)
        self.assertNotIn("timeline", payload)
        self.assertIn("Timeline Waterfall", html)
        self.assertIn("Timeline Detail Table", html)
        self.assertIn("Flat active-latency trace", html)
        self.assertIn("Flat latency stages with true wall timing", html)
        self.assertIn(
            "https://cdn.jsdelivr.net/npm/echarts@6.0.0/dist/echarts.min.js",
            html,
        )
        self.assertIn("function renderTimelineDiagnostics", html)
        self.assertIn("function timelineTrace", html)
        self.assertIn("function initTimelineWaterfallChart", html)
        self.assertIn("window.echarts.init", html)
        self.assertIn('type: "custom"', html)
        self.assertIn("timeline_stage_model", html)
        self.assertIn("`Model: ${model}`", html)
        self.assertIn("const modelStage = timelineModelStageLabel(trajectory)", js)
        self.assertNotIn("Model Call", html)
        self.assertNotIn("Agent turn", html)
        self.assertIn("return `≈${value}`", js)
        self.assertNotIn("${value} (estimated)", js)
        self.assertIn("timeline_stage_input_processing", html)
        self.assertIn("timeline_stage_system_processing", html)
        self.assertIn("timeline_marker_user_input", html)
        self.assertIn("timeline_marker_system_context", html)
        self.assertIn("timeline_active_offset", html)
        self.assertIn("active_total_ms", html)
        self.assertIn("function timelineAssignActiveOffsets", html)
        self.assertIn("function timelineYAxisLabelWidth", html)
        self.assertIn("function timelineXAxisScale", html)
        self.assertIn("function timelineNiceIntervalMs", html)
        self.assertIn("function openTimelineStep", html)
        self.assertIn("function bindTimelineControls", html)
        self.assertIn("grid: { left: labelWidth + 18", html)
        self.assertIn("width: labelWidth", html)
        self.assertIn("interval: xAxisScale.interval", html)
        self.assertIn("minInterval: xAxisScale.interval", html)
        self.assertIn("formatter: value => fmtTimelineAxis(value, xAxisScale.interval)", html)
        self.assertIn('return interval && interval < 1000 ? "0ms" : "0s"', html)
        self.assertIn("TIMELINE_INPUT_STAGE_THRESHOLD_MS = 50", html)
        self.assertNotIn("message", stage_source)
        self.assertNotIn("reasoning_content", stage_source)
        self.assertNotIn("valuePreview", stage_source)
        self.assertNotIn("firstToolName", stage_source)
        self.assertIn("function timelineActivePctValue", html)
        self.assertIn("function renderTimelineActiveShare", html)
        self.assertIn("--active-share-pct", html)
        self.assertIn("timeline-active-share", html)
        self.assertIn("function timelineDetailColumns(model)", html)
        self.assertIn("model.active_total_ms", detail_pct_source)
        self.assertNotIn("model.wall_total_ms", detail_pct_source)
        self.assertNotIn("model.wall_total_ms", detail_columns_source)
        self.assertIn('key: "stage", label: t("timeline_col_stage", "Stage"), sortable: true, filterable: true', detail_columns_source)
        self.assertIn('key: "duration_ms", label: t("timeline_col_duration", "Duration"), type: "number", numeric: true, sortable: true, metric: true', detail_columns_source)
        self.assertIn('key: "active_pct", label: t("timeline_col_total_pct", "Active Share"), type: "number", numeric: true, sortable: true, metric: true', detail_columns_source)
        self.assertIn("html: row => renderTimelineActiveShare(row, model)", detail_columns_source)
        self.assertIn('className: "active-share-cell"', detail_columns_source)
        self.assertNotIn('key: "distribution"', detail_columns_source)
        self.assertNotIn("timeline_col_distribution", detail_columns_source)
        self.assertNotIn('t("timeline_col_category", "Category")', detail_columns_source)
        self.assertIn('applyDataTableControls("timeline", rows, columns, rows)', detail_table_source)
        self.assertIn("renderTimelineStageLabel(row)", detail_columns_source)
        self.assertIn("data-timeline-step-id", detail_table_source)
        self.assertIn("timeline-detail-row", detail_table_source)
        self.assertIn("timeline-detail-selected", detail_table_source)
        self.assertIn("tabindex=\"0\"", detail_table_source)
        self.assertNotIn('t("timeline_col_category", "Category")', detail_table_source)
        self.assertNotIn('applyDataTableControls("timeline"', chart_option_source)
        self.assertNotIn('tableControls("timeline"', chart_option_source)
        self.assertIn("timeline-waterfall-chart", html)
        self.assertIn("timeline-fallback", html)
        self.assertIn("ECharts did not load", html)
        self.assertIn("const label = api.value(5)", html)
        self.assertIn("type: \"text\"", html)
        self.assertIn("fill: labelInside ? \"#fffdf8\" : color", html)
        self.assertIn("const color = api.value(4)", html)
        self.assertIn('cursor: "pointer"', html)
        self.assertIn('node.addEventListener("click", event => event.stopPropagation())', html)
        self.assertIn('state.timelineChart.on("click"', html)
        self.assertIn("openTimelineStep(params?.data?.trace_item)", html)
        self.assertIn("state.selectedStep = { trialKey: state.selectedTrial, stepId: String(item.step_id) }", html)
        self.assertIn("renderComparisonPanels();", html)
        self.assertIn("row.addEventListener(\"click\", open)", html)
        self.assertIn('event.key !== "Enter" && event.key !== " "', html)
        self.assertIn("fmtTimelineDuration(stage.duration_ms)", html)
        self.assertIn('"timeline_category_agent": "Agent"', html)
        self.assertNotIn("Agent/LLM", html)
        self.assertIn("timeline-stage-label", html)
        self.assertIn("timeline-category-error", html)
        self.assertIn("Active Share", html)
        self.assertNotIn("% Active", html)
        self.assertNotIn("timeline-distribution", html)
        self.assertIn("timeline-table", html)
        self.assertNotIn("timeline_stage_idle_gap", html)
        self.assertNotIn("timeline_category_idle", html)
        self.assertNotIn("timeline-category-idle", html)
        self.assertNotIn("function timelineAxisBreaks", html)
        self.assertNotIn("function trueToDisplayMs", html)
        self.assertNotIn("function displayToTrueMs", html)
        self.assertNotIn("function timelineBreakMarkArea", html)
        self.assertNotIn("TIMELINE_IDLE_GAP_BREAK_THRESHOLD_MS", html)
        self.assertNotIn("TIMELINE_IDLE_GAP_COMPRESSED_MS", html)
        self.assertNotIn("markArea:", html)
        self.assertNotIn("function renderTimelineWaterfallSvg", html)
        self.assertNotIn("timeline-waterfall-svg", html)
        self.assertNotIn("timeline-svg-grid", html)
        self.assertNotIn("Idle gap", html)
        self.assertNotIn("chart.js", html.lower())

    def test_html_timeline_trace_keeps_zero_duration_tool_stages(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js timeline helpers")
        report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:zero-tool",
                    "session_id": "zero-tool",
                    "agent": {"name": "custom", "model_name": "test-model"},
                    "steps": [
                        {
                            "step_id": 1,
                            "source": "agent",
                            "message": "inspect files",
                            "tool_calls": [
                                {
                                    "tool_call_id": "call-positive",
                                    "function_name": "exec_command",
                                    "arguments": {"cmd": "true"},
                                },
                                {
                                    "tool_call_id": "call-zero",
                                    "function_name": "read",
                                    "arguments": {"path": "README.md"},
                                },
                                {
                                    "tool_call_id": "call-missing",
                                    "function_name": "glob",
                                    "arguments": {"pattern": "*.md"},
                                },
                            ],
                        }
                    ],
                    "final_metrics": {},
                }
            ],
            "trajectory_meta": [
                {
                    "trial_key": "trial:zero-tool",
                    "status": "passed",
                    "started_at_ms": 1_000,
                    "duration_ms": 75,
                    "steps": [
                        {
                            "step_id": 1,
                            "timestamp_ms": 1_000,
                            "elapsed_ms": 0,
                            "duration_ms": 50,
                            "tool_calls": [
                                {
                                    "tool_call_id": "call-positive",
                                    "title": "exec_command",
                                    "timestamp_ms": 1_050,
                                    "execution_duration_ms": 25,
                                },
                                {
                                    "tool_call_id": "call-zero",
                                    "title": "read",
                                    "timestamp_ms": 1_075,
                                    "execution_duration_ms": 0,
                                },
                                {
                                    "tool_call_id": "call-missing",
                                    "title": "glob",
                                    "timestamp_ms": 1_080,
                                    "execution_duration_ms": None,
                                },
                            ],
                            "observations": [],
                        }
                    ],
                    "warnings": [],
                }
            ],
        }
        before = json.loads(json.dumps(report))
        html = render_html(report)
        payload = script_json(html, "peval-py-data")
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = f"""
const vm = require("vm");
const asset = {json.dumps(asset)};
const report = {json.dumps(report)};
const context = {{
  document: {{ getElementById: () => null, querySelector: () => null }},
  window: {{}},
  console,
  JSON,
  Number,
  String,
  Object,
  Math,
  Date,
  Set,
  Array,
  RegExp,
  report,
}};
vm.createContext(context);
vm.runInContext(asset, context);
const trace = context.timelineTrace(report.trajectory[0], report.trajectory_meta[0]);
console.log(JSON.stringify(trace.stages.map(stage => ({{
  stage: stage.stage,
  tool_call_id: stage.tool_call_id,
  duration_ms: stage.duration_ms,
  active_pct: context.timelineActivePctValue(stage, trace.model),
}}))));
"""
        result = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        stages = json.loads(result.stdout)
        by_call_id = {
            stage["tool_call_id"]: stage
            for stage in stages
            if stage.get("tool_call_id")
        }

        self.assertEqual(report, before)
        self.assertEqual(payload, before)
        self.assertIn("call-positive", by_call_id)
        self.assertIn("call-zero", by_call_id)
        self.assertNotIn("call-missing", by_call_id)
        self.assertEqual(by_call_id["call-zero"]["stage"], "Tool: read")
        self.assertEqual(by_call_id["call-zero"]["duration_ms"], 0)
        self.assertEqual(by_call_id["call-zero"]["active_pct"], 0)


    def test_html_timeline_trace_omits_order_only_timestamp_estimates(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js timeline helpers")
        report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:order-only",
                    "session_id": "order-only",
                    "agent": {"name": "hermes", "model_name": "test-model"},
                    "steps": [
                        {"step_id": 1, "source": "user", "message": "start"},
                        {
                            "step_id": 2,
                            "source": "agent",
                            "message": "run tool",
                            "tool_calls": [
                                {
                                    "tool_call_id": "call-missing",
                                    "function_name": "terminal",
                                    "arguments": {"cmd": "sleep 30"},
                                }
                            ],
                        },
                    ],
                    "final_metrics": {},
                }
            ],
            "trajectory_meta": [
                {
                    "trial_key": "trial:order-only",
                    "timestamp_semantics": "order_only",
                    "status": "passed",
                    "started_at_ms": 1_000,
                    "duration_ms": None,
                    "steps": [
                        {
                            "step_id": 1,
                            "timestamp_ms": 1_000,
                            "elapsed_ms": 0,
                            "duration_ms": None,
                            "tool_calls": [],
                            "observations": [],
                        },
                        {
                            "step_id": 2,
                            "timestamp_ms": 2_000,
                            "elapsed_ms": 1_000,
                            "duration_ms": None,
                            "tool_calls": [
                                {
                                    "tool_call_id": "call-missing",
                                    "title": "terminal",
                                    "timestamp_ms": 2_050,
                                    "execution_duration_ms": None,
                                }
                            ],
                            "observations": [
                                {
                                    "source_call_id": "call-missing",
                                    "timestamp_ms": 5_000,
                                    "status": "completed",
                                }
                            ],
                        },
                    ],
                    "warnings": [],
                }
            ],
        }
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = f"""
const vm = require("vm");
const asset = {json.dumps(asset)};
const report = {json.dumps(report)};
const context = {{
  document: {{ getElementById: () => null, querySelector: () => null }},
  window: {{}},
  console,
  JSON,
  Number,
  String,
  Object,
  Math,
  Date,
  Set,
  Array,
  RegExp,
  report,
}};
vm.createContext(context);
vm.runInContext(asset, context);
const trace = context.timelineTrace(report.trajectory[0], report.trajectory_meta[0]);
console.log(JSON.stringify(trace.stages.map(stage => ({{
  stage: stage.stage,
  tool_call_id: stage.tool_call_id,
  duration_ms: stage.duration_ms,
  estimated: stage.estimated,
}}))));
"""
        result = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        stages = json.loads(result.stdout)

        self.assertEqual(stages, [])


    def test_html_timeline_trace_uses_hermes_log_fused_timing(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js timeline helpers")
        with tempfile.TemporaryDirectory() as tmp:
            db_path = create_hermes_log_timing_home(Path(tmp) / ".hermes")
            config = ToolConfig(adapter="hermes")
            result = convert_db(str(db_path), None, config)
            report = build_report(result, config, "inline")
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = f"""
const vm = require("vm");
const asset = {json.dumps(asset)};
const report = {json.dumps(report)};
const context = {{
  document: {{ getElementById: () => null, querySelector: () => null }},
  window: {{}},
  console,
  JSON,
  Number,
  String,
  Object,
  Math,
  Date,
  Set,
  Array,
  RegExp,
  report,
}};
vm.createContext(context);
vm.runInContext(asset, context);
const trace = context.timelineTrace(report.trajectory[0], report.trajectory_meta[0]);
console.log(JSON.stringify(trace.stages.map(stage => ({{
  stage: stage.stage,
  kind: stage.kind,
  tool_call_id: stage.tool_call_id,
  duration_ms: stage.duration_ms,
  estimated: stage.estimated,
}}))));
"""
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        stages = json.loads(node.stdout)
        model_stages = [stage for stage in stages if stage["kind"] == "agent"]
        tool_stages = {
            stage["tool_call_id"]: stage
            for stage in stages
            if stage.get("tool_call_id")
        }

        self.assertEqual(model_stages[0]["duration_ms"], 5_700)
        self.assertFalse(model_stages[0]["estimated"])
        self.assertEqual(tool_stages["call-fetch"]["duration_ms"], 53_890)
        self.assertEqual(tool_stages["call-error"]["duration_ms"], 80)


    def test_html_timeline_trace_uses_opencode_event_fused_timing(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js timeline helpers")
        with tempfile.TemporaryDirectory() as tmp:
            db_path = Path(tmp) / "opencode.db"
            create_opencode_event_timing_db(db_path)
            config = ToolConfig(adapter="opencode")
            result = convert_db(str(db_path), None, config)
            report = build_report(result, config, "inline")
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = f"""
const vm = require("vm");
const asset = {json.dumps(asset)};
const report = {json.dumps(report)};
const context = {{
  document: {{ getElementById: () => null, querySelector: () => null }},
  window: {{}},
  console,
  JSON,
  Number,
  String,
  Object,
  Math,
  Date,
  Set,
  Array,
  RegExp,
}};
vm.createContext(context);
vm.runInContext(asset, context);
const trace = context.timelineTrace(report.trajectory[0], report.trajectory_meta[0]);
console.log(JSON.stringify(trace.stages.map(stage => ({{
  stage: stage.stage,
  kind: stage.kind,
  tool_call_id: stage.tool_call_id,
  duration_ms: stage.duration_ms,
  estimated: stage.estimated,
}}))));
"""
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        stages = json.loads(node.stdout)
        model_stages = [stage for stage in stages if stage["kind"] == "agent"]
        tool_stages = {
            stage["tool_call_id"]: stage
            for stage in stages
            if stage.get("tool_call_id")
        }

        self.assertEqual(model_stages[0]["duration_ms"], 100)
        self.assertTrue(model_stages[0]["estimated"])
        self.assertEqual(tool_stages["call-read"]["duration_ms"], 48_000)
        self.assertFalse(tool_stages["call-read"]["estimated"])


    def test_html_timeline_click_opens_drawer_for_single_session_report(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:single",
                    "session_id": "single",
                    "agent": {"name": "hermes", "model_name": "test-model"},
                    "steps": [
                        {"step_id": 1, "source": "user", "message": "run it"},
                        {
                            "step_id": 2,
                            "source": "agent",
                            "message": "reading",
                            "tool_calls": [
                                {
                                    "tool_call_id": "call-read",
                                    "function_name": "read",
                                    "arguments": {"file_path": "README.md"},
                                }
                            ],
                        },
                    ],
                    "final_metrics": {},
                }
            ],
            "trajectory_meta": [
                {
                    "trial_key": "trial:single",
                    "status": "passed",
                    "started_at_ms": 1_000,
                    "finished_at_ms": 1_200,
                    "duration_ms": 100,
                    "steps": [
                        {
                            "step_id": 1,
                            "timestamp_ms": 1_000,
                            "elapsed_ms": 0,
                            "duration_ms": None,
                            "tool_calls": [],
                            "observations": [],
                        },
                        {
                            "step_id": 2,
                            "timestamp_ms": 1_100,
                            "elapsed_ms": 100,
                            "duration_ms": 100,
                            "tool_calls": [
                                {
                                    "tool_call_id": "call-read",
                                    "title": "read",
                                    "timestamp_ms": 1_120,
                                    "execution_duration_ms": 50,
                                }
                            ],
                            "observations": [],
                        },
                    ],
                    "warnings": [],
                }
            ],
        }
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = f"""
const vm = require("vm");
const asset = {json.dumps(asset)};
const report = {json.dumps(report)};
const context = {{
  document: {{
    body: {{ classList: {{ toggle() {{}} }} }},
    addEventListener() {{}},
    getElementById: () => null,
    querySelector: () => null,
  }},
  window: {{ addEventListener() {{}} }},
  console,
  JSON,
  Number,
  String,
  Object,
  Math,
  Date,
  Set,
  Array,
  RegExp,
  report,
  rendered: [],
}};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`
  state.view = report;
  state.selectedTrial = report.trajectory_meta[0].trial_key;
  renderLeaderboard = () => rendered.push("leaderboard");
  renderTrajectoryOverview = () => rendered.push("overview");
  renderTrace = () => rendered.push("trace");
  renderStepDrawer = () => rendered.push(state.selectedStep ? "drawer-open" : "drawer-closed");
  openTimelineStep({{ kind: "stage", trial_key: "trial:single", step_id: 2 }});
  const stageStep = state.selectedStep;
  openTimelineStep({{ kind: "marker", trial_key: "trial:single", step_id: 1 }});
  JSON.stringify({{ selectedTrial: state.selectedTrial, selectedStep: state.selectedStep, stageStep, rendered }});
`, context);
console.log(result);
"""
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        result = json.loads(node.stdout)

        self.assertEqual(result["selectedTrial"], "trial:single")
        self.assertEqual(
            result["stageStep"],
            {"trialKey": "trial:single", "stepId": "2"},
        )
        self.assertEqual(
            result["selectedStep"],
            {"trialKey": "trial:single", "stepId": "1"},
        )
        self.assertIn("drawer-open", result["rendered"])


    def test_html_trajectory_overview_nodes_render_duration_heat(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:overview",
                    "session_id": "overview",
                    "agent": {"name": "psychevo"},
                    "steps": [
                        {"step_id": 1, "source": "user", "message": "start"},
                        {"step_id": 2, "source": "agent", "message": "fast"},
                        {"step_id": 3, "source": "agent", "message": "slow"},
                    ],
                    "final_metrics": {},
                },
                {
                    "trajectory_id": "trial:overview-2",
                    "session_id": "overview-2",
                    "agent": {"name": "psychevo"},
                    "steps": [
                        {"step_id": 1, "source": "user", "message": "start"},
                    ],
                    "final_metrics": {},
                }
            ],
            "trajectory_meta": [
                {
                    "trial_key": "trial:overview",
                    "status": "passed",
                    "steps": [
                        {"step_id": 1, "duration_ms": 0},
                        {"step_id": 2, "duration_ms": 120},
                        {"step_id": 3, "duration_ms": 240},
                    ],
                    "warnings": [],
                },
                {
                    "trial_key": "trial:overview-2",
                    "status": "passed",
                    "steps": [
                        {"step_id": 1, "duration_ms": 0},
                    ],
                    "warnings": [],
                }
            ],
        }
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = f"""
const vm = require("vm");
const asset = {json.dumps(asset)};
const report = {json.dumps(report)};
const context = {{
  document: {{
    body: {{ classList: {{ toggle() {{}} }} }},
    addEventListener() {{}},
    getElementById: () => null,
    querySelector: () => null,
  }},
  window: {{ addEventListener() {{}} }},
  console,
  JSON,
  Number,
  String,
  Object,
  Math,
  Date,
  Set,
  Array,
  RegExp,
  report,
}};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`
  state.view = report;
  state.selectedTrial = "trial:overview";
  state.selectedStep = {{ trialKey: "trial:overview", stepId: "3" }};
  renderTrajectoryOverviewRow(reportRows()[0]);
`, context);
console.log(result);
"""
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        row_html = node.stdout
        buttons = {
            match.group("id"): match.group("tag")
            for match in re.finditer(
                r'(?P<tag><button class="[^"]*"[^>]*data-step-id="(?P<id>[^"]+)"[^>]*>)',
                row_html,
            )
        }

        self.assertIn("1", buttons)
        self.assertIn("2", buttons)
        self.assertIn("3", buttons)
        self.assertNotIn("duration-heat-", buttons["1"])
        self.assertNotIn("--time-pct", buttons["1"])
        self.assertIn("step 0.0s", buttons["1"])
        self.assertIn("duration-heat-5", buttons["2"])
        self.assertNotIn("--time-pct", buttons["2"])
        self.assertIn("duration-heat-10", buttons["3"])
        self.assertIn("selected-node", buttons["3"])
        self.assertNotIn("--time-pct", buttons["3"])
        self.assertIn("step 0.2s; 100% of slowest step", buttons["3"])

    def test_html_runtime_rows_and_export_subset_avoid_persisted_comparison(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:one",
                    "session_id": "one",
                    "agent": {"name": "agent-a", "model_name": "model-a"},
                    "steps": [],
                    "final_metrics": {
                        "total_prompt_tokens": 80,
                        "total_completion_tokens": 40,
                        "total_cost_usd": 0.03,
                        "extra": {
                            "total_turns": 2,
                            "total_tool_calls": 4,
                            "total_tool_errors": 1,
                        },
                    },
                },
                {
                    "trajectory_id": "trial:two",
                    "session_id": "two",
                    "agent": {"name": "agent-b", "model_name": "model-b"},
                    "steps": [],
                    "final_metrics": {
                        "extra": {
                            "total_turns": 1,
                            "total_tool_calls": 0,
                            "total_tool_errors": 0,
                        },
                    },
                },
            ],
            "trajectory_meta": [
                {
                    "trial_key": "trial:one",
                    "adapter": "psychevo",
                    "status": "passed",
                    "finished_at_ms": 300,
                    "duration_ms": 100,
                    "wall_duration_ms": 300,
                    "warnings": ["warn"],
                    "steps": [],
                },
                {
                    "trial_key": "trial:two",
                    "adapter": "opencode",
                    "status": "failed",
                    "finished_at_ms": 500,
                    "duration_ms": 50,
                    "wall_duration_ms": 500,
                    "warnings": [],
                    "steps": [],
                },
            ],
            "annotations": {
                "report_notes": [],
                "notes": [{"trial_key": "trial:one", "markdown": "keep"}],
                "analysis": [{"trial_key": "trial:two", "status": "computed"}],
            },
        }
        legacy_report = {
            "schema_version": 19,
            "includes": ["core", "comparison"],
            "trajectory": [],
            "trajectory_meta": [],
            "comparison": {
                "leaderboard": {
                    "entries": [{"trial_key": "trial:single", "adapter": "legacy"}]
                }
            },
        }
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = f"""
const vm = require("vm");
const asset = {json.dumps(asset)};
const report = {json.dumps(report)};
const legacyReport = {json.dumps(legacy_report)};
const context = {{
  document: {{
    body: {{ classList: {{ toggle() {{}} }} }},
    addEventListener() {{}},
    getElementById: () => null,
    querySelector: () => null,
  }},
  window: {{ addEventListener() {{}} }},
  console,
  JSON,
  Number,
  String,
  Object,
  Math,
  Date,
  Set,
  Array,
  RegExp,
  report,
  legacyReport,
}};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`
  state.view = report;
  const rows = reportRows();
  const subset = reportSubset(rows);
  state.view = legacyReport;
  const legacyRows = reportRows();
  JSON.stringify({{
    rowCount: rows.length,
    firstAdapter: rows[0].adapter,
    firstErrorRate: rowToolErrorRate(rows[0]),
    subsetHasComparison: Object.prototype.hasOwnProperty.call(subset, "comparison"),
    subsetIncludes: subset.includes,
    subsetNotes: subset.annotations.notes.map(note => note.markdown),
    subsetAnalysisKeys: subset.annotations.analysis.map(item => item.trial_key),
    legacyRowCount: legacyRows.length
  }});
`, context);
console.log(result);
"""
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        result = json.loads(node.stdout)
        self.assertEqual(result["rowCount"], 2)
        self.assertEqual(result["firstAdapter"], "psychevo")
        self.assertAlmostEqual(result["firstErrorRate"], 0.25)
        self.assertFalse(result["subsetHasComparison"])
        self.assertEqual(result["subsetIncludes"], ["core"])
        self.assertEqual(result["subsetNotes"], ["keep"])
        self.assertEqual(result["subsetAnalysisKeys"], ["trial:two"])
        self.assertEqual(result["legacyRowCount"], 0)


    def test_html_submenu_outside_click_closer_only_targets_menus(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = """
const vm = require("vm");
const asset = __ASSET__;
const exportMenu = { id: "export", open: true };
const filterMenu = { id: "filter", open: true };
const timelineSection = { id: "timeline", open: true };
const handlers = [];
const documentStub = {
  body: { classList: { toggle() {} } },
  addEventListener(type, handler, options) {
    handlers.push({ type, handler, capture: options === true || options?.capture === true });
  },
  getElementById: () => null,
  querySelector: () => null,
  querySelectorAll(selector) {
    if (selector !== ".export-menu[open],.filter-control[open]") {
      throw new Error(`unexpected selector: ${selector}`);
    }
    return [exportMenu, filterMenu].filter(details => details.open);
  },
};
const context = {
  document: documentStub,
  window: { addEventListener() {} },
  console,
  JSON,
  Number,
  String,
  Object,
  Math,
  Date,
  Set,
  Array,
  RegExp,
  exportMenu,
  filterMenu,
  timelineSection,
  handlers,
};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`
  bindGlobalControls();
  const clickHandler = handlers.find(item => item.type === "click" && item.capture).handler;
  filterMenu.open = true;
  exportMenu.open = true;
  clickHandler({ target: { closest: selector => selector === SUBMENU_DETAILS_SELECTOR ? exportMenu : null } });
  const insideExport = { exportOpen: exportMenu.open, filterOpen: filterMenu.open, timelineOpen: timelineSection.open };
  filterMenu.open = true;
  exportMenu.open = true;
  clickHandler({ target: { closest: () => null } });
  const outside = { exportOpen: exportMenu.open, filterOpen: filterMenu.open, timelineOpen: timelineSection.open };
  JSON.stringify({ insideExport, outside, clickHandlerCapture: Boolean(clickHandler) });
`, context);
console.log(result);
""".replace("__ASSET__", json.dumps(asset))
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        result = json.loads(node.stdout)

        self.assertEqual(
            result["insideExport"],
            {"exportOpen": True, "filterOpen": False, "timelineOpen": True},
        )
        self.assertEqual(
            result["outside"],
            {"exportOpen": False, "filterOpen": False, "timelineOpen": True},
        )
        self.assertTrue(result["clickHandlerCapture"])


    def test_html_inlines_template_css_and_js_package_assets(self) -> None:
        report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:assets",
                    "session_id": "assets",
                    "agent": {"name": "custom"},
                    "steps": [],
                    "final_metrics": {},
                }
            ],
            "trajectory_meta": [
                {
                    "trial_key": "trial:assets",
                    "status": "passed",
                    "steps": [],
                    "warnings": [],
                }
            ],
        }
        css = load_asset_text("report.css")
        js = load_asset_text("report.js")
        template = load_asset_text("report.html")
        html = render_html(report)
        compact_css = compact_css_text(css)

        self.assertIn("__SERVE_SOURCE_MANAGER__", template)
        self.assertIn('<script type="application/json" id="peval-py-data">__DATA__</script>', template)
        self.assertIn(".time-gradient", css)
        self.assertIn("flex-wrap:wrap", css)
        self.assertIn(".timeline-waterfall-shell", css)
        self.assertIn(".timeline-waterfall-chart", css)
        self.assertIn(".timeline-fallback", css)
        self.assertIn(".timeline-section", css)
        self.assertIn(".timeline-section-body", css)
        self.assertIn(".source-adapter-select", css)
        self.assertIn(".source-add-actions", css)
        self.assertIn(".source-form select", css)
        self.assertNotIn(".adapter-choice-group", css)
        for level in range(1, 11):
            self.assertIn(f".trajectory-node.duration-heat-{level}", css)
        self.assertIn("background:#d6a456", css)
        self.assertNotIn("background:#b88431", css)
        self.assertNotIn("background:#56380d", css)
        self.assertNotIn("color:#fff8ec", css)
        self.assertIn('.trajectory-node.selected-node[class*="duration-heat-"]', css)
        self.assertNotIn(".trajectory-node.time-gradient", css)
        self.assertNotIn(".trajectory-node.selected-node.time-gradient", css)
        self.assertNotIn("conic-gradient", css)
        self.assertIn(".serve-notice", css)
        self.assertIn(
            compact_css_text(
                ".timeline-section{display:grid;gap:10px;border:1px solid var(--rule);"
                "border-radius:var(--radius);background:transparent;padding:12px}"
            ),
            compact_css,
        )
        self.assertIn(
            compact_css_text(
                ".source-form{display:grid;gap:8px;border:1px solid var(--rule);"
                "border-radius:var(--radius);background:transparent;padding:12px}"
            ),
            compact_css,
        )
        self.assertIn("table-layout:auto", css)
        self.assertIn("min-width:max-content", css)
        self.assertIn(
            compact_css_text(".data-table th,.data-table td{max-width:260px}"),
            compact_css,
        )
        self.assertIn(
            compact_css_text(
                "td.num,th.num{text-align:right;font-variant-numeric:tabular-nums;max-width:132px}"
            ),
            compact_css,
        )
        self.assertIn(".timeline-table td.metric-cell", css)
        self.assertIn(".timeline-table td.metric-cell.metric-shade-4", css)
        self.assertIn(
            compact_css_text(".timeline-detail-row{cursor:pointer}"),
            compact_css,
        )
        self.assertIn(
            compact_css_text(
                ".timeline-detail-selected td{background:color-mix(in oklch,var(--focus),#fff 92%)}"
            ),
            compact_css,
        )
        self.assertIn(
            compact_css_text(
                ".timeline-detail-selected td:first-child{box-shadow:inset 3px 0 0 var(--focus)}"
            ),
            compact_css,
        )
        self.assertNotIn(
            compact_css_text(
                ".timeline-detail-selected td{background:color-mix(in oklch,var(--focus),#fff 92%);box-shadow"
            ),
            compact_css,
        )
        self.assertIn(".timeline-stage-label", css)
        self.assertIn(".timeline-active-share", css)
        self.assertIn("--active-share-pct", css)
        self.assertIn("active-share-cell", css)
        self.assertIn(
            compact_css_text(
                ".timeline-category-external{background:#e2e8f0;color:#475569}"
            ),
            compact_css,
        )
        self.assertIn(
            compact_css_text(
                ".timeline-category-error{background:#fee2e2;color:#dc2626}"
            ),
            compact_css,
        )
        self.assertNotIn(".timeline-distribution", css)
        self.assertNotIn("table-layout:fixed", css)
        self.assertNotIn(".timeline-col-stage", css)
        self.assertNotIn(".timeline-category-chip", css)
        self.assertNotIn(".timeline-category-external,.timeline-category-error", css)
        self.assertNotIn(".timeline-category-idle", css)
        self.assertNotIn(".timeline-waterfall-svg", css)
        self.assertIn("function renderTrace()", js)
        self.assertIn("function renderDataTable", js)
        self.assertIn("function applyDataTableControls", js)
        self.assertIn("function bindDataTableControls", js)
        self.assertIn("function timelineDetailColumns", js)
        self.assertIn('bindDataTableControls(target, "leaderboard"', js)
        self.assertIn('bindDataTableControls(target, "timeline"', js)
        self.assertIn("function renderTimelineDiagnostics", js)
        self.assertIn("function renderTimelineSection", js)
        self.assertIn("function timelineTrace", js)
        self.assertIn("function timelineAssignActiveOffsets", js)
        self.assertIn("function timelineYAxisLabelWidth", js)
        self.assertIn("function timelineXAxisScale", js)
        self.assertIn("function timelineNiceIntervalMs", js)
        self.assertIn("function openTimelineStep", js)
        self.assertIn("function bindTimelineControls", js)
        self.assertIn("function initTimelineWaterfallChart", js)
        self.assertIn('const SUBMENU_DETAILS_SELECTOR = ".export-menu,.filter-control"', js)
        self.assertIn(
            'const OPEN_SUBMENU_DETAILS_SELECTOR = ".export-menu[open],.filter-control[open]"',
            js,
        )
        self.assertIn("function closeOpenSubmenus", js)
        self.assertIn(
            "closeOpenSubmenus(event.target?.closest?.(SUBMENU_DETAILS_SELECTOR) || null)",
            js,
        )
        self.assertIn('button.closest("details")?.removeAttribute("open")', js)
        self.assertIn('node.addEventListener("click", event => event.stopPropagation())', js)
        self.assertIn('state.timelineChart.on("click"', js)
        self.assertIn("if (!item || !item.step_id) return;", js)
        self.assertIn("window.echarts.init", js)
        self.assertIn('type: "custom"', js)
        self.assertIn("active_total_ms", js)
        self.assertIn("const label = api.value(5)", js)
        self.assertIn("const color = api.value(4)", js)
        self.assertIn('color: "#64748b"', js)
        self.assertIn("grid: { left: labelWidth + 18", js)
        self.assertIn("const xAxisScale = timelineXAxisScale", js)
        self.assertIn("max: xAxisScale.max", js)
        self.assertIn("interval: xAxisScale.interval", js)
        self.assertIn("minInterval: xAxisScale.interval", js)
        self.assertIn("hideOverlap: true", js)
        self.assertIn("formatter: value => fmtTimelineAxis(value, xAxisScale.interval)", js)
        self.assertNotIn("formatter: value => fmtTimelineAxis(value),", js)
        self.assertNotIn("function timelineBreakMarkArea", js)
        self.assertNotIn("function timelineAxisBreaks", js)
        self.assertNotIn("TIMELINE_IDLE_GAP_BREAK_THRESHOLD_MS", js)
        self.assertNotIn("function renderTimelineWaterfallSvg", js)
        self.assertNotIn("<colgroup><col class=\"timeline-col-row\"", js)
        self.assertNotIn("column.width", js)
        self.assertIn("<style>\n:root", html)
        self.assertIn(
            "https://cdn.jsdelivr.net/npm/echarts@6.0.0/dist/echarts.min.js",
            html,
        )
        self.assertIn("function renderTrace()", html)
        self.assertIn('<script type="application/json" id="peval-py-data">', html)
        self.assertNotIn("__SERVE_SOURCE_MANAGER__", html)
        self.assertNotIn("__DATA__", html)
        self.assertNotIn("__CSS__", html)
        self.assertNotIn("__JS__", html)


    def test_html_timing_gradients_ignore_missing_values_without_mutating_report(self) -> None:
        report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:missing-time",
                    "session_id": "missing-time",
                    "agent": {"name": "custom"},
                    "steps": [
                        {
                            "step_id": 1,
                            "source": "agent",
                            "message": "no timing",
                            "tool_calls": [
                                {
                                    "tool_call_id": "call-1",
                                    "function_name": "exec_command",
                                    "arguments": {"cmd": "true"},
                                }
                            ],
                        }
                    ],
                    "final_metrics": {},
                }
            ],
            "trajectory_meta": [
                {
                    "trial_key": "trial:missing-time",
                    "status": "passed",
                    "steps": [
                        {
                            "step_id": 1,
                            "duration_ms": 0,
                            "elapsed_ms": None,
                            "tool_calls": [
                                {
                                    "tool_call_id": "call-1",
                                    "title": "exec_command",
                                    "execution_duration_ms": None,
                                }
                            ],
                        }
                    ],
                    "warnings": [],
                }
            ],
        }
        before = json.loads(json.dumps(report))
        html = render_html(report)
        payload = script_json(html, "peval-py-data")

        self.assertEqual(report, before)
        self.assertEqual(payload, before)
        self.assertIn("function positiveMetric", html)
        self.assertIn("if (!positiveMetric(value) || !positiveMetric(max)) return null", html)


    def test_html_estimates_missing_step_token_chips_without_mutating_report(self) -> None:
        report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:estimate",
                    "session_id": "estimate-session",
                    "agent": {"name": "custom", "model_name": "unknown-model"},
                    "steps": [
                        {
                            "step_id": 1,
                            "source": "agent",
                            "message": "abcdefgh",
                            "tool_calls": [
                                {
                                    "tool_call_id": "call-1",
                                    "function_name": "read",
                                    "arguments": {"path": "README.md"},
                                }
                            ],
                        }
                    ],
                    "final_metrics": {"total_prompt_tokens": 100},
                }
            ],
            "trajectory_meta": [
                {
                    "trial_key": "trial:estimate",
                    "status": "passed",
                    "steps": [{"step_id": 1, "duration_ms": None}],
                    "warnings": [],
                }
            ],
        }
        before = json.loads(json.dumps(report))
        with patch("peval_py.html.import_module", side_effect=ImportError("missing")):
            html = render_html(report)

        self.assertEqual(report, before)
        estimates = script_json(html, "peval-py-token-estimates")
        self.assertIn("trial:estimate", estimates)
        estimate = estimates["trial:estimate"]["1"]
        self.assertEqual(estimate["method"], "byte_length_div_4")
        self.assertEqual(estimate["source"], "visible_step_text")
        self.assertTrue(estimate["estimated"])
        self.assertGreater(estimate["tokens"], 0)
        self.assertIn("renderStepRail(step, sm, meta?.trial_key, timingStats)", html)
        self.assertIn("stepTokenInfo(step, trialKey)", html)
        self.assertIn("stepTokenEstimate(trialKey, step.step_id)", html)
        self.assertIn("estimated tokens", html)
        self.assertIn("from visible step text", html)
        self.assertIn("≈", html)
        self.assertNotIn("estimated", script_json(html, "peval-py-data")["trajectory"][0]["steps"][0])


    def test_html_preserves_exact_step_tokens_without_estimate(self) -> None:
        report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:exact",
                    "session_id": "exact-session",
                    "agent": {"name": "custom", "model_name": "unknown-model"},
                    "steps": [
                        {
                            "step_id": 1,
                            "source": "agent",
                            "message": "abcdefgh",
                            "metrics": {"prompt_tokens": 3, "completion_tokens": 4},
                        }
                    ],
                    "final_metrics": {"total_prompt_tokens": 3, "total_completion_tokens": 4},
                }
            ],
            "trajectory_meta": [
                {
                    "trial_key": "trial:exact",
                    "status": "passed",
                    "steps": [{"step_id": 1, "duration_ms": None}],
                    "warnings": [],
                }
            ],
        }
        with patch("peval_py.html.import_module", side_effect=ImportError("missing")):
            html = render_html(report)

        self.assertEqual(script_json(html, "peval-py-token-estimates"), {})
        payload = script_json(html, "peval-py-data")
        self.assertEqual(payload["trajectory"][0]["steps"][0]["metrics"]["prompt_tokens"], 3)


    def test_html_estimated_tokens_can_use_optional_tiktoken(self) -> None:
        class FakeEncoding:
            name = "fake-model-encoding"

            def encode(self, text: str):
                return list(range(7))

        class FakeTiktoken:
            def encoding_for_model(self, model: str):
                self.model = model
                return FakeEncoding()

            def get_encoding(self, name: str):
                raise AssertionError("model encoding should be used")

        report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:tiktoken",
                    "session_id": "tiktoken-session",
                    "agent": {"name": "custom", "model_name": "fake-model"},
                    "steps": [
                        {
                            "step_id": 1,
                            "source": "agent",
                            "message": "model counted text",
                        }
                    ],
                    "final_metrics": {},
                }
            ],
            "trajectory_meta": [
                {
                    "trial_key": "trial:tiktoken",
                    "status": "passed",
                    "steps": [{"step_id": 1, "duration_ms": None}],
                    "warnings": [],
                }
            ],
        }
        fake = FakeTiktoken()
        with patch("peval_py.html.import_module", return_value=fake):
            html = render_html(report)

        self.assertEqual(fake.model, "fake-model")
        estimate = script_json(html, "peval-py-token-estimates")["trial:tiktoken"]["1"]
        self.assertEqual(estimate["tokens"], 7)
        self.assertEqual(estimate["method"], "tiktoken:fake-model-encoding")
