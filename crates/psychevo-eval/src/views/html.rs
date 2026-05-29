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
    <section class="evidence-wrap" id="evidence"></section>
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
const fmtMs = value => value === null || value === undefined || Number.isNaN(Number(value)) ? "-" : Number(value) > 0 && Number(value) < 1000 ? "<1s" : `${Math.round(Number(value) / 1000)}s`;
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
  if (state.metricMode === "tokens") return trialFor(cell.representative_trial_key)?.total_tokens ?? null;
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
  const visibleKeys = new Set(visibleRows.map(row => `${row.task_id}\u0000${row.agent_id}\u0000${row.model_name || "-"}`));
  return (state.view.matrix?.cells || []).filter(cell => {
    const model = cell.model_name || "-";
    return visibleKeys.has(`${cell.task_id}\u0000${cell.agent_id}\u0000${model}`);
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
  return (state.view?.trials || []).find(trial => trial.trial_key === trialKey);
}
function trajectoryFor(trialKey) {
  return (state.view?.trajectory || []).find(trajectory => trajectory.trajectory_id === trialKey);
}
function trajectoryMetaFor(trialKey) {
  return (state.view?.trajectory_meta || []).find(meta => meta.trial_key === trialKey);
}
function render(view) {
  state.view = view;
  if (!state.selectedTrial) {
    const firstFailed = (view.matrix?.cells || []).find(cell => lower(cell.status) !== "passed");
    state.selectedTrial = (firstFailed || (view.matrix?.cells || [])[0])?.representative_trial_key || view.trials?.[0]?.trial_key || null;
  }
  $("report-title").textContent = view.scope?.benchmark || "Evaluation Workbench";
  $("report-copy").textContent = `${view.summary.total_trials} trials - ${view.summary.passed_trials} passed - ${view.summary.failed_trials} failed - includes ${includeList(view)}`;
  renderScoreStrip(view);
  renderMetricControls();
  renderLeaderboard();
  renderMatrix();
  renderTrace();
  renderEvidence();
}
function renderScoreStrip(view) {
  $("score-strip").innerHTML = [
    [view.summary.total_trials, "trials"],
    [pct(view.summary.total_trials ? view.summary.passed_trials / view.summary.total_trials : 0), "pass rate"],
    [fmtCost(view.summary.metrics?.cost?.amount_usd), "cost"]
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
  const tasks = state.view.matrix?.task_axis || [];
  const agents = state.view.matrix?.agent_axis || [];
  const visible = visibleCells();
  if (!visible.some(cell => cell.representative_trial_key === state.selectedTrial)) {
    state.selectedTrial = visible[0]?.representative_trial_key || state.view.trials?.[0]?.trial_key || null;
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
      cells.push(`<button class="cell ${statusClass(cell.status, cell.failure_class)} ${shade} ${cell.representative_trial_key === state.selectedTrial ? "selected" : ""}" type="button" data-trial="${esc(cell.representative_trial_key)}"><strong>${esc(formatMetric(value))}</strong><span>${esc(cell.status)} / ${esc(cell.representative_trial_key.replace(/:t001$/, ""))}<br>${fmtMs(cell.duration_ms)}</span></button>`);
    });
  });
  $("matrix").innerHTML = cells.join("");
  document.querySelectorAll("[data-trial]").forEach(button => {
    button.addEventListener("click", () => {
      state.selectedTrial = button.dataset.trial;
      state.selectedStepId = null;
      renderMatrix();
      renderTrace();
      $("trace").scrollIntoView({ block: "start" });
    });
  });
}
function infoGrid(items) {
  return `<div class="info-grid">${items.map(([label, value]) => `<div><span>${esc(label)}</span><strong>${esc(value)}</strong></div>`).join("")}</div>`;
}
function renderTrace() {
  const trial = trialFor(state.selectedTrial) || trialFor(visibleCells()[0]?.representative_trial_key) || (state.view.trials || [])[0];
  if (!trial) {
    $("trace").innerHTML = `<div class="trace-head"><div><p class="eyebrow">selected trial trajectory</p><h2 id="trace-title">No trials</h2></div></div>`;
    return;
  }
  state.selectedTrial = trial.trial_key;
  const trajectory = trajectoryFor(trial.trial_key);
  const trajectoryMeta = trajectoryMetaFor(trial.trial_key);
  const model = trial.model_name || trajectory?.agent?.model_name || "-";
  const agentName = trajectory?.agent?.name || trial.agent_id || "-";
  $("trace").innerHTML = `
    <div class="trace-head">
      <div>
        <p class="eyebrow">selected trial trajectory</p>
        <h2 id="trace-title" class="trace-title"><span>${esc(trial.task_id)}</span><code>${esc(trial.trial_key)}</code></h2>
      </div>
    </div>
    <h3>Run</h3>
    ${infoGrid([
      ["trial", trial.trial_key],
      ["session", trajectory?.session_id || "-"],
      ["agent / model", `${agentName} / ${model}`],
      ["time", `${fmtDate(trial.started_at_ms)} -> ${fmtDate(trial.finished_at_ms)}`],
      ["duration", fmtMs(trial.duration_ms)],
      ["steps/events", `${trajectoryMeta?.total_steps ?? (trajectory?.steps || []).length}/${trajectoryMeta?.total_events ?? "-"}`],
      ["system exposed", trajectoryMeta?.system_exposed ? "yes" : "no"],
      ["reasoning exposed", trajectoryMeta?.reasoning_exposed ? "yes" : "no"]
    ])}
    <h3>Result</h3>
    ${infoGrid([
      ["status", trial.status],
      ["score", fmtScore(trial.score)],
      ["evaluator", trial.score_message || "-"],
      ["tokens", fmtNum(trial.total_tokens)],
      ["turns", trial.turns ?? "-"],
      ["tool success / total", toolCallRatio(trial.tool_calls, trial.tool_errors)],
      ["cost", fmtCost(trial.cost_usd)]
    ])}
    <h3>Steps (${trajectoryMeta?.total_steps ?? (trajectory?.steps || []).length ?? 0})</h3>
    <div class="step-list" id="step-list">${trajectory ? (trajectory.steps || []).map(step => renderStep(step, trajectoryMeta)).join("") : `<p class="copy">No trajectory include for this Trial.</p>`}</div>
  `;
  document.querySelectorAll("#step-list .step").forEach(row => {
    row.addEventListener("click", () => {
      state.selectedStepId = Number(row.dataset.step);
      document.querySelectorAll("#step-list .step").forEach(item => item.classList.toggle("selected-step", Number(item.dataset.step) === state.selectedStepId));
    });
  });
}
function renderStep(step) {
  const meta = arguments[1] ? stepMeta(arguments[1], step.step_id) : null;
  const preview = valuePreview(step.message).trim() || "(Empty Message)";
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
    html += `<div class="block tool-block"><h4>Tool Calls</h4><p><span class="chip">${esc(tool.function_name)}</span> <span class="muted">ID: ${esc(tool.tool_call_id)}${toolMeta?.status ? ` / ${esc(toolMeta.status)}` : ""}${renderToolTiming(toolMeta)}</span></p><pre>${esc(valuePreview(tool.arguments))}</pre></div>`;
  });
  ((step.observation && step.observation.results) || []).forEach(observation => {
    const observationMeta = observationMetaFor(meta, observation.source_call_id);
    html += `<div class="block observation-block"><h4 class="${observationMeta?.tool_error ? "danger" : ""}">Observations</h4><p class="muted">Result for: ${esc(observation.source_call_id || "-")}${observationMeta?.status ? ` / ${esc(observationMeta.status)}` : ""}</p><pre>${esc(valuePreview(observation.content))}</pre></div>`;
  });
  html += renderStepMetrics(step, meta);
  return html;
}
function hasMetricValue(value) {
  return value !== null && value !== undefined && value !== "" && !Number.isNaN(Number(value));
}
function renderStepMetrics(step, meta) {
  const metrics = step.metrics || {};
  const items = [];
  const toolExecutionMs = stepToolExecutionMs(meta);
  if (hasMetricValue(meta?.duration_ms)) items.push(["step span", fmtMs(meta.duration_ms)]);
  if (hasMetricValue(meta?.elapsed_ms)) items.push(["elapsed", fmtMs(meta.elapsed_ms)]);
  if (hasMetricValue(toolExecutionMs)) items.push(["tool time", fmtMs(toolExecutionMs)]);
  if (hasMetricValue(metrics.prompt_tokens)) items.push(["prompt", fmtNum(metrics.prompt_tokens)]);
  if (hasMetricValue(metrics.completion_tokens)) items.push(["completion", fmtNum(metrics.completion_tokens)]);
  if (hasMetricValue(metrics.cached_tokens)) items.push(["cached", fmtNum(metrics.cached_tokens)]);
  if (hasMetricValue(metrics.cost_usd)) items.push(["cost", fmtCost(metrics.cost_usd)]);
  if (hasMetricValue(meta?.llm_call_count)) items.push(["llm calls", fmtNum(meta.llm_call_count)]);
  if (!items.length) return "";
  return `<div class="block"><h4>Metrics</h4><pre>${items.map(([key, value]) => `${esc(key)} ${esc(value)}`).join(" / ")}</pre></div>`;
}
function renderToolTiming(toolMeta) {
  const parts = [];
  if (hasMetricValue(toolMeta?.generation_duration_ms)) parts.push(`generation ${fmtMs(toolMeta.generation_duration_ms)}`);
  if (hasMetricValue(toolMeta?.execution_duration_ms)) parts.push(`tool time ${fmtMs(toolMeta.execution_duration_ms)}`);
  return parts.length ? ` / ${parts.map(esc).join(" / ")}` : "";
}
function stepToolExecutionMs(meta) {
  const values = (meta?.tool_calls || [])
    .map(tool => tool.execution_duration_ms)
    .filter(hasMetricValue)
    .map(Number);
  return values.length ? values.reduce((sum, value) => sum + value, 0) : null;
}
function renderStepRail(step, meta) {
  const items = [];
  if (meta?.duration_ms !== null && meta?.duration_ms !== undefined) items.push(`<span>step ${fmtMs(meta.duration_ms)}</span>`);
  if (meta?.elapsed_ms !== null && meta?.elapsed_ms !== undefined) items.push(`<span>elapsed ${fmtMs(meta.elapsed_ms)}</span>`);
  const toolExecutionMs = stepToolExecutionMs(meta);
  if (hasMetricValue(toolExecutionMs)) items.push(`<span>tool ${fmtMs(toolExecutionMs)}</span>`);
  const tokens = stepTokenTotal(step, meta);
  if (tokens !== null && tokens !== undefined) items.push(`<span>${fmtNum(tokens)} tok</span>`);
  const toolCalls = (step.tool_calls || []).length;
  const toolErrors = meta?.tool_error ? 1 : 0;
  if (toolCalls || toolErrors) items.push(`<span>tools ${toolCallRatio(toolCalls, toolErrors)}</span>`);
  return items.join("");
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
  if (meta?.token_total !== null && meta?.token_total !== undefined) return meta.token_total;
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
  const entries = state.view.leaderboard?.entries || [];
  if (!entries.length) {
    $("leaderboard").innerHTML = `<div class="panel-head"><div><p class="eyebrow">leaderboard</p><h2 id="leaderboard-title">Leaderboard</h2><p class="copy">No leaderboard entries.</p></div></div>`;
    return;
  }
  const comparisonRows = leaderboardComparisonRows(entries);
  const trialRows = leaderboardTrialRows(entries);
  $("leaderboard").innerHTML = `
    <div class="panel-head"><div><p class="eyebrow">leaderboard</p><h2 id="leaderboard-title">Agent / Model Comparison</h2><p class="copy">Flat rows compare agent, model, and task results directly. Identity columns filter; numeric columns sort.</p></div></div>
    ${renderInteractiveTable("leaderboard-aggregate", comparisonColumns(), comparisonRows, "aggregate rows")}
    <details class="trial-details flat-trials" data-detail-key="leaderboard-trials" ${state.openDetails["leaderboard-trials"] ? "open" : ""}>
      <summary>Trial details</summary>
      ${renderInteractiveTable("leaderboard-trials", trialColumns(), trialRows, "trial rows")}
    </details>
  `;
  bindLeaderboardControls();
}
function leaderboardComparisonRows(entries) {
  return entries.flatMap(entry => (entry.tasks || []).map(task => {
    const taskTrials = (task.trial_keys || []).map(trialFor).filter(Boolean);
    return {
      rank: entry.rank,
      agent_id: entry.agent_id,
      model_name: entry.model_name || "-",
      task_id: task.task_id,
      task_family: task.task_family || "-",
      total_trials: task.total_trials,
      successes: task.successes,
      pass_rate: task.pass_rate,
      average_score: task.average_score,
      average_duration_ms: task.average_duration_ms,
      total_tokens: sumOptional(taskTrials.map(trial => trial.total_tokens)),
      total_cost_usd: sumOptional(taskTrials.map(trial => trial.cost_usd))
    };
  }));
}
function filteredLeaderboardComparisonRows() {
  return applyTableFilters("leaderboard-aggregate", comparisonColumns(), leaderboardComparisonRows(state.view?.leaderboard?.entries || []));
}
function leaderboardTrialRows(entries) {
  return entries.flatMap(entry => (entry.trial_keys || []).map(trialFor).filter(Boolean).map(trial => ({
    ...trial,
    rank: entry.rank,
    agent_id: trial.agent_id || entry.agent_id,
    model_name: trial.model_name || entry.model_name || "-"
  })));
}
function sumOptional(values) {
  const numeric = values.filter(value => value !== null && value !== undefined && !Number.isNaN(Number(value))).map(Number);
  return numeric.length ? numeric.reduce((sum, value) => sum + value, 0) : null;
}
function comparisonColumns() {
  return [
    { key: "rank", label: "Rank", width: "70px", type: "number", numeric: true, sortable: true, value: row => row.rank, format: value => `#${value}` },
    { key: "agent_id", label: "Agent", width: "160px", filterable: true, value: row => row.agent_id },
    { key: "model_name", label: "Model", width: "170px", filterable: true, value: row => row.model_name },
    { key: "task_id", label: "Task", width: "210px", filterable: true, value: row => row.task_id },
    { key: "task_family", label: "Family", width: "160px", filterable: true, value: row => row.task_family },
    { key: "total_trials", label: "Trials", width: "82px", type: "number", numeric: true, sortable: true, value: row => row.total_trials },
    { key: "successes", label: "Successes", width: "96px", type: "number", numeric: true, sortable: true, value: row => row.successes },
    { key: "pass_rate", label: "Pass Rate", width: "104px", type: "number", numeric: true, sortable: true, value: row => row.pass_rate, format: pct },
    { key: "average_score", label: "Score", width: "88px", type: "number", numeric: true, sortable: true, value: row => row.average_score, format: fmtScore },
    { key: "average_duration_ms", label: "Duration", width: "104px", type: "number", numeric: true, sortable: true, value: row => row.average_duration_ms, format: fmtMs },
    { key: "total_tokens", label: "Tokens", width: "100px", type: "number", numeric: true, sortable: true, value: row => row.total_tokens, format: fmtNum },
    { key: "total_cost_usd", label: "Cost", width: "92px", type: "number", numeric: true, sortable: true, value: row => row.total_cost_usd, format: fmtCost }
  ];
}
function trialColumns() {
  return [
    { key: "trial_key", label: "Trial", width: "17%", filterable: true, value: row => row.trial_key },
    { key: "agent_id", label: "Agent", width: "11%", filterable: true, value: row => row.agent_id },
    { key: "model_name", label: "Model", width: "11%", filterable: true, value: row => row.model_name },
    { key: "task_id", label: "Task", width: "14%", filterable: true, value: row => row.task_id },
    { key: "status", label: "Result", filterable: true, value: row => row.status, html: row => `<span class="stamp ${statusClass(row.status)}">${esc(row.status)}</span>` },
    { key: "score", label: "Score", type: "number", numeric: true, sortable: true, value: row => row.score, format: fmtScore },
    { key: "total_tokens", label: "Tokens", type: "number", numeric: true, sortable: true, value: row => row.total_tokens, format: fmtNum },
    { key: "cost_usd", label: "Cost", type: "number", numeric: true, sortable: true, value: row => row.cost_usd, format: fmtCost },
    { key: "duration_ms", label: "Duration", type: "number", numeric: true, sortable: true, value: row => row.duration_ms, format: fmtMs },
    { key: "cell_root_relative", label: "Cell Root", width: "22%", filterable: true, value: row => row.cell_root_relative || "-", html: row => `<code>${esc(row.cell_root_relative || "-")}</code>` }
  ];
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
function renderInteractiveTable(tableKey, columns, rows, label) {
  const controls = tableControls(tableKey);
  const filtered = applyTableControls(tableKey, columns, rows);
  const colgroup = columns.map(column => `<col ${column.width ? `style="width:${esc(column.width)}"` : ""}>`).join("");
  const headers = columns.map(column => {
    const active = controls.sort === column.key;
    const mark = active ? (controls.direction === "desc" ? "&#9660;" : "&#9650;") : "&#8597;";
    if (!column.sortable) {
      return `<th class="${column.numeric ? "num" : ""}"><span class="static-head">${esc(column.label)}</span></th>`;
    }
    return `<th class="${column.numeric ? "num" : ""}"><button class="sort-button ${active ? `active ${controls.direction === "desc" ? "sort-desc" : "sort-asc"}` : ""}" type="button" data-table-sort="${esc(tableKey)}" data-column="${esc(column.key)}" aria-label="Sort ${esc(column.label)}"><span class="sort-label">${esc(column.label)}</span><span class="sort-mark">${mark}</span></button></th>`;
  }).join("");
  const filters = columns.map(column => {
    if (!column.filterable) return `<th class="${column.numeric ? "num" : ""}"><span class="filter-slot" aria-label="sort only"></span></th>`;
    return `<th class="${column.numeric ? "num" : ""}">${renderMultiFilter(tableKey, column, rows)}</th>`;
  }).join("");
  const body = filtered.length
    ? filtered.map(row => `<tr>${columns.map(column => renderDataCell(row, column)).join("")}</tr>`).join("")
    : `<tr><td class="table-empty" colspan="${columns.length}">No matching rows</td></tr>`;
  return `<div class="table-shell"><div class="table-meta">Showing ${filtered.length} of ${rows.length} ${esc(label || "rows")}</div><div class="table-wrap"><table class="data-table"><colgroup>${colgroup}</colgroup><thead><tr>${headers}</tr><tr class="table-filters">${filters}</tr></thead><tbody>${body}</tbody></table></div></div>`;
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
  return `<td class="${classes}">${html}</td>`;
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
  document.querySelectorAll("[data-detail-key]").forEach(details => {
    details.addEventListener("toggle", () => {
      state.openDetails[details.dataset.detailKey] = details.open;
    });
  });
}
function renderEvidence() {
  const view = state.view;
  $("evidence").innerHTML = `
    <section class="evidence-ledger" aria-labelledby="evidence-title">
      <div class="evidence-head">
        <div>
          <p class="eyebrow">report evidence</p>
          <h2 id="evidence-title">Evidence Ledger</h2>
        </div>
        <span class="chip">${esc(includeList(view))}</span>
      </div>
      <div class="evidence-grid">
        ${renderScoringEvidence(view.trials || [])}
        ${renderUsageEvidence(view.usage || [])}
        ${renderWarningsEvidence(view.warnings || [])}
        ${renderAnalysisEvidence(view.analysis || [])}
        ${renderArtifactsEvidence(view.artifacts || [])}
      </div>
    </section>
  `;
}
function evidenceSection(title, html) {
  return `<section class="evidence-section"><h3>${esc(title)}</h3>${html}</section>`;
}
function showTrialColumn() {
  return (state.view?.trials || []).length > 1;
}
function trialHeader() {
  return showTrialColumn() ? "<th>Trial</th>" : "";
}
function trialCell(row) {
  return showTrialColumn() ? `<td>${esc(row.trial_key)}</td>` : "";
}
function renderScoringEvidence(rows) {
  if (!rows.length) return "";
  return evidenceSection("Scoring", `<table><thead><tr>${trialHeader()}<th>Status</th><th>Score</th><th>Passed</th><th>Evaluator</th><th>Details</th></tr></thead><tbody>${rows.map(row => `<tr>${trialCell(row)}<td>${esc(row.status)}</td><td class="num">${fmtScore(row.score)}</td><td>${esc(String(!!row.score_passed))}</td><td>${esc(row.score_message || "-")}</td><td><pre>${esc(shortJson(row.score_details))}</pre></td></tr>`).join("")}</tbody></table>`);
}
function renderUsageEvidence(rows) {
  if (!rows.length) return "";
  return evidenceSection("Usage", `<table><thead><tr>${trialHeader()}<th>Input</th><th>Output</th><th>Cache Read</th><th>Total</th><th>Cost</th></tr></thead><tbody>${rows.map(row => `<tr>${trialCell(row)}<td class="num">${fmtNum(row.input_tokens)}</td><td class="num">${fmtNum(row.output_tokens)}</td><td class="num">${fmtNum(row.cache_read_tokens)}</td><td class="num">${fmtNum(row.total_tokens)}</td><td class="num">${fmtCost(row.cost_usd)}</td></tr>`).join("")}</tbody></table>`);
}
function renderWarningsEvidence(rows) {
  if (!rows.length) return "";
  return evidenceSection("Warnings", `<table><thead><tr>${trialHeader()}<th>Warning</th></tr></thead><tbody>${rows.map(row => `<tr>${trialCell(row)}<td>${esc(row.warning)}</td></tr>`).join("")}</tbody></table>`);
}
function renderArtifactsEvidence(indexes) {
  if (!indexes.length) return "";
  const rows = indexes.flatMap(index => (index.paths || []).map(path => `<tr>${trialCell(index)}<td><code>${esc(path)}</code></td></tr>`)).join("");
  return evidenceSection("Artifacts", `<table><thead><tr>${trialHeader()}<th>Absolute Path</th></tr></thead><tbody>${rows}</tbody></table>`);
}
function renderAnalysisEvidence(rows) {
  if (!rows.length) return "";
  return evidenceSection("Analysis", `<table><thead><tr>${trialHeader()}<th>Status</th><th>Summary</th></tr></thead><tbody>${rows.map(row => `<tr>${trialCell(row)}<td>${esc(row.status)}</td><td><pre>${esc(row.summary || row.error || "-")}</pre></td></tr>`).join("")}</tbody></table>`);
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
