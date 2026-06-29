from __future__ import annotations

from cli_inputs_support import *

class PevalPyCliInputSourceTests(unittest.TestCase):
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
