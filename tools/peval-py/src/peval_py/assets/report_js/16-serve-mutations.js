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
