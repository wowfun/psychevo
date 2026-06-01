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

pub(crate) fn workbench_js() -> &'static str {
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
      renderLeaderboard();
      renderMatrix();
      renderTrace();
    });
  });
}
function renderStepsHeader(trajectory, trajectoryMeta) {
  const count = (trajectory?.steps || []).length ?? 0;
  const disabled = trajectory && count > 0 ? "" : "disabled";
  return `<div class="steps-head"><h3>Steps (${count})</h3><div class="step-actions" aria-label="step visibility toggle"><button class="step-toggle-button" type="button" data-step-action="toggle" aria-pressed="false" ${disabled}>Expand all</button></div></div>`;
}
function stepRows() {
  return Array.from(document.querySelectorAll("#step-list .step"));
}
function refreshStepToggleButton() {
  const button = document.querySelector("[data-step-action=\"toggle\"]");
  if (!button) return;
  const rows = stepRows();
  const hasRows = rows.length > 0;
  const allOpen = hasRows && rows.every(row => row.open);
  button.disabled = !hasRows;
  button.dataset.stepState = allOpen ? "expanded" : "collapsed";
  button.textContent = allOpen ? "Collapse all" : "Expand all";
  button.setAttribute("aria-pressed", allOpen ? "true" : "false");
}
function bindStepControls() {
  const button = document.querySelector("[data-step-action=\"toggle\"]");
  stepRows().forEach(row => row.addEventListener("toggle", refreshStepToggleButton));
  if (!button) return;
  button.addEventListener("click", event => {
    event.stopPropagation();
    const rows = stepRows();
    const shouldOpen = !rows.every(row => row.open);
    rows.forEach(row => {
      row.open = shouldOpen;
      if (!shouldOpen) row.classList.remove("selected-step");
    });
    if (!shouldOpen) state.selectedStepId = null;
    refreshStepToggleButton();
  });
  refreshStepToggleButton();
}
function renderSelectedNotes(trialKey) {
  const notes = notesFor(trialKey);
  const body = notes.length ? `<div class="note-list">${notes.map(renderManualNote).join("")}</div>` : `<p class="copy">No notes.</p>`;
  return `<section class="selected-extra"><h3>Notes</h3>${body}</section>`;
}
function renderSelectedAnalysis(trialKey) {
  const analysis = analysisFor(trialKey);
  if (!analysis || analysis.status === "missing") {
    return `<section class="selected-extra"><h3>Analysis</h3><p class="copy">No cached analysis.</p></section>`;
  }
  return `<section class="selected-extra"><h3>Analysis</h3><article class="analysis-card"><div class="note-meta"><span class="chip">${esc(analysis.status || "cached")}</span>${analysis.json_ref?.relative_path ? `<strong>${esc(analysis.json_ref.relative_path)}</strong>` : ""}</div><pre>${esc(analysis.summary || analysis.error || "-")}</pre></article></section>`;
}
function renderSelectedEvidence(trial) {
  const blocks = [
    renderSelectedCellRoot(trial),
    renderSelectedScoreDetails(trial),
    renderSelectedUsage(trial.trial_key),
    renderSelectedWarnings(trial),
    renderSelectedArtifacts(trial)
  ].filter(Boolean);
  return blocks.length ? `<section class="selected-extra selected-evidence"><h3>Evidence</h3><div class="selected-evidence-list">${blocks.join("")}</div></section>` : "";
}
function renderSelectedCellRoot(trial) {
  if (!trial.cell_root_relative) return "";
  return `<article class="selected-evidence-card"><h4>Cell Root</h4><code>${esc(trial.cell_root_relative)}</code></article>`;
}
function isEmptyObject(value) {
  return value === null || value === undefined || (typeof value === "object" && !Array.isArray(value) && Object.keys(value).length === 0);
}
function renderSelectedScoreDetails(trial) {
  if (isEmptyObject(trial.score_details)) return "";
  return `<details class="selected-evidence-card"><summary>Score Details</summary><pre>${esc(shortJson(trial.score_details))}</pre></details>`;
}
function renderSelectedUsage(trialKey) {
  const metrics = finalMetricsFor(trialKey);
  const usage = metrics.usage || {};
  const accounting = metrics.accounting || {};
  if (!metrics.usage && !metrics.accounting && !hasMetricValue(metrics.total_prompt_tokens) && !hasMetricValue(metrics.total_completion_tokens) && !hasMetricValue(metrics.total_cached_tokens)) return "";
  return `<article class="selected-evidence-card"><h4>Usage Breakdown</h4>${infoGrid([
    ["input", fmtNum(usage.input_tokens ?? metrics.total_prompt_tokens)],
    ["output", fmtNum(usage.output_tokens ?? metrics.total_completion_tokens)],
    ["cache read", fmtNum(usage.cache_read_tokens ?? metrics.total_cached_tokens)],
    ["cache write", fmtNum(usage.cache_write_tokens)],
    ["reasoning", fmtNum(usage.reasoning_tokens)],
    ["billable input", fmtNum(accounting.billable_input_tokens)],
    ["billable output", fmtNum(accounting.billable_output_tokens)],
    ["pricing", accounting.pricing_source || "-"]
  ])}</article>`;
}
function renderSelectedWarnings(trial) {
  const warnings = trial.warnings || [];
  if (!warnings.length) return "";
  return `<article class="selected-evidence-card"><h4>Warnings</h4><ul class="evidence-list">${warnings.map(warning => `<li>${esc(warning)}</li>`).join("")}</ul></article>`;
}
function renderSelectedArtifacts(trial) {
  const refs = attachmentFor(trial.trial_key)?.refs || [];
  if (!refs.length) return "";
  return `<details class="selected-evidence-card"><summary>Artifacts</summary><ul class="artifact-list">${refs.map(ref => `<li title="${esc([ref.kind, ref.mime, ref.label].filter(Boolean).join(" / "))}"><span class="chip">${esc(ref.kind || "artifact")}</span><code>${esc(ref.relative_path || ref.label || "-")}</code></li>`).join("")}</ul></details>`;
}
function renderStep(step) {
  const meta = arguments[1] ? stepMeta(arguments[1], step.step_id) : null;
  const preview = valuePreview(step.message).trim() || "(No Message)";
  const open = step.step_id === state.selectedStepId ? "open" : "";
  const selected = step.step_id === state.selectedStepId ? "selected-step" : "";
  return `
    <details class="step ${selected}" data-step="${step.step_id}" ${open}>
      <summary>
        <div class="step-row">
          <span class="step-id">#${esc(step.step_id)}</span>
          <span class="role ${esc(step.source)}">${esc(step.source)}</span>
          <span class="preview">${esc(preview)}</span>
        </div>
        <div class="rail">
          ${renderStepRail(step, meta)}
        </div>
      </summary>
      <div class="step-body">${renderStepBlocks(step, meta)}</div>
    </details>
  `;
}
function renderStepBlocks(step, meta) {
  let html = "";
  if (step.reasoning_content) {
    html += `<div class="block reasoning-block"><h4>Reasoning</h4><pre>${esc(step.reasoning_content)}</pre></div>`;
  }
  const message = valuePreview(step.message);
  if (message) {
    const label = step.source === "system" ? "System Prompt" : "Message";
    html += `<div class="block message-block"><h4>${esc(label)}</h4><pre>${esc(message)}</pre></div>`;
  }
  (step.tool_calls || []).forEach(tool => {
    const toolMeta = toolMetaFor(meta, tool.tool_call_id);
    html += `<div class="block tool-block"><h4>Tool Calls</h4><p>${renderToolNameChip(tool, toolMeta)} <span class="muted">ID: ${esc(tool.tool_call_id)}${toolMeta?.status ? ` / ${esc(toolMeta.status)}` : ""}${renderToolTiming(toolMeta)}</span></p><pre>${esc(valuePreview(tool.arguments))}</pre></div>`;
  });
  ((step.observation && step.observation.results) || []).forEach(observation => {
    const observationMeta = observationMetaFor(meta, observation.source_call_id);
    html += `<div class="block observation-block"><h4 class="${observationMeta?.tool_error ? "danger" : ""}">Observations</h4><p class="muted">Result for: ${esc(observation.source_call_id || "-")}${observationMeta?.status ? ` / ${esc(observationMeta.status)}` : ""}</p><pre>${esc(valuePreview(observation.content))}</pre></div>`;
  });
  return html;
}
function hasMetricValue(value) {
  return value !== null && value !== undefined && value !== "" && !Number.isNaN(Number(value));
}
function toolExecutionText(toolMeta) {
  return hasMetricValue(toolMeta?.execution_duration_ms) ? fmtRailMs(toolMeta.execution_duration_ms) : "";
}
function renderToolNameChip(tool, toolMeta) {
  const exec = toolExecutionText(toolMeta);
  const title = exec ? ` title="${esc(`tool exec ${exec}`)}"` : "";
  const execHtml = exec ? ` <span class="tool-exec-inline">${esc(exec)}</span>` : "";
  return `<span class="chip tool-name-chip"${title}>${esc(tool.function_name)}${execHtml}</span>`;
}
function renderToolTiming(toolMeta) {
  const parts = [];
  if (hasMetricValue(toolMeta?.generation_duration_ms)) parts.push(`generation ${fmtMs(toolMeta.generation_duration_ms)}`);
  return parts.length ? ` / ${parts.map(esc).join(" / ")}` : "";
}
function stepToolLabels(step, meta) {
  const labels = [];
  (step.tool_calls || []).forEach(tool => {
    const name = String(tool.function_name || "").trim();
    if (!name) return;
    const exec = toolExecutionText(toolMetaFor(meta, tool.tool_call_id));
    labels.push(exec ? `${name} ${exec}` : name);
  });
  return labels;
}
function renderStepRail(step, meta) {
  const toolItems = [];
  const toolCalls = (step.tool_calls || []).length;
  const toolErrors = meta?.tool_error ? 1 : 0;
  if (toolCalls || toolErrors) {
    toolItems.push(`<span class="rail-chip rail-chip-tools">${esc(toolCallRatio(toolCalls, toolErrors))} tools</span>`);
    const toolLabels = stepToolLabels(step, meta);
    if (toolLabels.length) {
      const text = toolLabels.join(", ");
      toolItems.push(`<span class="rail-chip rail-chip-tool-list" title="${esc(text)}">${esc(text)}</span>`);
    }
  }
  const tokens = stepTokenTotal(step, meta);
  if (tokens !== null && tokens !== undefined) toolItems.push(`<span class="rail-chip rail-chip-tokens">${fmtNum(tokens)} tok</span>`);
  const tools = toolItems.length ? `<div class="rail-tools">${toolItems.join("")}</div>` : `<div class="rail-tools"></div>`;
  const time = `<div class="rail-time"><span class="rail-chip rail-chip-step-time" title="step span">step ${esc(fmtRailMs(meta?.duration_ms))}</span><span class="rail-chip rail-chip-elapsed-time" title="elapsed since trajectory start">elapsed ${esc(fmtRailMs(meta?.elapsed_ms))}</span></div>`;
  return `${tools}${time}`;
}
function stepMeta(meta, stepId) {
  return (meta?.steps || []).find(item => item.step_id === stepId) || null;
}
function toolCallRatio(total, errors) {
  const callTotal = Math.max(0, Number(total || 0));
  const errorTotal = Math.max(0, Number(errors || 0));
  const successful = Math.max(0, callTotal - errorTotal);
  return `${successful}/${callTotal}`;
}
function toolMetaFor(meta, toolCallId) {
  return (meta?.tool_calls || []).find(item => item.tool_call_id === toolCallId) || null;
}
function observationMetaFor(meta, sourceCallId) {
  return (meta?.observations || []).find(item => item.source_call_id === sourceCallId) || null;
}
function valuePreview(value) {
  if (value === null || value === undefined) return "";
  if (typeof value === "string") return value;
  return JSON.stringify(value, null, 2);
}
function stepTokenTotal(step, meta) {
  const metrics = step.metrics || {};
  const values = [metrics.prompt_tokens, metrics.completion_tokens, metrics.cached_tokens, metrics.usage?.total_tokens].filter(value => value !== null && value !== undefined && !Number.isNaN(Number(value))).map(Number);
  return values.length ? values.reduce((sum, value) => sum + value, 0) : null;
}
function shortJson(value) {
  if (value === null || value === undefined) return "-";
  const text = typeof value === "string" ? value : JSON.stringify(value);
  return text.length > 180 ? `${text.slice(0, 180)}...` : text;
}
function renderLeaderboard() {
  const entries = state.view.comparison?.leaderboard?.entries || [];
  if (!entries.length) {
    $("leaderboard").innerHTML = `<div class="panel-head"><div><p class="eyebrow">leaderboard</p><h2 id="leaderboard-title">Leaderboard</h2><p class="copy">No comparison include or no leaderboard entries.</p></div></div>`;
    return;
  }
  const comparisonRows = leaderboardComparisonRows(entries);
  const trialRows = leaderboardTrialRows(entries);
  const aggregateColumns = comparisonColumns(comparisonRows);
  const detailColumns = trialColumns(trialRows);
  $("leaderboard").innerHTML = `
    <div class="panel-head"><div><p class="eyebrow">leaderboard</p><h2 id="leaderboard-title">Agent / Model Comparison</h2><p class="copy">Flat rows compare agent, model, and task results directly. Identity columns filter; numeric columns sort.</p></div></div>
    ${renderInteractiveTable("leaderboard-aggregate", aggregateColumns, comparisonRows, "aggregate rows")}
    <details class="trial-details flat-trials" data-detail-key="leaderboard-trials" ${state.openDetails["leaderboard-trials"] ? "open" : ""}>
      <summary>Trial details</summary>
      ${renderInteractiveTable("leaderboard-trials", detailColumns, trialRows, "trial rows")}
    </details>
  `;
  bindLeaderboardControls();
}
function leaderboardComparisonRows(entries) {
  return entries.flatMap(entry => (entry.tasks || []).map(task => {
    const taskTrials = (task.trial_keys || []).map(trialFor).filter(Boolean);
    return {
      rank: entry.rank,
      variant_id: entry.variant_id || "-",
      variant_label: entry.variant_label || "-",
      agent_id: entry.agent_id,
      model_name: entry.model_name || "-",
      task_id: task.task_id,
      task_family: task.task_family || "-",
      total_trials: task.total_trials,
      successes: task.successes,
      pass_rate: task.pass_rate,
      average_score: task.average_score,
      average_duration_ms: task.average_duration_ms,
      trial_keys: task.trial_keys || [],
      average_tokens: averageOptional(taskTrials.map(trial => trialTotalTokens(trial.trial_key))),
      average_cost_usd: averageOptional(taskTrials.map(trial => trialCost(trial.trial_key)))
    };
  }));
}
function filteredLeaderboardComparisonRows() {
  const rows = leaderboardComparisonRows(state.view?.comparison?.leaderboard?.entries || []);
  return applyTableFilters("leaderboard-aggregate", comparisonColumns(rows), rows);
}
function leaderboardTrialRows(entries) {
  return entries.flatMap(entry => (entry.trial_keys || []).map(trialFor).filter(Boolean).map(trial => ({
    ...trial,
    rank: entry.rank,
    variant_id: trial.variant_id || entry.variant_id || "-",
    variant_label: trial.variant_label || entry.variant_label || "-",
    agent_id: trajectoryFor(trial.trial_key)?.agent?.name || entry.agent_id,
    model_name: trajectoryFor(trial.trial_key)?.agent?.model_name || entry.model_name || "-",
    total_tokens: trialTotalTokens(trial.trial_key),
    total_cost_usd: trialCost(trial.trial_key)
  })));
}
function sumOptional(values) {
  const numeric = values.filter(value => value !== null && value !== undefined && !Number.isNaN(Number(value))).map(Number);
  return numeric.length ? numeric.reduce((sum, value) => sum + value, 0) : null;
}
function averageOptional(values) {
  const numeric = values.filter(value => value !== null && value !== undefined && !Number.isNaN(Number(value))).map(Number);
  return numeric.length ? numeric.reduce((sum, value) => sum + value, 0) / numeric.length : null;
}
function notesForKeys(trialKeys) {
  const keys = new Set((trialKeys || []).filter(Boolean));
  return (state.view?.annotations?.notes || []).filter(note => keys.has(note.trial_key));
}
function notesPlainText(notes) {
  return notes.map(note => String(note.markdown || "").trim()).filter(Boolean).join("\n\n");
}
function notesFullTextForKeys(trialKeys) {
  return notesPlainText(notesForKeys(trialKeys));
}
function notesSummaryForKeys(trialKeys) {
  const notes = notesForKeys(trialKeys);
  if (!notes.length) return "-";
  const text = notesPlainText(notes).replace(/\s+/g, " ").trim();
  return text.length > 96 ? `${text.slice(0, 96)}...` : text;
}
function renderNotesCell(trialKeys) {
  const summary = notesSummaryForKeys(trialKeys);
  return summary === "-" ? `<span class="muted">-</span>` : `<span class="note-snippet">${esc(summary)}</span>`;
}
function hasVisibleVariant(rows) {
  return rows.some(row => [row.variant_id, row.variant_label].some(value => value && value !== "-"));
}
function variantColumn() {
  return { key: "variant_label", label: "Variant", width: "220px", filterable: true, value: row => row.variant_label || "-" };
}
function comparisonColumns(rows = []) {
  const columns = [
    { key: "model_name", label: "Model", width: "170px", filterable: true, value: row => row.model_name },
    { key: "task_id", label: "Task", width: "210px", filterable: true, value: row => row.task_id },
    { key: "task_family", label: "Family", width: "150px", filterable: true, value: row => row.task_family },
    { key: "total_trials", label: "Trials", width: "82px", type: "number", numeric: true, sortable: true, value: row => row.total_trials },
    { key: "successes", label: "Successes", width: "96px", type: "number", numeric: true, sortable: true, value: row => row.successes },
    { key: "pass_rate", label: "Pass Rate", width: "104px", type: "number", numeric: true, sortable: true, value: row => row.pass_rate, format: pct },
    { key: "average_score", label: "Score", width: "88px", type: "number", numeric: true, sortable: true, value: row => row.average_score, format: fmtScore },
    { key: "average_duration_ms", label: "Duration", width: "104px", type: "number", numeric: true, sortable: true, value: row => row.average_duration_ms, format: fmtMs },
    { key: "average_tokens", label: "Tokens", width: "100px", type: "number", numeric: true, sortable: true, value: row => row.average_tokens, format: fmtNum },
    { key: "average_cost_usd", label: "Cost", width: "92px", type: "number", numeric: true, sortable: true, value: row => row.average_cost_usd, format: fmtCost },
    { key: "notes", label: "Notes", width: "180px", value: row => notesSummaryForKeys(row.trial_keys || []), html: row => renderNotesCell(row.trial_keys || []), cellTitle: row => notesFullTextForKeys(row.trial_keys || []) }
  ];
  if (hasVisibleVariant(rows)) columns.unshift(variantColumn());
  return columns;
}
function trialColumns(rows = []) {
  const columns = [
    { key: "agent_id", label: "Agent", width: "150px", filterable: true, value: row => row.agent_id },
    { key: "model_name", label: "Model", width: "160px", filterable: true, value: row => row.model_name },
    { key: "task_id", label: "Task", width: "190px", filterable: true, value: row => row.task_id },
    { key: "status", label: "Result", width: "96px", filterable: true, value: row => row.status, html: row => `<span class="stamp ${statusClass(row.status)}">${esc(row.status)}</span>` },
    { key: "score", label: "Score", width: "88px", type: "number", numeric: true, sortable: true, value: row => row.score, format: fmtScore },
    { key: "duration_ms", label: "Duration", width: "104px", type: "number", numeric: true, sortable: true, value: row => row.duration_ms, format: fmtMs },
    { key: "total_tokens", label: "Tokens", width: "100px", type: "number", numeric: true, sortable: true, value: row => row.total_tokens, format: fmtNum },
    { key: "notes", label: "Notes", width: "220px", value: row => notesSummaryForKeys([row.trial_key]), html: row => renderNotesCell([row.trial_key]), cellTitle: row => notesFullTextForKeys([row.trial_key]) }
  ];
  if (hasMultiTrialMatrixCell()) columns.unshift(trialIdentityColumn());
  if (hasVisibleVariant(rows)) columns.unshift(variantColumn());
  return columns;
}
function trialIdentityColumn() {
  return {
    key: "trial_identity",
    label: "Trial",
    width: "110px",
    value: row => {
      const ordinal = trialOrdinal(row.trial_key);
      return ordinal ? `#${ordinal} ${shortTrialKey(row.trial_key)}` : shortTrialKey(row.trial_key);
    },
    html: row => {
      const ordinal = trialOrdinal(row.trial_key);
      const latest = matrixCellForTrial(row.trial_key)?.representative_trial_key === row.trial_key;
      return `<span class="trial-id-chip">${ordinal ? `#${ordinal}` : "-"}<code>${esc(shortTrialKey(row.trial_key))}</code>${latest ? `<em>latest</em>` : ""}</span>`;
    },
    cellTitle: row => row.trial_key || ""
  };
}
function tableControls(tableKey) {
  state.tables[tableKey] ||= { sort: null, direction: "asc", filters: {} };
  return state.tables[tableKey];
}
function tableText(row, column) {
  const raw = column.value(row);
  return column.format ? column.format(raw, row) : (raw ?? "-");
}
function tableFilterText(row, column) {
  const raw = column.value(row);
  return `${raw ?? ""} ${tableText(row, column)}`;
}
function tableFilterValue(row, column) {
  return String(tableText(row, column));
}
function selectedFilters(controls, columnKey) {
  const value = controls.filters[columnKey];
  if (Array.isArray(value)) return value;
  if (value === null || value === undefined || value === "") return [];
  controls.filters[columnKey] = [String(value)];
  return controls.filters[columnKey];
}
function filterStateKey(tableKey, columnKey) {
  return `${tableKey}--${columnKey}`;
}
function filterOptions(rows, column) {
  return [...new Set(rows.map(row => tableFilterValue(row, column)).filter(Boolean))]
    .sort((left, right) => left.localeCompare(right, undefined, { numeric: true, sensitivity: "base" }));
}
function compareTableValues(left, right, type, direction) {
  const leftMissing = left === null || left === undefined || left === "" || (type === "number" && Number.isNaN(Number(left)));
  const rightMissing = right === null || right === undefined || right === "" || (type === "number" && Number.isNaN(Number(right)));
  if (leftMissing || rightMissing) return leftMissing === rightMissing ? 0 : leftMissing ? 1 : -1;
  const delta = type === "number"
    ? Number(left) - Number(right)
    : String(left).localeCompare(String(right), undefined, { numeric: true, sensitivity: "base" });
  return direction === "desc" ? -delta : delta;
}
function applyTableControls(tableKey, columns, rows) {
  const filtered = applyTableFilters(tableKey, columns, rows);
  const controls = tableControls(tableKey);
  const sortColumn = columns.find(column => column.key === controls.sort && column.sortable);
  if (sortColumn) {
    filtered.sort((left, right) => compareTableValues(sortColumn.value(left), sortColumn.value(right), sortColumn.type, controls.direction));
  }
  return filtered;
}
function applyTableFilters(tableKey, columns, rows) {
  const controls = tableControls(tableKey);
  const filtered = rows.filter(row => columns.every(column => {
    if (!column.filterable) return true;
    const selected = selectedFilters(controls, column.key);
    return !selected.length || selected.includes(tableFilterValue(row, column));
  }));
  return filtered;
}
function tableRowTrialKey(row) {
  if (row.trial_key) return row.trial_key;
  return (row.trial_keys || [])[0] || "";
}
function tableRowHasSelectedTrial(row) {
  if (!state.selectedTrial) return false;
  if (row.trial_key) return row.trial_key === state.selectedTrial;
  return (row.trial_keys || []).includes(state.selectedTrial);
}
function renderTableRow(row, columns) {
  const trialKey = tableRowTrialKey(row);
  const classes = [trialKey ? "clickable-row" : "", tableRowHasSelectedTrial(row) ? "selected-row" : ""].filter(Boolean).join(" ");
  const attrs = [
    classes ? `class="${classes}"` : "",
    trialKey ? `data-row-trial="${esc(trialKey)}"` : "",
    trialKey ? `title="${esc(trialKey)}"` : ""
  ].filter(Boolean).join(" ");
  return `<tr ${attrs}>${columns.map(column => renderDataCell(row, column)).join("")}</tr>`;
}
function renderInteractiveTable(tableKey, columns, rows, label) {
  const controls = tableControls(tableKey);
  const filtered = applyTableControls(tableKey, columns, rows);
  const colgroup = columns.map(column => `<col ${column.width ? `style="width:${esc(column.width)}"` : ""}>`).join("");
  const headers = columns.map(column => {
    const active = controls.sort === column.key;
    const mark = active ? (controls.direction === "desc" ? "&#9660;" : "&#9650;") : "&#8597;";
    if (!column.sortable) {
      return `<th class="${column.numeric ? "num" : ""}" title="${esc(column.label)}"><span class="static-head" title="${esc(column.label)}">${esc(column.label)}</span></th>`;
    }
    return `<th class="${column.numeric ? "num" : ""}" title="${esc(column.label)}"><button class="sort-button ${active ? `active ${controls.direction === "desc" ? "sort-desc" : "sort-asc"}` : ""}" type="button" data-table-sort="${esc(tableKey)}" data-column="${esc(column.key)}" aria-label="Sort ${esc(column.label)}" title="${esc(column.label)}"><span class="sort-label">${esc(column.label)}</span><span class="sort-mark">${mark}</span></button></th>`;
  }).join("");
  const filters = columns.map(column => {
    if (!column.filterable) return `<th class="${column.numeric ? "num" : ""}"><span class="filter-slot" aria-label="sort only"></span></th>`;
    return `<th class="${column.numeric ? "num" : ""}">${renderMultiFilter(tableKey, column, rows)}</th>`;
  }).join("");
  const body = filtered.length
    ? filtered.map(row => renderTableRow(row, columns)).join("")
    : `<tr><td class="table-empty" colspan="${columns.length}">No matching rows</td></tr>`;
  return `<div class="table-shell"><div class="table-wrap"><table class="data-table"><colgroup>${colgroup}</colgroup><thead><tr>${headers}</tr><tr class="table-filters">${filters}</tr></thead><tbody>${body}</tbody></table></div></div>`;
}
function renderMultiFilter(tableKey, column, rows) {
  const controls = tableControls(tableKey);
  const selected = selectedFilters(controls, column.key);
  const options = filterOptions(rows, column);
  const key = filterStateKey(tableKey, column.key);
  const label = selected.length ? `${selected.length} selected` : "All";
  const checks = options.map(option => {
    const checked = selected.includes(option) ? "checked" : "";
    return `<label class="multi-option" title="${esc(option)}"><input type="checkbox" data-table-filter="${esc(tableKey)}" data-column="${esc(column.key)}" data-filter-value="${esc(option)}" ${checked}> <span>${esc(option)}</span></label>`;
  }).join("");
  return `<details class="multi-filter" data-filter-popover="${esc(key)}" ${state.openFilters[key] ? "open" : ""}><summary><span>${esc(label)}</span><span class="multi-caret">&#9662;</span></summary><div class="multi-menu">${checks || `<p class="multi-empty">No values</p>`}<button class="filter-clear" type="button" data-filter-clear="${esc(tableKey)}" data-column="${esc(column.key)}" ${selected.length ? "" : "disabled"}>Clear</button></div></details>`;
}
function renderDataCell(row, column) {
  const classes = [column.numeric ? "num" : "", column.className || ""].filter(Boolean).join(" ");
  const html = column.html ? column.html(row) : esc(tableText(row, column));
  const title = column.cellTitle ? column.cellTitle(row) : "";
  return `<td class="${classes}" ${title ? `title="${esc(title)}"` : ""}>${html}</td>`;
}
function bindLeaderboardControls() {
  document.querySelectorAll("[data-table-sort]").forEach(button => {
    button.addEventListener("click", () => {
      const controls = tableControls(button.dataset.tableSort);
      if (controls.sort === button.dataset.column) {
        controls.direction = controls.direction === "asc" ? "desc" : "asc";
      } else {
        controls.sort = button.dataset.column;
        controls.direction = "asc";
      }
      renderLeaderboard();
    });
  });
  document.querySelectorAll("[data-table-filter]").forEach(input => {
    input.addEventListener("change", () => {
      const controls = tableControls(input.dataset.tableFilter);
      const selected = new Set(selectedFilters(controls, input.dataset.column));
      input.checked ? selected.add(input.dataset.filterValue) : selected.delete(input.dataset.filterValue);
      controls.filters[input.dataset.column] = [...selected];
      state.openFilters[filterStateKey(input.dataset.tableFilter, input.dataset.column)] = true;
      renderLeaderboard();
      if (input.dataset.tableFilter === "leaderboard-aggregate") {
        renderMatrix();
        renderTrace();
      }
    });
  });
  document.querySelectorAll("[data-filter-clear]").forEach(button => {
    button.addEventListener("click", () => {
      const controls = tableControls(button.dataset.filterClear);
      controls.filters[button.dataset.column] = [];
      state.openFilters[filterStateKey(button.dataset.filterClear, button.dataset.column)] = true;
      renderLeaderboard();
      if (button.dataset.filterClear === "leaderboard-aggregate") {
        renderMatrix();
        renderTrace();
      }
    });
  });
  document.querySelectorAll("[data-filter-popover]").forEach(details => {
    details.addEventListener("toggle", () => {
      state.openFilters[details.dataset.filterPopover] = details.open;
    });
  });
  document.querySelectorAll("[data-row-trial]").forEach(row => {
    row.addEventListener("click", () => {
      state.selectedTrial = row.dataset.rowTrial;
      state.selectedStepId = null;
      renderLeaderboard();
      renderMatrix();
      renderTrace();
    });
  });
  document.querySelectorAll("[data-detail-key]").forEach(details => {
    details.addEventListener("toggle", () => {
      state.openDetails[details.dataset.detailKey] = details.open;
    });
  });
}
function renderManualNote(row) {
  return `<article class="manual-note"><div class="note-body">${renderMarkdown(row.markdown || "")}</div></article>`;
}
function renderMarkdown(markdown) {
  const lines = String(markdown ?? "").split(/\r?\n/);
  const out = [];
  let paragraph = [];
  let list = [];
  let code = [];
  let inCode = false;
  const flushParagraph = () => {
    if (paragraph.length) {
      out.push(`<p>${renderInlineMarkdown(paragraph.join(" "))}</p>`);
      paragraph = [];
    }
  };
  const flushList = () => {
    if (list.length) {
      out.push(`<ul>${list.map(item => `<li>${renderInlineMarkdown(item)}</li>`).join("")}</ul>`);
      list = [];
    }
  };
  const flushCode = () => {
    out.push(`<pre class="note-code">${code.join("\n")}</pre>`);
    code = [];
  };
  for (const rawLine of lines) {
    const line = rawLine.replace(/\s+$/, "");
    if (line.trim().startsWith("```")) {
      if (inCode) {
        flushCode();
        inCode = false;
      } else {
        flushParagraph();
        flushList();
        inCode = true;
      }
      continue;
    }
    if (inCode) {
      code.push(esc(line));
      continue;
    }
    if (!line.trim()) {
      flushParagraph();
      flushList();
      continue;
    }
    const heading = line.match(/^(#{1,4})\s+(.+)$/);
    if (heading) {
      flushParagraph();
      flushList();
      out.push(`<h4>${renderInlineMarkdown(heading[2])}</h4>`);
      continue;
    }
    const bullet = line.match(/^\s*[-*]\s+(.+)$/);
    if (bullet) {
      flushParagraph();
      list.push(bullet[1]);
      continue;
    }
    flushList();
    paragraph.push(line.trim());
  }
  if (inCode) flushCode();
  flushParagraph();
  flushList();
  return out.join("") || `<p class="muted">No note text.</p>`;
}
function renderInlineMarkdown(value) {
  return esc(value)
    .replace(/\[([^\]]+)\]\((https?:\/\/[^)\s"]+)\)/g, `<a href="$2" rel="noreferrer noopener">$1</a>`)
    .replace(/`([^`]+)`/g, `<code>$1</code>`)
    .replace(/\*\*([^*]+)\*\*/g, `<strong>$1</strong>`);
}
function includeList(view) {
  return (view.includes || []).join(",");
}
async function refreshAnalysisStatus() {
  if (!window.PEVAL_IS_SERVE) return;
  try {
    const status = await rpc("analysis.status");
    state.analysisEnabled = !!status.enabled;
    $("analyze").disabled = !state.analysisEnabled;
    $("batch").disabled = !state.analysisEnabled;
  } catch (_err) {
    state.analysisEnabled = false;
  }
}
async function analyzeSelected() {
  if (!state.selectedTrial) return;
  await rpc("analysis.run", { trial_key: state.selectedTrial, overwrite: true });
  await loadView();
}
async function analyzeFailed() {
  await rpc("analysis.batch_failed", { overwrite: true });
  await loadView();
}
$("refresh").addEventListener("click", loadView);
$("analyze").addEventListener("click", analyzeSelected);
$("batch").addEventListener("click", analyzeFailed);
if (window.PEVAL_IS_SERVE) {
  $("serve-actions").hidden = false;
  refreshAnalysisStatus();
}
loadView();
"###
}
