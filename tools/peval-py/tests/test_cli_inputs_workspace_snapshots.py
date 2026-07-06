from __future__ import annotations

from cli_inputs_support import *

class PevalPyCliInputWorkspaceSnapshotTests(unittest.TestCase):
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
                'analysis_eval_slug = "default"\n',
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

    def test_cli_trial_cell_path_inputs_accept_globs_and_descendants(self) -> None:
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
            )
            glob_out = root / "glob-inspect.json"
            descendant_out = root / "descendant-inspect.json"
            export_out = root / "descendant-export.json"

            with contextlib.chdir(outside):
                result = main(
                    [
                        "view",
                        "tr",
                        "-p",
                        f"{cell_dir}/**",
                        "-p",
                        f"{cell_dir}/**/*",
                        "-o",
                        str(glob_out),
                    ]
                )
                self.assertEqual(result, 0)

                result = main(
                    [
                        "view",
                        "tr",
                        "-p",
                        str(cell_dir / "agent"),
                        "-p",
                        str(cell_dir / "agent" / "trajectory.json"),
                        "-o",
                        str(descendant_out),
                    ]
                )
                self.assertEqual(result, 0)

                result = main(
                    [
                        "export",
                        "tr",
                        "-p",
                        str(cell_dir / "agent" / "trajectory.json"),
                        "-o",
                        str(export_out),
                    ]
                )
                self.assertEqual(result, 0)

            glob_payload = json.loads(glob_out.read_text(encoding="utf-8"))
            self.assertEqual(len(glob_payload["sources"]), 1)
            self.assertEqual(glob_payload["sources"][0]["session_id"], "artifact-session")
            descendant_payload = json.loads(descendant_out.read_text(encoding="utf-8"))
            self.assertEqual(len(descendant_payload["sources"]), 1)
            self.assertEqual(
                descendant_payload["sources"][0]["session_id"],
                "artifact-session",
            )
            export_payload = json.loads(export_out.read_text(encoding="utf-8"))
            self.assertEqual(export_payload["session_id"], "artifact-session")

    def test_cli_trial_cell_path_ignores_conflicting_input_selectors(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            workspace = root / "workspace"
            other_workspace = root / "other-workspace"
            outside = root / "outside"
            outside.mkdir()
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
            write_trial_cell_artifacts(cell_dir, session_id="artifact-session")
            inspect_out = root / "cell-wins.json"

            with contextlib.chdir(outside):
                result = main(
                    [
                        "view",
                        "tr",
                        "-p",
                        str(cell_dir),
                        "-p",
                        str(root / "not-a-cell.jsonl"),
                        "-r",
                        str(other_workspace),
                        "-a",
                        "missing-adapter",
                        "-d",
                        str(root / "missing.db"),
                        "-s",
                        "missing-session",
                        "-i",
                        str(root / "missing.csv"),
                        "--list",
                        "-o",
                        str(inspect_out),
                    ]
                )

            self.assertEqual(result, 0)
            payload = json.loads(inspect_out.read_text(encoding="utf-8"))
            self.assertEqual(len(payload["sources"]), 1)
            self.assertEqual(payload["sources"][0]["session_id"], "artifact-session")

    def test_cli_trial_cell_path_accepts_windows_drive_path_with_wsl_mapping(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            mount_root = root / "mnt"
            workspace = mount_root / "c" / "Users" / "kevin" / "workspace"
            outside = root / "outside"
            outside.mkdir()
            write_peval_workspace(workspace)
            cell_dir = (
                workspace
                / "runs"
                / "default"
                / "psychevo"
                / "windows-artifact"
                / "session_t001"
            )
            write_trial_cell_artifacts(
                cell_dir,
                session_id="windows-artifact",
                trial_key="session_t001",
            )
            raw_out = root / "windows-artifact-raw.json"

            with (
                patch("peval_py.config.WINDOWS_DRIVE_MOUNT_ROOT", mount_root),
                contextlib.chdir(outside),
            ):
                result = main(
                    [
                        "view",
                        "tr",
                        "-m",
                        "raw",
                        "-p",
                        "C:/Users/kevin/workspace/runs/default/psychevo/windows-artifact/session_t001",
                        "-f",
                        "json",
                        "-o",
                        str(raw_out),
                    ]
                )
            self.assertEqual(result, 0)

            payload = json.loads(raw_out.read_text(encoding="utf-8"))
            self.assertEqual(payload["trajectory"][0]["session_id"], "windows-artifact")
            self.assertEqual(
                payload["trajectory_meta"][0]["artifact_ref"]["workspace_relative_path"],
                "runs/default/psychevo/windows-artifact/session_t001",
            )

    def test_unmapped_windows_paths_are_not_resolved_under_cwd(self) -> None:
        from peval_py._inputs.workspace_snapshots import resolved_local_path

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            cwd = root / "cwd"
            mount_root = root / "empty-mnt"
            fake_windows_path = cwd / "C:" / "Users" / "kevin" / "workspace"
            fake_windows_path.mkdir(parents=True)
            with (
                patch("peval_py.config.WINDOWS_DRIVE_MOUNT_ROOT", mount_root),
                contextlib.chdir(cwd),
            ):
                self.assertIsNone(resolved_local_path("C:/Users/kevin/workspace"))
                self.assertIsNone(resolved_local_path(r"C:\Users\kevin\workspace"))
                self.assertIsNone(resolved_local_path(r"\\server\share\workspace"))

    def test_cli_trial_cell_path_malformed_descendant_error_is_actionable(self) -> None:
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
                result = main(
                    [
                        "view",
                        "tr",
                        "-p",
                        str(malformed / "agent"),
                    ]
                )

            message = stderr.getvalue()
            self.assertNotEqual(result, 0)
            self.assertIn("Trial cell artifact directory", message)
            self.assertIn("agent/trajectory.json", message)
            self.assertIn("agent/trajectory_meta.json", message)

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
