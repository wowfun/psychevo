from __future__ import annotations

from reports_html_support import *

class PevalPyReportHtmlCoreTests(unittest.TestCase):
    def test_report_json_subset_and_html_safe_embedding(self) -> None:
        records = read_jsonl(str(FIXTURES / "psychevo_session.jsonl"))
        config = ToolConfig(adapter="psychevo")
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
        self.assertEqual(analysis["trial_key"], "session:t001")
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
