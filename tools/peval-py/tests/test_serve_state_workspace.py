from __future__ import annotations

from serve_state_support import *

class PevalPyServeStateWorkspaceTests(unittest.TestCase):
    def test_workspace_discovery_and_missing_workspace_diagnostics(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp) / "workspace")
            child = root / "nested" / "child"
            child.mkdir(parents=True)
            old_cwd = Path.cwd()
            old_env = os.environ.pop("PEVAL_ROOT", None)
            try:
                os.chdir(child)
                self.assertEqual(resolve_workspace_root(), root.resolve())
                with patch.dict(os.environ, {"PEVAL_ROOT": str(root)}):
                    self.assertEqual(resolve_workspace_root(), root.resolve())
            finally:
                os.chdir(old_cwd)
                if old_env is not None:
                    os.environ["PEVAL_ROOT"] = old_env

            created = Path(tmp) / "created"
            store = open_workspace_state(str(created))
            try:
                self.assertTrue((created / "peval-py.toml").is_file())
                self.assertTrue((created / "state.db").is_file())
                self.assertFalse((created / "peval.toml").exists())
            finally:
                store.close()

            isolated = Path(tmp) / "isolated"
            isolated.mkdir()
            old_cwd = Path.cwd()
            old_env = os.environ.pop("PEVAL_ROOT", None)
            try:
                os.chdir(isolated)
                with self.assertRaisesRegex(ValueError, "peval-py workspace is not initialized"):
                    resolve_workspace_root()
            finally:
                os.chdir(old_cwd)
                if old_env is not None:
                    os.environ["PEVAL_ROOT"] = old_env

    def test_port_policy_fallback_and_explicit_strict_failure(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            config = ToolConfig(adapter="opencode")
            store = open_workspace_state(str(root))
            handler = make_handler(store, config)
            explicit_blocker = LocalHTTPServer(("127.0.0.1", 0), handler)
            try:
                with self.assertRaises(OSError):
                    bind_server("127.0.0.1", explicit_blocker.server_port, handler)
            finally:
                explicit_blocker.server_close()

            try:
                default_blocker = LocalHTTPServer(("127.0.0.1", DEFAULT_PORT_START), handler)
            except OSError:
                store.close()
                self.skipTest(f"port {DEFAULT_PORT_START} is already in use")

            server = None
            try:
                server = bind_server("127.0.0.1", None, handler)
                self.assertGreater(server.server_port, DEFAULT_PORT_START)
                self.assertLessEqual(server.server_port, DEFAULT_PORT_END)
            finally:
                if server is not None:
                    server.server_close()
                default_blocker.server_close()
                store.close()

    def test_workspace_state_refresh_and_archive_lifecycle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            source = root / "common_session.jsonl"
            shutil.copy(FIXTURES / "common_session.jsonl", source)
            config = ToolConfig(adapter="opencode", workspace_root=str(root))
            analysis_path = write_cached_analysis(
                root,
                agent_id="opencode",
                session_id="common_session",
                summary="Initial cached analysis.",
            )
            markdown_path = write_cached_markdown(
                root,
                agent_id="opencode",
                session_id="common_session",
                markdown="Initial cached markdown.",
            )
            note_path = write_cached_note(
                root,
                agent_id="opencode",
                session_id="common_session",
                markdown="Initial cell note.",
            )
            store = open_workspace_state(str(root))
            try:
                args = serve_args(path=[str(source)])
                loaded = load_serve_inputs(
                    args,
                    parse_adapter_assignments(["opencode"], config.adapter),
                )
                keys = store.upsert_loaded_sources(loaded, config)
                store.refresh_sources(keys, config)

                self.assertEqual(len(keys), 1)
                self.assertTrue((root / "peval-py.toml").is_file())
                self.assertTrue((root / "state.db").is_file())
                config_text = (root / "peval-py.toml").read_text(encoding="utf-8")
                self.assertIn('state_db = "state.db"\n', config_text)

                table_names = [
                    row[0]
                    for row in store.conn.execute(
                        "SELECT name FROM sqlite_master WHERE type = 'table'"
                    ).fetchall()
                ]
                self.assertTrue(table_names)
                self.assertTrue(
                    all(name.startswith("peval_py_") or name == "sqlite_sequence" for name in table_names)
                )
                self.assertEqual(len(store.source_payload()), 1)
                active_report = store.active_report()
                self.assertEqual(len(active_report["trajectory"]), 1)
                source_payload = store.source_payload()[0]
                artifact_dir = root / source_payload["artifact_dir"]
                self.assertEqual(
                    artifact_dir.relative_to(root).parts[:5],
                    ("runs", "default", "opencode", "common_session", "session_t001"),
                )
                agent_artifact_files = {
                    path.relative_to(artifact_dir / "agent").as_posix()
                    for path in (artifact_dir / "agent").rglob("*.json")
                }
                self.assertEqual(
                    agent_artifact_files,
                    {
                        "trajectory.json",
                        "trajectory_meta.json",
                    },
                )
                source_columns = {
                    row["name"]
                    for row in store.conn.execute(
                        "PRAGMA table_info(peval_py_sources)"
                    ).fetchall()
                }
                self.assertIn("artifact_dir", source_columns)
                self.assertIn("artifact_updated_at_ms", source_columns)
                trial_table = store.conn.execute(
                    """
                    SELECT name
                    FROM sqlite_master
                    WHERE type = 'table' AND name = 'peval_py_trials'
                    """
                ).fetchone()
                self.assertIsNone(
                    trial_table,
                    "Trial cell artifact pointers belong on peval_py_sources",
                )
                self.assertEqual(
                    active_report["annotations"]["analysis"][0]["summary"],
                    "Initial cached analysis.",
                )
                self.assertEqual(
                    active_report["annotations"]["analysis"][0]["md_report"],
                    "Initial cached markdown.",
                )
                self.assertEqual(
                    active_report["annotations"]["notes"][0]["markdown"],
                    "Initial cell note.",
                )
                self.assertEqual(
                    active_report["annotations"]["notes"][0]["source_ref"]["relative_path"],
                    note_path.relative_to(root).as_posix(),
                )

                duplicate_keys = store.upsert_loaded_sources(loaded, config)
                self.assertEqual(duplicate_keys, keys)
                self.assertEqual(len(store.source_payload()), 1)

                analysis_path.write_text(
                    json.dumps({"summary": "Updated cached analysis.", "checks": {}}),
                    encoding="utf-8",
                )
                markdown_path.write_text("Updated cached markdown.", encoding="utf-8")
                note_path.write_text("Updated cell note.", encoding="utf-8")
                store.refresh_sources(keys, config)
                refreshed_report = store.active_report()
                refreshed_analysis = refreshed_report["annotations"]["analysis"][0]
                self.assertEqual(refreshed_analysis["summary"], "Updated cached analysis.")
                self.assertEqual(refreshed_analysis["md_report"], "Updated cached markdown.")
                self.assertEqual(
                    refreshed_report["annotations"]["notes"][0]["markdown"],
                    "Updated cell note.",
                )

                store.set_source_active(keys[0], False)
                self.assertEqual(store.active_report()["trajectory"], [])
                self.assertFalse(store.source_payload()[0]["active"])

                store.set_source_active(keys[0], True)
                self.assertEqual(len(store.active_report()["trajectory"]), 1)
                log_count = store.conn.execute(
                    "SELECT COUNT(*) FROM peval_py_refresh_log"
                ).fetchone()[0]
                self.assertGreaterEqual(log_count, 1)

                for index in range(REFRESH_LOG_LIMIT + 5):
                    store.log_refresh(keys[0], "ok", 0, None, index)
                store.conn.commit()
                bounded_count = store.conn.execute(
                    "SELECT COUNT(*) FROM peval_py_refresh_log"
                ).fetchone()[0]
                self.assertEqual(bounded_count, REFRESH_LOG_LIMIT)
            finally:
                store.close()

    def test_active_report_overlays_current_annotations_without_refresh(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            source = root / "common_session.jsonl"
            shutil.copy(FIXTURES / "common_session.jsonl", source)
            config = ToolConfig(adapter="opencode", workspace_root=str(root))
            store = open_workspace_state(str(root))
            try:
                loaded = load_serve_inputs(
                    serve_args(path=[str(source)]),
                    parse_adapter_assignments(["opencode"], config.adapter),
                    config,
                )
                keys = store.import_loaded_sources(loaded, config)
                initial_report = store.active_report()
                self.assertEqual(initial_report["annotations"]["notes"], [])
                self.assertEqual(
                    initial_report["annotations"]["analysis"][0]["status"],
                    "computed",
                )
                self.assertIn(
                    "auto",
                    initial_report["annotations"]["analysis"][0]["analysis_metrics"],
                )

                artifact_dir = root / store.source_payload()[0]["artifact_dir"]
                store.conn.execute(
                    """
                    UPDATE peval_py_sources
                    SET last_status = 'error', last_error = 'source session vanished'
                    WHERE source_key = ?
                    """,
                    (keys[0],),
                )
                store.conn.commit()

                analysis_json = artifact_dir / "analysis.json"
                analysis_json.write_text(
                    json.dumps({"summary": "Live overlay summary."}),
                    encoding="utf-8",
                )
                analysis_md = artifact_dir / "analysis.md"
                analysis_md.write_text("Live overlay analysis.", encoding="utf-8")
                note_md = artifact_dir / "notes.md"
                note_md.write_text("Live overlay note.", encoding="utf-8")

                active_report = store.active_report(config)
                annotations = active_report["annotations"]
                self.assertEqual(
                    annotations["analysis"][0]["summary"],
                    "Live overlay summary.",
                )
                self.assertEqual(
                    annotations["analysis"][0]["md_report"],
                    "Live overlay analysis.",
                )
                self.assertEqual(annotations["analysis"][0]["status"], "cached")
                self.assertEqual(
                    [item["markdown"] for item in annotations["notes"]],
                    ["Live overlay note."],
                )
                self.assertEqual(store.source_payload()[0]["last_status"], "error")

                server = LocalHTTPServer(
                    ("127.0.0.1", 0),
                    make_handler(store, config),
                )
                thread = threading.Thread(target=server.serve_forever, daemon=True)
                thread.start()
                try:
                    status, _, raw_report = request_text(server.server_port, "/api/report")
                    self.assertEqual(status, 200)
                    api_report = json.loads(raw_report)
                    self.assertEqual(
                        api_report["annotations"]["analysis"][0]["md_report"],
                        "Live overlay analysis.",
                    )
                finally:
                    server.shutdown()
                    server.server_close()
                    thread.join(timeout=5)

                analysis_json.unlink()
                analysis_md.unlink()
                note_md.unlink()
                deleted_report = store.active_report(config)
                deleted_annotations = deleted_report["annotations"]
                self.assertEqual(deleted_annotations["notes"], [])
                self.assertEqual(
                    deleted_annotations["analysis"][0]["status"],
                    "computed",
                )
                self.assertNotIn("relative_path", deleted_annotations["analysis"][0])
                self.assertNotIn("relative_paths", deleted_annotations["analysis"][0])

                store.set_source_alias(keys[0], "Readable source")
                payload = store.source_payload()[0]
                self.assertEqual(payload["source_alias"], "Readable source")
                self.assertEqual(payload["trial_key"], "session:t001")
                self.assertEqual(payload["trial_session_id"], "common_session")
                self.assertEqual(
                    payload["last_turn_finished_at_ms"],
                    active_report["trajectory_meta"][0]["finished_at_ms"],
                )
                self.assertEqual(
                    store.active_report(config)["trajectory"][0]["session_id"],
                    "common_session",
                )

                snapshot_report = sample_report(config)
                snapshot_report["trajectory"][0]["trajectory_id"] = "snapshot:one"
                snapshot_report["trajectory"][0]["session_id"] = "snapshot-session"
                snapshot_report["trajectory_meta"][0]["trial_key"] = "snapshot:t001"
                snapshot_report.pop("annotations", None)
                snapshot_keys = store.ingest_upload(
                    "snapshot-report.json",
                    json.dumps(snapshot_report),
                    config,
                )
                snapshot_payload = next(
                    item
                    for item in store.source_payload()
                    if item["source_key"] == snapshot_keys[0]
                )
                self.assertTrue(snapshot_payload["snapshot"])
                snapshot_artifact_dir = root / snapshot_payload["artifact_dir"]
                (snapshot_artifact_dir / "analysis.json").write_text(
                    json.dumps({"summary": "Snapshot live summary."}),
                    encoding="utf-8",
                )
                (snapshot_artifact_dir / "analysis.md").write_text(
                    "Snapshot live analysis.",
                    encoding="utf-8",
                )
                snapshot_active_report = store.active_report(config)
                snapshot_analysis = next(
                    item
                    for item in snapshot_active_report["annotations"]["analysis"]
                    if item["trial_key"] == "snapshot:t001"
                )
                self.assertEqual(snapshot_analysis["status"], "cached")
                self.assertEqual(snapshot_analysis["summary"], "Snapshot live summary.")
                self.assertEqual(snapshot_analysis["md_report"], "Snapshot live analysis.")
            finally:
                store.close()

    def test_report_json_upload_is_non_refreshable_snapshot(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            config = ToolConfig(adapter="opencode")
            report = sample_report(config)
            trial_key = report["trajectory_meta"][0]["trial_key"]
            report["annotations"] = {
                "report_notes": [
                    {"label": "Report note 1", "markdown": "Ignored report note."}
                ],
                "notes": [
                    {
                        "trial_key": trial_key,
                        "source": "cli",
                        "label": "Uploaded note",
                        "markdown": "Uploaded report note.",
                    },
                    {
                        "trial_key": trial_key,
                        "source": "review",
                        "label": "Second note",
                        "markdown": "Second uploaded note.",
                    },
                    {
                        "trial_key": "other",
                        "source": "cli",
                        "label": "Other note",
                        "markdown": "Wrong Trial note.",
                    },
                ],
                "analysis": [
                    {
                        "trial_key": trial_key,
                        "status": "cached",
                        "relative_path": "old/analysis.json",
                        "summary": "Uploaded summary.",
                        "md_report": "Uploaded markdown.",
                        "analysis_status": "reviewed",
                        "subject": {"session_id": "common_session", "trial_key": trial_key},
                        "findings": [
                            {
                                "severity": "high",
                                "title": "Uploaded finding.",
                            }
                        ],
                        "recommendations": ["Preserve uploaded recommendation."],
                        "limitations": ["Preserve uploaded limitation."],
                        "commands": ["peval-py view tr -f json"],
                        "analysis_metrics": {"review_count": 1},
                        "confidence": 0.8,
                    },
                    {
                        "trial_key": trial_key,
                        "status": "cached",
                        "relative_path": "old/analysis-2.json",
                        "summary": "Second summary.",
                        "md_report": "Second markdown.",
                        "findings": [
                            {
                                "severity": "low",
                                "title": "Second finding.",
                            }
                        ],
                    },
                    {
                        "trial_key": "other",
                        "status": "cached",
                        "relative_path": "old/other.json",
                        "summary": "Wrong Trial summary.",
                    }
                ],
            }
            store = open_workspace_state(str(root))
            try:
                keys = store.ingest_upload(
                    "saved-report.json",
                    json.dumps(report),
                    config,
                )
                self.assertEqual(len(keys), 1)
                sources = store.source_payload()
                self.assertEqual(len(sources), 1)
                self.assertEqual(sources[0]["kind"], "snapshot")
                self.assertTrue(sources[0]["snapshot"])
                self.assertFalse(sources[0]["refreshable"])
                artifact_dir = root / sources[0]["artifact_dir"]
                agent_artifact_files = {
                    path.relative_to(artifact_dir / "agent").as_posix()
                    for path in (artifact_dir / "agent").rglob("*.json")
                }
                self.assertEqual(
                    agent_artifact_files,
                    {"trajectory.json", "trajectory_meta.json"},
                )
                note_text = (artifact_dir / "notes.md").read_text(encoding="utf-8")
                self.assertIn("## Uploaded note", note_text)
                self.assertIn("Source: cli", note_text)
                self.assertIn("Uploaded report note.", note_text)
                self.assertIn("## Second note", note_text)
                self.assertIn("Second uploaded note.", note_text)
                self.assertNotIn("Wrong Trial note.", note_text)
                analysis_json = json.loads(
                    (artifact_dir / "analysis.json").read_text(encoding="utf-8")
                )
                self.assertEqual(
                    analysis_json["summary"],
                    "Uploaded summary.\n\nSecond summary.",
                )
                self.assertEqual(
                    [item["summary"] for item in analysis_json["items"]],
                    ["Uploaded summary.", "Second summary."],
                )
                self.assertEqual(analysis_json["status"], "reviewed")
                self.assertEqual(analysis_json["subject"]["trial_key"], trial_key)
                self.assertEqual(
                    [item["title"] for item in analysis_json["findings"]],
                    ["Uploaded finding.", "Second finding."],
                )
                self.assertEqual(
                    analysis_json["recommendations"],
                    ["Preserve uploaded recommendation."],
                )
                self.assertEqual(
                    analysis_json["limitations"],
                    ["Preserve uploaded limitation."],
                )
                self.assertEqual(analysis_json["commands"], ["peval-py view tr -f json"])
                self.assertEqual(analysis_json["metrics"], {"review_count": 1})
                self.assertEqual(analysis_json["confidence"], 0.8)
                analysis_md = (artifact_dir / "analysis.md").read_text(encoding="utf-8")
                self.assertEqual(
                    analysis_md,
                    "Uploaded markdown.\n\nSecond markdown.\n",
                )
                active_report = store.active_report()
                self.assertEqual(len(active_report["trajectory"]), 1)
                self.assertEqual(
                    active_report["annotations"]["notes"][0]["markdown"],
                    note_text,
                )
                self.assertEqual(
                    active_report["annotations"]["analysis"][0]["summary"],
                    "Uploaded summary.\n\nSecond summary.",
                )
                self.assertEqual(
                    active_report["annotations"]["analysis"][0]["analysis_status"],
                    "reviewed",
                )
                self.assertEqual(
                    active_report["annotations"]["analysis"][0]["analysis_metrics"][
                        "review_count"
                    ],
                    1,
                )
                self.assertIn(
                    "auto",
                    active_report["annotations"]["analysis"][0]["analysis_metrics"],
                )
                self.assertEqual(
                    [
                        item["title"]
                        for item in active_report["annotations"]["analysis"][0]["findings"]
                    ],
                    ["Uploaded finding.", "Second finding."],
                )
                self.assertEqual(
                    active_report["annotations"]["analysis"][0]["md_report"],
                    analysis_md,
                )
                self.assertEqual(active_report["annotations"]["report_notes"], [])

                with self.assertRaisesRegex(ValueError, "20 MiB"):
                    store.ingest_upload(
                        "large-report.json",
                        "x" * (UPLOAD_LIMIT_BYTES + 1),
                        config,
                    )
            finally:
                store.close()

    def test_duplicate_cell_import_updates_one_source_and_delete_removes_cell(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            config = ToolConfig(adapter="opencode")
            report = sample_report(config)
            store = open_workspace_state(str(root))
            try:
                key_a = store.ingest_upload("a-report.json", json.dumps(report), config)[0]
                key_b = store.ingest_upload("b-report.json", json.dumps(report), config)[0]
                self.assertEqual(key_a, key_b)
                payload_by_key = {
                    item["source_key"]: item
                    for item in store.source_payload()
                }
                self.assertEqual(len(payload_by_key), 1)
                artifact_a = root / payload_by_key[key_a]["artifact_dir"]
                self.assertTrue(artifact_a.is_dir())

                store.delete_source(key_a)
                self.assertFalse(artifact_a.exists())
                self.assertEqual(store.source_payload(), [])
            finally:
                store.close()

    def test_missing_artifact_source_stays_listed_and_reports_skip_it(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            config = ToolConfig(adapter="opencode")
            report = sample_report(config)
            store = open_workspace_state(str(root))
            try:
                source_key = store.ingest_upload("saved-report.json", json.dumps(report), config)[0]
                source = store.source_payload()[0]
                artifact_dir = root / source["artifact_dir"]
                backup_dir = Path(tmp) / "backup-cell"
                shutil.copytree(artifact_dir, backup_dir)

                shutil.rmtree(artifact_dir)
                missing_source = store.source_payload()[0]
                self.assertEqual(missing_source["source_key"], source_key)
                self.assertEqual(missing_source["last_status"], "missing")
                self.assertIn("Trial cell artifacts not found", missing_source["last_error"])
                self.assertEqual(store.active_report(config)["trajectory"], [])

                shutil.copytree(backup_dir, artifact_dir)
                store.set_source_alias(source_key, "Restored cell")
                first_sync = store.sync_artifact_sources(config)
                second_sync = store.sync_artifact_sources(config)
                self.assertIn(source_key, first_sync)
                self.assertEqual(first_sync, second_sync)
                restored_sources = store.source_payload()
                self.assertEqual(len(restored_sources), 1)
                self.assertEqual(restored_sources[0]["source_key"], source_key)
                self.assertEqual(restored_sources[0]["source_alias"], "Restored cell")
                self.assertNotEqual(restored_sources[0]["last_status"], "missing")
                self.assertEqual(
                    store.active_report(config)["trajectory"][0]["trajectory_id"],
                    report["trajectory"][0]["trajectory_id"],
                )
            finally:
                store.close()

    def test_sync_artifact_sources_discovers_unregistered_trial_cells(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            config = ToolConfig(adapter="opencode")
            report = sample_report(config)
            store = open_workspace_state(str(root))
            try:
                source_key = store.ingest_upload("saved-report.json", json.dumps(report), config)[0]
                artifact_dir = root / store.source_payload()[0]["artifact_dir"]
                store.conn.execute("DELETE FROM peval_py_refresh_log")
                store.conn.execute("DELETE FROM peval_py_sources")
                store.conn.commit()
                self.assertEqual(store.source_payload(), [])

                synced = store.sync_artifact_sources(config)
                self.assertEqual(synced, [source_key])
                sources = store.source_payload()
                self.assertEqual(len(sources), 1)
                self.assertEqual(sources[0]["source_key"], source_key)
                self.assertEqual(sources[0]["kind"], "trial-artifact")
                self.assertFalse(sources[0]["refreshable"])
                self.assertTrue(sources[0]["snapshot"])
                self.assertEqual(sources[0]["trial_key"], "session:t001")

                store.sync_artifact_sources(config)
                self.assertEqual(len(store.source_payload()), 1)
            finally:
                store.close()
