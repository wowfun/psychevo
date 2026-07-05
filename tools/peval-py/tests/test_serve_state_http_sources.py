from __future__ import annotations

from serve_state_support import *

class PevalPyServeStateHttpSourceTests(unittest.TestCase):
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

                status, _, html = request_text(port, "/")
                self.assertEqual(status, 200)
                embedded = script_json(html, "peval-py-data")
                options = script_json(html, "peval-py-render-options")
                comparison = report_js_comparison_state(
                    embedded,
                    sources=options["sources"],
                )
                self.assertEqual(comparison["reportRows"], 1)
                self.assertTrue(comparison["hasLeaderboard"])
                self.assertFalse(comparison["hasSummary"])
                self.assertTrue(comparison["hasOverview"])
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
                with patch("peval_py.serve.assets.download_echarts_asset", return_value=b"console.log('downloaded');"):
                    self.assertEqual(cached_echarts_asset(store), b"console.log('downloaded');")
                self.assertEqual(cache_path.read_bytes(), b"console.log('downloaded');")

                cache_path.unlink()
                with patch("peval_py.serve.assets.download_echarts_asset", side_effect=RuntimeError("network down")):
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
                with patch("peval_py.serve.assets.download_echarts_asset", side_effect=RuntimeError("network down")):
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

    def test_http_adapter_default_db_endpoint_writes_config_and_updates_rendering(self) -> None:
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
                    "/api/config/adapter-default-db",
                    {"adapter": "opencode", "default_db_path": "db/opencode.db"},
                    origin="http://example.test",
                )
                self.assertEqual(status, 403)
                self.assertIn("same-origin", rejected["error"])

                status, _, invalid = request_json(
                    port,
                    "POST",
                    "/api/config/adapter-default-db",
                    {"adapter": "missing", "default_db_path": "db/missing.db"},
                    origin=origin,
                )
                self.assertEqual(status, 400)
                self.assertIn(
                    "unsupported adapter for adapter default DB: missing",
                    invalid["error"],
                )

                status, _, body = request_json(
                    port,
                    "POST",
                    "/api/config/adapter-default-db",
                    {"adapter": "opencode", "default_db_path": "db/opencode.db"},
                    origin=origin,
                )
                expected = str((root / "db/opencode.db").resolve())
                self.assertEqual(status, 200)
                self.assertEqual(body["adapter"], "opencode")
                self.assertEqual(body["default_db_path"], expected)
                self.assertEqual(body["adapter_defaults"]["opencode"], expected)
                config_text = (root / "peval-py.toml").read_text(encoding="utf-8")
                self.assertIn("[adapters.opencode]\n", config_text)
                self.assertIn('default_db_path = "db/opencode.db"\n', config_text)

                status, _, html = request_text(port, "/")
                self.assertEqual(status, 200)
                self.assertIn(
                    f'<option value="opencode" selected data-default-db="{expected}">opencode</option>',
                    html,
                )

                status, _, body = request_json(
                    port,
                    "POST",
                    "/api/config/adapter-default-db",
                    {"adapter": "opencode", "default_db_path": ""},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(body["adapter"], "opencode")
                self.assertIsNone(body["default_db_path"])
                self.assertNotIn("opencode", body["adapter_defaults"])
                self.assertNotIn(
                    "default_db_path",
                    (root / "peval-py.toml").read_text(encoding="utf-8"),
                )

                status, _, html = request_text(port, "/")
                self.assertEqual(status, 200)
                self.assertNotIn(expected, html)
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
                source_keys = [source["source_key"] for source in body["sources"]]
                self.assertEqual(body["report_source_key"], source_keys[0])
                self.assertEqual(len(body["report"]["trajectory"]), 2)
                self.assertEqual(
                    [meta["trial_key"] for meta in body["report"]["trajectory_meta"]],
                    ["session:t001", "session:t001:2"],
                )
                artifact_dirs = {
                    source["source_key"]: root / source["artifact_dir"]
                    for source in body["sources"]
                }
                self.assertTrue(artifact_dirs[source_keys[0]].is_dir())
                self.assertTrue(artifact_dirs[source_keys[1]].is_dir())
                status, _, html = request_text(port, "/")
                self.assertEqual(status, 200)
                embedded = script_json(html, "peval-py-data")
                options = script_json(html, "peval-py-render-options")
                self.assertEqual(len(embedded["trajectory"]), 2)
                comparison = report_js_comparison_state(
                    embedded,
                    sources=options["sources"],
                )
                self.assertEqual(comparison["reportRows"], 2)
                self.assertTrue(comparison["hasLeaderboard"])
                self.assertTrue(comparison["hasSummary"])
                self.assertTrue(comparison["hasOverview"])
                status, _, body = request_json(
                    port,
                    "POST",
                    f"/api/sources/{source_keys[1]}/alias",
                    {"alias": "Second source"},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(body["report_source_key"], source_keys[1])
                self.assertEqual(len(body["report"]["trajectory"]), 2)
                self.assertEqual(
                    body["report"]["trajectory_meta"][1]["source_alias"],
                    "Second source",
                )

                status, _, body = request_json(
                    port,
                    "POST",
                    "/api/refresh",
                    {},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(len(body["report"]["trajectory"]), 2)

                status, _, body = request_json(
                    port,
                    "POST",
                    "/api/sources/reload",
                    {},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(len(body["sources"]), 2)
                self.assertEqual(len(body["report"]["trajectory"]), 2)
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
                self.assertEqual(body["report_source_key"], source_keys[1])
                self.assertEqual(len(body["report"]["trajectory"]), 1)
                self.assertTrue(source_a.exists())
                self.assertFalse(artifact_dirs[source_keys[0]].exists())
                self.assertTrue(artifact_dirs[source_keys[1]].is_dir())
                self.assertEqual(
                    store.conn.execute(
                        "SELECT COUNT(*) FROM peval_py_sources WHERE source_key = ?",
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

                status, _, body = request_json(
                    port,
                    "POST",
                    f"/api/sources/{source_keys[1]}/delete",
                    {},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(body["sources"], [])
                self.assertIsNone(body["report_source_key"])
                self.assertEqual(body["report"]["trajectory"], [])
                self.assertFalse(artifact_dirs[source_keys[1]].exists())
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=5)
                store.close()

    def test_http_report_source_state_and_batch_archive_activate(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            source_a = root / "common_one.jsonl"
            source_b = root / "common_two.jsonl"
            source_c = root / "common_three.jsonl"
            shutil.copy(FIXTURES / "common_session.jsonl", source_a)
            shutil.copy(FIXTURES / "common_session.jsonl", source_b)
            shutil.copy(FIXTURES / "common_session.jsonl", source_c)
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

            def get_report(path: str) -> tuple[int, dict]:
                conn = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
                conn.request("GET", path)
                response = conn.getresponse()
                payload = json.loads(response.read().decode("utf-8"))
                conn.close()
                return response.status, payload

            try:
                status, _, body = request_json(
                    port,
                    "POST",
                    "/api/sources",
                    {"path": "common_one.jsonl common_two.jsonl common_three.jsonl"},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                source_keys = [source["source_key"] for source in body["sources"]]
                self.assertEqual(len(source_keys), 3)
                self.assertEqual(len(body["report"]["trajectory"]), 3)

                status, _, body = request_json(
                    port,
                    "POST",
                    "/api/sources/state",
                    {
                        "source_keys": source_keys[1:],
                        "active": False,
                        "report_source_state": "active",
                    },
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(body["report_source_state"], "active")
                self.assertEqual(body["report_source_key"], source_keys[0])
                self.assertEqual(len(body["report"]["trajectory"]), 1)
                self.assertEqual(
                    [source["active"] for source in body["sources"]],
                    [True, False, False],
                )

                status, active_report = get_report("/api/report")
                self.assertEqual(status, 200)
                self.assertEqual(len(active_report["trajectory"]), 1)
                status, archived_report = get_report("/api/report?source_state=archived")
                self.assertEqual(status, 200)
                self.assertEqual(len(archived_report["trajectory"]), 2)
                self.assertEqual(
                    [meta["trial_key"] for meta in archived_report["trajectory_meta"]],
                    ["session:t001", "session:t001:2"],
                )
                status, single_report = get_report(
                    f"/api/report?source_key={source_keys[1]}&source_state=active"
                )
                self.assertEqual(status, 200)
                self.assertEqual(len(single_report["trajectory"]), 1)
                status, invalid_state = get_report("/api/report?source_state=all")
                self.assertEqual(status, 400)
                self.assertIn("source_state must be active or archived", invalid_state["error"])

                status, _, bad_keys = request_json(
                    port,
                    "POST",
                    "/api/sources/state",
                    {
                        "source_keys": [source_keys[2], "missing-source"],
                        "active": True,
                        "report_source_state": "archived",
                    },
                    origin=origin,
                )
                self.assertEqual(status, 400)
                self.assertIn("unknown source", bad_keys["error"])
                self.assertFalse(
                    next(
                        source
                        for source in store.source_payload()
                        if source["source_key"] == source_keys[2]
                    )["active"]
                )

                status, _, bad_active = request_json(
                    port,
                    "POST",
                    "/api/sources/state",
                    {
                        "source_keys": [source_keys[1]],
                        "active": "yes",
                        "report_source_state": "archived",
                    },
                    origin=origin,
                )
                self.assertEqual(status, 400)
                self.assertIn("active must be true or false", bad_active["error"])

                status, _, bad_state = request_json(
                    port,
                    "POST",
                    "/api/sources/state",
                    {
                        "source_keys": [source_keys[1]],
                        "active": True,
                        "report_source_state": "all",
                    },
                    origin=origin,
                )
                self.assertEqual(status, 400)
                self.assertIn("report_source_state must be active or archived", bad_state["error"])

                status, _, body = request_json(
                    port,
                    "POST",
                    "/api/sources/state",
                    {
                        "source_keys": [source_keys[1]],
                        "active": True,
                        "report_source_state": "archived",
                    },
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(body["report_source_state"], "archived")
                self.assertEqual(body["report_source_key"], source_keys[2])
                self.assertEqual(len(body["report"]["trajectory"]), 1)
                self.assertEqual(
                    [source["active"] for source in body["sources"]],
                    [True, True, False],
                )

                status, _, rejected = request_json(
                    port,
                    "POST",
                    "/api/sources/state",
                    {
                        "source_keys": [source_keys[2]],
                        "active": True,
                        "report_source_state": "archived",
                    },
                    origin="http://example.test",
                )
                self.assertEqual(status, 403)
                self.assertIn("same-origin", rejected["error"])
                self.assertFalse(
                    next(
                        source
                        for source in store.source_payload()
                        if source["source_key"] == source_keys[2]
                    )["active"]
                )
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=5)
                store.close()

    def test_http_reload_discovers_cells_and_missing_report_is_clear(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp))
            config = ToolConfig(adapter="opencode")
            report = sample_report(config)
            store = open_workspace_state(str(root))
            source_key = store.ingest_upload("saved-report.json", json.dumps(report), config)[0]
            source = store.source_payload()[0]
            artifact_dir = root / source["artifact_dir"]
            store.conn.execute("DELETE FROM peval_py_refresh_log")
            store.conn.execute("DELETE FROM peval_py_sources")
            store.conn.commit()
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
                    "/api/sources/reload",
                    {},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(len(body["sources"]), 1)
                self.assertEqual(body["sources"][0]["source_key"], source_key)
                self.assertEqual(body["sources"][0]["kind"], "trial-artifact")

                conn = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
                conn.request("GET", f"/api/report?source_key={source_key}")
                response = conn.getresponse()
                payload = json.loads(response.read().decode("utf-8"))
                conn.close()
                self.assertEqual(response.status, 200)
                self.assertEqual(len(payload["trajectory"]), 1)

                shutil.rmtree(artifact_dir)
                status, _, html = request_text(port, "/")
                self.assertEqual(status, 200)
                options = script_json(html, "peval-py-render-options")
                self.assertEqual(options["sources"][0]["last_status"], "missing")

                conn = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
                conn.request("GET", f"/api/report?source_key={source_key}")
                response = conn.getresponse()
                missing = json.loads(response.read().decode("utf-8"))
                conn.close()
                self.assertEqual(response.status, 400)
                self.assertIn("Trial cell artifacts not found", missing["error"])
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=5)
                store.close()

    def test_http_path_source_recursively_imports_external_runs_tree(self) -> None:
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
            write_trial_cell_artifacts(first_cell, session_id="session-a", trial_key="session_t001")
            write_trial_cell_artifacts(second_cell, session_id="session-b", trial_key="session_t002")
            (first_cell / "notes.md").write_text("Imported note.", encoding="utf-8")
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
                    {"path": str(external / "runs")},
                    origin=origin,
                )
                self.assertEqual(status, 200)
                self.assertEqual(len(body["sources"]), 2)
                self.assertEqual(body["report_source_key"], body["sources"][0]["source_key"])
                self.assertEqual(len(body["report"]["trajectory"]), 2)
                self.assertEqual(
                    [source["trial_session_id"] for source in body["sources"]],
                    ["session-a", "session-b"],
                )
                self.assertTrue(body["sources"][0]["snapshot"])
                self.assertFalse(body["sources"][0]["refreshable"])
                copied_note = root / body["sources"][0]["artifact_dir"] / "notes.md"
                self.assertEqual(copied_note.read_text(encoding="utf-8"), "Imported note.")
                self.assertTrue((first_cell / "notes.md").is_file())
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=5)
                store.close()

    def test_http_empty_runs_import_fails_without_persisting_sources(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = peval_py_workspace(Path(tmp) / "workspace")
            external = peval_py_workspace(Path(tmp) / "external")
            (external / "runs" / "empty").mkdir(parents=True)
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
                    {"path": str(external / "runs")},
                    origin=origin,
                )
                self.assertEqual(status, 400)
                self.assertIn("no complete Trial cells found", body["error"])
                self.assertEqual(store.source_payload(), [])
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=5)
                store.close()
