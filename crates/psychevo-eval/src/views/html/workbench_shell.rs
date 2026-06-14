#[allow(unused_imports)]
use super::*;

pub(crate) fn render_view(report: &ViewReport, format: ViewFormat) -> Result<String> {
    match format {
        ViewFormat::Json => Ok(serde_json::to_string_pretty(report)?),
        ViewFormat::Html => Ok(render_view_html(report)),
    }
}

pub(crate) fn render_view_html(report: &ViewReport) -> String {
    render_workbench_html("peval view", "peval static report", Some(report), None)
}

pub(crate) fn render_workbench_html(
    title: &str,
    eyebrow: &str,
    report: Option<&ViewReport>,
    token: Option<&str>,
) -> String {
    let initial_data = report
        .map(|report| {
            format!(
                "<script type=\"application/json\" id=\"peval-view-data\">{}</script>",
                safe_json_for_script(
                    &serde_json::to_string(report).unwrap_or_else(|_| "{}".into())
                )
            )
        })
        .unwrap_or_default();
    let token_json = serde_json::to_string(token.unwrap_or("")).unwrap_or_else(|_| "\"\"".into());
    let is_serve = token.is_some();
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
{css}
</head>
<body>
<div class="workspace">
  <main class="main">
    <section class="topline">
      <div>
        <p class="eyebrow">{eyebrow}</p>
        <h1 id="report-title">Evaluation Workbench</h1>
        <p class="copy" id="report-copy">Matrix-first evaluation view with Harbor-style transparent trajectory rows.</p>
        <div class="report-notes" id="report-notes"></div>
      </div>
      <div class="top-actions">
        <div class="score-strip" id="score-strip" aria-label="run summary"></div>
        <div class="serve-actions" id="serve-actions" hidden>
        <button type="button" id="refresh">Refresh</button>
        <button type="button" id="analyze" disabled>Analyze Trial</button>
        <button type="button" id="batch" disabled>Analyze Failed</button>
        </div>
      </div>
    </section>
    <section class="leaderboard panel" id="leaderboard" aria-labelledby="leaderboard-title"></section>
    <section class="panel" aria-labelledby="matrix-title">
      <div class="panel-head">
        <div>
          <p class="eyebrow">matrix scope</p>
          <h2 id="matrix-title">Visible Trial Heatmap</h2>
          <p class="copy">Hue follows outcome. Shade follows the selected metric's relative value across currently visible numeric cells.</p>
        </div>
        <div class="metric-controls" aria-label="metric mode">
          <div class="segmented" id="metric-buttons"></div>
        </div>
      </div>
      <div class="matrix-scroll"><div class="matrix" id="matrix"></div></div>
    </section>
    <section class="trace-panel" id="trace" aria-labelledby="trace-title"></section>
  </main>
</div>
{initial_data}
<script>window.PEVAL_SERVE_TOKEN = {token_json}; window.PEVAL_IS_SERVE = {is_serve};</script>
<script>
{script}
</script>
</body>
</html>"#,
        title = escape_html(title),
        eyebrow = escape_html(eyebrow),
        css = report_css(),
        initial_data = initial_data,
        token_json = token_json,
        is_serve = if is_serve { "true" } else { "false" },
        script = workbench_js(),
    )
}

pub(crate) fn safe_json_for_script(value: &str) -> String {
    value
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
}

pub(crate) fn workbench_js() -> String {
    [
        workbench_js_boot(),
        workbench_js_tables(),
        workbench_js_rendering(),
    ]
    .concat()
}

fn workbench_js_boot() -> &'static str {
    r###"
const METRICS = [
  { key: "score", label: "score" },
  { key: "duration", label: "duration" },
  { key: "tokens", label: "tokens" },
  { key: "tools", label: "tool calls" },
  { key: "turns", label: "turns" }
];
const state = { view: null, metricMode: "score", selectedTrial: null, selectedStepId: null, analysisEnabled: false, tables: {}, openDetails: {}, openFilters: {} };
const $ = id => document.getElementById(id);
const esc = value => String(value ?? "").replace(/[&<>"]/g, ch => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", "\"": "&quot;" }[ch]));
const lower = value => String(value ?? "").toLowerCase();
const pct = value => Number.isFinite(value) ? `${Math.round(value * 100)}%` : "-";
const fmtNum = value => value === null || value === undefined || value === "" ? "-" : Number(value).toLocaleString();
const fmtScore = value => value === null || value === undefined || Number.isNaN(Number(value)) ? "-" : Number(value).toFixed(2);
const fmtCost = value => value === null || value === undefined || Number.isNaN(Number(value)) ? "-" : `$${Number(value).toFixed(4)}`;
const fmtMs = value => fmtDurationMs(value);
const fmtRailMs = value => fmtDurationMs(value);
function fmtRailTokens(value) {
  if (value === null || value === undefined || value === "" || Number.isNaN(Number(value))) return "-";
  const number = Number(value);
  return Math.abs(number) >= 1000 ? `${(number / 1000).toFixed(1)}k` : fmtNum(number);
}
function fmtDurationMs(value) {
  if (value === null || value === undefined || Number.isNaN(Number(value))) return "-";
  const seconds = Math.max(0, Number(value) / 1000);
  if (seconds >= 60) {
    const minutes = Math.floor(seconds / 60);
    return `${minutes}m${(seconds - minutes * 60).toFixed(1)}s`;
  }
  return `${seconds.toFixed(1)}s`;
}
const fmtDate = value => value === null || value === undefined ? "-" : new Date(Number(value)).toLocaleString();

function initialView() {
  const el = $("peval-view-data");
  if (!el || !el.textContent.trim()) return null;
  return JSON.parse(el.textContent);
}
function rpc(method, params = {}) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(`${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/ws?token=${encodeURIComponent(window.PEVAL_SERVE_TOKEN || "")}`);
    ws.onopen = () => ws.send(JSON.stringify({ id: 1, method, params }));
    ws.onmessage = event => {
      const msg = JSON.parse(event.data);
      ws.close();
      msg.error ? reject(msg.error) : resolve(msg.result);
    };
    ws.onerror = () => reject(new Error("websocket failed"));
  });
}
async function loadView() {
  try {
    const local = initialView();
    render(local || await rpc("view.get"));
  } catch (err) {
    $("report-copy").textContent = err.message || String(err);
  }
}
function statusClass(status, failureClass = "") {
  const value = lower(status);
  const failure = lower(failureClass);
  if (value === "passed") return "passed";
  if (value === "timeout" || failure.includes("timeout")) return "timeout";
  return "failed";
}
function metricValue(cell) {
  if (!cell) return null;
  if (state.metricMode === "score") return cell.score;
  if (state.metricMode === "duration") return Number(cell.duration_ms);
  if (state.metricMode === "tokens") return trialTotalTokens(cell.representative_trial_key);
  if (state.metricMode === "tools") return Number(cell.tool_calls);
  if (state.metricMode === "turns") return cell.turns;
  return null;
}
function formatMetric(value) {
  if (state.metricMode === "score") return fmtScore(value);
  if (state.metricMode === "duration") return fmtMs(value);
  if (state.metricMode === "tokens") return value == null ? "-" : `${Math.round(Number(value) / 1000)}k`;
  return value == null ? "-" : fmtNum(value);
}
function isNumericValue(value) {
  return typeof value === "number" && Number.isFinite(value);
}
function visibleCells() {
  if (!state.view) return [];
  const visibleRows = filteredLeaderboardComparisonRows();
  const visibleKeys = new Set(visibleRows.map(row => `${row.task_id}\u0000${row.agent_id}\u0000${row.model_name || "-"}\u0000${row.variant_id || "-"}`));
  return (state.view.comparison?.matrix?.cells || []).filter(cell => {
    const model = cell.model_name || "-";
    return visibleKeys.has(`${cell.task_id}\u0000${cell.agent_id}\u0000${model}\u0000${cell.variant_id || "-"}`);
  });
}
function shadeFor(cell, visible) {
  const values = visible.map(metricValue).filter(isNumericValue);
  const value = metricValue(cell);
  if (!values.length || !isNumericValue(value)) return "shade-3 missing-metric";
  const min = Math.min(...values);
  const max = Math.max(...values);
  if (min === max) return "shade-3";
  return `shade-${Math.min(5, Math.max(1, Math.floor(((value - min) / (max - min)) * 5) + 1))}`;
}
function trialFor(trialKey) {
  return trajectoryMetaFor(trialKey);
}
function trialKeyInCell(cell, trialKey) {
  return !!trialKey && (cell?.trial_keys || []).includes(trialKey);
}
function matrixCellForTrial(trialKey) {
  return (state.view?.comparison?.matrix?.cells || []).find(cell => trialKeyInCell(cell, trialKey)) || null;
}
function selectedTrialInVisibleCells(visible) {
  return visible.some(cell => trialKeyInCell(cell, state.selectedTrial));
}
function hasMultiTrialMatrixCell() {
  return (state.view?.comparison?.matrix?.cells || []).some(cell => (cell.trial_keys || []).length > 1);
}
function trialOrdinal(trialKey) {
  const cell = matrixCellForTrial(trialKey);
  const index = (cell?.trial_keys || []).indexOf(trialKey);
  return index >= 0 ? index + 1 : null;
}
function shortTrialKey(trialKey) {
  return String(trialKey || "").replace(/:t\d+$/, "").slice(0, 8) || "-";
}
function trajectoryFor(trialKey) {
  return (state.view?.trajectory || []).find(trajectory => trajectory.trajectory_id === trialKey);
}
function trajectoryMetaFor(trialKey) {
  return (state.view?.trajectory_meta || []).find(meta => meta.trial_key === trialKey);
}
function finalMetricsFor(trialKey) {
  return trajectoryFor(trialKey)?.final_metrics || {};
}
function trialTotalTokens(trialKey) {
  const metrics = finalMetricsFor(trialKey);
  return metrics.usage?.total_tokens ?? sumOptional([metrics.total_prompt_tokens, metrics.total_completion_tokens, metrics.total_cached_tokens]);
}
function trialCost(trialKey) {
  return finalMetricsFor(trialKey).total_cost_usd;
}
function trialTurns(trialKey) {
  return finalMetricsFor(trialKey).total_turns;
}
function trialToolCalls(trialKey) {
  return finalMetricsFor(trialKey).total_tool_calls ?? 0;
}
function trialToolErrors(trialKey) {
  return finalMetricsFor(trialKey).total_tool_errors ?? 0;
}
function systemExposed(trajectory) {
  return (trajectory?.steps || []).some(step => step.source === "system");
}
function reasoningExposed(trajectory) {
  return (trajectory?.steps || []).some(step => step.reasoning_content);
}
function notesFor(trialKey) {
  return (state.view?.annotations?.notes || []).filter(note => note.trial_key === trialKey);
}
function analysisFor(trialKey) {
  return (state.view?.annotations?.analysis || []).find(report => report.trial_key === trialKey);
}
function attachmentFor(trialKey) {
  return (state.view?.attachments?.artifacts || []).find(report => report.trial_key === trialKey);
}
function render(view) {
  state.view = view;
  const comparison = view.comparison || {};
  if (comparison.default_metric && !state.metricInitialized) {
    state.metricMode = comparison.default_metric;
    state.metricInitialized = true;
  }
  if (!state.selectedTrial) {
    const firstFailed = (comparison.matrix?.cells || []).find(cell => lower(cell.status) !== "passed");
    state.selectedTrial = (firstFailed || (comparison.matrix?.cells || [])[0])?.representative_trial_key || view.trajectory_meta?.[0]?.trial_key || null;
  }
  $("report-title").textContent = view.scope?.benchmark || "Evaluation Workbench";
  const summary = comparison.summary || { total_trials: view.trajectory_meta?.length || 0, passed_trials: (view.trajectory_meta || []).filter(trial => lower(trial.status) === "passed").length, failed_trials: (view.trajectory_meta || []).filter(trial => lower(trial.status) !== "passed").length };
  $("report-copy").textContent = `${summary.total_trials} trials - ${summary.passed_trials} passed - ${summary.failed_trials} failed - includes ${includeList(view)}`;
  renderReportNotes(view.annotations?.report_notes || []);
  renderScoreStrip(view);
  renderMetricControls();
  renderLeaderboard();
  renderMatrix();
  renderTrace();
}
function renderReportNotes(notes) {
  $("report-notes").innerHTML = notes.length ? `<div class="report-note-list">${notes.map(note => `<article class="report-note"><strong>${esc(note.label || "Report note")}</strong><div class="note-body">${renderMarkdown(note.markdown || "")}</div></article>`).join("")}</div>` : "";
}
function renderScoreStrip(view) {
  const summary = view.comparison?.summary || { total_trials: view.trajectory_meta?.length || 0, passed_trials: (view.trajectory_meta || []).filter(trial => lower(trial.status) === "passed").length, metrics: null };
  $("score-strip").innerHTML = [
    [summary.total_trials, "trials"],
    [pct(summary.total_trials ? summary.passed_trials / summary.total_trials : 0), "pass rate"],
    [fmtCost(summary.metrics?.cost?.amount_usd ?? sumOptional((view.trajectory || []).map(trajectory => trajectory.final_metrics?.total_cost_usd))), "cost"]
  ].map(([value, label]) => `<div class="score-box"><strong>${esc(value)}</strong><span>${esc(label)}</span></div>`).join("");
}
function renderMetricControls() {
  $("metric-buttons").innerHTML = METRICS.map(metric => `<button class="metric-button ${metric.key === state.metricMode ? "active" : ""}" type="button" data-metric="${metric.key}">${esc(metric.label)}</button>`).join("");
  document.querySelectorAll("[data-metric]").forEach(button => {
    button.addEventListener("click", () => {
      state.metricMode = button.dataset.metric;
      renderMatrix();
      renderMetricControls();
    });
  });
}
function renderMatrix() {
  const matrix = state.view.comparison?.matrix;
  if (!matrix) {
    $("matrix").style.gridTemplateColumns = "1fr";
    $("matrix").innerHTML = `<p class="copy">No comparison include.</p>`;
    return;
  }
  const tasks = matrix.task_axis || [];
  const agents = matrix.agent_axis || [];
  const visible = visibleCells();
  if (!selectedTrialInVisibleCells(visible)) {
    state.selectedTrial = visible[0]?.representative_trial_key || state.view.trajectory_meta?.[0]?.trial_key || null;
    state.selectedStepId = null;
  }
  const bySlot = new Map(visible.map(cell => [`${cell.task_id}\u0000${cell.agent_axis_id || cell.agent_id}`, cell]));
  $("matrix").style.gridTemplateColumns = `162px repeat(${Math.max(agents.length, 1)}, minmax(140px, 1fr))`;
  const cells = [`<div class="axis-head">task / agent</div>`];
  agents.forEach(agent => cells.push(`<div class="agent-head">${esc(agent.label || agent.id)}</div>`));
  tasks.forEach(task => {
    cells.push(`<div class="task-axis">${esc(task.label || task.id)}</div>`);
    agents.forEach(agent => {
      const cell = bySlot.get(`${task.id}\u0000${agent.id}`);
      if (!cell) {
        cells.push(`<button class="cell empty" type="button" disabled><strong>-</strong><span>no visible trial</span></button>`);
        return;
      }
      const shade = shadeFor(cell, visible);
      const value = metricValue(cell);
      cells.push(`<button class="cell ${statusClass(cell.status, cell.failure_class)} ${shade} ${trialKeyInCell(cell, state.selectedTrial) ? "selected" : ""}" type="button" data-trial="${esc(cell.representative_trial_key)}"><strong>${esc(formatMetric(value))}</strong><span>${esc(cell.status)} / ${esc(cell.representative_trial_key.replace(/:t001$/, ""))}<br>${fmtMs(cell.duration_ms)}</span></button>`);
    });
  });
  $("matrix").innerHTML = cells.join("");
  document.querySelectorAll("[data-trial]").forEach(button => {
    button.addEventListener("click", () => {
      const cell = visible.find(item => item.representative_trial_key === button.dataset.trial);
      if (!trialKeyInCell(cell, state.selectedTrial)) {
        state.selectedTrial = button.dataset.trial;
        state.selectedStepId = null;
      }
      renderMatrix();
      renderTrace();
    });
  });
}
function infoGrid(items) {
  return `<div class="info-grid">${items.map(([label, value]) => `<div><span>${esc(label)}</span><strong>${esc(value)}</strong></div>`).join("")}</div>`;
}
function trialWallDurationMs(trial) {
  if (hasMetricValue(trial?.started_at_ms) && hasMetricValue(trial?.finished_at_ms)) {
    return Math.max(0, Number(trial.finished_at_ms) - Number(trial.started_at_ms));
  }
  return trial?.duration_ms;
}
function renderTrace() {
  const trial = trialFor(state.selectedTrial) || trialFor(visibleCells()[0]?.representative_trial_key) || (state.view.trajectory_meta || [])[0];
  if (!trial) {
    $("trace").innerHTML = `<div class="trace-head"><div><p class="eyebrow">selected trial trajectory</p><h2 id="trace-title">No core include</h2><p class="copy">Add \`-i core\` or use the default include set to inspect a selected Trial.</p></div></div>`;
    return;
  }
  state.selectedTrial = trial.trial_key;
  const trajectory = trajectoryFor(trial.trial_key);
  const trajectoryMeta = trajectoryMetaFor(trial.trial_key);
  const model = trajectory?.agent?.model_name || "-";
  const agentName = trajectory?.agent?.name || "-";
  const siblingSwitcher = renderTrialSiblingSwitcher(trial.trial_key);
  $("trace").innerHTML = `
    <div class="trace-head">
      <div>
        <p class="eyebrow">selected trial trajectory</p>
        <h2 id="trace-title" class="trace-title"><span>${esc(trial.task_id)}</span><code>${esc(trial.trial_key)}</code></h2>
      </div>
      ${siblingSwitcher}
    </div>
    <h3>Run</h3>
    ${infoGrid([
      ["trial", trial.trial_key],
      ["variant", trial.variant_label || "-"],
      ["session", trajectory?.session_id || "-"],
      ["agent / model", `${agentName} / ${model}`],
      ["time", `${fmtDate(trial.started_at_ms)} -> ${fmtDate(trial.finished_at_ms)}`],
      ["wall duration", fmtMs(trialWallDurationMs(trial))],
      ["steps/events", `${(trajectory?.steps || []).length}/${trajectoryMeta?.total_events ?? "-"}`],
      ["system exposed", systemExposed(trajectory) ? "yes" : "no"],
      ["reasoning exposed", reasoningExposed(trajectory) ? "yes" : "no"]
    ])}
    <h3>Result</h3>
    ${infoGrid([
      ["status", trial.status],
      ["score", fmtScore(trial.score)],
      ["evaluator", trial.score_message || "-"],
      ["tokens", fmtNum(trialTotalTokens(trial.trial_key))],
      ["turns", trialTurns(trial.trial_key) ?? "-"],
      ["tool success / total", toolCallRatio(trialToolCalls(trial.trial_key), trialToolErrors(trial.trial_key))],
      ["cost", fmtCost(trialCost(trial.trial_key))]
    ])}
    ${renderSelectedNotes(trial.trial_key)}
    ${renderSelectedAnalysis(trial.trial_key)}
    ${renderSelectedEvidence(trial)}
    ${renderStepsHeader(trajectory, trajectoryMeta)}
    <div class="step-list" id="step-list">${trajectory ? (trajectory.steps || []).map(step => renderStep(step, trajectoryMeta)).join("") : `<p class="copy">No trajectory include for this Trial.</p>`}</div>
  `;
  bindTrialSwitcher();
  bindStepControls();
  document.querySelectorAll("#step-list .step").forEach(row => {
    row.addEventListener("click", () => {
      state.selectedStepId = Number(row.dataset.step);
      document.querySelectorAll("#step-list .step").forEach(item => item.classList.toggle("selected-step", Number(item.dataset.step) === state.selectedStepId));
    });
  });
}
function renderTrialSiblingSwitcher(trialKey) {
  const cell = matrixCellForTrial(trialKey);
  const trialKeys = cell?.trial_keys || [];
  if (trialKeys.length <= 1) return "";
  return `<div class="trial-switcher" aria-label="Trials in selected matrix cell">${trialKeys.map((key, index) => {
    const trial = trialFor(key);
    const active = key === trialKey;
    const latest = key === cell.representative_trial_key;
    const title = [
      key,
      trial?.matrix_cell_key ? `cell ${trial.matrix_cell_key}` : "",
      trial?.started_at_ms ? `started ${fmtDate(trial.started_at_ms)}` : "",
      trial?.duration_ms !== undefined ? `duration ${fmtMs(trial.duration_ms)}` : "",
      trial?.status ? `status ${trial.status}` : ""
    ].filter(Boolean).join(" / ");
    return `<button class="trial-switch ${active ? "active" : ""}" type="button" data-switch-trial="${esc(key)}" title="${esc(title)}"><span>#${index + 1}</span>${latest ? `<em>latest</em>` : ""}</button>`;
  }).join("")}</div>`;
}
function bindTrialSwitcher() {
  document.querySelectorAll("[data-switch-trial]").forEach(button => {
    button.addEventListener("click", event => {
      event.stopPropagation();
      state.selectedTrial = button.dataset.switchTrial;
      state.selectedStepId = null;
"###
}
