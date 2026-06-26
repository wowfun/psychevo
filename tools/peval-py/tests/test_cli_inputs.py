from __future__ import annotations

import os

from peval_py_test_support import *


def write_cli_cached_analysis(
    root: Path,
    *,
    eval_slug: str = "default",
    agent_id: str = "agent-a",
    session_id: str = "common_session",
    cell_key: str = "session_t001",
    summary: str = "Root-selected cached analysis.",
) -> Path:
    path = root / "runs" / eval_slug / agent_id / session_id / cell_key / "analysis.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(
            {
                "summary": summary,
                "findings": [{"title": "Root-selected finding."}],
                "metrics": {"review_turns": 2},
            }
        ),
        encoding="utf-8",
    )
    return path


def write_cli_cached_markdown(
    root: Path,
    *,
    eval_slug: str = "default",
    agent_id: str = "agent-a",
    session_id: str = "common_session",
    cell_key: str = "session_t001",
    markdown: str = "## Root selected analysis\n\nCached markdown body.",
) -> Path:
    path = root / "runs" / eval_slug / agent_id / session_id / cell_key / "analysis.md"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(markdown, encoding="utf-8")
    return path


def write_peval_workspace(root: Path) -> None:
    root.mkdir(parents=True, exist_ok=True)
    (root / "peval-py.toml").write_text(
        'analysis_eval_slug = "default"\n',
        encoding="utf-8",
    )


def written_report_path(stdout: str, cwd: Path) -> Path:
    match = re.fullmatch(r"wrote report: (.+)\n", stdout)
    if not match:
        raise AssertionError(f"missing written report path in stdout: {stdout!r}")
    path = Path(match.group(1))
    return path if path.is_absolute() else cwd / path


def write_trial_cell_artifacts(
    cell_dir: Path,
    *,
    session_id: str = "artifact-session",
    trial_key: str = "session_t001",
    agent_id: str = "psychevo",
    adapter: str = "psychevo",
    tool_error: bool = False,
) -> None:
    agent_dir = cell_dir / "agent"
    agent_dir.mkdir(parents=True, exist_ok=True)
    trajectory = {
        "schema_version": "ATIF-v1.7",
        "trajectory_id": trial_key,
        "session_id": session_id,
        "agent": {"name": agent_id, "version": "test"},
        "steps": [
            {
                "step_id": 1,
                "source": "user",
                "message": "direct artifact prompt",
            },
            {
                "step_id": 2,
                "source": "assistant",
                "message": "direct artifact response",
                **(
                    {
                        "tool_calls": [
                            {
                                "tool_call_id": "call_error",
                                "function_name": "exec_command",
                                "arguments": {"cmd": "false"},
                            }
                        ],
                        "observation": {
                            "results": [
                                {
                                    "tool_call_id": "call_error",
                                    "content": "command failed",
                                }
                            ]
                        },
                    }
                    if tool_error
                    else {}
                ),
            },
        ],
        "final_metrics": {
            "total_steps": 2,
            "extra": {
                "total_turns": 1,
                "total_tool_calls": 1 if tool_error else 0,
                "total_tool_errors": 1 if tool_error else 0,
            },
        },
    }
    meta = {
        "trial_key": trial_key,
        "adapter": adapter,
        "started_at_ms": 1000,
        "finished_at_ms": 1200,
        "wall_duration_ms": 200,
        "duration_ms": 200,
        "status": "passed",
        "score": None,
        "score_message": "",
        "warnings": [],
        "total_events": 2,
        "unmapped_events": 0,
        "prompt_unavailable": False,
        "steps": [
            {
                "step_id": 1,
                "tool_calls": [],
                "observations": [],
                "tool_error": False,
                "truncated": False,
            },
            {
                "step_id": 2,
                "tool_calls": [
                    {
                        "tool_call_id": "call_error",
                        "status": "error",
                        "title": "exec_command",
                    }
                ]
                if tool_error
                else [],
                "observations": [
                    {
                        "tool_call_id": "call_error",
                        "status": "error",
                    }
                ]
                if tool_error
                else [],
                "tool_error": tool_error,
                "truncated": False,
            },
        ],
    }
    (agent_dir / "trajectory.json").write_text(
        json.dumps(trajectory),
        encoding="utf-8",
    )
    (agent_dir / "trajectory_meta.json").write_text(
        json.dumps(meta),
        encoding="utf-8",
    )


class PevalPyCliInputTests(unittest.TestCase):
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

    def test_cli_uses_custom_path_adapter_and_rejects_db_when_path_only(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            first = tmp_path / "first.txt"
            second = tmp_path / "second.txt"
            first.write_text("first prompt\n", encoding="utf-8")
            second.write_text("second prompt\n", encoding="utf-8")
            config_path = tmp_path / "custom.toml"
            config_path.write_text(
                """
[defaults]
adapter = "custom"

[adapters.custom]
label_prefix = "configured"
""",
                encoding="utf-8",
            )
            export_out = tmp_path / "trajectory.json"
            view_out = tmp_path / "report.json"
            entry = FakeEntryPoint("custom", CustomPathAdapter)
            with patch(
                "peval_py.adapters.entry_points",
                return_value=FakeEntryPoints([entry]),
            ):
                result = main(
                    [
                        "export",
                        "tr",
                        "-c",
                        str(config_path),
                        "-p",
                        str(first),
                        "-o",
                        str(export_out),
                    ]
                )
                self.assertEqual(result, 0)
                payload = json.loads(export_out.read_text(encoding="utf-8"))
                self.assertEqual(payload["session_id"], "configured:first")
                self.assertEqual(payload["steps"][0]["message"], "first prompt")

                result = main(
                    [
                        "view",
                        "tr",
                        "-m",
                        "raw",
                        "-c",
                        str(config_path),
                        "-p",
                        str(first),
                        "-p",
                        str(second),
                        "-f",
                        "json",
                        "-o",
                        str(view_out),
                    ]
                )
                self.assertEqual(result, 0)
                payload = json.loads(view_out.read_text(encoding="utf-8"))
                self.assertEqual(
                    [item["session_id"] for item in payload["trajectory"]],
                    ["configured:first", "configured:second"],
                )

                db_path = tmp_path / "state.db"
                create_messages_db(db_path)
                stderr = io.StringIO()
                with contextlib.redirect_stderr(stderr):
                    result = main(
                        [
                            "view",
                            "tr",
                        "-m",
                        "raw",
                            "-c",
                            str(config_path),
                            "-d",
                            str(db_path),
                            "-s",
                            "db-a",
                            "-f",
                            "json",
                            "-o",
                            str(tmp_path / "db-report.json"),
                        ]
                )
                self.assertNotEqual(result, 0)
                self.assertIn("does not support DB input", stderr.getvalue())


    def test_cli_adapter_selectors_apply_per_path_input(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            custom_path = tmp_path / "custom.txt"
            custom_path.write_text("custom prompt\n", encoding="utf-8")
            config_path = tmp_path / "custom.toml"
            config_path.write_text(
                """
[adapters.custom]
label_prefix = "selected"
""",
                encoding="utf-8",
            )
            out_path = tmp_path / "report.json"
            entry = FakeEntryPoint("custom", CustomPathAdapter)
            with patch(
                "peval_py.adapters.entry_points",
                return_value=FakeEntryPoints([entry]),
            ):
                result = main(
                    [
                        "view",
                        "tr",
                        "-m",
                        "raw",
                        "-c",
                        str(config_path),
                        "-a",
                        "opencode",
                        "-a",
                        "p2=custom",
                        "-p",
                        str(FIXTURES / "common_session.jsonl"),
                        "-p",
                        str(custom_path),
                        "-f",
                        "json",
                        "-o",
                        str(out_path),
                    ]
                )
                self.assertEqual(result, 0)
                payload = json.loads(out_path.read_text(encoding="utf-8"))
                self.assertEqual(
                    [item["agent"]["name"] for item in payload["trajectory"]],
                    ["opencode", "custom"],
                )
                self.assertEqual(
                    [item["adapter"] for item in payload["trajectory_meta"]],
                    ["opencode", "custom"],
                )
                self.assertNotIn("comparison", payload)
                self.assertEqual(
                    payload["trajectory"][1]["session_id"],
                    "selected:custom",
                )

                for argv, message in [
                    (
                        [
                            "view",
                            "tr",
                        "-m",
                        "raw",
                            "-a",
                            "p1=custom",
                            "-a",
                            "p1=opencode",
                            "-p",
                            str(custom_path),
                        ],
                        "duplicate adapter selector: p1",
                    ),
                    (
                        [
                            "view",
                            "tr",
                        "-m",
                        "raw",
                            "-a",
                            "p2=custom",
                            "-p",
                            str(custom_path),
                        ],
                        "no matching --path input",
                    ),
                    (
                        [
                            "view",
                            "tr",
                        "-m",
                        "raw",
                            "-a",
                            "p1=missing",
                            "-p",
                            str(custom_path),
                        ],
                        "available adapters",
                    ),
                ]:
                    with self.subTest(message=message):
                        stderr = io.StringIO()
                        with contextlib.redirect_stderr(stderr):
                            result = main(argv)
                        self.assertNotEqual(result, 0)
                        self.assertIn(message, stderr.getvalue())

    def test_cli_infers_adapter_from_path_tokens_only_when_not_explicit(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            hermes_dir = tmp_path / ".hermes"
            psychevo_dir = tmp_path / ".psychevo"
            hermes_dir.mkdir()
            psychevo_dir.mkdir()
            hermes_path = hermes_dir / "common_session.jsonl"
            psychevo_path = psychevo_dir / "psychevo_session.jsonl"
            shutil.copy(FIXTURES / "common_session.jsonl", hermes_path)
            shutil.copy(FIXTURES / "psychevo_session.jsonl", psychevo_path)

            inferred_path_report = tmp_path / "inferred-path.json"
            result = main(
                [
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "-p",
                    str(hermes_path),
                    "-p",
                    str(psychevo_path),
                    "-f",
                    "json",
                    "-o",
                    str(inferred_path_report),
                ]
            )
            self.assertEqual(result, 0)
            payload = json.loads(inferred_path_report.read_text(encoding="utf-8"))
            self.assertEqual(
                [meta["adapter"] for meta in payload["trajectory_meta"]],
                ["hermes", "psychevo"],
            )

            explicit_report = tmp_path / "explicit.json"
            result = main(
                [
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "-a",
                    "opencode",
                    "-p",
                    str(hermes_path),
                    "-f",
                    "json",
                    "-o",
                    str(explicit_report),
                ]
            )
            self.assertEqual(result, 0)
            payload = json.loads(explicit_report.read_text(encoding="utf-8"))
            self.assertEqual(payload["trajectory_meta"][0]["adapter"], "opencode")

            create_hermes_db(hermes_dir / "state.db")
            create_messages_db(psychevo_dir / "state.db")
            inferred_db_report = tmp_path / "inferred-db.json"
            result = main(
                [
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "-d",
                    str(hermes_dir / "state.db"),
                    "-d",
                    str(psychevo_dir / "state.db"),
                    "-s",
                    "d1=hermes-old",
                    "-s",
                    "d2=db-a",
                    "-f",
                    "json",
                    "-o",
                    str(inferred_db_report),
                ]
            )
            self.assertEqual(result, 0)
            payload = json.loads(inferred_db_report.read_text(encoding="utf-8"))
            self.assertEqual(
                [meta["adapter"] for meta in payload["trajectory_meta"]],
                ["hermes", "psychevo"],
            )

            ambiguous = tmp_path / "hermes" / "opencode" / "common_session.jsonl"
            ambiguous.parent.mkdir(parents=True)
            shutil.copy(FIXTURES / "common_session.jsonl", ambiguous)
            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr):
                result = main(["view", "tr", "-p", str(ambiguous)])
            self.assertNotEqual(result, 0)
            self.assertIn("ambiguous adapter inference", stderr.getvalue())


    def test_cli_multi_db_keyed_sessions_and_mixed_sources(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            hermes_db = tmp_path / "hermes.db"
            opencode_db = tmp_path / "opencode.db"
            create_hermes_db(hermes_db)
            create_opencode_db(opencode_db)

            multi_db_out = tmp_path / "multi-db.json"
            result = main(
                [
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "-d",
                    str(hermes_db),
                    "-d",
                    str(opencode_db),
                    "-a",
                    "d1=hermes",
                    "-a",
                    "d2=opencode",
                    "-s",
                    "d1=hermes-old",
                    "-s",
                    "d2=ses-old",
                    "-f",
                    "json",
                    "-o",
                    str(multi_db_out),
                ]
            )
            self.assertEqual(result, 0)
            payload = json.loads(multi_db_out.read_text(encoding="utf-8"))
            self.assertEqual(
                [item["session_id"] for item in payload["trajectory"]],
                ["hermes-old", "ses-old"],
            )
            self.assertEqual(
                [item["adapter"] for item in payload["trajectory_meta"]],
                ["hermes", "opencode"],
            )
            self.assertNotIn("comparison", payload)

            mixed_out = tmp_path / "mixed.json"
            result = main(
                [
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-d",
                    str(opencode_db),
                    "-f",
                    "json",
                    "-o",
                    str(mixed_out),
                ]
            )
            self.assertEqual(result, 0)
            payload = json.loads(mixed_out.read_text(encoding="utf-8"))
            self.assertEqual(len(payload["trajectory"]), 2)
            self.assertEqual(
                [item["adapter"] for item in payload["trajectory_meta"]],
                ["opencode", "opencode"],
            )
            self.assertEqual(payload["trajectory"][1]["session_id"], "ses-latest")

            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr):
                result = main(
                    [
                        "view",
                        "tr",
                        "-m",
                        "raw",
                        "-d",
                        str(hermes_db),
                        "-d",
                        str(opencode_db),
                        "-a",
                        "d1=hermes",
                        "-a",
                        "d2=opencode",
                        "-s",
                        "ses-old",
                    ]
            )
            self.assertNotEqual(result, 0)
            self.assertIn("bare --session-id", stderr.getvalue())

    def test_cli_db_session_list_and_index_selection(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            hermes_dir = tmp_path / ".hermes"
            opencode_dir = tmp_path / ".opencode"
            hermes_dir.mkdir()
            opencode_dir.mkdir()
            hermes_db = hermes_dir / "state.db"
            opencode_db = opencode_dir / "opencode.db"
            create_hermes_db(hermes_db)
            create_opencode_db(opencode_db)
            conn = sqlite3.connect(hermes_db)
            conn.execute(
                """
                INSERT INTO sessions (id, title, started_at, ended_at)
                VALUES ('1', 'Numeric Hermes', 1.0, 2.0)
                """
            )
            conn.execute(
                """
                INSERT INTO messages (session_id, role, content, timestamp, active)
                VALUES ('1', 'user', 'numeric prompt', 1.5, 1)
                """
            )
            conn.commit()
            conn.close()

            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                result = main(["view", "tr", "-d", str(hermes_db), "--list"])
            self.assertEqual(result, 0)
            listing = stdout.getvalue()
            self.assertIn("#  session_id", listing)
            self.assertIn("1  hermes-latest", listing)
            self.assertIn("Latest Hermes", listing)

            index_report = tmp_path / "index.json"
            result = main(
                [
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "-d",
                    str(hermes_db),
                    "-s",
                    "#2",
                    "-f",
                    "json",
                    "-o",
                    str(index_report),
                ]
            )
            self.assertEqual(result, 0)
            payload = json.loads(index_report.read_text(encoding="utf-8"))
            self.assertEqual(payload["trajectory"][0]["session_id"], "hermes-old")

            id_first_report = tmp_path / "id-first.json"
            result = main(
                [
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "-d",
                    str(hermes_db),
                    "-s",
                    "1",
                    "-f",
                    "json",
                    "-o",
                    str(id_first_report),
                ]
            )
            self.assertEqual(result, 0)
            payload = json.loads(id_first_report.read_text(encoding="utf-8"))
            self.assertEqual(payload["trajectory"][0]["session_id"], "1")
            self.assertEqual(
                payload["trajectory"][0]["steps"][-1]["message"],
                "numeric prompt",
            )

            multi_report = tmp_path / "multi-index.json"
            result = main(
                [
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "-d",
                    str(hermes_db),
                    "-d",
                    str(opencode_db),
                    "-s",
                    "d1=#2",
                    "-s",
                    "d2=#2",
                    "-f",
                    "json",
                    "-o",
                    str(multi_report),
                ]
            )
            self.assertEqual(result, 0)
            payload = json.loads(multi_report.read_text(encoding="utf-8"))
            self.assertEqual(
                [item["session_id"] for item in payload["trajectory"]],
                ["hermes-old", "ses-old"],
            )

    def test_cli_db_session_interactive_selection(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            hermes_dir = tmp_path / ".hermes"
            hermes_dir.mkdir()
            hermes_db = hermes_dir / "state.db"
            create_hermes_db(hermes_db)

            interactive_report = tmp_path / "interactive.json"
            stdout = io.StringIO()
            with (
                contextlib.redirect_stdout(stdout),
                patch("peval_py.cli.sys.stdin.isatty", return_value=True),
                patch("builtins.input", return_value="1-2"),
            ):
                result = main(
                    [
                        "view",
                        "tr",
                        "-m",
                        "raw",
                        "-d",
                        str(hermes_db),
                        "-li",
                        "-f",
                        "json",
                        "-o",
                        str(interactive_report),
                    ]
                )
            self.assertEqual(result, 0)
            self.assertIn("hermes-latest", stdout.getvalue())
            payload = json.loads(interactive_report.read_text(encoding="utf-8"))
            self.assertEqual(
                [item["session_id"] for item in payload["trajectory"]],
                ["hermes-latest", "hermes-old"],
            )

            all_report = tmp_path / "interactive-all.json"
            with (
                contextlib.redirect_stdout(io.StringIO()),
                patch("peval_py.cli.sys.stdin.isatty", return_value=True),
                patch("builtins.input", return_value="all"),
            ):
                result = main(
                    [
                        "view",
                        "tr",
                        "-m",
                        "raw",
                        "-d",
                        str(hermes_db),
                        "-li",
                        "-f",
                        "json",
                        "-o",
                        str(all_report),
                    ]
                )
            self.assertEqual(result, 0)
            payload = json.loads(all_report.read_text(encoding="utf-8"))
            self.assertEqual(len(payload["trajectory"]), 2)

            blank_report = tmp_path / "blank.json"
            with (
                contextlib.redirect_stdout(io.StringIO()),
                patch("peval_py.cli.sys.stdin.isatty", return_value=True),
                patch("builtins.input", return_value=""),
            ):
                result = main(
                    [
                        "view",
                        "tr",
                        "-m",
                        "raw",
                        "-d",
                        str(hermes_db),
                        "--list-interactive",
                        "-f",
                        "json",
                        "-o",
                        str(blank_report),
                    ]
                )
            self.assertEqual(result, 0)
            self.assertFalse(blank_report.exists())

            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr):
                result = main(
                    [
                        "view",
                        "tr",
                        "-m",
                        "raw",
                        "-d",
                        str(hermes_db),
                        "--list-interactive",
                    ]
                )
            self.assertNotEqual(result, 0)
            self.assertIn("requires an interactive terminal", stderr.getvalue())


    def test_cli_view_accepts_exported_atif_json_path(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            source = convert_records(
                read_jsonl(str(FIXTURES / "common_session.jsonl")),
                ToolConfig(adapter="opencode"),
            )
            atif_path = tmp_path / "trajectory.json"
            out_path = tmp_path / "report.json"
            atif_path.write_text(
                json.dumps(source.trajectory, ensure_ascii=False),
                encoding="utf-8",
            )

            result = main(
                [
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "-p",
                    str(atif_path),
                    "-f",
                    "json",
                    "-o",
                    str(out_path),
                ]
            )
            self.assertEqual(result, 0)
            payload = json.loads(out_path.read_text(encoding="utf-8"))
            self.assertEqual(payload["trajectory"][0], source.trajectory)
            self.assertEqual(payload["trajectory_meta"][0]["adapter"], "atif")

            missing_adapter_config = tmp_path / "missing.toml"
            missing_adapter_config.write_text(
                "[defaults]\nadapter = \"missing\"\n",
                encoding="utf-8",
            )
            result = main(
                [
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "-c",
                    str(missing_adapter_config),
                    "-p",
                    str(atif_path),
                    "-f",
                    "json",
                    "-o",
                    str(out_path),
                ]
            )
            self.assertEqual(result, 0)
            payload = json.loads(out_path.read_text(encoding="utf-8"))
            self.assertEqual(payload["trajectory_meta"][0]["adapter"], "atif")

            result = main(
                [
                    "export",
                    "tr",
                    "-c",
                    str(missing_adapter_config),
                    "-p",
                    str(atif_path),
                    "-o",
                    str(tmp_path / "exported-again.json"),
                ]
            )
            self.assertEqual(result, 0)


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
                        "-m",
                        "raw",
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
            self.assertNotIn("comparison", payload)
            self.assertEqual(payload["annotations"]["report_notes"][0]["markdown"], "DB report")
            self.assertEqual(payload["annotations"]["notes"][0]["markdown"], "DB B")

            bad_note = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "peval_py.cli",
                    "view",
                    "tr",
                        "-m",
                        "raw",
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

    def test_cli_db_default_token_uses_configured_adapter_path(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            db_path = root / "state.db"
            create_messages_db(db_path)
            config_path = root / "peval-py.toml"
            config_path.write_text(
                """
[adapters.psychevo]
default_db_path = "state.db"
""",
                encoding="utf-8",
            )
            out_path = root / "default-db.json"
            result = main(
                [
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "-c",
                    str(config_path),
                    "-d",
                    "@psychevo",
                    "-s",
                    "db-a",
                    "-f",
                    "json",
                    "-o",
                    str(out_path),
                ]
            )
            self.assertEqual(result, 0)
            payload = json.loads(out_path.read_text(encoding="utf-8"))
            self.assertEqual(payload["trajectory"][0]["session_id"], "db-a")
            self.assertEqual(payload["trajectory_meta"][0]["adapter"], "psychevo")

            for argv, message in [
                (
                    [
                        "view",
                        "tr",
                        "-m",
                        "raw",
                        "-c",
                        str(config_path),
                        "-d",
                        "@psychevo",
                        "-a",
                        "d1=opencode",
                    ],
                    "uses @psychevo but adapter selector d1=opencode",
                ),
                (
                    ["view", "tr", "-c", str(config_path), "-d", "@missing"],
                    "no default_db_path configured for adapter: missing",
                ),
            ]:
                with self.subTest(message=message):
                    stderr = io.StringIO()
                    with contextlib.redirect_stderr(stderr):
                        result = main(argv)
                    self.assertNotEqual(result, 0)
                    self.assertIn(message, stderr.getvalue())

    def test_cli_root_option_loads_workspace_config_for_view_export_and_list(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            workspace = root / "workspace"
            outside = root / "outside"
            workspace.mkdir()
            outside.mkdir()
            db_path = workspace / "state.db"
            create_messages_db(db_path)
            (workspace / "peval-py.toml").write_text(
                """
[adapters.psychevo]
default_db_path = "state.db"
""",
                encoding="utf-8",
            )

            view_out = root / "root-view.json"
            export_out = root / "root-trajectory.json"
            with contextlib.chdir(outside):
                result = main(
                    [
                        "view",
                        "tr",
                        "-m",
                        "raw",
                        "-r",
                        str(workspace),
                        "-d",
                        "@psychevo",
                        "-s",
                        "db-a",
                        "-f",
                        "json",
                        "-o",
                        str(view_out),
                    ]
                )
                self.assertEqual(result, 0)

                result = main(
                    [
                        "export",
                        "tr",
                        "-r",
                        str(workspace),
                        "-d",
                        "@psychevo",
                        "-s",
                        "db-a",
                        "-o",
                        str(export_out),
                    ]
                )
                self.assertEqual(result, 0)

                stdout = io.StringIO()
                with contextlib.redirect_stdout(stdout):
                    result = main(
                        [
                            "view",
                            "tr",
                        "-m",
                        "raw",
                            "-r",
                            str(workspace),
                            "-d",
                            "@psychevo",
                            "--list",
                        ]
                    )
                self.assertEqual(result, 0)

            view_payload = json.loads(view_out.read_text(encoding="utf-8"))
            self.assertEqual(view_payload["trajectory"][0]["session_id"], "db-a")
            self.assertEqual(view_payload["trajectory_meta"][0]["adapter"], "psychevo")
            export_payload = json.loads(export_out.read_text(encoding="utf-8"))
            self.assertEqual(export_payload["session_id"], "db-a")
            self.assertEqual(export_payload["agent"]["name"], "psychevo")
            self.assertIn("db-a", stdout.getvalue())
            self.assertIn("DB A", stdout.getvalue())

    def test_cli_root_option_reads_saved_workspace_state_snapshots(self) -> None:
        from peval_py.cli import main
        from peval_py.state import open_workspace_state

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            workspace = root / "workspace"
            outside = root / "outside"
            source_dir = root / "sources"
            workspace.mkdir()
            outside.mkdir()
            source_dir.mkdir()
            psychevo_db = source_dir / "psychevo.db"
            create_messages_db(psychevo_db)
            (workspace / "peval-py.toml").write_text(
                'state_db = "state.db"\n',
                encoding="utf-8",
            )
            store = open_workspace_state(str(workspace))
            config = load_config(None, workspace_root=str(workspace))
            keys = store.import_loaded_sources(
                LoadedInputs(
                    sessions=[
                        LoadedSession(
                            records=None,
                            input_label="psychevo.db:db-a",
                            adapter_id="psychevo",
                            input_path=str(psychevo_db),
                            db_path=str(psychevo_db),
                            session_hint="db-a",
                            source_kind="db",
                        ),
                        LoadedSession(
                            records=None,
                            input_label="psychevo.db:db-b",
                            adapter_id="psychevo",
                            input_path=str(psychevo_db),
                            db_path=str(psychevo_db),
                            session_hint="db-b",
                            source_kind="db",
                        ),
                    ],
                    notes=[],
                ),
                config,
            )
            snapshot_keys = store.ingest_report_snapshot(
                {
                    "schema_version": 19,
                    "trajectory": [
                        {
                            "schema_version": "ATIF-v1.7",
                            "trajectory_id": "trial-unique",
                            "session_id": "snap-1",
                            "agent": {"name": "snapshot", "version": "0.1.0"},
                            "steps": [
                                {
                                    "step_id": 1,
                                    "source": "user",
                                    "message": "saved snapshot prompt",
                                }
                            ],
                            "final_metrics": {"total_steps": 1},
                        }
                    ],
                    "trajectory_meta": [
                        {
                            "trial_key": "trial-unique",
                            "adapter": "snapshot",
                            "started_at_ms": 1000,
                            "finished_at_ms": 1000,
                            "wall_duration_ms": 0,
                            "duration_ms": 0,
                            "status": "passed",
                            "score": None,
                            "score_message": "snapshot",
                            "warnings": [],
                            "total_events": 1,
                            "unmapped_events": 0,
                            "prompt_unavailable": False,
                            "steps": [],
                        }
                    ],
                },
                "snapshot-report.json",
                config,
                adapter="snapshot",
            )
            store.set_source_alias(keys[0], "Saved DB A")
            store.set_source_active(keys[1], False)
            store.set_source_active(snapshot_keys[0], False)
            sources = store.source_payload()
            store.close()
            psychevo_db.unlink()
            workspace_db = workspace / "state.db"
            active_view_out = root / "active-saved-view.json"
            archived_view_out = root / "archived-saved-view.json"
            key_view_out = root / "key-saved-view.json"
            session_view_out = root / "session-saved-view.json"
            trial_view_out = root / "trial-saved-view.json"
            export_out = root / "saved-export.json"
            default_export_out = root / "saved-default-export.json"
            active_source = next(item for item in sources if item["source_key"] == keys[0])
            archived_source = next(item for item in sources if item["source_key"] == keys[1])
            trial_source = next(item for item in sources if item["source_key"] == snapshot_keys[0])

            with contextlib.chdir(outside):
                stdout = io.StringIO()
                with contextlib.redirect_stdout(stdout):
                    result = main(
                        [
                            "view",
                            "tr",
                        "-m",
                        "raw",
                            "-r",
                            str(workspace),
                            "-d",
                            str(workspace_db),
                            "--list",
                        ]
                    )
                self.assertEqual(result, 0)
                listing = stdout.getvalue()
                self.assertIn("source_key", listing)
                self.assertIn("trial_key", listing)
                self.assertIn(keys[0], listing)
                self.assertIn("Saved DB A", listing)
                self.assertIn("no", listing)

                result = main(
                    [
                        "view",
                        "tr",
                        "-m",
                        "raw",
                        "-r",
                        str(workspace),
                        "-d",
                        str(workspace_db),
                        "-f",
                        "json",
                        "-o",
                        str(active_view_out),
                    ]
                )
                self.assertEqual(result, 0)

                result = main(
                    [
                        "view",
                        "tr",
                        "-m",
                        "raw",
                        "-r",
                        str(workspace),
                        "-d",
                        str(workspace_db),
                        "-s",
                        "db-b",
                        "-f",
                        "json",
                        "-o",
                        str(session_view_out),
                    ]
                )
                self.assertEqual(result, 0)

                result = main(
                    [
                        "view",
                        "tr",
                        "-m",
                        "raw",
                        "-r",
                        str(workspace),
                        "-d",
                        str(workspace_db),
                        "-s",
                        "#2",
                        "-f",
                        "json",
                        "-o",
                        str(archived_view_out),
                    ]
                )
                self.assertEqual(result, 0)

                result = main(
                    [
                        "view",
                        "tr",
                        "-m",
                        "raw",
                        "-r",
                        str(workspace),
                        "-d",
                        str(workspace_db),
                        "-s",
                        archived_source["source_key"],
                        "-f",
                        "json",
                        "-o",
                        str(key_view_out),
                    ]
                )
                self.assertEqual(result, 0)

                result = main(
                    [
                        "view",
                        "tr",
                        "-m",
                        "raw",
                        "-r",
                        str(workspace),
                        "-d",
                        str(workspace_db),
                        "-s",
                        trial_source["trial_key"],
                        "-f",
                        "json",
                        "-o",
                        str(trial_view_out),
                    ]
                )
                self.assertEqual(result, 0)

                result = main(
                    [
                        "export",
                        "tr",
                        "-r",
                        str(workspace),
                        "-d",
                        str(workspace_db),
                        "-s",
                        active_source["source_key"],
                        "-o",
                        str(export_out),
                    ]
                )
                self.assertEqual(result, 0)

                result = main(
                    [
                        "export",
                        "tr",
                        "-r",
                        str(workspace),
                        "-d",
                        str(workspace_db),
                        "-o",
                        str(default_export_out),
                    ]
                )
                self.assertEqual(result, 0)

            active_payload = json.loads(active_view_out.read_text(encoding="utf-8"))
            self.assertEqual([item["session_id"] for item in active_payload["trajectory"]], ["db-a"])
            self.assertEqual(
                active_payload["trajectory_meta"][0]["source_alias"],
                "Saved DB A",
            )
            archived_payload = json.loads(archived_view_out.read_text(encoding="utf-8"))
            self.assertEqual(archived_payload["trajectory"][0]["session_id"], "db-b")
            key_payload = json.loads(key_view_out.read_text(encoding="utf-8"))
            self.assertEqual(key_payload["trajectory"][0]["session_id"], "db-b")
            session_payload = json.loads(session_view_out.read_text(encoding="utf-8"))
            self.assertEqual(session_payload["trajectory"][0]["session_id"], "db-b")
            trial_payload = json.loads(trial_view_out.read_text(encoding="utf-8"))
            self.assertEqual(trial_payload["trajectory"][0]["session_id"], "snap-1")
            export_payload = json.loads(export_out.read_text(encoding="utf-8"))
            self.assertEqual(export_payload["session_id"], "db-a")
            default_export_payload = json.loads(default_export_out.read_text(encoding="utf-8"))
            self.assertEqual(default_export_payload["session_id"], "db-a")

    def test_cli_trial_cell_path_input_uses_workspace_source_metadata(self) -> None:
        from peval_py.cli import main
        from peval_py.state import open_workspace_state

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            workspace = root / "workspace"
            outside = root / "outside"
            source_db = root / "source.db"
            workspace.mkdir()
            outside.mkdir()
            create_messages_db(source_db)
            (workspace / "peval-py.toml").write_text(
                'state_db = "state.db"\nanalysis_eval_slug = "default"\n',
                encoding="utf-8",
            )
            store = open_workspace_state(str(workspace))
            config = load_config(None, workspace_root=str(workspace))
            keys = store.import_loaded_sources(
                LoadedInputs(
                    sessions=[
                        LoadedSession(
                            records=None,
                            input_label="source.db:db-a",
                            adapter_id="psychevo",
                            input_path=str(source_db),
                            db_path=str(source_db),
                            session_hint="db-a",
                            source_kind="db",
                        )
                    ],
                    notes=[],
                ),
                config,
            )
            store.set_source_alias(keys[0], "Cell Path Alias")
            source = next(
                item for item in store.source_payload() if item["source_key"] == keys[0]
            )
            cell_dir = workspace / str(source["artifact_dir"])
            write_cli_cached_analysis(
                workspace,
                agent_id=str(source.get("agent_name") or source.get("adapter")),
                session_id=str(source["session_id"]),
                cell_key=cell_dir.name,
                summary="Cell path cached analysis.",
            )
            store.close()
            source_db.unlink()
            inspect_out = root / "inspect.json"
            raw_out = root / "raw-report.json"
            export_out = root / "export.json"

            with contextlib.chdir(outside):
                result = main(
                    [
                        "view",
                        "tr",
                        "-p",
                        str(cell_dir),
                        "-o",
                        str(inspect_out),
                    ]
                )
                self.assertEqual(result, 0)

                result = main(
                    [
                        "view",
                        "tr",
                        "-m",
                        "raw",
                        "-p",
                        str(cell_dir),
                        "-f",
                        "json",
                        "-o",
                        str(raw_out),
                    ]
                )
                self.assertEqual(result, 0)

                result = main(
                    [
                        "export",
                        "tr",
                        "-p",
                        str(cell_dir),
                        "-o",
                        str(export_out),
                    ]
                )
                self.assertEqual(result, 0)

            inspect_payload = json.loads(inspect_out.read_text(encoding="utf-8"))
            source_payload = inspect_payload["sources"][0]
            self.assertEqual(source_payload["session_id"], "db-a")
            self.assertNotIn("source_alias", source_payload)
            self.assertNotIn("label", source_payload)
            self.assertNotIn("artifact_ref", source_payload)
            expected_ref = {
                "kind": "trial-cell-artifact",
                "path": os.path.relpath(cell_dir, outside),
                "workspace_relative_path": str(source["artifact_dir"]),
                "source_key": keys[0],
            }
            raw_payload = json.loads(raw_out.read_text(encoding="utf-8"))
            self.assertEqual(raw_payload["trajectory"][0]["session_id"], "db-a")
            self.assertEqual(
                raw_payload["trajectory_meta"][0]["source_alias"],
                "Cell Path Alias",
            )
            self.assertEqual(
                raw_payload["trajectory_meta"][0]["artifact_ref"],
                expected_ref,
            )
            self.assertEqual(
                raw_payload["annotations"]["analysis"][0]["summary"],
                "Cell path cached analysis.",
            )
            export_payload = json.loads(export_out.read_text(encoding="utf-8"))
            self.assertEqual(export_payload["session_id"], "db-a")
            self.assertNotIn("artifact_ref", export_payload)
            self.assertNotIn("trajectory_meta", export_payload)

    def test_cli_trial_cell_path_input_reads_unregistered_artifact_snapshot(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            workspace = root / "workspace"
            outside = root / "outside"
            outside.mkdir()
            write_peval_workspace(workspace)
            cell_dir = (
                workspace
                / "runs"
                / "default"
                / "psychevo"
                / "artifact-session"
                / "session_t001"
            )
            write_trial_cell_artifacts(
                cell_dir,
                session_id="artifact-session",
                trial_key="session_t001",
                tool_error=True,
            )
            inspect_out = root / "unregistered-inspect.json"
            raw_out = root / "unregistered-raw.json"
            export_out = root / "unregistered-export.json"

            with contextlib.chdir(outside):
                result = main(
                    [
                        "view",
                        "tr",
                        "-p",
                        str(cell_dir),
                        "-o",
                        str(inspect_out),
                    ]
                )
                self.assertEqual(result, 0)

                result = main(
                    [
                        "view",
                        "tr",
                        "-m",
                        "raw",
                        "-p",
                        str(cell_dir),
                        "-f",
                        "json",
                        "-o",
                        str(raw_out),
                    ]
                )
                self.assertEqual(result, 0)

                result = main(
                    [
                        "export",
                        "tr",
                        "-p",
                        str(cell_dir),
                        "-o",
                        str(export_out),
                    ]
                )
                self.assertEqual(result, 0)

            inspect_payload = json.loads(inspect_out.read_text(encoding="utf-8"))
            source_payload = inspect_payload["sources"][0]
            self.assertEqual(source_payload["session_id"], "artifact-session")
            self.assertNotIn("artifact_ref", source_payload)
            self.assertEqual(
                source_payload["tools"]["errors"],
                [
                    {
                        "step_id": 2,
                        "tool_call_id": "call_error",
                        "tool_name": "exec_command",
                    }
                ],
            )
            expected_ref = {
                "kind": "trial-cell-artifact",
                "path": os.path.relpath(cell_dir, outside),
                "workspace_relative_path": cell_dir.relative_to(workspace).as_posix(),
            }
            self.assertEqual(
                source_payload["total_tool_errors"],
                1,
            )
            raw_payload = json.loads(raw_out.read_text(encoding="utf-8"))
            self.assertEqual(raw_payload["trajectory"][0]["session_id"], "artifact-session")
            self.assertEqual(
                raw_payload["trajectory_meta"][0]["artifact_ref"],
                expected_ref,
            )
            export_payload = json.loads(export_out.read_text(encoding="utf-8"))
            self.assertEqual(export_payload["session_id"], "artifact-session")
            self.assertNotIn("artifact_ref", export_payload)
            self.assertNotIn("trajectory_meta", export_payload)

    def test_cli_trial_cell_path_root_conflict_is_clear(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            workspace = root / "workspace"
            other_workspace = root / "other-workspace"
            write_peval_workspace(workspace)
            write_peval_workspace(other_workspace)
            cell_dir = (
                workspace
                / "runs"
                / "default"
                / "psychevo"
                / "artifact-session"
                / "session_t001"
            )
            write_trial_cell_artifacts(cell_dir)

            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr):
                result = main(
                    [
                        "view",
                        "tr",
                        "-r",
                        str(other_workspace),
                        "-p",
                        str(cell_dir),
                    ]
                )

            self.assertNotEqual(result, 0)
            self.assertIn("conflicts with inferred workspace root", stderr.getvalue())
            self.assertIn(str(workspace), stderr.getvalue())
            self.assertIn(str(other_workspace), stderr.getvalue())

    def test_cli_trial_cell_path_malformed_directory_error_is_actionable(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            workspace = root / "workspace"
            write_peval_workspace(workspace)
            malformed = (
                workspace
                / "runs"
                / "default"
                / "psychevo"
                / "artifact-session"
                / "session_t001"
            )
            (malformed / "agent").mkdir(parents=True)

            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr):
                result = main(["view", "tr", "-p", str(malformed)])

            message = stderr.getvalue()
            self.assertNotEqual(result, 0)
            self.assertIn("Trial cell artifact directory", message)
            self.assertIn("agent/trajectory.json", message)
            self.assertIn("agent/trajectory_meta.json", message)
            self.assertNotIn("Psychevo DB not found", message)

    def test_cli_workspace_state_db_saved_snapshot_errors_are_clear(self) -> None:
        from peval_py.cli import main
        from peval_py.state import open_workspace_state

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            workspace = root / "workspace"
            source_db = root / "source.db"
            workspace.mkdir()
            create_messages_db(source_db)
            (workspace / "peval-py.toml").write_text(
                'state_db = "state.db"\n',
                encoding="utf-8",
            )
            store = open_workspace_state(str(workspace))
            config = load_config(None, workspace_root=str(workspace))
            store.import_loaded_sources(
                LoadedInputs(
                    sessions=[
                        LoadedSession(
                            records=None,
                            input_label="source.db:db-a",
                            adapter_id="psychevo",
                            input_path=str(source_db),
                            db_path=str(source_db),
                            session_hint="db-a",
                            source_kind="db",
                        ),
                        LoadedSession(
                            records=None,
                            input_label="source.db:db-b",
                            adapter_id="psychevo",
                            input_path=str(source_db),
                            db_path=str(source_db),
                            session_hint="db-b",
                            source_kind="db",
                        ),
                    ],
                    notes=[],
                ),
                config,
            )
            store.close()
            workspace_db = workspace / "state.db"

            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr):
                result = main(
                    [
                        "export",
                        "tr",
                        "-r",
                        str(workspace),
                        "-d",
                        str(workspace_db),
                    ]
                )
            self.assertNotEqual(result, 0)
            self.assertIn("exactly one active saved source", stderr.getvalue())
            self.assertIn("explicit -s selector", stderr.getvalue())

            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr):
                result = main(
                    [
                        "view",
                        "tr",
                        "-m",
                        "raw",
                        "-d",
                        str(workspace_db),
                        "--list",
                    ]
                )
            self.assertNotEqual(result, 0)
            self.assertIn("peval-py workspace state DB", stderr.getvalue())
            self.assertIn("-r <workspace>", stderr.getvalue())
            self.assertIn("-d @adapter", stderr.getvalue())

    def test_cli_root_option_reads_cached_analysis_from_workspace(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            workspace = root / "workspace"
            outside = root / "outside"
            workspace.mkdir()
            outside.mkdir()
            (workspace / "peval-py.toml").write_text(
                'analysis_eval_slug = "default"\n',
                encoding="utf-8",
            )
            analysis_path = write_cli_cached_analysis(workspace)
            markdown_path = write_cli_cached_markdown(workspace)
            out_path = root / "report.json"

            with contextlib.chdir(outside):
                result = main(
                    [
                        "view",
                        "tr",
                        "-m",
                        "raw",
                        "-r",
                        str(workspace),
                        "-a",
                        "opencode",
                        "-p",
                        str(FIXTURES / "common_session.jsonl"),
                        "--agent-name",
                        "agent-a",
                        "-f",
                        "json",
                        "-o",
                        str(out_path),
                    ]
                )
            self.assertEqual(result, 0)

            payload = json.loads(out_path.read_text(encoding="utf-8"))
            analysis = payload["annotations"]["analysis"][0]
            self.assertEqual(analysis["summary"], "Root-selected cached analysis.")
            self.assertEqual(analysis["findings"][0]["title"], "Root-selected finding.")
            self.assertEqual(analysis["analysis_metrics"]["review_turns"], 2)
            self.assertIn("auto", analysis["analysis_metrics"])
            self.assertIn("Cached markdown body.", analysis["md_report"])
            self.assertEqual(
                analysis["relative_paths"],
                {
                    "json": analysis_path.relative_to(workspace).as_posix(),
                    "md": markdown_path.relative_to(workspace).as_posix(),
                },
            )

    def test_cli_import_analysis_json_compiles_and_view_reads_it(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            workspace = root / "workspace"
            write_peval_workspace(workspace)
            analysis_report = root / "analysis-report.json"
            analysis_report.write_text(
                json.dumps(
                    {
                        "summary": "Imported analysis summary.",
                        "findings": [{"title": "Imported finding."}],
                        "recommendations": ["Keep the import workflow."],
                        "limitations": ["No live validation."],
                        "confidence": "medium",
                        "subject": {"agent_id": "input-agent"},
                        "metrics": {"input_turns": 99, "auto": {"bad": True}},
                        "commands": ["agent-authored command"],
                        "custom": {"from": "top-level"},
                        "extra": {
                            "custom": {"from": "input-extra"},
                            "input_only": True,
                        },
                    }
                ),
                encoding="utf-8",
            )
            run_path = "runs/default/agent-a/common_session/session_t001"

            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                result = main(
                    [
                        "import",
                        "analysis",
                        "-r",
                        str(workspace),
                        "--run-path",
                        run_path,
                        "-p",
                        str(analysis_report),
                    ]
                )
            self.assertEqual(result, 0)
            self.assertNotIn("warning", stdout.getvalue().lower())

            analysis_path = workspace / run_path / "analysis.json"
            payload = json.loads(analysis_path.read_text(encoding="utf-8"))
            self.assertEqual(payload["summary"], "Imported analysis summary.")
            self.assertEqual(payload["status"], "analyzed")
            self.assertEqual(
                payload["subject"],
                {
                    "eval_slug": "default",
                    "agent_id": "agent-a",
                    "session_id": "common_session",
                    "cell_key": "session_t001",
                },
            )
            self.assertNotIn("metrics", payload)
            self.assertNotIn("commands", payload)
            self.assertEqual(
                payload["extra"],
                {
                    "custom": {"from": "top-level"},
                    "input_only": True,
                    "subject": {"agent_id": "input-agent"},
                    "metrics": {"input_turns": 99, "auto": {"bad": True}},
                    "commands": ["agent-authored command"],
                },
            )

            out_path = root / "report.json"
            result = main(
                [
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "-r",
                    str(workspace),
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "--agent-name",
                    "agent-a",
                    "-f",
                    "json",
                    "-o",
                    str(out_path),
                ]
            )
            self.assertEqual(result, 0)
            report = json.loads(out_path.read_text(encoding="utf-8"))
            analysis = report["annotations"]["analysis"][0]
            self.assertEqual(analysis["summary"], "Imported analysis summary.")
            self.assertEqual(analysis["analysis_status"], "analyzed")
            self.assertEqual(analysis["findings"][0]["title"], "Imported finding.")
            self.assertEqual(analysis["subject"]["agent_id"], "agent-a")
            self.assertEqual(analysis["analysis_metrics"]["input_turns"], 99)
            self.assertIn("auto", analysis["analysis_metrics"])
            self.assertNotIn("bad", analysis["analysis_metrics"]["auto"])
            self.assertNotIn("extra", analysis)
            self.assertEqual(
                analysis["relative_path"],
                f"{run_path}/analysis.json",
            )

    def test_cli_import_analysis_markdown_only_with_json_output(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            workspace = root / "workspace"
            write_peval_workspace(workspace)
            analysis_report = root / "analysis-report.markdown"
            analysis_report.write_text(
                "# Analysis\n\nMarkdown-only body.\n",
                encoding="utf-8",
            )
            run_cell = (
                workspace
                / "runs"
                / "default"
                / "agent-a"
                / "common_session"
                / "session_t001"
            )

            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                result = main(
                    [
                        "import",
                        "analysis",
                        "-r",
                        str(workspace),
                        "--run-path",
                        str(run_cell),
                        "-p",
                        str(analysis_report),
                        "--json",
                    ]
                )
            self.assertEqual(result, 0)
            payload = json.loads(stdout.getvalue())
            self.assertEqual(
                payload["run_path"],
                "runs/default/agent-a/common_session/session_t001",
            )
            self.assertEqual(
                payload["written"],
                {
                    "analysis_md": (
                        "runs/default/agent-a/common_session/session_t001/analysis.md"
                    )
                },
            )
            self.assertEqual(
                (run_cell / "analysis.md").read_text(encoding="utf-8"),
                "# Analysis\n\nMarkdown-only body.\n",
            )
            self.assertFalse((run_cell / "analysis.json").exists())
            self.assertEqual(payload["warnings"], [])

    def test_cli_import_analysis_json_warnings_are_json_only(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            workspace = root / "workspace"
            write_peval_workspace(workspace)
            analysis_report = root / "analysis-report.json"
            analysis_report.write_text(
                json.dumps(
                    {
                        "summary": "Compiled summary.",
                        "subject": {"agent_id": "input-agent"},
                        "metrics": {"input_turns": 3},
                        "commands": ["external command"],
                        "analysis_status": "reviewed",
                        "analysis_metrics": {"reported_turns": 3},
                        "auto": {"bad": True},
                        "custom": {"silent": True},
                        "extra": {
                            "summary": "Nested summary.",
                            "status": "nested-status",
                            "findings": [{"title": "Nested finding."}],
                            "recommendations": ["Nested recommendation."],
                            "limitations": ["Nested limitation."],
                            "confidence": "low",
                            "custom_nested": True,
                        },
                    }
                ),
                encoding="utf-8",
            )
            run_path = "runs/default/agent-a/common_session/session_t001"

            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                result = main(
                    [
                        "import",
                        "analysis",
                        "-r",
                        str(workspace),
                        "--run-path",
                        run_path,
                        "-p",
                        str(analysis_report),
                        "--json",
                    ]
                )
            self.assertEqual(result, 0)
            output = json.loads(stdout.getvalue())
            self.assertEqual(
                [
                    (
                        item["code"],
                        item["field"],
                        item["location"],
                        item["stored_as"],
                    )
                    for item in output["warnings"]
                ],
                [
                    (
                        "field_preserved_in_extra",
                        "subject",
                        "top_level",
                        "extra.subject",
                    ),
                    (
                        "field_preserved_in_extra",
                        "metrics",
                        "top_level",
                        "extra.metrics",
                    ),
                    (
                        "field_preserved_in_extra",
                        "commands",
                        "top_level",
                        "extra.commands",
                    ),
                    (
                        "field_preserved_in_extra",
                        "analysis_status",
                        "top_level",
                        "extra.analysis_status",
                    ),
                    (
                        "field_preserved_in_extra",
                        "analysis_metrics",
                        "top_level",
                        "extra.analysis_metrics",
                    ),
                    (
                        "field_preserved_in_extra",
                        "auto",
                        "top_level",
                        "extra.auto",
                    ),
                    (
                        "standard_field_nested_in_extra",
                        "summary",
                        "extra",
                        "extra.summary",
                    ),
                    (
                        "standard_field_nested_in_extra",
                        "status",
                        "extra",
                        "extra.status",
                    ),
                    (
                        "standard_field_nested_in_extra",
                        "findings",
                        "extra",
                        "extra.findings",
                    ),
                    (
                        "standard_field_nested_in_extra",
                        "recommendations",
                        "extra",
                        "extra.recommendations",
                    ),
                    (
                        "standard_field_nested_in_extra",
                        "limitations",
                        "extra",
                        "extra.limitations",
                    ),
                    (
                        "standard_field_nested_in_extra",
                        "confidence",
                        "extra",
                        "extra.confidence",
                    ),
                ],
            )
            self.assertTrue(
                all(item["message"] for item in output["warnings"]),
            )
            self.assertNotIn(
                "custom",
                {item["field"] for item in output["warnings"]},
            )
            self.assertNotIn(
                "custom_nested",
                {item["field"] for item in output["warnings"]},
            )

            compiled = json.loads(
                (workspace / run_path / "analysis.json").read_text(encoding="utf-8")
            )
            self.assertEqual(compiled["summary"], "Compiled summary.")
            self.assertEqual(compiled["status"], "analyzed")
            self.assertEqual(compiled["subject"]["agent_id"], "agent-a")
            self.assertNotIn("metrics", compiled)
            self.assertNotIn("commands", compiled)
            self.assertEqual(compiled["extra"]["analysis_status"], "reviewed")
            self.assertEqual(
                compiled["extra"]["analysis_metrics"],
                {"reported_turns": 3},
            )
            self.assertEqual(compiled["extra"]["auto"], {"bad": True})
            self.assertEqual(compiled["extra"]["summary"], "Nested summary.")
            self.assertEqual(compiled["extra"]["custom"], {"silent": True})
            self.assertTrue(compiled["extra"]["custom_nested"])

    def test_cli_import_analysis_json_and_markdown_write_both(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            workspace = root / "workspace"
            write_peval_workspace(workspace)
            json_report = root / "analysis-report.json"
            json_report.write_text(
                json.dumps({"summary": "Structured import.", "status": "reviewed"}),
                encoding="utf-8",
            )
            md_report = root / "analysis-report.md"
            md_report.write_text("Narrative import.\n", encoding="utf-8")
            run_path = "runs/default/agent-a/common_session/session_t001"

            with contextlib.redirect_stdout(io.StringIO()):
                result = main(
                    [
                        "import",
                        "analysis",
                        "-r",
                        str(workspace),
                        "--run-path",
                        run_path,
                        "-p",
                        str(json_report),
                        "-p",
                        str(md_report),
                    ]
                )
            self.assertEqual(result, 0)
            cell = workspace / run_path
            analysis_json = json.loads(
                (cell / "analysis.json").read_text(encoding="utf-8")
            )
            self.assertEqual(analysis_json["status"], "reviewed")
            self.assertEqual(
                (cell / "analysis.md").read_text(encoding="utf-8"),
                "Narrative import.\n",
            )

    def test_cli_import_analysis_rejects_invalid_inputs_without_partial_writes(
        self,
    ) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            workspace = root / "workspace"
            outside = root / "outside"
            outside.mkdir()
            write_peval_workspace(workspace)
            valid_json = root / "valid.json"
            valid_json.write_text(json.dumps({"summary": "ok"}), encoding="utf-8")
            duplicate_json = root / "duplicate.json"
            duplicate_json.write_text(
                json.dumps({"summary": "duplicate"}),
                encoding="utf-8",
            )
            valid_md = root / "valid.md"
            valid_md.write_text("ok\n", encoding="utf-8")
            duplicate_md = root / "duplicate.markdown"
            duplicate_md.write_text("duplicate\n", encoding="utf-8")
            unsupported = root / "analysis-report.txt"
            unsupported.write_text("nope\n", encoding="utf-8")
            invalid_json = root / "invalid.json"
            invalid_json.write_text("{", encoding="utf-8")
            bad_extra_json = root / "bad-extra.json"
            bad_extra_json.write_text(
                json.dumps({"summary": "bad", "extra": ["not", "object"]}),
                encoding="utf-8",
            )

            cases = [
                (
                    [
                        "--run-path",
                        "runs/default/agent-a/common_session/session_t001",
                        "-p",
                        str(unsupported),
                    ],
                    "unsupported analysis input suffix",
                ),
                (
                    [
                        "--run-path",
                        "runs/default/agent-a/common_session/session_t002",
                        "-p",
                        str(valid_json),
                        "-p",
                        str(duplicate_json),
                    ],
                    "at most one JSON analysis input",
                ),
                (
                    [
                        "--run-path",
                        "runs/default/agent-a/common_session/session_t003",
                        "-p",
                        str(valid_md),
                        "-p",
                        str(duplicate_md),
                    ],
                    "at most one Markdown analysis input",
                ),
                (
                    [
                        "--run-path",
                        "runs/default/agent-a/common_session/session_t004",
                        "-p",
                        str(invalid_json),
                    ],
                    "failed to parse analysis JSON input",
                ),
                (
                    [
                        "--run-path",
                        "runs/default/agent-a/common_session/session_t005",
                        "-p",
                        str(bad_extra_json),
                    ],
                    "field 'extra' must be an object",
                ),
                (
                    ["--run-path", "runs/default/agent-a", "-p", str(valid_json)],
                    "must have form",
                ),
                (
                    [
                        "--run-path",
                        str(outside / "runs/default/agent-a/common/session"),
                        "-p",
                        str(valid_json),
                    ],
                    "inside the workspace root",
                ),
                (
                    [
                        "--run-path",
                        "not-runs/default/agent-a/common/session",
                        "-p",
                        str(valid_json),
                    ],
                    "under the workspace runs/",
                ),
            ]
            for index, (extra_args, expected) in enumerate(cases, start=1):
                with self.subTest(index=index):
                    stderr = io.StringIO()
                    with contextlib.redirect_stderr(stderr):
                        result = main(
                            [
                                "import",
                                "analysis",
                                "-r",
                                str(workspace),
                                *extra_args,
                            ]
                        )
                    self.assertNotEqual(result, 0)
                    self.assertIn(expected, stderr.getvalue())
                    run_root = workspace / "runs" / "default" / "agent-a"
                    if run_root.exists():
                        self.assertEqual(
                            list(run_root.rglob("analysis.json"))
                            + list(run_root.rglob("analysis.md")),
                            [],
                        )

    def test_cli_root_option_requires_initialized_workspace(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            missing = root / "missing"
            for command in ("view", "export"):
                with self.subTest(command=command):
                    stderr = io.StringIO()
                    with contextlib.redirect_stderr(stderr):
                        result = main(
                            [
                                command,
                                "tr",
                                "-r",
                                str(missing),
                                "-p",
                                str(FIXTURES / "common_session.jsonl"),
                            ]
                        )
                    self.assertNotEqual(result, 0)
                    self.assertIn(
                        "is not an initialized peval-py workspace",
                        stderr.getvalue(),
                    )
                    self.assertIn("peval-py init -r", stderr.getvalue())
            analysis_report = root / "analysis-report.json"
            analysis_report.write_text(
                json.dumps({"summary": "missing root"}),
                encoding="utf-8",
            )
            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr):
                result = main(
                    [
                        "import",
                        "analysis",
                        "-r",
                        str(missing),
                        "--run-path",
                        "runs/default/agent-a/common_session/session_t001",
                        "-p",
                        str(analysis_report),
                    ]
                )
            self.assertNotEqual(result, 0)
            self.assertIn("is not an initialized peval-py workspace", stderr.getvalue())
            self.assertIn("peval-py init -r", stderr.getvalue())

    def test_cli_input_table_csv_expands_sessions_and_overrides(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            shutil.copy(FIXTURES / "common_session.jsonl", root / "common_session.jsonl")
            db_path = root / "state.db"
            create_messages_db(db_path)
            table = root / "inputs.csv"
            table.write_text(
                "\n".join(
                    [
                        "p,db,session,adapter,n,report_note,agent_name,agent_version,model",
                        "common_session.jsonl,,,"
                        "opencode,CSV row note,,csv-agent,9.9.9,csv-model",
                        ",state.db,db-b,psychevo,2=DB indexed note,CSV report note,,,",
                    ]
                )
                + "\n",
                encoding="utf-8",
            )
            out_path = root / "report.json"
            result = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "peval_py.cli",
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "--agent-name",
                    "global-agent",
                    "--model",
                    "global-model",
                    "-i",
                    str(table),
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
            self.assertEqual(
                [item["session_id"] for item in payload["trajectory"]],
                ["common_session", "db-b"],
            )
            self.assertEqual(
                [item["adapter"] for item in payload["trajectory_meta"]],
                ["opencode", "psychevo"],
            )
            self.assertEqual(payload["trajectory"][0]["agent"]["name"], "csv-agent")
            self.assertEqual(payload["trajectory"][0]["agent"]["version"], "9.9.9")
            self.assertEqual(payload["trajectory"][0]["agent"]["model_name"], "csv-model")
            self.assertEqual(payload["trajectory"][1]["agent"]["name"], "global-agent")
            self.assertEqual(payload["trajectory"][1]["agent"]["model_name"], "global-model")
            self.assertEqual(payload["annotations"]["report_notes"][0]["markdown"], "CSV report note")
            self.assertEqual(
                [item["markdown"] for item in payload["annotations"]["notes"]],
                ["CSV row note", "DB indexed note"],
            )

    def test_cli_source_aliases_are_display_only(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            shutil.copy(FIXTURES / "common_session.jsonl", root / "common_session.jsonl")
            db_path = root / "state.db"
            create_messages_db(db_path)
            table = root / "inputs.json"
            table.write_text(
                json.dumps(
                    [
                        {
                            "path": str(root / "common_session.jsonl"),
                            "adapter": "opencode",
                            "alias": "Table path alias",
                        },
                        {
                            "db": str(db_path),
                            "session_id": "db-a",
                            "adapter": "psychevo",
                            "source_alias": "Table DB alias",
                        },
                    ]
                ),
                encoding="utf-8",
            )
            out_path = root / "aliases.json"
            result = main(
                [
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "-i",
                    str(table),
                    "--source-alias",
                    "1=CLI path alias",
                    "-f",
                    "json",
                    "-o",
                    str(out_path),
                ]
            )
            self.assertEqual(result, 0)
            payload = json.loads(out_path.read_text(encoding="utf-8"))
            self.assertEqual(
                [item.get("source_alias") for item in payload["trajectory_meta"]],
                ["CLI path alias", "Table DB alias"],
            )
            self.assertNotIn("comparison", payload)
            self.assertEqual(
                [item["session_id"] for item in payload["trajectory"]],
                ["common_session", "db-a"],
            )
            self.assertEqual(
                payload["trajectory_meta"][0]["data_ref"]["label"],
                "common_session.jsonl",
            )
            self.assertEqual(
                payload["trajectory_meta"][1]["data_ref"]["label"],
                "state.db:db-a",
            )

            for alias_arg, message in [
                ("2=duplicate", "duplicate --source-alias index: 2"),
                ("3=missing", "out of range for 2 sessions"),
                ("1=", "text must not be empty"),
            ]:
                with self.subTest(alias_arg=alias_arg):
                    stderr = io.StringIO()
                    with contextlib.redirect_stderr(stderr):
                        result = main(
                            [
                                "view",
                                "tr",
                        "-m",
                        "raw",
                                "-i",
                                str(table),
                                "--source-alias",
                                "2=first",
                                "--source-alias",
                                alias_arg,
                            ]
                        )
                    self.assertNotEqual(result, 0)
                    self.assertIn(message, stderr.getvalue())


    def test_cli_input_table_json_forms_notes_and_export_boundary(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            shutil.copy(FIXTURES / "common_session.jsonl", root / "common_session.jsonl")
            db_path = root / "state.db"
            create_messages_db(db_path)
            table = root / "inputs.json"
            table.write_text(
                json.dumps(
                    {
                        "report_notes": ["JSON report"],
                        "rows": [
                            {
                                "path": "common_session.jsonl",
                                "adapter": "opencode",
                                "notes": ["JSON path note", "0=JSON inline report"],
                            },
                            {
                                "db": "state.db",
                                "session_id": "db-a",
                                "adapter": "psychevo",
                                "note": "JSON DB note",
                                "model": "json-db-model",
                            },
                        ],
                    }
                ),
                encoding="utf-8",
            )
            out_path = root / "report.json"
            result = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "peval_py.cli",
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "-i",
                    str(table),
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
            self.assertEqual([item["session_id"] for item in payload["trajectory"]], ["common_session", "db-a"])
            self.assertEqual(payload["trajectory"][1]["agent"]["model_name"], "json-db-model")
            self.assertEqual(
                [item["markdown"] for item in payload["annotations"]["report_notes"]],
                ["JSON report", "JSON inline report"],
            )
            self.assertEqual(
                [item["markdown"] for item in payload["annotations"]["notes"]],
                ["JSON path note", "JSON DB note"],
            )

            export_multi = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "peval_py.cli",
                    "export",
                    "tr",
                    "-i",
                    str(table),
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(export_multi.returncode, 0)
            self.assertIn("exactly one input session", export_multi.stderr)

            single_table = root / "single.json"
            single_table.write_text(
                json.dumps([{"path": "common_session.jsonl", "adapter": "opencode"}]),
                encoding="utf-8",
            )
            export_out = root / "trajectory.json"
            result = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "peval_py.cli",
                    "export",
                    "tr",
                    "-i",
                    str(single_table),
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

            direct_plus_table = root / "direct-plus-table.json"
            direct_plus_table.write_text(
                json.dumps([{"db": "state.db", "session_id": "db-a", "adapter": "psychevo"}]),
                encoding="utf-8",
            )
            direct_plus_out = root / "direct-plus-report.json"
            result = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "peval_py.cli",
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "-a",
                    "opencode",
                    "-p",
                    str(root / "common_session.jsonl"),
                    "-i",
                    str(direct_plus_table),
                    "-f",
                    "json",
                    "-o",
                    str(direct_plus_out),
                ],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            payload = json.loads(direct_plus_out.read_text(encoding="utf-8"))
            self.assertEqual(
                [item["session_id"] for item in payload["trajectory"]],
                ["common_session", "db-a"],
            )


    def test_input_table_validation_errors(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            cases = {
                "unknown.csv": ("wat\nx\n", "unknown input table column"),
                "duplicate.csv": ("path,p\none,two\n", "duplicate input table column"),
                "both.csv": ("path,db\none.jsonl,state.db\n", "provide exactly one"),
                "neither.csv": ("adapter\nopencode\n", "provide exactly one"),
                "path_session.csv": ("path,session_id\none.jsonl,s1\n", "session_id is only valid"),
            }
            for name, (content, message) in cases.items():
                with self.subTest(name=name):
                    path = root / name
                    path.write_text(content, encoding="utf-8")
                    with self.assertRaisesRegex(ValueError, message):
                        read_input_table(str(path))

            xls_path = root / "inputs.xls"
            xls_path.write_text("not excel", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, ".xls input tables are unsupported"):
                read_input_table(str(xls_path))

            xlsx_path = root / "inputs.xlsx"
            xlsx_path.write_text("not excel", encoding="utf-8")
            with patch("peval_py.input_table.import_module", side_effect=ImportError):
                with self.assertRaisesRegex(ValueError, "requires optional dependency openpyxl"):
                    read_input_table(str(xlsx_path))


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
                        "-m",
                        "raw",
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
            self.assertNotIn("comparison", payload)
            subprocess.run(
                [sys.executable, "-m", "json.tool", str(out_path)],
                check=True,
                text=True,
                capture_output=True,
            )

            mixed_db = Path(tmp) / "opencode.db"
            mixed_out = Path(tmp) / "mixed.json"
            create_opencode_db(mixed_db)
            mixed = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "peval_py.cli",
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-d",
                    str(mixed_db),
                    "-f",
                    "json",
                    "-o",
                    str(mixed_out),
                ],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(mixed.stderr, "")
            payload = json.loads(mixed_out.read_text(encoding="utf-8"))
            self.assertEqual(len(payload["trajectory"]), 2)
            self.assertEqual(
                [item["adapter"] for item in payload["trajectory_meta"]],
                ["opencode", "opencode"],
            )

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
            self.assertIn("exactly one input session", export_multi.stderr)

            legacy_jsonl_flag = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "peval_py.cli",
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "-j",
                    str(FIXTURES / "common_session.jsonl"),
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(legacy_jsonl_flag.returncode, 0)


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
                        "-m",
                        "raw",
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
                    self.assertNotIn("--trajectory-id", result.stdout)

            serve_help = subprocess.run(
                [command, "serve", "--help"],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(serve_help.returncode, 0)
            self.assertNotIn("--trajectory-id", serve_help.stdout)

            result = subprocess.run(
                [command, "view", "tr", "--help"],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertIn("-p", result.stdout)
            self.assertIn("--path", result.stdout)
            self.assertIn("-i", result.stdout)
            self.assertIn("--input-table", result.stdout)
            self.assertIn("-n", result.stdout)
            self.assertIn("--note", result.stdout)
            self.assertIn("--head", result.stdout)
            self.assertIn("--tail", result.stdout)
            self.assertIn("--top", result.stdout)
            self.assertIn("--step", result.stdout)
            self.assertIn("--tool-call", result.stdout)
            self.assertNotIn("--on", result.stdout)
            self.assertIn("raw report options", result.stdout)
            self.assertIn("--agent-name", result.stdout)
            self.assertNotIn("--trajectory-id", result.stdout)

            result = subprocess.run(
                [
                    command,
                    "view",
                    "tr",
                        "-m",
                        "raw",
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
            default_report = written_report_path(result.stdout, Path(tmp))
            self.assertRegex(
                default_report.name,
                r"^report-opencode-common_session-\d{8}-\d{6}-\d{6}\.html$",
            )
            self.assertIn("<!doctype html>", default_report.read_text(encoding="utf-8"))

            zh_config = Path(tmp) / "zh.toml"
            zh_config.write_text(
                "[defaults]\nlocale = \"zh-CN\"\n",
                encoding="utf-8",
            )
            zh_report = Path(tmp) / "zh-report.html"
            result = subprocess.run(
                [
                    command,
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "-c",
                    str(zh_config),
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-f",
                    "html",
                    "-o",
                    str(zh_report),
                ],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            html = zh_report.read_text(encoding="utf-8")
            self.assertIn('<html lang="zh-CN">', html)
            self.assertIn("<h1>Agent 轨迹报告</h1>", html)
            self.assertIn('"run": "Run"', html)
            self.assertIn('"session": "Session"', html)
            self.assertIn('"agent": "Agent"', html)
            self.assertIn('"trajectory_overview": "轨迹概览"', html)
            self.assertIn('"filter": "筛选"', html)
            self.assertIn('"step_details": "Step 详情"', html)
            self.assertIn('"open_step_details": "打开 Step 详情"', html)
            self.assertIn('"close": "关闭"', html)
            self.assertIn('"evidence": "Evidence"', html)
            self.assertIn('"turns": "Turns"', html)
            self.assertIn('"tool_calls": "Tool Calls"', html)
            self.assertIn('"tool_success_total": "tool success / total"', html)
            self.assertNotIn('"visible_heatmap"', html)
            self.assertNotIn('"run": "运行"', html)
            self.assertNotIn('"turns": "轮次"', html)
            self.assertNotIn('"tool_calls": "工具调用"', html)
            self.assertIn("<h3>Steps (${count})</h3>", html)

            opencode_db = Path(tmp) / "opencode.db"
            create_opencode_db(opencode_db)
            opencode_db_report = Path(tmp) / "opencode-db-report.json"
            result = subprocess.run(
                [
                    command,
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "-a",
                    "opencode",
                    "-d",
                    str(opencode_db),
                    "-f",
                    "json",
                    "-o",
                    str(opencode_db_report),
                ],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            payload = json.loads(opencode_db_report.read_text(encoding="utf-8"))
            self.assertEqual(payload["trajectory"][0]["session_id"], "ses-latest")
            self.assertEqual(payload["trajectory"][0]["steps"][0]["message"], "latest prompt")

            hermes_db = Path(tmp) / "state.db"
            create_hermes_db(hermes_db)
            hermes_db_report = Path(tmp) / "hermes-db-report.json"
            result = subprocess.run(
                [
                    command,
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "-a",
                    "hermes",
                    "-d",
                    str(hermes_db),
                    "-f",
                    "json",
                    "-o",
                    str(hermes_db_report),
                ],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            payload = json.loads(hermes_db_report.read_text(encoding="utf-8"))
            self.assertEqual(payload["trajectory"][0]["session_id"], "hermes-latest")
            self.assertEqual(payload["trajectory"][0]["steps"][0]["source"], "system")
            self.assertEqual(
                payload["trajectory"][0]["steps"][0]["message"],
                "Hermes system prompt",
            )
            self.assertEqual(
                payload["trajectory_meta"][0]["timestamp_semantics"],
                "order_only",
            )
            self.assertIsNone(payload["trajectory_meta"][0]["duration_ms"])

            psychevo_db = Path(tmp) / "psychevo-state.db"
            create_messages_db(psychevo_db)
            psychevo_db_report = Path(tmp) / "psychevo-db-report.json"
            result = subprocess.run(
                [
                    command,
                    "view",
                    "tr",
                        "-m",
                        "raw",
                    "-a",
                    "psychevo",
                    "-d",
                    str(psychevo_db),
                    "-f",
                    "json",
                    "-o",
                    str(psychevo_db_report),
                ],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            payload = json.loads(psychevo_db_report.read_text(encoding="utf-8"))
            self.assertEqual(payload["trajectory"][0]["session_id"], "db-b")
            self.assertEqual(payload["trajectory"][0]["steps"][0]["message"], "hello b")

            result = subprocess.run(
                [
                    command,
                    "view",
                    "tr",
                        "-m",
                        "raw",
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
            default_report_json = written_report_path(result.stdout, Path(tmp))
            self.assertRegex(
                default_report_json.name,
                r"^report-opencode-common_session-\d{8}-\d{6}-\d{6}\.json$",
            )
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
