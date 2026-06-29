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
