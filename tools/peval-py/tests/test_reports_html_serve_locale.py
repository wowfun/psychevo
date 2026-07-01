from __future__ import annotations

from reports_html_support import *

class PevalPyReportHtmlServeLocaleTests(unittest.TestCase):
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
        self.assertIn('t("export_xlsx_table", "Table (.xlsx)")', serve_html)
        self.assertIn('data-export-kind="xlsx"', serve_html)
        self.assertNotIn('data-export-kind="csv"', serve_html)
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
        self.assertIn("peval-leaderboard-visible.xlsx", serve_html)
        self.assertIn("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", serve_html)
        self.assertIn("function xlsxBytesForRows(rows)", serve_html)
        self.assertNotIn("peval-leaderboard-visible.csv", serve_html)


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
        config = ToolConfig(adapter="psychevo")
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
