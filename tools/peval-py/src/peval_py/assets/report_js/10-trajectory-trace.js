function renderTrajectoryOverview(rows = leaderboardRows()) {
  const target = $("trajectory-overview");
  if (!target) return;
  const body = rows.length
    ? rows.map(row => renderTrajectoryOverviewRow(row)).join("")
    : `<div class="trajectory-empty">${esc(t("no_matching_rows", "No matching rows"))}</div>`;
  target.innerHTML = `
    <div class="panel-head"><div><h2 id="trajectory-overview-title">${esc(t("trajectory_overview", "Trajectory Overview"))}</h2><p class="copy">${esc(t("trajectory_overview_copy", "Rows follow the current Leaderboard order. Nodes align by step index and show role initials."))}</p></div>${renderServeSourceStateControls(rows)}</div>
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
  const selection = serveMode() ? `<div class="trajectory-select">${renderRowSelection(row)}</div>` : "";
  const classes = ["trajectory-row", serveMode() ? "trajectory-row-selectable" : "", selected ? "selected-row" : ""].filter(Boolean).join(" ");
  return `<div class="${esc(classes)}" data-trial-key="${esc(row.trial_key)}" title="${esc(row.trial_key)}">${selection}<div class="trajectory-label"><strong>${esc(session)}</strong><span>${esc(secondary)}</span></div><div class="trajectory-track">${steps.map((step, index) => renderTrajectoryNode(step, index, row.trial_key, timingModel)).join("")}</div></div>`;
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
  bindServeSourceStateControls(target);
  bindServeSelectionControls(target);
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
  const target = $("trace");
  const trial = metaFor(selectedKey());
  if (!trial?.trial_key) {
    state.selectedTrial = null;
    state.selectedStep = null;
    disposeTimelineChart();
    if (target) target.innerHTML = "";
    renderStepDrawer();
    return;
  }
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
  target.innerHTML = `
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
