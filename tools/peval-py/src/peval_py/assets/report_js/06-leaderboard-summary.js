function renderLeaderboardSummary(rows = leaderboardRows()) {
  const target = $("leaderboard-summary");
  if (!target) return;
  const visibleRows = Array.isArray(rows) ? rows : [];
  const summaryRows = leaderboardSummaryRows(visibleRows);
  const body = visibleRows.length
    ? `<div class="leaderboard-summary-layout">${renderLeaderboardSummaryTable(summaryRows)}${renderLeaderboardSummaryDistributionPanel(summaryRows)}</div>`
    : `<p class="leaderboard-summary-empty">${esc(t("leaderboard_summary_empty", "No visible rows to summarize."))}</p>`;
  target.innerHTML = `
    <div class="panel-head">
      <div>
        <h2 id="leaderboard-summary-title">${esc(t("leaderboard_summary", "Leaderboard Summary"))}</h2>
        <p class="copy">${esc(t("leaderboard_summary_copy", "Distribution of metrics across the current visible Leaderboard rows."))}</p>
      </div>
    </div>
    ${body}
  `;
}

function leaderboardSummaryRows(rows = leaderboardRows()) {
  const visibleRows = Array.isArray(rows) ? rows : [];
  return leaderboardSummaryDefinitions().map(definition => {
    const values = visibleRows
      .map(row => summaryNumber(definition.value(row)))
      .filter(value => value !== null);
    const total = values.reduce((sum, value) => sum + value, 0);
    return {
      key: definition.key,
      label: definition.label,
      type: definition.type,
      rate: Boolean(definition.rate),
      count: values.length,
      missing: Math.max(0, visibleRows.length - values.length),
      total: definition.rate || !values.length ? null : total,
      mean: values.length ? total / values.length : null,
      distribution: leaderboardSummaryDistribution(values),
    };
  });
}

function leaderboardSummaryDefinitions() {
  return [
    { key: "duration_ms", label: t("duration", "Active Duration"), type: "duration", value: row => row?.duration_ms },
    { key: "tokens", label: t("tokens", "Tokens"), type: "number", value: row => row?.tokens },
    { key: "turns", label: t("turns", "Turns"), type: "number", value: row => row?.turns },
    { key: "model_duration_ms", label: t("model_call_duration", "Model call duration"), type: "duration", value: row => measuredModelDurationForRow(row) },
    { key: "total_tool_calls", label: t("tool_calls", "Tool Calls"), type: "number", value: row => row?.total_tool_calls },
    { key: "tool_error_rate", label: t("tool_error_rate", "Tool Error Rate"), type: "percent", rate: true, value: row => rowToolErrorRate(row) },
  ];
}

function measuredModelDurationForRow(row) {
  const metas = listValue(state.view?.trajectory_meta);
  const index = metas.findIndex(meta => meta?.trial_key === row?.trial_key);
  if (index < 0) return null;
  const trajectory = listValue(state.view?.trajectory)[index] || {};
  const trajectorySteps = listValue(trajectory.steps);
  const metaSteps = listValue(metas[index]?.steps);
  let total = 0;
  let count = 0;
  metaSteps.forEach((step, stepIndex) => {
    if (!step || typeof step !== "object") return;
    const source = lower(trajectorySteps[stepIndex]?.source);
    if (source !== "agent" && source !== "assistant") return;
    if (lower(step.duration_source).includes("estimate")) return;
    const duration = summaryNumber(step.duration_ms);
    if (duration === null) return;
    total += duration;
    count += 1;
  });
  return count ? total : null;
}

function renderLeaderboardSummaryTable(rows) {
  const columns = Array.isArray(rows) ? rows : [];
  const headers = columns.map(row => `<th class="num">${esc(row.label)}</th>`).join("");
  const body = leaderboardSummaryStatistics().map(statistic => `
    <tr>
      <th scope="row">${esc(statistic.label)}</th>
      ${columns.map(row => `<td class="num">${esc(statistic.value(row))}</td>`).join("")}
    </tr>
  `).join("");
  return `<div class="leaderboard-summary-table-panel"><div class="table-shell leaderboard-summary-shell"><div class="table-wrap"><table class="data-table leaderboard-summary-table"><thead><tr>
    <th>${esc(t("summary_statistic", "Statistic"))}</th>
    ${headers}
  </tr></thead><tbody>${body}</tbody></table></div></div></div>`;
}

function leaderboardSummaryStatistics() {
  return [
    { key: "count", label: t("summary_count", "Count"), value: row => fmtNum(row.count) },
    { key: "missing", label: t("summary_missing", "Missing"), value: row => fmtNum(row.missing) },
    { key: "total", label: t("summary_total", "Total"), value: row => row.rate ? "-" : leaderboardSummaryValue(row, row.total) },
    { key: "mean", label: t("summary_mean", "Mean"), value: row => leaderboardSummaryValue(row, row.mean) },
    { key: "min", label: t("metric_min", "Min"), value: row => leaderboardSummaryValue(row, row.distribution?.min) },
    { key: "q1", label: t("summary_q1", "Q1"), value: row => leaderboardSummaryValue(row, row.distribution?.q1) },
    { key: "p50", label: t("summary_p50", "P50"), value: row => leaderboardSummaryValue(row, row.distribution?.p50) },
    { key: "q3", label: t("summary_q3", "Q3"), value: row => leaderboardSummaryValue(row, row.distribution?.q3) },
    { key: "p95", label: t("summary_p95", "P95"), value: row => leaderboardSummaryValue(row, row.distribution?.p95) },
    { key: "max", label: t("metric_max", "Max"), value: row => leaderboardSummaryValue(row, row.distribution?.max) },
  ];
}

function renderLeaderboardSummaryDistributionPanel(rows) {
  const cards = rows.map(row => renderLeaderboardSummaryBoxplotCard(row)).join("");
  return `<div class="leaderboard-summary-chart-panel">
    <div class="leaderboard-summary-chart-head">
      <h3>${esc(t("leaderboard_summary_distributions", "Leaderboard Summary Distributions"))}</h3>
    </div>
    <div class="summary-boxplot-grid">${cards}</div>
  </div>`;
}

function renderLeaderboardSummaryBoxplotCard(row) {
  const distribution = row.distribution;
  const plot = distribution ? renderLeaderboardSummaryBoxplot(row) : `<span class="muted">-</span>`;
  return `<div class="summary-boxplot-card">
    <div class="summary-boxplot-card-head">
      <strong>${esc(row.label)}</strong>
      <span>${esc(row.count ? `${leaderboardSummaryValue(row, row.distribution?.min)} - ${leaderboardSummaryValue(row, row.distribution?.max)}` : "-")}</span>
    </div>
    ${plot}
  </div>`;
}

function renderLeaderboardSummaryBoxplot(row) {
  const distribution = row.distribution;
  if (!distribution) return `<span class="muted">-</span>`;
  const positions = leaderboardSummaryBoxplotPositions(distribution);
  const style = [
    `--summary-whisker-bottom:${positions.min}%`,
    `--summary-whisker-height:${Math.max(positions.max - positions.min, 1)}%`,
    `--summary-box-bottom:${positions.q1}%`,
    `--summary-box-height:${Math.max(positions.q3 - positions.q1, 1)}%`,
    `--summary-median-bottom:${positions.p50}%`,
    `--summary-p95-bottom:${positions.p95}%`,
  ].join(";");
  const title = [
    `${row.label}`,
    `${t("metric_min", "Min")} ${leaderboardSummaryValue(row, distribution.min)}`,
    `${t("summary_q1", "Q1")} ${leaderboardSummaryValue(row, distribution.q1)}`,
    `${t("summary_p50", "P50")} ${leaderboardSummaryValue(row, distribution.p50)}`,
    `${t("summary_q3", "Q3")} ${leaderboardSummaryValue(row, distribution.q3)}`,
    `${t("summary_p95", "P95")} ${leaderboardSummaryValue(row, distribution.p95)}`,
    `${t("metric_max", "Max")} ${leaderboardSummaryValue(row, distribution.max)}`,
  ].join("; ");
  return `<div class="summary-boxplot summary-boxplot-vertical ${positions.flat ? "summary-boxplot-flat" : ""}" style="${esc(style)}" title="${esc(title)}" aria-label="${esc(title)}"><span class="summary-boxplot-axis"></span><span class="summary-boxplot-whisker"></span><span class="summary-boxplot-box"></span><span class="summary-boxplot-median"></span><span class="summary-boxplot-p95"></span></div>`;
}

function leaderboardSummaryDistribution(values) {
  if (!values.length) return null;
  const ordered = [...values].sort((left, right) => left - right);
  return {
    min: ordered[0],
    q1: leaderboardSummaryPercentile(ordered, 25),
    p50: leaderboardSummaryPercentile(ordered, 50),
    q3: leaderboardSummaryPercentile(ordered, 75),
    p95: leaderboardSummaryPercentile(ordered, 95),
    max: ordered[ordered.length - 1],
  };
}

function leaderboardSummaryPercentile(ordered, percentile) {
  if (ordered.length === 1) return ordered[0];
  const position = (ordered.length - 1) * (percentile / 100);
  const lowerIndex = Math.floor(position);
  const upperIndex = Math.ceil(position);
  if (lowerIndex === upperIndex) return ordered[lowerIndex];
  return ordered[lowerIndex] + (ordered[upperIndex] - ordered[lowerIndex]) * (position - lowerIndex);
}

function leaderboardSummaryBoxplotPositions(distribution) {
  const min = Number(distribution.min);
  const max = Number(distribution.max);
  if (min === max) {
    return { min: 45, q1: 48, p50: 50, q3: 52, p95: 50, max: 55, flat: true };
  }
  const pct = value => Number(Math.max(0, Math.min(100, ((Number(value) - min) / (max - min)) * 100)).toFixed(2));
  return {
    min: pct(distribution.min),
    q1: pct(distribution.q1),
    p50: pct(distribution.p50),
    q3: pct(distribution.q3),
    p95: pct(distribution.p95),
    max: pct(distribution.max),
    flat: false,
  };
}

function leaderboardSummaryValue(row, value) {
  if (!hasMetricValue(value)) return "-";
  if (row.type === "duration") return fmtMs(value);
  if (row.type === "percent") return fmtPct(value);
  return fmtNum(value);
}

function summaryNumber(value) {
  if (!hasMetricValue(value)) return null;
  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}
