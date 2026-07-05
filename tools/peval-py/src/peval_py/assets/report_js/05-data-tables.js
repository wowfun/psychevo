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
    { key: "analysised", label: t("analysised", "Analysised"), width: "112px", filterable: true, value: row => rowAnalysised(row) },
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
    <div class="panel-head"><div><h2 id="leaderboard-title">${esc(t("leaderboard", "Leaderboard"))}</h2><p class="copy">${esc(t("leaderboard_copy", "Each row is one visible session-as-Trial. Numeric cells shade by column value; rows update the selected Trial."))}</p></div>${renderLeaderboardPanelControls(rows)}</div>
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
function renderLeaderboardPanelControls(rows) {
  if (!serveMode()) return "";
  return `<div class="leaderboard-actions">${renderServeSourceStateControls(rows)}${renderLeaderboardExportControls()}</div>`;
}
function renderLeaderboardExportControls() {
  if (!serveMode()) return "";
  return `<div class="leaderboard-export" data-serve-only>
    <details class="export-menu">
      <summary class="export-menu-button" aria-label="${esc(t("export_options", "Export options"))}">${esc(t("export", "Export"))}</summary>
      <div class="export-menu-panel">
        <button type="button" data-export-kind="xlsx">${esc(t("export_xlsx_table", "Table (.xlsx)"))}</button>
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
  bindServeSourceStateControls(target);
  bindServeExportControls(target);
  bindTrialSelection(target);
}
