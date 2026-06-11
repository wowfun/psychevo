from __future__ import annotations

import http.client
import os
import threading

from peval_py_test_support import *

from peval_py.inputs import parse_adapter_assignments
from peval_py.serve import (
    DEFAULT_PORT_END,
    DEFAULT_PORT_START,
    LocalHTTPServer,
    bind_server,
    make_handler,
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
            config = ToolConfig(adapter="opencode")
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
                self.assertEqual(len(store.active_report()["trajectory"]), 1)

                duplicate_keys = store.upsert_loaded_sources(loaded, config)
                self.assertEqual(duplicate_keys, keys)
                self.assertEqual(len(store.source_payload()), 1)

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


if __name__ == "__main__":
    unittest.main()
