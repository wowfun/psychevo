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
