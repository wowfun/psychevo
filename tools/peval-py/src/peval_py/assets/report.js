const $ = id => document.getElementById(id);
const esc = value => String(value ?? "").replace(/[&<>"]/g, ch => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[ch]));
const lower = value => String(value || "").toLowerCase();
function scriptJson(id, fallback) {
  const node = $(id);
  if (!node) return fallback;
  try {
    return JSON.parse(node.textContent || JSON.stringify(fallback));
  } catch {
    return fallback;
  }
}
const I18N = scriptJson("peval-py-i18n", {});
const TOKEN_ESTIMATES = scriptJson("peval-py-token-estimates", {});
const RENDER_OPTIONS = scriptJson("peval-py-render-options", { mode: "report" });
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
function fmtPct(value) { return hasMetricValue(value) ? `${(Number(value) * 100).toFixed(1)}%` : "-"; }
function fmtScore(value) { return hasMetricValue(value) ? Number(value).toLocaleString() : "-"; }
function hasMetricValue(value) { return value !== null && value !== undefined && value !== "" && !Number.isNaN(Number(value)); }
function data() { return JSON.parse($("peval-py-data").textContent || "{}"); }
function serveMode() { return RENDER_OPTIONS?.mode === "serve"; }
function initialAdapterDefaults() {
  return RENDER_OPTIONS?.adapter_defaults && typeof RENDER_OPTIONS.adapter_defaults === "object"
    ? { ...RENDER_OPTIONS.adapter_defaults }
    : {};
}
function adapterDefaults() {
  return state.adapterDefaults || {};
}
const state = { view: null, selectedTrial: null, selectedStep: null, rowSelection: new Set(), tables: {}, timelineChart: null, boundGlobalControls: false, serveSources: Array.isArray(RENDER_OPTIONS?.sources) ? RENDER_OPTIONS.sources : [], adapterDefaults: initialAdapterDefaults(), notesEditor: null };
const SUBMENU_DETAILS_SELECTOR = ".export-menu,.filter-control";
const OPEN_SUBMENU_DETAILS_SELECTOR = ".export-menu[open],.filter-control[open]";
function closeOpenSubmenus(except = null) {
  document.querySelectorAll(OPEN_SUBMENU_DETAILS_SELECTOR).forEach(details => {
    if (details !== except) details.open = false;
  });
}
function listValue(value) {
  return Array.isArray(value) ? value : [];
}
function reportRows() {
  const trajectories = listValue(state.view?.trajectory);
  const metas = listValue(state.view?.trajectory_meta);
  if (metas.length > 1) {
    return metas
      .map((meta, index) => synthesizedReportRow(trajectories[index] || {}, meta))
      .filter(row => row.trial_key);
  }
  return [];
}
function synthesizedReportRow(trajectory, meta) {
  const metrics = trajectory?.final_metrics || {};
  const totalToolCalls = hasMetricValue(finalMetric(metrics, "total_tool_calls")) ? Number(finalMetric(metrics, "total_tool_calls")) : 0;
  const totalToolErrors = hasMetricValue(finalMetric(metrics, "total_tool_errors")) ? Number(finalMetric(metrics, "total_tool_errors")) : 0;
  const agent = trajectory?.agent || {};
  return {
    trial_key: meta?.trial_key,
    session_id: trajectory?.session_id || "-",
    source_alias: meta?.source_alias,
    adapter: meta?.adapter,
    model: agent.model_name,
    status: meta?.status,
    finished_at_ms: meta?.finished_at_ms,
    duration_ms: meta?.duration_ms,
    wall_duration_ms: trialWallDurationMs(meta),
    turns: finalMetric(metrics, "total_turns"),
    total_tool_calls: totalToolCalls,
    total_tool_errors: totalToolErrors,
    tokens: tokenTotal(metrics),
    cost_usd: metrics.total_cost_usd,
    warnings: Array.isArray(meta?.warnings) ? meta.warnings.length : 0,
  };
}
function selectedKey() {
  return state.selectedTrial || state.view?.trajectory_meta?.[0]?.trial_key || null;
}
function selectedIndex() {
  const key = selectedKey();
  const metas = state.view?.trajectory_meta || [];
  const index = metas.findIndex(meta => meta.trial_key === key);
  return index >= 0 ? index : 0;
}
function trialIndexFor(trialKey) {
  const metas = state.view?.trajectory_meta || [];
  return metas.findIndex(meta => meta.trial_key === trialKey);
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
  if (!state.selectedTrial) {
    const firstFailed = reportRows().find(row => lower(row.status) !== "passed");
    state.selectedTrial = (firstFailed || reportRows()[0])?.trial_key || view.trajectory_meta?.[0]?.trial_key || null;
  }
  if (serveMode()) renderServeSources();
  bindGlobalControls();
  renderReportNotes(view.annotations?.report_notes || []);
  renderComparison();
  renderTrace();
  renderStepDrawer();
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
    <section class="leaderboard panel" aria-labelledby="leaderboard-title" id="leaderboard"></section>
    <section class="trajectory-overview panel" aria-labelledby="trajectory-overview-title" id="trajectory-overview"></section>
  `;
  renderComparisonPanels({ trace: false });
}
function notesFor(trialKey) {
  return (state.view?.annotations?.notes || []).filter(note => note.trial_key === trialKey);
}
function cellNoteFor(trialKey) {
  return notesFor(trialKey).find(note => note.source === "cell" && note.label === "notes.md") || null;
}
function analysisFor(trialKey) {
  return (state.view?.annotations?.analysis || []).find(item => item.trial_key === trialKey) || null;
}
function activeServeSources() {
  return (Array.isArray(state.serveSources) ? state.serveSources : []).filter(source => source?.active !== false);
}
function sourceForTrialKey(trialKey) {
  if (!serveMode()) return null;
  const index = trialIndexFor(trialKey);
  if (index < 0) return null;
  return activeServeSources()[index] || null;
}
function editableNotesSource(trialKey) {
  const source = sourceForTrialKey(trialKey);
  if (!source || source.refreshable === false || source.snapshot) return null;
  return source;
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
function sourceAliasFor(row) {
  return String(row?.source_alias || "").trim();
}
function sourceIdentityFor(row) {
  return row?.session_id || row?.trial_key || "-";
}
function sourceDisplayFor(row) {
  return sourceAliasFor(row) || sourceIdentityFor(row);
}
function sessionAliasValue(row) {
  return sourceAliasFor(row) || "-";
}
function renderSessionAliasCell(row) {
  const alias = sourceAliasFor(row);
  return alias ? esc(alias) : `<span class="muted">-</span>`;
}
function renderComparisonPanels(options = {}) {
  const rows = leaderboardRows();
  syncSelectionWithVisibleRows(rows);
  renderLeaderboard(rows);
  renderTrajectoryOverview(rows);
  if (options.trace !== false) renderTrace();
  renderStepDrawer();
}
function selectionColumn() {
  return {
    key: "__select",
    width: "46px",
    select: true,
    label: t("select_rows", "Select rows"),
    html: row => renderRowSelection(row)
  };
}
function leaderboardColumns() {
  return [
    { key: "session_id", label: t("session", "Session"), width: "180px", filterable: true, value: row => sourceIdentityFor(row), cellTitle: row => row.trial_key && row.trial_key !== sourceIdentityFor(row) ? row.trial_key : "" },
    { key: "source_alias", label: t("session_alias", "Session Alias"), width: "140px", value: row => sessionAliasValue(row), html: row => renderSessionAliasCell(row) },
    { key: "agent", label: t("agent", "Agent"), width: "120px", filterable: true, value: row => agentNameFor(row) },
    { key: "model", label: t("model", "Model"), width: "150px", filterable: true, value: row => row.model || "-" },
    { key: "status", label: t("result", "Result"), width: "104px", filterable: true, value: row => row.status || "-", filterLabel: value => statusLabel(value), html: row => `<span class="stamp ${lower(row.status || "passed")}">${esc(statusLabel(row.status))}</span>` },
    { key: "finished_at_ms", label: t("last_turn_end", "Last Turn End"), width: "156px", type: "number", numeric: true, sortable: true, value: row => row.finished_at_ms, format: fmtDate },
    { key: "duration_ms", label: t("duration", "Active Duration"), width: "124px", type: "number", numeric: true, sortable: true, metric: true, value: row => row.duration_ms, format: fmtMs },
    { key: "turns", label: t("turns", "Turns"), width: "82px", type: "number", numeric: true, sortable: true, metric: true, value: row => row.turns, format: fmtNum },
    { key: "total_tool_calls", label: t("tool_calls", "Tool Calls"), width: "106px", type: "number", numeric: true, sortable: true, metric: true, value: row => row.total_tool_calls, format: value => hasMetricValue(value) ? fmtNum(value) : "-" },
    { key: "tool_error_rate", label: t("tool_error_rate", "Tool Error Rate"), width: "126px", type: "number", numeric: true, sortable: true, metric: true, value: row => rowToolErrorRate(row), format: fmtPct },
    { key: "tokens", label: t("tokens", "Tokens"), width: "100px", type: "number", numeric: true, sortable: true, metric: true, value: row => row.tokens, format: fmtNum },
    { key: "cost_usd", label: t("cost", "Cost"), width: "92px", type: "number", numeric: true, sortable: true, value: row => row.cost_usd, format: fmtCost },
    { key: "notes", label: t("notes", "Notes"), width: "220px", value: row => noteSnippetFor(row.trial_key), html: row => renderNotesCell(row.trial_key), cellTitle: row => {
      const text = notesPlainText(notesFor(row.trial_key));
      return text && text !== noteSnippetFor(row.trial_key) ? text : "";
    } }
  ];
}
function displayLeaderboardColumns() {
  return serveMode() ? [selectionColumn(), ...leaderboardColumns()] : leaderboardColumns();
}
function agentNameFor(row) {
  const name = trajectoryFor(row?.trial_key)?.agent?.name;
  return name || row?.adapter || "-";
}
function rowToolErrorRate(row) {
  if (!hasMetricValue(row?.total_tool_calls) || Number(row.total_tool_calls) === 0) return null;
  return Number(row.total_tool_errors || 0) / Number(row.total_tool_calls);
}
function renderLeaderboard(rows = leaderboardRows()) {
  const target = $("leaderboard");
  if (!target) return;
  const columns = displayLeaderboardColumns();
  target.innerHTML = `
    <div class="panel-head"><div><h2 id="leaderboard-title">${esc(t("leaderboard", "Leaderboard"))}</h2><p class="copy">${esc(t("leaderboard_copy", "Each row is one visible session-as-Trial. Numeric cells shade by column value; rows update the selected Trial."))}</p></div>${renderLeaderboardExportControls()}</div>
    ${renderDataTable({
      tableId: "leaderboard",
      columns,
      rows,
      filterOptionsRows: reportRows(),
      rowClass: row => `clickable-row ${row.trial_key === selectedKey() ? "selected-row" : ""}`,
      rowAttrs: row => `data-trial-key="${esc(row.trial_key)}"`,
      rowTitle: row => row.trial_key,
    })}
  `;
  bindLeaderboardControls();
}
function renderLeaderboardExportControls() {
  if (!serveMode()) return "";
  return `<div class="leaderboard-export" data-serve-only>
    <details class="export-menu">
      <summary class="export-menu-button" aria-label="${esc(t("export_options", "Export options"))}">${esc(t("export", "Export"))}</summary>
      <div class="export-menu-panel">
        <button type="button" data-export-kind="csv">${esc(t("export_table", "Table"))}</button>
        <button type="button" data-export-kind="json">${esc(t("export_json_report", "JSON report"))}</button>
        <button type="button" data-export-kind="html">${esc(t("export_html_report", "HTML report"))}</button>
      </div>
    </details>
  </div>`;
}
function leaderboardRows() {
  return applyDataTableControls("leaderboard", reportRows(), leaderboardColumns(), reportRows());
}
function tableControls(tableId) {
  const controls = state.tables[tableId] || {};
  if (!Object.prototype.hasOwnProperty.call(controls, "sort")) controls.sort = null;
  controls.direction ||= "asc";
  controls.filters ||= {};
  state.tables[tableId] = controls;
  return controls;
}
function filterableColumns(columns) {
  return columns.filter(column => column.filterable);
}
function activeFilterValues(tableId, key) {
  const values = tableControls(tableId).filters?.[key];
  return Array.isArray(values) ? values : [];
}
function filterValue(row, column) {
  const source = column.filterValue || column.value || (item => item?.[column.key]);
  const raw = source(row);
  const text = raw === null || raw === undefined || raw === "" ? "-" : String(raw);
  return text;
}
function filterLabel(column, value) {
  return column.filterLabel ? column.filterLabel(value) : value;
}
function applyDataTableFilters(tableId, rows, columns) {
  const activeColumns = filterableColumns(columns);
  return rows.filter(row => columns.every(column => {
    if (!activeColumns.includes(column)) return true;
    const selected = activeFilterValues(tableId, column.key);
    if (!selected.length) return true;
    return selected.includes(filterValue(row, column));
  }));
}
function filterOptions(column, rows) {
  const values = rows.map(row => filterValue(row, column));
  return Array.from(new Set(values)).sort((left, right) => left.localeCompare(right, undefined, { numeric: true, sensitivity: "base" }));
}
function setFilterValue(tableId, key, value, checked) {
  const controls = tableControls(tableId);
  const selected = new Set(activeFilterValues(tableId, key));
  if (checked) selected.add(value);
  else selected.delete(value);
  const values = Array.from(selected);
  if (values.length) controls.filters[key] = values;
  else delete controls.filters[key];
}
function clearFilter(tableId, key) {
  delete tableControls(tableId).filters[key];
}
function toggleDataTableSort(tableId, key) {
  const controls = tableControls(tableId);
  if (controls.sort !== key) {
    controls.sort = key;
    controls.direction = "asc";
    return;
  }
  if (controls.direction === "asc") {
    controls.direction = "desc";
    return;
  }
  controls.sort = null;
  controls.direction = "asc";
}
function syncSelectionWithVisibleRows(rows) {
  const allRows = reportRows();
  if (!allRows.length) {
    if (!state.selectedTrial) state.selectedTrial = selectedKey();
    if (!selectedStepExists(state.selectedStep)) state.selectedStep = null;
    return;
  }
  const key = selectedKey();
  const selectedVisible = rows.some(row => row.trial_key === key);
  if (!rows.length) {
    if (state.selectedStep) state.selectedStep = null;
    return;
  }
  if (!selectedVisible) {
    state.selectedTrial = rows[0].trial_key;
    state.selectedStep = null;
    return;
  }
  if (!selectedStepVisible(rows)) state.selectedStep = null;
}
function selectedStepExists(selection) {
  if (!selection) return true;
  const { trialKey, stepId } = selection;
  const metas = state.view?.trajectory_meta || [];
  const index = metas.findIndex(meta => meta.trial_key === trialKey);
  if (index < 0) return false;
  const steps = (state.view?.trajectory || [])[index]?.steps || [];
  return steps.some(step => String(step.step_id) === String(stepId));
}
function selectedStepVisible(rows) {
  if (!state.selectedStep) return true;
  const { trialKey, stepId } = state.selectedStep;
  if (!rows.some(row => row.trial_key === trialKey)) return false;
  return selectedStepExists({ trialKey, stepId });
}
function compareTableValues(left, right, type, direction) {
  const leftMissing = left === null || left === undefined || left === "" || (type === "number" && Number.isNaN(Number(left)));
  const rightMissing = right === null || right === undefined || right === "" || (type === "number" && Number.isNaN(Number(right)));
  if (leftMissing || rightMissing) return leftMissing === rightMissing ? 0 : leftMissing ? 1 : -1;
  const delta = type === "number" ? Number(left) - Number(right) : String(left).localeCompare(String(right), undefined, { numeric: true, sensitivity: "base" });
  return direction === "desc" ? -delta : delta;
}
function applyDataTableControls(tableId, rows, columns, filterOptionsRows = rows) {
  const controls = tableControls(tableId);
  const filtered = applyDataTableFilters(tableId, rows, columns, filterOptionsRows);
  const sortColumn = columns.find(column => column.key === controls.sort && column.sortable);
  const out = [...filtered];
  if (sortColumn) out.sort((left, right) => compareTableValues(sortColumn.value(left), sortColumn.value(right), sortColumn.type, controls.direction));
  return out;
}
function tableText(row, column) {
  const raw = column.value(row);
  return column.format ? column.format(raw, row) : (raw ?? "-");
}
function renderDataTable({ tableId, columns, rows, tableClass = "", shellClass = "", rowClass = "", rowAttrs = "", rowTitle = null, emptyText = null, filterOptionsRows = rows }) {
  const controls = tableControls(tableId);
  const headers = columns.map(column => renderTableHeader(tableId, column, controls, rows, filterOptionsRows)).join("");
  const rowOptions = { rowClass, rowAttrs, rowTitle };
  const body = rows.length
    ? rows.map(row => renderTableRow(row, columns, rows, rowOptions)).join("")
    : `<tr><td class="table-empty" colspan="${columns.length}">${esc(emptyText || t("no_matching_rows", "No matching rows"))}</td></tr>`;
  const classes = ["data-table", tableClass].filter(Boolean).join(" ");
  const shellClasses = ["table-shell", shellClass].filter(Boolean).join(" ");
  return `<div class="${esc(shellClasses)}"><div class="table-wrap"><table class="${esc(classes)}" data-table-id="${esc(tableId)}"><thead><tr>${headers}</tr></thead><tbody>${body}</tbody></table></div></div>`;
}
function renderTableHeader(tableId, column, controls, rows = [], filterOptionsRows = rows) {
  if (column.select) return renderSelectionHeader(rows);
  const active = controls.sort === column.key;
  const mark = active ? (controls.direction === "desc" ? "&#9660;" : "&#9650;") : "&#8597;";
  const label = column.sortable
    ? `<button class="sort-button ${active ? "active" : ""}" type="button" data-table-sort="${esc(column.key)}" aria-label="${esc(t("sort", "Sort"))} ${esc(column.label)}"><span class="sort-label">${esc(column.label)}</span><span class="sort-mark">${mark}</span></button>`
    : `<span class="static-head">${esc(column.label)}</span>`;
  const filter = column.filterable ? renderFilterControl(tableId, column, filterOptionsRows) : "";
  const contentClass = column.filterable ? "table-head-cell table-head-inline" : "table-head-cell";
  return `<th class="${column.numeric ? "num" : ""}"><div class="${contentClass}">${label}${filter}</div></th>`;
}
function renderFilterControl(tableId, column, rows) {
  const selected = new Set(activeFilterValues(tableId, column.key));
  const options = filterOptions(column, rows);
  const count = selected.size;
  const countText = count ? `<span class="filter-count">${esc(`${count} ${t("selected_count", "selected")}`)}</span>` : "";
  const optionHtml = options.length
    ? options.map(value => `<label class="filter-option"><input type="checkbox" data-filter-key="${esc(column.key)}" value="${esc(value)}" ${selected.has(value) ? "checked" : ""}><span>${esc(filterLabel(column, value))}</span></label>`).join("")
    : `<p class="filter-empty">${esc(t("no_matching_rows", "No matching rows"))}</p>`;
  return `<details class="filter-control ${count ? "active" : ""}" data-filter-menu="${esc(column.key)}"><summary class="filter-button" aria-label="${esc(t("filter", "Filter"))} ${esc(column.label)}"><span class="filter-icon">&#9662;</span>${countText}</summary><div class="filter-menu"><div class="filter-menu-head"><strong>${esc(column.label)}</strong><button class="filter-clear" type="button" data-filter-clear="${esc(column.key)}" ${count ? "" : "disabled"}>${esc(t("clear", "Clear"))}</button></div><div class="filter-options">${optionHtml}</div></div></details>`;
}
function renderSelectionHeader(rows) {
  const visible = rows.filter(row => row?.trial_key);
  const selected = visible.filter(row => state.rowSelection.has(row.trial_key));
  const checked = visible.length > 0 && selected.length === visible.length;
  const partial = selected.length > 0 && selected.length < visible.length;
  return `<th class="select-col"><label class="select-box"><input type="checkbox" data-select-visible ${checked ? "checked" : ""} ${partial ? "data-partial=\"true\"" : ""} aria-label="${esc(t("select_visible_rows", "Select visible rows"))}"><span></span></label></th>`;
}
function renderRowSelection(row) {
  const key = row.trial_key || "";
  const checked = state.rowSelection.has(key);
  return `<label class="select-box"><input type="checkbox" data-row-select="${esc(key)}" ${checked ? "checked" : ""} aria-label="${esc(t("select_row_for_export", "Select row for export"))}: ${esc(key)}"><span></span></label>`;
}
function tableOptionValue(option, row, fallback = "") {
  return typeof option === "function" ? option(row) : (option || fallback);
}
function renderTableRow(row, columns, rows, options = {}) {
  const className = tableOptionValue(options.rowClass, row);
  const attrs = tableOptionValue(options.rowAttrs, row);
  const title = tableOptionValue(options.rowTitle, row);
  const titleAttr = title && !String(attrs).includes("title=") ? ` title="${esc(title)}"` : "";
  return `<tr class="${esc(className)}"${attrs ? ` ${attrs}` : ""}${titleAttr}>${columns.map(column => renderDataCell(row, column, rows)).join("")}</tr>`;
}
function renderDataCell(row, column, rows) {
  if (column.select) return `<td class="select-col">${column.html(row)}</td>`;
  const classes = [column.numeric ? "num" : "", column.metric ? metricCellShade(row, column, rows) : "", column.className || ""].filter(Boolean).join(" ");
  const html = column.html ? column.html(row) : esc(tableText(row, column));
  const title = column.cellTitle ? column.cellTitle(row) : "";
  return `<td class="${classes}" ${title ? `title="${esc(title)}"` : ""}>${html}</td>`;
}
function metricCellShade(row, column, rows) {
  const value = column.value(row);
  if (!hasMetricValue(value)) return "metric-cell metric-missing";
  const values = rows.map(item => column.value(item)).filter(hasMetricValue).map(Number);
  if (!values.length) return "metric-cell metric-missing";
  const min = Math.min(...values);
  const max = Math.max(...values);
  if (min === max) return "metric-cell metric-shade-2";
  const bucket = Math.max(0, Math.min(4, Math.round(((Number(value) - min) / (max - min)) * 4)));
  return `metric-cell metric-shade-${bucket}`;
}
function bindDataTableControls(root, tableId, onChange) {
  if (!root) return;
  const rerender = typeof onChange === "function" ? onChange : (() => {});
  root.querySelectorAll("[data-table-sort]").forEach(button => {
    button.addEventListener("click", event => {
      event.stopPropagation();
      toggleDataTableSort(tableId, button.dataset.tableSort);
      rerender();
    });
  });
  root.querySelectorAll("[data-filter-key]").forEach(input => {
    input.addEventListener("change", event => {
      event.stopPropagation();
      setFilterValue(tableId, input.dataset.filterKey, input.value, input.checked);
      rerender();
    });
  });
  root.querySelectorAll("[data-filter-clear]").forEach(button => {
    button.addEventListener("click", event => {
      event.stopPropagation();
      clearFilter(tableId, button.dataset.filterClear);
      rerender();
    });
  });
}
function bindLeaderboardControls() {
  const target = $("leaderboard");
  if (!target) return;
  bindDataTableControls(target, "leaderboard", () => renderComparisonPanels());
  bindServeSelectionControls(target);
  bindServeExportControls(target);
  bindTrialSelection(target);
}
function bindServeSelectionControls(target) {
  if (!serveMode()) return;
  target.querySelectorAll(".select-box").forEach(control => {
    control.addEventListener("click", event => event.stopPropagation());
  });
  target.querySelectorAll("[data-row-select]").forEach(input => {
    input.addEventListener("click", event => event.stopPropagation());
    input.addEventListener("change", event => {
      event.stopPropagation();
      const key = input.dataset.rowSelect;
      if (!key) return;
      if (input.checked) state.rowSelection.add(key);
      else state.rowSelection.delete(key);
      renderComparisonPanels({ trace: false });
    });
  });
  target.querySelectorAll("[data-select-visible]").forEach(input => {
    input.indeterminate = input.hasAttribute("data-partial");
    input.addEventListener("click", event => event.stopPropagation());
    input.addEventListener("change", event => {
      event.stopPropagation();
      const rows = leaderboardRows();
      const visibleKeys = rows.map(row => row.trial_key).filter(Boolean);
      const allSelected = visibleKeys.length > 0 && visibleKeys.every(key => state.rowSelection.has(key));
      visibleKeys.forEach(key => {
        if (allSelected) state.rowSelection.delete(key);
        else state.rowSelection.add(key);
      });
      renderComparisonPanels({ trace: false });
    });
  });
}
function bindServeExportControls(target) {
  if (!serveMode()) return;
  target.querySelectorAll("[data-export-kind]").forEach(button => {
    button.addEventListener("click", event => {
      event.stopPropagation();
      exportCurrentScope(button.dataset.exportKind || "csv");
      button.closest("details")?.removeAttribute("open");
    });
  });
}
function bindTrialSelection(root) {
  root.querySelectorAll("[data-trial-key]").forEach(node => {
    node.addEventListener("click", event => {
      event.stopPropagation();
      state.selectedTrial = node.getAttribute("data-trial-key");
      state.selectedStep = null;
      renderComparisonPanels();
    });
  });
}
function exportScopeRows() {
  const rows = leaderboardRows();
  const selected = rows.filter(row => state.rowSelection.has(row.trial_key));
  return selected.length ? selected : rows;
}
function exportCurrentScope(kind) {
  const rows = exportScopeRows();
  if (kind === "json") {
    downloadText("peval-report-v19.json", "application/json", JSON.stringify(reportSubset(rows), null, 2));
    return;
  }
  if (kind === "html") {
    downloadText("peval-report.html", "text/html", htmlReportForSubset(reportSubset(rows)));
    return;
  }
  downloadText("peval-leaderboard-visible.csv", "text/csv", csvForRows(rows));
}
function csvForRows(rows) {
  const columns = leaderboardColumns();
  const header = columns.map(column => csvValue(column.label));
  const body = rows.map(row => columns.map(column => csvValue(tableText(row, column))).join(","));
  return [header.join(","), ...body].join("\n");
}
function csvValue(value) {
  const text = String(value ?? "");
  return /[",\n\r]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}
function reportSubset(rows) {
  const original = state.view || {};
  const metas = original.trajectory_meta || [];
  const trajectories = original.trajectory || [];
  const selectedKeys = new Set(rows.map(row => row.trial_key));
  const orderedMeta = [];
  const orderedTrajectories = [];
  rows.forEach(row => {
    const index = metas.findIndex(meta => meta.trial_key === row.trial_key);
    if (index >= 0) {
      orderedMeta.push({ ...metas[index] });
      orderedTrajectories.push(trajectories[index]);
    }
  });
  const subset = {
    schema_version: original.schema_version,
    includes: listValue(original.includes).filter(item => item !== "comparison"),
    trajectory: orderedTrajectories,
    trajectory_meta: orderedMeta
  };
  if (original.annotations) {
    subset.annotations = {
      ...original.annotations,
      notes: (original.annotations.notes || []).filter(note => selectedKeys.has(note.trial_key)),
      analysis: (original.annotations.analysis || []).filter(item => selectedKeys.has(item.trial_key))
    };
  }
  return subset;
}
function htmlReportForSubset(report) {
  const clone = document.documentElement.cloneNode(true);
  clone.querySelectorAll("[data-serve-only]").forEach(node => node.remove());
  ["report-notes", "comparison", "trace"].forEach(id => {
    const node = clone.querySelector(`#${id}`);
    if (node) node.innerHTML = "";
  });
  const dataNode = clone.querySelector("#peval-py-data");
  if (dataNode) dataNode.textContent = safeJsonForScript(JSON.stringify(report));
  const optionsNode = clone.querySelector("#peval-py-render-options");
  if (optionsNode) optionsNode.textContent = safeJsonForScript(JSON.stringify({ mode: "report" }));
  const body = clone.querySelector("body");
  if (body) {
    body.classList.remove("serve-mode");
    body.classList.add("report-mode");
  }
  return `<!doctype html>\n${clone.outerHTML}`;
}
function safeJsonForScript(value) {
  return String(value).replace(/&/g, "\\u0026").replace(/</g, "\\u003c").replace(/>/g, "\\u003e");
}
function downloadText(filename, mime, text) {
  const blob = new Blob([text], { type: `${mime};charset=utf-8` });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}
function renderTrajectoryOverview(rows = leaderboardRows()) {
  const target = $("trajectory-overview");
  if (!target) return;
  const body = rows.length
    ? rows.map(row => renderTrajectoryOverviewRow(row)).join("")
    : `<div class="trajectory-empty">${esc(t("no_matching_rows", "No matching rows"))}</div>`;
  target.innerHTML = `
    <div class="panel-head"><div><h2 id="trajectory-overview-title">${esc(t("trajectory_overview", "Trajectory Overview"))}</h2><p class="copy">${esc(t("trajectory_overview_copy", "Rows follow the current Leaderboard order. Nodes align by step index and show role initials."))}</p></div></div>
    <div class="trajectory-overview-list">${body}</div>
  `;
  bindTrajectoryControls(target);
}
function renderTrajectoryOverviewRow(row) {
  const trajectory = trajectoryFor(row.trial_key);
  const steps = trajectory?.steps || [];
  const selected = row.trial_key === selectedKey();
  const session = sourceDisplayFor(row);
  const agent = agentNameFor(row);
  const secondary = sourceAliasFor(row) ? `${sourceIdentityFor(row)} / ${agent}` : agent;
  const timingModel = trajectoryOverviewTimingModel(row.trial_key);
  return `<div class="trajectory-row ${selected ? "selected-row" : ""}" data-trial-key="${esc(row.trial_key)}" title="${esc(row.trial_key)}"><div class="trajectory-label"><strong>${esc(session)}</strong><span>${esc(secondary)}</span></div><div class="trajectory-track">${steps.map((step, index) => renderTrajectoryNode(step, index, row.trial_key, timingModel)).join("")}</div></div>`;
}
function trajectoryOverviewTimingModel(trialKey) {
  const meta = metaFor(trialKey);
  const steps = meta?.steps || [];
  return { meta, maxStepDurationMs: maxPositiveMetric(steps.map(item => item.duration_ms)) };
}
function overviewStepMeta(meta, stepId) {
  return (meta?.steps || []).find(item => String(item.step_id) === String(stepId)) || {};
}
function renderTrajectoryNode(step, index, trialKey, timingModel) {
  const rawStepId = step?.step_id ?? index + 1;
  const stepId = String(rawStepId);
  const selected = state.selectedStep?.trialKey === trialKey && String(state.selectedStep?.stepId) === stepId;
  const stepDuration = overviewStepMeta(timingModel?.meta, rawStepId).duration_ms;
  const ratio = timingRatio(stepDuration, timingModel?.maxStepDurationMs);
  const classes = ["trajectory-node", selected ? "selected-node" : "", trajectoryDurationHeatClass(ratio)].filter(Boolean).join(" ");
  const label = stepTitle(step, index, stepDuration, ratio);
  return `<button class="${esc(classes)}" type="button" data-trial-key="${esc(trialKey)}" data-step-id="${esc(stepId)}" title="${esc(label)}" aria-label="${esc(label)}"><span class="trajectory-node-letter">${esc(roleLetter(step?.source))}</span></button>`;
}
function roleLetter(source) {
  const role = lower(source);
  if (role === "system") return "S";
  if (role === "user") return "U";
  if (role === "agent") return "A";
  return "?";
}
function stepTitle(step, index, stepDuration = null, durationRatio = null) {
  const id = step?.step_id ?? index + 1;
  const role = step?.source || "unknown";
  const preview = stepPreviewText(step);
  const duration = hasMetricValue(stepDuration) ? timeTitle("step", stepDuration, durationRatio, "slowest step") : "";
  const head = duration ? `#${id} ${role}; ${duration}` : `#${id} ${role}`;
  return preview ? `${head}: ${preview}` : head;
}
function bindTrajectoryControls(target) {
  target.querySelectorAll("[data-step-id]").forEach(node => {
    node.addEventListener("click", event => {
      event.stopPropagation();
      state.selectedTrial = node.dataset.trialKey;
      state.selectedStep = { trialKey: node.dataset.trialKey, stepId: node.dataset.stepId };
      renderComparisonPanels();
    });
  });
  target.querySelectorAll(".trajectory-row[data-trial-key]").forEach(row => {
    row.addEventListener("click", event => {
      event.stopPropagation();
      state.selectedTrial = row.getAttribute("data-trial-key");
      state.selectedStep = null;
      renderComparisonPanels();
    });
  });
}
function firstToolName(step) {
  const tool = (step?.tool_calls || [])[0];
  return tool?.function_name || "";
}
function shortText(value) {
  const text = String(value || "").replace(/\s+/g, " ").trim();
  return text.length > 80 ? `${text.slice(0, 80)}...` : text;
}
function stepPreviewText(step) {
  if (!step) return "";
  return shortText(valuePreview(step?.message).trim() || valuePreview(step?.reasoning_content).trim());
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
  const runItems = [
    [t("trial", "Trial"), trial.trial_key || "-"],
    [t("variant", "Variant"), trial.variant_label || "-"],
    [t("session", "Session"), trajectory?.session_id || "-"],
    [t("agent_model", "Agent / model"), `${agentName} / ${model}`],
    [t("time", "Time"), `${fmtDate(trial.started_at_ms)} -> ${fmtDate(trial.finished_at_ms)}`],
    [t("wall_duration", "Wall duration"), fmtMs(trialWallDurationMs(trial))],
    [t("steps_events", "Steps/events"), `${(trajectory?.steps || []).length}/${trial.total_events ?? "-"}`],
    [t("system_exposed", "System exposed"), systemExposed(trajectory) ? t("yes", "yes") : t("no", "no")],
    [t("reasoning_exposed", "Reasoning exposed"), reasoningExposed(trajectory) ? t("yes", "yes") : t("no", "no")]
  ];
  if (trial.source_alias) {
    runItems.splice(3, 0, [t("source_alias", "Source alias"), trial.source_alias]);
  }
  disposeTimelineChart();
  $("trace").innerHTML = `
    <div class="trace-head"><div><p class="eyebrow">${esc(t("selected_trial_trajectory", "selected trial trajectory"))}</p><h2 id="trace-title" class="trace-title"><span>${esc(t("selected_session_label", "session"))}</span><code>${esc(trial.trial_key || "-")}</code></h2></div><span class="status ${status}">${esc(statusLabel(status))}</span></div>
    <h3>${esc(t("run", "Run"))}</h3>
    ${infoGrid(runItems)}
    <h3>${esc(t("result", "Result"))}</h3>
    ${infoGrid([
      [t("status", "Status"), statusLabel(trial.status || "-")],
      [t("score", "Score"), fmtScore(trial.score)],
      [t("evaluator", "Evaluator"), trial.score_message || "-"],
      [t("tokens", "Tokens"), fmtNum(tokenTotal(metrics))],
      [t("turns", "Turns"), finalMetric(metrics, "total_turns") ?? "-"],
      [t("tool_success_total", "Tool success / total"), toolCallRatio(finalMetric(metrics, "total_tool_calls") ?? 0, finalMetric(metrics, "total_tool_errors") ?? 0)],
      [t("cost", "Cost"), fmtCost(metrics.total_cost_usd)]
    ])}
    ${renderSelectedNotes(trial.trial_key)}
    ${renderSelectedAnalysis(trial.trial_key)}
    ${renderSelectedEvidence(trajectory, trial)}
    ${renderTimelineDiagnostics(trajectory, trial)}
    ${renderStepsHeader(trajectory)}
    <div class="step-list" id="step-list">${(trajectory?.steps || []).map(step => renderStep(step, trial, timingStats)).join("")}</div>
  `;
  initTimelineDiagnostics(trajectory, trial);
  bindTimelineControls();
  bindStepToggle();
}
function renderStepDrawer() {
  const target = $("step-drawer");
  if (!target) return;
  if (!state.selectedStep) {
    setStepDrawerOpen(false);
    target.hidden = true;
    target.innerHTML = "";
    return;
  }
  const { trialKey, stepId } = state.selectedStep;
  const metas = state.view?.trajectory_meta || [];
  const index = metas.findIndex(meta => meta.trial_key === trialKey);
  const trial = index >= 0 ? metas[index] : null;
  const trajectory = index >= 0 ? (state.view?.trajectory || [])[index] : null;
  const step = (trajectory?.steps || []).find(item => String(item.step_id) === String(stepId));
  if (!trial || !trajectory || !step) {
    state.selectedStep = null;
    setStepDrawerOpen(false);
    target.hidden = true;
    target.innerHTML = "";
    return;
  }
  const timingStats = stepTimingStats(trial);
  setStepDrawerOpen(true);
  target.hidden = false;
  target.innerHTML = `
    <div class="step-drawer-panel" role="dialog" aria-modal="false" aria-labelledby="step-drawer-title">
      <div class="step-drawer-head">
        <div><p class="eyebrow">${esc(t("step_details", "Step details"))}</p><h2 id="step-drawer-title">#${esc(step.step_id)}</h2><p class="copy">${esc(trial.trial_key || "-")}</p></div>
        <button class="step-drawer-close" type="button" data-step-drawer-close aria-label="${esc(t("close", "Close"))}">${esc(t("close", "Close"))}</button>
      </div>
      <div class="step-drawer-body">${renderStep(step, trial, timingStats, { open: true })}</div>
    </div>
  `;
  target.querySelectorAll("[data-step-drawer-close]").forEach(button => {
    button.addEventListener("click", event => {
      event.stopPropagation();
      state.selectedStep = null;
      renderComparisonPanels();
    });
  });
}
function setStepDrawerOpen(open) {
  document.body.classList.toggle("step-drawer-open", Boolean(open));
}
function bindGlobalControls() {
  if (state.boundGlobalControls) return;
  document.addEventListener("keydown", event => {
    if (event.key === "Escape" && closeServeSourceManager()) {
      return;
    }
    if (event.key !== "Escape" || !state.selectedStep) return;
    state.selectedStep = null;
    renderComparisonPanels();
  });
  document.addEventListener("click", event => {
    closeOpenSubmenus(event.target?.closest?.(SUBMENU_DETAILS_SELECTOR) || null);
  }, true);
  document.addEventListener("click", event => {
    if (!state.selectedStep) return;
    const target = event.target;
    if (target?.closest?.("#step-drawer") || target?.closest?.("[data-source-manager]") || target?.closest?.("[data-step-id]") || target?.closest?.("[data-timeline-step-id]") || target?.closest?.("[data-timeline-chart]")) return;
    state.selectedStep = null;
    renderComparisonPanels();
  });
  document.addEventListener("click", event => {
    if (!serveMode()) return;
    const editButton = event.target?.closest?.("[data-notes-edit]");
    if (editButton) {
      event.preventDefault();
      beginNotesEdit(editButton.dataset.trialKey || selectedKey());
      return;
    }
    const cancelButton = event.target?.closest?.("[data-notes-cancel]");
    if (cancelButton) {
      event.preventDefault();
      cancelNotesEdit();
      return;
    }
    const saveButton = event.target?.closest?.("[data-notes-save]");
    if (saveButton) {
      event.preventDefault();
      saveSelectedNotes(saveButton);
    }
  });
  window.addEventListener("resize", () => {
    if (state.timelineChart) state.timelineChart.resize();
  });
  if (serveMode()) bindServeSourceControls();
  state.boundGlobalControls = true;
}
function bindServeSourceControls() {
  document.querySelectorAll("[data-source-manager-open]").forEach(button => {
    button.addEventListener("click", event => {
      event.preventDefault();
      openServeSourceManager();
    });
  });
  document.querySelectorAll("[data-source-manager-close]").forEach(button => {
    button.addEventListener("click", event => {
      event.preventDefault();
      closeServeSourceManager();
    });
  });
  const manager = document.querySelector("[data-source-manager]");
  if (manager) {
    manager.addEventListener("click", event => {
      if (event.target === manager) closeServeSourceManager();
    });
  }
  document.querySelectorAll("[data-refresh-all]").forEach(button => {
    button.addEventListener("click", () => refreshServeReportFromServer({ refresh: true }));
  });
  document.querySelectorAll("[data-refresh-sources]").forEach(button => {
    button.addEventListener("click", () => refreshServeSourcesFromServer());
  });
  document.querySelectorAll("[data-locale-select]").forEach(select => {
    select.addEventListener("change", event => {
      changeServeLocale(event.target.value);
    });
  });
  document.querySelectorAll("[data-source-add-form]").forEach(form => {
    form.addEventListener("submit", event => {
      event.preventDefault();
      submitServeSourceForm(form);
    });
  });
  document.querySelectorAll("[data-db-inspect]").forEach(button => {
    button.addEventListener("click", event => {
      event.preventDefault();
      inspectDbSessions(button.closest("[data-source-add-form]"));
    });
  });
  document.querySelectorAll("[data-db-session-picker]").forEach(picker => {
    picker.addEventListener("change", event => {
      if (event.target?.matches?.("[data-db-select-all]")) {
        setDbSessionSelection(picker, event.target.checked);
      }
    });
    picker.addEventListener("click", event => {
      const button = event.target?.closest?.("[data-db-add-selected]");
      if (!button) return;
      event.preventDefault();
      addSelectedDbSessions(button.closest("[data-source-add-form]"));
    });
  });
  document.querySelectorAll("[data-source-upload-form]").forEach(form => {
    form.addEventListener("submit", event => {
      event.preventDefault();
      submitServeUploadForm(form);
    });
  });
  bindAdapterDefaultDbControls();
  const sourceList = document.querySelector("[data-source-list]");
  if (sourceList) {
    sourceList.addEventListener("click", event => {
      const aliasButton = event.target?.closest?.("[data-source-alias-save]");
      if (aliasButton) {
        event.preventDefault();
        saveSourceAlias(aliasButton);
        return;
      }
      const button = event.target?.closest?.("[data-source-action]");
      if (!button) return;
      event.preventDefault();
      mutateServeSource(button.dataset.sourceKey, button.dataset.sourceAction);
    });
  }
}
async function changeServeLocale(locale) {
  try {
    await serveApi("/api/config/locale", {
      method: "POST",
      body: { locale }
    });
    window.location.reload();
  } catch (error) {
    setServeStatus(error.message || String(error), true);
  }
}
function bindAdapterDefaultDbControls() {
  bindAdapterDefaultDbConfigForm();
  document.querySelectorAll("[data-source-add-form][data-source-kind=\"db\"]").forEach(form => {
    const select = form.querySelector("[name=\"adapter\"]");
    if (!select) return;
    select.addEventListener("change", () => applyDefaultDbToForm(form, { force: true }));
    applyDefaultDbToForm(form);
  });
}
function bindAdapterDefaultDbConfigForm() {
  const form = document.querySelector("[data-adapter-default-db-form]");
  if (!form) return;
  const select = form.querySelector("[name=\"adapter\"]");
  const input = form.querySelector("[name=\"default_db_path\"]");
  if (!select || !input) return;
  select.addEventListener("change", () => syncAdapterDefaultDbForm(form));
  form.addEventListener("submit", event => {
    event.preventDefault();
    saveAdapterDefaultDb(form);
  });
  form.querySelector("[data-adapter-default-db-clear]")?.addEventListener("click", event => {
    event.preventDefault();
    input.value = "";
    saveAdapterDefaultDb(form);
  });
  syncAdapterDefaultDbForm(form);
}
function syncAdapterDefaultDbForm(form) {
  const select = form?.querySelector?.("[name=\"adapter\"]");
  const input = form?.querySelector?.("[name=\"default_db_path\"]");
  if (!select || !input) return;
  input.value = adapterDefaults()[select.value] || "";
}
async function saveAdapterDefaultDb(form) {
  const adapter = String(form?.querySelector?.("[name=\"adapter\"]")?.value || "").trim();
  const input = form?.querySelector?.("[name=\"default_db_path\"]");
  const defaultDbPath = String(input?.value || "").trim();
  if (!adapter) return;
  try {
    const payload = await serveApi("/api/config/adapter-default-db", {
      method: "POST",
      body: {
        adapter,
        default_db_path: defaultDbPath
      }
    });
    state.adapterDefaults = payload?.adapter_defaults && typeof payload.adapter_defaults === "object"
      ? { ...payload.adapter_defaults }
      : { ...adapterDefaults(), [adapter]: payload?.default_db_path || "" };
    if (!payload?.default_db_path) delete state.adapterDefaults[adapter];
    updateAdapterDefaultOptions();
    syncAdapterDefaultDbForm(form);
    applyUpdatedAdapterDefaultToDbForms(adapter);
    const message = payload?.default_db_path
      ? t("serve_adapter_default_db_saved", "Adapter default DB saved")
      : t("serve_adapter_default_db_cleared", "Adapter default DB cleared");
    setServeStatus(message);
    showServeNotice(message);
  } catch (error) {
    setServeStatus(error.message || String(error), true);
    showServeNotice(error.message || String(error), true);
  }
}
function updateAdapterDefaultOptions() {
  document.querySelectorAll("select[name=\"adapter\"] option").forEach(option => {
    const defaultDb = adapterDefaults()[option.value] || "";
    if (defaultDb) {
      option.dataset.defaultDb = defaultDb;
    } else {
      delete option.dataset.defaultDb;
    }
  });
}
function applyUpdatedAdapterDefaultToDbForms(adapter) {
  document.querySelectorAll("[data-source-add-form][data-source-kind=\"db\"]").forEach(form => {
    const selected = selectedAdapterValue(form);
    applyDefaultDbToForm(form, { force: Boolean(selected && selected === adapter) });
  });
}
function dbFieldFor(form) {
  return form?.querySelector?.("[name=\"db\"]") || null;
}
function defaultDbForAdapter(form) {
  const select = form?.querySelector?.("[name=\"adapter\"]");
  const value = selectedAdapterValue(form);
  if (!select || !value) return "";
  const selected = Array.from(select.options || []).find(option => option.value === value);
  return selected?.dataset?.defaultDb || adapterDefaults()[value] || "";
}
function applyDefaultDbToForm(form, options = {}) {
  const field = dbFieldFor(form);
  if (!field) return "";
  const defaultDb = defaultDbForAdapter(form);
  if (defaultDb && (options.force || !String(field.value || "").trim())) {
    field.value = defaultDb;
  }
  return defaultDb;
}
function openServeSourceManager() {
  const manager = document.querySelector("[data-source-manager]");
  if (!manager) return;
  manager.hidden = false;
  document.body.classList.add("source-manager-open");
}
function closeServeSourceManager() {
  const manager = document.querySelector("[data-source-manager]");
  if (!manager || manager.hidden) return false;
  manager.hidden = true;
  document.body.classList.remove("source-manager-open");
  return true;
}
function renderServeSources() {
  if (!serveMode()) return;
  const sources = Array.isArray(state.serveSources) ? state.serveSources : [];
  const countNode = document.querySelector("[data-source-count]");
  if (countNode) {
    const word = sources.length === 1 ? t("serve_source_count", "source") : t("serve_sources_count", "sources");
    countNode.textContent = `${sources.length} ${word}`;
  }
  const list = document.querySelector("[data-source-list]");
  if (list) {
    if (!sources.length) {
      list.innerHTML = `<li class="source-row empty">${esc(t("serve_no_sources", "No sources loaded"))}</li>`;
      return;
    }
    const rows = sourceRows();
    list.innerHTML = `<li class="source-table-item">${renderDataTable({
      tableId: "sources",
      columns: sourceColumns(),
      rows,
      tableClass: "source-table",
      shellClass: "source-table-shell",
      rowClass: source => `source-table-row ${source?.active === false ? "archived" : ""}`,
      rowAttrs: source => `data-source-row data-source-key="${esc(source?.source_key || "")}"`,
      rowTitle: source => source?.source_key || source?.label || ""
    })}</li>`;
    bindDataTableControls(list, "sources", () => renderServeSources());
  }
}
function sourceColumns() {
  return [
    { key: "label", label: t("source", "Source"), width: "220px", value: source => sourceDisplayLabel(source), html: renderServeSourceLabel, cellTitle: source => source?.label || "" },
    { key: "last_turn_finished_at_ms", label: t("last_turn_end", "Last Turn End"), width: "156px", type: "number", numeric: true, sortable: true, value: source => source?.last_turn_finished_at_ms, format: fmtDate },
    { key: "status", label: t("status", "status"), width: "170px", value: source => sourceStatusText(source), html: renderServeSourceStatus },
    { key: "alias", label: t("serve_source_alias", "Alias"), width: "240px", value: source => String(source?.source_alias || "").trim() || "-", html: renderServeSourceAliasEdit },
    { key: "actions", label: t("more", "More"), width: "210px", value: () => "", html: renderServeSourceActions }
  ];
}
function sourceRows() {
  return applyDataTableControls("sources", Array.isArray(state.serveSources) ? state.serveSources : [], sourceColumns());
}
function sourceDisplayLabel(source) {
  const label = source?.label || source?.source_key || "source";
  const alias = String(source?.source_alias || "").trim();
  return alias || label;
}
function sourceStatusText(source) {
  const active = source?.active !== false;
  const stateLabel = active ? t("serve_active", "active") : t("serve_archived", "archived");
  return `${source?.kind || "source"} / ${source?.adapter || "-"} / ${source?.last_status || "-"} / ${stateLabel}`;
}
function renderServeSourceLabel(source) {
  const key = source?.source_key || "";
  const label = source?.label || key || "source";
  const alias = String(source?.source_alias || "").trim();
  const displayLabel = alias || label;
  const origin = alias ? `<span class="source-origin">${esc(label)}</span>` : "";
  const session = source?.trial_session_id || source?.session_id || "";
  const sessionLine = session ? `<span>${esc(t("session", "Session"))}: <code>${esc(session)}</code></span>` : "";
  return `<span class="source-label-stack"><strong>${esc(displayLabel)}</strong>${origin}${sessionLine}</span>`;
}
function renderServeSourceStatus(source) {
  return `<span class="source-status-text">${esc(sourceStatusText(source))}</span>`;
}
function renderServeSourceAliasEdit(source) {
  const key = source?.source_key || "";
  const alias = String(source?.source_alias || "").trim();
  return `<label class="source-alias-edit">
    <span>${esc(t("serve_source_alias", "Alias"))}</span>
    <input data-source-alias-input data-source-key="${esc(key)}" value="${esc(alias)}" autocomplete="off">
    <button type="button" data-source-alias-save data-source-key="${esc(key)}">${esc(t("serve_save_alias", "Save alias"))}</button>
  </label>`;
}
function renderServeSourceActions(source) {
  const key = source?.source_key || "";
  const active = source?.active !== false;
  const snapshot = Boolean(source?.snapshot);
  const refreshable = source?.refreshable !== false && !snapshot;
  const refreshButton = refreshable && key
    ? `<button type="button" data-source-action="refresh" data-source-key="${esc(key)}">${esc(t("serve_refresh", "Refresh"))}</button>`
    : `<span>${esc(t("serve_snapshot", "snapshot"))}</span>`;
  const archiveAction = active ? "archive" : "activate";
  const archiveLabel = active ? t("serve_archive", "Archive") : t("serve_activate", "Activate");
  const archiveButton = key ? `<button type="button" data-source-action="${archiveAction}" data-source-key="${esc(key)}">${esc(archiveLabel)}</button>` : "";
  const deleteButton = key ? `<button type="button" data-source-action="delete" data-source-key="${esc(key)}">${esc(t("serve_delete", "Delete"))}</button>` : "";
  return `<div class="source-row-actions">${refreshButton}${archiveButton}${deleteButton}</div>`;
}
async function submitServeSourceForm(form) {
  if (form?.dataset?.sourceKind === "db") applyDefaultDbToForm(form);
  const body = formPayload(form);
  const kind = form.dataset.sourceKind;
  if (!kind) return;
  const sourceValue = String(body[kind] || "").trim();
  if (!sourceValue) return;
  try {
    setServeStatus(t("serve_refresh", "Refresh"));
    const payload = await serveApi("/api/sources", { method: "POST", body });
    form.reset();
    applyServeMutationPayload(payload);
  } catch (error) {
    showServeNotice(`${t("serve_import_failed", "Import failed")}: ${error.message || String(error)}`, true);
    setServeStatus(error.message || String(error), true);
  }
}
async function inspectDbSessions(form) {
  if (!form) return;
  applyDefaultDbToForm(form);
  const body = formPayload(form);
  const db = String(body.db || "").trim();
  if (!db) return;
  const picker = form.querySelector("[data-db-session-picker]");
  try {
    setServeStatus(t("serve_inspect_db", "Inspect DB"));
    const payload = await serveApi("/api/db-sessions", {
      method: "POST",
      body: {
        db,
        adapter: selectedAdapterValue(form)
      }
    });
    if (payload?.adapter) setAdapterChoice(form, payload.adapter);
    renderDbSessionPicker(form, payload);
    setServeStatus(t("serve_latest_snapshots", "Latest snapshots"));
  } catch (error) {
    if (picker) {
      picker.hidden = false;
      picker.innerHTML = `<p class="copy danger">${esc(error.message || String(error))}</p>`;
    }
    setServeStatus(error.message || String(error), true);
  }
}
function renderDbSessionPicker(form, payload) {
  const picker = form.querySelector("[data-db-session-picker]");
  if (!picker) return;
  const sessions = Array.isArray(payload?.sessions) ? payload.sessions : [];
  form.dataset.inspectedDb = payload?.db || "";
  form.dataset.inspectedAdapter = payload?.adapter || "";
  picker.hidden = false;
  if (!sessions.length) {
    picker.innerHTML = `<div class="db-picker-head"><strong>${esc(t("serve_db_sessions", "DB sessions"))}</strong><span>${esc(t("serve_no_sessions", "No sessions found"))}</span></div>`;
    return;
  }
  const adapterLabel = payload?.inferred ? t("serve_adapter_inferred", "Adapter inferred") : t("serve_adapter_selected", "Adapter selected");
  picker.innerHTML = `
    <div class="db-picker-head">
      <div><strong>${esc(t("serve_db_sessions", "DB sessions"))}</strong><span>${esc(adapterLabel)}: ${esc(payload?.adapter || "-")}</span></div>
      <label class="db-select-all"><input type="checkbox" data-db-select-all> ${esc(t("serve_select_all_visible", "Select all visible"))}</label>
    </div>
    <div class="db-session-table-wrap">
      <table class="db-session-table">
        <thead><tr><th></th><th>#</th><th>${esc(t("session", "Session"))}</th><th>${esc(t("serve_session_name", "Name"))}</th></tr></thead>
        <tbody>${sessions.map(renderDbSessionRow).join("")}</tbody>
      </table>
    </div>
    <div class="db-picker-actions">
      <span data-db-selected-count>0 ${esc(t("serve_selected_count", "selected"))}</span>
      <button class="step-toggle-button primary" type="button" data-db-add-selected>${esc(t("serve_add_selected", "Add selected"))}</button>
    </div>
  `;
  bindDbSessionSelectionCounters(picker);
}
function renderDbSessionRow(session) {
  const sessionId = String(session?.session_id || "");
  return `<tr>
    <td><input type="checkbox" data-db-session-checkbox value="${esc(sessionId)}" aria-label="${esc(sessionId)}"></td>
    <td>${esc(session?.index || "")}</td>
    <td><code>${esc(sessionId)}</code></td>
    <td>${esc(session?.name || "-")}</td>
  </tr>`;
}
function bindDbSessionSelectionCounters(picker) {
  picker.querySelectorAll("[data-db-session-checkbox]").forEach(box => {
    box.addEventListener("change", () => updateDbSelectedCount(picker));
  });
  updateDbSelectedCount(picker);
}
function setDbSessionSelection(picker, checked) {
  picker.querySelectorAll("[data-db-session-checkbox]").forEach(box => {
    box.checked = Boolean(checked);
  });
  updateDbSelectedCount(picker);
}
function selectedDbSessionIds(form) {
  return Array.from(form.querySelectorAll("[data-db-session-checkbox]:checked"))
    .map(box => String(box.value || "").trim())
    .filter(Boolean);
}
function updateDbSelectedCount(picker) {
  const count = picker.querySelectorAll("[data-db-session-checkbox]:checked").length;
  const target = picker.querySelector("[data-db-selected-count]");
  if (target) target.textContent = `${count} ${t("serve_selected_count", "selected")}`;
}
async function addSelectedDbSessions(form) {
  if (!form) return;
  const sessionIds = selectedDbSessionIds(form);
  if (!sessionIds.length) {
    setServeStatus(t("serve_select_sessions", "Select sessions"), true);
    return;
  }
  const body = formPayload(form);
  try {
    setServeStatus(t("serve_refresh", "Refresh"));
    const payload = await serveApi("/api/sources", {
      method: "POST",
      body: {
        db: form.dataset.inspectedDb || body.db,
        adapter: form.dataset.inspectedAdapter || selectedAdapterValue(form),
        session_ids: sessionIds,
        alias: body.alias
      }
    });
    form.reset();
    const picker = form.querySelector("[data-db-session-picker]");
    if (picker) {
      picker.hidden = true;
      picker.innerHTML = "";
    }
    delete form.dataset.inspectedDb;
    delete form.dataset.inspectedAdapter;
    applyServeMutationPayload(payload);
  } catch (error) {
    showServeNotice(`${t("serve_import_failed", "Import failed")}: ${error.message || String(error)}`, true);
    setServeStatus(error.message || String(error), true);
  }
}
async function submitServeUploadForm(form) {
  const formData = new FormData(form);
  const file = formData.get("file");
  if (!file || !file.name || typeof file.text !== "function") return;
  try {
    setServeStatus(t("serve_upload", "Upload"));
    const payload = await serveApi("/api/upload", {
      method: "POST",
      body: {
        filename: file.name,
        content: await file.text(),
        adapter: normalizeAdapterValue(formData.get("adapter")),
        alias: String(formData.get("alias") || "").trim()
      }
    });
    form.reset();
    applyServeMutationPayload(payload);
  } catch (error) {
    showServeNotice(`${t("serve_import_failed", "Import failed")}: ${error.message || String(error)}`, true);
    setServeStatus(error.message || String(error), true);
  }
}
function formPayload(form) {
  const formData = new FormData(form);
  const body = {};
  for (const [key, value] of formData.entries()) {
    const text = String(value || "").trim();
    if (text) body[key] = text;
  }
  return body;
}
async function mutateServeSource(sourceKey, action) {
  if (!sourceKey || !action) return;
  if (action === "delete" && !window.confirm(t("serve_delete_confirm", "Delete this source from peval-py state?"))) return;
  try {
    const payload = await serveApi(`/api/sources/${encodeURIComponent(sourceKey)}/${encodeURIComponent(action)}`, { method: "POST", body: {} });
    applyServeMutationPayload(payload);
  } catch (error) {
    setServeStatus(error.message || String(error), true);
  }
}
async function saveSourceAlias(button) {
  const sourceKey = button?.dataset?.sourceKey;
  if (!sourceKey) return;
  const row = button.closest("[data-source-row]");
  const input = row?.querySelector?.("[data-source-alias-input]");
  const alias = String(input?.value || "").trim();
  try {
    const payload = await serveApi(`/api/sources/${encodeURIComponent(sourceKey)}/alias`, {
      method: "POST",
      body: { alias }
    });
    applyServeMutationPayload(payload, { preserveTrial: selectedKey() });
  } catch (error) {
    setServeStatus(error.message || String(error), true);
  }
}
function selectedAdapterValue(form) {
  return normalizeAdapterValue(new FormData(form).get("adapter"));
}
function normalizeAdapterValue(value) {
  const text = String(value || "").trim();
  return text && text.toLowerCase() !== "auto" ? text : undefined;
}
function setAdapterChoice(form, adapter) {
  const value = String(adapter || "").trim();
  if (!value) return;
  const control = form.querySelector('[name="adapter"]');
  if (!control) return;
  if (control.tagName === "SELECT") {
    if (Array.from(control.options || []).some(option => option.value === value)) {
      control.value = value;
    }
    return;
  }
  const radio = Array.from(form.querySelectorAll('[name="adapter"]')).find(input => input.value === value);
  if (radio) radio.checked = true;
}
async function refreshServeReportFromServer(options = {}) {
  try {
    const payload = options.refresh
      ? await serveApi("/api/refresh", { method: "POST", body: {} })
      : { report: await serveApi("/api/report") };
    applyServeMutationPayload(payload);
  } catch (error) {
    setServeStatus(error.message || String(error), true);
  }
}
async function refreshServeSourcesFromServer() {
  try {
    const payload = await serveApi("/api/sources");
    if (Array.isArray(payload.sources)) {
      state.serveSources = payload.sources;
      renderServeSources();
      setServeStatus(t("serve_latest_snapshots", "Latest snapshots"));
    }
  } catch (error) {
    setServeStatus(error.message || String(error), true);
  }
}
async function serveApi(path, options = {}) {
  const headers = { ...(options.headers || {}) };
  let body = options.body;
  if (body !== undefined && typeof body !== "string") {
    headers["Content-Type"] = "application/json";
    body = JSON.stringify(body);
  }
  const response = await fetch(path, {
    method: options.method || "GET",
    headers,
    body,
    credentials: "same-origin"
  });
  const text = await response.text();
  const payload = text ? JSON.parse(text) : {};
  if (!response.ok) {
    throw new Error(payload?.error || response.statusText);
  }
  return payload;
}
function applyServeMutationPayload(payload, options = {}) {
  hideServeNotice();
  if (Array.isArray(payload?.sources)) {
    state.serveSources = payload.sources;
    renderServeSources();
  }
  if (payload?.report) {
    state.selectedTrial = options.preserveTrial || null;
    state.selectedStep = null;
    state.rowSelection.clear();
    render(payload.report);
  }
  setServeStatus(t("serve_latest_snapshots", "Latest snapshots"));
}
function setServeStatus(text, error = false) {
  const node = document.querySelector("[data-source-status]");
  if (!node) return;
  node.textContent = text;
  node.classList.toggle("danger", Boolean(error));
}
function showServeNotice(text, error = false) {
  const manager = document.querySelector("[data-source-manager]");
  if (!manager) return;
  let notice = manager.querySelector("[data-serve-notice]");
  if (!notice) {
    notice = document.createElement("div");
    notice.dataset.serveNotice = "";
    notice.className = "serve-notice";
    const modal = manager.querySelector(".source-manager-modal");
    modal?.prepend(notice);
  }
  notice.textContent = text;
  notice.classList.toggle("danger", Boolean(error));
  notice.hidden = false;
}
function hideServeNotice() {
  const notice = document.querySelector("[data-serve-notice]");
  if (notice) notice.hidden = true;
}
function infoGrid(items) {
  return `<div class="info-grid">${items.map(([label, value]) => `<div><span>${esc(label)}</span><strong>${esc(value)}</strong></div>`).join("")}</div>`;
}
function trialWallDurationMs(trial) {
  if (hasMetricValue(trial?.wall_duration_ms)) return Number(trial.wall_duration_ms);
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
function trajectoryDurationHeatClass(ratio) {
  if (ratio === null || ratio === undefined) return "";
  const level = Math.max(1, Math.min(10, Math.ceil(ratio * 10)));
  return `duration-heat-${level}`;
}
function timeTitle(label, value, ratio, basis) {
  const text = `${label} ${fmtMs(value)}`;
  return ratio === null || ratio === undefined ? text : `${text}; ${Math.round(ratio * 100)}% of ${basis}`;
}
function systemExposed(trajectory) { return (trajectory?.steps || []).some(step => step.source === "system"); }
function reasoningExposed(trajectory) { return (trajectory?.steps || []).some(step => step.reasoning_content); }
function tokenTotal(metrics) {
  const values = [metrics?.total_prompt_tokens, metrics?.total_completion_tokens].filter(hasMetricValue).map(Number);
  if (values.length) return values.reduce((sum, value) => sum + value, 0);
  const direct = metricExtra(metrics).usage?.total_tokens;
  return hasMetricValue(direct) ? Number(direct) : null;
}
function finalMetric(metrics, key) {
  return Object.prototype.hasOwnProperty.call(metrics || {}, key) ? metrics[key] : metricExtra(metrics)[key];
}
function metricExtra(metrics) {
  return metrics?.extra && typeof metrics.extra === "object" ? metrics.extra : {};
}
function renderSelectedNotes(trialKey) {
  const notes = notesFor(trialKey);
  const editor = renderNotesEditor(trialKey);
  const action = renderNotesAction(trialKey);
  const body = notes.length ? `<div class="note-list">${notes.map(renderManualNote).join("")}</div>` : `<p class="copy">${esc(t("no_notes", "No notes."))}</p>`;
  return `<section class="selected-extra selected-notes">
    <div class="selected-extra-head"><h3>${esc(t("notes", "Notes"))}</h3>${action}</div>
    ${editor}
    ${body}
  </section>`;
}
function renderNotesAction(trialKey) {
  const source = editableNotesSource(trialKey);
  if (!source || state.notesEditor?.trialKey === trialKey) return "";
  const cellNote = cellNoteFor(trialKey);
  const label = cellNote ? t("edit_notes", "Edit notes") : t("add_notes", "Add notes");
  return `<button class="step-toggle-button notes-edit-button" type="button" data-notes-edit data-trial-key="${esc(trialKey)}">${esc(label)}</button>`;
}
function renderNotesEditor(trialKey) {
  if (!serveMode() || state.notesEditor?.trialKey !== trialKey) return "";
  const markdown = state.notesEditor.markdown ?? "";
  const error = state.notesEditor.error ? `<p class="copy danger">${esc(state.notesEditor.error)}</p>` : "";
  const disabled = state.notesEditor.saving ? " disabled" : "";
  return `<article class="notes-editor-panel" data-notes-editor-panel>
    <textarea data-notes-editor data-trial-key="${esc(trialKey)}" rows="8">${esc(markdown)}</textarea>
    ${error}
    <div class="notes-editor-actions">
      <button class="step-toggle-button primary" type="button" data-notes-save data-trial-key="${esc(trialKey)}"${disabled}>${esc(t("save_notes", "Save notes"))}</button>
      <button class="step-toggle-button" type="button" data-notes-cancel${disabled}>${esc(t("cancel", "Cancel"))}</button>
    </div>
  </article>`;
}
function beginNotesEdit(trialKey) {
  if (!trialKey || !editableNotesSource(trialKey)) return;
  const note = cellNoteFor(trialKey);
  state.notesEditor = { trialKey, markdown: note?.markdown || "", error: "", saving: false };
  renderTrace();
}
function cancelNotesEdit() {
  state.notesEditor = null;
  renderTrace();
}
async function saveSelectedNotes(button) {
  const trialKey = button?.dataset?.trialKey || selectedKey();
  const source = editableNotesSource(trialKey);
  const panel = button?.closest?.("[data-notes-editor-panel]");
  const textarea = panel?.querySelector?.("[data-notes-editor]");
  if (!source?.source_key || !textarea) return;
  const markdown = textarea.value || "";
  state.notesEditor = { trialKey, markdown, error: "", saving: true };
  renderTrace();
  try {
    const payload = await serveApi(`/api/sources/${encodeURIComponent(source.source_key)}/notes`, {
      method: "POST",
      body: { markdown }
    });
    state.notesEditor = null;
    applyServeMutationPayload(payload, { preserveTrial: trialKey });
  } catch (error) {
    const message = `${t("notes_save_failed", "Save notes failed")}: ${error.message || String(error)}`;
    state.notesEditor = { trialKey, markdown, error: message, saving: false };
    setServeStatus(message, true);
    renderTrace();
  }
}
function renderSelectedAnalysis(trialKey) {
  const analysis = analysisFor(trialKey);
  if (!analysis) return "";
  const summary = analysis.summary ? `<pre>${esc(analysis.summary)}</pre>` : "";
  const markdown = analysis.md_report ? `<div class="note-body analysis-md">${renderMarkdown(analysis.md_report)}</div>` : "";
  const structured = renderStructuredAnalysis(analysis);
  const paths = renderAnalysisPaths(analysis);
  if (!summary && !markdown && !structured && !paths && analysis.status === "computed") return "";
  return `<section class="selected-extra selected-analysis"><h3>${esc(t("analysis", "Analysis"))}</h3><article class="selected-evidence-card analysis-card">${summary}${markdown}${structured}${paths}</article></section>`;
}
function renderStructuredAnalysis(analysis) {
  const blocks = [
    renderAnalysisFindings(analysis.findings),
    renderAnalysisList(t("recommendations", "Recommendations"), analysis.recommendations),
    renderAnalysisList(t("limitations", "Limitations"), analysis.limitations),
    renderAnalysisList(t("analysis_commands", "Analysis Commands"), analysis.commands),
    renderAnalysisDetails(analysis),
  ].filter(Boolean);
  return blocks.length ? `<div class="analysis-structured">${blocks.join("")}</div>` : "";
}
function renderAnalysisFindings(findings) {
  if (!Array.isArray(findings) || !findings.length) return "";
  return `<div class="analysis-block"><h4>${esc(t("findings", "Findings"))}</h4><ul class="evidence-list analysis-list">${findings.map(renderAnalysisFinding).join("")}</ul></div>`;
}
function renderAnalysisFinding(finding) {
  if (!isPlainObject(finding)) return `<li>${renderAnalysisValue(finding)}</li>`;
  const severity = finding.severity ? `<span class="chip">${esc(finding.severity)}</span>` : "";
  const title = finding.title || finding.summary || finding.message || t("finding", "Finding");
  const evidence = Array.isArray(finding.evidence) && finding.evidence.length
    ? `<p class="copy">${esc(t("evidence", "Evidence"))}: ${finding.evidence.map(analysisValueText).map(esc).join("; ")}</p>`
    : "";
  const recommendation = finding.recommendation
    ? `<p class="copy">${esc(t("recommendation", "Recommendation"))}: ${renderAnalysisValue(finding.recommendation)}</p>`
    : "";
  return `<li><div class="analysis-finding-head">${severity}<strong>${esc(title)}</strong></div>${evidence}${recommendation}</li>`;
}
function renderAnalysisList(label, values) {
  if (!Array.isArray(values) || !values.length) return "";
  return `<div class="analysis-block"><h4>${esc(label)}</h4><ul class="evidence-list analysis-list">${values.map(value => `<li>${renderAnalysisValue(value)}</li>`).join("")}</ul></div>`;
}
function renderAnalysisDetails(analysis) {
  const statusRows = [];
  if (analysis.analysis_status) statusRows.push([t("analysis_status", "Analysis Status"), analysis.analysis_status]);
  if (analysis.confidence !== undefined && analysis.confidence !== null && String(analysis.confidence).trim()) statusRows.push([t("confidence", "Confidence"), analysis.confidence]);
  const blocks = [];
  if (statusRows.length) blocks.push(infoGrid(statusRows));
  blocks.push(renderAnalysisObject(t("subject", "Subject"), analysis.subject));
  blocks.push(renderAnalysisMetrics(analysis.analysis_metrics));
  const html = blocks.filter(Boolean).join("");
  return html ? `<div class="analysis-block analysis-details">${html}</div>` : "";
}
function renderAnalysisMetrics(metrics) {
  if (!isPlainObject(metrics) || !Object.keys(metrics).length) return "";
  const blocks = [];
  if (isPlainObject(metrics.auto) && Object.keys(metrics.auto).length) {
    blocks.push(renderAnalysisMetricGroups(metrics.auto));
  }
  const imported = Object.fromEntries(Object.entries(metrics).filter(([key]) => key !== "auto"));
  if (Object.keys(imported).length) {
    blocks.push(renderAnalysisObject(t("imported_metrics", "Imported Metrics"), imported));
  }
  return blocks.length ? `<div class="analysis-metrics">${blocks.join("")}</div>` : "";
}
function renderAnalysisMetricGroups(autoMetrics) {
  return Object.entries(autoMetrics)
    .filter(([, value]) => isPlainObject(value) && Object.keys(value).length)
    .map(([key, value]) => renderAutoMetricGroup(key, value))
    .join("");
}
function renderAutoMetricGroup(key, metrics) {
  const blocks = [];
  const scalarRows = autoMetricScalarRows(key, metrics);
  if (scalarRows.length) blocks.push(infoGrid(scalarRows));
  if (key === "latency") {
    blocks.push(renderLatencyComparison(metrics));
  }
  return blocks.filter(Boolean).length
    ? `<div class="analysis-metric-group analysis-metric-group-${esc(key)}"><h5>${esc(analysisMetricGroupLabel(key))}</h5>${blocks.filter(Boolean).join("")}</div>`
    : "";
}
function autoMetricScalarRows(group, metrics) {
  return Object.entries(metrics)
    .filter(([key, value]) => isMetricScalar(value) && !autoMetricStructuredKeys(group).has(key))
    .map(([key, value]) => [analysisMetricLabel(key), metricValueText(key, value)]);
}
function autoMetricStructuredKeys(group) {
  const keys = {
    latency: ["step_duration_ms", "tool_execution_duration_ms", "model_duration_ms"],
  };
  return new Set(keys[group] || []);
}
function analysisMetricGroupLabel(key) {
  const labels = {
    tooling: t("metric_group_tooling", "Tooling"),
    cost: t("metric_group_cost", "Cost"),
    latency: t("metric_group_latency", "Latency"),
  };
  return labels[key] || key;
}
function analysisMetricLabel(key) {
  const labels = {
    tool_error_rate: t("metric_tool_error_rate", "Tool error rate"),
    distinct_tools: t("metric_distinct_tools", "Distinct tools"),
    cost_per_1k_tokens: t("metric_cost_per_1k_tokens", "Cost / 1k tokens"),
    model_duration_ms: t("metric_model_duration_ms", "Model duration"),
    count: t("metric_count", "Count"),
    errors: t("metric_errors", "Errors"),
    duration_ms: t("metric_duration", "Duration"),
    p50: "p50",
    q1: "q1",
    q3: "q3",
    p95: "p95",
    min: t("metric_min", "Min"),
    max: t("metric_max", "Max"),
  };
  return labels[key] || key;
}
function renderAnalysisObject(label, value) {
  if (!isPlainObject(value) || !Object.keys(value).length) return "";
  return `<div class="analysis-object"><h4>${esc(label)}</h4>${renderMetricTable(value)}</div>`;
}
function renderMetricTable(value, depth = 0) {
  if (!isPlainObject(value) || !Object.keys(value).length) return "";
  const rows = Object.entries(value)
    .map(([key, item]) => `<tr><th>${esc(analysisMetricLabel(key))}</th><td>${renderMetricValue(key, item, depth)}</td></tr>`)
    .join("");
  return `<table class="analysis-kv-table"><tbody>${rows}</tbody></table>`;
}
function renderMetricValue(key, value, depth = 0) {
  if (isMetricScalar(value)) return esc(metricValueText(key, value));
  if (Array.isArray(value)) {
    if (!value.length) return `<span class="muted">[]</span>`;
    if (depth < 1 && value.every(isPlainObject)) return renderMetricArrayTable(value, depth + 1);
    if (depth < 1 && value.every(isMetricScalar)) return `<span class="analysis-inline-list">${value.map(item => esc(metricValueText(key, item))).join(", ")}</span>`;
    return renderMetricDetails(value);
  }
  if (isPlainObject(value)) {
    if (depth < 1) return renderMetricTable(value, depth + 1);
    return renderMetricDetails(value);
  }
  return esc(analysisValueText(value));
}
function renderMetricArrayTable(values, depth = 0) {
  const keys = Array.from(new Set(values.flatMap(item => Object.keys(item))));
  if (!keys.length) return renderMetricDetails(values);
  const head = keys.map(key => `<th>${esc(analysisMetricLabel(key))}</th>`).join("");
  const rows = values.map(item => `<tr>${keys.map(key => `<td>${renderMetricValue(key, item[key], depth)}</td>`).join("")}</tr>`).join("");
  return `<div class="analysis-table-wrap"><table class="analysis-data-table"><thead><tr>${head}</tr></thead><tbody>${rows}</tbody></table></div>`;
}
function renderMetricDetails(value) {
  const summary = Array.isArray(value)
    ? t("metric_array_summary", "Array value")
    : t("metric_object_summary", "Object value");
  return `<details class="analysis-json-details"><summary>${esc(summary)}</summary><pre>${esc(JSON.stringify(value, null, 2))}</pre></details>`;
}
function renderLatencyComparison(metrics) {
  const rows = [
    ["step_duration_ms", t("metric_step_duration_ms", "Step duration"), metrics.step_duration_ms],
    ["tool_execution_duration_ms", t("metric_tool_execution_duration_ms", "Tool execution"), metrics.tool_execution_duration_ms],
    ["model_duration_ms", t("metric_model_duration_ms", "Model duration"), metrics.model_duration_ms],
  ].filter(([, , distribution]) => isPlainObject(distribution) && Object.keys(distribution).length);
  if (!rows.length) return "";
  const max = Math.max(...rows.map(([, , distribution]) => Number(distribution.max || 0)), 0);
  const body = rows.map(([key, label, distribution]) => renderLatencyComparisonRow(key, label, distribution, max)).join("");
  return `<div class="analysis-latency-chart">${body}</div>`;
}
function renderLatencyComparisonRow(key, label, distribution, max) {
  return `<div class="analysis-latency-row analysis-latency-${esc(key)}">${renderLatencyBoxPlot(label, distribution, max)}<h6>${esc(label)}</h6></div>`;
}
function renderLatencyBoxPlot(label, distribution, max) {
  const value = key => hasMetricValue(distribution[key]) ? Number(distribution[key]) : null;
  const min = value("min") ?? value("p50") ?? 0;
  const q1 = value("q1") ?? value("p50") ?? min;
  const p50 = value("p50") ?? q1;
  const q3 = value("q3") ?? p50;
  const p95 = value("p95") ?? q3;
  const high = value("max") ?? p95;
  const pct = item => max > 0 ? Math.max(0, Math.min(100, (Number(item || 0) / max) * 100)) : 0;
  const style = [
    `--whisker-bottom:${pct(min)}%`,
    `--whisker-height:${Math.max(0, pct(high) - pct(min))}%`,
    `--box-bottom:${pct(q1)}%`,
    `--box-height:${Math.max(0, pct(q3) - pct(q1))}%`,
    `--median-bottom:${pct(p50)}%`,
    `--p95-bottom:${pct(p95)}%`,
  ].join(";");
  const title = [
    `${label}`,
    `min ${metricValueText("duration_ms", min)}`,
    `q1 ${metricValueText("duration_ms", q1)}`,
    `p50 ${metricValueText("duration_ms", p50)}`,
    `q3 ${metricValueText("duration_ms", q3)}`,
    `p95 ${metricValueText("duration_ms", p95)}`,
    `max ${metricValueText("duration_ms", high)}`,
  ].join("; ");
  const labels = [
    ["max", high],
    ["p95", p95],
    ["p50", p50],
    ["min", min],
  ].filter(([stat, item], index, values) => index === values.findIndex(([otherStat]) => otherStat === stat));
  const labelHtml = labels.map(([stat, item]) => {
    const valuePct = pct(item);
    return `<span class="analysis-box-label analysis-box-label-${esc(stat)}" style="--label-bottom:${esc(`${valuePct}%`)}"><b>${esc(analysisMetricLabel(stat))}</b> ${esc(metricValueText("duration_ms", item))}</span>`;
  }).join("");
  return `<div class="analysis-boxplot" style="${esc(style)}" title="${esc(title)}" aria-label="${esc(title)}"><span class="analysis-box-axis"></span><span class="analysis-box-whisker"></span><span class="analysis-box-range"></span><span class="analysis-box-median"></span><span class="analysis-box-p95"></span>${labelHtml}</div>`;
}
function isMetricScalar(value) {
  return value === null || value === undefined || ["string", "number", "boolean"].includes(typeof value);
}
function metricValueText(key, value) {
  if (value === null || value === undefined || value === "") return "-";
  if (key === "tool_error_rate") return fmtPct(value);
  if (String(key).endsWith("_ms") || key === "duration_ms") return fmtMs(value);
  if (typeof value === "number") return fmtNum(value);
  return String(value);
}
function renderAnalysisValue(value) {
  return esc(analysisValueText(value));
}
function analysisValueText(value) {
  if (value === null || value === undefined) return "-";
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  try {
    return JSON.stringify(value);
  } catch (_error) {
    return String(value);
  }
}
function isPlainObject(value) {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}
function renderAnalysisPaths(analysis) {
  const paths = analysis.relative_paths || {};
  const rows = [];
  if (paths.json) rows.push(["JSON", paths.json]);
  if (paths.md) rows.push(["Markdown", paths.md]);
  if (!rows.length && analysis.relative_path) rows.push(["Source", analysis.relative_path]);
  return rows.length ? `<div class="analysis-source-list">${rows.map(([label, path]) => `<p class="copy analysis-path"><span class="analysis-source-label">${esc(label)}</span><code>${esc(path)}</code></p>`).join("")}</div>` : "";
}
function renderSelectedEvidence(trajectory, meta) {
  const blocks = [renderSelectedUsage(trajectory), renderSelectedWarnings(meta), renderSelectedSource(meta)].filter(Boolean);
  return blocks.length ? `<section class="selected-extra selected-evidence"><h3>${esc(t("evidence", "Evidence"))}</h3><div class="selected-evidence-list">${blocks.join("")}</div></section>` : "";
}
function renderSelectedUsage(trajectory) {
  const metrics = trajectory?.final_metrics || {};
  const extra = metricExtra(metrics);
  const usage = extra.usage || {};
  const accounting = extra.accounting || {};
  if (!extra.usage && !extra.accounting && !hasMetricValue(metrics.total_prompt_tokens) && !hasMetricValue(metrics.total_completion_tokens) && !hasMetricValue(metrics.total_cached_tokens)) return "";
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
const TIMELINE_INPUT_STAGE_THRESHOLD_MS = 50;
const TIMELINE_ESTIMATED_MODEL_CALL_CAP_MS = 600000;
function renderTimelineDiagnostics(trajectory, meta) {
  const trace = timelineTrace(trajectory, meta);
  const waterfall = trace.stages.length
    ? renderTimelineWaterfall(trace)
    : `<p class="timeline-empty">${esc(t("timeline_empty", "No timed step or tool durations available."))}</p>`;
  const table = trace.stages.length
    ? renderTimelineDetailTable(trace.stages, trace.model)
    : `<p class="timeline-empty">${esc(t("timeline_empty", "No timed step or tool durations available."))}</p>`;
  return `<section class="selected-extra timeline-diagnostics">
    ${renderTimelineSection("timeline-waterfall-section", t("timeline_waterfall", "Timeline Waterfall"), t("timeline_waterfall_copy", "Flat active-latency trace for meaningful delay."), `<span class="timeline-total">${esc(fmtTimelineDuration(trace.model.active_total_ms))}</span>`, waterfall)}
    ${renderTimelineSection("timeline-table-section", t("timeline_detail_table", "Timeline Detail Table"), t("timeline_table_copy", "Flat latency stages with true wall timing."), "", table)}
  </section>`;
}
function renderTimelineSection(className, title, copy, meta, body) {
  return `<details class="timeline-section ${esc(className)}" open>
    <summary class="timeline-head"><div><h3>${esc(title)}</h3><p class="copy">${esc(copy)}</p></div>${meta || ""}</summary>
    <div class="timeline-section-body">${body}</div>
  </details>`;
}
function renderTimelineWaterfall(trace) {
  const height = Math.max(300, trace.stages.length * 34 + 96);
  return `<div class="timeline-waterfall-shell"><div class="timeline-waterfall-chart" data-timeline-chart style="height:${esc(height)}px"></div><p class="timeline-fallback" data-timeline-fallback>${esc(t("timeline_echarts_unavailable", "ECharts did not load. Timeline Waterfall is unavailable, but the detail table is still shown."))}</p></div>`;
}
function initTimelineDiagnostics(trajectory, meta) {
  const trace = timelineTrace(trajectory, meta);
  initTimelineWaterfallChart(trace);
}
function disposeTimelineChart() {
  if (!state.timelineChart) return;
  state.timelineChart.dispose();
  state.timelineChart = null;
}
function initTimelineWaterfallChart(trace) {
  const node = document.querySelector("[data-timeline-chart]");
  const fallback = document.querySelector("[data-timeline-fallback]");
  if (!node || !trace.stages.length) return;
  if (!window.echarts) {
    node.hidden = true;
    return;
  }
  node.hidden = false;
  if (fallback) fallback.hidden = true;
  node.addEventListener("click", event => event.stopPropagation());
  state.timelineChart = window.echarts.init(node, null, { renderer: "canvas" });
  state.timelineChart.setOption(timelineChartOption(trace), true);
  state.timelineChart.on("click", params => openTimelineStep(params?.data?.trace_item));
}
function timelineTrace(trajectory, meta) {
  const steps = trajectory?.steps || [];
  const stepMetas = meta?.steps || [];
  const origin = timelineOriginMs(meta, stepMetas);
  const modelStage = timelineModelStageLabel(trajectory);
  const stages = [];
  const markers = [];
  let fallbackCursor = origin;
  let previousTimestamp = origin;
  steps.forEach((step, index) => {
    const stepId = step?.step_id ?? index + 1;
    const sm = stepMeta(meta, stepId);
    const stepDuration = timelineDurationMs(sm?.duration_ms);
    const stepStart = timelineStepStartMs(meta, sm, fallbackCursor);
    const stepEnd = timelineEndMs(stepStart, stepDuration);
    const source = lower(step?.source);
    if (source === "user" || source === "system") {
      if (positiveMetric(stepDuration) && Number(stepDuration) > TIMELINE_INPUT_STAGE_THRESHOLD_MS) {
        timelinePushStage(stages, {
          kind: "input",
          stage: source === "system" ? t("timeline_stage_system_processing", "System context processing") : t("timeline_stage_input_processing", "Input processing"),
          status: step?.source || "input",
          category: "io",
          wallStart: stepStart,
          wallEnd: stepEnd,
          duration: stepDuration,
          origin,
          trialKey: meta?.trial_key,
          stepId,
          ref: timelineStepRef(stepId),
        });
      } else {
        timelinePushMarker(markers, {
          name: source === "system" ? t("timeline_marker_system_context", "System context") : t("timeline_marker_user_input", "User input"),
          category: "io",
          wallStart: stepStart,
          origin,
          trialKey: meta?.trial_key,
          stepId,
          ref: timelineStepRef(stepId),
        });
      }
    } else if (source === "agent" || source === "assistant") {
      const hasExactModelDuration = positiveMetric(stepDuration);
      const modelDurationIsBoundaryEstimate = timelineModelDurationIsEstimate(sm);
      const modelEstimate = hasExactModelDuration || !timelineAllowsTimestampEstimates(meta)
        ? {}
        : timelineEstimatedModelCall(sm, step, stepStart, previousTimestamp);
      const modelDuration = hasExactModelDuration ? stepDuration : modelEstimate.duration_ms;
      timelinePushStage(stages, {
        kind: "agent",
        stage: modelStage,
        status: step?.source || "agent",
        category: "agent",
        wallStart: hasExactModelDuration ? stepStart : modelEstimate.wall_start_ms,
        wallEnd: hasExactModelDuration ? stepEnd : modelEstimate.wall_end_ms,
        duration: modelDuration,
        estimated: modelDurationIsBoundaryEstimate || (!hasExactModelDuration && modelEstimate.estimated),
        origin,
        trialKey: meta?.trial_key,
        stepId,
        ref: timelineStepRef(stepId),
      });
    } else if (positiveMetric(stepDuration)) {
      timelinePushStage(stages, {
        kind: "step",
        stage: t("timeline_stage_input_processing", "Input processing"),
        status: step?.source || "step",
        category: "tool",
        wallStart: stepStart,
        wallEnd: stepEnd,
        duration: stepDuration,
        origin,
        trialKey: meta?.trial_key,
        stepId,
        ref: timelineStepRef(stepId),
      });
    }
    (step?.tool_calls || []).forEach((tool, toolIndex) => {
      const toolMeta = toolMetaFor(sm, tool.tool_call_id);
      const toolDuration = timelineDurationMs(toolMeta?.execution_duration_ms);
      const toolStart = hasMetricValue(toolMeta?.timestamp_ms) ? Number(toolMeta.timestamp_ms) : stepStart;
      const toolEnd = timelineEndMs(toolStart, toolDuration);
      const category = timelineToolCategory(tool, toolMeta);
      timelinePushStage(stages, {
        kind: "tool",
        stage: `Tool: ${timelineToolLabel(tool, toolMeta)}`,
        status: toolMeta?.status || toolMeta?.title || "tool",
        category,
        wallStart: toolStart,
        wallEnd: toolEnd,
        duration: toolDuration,
        origin,
        trialKey: meta?.trial_key,
        stepId,
        toolCallId: tool.tool_call_id,
        ref: timelineToolRef(stepId, tool, toolIndex),
      });
    });
    fallbackCursor = Math.max(fallbackCursor, stepEnd ?? stepStart ?? fallbackCursor);
    if (hasMetricValue(stepStart)) previousTimestamp = Number(stepStart);
  });
  const orderedStages = stages.sort(timelineStageSort).map((stage, index) => ({
    ...stage,
    number: String(index + 1),
    category_meta: timelineCategoryMeta(stage.category),
  }));
  const model = timelineModel(orderedStages, markers);
  const displayStages = timelineAssignActiveOffsets(orderedStages, model);
  const displayMarkers = markers.map((marker, index) => ({
    ...marker,
    number: String(index + 1),
    category_meta: timelineCategoryMeta(marker.category),
    active_total_ms: model.active_total_ms,
    display_offset_ms: timelineMarkerActiveOffset(marker, displayStages),
  }));
  return { stages: displayStages, markers: displayMarkers, model };
}
function timelineAllowsTimestampEstimates(meta) {
  return meta?.timestamp_semantics !== "order_only";
}
function timelineModelDurationIsEstimate(stepMeta) {
  return lower(stepMeta?.duration_source) === "opencode_model_boundary_estimate";
}
function timelineAssignActiveOffsets(stages, model) {
  let cursor = 0;
  return stages.map(stage => {
    const duration = Math.max(0, Number(stage.duration_ms || 0));
    const out = {
      ...stage,
      active_total_ms: model.active_total_ms,
      display_start_ms: cursor,
      display_end_ms: cursor + duration,
    };
    cursor += duration;
    return out;
  });
}
function timelineMarkerActiveOffset(marker, stages) {
  const markerStart = Number(marker.start_offset_ms || 0);
  let cursor = 0;
  stages.forEach(stage => {
    if (Number(stage.start_offset_ms || 0) < markerStart) {
      cursor = Math.max(cursor, Number(stage.display_end_ms || 0));
    }
  });
  return cursor;
}
function timelinePushStage(stages, args) {
  const stage = timelineStage(args);
  if (!timelineStageHasMeasuredDuration(stage) || !hasMetricValue(stage.start_offset_ms)) return;
  stages.push(stage);
}
function timelineStageHasMeasuredDuration(stage) {
  if (!stage || !hasMetricValue(stage.duration_ms)) return false;
  return stage.kind === "tool" ? Number(stage.duration_ms) >= 0 : positiveMetric(stage.duration_ms);
}
function timelinePushMarker(markers, args) {
  const marker = timelineMarker(args);
  if (!marker || !hasMetricValue(marker.start_offset_ms)) return;
  markers.push(marker);
}
function timelineStage({ kind, stage, status, category, wallStart, wallEnd, duration, estimated, origin, trialKey, stepId, toolCallId, ref }) {
  const startOffset = hasMetricValue(wallStart) && hasMetricValue(origin) ? Math.max(0, Number(wallStart) - Number(origin)) : null;
  const endOffset = hasMetricValue(wallEnd) && hasMetricValue(origin) ? Math.max(startOffset || 0, Number(wallEnd) - Number(origin)) : startOffset;
  return {
    kind,
    stage,
    status,
    category,
    trial_key: trialKey,
    step_id: stepId,
    tool_call_id: toolCallId,
    wall_start_ms: wallStart,
    wall_end_ms: wallEnd,
    start_offset_ms: startOffset,
    end_offset_ms: endOffset,
    duration_ms: duration,
    estimated: Boolean(estimated),
    ref,
  };
}
function timelineModelStageLabel(trajectory) {
  const model = trajectory?.agent?.model_name;
  return model ? `Model: ${model}` : t("timeline_stage_model", "Model");
}
function timelineEstimatedModelCall(stepMeta, step, stepStart, previousTimestamp) {
  if (!hasMetricValue(stepStart)) return {};
  const start = Number(stepStart);
  const toolStarts = (step?.tool_calls || [])
    .map(tool => toolMetaFor(stepMeta, tool.tool_call_id)?.timestamp_ms)
    .filter(hasMetricValue)
    .map(Number)
    .filter(value => value > start);
  if (toolStarts.length) {
    const end = Math.min(...toolStarts);
    const duration = timelineEstimatedDurationMs(start, end);
    if (positiveMetric(duration)) {
      return {
        wall_start_ms: start,
        wall_end_ms: end,
        duration_ms: duration,
        estimated: true,
      };
    }
  }
  const previous = hasMetricValue(previousTimestamp) ? Number(previousTimestamp) : null;
  if (previous !== null && previous < start) {
    const duration = timelineEstimatedDurationMs(previous, start);
    if (positiveMetric(duration)) {
      return {
        wall_start_ms: previous,
        wall_end_ms: start,
        duration_ms: duration,
        estimated: true,
      };
    }
  }
  return {};
}
function timelineEstimatedDurationMs(start, end) {
  if (!hasMetricValue(start) || !hasMetricValue(end)) return null;
  const duration = Number(end) - Number(start);
  return duration > 0 && duration <= TIMELINE_ESTIMATED_MODEL_CALL_CAP_MS ? duration : null;
}
function timelineMarker({ name, category, wallStart, origin, trialKey, stepId, ref }) {
  if (!hasMetricValue(wallStart)) return null;
  const startOffset = hasMetricValue(origin) ? Math.max(0, Number(wallStart) - Number(origin)) : null;
  return {
    kind: "marker",
    name,
    category,
    trial_key: trialKey,
    step_id: stepId,
    wall_start_ms: wallStart,
    start_offset_ms: startOffset,
    ref,
  };
}
function timelineOriginMs(meta, stepMetas) {
  if (hasMetricValue(meta?.started_at_ms)) return Number(meta.started_at_ms);
  const timestamps = (stepMetas || []).map(step => step?.timestamp_ms).filter(hasMetricValue).map(Number);
  return timestamps.length ? Math.min(...timestamps) : 0;
}
function timelineStepStartMs(meta, sm, fallback) {
  if (hasMetricValue(sm?.timestamp_ms)) return Number(sm.timestamp_ms);
  if (hasMetricValue(meta?.started_at_ms) && hasMetricValue(sm?.elapsed_ms)) return Number(meta.started_at_ms) + Number(sm.elapsed_ms);
  return hasMetricValue(fallback) ? Number(fallback) : null;
}
function timelineEndMs(start, duration) {
  if (!hasMetricValue(start)) return null;
  return Number(start) + (hasMetricValue(duration) ? Number(duration) : 0);
}
function timelineToolLabel(tool, toolMeta) {
  return String(tool?.function_name || toolMeta?.title || tool?.tool_call_id || "tool");
}
function timelineStepRef(stepId) {
  return `step ${stepId}`;
}
function timelineToolRef(stepId, tool, toolIndex) {
  return `step ${stepId} / ${tool?.tool_call_id || `tool ${toolIndex + 1}`}`;
}
function timelineDurationMs(value) {
  return hasMetricValue(value) ? Math.max(0, Number(value)) : null;
}
function timelineToolCategory(tool, toolMeta) {
  if (toolFailed(toolMeta)) return "error";
  const text = lower(`${tool?.function_name || ""} ${toolMeta?.title || ""}`);
  if (/(search|http|web|fetch|browser|mcp|curl|wget|request)/.test(text)) return "network";
  if (/(shell|terminal|exec|python|query|task|bash|sh|cmd|command|subprocess)/.test(text)) return "external";
  if (/(file|read|write|glob|grep|list|open|cat|sed|rg|ls|path|fs)/.test(text)) return "io";
  return "tool";
}
function timelineCategoryMeta(key) {
  const items = {
    io: { key: "io", label: t("timeline_category_io", "I/O"), color: "#3b82f6" },
    agent: { key: "agent", label: t("timeline_category_agent", "Agent"), color: "#7c3aed" },
    network: { key: "network", label: t("timeline_category_network", "Network"), color: "#f59e0b" },
    external: { key: "external", label: t("timeline_category_external", "External"), color: "#64748b" },
    tool: { key: "tool", label: t("timeline_category_tool", "Tool"), color: "#0891b2" },
    error: { key: "error", label: t("timeline_category_error", "Error"), color: "#dc2626" },
  };
  return items[key] || items.tool;
}
function timelineStageSort(left, right) {
  return Number(left.start_offset_ms || 0) - Number(right.start_offset_ms || 0)
    || Number(right.duration_ms || 0) - Number(left.duration_ms || 0)
    || String(left.stage || "").localeCompare(String(right.stage || ""));
}
function timelineModel(stages) {
  const activeTotal = stages.reduce((sum, stage) => sum + Math.max(0, Number(stage.duration_ms || 0)), 0);
  return {
    active_total_ms: activeTotal,
    display_total_ms: Math.max(1, activeTotal),
  };
}
function timelineChartOption(trace) {
  const labels = trace.stages.map(stage => `#${stage.number} ${stage.stage}`);
  const labelWidth = timelineYAxisLabelWidth(labels);
  const xAxisScale = timelineXAxisScale(trace.model.display_total_ms);
  return {
    animation: false,
    grid: { left: labelWidth + 18, right: 28, top: 38, bottom: 48 },
    tooltip: {
      trigger: "item",
      confine: true,
      borderWidth: 1,
      borderColor: "#d5cdbb",
      backgroundColor: "#fffdf8",
      textStyle: { color: "#27231b", fontFamily: "system-ui,-apple-system,BlinkMacSystemFont,\"Segoe UI\",sans-serif", fontSize: 12 },
      formatter: timelineTooltipFormatter,
    },
    xAxis: {
      type: "value",
      min: 0,
      max: xAxisScale.max,
      interval: xAxisScale.interval,
      minInterval: xAxisScale.interval,
      axisLine: { lineStyle: { color: "#d9e2ef" } },
      axisTick: { show: true, lineStyle: { color: "#d9e2ef" } },
      axisLabel: {
        color: "#6b7280",
        fontSize: 12,
        hideOverlap: true,
        formatter: value => fmtTimelineAxis(value, xAxisScale.interval),
      },
      splitLine: { show: true, lineStyle: { color: "#edf2f7" } },
    },
    yAxis: {
      type: "category",
      inverse: true,
      data: labels,
      axisTick: { show: false },
      axisLine: { lineStyle: { color: "#d9e2ef" } },
      axisLabel: {
        color: "#526581",
        fontSize: 12,
        fontFamily: "system-ui,-apple-system,BlinkMacSystemFont,\"Segoe UI\",sans-serif",
        width: labelWidth,
        overflow: "truncate",
      },
      splitLine: { show: true, lineStyle: { color: "#f6f1e8" } },
    },
    series: [
      {
        name: t("timeline_waterfall", "Timeline Waterfall"),
        type: "custom",
        renderItem: timelineBarRenderItem,
        cursor: "pointer",
        encode: { x: [0, 1], y: 2 },
        data: trace.stages.map((stage, index) => ({
          value: [stage.display_start_ms, stage.display_end_ms, index, stage.duration_ms, stage.category_meta.color, fmtTimelineDuration(stage.duration_ms)],
          itemStyle: { color: stage.category_meta.color },
          trace_item: stage,
        })),
      },
      {
        name: t("timeline_markers", "Markers"),
        type: "custom",
        renderItem: timelineMarkerRenderItem,
        cursor: "pointer",
        data: trace.markers.map(marker => ({
          value: [marker.display_offset_ms, 0],
          trace_item: marker,
        })),
      },
    ],
  };
}
function timelineYAxisLabelWidth(labels) {
  const longest = labels.reduce((max, label) => Math.max(max, String(label || "").length), 0);
  return Math.max(100, Math.min(158, longest * 7 + 16));
}
function timelineXAxisScale(totalMs) {
  const total = Math.max(1, Number(totalMs || 0));
  const interval = timelineNiceIntervalMs(total / 5);
  return {
    interval,
    max: Math.max(interval, Math.ceil(total / interval) * interval),
  };
}
function timelineNiceIntervalMs(targetMs) {
  const target = Math.max(1, Number(targetMs || 0));
  const magnitude = Math.pow(10, Math.floor(Math.log10(target)));
  for (const factor of [1, 2, 2.5, 5, 10]) {
    const interval = factor * magnitude;
    if (interval >= target) return Math.max(1, interval);
  }
  return Math.max(1, 10 * magnitude);
}
function timelineBarRenderItem(params, api) {
  const start = api.coord([api.value(0), api.value(2)]);
  const end = api.coord([api.value(1), api.value(2)]);
  const bandHeight = api.size([0, 1])[1];
  const barHeight = Math.max(7, Math.min(22, bandHeight * 0.62));
  const color = api.value(4) || "#0891b2";
  const shape = window.echarts.graphic.clipRectByRect({
    x: start[0],
    y: start[1] - barHeight / 2,
    width: Math.max(2, end[0] - start[0]),
    height: barHeight,
  }, {
    x: params.coordSys.x,
    y: params.coordSys.y,
    width: params.coordSys.width,
    height: params.coordSys.height,
  });
  if (!shape) return null;
  const label = api.value(5) || fmtTimelineDuration(api.value(3));
  const labelInside = shape.width >= 58;
  const chartRight = params.coordSys.x + params.coordSys.width;
  const labelOutsideRight = !labelInside && shape.x + shape.width + 58 >= chartRight;
  const labelX = labelInside
    ? shape.x + Math.min(8, Math.max(3, shape.width / 4))
    : (labelOutsideRight ? chartRight - 4 : shape.x + shape.width + 6);
  const textAlign = labelInside || !labelOutsideRight ? "left" : "right";
  return {
    type: "group",
    children: [
      {
        type: "rect",
        shape,
        style: api.style({ fill: color }),
      },
      {
        type: "text",
        style: {
          x: labelX,
          y: shape.y + shape.height / 2,
          text: label,
          fill: labelInside ? "#fffdf8" : color,
          font: "700 12px ui-monospace,SFMono-Regular,Menlo,Consolas,\"Liberation Mono\",monospace",
          textAlign,
          textVerticalAlign: "middle",
        },
        silent: true,
      },
    ],
  };
}
function timelineMarkerRenderItem(params, api) {
  const x = api.coord([api.value(0), 0])[0];
  const top = params.coordSys.y;
  const bottom = top + params.coordSys.height;
  return {
    type: "group",
    children: [
      { type: "line", shape: { x1: x, y1: top, x2: x, y2: bottom }, style: { stroke: "#315f8f", lineDash: [4, 4], opacity: 0.36, lineWidth: 1 } },
      { type: "circle", shape: { cx: x, cy: top + 10, r: 4 }, style: { fill: "#fffdf8", stroke: "#315f8f", lineWidth: 2 } },
    ],
  };
}
function timelineTooltipFormatter(params) {
  const param = Array.isArray(params) ? params[0] : params;
  return timelineTooltipHtml(param?.data?.trace_item);
}
function timelineTooltipHtml(item) {
  if (!item) return "";
  const isMarker = item.kind === "marker";
  const title = isMarker ? item.name : `#${item.number} ${item.stage}`;
  const pct = !isMarker && positiveMetric(item.duration_ms) && positiveMetric(item.active_total_ms)
    ? `${(Number(item.duration_ms) / Number(item.active_total_ms) * 100).toFixed(1)}%`
    : "-";
  const rows = [
    [t("timeline_col_category", "Category"), item.category_meta?.label || "-"],
    [t("timeline_col_start", "Start"), fmtTimelineMaybeEstimated(fmtClockMs(item.wall_start_ms), item)],
    [t("timeline_active_offset", "Active offset"), fmtTimelineDuration(item.display_offset_ms ?? item.display_start_ms)],
    [t("timeline_ref", "Ref"), item.ref || "-"],
  ];
  if (!isMarker) {
    rows.splice(2, 0, [t("timeline_col_end", "End"), fmtTimelineMaybeEstimated(fmtClockMs(item.wall_end_ms), item)]);
    rows.splice(4, 0, [t("timeline_col_duration", "Duration"), fmtTimelineMaybeEstimated(fmtTimelineDuration(item.duration_ms), item)]);
    rows.splice(5, 0, [t("timeline_col_total_pct", "Active Share"), pct]);
  }
  return `<div class="timeline-tooltip"><strong>${esc(title)}</strong>${rows.map(([key, value]) => `<br><span>${esc(key)}:</span> ${esc(value)}`).join("")}</div>`;
}
function openTimelineStep(item) {
  if (!item || !item.step_id) return;
  state.selectedTrial = item.trial_key || selectedKey();
  state.selectedStep = { trialKey: state.selectedTrial, stepId: String(item.step_id) };
  renderComparisonPanels();
}
function bindTimelineControls() {
  const target = document.querySelector(".timeline-diagnostics");
  if (!target) return;
  bindDataTableControls(target, "timeline", () => renderTrace());
  target.querySelectorAll("[data-timeline-step-id]").forEach(row => {
    const open = event => {
      event.stopPropagation();
      openTimelineStep({
        kind: "stage",
        trial_key: row.dataset.trialKey || selectedKey(),
        step_id: row.dataset.timelineStepId,
      });
    };
    row.addEventListener("click", open);
    row.addEventListener("keydown", event => {
      if (event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault();
      open(event);
    });
  });
}
function timelineActivePctValue(row, model) {
  return hasMetricValue(row.duration_ms) && positiveMetric(model.active_total_ms)
    ? Number(row.duration_ms) / Number(model.active_total_ms) * 100
    : null;
}
function renderTimelineActiveShare(row, model) {
  const value = timelineActivePctValue(row, model);
  if (!hasMetricValue(value)) return "-";
  const pct = Math.max(0, Math.min(100, Number(value)));
  const label = `${Number(value).toFixed(1)}%`;
  return `<span class="timeline-active-share" style="--active-share-pct:${esc(`${pct}%`)}" title="${esc(label)}"><span>${esc(label)}</span></span>`;
}
function timelineDetailColumns(model) {
  return [
    { key: "number", label: t("timeline_col_row", "#"), type: "number", numeric: true, sortable: true, value: row => Number(row.number), format: value => fmtNum(value) },
    { key: "stage", label: t("timeline_col_stage", "Stage"), sortable: true, filterable: true, value: row => row.stage || "-", html: row => renderTimelineStageLabel(row), cellTitle: row => row.stage || "-", className: "timeline-label-cell" },
    { key: "wall_start_ms", label: t("timeline_col_start", "Start"), type: "number", numeric: true, sortable: true, value: row => row.wall_start_ms, format: (value, row) => fmtTimelineMaybeEstimated(fmtClockMs(value), row) },
    { key: "wall_end_ms", label: t("timeline_col_end", "End"), type: "number", numeric: true, sortable: true, value: row => row.wall_end_ms, format: (value, row) => fmtTimelineMaybeEstimated(fmtClockMs(value), row) },
    { key: "duration_ms", label: t("timeline_col_duration", "Duration"), type: "number", numeric: true, sortable: true, metric: true, value: row => row.duration_ms, format: (value, row) => fmtTimelineMaybeEstimated(fmtTimelineDuration(value), row), className: "strong-num" },
    { key: "active_pct", label: t("timeline_col_total_pct", "Active Share"), type: "number", numeric: true, sortable: true, metric: true, value: row => timelineActivePctValue(row, model), format: value => hasMetricValue(value) ? `${Number(value).toFixed(1)}%` : "-", html: row => renderTimelineActiveShare(row, model), className: "active-share-cell" },
  ];
}
function renderTimelineDetailTable(rows, model) {
  const columns = timelineDetailColumns(model);
  const visibleRows = applyDataTableControls("timeline", rows, columns, rows);
  return renderDataTable({
    tableId: "timeline",
    columns,
    rows: visibleRows,
    tableClass: "timeline-table",
    shellClass: "timeline-table-shell",
    filterOptionsRows: rows,
    rowClass: row => {
      const selected = state.selectedStep?.trialKey === row.trial_key && String(state.selectedStep?.stepId) === String(row.step_id);
      return `timeline-detail-${row.kind} timeline-detail-row ${selected ? "timeline-detail-selected" : ""}`;
    },
    rowAttrs: row => `data-timeline-step-id="${esc(row.step_id || "")}" data-trial-key="${esc(row.trial_key || "")}" tabindex="0" title="${esc(t("open_step_details", "Open step details"))}: #${esc(row.step_id || "-")}"`,
  });
}
function renderTimelineStageLabel(row) {
  const meta = row.category_meta || timelineCategoryMeta("tool");
  return `<span class="timeline-stage-label timeline-category-${esc(meta.key)}" aria-label="${esc(`${meta.label}: ${row.stage}`)}">${esc(row.stage)}</span>`;
}
function fmtTimelineDuration(value) {
  if (!hasMetricValue(value)) return "-";
  const ms = Math.max(0, Number(value));
  const seconds = ms / 1000;
  if (seconds < 60) return `${seconds.toFixed(3)}s`;
  return `${Math.floor(seconds / 60)}m${(seconds % 60).toFixed(1)}s`;
}
function fmtTimelineMaybeEstimated(value, row) {
  if (!row?.estimated || value === "-") return value;
  return `≈${value}`;
}
function fmtTimelineAxis(value, intervalMs = null) {
  const ms = Math.max(0, Number(value || 0));
  const interval = hasMetricValue(intervalMs) ? Math.max(1, Number(intervalMs)) : null;
  if (ms === 0) return interval && interval < 1000 ? "0ms" : "0s";
  if (interval && interval < 1000) return `${Math.round(ms)}ms`;
  const seconds = ms / 1000;
  if (seconds < 60) {
    const decimals = interval && interval < 10000 ? 1 : 0;
    return `${seconds.toFixed(decimals)}s`;
  }
  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;
  if (interval && interval < 60000 && remainder) return `${minutes}m${Math.round(remainder)}s`;
  return `${minutes}m`;
}
function fmtClockMs(value) {
  if (!hasMetricValue(value)) return "-";
  const date = new Date(Number(value));
  const pad = (number, size = 2) => String(number).padStart(size, "0");
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}.${pad(date.getMilliseconds(), 3)}`;
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
function renderStep(step, meta, timingStats, options = {}) {
  const sm = stepMeta(meta, step.step_id);
  const preview = stepPreviewText(step) || "(No Message)";
  return `<details class="step" data-step="${esc(step.step_id)}"${options.open ? " open" : ""}><summary><div class="step-row"><span class="step-id">#${esc(step.step_id)}</span><span class="role ${esc(step.source)}">${esc(step.source)}</span><span class="preview">${esc(preview)}</span></div><div class="rail">${renderStepRail(step, sm, meta?.trial_key, timingStats)}</div></summary><div class="step-body">${renderBlocks(step, sm, timingStats)}</div></details>`;
}
function renderBlocks(step, meta, timingStats) {
  let html = "";
  if (step.reasoning_content) html += block("Reasoning", step.reasoning_content, "reasoning-block");
  const message = valuePreview(step.message);
  if (message.trim()) html += block(step.source === "system" ? "System Prompt" : "Message", message, "message-block");
  html += renderStepActivityBlocks(step, meta, timingStats);
  return html || `<p class="copy">No visible content.</p>`;
}
function renderStepActivityBlocks(step, meta, timingStats) {
  const entries = [];
  (step.tool_calls || []).forEach(tool => {
    const toolMeta = toolMetaFor(meta, tool.tool_call_id);
    entries.push({
      timestamp: blockTimestamp(toolMeta?.timestamp_ms),
      order: entries.length,
      html: renderToolCallBlock(tool, toolMeta, timingStats),
    });
  });
  ((step.observation && step.observation.results) || []).forEach(observation => {
    const observationMeta = observationMetaFor(meta, observation.source_call_id);
    entries.push({
      timestamp: blockTimestamp(observationMeta?.timestamp_ms),
      order: entries.length,
      html: renderObservationBlock(observation, observationMeta),
    });
  });
  return entries.sort(compareStepActivityBlocks).map(entry => entry.html).join("");
}
function renderToolCallBlock(tool, toolMeta, timingStats) {
  const ratio = timingRatio(toolMeta?.execution_duration_ms, timingStats?.maxToolExecutionMs);
  return `<div class="block tool-block"><h4>Tool Calls</h4><p>${renderToolNameChip(tool, toolMeta, "", ratio)} <span class="muted">ID: ${esc(tool.tool_call_id || "-")}${toolMeta?.status ? ` / ${esc(toolMeta.status)}` : ""}${renderToolTiming(toolMeta)}</span></p><pre>${esc(valuePreview(tool.arguments || {}))}</pre></div>`;
}
function renderObservationBlock(observation, observationMeta) {
  return `<div class="block observation-block"><h4 class="${observationMeta?.tool_error ? "danger" : ""}">Observations</h4><p class="muted">Result for: ${esc(observation.source_call_id || "-")}${observationMeta?.status ? ` / ${esc(observationMeta.status)}` : ""}</p><pre>${esc(valuePreview(observation.content))}</pre></div>`;
}
function compareStepActivityBlocks(a, b) {
  if (a.timestamp !== null && b.timestamp !== null && a.timestamp !== b.timestamp) return a.timestamp - b.timestamp;
  if (a.timestamp !== null && b.timestamp === null) return -1;
  if (a.timestamp === null && b.timestamp !== null) return 1;
  return a.order - b.order;
}
function blockTimestamp(value) {
  return hasMetricValue(value) ? Number(value) : null;
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
  const toolErrors = toolErrorCount(meta);
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
function toolErrorCount(meta) {
  return listValue(meta?.tool_calls).filter(tool => lower(tool?.status) === "error").length;
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
  const values = [metrics.prompt_tokens, metrics.completion_tokens].filter(hasMetricValue).map(Number);
  if (values.length) return values.reduce((sum, value) => sum + value, 0);
  const direct = metricExtra(metrics).usage?.total_tokens;
  return hasMetricValue(direct) ? Number(direct) : null;
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
  const sourcePath = row?.source_ref?.relative_path;
  const meta = row?.label || sourcePath
    ? `<div class="note-meta"><strong>${esc(row?.label || t("notes", "Notes"))}</strong>${sourcePath ? `<code>${esc(sourcePath)}</code>` : ""}</div>`
    : "";
  return `<article class="manual-note">${meta}<div class="note-body">${renderMarkdown(row.markdown || "")}</div></article>`;
}
function renderMarkdown(markdown) {
  const lines = String(markdown ?? "").split(/\r?\n/);
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
      out.push(`<pre class="note-code">${esc(code.join("\n"))}</pre>`);
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
    const heading = line.match(/^#{1,4}\s+(.+)$/);
    if (heading) {
      flushParagraph();
      flushList();
      out.push(`<h4>${inlineMarkdown(heading[1])}</h4>`);
      continue;
    }
    const bullet = line.match(/^[-*]\s+(.+)$/);
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
