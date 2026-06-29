from __future__ import annotations

from reports_html_support import *

class PevalPyReportHtmlInteractionTests(unittest.TestCase):
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
