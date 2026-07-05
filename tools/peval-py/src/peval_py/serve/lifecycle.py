from __future__ import annotations

from argparse import Namespace
from dataclasses import replace
from http.server import BaseHTTPRequestHandler, HTTPServer
from typing import Any

from peval_py.config import ToolConfig
from peval_py.inputs import AdapterAssignments, LoadedInputs
from peval_py.serve.constants import DEFAULT_PORT_END, DEFAULT_PORT_START, LOCALHOSTS
from peval_py.serve.handler import make_handler
from peval_py.serve.sources import load_serve_inputs
from peval_py.state import open_workspace_state

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
        store.import_loaded_sources(loaded_inputs, config)
        store.sync_artifact_sources(config)

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
