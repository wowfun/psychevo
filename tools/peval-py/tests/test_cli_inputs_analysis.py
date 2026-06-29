from __future__ import annotations

from cli_inputs_support import *

class PevalPyCliInputAnalysisTests(unittest.TestCase):
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
