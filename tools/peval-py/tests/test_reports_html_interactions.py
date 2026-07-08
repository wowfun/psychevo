from __future__ import annotations

from reports_html_support import *

class PevalPyReportHtmlInteractionTests(unittest.TestCase):
    def test_serve_startup_loading_status_recovers_after_ready_payload(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = """
const vm = require("vm");
const asset = __ASSET__;
function makeNode() {
  return {
    textContent: "",
    innerHTML: "",
    listeners: {},
    classList: {
      values: new Set(),
      toggle(name, force) {
        const active = force === undefined ? !this.values.has(name) : Boolean(force);
        if (active) this.values.add(name);
        else this.values.delete(name);
        return active;
      },
      contains(name) { return this.values.has(name); }
    },
    addEventListener(type, handler) { (this.listeners[type] ||= []).push(handler); },
    querySelector() { return null; },
    querySelectorAll() { return []; },
    setAttribute() {}
  };
}
const countNode = makeNode();
const statusNode = makeNode();
const listNode = makeNode();
const nodes = {
  "peval-py-data": { textContent: "{}" },
  "peval-py-token-estimates": { textContent: "{}" },
  "peval-py-i18n": { textContent: JSON.stringify({
    serve_loading_sources: "Loading sources",
    serve_scanning_runs: "Scanning runs; sessions will appear when discovery finishes.",
    serve_latest_snapshots: "Latest snapshots",
    serve_active_snapshots: "Active snapshots",
    serve_source_count: "source",
    serve_sources_count: "sources",
    serve_no_sources: "No sources loaded"
  }) },
  "peval-py-render-options": { textContent: JSON.stringify({ mode: "serve", loading: true, sources: [] }) }
};
const context = {
  document: {
    body: { classList: { add() {}, remove() {}, toggle() {} } },
    addEventListener() {},
    getElementById(id) { return nodes[id] || null; },
    querySelector(selector) {
      if (selector === "[data-source-count]") return countNode;
      if (selector === "[data-source-status]") return statusNode;
      if (selector === "[data-source-list]") return listNode;
      return null;
    },
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
  countNode,
  statusNode,
  listNode,
};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`(() => {
  renderServeSources();
  const loading = {
    count: countNode.textContent,
    status: statusNode.textContent,
    statusLoading: statusNode.classList.contains("loading"),
    statusDanger: statusNode.classList.contains("danger"),
    list: listNode.innerHTML,
  };
  render = view => {
    state.view = view;
    renderServeSources();
  };
  applyServeMutationPayload({
    loading: false,
    sources: [{
      source_key: "source-1",
      label: "source one",
      artifact_dir: "runs/a",
      active: true,
      last_status: "ok",
      snapshot: true,
      refreshable: false
    }],
    report: { schema_version: 19, includes: ["core"], trajectory: [], trajectory_meta: [] },
    report_source_key: "source-1",
    report_source_state: "active"
  });
  const ready = {
    count: countNode.textContent,
    status: statusNode.textContent,
    statusLoading: statusNode.classList.contains("loading"),
    statusDanger: statusNode.classList.contains("danger"),
    list: listNode.innerHTML,
  };
  return JSON.stringify({ loading, ready });
})()`, context);
console.log(result);
""".replace("__ASSET__", json.dumps(asset))
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        result = json.loads(node.stdout)

        self.assertEqual(result["loading"]["count"], "Loading sources")
        self.assertEqual(
            result["loading"]["status"],
            "Scanning runs; sessions will appear when discovery finishes.",
        )
        self.assertTrue(result["loading"]["statusLoading"])
        self.assertFalse(result["loading"]["statusDanger"])
        self.assertIn("source-row empty loading", result["loading"]["list"])
        self.assertNotIn("No sources loaded", result["loading"]["list"])
        self.assertEqual(result["ready"]["count"], "1 source")
        self.assertEqual(result["ready"]["status"], "Active snapshots")
        self.assertFalse(result["ready"]["statusLoading"])
        self.assertFalse(result["ready"]["statusDanger"])
        self.assertIn("source one", result["ready"]["list"])

    def test_inline_source_edit_click_does_not_trigger_trial_selection_rerender(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = f"""
const vm = require("vm");
const asset = {json.dumps(asset)};
const listeners = {{ click: [], dblclick: [] }};
const probe = {{ renderCount: 0, replaced: false, focused: false, selected: false }};
const input = {{
  className: "",
  value: "",
  setAttribute() {{}},
  addEventListener() {{}},
  focus() {{ probe.focused = true; }},
  select() {{ probe.selected = true; }}
}};
const cell = {{
  dataset: {{
    sourceKey: "source-1",
    sourceInlineEdit: "alias",
    value: "",
    trialKey: "trial-1"
  }},
  innerHTML: "<span>-</span>",
  addEventListener(type, handler) {{
    if (listeners[type]) listeners[type].push(handler);
  }},
  querySelector(selector) {{
    return probe.replaced && selector === "input" ? input : null;
  }},
  replaceChildren(node) {{
    probe.replaced = node === input;
  }},
  getAttribute(name) {{
    return name === "data-trial-key" ? "trial-1" : null;
  }}
}};
const target = {{
  querySelector() {{ return null; }},
  querySelectorAll(selector) {{
    if (selector === "[data-source-inline-edit]") return [cell];
    if (selector === "[data-trial-key]") return [cell];
    return [];
  }}
}};
const nodes = {{
  "peval-py-data": {{ textContent: "{{}}" }},
  "peval-py-i18n": {{ textContent: "{{}}" }},
  "peval-py-token-estimates": {{ textContent: "{{}}" }},
  "peval-py-render-options": {{ textContent: JSON.stringify({{
    mode: "serve",
    sources: [{{ source_key: "source-1", trial_key: "trial-1", active: true, artifact_dir: "runs/a", last_status: "ok" }}]
  }}) }},
}};
const context = {{
  document: {{
    body: {{ classList: {{ add() {{}}, remove() {{}}, toggle() {{}} }} }},
    addEventListener() {{}},
    createElement() {{ return input; }},
    getElementById(id) {{ return nodes[id] || null; }},
    querySelector() {{ return null; }},
    querySelectorAll() {{ return []; }},
  }},
  window: {{ addEventListener() {{}} }},
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
  requestAnimationFrame(callback) {{ callback(); }},
  target,
  listeners,
  probe,
}};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`(() => {{
  state.view = {{
    trajectory: [{{ session_id: "session-1", final_metrics: {{}} }}],
    trajectory_meta: [{{ trial_key: "trial-1", status: "passed", steps: [] }}]
  }};
  state.serveSources = [{{ source_key: "source-1", trial_key: "trial-1", active: true, artifact_dir: "runs/a", last_status: "ok" }}];
  renderComparisonPanels = () => {{ probe.renderCount += 1; }};
  bindInlineSourceEditors(target);
  bindTrialSelection(target);
  const clickEvent = {{ preventDefault() {{}}, stopPropagation() {{ this.stopped = true; }} }};
  listeners.click.forEach(handler => handler(clickEvent));
  const afterClick = {{ renderCount: probe.renderCount, replaced: probe.replaced, stopped: Boolean(clickEvent.stopped) }};
  const dblclickEvent = {{ preventDefault() {{ this.defaultPrevented = true; }}, stopPropagation() {{ this.stopped = true; }} }};
  listeners.dblclick.forEach(handler => handler(dblclickEvent));
  return JSON.stringify({{ afterClick, afterDoubleClick: {{ renderCount: probe.renderCount, replaced: probe.replaced, focused: probe.focused, selected: probe.selected, stopped: Boolean(dblclickEvent.stopped), defaultPrevented: Boolean(dblclickEvent.defaultPrevented) }} }});
}})()`, context);
console.log(result);
"""
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        result = json.loads(node.stdout)

        self.assertEqual(result["afterClick"]["renderCount"], 0)
        self.assertFalse(result["afterClick"]["replaced"])
        self.assertTrue(result["afterClick"]["stopped"])
        self.assertEqual(result["afterDoubleClick"]["renderCount"], 0)
        self.assertTrue(result["afterDoubleClick"]["replaced"])
        self.assertTrue(result["afterDoubleClick"]["focused"])
        self.assertTrue(result["afterDoubleClick"]["selected"])

    def test_leaderboard_row_click_opens_first_user_step_when_present(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:user",
                    "session_id": "with-user",
                    "steps": [
                        {"step_id": 1, "source": "agent", "message": "first"},
                        {"step_id": 2, "source": "User", "message": "open this"},
                        {"step_id": 3, "source": "user", "message": "not this"},
                    ],
                    "final_metrics": {},
                },
                {
                    "trajectory_id": "trial:no-user",
                    "session_id": "without-user",
                    "steps": [
                        {"step_id": 1, "source": "agent", "message": "only agent"},
                    ],
                    "final_metrics": {},
                },
            ],
            "trajectory_meta": [
                {"trial_key": "trial:user", "status": "passed", "steps": []},
                {"trial_key": "trial:no-user", "status": "passed", "steps": []},
            ],
        }
        script = f"""
const vm = require("vm");
const asset = {json.dumps(asset)};
const report = {json.dumps(report)};
const probe = {{ renderCount: 0 }};
function makeRow(trialKey) {{
  return {{
    listeners: {{}},
    addEventListener(type, handler) {{ this.listeners[type] = handler; }},
    getAttribute(name) {{ return name === "data-trial-key" ? trialKey : null; }}
  }};
}}
const userRow = makeRow("trial:user");
const noUserRow = makeRow("trial:no-user");
const root = {{
  querySelectorAll(selector) {{
    return selector === "tr[data-trial-key]" ? [userRow, noUserRow] : [];
  }}
}};
const context = {{
  document: {{
    body: {{ classList: {{ toggle() {{}} }} }},
    addEventListener() {{}},
    getElementById: () => null,
    querySelector: () => null,
    querySelectorAll: () => [],
  }},
  window: {{ addEventListener() {{}} }},
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
  requestAnimationFrame(callback) {{ callback(); }},
  report,
  probe,
  root,
  userRow,
  noUserRow,
}};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`(() => {{
  state.view = report;
  renderComparisonPanels = () => {{ probe.renderCount += 1; }};
  bindTrialSelection(root);
  const userEvent = {{ stopped: false, stopPropagation() {{ this.stopped = true; }} }};
  userRow.listeners.click(userEvent);
  const afterUser = {{
    selectedTrial: state.selectedTrial,
    selectedStep: state.selectedStep,
    stopped: userEvent.stopped,
    renderCount: probe.renderCount
  }};
  const noUserEvent = {{ stopped: false, stopPropagation() {{ this.stopped = true; }} }};
  noUserRow.listeners.click(noUserEvent);
  return JSON.stringify({{
    afterUser,
    afterNoUser: {{
      selectedTrial: state.selectedTrial,
      selectedStep: state.selectedStep,
      stopped: noUserEvent.stopped,
      renderCount: probe.renderCount
    }}
  }});
}})()`, context);
console.log(result);
"""
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        result = json.loads(node.stdout)

        self.assertEqual(result["afterUser"]["selectedTrial"], "trial:user")
        self.assertEqual(
            result["afterUser"]["selectedStep"],
            {"trialKey": "trial:user", "stepId": "2"},
        )
        self.assertTrue(result["afterUser"]["stopped"])
        self.assertEqual(result["afterUser"]["renderCount"], 1)
        self.assertEqual(result["afterNoUser"]["selectedTrial"], "trial:no-user")
        self.assertIsNone(result["afterNoUser"]["selectedStep"])
        self.assertTrue(result["afterNoUser"]["stopped"])
        self.assertEqual(result["afterNoUser"]["renderCount"], 2)

    def test_path_picker_fills_path_textarea_and_preserves_input_on_cancel_or_error(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = f"""
const vm = require("vm");
const asset = {json.dumps(asset)};
const statusNode = {{
  textContent: "",
  classList: {{ toggle() {{}} }}
}};
const field = {{ value: "existing" }};
const form = {{
  querySelector(selector) {{ return selector === "[name=\\"path\\"]" ? field : null; }}
}};
const button = {{
  closest(selector) {{ return selector === "[data-source-add-form]" ? form : null; }}
}};
const responses = [
  {{ ok: true, payload: {{ paths: ["/tmp/one.jsonl", "/tmp/two.json"] }} }},
  {{ ok: true, payload: {{ paths: [] }} }},
  {{ ok: false, statusText: "Service Unavailable", payload: {{ error: "native file picker unavailable" }} }},
];
const fetchCalls = [];
function makeResponse(item) {{
  return {{
    ok: item.ok,
    statusText: item.statusText || "OK",
    text() {{ return Promise.resolve(JSON.stringify(item.payload)); }}
  }};
}}
const context = {{
  document: {{
    body: {{ classList: {{ toggle() {{}} }} }},
    addEventListener() {{}},
    getElementById: () => null,
    querySelector(selector) {{
      if (selector === "[data-source-status]") return statusNode;
      return null;
    }},
    querySelectorAll: () => [],
  }},
  window: {{ addEventListener() {{}} }},
  fetch(path, options) {{
    fetchCalls.push({{ path, body: options.body, method: options.method }});
    return Promise.resolve(makeResponse(responses.shift()));
  }},
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
  requestAnimationFrame(callback) {{ callback(); }},
  statusNode,
  field,
  button,
  fetchCalls,
}};
vm.createContext(context);
vm.runInContext(asset, context);
const promise = vm.runInContext(`(async () => {{
  await choosePathSourceFiles(button);
  const afterSuccess = {{ value: field.value, status: statusNode.textContent }};
  field.value = "keep cancel";
  await choosePathSourceFiles(button);
  const afterCancel = {{ value: field.value, status: statusNode.textContent }};
  field.value = "keep error";
  await choosePathSourceFiles(button);
  return JSON.stringify({{
    afterSuccess,
    afterCancel,
    afterError: {{ value: field.value, status: statusNode.textContent }},
    fetchCalls
  }});
}})()`, context);
promise.then(result => console.log(result)).catch(error => {{ console.error(error && error.stack || error); process.exit(1); }});
"""
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        result = json.loads(node.stdout)

        self.assertEqual(result["afterSuccess"]["value"], "/tmp/one.jsonl\n/tmp/two.json")
        self.assertEqual(result["afterSuccess"]["status"], "Path selection updated")
        self.assertEqual(result["afterCancel"]["value"], "keep cancel")
        self.assertEqual(result["afterError"]["value"], "keep error")
        self.assertEqual(result["afterError"]["status"], "native file picker unavailable")
        self.assertEqual([call["path"] for call in result["fetchCalls"]], ["/api/path-picker"] * 3)
        self.assertTrue(
            all(json.loads(call["body"]) == {"multiple": True} for call in result["fetchCalls"])
        )

    def test_inline_source_tags_editor_can_toggle_existing_tags(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = f"""
const vm = require("vm");
const asset = {json.dumps(asset)};
const probe = {{ renderCount: 0, focused: false, selected: false }};
class Element {{
  constructor(tagName = "div") {{
    this.tagName = String(tagName).toUpperCase();
    this.children = [];
    this.dataset = {{}};
    this.attributes = {{}};
    this.listeners = {{}};
    this.className = "";
    this.value = "";
    this.textContent = "";
    this.innerHTML = "";
    this.type = "";
    this.classList = {{
      values: new Set(),
      toggle: (name, force) => {{
        const active = force === undefined ? !this.classList.values.has(name) : Boolean(force);
        if (active) this.classList.values.add(name);
        else this.classList.values.delete(name);
        return active;
      }},
      contains: name => this.classList.values.has(name)
    }};
  }}
  appendChild(node) {{
    this.children.push(node);
    node.parentNode = this;
    return node;
  }}
  replaceChildren(...nodes) {{
    this.children = nodes;
    nodes.forEach(node => {{ node.parentNode = this; }});
  }}
  addEventListener(type, handler) {{
    (this.listeners[type] ||= []).push(handler);
  }}
  dispatch(type) {{
    const event = {{
      defaultPrevented: false,
      stopped: false,
      preventDefault() {{ this.defaultPrevented = true; }},
      stopPropagation() {{ this.stopped = true; }}
    }};
    (this.listeners[type] || []).forEach(handler => handler(event));
    return event;
  }}
  setAttribute(name, value) {{
    this.attributes[name] = String(value);
  }}
  getAttribute(name) {{
    return this.attributes[name] || null;
  }}
  querySelector(selector) {{
    return this.querySelectorAll(selector)[0] || null;
  }}
  querySelectorAll(selector) {{
    const matches = node => {{
      if (selector === "input") return node.tagName === "INPUT";
      if (selector === "[data-source-tag-option]") return Object.prototype.hasOwnProperty.call(node.dataset, "sourceTagOption");
      return false;
    }};
    const found = [];
    const visit = node => {{
      if (matches(node)) found.push(node);
      node.children.forEach(visit);
    }};
    this.children.forEach(visit);
    return found;
  }}
  focus() {{ probe.focused = true; }}
  select() {{ probe.selected = true; }}
}}
const cell = new Element("span");
cell.dataset = {{
  sourceKey: "source-1",
  sourceInlineEdit: "tags",
  value: "green, custom",
  trialKey: "trial-1"
}};
const nodes = {{
  "peval-py-data": {{ textContent: "{{}}" }},
  "peval-py-i18n": {{ textContent: "{{}}" }},
  "peval-py-token-estimates": {{ textContent: "{{}}" }},
  "peval-py-render-options": {{ textContent: JSON.stringify({{
    mode: "serve",
    sources: [
      {{ source_key: "source-1", trial_key: "trial-1", active: true, artifact_dir: "runs/a", last_status: "ok", source_tags: ["green", "blue"] }},
      {{ source_key: "source-2", trial_key: "trial-2", active: false, artifact_dir: "runs/b", last_status: "ok", source_tags: ["red"] }}
    ]
  }}) }},
}};
const context = {{
  document: {{
    body: {{ classList: {{ add() {{}}, remove() {{}}, toggle() {{}} }} }},
    addEventListener() {{}},
    createElement(tagName) {{ return new Element(tagName); }},
    getElementById(id) {{ return nodes[id] || null; }},
    querySelector() {{ return null; }},
    querySelectorAll() {{ return []; }},
  }},
  window: {{ addEventListener() {{}} }},
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
  requestAnimationFrame(callback) {{ callback(); }},
  probe,
  cell,
}};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`(() => {{
  state.view = {{
    trajectory: [{{ session_id: "session-1", final_metrics: {{}} }}],
    trajectory_meta: [{{ trial_key: "trial-1", status: "passed", steps: [], source_tags: ["blue", "yellow"] }}]
  }};
  state.serveReportCache = {{
    archived: {{ trajectory_meta: [{{ trial_key: "trial-2", source_tags: ["purple"] }}] }}
  }};
  renderComparisonPanels = () => {{ probe.renderCount += 1; }};
  beginInlineSourceEdit(cell);
  const editor = cell.children[0];
  const input = editor.querySelector("input");
  const options = editor.querySelectorAll("[data-source-tag-option]");
  const labels = options.map(option => option.textContent);
  const before = {{
    value: input.value,
    labels,
    greenSelected: options.find(option => option.textContent === "green").classList.contains("selected"),
    blueSelected: options.find(option => option.textContent === "blue").classList.contains("selected")
  }};
  const blueClick = options.find(option => option.textContent === "blue").dispatch("click");
  const afterBlue = {{
    value: input.value,
    blueSelected: options.find(option => option.textContent === "blue").classList.contains("selected"),
    stopped: blueClick.stopped,
    defaultPrevented: blueClick.defaultPrevented
  }};
  const greenClick = options.find(option => option.textContent === "green").dispatch("click");
  input.value = input.value + ", manual";
  input.listeners.input.forEach(handler => handler({{}}));
  const redClick = options.find(option => option.textContent === "red").dispatch("click");
  return JSON.stringify({{
    before,
    afterBlue,
    afterGreenAndRed: {{
      value: input.value,
      greenSelected: options.find(option => option.textContent === "green").classList.contains("selected"),
      redSelected: options.find(option => option.textContent === "red").classList.contains("selected"),
      greenStopped: greenClick.stopped,
      redStopped: redClick.stopped
    }},
    renderCount: probe.renderCount,
    focused: probe.focused,
    selected: probe.selected
  }});
}})()`, context);
console.log(result);
"""
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        result = json.loads(node.stdout)

        self.assertEqual(result["before"]["value"], "green, custom")
        self.assertEqual(result["before"]["labels"], ["green", "blue", "red", "yellow", "purple"])
        self.assertTrue(result["before"]["greenSelected"])
        self.assertFalse(result["before"]["blueSelected"])
        self.assertEqual(result["afterBlue"]["value"], "green, custom, blue")
        self.assertTrue(result["afterBlue"]["blueSelected"])
        self.assertTrue(result["afterBlue"]["stopped"])
        self.assertTrue(result["afterBlue"]["defaultPrevented"])
        self.assertEqual(result["afterGreenAndRed"]["value"], "custom, blue, manual, red")
        self.assertFalse(result["afterGreenAndRed"]["greenSelected"])
        self.assertTrue(result["afterGreenAndRed"]["redSelected"])
        self.assertTrue(result["afterGreenAndRed"]["greenStopped"])
        self.assertTrue(result["afterGreenAndRed"]["redStopped"])
        self.assertEqual(result["renderCount"], 0)
        self.assertTrue(result["focused"])
        self.assertTrue(result["selected"])

    def test_leaderboard_search_and_tag_filters_use_serve_source_rows(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        sources = [
            {
                "source_key": "source-active",
                "active": True,
                "artifact_dir": "runs/a",
                "last_status": "ok",
                "trial_key": "trial:active",
                "source_tags": ["green"],
            },
            {
                "source_key": "source-archived",
                "active": False,
                "artifact_dir": "runs/b",
                "last_status": "ok",
                "trial_key": "trial:archived",
                "source_tags": ["red", "blue"],
            },
        ]
        report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:active",
                    "session_id": "active",
                    "steps": [{"step_id": 1, "source": "user", "message": "needle in message"}],
                    "final_metrics": {},
                },
                {
                    "trajectory_id": "trial:archived",
                    "session_id": "archived",
                    "steps": [
                        {
                            "step_id": 1,
                            "source": "agent",
                            "reasoning_content": "hidden thought",
                            "tool_calls": [{"function_name": "lookup", "arguments": {"q": "blue"}}],
                            "observation": {"content": "observed target"},
                        }
                    ],
                    "final_metrics": {},
                },
            ],
            "trajectory_meta": [
                {"trial_key": "trial:active", "status": "passed", "steps": [], "source_tags": ["green"]},
                {"trial_key": "trial:archived", "status": "passed", "steps": [], "source_tags": ["red", "blue"]},
            ],
        }
        script = f"""
const vm = require("vm");
const asset = {json.dumps(asset)};
const report = {json.dumps(report)};
const sources = {json.dumps(sources)};
const nodes = {{
  "peval-py-data": {{ textContent: "{{}}" }},
  "peval-py-i18n": {{ textContent: "{{}}" }},
  "peval-py-token-estimates": {{ textContent: "{{}}" }},
  "peval-py-render-options": {{ textContent: JSON.stringify({{ mode: "serve", sources }}) }},
}};
const context = {{
  document: {{
    body: {{ classList: {{ add() {{}}, remove() {{}}, toggle() {{}} }} }},
    addEventListener() {{}},
    getElementById(id) {{ return nodes[id] || null; }},
    querySelector() {{ return null; }},
    querySelectorAll() {{ return []; }},
  }},
  window: {{ addEventListener() {{}} }},
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
  requestAnimationFrame(callback) {{ callback(); }},
  report,
  sources,
}};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`(() => {{
  state.view = report;
  state.serveSources = sources;
  state.serveSourceMode = "all";
  state.search.scope = "all";
  state.search.query = "needle";
  const messageRows = leaderboardRows().map(row => [row.trial_key, row.source_key, row.source_tags]);
  state.search.query = "observed target";
  const observationRows = leaderboardRows().map(row => row.trial_key);
  state.search.query = "";
  setFilterValue("leaderboard", "source_tags", "blue", true);
  const tagRows = leaderboardRows().map(row => row.trial_key);
  return JSON.stringify({{ messageRows, observationRows, tagRows }});
}})()`, context);
console.log(result);
"""
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        result = json.loads(node.stdout)

        self.assertEqual(
            result["messageRows"],
            [["trial:active", "source-active", ["green"]]],
        )
        self.assertEqual(result["observationRows"], ["trial:archived"])
        self.assertEqual(result["tagRows"], ["trial:archived"])

    def test_markdown_renderer_renders_analysis_md_headings_tables_and_escapes(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        markdown = (
            "# Cached Review\n\n"
            "## Slow step\n\n"
            "This is **strong** and _emphasis_ with `inline_code`.\n\n"
            "| Check | Result | Count |\n"
            "| :--- | :---: | ---: |\n"
            "| <script>alert(1)</script> | **pass** | 3 |\n"
            "| Pipe \\| ok | _warn_ | 12 |\n\n"
            "Not | a table\n\n"
            "```\n"
            "| raw | code |\n"
            "```"
        )
        script = f"""
const vm = require("vm");
const asset = {json.dumps(asset)};
const markdown = {json.dumps(markdown)};
const context = {{
  document: {{
    body: {{ classList: {{ toggle() {{}} }} }},
    addEventListener() {{}},
    getElementById: () => null,
    querySelector: () => null,
    querySelectorAll: () => [],
  }},
  window: {{ addEventListener() {{}} }},
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
  markdown,
}};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`
  state.view = {{
    annotations: {{
      analysis: [{{ trial_key: "trial:md", status: "cached", md_report: markdown }}]
    }}
  }};
  JSON.stringify({{
    markdown: renderMarkdown(markdown),
    analysis: renderSelectedAnalysis("trial:md")
  }});
`, context);
console.log(result);
"""
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        result = json.loads(node.stdout)
        rendered = result["analysis"]
        self.assertIn('<h4 class="markdown-heading markdown-heading-1">Cached Review</h4>', rendered)
        self.assertIn('<h5 class="markdown-heading markdown-heading-2">Slow step</h5>', rendered)
        self.assertIn("<strong>strong</strong>", rendered)
        self.assertIn("<em>emphasis</em>", rendered)
        self.assertIn("<code>inline_code</code>", rendered)
        self.assertIn('<div class="markdown-table-wrap"><table class="markdown-table">', rendered)
        self.assertIn('<th class="align-left">Check</th>', rendered)
        self.assertIn('<th class="align-center">Result</th>', rendered)
        self.assertIn('<th class="align-right">Count</th>', rendered)
        self.assertIn("&lt;script&gt;alert(1)&lt;/script&gt;", rendered)
        self.assertIn("<strong>pass</strong>", rendered)
        self.assertIn("Pipe | ok", rendered)
        self.assertIn("<em>warn</em>", rendered)
        self.assertIn("<p>Not | a table</p>", rendered)
        self.assertIn('<pre class="note-code">| raw | code |</pre>', rendered)
        self.assertNotIn("<script>alert(1)</script>", rendered)

    def test_html_timeline_click_opens_drawer_for_single_session_report(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:single",
                    "session_id": "single",
                    "agent": {"name": "hermes", "model_name": "test-model"},
                    "steps": [
                        {"step_id": 1, "source": "user", "message": "run it"},
                        {
                            "step_id": 2,
                            "source": "agent",
                            "message": "reading",
                            "tool_calls": [
                                {
                                    "tool_call_id": "call-read",
                                    "function_name": "read",
                                    "arguments": {"file_path": "README.md"},
                                }
                            ],
                        },
                    ],
                    "final_metrics": {},
                }
            ],
            "trajectory_meta": [
                {
                    "trial_key": "trial:single",
                    "status": "passed",
                    "started_at_ms": 1_000,
                    "finished_at_ms": 1_200,
                    "duration_ms": 100,
                    "steps": [
                        {
                            "step_id": 1,
                            "timestamp_ms": 1_000,
                            "elapsed_ms": 0,
                            "duration_ms": None,
                            "tool_calls": [],
                            "observations": [],
                        },
                        {
                            "step_id": 2,
                            "timestamp_ms": 1_100,
                            "elapsed_ms": 100,
                            "duration_ms": 100,
                            "tool_calls": [
                                {
                                    "tool_call_id": "call-read",
                                    "title": "read",
                                    "timestamp_ms": 1_120,
                                    "execution_duration_ms": 50,
                                }
                            ],
                            "observations": [],
                        },
                    ],
                    "warnings": [],
                }
            ],
        }
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = f"""
const vm = require("vm");
const asset = {json.dumps(asset)};
const report = {json.dumps(report)};
const context = {{
  document: {{
    body: {{ classList: {{ toggle() {{}} }} }},
    addEventListener() {{}},
    getElementById: () => null,
    querySelector: () => null,
  }},
  window: {{ addEventListener() {{}} }},
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
  report,
  rendered: [],
}};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`
  state.view = report;
  state.selectedTrial = report.trajectory_meta[0].trial_key;
  renderLeaderboard = () => rendered.push("leaderboard");
  renderTrajectoryOverview = () => rendered.push("overview");
  renderTrace = () => rendered.push("trace");
  renderStepDrawer = () => rendered.push(state.selectedStep ? "drawer-open" : "drawer-closed");
  openTimelineStep({{ kind: "stage", trial_key: "trial:single", step_id: 2 }});
  const stageStep = state.selectedStep;
  openTimelineStep({{ kind: "marker", trial_key: "trial:single", step_id: 1 }});
  JSON.stringify({{ selectedTrial: state.selectedTrial, selectedStep: state.selectedStep, stageStep, rendered }});
`, context);
console.log(result);
"""
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        result = json.loads(node.stdout)

        self.assertEqual(result["selectedTrial"], "trial:single")
        self.assertEqual(
            result["stageStep"],
            {"trialKey": "trial:single", "stepId": "2"},
        )
        self.assertEqual(
            result["selectedStep"],
            {"trialKey": "trial:single", "stepId": "1"},
        )
        self.assertIn("drawer-open", result["rendered"])


    def test_html_trajectory_overview_nodes_render_duration_heat(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:overview",
                    "session_id": "overview",
                    "agent": {"name": "psychevo"},
                    "steps": [
                        {"step_id": 1, "source": "user", "message": "start"},
                        {"step_id": 2, "source": "agent", "message": "fast"},
                        {"step_id": 3, "source": "agent", "message": "slow"},
                    ],
                    "final_metrics": {},
                },
                {
                    "trajectory_id": "trial:overview-2",
                    "session_id": "overview-2",
                    "agent": {"name": "psychevo"},
                    "steps": [
                        {"step_id": 1, "source": "user", "message": "start"},
                    ],
                    "final_metrics": {},
                }
            ],
            "trajectory_meta": [
                {
                    "trial_key": "trial:overview",
                    "status": "passed",
                    "steps": [
                        {"step_id": 1, "duration_ms": 0},
                        {"step_id": 2, "duration_ms": 120},
                        {"step_id": 3, "duration_ms": 240},
                    ],
                    "warnings": [],
                },
                {
                    "trial_key": "trial:overview-2",
                    "status": "passed",
                    "steps": [
                        {"step_id": 1, "duration_ms": 0},
                    ],
                    "warnings": [],
                }
            ],
        }
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = f"""
const vm = require("vm");
const asset = {json.dumps(asset)};
const report = {json.dumps(report)};
const context = {{
  document: {{
    body: {{ classList: {{ toggle() {{}} }} }},
    addEventListener() {{}},
    getElementById: () => null,
    querySelector: () => null,
  }},
  window: {{ addEventListener() {{}} }},
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
  report,
}};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`
  state.view = report;
  state.selectedTrial = "trial:overview";
  state.selectedStep = {{ trialKey: "trial:overview", stepId: "3" }};
  renderTrajectoryOverviewRow(reportRows()[0]);
`, context);
console.log(result);
"""
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        row_html = node.stdout
        buttons = {
            match.group("id"): match.group("tag")
            for match in re.finditer(
                r'(?P<tag><button class="[^"]*"[^>]*data-step-id="(?P<id>[^"]+)"[^>]*>)',
                row_html,
            )
        }

        self.assertIn("1", buttons)
        self.assertIn("2", buttons)
        self.assertIn("3", buttons)
        self.assertNotIn("duration-heat-", buttons["1"])
        self.assertNotIn("--time-pct", buttons["1"])
        self.assertIn("step 0.0s", buttons["1"])
        self.assertIn("duration-heat-5", buttons["2"])
        self.assertNotIn("--time-pct", buttons["2"])
        self.assertIn("duration-heat-10", buttons["3"])
        self.assertIn("selected-node", buttons["3"])
        self.assertNotIn("--time-pct", buttons["3"])
        self.assertIn("step 0.2s; 100% of slowest step", buttons["3"])

    def test_html_runtime_rows_and_export_subset_avoid_persisted_comparison(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:one",
                    "session_id": "one",
                    "agent": {"name": "agent-a", "model_name": "model-a"},
                    "steps": [],
                    "final_metrics": {
                        "total_prompt_tokens": 80,
                        "total_completion_tokens": 40,
                        "total_cost_usd": 0.03,
                        "extra": {
                            "total_turns": 2,
                            "total_tool_calls": 4,
                            "total_tool_errors": 1,
                        },
                    },
                },
                {
                    "trajectory_id": "trial:two",
                    "session_id": "two",
                    "agent": {"name": "agent-b", "model_name": "model-b"},
                    "steps": [],
                    "final_metrics": {
                        "extra": {
                            "total_turns": 1,
                            "total_tool_calls": 0,
                            "total_tool_errors": 0,
                        },
                    },
                },
            ],
            "trajectory_meta": [
                {
                    "trial_key": "trial:one",
                    "adapter": "psychevo",
                    "status": "passed",
                    "finished_at_ms": 300,
                    "duration_ms": 100,
                    "wall_duration_ms": 300,
                    "warnings": ["warn"],
                    "steps": [],
                },
                {
                    "trial_key": "trial:two",
                    "adapter": "opencode",
                    "status": "failed",
                    "finished_at_ms": 500,
                    "duration_ms": 50,
                    "wall_duration_ms": 500,
                    "warnings": [],
                    "steps": [],
                },
            ],
            "annotations": {
                "report_notes": [],
                "notes": [{"trial_key": "trial:one", "markdown": "keep"}],
                "analysis": [
                    {
                        "trial_key": "trial:one",
                        "status": "cached",
                        "relative_paths": {
                            "json": "runs/default/agent-a/one/trial_one/analysis.json",
                            "md": "runs/default/agent-a/one/trial_one/analysis.md",
                        },
                    },
                    {"trial_key": "trial:two", "status": "computed"},
                ],
            },
        }
        legacy_report = {
            "schema_version": 19,
            "includes": ["core", "comparison"],
            "trajectory": [],
            "trajectory_meta": [],
            "comparison": {
                "leaderboard": {
                    "entries": [{"trial_key": "trial:single", "adapter": "legacy"}]
                }
            },
        }
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = f"""
const vm = require("vm");
const asset = {json.dumps(asset)};
const report = {json.dumps(report)};
const legacyReport = {json.dumps(legacy_report)};
class BlobStub {{
  constructor(parts, options = {{}}) {{
    this.parts = parts;
    this.type = options.type || "";
    this.size = parts.reduce((total, part) => total + (part.length || part.byteLength || String(part).length), 0);
  }}
}}
const context = {{
  document: {{
    body: {{ classList: {{ toggle() {{}} }} }},
    addEventListener() {{}},
    getElementById: () => null,
    querySelector: () => null,
  }},
  window: {{ addEventListener() {{}} }},
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
  TextEncoder,
  Uint8Array,
  DataView,
  Buffer,
  Blob: BlobStub,
  report,
  legacyReport,
}};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`
  state.view = report;
  const rows = reportRows();
  const subset = reportSubset(rows);
  const analysisedValues = rows.map(row => rowAnalysised(row));
  const analysisedColumn = leaderboardColumns().find(column => column.key === "analysised");
  const analysisedFilterable = Boolean(analysisedColumn?.filterable);
  const analysisedOptions = filterOptions(analysisedColumn, reportRows());
  setFilterValue("leaderboard", "analysised", "True", true);
  const trueFilteredKeys = leaderboardRows().map(row => row.trial_key);
  clearFilter("leaderboard", "analysised");
  setFilterValue("leaderboard", "analysised", "False", true);
  const falseFilteredKeys = leaderboardRows().map(row => row.trial_key);
  clearFilter("leaderboard", "analysised");
  const xlsxBytes = xlsxBytesForRows(rows);
  const xlsxText = Buffer.from(xlsxBytes).toString("utf8");
  let downloaded = null;
  downloadBlob = (filename, mime, blob) => {{
    downloaded = {{ filename, mime, type: blob.type, size: blob.size }};
  }};
  exportCurrentScope("xlsx");
  state.view = legacyReport;
  const legacyRows = reportRows();
  JSON.stringify({{
    rowCount: rows.length,
    firstAdapter: rows[0].adapter,
    firstErrorRate: rowToolErrorRate(rows[0]),
    analysisedValues,
    analysisedFilterable,
    analysisedOptions,
    trueFilteredKeys,
    falseFilteredKeys,
    pathChecks: [
      isAnalysisArtifactPath("runs/default/agent/session/cell/analysis.md"),
      isAnalysisArtifactPath("runs/default/agent/session/cell/analysis.json"),
      isAnalysisArtifactPath("runs/default/agent/session/cell/notes.md")
    ],
    xlsxZipMagic: [xlsxBytes[0], xlsxBytes[1], xlsxBytes[2], xlsxBytes[3]],
    xlsxHasHeader: xlsxText.includes("Analysised"),
    xlsxHasTrue: xlsxText.includes("<t>True</t>"),
    xlsxHasFalse: xlsxText.includes("<t>False</t>"),
    downloaded,
    subsetHasComparison: Object.prototype.hasOwnProperty.call(subset, "comparison"),
    subsetIncludes: subset.includes,
    subsetNotes: subset.annotations.notes.map(note => note.markdown),
    subsetAnalysisKeys: subset.annotations.analysis.map(item => item.trial_key),
    legacyRowCount: legacyRows.length
  }});
`, context);
console.log(result);
"""
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        result = json.loads(node.stdout)
        self.assertEqual(result["rowCount"], 2)
        self.assertEqual(result["firstAdapter"], "psychevo")
        self.assertAlmostEqual(result["firstErrorRate"], 0.25)
        self.assertEqual(result["analysisedValues"], ["True", "False"])
        self.assertTrue(result["analysisedFilterable"])
        self.assertEqual(result["analysisedOptions"], ["False", "True"])
        self.assertEqual(result["trueFilteredKeys"], ["trial:one"])
        self.assertEqual(result["falseFilteredKeys"], ["trial:two"])
        self.assertEqual(result["pathChecks"], [True, True, False])
        self.assertEqual(result["xlsxZipMagic"], [80, 75, 3, 4])
        self.assertTrue(result["xlsxHasHeader"])
        self.assertTrue(result["xlsxHasTrue"])
        self.assertTrue(result["xlsxHasFalse"])
        self.assertEqual(result["downloaded"]["filename"], "peval-leaderboard-visible.xlsx")
        self.assertEqual(
            result["downloaded"]["mime"],
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        )
        self.assertEqual(
            result["downloaded"]["type"],
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        )
        self.assertGreater(result["downloaded"]["size"], 0)
        self.assertFalse(result["subsetHasComparison"])
        self.assertEqual(result["subsetIncludes"], ["core"])
        self.assertEqual(result["subsetNotes"], ["keep"])
        self.assertEqual(result["subsetAnalysisKeys"], ["trial:one", "trial:two"])
        self.assertEqual(result["legacyRowCount"], 0)

    def test_leaderboard_summary_uses_filtered_visible_rows(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:alpha",
                    "session_id": "alpha",
                    "agent": {"name": "agent-a", "model_name": "model-a"},
                    "steps": [
                        {"step_id": 1, "source": "user"},
                        {"step_id": 2, "source": "agent"},
                        {"step_id": 3, "source": "assistant"},
                        {"step_id": 4, "source": "tool"},
                    ],
                    "final_metrics": {
                        "total_prompt_tokens": 60,
                        "total_completion_tokens": 40,
                        "extra": {
                            "total_turns": 2,
                            "total_tool_calls": 2,
                            "total_tool_errors": 0,
                        },
                    },
                },
                {
                    "trajectory_id": "trial:beta",
                    "session_id": "beta",
                    "agent": {"name": "agent-b", "model_name": "model-b"},
                    "steps": [
                        {"step_id": 1, "source": "assistant"},
                    ],
                    "final_metrics": {
                        "total_prompt_tokens": 150,
                        "total_completion_tokens": 50,
                        "extra": {
                            "total_turns": 4,
                            "total_tool_calls": 4,
                            "total_tool_errors": 2,
                        },
                    },
                },
                {
                    "trajectory_id": "trial:gamma",
                    "session_id": "gamma",
                    "agent": {"name": "agent-c", "model_name": "model-c"},
                    "steps": [
                        {"step_id": 1, "source": "assistant"},
                        {"step_id": 2, "source": "agent"},
                    ],
                    "final_metrics": {
                        "total_prompt_tokens": 220,
                        "total_completion_tokens": 80,
                        "extra": {
                            "total_turns": 6,
                            "total_tool_calls": 0,
                            "total_tool_errors": 0,
                        },
                    },
                },
            ],
            "trajectory_meta": [
                {
                    "trial_key": "trial:alpha",
                    "status": "passed",
                    "duration_ms": 2000,
                    "steps": [
                        {"step_id": 1, "duration_ms": 100},
                        {"step_id": 2, "duration_ms": 1000, "duration_source": "measured"},
                        {"step_id": 3, "duration_ms": 2000, "duration_source": "boundary_estimate"},
                        {"step_id": 4, "duration_ms": 500},
                    ],
                    "warnings": [],
                },
                {
                    "trial_key": "trial:beta",
                    "status": "failed",
                    "duration_ms": 3000,
                    "steps": [
                        {"step_id": 1, "duration_ms": 3000, "duration_source": "measured"},
                    ],
                    "warnings": [],
                },
                {
                    "trial_key": "trial:gamma",
                    "status": "passed",
                    "duration_ms": 6000,
                    "steps": [
                        {"step_id": 1, "duration_ms": 500, "duration_source": "measured"},
                        {"step_id": 2, "duration_ms": None, "duration_source": "measured"},
                    ],
                    "warnings": [],
                },
            ],
        }
        single_report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [{"trajectory_id": "trial:single", "session_id": "single", "steps": []}],
            "trajectory_meta": [{"trial_key": "trial:single", "status": "passed", "steps": []}],
        }
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = """
const vm = require("vm");
const asset = __ASSET__;
const report = __REPORT__;
const singleReport = __SINGLE_REPORT__;
const nodes = {
  "peval-py-i18n": { textContent: "{}" },
  "peval-py-token-estimates": { textContent: "{}" },
  "peval-py-render-options": { textContent: JSON.stringify({ mode: "report" }) },
  "leaderboard-summary": { innerHTML: "" },
  "comparison": { innerHTML: "" },
};
const context = {
  nodes,
  document: {
    body: { classList: { toggle() {} } },
    addEventListener() {},
    getElementById(id) { return nodes[id] || null; },
    querySelector: () => null,
    querySelectorAll: () => [],
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
  report,
  singleReport,
};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`
  function byKey(rows) {
    return Object.fromEntries(rows.map(row => [row.key, row]));
  }
  state.view = report;
  state.selectedTrial = "trial:alpha";
  state.rowSelection.add("trial:beta");
  setFilterValue("leaderboard", "status", "passed", true);
  const rows = leaderboardRows();
  renderLeaderboardSummary(rows);
  const summary = byKey(leaderboardSummaryRows(rows));
  const selectionProof = byKey(leaderboardSummaryRows(leaderboardRows()));

  const originalRenderComparisonPanels = renderComparisonPanels;
  const comparisonCalls = [];
  renderComparisonPanels = options => comparisonCalls.push(options);
  nodes.comparison.innerHTML = "";
  renderComparison();
  const multiHtml = nodes.comparison.innerHTML;
  state.view = singleReport;
  nodes.comparison.innerHTML = "sentinel";
  renderComparison();
  const singleHtml = nodes.comparison.innerHTML;
  const singleRows = reportRows();
  clearFilter("leaderboard", "status");
  setFilterValue("leaderboard", "status", "failed", true);
  const singleFilteredRows = leaderboardRows();
  renderComparisonPanels = originalRenderComparisonPanels;

  JSON.stringify({
    visibleKeys: rows.map(row => row.trial_key),
    duration: summary.duration_ms,
    tokens: summary.tokens,
    model: summary.model_duration_ms,
    toolCalls: summary.total_tool_calls,
    toolRate: summary.tool_error_rate,
    selectedDurationTotal: selectionProof.duration_ms.total,
    html: nodes["leaderboard-summary"].innerHTML,
    multiHtml,
    singleHtml,
    singleRows: singleRows.map(row => row.trial_key),
    singleFilteredRows: singleFilteredRows.map(row => row.trial_key),
    comparisonCalls,
  });
`, context);
console.log(result);
""".replace("__ASSET__", json.dumps(asset)).replace("__REPORT__", json.dumps(report)).replace("__SINGLE_REPORT__", json.dumps(single_report))
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        result = json.loads(node.stdout)

        self.assertEqual(result["visibleKeys"], ["trial:alpha", "trial:gamma"])
        self.assertEqual(result["duration"]["count"], 2)
        self.assertEqual(result["duration"]["missing"], 0)
        self.assertEqual(result["duration"]["total"], 8000)
        self.assertEqual(result["tokens"]["total"], 400)
        self.assertEqual(result["model"]["count"], 2)
        self.assertEqual(result["model"]["missing"], 0)
        self.assertEqual(result["model"]["total"], 1500)
        self.assertEqual(result["toolCalls"]["total"], 2)
        self.assertEqual(result["toolRate"]["count"], 1)
        self.assertEqual(result["toolRate"]["missing"], 1)
        self.assertIsNone(result["toolRate"]["total"])
        self.assertEqual(result["toolRate"]["mean"], 0)
        self.assertEqual(result["selectedDurationTotal"], 8000)
        self.assertIn("Leaderboard Summary", result["html"])
        self.assertIn("Leaderboard Summary Distributions", result["html"])
        self.assertIn("<th>Statistic</th>", result["html"])
        self.assertIn('<th scope="row">Count</th>', result["html"])
        self.assertIn('<th scope="row">Missing</th>', result["html"])
        self.assertIn('<th scope="row">Total</th>', result["html"])
        self.assertIn('<th scope="row">P95</th>', result["html"])
        self.assertIn('<th class="num">Active Duration</th>', result["html"])
        self.assertIn("Model call duration", result["html"])
        self.assertIn("summary-boxplot", result["html"])
        self.assertIn("summary-boxplot-card", result["html"])
        self.assertIn("summary-boxplot-vertical", result["html"])
        self.assertIn("summary-boxplot-flat", result["html"])
        self.assertIn("--summary-whisker-bottom", result["html"])
        self.assertIn("--summary-p95-bottom", result["html"])
        self.assertIn("0.0%", result["html"])
        self.assertIn("<td class=\"num\">-</td>", result["html"])
        self.assertNotIn("No visible rows to summarize.", result["html"])
        self.assertNotIn("leaderboard-summary-count", result["html"])
        self.assertNotIn("leaderboard-summary-distribution", result["html"])
        self.assertNotIn("--summary-p95-left", result["html"])
        self.assertIn('id="leaderboard-summary"', result["multiHtml"])
        self.assertIn('id="leaderboard"', result["singleHtml"])
        self.assertIn('id="trajectory-overview"', result["singleHtml"])
        self.assertNotIn('id="leaderboard-summary"', result["singleHtml"])
        self.assertEqual(result["singleRows"], ["trial:single"])
        self.assertEqual(result["singleFilteredRows"], [])
        self.assertEqual(result["comparisonCalls"], [{"trace": False}, {"trace": False}])

    def test_serve_source_selection_uses_full_report_uniquified_trials(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        sources = [
            {
                "source_key": "source-a",
                "active": True,
                "artifact_dir": "runs/default/a/session_t001",
                "last_status": "ok",
                "trial_key": "session:t001",
            },
            {
                "source_key": "source-b",
                "active": True,
                "artifact_dir": "runs/default/b/session_t001",
                "last_status": "ok",
                "trial_key": "session:t001",
            },
            {
                "source_key": "source-c",
                "active": True,
                "artifact_dir": "runs/default/c/session_t001",
                "last_status": "ok",
                "trial_key": "session:t001",
            },
        ]
        report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:a",
                    "session_id": "a",
                    "steps": [],
                    "final_metrics": {"extra": {"total_turns": 1, "total_tool_calls": 0}},
                },
                {
                    "trajectory_id": "trial:b",
                    "session_id": "b",
                    "steps": [],
                    "final_metrics": {"extra": {"total_turns": 1, "total_tool_calls": 0}},
                },
                {
                    "trajectory_id": "trial:c",
                    "session_id": "c",
                    "steps": [],
                    "final_metrics": {"extra": {"total_turns": 1, "total_tool_calls": 0}},
                },
            ],
            "trajectory_meta": [
                {"trial_key": "session:t001", "status": "passed", "duration_ms": 100, "steps": [], "warnings": []},
                {"trial_key": "session:t001:2", "status": "passed", "duration_ms": 200, "steps": [], "warnings": []},
                {"trial_key": "session:t001:3", "status": "failed", "duration_ms": 300, "steps": [], "warnings": []},
            ],
        }
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = """
const vm = require("vm");
const asset = __ASSET__;
const report = __REPORT__;
const sources = __SOURCES__;
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
nodes["peval-py-data"].textContent = "{}";
nodes["peval-py-i18n"].textContent = "{}";
nodes["peval-py-token-estimates"].textContent = "{}";
nodes["peval-py-render-options"].textContent = JSON.stringify({ mode: "serve", sources: [] });
const context = {
  nodes,
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
  fetch() { throw new Error("source selection must not fetch a single-source report"); },
  report,
  sources,
};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`
  applyServeMutationPayload({ sources, report, report_source_key: "source-b" });
  const afterMutation = {
    selectedTrial: state.selectedTrial,
    selectedSourceKey: state.selectedSourceKey,
    reportRows: reportRows().length,
    hasLeaderboard: nodes.leaderboard.innerHTML.includes("Leaderboard"),
    hasSummary: nodes["leaderboard-summary"].innerHTML.includes("Leaderboard Summary"),
    mappedSecond: sourceKeyForTrialKey("session:t001:2"),
  };
  state.rowSelection.add("session:t001:2");
  selectServeSource("source-c");
  const afterSourceSelect = {
    selectedTrial: state.selectedTrial,
    selectedSourceKey: state.selectedSourceKey,
    reportRows: reportRows().length,
    rowSelectionKept: state.rowSelection.has("session:t001:2"),
    hasLeaderboard: nodes.leaderboard.innerHTML.includes("Leaderboard"),
    hasSummary: nodes["leaderboard-summary"].innerHTML.includes("Leaderboard Summary"),
    hasOverview: nodes["trajectory-overview"].innerHTML.includes("Trajectory Overview"),
  };
  loadServeSourceReport("source-a");
  const afterLegacyLoadName = {
    selectedTrial: state.selectedTrial,
    selectedSourceKey: state.selectedSourceKey,
    reportRows: reportRows().length,
  };
  JSON.stringify({ afterMutation, afterSourceSelect, afterLegacyLoadName });
`, context);
console.log(result);
""".replace("__ASSET__", json.dumps(asset)).replace("__REPORT__", json.dumps(report)).replace("__SOURCES__", json.dumps(sources))
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        result = json.loads(node.stdout)

        self.assertEqual(result["afterMutation"]["selectedTrial"], "session:t001:2")
        self.assertEqual(result["afterMutation"]["selectedSourceKey"], "source-b")
        self.assertEqual(result["afterMutation"]["reportRows"], 3)
        self.assertTrue(result["afterMutation"]["hasLeaderboard"])
        self.assertTrue(result["afterMutation"]["hasSummary"])
        self.assertEqual(result["afterMutation"]["mappedSecond"], "source-b")
        self.assertEqual(result["afterSourceSelect"]["selectedTrial"], "session:t001:3")
        self.assertEqual(result["afterSourceSelect"]["selectedSourceKey"], "source-c")
        self.assertEqual(result["afterSourceSelect"]["reportRows"], 3)
        self.assertTrue(result["afterSourceSelect"]["rowSelectionKept"])
        self.assertTrue(result["afterSourceSelect"]["hasLeaderboard"])
        self.assertTrue(result["afterSourceSelect"]["hasSummary"])
        self.assertTrue(result["afterSourceSelect"]["hasOverview"])
        self.assertEqual(result["afterLegacyLoadName"]["selectedTrial"], "session:t001")
        self.assertEqual(result["afterLegacyLoadName"]["selectedSourceKey"], "source-a")
        self.assertEqual(result["afterLegacyLoadName"]["reportRows"], 3)

    def test_serve_archived_mode_lazy_loads_and_batches_visible_selection(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        sources = [
            {"source_key": "source-a", "active": True, "artifact_dir": "runs/a", "last_status": "ok", "trial_key": "trial:active-a"},
            {"source_key": "source-b", "active": True, "artifact_dir": "runs/b", "last_status": "ok", "trial_key": "trial:active-b"},
            {"source_key": "source-c", "active": True, "artifact_dir": "runs/c", "last_status": "ok", "trial_key": "trial:active-c"},
            {"source_key": "source-d", "active": False, "artifact_dir": "runs/d", "last_status": "ok", "trial_key": "trial:archived-d"},
        ]
        sources_after_archive = [
            {**sources[0], "active": False},
            sources[1],
            sources[2],
            sources[3],
        ]
        active_report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {"trajectory_id": "trial:active-a", "session_id": "active-a", "steps": [], "final_metrics": {"extra": {"total_turns": 1}}},
                {"trajectory_id": "trial:active-b", "session_id": "active-b", "steps": [], "final_metrics": {"extra": {"total_turns": 2}}},
                {"trajectory_id": "trial:active-c", "session_id": "active-c", "steps": [], "final_metrics": {"extra": {"total_turns": 3}}},
            ],
            "trajectory_meta": [
                {"trial_key": "trial:active-a", "status": "passed", "duration_ms": 1000, "steps": [], "warnings": []},
                {"trial_key": "trial:active-b", "status": "failed", "duration_ms": 2000, "steps": [], "warnings": []},
                {"trial_key": "trial:active-c", "status": "passed", "duration_ms": 3000, "steps": [], "warnings": []},
            ],
        }
        active_after_archive = {
            **active_report,
            "trajectory": active_report["trajectory"][1:],
            "trajectory_meta": active_report["trajectory_meta"][1:],
        }
        archived_report = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {"trajectory_id": "trial:archived-d", "session_id": "archived-d", "steps": [], "final_metrics": {"extra": {"total_turns": 4}}},
            ],
            "trajectory_meta": [
                {"trial_key": "trial:archived-d", "status": "passed", "duration_ms": 4000, "steps": [], "warnings": []},
            ],
        }
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = """
const vm = require("vm");
const asset = __ASSET__;
const activeReport = __ACTIVE_REPORT__;
const archivedReport = __ARCHIVED_REPORT__;
const sources = __SOURCES__;
const sourcesAfterArchive = __SOURCES_AFTER_ARCHIVE__;
const activeAfterArchive = __ACTIVE_AFTER_ARCHIVE__;
const nodes = {};
function makeNode(id) {
  const node = {
    id,
    textContent: "",
    hidden: false,
    dataset: {},
    classList: { add() {}, remove() {}, toggle() {} },
    addEventListener() {},
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
  "peval-py-i18n",
  "peval-py-token-estimates",
  "peval-py-render-options",
  "report-notes",
  "comparison",
  "trace",
  "step-drawer",
].forEach(id => nodes[id] = makeNode(id));
nodes["peval-py-i18n"].textContent = "{}";
nodes["peval-py-token-estimates"].textContent = "{}";
nodes["peval-py-render-options"].textContent = JSON.stringify({ mode: "serve", sources });
const fetchCalls = [];
function response(payload) {
  return { ok: true, statusText: "OK", text: async () => JSON.stringify(payload) };
}
const context = {
  nodes,
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
  fetch: async (path, options = {}) => {
    const body = options.body ? JSON.parse(options.body) : null;
    fetchCalls.push({ path, body });
    if (String(path).includes("source_state=archived")) return response(archivedReport);
    if (String(path) === "/api/sources/state") {
      return response({
        sources: sourcesAfterArchive,
        report: activeAfterArchive,
        report_source_key: "source-b",
        report_source_state: "active",
      });
    }
    throw new Error(`unexpected fetch ${path}`);
  },
  activeReport,
  archivedReport,
  sources,
  sourcesAfterArchive,
  activeAfterArchive,
  fetchCalls,
};
vm.createContext(context);
vm.runInContext(asset, context);
const promise = vm.runInContext(`(async () => {
  applyServeMutationPayload({ sources, report: activeReport, report_source_key: "source-a", report_source_state: "active" });
  const initial = {
    mode: state.serveSourceMode,
    leaderboardControls: (nodes.leaderboard.innerHTML.match(/data-source-state-controls/g) || []).length,
    overviewControls: (nodes["trajectory-overview"].innerHTML.match(/data-source-state-controls/g) || []).length,
    actionLabel: nodes.leaderboard.innerHTML.includes("Archive selected"),
    archivedToggleEnabled: !nodes.leaderboard.innerHTML.includes("data-source-state-toggle  disabled"),
    overviewCheckboxes: (nodes["trajectory-overview"].innerHTML.match(/data-row-select/g) || []).length,
  };
  await switchServeSourceMode("archived");
  const afterArchived = {
    mode: state.serveSourceMode,
    reportRows: reportRows().length,
    selectedSourceKey: state.selectedSourceKey,
    checkedInLeaderboard: nodes.leaderboard.innerHTML.includes("data-source-state-toggle checked"),
    checkedInOverview: nodes["trajectory-overview"].innerHTML.includes("data-source-state-toggle checked"),
    actionLabel: nodes.leaderboard.innerHTML.includes("Activate selected"),
    archivedFetches: fetchCalls.filter(call => String(call.path).includes("source_state=archived")).length,
    hasSummary: nodes.comparison.innerHTML.includes('id="leaderboard-summary"'),
  };
  await switchServeSourceMode("active");
  await switchServeSourceMode("archived");
  const cachedFetches = fetchCalls.filter(call => String(call.path).includes("source_state=archived")).length;
  await switchServeSourceMode("active");
  setFilterValue("leaderboard", "status", "passed", true);
  state.rowSelection.add("trial:active-a");
  state.rowSelection.add("trial:active-b");
  renderComparisonPanels({ trace: false });
  const activeSingleSelection = {
    actionEnabled: nodes.leaderboard.innerHTML.includes("data-source-state-action >Archive selected"),
    overviewChecked: nodes["trajectory-overview"].innerHTML.includes('data-row-select="trial:active-a" checked'),
  };
  await mutateVisibleServeSourceState();
  const statePost = fetchCalls.find(call => call.path === "/api/sources/state");
  const afterArchive = {
    mode: state.serveSourceMode,
    reportRows: reportRows().length,
    selectedSourceKey: state.selectedSourceKey,
    rowSelectionSize: state.rowSelection.size,
    statePayload: statePost.body,
  };
  const callsBeforeUnavailable = fetchCalls.length;
  state.serveSources = [sources[0], sources[1], sources[2]];
  state.serveReportCache = { active: activeReport };
  state.serveSourceMode = "active";
  render(activeReport);
  const zeroTargetDisabled = nodes.leaderboard.innerHTML.includes("data-source-state-toggle  disabled");
  await switchServeSourceMode("archived");
  const unavailable = {
    mode: state.serveSourceMode,
    fetchUnchanged: fetchCalls.length === callsBeforeUnavailable,
    zeroTargetDisabled,
  };
  return JSON.stringify({ initial, afterArchived, cachedFetches, activeSingleSelection, afterArchive, unavailable });
})()`, context);
promise.then(result => console.log(result)).catch(error => { console.error(error && error.stack || error); process.exit(1); });
""".replace("__ASSET__", json.dumps(asset)).replace("__ACTIVE_REPORT__", json.dumps(active_report)).replace("__ARCHIVED_REPORT__", json.dumps(archived_report)).replace("__SOURCES__", json.dumps(sources)).replace("__SOURCES_AFTER_ARCHIVE__", json.dumps(sources_after_archive)).replace("__ACTIVE_AFTER_ARCHIVE__", json.dumps(active_after_archive))
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        result = json.loads(node.stdout)

        self.assertEqual(result["initial"]["mode"], "active")
        self.assertEqual(result["initial"]["leaderboardControls"], 1)
        self.assertEqual(result["initial"]["overviewControls"], 1)
        self.assertTrue(result["initial"]["actionLabel"])
        self.assertTrue(result["initial"]["archivedToggleEnabled"])
        self.assertEqual(result["initial"]["overviewCheckboxes"], 3)
        self.assertEqual(result["afterArchived"]["mode"], "archived")
        self.assertEqual(result["afterArchived"]["reportRows"], 1)
        self.assertEqual(result["afterArchived"]["selectedSourceKey"], "source-d")
        self.assertTrue(result["afterArchived"]["checkedInLeaderboard"])
        self.assertTrue(result["afterArchived"]["checkedInOverview"])
        self.assertTrue(result["afterArchived"]["actionLabel"])
        self.assertEqual(result["afterArchived"]["archivedFetches"], 1)
        self.assertFalse(result["afterArchived"]["hasSummary"])
        self.assertEqual(result["cachedFetches"], 1)
        self.assertTrue(result["activeSingleSelection"]["actionEnabled"])
        self.assertTrue(result["activeSingleSelection"]["overviewChecked"])
        self.assertEqual(result["afterArchive"]["mode"], "active")
        self.assertEqual(result["afterArchive"]["reportRows"], 2)
        self.assertEqual(result["afterArchive"]["selectedSourceKey"], "source-b")
        self.assertEqual(result["afterArchive"]["rowSelectionSize"], 0)
        self.assertEqual(result["afterArchive"]["statePayload"]["source_keys"], ["source-a"])
        self.assertFalse(result["afterArchive"]["statePayload"]["active"])
        self.assertEqual(result["afterArchive"]["statePayload"]["report_source_state"], "active")
        self.assertEqual(result["unavailable"]["mode"], "active")
        self.assertTrue(result["unavailable"]["fetchUnchanged"])
        self.assertTrue(result["unavailable"]["zeroTargetDisabled"])

    def test_serve_source_state_auto_switches_when_current_mode_becomes_empty(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        sources = [
            {"source_key": "source-a", "active": True, "artifact_dir": "runs/a", "last_status": "ok", "trial_key": "trial:active-a"},
            {"source_key": "source-d", "active": False, "artifact_dir": "runs/d", "last_status": "ok", "trial_key": "trial:archived-d"},
        ]
        active_single = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {"trajectory_id": "trial:active-a", "session_id": "active-a", "steps": [], "final_metrics": {"extra": {"total_turns": 1}}},
            ],
            "trajectory_meta": [
                {"trial_key": "trial:active-a", "status": "passed", "duration_ms": 1000, "steps": [], "warnings": []},
            ],
            "annotations": {"notes": []},
        }
        archived_single = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                {"trajectory_id": "trial:archived-d", "session_id": "archived-d", "steps": [], "final_metrics": {"extra": {"total_turns": 2}}},
            ],
            "trajectory_meta": [
                {"trial_key": "trial:archived-d", "status": "passed", "duration_ms": 2000, "steps": [], "warnings": []},
            ],
            "annotations": {"notes": [{"trial_key": "trial:archived-d", "source": "cell", "label": "notes.md", "markdown": "Archived note."}]},
        }
        active_after_activate = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                active_single["trajectory"][0],
                archived_single["trajectory"][0],
            ],
            "trajectory_meta": [
                active_single["trajectory_meta"][0],
                archived_single["trajectory_meta"][0],
            ],
            "annotations": {"notes": archived_single["annotations"]["notes"]},
        }
        archived_after_archive = {
            "schema_version": 19,
            "includes": ["core"],
            "trajectory": [
                active_single["trajectory"][0],
                archived_single["trajectory"][0],
            ],
            "trajectory_meta": [
                active_single["trajectory_meta"][0],
                archived_single["trajectory_meta"][0],
            ],
            "annotations": {"notes": archived_single["annotations"]["notes"]},
        }
        empty_active = {"schema_version": 19, "includes": ["core"], "trajectory": [], "trajectory_meta": [], "annotations": {"notes": []}}
        empty_archived = {"schema_version": 19, "includes": ["core"], "trajectory": [], "trajectory_meta": [], "annotations": {"notes": []}}
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = """
const vm = require("vm");
const asset = __ASSET__;
const scenarios = __SCENARIOS__;

function makeNodeFactory(nodes) {
  return function makeNode(id) {
    const node = {
      id,
      textContent: "",
      hidden: false,
      dataset: {},
      classList: { add() {}, remove() {}, toggle() {} },
      addEventListener() {},
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
  };
}

function response(payload) {
  return { ok: true, statusText: "OK", text: async () => JSON.stringify(payload) };
}

function createContext(scenario) {
  const nodes = {};
  const makeNode = makeNodeFactory(nodes);
  [
    "peval-py-i18n",
    "peval-py-token-estimates",
    "peval-py-render-options",
    "report-notes",
    "comparison",
    "trace",
    "step-drawer",
  ].forEach(id => nodes[id] = makeNode(id));
  nodes["peval-py-i18n"].textContent = "{}";
  nodes["peval-py-token-estimates"].textContent = "{}";
  nodes["peval-py-render-options"].textContent = JSON.stringify({ mode: "serve", sources: scenario.sources });
  const fetchCalls = [];
  const context = {
    nodes,
    scenario,
    fetchCalls,
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
    fetch: async (path, options = {}) => {
      const body = options.body ? JSON.parse(options.body) : null;
      fetchCalls.push({ path, body });
      if (String(path) === "/api/sources/state") return response(scenario.statePayload);
      if (String(path).includes(`source_state=${scenario.targetMode}`)) return response(scenario.targetReport);
      throw new Error(`unexpected fetch ${path}`);
    },
  };
  vm.createContext(context);
  vm.runInContext(asset, context);
  return context;
}

async function runScenario(scenario) {
  const context = createContext(scenario);
  const result = await vm.runInContext(`(async () => {
    applyServeMutationPayload({
      sources: scenario.sources,
      report: scenario.initialReport,
      report_source_key: scenario.initialSourceKey,
      report_source_state: scenario.initialMode,
    });
    const nullEditorResult = (() => {
      try {
        return renderNotesEditor(undefined);
      } catch (error) {
        return error.message;
      }
    })();
    state.rowSelection.add(scenario.selectedTrial);
    state.notesEditor = { trialKey: scenario.selectedTrial, markdown: "draft", error: "", saving: false };
    renderComparisonPanels({ trace: false });
    await mutateVisibleServeSourceState();
    return JSON.stringify({
      nullEditorResult,
      mode: state.serveSourceMode,
      reportRows: reportRows().length,
      selectedSourceKey: state.selectedSourceKey,
      selectedTrial: state.selectedTrial,
      rowSelectionSize: state.rowSelection.size,
      hasLeaderboard: nodes.leaderboard.innerHTML.includes("Leaderboard"),
      hasOverview: nodes["trajectory-overview"].innerHTML.includes("Trajectory Overview"),
      comparisonLength: nodes.comparison.innerHTML.length,
      traceLength: nodes.trace.innerHTML.length,
      targetFetches: fetchCalls.filter(call => String(call.path).includes("source_state=" + scenario.targetMode)).length,
      statePayload: fetchCalls.find(call => call.path === "/api/sources/state").body,
    });
  })()`, context);
  return JSON.parse(result);
}

Promise.all(scenarios.map(runScenario))
  .then(result => console.log(JSON.stringify(result)))
  .catch(error => { console.error(error && error.stack || error); process.exit(1); });
""".replace("__ASSET__", json.dumps(asset)).replace("__SCENARIOS__", json.dumps([
            {
                "name": "activate-last-archived",
                "sources": sources,
                "initialMode": "archived",
                "targetMode": "active",
                "initialSourceKey": "source-d",
                "selectedTrial": "trial:archived-d",
                "initialReport": archived_single,
                "targetReport": active_after_activate,
                "statePayload": {
                    "sources": [sources[0], {**sources[1], "active": True}],
                    "report": empty_archived,
                    "report_source_key": None,
                    "report_source_state": "archived",
                },
            },
            {
                "name": "archive-last-active",
                "sources": sources,
                "initialMode": "active",
                "targetMode": "archived",
                "initialSourceKey": "source-a",
                "selectedTrial": "trial:active-a",
                "initialReport": active_single,
                "targetReport": archived_after_archive,
                "statePayload": {
                    "sources": [{**sources[0], "active": False}, sources[1]],
                    "report": empty_active,
                    "report_source_key": None,
                    "report_source_state": "active",
                },
            },
        ]))
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        activate, archive = json.loads(node.stdout)

        self.assertEqual(activate["nullEditorResult"], "")
        self.assertEqual(activate["mode"], "active")
        self.assertEqual(activate["reportRows"], 2)
        self.assertEqual(activate["selectedSourceKey"], "source-d")
        self.assertEqual(activate["selectedTrial"], "trial:archived-d")
        self.assertEqual(activate["rowSelectionSize"], 0)
        self.assertTrue(activate["hasLeaderboard"])
        self.assertTrue(activate["hasOverview"])
        self.assertGreater(activate["comparisonLength"], 0)
        self.assertGreater(activate["traceLength"], 0)
        self.assertEqual(activate["targetFetches"], 1)
        self.assertEqual(activate["statePayload"]["source_keys"], ["source-d"])
        self.assertTrue(activate["statePayload"]["active"])
        self.assertEqual(activate["statePayload"]["report_source_state"], "archived")

        self.assertEqual(archive["nullEditorResult"], "")
        self.assertEqual(archive["mode"], "archived")
        self.assertEqual(archive["reportRows"], 2)
        self.assertEqual(archive["selectedSourceKey"], "source-a")
        self.assertEqual(archive["selectedTrial"], "trial:active-a")
        self.assertEqual(archive["rowSelectionSize"], 0)
        self.assertTrue(archive["hasLeaderboard"])
        self.assertTrue(archive["hasOverview"])
        self.assertGreater(archive["comparisonLength"], 0)
        self.assertGreater(archive["traceLength"], 0)
        self.assertEqual(archive["targetFetches"], 1)
        self.assertEqual(archive["statePayload"]["source_keys"], ["source-a"])
        self.assertFalse(archive["statePayload"]["active"])
        self.assertEqual(archive["statePayload"]["report_source_state"], "active")

    def test_comparison_panel_rerenders_preserve_scroll_positions(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = """
const vm = require("vm");
const asset = __ASSET__;
const context = {
  leaderboardWrap: { scrollTop: 96, scrollLeft: 42, addEventListener() {} },
  overviewList: { scrollTop: 128, scrollLeft: 7, addEventListener() {} },
  document: {
    body: { classList: { toggle() {} } },
    addEventListener() {},
    getElementById: () => null,
    querySelector(selector) {
      if (selector === "#leaderboard .table-wrap") return context.leaderboardWrap;
      if (selector === "#trajectory-overview .trajectory-overview-list") return context.overviewList;
      return null;
    },
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
};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`
  const calls = [];
  leaderboardRows = () => [{ trial_key: "trial:one" }];
  syncSelectionWithVisibleRows = rows => calls.push(["sync", rows.length]);
  renderLeaderboard = rows => {
    calls.push(["leaderboard", rows.length]);
    globalThis.leaderboardWrap = { scrollTop: 0, scrollLeft: 0, addEventListener() {} };
  };
  renderLeaderboardSummary = rows => calls.push(["summary", rows.length]);
  renderTrajectoryOverview = rows => {
    calls.push(["overview", rows.length]);
    globalThis.overviewList = { scrollTop: 0, scrollLeft: 0, addEventListener() {} };
  };
  renderTrace = () => calls.push(["trace"]);
  renderStepDrawer = () => calls.push(["drawer"]);
  renderComparisonPanels();
  JSON.stringify({
    leaderboardTop: leaderboardWrap.scrollTop,
    leaderboardLeft: leaderboardWrap.scrollLeft,
    overviewTop: overviewList.scrollTop,
    overviewLeft: overviewList.scrollLeft,
    calls
  });
`, context);
console.log(result);
""".replace("__ASSET__", json.dumps(asset))
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        result = json.loads(node.stdout)

        self.assertEqual(result["leaderboardTop"], 96)
        self.assertEqual(result["leaderboardLeft"], 42)
        self.assertEqual(result["overviewTop"], 128)
        self.assertEqual(result["overviewLeft"], 0)
        self.assertEqual(
            result["calls"],
            [["sync", 1], ["leaderboard", 1], ["overview", 1], ["trace"], ["drawer"]],
        )

    def test_comparison_panel_scroll_progress_syncs_in_both_directions(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = """
const vm = require("vm");
const asset = __ASSET__;
const writes = { leaderboard: [], overview: [] };
function makeNode(name, scrollTop, scrollLeft, scrollHeight, clientHeight) {
  const node = {
    handlers: [],
    scrollHeight,
    clientHeight,
    addEventListener(type, handler) {
      if (type === "scroll") this.handlers.push(handler);
    },
    triggerScroll() {
      this.handlers.forEach(handler => handler({ target: this }));
    }
  };
  let top = scrollTop;
  let left = scrollLeft;
  Object.defineProperty(node, "scrollTop", {
    get() { return top; },
    set(value) {
      top = value;
      writes[name].push({ field: "top", value });
      if (name === "overview" && context.triggerOverviewNested) {
        context.triggerOverviewNested = false;
        node.triggerScroll();
      }
      if (name === "leaderboard" && context.triggerLeaderboardNested) {
        context.triggerLeaderboardNested = false;
        node.triggerScroll();
      }
    }
  });
  Object.defineProperty(node, "scrollLeft", {
    get() { return left; },
    set(value) {
      left = value;
      writes[name].push({ field: "left", value });
    }
  });
  return node;
}
const context = {
  leaderboardWrap: makeNode("leaderboard", 250, 77, 1200, 200),
  overviewList: makeNode("overview", 0, 11, 2200, 200),
  triggerOverviewNested: false,
  triggerLeaderboardNested: false,
  rafCalls: 0,
  writes,
  document: {
    body: { classList: { toggle() {} } },
    addEventListener() {},
    getElementById: () => null,
    querySelector(selector) {
      if (selector === "#leaderboard .table-wrap") return context.leaderboardWrap;
      if (selector === "#trajectory-overview .trajectory-overview-list") return context.overviewList;
      return null;
    },
  },
  requestAnimationFrame(callback) {
    context.rafCalls += 1;
    callback();
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
};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`
  bindComparisonScrollSync();
  const listenerCounts = {
    leaderboard: leaderboardWrap.handlers.length,
    overview: overviewList.handlers.length
  };

  globalThis.triggerOverviewNested = true;
  leaderboardWrap.triggerScroll();
  const afterLeaderboardScroll = {
    leaderboardTop: leaderboardWrap.scrollTop,
    leaderboardLeft: leaderboardWrap.scrollLeft,
    overviewTop: overviewList.scrollTop,
    overviewLeft: overviewList.scrollLeft,
    leaderboardWrites: writes.leaderboard.slice(),
    overviewWrites: writes.overview.slice(),
    syncingReleased: state.comparisonScrollSyncing === false
  };

  writes.leaderboard.length = 0;
  writes.overview.length = 0;
  overviewList.scrollTop = 1500;
  writes.overview.length = 0;
  globalThis.triggerLeaderboardNested = true;
  overviewList.triggerScroll();
  const afterOverviewScroll = {
    leaderboardTop: leaderboardWrap.scrollTop,
    leaderboardLeft: leaderboardWrap.scrollLeft,
    overviewTop: overviewList.scrollTop,
    overviewLeft: overviewList.scrollLeft,
    leaderboardWrites: writes.leaderboard.slice(),
    overviewWrites: writes.overview.slice(),
    syncingReleased: state.comparisonScrollSyncing === false
  };

  JSON.stringify({ listenerCounts, afterLeaderboardScroll, afterOverviewScroll, rafCalls });
`, context);
console.log(result);
""".replace("__ASSET__", json.dumps(asset))
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        result = json.loads(node.stdout)

        self.assertEqual(result["listenerCounts"], {"leaderboard": 1, "overview": 1})
        self.assertEqual(result["afterLeaderboardScroll"]["leaderboardTop"], 250)
        self.assertEqual(result["afterLeaderboardScroll"]["leaderboardLeft"], 77)
        self.assertEqual(result["afterLeaderboardScroll"]["overviewTop"], 500)
        self.assertEqual(result["afterLeaderboardScroll"]["overviewLeft"], 11)
        self.assertEqual(result["afterLeaderboardScroll"]["leaderboardWrites"], [])
        self.assertEqual(
            result["afterLeaderboardScroll"]["overviewWrites"],
            [{"field": "top", "value": 500}],
        )
        self.assertTrue(result["afterLeaderboardScroll"]["syncingReleased"])
        self.assertEqual(result["afterOverviewScroll"]["leaderboardTop"], 750)
        self.assertEqual(result["afterOverviewScroll"]["leaderboardLeft"], 77)
        self.assertEqual(result["afterOverviewScroll"]["overviewTop"], 1500)
        self.assertEqual(result["afterOverviewScroll"]["overviewLeft"], 11)
        self.assertEqual(
            result["afterOverviewScroll"]["leaderboardWrites"],
            [{"field": "top", "value": 750}],
        )
        self.assertEqual(result["afterOverviewScroll"]["overviewWrites"], [])
        self.assertTrue(result["afterOverviewScroll"]["syncingReleased"])
        self.assertGreaterEqual(result["rafCalls"], 4)


    def test_html_submenu_outside_click_closer_only_targets_menus(self) -> None:
        if not shutil.which("node"):
            self.skipTest("node is required to execute report.js interaction helpers")
        asset = load_asset_text("report.js")
        self.assertIn("\nrender(data());", asset)
        asset = asset.rsplit("\nrender(data());", 1)[0]
        script = """
const vm = require("vm");
const asset = __ASSET__;
const exportMenu = { id: "export", open: true };
const filterMenu = { id: "filter", open: true };
const timelineSection = { id: "timeline", open: true };
const handlers = [];
const documentStub = {
  body: { classList: { toggle() {} } },
  addEventListener(type, handler, options) {
    handlers.push({ type, handler, capture: options === true || options?.capture === true });
  },
  getElementById: () => null,
  querySelector: () => null,
  querySelectorAll(selector) {
    if (selector !== ".export-menu[open],.filter-control[open]") {
      throw new Error(`unexpected selector: ${selector}`);
    }
    return [exportMenu, filterMenu].filter(details => details.open);
  },
};
const context = {
  document: documentStub,
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
  exportMenu,
  filterMenu,
  timelineSection,
  handlers,
};
vm.createContext(context);
vm.runInContext(asset, context);
const result = vm.runInContext(`
  bindGlobalControls();
  const clickHandler = handlers.find(item => item.type === "click" && item.capture).handler;
  filterMenu.open = true;
  exportMenu.open = true;
  clickHandler({ target: { closest: selector => selector === SUBMENU_DETAILS_SELECTOR ? exportMenu : null } });
  const insideExport = { exportOpen: exportMenu.open, filterOpen: filterMenu.open, timelineOpen: timelineSection.open };
  filterMenu.open = true;
  exportMenu.open = true;
  clickHandler({ target: { closest: () => null } });
  const outside = { exportOpen: exportMenu.open, filterOpen: filterMenu.open, timelineOpen: timelineSection.open };
  JSON.stringify({ insideExport, outside, clickHandlerCapture: Boolean(clickHandler) });
`, context);
console.log(result);
""".replace("__ASSET__", json.dumps(asset))
        node = subprocess.run(
            ["node"],
            input=script,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(node.returncode, 0, node.stderr)
        result = json.loads(node.stdout)

        self.assertEqual(
            result["insideExport"],
            {"exportOpen": True, "filterOpen": False, "timelineOpen": True},
        )
        self.assertEqual(
            result["outside"],
            {"exportOpen": False, "filterOpen": False, "timelineOpen": True},
        )
        self.assertTrue(result["clickHandlerCapture"])
