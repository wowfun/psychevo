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
      body: { alias, report_source_state: currentServeSourceMode() }
    });
    applyServeMutationPayload(payload, { preserveTrial: selectedKey() });
  } catch (error) {
    setServeStatus(error.message || String(error), true);
  }
}
function bindLeaderboardSearchControls(target) {
  if (!serveMode() || !target) return;
  const input = target.querySelector("[data-leaderboard-search-input]");
  if (input) {
    input.addEventListener("click", event => event.stopPropagation());
    input.addEventListener("input", event => {
      event.stopPropagation();
      state.search.query = String(input.value || "");
      applyLeaderboardSearchMode();
    });
  }
  target.querySelectorAll("input[name=\"leaderboard-search-scope\"]").forEach(control => {
    control.addEventListener("click", event => event.stopPropagation());
    control.addEventListener("change", event => {
      event.stopPropagation();
      state.search.scope = control.value === "all" ? "all" : "visible";
      applyLeaderboardSearchMode();
    });
  });
}
async function applyLeaderboardSearchMode() {
  if (!serveMode()) {
    renderComparisonPanels();
    return;
  }
  if (allSearchActive()) {
    if (currentServeSourceMode() !== "all") {
      state.search.normalSourceMode = currentServeSourceMode();
    }
    const cached = state.serveReportCache?.all;
    if (cached) {
      state.serveSourceMode = "all";
      render(cached);
      setServeStatus(serveSourceModeStatusText("all"));
      focusLeaderboardSearchInput();
      return;
    }
    try {
      const sourceKey = state.selectedSourceKey || sourceForTrialKey(selectedKey())?.source_key;
      const report = await serveApi("/api/report?source_state=all");
      applyServeMutationPayload(
        { report, report_source_key: sourceKey || null, report_source_state: "all" },
        { selectedSourceKey: sourceKey || null }
      );
      focusLeaderboardSearchInput();
    } catch (error) {
      setServeStatus(error.message || String(error), true);
      renderComparisonPanels({ trace: false });
      focusLeaderboardSearchInput();
    }
    return;
  }
  if (currentServeSourceMode() === "all") {
    const restoreMode = normalizeServeSourceMode(state.search.normalSourceMode || "active") === "all"
      ? "active"
      : normalizeServeSourceMode(state.search.normalSourceMode || "active");
    const cached = state.serveReportCache?.[restoreMode];
    if (cached) {
      state.serveSourceMode = restoreMode;
      render(cached);
      setServeStatus(serveSourceModeStatusText(restoreMode));
      focusLeaderboardSearchInput();
      return;
    }
    await switchServeSourceMode(restoreMode);
    focusLeaderboardSearchInput();
    return;
  }
  renderComparisonPanels();
  focusLeaderboardSearchInput();
}
function focusLeaderboardSearchInput() {
  const apply = () => {
    const input = document.querySelector("[data-leaderboard-search-input]");
    if (!input) return;
    input.focus();
    const end = String(input.value || "").length;
    if (typeof input.setSelectionRange === "function") input.setSelectionRange(end, end);
  };
  if (typeof requestAnimationFrame === "function") requestAnimationFrame(apply);
  else apply();
}
function bindInlineSourceEditors(target) {
  if (!serveMode() || !target) return;
  target.querySelectorAll("[data-source-inline-edit]").forEach(cell => {
    cell.addEventListener("click", event => event.stopPropagation());
    cell.addEventListener("dblclick", event => {
      event.preventDefault();
      event.stopPropagation();
      beginInlineSourceEdit(cell);
    });
  });
}
function beginInlineSourceEdit(cell) {
  const sourceKey = cell?.dataset?.sourceKey;
  const field = cell?.dataset?.sourceInlineEdit;
  if (!sourceKey || !field || cell.querySelector("input")) return;
  const input = document.createElement("input");
  input.className = "inline-source-edit";
  input.value = cell.dataset.value || "";
  input.setAttribute("aria-label", field === "tags" ? t("tags", "Tags") : t("session_alias", "Session Alias"));
  const original = cell.innerHTML;
  let finished = false;
  const cancel = () => {
    if (finished) return;
    finished = true;
    cell.innerHTML = original;
    bindInlineSourceEditors(cell);
  };
  const save = async () => {
    if (finished) return;
    finished = true;
    await saveInlineSourceEdit(cell, field, sourceKey, input.value);
  };
  input.addEventListener("click", event => event.stopPropagation());
  input.addEventListener("dblclick", event => event.stopPropagation());
  input.addEventListener("keydown", event => {
    if (event.key === "Escape") {
      event.preventDefault();
      cancel();
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      save();
    }
  });
  input.addEventListener("blur", () => save());
  cell.replaceChildren(input);
  input.focus();
  input.select();
}
async function saveInlineSourceEdit(cell, field, sourceKey, value) {
  const action = field === "tags" ? "tags" : "alias";
  const body = {
    report_source_state: currentServeSourceMode(),
    [action === "tags" ? "tags" : "alias"]: String(value || "").trim()
  };
  try {
    const payload = await serveApi(`/api/sources/${encodeURIComponent(sourceKey)}/${action}`, {
      method: "POST",
      body
    });
    applyServeMutationPayload(payload, { preserveTrial: cell?.dataset?.trialKey || selectedKey(), selectedSourceKey: sourceKey });
  } catch (error) {
    setServeStatus(error.message || String(error), true);
    renderComparisonPanels({ trace: false });
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
    const mode = currentServeSourceMode();
    const sourceKey = state.selectedSourceKey || sourceForTrialKey(selectedKey())?.source_key;
    const payload = options.refresh
      ? await serveApi("/api/refresh", { method: "POST", body: {} })
      : { report: await serveApi(`/api/report?source_state=${encodeURIComponent(mode)}`), report_source_key: sourceKey || null, report_source_state: mode };
    applyServeMutationPayload(payload, { selectedSourceKey: sourceKey || null });
  } catch (error) {
    setServeStatus(error.message || String(error), true);
  }
}
async function refreshServeSourcesFromServer() {
  try {
    const preserveSourceKey = state.selectedSourceKey || sourceForTrialKey(selectedKey())?.source_key;
    const payload = await serveApi("/api/sources/reload", { method: "POST", body: {} });
    applyServeMutationPayload(payload, { selectedSourceKey: preserveSourceKey || null });
  } catch (error) {
    setServeStatus(error.message || String(error), true);
  }
}
function selectServeSource(sourceKey) {
  if (!sourceKey) return;
  const nextSourceKey = readableSourceKey(sourceKey);
  const trialKey = trialKeyForServeSource(nextSourceKey);
  if (!nextSourceKey || !trialKey) return;
  state.selectedSourceKey = nextSourceKey;
  state.selectedTrial = trialKey;
  state.selectedStep = null;
  renderServeSources();
  renderComparisonPanels();
  setServeStatus(t("serve_latest_snapshots", "Latest snapshots"));
}
async function loadServeSourceReport(sourceKey) {
  selectServeSource(sourceKey);
}
function readableSourceKey(preferred = null, mode = currentServeSourceMode()) {
  if (preferred) {
    const match = readableServeSources(mode).find(source => source?.source_key === preferred);
    if (match) return match.source_key;
  }
  return readableServeSources(mode)[0]?.source_key || null;
}
function emptyServeReport() {
  return {
    schema_version: state.view?.schema_version || data()?.schema_version || 19,
    includes: ["core"],
    trajectory: [],
    trajectory_meta: []
  };
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
    const payloadMode = normalizeServeSourceMode(payload.report_source_state || options.sourceMode || currentServeSourceMode());
    if (Array.isArray(payload?.sources)) clearServeReportCacheExcept(payloadMode);
    state.serveSourceMode = payloadMode;
    state.serveReportCache[payloadMode] = payload.report;
    const preservedTrial = options.preserveTrial && reportHasTrialKey(payload.report, options.preserveTrial)
      ? options.preserveTrial
      : null;
    const preservedSourceKey = preservedTrial ? sourceKeyForTrialKey(preservedTrial, payload.report) : null;
    const requestedSourceKey = Object.prototype.hasOwnProperty.call(options, "selectedSourceKey")
      ? options.selectedSourceKey
      : preservedSourceKey || payload.report_source_key || state.selectedSourceKey;
    const nextSourceKey = readableSourceKey(requestedSourceKey, payloadMode) || readableSourceKey(null, payloadMode);
    state.selectedSourceKey = nextSourceKey;
    state.selectedTrial = preservedTrial || trialKeyForServeSource(nextSourceKey, payload.report, payloadMode) || null;
    state.selectedStep = null;
    state.rowSelection.clear();
    render(payload.report);
  }
  setServeStatus(serveSourceModeStatusText());
}
function clearServeReportCacheExcept(mode) {
  const keep = normalizeServeSourceMode(mode);
  state.serveReportCache = Object.fromEntries(
    Object.entries(state.serveReportCache || {}).filter(([key]) => normalizeServeSourceMode(key) === keep)
  );
}
function reportHasTrialKey(report, trialKey) {
  return Boolean(trialKey) && listValue(report?.trajectory_meta).some(meta => meta?.trial_key === trialKey);
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
