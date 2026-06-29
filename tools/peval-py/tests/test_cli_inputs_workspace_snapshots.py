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

    def test_cli_workspace_state_db_accepts_windows_drive_paths_with_wsl_mapping(self) -> None:
        from peval_py.cli import main
        from peval_py.state import open_workspace_state

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            mount_root = root / "mnt"
            workspace = mount_root / "c" / "Users" / "kevin" / "workspace"
            outside = root / "outside"
            source_db = root / "source.db"
            workspace.mkdir(parents=True)
            outside.mkdir()
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
                        )
                    ],
                    notes=[],
                ),
                config,
            )
            store.close()
            source_db.unlink()
            view_out = root / "windows-workspace-state.json"

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
                        "-r",
                        "C:/Users/kevin/workspace",
                        "-d",
                        "C:/Users/kevin/workspace/state.db",
                        "-f",
                        "json",
                        "-o",
                        str(view_out),
                    ]
                )
            self.assertEqual(result, 0)

            payload = json.loads(view_out.read_text(encoding="utf-8"))
            self.assertEqual(payload["trajectory"][0]["session_id"], "db-a")
            self.assertEqual(payload["trajectory_meta"][0]["adapter"], "psychevo")

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
