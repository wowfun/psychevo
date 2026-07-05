from __future__ import annotations

import json
from dataclasses import replace
from http.server import BaseHTTPRequestHandler
from types import SimpleNamespace
from typing import Any
from urllib.parse import urlsplit

from peval_py.config import ToolConfig, write_workspace_adapter_default_db, write_workspace_locale
from peval_py.html import render_serve_html
from peval_py.i18n import normalize_locale
from peval_py.serve.assets import ECHARTS_ASSET_PATH, cached_echarts_asset
from peval_py.serve.constants import MAX_JSON_BODY_BYTES
from peval_py.serve.errors import HttpError
from peval_py.serve.payloads import (
    adapter_default_db_payload,
    adapter_override_payload,
    alias_payload,
    markdown_payload,
    required_bool,
    required_string,
    source_action_path,
    source_keys_payload,
    source_state_payload,
)
from peval_py.serve.reporting import mutation_payload, single_query_value
from peval_py.serve.sources import add_source_payload, db_sessions_payload
from peval_py.state import ServeStateStore

def make_handler(
    store: ServeStateStore,
    config: ToolConfig,
) -> type[BaseHTTPRequestHandler]:
    runtime = SimpleNamespace(config=config)

    class ServeHandler(BaseHTTPRequestHandler):
        server_version = "peval-py-serve/1"

        def log_message(self, format: str, *args: Any) -> None:  # noqa: A002
            return

        def do_GET(self) -> None:  # noqa: N802
            parsed_url = urlsplit(self.path)
            path = parsed_url.path
            try:
                if path == "/":
                    self.write_html(
                        render_serve_html(
                            store.active_report(runtime.config),
                            locale=runtime.config.locale,
                            sources=store.source_payload(),
                            adapter_defaults=runtime.config.adapter_default_db_paths,
                        )
                    )
                    return
                if path == ECHARTS_ASSET_PATH:
                    self.write_js(cached_echarts_asset(store))
                    return
                if path == "/api/report":
                    source_key = single_query_value(parsed_url.query, "source_key")
                    source_state = (
                        "active"
                        if source_key
                        else source_state_payload(
                            single_query_value(parsed_url.query, "source_state")
                        )
                    )
                    try:
                        self.write_json(
                            store.active_report(
                                runtime.config,
                                source_keys=[source_key] if source_key else None,
                                source_state=source_state,
                            )
                        )
                    except ValueError as exc:
                        raise HttpError(400, str(exc)) from exc
                    return
                if path == "/api/sources":
                    self.write_json({"sources": store.source_payload()})
                    return
                raise HttpError(404, "not found")
            except HttpError as exc:
                self.write_error(exc.status, exc.message)
            except Exception as exc:  # noqa: BLE001 - HTTP boundary.
                self.write_error(500, str(exc))

        def do_POST(self) -> None:  # noqa: N802
            path = urlsplit(self.path).path
            try:
                payload = self.read_json_payload()
                if path == "/api/config/locale":
                    locale = normalize_locale(required_string(payload, "locale"))
                    write_workspace_locale(store.paths.config_path, locale)
                    runtime.config = replace(runtime.config, locale=locale)
                    self.write_json({"locale": locale})
                    return
                if path == "/api/config/adapter-default-db":
                    adapter_id, raw_db_path = adapter_default_db_payload(payload)
                    resolved = write_workspace_adapter_default_db(
                        store.paths.config_path,
                        adapter_id,
                        raw_db_path,
                    )
                    adapter_defaults = dict(runtime.config.adapter_default_db_paths)
                    if resolved:
                        adapter_defaults[adapter_id] = resolved
                    else:
                        adapter_defaults.pop(adapter_id, None)
                    runtime.config = replace(
                        runtime.config,
                        adapter_default_db_paths=adapter_defaults,
                    )
                    self.write_json(
                        {
                            "adapter": adapter_id,
                            "default_db_path": resolved,
                            "adapter_defaults": adapter_defaults,
                        }
                    )
                    return
                if path == "/api/db-sessions":
                    self.write_json(db_sessions_payload(store, payload))
                    return
                if path == "/api/sources/state":
                    source_keys = source_keys_payload(payload)
                    if not source_keys:
                        raise HttpError(400, "source_keys must include at least one source")
                    active = required_bool(payload, "active")
                    report_source_state = source_state_payload(
                        payload.get("report_source_state"),
                        field="report_source_state",
                    )
                    for source_key in source_keys:
                        store.source_by_key(source_key)
                    for source_key in source_keys:
                        store.set_source_active(source_key, active)
                    self.write_json(
                        mutation_payload(
                            store,
                            runtime.config,
                            source_state=report_source_state,
                        )
                    )
                    return
                if path == "/api/sources":
                    keys = add_source_payload(store, runtime.config, payload)
                    self.write_json(
                        mutation_payload(
                            store,
                            runtime.config,
                            source_key=keys[0] if keys else None,
                        )
                    )
                    return
                if path == "/api/sources/reload":
                    store.sync_artifact_sources(runtime.config)
                    self.write_json(mutation_payload(store, runtime.config))
                    return
                if path == "/api/upload":
                    filename = required_string(payload, "filename")
                    content = required_string(payload, "content")
                    adapter = adapter_override_payload(payload)
                    keys = store.ingest_upload(
                        filename,
                        content,
                        runtime.config,
                        adapter=adapter,
                    )
                    upload_alias = alias_payload(payload)
                    if upload_alias is not None:
                        for source_key in keys:
                            store.set_source_alias(source_key, upload_alias)
                    self.write_json(
                        mutation_payload(
                            store,
                            runtime.config,
                            source_key=keys[0] if keys else None,
                        )
                    )
                    return
                if path == "/api/refresh":
                    source_keys = source_keys_payload(payload)
                    store.refresh_sources(source_keys, runtime.config)
                    self.write_json(
                        mutation_payload(
                            store,
                            runtime.config,
                            source_key=source_keys[0] if source_keys else None,
                        )
                    )
                    return

                source_action = source_action_path(path)
                if source_action is not None:
                    source_key, action = source_action
                    if action == "archive":
                        store.set_source_active(source_key, False)
                    elif action == "activate":
                        store.set_source_active(source_key, True)
                    elif action == "refresh":
                        store.refresh_sources([source_key], runtime.config)
                    elif action == "delete":
                        store.delete_source(source_key)
                    elif action == "alias":
                        store.set_source_alias(source_key, alias_payload(payload))
                    elif action == "notes":
                        store.save_source_notes(
                            source_key,
                            markdown_payload(payload),
                            runtime.config,
                        )
                    else:
                        raise HttpError(404, "unknown source action")
                    self.write_json(
                        mutation_payload(
                            store,
                            runtime.config,
                            source_key=None if action == "delete" else source_key,
                        )
                    )
                    return

                raise HttpError(404, "not found")
            except HttpError as exc:
                self.write_error(exc.status, exc.message)
            except Exception as exc:  # noqa: BLE001 - HTTP boundary.
                self.write_error(400, str(exc))

        def read_json_payload(self) -> dict[str, Any]:
            self.require_same_origin()
            content_type = self.headers.get("Content-Type", "")
            media_type = content_type.split(";", 1)[0].strip().lower()
            if media_type != "application/json":
                raise HttpError(415, "mutating APIs require application/json POST")
            try:
                content_length = int(self.headers.get("Content-Length", "0"))
            except ValueError as exc:
                raise HttpError(400, "invalid Content-Length") from exc
            if content_length > MAX_JSON_BODY_BYTES:
                raise HttpError(413, "request body exceeds serve upload limit")
            raw = self.rfile.read(content_length) if content_length else b"{}"
            try:
                payload = json.loads(raw.decode("utf-8"))
            except (UnicodeDecodeError, json.JSONDecodeError) as exc:
                raise HttpError(400, "request body must be a JSON object") from exc
            if not isinstance(payload, dict):
                raise HttpError(400, "request body must be a JSON object")
            return payload

        def require_same_origin(self) -> None:
            origin = self.headers.get("Origin")
            if origin and not self.is_same_origin(origin):
                raise HttpError(403, "mutating APIs require same-origin Origin")
            referer = self.headers.get("Referer")
            if not origin and referer and not self.is_same_origin(referer):
                raise HttpError(403, "mutating APIs require same-origin Referer")

        def is_same_origin(self, value: str) -> bool:
            parsed = urlsplit(value)
            if not parsed.scheme or not parsed.netloc:
                return True
            host = self.headers.get("Host")
            if not host:
                return False
            return parsed.scheme == "http" and parsed.netloc.lower() == host.lower()

        def write_html(self, html: str, status: int = 200) -> None:
            data = html.encode("utf-8")
            self.send_response(status)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def write_json(self, payload: Any, status: int = 200) -> None:
            data = json.dumps(payload, ensure_ascii=False).encode("utf-8")
            self.send_response(status)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def write_js(self, data: bytes, status: int = 200) -> None:
            self.send_response(status)
            self.send_header("Content-Type", "application/javascript; charset=utf-8")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def write_error(self, status: int, message: str) -> None:
            if urlsplit(self.path).path.startswith("/api/"):
                self.write_json({"error": message}, status=status)
                return
            self.write_html(f"{status} {message}\n", status=status)

    return ServeHandler
