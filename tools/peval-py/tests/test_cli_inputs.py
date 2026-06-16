from __future__ import annotations

from peval_py_test_support import *


class PevalPyCliInputTests(unittest.TestCase):
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
                self.assertEqual(
                    [item["adapter"] for item in payload["comparison"]["leaderboard"]["entries"]],
                    ["opencode", "custom"],
                )
                self.assertEqual(
                    payload["trajectory"][1]["session_id"],
                    "selected:custom",
                )

                for argv, message in [
                    (
                        [
                            "view",
                            "tr",
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
            self.assertEqual(
                [item["adapter"] for item in payload["comparison"]["leaderboard"]["entries"]],
                ["hermes", "opencode"],
            )

            mixed_out = tmp_path / "mixed.json"
            result = main(
                [
                    "view",
                    "tr",
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
            self.assertEqual(
                [
                    item.get("source_alias")
                    for item in payload["comparison"]["leaderboard"]["entries"]
                ],
                ["CLI path alias", "Table DB alias"],
            )
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
            self.assertIn("-i", result.stdout)
            self.assertIn("--input-table", result.stdout)
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
