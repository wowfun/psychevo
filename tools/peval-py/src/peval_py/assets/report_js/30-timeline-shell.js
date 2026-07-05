const TIMELINE_INPUT_STAGE_THRESHOLD_MS = 50;
const TIMELINE_ESTIMATED_MODEL_CALL_CAP_MS = 600000;
function renderTimelineDiagnostics(trajectory, meta) {
  const trace = timelineTrace(trajectory, meta);
  const waterfall = trace.stages.length
    ? renderTimelineWaterfall(trace)
    : `<p class="timeline-empty">${esc(t("timeline_empty", "No timed step or tool durations available."))}</p>`;
  const table = trace.stages.length
    ? renderTimelineDetailTable(trace.stages, trace.model)
    : `<p class="timeline-empty">${esc(t("timeline_empty", "No timed step or tool durations available."))}</p>`;
  return `<section class="selected-extra timeline-diagnostics">
    ${renderTimelineSection("timeline-waterfall-section", t("timeline_waterfall", "Timeline Waterfall"), t("timeline_waterfall_copy", "Flat active-latency trace for meaningful delay."), `<span class="timeline-total">${esc(fmtTimelineDuration(trace.model.active_total_ms))}</span>`, waterfall)}
    ${renderTimelineSection("timeline-table-section", t("timeline_detail_table", "Timeline Detail Table"), t("timeline_table_copy", "Flat latency stages with true wall timing."), "", table)}
  </section>`;
}
function renderTimelineSection(className, title, copy, meta, body) {
  return `<details class="timeline-section ${esc(className)}" open>
    <summary class="timeline-head"><div><h3>${esc(title)}</h3><p class="copy">${esc(copy)}</p></div>${meta || ""}</summary>
    <div class="timeline-section-body">${body}</div>
  </details>`;
}
function renderTimelineWaterfall(trace) {
  const height = Math.max(300, trace.stages.length * 34 + 96);
  return `<div class="timeline-waterfall-shell"><div class="timeline-waterfall-chart" data-timeline-chart style="height:${esc(height)}px"></div><p class="timeline-fallback" data-timeline-fallback>${esc(t("timeline_echarts_unavailable", "ECharts did not load. Timeline Waterfall is unavailable, but the detail table is still shown."))}</p></div>`;
}
function initTimelineDiagnostics(trajectory, meta) {
  const trace = timelineTrace(trajectory, meta);
  initTimelineWaterfallChart(trace);
}
function disposeTimelineChart() {
  if (!state.timelineChart) return;
  state.timelineChart.dispose();
  state.timelineChart = null;
}
function initTimelineWaterfallChart(trace) {
  const node = document.querySelector("[data-timeline-chart]");
  const fallback = document.querySelector("[data-timeline-fallback]");
  if (!node || !trace.stages.length) return;
  if (!window.echarts) {
    node.hidden = true;
    return;
  }
  node.hidden = false;
  if (fallback) fallback.hidden = true;
  node.addEventListener("click", event => event.stopPropagation());
  state.timelineChart = window.echarts.init(node, null, { renderer: "canvas" });
  state.timelineChart.setOption(timelineChartOption(trace), true);
  state.timelineChart.on("click", params => openTimelineStep(params?.data?.trace_item));
}
