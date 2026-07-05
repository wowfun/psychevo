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
