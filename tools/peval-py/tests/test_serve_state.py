from __future__ import annotations

import http.client
import os
import threading

from peval_py_test_support import *

from peval_py.inputs import parse_adapter_assignments
from peval_py.serve import (
    DEFAULT_PORT_END,
    DEFAULT_PORT_START,
    ECHARTS_ASSET_PATH,
    HttpError,
    LocalHTTPServer,
    bind_server,
    cached_echarts_asset,
    echarts_cache_path,
    make_handler,
    source_path_values,
    workspace_relative_path,
)
from peval_py.state import (
    REFRESH_LOG_LIMIT,
    UPLOAD_LIMIT_BYTES,
    load_serve_inputs,
    open_workspace_state,
    resolve_workspace_root,
)


class PevalPyServeStateTests(unittest.TestCase):
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
                self.assertEqual(
                    (root / "peval-py.toml").read_text(encoding="utf-8"),
                    'state_db = "state.db"\n',
                )

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
                self.assertNotIn("annotations", store.active_report())

                stored_report = json.loads(
                    store.conn.execute(
                        "SELECT report_json FROM peval_py_trials WHERE source_key = ?",
                        (keys[0],),
                    ).fetchone()[0]
                )
                stored_report["annotations"] = {
                    "report_notes": [],
                    "notes": [
                        {
                            "trial_key": stored_report["trajectory_meta"][0]["trial_key"],
                            "source": "cli",
                            "label": "CLI note 1",
                            "markdown": "Stored CLI note.",
                        }
                    ],
                }
                store.conn.execute(
                    "UPDATE peval_py_trials SET report_json = ? WHERE source_key = ?",
                    (json.dumps(stored_report), keys[0]),
                )
                store.conn.execute(
                    """
                    UPDATE peval_py_sources
                    SET last_status = 'error', last_error = 'source session vanished'
                    WHERE source_key = ?
                    """,
                    (keys[0],),
                )
                store.conn.commit()

                write_cached_markdown(
                    root,
                    agent_id="opencode",
                    session_id="common_session",
                    markdown="Live overlay analysis.",
                )
                write_cached_note(
                    root,
                    agent_id="opencode",
                    session_id="common_session",
                    markdown="Live overlay note.",
                )

                active_report = store.active_report(config)
                annotations = active_report["annotations"]
                self.assertEqual(
                    annotations["analysis"][0]["md_report"],
                    "Live overlay analysis.",
                )
                self.assertEqual(
                    [item["markdown"] for item in annotations["notes"]],
                    ["Live overlay note.", "Stored CLI note."],
                )
                self.assertEqual(store.source_payload()[0]["last_status"], "error")

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
            finally:
                store.close()

    def test_report_json_upload_is_non_refreshable_snapshot(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            config = ToolConfig(adapter="opencode")
            report = sample_report(config)
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
                self.assertEqual(len(store.active_report()["trajectory"]), 1)

                with self.assertRaisesRegex(ValueError, "20 MiB"):
                    store.ingest_upload(
                        "large-report.json",
                        "x" * (UPLOAD_LIMIT_BYTES + 1),
                        config,
                    )
            finally:
                store.close()

    def test_http_upload_report_json_and_same_origin_rejection(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            config = ToolConfig(adapter="opencode")
            report = sample_report(config)
            store = open_workspace_state(str(root))
            server = LocalHTTPServer(
                ("127.0.0.1", 0),
                make_handler(store, config),
            )
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            port = server.server_port
            try:
                status, headers, body = request_json(
                    port,
                    "POST",
                    "/api/upload",
                    {
                        "filename": "report.json",
                        "content": json.dumps(report),
                    },
                    origin="http://example.test",
                )
                self.assertEqual(status, 403)
                self.assertNotIn("access-control-allow-origin", headers)
                self.assertIn("same-origin", body["error"])

                status, headers, body = request_json(
                    port,
                    "POST",
                    "/api/upload",
                    {
                        "filename": "report.json",
                        "content": json.dumps(report),
                    },
                    origin=f"http://127.0.0.1:{port}",
                )
                self.assertEqual(status, 200)
                self.assertNotIn("access-control-allow-origin", headers)
                self.assertEqual(len(body["sources"]), 1)
                self.assertEqual(body["sources"][0]["kind"], "snapshot")
                self.assertEqual(len(body["report"]["trajectory"]), 1)

                conn = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
                conn.request("GET", "/api/report")
                response = conn.getresponse()
                payload = json.loads(response.read().decode("utf-8"))
                conn.close()
                self.assertEqual(response.status, 200)
                self.assertEqual(len(payload["trajectory"]), 1)
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=5)
                store.close()

    def test_http_echarts_asset_uses_workspace_cache_and_fake_download(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            config = ToolConfig(adapter="opencode")
            store = open_workspace_state(str(root))
            try:
                cache_path = echarts_cache_path(store)
                self.assertEqual(
                    cache_path,
                    root / ".cache" / "echarts" / "6.0.0" / "echarts.min.js",
                )
                cache_path.parent.mkdir(parents=True)
                cache_path.write_bytes(b"console.log('cached');")
                self.assertEqual(cached_echarts_asset(store), b"console.log('cached');")

                cache_path.unlink()
                with patch("peval_py.serve.download_echarts_asset", return_value=b"console.log('downloaded');"):
                    self.assertEqual(cached_echarts_asset(store), b"console.log('downloaded');")
                self.assertEqual(cache_path.read_bytes(), b"console.log('downloaded');")

                cache_path.unlink()
                with patch("peval_py.serve.download_echarts_asset", side_effect=RuntimeError("network down")):
                    with self.assertRaisesRegex(HttpError, "failed to cache ECharts"):
                        cached_echarts_asset(store)
            finally:
                store.close()

        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            config = ToolConfig(adapter="opencode")
            store = open_workspace_state(str(root))
            cache_path = echarts_cache_path(store)
            cache_path.parent.mkdir(parents=True)
            cache_path.write_bytes(b"window.echarts={};")
            server = LocalHTTPServer(
                ("127.0.0.1", 0),
                make_handler(store, config),
            )
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            port = server.server_port
            try:
                status, headers, body = request_bytes(port, ECHARTS_ASSET_PATH)
                self.assertEqual(status, 200)
                self.assertIn("application/javascript", headers["content-type"])
                self.assertEqual(body, b"window.echarts={};")

                cache_path.unlink()
                with patch("peval_py.serve.download_echarts_asset", side_effect=RuntimeError("network down")):
                    status, _, body = request_bytes(port, ECHARTS_ASSET_PATH)
                self.assertEqual(status, 502)
                self.assertIn(b"failed to cache ECharts", body)
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=5)
                store.close()

    def test_http_source_alias_is_display_only_and_editable(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            source = root / "common_session.jsonl"
            shutil.copy(FIXTURES / "common_session.jsonl", source)
            config = ToolConfig(adapter="opencode")
            store = open_workspace_state(str(root))
            server = LocalHTTPServer(
                ("127.0.0.1", 0),
                make_handler(store, config),
            )
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            port = server.server_port
            origin = f"http://127.0.0.1:{port}"
            try:
                status, _, body = request_json(
                    port,
                    "POST",
                    "/api/sources",
                    {
                        "path": "common_session.jsonl",
                        "adapter": "opencode",
                        "alias": "Readable source",
                    },
                    origin=origin,
                )
                self.assertEqual(status, 200)
                source_key = body["sources"][0]["source_key"]
                self.assertEqual(body["sources"][0]["source_alias"], "Readable source")
                self.assertEqual(
                    body["report"]["trajectory_meta"][0]["source_alias"],
                    "Readable source",
                )
                self.assertEqual(
                    body["report"]["trajectory"][0]["session_id"],
                    "common_session",
                )
                self.assertEqual(
                    body["report"]["trajectory_meta"][0]["data_ref"]["label"],
                    "common_session.jsonl",
                )

                status, _, body = request_json(
                    port,
                    "POST",
                    f"/api/sources/{source_key}/alias",
                    {"alias": "Renamed source"},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(body["sources"][0]["source_key"], source_key)
                self.assertEqual(body["sources"][0]["source_alias"], "Renamed source")
                self.assertEqual(
                    body["report"]["trajectory_meta"][0]["source_alias"],
                    "Renamed source",
                )

                status, _, body = request_json(
                    port,
                    "POST",
                    "/api/sources",
                    {"path": "common_session.jsonl", "adapter": "opencode"},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(body["sources"][0]["source_key"], source_key)
                self.assertEqual(body["sources"][0]["source_alias"], "Renamed source")

                status, _, body = request_json(
                    port,
                    "POST",
                    f"/api/sources/{source_key}/alias",
                    {"alias": ""},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(body["sources"][0]["source_key"], source_key)
                self.assertIsNone(body["sources"][0]["source_alias"])
                self.assertNotIn("source_alias", body["report"]["trajectory_meta"][0])
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=5)
                store.close()

    def test_http_locale_endpoint_writes_workspace_config_and_updates_rendering(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            config = ToolConfig(adapter="opencode", locale="en")
            store = open_workspace_state(str(root))
            server = LocalHTTPServer(
                ("127.0.0.1", 0),
                make_handler(store, config),
            )
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            port = server.server_port
            origin = f"http://127.0.0.1:{port}"
            try:
                status, _, rejected = request_json(
                    port,
                    "POST",
                    "/api/config/locale",
                    {"locale": "zh"},
                    origin="http://example.test",
                )
                self.assertEqual(status, 403)
                self.assertIn("same-origin", rejected["error"])

                status, _, body = request_json(
                    port,
                    "POST",
                    "/api/config/locale",
                    {"locale": "zh"},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(body, {"locale": "zh-CN"})
                config_text = (root / "peval-py.toml").read_text(encoding="utf-8")
                self.assertIn('locale = "zh-CN"\n', config_text)

                status, _, html = request_text(port, "/")
                self.assertEqual(status, 200)
                self.assertIn('<html lang="zh-CN">', html)
                self.assertIn("<h1>Agent 轨迹报告</h1>", html)

                status, _, body = request_json(
                    port,
                    "POST",
                    "/api/config/locale",
                    {"locale": "en-US"},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(body, {"locale": "en"})
                self.assertIn(
                    'locale = "en"\n',
                    (root / "peval-py.toml").read_text(encoding="utf-8"),
                )
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=5)
                store.close()

    def test_http_sources_batch_path_quotes_failure_and_delete(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            source_a = root / "common one.jsonl"
            source_b = root / "common_two.jsonl"
            shutil.copy(FIXTURES / "common_session.jsonl", source_a)
            shutil.copy(FIXTURES / "common_session.jsonl", source_b)
            config = ToolConfig(adapter="opencode")
            store = open_workspace_state(str(root))
            server = LocalHTTPServer(
                ("127.0.0.1", 0),
                make_handler(store, config),
            )
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            port = server.server_port
            origin = f"http://127.0.0.1:{port}"
            try:
                status, _, body = request_json(
                    port,
                    "POST",
                    "/api/sources",
                    {"path": '"common one.jsonl" common_two.jsonl', "adapter": "auto"},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(len(body["sources"]), 2)
                self.assertEqual(len(body["report"]["trajectory"]), 2)
                source_keys = [source["source_key"] for source in body["sources"]]
                log_count = store.conn.execute(
                    "SELECT COUNT(*) FROM peval_py_refresh_log"
                ).fetchone()[0]

                status, _, failed = request_json(
                    port,
                    "POST",
                    "/api/sources",
                    {"path": '"common one.jsonl" missing.jsonl', "adapter": "auto"},
                    origin=origin,
                )
                self.assertEqual(status, 400)
                self.assertIn("missing.jsonl", failed["error"])
                self.assertEqual(len(store.source_payload()), 2)
                self.assertEqual(
                    store.conn.execute(
                        "SELECT COUNT(*) FROM peval_py_refresh_log"
                    ).fetchone()[0],
                    log_count,
                )

                status, _, malformed = request_json(
                    port,
                    "POST",
                    "/api/sources",
                    {"path": '"unterminated', "adapter": "auto"},
                    origin=origin,
                )
                self.assertEqual(status, 400)
                self.assertIn("path list is invalid", malformed["error"])
                self.assertEqual(len(store.source_payload()), 2)

                status, _, body = request_json(
                    port,
                    "POST",
                    f"/api/sources/{source_keys[0]}/delete",
                    {},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(len(body["sources"]), 1)
                self.assertTrue(source_a.exists())
                self.assertEqual(
                    store.conn.execute(
                        "SELECT COUNT(*) FROM peval_py_trials WHERE source_key = ?",
                        (source_keys[0],),
                    ).fetchone()[0],
                    0,
                )
                self.assertEqual(
                    store.conn.execute(
                        "SELECT COUNT(*) FROM peval_py_refresh_log WHERE source_key = ?",
                        (source_keys[0],),
                    ).fetchone()[0],
                    0,
                )

                status, _, rejected = request_json(
                    port,
                    "POST",
                    f"/api/sources/{source_keys[1]}/delete",
                    {},
                    origin="http://example.test",
                )
                self.assertEqual(status, 403)
                self.assertIn("same-origin", rejected["error"])
                self.assertEqual(len(store.source_payload()), 1)
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=5)
                store.close()

    def test_http_source_notes_save_refreshes_snapshot_and_validates_source(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            source = root / "common_session.jsonl"
            shutil.copy(FIXTURES / "common_session.jsonl", source)
            note_path = write_cached_note(
                root,
                agent_id="opencode",
                session_id="common_session",
                markdown="Initial HTTP note.",
            )
            config = ToolConfig(adapter="opencode", workspace_root=str(root))
            store = open_workspace_state(str(root))
            server = LocalHTTPServer(
                ("127.0.0.1", 0),
                make_handler(store, config),
            )
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            port = server.server_port
            origin = f"http://127.0.0.1:{port}"
            try:
                status, _, body = request_json(
                    port,
                    "POST",
                    "/api/sources",
                    {"path": "common_session.jsonl", "adapter": "opencode"},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                source_key = body["sources"][0]["source_key"]
                self.assertEqual(
                    body["report"]["annotations"]["notes"][0]["markdown"],
                    "Initial HTTP note.",
                )

                status, _, body = request_json(
                    port,
                    "POST",
                    f"/api/sources/{source_key}/notes",
                    {"markdown": "Updated HTTP note."},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(note_path.read_text(encoding="utf-8"), "Updated HTTP note.")
                self.assertEqual(
                    body["report"]["annotations"]["notes"][0]["markdown"],
                    "Updated HTTP note.",
                )

                note_path.unlink()
                analysis_path = write_cached_markdown(
                    root,
                    agent_id="opencode",
                    session_id="common_session",
                    markdown="Analysis-only cell.",
                )
                status, _, body = request_json(
                    port,
                    "POST",
                    f"/api/sources/{source_key}/notes",
                    {"markdown": "Saved beside analysis."},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(
                    (analysis_path.parent / "notes.md").read_text(encoding="utf-8"),
                    "Saved beside analysis.",
                )
                self.assertEqual(
                    body["report"]["annotations"]["notes"][0]["markdown"],
                    "Saved beside analysis.",
                )

                status, _, rejected = request_json(
                    port,
                    "POST",
                    f"/api/sources/{source_key}/notes",
                    {"markdown": "bad origin"},
                    origin="http://example.test",
                )
                self.assertEqual(status, 403)
                self.assertIn("same-origin", rejected["error"])

                status, _, too_large = request_json(
                    port,
                    "POST",
                    f"/api/sources/{source_key}/notes",
                    {"markdown": "x" * (1024 * 1024 + 1)},
                    origin=origin,
                )
                self.assertEqual(status, 400)
                self.assertIn("1 MiB", too_large["error"])

                snapshot_keys = store.ingest_upload(
                    "saved-report.json",
                    json.dumps(sample_report(config)),
                    config,
                )
                status, _, snapshot_error = request_json(
                    port,
                    "POST",
                    f"/api/sources/{snapshot_keys[0]}/notes",
                    {"markdown": "snapshot note"},
                    origin=origin,
                )
                self.assertEqual(status, 400)
                self.assertIn("refreshable", snapshot_error["error"])
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=5)
                store.close()

        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            source = root / "common_session.jsonl"
            shutil.copy(FIXTURES / "common_session.jsonl", source)
            config = ToolConfig(adapter="opencode", workspace_root=str(root))
            store = open_workspace_state(str(root))
            try:
                args = serve_args(path=[str(source)])
                loaded = load_serve_inputs(
                    args,
                    parse_adapter_assignments(["opencode"], config.adapter),
                )
                keys = store.upsert_loaded_sources(loaded, config)
                store.refresh_sources(keys, config)
                store.save_source_notes(keys[0], "", config)
                created = (
                    root
                    / "runs"
                    / "default"
                    / "opencode"
                    / "common_session"
                    / "peval-py-notes"
                    / "notes.md"
                )
                self.assertTrue(created.is_file())
                self.assertEqual(created.read_text(encoding="utf-8"), "")
                self.assertEqual(
                    store.active_report()["annotations"]["notes"][0]["source_ref"]["relative_path"],
                    created.relative_to(root).as_posix(),
                )
            finally:
                store.close()

    def test_source_notes_save_rejects_ambiguous_cells(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            source = root / "common_session.jsonl"
            shutil.copy(FIXTURES / "common_session.jsonl", source)
            config = ToolConfig(adapter="opencode", workspace_root=str(root))
            write_cached_note(
                root,
                agent_id="opencode",
                session_id="common_session",
                cell_key="one",
                markdown="one",
            )
            write_cached_note(
                root,
                agent_id="opencode",
                session_id="common_session",
                cell_key="two",
                markdown="two",
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
                with self.assertRaisesRegex(ValueError, "multiple notes cells"):
                    store.save_source_notes(keys[0], "cannot pick", config)
            finally:
                store.close()

        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            source = root / "common_session.jsonl"
            shutil.copy(FIXTURES / "common_session.jsonl", source)
            config = ToolConfig(adapter="opencode", workspace_root=str(root))
            write_cached_markdown(
                root,
                agent_id="opencode",
                session_id="common_session",
                cell_key="one",
                markdown="one",
            )
            write_cached_markdown(
                root,
                agent_id="opencode",
                session_id="common_session",
                cell_key="two",
                markdown="two",
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
                with self.assertRaisesRegex(ValueError, "multiple analysis cells"):
                    store.save_source_notes(keys[0], "cannot pick", config)
            finally:
                store.close()

    def test_source_path_values_preserve_windows_paths_and_wsl_fallback(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            store = open_workspace_state(str(root))
            try:
                values = source_path_values(
                    store,
                    {
                        "path": (
                            r"C:\Users\kevin\AppData\Local\state.db "
                            r'"D:\Data Dir\session.jsonl" '
                            r"C:/Users/kevin/.hermes/state.db "
                            r"\\server\share\state.db "
                            "relative.jsonl"
                        )
                    },
                    "path",
                )
                self.assertEqual(
                    values,
                    [
                        r"C:\Users\kevin\AppData\Local\state.db",
                        r"D:\Data Dir\session.jsonl",
                        "C:/Users/kevin/.hermes/state.db",
                        r"\\server\share\state.db",
                        str(root / "relative.jsonl"),
                    ],
                )

                mount_root = root / "mnt"
                mapped = mount_root / "c" / "Users" / "kevin" / ".hermes" / "state.db"
                mapped.parent.mkdir(parents=True)
                mapped.write_text("", encoding="utf-8")
                self.assertEqual(
                    workspace_relative_path(
                        store,
                        r"C:\Users\kevin\.hermes\state.db",
                        windows_mount_root=mount_root,
                    ),
                    str(mapped),
                )
                self.assertEqual(
                    workspace_relative_path(
                        store,
                        "C:/Users/kevin/.hermes/state.db",
                        windows_mount_root=mount_root,
                    ),
                    str(mapped),
                )
            finally:
                store.close()

        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            store = open_workspace_state(str(root))
            try:
                with self.assertRaisesRegex(HttpError, "path list is invalid"):
                    source_path_values(store, {"path": '"unterminated'}, "path")
            finally:
                store.close()

    def test_http_db_session_inspect_infers_adapters_and_batch_adds_sources(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            hermes_dir = root / ".hermes"
            psychevo_dir = root / ".psychevo"
            opencode_dir = root / ".opencode"
            hermes_dir.mkdir()
            psychevo_dir.mkdir()
            opencode_dir.mkdir()
            hermes_db = hermes_dir / "state.db"
            psychevo_db = psychevo_dir / "state.db"
            opencode_db = opencode_dir / "opencode.db"
            create_hermes_db(hermes_db)
            create_messages_db(psychevo_db)
            create_opencode_db(opencode_db)

            config = ToolConfig(adapter="opencode")
            store = open_workspace_state(str(root))
            server = LocalHTTPServer(
                ("127.0.0.1", 0),
                make_handler(store, config),
            )
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            port = server.server_port
            origin = f"http://127.0.0.1:{port}"
            try:
                status, _, body = request_json(
                    port,
                    "POST",
                    "/api/db-sessions",
                    {"db": ".hermes/state.db"},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(body["adapter"], "hermes")
                self.assertTrue(body["inferred"])
                self.assertEqual(body["sessions"][0]["session_id"], "hermes-latest")
                self.assertEqual(body["sessions"][0]["index"], 1)

                status, _, body = request_json(
                    port,
                    "POST",
                    "/api/db-sessions",
                    {"db": ".psychevo/state.db"},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(body["adapter"], "psychevo")
                self.assertEqual(body["sessions"][0]["session_id"], "db-b")

                status, _, body = request_json(
                    port,
                    "POST",
                    "/api/db-sessions",
                    {"db": ".opencode/opencode.db"},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(body["adapter"], "opencode")
                self.assertEqual(body["sessions"][0]["session_id"], "ses-latest")

                status, _, body = request_json(
                    port,
                    "POST",
                    "/api/sources",
                    {
                        "db": ".hermes/state.db",
                        "adapter": "hermes",
                        "session_ids": ["hermes-latest", "hermes-old"],
                    },
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(len(body["sources"]), 2)
                self.assertEqual(
                    {
                        source["session_id"]
                        for source in body["sources"]
                    },
                    {"hermes-latest", "hermes-old"},
                )
                self.assertEqual(
                    {
                        trajectory["session_id"]
                        for trajectory in body["report"]["trajectory"]
                    },
                    {"hermes-latest", "hermes-old"},
                )
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=5)
                store.close()

    def test_http_sources_and_db_inspect_use_windows_path_wsl_fallback(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            mount_root = root / "mnt"
            mapped_db = mount_root / "c" / "Users" / "kevin" / ".hermes" / "state.db"
            mapped_path = mount_root / "d" / "sessions" / "common.jsonl"
            mapped_db.parent.mkdir(parents=True)
            mapped_path.parent.mkdir(parents=True)
            create_hermes_db(mapped_db)
            shutil.copy(FIXTURES / "common_session.jsonl", mapped_path)

            config = ToolConfig(adapter="opencode")
            store = open_workspace_state(str(root))
            server = LocalHTTPServer(
                ("127.0.0.1", 0),
                make_handler(store, config),
            )
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            port = server.server_port
            origin = f"http://127.0.0.1:{port}"
            try:
                with patch("peval_py.serve.WINDOWS_DRIVE_MOUNT_ROOT", mount_root):
                    status, _, body = request_json(
                        port,
                        "POST",
                        "/api/db-sessions",
                        {"db": r"C:\Users\kevin\.hermes\state.db"},
                        origin=origin,
                    )
                    self.assertEqual(status, 200)
                    self.assertEqual(body["db"], str(mapped_db))
                    self.assertEqual(body["adapter"], "hermes")
                    self.assertEqual(body["sessions"][0]["session_id"], "hermes-latest")

                    status, _, body = request_json(
                        port,
                        "POST",
                        "/api/sources",
                        {
                            "db": r"C:\Users\kevin\.hermes\state.db",
                            "adapter": "hermes",
                            "session_ids": ["hermes-latest"],
                        },
                        origin=origin,
                    )
                    self.assertEqual(status, 200)
                    self.assertEqual(body["sources"][0]["db_path"], str(mapped_db))
                    self.assertEqual(body["sources"][0]["session_id"], "hermes-latest")

                    status, _, body = request_json(
                        port,
                        "POST",
                        "/api/sources",
                        {
                            "path": r"D:\sessions\common.jsonl",
                            "adapter": "opencode",
                        },
                        origin=origin,
                    )
                    self.assertEqual(status, 200)
                    self.assertIn(
                        str(mapped_path),
                        {source["input_path"] for source in body["sources"]},
                    )
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=5)
                store.close()

    def test_http_db_session_inspect_errors_are_clear(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            data_dir = root / "data"
            ambiguous_dir = root / "hermes" / "opencode"
            custom_dir = root / "custom"
            data_dir.mkdir()
            ambiguous_dir.mkdir(parents=True)
            custom_dir.mkdir()
            data_db = data_dir / "state.db"
            ambiguous_db = ambiguous_dir / "state.db"
            custom_db = custom_dir / "state.db"
            create_hermes_db(data_db)
            create_hermes_db(custom_db)
            ambiguous_db.write_text("", encoding="utf-8")

            config = ToolConfig(adapter="opencode")
            store = open_workspace_state(str(root))
            server = LocalHTTPServer(
                ("127.0.0.1", 0),
                make_handler(store, config),
            )
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            port = server.server_port
            origin = f"http://127.0.0.1:{port}"
            try:
                status, _, body = request_json(
                    port,
                    "POST",
                    "/api/db-sessions",
                    {"db": "data/state.db"},
                    origin=origin,
                )
                self.assertEqual(status, 400)
                self.assertIn("could not infer adapter", body["error"])
                self.assertIn("available adapters", body["error"])

                status, _, body = request_json(
                    port,
                    "POST",
                    "/api/db-sessions",
                    {"db": "data/state.db", "adapter": "hermes"},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertFalse(body["inferred"])
                self.assertEqual(body["adapter"], "hermes")

                status, _, body = request_json(
                    port,
                    "POST",
                    "/api/db-sessions",
                    {"db": "hermes/opencode/state.db"},
                    origin=origin,
                )
                self.assertEqual(status, 400)
                self.assertIn("ambiguous adapter inference", body["error"])

                missing_db = root / "missing" / "state.db"
                status, _, body = request_json(
                    port,
                    "POST",
                    "/api/db-sessions",
                    {"db": "missing/state.db"},
                    origin=origin,
                )
                self.assertEqual(status, 400)
                self.assertIn("DB path does not exist", body["error"])
                self.assertFalse(missing_db.exists())

                custom_entry = FakeEntryPoint("custom", CustomPathAdapter)
                with patch(
                    "peval_py.adapters.entry_points",
                    return_value=FakeEntryPoints([custom_entry]),
                ):
                    status, _, body = request_json(
                        port,
                        "POST",
                        "/api/db-sessions",
                        {"db": "custom/state.db", "adapter": "custom"},
                        origin=origin,
                    )
                self.assertEqual(status, 400)
                self.assertIn("does not support session listing", body["error"])

                status, headers, body = request_json(
                    port,
                    "POST",
                    "/api/db-sessions",
                    {"db": "data/state.db", "adapter": "hermes"},
                    origin="http://example.test",
                )
                self.assertEqual(status, 403)
                self.assertNotIn("access-control-allow-origin", headers)
                self.assertIn("same-origin", body["error"])
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=5)
                store.close()


def peval_py_workspace(root: Path) -> Path:
    root.mkdir(parents=True, exist_ok=True)
    (root / "peval-py.toml").write_text('state_db = "state.db"\n', encoding="utf-8")
    return root


def write_cached_analysis(
    root: Path,
    *,
    agent_id: str,
    session_id: str,
    summary: str,
    eval_slug: str = "default",
    cell_key: str = "abcdef0123456789",
) -> Path:
    path = root / "runs" / eval_slug / agent_id / session_id / cell_key / "analysis.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps({"summary": summary, "checks": {}}),
        encoding="utf-8",
    )
    return path


def write_cached_markdown(
    root: Path,
    *,
    agent_id: str,
    session_id: str,
    markdown: str,
    eval_slug: str = "default",
    cell_key: str = "abcdef0123456789",
) -> Path:
    path = root / "runs" / eval_slug / agent_id / session_id / cell_key / "analysis.md"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(markdown, encoding="utf-8")
    return path


def write_cached_note(
    root: Path,
    *,
    agent_id: str,
    session_id: str,
    markdown: str,
    eval_slug: str = "default",
    cell_key: str = "abcdef0123456789",
) -> Path:
    path = root / "runs" / eval_slug / agent_id / session_id / cell_key / "notes.md"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(markdown, encoding="utf-8")
    return path


def serve_args(**overrides):
    values = {
        "path": None,
        "db": None,
        "input_table": None,
        "session_id": None,
        "adapter": None,
        "note": [],
    }
    values.update(overrides)
    return SimpleNamespace(**values)


def sample_report(config: ToolConfig) -> dict:
    result = convert_records(
        read_jsonl(str(FIXTURES / "common_session.jsonl")),
        config,
    )
    return build_report(result, config, "common_session.jsonl")


def request_json(
    port: int,
    method: str,
    path: str,
    payload: dict,
    *,
    origin: str,
) -> tuple[int, dict[str, str], dict]:
    body = json.dumps(payload)
    headers = {
        "Content-Type": "application/json",
        "Origin": origin,
    }
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
    conn.request(method, path, body=body, headers=headers)
    response = conn.getresponse()
    raw = response.read().decode("utf-8")
    result = json.loads(raw)
    response_headers = {key.lower(): value for key, value in response.getheaders()}
    conn.close()
    return response.status, response_headers, result


def request_bytes(port: int, path: str) -> tuple[int, dict[str, str], bytes]:
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
    conn.request("GET", path)
    response = conn.getresponse()
    body = response.read()
    response_headers = {key.lower(): value for key, value in response.getheaders()}
    conn.close()
    return response.status, response_headers, body


def request_text(port: int, path: str) -> tuple[int, dict[str, str], str]:
    status, headers, body = request_bytes(port, path)
    return status, headers, body.decode("utf-8")


if __name__ == "__main__":
    unittest.main()
