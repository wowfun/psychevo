fn workbench_js_tables() -> &'static str {
    r###"
      renderLeaderboard();
      renderMatrix();
      renderTrace();
    });
  });
}
function renderStepsHeader(trajectory, trajectoryMeta) {
  const count = (trajectory?.steps || []).length ?? 0;
  const disabled = trajectory && count > 0 ? "" : "disabled";
  return `<div class="steps-head"><h3>Steps (${count})</h3><div class="step-actions" aria-label="step visibility toggle"><button class="step-toggle-button" type="button" data-step-action="toggle" aria-pressed="false" ${disabled}>Expand all</button></div></div>`;
}
function stepRows() {
  return Array.from(document.querySelectorAll("#step-list .step"));
}
function refreshStepToggleButton() {
  const button = document.querySelector("[data-step-action=\"toggle\"]");
  if (!button) return;
  const rows = stepRows();
  const hasRows = rows.length > 0;
  const allOpen = hasRows && rows.every(row => row.open);
  button.disabled = !hasRows;
  button.dataset.stepState = allOpen ? "expanded" : "collapsed";
  button.textContent = allOpen ? "Collapse all" : "Expand all";
  button.setAttribute("aria-pressed", allOpen ? "true" : "false");
}
function bindStepControls() {
  const button = document.querySelector("[data-step-action=\"toggle\"]");
  stepRows().forEach(row => row.addEventListener("toggle", refreshStepToggleButton));
  if (!button) return;
  button.addEventListener("click", event => {
    event.stopPropagation();
    const rows = stepRows();
    const shouldOpen = !rows.every(row => row.open);
    rows.forEach(row => {
      row.open = shouldOpen;
      if (!shouldOpen) row.classList.remove("selected-step");
    });
    if (!shouldOpen) state.selectedStepId = null;
    refreshStepToggleButton();
  });
  refreshStepToggleButton();
}
function renderSelectedNotes(trialKey) {
  const notes = notesFor(trialKey);
  const body = notes.length ? `<div class="note-list">${notes.map(renderManualNote).join("")}</div>` : `<p class="copy">No notes.</p>`;
  return `<section class="selected-extra"><h3>Notes</h3>${body}</section>`;
}
function renderSelectedAnalysis(trialKey) {
  const analysis = analysisFor(trialKey);
  if (!analysis || analysis.status === "missing") {
    return `<section class="selected-extra"><h3>Analysis</h3><p class="copy">No cached analysis.</p></section>`;
  }
  return `<section class="selected-extra"><h3>Analysis</h3><article class="analysis-card"><div class="note-meta"><span class="chip">${esc(analysis.status || "cached")}</span>${analysis.json_ref?.relative_path ? `<strong>${esc(analysis.json_ref.relative_path)}</strong>` : ""}</div><pre>${esc(analysis.summary || analysis.error || "-")}</pre></article></section>`;
}
function renderSelectedEvidence(trial) {
  const blocks = [
    renderSelectedCellRoot(trial),
    renderSelectedScoreDetails(trial),
    renderSelectedUsage(trial.trial_key),
    renderSelectedWarnings(trial),
    renderSelectedArtifacts(trial)
  ].filter(Boolean);
  return blocks.length ? `<section class="selected-extra selected-evidence"><h3>Evidence</h3><div class="selected-evidence-list">${blocks.join("")}</div></section>` : "";
}
function renderSelectedCellRoot(trial) {
  if (!trial.cell_root_relative) return "";
  return `<article class="selected-evidence-card"><h4>Cell Root</h4><code>${esc(trial.cell_root_relative)}</code></article>`;
}
function isEmptyObject(value) {
  return value === null || value === undefined || (typeof value === "object" && !Array.isArray(value) && Object.keys(value).length === 0);
}
function renderSelectedScoreDetails(trial) {
  if (isEmptyObject(trial.score_details)) return "";
  return `<details class="selected-evidence-card"><summary>Score Details</summary><pre>${esc(shortJson(trial.score_details))}</pre></details>`;
}
function renderSelectedUsage(trialKey) {
  const metrics = finalMetricsFor(trialKey);
  const usage = metrics.usage || {};
  const accounting = metrics.accounting || {};
  if (!metrics.usage && !metrics.accounting && !hasMetricValue(metrics.total_prompt_tokens) && !hasMetricValue(metrics.total_completion_tokens) && !hasMetricValue(metrics.total_cached_tokens)) return "";
  return `<article class="selected-evidence-card"><h4>Usage Breakdown</h4>${infoGrid([
    ["input", fmtNum(usage.input_tokens ?? metrics.total_prompt_tokens)],
    ["output", fmtNum(usage.output_tokens ?? metrics.total_completion_tokens)],
    ["cache read", fmtNum(usage.cache_read_tokens ?? metrics.total_cached_tokens)],
    ["cache write", fmtNum(usage.cache_write_tokens)],
    ["reasoning", fmtNum(usage.reasoning_tokens)],
    ["billable input", fmtNum(accounting.billable_input_tokens)],
    ["billable output", fmtNum(accounting.billable_output_tokens)],
    ["pricing", accounting.pricing_source || "-"]
  ])}</article>`;
}
function renderSelectedWarnings(trial) {
  const warnings = trial.warnings || [];
  if (!warnings.length) return "";
  return `<article class="selected-evidence-card"><h4>Warnings</h4><ul class="evidence-list">${warnings.map(warning => `<li>${esc(warning)}</li>`).join("")}</ul></article>`;
}
function renderSelectedArtifacts(trial) {
  const refs = attachmentFor(trial.trial_key)?.refs || [];
  if (!refs.length) return "";
  return `<details class="selected-evidence-card"><summary>Artifacts</summary><ul class="artifact-list">${refs.map(ref => `<li title="${esc([ref.kind, ref.mime, ref.label].filter(Boolean).join(" / "))}"><span class="chip">${esc(ref.kind || "artifact")}</span><code>${esc(ref.relative_path || ref.label || "-")}</code></li>`).join("")}</ul></details>`;
}
function renderStep(step) {
  const meta = arguments[1] ? stepMeta(arguments[1], step.step_id) : null;
  const preview = valuePreview(step.message).trim() || "(No Message)";
  const open = step.step_id === state.selectedStepId ? "open" : "";
  const selected = step.step_id === state.selectedStepId ? "selected-step" : "";
  return `
    <details class="step ${selected}" data-step="${step.step_id}" ${open}>
      <summary>
        <div class="step-row">
          <span class="step-id">#${esc(step.step_id)}</span>
          <span class="role ${esc(step.source)}">${esc(step.source)}</span>
          <span class="preview">${esc(preview)}</span>
        </div>
        <div class="rail">
          ${renderStepRail(step, meta)}
        </div>
      </summary>
      <div class="step-body">${renderStepBlocks(step, meta)}</div>
    </details>
  `;
}
function renderStepBlocks(step, meta) {
  let html = "";
  if (step.reasoning_content) {
    html += `<div class="block reasoning-block"><h4>Reasoning</h4><pre>${esc(step.reasoning_content)}</pre></div>`;
  }
  const message = valuePreview(step.message);
  if (message) {
    const label = step.source === "system" ? "System Prompt" : "Message";
    html += `<div class="block message-block"><h4>${esc(label)}</h4><pre>${esc(message)}</pre></div>`;
  }
  (step.tool_calls || []).forEach(tool => {
    const toolMeta = toolMetaFor(meta, tool.tool_call_id);
    html += `<div class="block tool-block"><h4>Tool Calls</h4><p>${renderToolNameChip(tool, toolMeta)} <span class="muted">ID: ${esc(tool.tool_call_id)}${toolMeta?.status ? ` / ${esc(toolMeta.status)}` : ""}${renderToolTiming(toolMeta)}</span></p><pre>${esc(valuePreview(tool.arguments))}</pre></div>`;
  });
  ((step.observation && step.observation.results) || []).forEach(observation => {
    const observationMeta = observationMetaFor(meta, observation.source_call_id);
    html += `<div class="block observation-block"><h4 class="${observationMeta?.tool_error ? "danger" : ""}">Observations</h4><p class="muted">Result for: ${esc(observation.source_call_id || "-")}${observationMeta?.status ? ` / ${esc(observationMeta.status)}` : ""}</p><pre>${esc(valuePreview(observation.content))}</pre></div>`;
  });
  return html;
}
function hasMetricValue(value) {
  return value !== null && value !== undefined && value !== "" && !Number.isNaN(Number(value));
}
function toolExecutionText(toolMeta) {
  return hasMetricValue(toolMeta?.execution_duration_ms) ? fmtRailMs(toolMeta.execution_duration_ms) : "";
}
function toolFailed(toolMeta) {
  const status = lower(toolMeta?.status);
  return status === "error" || status === "failed";
}
function renderToolNameChip(tool, toolMeta, extraClass = "") {
  const exec = toolExecutionText(toolMeta);
  const title = exec ? ` title="${esc(`tool exec ${exec}`)}"` : "";
  const execHtml = exec ? ` <span class="tool-exec-inline">${esc(exec)}</span>` : "";
  const classes = ["chip", "tool-name-chip", extraClass, toolFailed(toolMeta) ? "tool-error-chip" : ""].filter(Boolean).join(" ");
  return `<span class="${esc(classes)}"${title}>${esc(tool.function_name)}${execHtml}</span>`;
}
function renderToolTiming(toolMeta) {
  const parts = [];
  if (hasMetricValue(toolMeta?.generation_duration_ms)) parts.push(`generation ${fmtMs(toolMeta.generation_duration_ms)}`);
  return parts.length ? ` / ${parts.map(esc).join(" / ")}` : "";
}
function stepToolChips(step, meta) {
  const chips = [];
  (step.tool_calls || []).forEach(tool => {
    const name = String(tool.function_name || "").trim();
    if (!name) return;
    chips.push(renderToolNameChip(tool, toolMetaFor(meta, tool.tool_call_id), "rail-chip-tool-list"));
  });
  return chips;
}
function renderStepRail(step, meta) {
  const summaryItems = [];
  const toolCalls = (step.tool_calls || []).length;
  const toolErrors = meta?.tool_error ? 1 : 0;
  if (toolCalls || toolErrors) {
    summaryItems.push(`<span class="rail-chip rail-chip-tools">${esc(toolCallRatio(toolCalls, toolErrors))} tools</span>`);
  }
  const tokens = stepTokenTotal(step, meta);
  if (tokens !== null && tokens !== undefined) summaryItems.push(`<span class="rail-chip rail-chip-tokens" title="${esc(`${fmtNum(tokens)} tokens`)}">${esc(fmtRailTokens(tokens))} tok</span>`);
  const time = `<div class="rail-time"><span class="rail-chip rail-chip-step-time" title="step span">step ${esc(fmtRailMs(meta?.duration_ms))}</span><span class="rail-chip rail-chip-elapsed-time" title="elapsed since trajectory start">elapsed ${esc(fmtRailMs(meta?.elapsed_ms))}</span></div>`;
  const summary = `<div class="rail-summary">${summaryItems.join("")}${time}</div>`;
  const toolChips = stepToolChips(step, meta);
  const tools = toolChips.length ? `<div class="rail-tool-row">${toolChips.join("")}</div>` : "";
  return `${summary}${tools}`;
}
function stepMeta(meta, stepId) {
  return (meta?.steps || []).find(item => item.step_id === stepId) || null;
}
function toolCallRatio(total, errors) {
  const callTotal = Math.max(0, Number(total || 0));
  const errorTotal = Math.max(0, Number(errors || 0));
  const successful = Math.max(0, callTotal - errorTotal);
  return `${successful}/${callTotal}`;
}
function toolMetaFor(meta, toolCallId) {
  return (meta?.tool_calls || []).find(item => item.tool_call_id === toolCallId) || null;
}
function observationMetaFor(meta, sourceCallId) {
  return (meta?.observations || []).find(item => item.source_call_id === sourceCallId) || null;
}
function valuePreview(value) {
  if (value === null || value === undefined) return "";
  if (typeof value === "string") return value;
  return JSON.stringify(value, null, 2);
}
function stepTokenTotal(step, meta) {
  const metrics = step.metrics || {};
  const values = [metrics.prompt_tokens, metrics.completion_tokens, metrics.cached_tokens, metrics.usage?.total_tokens].filter(value => value !== null && value !== undefined && !Number.isNaN(Number(value))).map(Number);
  return values.length ? values.reduce((sum, value) => sum + value, 0) : null;
}
function shortJson(value) {
  if (value === null || value === undefined) return "-";
  const text = typeof value === "string" ? value : JSON.stringify(value);
  return text.length > 180 ? `${text.slice(0, 180)}...` : text;
}
function renderLeaderboard() {
  const entries = state.view.comparison?.leaderboard?.entries || [];
  if (!entries.length) {
    $("leaderboard").innerHTML = `<div class="panel-head"><div><p class="eyebrow">leaderboard</p><h2 id="leaderboard-title">Leaderboard</h2><p class="copy">No comparison include or no leaderboard entries.</p></div></div>`;
    return;
  }
  const comparisonRows = leaderboardComparisonRows(entries);
  const trialRows = leaderboardTrialRows(entries);
  const aggregateColumns = comparisonColumns(comparisonRows);
  const detailColumns = trialColumns(trialRows);
  $("leaderboard").innerHTML = `
    <div class="panel-head"><div><p class="eyebrow">leaderboard</p><h2 id="leaderboard-title">Agent / Model Comparison</h2><p class="copy">Flat rows compare agent, model, and task results directly. Identity columns filter; numeric columns sort.</p></div></div>
    ${renderInteractiveTable("leaderboard-aggregate", aggregateColumns, comparisonRows, "aggregate rows")}
    <details class="trial-details flat-trials" data-detail-key="leaderboard-trials" ${state.openDetails["leaderboard-trials"] ? "open" : ""}>
      <summary>Trial details</summary>
      ${renderInteractiveTable("leaderboard-trials", detailColumns, trialRows, "trial rows")}
    </details>
  `;
  bindLeaderboardControls();
}
function leaderboardComparisonRows(entries) {
  return entries.flatMap(entry => (entry.tasks || []).map(task => {
    const taskTrials = (task.trial_keys || []).map(trialFor).filter(Boolean);
    return {
      rank: entry.rank,
      variant_id: entry.variant_id || "-",
      variant_label: entry.variant_label || "-",
      agent_id: entry.agent_id,
      model_name: entry.model_name || "-",
      task_id: task.task_id,
      task_family: task.task_family || "-",
      total_trials: task.total_trials,
      successes: task.successes,
      pass_rate: task.pass_rate,
      average_score: task.average_score,
      average_duration_ms: task.average_duration_ms,
      trial_keys: task.trial_keys || [],
      average_tokens: averageOptional(taskTrials.map(trial => trialTotalTokens(trial.trial_key))),
      average_cost_usd: averageOptional(taskTrials.map(trial => trialCost(trial.trial_key)))
    };
  }));
}
function filteredLeaderboardComparisonRows() {
  const rows = leaderboardComparisonRows(state.view?.comparison?.leaderboard?.entries || []);
  return applyTableFilters("leaderboard-aggregate", comparisonColumns(rows), rows);
}
function leaderboardTrialRows(entries) {
  return entries.flatMap(entry => (entry.trial_keys || []).map(trialFor).filter(Boolean).map(trial => ({
    ...trial,
    rank: entry.rank,
    variant_id: trial.variant_id || entry.variant_id || "-",
    variant_label: trial.variant_label || entry.variant_label || "-",
    agent_id: trajectoryFor(trial.trial_key)?.agent?.name || entry.agent_id,
    model_name: trajectoryFor(trial.trial_key)?.agent?.model_name || entry.model_name || "-",
    total_tokens: trialTotalTokens(trial.trial_key),
    total_cost_usd: trialCost(trial.trial_key)
  })));
}
function sumOptional(values) {
  const numeric = values.filter(value => value !== null && value !== undefined && !Number.isNaN(Number(value))).map(Number);
  return numeric.length ? numeric.reduce((sum, value) => sum + value, 0) : null;
}
function averageOptional(values) {
  const numeric = values.filter(value => value !== null && value !== undefined && !Number.isNaN(Number(value))).map(Number);
  return numeric.length ? numeric.reduce((sum, value) => sum + value, 0) / numeric.length : null;
}
function notesForKeys(trialKeys) {
  const keys = new Set((trialKeys || []).filter(Boolean));
  return (state.view?.annotations?.notes || []).filter(note => keys.has(note.trial_key));
}
function notesPlainText(notes) {
  return notes.map(note => String(note.markdown || "").trim()).filter(Boolean).join("\n\n");
}
function notesFullTextForKeys(trialKeys) {
  return notesPlainText(notesForKeys(trialKeys));
}
function notesSummaryForKeys(trialKeys) {
  const notes = notesForKeys(trialKeys);
  if (!notes.length) return "-";
  const text = notesPlainText(notes).replace(/\s+/g, " ").trim();
  return text.length > 96 ? `${text.slice(0, 96)}...` : text;
}
function renderNotesCell(trialKeys) {
  const summary = notesSummaryForKeys(trialKeys);
  return summary === "-" ? `<span class="muted">-</span>` : `<span class="note-snippet">${esc(summary)}</span>`;
}
function hasVisibleVariant(rows) {
  return rows.some(row => [row.variant_id, row.variant_label].some(value => value && value !== "-"));
}
function variantColumn() {
  return { key: "variant_label", label: "Variant", width: "220px", filterable: true, value: row => row.variant_label || "-" };
}
function comparisonColumns(rows = []) {
  const columns = [
    { key: "model_name", label: "Model", width: "170px", filterable: true, value: row => row.model_name },
    { key: "task_id", label: "Task", width: "210px", filterable: true, value: row => row.task_id },
    { key: "task_family", label: "Family", width: "150px", filterable: true, value: row => row.task_family },
    { key: "total_trials", label: "Trials", width: "82px", type: "number", numeric: true, sortable: true, value: row => row.total_trials },
    { key: "successes", label: "Successes", width: "96px", type: "number", numeric: true, sortable: true, value: row => row.successes },
    { key: "pass_rate", label: "Pass Rate", width: "104px", type: "number", numeric: true, sortable: true, value: row => row.pass_rate, format: pct },
    { key: "average_score", label: "Score", width: "88px", type: "number", numeric: true, sortable: true, value: row => row.average_score, format: fmtScore },
    { key: "average_duration_ms", label: "Duration", width: "104px", type: "number", numeric: true, sortable: true, value: row => row.average_duration_ms, format: fmtMs },
    { key: "average_tokens", label: "Tokens", width: "100px", type: "number", numeric: true, sortable: true, value: row => row.average_tokens, format: fmtNum },
    { key: "average_cost_usd", label: "Cost", width: "92px", type: "number", numeric: true, sortable: true, value: row => row.average_cost_usd, format: fmtCost },
    { key: "notes", label: "Notes", width: "180px", value: row => notesSummaryForKeys(row.trial_keys || []), html: row => renderNotesCell(row.trial_keys || []), cellTitle: row => notesFullTextForKeys(row.trial_keys || []) }
  ];
  if (hasVisibleVariant(rows)) columns.unshift(variantColumn());
  return columns;
}
function trialColumns(rows = []) {
  const columns = [
    { key: "agent_id", label: "Agent", width: "150px", filterable: true, value: row => row.agent_id },
    { key: "model_name", label: "Model", width: "160px", filterable: true, value: row => row.model_name },
    { key: "task_id", label: "Task", width: "190px", filterable: true, value: row => row.task_id },
    { key: "status", label: "Result", width: "96px", filterable: true, value: row => row.status, html: row => `<span class="stamp ${statusClass(row.status)}">${esc(row.status)}</span>` },
    { key: "score", label: "Score", width: "88px", type: "number", numeric: true, sortable: true, value: row => row.score, format: fmtScore },
    { key: "duration_ms", label: "Duration", width: "104px", type: "number", numeric: true, sortable: true, value: row => row.duration_ms, format: fmtMs },
    { key: "total_tokens", label: "Tokens", width: "100px", type: "number", numeric: true, sortable: true, value: row => row.total_tokens, format: fmtNum },
    { key: "notes", label: "Notes", width: "220px", value: row => notesSummaryForKeys([row.trial_key]), html: row => renderNotesCell([row.trial_key]), cellTitle: row => notesFullTextForKeys([row.trial_key]) }
  ];
  if (hasMultiTrialMatrixCell()) columns.unshift(trialIdentityColumn());
  if (hasVisibleVariant(rows)) columns.unshift(variantColumn());
"###
}
