function infoGrid(items) {
  return `<div class="info-grid">${items.map(([label, value]) => `<div><span>${esc(label)}</span><strong>${esc(value)}</strong></div>`).join("")}</div>`;
}
function trialWallDurationMs(trial) {
  if (hasMetricValue(trial?.wall_duration_ms)) return Number(trial.wall_duration_ms);
  if (hasMetricValue(trial?.started_at_ms) && hasMetricValue(trial?.finished_at_ms)) return Math.max(0, Number(trial.finished_at_ms) - Number(trial.started_at_ms));
  return trial?.duration_ms;
}
function stepTimingStats(meta) {
  const steps = meta?.steps || [];
  const stepDurations = steps.map(step => step?.duration_ms);
  const toolDurations = steps.flatMap(step => (step?.tool_calls || []).map(tool => tool?.execution_duration_ms));
  const elapsedValues = steps.map(step => step?.elapsed_ms);
  const wallDuration = trialWallDurationMs(meta);
  return {
    maxStepDurationMs: maxPositiveMetric(stepDurations),
    maxToolExecutionMs: maxPositiveMetric(toolDurations),
    elapsedMaxMs: positiveMetric(wallDuration) ? Number(wallDuration) : maxPositiveMetric(elapsedValues)
  };
}
function positiveMetric(value) { return hasMetricValue(value) && Number(value) > 0; }
function maxPositiveMetric(values) {
  const numeric = values.filter(positiveMetric).map(Number);
  return numeric.length ? Math.max(...numeric) : null;
}
function timingRatio(value, max) {
  if (!positiveMetric(value) || !positiveMetric(max)) return null;
  return Math.max(0, Math.min(1, Number(value) / Number(max)));
}
function timeGradientStyle(ratio) {
  if (ratio === null || ratio === undefined) return "";
  return ` style="--time-pct: ${esc((ratio * 100).toFixed(1))}%"`;
}
function timeGradientClass(ratio) { return ratio === null || ratio === undefined ? "" : "time-gradient"; }
function trajectoryDurationHeatClass(ratio) {
  if (ratio === null || ratio === undefined) return "";
  const level = Math.max(1, Math.min(10, Math.ceil(ratio * 10)));
  return `duration-heat-${level}`;
}
function timeTitle(label, value, ratio, basis) {
  const text = `${label} ${fmtMs(value)}`;
  return ratio === null || ratio === undefined ? text : `${text}; ${Math.round(ratio * 100)}% of ${basis}`;
}
function systemExposed(trajectory) { return (trajectory?.steps || []).some(step => step.source === "system"); }
function reasoningExposed(trajectory) { return (trajectory?.steps || []).some(step => step.reasoning_content); }
function tokenTotal(metrics) {
  const values = [metrics?.total_prompt_tokens, metrics?.total_completion_tokens].filter(hasMetricValue).map(Number);
  if (values.length) return values.reduce((sum, value) => sum + value, 0);
  const direct = metricExtra(metrics).usage?.total_tokens;
  return hasMetricValue(direct) ? Number(direct) : null;
}
function finalMetric(metrics, key) {
  return Object.prototype.hasOwnProperty.call(metrics || {}, key) ? metrics[key] : metricExtra(metrics)[key];
}
function metricExtra(metrics) {
  return metrics?.extra && typeof metrics.extra === "object" ? metrics.extra : {};
}
function renderSelectedNotes(trialKey) {
  const notes = notesFor(trialKey);
  const editor = renderNotesEditor(trialKey);
  const action = renderNotesAction(trialKey);
  const body = notes.length ? `<div class="note-list">${notes.map(renderManualNote).join("")}</div>` : `<p class="copy">${esc(t("no_notes", "No notes."))}</p>`;
  return `<section class="selected-extra selected-notes">
    <div class="selected-extra-head"><h3>${esc(t("notes", "Notes"))}</h3>${action}</div>
    ${editor}
    ${body}
  </section>`;
}
function renderNotesAction(trialKey) {
  const source = editableNotesSource(trialKey);
  if (!source || state.notesEditor?.trialKey === trialKey) return "";
  const cellNote = cellNoteFor(trialKey);
  const label = cellNote ? t("edit_notes", "Edit notes") : t("add_notes", "Add notes");
  return `<button class="step-toggle-button notes-edit-button" type="button" data-notes-edit data-trial-key="${esc(trialKey)}">${esc(label)}</button>`;
}
function renderNotesEditor(trialKey) {
  if (!serveMode() || state.notesEditor?.trialKey !== trialKey) return "";
  const markdown = state.notesEditor.markdown ?? "";
  const error = state.notesEditor.error ? `<p class="copy danger">${esc(state.notesEditor.error)}</p>` : "";
  const disabled = state.notesEditor.saving ? " disabled" : "";
  return `<article class="notes-editor-panel" data-notes-editor-panel>
    <textarea data-notes-editor data-trial-key="${esc(trialKey)}" rows="8">${esc(markdown)}</textarea>
    ${error}
    <div class="notes-editor-actions">
      <button class="step-toggle-button primary" type="button" data-notes-save data-trial-key="${esc(trialKey)}"${disabled}>${esc(t("save_notes", "Save notes"))}</button>
      <button class="step-toggle-button" type="button" data-notes-cancel${disabled}>${esc(t("cancel", "Cancel"))}</button>
    </div>
  </article>`;
}
function beginNotesEdit(trialKey) {
  if (!trialKey || !editableNotesSource(trialKey)) return;
  const note = cellNoteFor(trialKey);
  state.notesEditor = { trialKey, markdown: note?.markdown || "", error: "", saving: false };
  renderTrace();
}
function cancelNotesEdit() {
  state.notesEditor = null;
  renderTrace();
}
async function saveSelectedNotes(button) {
  const trialKey = button?.dataset?.trialKey || selectedKey();
  const source = editableNotesSource(trialKey);
  const panel = button?.closest?.("[data-notes-editor-panel]");
  const textarea = panel?.querySelector?.("[data-notes-editor]");
  if (!source?.source_key || !textarea) return;
  const markdown = textarea.value || "";
  state.notesEditor = { trialKey, markdown, error: "", saving: true };
  renderTrace();
  try {
    const payload = await serveApi(`/api/sources/${encodeURIComponent(source.source_key)}/notes`, {
      method: "POST",
      body: { markdown }
    });
    state.notesEditor = null;
    applyServeMutationPayload(payload, { preserveTrial: trialKey });
  } catch (error) {
    const message = `${t("notes_save_failed", "Save notes failed")}: ${error.message || String(error)}`;
    state.notesEditor = { trialKey, markdown, error: message, saving: false };
    setServeStatus(message, true);
    renderTrace();
  }
}
function renderSelectedAnalysis(trialKey) {
  const analysis = analysisFor(trialKey);
  if (!analysis) return "";
  const summary = analysis.summary ? `<pre>${esc(analysis.summary)}</pre>` : "";
  const markdown = analysis.md_report ? `<div class="note-body analysis-md">${renderMarkdown(analysis.md_report)}</div>` : "";
  const structured = renderStructuredAnalysis(analysis);
  const paths = renderAnalysisPaths(analysis);
  if (!summary && !markdown && !structured && !paths && analysis.status === "computed") return "";
  return `<section class="selected-extra selected-analysis"><h3>${esc(t("analysis", "Analysis"))}</h3><article class="selected-evidence-card analysis-card">${summary}${markdown}${structured}${paths}</article></section>`;
}
function renderStructuredAnalysis(analysis) {
  const blocks = [
    renderAnalysisFindings(analysis.findings),
    renderAnalysisList(t("recommendations", "Recommendations"), analysis.recommendations),
    renderAnalysisList(t("limitations", "Limitations"), analysis.limitations),
    renderAnalysisList(t("analysis_commands", "Analysis Commands"), analysis.commands),
    renderAnalysisDetails(analysis),
  ].filter(Boolean);
  return blocks.length ? `<div class="analysis-structured">${blocks.join("")}</div>` : "";
}
function renderAnalysisFindings(findings) {
  if (!Array.isArray(findings) || !findings.length) return "";
  return `<div class="analysis-block"><h4>${esc(t("findings", "Findings"))}</h4><ul class="evidence-list analysis-list">${findings.map(renderAnalysisFinding).join("")}</ul></div>`;
}
function renderAnalysisFinding(finding) {
  if (!isPlainObject(finding)) return `<li>${renderAnalysisValue(finding)}</li>`;
  const severity = finding.severity ? `<span class="chip">${esc(finding.severity)}</span>` : "";
  const title = finding.title || finding.summary || finding.message || t("finding", "Finding");
  const evidence = Array.isArray(finding.evidence) && finding.evidence.length
    ? `<p class="copy">${esc(t("evidence", "Evidence"))}: ${finding.evidence.map(analysisValueText).map(esc).join("; ")}</p>`
    : "";
  const recommendation = finding.recommendation
    ? `<p class="copy">${esc(t("recommendation", "Recommendation"))}: ${renderAnalysisValue(finding.recommendation)}</p>`
    : "";
  return `<li><div class="analysis-finding-head">${severity}<strong>${esc(title)}</strong></div>${evidence}${recommendation}</li>`;
}
function renderAnalysisList(label, values) {
  if (!Array.isArray(values) || !values.length) return "";
  return `<div class="analysis-block"><h4>${esc(label)}</h4><ul class="evidence-list analysis-list">${values.map(value => `<li>${renderAnalysisValue(value)}</li>`).join("")}</ul></div>`;
}
function renderAnalysisDetails(analysis) {
  const statusRows = [];
  if (analysis.analysis_status) statusRows.push([t("analysis_status", "Analysis Status"), analysis.analysis_status]);
  if (analysis.confidence !== undefined && analysis.confidence !== null && String(analysis.confidence).trim()) statusRows.push([t("confidence", "Confidence"), analysis.confidence]);
  const blocks = [];
  if (statusRows.length) blocks.push(infoGrid(statusRows));
  blocks.push(renderAnalysisObject(t("subject", "Subject"), analysis.subject));
  blocks.push(renderAnalysisMetrics(analysis.analysis_metrics));
  const html = blocks.filter(Boolean).join("");
  return html ? `<div class="analysis-block analysis-details">${html}</div>` : "";
}
function renderAnalysisMetrics(metrics) {
  if (!isPlainObject(metrics) || !Object.keys(metrics).length) return "";
  const blocks = [];
  if (isPlainObject(metrics.auto) && Object.keys(metrics.auto).length) {
    blocks.push(renderAnalysisMetricGroups(metrics.auto));
  }
  const imported = Object.fromEntries(Object.entries(metrics).filter(([key]) => key !== "auto"));
  if (Object.keys(imported).length) {
    blocks.push(renderAnalysisObject(t("imported_metrics", "Imported Metrics"), imported));
  }
  return blocks.length ? `<div class="analysis-metrics">${blocks.join("")}</div>` : "";
}
function renderAnalysisMetricGroups(autoMetrics) {
  return Object.entries(autoMetrics)
    .filter(([, value]) => isPlainObject(value) && Object.keys(value).length)
    .map(([key, value]) => renderAutoMetricGroup(key, value))
    .join("");
}
function renderAutoMetricGroup(key, metrics) {
  const blocks = [];
  const scalarRows = autoMetricScalarRows(key, metrics);
  if (scalarRows.length) blocks.push(infoGrid(scalarRows));
  if (key === "latency") {
    blocks.push(renderLatencyComparison(metrics));
  }
  return blocks.filter(Boolean).length
    ? `<div class="analysis-metric-group analysis-metric-group-${esc(key)}"><h5>${esc(analysisMetricGroupLabel(key))}</h5>${blocks.filter(Boolean).join("")}</div>`
    : "";
}
function autoMetricScalarRows(group, metrics) {
  return Object.entries(metrics)
    .filter(([key, value]) => isMetricScalar(value) && !autoMetricStructuredKeys(group).has(key))
    .map(([key, value]) => [analysisMetricLabel(key), metricValueText(key, value)]);
}
function autoMetricStructuredKeys(group) {
  const keys = {
    latency: ["step_duration_ms", "tool_execution_duration_ms", "model_duration_ms"],
  };
  return new Set(keys[group] || []);
}
function analysisMetricGroupLabel(key) {
  const labels = {
    tooling: t("metric_group_tooling", "Tooling"),
    cost: t("metric_group_cost", "Cost"),
    latency: t("metric_group_latency", "Latency"),
  };
  return labels[key] || key;
}
function analysisMetricLabel(key) {
  const labels = {
    tool_error_rate: t("metric_tool_error_rate", "Tool error rate"),
    distinct_tools: t("metric_distinct_tools", "Distinct tools"),
    cost_per_1k_tokens: t("metric_cost_per_1k_tokens", "Cost / 1k tokens"),
    model_duration_ms: t("metric_model_duration_ms", "Model duration"),
    count: t("metric_count", "Count"),
    errors: t("metric_errors", "Errors"),
    duration_ms: t("metric_duration", "Duration"),
    p50: "p50",
    q1: "q1",
    q3: "q3",
    p95: "p95",
    min: t("metric_min", "Min"),
    max: t("metric_max", "Max"),
  };
  return labels[key] || key;
}
function renderAnalysisObject(label, value) {
  if (!isPlainObject(value) || !Object.keys(value).length) return "";
  return `<div class="analysis-object"><h4>${esc(label)}</h4>${renderMetricTable(value)}</div>`;
}
function renderMetricTable(value, depth = 0) {
  if (!isPlainObject(value) || !Object.keys(value).length) return "";
  const rows = Object.entries(value)
    .map(([key, item]) => `<tr><th>${esc(analysisMetricLabel(key))}</th><td>${renderMetricValue(key, item, depth)}</td></tr>`)
    .join("");
  return `<table class="analysis-kv-table"><tbody>${rows}</tbody></table>`;
}
function renderMetricValue(key, value, depth = 0) {
  if (isMetricScalar(value)) return esc(metricValueText(key, value));
  if (Array.isArray(value)) {
    if (!value.length) return `<span class="muted">[]</span>`;
    if (depth < 1 && value.every(isPlainObject)) return renderMetricArrayTable(value, depth + 1);
    if (depth < 1 && value.every(isMetricScalar)) return `<span class="analysis-inline-list">${value.map(item => esc(metricValueText(key, item))).join(", ")}</span>`;
    return renderMetricDetails(value);
  }
  if (isPlainObject(value)) {
    if (depth < 1) return renderMetricTable(value, depth + 1);
    return renderMetricDetails(value);
  }
  return esc(analysisValueText(value));
}
function renderMetricArrayTable(values, depth = 0) {
  const keys = Array.from(new Set(values.flatMap(item => Object.keys(item))));
  if (!keys.length) return renderMetricDetails(values);
  const head = keys.map(key => `<th>${esc(analysisMetricLabel(key))}</th>`).join("");
  const rows = values.map(item => `<tr>${keys.map(key => `<td>${renderMetricValue(key, item[key], depth)}</td>`).join("")}</tr>`).join("");
  return `<div class="analysis-table-wrap"><table class="analysis-data-table"><thead><tr>${head}</tr></thead><tbody>${rows}</tbody></table></div>`;
}
function renderMetricDetails(value) {
  const summary = Array.isArray(value)
    ? t("metric_array_summary", "Array value")
    : t("metric_object_summary", "Object value");
  return `<details class="analysis-json-details"><summary>${esc(summary)}</summary><pre>${esc(JSON.stringify(value, null, 2))}</pre></details>`;
}
function renderLatencyComparison(metrics) {
  const rows = [
    ["step_duration_ms", t("metric_step_duration_ms", "Step duration"), metrics.step_duration_ms],
    ["tool_execution_duration_ms", t("metric_tool_execution_duration_ms", "Tool execution"), metrics.tool_execution_duration_ms],
    ["model_duration_ms", t("metric_model_duration_ms", "Model duration"), metrics.model_duration_ms],
  ].filter(([, , distribution]) => isPlainObject(distribution) && Object.keys(distribution).length);
  if (!rows.length) return "";
  const max = Math.max(...rows.map(([, , distribution]) => Number(distribution.max || 0)), 0);
  const body = rows.map(([key, label, distribution]) => renderLatencyComparisonRow(key, label, distribution, max)).join("");
  return `<div class="analysis-latency-chart">${body}</div>`;
}
function renderLatencyComparisonRow(key, label, distribution, max) {
  return `<div class="analysis-latency-row analysis-latency-${esc(key)}">${renderLatencyBoxPlot(label, distribution, max)}<h6>${esc(label)}</h6></div>`;
}
function renderLatencyBoxPlot(label, distribution, max) {
  const value = key => hasMetricValue(distribution[key]) ? Number(distribution[key]) : null;
  const min = value("min") ?? value("p50") ?? 0;
  const q1 = value("q1") ?? value("p50") ?? min;
  const p50 = value("p50") ?? q1;
  const q3 = value("q3") ?? p50;
  const p95 = value("p95") ?? q3;
  const high = value("max") ?? p95;
  const pct = item => max > 0 ? Math.max(0, Math.min(100, (Number(item || 0) / max) * 100)) : 0;
  const style = [
    `--whisker-bottom:${pct(min)}%`,
    `--whisker-height:${Math.max(0, pct(high) - pct(min))}%`,
    `--box-bottom:${pct(q1)}%`,
    `--box-height:${Math.max(0, pct(q3) - pct(q1))}%`,
    `--median-bottom:${pct(p50)}%`,
    `--p95-bottom:${pct(p95)}%`,
  ].join(";");
  const title = [
    `${label}`,
    `min ${metricValueText("duration_ms", min)}`,
    `q1 ${metricValueText("duration_ms", q1)}`,
    `p50 ${metricValueText("duration_ms", p50)}`,
    `q3 ${metricValueText("duration_ms", q3)}`,
    `p95 ${metricValueText("duration_ms", p95)}`,
    `max ${metricValueText("duration_ms", high)}`,
  ].join("; ");
  const labels = [
    ["max", high],
    ["p95", p95],
    ["p50", p50],
    ["min", min],
  ].filter(([stat, item], index, values) => index === values.findIndex(([otherStat]) => otherStat === stat));
  const labelHtml = labels.map(([stat, item]) => {
    const valuePct = pct(item);
    return `<span class="analysis-box-label analysis-box-label-${esc(stat)}" style="--label-bottom:${esc(`${valuePct}%`)}"><b>${esc(analysisMetricLabel(stat))}</b> ${esc(metricValueText("duration_ms", item))}</span>`;
  }).join("");
  return `<div class="analysis-boxplot" style="${esc(style)}" title="${esc(title)}" aria-label="${esc(title)}"><span class="analysis-box-axis"></span><span class="analysis-box-whisker"></span><span class="analysis-box-range"></span><span class="analysis-box-median"></span><span class="analysis-box-p95"></span>${labelHtml}</div>`;
}
function isMetricScalar(value) {
  return value === null || value === undefined || ["string", "number", "boolean"].includes(typeof value);
}
function metricValueText(key, value) {
  if (value === null || value === undefined || value === "") return "-";
  if (key === "tool_error_rate") return fmtPct(value);
  if (String(key).endsWith("_ms") || key === "duration_ms") return fmtMs(value);
  if (typeof value === "number") return fmtNum(value);
  return String(value);
}
function renderAnalysisValue(value) {
  return esc(analysisValueText(value));
}
function analysisValueText(value) {
  if (value === null || value === undefined) return "-";
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  try {
    return JSON.stringify(value);
  } catch (_error) {
    return String(value);
  }
}
function isPlainObject(value) {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}
function renderAnalysisPaths(analysis) {
  const paths = analysis.relative_paths || {};
  const rows = [];
  if (paths.json) rows.push(["JSON", paths.json]);
  if (paths.md) rows.push(["Markdown", paths.md]);
  if (!rows.length && analysis.relative_path) rows.push(["Source", analysis.relative_path]);
  return rows.length ? `<div class="analysis-source-list">${rows.map(([label, path]) => `<p class="copy analysis-path"><span class="analysis-source-label">${esc(label)}</span><code>${esc(path)}</code></p>`).join("")}</div>` : "";
}
function renderSelectedEvidence(trajectory, meta) {
  const blocks = [renderSelectedUsage(trajectory), renderSelectedWarnings(meta), renderSelectedSource(meta)].filter(Boolean);
  return blocks.length ? `<section class="selected-extra selected-evidence"><h3>${esc(t("evidence", "Evidence"))}</h3><div class="selected-evidence-list">${blocks.join("")}</div></section>` : "";
}
function renderSelectedUsage(trajectory) {
  const metrics = trajectory?.final_metrics || {};
  const extra = metricExtra(metrics);
  const usage = extra.usage || {};
  const accounting = extra.accounting || {};
  if (!extra.usage && !extra.accounting && !hasMetricValue(metrics.total_prompt_tokens) && !hasMetricValue(metrics.total_completion_tokens) && !hasMetricValue(metrics.total_cached_tokens)) return "";
  return `<article class="selected-evidence-card"><h4>${esc(t("usage_breakdown", "Usage Breakdown"))}</h4>${infoGrid([
    [t("input", "Input"), fmtNum(usage.input_tokens ?? metrics.total_prompt_tokens)],
    [t("output", "Output"), fmtNum(usage.output_tokens ?? metrics.total_completion_tokens)],
    [t("cache_read", "Cache read"), fmtNum(usage.cache_read_tokens ?? metrics.total_cached_tokens)],
    [t("cache_write", "Cache write"), fmtNum(usage.cache_write_tokens)],
    [t("reasoning", "Reasoning"), fmtNum(usage.reasoning_tokens)],
    [t("billable_input", "Billable input"), fmtNum(accounting.billable_input_tokens)],
    [t("billable_output", "Billable output"), fmtNum(accounting.billable_output_tokens)],
    [t("pricing", "Pricing"), accounting.pricing_source || "-"]
  ])}</article>`;
}
function renderSelectedWarnings(meta) {
  const warnings = meta.warnings || [];
  if (!warnings.length) return "";
  return `<article class="selected-evidence-card"><h4>${esc(t("warnings", "Warnings"))}</h4><ul class="evidence-list">${warnings.map(warning => `<li>${esc(warning)}</li>`).join("")}</ul></article>`;
}
function renderSelectedSource(meta) {
  const path = meta.data_ref?.relative_path;
  return path ? `<article class="selected-evidence-card"><h4>${esc(t("input_source", "Input Source"))}</h4><code>${esc(path)}</code></article>` : "";
}
