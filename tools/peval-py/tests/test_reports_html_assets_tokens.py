from __future__ import annotations

from importlib.resources import as_file, files

from peval_py.html import ASSET_BUNDLES
from reports_html_support import *

class PevalPyReportHtmlAssetTokenTests(unittest.TestCase):
    def test_report_js_asset_parts_have_valid_syntax(self) -> None:
        node = shutil.which("node")
        if not node:
            self.skipTest("node is required for report JS syntax validation")
        asset_root = files("peval_py.assets")
        for name in ASSET_BUNDLES["report.js"]:
            with self.subTest(asset=name), as_file(asset_root.joinpath(name)) as path:
                result = subprocess.run(
                    [node, "--check", str(path)],
                    text=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                )
                self.assertEqual(result.returncode, 0, result.stderr)

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

        self.assertIn("report_css/06-leaderboard-summary.css", ASSET_BUNDLES["report.css"])
        self.assertIn("report_js/06-leaderboard-summary.js", ASSET_BUNDLES["report.js"])
        self.assertIn("report_js/09-source-state-controls.js", ASSET_BUNDLES["report.js"])
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
        self.assertIn(".leaderboard-summary-layout", css)
        self.assertIn(".leaderboard-summary-table-panel", css)
        self.assertIn(".leaderboard-summary-table", css)
        self.assertIn(".leaderboard-summary-chart-panel", css)
        self.assertIn(".summary-boxplot-card", css)
        self.assertIn(".summary-boxplot-vertical", css)
        self.assertIn(".summary-boxplot-median", css)
        self.assertIn("--summary-whisker-bottom", css)
        self.assertIn("--summary-p95-bottom", css)
        self.assertIn(".trajectory-row.trajectory-row-selectable", css)
        self.assertIn(".trajectory-select", css)
        self.assertIn(".source-state-controls", css)
        self.assertIn(".source-state-toggle", css)
        self.assertIn(".source-state-action", css)
        self.assertNotIn(".leaderboard-summary-count", css)
        self.assertNotIn(".leaderboard-summary-distribution", css)
        self.assertNotIn("--summary-whisker-left", css)
        self.assertNotIn("--summary-p95-left", css)
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
        self.assertIn("if (metas.length >= 1)", js)
        self.assertIn("rows.length > 1", js)
        self.assertIn('if (rows.length > 1 && $("leaderboard-summary")) renderLeaderboardSummary(rows);', js)
        self.assertIn("function renderLeaderboardSummary(rows = leaderboardRows())", js)
        self.assertIn("function leaderboardSummaryRows(rows = leaderboardRows())", js)
        self.assertIn("function measuredModelDurationForRow(row)", js)
        self.assertIn("function leaderboardSummaryStatistics()", js)
        self.assertIn("function renderLeaderboardSummaryDistributionPanel(rows)", js)
        self.assertIn("function renderLeaderboardSummaryBoxplotCard(row)", js)
        self.assertIn("function renderLeaderboardSummaryBoxplot(row)", js)
        self.assertIn("function renderServeSourceStateControls(rows = leaderboardRows())", js)
        self.assertIn("function switchServeSourceMode(mode)", js)
        self.assertIn("function mutateVisibleServeSourceState()", js)
        self.assertIn("function applyServeSourceStateMutationPayload(payload, options = {})", js)
        self.assertIn("function firstReadableSourceKeyFrom(sourceKeys, sources, mode)", js)
        self.assertIn("function readableServeSourcesFrom(sources, mode = currentServeSourceMode())", js)
        self.assertIn("const emptiedCurrentMode = payload?.report && listValue(payload.report?.trajectory_meta).length === 0", js)
        self.assertIn("if (!trial?.trial_key)", js)
        self.assertIn("!state.notesEditor", js)
        self.assertIn("trajectory-row-selectable", js)
        self.assertIn("trajectory-select", js)
        self.assertIn("bindServeSelectionControls(target);", js)
        self.assertIn('/api/report?source_state=', js)
        self.assertIn('/api/sources/state', js)
        self.assertIn("show_archived", js)
        self.assertIn("targetReadableCount < 1", js)
        self.assertIn("readableServeSources(nextMode).length < 1", js)
        self.assertNotIn("targetReadableCount < 2", js)
        self.assertNotIn("readableServeSources(nextMode).length < 2", js)
        self.assertNotIn("Not enough archived sessions", js)
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
        self.assertIn('id="leaderboard-summary"', html)
        self.assertIn("function renderLeaderboardSummary(rows = leaderboardRows())", html)
        self.assertIn("function renderServeSourceStateControls(rows = leaderboardRows())", html)
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
