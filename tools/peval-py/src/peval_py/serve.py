from __future__ import annotations

import json
import os
import re
import shlex
import sys
from argparse import Namespace
from dataclasses import replace
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from types import SimpleNamespace
from typing import Any
from urllib.parse import unquote, urlsplit
from urllib.request import urlopen

from peval_py.adapters import available_adapter_ids, normalize_adapter_id
from peval_py.config import ToolConfig, write_workspace_locale
from peval_py.html import render_serve_html
from peval_py.inputs import (
    AdapterAssignments,
    LoadedInputs,
    infer_adapter_from_path,
    parse_adapter_assignments,
    validate_selected_adapter,
)
from peval_py.i18n import normalize_locale
from peval_py.session_select import list_adapter_sessions
from peval_py.state import (
    UPLOAD_LIMIT_BYTES,
    ServeStateStore,
    load_serve_inputs,
    open_workspace_state,
)

DEFAULT_PORT_START = 58010
DEFAULT_PORT_END = 58029
LOCALHOSTS = {"127.0.0.1", "localhost", "::1"}
MAX_JSON_BODY_BYTES = UPLOAD_LIMIT_BYTES + 2 * 1024 * 1024
WINDOWS_DRIVE_PATH_RE = re.compile(r"^[A-Za-z]:[\\/]")
WINDOWS_DRIVE_MOUNT_ROOT = Path("/mnt")
ECHARTS_VERSION = "6.0.0"
ECHARTS_ASSET_PATH = f"/assets/echarts/{ECHARTS_VERSION}/echarts.min.js"
ECHARTS_CDN_URL = f"https://cdn.jsdelivr.net/npm/echarts@{ECHARTS_VERSION}/dist/echarts.min.js"


class HttpError(Exception):
    def __init__(self, status: int, message: str) -> None:
        super().__init__(message)
        self.status = status
        self.message = message


class LocalHTTPServer(HTTPServer):
    allow_reuse_address = True


def run_serve_command(
    args: Namespace,
    config: ToolConfig,
    adapter_assignments: AdapterAssignments,
) -> None:
    host = validate_localhost(getattr(args, "host", None) or "127.0.0.1")
    store = open_workspace_state(getattr(args, "root", None))
    config = replace(config, workspace_root=str(store.paths.root))
    server: HTTPServer | None = None
    try:
        loaded_inputs = load_serve_inputs(args, adapter_assignments, config)
        source_keys = store.upsert_loaded_sources(loaded_inputs, config)
        if source_keys:
            store.refresh_sources(source_keys, config)

        handler = make_handler(store, config)
        server = bind_server(host, getattr(args, "port", None), handler)
        print(f"peval-py serve: {format_url(host, server.server_port)}", flush=True)
        server.serve_forever()
    except KeyboardInterrupt:
        return
    finally:
        if server is not None:
            server.server_close()
        store.close()


def validate_localhost(host: str) -> str:
    text = str(host).strip()
    normalized = text[1:-1] if text.startswith("[") and text.endswith("]") else text
    if normalized.lower() not in LOCALHOSTS:
        raise ValueError("serve only binds localhost by default; use 127.0.0.1, localhost, or ::1")
    return normalized


def bind_server(
    host: str,
    requested_port: int | None,
    handler: type[BaseHTTPRequestHandler],
) -> HTTPServer:
    if requested_port is not None:
        return LocalHTTPServer((host, requested_port), handler)

    last_error: OSError | None = None
    for port in range(DEFAULT_PORT_START, DEFAULT_PORT_END + 1):
        try:
            return LocalHTTPServer((host, port), handler)
        except OSError as exc:
            last_error = exc
    raise OSError(
        f"could not bind {host}:{DEFAULT_PORT_START}..{DEFAULT_PORT_END}"
    ) from last_error


def format_url(host: str, port: int) -> str:
    display_host = f"[{host}]" if ":" in host and not host.startswith("[") else host
    return f"http://{display_host}:{port}/"


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
            path = urlsplit(self.path).path
            try:
                if path == "/":
                    self.write_html(
                        render_serve_html(
                            store.active_report(),
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
                    self.write_json(store.active_report())
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
                if path == "/api/db-sessions":
                    self.write_json(db_sessions_payload(store, payload))
                    return
                if path == "/api/sources":
                    add_source_payload(store, runtime.config, payload)
                    self.write_json(mutation_payload(store))
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
                    self.write_json(mutation_payload(store))
                    return
                if path == "/api/refresh":
                    store.refresh_sources(source_keys_payload(payload), runtime.config)
                    self.write_json(mutation_payload(store))
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
                    self.write_json(mutation_payload(store))
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


def cached_echarts_asset(store: ServeStateStore) -> bytes:
    path = echarts_cache_path(store)
    if path.is_file():
        return path.read_bytes()
    try:
        data = download_echarts_asset()
    except Exception as exc:  # noqa: BLE001 - HTTP asset boundary.
        raise HttpError(502, f"failed to cache ECharts: {exc}") from exc
    if not data:
        raise HttpError(502, "failed to cache ECharts: empty response")
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = path.with_name(path.name + ".tmp")
    tmp_path.write_bytes(data)
    tmp_path.replace(path)
    return data


def echarts_cache_path(store: ServeStateStore) -> Path:
    return store.paths.root / ".cache" / "echarts" / ECHARTS_VERSION / "echarts.min.js"


def download_echarts_asset() -> bytes:
    with urlopen(ECHARTS_CDN_URL, timeout=15) as response:  # noqa: S310 - fixed URL.
        return response.read()


def mutation_payload(store: ServeStateStore) -> dict[str, Any]:
    return {
        "sources": store.source_payload(),
        "report": store.active_report(),
    }


def add_source_payload(
    store: ServeStateStore,
    config: ToolConfig,
    payload: dict[str, Any],
) -> list[str]:
    source_args = source_args_from_payload(store, payload)
    raw_adapter = adapter_override_payload(payload)
    assignments = parse_adapter_assignments(
        [raw_adapter] if raw_adapter else [],
        config.adapter,
    )
    loaded = load_serve_inputs(source_args, assignments, config)
    loaded = apply_payload_alias(loaded, optional_string(payload.get("alias")))
    return store.import_loaded_sources(loaded, config)


def apply_payload_alias(loaded: LoadedInputs, alias: str | None) -> LoadedInputs:
    if alias is None:
        return loaded
    return LoadedInputs(
        sessions=[replace(session, source_alias=alias) for session in loaded.sessions],
        notes=loaded.notes,
    )


def db_sessions_payload(
    store: ServeStateStore,
    payload: dict[str, Any],
) -> dict[str, Any]:
    db_paths = source_path_values(store, payload, "db")
    if len(db_paths) != 1:
        raise HttpError(400, "DB Inspect requires exactly one DB path")
    db_path = db_paths[0]
    path = Path(db_path)
    if not path.is_file():
        raise HttpError(400, f"DB path does not exist: {path}")
    raw_adapter = adapter_override_payload(payload)
    adapter_id, inferred = adapter_for_db_inspect(str(path), raw_adapter)
    sessions = list_adapter_sessions(adapter_id, str(path))
    return {
        "db": str(path),
        "adapter": adapter_id,
        "inferred": inferred,
        "sessions": [
            {
                "index": index,
                "session_id": session.session_id,
                "name": session.name,
            }
            for index, session in enumerate(sessions, start=1)
        ],
    }


def adapter_for_db_inspect(path: str, raw_adapter: str | None) -> tuple[str, bool]:
    available = set(available_adapter_ids())
    if raw_adapter:
        return validate_selected_adapter(
            normalize_adapter_id(raw_adapter),
            available,
            "DB session inspect",
        ), False
    adapter_id = infer_adapter_from_path(path, available)
    if adapter_id is None:
        options = ", ".join(sorted(available)) or "<none>"
        raise HttpError(
            400,
            f"could not infer adapter for {path}; choose adapter "
            f"(available adapters: {options})",
        )
    return adapter_id, True


def source_args_from_payload(
    store: ServeStateStore,
    payload: dict[str, Any],
) -> SimpleNamespace:
    paths = source_path_values(store, payload, "path")
    dbs = source_path_values(store, payload, "db")
    input_table = optional_string(payload.get("input_table"))
    present = [value for value in [paths, dbs, input_table] if value]
    if len(present) != 1:
        raise HttpError(400, "provide exactly one source: path, db, or input_table")
    session_id = optional_string(payload.get("session_id"))
    session_ids = session_ids_payload(payload)
    if session_id and session_ids:
        raise HttpError(400, "provide either session_id or session_ids, not both")
    if (session_id or session_ids) and not dbs:
        raise HttpError(400, "session_id and session_ids are only valid with db sources")
    if (session_id or session_ids) and len(dbs) != 1:
        raise HttpError(400, "session_id and session_ids require exactly one db source")
    return SimpleNamespace(
        path=paths or None,
        db=dbs or None,
        input_table=[workspace_relative_path(store, input_table)] if input_table else None,
        session_id=([session_id] if session_id and dbs else session_ids if dbs else None),
        adapter=[],
        note=[],
    )


def source_path_values(
    store: ServeStateStore,
    payload: dict[str, Any],
    key: str,
) -> list[str]:
    raw = optional_string(payload.get(key))
    if raw is None:
        return []
    parts = split_source_path_list(raw, key)
    if not parts:
        raise HttpError(400, f"{key} path list is empty")
    return [workspace_relative_path(store, part) for part in parts]


def split_source_path_list(raw: str, key: str) -> list[str]:
    try:
        raw_parts = shlex.split(raw, posix=False)
    except ValueError as exc:
        raise HttpError(400, f"{key} path list is invalid: {exc}") from exc
    return [
        unquote_path_token(part)
        for part in raw_parts
        if unquote_path_token(part)
    ]


def unquote_path_token(raw: object) -> str:
    text = str(raw).strip()
    if len(text) >= 2 and text[0] == text[-1] and text[0] in {"'", '"'}:
        return text[1:-1]
    return text


def adapter_override_payload(payload: dict[str, Any]) -> str | None:
    adapter = optional_string(payload.get("adapter"))
    if adapter is None or adapter.lower() == "auto":
        return None
    return adapter


def session_ids_payload(payload: dict[str, Any]) -> list[str] | None:
    raw = payload.get("session_ids")
    if raw is None:
        return None
    if not isinstance(raw, list):
        raise HttpError(400, "session_ids must be an array")
    session_ids: list[str] = []
    for value in raw:
        text = optional_string(value)
        if text is not None:
            session_ids.append(text)
    if not session_ids:
        raise HttpError(400, "session_ids must include at least one session id")
    return session_ids


def workspace_relative_path(
    store: ServeStateStore,
    raw_path: str | None,
    *,
    windows_mount_root: Path | None = None,
) -> str | None:
    if raw_path is None:
        return None
    text = unquote_path_token(raw_path)
    if not text:
        return None
    if is_windows_absolute_like_path(text):
        return resolve_windows_absolute_like_path(text, windows_mount_root)
    path = Path(text).expanduser()
    if not path.is_absolute():
        path = store.paths.root / path
    return str(path)


def is_windows_absolute_like_path(path: str) -> bool:
    return bool(WINDOWS_DRIVE_PATH_RE.match(path)) or path.startswith("\\\\") or path.startswith("//")


def resolve_windows_absolute_like_path(
    raw_path: str,
    windows_mount_root: Path | None = None,
) -> str:
    if os.name == "nt":
        return str(Path(raw_path).expanduser())
    original = Path(raw_path).expanduser()
    if original.exists():
        return str(original)
    mapped = windows_drive_mount_path(raw_path, windows_mount_root or WINDOWS_DRIVE_MOUNT_ROOT)
    if mapped is not None and mapped.exists():
        return str(mapped)
    return raw_path


def windows_drive_mount_path(raw_path: str, mount_root: Path) -> Path | None:
    if not WINDOWS_DRIVE_PATH_RE.match(raw_path):
        return None
    drive = raw_path[0].lower()
    rest = raw_path[2:].lstrip("\\/")
    parts = [part for part in re.split(r"[\\/]+", rest) if part]
    return Path(mount_root) / drive / Path(*parts)


def source_keys_payload(payload: dict[str, Any]) -> list[str] | None:
    raw_keys = payload.get("source_keys")
    if raw_keys is None and payload.get("source_key") is not None:
        raw_keys = [payload["source_key"]]
    if raw_keys is None:
        return None
    if not isinstance(raw_keys, list):
        raise HttpError(400, "source_keys must be an array")
    return [str(key) for key in raw_keys]


def source_action_path(path: str) -> tuple[str, str] | None:
    prefix = "/api/sources/"
    if not path.startswith(prefix):
        return None
    parts = path[len(prefix) :].split("/")
    if len(parts) != 2 or not parts[0] or not parts[1]:
        raise HttpError(404, "unknown source action")
    return unquote(parts[0]), parts[1]


def required_string(payload: dict[str, Any], key: str) -> str:
    value = payload.get(key)
    if not isinstance(value, str) or not value:
        raise HttpError(400, f"{key} is required")
    return value


def markdown_payload(payload: dict[str, Any]) -> str:
    value = payload.get("markdown")
    if not isinstance(value, str):
        raise HttpError(400, "markdown is required")
    return value


def alias_payload(payload: dict[str, Any]) -> str | None:
    value = payload.get("alias")
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def optional_string(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


if __name__ == "__main__":
    print("peval_py.serve is not a standalone entry point", file=sys.stderr)
