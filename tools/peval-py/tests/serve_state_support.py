from __future__ import annotations

import http.client
import os
import threading

from peval_py_test_support import *

from cli_inputs_support import write_trial_cell_artifacts
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
    load_serve_inputs,
    make_handler,
    source_path_values,
    workspace_relative_path,
)
from peval_py.state import (
    REFRESH_LOG_LIMIT,
    UPLOAD_LIMIT_BYTES,
    discover_complete_trial_cell_dirs,
    loaded_trial_cell_import_session,
    open_workspace_state,
    resolve_workspace_root,
)




def peval_py_workspace(root: Path) -> Path:
    root.mkdir(parents=True, exist_ok=True)
    (root / "peval-py.toml").write_text("", encoding="utf-8")
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


def report_js_comparison_state(
    report: dict,
    *,
    sources: list[dict] | None = None,
    mode: str = "serve",
) -> dict:
    if not shutil.which("node"):
        raise unittest.SkipTest("node is required to execute report.js")
    asset = load_asset_text("report.js")
    script = """
const vm = require("vm");
const asset = __ASSET__;
const report = __REPORT__;
const renderOptions = __RENDER_OPTIONS__;
const nodes = {};
function makeNode(id) {
  const node = {
    id,
    textContent: "",
    hidden: false,
    dataset: {},
    style: {},
    classList: { add() {}, remove() {}, toggle() {} },
    addEventListener() {},
    removeEventListener() {},
    querySelector() { return null; },
    querySelectorAll() { return []; },
    closest() { return null; },
    _innerHTML: "",
  };
  Object.defineProperty(node, "innerHTML", {
    get() { return this._innerHTML; },
    set(value) {
      this._innerHTML = String(value || "");
      for (const match of this._innerHTML.matchAll(/id="([^"]+)"/g)) {
        if (!nodes[match[1]]) nodes[match[1]] = makeNode(match[1]);
      }
    },
  });
  return node;
}
[
  "peval-py-data",
  "peval-py-i18n",
  "peval-py-token-estimates",
  "peval-py-render-options",
  "report-notes",
  "comparison",
  "trace",
  "step-drawer",
].forEach(id => nodes[id] = makeNode(id));
nodes["peval-py-data"].textContent = JSON.stringify(report);
nodes["peval-py-i18n"].textContent = "{}";
nodes["peval-py-token-estimates"].textContent = "{}";
nodes["peval-py-render-options"].textContent = JSON.stringify(renderOptions);
const context = {
  document: {
    body: { classList: { add() {}, remove() {}, toggle() {} } },
    addEventListener() {},
    getElementById(id) { return nodes[id] || null; },
    querySelector() { return null; },
    querySelectorAll() { return []; },
  },
  window: { addEventListener() {} },
  console,
  JSON,
  Number,
  String,
  Object,
  Math,
  Date,
  Set,
  Array,
  RegExp,
  requestAnimationFrame(callback) { callback(); },
};
vm.createContext(context);
vm.runInContext(asset, context);
console.log(JSON.stringify({
  reportRows: vm.runInContext("reportRows().length", context),
  selectedTrial: vm.runInContext("selectedKey()", context),
  selectedSourceKey: vm.runInContext("state.selectedSourceKey", context),
  comparisonLength: nodes.comparison.innerHTML.length,
  hasLeaderboard: Boolean(nodes.leaderboard?.innerHTML.includes("Leaderboard")),
  hasSummary: Boolean(nodes["leaderboard-summary"]?.innerHTML.includes("Leaderboard Summary")),
  hasOverview: Boolean(nodes["trajectory-overview"]?.innerHTML.includes("Trajectory Overview")),
  traceLength: nodes.trace.innerHTML.length,
}));
""".replace("__ASSET__", json.dumps(asset)).replace(
        "__REPORT__",
        json.dumps(report),
    ).replace(
        "__RENDER_OPTIONS__",
        json.dumps({"mode": mode, "sources": sources or []}),
    )
    result = subprocess.run(
        ["node"],
        input=script,
        text=True,
        capture_output=True,
        timeout=10,
        check=False,
    )
    if result.returncode != 0:
        raise AssertionError(result.stderr)
    return json.loads(result.stdout)


if __name__ == "__main__":
    unittest.main()
