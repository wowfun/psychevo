from __future__ import annotations

from reports_html_support import *

class PevalPyReportHtmlTimelineTests(unittest.TestCase):
    def test_timeline_numbers_are_step_scoped(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js timeline helpers")
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:numbering",
                    "session_id": "numbering",
                    "agent": {"name": "custom", "model_name": "test-model"},
                    "steps": [
                        {"step_id": 1, "source": "user", "message": "start"},
                        {
                            "step_id": 2,
                            "source": "agent",
                            "message": "run tool",
                            "tool_calls": [
                                {"tool_call_id": "call-read", "function_name": "read_file"}
                            ],
                        },
                    ],
                    "final_metrics": {},
                }
            ],
            "trajectory_meta": [
                {
                    "trial_key": "trial:numbering",
                    "status": "passed",
                    "started_at_ms": 1_000,
                    "duration_ms": 80,
                    "steps": [
                        {"step_id": 1, "timestamp_ms": 1_000, "duration_ms": None},
                        {
                            "step_id": 2,
                            "timestamp_ms": 1_100,
                            "duration_ms": 80,
                            "tool_calls": [
                                {
                                    "tool_call_id": "call-read",
                                    "title": "read_file",
                                    "timestamp_ms": 1_130,
                                    "execution_duration_ms": 20,
                                    "status": "ok",
                                }
                            ],
                        },
                    ],
                }
            ],
        }
        script = f"""
const vm = require("vm");
const asset = {json.dumps(asset)};
const report = {json.dumps(report)};
const context = {{
  document: {{
    body: {{ classList: {{ add() {{}}, remove() {{}}, toggle() {{}} }} }},
    addEventListener() {{}},
    getElementById() {{ return null; }},
    querySelector() {{ return null; }},
    querySelectorAll() {{ return []; }},
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
  requestAnimationFrame(callback) {{ callback(); }},
  report,
}};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`(() => {{
  const trace = timelineTrace(report.trajectory[0], report.trajectory_meta[0]);
  return JSON.stringify(trace.stages.map(stage => [stage.number, stage.stage]));
}})()`, context);
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
        self.assertEqual(
            json.loads(node.stdout),
            [["2.1", "Model: test-model"], ["2.2", "Tool: read_file"]],
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
