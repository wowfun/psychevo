from __future__ import annotations

from cli_inputs_support import *

class PevalPyCliInputTableExportTests(unittest.TestCase):
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
