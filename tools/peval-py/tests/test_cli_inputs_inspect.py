from __future__ import annotations

from cli_inputs_support import *

class PevalPyCliInputInspectTests(unittest.TestCase):
    def test_cli_view_inspect_is_default_fixed_digest(self) -> None:
        from peval_py.cli import main

        stdout = io.StringIO()
        with contextlib.redirect_stdout(stdout):
            result = main(
                [
                    "view",
                    "tr",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "--top",
                    "1",
                    "--preview-chars",
                    "12",
                ]
            )

        self.assertEqual(result, 0)
        payload = json.loads(stdout.getvalue())
        self.assertEqual(payload["inspect_schema_version"], 2)
        self.assertNotIn("mode", payload)
        self.assertNotIn("on", payload)
        self.assertNotIn("selection", payload)
        source = payload["sources"][0]
        self.assertEqual(source["session_id"], "common_session")
        self.assertEqual(source["agent"], "opencode")
        self.assertEqual(source["total_tokens"], 15)
        self.assertEqual(source["active_duration"], 0.1)
        self.assertEqual(source["total_input_tokens"], 7)
        self.assertEqual(source["total_output_tokens"], 8)
        self.assertEqual(source["total_tool_calls"], 1)
        self.assertEqual(source["total_tool_errors"], 0)
        self.assertEqual(source["total_turns"], 2)
        self.assertNotIn("status", source)
        self.assertEqual(len(source["steps"]["head"]), 2)
        self.assertEqual(len(source["steps"]["tail"]), 2)
        self.assertIn("[truncated]", source["steps"]["tail"][1]["message_preview"])
        self.assertEqual(source["steps"]["top_tokens"][0]["step_id"], 3)
        self.assertEqual(source["tools"]["top_durations"][0]["duration"], 0.1)
        self.assertEqual(source["tools"]["duration_distribution"]["sum"], 0.1)

    def test_cli_view_inspect_output_paths_are_reported(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            with contextlib.chdir(root):
                stdout = io.StringIO()
                with contextlib.redirect_stdout(stdout):
                    result = main(
                        [
                            "view",
                            "tr",
                            "-a",
                            "opencode",
                            "-p",
                            str(FIXTURES / "common_session.jsonl"),
                            "-o",
                        ]
                    )
                self.assertEqual(result, 0)
                default_path = written_report_path(stdout.getvalue(), root)
                self.assertRegex(
                    default_path.name,
                    r"^inspect-\d{8}-\d{6}-\d{6}\.json$",
                )
                payload = json.loads(default_path.read_text(encoding="utf-8"))
                self.assertEqual(payload["inspect_schema_version"], 2)

            explicit = root / "explicit-inspect.json"
            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                result = main(
                    [
                        "view",
                        "tr",
                        "-a",
                        "opencode",
                        "-p",
                        str(FIXTURES / "common_session.jsonl"),
                        "-o",
                        str(explicit),
                    ]
                )
            self.assertEqual(result, 0)
            self.assertEqual(stdout.getvalue(), f"wrote report: {explicit}\n")
            self.assertTrue(explicit.exists())

    def test_cli_view_inspect_rejects_html_removed_flags_and_raw_only_overrides(self) -> None:
        from peval_py.cli import main

        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            html_result = main(
                [
                    "view",
                    "tr",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-f",
                    "html",
                ]
            )
        self.assertNotEqual(html_result, 0)
        self.assertIn("supports only JSON", stderr.getvalue())

        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr), self.assertRaises(SystemExit) as cm:
            main(
                [
                    "view",
                    "tr",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "--on",
                    "all",
                ]
            )
        self.assertNotEqual(cm.exception.code, 0)
        self.assertIn("unrecognized arguments: --on all", stderr.getvalue())

        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr), self.assertRaises(SystemExit) as cm:
            main(
                [
                    "view",
                    "tr",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "--errors-only",
                ]
            )
        self.assertNotEqual(cm.exception.code, 0)
        self.assertIn("unrecognized arguments: --errors-only", stderr.getvalue())

        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            raw_result = main(
                [
                    "view",
                    "tr",
                    "-m",
                    "raw",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "--head",
                    "0",
                    "--tool-call",
                    "tool-1",
                ]
            )
        self.assertNotEqual(raw_result, 0)
        self.assertIn("inspect-only option(s)", stderr.getvalue())
        self.assertIn("--tool-call", stderr.getvalue())

        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            raw_only_result = main(
                [
                    "view",
                    "tr",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "--agent-name",
                    "agent-a",
                    "--model",
                    "model-a",
                    "--no-redact",
                ]
            )
        self.assertNotEqual(raw_only_result, 0)
        self.assertIn("raw-only option(s) require -m raw", stderr.getvalue())
        self.assertIn("--agent-name", stderr.getvalue())
        self.assertIn("--model", stderr.getvalue())
        self.assertIn("--no-redact", stderr.getvalue())

    def test_cli_view_inspect_reads_report_and_meta_json_directly(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            report_path = root / "report.json"
            report_path.write_text(
                json.dumps(
                    {
                        "trajectory": [
                            {
                                "schema_version": "ATIF-v1.7",
                                "session_id": "direct-session",
                                "agent": {
                                    "name": "direct-agent",
                                    "model_name": "direct-model",
                                },
                                "steps": [
                                    {
                                        "step_id": 1,
                                        "source": "user",
                                        "message": "hello",
                                        "metrics": {
                                            "prompt_tokens": 3,
                                            "completion_tokens": 2,
                                            "cached_tokens": 1,
                                        },
                                    },
                                    {
                                        "step_id": 2,
                                        "source": "agent",
                                        "message": "done",
                                        "tool_calls": [
                                            {
                                                "tool_call_id": "call-1",
                                                "function_name": "shell",
                                                "arguments": {"cmd": "false"},
                                            }
                                        ],
                                        "observation": {
                                            "results": [
                                                {
                                                    "source_call_id": "call-1",
                                                    "content": "command failed",
                                                }
                                            ]
                                        },
                                        "metrics": {
                                            "prompt_tokens": 9,
                                            "completion_tokens": 4,
                                            "cached_tokens": 0,
                                        },
                                    },
                                ],
                                "final_metrics": {
                                    "total_prompt_tokens": 12,
                                    "total_completion_tokens": 6,
                                    "total_cached_tokens": 1,
                                    "extra": {
                                        "total_turns": 1,
                                        "total_tool_calls": 1,
                                        "total_tool_errors": 1,
                                    },
                                },
                            }
                        ],
                        "trajectory_meta": [
                            {
                                "trial_key": "direct-trial",
                                "adapter": "atif",
                                "status": "failed",
                                "score": 0,
                                "duration_ms": 4000,
                                "wall_duration_ms": 5000,
                                "warnings": [],
                                "steps": [
                                    {
                                        "step_id": 1,
                                        "duration_ms": 1000,
                                        "tool_calls": [],
                                        "observations": [],
                                    },
                                    {
                                        "step_id": 2,
                                        "duration_ms": 3000,
                                        "tool_error": True,
                                        "tool_calls": [
                                            {
                                                "tool_call_id": "call-1",
                                                "status": "error",
                                                "title": "shell",
                                                "execution_duration_ms": 2500,
                                            }
                                        ],
                                        "observations": [],
                                    },
                                ],
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )
            meta_path = root / "trajectory_meta.json"
            meta_path.write_text(
                json.dumps(
                    {
                        "trial_key": "meta-only",
                        "adapter": "snapshot",
                        "status": "passed",
                        "duration_ms": 5,
                        "warnings": ["meta warning"],
                        "steps": [],
                    }
                ),
                encoding="utf-8",
            )

            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                result = main(
                    [
                        "view",
                        "tr",
                        "-p",
                        str(report_path),
                        "-p",
                        str(meta_path),
                        "--step",
                        "2",
                        "--tool-call",
                        "call-1",
                    ]
                )
            tool_only_stdout = io.StringIO()
            with contextlib.redirect_stdout(tool_only_stdout):
                tool_only_result = main(
                    [
                        "view",
                        "tr",
                        "-p",
                        str(report_path),
                        "--tool-call",
                        "call-1",
                    ]
                )

        self.assertEqual(result, 0)
        payload = json.loads(stdout.getvalue())
        self.assertEqual(payload["inspect_schema_version"], 2)
        first = payload["sources"][0]
        self.assertEqual(first["session_id"], "direct-session")
        self.assertEqual(first["agent"], "direct-agent")
        self.assertEqual(first["model"], "direct-model")
        self.assertEqual(first["status"], "failed")
        self.assertEqual(first["score"], 0)
        self.assertEqual(first["active_duration"], 4)
        self.assertEqual(first["total_tokens"], 18)
        self.assertEqual(first["total_input_tokens"], 12)
        self.assertEqual(first["total_output_tokens"], 6)
        self.assertEqual(first["total_cached_tokens"], 1)
        self.assertEqual(first["total_tool_calls"], 1)
        self.assertEqual(first["total_tool_errors"], 1)
        self.assertEqual(first["total_turns"], 1)
        self.assertEqual(first["steps"]["top_durations"][0], {"step_id": 2, "duration": 3})
        self.assertEqual(
            first["steps"]["top_tokens"][0],
            {"step_id": 2, "input": 9, "output": 4, "cached": 0},
        )
        self.assertEqual(first["steps"]["duration_distribution"]["sum"], 4)
        self.assertEqual(
            first["tools"]["errors"],
            [{"step_id": 2, "tool_call_id": "call-1", "tool_name": "shell"}],
        )
        self.assertEqual(first["tools"]["top_durations"][0]["duration"], 2.5)
        self.assertEqual(first["tools"]["duration_distribution"]["sum"], 2.5)
        self.assertEqual(first["selected_steps"][0]["step_id"], 2)
        self.assertEqual(first["selected_steps"][0]["tool_calls"][0]["tool_call_id"], "call-1")
        self.assertEqual(
            first["selected_steps"][0]["tool_results"][0]["content_preview"],
            "command failed",
        )
        self.assertEqual(first["selected_tool_calls"][0]["tool_call_id"], "call-1")
        self.assertEqual(
            first["selected_tool_calls"][0]["tool_result"]["content_preview"],
            "command failed",
        )
        second = payload["sources"][1]
        self.assertEqual(second["session_id"], "meta-only")
        self.assertEqual(second["agent"], "snapshot")
        self.assertNotIn("status", second)

        self.assertEqual(tool_only_result, 0)
        tool_only = json.loads(tool_only_stdout.getvalue())["sources"][0]
        self.assertNotIn("selected_steps", tool_only)
        self.assertEqual(tool_only["selected_tool_calls"][0]["tool_call_id"], "call-1")
        self.assertEqual(
            tool_only["selected_tool_calls"][0]["tool_result"]["content_preview"],
            "command failed",
        )
