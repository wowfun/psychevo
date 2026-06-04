const $ = id => document.getElementById(id);
const esc = value => String(value ?? "").replace(/[&<>"]/g, ch => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[ch]));
const lower = value => String(value || "").toLowerCase();
const I18N = JSON.parse($("peval-py-i18n").textContent || "{}");
const TOKEN_ESTIMATES = JSON.parse($("peval-py-token-estimates").textContent || "{}");
function t(key, fallback) { return Object.prototype.hasOwnProperty.call(I18N, key) ? I18N[key] : (fallback ?? key); }
function statusLabel(value) {
  const raw = String(value || "-");
  return t(`status.${lower(raw)}`, raw);
}
const fmtNum = value => value === null || value === undefined ? "-" : Number(value).toLocaleString();
function fmtMs(value) {
  if (value === null || value === undefined || Number.isNaN(Number(value))) return "-";
  const seconds = Math.max(0, Number(value) / 1000);
  return seconds >= 60 ? `${Math.floor(seconds / 60)}m${(seconds % 60).toFixed(1)}s` : `${seconds.toFixed(1)}s`;
}
function fmtDate(value) { return value ? new Date(Number(value)).toLocaleString() : "-"; }
function fmtCost(value) { return hasMetricValue(value) ? `$${Number(value).toFixed(4)}` : "-"; }
function fmtScore(value) { return hasMetricValue(value) ? Number(value).toLocaleString() : "-"; }
function hasMetricValue(value) { return value !== null && value !== undefined && value !== "" && !Number.isNaN(Number(value)); }
function data() { return JSON.parse($("peval-py-data").textContent || "{}"); }
const state = { view: null, metricMode: "duration", metricInitialized: false, selectedTrial: null, tables: { leaderboard: { sort: null, direction: "asc" } } };
const METRICS = [
  { key: "duration", labelKey: "duration", fallback: "Duration" },
  { key: "tokens", labelKey: "tokens", fallback: "Tokens" },
  { key: "tools", labelKey: "tool_calls", fallback: "Tool Calls" },
  { key: "turns", labelKey: "turns", fallback: "Turns" }
];
function reportRows() {
  return state.view?.comparison?.leaderboard?.entries || state.view?.comparison?.session_table?.rows || [];
}
function selectedKey() {
  return state.selectedTrial || state.view?.comparison?.selected_trial_key || state.view?.trajectory_meta?.[0]?.trial_key || null;
}
function selectedIndex() {
  const key = selectedKey();
  const metas = state.view?.trajectory_meta || [];
  const index = metas.findIndex(meta => meta.trial_key === key);
  return index >= 0 ? index : 0;
}
function trajectoryFor(trialKey) {
  const metas = state.view?.trajectory_meta || [];
  const index = metas.findIndex(meta => meta.trial_key === trialKey);
  return (state.view?.trajectory || [])[index >= 0 ? index : selectedIndex()] || { steps: [] };
}
function metaFor(trialKey) {
  return (state.view?.trajectory_meta || []).find(meta => meta.trial_key === trialKey) || (state.view?.trajectory_meta || [])[selectedIndex()] || { steps: [] };
}
function finalMetricsFor(trialKey) { return trajectoryFor(trialKey)?.final_metrics || {}; }
function stepMeta(meta, stepId) { return (meta.steps || []).find(item => item.step_id === stepId) || {}; }
function render(view) {
  state.view = view;
  if (view.comparison?.default_metric && !state.metricInitialized) {
    state.metricMode = view.comparison.default_metric === "status" ? "duration" : view.comparison.default_metric;
    state.metricInitialized = true;
  }
  if (!state.selectedTrial) {
    const firstFailed = reportRows().find(row => lower(row.status) !== "passed");
    state.selectedTrial = (firstFailed || reportRows()[0])?.trial_key || view.trajectory_meta?.[0]?.trial_key || null;
  }
  renderReportNotes(view.annotations?.report_notes || []);
  renderComparison();
  renderTrace();
}
function renderReportNotes(notes) {
  $("report-notes").innerHTML = notes.length ? `<div class="report-note-list">${notes.map(note => `<article class="report-note"><strong>${esc(note.label || t("report_note", "Report note"))}</strong><div class="note-body">${renderMarkdown(note.markdown || "")}</div></article>`).join("")}</div>` : "";
}
function renderComparison() {
  const rows = reportRows();
  if (!rows.length) {
    $("comparison").innerHTML = "";
    return;
  }
  $("comparison").innerHTML = `
    <section class="panel" aria-labelledby="heatmap-title">
      <div class="panel-head"><div><h2 id="heatmap-title">${esc(t("visible_heatmap", "Visible Heatmap"))}</h2><p class="copy">${esc(t("visible_heatmap_copy", "Hue follows outcome. Shade follows the selected metric across visible sessions."))}</p></div><div class="metric-controls"><div class="segmented" id="metric-buttons"></div></div></div>
      <div class="visible-grid" id="visible-heatmap"></div>
    </section>
    <section class="leaderboard panel" aria-labelledby="leaderboard-title" id="leaderboard"></section>
  `;
  renderMetricControls();
  renderVisibleHeatmap();
  renderLeaderboard();
}
function renderMetricControls() {
  const target = $("metric-buttons");
  if (!target) return;
  target.innerHTML = METRICS.map(metric => `<button class="metric-button ${metric.key === state.metricMode ? "active" : ""}" type="button" data-metric="${esc(metric.key)}">${esc(t(metric.labelKey, metric.fallback))}</button>`).join("");
  document.querySelectorAll("[data-metric]").forEach(button => {
    button.addEventListener("click", () => {
      state.metricMode = button.dataset.metric;
      renderMetricControls();
      renderVisibleHeatmap();
    });
  });
}
function metricValue(row) {
  if (state.metricMode === "duration") return row.duration_ms;
  if (state.metricMode === "tokens") return row.tokens;
  if (state.metricMode === "tools") return row.total_tool_calls;
  if (state.metricMode === "turns") return row.turns;
  return row.duration_ms;
}
function formatMetric(value) {
  if (state.metricMode === "duration") return fmtMs(value);
  return hasMetricValue(value) ? fmtNum(value) : "-";
}
function shadeFor(row, rows) {
  const values = rows.map(metricValue).filter(hasMetricValue).map(Number);
  const value = metricValue(row);
  if (!values.length || !hasMetricValue(value)) return "shade-3 missing-metric";
  const min = Math.min(...values);
  const max = Math.max(...values);
  if (min === max) return "shade-2";
  const bucket = Math.max(0, Math.min(4, Math.round(((Number(value) - min) / (max - min)) * 4)));
  return `shade-${bucket}`;
}
function renderVisibleHeatmap() {
  const rows = reportRows();
  const target = $("visible-heatmap");
  if (!target) return;
  target.innerHTML = rows.map(row => {
    const status = lower(row.status || "passed");
    const session = row.session_id || row.trial_key || "-";
    const adapter = row.adapter || "-";
    const model = row.model || "-";
    return `<div class="session-axis"><strong>${esc(session)}</strong><span>${esc(adapter)} ${esc(model)}</span></div><button class="cell ${status} ${shadeFor(row, rows)} ${row.trial_key === selectedKey() ? "selected" : ""}" type="button" data-trial-key="${esc(row.trial_key)}"><strong>${esc(formatMetric(metricValue(row)))}</strong><span>${esc(statusLabel(row.status))} / ${esc(session)}<br>${esc(adapter)} ${esc(model)}</span></button>`;
  }).join("");
  bindTrialSelection();
}
function notesFor(trialKey) {
  return (state.view?.annotations?.notes || []).filter(note => note.trial_key === trialKey);
}
function notesPlainText(notes) {
  return notes.map(note => String(note.markdown || "").trim()).filter(Boolean).join("\\n\\n");
}
function noteSnippetFor(trialKey) {
  const text = notesPlainText(notesFor(trialKey)).replace(/\\s+/g, " ").trim();
  if (!text) return "-";
  return text.length > 96 ? `${text.slice(0, 96)}...` : text;
}
function renderNotesCell(trialKey) {
  const summary = noteSnippetFor(trialKey);
  return summary === "-" ? `<span class="muted">-</span>` : `<span class="note-snippet">${esc(summary)}</span>`;
}
function leaderboardColumns() {
  return [
    { key: "session_id", label: t("session", "Session"), width: "180px", value: row => row.session_id || row.trial_key },
    { key: "adapter", label: t("adapter", "Adapter"), width: "120px", value: row => row.adapter || "-" },
    { key: "model", label: t("model", "Model"), width: "150px", value: row => row.model || "-" },
    { key: "status", label: t("result", "Result"), width: "104px", value: row => row.status || "-", html: row => `<span class="stamp ${lower(row.status || "passed")}">${esc(statusLabel(row.status))}</span>` },
    { key: "duration_ms", label: t("duration", "Duration"), width: "104px", type: "number", numeric: true, sortable: true, value: row => row.duration_ms, format: fmtMs },
    { key: "turns", label: t("turns", "Turns"), width: "82px", type: "number", numeric: true, sortable: true, value: row => row.turns, format: fmtNum },
    { key: "total_tool_calls", label: t("tool_calls", "Tool Calls"), width: "106px", type: "number", numeric: true, sortable: true, value: row => row.total_tool_calls, format: value => hasMetricValue(value) ? fmtNum(value) : "-" },
    { key: "tokens", label: t("tokens", "Tokens"), width: "100px", type: "number", numeric: true, sortable: true, value: row => row.tokens, format: fmtNum },
    { key: "cost_usd", label: t("cost", "Cost"), width: "92px", type: "number", numeric: true, sortable: true, value: row => row.cost_usd, format: fmtCost },
    { key: "notes", label: t("notes", "Notes"), width: "220px", value: row => noteSnippetFor(row.trial_key), html: row => renderNotesCell(row.trial_key), cellTitle: row => notesPlainText(notesFor(row.trial_key)) }
  ];
}
function renderLeaderboard() {
  const target = $("leaderboard");
  if (!target) return;
  const rows = applyTableControls(reportRows());
  const columns = leaderboardColumns();
  target.innerHTML = `
    <div class="panel-head"><div><h2 id="leaderboard-title">${esc(t("leaderboard", "Leaderboard"))}</h2><p class="copy">${esc(t("leaderboard_copy", "Each row is one visible session-as-Trial. Numeric columns sort; rows update the selected Trial."))}</p></div></div>
    ${renderInteractiveTable(columns, rows)}
  `;
  bindLeaderboardControls();
}
function tableControls() {
  state.tables.leaderboard ||= { sort: null, direction: "asc" };
  return state.tables.leaderboard;
}
function compareTableValues(left, right, type, direction) {
  const leftMissing = left === null || left === undefined || left === "" || (type === "number" && Number.isNaN(Number(left)));
  const rightMissing = right === null || right === undefined || right === "" || (type === "number" && Number.isNaN(Number(right)));
  if (leftMissing || rightMissing) return leftMissing === rightMissing ? 0 : leftMissing ? 1 : -1;
  const delta = type === "number" ? Number(left) - Number(right) : String(left).localeCompare(String(right), undefined, { numeric: true, sensitivity: "base" });
  return direction === "desc" ? -delta : delta;
}
function applyTableControls(rows) {
  const controls = tableControls();
  const columns = leaderboardColumns();
  const sortColumn = columns.find(column => column.key === controls.sort && column.sortable);
  const out = [...rows];
  if (sortColumn) out.sort((left, right) => compareTableValues(sortColumn.value(left), sortColumn.value(right), sortColumn.type, controls.direction));
  return out;
}
function tableText(row, column) {
  const raw = column.value(row);
  return column.format ? column.format(raw, row) : (raw ?? "-");
}
function renderInteractiveTable(columns, rows) {
  const controls = tableControls();
  const colgroup = columns.map(column => `<col ${column.width ? `style="width:${esc(column.width)}"` : ""}>`).join("");
  const headers = columns.map(column => {
    const active = controls.sort === column.key;
    const mark = active ? (controls.direction === "desc" ? "&#9660;" : "&#9650;") : "&#8597;";
    if (!column.sortable) return `<th class="${column.numeric ? "num" : ""}" title="${esc(column.label)}"><span class="static-head">${esc(column.label)}</span></th>`;
    return `<th class="${column.numeric ? "num" : ""}" title="${esc(column.label)}"><button class="sort-button ${active ? "active" : ""}" type="button" data-table-sort="${esc(column.key)}" aria-label="${esc(t("sort", "Sort"))} ${esc(column.label)}"><span class="sort-label">${esc(column.label)}</span><span class="sort-mark">${mark}</span></button></th>`;
  }).join("");
  const body = rows.length ? rows.map(row => renderTableRow(row, columns)).join("") : `<tr><td class="table-empty" colspan="${columns.length}">${esc(t("no_matching_rows", "No matching rows"))}</td></tr>`;
  return `<div class="table-shell"><div class="table-wrap"><table class="data-table"><colgroup>${colgroup}</colgroup><thead><tr>${headers}</tr></thead><tbody>${body}</tbody></table></div></div>`;
}
function renderTableRow(row, columns) {
  const selected = row.trial_key === selectedKey();
  return `<tr class="clickable-row ${selected ? "selected-row" : ""}" data-trial-key="${esc(row.trial_key)}" title="${esc(row.trial_key)}">${columns.map(column => renderDataCell(row, column)).join("")}</tr>`;
}
function renderDataCell(row, column) {
  const classes = [column.numeric ? "num" : "", column.className || ""].filter(Boolean).join(" ");
  const html = column.html ? column.html(row) : esc(tableText(row, column));
  const title = column.cellTitle ? column.cellTitle(row) : "";
  return `<td class="${classes}" ${title ? `title="${esc(title)}"` : ""}>${html}</td>`;
}
function bindLeaderboardControls() {
  document.querySelectorAll("[data-table-sort]").forEach(button => {
    button.addEventListener("click", event => {
      event.stopPropagation();
      const controls = tableControls();
      if (controls.sort === button.dataset.tableSort) {
        controls.direction = controls.direction === "asc" ? "desc" : "asc";
      } else {
        controls.sort = button.dataset.tableSort;
        controls.direction = "asc";
      }
      renderLeaderboard();
    });
  });
  bindTrialSelection();
}
function bindTrialSelection() {
  document.querySelectorAll("[data-trial-key]").forEach(node => {
    node.addEventListener("click", () => {
      state.selectedTrial = node.getAttribute("data-trial-key");
      renderVisibleHeatmap();
      renderLeaderboard();
      renderTrace();
    });
  });
}
function renderTrace() {
  const trial = metaFor(selectedKey());
  state.selectedTrial = trial.trial_key;
  const trajectory = trajectoryFor(trial.trial_key);
  const metrics = finalMetricsFor(trial.trial_key);
  const timingStats = stepTimingStats(trial);
  const status = lower(trial.status || "passed");
  const agentName = trajectory?.agent?.name || "-";
  const model = trajectory?.agent?.model_name || "-";
  $("trace").innerHTML = `
    <div class="trace-head"><div><p class="eyebrow">${esc(t("selected_trial_trajectory", "selected trial trajectory"))}</p><h2 id="trace-title" class="trace-title"><span>${esc(t("selected_session_label", "session"))}</span><code>${esc(trial.trial_key || "-")}</code></h2></div><span class="status ${status}">${esc(statusLabel(status))}</span></div>
    <h3>${esc(t("run", "Run"))}</h3>
    ${infoGrid([
      [t("trial", "Trial"), trial.trial_key || "-"],
      [t("variant", "Variant"), trial.variant_label || "-"],
      [t("session", "Session"), trajectory?.session_id || "-"],
      [t("agent_model", "Agent / model"), `${agentName} / ${model}`],
      [t("time", "Time"), `${fmtDate(trial.started_at_ms)} -> ${fmtDate(trial.finished_at_ms)}`],
      [t("wall_duration", "Wall duration"), fmtMs(trialWallDurationMs(trial))],
      [t("steps_events", "Steps/events"), `${(trajectory?.steps || []).length}/${trial.total_events ?? "-"}`],
      [t("system_exposed", "System exposed"), systemExposed(trajectory) ? t("yes", "yes") : t("no", "no")],
      [t("reasoning_exposed", "Reasoning exposed"), reasoningExposed(trajectory) ? t("yes", "yes") : t("no", "no")]
    ])}
    <h3>${esc(t("result", "Result"))}</h3>
    ${infoGrid([
      [t("status", "Status"), statusLabel(trial.status || "-")],
      [t("score", "Score"), fmtScore(trial.score)],
      [t("evaluator", "Evaluator"), trial.score_message || "-"],
      [t("tokens", "Tokens"), fmtNum(tokenTotal(metrics))],
      [t("turns", "Turns"), metrics.total_turns ?? "-"],
      [t("tool_success_total", "Tool success / total"), toolCallRatio(metrics.total_tool_calls ?? 0, metrics.total_tool_errors ?? 0)],
      [t("cost", "Cost"), fmtCost(metrics.total_cost_usd)]
    ])}
    ${renderSelectedNotes(trial.trial_key)}
    ${renderSelectedEvidence(trajectory, trial)}
    ${renderStepsHeader(trajectory)}
    <div class="step-list" id="step-list">${(trajectory?.steps || []).map(step => renderStep(step, trial, timingStats)).join("")}</div>
  `;
  bindStepToggle();
}
function infoGrid(items) {
  return `<div class="info-grid">${items.map(([label, value]) => `<div><span>${esc(label)}</span><strong>${esc(value)}</strong></div>`).join("")}</div>`;
}
function trialWallDurationMs(trial) {
  if (hasMetricValue(trial?.started_at_ms) && hasMetricValue(trial?.finished_at_ms)) return Math.max(0, Number(trial.finished_at_ms) - Number(trial.started_at_ms));
  return trial?.duration_ms;
}
function stepTimingStats(meta) {
  const steps = meta?.steps || [];
  const stepDurations = steps.map(step => step?.duration_ms);
  const toolDurations = steps.flatMap(step => (step?.tool_calls || []).map(tool => tool?.execution_duration_ms));
  const elapsedValues = steps.map(step => step?.elapsed_ms);
  const wallDuration = trialWallDurationMs(meta);
  return {
    maxStepDurationMs: maxPositiveMetric(stepDurations),
    maxToolExecutionMs: maxPositiveMetric(toolDurations),
    elapsedMaxMs: positiveMetric(wallDuration) ? Number(wallDuration) : maxPositiveMetric(elapsedValues)
  };
}
function positiveMetric(value) { return hasMetricValue(value) && Number(value) > 0; }
function maxPositiveMetric(values) {
  const numeric = values.filter(positiveMetric).map(Number);
  return numeric.length ? Math.max(...numeric) : null;
}
function timingRatio(value, max) {
  if (!positiveMetric(value) || !positiveMetric(max)) return null;
  return Math.max(0, Math.min(1, Number(value) / Number(max)));
}
function timeGradientStyle(ratio) {
  if (ratio === null || ratio === undefined) return "";
  return ` style="--time-pct: ${esc((ratio * 100).toFixed(1))}%"`;
}
function timeGradientClass(ratio) { return ratio === null || ratio === undefined ? "" : "time-gradient"; }
function timeTitle(label, value, ratio, basis) {
  const text = `${label} ${fmtMs(value)}`;
  return ratio === null || ratio === undefined ? text : `${text}; ${Math.round(ratio * 100)}% of ${basis}`;
}
function systemExposed(trajectory) { return (trajectory?.steps || []).some(step => step.source === "system"); }
function reasoningExposed(trajectory) { return (trajectory?.steps || []).some(step => step.reasoning_content); }
function tokenTotal(metrics) {
  const direct = metrics.usage?.total_tokens;
  if (hasMetricValue(direct)) return Number(direct);
  const values = [metrics.total_prompt_tokens, metrics.total_completion_tokens, metrics.total_cached_tokens].filter(hasMetricValue).map(Number);
  return values.length ? values.reduce((sum, value) => sum + value, 0) : null;
}
function renderSelectedNotes(trialKey) {
  const notes = notesFor(trialKey);
  const body = notes.length ? `<div class="note-list">${notes.map(renderManualNote).join("")}</div>` : `<p class="copy">${esc(t("no_notes", "No notes."))}</p>`;
  return `<section class="selected-extra"><h3>${esc(t("notes", "Notes"))}</h3>${body}</section>`;
}
function renderSelectedEvidence(trajectory, meta) {
  const blocks = [renderSelectedUsage(trajectory), renderSelectedWarnings(meta), renderSelectedSource(meta)].filter(Boolean);
  return blocks.length ? `<section class="selected-extra selected-evidence"><h3>${esc(t("evidence", "Evidence"))}</h3><div class="selected-evidence-list">${blocks.join("")}</div></section>` : "";
}
function renderSelectedUsage(trajectory) {
  const metrics = trajectory?.final_metrics || {};
  const usage = metrics.usage || {};
  const accounting = metrics.accounting || {};
  if (!metrics.usage && !metrics.accounting && !hasMetricValue(metrics.total_prompt_tokens) && !hasMetricValue(metrics.total_completion_tokens) && !hasMetricValue(metrics.total_cached_tokens)) return "";
  return `<article class="selected-evidence-card"><h4>${esc(t("usage_breakdown", "Usage Breakdown"))}</h4>${infoGrid([
    [t("input", "Input"), fmtNum(usage.input_tokens ?? metrics.total_prompt_tokens)],
    [t("output", "Output"), fmtNum(usage.output_tokens ?? metrics.total_completion_tokens)],
    [t("cache_read", "Cache read"), fmtNum(usage.cache_read_tokens ?? metrics.total_cached_tokens)],
    [t("cache_write", "Cache write"), fmtNum(usage.cache_write_tokens)],
    [t("reasoning", "Reasoning"), fmtNum(usage.reasoning_tokens)],
    [t("billable_input", "Billable input"), fmtNum(accounting.billable_input_tokens)],
    [t("billable_output", "Billable output"), fmtNum(accounting.billable_output_tokens)],
    [t("pricing", "Pricing"), accounting.pricing_source || "-"]
  ])}</article>`;
}
function renderSelectedWarnings(meta) {
  const warnings = meta.warnings || [];
  if (!warnings.length) return "";
  return `<article class="selected-evidence-card"><h4>${esc(t("warnings", "Warnings"))}</h4><ul class="evidence-list">${warnings.map(warning => `<li>${esc(warning)}</li>`).join("")}</ul></article>`;
}
function renderSelectedSource(meta) {
  const path = meta.data_ref?.relative_path;
  return path ? `<article class="selected-evidence-card"><h4>${esc(t("input_source", "Input Source"))}</h4><code>${esc(path)}</code></article>` : "";
}
function renderStepsHeader(trajectory) {
  const count = (trajectory?.steps || []).length;
  return `<div class="steps-head"><h3>Steps (${count})</h3><button class="step-toggle-button" type="button" data-step-action="toggle" ${count ? "" : "disabled"}>Expand all</button></div>`;
}
function valuePreview(value) {
  if (value === null || value === undefined) return "";
  if (typeof value === "string") return value;
  return JSON.stringify(value, null, 2);
}
function renderStep(step, meta, timingStats) {
  const sm = stepMeta(meta, step.step_id);
  const preview = valuePreview(step.message).trim() || "(No Message)";
  return `<details class="step" data-step="${esc(step.step_id)}"><summary><div class="step-row"><span class="step-id">#${esc(step.step_id)}</span><span class="role ${esc(step.source)}">${esc(step.source)}</span><span class="preview">${esc(preview)}</span></div><div class="rail">${renderStepRail(step, sm, meta?.trial_key, timingStats)}</div></summary><div class="step-body">${renderBlocks(step, sm, timingStats)}</div></details>`;
}
function renderBlocks(step, meta, timingStats) {
  let html = "";
  if (step.reasoning_content) html += block("Reasoning", step.reasoning_content, "reasoning-block");
  const message = valuePreview(step.message);
  if (message.trim()) html += block(step.source === "system" ? "System Prompt" : "Message", message, "message-block");
  (step.tool_calls || []).forEach(tool => {
    const toolMeta = toolMetaFor(meta, tool.tool_call_id);
    const ratio = timingRatio(toolMeta?.execution_duration_ms, timingStats?.maxToolExecutionMs);
    html += `<div class="block tool-block"><h4>Tool Calls</h4><p>${renderToolNameChip(tool, toolMeta, "", ratio)} <span class="muted">ID: ${esc(tool.tool_call_id || "-")}${toolMeta?.status ? ` / ${esc(toolMeta.status)}` : ""}${renderToolTiming(toolMeta)}</span></p><pre>${esc(valuePreview(tool.arguments || {}))}</pre></div>`;
  });
  ((step.observation && step.observation.results) || []).forEach(observation => {
    const observationMeta = observationMetaFor(meta, observation.source_call_id);
    html += `<div class="block observation-block"><h4 class="${observationMeta?.tool_error ? "danger" : ""}">Observations</h4><p class="muted">Result for: ${esc(observation.source_call_id || "-")}${observationMeta?.status ? ` / ${esc(observationMeta.status)}` : ""}</p><pre>${esc(valuePreview(observation.content))}</pre></div>`;
  });
  return html || `<p class="copy">No visible content.</p>`;
}
function block(title, content, cls) { return `<div class="block ${cls}"><h4>${esc(title)}</h4><pre>${esc(content)}</pre></div>`; }
function fmtRailTokens(value) {
  if (!hasMetricValue(value)) return "-";
  const number = Number(value);
  return Math.abs(number) >= 1000 ? `${(number / 1000).toFixed(1)}k` : fmtNum(number);
}
function toolExecutionText(toolMeta) { return hasMetricValue(toolMeta?.execution_duration_ms) ? fmtMs(toolMeta.execution_duration_ms) : ""; }
function toolFailed(toolMeta) {
  const status = lower(toolMeta?.status);
  return status === "error" || status === "failed";
}
function renderToolNameChip(tool, toolMeta, extraClass = "", timingFill = null) {
  const name = tool.function_name || toolMeta?.title || "tool";
  const exec = toolExecutionText(toolMeta);
  const titleText = exec ? timeTitle("tool exec", toolMeta?.execution_duration_ms, timingFill, "slowest tool") : "";
  const title = titleText ? ` title="${esc(titleText)}"` : "";
  const execHtml = exec ? ` <span class="tool-exec-inline">${esc(exec)}</span>` : "";
  const classes = ["chip", "tool-name-chip", extraClass, toolFailed(toolMeta) ? "tool-error-chip" : "", timeGradientClass(timingFill)].filter(Boolean).join(" ");
  return `<span class="${esc(classes)}"${timeGradientStyle(timingFill)}${title}>${esc(name)}${execHtml}</span>`;
}
function renderToolTiming(toolMeta) {
  const parts = [];
  if (hasMetricValue(toolMeta?.generation_duration_ms)) parts.push(`generation ${fmtMs(toolMeta.generation_duration_ms)}`);
  return parts.length ? ` / ${parts.map(esc).join(" / ")}` : "";
}
function stepToolChips(step, meta, timingStats) {
  const chips = [];
  (step.tool_calls || []).forEach(tool => {
    const toolMeta = toolMetaFor(meta, tool.tool_call_id);
    const name = String(tool.function_name || toolMeta?.title || "").trim();
    const ratio = timingRatio(toolMeta?.execution_duration_ms, timingStats?.maxToolExecutionMs);
    if (name) chips.push(renderToolNameChip(tool, toolMeta, "rail-chip-tool-list", ratio));
  });
  return chips;
}
function renderStepRail(step, meta, trialKey, timingStats) {
  const summaryItems = [];
  const toolCalls = (step.tool_calls || []).length;
  const observations = (step.observation?.results || []).length;
  const toolErrors = meta?.tool_error ? 1 : 0;
  if (toolCalls || toolErrors) summaryItems.push(`<span class="rail-chip rail-chip-tools">${esc(toolCallRatio(toolCalls, toolErrors))} tools</span>`);
  else if (observations) summaryItems.push(`<span class="rail-chip rail-chip-tools">${esc(observations)} observations</span>`);
  const tokenInfo = stepTokenInfo(step, trialKey);
  if (tokenInfo) {
    const classes = ["rail-chip", "rail-chip-tokens", tokenInfo.estimated ? "rail-chip-estimated" : ""].filter(Boolean).join(" ");
    const prefix = tokenInfo.estimated ? "≈" : "";
    const title = tokenInfo.estimated ? `estimated tokens (${tokenInfo.method}; from visible step text): ${fmtNum(tokenInfo.tokens)}` : `${fmtNum(tokenInfo.tokens)} tokens`;
    summaryItems.push(`<span class="${esc(classes)}" title="${esc(title)}">${esc(`${prefix}${fmtRailTokens(tokenInfo.tokens)} tok`)}</span>`);
  }
  const stepRatio = timingRatio(meta?.duration_ms, timingStats?.maxStepDurationMs);
  const elapsedRatio = timingRatio(meta?.elapsed_ms, timingStats?.elapsedMaxMs);
  const stepClasses = ["rail-chip", "rail-chip-step-time", timeGradientClass(stepRatio)].filter(Boolean).join(" ");
  const elapsedClasses = ["rail-chip", "rail-chip-elapsed-time", timeGradientClass(elapsedRatio)].filter(Boolean).join(" ");
  const time = `<div class="rail-time"><span class="${esc(stepClasses)}"${timeGradientStyle(stepRatio)} title="${esc(timeTitle("step span", meta?.duration_ms, stepRatio, "slowest step"))}">step ${esc(fmtMs(meta?.duration_ms))}</span><span class="${esc(elapsedClasses)}"${timeGradientStyle(elapsedRatio)} title="${esc(timeTitle("elapsed", meta?.elapsed_ms, elapsedRatio, "trajectory"))}">elapsed ${esc(fmtMs(meta?.elapsed_ms))}</span></div>`;
  const summary = `<div class="rail-summary">${summaryItems.join("")}${time}</div>`;
  const toolChips = stepToolChips(step, meta, timingStats);
  return `${summary}${toolChips.length ? `<div class="rail-tool-row">${toolChips.join("")}</div>` : ""}`;
}
function toolCallRatio(total, errors) {
  const callTotal = Math.max(0, Number(total || 0));
  const errorTotal = Math.max(0, Number(errors || 0));
  return `${Math.max(0, callTotal - errorTotal)}/${callTotal}`;
}
function toolMetaFor(meta, toolCallId) { return (meta?.tool_calls || []).find(item => item.tool_call_id === toolCallId) || null; }
function observationMetaFor(meta, sourceCallId) { return (meta?.observations || []).find(item => item.source_call_id === sourceCallId) || null; }
function stepTokenInfo(step, trialKey) {
  const exact = stepTokenTotal(step);
  if (exact !== null && exact !== undefined) return { tokens: exact, estimated: false, method: "exact" };
  const estimate = stepTokenEstimate(trialKey, step.step_id);
  if (estimate && hasMetricValue(estimate.tokens)) return { tokens: Number(estimate.tokens), estimated: true, method: estimate.method || "estimated" };
  return null;
}
function stepTokenEstimate(trialKey, stepId) {
  if (!trialKey) return null;
  return TOKEN_ESTIMATES?.[trialKey]?.[String(stepId)] || null;
}
function stepTokenTotal(step) {
  const metrics = step.metrics || {};
  const values = [metrics.prompt_tokens, metrics.completion_tokens, metrics.cached_tokens, metrics.usage?.total_tokens].filter(hasMetricValue).map(Number);
  return values.length ? values.reduce((sum, value) => sum + value, 0) : null;
}
function bindStepToggle() {
  const button = document.querySelector("[data-step-action='toggle']");
  if (!button) return;
  const rows = () => Array.from(document.querySelectorAll("#step-list .step"));
  function refresh() {
    const allOpen = rows().length > 0 && rows().every(row => row.open);
    button.textContent = allOpen ? "Collapse all" : "Expand all";
    button.setAttribute("aria-pressed", allOpen ? "true" : "false");
  }
  rows().forEach(row => row.addEventListener("toggle", refresh));
  button.addEventListener("click", () => {
    const shouldOpen = !rows().every(row => row.open);
    rows().forEach(row => { row.open = shouldOpen; });
    refresh();
  });
  refresh();
}
function renderManualNote(row) {
  return `<article class="manual-note"><div class="note-body">${renderMarkdown(row.markdown || "")}</div></article>`;
}
function renderMarkdown(markdown) {
  const lines = String(markdown ?? "").split(/\\r?\\n/);
  const out = [];
  let paragraph = [];
  let list = [];
  let code = [];
  let inCode = false;
  function flushParagraph() {
    if (paragraph.length) {
      out.push(`<p>${inlineMarkdown(paragraph.join(" "))}</p>`);
      paragraph = [];
    }
  }
  function flushList() {
    if (list.length) {
      out.push(`<ul>${list.map(item => `<li>${inlineMarkdown(item)}</li>`).join("")}</ul>`);
      list = [];
    }
  }
  function flushCode() {
    if (code.length) {
      out.push(`<pre class="note-code">${esc(code.join("\\n"))}</pre>`);
      code = [];
    }
  }
  for (const line of lines) {
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
      code.push(line);
      continue;
    }
    if (!line.trim()) {
      flushParagraph();
      flushList();
      continue;
    }
    const heading = line.match(/^#{1,4}\\s+(.+)$/);
    if (heading) {
      flushParagraph();
      flushList();
      out.push(`<h4>${inlineMarkdown(heading[1])}</h4>`);
      continue;
    }
    const bullet = line.match(/^[-*]\\s+(.+)$/);
    if (bullet) {
      flushParagraph();
      list.push(bullet[1]);
      continue;
    }
    paragraph.push(line.trim());
  }
  flushParagraph();
  flushList();
  flushCode();
  return out.join("") || "<p></p>";
}
function inlineMarkdown(value) {
  return esc(value).replace(/`([^`]+)`/g, "<code>$1</code>");
}
render(data());
