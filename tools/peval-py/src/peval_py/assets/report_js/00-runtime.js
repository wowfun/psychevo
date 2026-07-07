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
const state = { view: null, selectedTrial: null, selectedStep: null, rowSelection: new Set(), tables: {}, timelineChart: null, boundGlobalControls: false, serveSources: Array.isArray(RENDER_OPTIONS?.sources) ? RENDER_OPTIONS.sources : [], selectedSourceKey: null, serveSourceMode: "active", serveReportCache: {}, adapterDefaults: initialAdapterDefaults(), notesEditor: null, search: { query: "", scope: "visible", normalSourceMode: "active" }, serveLoading: Boolean(RENDER_OPTIONS?.loading), serveStartupPolling: false };
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
  if (metas.length >= 1) {
    return metas
      .map((meta, index) => synthesizedReportRow(trajectories[index] || {}, meta, index))
      .filter(row => row.trial_key);
  }
  return [];
}
function synthesizedReportRow(trajectory, meta, index = -1) {
  const metrics = trajectory?.final_metrics || {};
  const totalToolCalls = hasMetricValue(finalMetric(metrics, "total_tool_calls")) ? Number(finalMetric(metrics, "total_tool_calls")) : 0;
  const totalToolErrors = hasMetricValue(finalMetric(metrics, "total_tool_errors")) ? Number(finalMetric(metrics, "total_tool_errors")) : 0;
  const agent = trajectory?.agent || {};
  const source = serveMode() ? sourceForTrialIndex(index) : null;
  return {
    trial_key: meta?.trial_key,
    session_id: trajectory?.session_id || "-",
    source_alias: meta?.source_alias,
    source_tags: sourceTagsForMeta(meta, source),
    source_key: source?.source_key || null,
    source_active: source?.active !== false,
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
  if (serveMode()) state.serveReportCache[currentServeSourceMode()] = view;
  if (!state.selectedTrial) {
    const firstFailed = reportRows().find(row => lower(row.status) !== "passed");
    state.selectedTrial = (firstFailed || reportRows()[0])?.trial_key || view.trajectory_meta?.[0]?.trial_key || null;
  }
  if (serveMode()) syncSelectedSourceFromView();
  if (serveMode()) renderServeSources();
  bindGlobalControls();
  if (serveMode()) scheduleServeStartupPoll();
  renderReportNotes(view.annotations?.report_notes || []);
  renderComparison();
  renderTrace();
  renderStepDrawer();
}
function syncSelectedSourceFromView() {
  const trialKey = selectedKey();
  if (!trialKey) return;
  const source = sourceForTrialKey(trialKey);
  if (source?.source_key) state.selectedSourceKey = source.source_key;
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
    ${rows.length > 1 ? `<section class="leaderboard-summary panel" aria-labelledby="leaderboard-summary-title" id="leaderboard-summary"></section>` : ""}
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
function analysisArtifactPathsFor(trialKey) {
  const analysis = analysisFor(trialKey);
  if (!analysis) return [];
  const paths = [];
  const relativePaths = analysis.relative_paths || {};
  if (typeof relativePaths === "object") {
    ["md", "json"].forEach(key => {
      if (typeof relativePaths[key] === "string") paths.push(relativePaths[key]);
    });
  }
  if (typeof analysis.relative_path === "string") paths.push(analysis.relative_path);
  return paths;
}
function isAnalysisArtifactPath(path) {
  const normalized = String(path || "").replace(/\\/g, "/");
  return normalized === "analysis.md" || normalized === "analysis.json" || normalized.endsWith("/analysis.md") || normalized.endsWith("/analysis.json");
}
function rowAnalysised(row) {
  return analysisArtifactPathsFor(row?.trial_key).some(isAnalysisArtifactPath) ? "True" : "False";
}
function normalizeServeSourceMode(mode) {
  if (mode === "all") return "all";
  return mode === "archived" ? "archived" : "active";
}
function currentServeSourceMode() {
  return normalizeServeSourceMode(state.serveSourceMode);
}
function serveSourcesForMode(mode = currentServeSourceMode()) {
  return serveSourcesForModeFrom(state.serveSources, mode);
}
function serveSourcesForModeFrom(sources, mode = currentServeSourceMode()) {
  const sourceMode = normalizeServeSourceMode(mode);
  if (sourceMode === "all") return Array.isArray(sources) ? sources : [];
  return (Array.isArray(sources) ? sources : []).filter(source => {
    const active = source?.active !== false;
    return sourceMode === "archived" ? !active : active;
  });
}
function activeServeSources() {
  return serveSourcesForMode("active");
}
function readableServeSources(mode = currentServeSourceMode()) {
  return readableServeSourcesFrom(state.serveSources, mode);
}
function readableServeSourcesFrom(sources, mode = currentServeSourceMode()) {
  return serveSourcesForModeFrom(sources, mode).filter(source => source?.source_key && source?.artifact_dir && source?.last_status !== "missing");
}
function trialIndexForView(trialKey, view = state.view) {
  const metas = listValue(view?.trajectory_meta);
  return metas.findIndex(meta => meta?.trial_key === trialKey);
}
function sourceForTrialIndex(index, mode = currentServeSourceMode()) {
  return index >= 0 ? readableServeSources(mode)[index] || null : null;
}
function sourceKeyForTrialKey(trialKey, view = state.view) {
  return sourceForTrialKey(trialKey, view)?.source_key || null;
}
function trialKeyForServeSource(sourceKey, view = state.view, mode = currentServeSourceMode()) {
  if (!serveMode() || !sourceKey) return null;
  const index = readableServeSources(mode).findIndex(source => source?.source_key === sourceKey);
  return index >= 0 ? listValue(view?.trajectory_meta)[index]?.trial_key || null : null;
}
function sourceForTrialKey(trialKey, view = state.view, mode = currentServeSourceMode()) {
  if (!serveMode()) return null;
  const index = trialIndexForView(trialKey, view);
  const byIndex = sourceForTrialIndex(index, mode);
  if (byIndex) return byIndex;
  if (listValue(view?.trajectory_meta).length <= 1 && state.selectedSourceKey) {
    const selected = readableServeSources(mode).find(source => source?.source_key === state.selectedSourceKey);
    if (selected) return selected;
  }
  return readableServeSources(mode).find(source => source?.trial_key === trialKey) || null;
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
  return renderEditableSourceCell(row, "alias", alias, alias ? esc(alias) : `<span class="muted">-</span>`);
}
function sourceTagsForMeta(meta, source = null) {
  const rawTags = listValue(meta?.source_tags).length ? meta.source_tags : source?.source_tags;
  return sourceTagsFromValue(rawTags);
}
function sourceTagsFromValue(value) {
  const tags = [];
  const seen = new Set();
  listValue(value).forEach(rawTag => {
    const tag = String(rawTag || "").trim();
    if (!tag || seen.has(tag)) return;
    seen.add(tag);
    tags.push(tag);
  });
  return tags;
}
function sourceTagsFor(row) {
  return sourceTagsFromValue(row?.source_tags);
}
function sourceTagsValue(row) {
  return sourceTagsFor(row).join(", ") || "-";
}
function sourceTagsEditValue(row) {
  return sourceTagsFor(row).join(", ");
}
function renderSourceTagsCell(row) {
  const tags = sourceTagsFor(row);
  const html = tags.length
    ? `<span class="source-tag-list">${tags.map(tag => `<span class="source-tag-chip">${esc(tag)}</span>`).join("")}</span>`
    : `<span class="muted">-</span>`;
  return renderEditableSourceCell(row, "tags", sourceTagsEditValue(row), html);
}
function renderEditableSourceCell(row, field, value, html) {
  if (!serveMode() || !row?.source_key) return html;
  return `<span class="editable-source-cell" data-source-inline-edit="${esc(field)}" data-source-key="${esc(row.source_key)}" data-trial-key="${esc(row.trial_key || "")}" data-value="${esc(value || "")}" title="${esc(t("double_click_to_edit", "Double-click to edit"))}">${html}</span>`;
}
function searchQuery() {
  return String(state.search?.query || "").trim().toLowerCase();
}
function searchScope() {
  return state.search?.scope === "all" ? "all" : "visible";
}
function allSearchActive() {
  return serveMode() && searchScope() === "all" && Boolean(searchQuery());
}
function applySessionSearch(rows) {
  const query = searchQuery();
  if (!query) return rows;
  return rows.filter(row => sessionSearchText(row).includes(query));
}
function sessionSearchText(row) {
  const trajectory = trajectoryFor(row?.trial_key);
  const meta = metaFor(row?.trial_key);
  const parts = [];
  listValue(trajectory?.steps).forEach(step => {
    parts.push(step?.message, step?.reasoning_content);
    parts.push(searchJson(step?.tool_calls), searchJson(step?.observation), searchJson(step?.observations));
  });
  listValue(meta?.steps).forEach(step => {
    parts.push(searchJson(step?.tool_calls), searchJson(step?.observations));
  });
  return parts.filter(value => value !== null && value !== undefined).join("\n").replace(/\s+/g, " ").toLowerCase();
}
function searchJson(value) {
  if (value === null || value === undefined) return "";
  try {
    return typeof value === "string" ? value : JSON.stringify(value);
  } catch {
    return String(value || "");
  }
}
function renderComparisonPanels(options = {}) {
  const scrollState = options.preserveScroll === false ? null : comparisonScrollState();
  const rows = leaderboardRows();
  syncSelectionWithVisibleRows(rows);
  renderLeaderboard(rows);
  if (rows.length > 1 && $("leaderboard-summary")) renderLeaderboardSummary(rows);
  else if ($("leaderboard-summary")) $("leaderboard-summary").innerHTML = "";
  renderTrajectoryOverview(rows);
  restoreComparisonScrollState(scrollState);
  bindComparisonScrollSync();
  if (options.trace !== false) renderTrace();
  renderStepDrawer();
}
function comparisonScrollState() {
  return {
    leaderboard: scrollPosition("#leaderboard .table-wrap", true),
    trajectoryOverview: scrollPosition("#trajectory-overview .trajectory-overview-list", false)
  };
}
function scrollPosition(selector, includeHorizontal) {
  const node = document.querySelector(selector);
  if (!node) return null;
  const position = { top: node.scrollTop || 0 };
  if (includeHorizontal) position.left = node.scrollLeft || 0;
  return position;
}
function restoreComparisonScrollState(state) {
  if (!state) return;
  restoreScrollPosition("#leaderboard .table-wrap", state.leaderboard);
  restoreScrollPosition("#trajectory-overview .trajectory-overview-list", state.trajectoryOverview);
}
function restoreScrollPosition(selector, position) {
  if (!position) return;
  const node = document.querySelector(selector);
  if (!node) return;
  node.scrollTop = position.top || 0;
  if (Object.prototype.hasOwnProperty.call(position, "left")) {
    node.scrollLeft = position.left || 0;
  }
}
function bindComparisonScrollSync() {
  const leaderboard = document.querySelector("#leaderboard .table-wrap");
  const overview = document.querySelector("#trajectory-overview .trajectory-overview-list");
  if (!leaderboard || !overview) return;
  leaderboard.addEventListener("scroll", () => syncComparisonScroll(leaderboard, overview), { passive: true });
  overview.addEventListener("scroll", () => syncComparisonScroll(overview, leaderboard), { passive: true });
}
function syncComparisonScroll(source, target) {
  if (state.comparisonScrollSyncing) return;
  const sourceRange = scrollRange(source);
  const targetRange = scrollRange(target);
  if (sourceRange <= 0 || targetRange <= 0) return;
  const targetTop = scrollProgress(source, sourceRange) * targetRange;
  state.comparisonScrollSyncing = true;
  const apply = () => {
    target.scrollTop = targetTop;
    const release = () => {
      state.comparisonScrollSyncing = false;
    };
    if (typeof requestAnimationFrame === "function") requestAnimationFrame(release);
    else release();
  };
  if (typeof requestAnimationFrame === "function") requestAnimationFrame(apply);
  else apply();
}
function scrollRange(node) {
  return Math.max(0, (node.scrollHeight || 0) - (node.clientHeight || 0));
}
function scrollProgress(node, range = scrollRange(node)) {
  if (range <= 0) return 0;
  return Math.max(0, Math.min(1, (node.scrollTop || 0) / range));
}
