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
      rowClass: source => ["source-table-row", source?.active === false ? "archived" : "", source?.last_status === "missing" ? "missing" : "", source?.source_key && source.source_key === state.selectedSourceKey ? "selected-row" : ""].filter(Boolean).join(" "),
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
    showImportResultsSummary(payload);
  } catch (error) {
    showServeNotice(`${t("serve_import_failed", "Import failed")}: ${error.message || String(error)}`, true);
    setServeStatus(error.message || String(error), true);
  }
}
function showImportResultsSummary(payload) {
  const results = Array.isArray(payload?.import_results) ? payload.import_results : [];
  if (!results.length) return;
  const imported = results.filter(result => result?.status === "ok").length;
  const failed = results.filter(result => result?.status === "error").length;
  const template = t("serve_import_summary", "Imported {imported}, failed {failed}");
  const message = template.replace("{imported}", String(imported)).replace("{failed}", String(failed));
  showServeNotice(message, failed > 0);
  setServeStatus(message, failed > 0);
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
