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
      selectServeSource(sourceKey);
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
