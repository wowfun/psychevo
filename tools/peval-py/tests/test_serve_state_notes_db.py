from __future__ import annotations

from serve_state_support import *

class PevalPyServeStateNotesDbTests(unittest.TestCase):
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
                created = root / store.source_payload()[0]["artifact_dir"] / "notes.md"
                self.assertTrue(created.is_file())
                self.assertEqual(created.read_text(encoding="utf-8"), "")
                self.assertEqual(
                    store.active_report()["annotations"]["notes"][0]["source_ref"]["relative_path"],
                    created.relative_to(root).as_posix(),
                )
            finally:
                store.close()

    def test_source_notes_save_uses_exact_trial_cell(self) -> None:
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
                store.save_source_notes(keys[0], "exact cell", config)
                created = root / store.source_payload()[0]["artifact_dir"] / "notes.md"
                self.assertEqual(created.read_text(encoding="utf-8"), "exact cell")
                self.assertEqual(
                    (root / "runs" / "default" / "opencode" / "common_session" / "one" / "notes.md").read_text(encoding="utf-8"),
                    "one",
                )
                self.assertEqual(
                    (root / "runs" / "default" / "opencode" / "common_session" / "two" / "notes.md").read_text(encoding="utf-8"),
                    "two",
                )
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
                store.save_source_notes(keys[0], "exact cell from analysis siblings", config)
                created = root / store.source_payload()[0]["artifact_dir"] / "notes.md"
                self.assertEqual(
                    created.read_text(encoding="utf-8"),
                    "exact cell from analysis siblings",
                )
                self.assertFalse(
                    (
                        root
                        / "runs"
                        / "default"
                        / "opencode"
                        / "common_session"
                        / "one"
                        / "notes.md"
                    ).exists()
                )
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
                            r"C:\Users\kevin\AppData\Local\state.db" "\n"
                            r'"D:\Data Dir\session.jsonl"' "\n"
                            r"C:/Users/kevin/.hermes/state.db" "\n"
                            r"\\server\share\state.db" "\n"
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
