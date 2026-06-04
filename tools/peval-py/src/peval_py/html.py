from __future__ import annotations

import json
from html import escape
from typing import Any


def render_html(report: dict[str, Any]) -> str:
    payload = HTML_TEMPLATE.replace("__TITLE__", escape("Agent Trajectory Report"))
    payload = payload.replace(
        "__DATA__",
        safe_json_for_script(json.dumps(report, ensure_ascii=False)),
    )
    return payload


HTML_TEMPLATE = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>__TITLE__</title>
<style>
:root{color-scheme:light;--canvas:#f5f3ee;--surface:#fffdf8;--surface-2:#ece7dd;--ink:#27231b;--muted:#706958;--rule:#d5cdbb;--focus:#315f8f;--pass:#2f8f5b;--fail:#ad3e32;--mono:ui-monospace,SFMono-Regular,Menlo,Consolas,"Liberation Mono",monospace;--sans:system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;--serif:Georgia,"Times New Roman",serif;--radius:8px}
*{box-sizing:border-box}body{margin:0;background:var(--canvas);color:var(--ink);font:14px/1.45 var(--sans)}.workspace{max-width:1180px;margin:0 auto;padding:24px;min-width:0}.topline{display:grid;grid-template-columns:minmax(0,1fr) auto;gap:18px;align-items:start;margin-bottom:18px}.eyebrow{margin:0 0 5px;color:var(--muted);font:700 11px/1 var(--mono);letter-spacing:.08em;text-transform:uppercase}h1{margin:0;font-size:34px;line-height:1.05}h2,h3{margin:0}.copy{margin:7px 0 0;color:var(--muted)}.trace-panel,.panel{min-width:0;border:1px solid var(--rule);border-radius:var(--radius);background:var(--surface);padding:18px;box-shadow:0 16px 38px rgba(51,44,27,.10)}.report-note-list,.panel-stack{display:grid;gap:14px;margin-bottom:18px}.report-note,.manual-note,.selected-evidence-card{border:1px solid var(--rule);border-radius:var(--radius);background:var(--surface);padding:12px}.report-note{border-left:4px solid var(--focus)}.report-note strong{display:block;margin:0 0 10px;font:700 18px/1.25 var(--sans)}.note-list,.selected-evidence-list{display:grid;gap:10px}.note-body{font-size:14px;line-height:1.6;overflow-wrap:anywhere}.note-body>*:first-child{margin-top:0}.note-body>*:last-child{margin-bottom:0}.note-body h4{margin:12px 0 6px;font:700 15px/1.2 var(--serif)}.note-body p{margin:8px 0}.note-body ul{margin:8px 0;padding-left:20px}.note-body code{font:13px/1.4 var(--mono);background:var(--surface-2);border-radius:4px;padding:1px 4px}.note-code{max-height:none;background:var(--surface-2);border-radius:var(--radius);padding:10px}.note-snippet{display:block;max-width:100%;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.muted{color:var(--muted)}.danger{color:var(--fail)}
.panel-head,.trace-head{display:grid;grid-template-columns:minmax(0,1fr) auto;gap:18px;align-items:start;margin-bottom:14px}.metric-controls{display:flex;align-items:center;gap:8px;justify-content:flex-end}.segmented{display:inline-flex;background:var(--surface-2);border-radius:999px;padding:4px;box-shadow:inset 0 0 0 1px rgba(51,44,27,.08)}.metric-button{border:0;min-height:32px;border-radius:999px;background:transparent;color:var(--muted);padding:0 11px;font:700 12px/1 var(--mono);white-space:nowrap}.metric-button.active{background:var(--surface);color:var(--ink);box-shadow:0 1px 3px rgba(49,42,25,.12)}.metric-button:hover{color:var(--ink)}
.visible-grid{display:grid;grid-template-columns:minmax(150px,220px) minmax(0,1fr);gap:8px;align-items:stretch}.session-axis{min-height:106px;border-right:1px solid var(--rule);padding:12px 12px 12px 0;display:flex;flex-direction:column;justify-content:center;color:var(--muted);font:12px/1.35 var(--mono);overflow-wrap:anywhere}.session-axis strong{display:block;margin-bottom:5px;color:var(--ink);font:700 14px/1.25 var(--mono)}.session-axis span{display:block}.cell{min-height:106px;width:100%;border:0;border-radius:var(--radius);padding:12px;text-align:left;color:#fff;box-shadow:inset 0 0 0 1px rgba(255,255,255,.22),0 1px 2px rgba(49,42,25,.12);cursor:pointer}.cell strong{display:block;font:700 30px/1 var(--mono);font-variant-numeric:tabular-nums}.cell span{display:block;margin-top:8px;color:rgba(255,255,255,.88);font:12px/1.35 var(--mono);overflow-wrap:anywhere}.cell.selected{box-shadow:inset 0 0 0 3px #fff,0 0 0 2px var(--focus),0 8px 18px rgba(49,42,25,.18)}.cell.missing-metric strong{opacity:.92}.cell.passed{background:#2f8f5b}.cell.failed{background:#ad3e32}.cell.shade-0{filter:saturate(.72) brightness(.94)}.cell.shade-1{filter:saturate(.86) brightness(.98)}.cell.shade-2{filter:saturate(1) brightness(1.02)}.cell.shade-3{filter:saturate(1.08) brightness(1.06)}.cell.shade-4{filter:saturate(1.16) brightness(1.12)}
.leaderboard{margin-top:18px}.table-shell{border:1px solid var(--rule);border-radius:var(--radius);background:color-mix(in oklch,var(--surface),var(--surface-2) 10%);overflow:hidden}.table-wrap{overflow-x:auto}.data-table{width:100%;border-collapse:collapse;table-layout:fixed;min-width:980px}th,td{padding:10px 12px;border-top:1px solid var(--rule);text-align:left;vertical-align:top;overflow-wrap:anywhere}th{color:var(--muted);font:700 12px/1.2 var(--mono);text-transform:uppercase;letter-spacing:.04em}.data-table thead th{padding:7px 8px;background:color-mix(in oklch,var(--surface),var(--surface-2) 38%)}.data-table tbody tr.clickable-row{cursor:pointer}.data-table tbody tr.clickable-row:hover{background:color-mix(in oklch,var(--surface),var(--surface-2) 34%)}.data-table tbody tr.selected-row{background:color-mix(in oklch,var(--focus),var(--surface) 90%);box-shadow:inset 3px 0 0 var(--focus)}td.num,th.num{text-align:right;font-variant-numeric:tabular-nums}.sort-button{width:100%;min-height:34px;border:1px solid transparent;border-radius:6px;background:transparent;color:var(--muted);display:flex;align-items:center;justify-content:space-between;gap:8px;padding:0 5px;font:700 12px/1.2 var(--mono);text-transform:uppercase;letter-spacing:.04em;white-space:nowrap}.sort-label{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.sort-button:hover{background:color-mix(in oklch,var(--surface),var(--surface-2) 44%)}.sort-button.active{color:var(--ink);background:var(--surface);border-color:color-mix(in oklch,var(--focus),transparent 58%);box-shadow:0 1px 3px rgba(49,42,25,.12)}.sort-mark{display:inline-flex;align-items:center;justify-content:center;width:22px;height:22px;flex:0 0 22px;border-radius:999px;background:color-mix(in oklch,var(--muted),transparent 86%);color:var(--muted);font-size:14px;line-height:1}.sort-button.active .sort-mark{background:var(--focus);color:#fff;font-size:13px}.static-head{min-height:34px;display:flex;align-items:center;padding:0 4px;color:var(--muted);font:700 12px/1.2 var(--mono);text-transform:uppercase;letter-spacing:.04em;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.stamp{display:inline-flex;align-items:center;min-height:24px;border-radius:999px;padding:0 9px;font:700 11px/1 var(--mono);text-transform:uppercase}.stamp.passed{background:#dceee4;color:var(--pass)}.stamp.failed{background:#f4ddd7;color:var(--fail)}.table-empty{color:var(--muted);text-align:center;font-family:var(--mono)}
.trace-title{display:flex;align-items:baseline;gap:10px;flex-wrap:wrap}.trace-title code{font:700 20px/1 var(--mono);overflow-wrap:anywhere}.info-grid{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:8px;margin:12px 0 18px}.info-grid div{min-width:0;border-top:1px solid var(--rule);padding-top:9px}.info-grid span{display:block;color:var(--muted);font:700 11px/1 var(--mono);letter-spacing:.05em;text-transform:uppercase}.info-grid strong{display:block;margin-top:6px;font:700 14px/1.3 var(--mono);overflow-wrap:anywhere;font-variant-numeric:tabular-nums}.status{display:inline-flex;align-items:center;min-height:26px;border-radius:999px;padding:0 10px;font:700 11px/1 var(--mono);text-transform:uppercase}.status.passed{background:#dceee4;color:var(--pass)}.status.failed{background:#f4ddd7;color:var(--fail)}.selected-extra{margin:14px 0}.steps-head{display:flex;align-items:center;justify-content:space-between;gap:12px;margin-top:18px;flex-wrap:wrap}.step-toggle-button{min-height:30px;min-width:104px;border:1px solid var(--rule);border-radius:999px;background:var(--surface);color:var(--ink);padding:0 11px;font:700 11px/1 var(--mono);text-transform:uppercase;box-shadow:0 4px 10px rgba(49,42,25,.10)}.step-toggle-button:hover:not(:disabled){border-color:var(--focus);background:var(--surface-2)}.step-list{display:grid;gap:8px;margin-top:12px}.step{border:1px solid var(--rule);border-radius:var(--radius);background:#faf8f2;overflow:hidden}.step[open]{background:var(--surface);box-shadow:0 8px 20px rgba(49,42,25,.08)}.step>summary{display:grid;grid-template-columns:minmax(0,1fr) minmax(220px,auto);gap:14px;align-items:center;min-height:64px;padding:12px;cursor:pointer;list-style:none}.step>summary::-webkit-details-marker{display:none}.step-row{display:grid;grid-template-columns:48px 74px minmax(0,1fr);gap:10px;align-items:center;min-width:0}.step-id,.role{font:700 11px/1 var(--mono);white-space:nowrap}.step-id{color:var(--muted)}.role{border-radius:999px;min-height:24px;display:inline-flex;align-items:center;justify-content:center;background:var(--surface-2);color:var(--muted);text-transform:uppercase}.role.system{color:#643d93;background:#eee3f4}.role.user{color:#225f91;background:#e1edf5}.role.agent{color:#216b42;background:#dfeee5}.preview{min-width:0;font-size:13px;line-height:1.35;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.rail{min-width:0;display:grid;gap:6px;justify-items:end;color:var(--muted);font:11px/1 var(--mono);white-space:nowrap}.rail-summary,.rail-tool-row,.rail-time{display:flex;align-items:center;justify-content:flex-end;gap:6px;min-width:0;flex-wrap:wrap}.rail-summary{gap:8px}.rail-tool-row:empty{display:none}.rail-chip{display:inline-flex;align-items:center;min-height:24px;border:1px solid var(--rule);border-radius:999px;padding:0 8px;background:var(--surface);font:700 11px/1 var(--mono);white-space:nowrap}.rail-chip-tools{color:#216b42;background:#e1f0e7}.rail-chip-tool-list,.rail-chip-tokens{color:#1d6573;background:#e0eff0;max-width:260px;overflow:hidden;text-overflow:ellipsis}.rail-chip-step-time{color:#6d5521;background:#f3ead3}.rail-chip-elapsed-time{color:#224f74;background:#dfeaf4}.chip{display:inline-flex;align-items:center;gap:5px;min-height:24px;border:1px solid var(--rule);border-radius:999px;padding:0 8px;font:700 11px/1 var(--mono);white-space:nowrap}.tool-name-chip{color:#1d6573;background:#cfe9ee;border-color:#a9cbd2}.tool-name-chip.tool-error-chip{color:#8d2c22;background:#f4ddd7;border-color:#e5b9ae;box-shadow:0 4px 10px rgba(146,54,34,.16)}.tool-exec-inline{color:#163f47}.tool-error-chip .tool-exec-inline{color:#8d2c22}.step-body{border-top:1px solid var(--rule);padding:12px;display:grid;gap:10px}.block{border-radius:var(--radius);background:var(--surface-2);padding:12px;border-left:4px solid var(--rule)}.block h4{margin:0 0 8px;font:700 13px/1 var(--mono);letter-spacing:.05em;text-transform:uppercase}.block p{margin:0 0 8px}pre{margin:8px 0 0;max-height:280px;overflow:auto;white-space:pre-wrap;overflow-wrap:anywhere;color:var(--ink);font:13px/1.5 var(--mono)}.message-block{border-left-color:#315f8f}.reasoning-block{border-left-color:#70429b}.tool-block{border-left-color:#2b6fa4;background:#e0edf6}.observation-block{border-left-color:#2f8f5b;background:#e1f0e7}.selected-evidence-card>summary{cursor:pointer;font:700 13px/1.2 var(--mono);text-transform:uppercase;letter-spacing:.04em;list-style:none}.selected-evidence-card h4{margin:0 0 8px;font:700 13px/1.2 var(--mono);text-transform:uppercase;letter-spacing:.04em;color:var(--muted)}.selected-evidence-card code{font:13px/1.45 var(--mono);overflow-wrap:anywhere}.selected-evidence-card .info-grid{margin:8px 0 0}.evidence-list{display:grid;gap:7px;margin:8px 0 0;padding-left:18px}
@media(max-width:700px){.workspace{padding:14px}.topline,.panel-head,.trace-head,.step>summary{grid-template-columns:1fr}.metric-controls{justify-content:flex-start}.segmented{flex-wrap:wrap;border-radius:8px}.visible-grid{grid-template-columns:1fr}.session-axis{min-height:auto;border-right:0;border-top:1px solid var(--rule);padding:10px 0 2px}.info-grid{grid-template-columns:1fr}.data-table{min-width:880px}.preview{white-space:normal}.rail{justify-items:start}.rail-summary,.rail-tool-row,.rail-time{justify-content:flex-start}}
</style>
</head>
<body>
<div class="workspace">
  <section class="topline">
    <h1>__TITLE__</h1>
  </section>
  <section id="report-notes"></section>
  <section class="panel-stack" id="comparison"></section>
  <section class="trace-panel" id="trace"></section>
</div>
<script type="application/json" id="peval-py-data">__DATA__</script>
<script>
const $ = id => document.getElementById(id);
const esc = value => String(value ?? "").replace(/[&<>"]/g, ch => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[ch]));
const lower = value => String(value || "").toLowerCase();
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
  { key: "duration", label: "Duration" },
  { key: "tokens", label: "Tokens" },
  { key: "tools", label: "Tool Calls" },
  { key: "turns", label: "Turns" }
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
  $("report-notes").innerHTML = notes.length ? `<div class="report-note-list">${notes.map(note => `<article class="report-note"><strong>${esc(note.label || "Report note")}</strong><div class="note-body">${renderMarkdown(note.markdown || "")}</div></article>`).join("")}</div>` : "";
}
function renderComparison() {
  const rows = reportRows();
  if (!rows.length) {
    $("comparison").innerHTML = "";
    return;
  }
  $("comparison").innerHTML = `
    <section class="panel" aria-labelledby="heatmap-title">
      <div class="panel-head"><div><p class="eyebrow">visible heatmap</p><h2 id="heatmap-title">Visible Heatmap</h2><p class="copy">Hue follows outcome. Shade follows the selected metric across visible sessions.</p></div><div class="metric-controls"><div class="segmented" id="metric-buttons"></div></div></div>
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
  target.innerHTML = METRICS.map(metric => `<button class="metric-button ${metric.key === state.metricMode ? "active" : ""}" type="button" data-metric="${esc(metric.key)}">${esc(metric.label)}</button>`).join("");
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
    return `<div class="session-axis"><strong>${esc(session)}</strong><span>${esc(adapter)} ${esc(model)}</span></div><button class="cell ${status} ${shadeFor(row, rows)} ${row.trial_key === selectedKey() ? "selected" : ""}" type="button" data-trial-key="${esc(row.trial_key)}"><strong>${esc(formatMetric(metricValue(row)))}</strong><span>${esc(row.status || "-")} / ${esc(session)}<br>${esc(adapter)} ${esc(model)}</span></button>`;
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
    { key: "session_id", label: "Session", width: "180px", value: row => row.session_id || row.trial_key },
    { key: "adapter", label: "Adapter", width: "120px", value: row => row.adapter || "-" },
    { key: "model", label: "Model", width: "150px", value: row => row.model || "-" },
    { key: "status", label: "Result", width: "104px", value: row => row.status || "-", html: row => `<span class="stamp ${lower(row.status || "passed")}">${esc(row.status || "-")}</span>` },
    { key: "duration_ms", label: "Duration", width: "104px", type: "number", numeric: true, sortable: true, value: row => row.duration_ms, format: fmtMs },
    { key: "turns", label: "Turns", width: "82px", type: "number", numeric: true, sortable: true, value: row => row.turns, format: fmtNum },
    { key: "total_tool_calls", label: "Tool Calls", width: "106px", type: "number", numeric: true, sortable: true, value: row => row.total_tool_calls, format: value => hasMetricValue(value) ? fmtNum(value) : "-" },
    { key: "tokens", label: "Tokens", width: "100px", type: "number", numeric: true, sortable: true, value: row => row.tokens, format: fmtNum },
    { key: "cost_usd", label: "Cost", width: "92px", type: "number", numeric: true, sortable: true, value: row => row.cost_usd, format: fmtCost },
    { key: "notes", label: "Notes", width: "220px", value: row => noteSnippetFor(row.trial_key), html: row => renderNotesCell(row.trial_key), cellTitle: row => notesPlainText(notesFor(row.trial_key)) }
  ];
}
function renderLeaderboard() {
  const target = $("leaderboard");
  if (!target) return;
  const rows = applyTableControls(reportRows());
  const columns = leaderboardColumns();
  target.innerHTML = `
    <div class="panel-head"><div><p class="eyebrow">leaderboard</p><h2 id="leaderboard-title">Leaderboard</h2><p class="copy">Each row is one visible session-as-Trial. Numeric columns sort; rows update the selected Trial.</p></div></div>
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
    return `<th class="${column.numeric ? "num" : ""}" title="${esc(column.label)}"><button class="sort-button ${active ? "active" : ""}" type="button" data-table-sort="${esc(column.key)}" aria-label="Sort ${esc(column.label)}"><span class="sort-label">${esc(column.label)}</span><span class="sort-mark">${mark}</span></button></th>`;
  }).join("");
  const body = rows.length ? rows.map(row => renderTableRow(row, columns)).join("") : `<tr><td class="table-empty" colspan="${columns.length}">No matching rows</td></tr>`;
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
  const status = lower(trial.status || "passed");
  const agentName = trajectory?.agent?.name || "-";
  const model = trajectory?.agent?.model_name || "-";
  $("trace").innerHTML = `
    <div class="trace-head"><div><p class="eyebrow">selected trial trajectory</p><h2 id="trace-title" class="trace-title"><span>session</span><code>${esc(trial.trial_key || "-")}</code></h2></div><span class="status ${status}">${esc(status)}</span></div>
    <h3>Run</h3>
    ${infoGrid([
      ["trial", trial.trial_key || "-"],
      ["variant", trial.variant_label || "-"],
      ["session", trajectory?.session_id || "-"],
      ["agent / model", `${agentName} / ${model}`],
      ["time", `${fmtDate(trial.started_at_ms)} -> ${fmtDate(trial.finished_at_ms)}`],
      ["wall duration", fmtMs(trialWallDurationMs(trial))],
      ["steps/events", `${(trajectory?.steps || []).length}/${trial.total_events ?? "-"}`],
      ["system exposed", systemExposed(trajectory) ? "yes" : "no"],
      ["reasoning exposed", reasoningExposed(trajectory) ? "yes" : "no"]
    ])}
    <h3>Result</h3>
    ${infoGrid([
      ["status", trial.status || "-"],
      ["score", fmtScore(trial.score)],
      ["evaluator", trial.score_message || "-"],
      ["tokens", fmtNum(tokenTotal(metrics))],
      ["turns", metrics.total_turns ?? "-"],
      ["tool success / total", toolCallRatio(metrics.total_tool_calls ?? 0, metrics.total_tool_errors ?? 0)],
      ["cost", fmtCost(metrics.total_cost_usd)]
    ])}
    ${renderSelectedNotes(trial.trial_key)}
    ${renderSelectedEvidence(trajectory, trial)}
    ${renderStepsHeader(trajectory)}
    <div class="step-list" id="step-list">${(trajectory?.steps || []).map(step => renderStep(step, trial)).join("")}</div>
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
  const body = notes.length ? `<div class="note-list">${notes.map(renderManualNote).join("")}</div>` : `<p class="copy">No notes.</p>`;
  return `<section class="selected-extra"><h3>Notes</h3>${body}</section>`;
}
function renderSelectedEvidence(trajectory, meta) {
  const blocks = [renderSelectedUsage(trajectory), renderSelectedWarnings(meta), renderSelectedSource(meta)].filter(Boolean);
  return blocks.length ? `<section class="selected-extra selected-evidence"><h3>Evidence</h3><div class="selected-evidence-list">${blocks.join("")}</div></section>` : "";
}
function renderSelectedUsage(trajectory) {
  const metrics = trajectory?.final_metrics || {};
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
function renderSelectedWarnings(meta) {
  const warnings = meta.warnings || [];
  if (!warnings.length) return "";
  return `<article class="selected-evidence-card"><h4>Warnings</h4><ul class="evidence-list">${warnings.map(warning => `<li>${esc(warning)}</li>`).join("")}</ul></article>`;
}
function renderSelectedSource(meta) {
  const path = meta.data_ref?.relative_path;
  return path ? `<article class="selected-evidence-card"><h4>Input Source</h4><code>${esc(path)}</code></article>` : "";
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
function renderStep(step, meta) {
  const sm = stepMeta(meta, step.step_id);
  const preview = valuePreview(step.message).trim() || "(No Message)";
  return `<details class="step" data-step="${esc(step.step_id)}"><summary><div class="step-row"><span class="step-id">#${esc(step.step_id)}</span><span class="role ${esc(step.source)}">${esc(step.source)}</span><span class="preview">${esc(preview)}</span></div><div class="rail">${renderStepRail(step, sm)}</div></summary><div class="step-body">${renderBlocks(step, sm)}</div></details>`;
}
function renderBlocks(step, meta) {
  let html = "";
  if (step.reasoning_content) html += block("Reasoning", step.reasoning_content, "reasoning-block");
  const message = valuePreview(step.message);
  if (message.trim()) html += block(step.source === "system" ? "System Prompt" : "Message", message, "message-block");
  (step.tool_calls || []).forEach(tool => {
    const toolMeta = toolMetaFor(meta, tool.tool_call_id);
    html += `<div class="block tool-block"><h4>Tool Calls</h4><p>${renderToolNameChip(tool, toolMeta)} <span class="muted">ID: ${esc(tool.tool_call_id || "-")}${toolMeta?.status ? ` / ${esc(toolMeta.status)}` : ""}${renderToolTiming(toolMeta)}</span></p><pre>${esc(valuePreview(tool.arguments || {}))}</pre></div>`;
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
function renderToolNameChip(tool, toolMeta, extraClass = "") {
  const name = tool.function_name || toolMeta?.title || "tool";
  const exec = toolExecutionText(toolMeta);
  const title = exec ? ` title="${esc(`tool exec ${exec}`)}"` : "";
  const execHtml = exec ? ` <span class="tool-exec-inline">${esc(exec)}</span>` : "";
  const classes = ["chip", "tool-name-chip", extraClass, toolFailed(toolMeta) ? "tool-error-chip" : ""].filter(Boolean).join(" ");
  return `<span class="${esc(classes)}"${title}>${esc(name)}${execHtml}</span>`;
}
function renderToolTiming(toolMeta) {
  const parts = [];
  if (hasMetricValue(toolMeta?.generation_duration_ms)) parts.push(`generation ${fmtMs(toolMeta.generation_duration_ms)}`);
  return parts.length ? ` / ${parts.map(esc).join(" / ")}` : "";
}
function stepToolChips(step, meta) {
  const chips = [];
  (step.tool_calls || []).forEach(tool => {
    const name = String(tool.function_name || toolMetaFor(meta, tool.tool_call_id)?.title || "").trim();
    if (name) chips.push(renderToolNameChip(tool, toolMetaFor(meta, tool.tool_call_id), "rail-chip-tool-list"));
  });
  return chips;
}
function renderStepRail(step, meta) {
  const summaryItems = [];
  const toolCalls = (step.tool_calls || []).length;
  const observations = (step.observation?.results || []).length;
  const toolErrors = meta?.tool_error ? 1 : 0;
  if (toolCalls || toolErrors) summaryItems.push(`<span class="rail-chip rail-chip-tools">${esc(toolCallRatio(toolCalls, toolErrors))} tools</span>`);
  else if (observations) summaryItems.push(`<span class="rail-chip rail-chip-tools">${esc(observations)} observations</span>`);
  const tokens = stepTokenTotal(step);
  if (tokens !== null && tokens !== undefined) summaryItems.push(`<span class="rail-chip rail-chip-tokens" title="${esc(`${fmtNum(tokens)} tokens`)}">${esc(fmtRailTokens(tokens))} tok</span>`);
  const time = `<div class="rail-time"><span class="rail-chip rail-chip-step-time" title="step span">step ${esc(fmtMs(meta?.duration_ms))}</span><span class="rail-chip rail-chip-elapsed-time" title="elapsed since trajectory start">elapsed ${esc(fmtMs(meta?.elapsed_ms))}</span></div>`;
  const summary = `<div class="rail-summary">${summaryItems.join("")}${time}</div>`;
  const toolChips = stepToolChips(step, meta);
  return `${summary}${toolChips.length ? `<div class="rail-tool-row">${toolChips.join("")}</div>` : ""}`;
}
function toolCallRatio(total, errors) {
  const callTotal = Math.max(0, Number(total || 0));
  const errorTotal = Math.max(0, Number(errors || 0));
  return `${Math.max(0, callTotal - errorTotal)}/${callTotal}`;
}
function toolMetaFor(meta, toolCallId) { return (meta?.tool_calls || []).find(item => item.tool_call_id === toolCallId) || null; }
function observationMetaFor(meta, sourceCallId) { return (meta?.observations || []).find(item => item.source_call_id === sourceCallId) || null; }
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
</script>
</body>
</html>"""


def safe_json_for_script(value: str) -> str:
    return value.replace("&", "\\u0026").replace("<", "\\u003c").replace(">", "\\u003e")
