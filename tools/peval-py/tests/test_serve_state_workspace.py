from __future__ import annotations

from serve_state_support import *

DERIVED_SOURCE_STATE_FIELDS = {
    "source_key",
    "kind",
    "adapter",
    "label",
    "input_path",
    "db_path",
    "session_id",
    "agent_name",
    "agent_version",
    "model",
    "artifact_dir",
    "artifact_updated_at_ms",
    "trial_key",
    "trial_session_id",
    "last_turn_finished_at_ms",
    "refreshable",
    "snapshot",
}


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
                self.assertTrue((created / "logs").is_dir())
                self.assertFalse((created / "state.db").exists())
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
                self.assertFalse((root / "state.db").exists())
                config_text = (root / "peval-py.toml").read_text(encoding="utf-8")
                self.assertNotIn("state_db", config_text)
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
                source_state_path = artifact_dir / ".peval" / "state.json"
                self.assertTrue(source_state_path.is_file())
                source_state = json.loads(source_state_path.read_text(encoding="utf-8"))
                self.assertEqual(source_state["schema_version"], 2)
                self.assertTrue(DERIVED_SOURCE_STATE_FIELDS.isdisjoint(source_state))
                self.assertEqual(
                    source_state["source"],
                    {
                        "kind": "path",
                        "adapter": "opencode",
                        "label": "common_session.jsonl",
                        "input_path": str(source.resolve()),
                        "session_id": "common_session",
                        "refreshable": True,
                    },
                )
                self.assertTrue(
                    store.paths.log_path.is_file(),
                    "refresh/import evidence belongs in the workspace JSONL log",
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

                store.close()
                store = open_workspace_state(str(root))
                reopened_source = store.source_payload()[0]
                self.assertEqual(reopened_source["source_key"], keys[0])
                self.assertTrue(reopened_source["refreshable"])
                store.refresh_sources(keys, config)
                self.assertEqual(
                    store.source_payload()[0]["source_key"],
                    keys[0],
                )

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
                store.refresh_sources(keys, config)
                self.assertEqual(store.active_report()["trajectory"], [])
                self.assertFalse(store.source_payload()[0]["active"])

                store.set_source_active(keys[0], True)
                self.assertEqual(len(store.active_report()["trajectory"]), 1)
                log_lines = store.paths.log_path.read_text(encoding="utf-8").splitlines()
                self.assertGreaterEqual(len(log_lines), 1)

                for index in range(REFRESH_LOG_LIMIT + 5):
                    store.log_refresh(keys[0], "ok", 0, None, index)
                appended_lines = store.paths.log_path.read_text(encoding="utf-8").splitlines()
                self.assertGreaterEqual(len(appended_lines), REFRESH_LOG_LIMIT + 5)
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
                store.update_source_status(
                    store.source_payload()[0],
                    "error",
                    "source session vanished",
                    1,
                )

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
                store.set_source_tags(keys[0], ["triage", "fast"])
                payload = store.source_payload()[0]
                self.assertEqual(payload["source_alias"], "Readable source")
                self.assertEqual(payload["source_tags"], ["triage", "fast"])
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
                self.assertEqual(
                    store.active_report(config)["trajectory_meta"][0]["source_tags"],
                    ["triage", "fast"],
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

    def test_v1_flat_source_state_is_ignored_and_rewritten_as_v2_overlay(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            cell_dir = (
                root
                / "runs"
                / "default"
                / "psychevo"
                / "legacy-session"
                / "session_t001"
            )
            write_trial_cell_artifacts(cell_dir, session_id="legacy-session")
            state_path = cell_dir / ".peval" / "state.json"
            state_path.parent.mkdir(parents=True)
            state_path.write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "source_key": "legacy-source-key",
                        "kind": "trial-artifact",
                        "adapter": "psychevo",
                        "label": "legacy label",
                        "input_path": str(cell_dir),
                        "db_path": None,
                        "session_id": "legacy-session",
                        "source_alias": "Legacy alias",
                        "agent_name": "psychevo",
                        "model": "legacy-model",
                        "artifact_dir": "runs/default/psychevo/legacy-session/session_t001",
                        "artifact_updated_at_ms": 10,
                        "trial_key": "session:t001",
                        "trial_session_id": "legacy-session",
                        "last_turn_finished_at_ms": 1200,
                        "refreshable": False,
                        "active": False,
                        "snapshot": True,
                        "created_at_ms": 100,
                        "updated_at_ms": 200,
                        "last_status": "error",
                        "last_error": "legacy error",
                    }
                ),
                encoding="utf-8",
            )

            store = open_workspace_state(str(root))
            try:
                source = store.source_payload()[0]
                self.assertIsNone(source["source_alias"])
                self.assertTrue(source["active"])
                self.assertEqual(source["last_status"], "ok")
                self.assertIsNone(source["last_error"])

                store.set_source_alias(source["source_key"], "Rewritten alias")
                rewritten = json.loads(state_path.read_text(encoding="utf-8"))
                self.assertEqual(rewritten["schema_version"], 2)
                self.assertEqual(rewritten["source_alias"], "Rewritten alias")
                self.assertNotIn("active", rewritten)
                self.assertNotIn("last_status", rewritten)
                self.assertNotIn("last_error", rewritten)
                self.assertTrue(DERIVED_SOURCE_STATE_FIELDS.isdisjoint(rewritten))
                self.assertNotIn("source", rewritten)
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
                source_state = json.loads(
                    (artifact_dir / ".peval" / "state.json").read_text(encoding="utf-8")
                )
                self.assertEqual(source_state["schema_version"], 2)
                self.assertTrue(DERIVED_SOURCE_STATE_FIELDS.isdisjoint(source_state))
                self.assertEqual(source_state["source"]["kind"], "snapshot")
                self.assertNotIn("refreshable", source_state["source"])
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

                shutil.rmtree(artifact_dir / "agent")
                missing_source = store.source_payload()[0]
                self.assertEqual(missing_source["source_key"], source_key)
                self.assertEqual(missing_source["last_status"], "missing")
                self.assertIn("Trial cell artifacts not found", missing_source["last_error"])
                self.assertEqual(store.active_report(config)["trajectory"], [])

                shutil.copytree(backup_dir / "agent", artifact_dir / "agent")
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
                shutil.rmtree(artifact_dir / ".peval")
                self.assertFalse((artifact_dir / ".peval" / "state.json").exists())
                self.assertEqual(len(store.source_payload()), 1)

                synced = store.sync_artifact_sources(config)
                self.assertEqual(synced, [source_key])
                sources = store.source_payload()
                self.assertEqual(len(sources), 1)
                self.assertEqual(sources[0]["source_key"], source_key)
                self.assertEqual(sources[0]["kind"], "trial-artifact")
                self.assertFalse(sources[0]["refreshable"])
                self.assertTrue(sources[0]["snapshot"])
                self.assertEqual(sources[0]["trial_key"], "session:t001")
                self.assertTrue((artifact_dir / ".peval" / "state.json").is_file())
                source_state = json.loads(
                    (artifact_dir / ".peval" / "state.json").read_text(encoding="utf-8")
                )
                self.assertEqual(source_state["schema_version"], 2)
                self.assertTrue(DERIVED_SOURCE_STATE_FIELDS.isdisjoint(source_state))
                self.assertNotIn("source", source_state)

                store.sync_artifact_sources(config)
                self.assertEqual(len(store.source_payload()), 1)
            finally:
                store.close()

    def test_external_runs_import_copies_trial_cells_and_sidecars(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp) / "workspace")
            external = peval_py_workspace(Path(tmp) / "external")
            first_cell = (
                external
                / "runs"
                / "external-eval"
                / "psychevo"
                / "session-a"
                / "session_t001"
            )
            second_cell = (
                external
                / "runs"
                / "external-eval"
                / "psychevo"
                / "session-b"
                / "session_t002"
            )
            write_trial_cell_artifacts(
                first_cell,
                session_id="session-a",
                trial_key="session_t001",
            )
            write_trial_cell_artifacts(
                second_cell,
                session_id="session-b",
                trial_key="session_t002",
            )
            (first_cell / "analysis.json").write_text(
                json.dumps({"summary": "external analysis"}),
                encoding="utf-8",
            )
            (first_cell / "analysis.md").write_text(
                "External analysis markdown.",
                encoding="utf-8",
            )
            (first_cell / "notes.md").write_text(
                "External note.",
                encoding="utf-8",
            )
            config = ToolConfig(adapter="opencode", workspace_root=str(root))
            store = open_workspace_state(str(root))
            try:
                cells = discover_complete_trial_cell_dirs(external)
                self.assertEqual(cells, [first_cell.resolve(), second_cell.resolve()])
                loaded = LoadedInputs(
                    sessions=[
                        loaded_trial_cell_import_session(cell, config)
                        for cell in cells
                    ],
                    notes=[],
                )
                keys = store.import_loaded_sources(loaded, config)
                self.assertEqual(len(keys), 2)

                sources = store.source_payload()
                self.assertEqual(
                    [source["trial_session_id"] for source in sources],
                    ["session-a", "session-b"],
                )
                first_source = sources[0]
                first_artifact = root / first_source["artifact_dir"]
                self.assertEqual(first_source["kind"], "trial-artifact")
                self.assertTrue(first_source["snapshot"])
                self.assertFalse(first_source["refreshable"])
                self.assertIn("runs/external-eval/psychevo/session-a/session_t001", first_source["artifact_dir"])
                self.assertEqual(
                    json.loads((first_artifact / "analysis.json").read_text(encoding="utf-8")),
                    {"summary": "external analysis"},
                )
                self.assertEqual(
                    (first_artifact / "analysis.md").read_text(encoding="utf-8"),
                    "External analysis markdown.",
                )
                self.assertEqual(
                    (first_artifact / "notes.md").read_text(encoding="utf-8"),
                    "External note.",
                )

                store.delete_source(first_source["source_key"])
                self.assertFalse(first_artifact.exists())
                self.assertTrue((first_cell / "agent" / "trajectory.json").is_file())
                self.assertTrue((first_cell / "notes.md").is_file())
                remaining = store.source_payload()
                self.assertEqual(len(remaining), 1)
                self.assertEqual(remaining[0]["trial_session_id"], "session-b")
            finally:
                store.close()
