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
    cell_key: str = "session_t001",
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
    cell_key: str = "session_t001",
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
    cell_key: str = "session_t001",
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
