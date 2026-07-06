function renderServeSourceStateControls(rows = leaderboardRows()) {
  if (!serveMode()) return "";
  const mode = currentServeSourceMode();
  const allMode = mode === "all";
  const archived = mode === "archived";
  const targetMode = archived ? "active" : "archived";
  const targetReadableCount = readableServeSources(targetMode).length;
  const toggleDisabled = allMode || targetReadableCount < 1 ? "disabled" : "";
  const selectedCount = visibleSelectedSourceKeys(rows).length;
  const actionLabel = archived
    ? t("activate_selected", "Activate selected")
    : t("archive_selected", "Archive selected");
  return `<div class="source-state-controls" data-source-state-controls>
    <label class="source-state-toggle">
      <input type="checkbox" data-source-state-toggle ${archived || allMode ? "checked" : ""} ${toggleDisabled}>
      <span>${esc(t("show_archived", "Show archived"))}</span>
    </label>
    <button class="source-state-action" type="button" data-source-state-action ${selectedCount && !allMode ? "" : "disabled"}>${esc(allMode ? t("mixed_state_action_disabled", "Mixed view") : actionLabel)}</button>
  </div>`;
}

function bindServeSourceStateControls(target) {
  if (!serveMode() || !target) return;
  target.querySelectorAll("[data-source-state-toggle]").forEach(input => {
    input.addEventListener("click", event => event.stopPropagation());
    input.addEventListener("change", event => {
      event.stopPropagation();
      switchServeSourceMode(input.checked ? "archived" : "active");
    });
  });
  target.querySelectorAll("[data-source-state-action]").forEach(button => {
    button.addEventListener("click", event => {
      event.stopPropagation();
      mutateVisibleServeSourceState();
    });
  });
}

function visibleSelectedSourceKeys(rows = leaderboardRows()) {
  const keys = rows
    .filter(row => row?.trial_key && state.rowSelection.has(row.trial_key))
    .map(row => sourceKeyForTrialKey(row.trial_key))
    .filter(Boolean);
  return Array.from(new Set(keys));
}

async function switchServeSourceMode(mode) {
  const nextMode = normalizeServeSourceMode(mode);
  if (nextMode === currentServeSourceMode()) return;
  if (readableServeSources(nextMode).length < 1) {
    setServeStatus(t("archived_view_unavailable", "No sessions are available in that view. Use Sources to manage archived sessions."), true);
    renderComparisonPanels({ trace: false });
    return;
  }
  state.rowSelection.clear();
  state.selectedStep = null;
  const cached = state.serveReportCache?.[nextMode];
  if (cached) {
    state.serveSourceMode = nextMode;
    state.selectedSourceKey = readableSourceKey(null, nextMode);
    state.selectedTrial = trialKeyForServeSource(state.selectedSourceKey, cached, nextMode) || null;
    render(cached);
    setServeStatus(serveSourceModeStatusText(nextMode));
    return;
  }
  try {
    const report = await serveApi(`/api/report?source_state=${encodeURIComponent(nextMode)}`);
    applyServeMutationPayload(
      { report, report_source_state: nextMode },
      { selectedSourceKey: readableSourceKey(null, nextMode) }
    );
    setServeStatus(serveSourceModeStatusText(nextMode));
  } catch (error) {
    setServeStatus(error.message || String(error), true);
    renderComparisonPanels({ trace: false });
  }
}

async function mutateVisibleServeSourceState() {
  const sourceKeys = visibleSelectedSourceKeys();
  if (!sourceKeys.length) return;
  const mode = currentServeSourceMode();
  if (mode === "all") {
    setServeStatus(t("mixed_state_action_disabled", "Mixed view"), true);
    return;
  }
  const targetMode = mode === "archived" ? "active" : "archived";
  try {
    const payload = await serveApi("/api/sources/state", {
      method: "POST",
      body: {
        source_keys: sourceKeys,
        active: targetMode === "active",
        report_source_state: mode
      }
    });
    await applyServeSourceStateMutationPayload(payload, { sourceKeys, targetMode });
  } catch (error) {
    setServeStatus(error.message || String(error), true);
  }
}

async function applyServeSourceStateMutationPayload(payload, options = {}) {
  const payloadMode = normalizeServeSourceMode(payload?.report_source_state || currentServeSourceMode());
  const targetMode = normalizeServeSourceMode(options.targetMode);
  const movedSourceKey = firstReadableSourceKeyFrom(options.sourceKeys, payload?.sources || state.serveSources, targetMode);
  const emptiedCurrentMode = payload?.report && listValue(payload.report?.trajectory_meta).length === 0;
  if (emptiedCurrentMode && targetMode !== payloadMode && movedSourceKey) {
    const targetReport = await serveApi(`/api/report?source_state=${encodeURIComponent(targetMode)}`);
    applyServeMutationPayload(
      {
        ...payload,
        report: targetReport,
        report_source_key: movedSourceKey,
        report_source_state: targetMode
      },
      { selectedSourceKey: movedSourceKey }
    );
    return;
  }
  applyServeMutationPayload(payload);
}

function firstReadableSourceKeyFrom(sourceKeys, sources, mode) {
  const requested = new Set(listValue(sourceKeys).map(key => String(key || "")).filter(Boolean));
  return readableServeSourcesFrom(sources, mode).find(source => requested.has(source.source_key))?.source_key || null;
}

function serveSourceModeStatusText(mode = currentServeSourceMode()) {
  if (normalizeServeSourceMode(mode) === "all") {
    return t("serve_all_sessions", "All sessions");
  }
  return normalizeServeSourceMode(mode) === "archived"
    ? t("serve_archived_snapshots", "Archived snapshots")
    : t("serve_active_snapshots", "Active snapshots");
}
