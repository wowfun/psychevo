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
      if (button) {
        event.preventDefault();
        mutateServeSource(button.dataset.sourceKey, button.dataset.sourceAction);
        return;
      }
      if (event.target?.closest?.("button,input,select,textarea,label")) return;
      const row = event.target?.closest?.("[data-source-row]");
      const sourceKey = row?.dataset?.sourceKey;
      if (!sourceKey) return;
      event.preventDefault();
      loadServeSourceReport(sourceKey);
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
    const sourceKey = state.selectedSourceKey || sourceForTrialKey(selectedKey())?.source_key;
    const payload = options.refresh
      ? await serveApi("/api/refresh", { method: "POST", body: {} })
      : sourceKey
        ? { report: await serveApi(`/api/report?source_key=${encodeURIComponent(sourceKey)}`), report_source_key: sourceKey }
        : { report: await serveApi("/api/report") };
    applyServeMutationPayload(payload);
    if (options.refresh && !payload?.report && sourceKey) {
      await loadServeSourceReport(sourceKey);
    }
  } catch (error) {
    setServeStatus(error.message || String(error), true);
  }
}
async function refreshServeSourcesFromServer() {
  try {
    const preserveSourceKey = state.selectedSourceKey || sourceForTrialKey(selectedKey())?.source_key;
    const payload = await serveApi("/api/sources/reload", { method: "POST", body: {} });
    if (Array.isArray(payload.sources)) {
      state.serveSources = payload.sources;
      renderServeSources();
      setServeStatus(t("serve_latest_snapshots", "Latest snapshots"));
      const nextSourceKey = readableSourceKey(preserveSourceKey) || readableSourceKey();
      if (nextSourceKey) {
        await loadServeSourceReport(nextSourceKey);
      } else {
        applyServeMutationPayload(
          { report: emptyServeReport(), report_source_key: null },
          { selectedSourceKey: null }
        );
      }
    }
  } catch (error) {
    setServeStatus(error.message || String(error), true);
  }
}
async function loadServeSourceReport(sourceKey) {
  if (!sourceKey) return;
  try {
    setServeStatus(t("serve_latest_snapshots", "Latest snapshots"));
    const report = await serveApi(`/api/report?source_key=${encodeURIComponent(sourceKey)}`);
    applyServeMutationPayload({ report, report_source_key: sourceKey }, { selectedSourceKey: sourceKey });
  } catch (error) {
    setServeStatus(error.message || String(error), true);
    showServeNotice(error.message || String(error), true);
  }
}
function readableSourceKey(preferred = null) {
  const sources = Array.isArray(state.serveSources) ? state.serveSources : [];
  const usable = source => source?.source_key && source?.active !== false && source?.artifact_dir && source?.last_status !== "missing";
  if (preferred) {
    const match = sources.find(source => source?.source_key === preferred && usable(source));
    if (match) return match.source_key;
  }
  return sources.find(usable)?.source_key || null;
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
    if (Object.prototype.hasOwnProperty.call(options, "selectedSourceKey")) {
      state.selectedSourceKey = options.selectedSourceKey;
    } else if (Object.prototype.hasOwnProperty.call(payload, "report_source_key")) {
      state.selectedSourceKey = payload.report_source_key || null;
    } else {
      state.selectedSourceKey = state.selectedSourceKey || null;
    }
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
