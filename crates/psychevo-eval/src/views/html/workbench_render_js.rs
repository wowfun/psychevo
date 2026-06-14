fn workbench_js_rendering() -> &'static str {
    r###"
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
