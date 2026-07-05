function bindTimelineControls() {
  const target = document.querySelector(".timeline-diagnostics");
  if (!target) return;
  bindDataTableControls(target, "timeline", () => renderTrace());
  target.querySelectorAll("[data-timeline-step-id]").forEach(row => {
    const open = event => {
      event.stopPropagation();
      openTimelineStep({
        kind: "stage",
        trial_key: row.dataset.trialKey || selectedKey(),
        step_id: row.dataset.timelineStepId,
      });
    };
    row.addEventListener("click", open);
    row.addEventListener("keydown", event => {
      if (event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault();
      open(event);
    });
  });
}
function timelineActivePctValue(row, model) {
  return hasMetricValue(row.duration_ms) && positiveMetric(model.active_total_ms)
    ? Number(row.duration_ms) / Number(model.active_total_ms) * 100
    : null;
}
function renderTimelineActiveShare(row, model) {
  const value = timelineActivePctValue(row, model);
  if (!hasMetricValue(value)) return "-";
  const pct = Math.max(0, Math.min(100, Number(value)));
  const label = `${Number(value).toFixed(1)}%`;
  return `<span class="timeline-active-share" style="--active-share-pct:${esc(`${pct}%`)}" title="${esc(label)}"><span>${esc(label)}</span></span>`;
}
function timelineDetailColumns(model) {
  return [
    { key: "number", label: t("timeline_col_row", "#"), type: "number", numeric: true, sortable: true, value: row => Number(row.number), format: value => fmtNum(value) },
    { key: "stage", label: t("timeline_col_stage", "Stage"), sortable: true, filterable: true, value: row => row.stage || "-", html: row => renderTimelineStageLabel(row), cellTitle: row => row.stage || "-", className: "timeline-label-cell" },
    { key: "wall_start_ms", label: t("timeline_col_start", "Start"), type: "number", numeric: true, sortable: true, value: row => row.wall_start_ms, format: (value, row) => fmtTimelineMaybeEstimated(fmtClockMs(value), row) },
    { key: "wall_end_ms", label: t("timeline_col_end", "End"), type: "number", numeric: true, sortable: true, value: row => row.wall_end_ms, format: (value, row) => fmtTimelineMaybeEstimated(fmtClockMs(value), row) },
    { key: "duration_ms", label: t("timeline_col_duration", "Duration"), type: "number", numeric: true, sortable: true, metric: true, value: row => row.duration_ms, format: (value, row) => fmtTimelineMaybeEstimated(fmtTimelineDuration(value), row), className: "strong-num" },
    { key: "active_pct", label: t("timeline_col_total_pct", "Active Share"), type: "number", numeric: true, sortable: true, metric: true, value: row => timelineActivePctValue(row, model), format: value => hasMetricValue(value) ? `${Number(value).toFixed(1)}%` : "-", html: row => renderTimelineActiveShare(row, model), className: "active-share-cell" },
  ];
}
function renderTimelineDetailTable(rows, model) {
  const columns = timelineDetailColumns(model);
  const visibleRows = applyDataTableControls("timeline", rows, columns, rows);
  return renderDataTable({
    tableId: "timeline",
    columns,
    rows: visibleRows,
    tableClass: "timeline-table",
    shellClass: "timeline-table-shell",
    filterOptionsRows: rows,
    rowClass: row => {
      const selected = state.selectedStep?.trialKey === row.trial_key && String(state.selectedStep?.stepId) === String(row.step_id);
      return `timeline-detail-${row.kind} timeline-detail-row ${selected ? "timeline-detail-selected" : ""}`;
    },
    rowAttrs: row => `data-timeline-step-id="${esc(row.step_id || "")}" data-trial-key="${esc(row.trial_key || "")}" tabindex="0" title="${esc(t("open_step_details", "Open step details"))}: #${esc(row.step_id || "-")}"`,
  });
}
function renderTimelineStageLabel(row) {
  const meta = row.category_meta || timelineCategoryMeta("tool");
  return `<span class="timeline-stage-label timeline-category-${esc(meta.key)}" aria-label="${esc(`${meta.label}: ${row.stage}`)}">${esc(row.stage)}</span>`;
}
function fmtTimelineDuration(value) {
  if (!hasMetricValue(value)) return "-";
  const ms = Math.max(0, Number(value));
  const seconds = ms / 1000;
  if (seconds < 60) return `${seconds.toFixed(3)}s`;
  return `${Math.floor(seconds / 60)}m${(seconds % 60).toFixed(1)}s`;
}
function fmtTimelineMaybeEstimated(value, row) {
  if (!row?.estimated || value === "-") return value;
  return `≈${value}`;
}
function fmtTimelineAxis(value, intervalMs = null) {
  const ms = Math.max(0, Number(value || 0));
  const interval = hasMetricValue(intervalMs) ? Math.max(1, Number(intervalMs)) : null;
  if (ms === 0) return interval && interval < 1000 ? "0ms" : "0s";
  if (interval && interval < 1000) return `${Math.round(ms)}ms`;
  const seconds = ms / 1000;
  if (seconds < 60) {
    const decimals = interval && interval < 10000 ? 1 : 0;
    return `${seconds.toFixed(decimals)}s`;
  }
  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;
  if (interval && interval < 60000 && remainder) return `${minutes}m${Math.round(remainder)}s`;
  return `${minutes}m`;
}
function fmtClockMs(value) {
  if (!hasMetricValue(value)) return "-";
  const date = new Date(Number(value));
  const pad = (number, size = 2) => String(number).padStart(size, "0");
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}.${pad(date.getMilliseconds(), 3)}`;
}
