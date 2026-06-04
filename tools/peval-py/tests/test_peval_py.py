from __future__ import annotations

import json
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from peval_py.atif import convert_records
from peval_py.config import ToolConfig, load_config
from peval_py.html import render_html
from peval_py.report import NoteInput, ReportSession, build_multi_report, build_report
from peval_py.sources import (
    ACCOUNTING_COLUMNS,
    MessageRecord,
    read_jsonl,
    read_sqlite_messages,
)

FIXTURES = Path(__file__).parent / "fixtures"


def create_messages_db(path: Path) -> None:
    conn = sqlite3.connect(path)
    columns = ", ".join(f"{name} INTEGER" for name in ACCOUNTING_COLUMNS[:-2])
    conn.execute(
        f"""
        CREATE TABLE messages (
            session_id TEXT,
            session_seq INTEGER,
            message_json TEXT,
            usage_json TEXT,
            metadata_json TEXT,
            {columns},
            pricing_source TEXT,
            pricing_tier TEXT
        )
        """
    )
    rows = [
        (
            "db-a",
            1,
            {"role": "user", "content": "hello a", "timestamp_ms": 100},
            None,
            None,
        ),
        (
            "db-a",
            2,
            {
                "role": "assistant",
                "content": "done a",
                "timestamp_ms": 200,
                "model": "db-model-a",
            },
            {"input_tokens": 2, "output_tokens": 3},
            None,
        ),
        (
            "db-b",
            1,
            {"role": "user", "content": "hello b", "timestamp_ms": 300},
            None,
            None,
        ),
        (
            "db-b",
            2,
            {
                "role": "assistant",
                "content": "done b",
                "timestamp_ms": 450,
                "model": "db-model-b",
            },
            {"input_tokens": 5, "output_tokens": 7},
            None,
        ),
    ]
    for session_id, seq, message, usage, metadata in rows:
        conn.execute(
            """
            INSERT INTO messages
            (session_id, session_seq, message_json, usage_json, metadata_json)
            VALUES (?, ?, ?, ?, ?)
            """,
            (
                session_id,
                seq,
                json.dumps(message),
                json.dumps(usage) if usage else None,
                json.dumps(metadata) if metadata else None,
            ),
        )
    conn.commit()
    conn.close()


class PevalPyTests(unittest.TestCase):
    def test_config_uses_adapter_default_and_accepts_legacy_agent_key(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            adapter_config = Path(tmp) / "adapter.toml"
            adapter_config.write_text(
                "[defaults]\nadapter = \"opencode\"\n",
                encoding="utf-8",
            )
            self.assertEqual(load_config(str(adapter_config)).adapter, "opencode")

            legacy_config = Path(tmp) / "legacy.toml"
            legacy_config.write_text(
                "[defaults]\nagent = \"hermes\"\n",
                encoding="utf-8",
            )
            self.assertEqual(load_config(str(legacy_config)).adapter, "hermes")

    def test_psychevo_sqlite_messages_are_ordered_by_session_seq(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            db_path = Path(tmp) / "state.db"
            conn = sqlite3.connect(db_path)
            columns = ", ".join(f"{name} INTEGER" for name in ACCOUNTING_COLUMNS[:-2])
            conn.execute(
                f"""
                CREATE TABLE messages (
                    session_id TEXT,
                    session_seq INTEGER,
                    message_json TEXT,
                    usage_json TEXT,
                    metadata_json TEXT,
                    {columns},
                    pricing_source TEXT,
                    pricing_tier TEXT
                )
                """
            )
            user = {"role": "user", "content": [{"text": "first"}], "timestamp_ms": 100}
            assistant = {
                "role": "assistant",
                "content": [{"type": "text", "text": "second"}],
                "timestamp_ms": 200,
                "model": "db-model",
            }
            conn.execute(
                "INSERT INTO messages (session_id, session_seq, message_json) VALUES (?, ?, ?)",
                ("s1", 2, json.dumps(assistant)),
            )
            conn.execute(
                "INSERT INTO messages (session_id, session_seq, message_json) VALUES (?, ?, ?)",
                ("s1", 1, json.dumps(user)),
            )
            conn.commit()
            conn.close()

            records = read_sqlite_messages(str(db_path), "s1", ToolConfig().db)
            self.assertEqual([record.session_seq for record in records], [1, 2])
            self.assertEqual([record.source_session_id for record in records], ["s1", "s1"])
            result = convert_records(records, ToolConfig(adapter="psychevo"))
            self.assertEqual(result.trajectory["session_id"], "s1")
            self.assertEqual(result.trajectory["steps"][0]["message"], "first")
            self.assertEqual(result.trajectory["steps"][1]["message"], "second")
            self.assertEqual(result.trajectory["agent"]["model_name"], "db-model")

    def test_psychevo_jsonl_conversion_shape(self) -> None:
        records = read_jsonl(str(FIXTURES / "psychevo_session.jsonl"))
        result = convert_records(
            records,
            ToolConfig(adapter="psychevo", trajectory_id="trial:psychevo"),
        )
        trajectory = result.trajectory
        self.assertEqual(trajectory["schema_version"], "ATIF-v1.7")
        self.assertEqual(trajectory["trajectory_id"], "trial:psychevo")
        self.assertEqual(trajectory["session_id"], "sess-psychevo")
        self.assertEqual([step["step_id"] for step in trajectory["steps"]], [1, 2, 3])
        self.assertEqual(trajectory["steps"][0]["source"], "user")
        self.assertEqual(trajectory["steps"][1]["reasoning_content"], "Inspect the file first.")
        self.assertEqual(
            trajectory["steps"][1]["tool_calls"][0]["tool_call_id"],
            "call-1",
        )
        self.assertEqual(
            trajectory["steps"][1]["tool_calls"][0]["arguments"]["api_key"],
            "<redacted>",
        )
        self.assertEqual(
            trajectory["steps"][1]["observation"]["results"][0]["source_call_id"],
            "call-1",
        )
        self.assertEqual(trajectory["steps"][2]["message"], "Done.")
        self.assertEqual(trajectory["final_metrics"]["total_prompt_tokens"], 8)
        self.assertEqual(trajectory["final_metrics"]["total_completion_tokens"], 10)
        self.assertEqual(trajectory["final_metrics"]["total_tool_calls"], 1)
        self.assertEqual(trajectory["final_metrics"]["total_tool_errors"], 0)
        self.assertEqual(trajectory["final_metrics"]["total_steps"], 3)
        self.assertEqual(trajectory["final_metrics"]["usage"]["input_tokens"], 8)
        self.assertEqual(trajectory["final_metrics"]["usage"]["output_tokens"], 10)
        self.assertEqual(trajectory["final_metrics"]["usage"]["cache_read_tokens"], 1)

        tool_meta = result.steps_meta[1].tool_calls[0]
        self.assertEqual(tool_meta.status, "completed")
        self.assertEqual(tool_meta.execution_duration_ms, 321)
        self.assertEqual(tool_meta.execution_duration_source, "message_metadata")

    def test_opencode_and_hermes_use_common_jsonl_adapter(self) -> None:
        records = read_jsonl(str(FIXTURES / "common_session.jsonl"))
        for adapter in ["opencode", "hermes"]:
            with self.subTest(adapter=adapter):
                result = convert_records(records, ToolConfig(adapter=adapter))
                self.assertEqual(result.trajectory["agent"]["name"], adapter)
                self.assertEqual(result.trajectory["steps"][0]["source"], "system")
                self.assertEqual(
                    result.trajectory["steps"][2]["tool_calls"][0]["function_name"],
                    "list",
                )
                self.assertEqual(
                    result.trajectory["steps"][2]["observation"]["results"][0][
                        "source_call_id"
                    ],
                    "tool-1",
                )

    def test_tool_timing_fallback_and_unmatched_warning(self) -> None:
        records = [
            MessageRecord(
                message={
                    "role": "assistant",
                    "tool_calls": [
                        {
                            "tool_call_id": "call-fallback",
                            "function_name": "read",
                            "arguments": {"path": "add.py"},
                        }
                    ],
                    "timestamp_ms": 1000,
                }
            ),
            MessageRecord(
                message={
                    "role": "tool_result",
                    "tool_call_id": "call-fallback",
                    "tool_name": "read",
                    "content": "ok",
                    "timestamp_ms": 1250,
                }
            ),
        ]
        result = convert_records(records, ToolConfig(adapter="psychevo"))
        self.assertEqual(len(result.trajectory["steps"]), 1)
        self.assertEqual(
            result.trajectory["steps"][0]["observation"]["results"][0][
                "source_call_id"
            ],
            "call-fallback",
        )
        tool_meta = result.steps_meta[0].tool_calls[0]
        self.assertEqual(tool_meta.execution_duration_ms, 250)
        self.assertEqual(tool_meta.execution_duration_source, "event_timestamps")

        unmatched = convert_records(
            [
                MessageRecord(
                    message={
                        "role": "tool_result",
                        "tool_call_id": "missing-call",
                        "tool_name": "read",
                        "content": "orphan",
                        "timestamp_ms": 1500,
                    }
                )
            ],
            ToolConfig(adapter="psychevo"),
        )
        self.assertEqual(len(unmatched.trajectory["steps"]), 1)
        self.assertEqual(
            unmatched.trajectory["steps"][0]["observation"]["results"][0][
                "source_call_id"
            ],
            "missing-call",
        )
        self.assertIn("unmatched tool result: missing-call", unmatched.warnings)

    def test_mid_session_tool_failure_is_nested_and_flow_continues(self) -> None:
        records = [
            MessageRecord(message={"role": "user", "content": "fix add.py", "timestamp_ms": 900}),
            MessageRecord(
                message={
                    "role": "assistant",
                    "content": "I will run tests.",
                    "tool_calls": [
                        {
                            "tool_call_id": "call-test-1",
                            "function_name": "exec_command",
                            "arguments": {"cmd": "pytest"},
                        }
                    ],
                    "timestamp_ms": 1000,
                }
            ),
            MessageRecord(
                message={
                    "role": "tool_result",
                    "tool_call_id": "call-test-1",
                    "tool_name": "exec_command",
                    "content": {"exit_code": 1, "stderr": "AssertionError"},
                    "is_error": True,
                    "timestamp_ms": 1125,
                },
                metadata={"elapsed_ms": 125},
            ),
            MessageRecord(
                message={
                    "role": "assistant",
                    "content": "I will inspect the failure.",
                    "tool_calls": [
                        {
                            "tool_call_id": "call-read-1",
                            "function_name": "read",
                            "arguments": {"path": "add.py"},
                        }
                    ],
                    "timestamp_ms": 1200,
                }
            ),
            MessageRecord(
                message={
                    "role": "tool_result",
                    "tool_call_id": "call-read-1",
                    "tool_name": "read",
                    "content": "def add(a, b): return a - b",
                    "timestamp_ms": 1300,
                }
            ),
            MessageRecord(
                message={
                    "role": "assistant",
                    "content": "The failing test is explained.",
                    "timestamp_ms": 1400,
                }
            ),
        ]

        result = convert_records(records, ToolConfig(adapter="psychevo"))
        trajectory = result.trajectory

        self.assertEqual([step["step_id"] for step in trajectory["steps"]], [1, 2, 3, 4])
        self.assertEqual(
            trajectory["steps"][1]["observation"]["results"][0]["source_call_id"],
            "call-test-1",
        )
        self.assertEqual(
            trajectory["steps"][2]["observation"]["results"][0]["source_call_id"],
            "call-read-1",
        )
        self.assertEqual(trajectory["final_metrics"]["total_tool_calls"], 2)
        self.assertEqual(trajectory["final_metrics"]["total_tool_errors"], 1)
        self.assertEqual(result.steps_meta[1].tool_calls[0].status, "error")
        self.assertTrue(result.steps_meta[1].tool_error)
        self.assertEqual(result.steps_meta[1].tool_calls[0].execution_duration_ms, 125)
        self.assertEqual(result.steps_meta[2].tool_calls[0].status, "completed")
        self.assertFalse(result.steps_meta[2].tool_error)
        self.assertEqual(result.warnings, [])

        config = ToolConfig(adapter="psychevo", trajectory_id="trial:failure")
        report = build_report(result, config, "inline")
        step_meta = report["trajectory_meta"][0]["steps"]
        self.assertEqual(step_meta[1]["duration_ms"], 125)
        self.assertEqual(step_meta[2]["duration_ms"], 100)
        html = render_html(report)
        self.assertIn("tool-error-chip", html)
        self.assertIn("exec_command", html)
        self.assertIn("tool success / total", html)

    def test_malformed_jsonl_reports_line_number(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bad.jsonl"
            path.write_text('{"role":"user","content":"ok"}\n{bad}\n', encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "line 2"):
                read_jsonl(str(path))

    def test_report_json_subset_and_html_safe_embedding(self) -> None:
        records = read_jsonl(str(FIXTURES / "psychevo_session.jsonl"))
        config = ToolConfig(adapter="psychevo", trajectory_id="trial:html")
        result = convert_records(records, config)
        report = build_report(result, config, "psychevo_session.jsonl")
        self.assertEqual(report["schema_version"], 17)
        self.assertEqual(report["includes"], ["core"])
        self.assertIn("trajectory", report)
        self.assertIn("trajectory_meta", report)
        self.assertEqual(report["trajectory_meta"][0]["adapter"], "psychevo")
        self.assertEqual(report["trajectory_meta"][0]["status"], "passed")

        html = render_html(report)
        self.assertIn("data-step-action=\"toggle\"", html)
        self.assertIn("<h1>Agent Trajectory Report</h1>", html)
        self.assertNotIn("<p class=\"eyebrow\">agent trajectory</p>", html)
        self.assertNotIn("id=\"report-copy\"", html)
        self.assertNotIn("id=\"score-strip\"", html)
        self.assertNotIn("class=\"metric-card\"", html)
        self.assertIn("<h3>Run</h3>", html)
        self.assertIn("<h3>Result</h3>", html)
        self.assertIn("<h3>Evidence</h3>", html)
        self.assertIn("Usage Breakdown", html)
        self.assertIn("wall duration", html)
        self.assertIn("tool success / total", html)
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

        self.assertEqual(report["includes"], ["core", "comparison", "annotations"])
        self.assertEqual(len(report["trajectory"]), 2)
        self.assertEqual(report["trajectory"][0]["session_id"], "common_session")
        self.assertEqual(report["trajectory"][1]["session_id"], "sess-psychevo")
        self.assertEqual(report["comparison"]["summary"]["session_count"], 2)
        self.assertEqual(
            report["comparison"]["selected_trial_key"],
            report["trajectory_meta"][0]["trial_key"],
        )
        self.assertEqual(report["comparison"]["default_metric"], "duration")
        rows = report["comparison"]["session_table"]["rows"]
        self.assertEqual(len(rows), 2)
        entries = report["comparison"]["leaderboard"]["entries"]
        self.assertEqual(len(entries), 2)
        forbidden = {"benchmark", "task", "task_id", "task_set_id", "task_family"}
        for row in [*rows, *entries]:
            self.assertTrue(forbidden.isdisjoint(row))
        self.assertEqual(report["annotations"]["report_notes"][0]["markdown"], "Report <script>note</script>")
        self.assertEqual(
            report["annotations"]["notes"][0]["trial_key"],
            report["trajectory_meta"][1]["trial_key"],
        )

        html = render_html(report)
        self.assertNotIn("<h3>Summary</h3>", html)
        self.assertNotIn("Session Heatmap", html)
        self.assertNotIn("Session Table", html)
        self.assertIn("report-note-list", html)
        self.assertIn("report-note", html)
        self.assertIn("Visible Heatmap", html)
        self.assertIn("session-axis", html)
        self.assertIn("grid-template-columns:minmax(150px,220px) minmax(0,1fr)", html)
        self.assertNotIn("repeat(${Math.max(rows.length, 1)}, minmax(150px, 1fr))", html)
        self.assertIn("metric-button", html)
        self.assertIn('label: "Duration"', html)
        self.assertIn('label: "Tokens"', html)
        self.assertIn('label: "Tool Calls"', html)
        self.assertIn('label: "Turns"', html)
        self.assertIn("Leaderboard", html)
        self.assertIn("data-table-sort", html)
        self.assertIn("selected-row", html)
        self.assertIn("data-trial-key", html)
        self.assertIn("selected trial trajectory", html)
        self.assertIn("note-list", html)
        self.assertIn("note-snippet", html)
        self.assertIn("Second session note", html)
        self.assertIn("Report \\u003cscript", html)
        self.assertNotIn("<script>note</script>", html)

    def test_cli_db_multi_session_view_and_note_validation(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            db_path = Path(tmp) / "state.db"
            out_path = Path(tmp) / "report.json"
            create_messages_db(db_path)

            result = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "peval_py.cli",
                    "view",
                    "tr",
                    "-d",
                    str(db_path),
                    "-s",
                    "db-a",
                    "-s",
                    "db-b",
                    "-n",
                    "0=DB report",
                    "--note",
                    "2=DB B",
                    "-f",
                    "json",
                    "-o",
                    str(out_path),
                ],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            payload = json.loads(out_path.read_text(encoding="utf-8"))
            self.assertEqual([item["session_id"] for item in payload["trajectory"]], ["db-a", "db-b"])
            self.assertEqual(payload["comparison"]["summary"]["session_count"], 2)
            self.assertEqual(payload["annotations"]["report_notes"][0]["markdown"], "DB report")
            self.assertEqual(payload["annotations"]["notes"][0]["markdown"], "DB B")

            bad_note = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "peval_py.cli",
                    "view",
                    "tr",
                    "-d",
                    str(db_path),
                    "-s",
                    "db-a",
                    "-n",
                    "2=missing",
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(bad_note.returncode, 0)
            self.assertIn("out of range", bad_note.stderr)

    def test_cli_multi_path_rules_and_export_single_session_boundary(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_path = Path(tmp) / "multi.json"
            result = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "peval_py.cli",
                    "view",
                    "tr",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-p",
                    str(FIXTURES / "psychevo_session.jsonl"),
                    "-n",
                    "1=First session note",
                    "-f",
                    "json",
                    "-o",
                    str(out_path),
                ],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            payload = json.loads(out_path.read_text(encoding="utf-8"))
            self.assertEqual(len(payload["trajectory"]), 2)
            self.assertIn("comparison", payload)
            subprocess.run(
                [sys.executable, "-m", "json.tool", str(out_path)],
                check=True,
                text=True,
                capture_output=True,
            )

            mixed = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "peval_py.cli",
                    "view",
                    "tr",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-d",
                    str(Path(tmp) / "state.db"),
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(mixed.returncode, 0)

            export_multi = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "peval_py.cli",
                    "export",
                    "tr",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-p",
                    str(FIXTURES / "psychevo_session.jsonl"),
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(export_multi.returncode, 0)
            self.assertIn("exactly one --path", export_multi.stderr)

            legacy_jsonl_flag = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "peval_py.cli",
                    "view",
                    "tr",
                    "-j",
                    str(FIXTURES / "common_session.jsonl"),
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(legacy_jsonl_flag.returncode, 0)

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
        self.assertIn("function fmtRailTokens", html)
        self.assertIn("fmtRailTokens(tokens)", html)
        self.assertIn("fmtNum(tokens)", html)
        self.assertIn("Tool Calls", html)
        self.assertIn("Observations", html)
        self.assertEqual(
            report["trajectory_meta"][0]["steps"][0]["tool_calls"][0][
                "execution_duration_ms"
            ],
            101,
        )

    def test_redaction_preserves_numeric_usage_metrics_and_masks_secrets(self) -> None:
        records = [
            MessageRecord(
                message={
                    "role": "assistant",
                    "content": "Use token=abc123 only in memory.",
                    "tool_calls": [
                        {
                            "tool_call_id": "call-secret",
                            "function_name": "exec_command",
                            "arguments": {
                                "api_key": "sk-secret",
                                "token_count": 7,
                                "token": "abc123",
                            },
                        }
                    ],
                    "timestamp_ms": 1000,
                },
                usage={"input_tokens": 11, "output_tokens": 13},
                accounting={"billable_input_tokens": 11, "billable_output_tokens": 13},
            )
        ]
        config = ToolConfig(adapter="psychevo", trajectory_id="trial:redaction")
        result = convert_records(records, config)
        step = result.trajectory["steps"][0]
        args = step["tool_calls"][0]["arguments"]
        self.assertEqual(step["message"], "Use token=<redacted> only in memory.")
        self.assertEqual(args["api_key"], "<redacted>")
        self.assertEqual(args["token"], "<redacted>")
        self.assertEqual(args["token_count"], 7)

        report = build_report(result, config, "inline")
        metrics = report["trajectory"][0]["final_metrics"]
        self.assertEqual(metrics["usage"]["input_tokens"], 11)
        self.assertEqual(metrics["usage"]["output_tokens"], 13)
        self.assertEqual(metrics["accounting"]["billable_input_tokens"], 11)
        self.assertEqual(metrics["accounting"]["billable_output_tokens"], 13)

    def test_cli_view_export_alias_smoke_and_legacy_commands_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            report_out = Path(tmp) / "report.json"
            export_out = Path(tmp) / "trajectory.json"
            command = shutil.which("peval-py") or "peval-py"
            result = subprocess.run(
                [
                    command,
                    "view",
                    "tr",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-o",
                    str(report_out),
                ],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            payload = json.loads(report_out.read_text(encoding="utf-8"))
            self.assertEqual(payload["trajectory"][0]["agent"]["name"], "opencode")
            subprocess.run(
                [sys.executable, "-m", "json.tool", str(report_out)],
                check=True,
                text=True,
                capture_output=True,
            )

            result = subprocess.run(
                [
                    command,
                    "export",
                    "tr",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-o",
                    str(export_out),
                ],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            payload = json.loads(export_out.read_text(encoding="utf-8"))
            self.assertEqual(payload["agent"]["name"], "opencode")
            subprocess.run(
                [sys.executable, "-m", "json.tool", str(export_out)],
                check=True,
                text=True,
                capture_output=True,
            )

            for legacy in ["report", "convert"]:
                with self.subTest(legacy=legacy):
                    result = subprocess.run(
                        [command, legacy, "--help"],
                        check=False,
                        text=True,
                        capture_output=True,
                    )
                    self.assertNotEqual(result.returncode, 0)

            for verb in ["view", "export"]:
                with self.subTest(verb=verb):
                    result = subprocess.run(
                        [command, verb, "trajectory", "--help"],
                        check=False,
                        text=True,
                        capture_output=True,
                    )
                    self.assertEqual(result.returncode, 0)

            result = subprocess.run(
                [command, "view", "tr", "--help"],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertIn("-p", result.stdout)
            self.assertIn("--path", result.stdout)
            self.assertIn("-n", result.stdout)
            self.assertIn("--note", result.stdout)

            default_report = Path(tmp) / "report-opencode-common_session.html"
            result = subprocess.run(
                [
                    command,
                    "view",
                    "tr",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-o",
                ],
                cwd=tmp,
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            self.assertIn("<!doctype html>", default_report.read_text(encoding="utf-8"))

            default_report_json = Path(tmp) / "report-opencode-common_session.json"
            result = subprocess.run(
                [
                    command,
                    "view",
                    "tr",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-f",
                    "json",
                    "-o",
                ],
                cwd=tmp,
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            subprocess.run(
                [sys.executable, "-m", "json.tool", str(default_report_json)],
                check=True,
                text=True,
                capture_output=True,
            )

            default_export = Path(tmp) / "trajectory-opencode-session.json"
            result = subprocess.run(
                [
                    command,
                    "export",
                    "tr",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-o",
                ],
                cwd=tmp,
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            subprocess.run(
                [sys.executable, "-m", "json.tool", str(default_export)],
                check=True,
                text=True,
                capture_output=True,
            )


if __name__ == "__main__":
    unittest.main()
