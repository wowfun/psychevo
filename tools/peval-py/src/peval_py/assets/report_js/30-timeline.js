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
function timelineTrace(trajectory, meta) {
  const steps = trajectory?.steps || [];
  const stepMetas = meta?.steps || [];
  const origin = timelineOriginMs(meta, stepMetas);
  const modelStage = timelineModelStageLabel(trajectory);
  const stages = [];
  const markers = [];
  let fallbackCursor = origin;
  let previousTimestamp = origin;
  steps.forEach((step, index) => {
    const stepId = step?.step_id ?? index + 1;
    const sm = stepMeta(meta, stepId);
    const stepDuration = timelineDurationMs(sm?.duration_ms);
    const stepStart = timelineStepStartMs(meta, sm, fallbackCursor);
    const stepEnd = timelineEndMs(stepStart, stepDuration);
    const source = lower(step?.source);
    if (source === "user" || source === "system") {
      if (positiveMetric(stepDuration) && Number(stepDuration) > TIMELINE_INPUT_STAGE_THRESHOLD_MS) {
        timelinePushStage(stages, {
          kind: "input",
          stage: source === "system" ? t("timeline_stage_system_processing", "System context processing") : t("timeline_stage_input_processing", "Input processing"),
          status: step?.source || "input",
          category: "io",
          wallStart: stepStart,
          wallEnd: stepEnd,
          duration: stepDuration,
          origin,
          trialKey: meta?.trial_key,
          stepId,
          ref: timelineStepRef(stepId),
        });
      } else {
        timelinePushMarker(markers, {
          name: source === "system" ? t("timeline_marker_system_context", "System context") : t("timeline_marker_user_input", "User input"),
          category: "io",
          wallStart: stepStart,
          origin,
          trialKey: meta?.trial_key,
          stepId,
          ref: timelineStepRef(stepId),
        });
      }
    } else if (source === "agent" || source === "assistant") {
      const hasExactModelDuration = positiveMetric(stepDuration);
      const modelDurationIsBoundaryEstimate = timelineModelDurationIsEstimate(sm);
      const modelEstimate = hasExactModelDuration || !timelineAllowsTimestampEstimates(meta)
        ? {}
        : timelineEstimatedModelCall(sm, step, stepStart, previousTimestamp);
      const modelDuration = hasExactModelDuration ? stepDuration : modelEstimate.duration_ms;
      timelinePushStage(stages, {
        kind: "agent",
        stage: modelStage,
        status: step?.source || "agent",
        category: "agent",
        wallStart: hasExactModelDuration ? stepStart : modelEstimate.wall_start_ms,
        wallEnd: hasExactModelDuration ? stepEnd : modelEstimate.wall_end_ms,
        duration: modelDuration,
        estimated: modelDurationIsBoundaryEstimate || (!hasExactModelDuration && modelEstimate.estimated),
        origin,
        trialKey: meta?.trial_key,
        stepId,
        ref: timelineStepRef(stepId),
      });
    } else if (positiveMetric(stepDuration)) {
      timelinePushStage(stages, {
        kind: "step",
        stage: t("timeline_stage_input_processing", "Input processing"),
        status: step?.source || "step",
        category: "tool",
        wallStart: stepStart,
        wallEnd: stepEnd,
        duration: stepDuration,
        origin,
        trialKey: meta?.trial_key,
        stepId,
        ref: timelineStepRef(stepId),
      });
    }
    (step?.tool_calls || []).forEach((tool, toolIndex) => {
      const toolMeta = toolMetaFor(sm, tool.tool_call_id);
      const toolDuration = timelineDurationMs(toolMeta?.execution_duration_ms);
      const toolStart = hasMetricValue(toolMeta?.timestamp_ms) ? Number(toolMeta.timestamp_ms) : stepStart;
      const toolEnd = timelineEndMs(toolStart, toolDuration);
      const category = timelineToolCategory(tool, toolMeta);
      timelinePushStage(stages, {
        kind: "tool",
        stage: `Tool: ${timelineToolLabel(tool, toolMeta)}`,
        status: toolMeta?.status || toolMeta?.title || "tool",
        category,
        wallStart: toolStart,
        wallEnd: toolEnd,
        duration: toolDuration,
        origin,
        trialKey: meta?.trial_key,
        stepId,
        toolCallId: tool.tool_call_id,
        ref: timelineToolRef(stepId, tool, toolIndex),
      });
    });
    fallbackCursor = Math.max(fallbackCursor, stepEnd ?? stepStart ?? fallbackCursor);
    if (hasMetricValue(stepStart)) previousTimestamp = Number(stepStart);
  });
  const orderedStages = stages.sort(timelineStageSort).map((stage, index) => ({
    ...stage,
    number: String(index + 1),
    category_meta: timelineCategoryMeta(stage.category),
  }));
  const model = timelineModel(orderedStages, markers);
  const displayStages = timelineAssignActiveOffsets(orderedStages, model);
  const displayMarkers = markers.map((marker, index) => ({
    ...marker,
    number: String(index + 1),
    category_meta: timelineCategoryMeta(marker.category),
    active_total_ms: model.active_total_ms,
    display_offset_ms: timelineMarkerActiveOffset(marker, displayStages),
  }));
  return { stages: displayStages, markers: displayMarkers, model };
}
function timelineAllowsTimestampEstimates(meta) {
  return meta?.timestamp_semantics !== "order_only";
}
function timelineModelDurationIsEstimate(stepMeta) {
  return lower(stepMeta?.duration_source) === "opencode_model_boundary_estimate";
}
function timelineAssignActiveOffsets(stages, model) {
  let cursor = 0;
  return stages.map(stage => {
    const duration = Math.max(0, Number(stage.duration_ms || 0));
    const out = {
      ...stage,
      active_total_ms: model.active_total_ms,
      display_start_ms: cursor,
      display_end_ms: cursor + duration,
    };
    cursor += duration;
    return out;
  });
}
function timelineMarkerActiveOffset(marker, stages) {
  const markerStart = Number(marker.start_offset_ms || 0);
  let cursor = 0;
  stages.forEach(stage => {
    if (Number(stage.start_offset_ms || 0) < markerStart) {
      cursor = Math.max(cursor, Number(stage.display_end_ms || 0));
    }
  });
  return cursor;
}
function timelinePushStage(stages, args) {
  const stage = timelineStage(args);
  if (!timelineStageHasMeasuredDuration(stage) || !hasMetricValue(stage.start_offset_ms)) return;
  stages.push(stage);
}
function timelineStageHasMeasuredDuration(stage) {
  if (!stage || !hasMetricValue(stage.duration_ms)) return false;
  return stage.kind === "tool" ? Number(stage.duration_ms) >= 0 : positiveMetric(stage.duration_ms);
}
function timelinePushMarker(markers, args) {
  const marker = timelineMarker(args);
  if (!marker || !hasMetricValue(marker.start_offset_ms)) return;
  markers.push(marker);
}
function timelineStage({ kind, stage, status, category, wallStart, wallEnd, duration, estimated, origin, trialKey, stepId, toolCallId, ref }) {
  const startOffset = hasMetricValue(wallStart) && hasMetricValue(origin) ? Math.max(0, Number(wallStart) - Number(origin)) : null;
  const endOffset = hasMetricValue(wallEnd) && hasMetricValue(origin) ? Math.max(startOffset || 0, Number(wallEnd) - Number(origin)) : startOffset;
  return {
    kind,
    stage,
    status,
    category,
    trial_key: trialKey,
    step_id: stepId,
    tool_call_id: toolCallId,
    wall_start_ms: wallStart,
    wall_end_ms: wallEnd,
    start_offset_ms: startOffset,
    end_offset_ms: endOffset,
    duration_ms: duration,
    estimated: Boolean(estimated),
    ref,
  };
}
function timelineModelStageLabel(trajectory) {
  const model = trajectory?.agent?.model_name;
  return model ? `Model: ${model}` : t("timeline_stage_model", "Model");
}
function timelineEstimatedModelCall(stepMeta, step, stepStart, previousTimestamp) {
  if (!hasMetricValue(stepStart)) return {};
  const start = Number(stepStart);
  const toolStarts = (step?.tool_calls || [])
    .map(tool => toolMetaFor(stepMeta, tool.tool_call_id)?.timestamp_ms)
    .filter(hasMetricValue)
    .map(Number)
    .filter(value => value > start);
  if (toolStarts.length) {
    const end = Math.min(...toolStarts);
    const duration = timelineEstimatedDurationMs(start, end);
    if (positiveMetric(duration)) {
      return {
        wall_start_ms: start,
        wall_end_ms: end,
        duration_ms: duration,
        estimated: true,
      };
    }
  }
  const previous = hasMetricValue(previousTimestamp) ? Number(previousTimestamp) : null;
  if (previous !== null && previous < start) {
    const duration = timelineEstimatedDurationMs(previous, start);
    if (positiveMetric(duration)) {
      return {
        wall_start_ms: previous,
        wall_end_ms: start,
        duration_ms: duration,
        estimated: true,
      };
    }
  }
  return {};
}
function timelineEstimatedDurationMs(start, end) {
  if (!hasMetricValue(start) || !hasMetricValue(end)) return null;
  const duration = Number(end) - Number(start);
  return duration > 0 && duration <= TIMELINE_ESTIMATED_MODEL_CALL_CAP_MS ? duration : null;
}
function timelineMarker({ name, category, wallStart, origin, trialKey, stepId, ref }) {
  if (!hasMetricValue(wallStart)) return null;
  const startOffset = hasMetricValue(origin) ? Math.max(0, Number(wallStart) - Number(origin)) : null;
  return {
    kind: "marker",
    name,
    category,
    trial_key: trialKey,
    step_id: stepId,
    wall_start_ms: wallStart,
    start_offset_ms: startOffset,
    ref,
  };
}
function timelineOriginMs(meta, stepMetas) {
  if (hasMetricValue(meta?.started_at_ms)) return Number(meta.started_at_ms);
  const timestamps = (stepMetas || []).map(step => step?.timestamp_ms).filter(hasMetricValue).map(Number);
  return timestamps.length ? Math.min(...timestamps) : 0;
}
function timelineStepStartMs(meta, sm, fallback) {
  if (hasMetricValue(sm?.timestamp_ms)) return Number(sm.timestamp_ms);
  if (hasMetricValue(meta?.started_at_ms) && hasMetricValue(sm?.elapsed_ms)) return Number(meta.started_at_ms) + Number(sm.elapsed_ms);
  return hasMetricValue(fallback) ? Number(fallback) : null;
}
function timelineEndMs(start, duration) {
  if (!hasMetricValue(start)) return null;
  return Number(start) + (hasMetricValue(duration) ? Number(duration) : 0);
}
function timelineToolLabel(tool, toolMeta) {
  return String(tool?.function_name || toolMeta?.title || tool?.tool_call_id || "tool");
}
function timelineStepRef(stepId) {
  return `step ${stepId}`;
}
function timelineToolRef(stepId, tool, toolIndex) {
  return `step ${stepId} / ${tool?.tool_call_id || `tool ${toolIndex + 1}`}`;
}
function timelineDurationMs(value) {
  return hasMetricValue(value) ? Math.max(0, Number(value)) : null;
}
function timelineToolCategory(tool, toolMeta) {
  if (toolFailed(toolMeta)) return "error";
  const text = lower(`${tool?.function_name || ""} ${toolMeta?.title || ""}`);
  if (/(search|http|web|fetch|browser|mcp|curl|wget|request)/.test(text)) return "network";
  if (/(shell|terminal|exec|python|query|task|bash|sh|cmd|command|subprocess)/.test(text)) return "external";
  if (/(file|read|write|glob|grep|list|open|cat|sed|rg|ls|path|fs)/.test(text)) return "io";
  return "tool";
}
function timelineCategoryMeta(key) {
  const items = {
    io: { key: "io", label: t("timeline_category_io", "I/O"), color: "#3b82f6" },
    agent: { key: "agent", label: t("timeline_category_agent", "Agent"), color: "#7c3aed" },
    network: { key: "network", label: t("timeline_category_network", "Network"), color: "#f59e0b" },
    external: { key: "external", label: t("timeline_category_external", "External"), color: "#64748b" },
    tool: { key: "tool", label: t("timeline_category_tool", "Tool"), color: "#0891b2" },
    error: { key: "error", label: t("timeline_category_error", "Error"), color: "#dc2626" },
  };
  return items[key] || items.tool;
}
function timelineStageSort(left, right) {
  return Number(left.start_offset_ms || 0) - Number(right.start_offset_ms || 0)
    || Number(right.duration_ms || 0) - Number(left.duration_ms || 0)
    || String(left.stage || "").localeCompare(String(right.stage || ""));
}
function timelineModel(stages) {
  const activeTotal = stages.reduce((sum, stage) => sum + Math.max(0, Number(stage.duration_ms || 0)), 0);
  return {
    active_total_ms: activeTotal,
    display_total_ms: Math.max(1, activeTotal),
  };
}
function timelineChartOption(trace) {
  const labels = trace.stages.map(stage => `#${stage.number} ${stage.stage}`);
  const labelWidth = timelineYAxisLabelWidth(labels);
  const xAxisScale = timelineXAxisScale(trace.model.display_total_ms);
  return {
    animation: false,
    grid: { left: labelWidth + 18, right: 28, top: 38, bottom: 48 },
    tooltip: {
      trigger: "item",
      confine: true,
      borderWidth: 1,
      borderColor: "#d5cdbb",
      backgroundColor: "#fffdf8",
      textStyle: { color: "#27231b", fontFamily: "system-ui,-apple-system,BlinkMacSystemFont,\"Segoe UI\",sans-serif", fontSize: 12 },
      formatter: timelineTooltipFormatter,
    },
    xAxis: {
      type: "value",
      min: 0,
      max: xAxisScale.max,
      interval: xAxisScale.interval,
      minInterval: xAxisScale.interval,
      axisLine: { lineStyle: { color: "#d9e2ef" } },
      axisTick: { show: true, lineStyle: { color: "#d9e2ef" } },
      axisLabel: {
        color: "#6b7280",
        fontSize: 12,
        hideOverlap: true,
        formatter: value => fmtTimelineAxis(value, xAxisScale.interval),
      },
      splitLine: { show: true, lineStyle: { color: "#edf2f7" } },
    },
    yAxis: {
      type: "category",
      inverse: true,
      data: labels,
      axisTick: { show: false },
      axisLine: { lineStyle: { color: "#d9e2ef" } },
      axisLabel: {
        color: "#526581",
        fontSize: 12,
        fontFamily: "system-ui,-apple-system,BlinkMacSystemFont,\"Segoe UI\",sans-serif",
        width: labelWidth,
        overflow: "truncate",
      },
      splitLine: { show: true, lineStyle: { color: "#f6f1e8" } },
    },
    series: [
      {
        name: t("timeline_waterfall", "Timeline Waterfall"),
        type: "custom",
        renderItem: timelineBarRenderItem,
        cursor: "pointer",
        encode: { x: [0, 1], y: 2 },
        data: trace.stages.map((stage, index) => ({
          value: [stage.display_start_ms, stage.display_end_ms, index, stage.duration_ms, stage.category_meta.color, fmtTimelineDuration(stage.duration_ms)],
          itemStyle: { color: stage.category_meta.color },
          trace_item: stage,
        })),
      },
      {
        name: t("timeline_markers", "Markers"),
        type: "custom",
        renderItem: timelineMarkerRenderItem,
        cursor: "pointer",
        data: trace.markers.map(marker => ({
          value: [marker.display_offset_ms, 0],
          trace_item: marker,
        })),
      },
    ],
  };
}
function timelineYAxisLabelWidth(labels) {
  const longest = labels.reduce((max, label) => Math.max(max, String(label || "").length), 0);
  return Math.max(100, Math.min(158, longest * 7 + 16));
}
function timelineXAxisScale(totalMs) {
  const total = Math.max(1, Number(totalMs || 0));
  const interval = timelineNiceIntervalMs(total / 5);
  return {
    interval,
    max: Math.max(interval, Math.ceil(total / interval) * interval),
  };
}
function timelineNiceIntervalMs(targetMs) {
  const target = Math.max(1, Number(targetMs || 0));
  const magnitude = Math.pow(10, Math.floor(Math.log10(target)));
  for (const factor of [1, 2, 2.5, 5, 10]) {
    const interval = factor * magnitude;
    if (interval >= target) return Math.max(1, interval);
  }
  return Math.max(1, 10 * magnitude);
}
function timelineBarRenderItem(params, api) {
  const start = api.coord([api.value(0), api.value(2)]);
  const end = api.coord([api.value(1), api.value(2)]);
  const bandHeight = api.size([0, 1])[1];
  const barHeight = Math.max(7, Math.min(22, bandHeight * 0.62));
  const color = api.value(4) || "#0891b2";
  const shape = window.echarts.graphic.clipRectByRect({
    x: start[0],
    y: start[1] - barHeight / 2,
    width: Math.max(2, end[0] - start[0]),
    height: barHeight,
  }, {
    x: params.coordSys.x,
    y: params.coordSys.y,
    width: params.coordSys.width,
    height: params.coordSys.height,
  });
  if (!shape) return null;
  const label = api.value(5) || fmtTimelineDuration(api.value(3));
  const labelInside = shape.width >= 58;
  const chartRight = params.coordSys.x + params.coordSys.width;
  const labelOutsideRight = !labelInside && shape.x + shape.width + 58 >= chartRight;
  const labelX = labelInside
    ? shape.x + Math.min(8, Math.max(3, shape.width / 4))
    : (labelOutsideRight ? chartRight - 4 : shape.x + shape.width + 6);
  const textAlign = labelInside || !labelOutsideRight ? "left" : "right";
  return {
    type: "group",
    children: [
      {
        type: "rect",
        shape,
        style: api.style({ fill: color }),
      },
      {
        type: "text",
        style: {
          x: labelX,
          y: shape.y + shape.height / 2,
          text: label,
          fill: labelInside ? "#fffdf8" : color,
          font: "700 12px ui-monospace,SFMono-Regular,Menlo,Consolas,\"Liberation Mono\",monospace",
          textAlign,
          textVerticalAlign: "middle",
        },
        silent: true,
      },
    ],
  };
}
function timelineMarkerRenderItem(params, api) {
  const x = api.coord([api.value(0), 0])[0];
  const top = params.coordSys.y;
  const bottom = top + params.coordSys.height;
  return {
    type: "group",
    children: [
      { type: "line", shape: { x1: x, y1: top, x2: x, y2: bottom }, style: { stroke: "#315f8f", lineDash: [4, 4], opacity: 0.36, lineWidth: 1 } },
      { type: "circle", shape: { cx: x, cy: top + 10, r: 4 }, style: { fill: "#fffdf8", stroke: "#315f8f", lineWidth: 2 } },
    ],
  };
}
function timelineTooltipFormatter(params) {
  const param = Array.isArray(params) ? params[0] : params;
  return timelineTooltipHtml(param?.data?.trace_item);
}
function timelineTooltipHtml(item) {
  if (!item) return "";
  const isMarker = item.kind === "marker";
  const title = isMarker ? item.name : `#${item.number} ${item.stage}`;
  const pct = !isMarker && positiveMetric(item.duration_ms) && positiveMetric(item.active_total_ms)
    ? `${(Number(item.duration_ms) / Number(item.active_total_ms) * 100).toFixed(1)}%`
    : "-";
  const rows = [
    [t("timeline_col_category", "Category"), item.category_meta?.label || "-"],
    [t("timeline_col_start", "Start"), fmtTimelineMaybeEstimated(fmtClockMs(item.wall_start_ms), item)],
    [t("timeline_active_offset", "Active offset"), fmtTimelineDuration(item.display_offset_ms ?? item.display_start_ms)],
    [t("timeline_ref", "Ref"), item.ref || "-"],
  ];
  if (!isMarker) {
    rows.splice(2, 0, [t("timeline_col_end", "End"), fmtTimelineMaybeEstimated(fmtClockMs(item.wall_end_ms), item)]);
    rows.splice(4, 0, [t("timeline_col_duration", "Duration"), fmtTimelineMaybeEstimated(fmtTimelineDuration(item.duration_ms), item)]);
    rows.splice(5, 0, [t("timeline_col_total_pct", "Active Share"), pct]);
  }
  return `<div class="timeline-tooltip"><strong>${esc(title)}</strong>${rows.map(([key, value]) => `<br><span>${esc(key)}:</span> ${esc(value)}`).join("")}</div>`;
}
function openTimelineStep(item) {
  if (!item || !item.step_id) return;
  state.selectedTrial = item.trial_key || selectedKey();
  state.selectedStep = { trialKey: state.selectedTrial, stepId: String(item.step_id) };
  renderComparisonPanels();
}
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
function renderStepsHeader(trajectory) {
  const count = (trajectory?.steps || []).length;
  return `<div class="steps-head"><h3>Steps (${count})</h3><button class="step-toggle-button" type="button" data-step-action="toggle" ${count ? "" : "disabled"}>Expand all</button></div>`;
}
function valuePreview(value) {
  if (value === null || value === undefined) return "";
  if (typeof value === "string") return value;
  return JSON.stringify(value, null, 2);
}
function renderStep(step, meta, timingStats, options = {}) {
  const sm = stepMeta(meta, step.step_id);
  const preview = stepPreviewText(step) || "(No Message)";
  return `<details class="step" data-step="${esc(step.step_id)}"${options.open ? " open" : ""}><summary><div class="step-row"><span class="step-id">#${esc(step.step_id)}</span><span class="role ${esc(step.source)}">${esc(step.source)}</span><span class="preview">${esc(preview)}</span></div><div class="rail">${renderStepRail(step, sm, meta?.trial_key, timingStats)}</div></summary><div class="step-body">${renderBlocks(step, sm, timingStats)}</div></details>`;
}
function renderBlocks(step, meta, timingStats) {
  let html = "";
  if (step.reasoning_content) html += block("Reasoning", step.reasoning_content, "reasoning-block");
  const message = valuePreview(step.message);
  if (message.trim()) html += block(step.source === "system" ? "System Prompt" : "Message", message, "message-block");
  html += renderStepActivityBlocks(step, meta, timingStats);
  return html || `<p class="copy">No visible content.</p>`;
}
function renderStepActivityBlocks(step, meta, timingStats) {
  const entries = [];
  (step.tool_calls || []).forEach(tool => {
    const toolMeta = toolMetaFor(meta, tool.tool_call_id);
    entries.push({
      timestamp: blockTimestamp(toolMeta?.timestamp_ms),
      order: entries.length,
      html: renderToolCallBlock(tool, toolMeta, timingStats),
    });
  });
  ((step.observation && step.observation.results) || []).forEach(observation => {
    const observationMeta = observationMetaFor(meta, observation.source_call_id);
    entries.push({
      timestamp: blockTimestamp(observationMeta?.timestamp_ms),
      order: entries.length,
      html: renderObservationBlock(observation, observationMeta),
    });
  });
  return entries.sort(compareStepActivityBlocks).map(entry => entry.html).join("");
}
function renderToolCallBlock(tool, toolMeta, timingStats) {
  const ratio = timingRatio(toolMeta?.execution_duration_ms, timingStats?.maxToolExecutionMs);
  return `<div class="block tool-block"><h4>Tool Calls</h4><p>${renderToolNameChip(tool, toolMeta, "", ratio)} <span class="muted">ID: ${esc(tool.tool_call_id || "-")}${toolMeta?.status ? ` / ${esc(toolMeta.status)}` : ""}${renderToolTiming(toolMeta)}</span></p><pre>${esc(valuePreview(tool.arguments || {}))}</pre></div>`;
}
function renderObservationBlock(observation, observationMeta) {
  return `<div class="block observation-block"><h4 class="${observationMeta?.tool_error ? "danger" : ""}">Observations</h4><p class="muted">Result for: ${esc(observation.source_call_id || "-")}${observationMeta?.status ? ` / ${esc(observationMeta.status)}` : ""}</p><pre>${esc(valuePreview(observation.content))}</pre></div>`;
}
function compareStepActivityBlocks(a, b) {
  if (a.timestamp !== null && b.timestamp !== null && a.timestamp !== b.timestamp) return a.timestamp - b.timestamp;
  if (a.timestamp !== null && b.timestamp === null) return -1;
  if (a.timestamp === null && b.timestamp !== null) return 1;
  return a.order - b.order;
}
function blockTimestamp(value) {
  return hasMetricValue(value) ? Number(value) : null;
}
function block(title, content, cls) { return `<div class="block ${cls}"><h4>${esc(title)}</h4><pre>${esc(content)}</pre></div>`; }
function fmtRailTokens(value) {
  if (!hasMetricValue(value)) return "-";
  const number = Number(value);
  return Math.abs(number) >= 1000 ? `${(number / 1000).toFixed(1)}k` : fmtNum(number);
}
function toolExecutionText(toolMeta) { return hasMetricValue(toolMeta?.execution_duration_ms) ? fmtMs(toolMeta.execution_duration_ms) : ""; }
function toolFailed(toolMeta) {
  const status = lower(toolMeta?.status);
  return status === "error" || status === "failed";
}
function renderToolNameChip(tool, toolMeta, extraClass = "", timingFill = null) {
  const name = tool.function_name || toolMeta?.title || "tool";
  const exec = toolExecutionText(toolMeta);
  const titleText = exec ? timeTitle("tool exec", toolMeta?.execution_duration_ms, timingFill, "slowest tool") : "";
  const title = titleText ? ` title="${esc(titleText)}"` : "";
  const execHtml = exec ? ` <span class="tool-exec-inline">${esc(exec)}</span>` : "";
  const classes = ["chip", "tool-name-chip", extraClass, toolFailed(toolMeta) ? "tool-error-chip" : "", timeGradientClass(timingFill)].filter(Boolean).join(" ");
  return `<span class="${esc(classes)}"${timeGradientStyle(timingFill)}${title}>${esc(name)}${execHtml}</span>`;
}
function renderToolTiming(toolMeta) {
  const parts = [];
  if (hasMetricValue(toolMeta?.generation_duration_ms)) parts.push(`generation ${fmtMs(toolMeta.generation_duration_ms)}`);
  return parts.length ? ` / ${parts.map(esc).join(" / ")}` : "";
}
function stepToolChips(step, meta, timingStats) {
  const chips = [];
  (step.tool_calls || []).forEach(tool => {
    const toolMeta = toolMetaFor(meta, tool.tool_call_id);
    const name = String(tool.function_name || toolMeta?.title || "").trim();
    const ratio = timingRatio(toolMeta?.execution_duration_ms, timingStats?.maxToolExecutionMs);
    if (name) chips.push(renderToolNameChip(tool, toolMeta, "rail-chip-tool-list", ratio));
  });
  return chips;
}
function renderStepRail(step, meta, trialKey, timingStats) {
  const summaryItems = [];
  const toolCalls = (step.tool_calls || []).length;
  const observations = (step.observation?.results || []).length;
  const toolErrors = toolErrorCount(meta);
  if (toolCalls || toolErrors) summaryItems.push(`<span class="rail-chip rail-chip-tools">${esc(toolCallRatio(toolCalls, toolErrors))} tools</span>`);
  else if (observations) summaryItems.push(`<span class="rail-chip rail-chip-tools">${esc(observations)} observations</span>`);
  const tokenInfo = stepTokenInfo(step, trialKey);
  if (tokenInfo) {
    const classes = ["rail-chip", "rail-chip-tokens", tokenInfo.estimated ? "rail-chip-estimated" : ""].filter(Boolean).join(" ");
    const prefix = tokenInfo.estimated ? "≈" : "";
    const title = tokenInfo.estimated ? `estimated tokens (${tokenInfo.method}; from visible step text): ${fmtNum(tokenInfo.tokens)}` : `${fmtNum(tokenInfo.tokens)} tokens`;
    summaryItems.push(`<span class="${esc(classes)}" title="${esc(title)}">${esc(`${prefix}${fmtRailTokens(tokenInfo.tokens)} tok`)}</span>`);
  }
  const stepRatio = timingRatio(meta?.duration_ms, timingStats?.maxStepDurationMs);
  const elapsedRatio = timingRatio(meta?.elapsed_ms, timingStats?.elapsedMaxMs);
  const stepClasses = ["rail-chip", "rail-chip-step-time", timeGradientClass(stepRatio)].filter(Boolean).join(" ");
  const elapsedClasses = ["rail-chip", "rail-chip-elapsed-time", timeGradientClass(elapsedRatio)].filter(Boolean).join(" ");
  const time = `<div class="rail-time"><span class="${esc(stepClasses)}"${timeGradientStyle(stepRatio)} title="${esc(timeTitle("step span", meta?.duration_ms, stepRatio, "slowest step"))}">step ${esc(fmtMs(meta?.duration_ms))}</span><span class="${esc(elapsedClasses)}"${timeGradientStyle(elapsedRatio)} title="${esc(timeTitle("elapsed", meta?.elapsed_ms, elapsedRatio, "trajectory"))}">elapsed ${esc(fmtMs(meta?.elapsed_ms))}</span></div>`;
  const summary = `<div class="rail-summary">${summaryItems.join("")}${time}</div>`;
  const toolChips = stepToolChips(step, meta, timingStats);
  return `${summary}${toolChips.length ? `<div class="rail-tool-row">${toolChips.join("")}</div>` : ""}`;
}
function toolCallRatio(total, errors) {
  const callTotal = Math.max(0, Number(total || 0));
  const errorTotal = Math.max(0, Number(errors || 0));
  return `${Math.max(0, callTotal - errorTotal)}/${callTotal}`;
}
function toolErrorCount(meta) {
  return listValue(meta?.tool_calls).filter(tool => lower(tool?.status) === "error").length;
}
function toolMetaFor(meta, toolCallId) { return (meta?.tool_calls || []).find(item => item.tool_call_id === toolCallId) || null; }
function observationMetaFor(meta, sourceCallId) { return (meta?.observations || []).find(item => item.source_call_id === sourceCallId) || null; }
function stepTokenInfo(step, trialKey) {
  const exact = stepTokenTotal(step);
  if (exact !== null && exact !== undefined) return { tokens: exact, estimated: false, method: "exact" };
  const estimate = stepTokenEstimate(trialKey, step.step_id);
  if (estimate && hasMetricValue(estimate.tokens)) return { tokens: Number(estimate.tokens), estimated: true, method: estimate.method || "estimated" };
  return null;
}
function stepTokenEstimate(trialKey, stepId) {
  if (!trialKey) return null;
  return TOKEN_ESTIMATES?.[trialKey]?.[String(stepId)] || null;
}
function stepTokenTotal(step) {
  const metrics = step.metrics || {};
  const values = [metrics.prompt_tokens, metrics.completion_tokens].filter(hasMetricValue).map(Number);
  if (values.length) return values.reduce((sum, value) => sum + value, 0);
  const direct = metricExtra(metrics).usage?.total_tokens;
  return hasMetricValue(direct) ? Number(direct) : null;
}
function bindStepToggle() {
  const button = document.querySelector("[data-step-action='toggle']");
  if (!button) return;
  const rows = () => Array.from(document.querySelectorAll("#step-list .step"));
  function refresh() {
    const allOpen = rows().length > 0 && rows().every(row => row.open);
    button.textContent = allOpen ? "Collapse all" : "Expand all";
    button.setAttribute("aria-pressed", allOpen ? "true" : "false");
  }
  rows().forEach(row => row.addEventListener("toggle", refresh));
  button.addEventListener("click", () => {
    const shouldOpen = !rows().every(row => row.open);
    rows().forEach(row => { row.open = shouldOpen; });
    refresh();
  });
  refresh();
}
function renderManualNote(row) {
  const sourcePath = row?.source_ref?.relative_path;
  const meta = row?.label || sourcePath
    ? `<div class="note-meta"><strong>${esc(row?.label || t("notes", "Notes"))}</strong>${sourcePath ? `<code>${esc(sourcePath)}</code>` : ""}</div>`
    : "";
  return `<article class="manual-note">${meta}<div class="note-body">${renderMarkdown(row.markdown || "")}</div></article>`;
}
function renderMarkdown(markdown) {
  const lines = String(markdown ?? "").split(/\r?\n/);
  const out = [];
  let paragraph = [];
  let list = [];
  let code = [];
  let inCode = false;
  function flushParagraph() {
    if (paragraph.length) {
      out.push(`<p>${inlineMarkdown(paragraph.join(" "))}</p>`);
      paragraph = [];
    }
  }
  function flushList() {
    if (list.length) {
      out.push(`<ul>${list.map(item => `<li>${inlineMarkdown(item)}</li>`).join("")}</ul>`);
      list = [];
    }
  }
  function flushCode() {
    if (code.length) {
      out.push(`<pre class="note-code">${esc(code.join("\n"))}</pre>`);
      code = [];
    }
  }
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (line.trim().startsWith("```")) {
      if (inCode) {
        flushCode();
        inCode = false;
      } else {
        flushParagraph();
        flushList();
        inCode = true;
      }
      continue;
    }
    if (inCode) {
      code.push(line);
      continue;
    }
    if (!line.trim()) {
      flushParagraph();
      flushList();
      continue;
    }
    const table = markdownTableAt(lines, index);
    if (table) {
      flushParagraph();
      flushList();
      out.push(renderMarkdownTable(table));
      index = table.endIndex;
      continue;
    }
    const heading = line.match(/^(#{1,6})\s+(.+?)\s*#*\s*$/);
    if (heading) {
      flushParagraph();
      flushList();
      const rank = Math.min(3, Math.max(1, heading[1].length));
      const tag = `h${rank + 3}`;
      out.push(`<${tag} class="markdown-heading markdown-heading-${rank}">${inlineMarkdown(heading[2])}</${tag}>`);
      continue;
    }
    const bullet = line.match(/^[-*]\s+(.+)$/);
    if (bullet) {
      flushParagraph();
      list.push(bullet[1]);
      continue;
    }
    paragraph.push(line.trim());
  }
  flushParagraph();
  flushList();
  flushCode();
  return out.join("") || "<p></p>";
}
function markdownTableAt(lines, index) {
  const header = parseMarkdownTableRow(lines[index]);
  const separator = parseMarkdownTableRow(lines[index + 1]);
  if (!header || !separator || !isMarkdownTableSeparator(separator)) return null;
  const alignments = separator.map(markdownTableAlignment);
  const body = [];
  let cursor = index + 2;
  while (cursor < lines.length) {
    const row = parseMarkdownTableRow(lines[cursor]);
    if (!row) break;
    body.push(row);
    cursor += 1;
  }
  return { header, alignments, body, endIndex: cursor - 1 };
}
function parseMarkdownTableRow(line) {
  const text = String(line ?? "").trim();
  if (!text || !text.includes("|")) return null;
  let inner = text;
  if (inner.startsWith("|")) inner = inner.slice(1);
  if (inner.endsWith("|")) inner = inner.slice(0, -1);
  const cells = [];
  let current = "";
  let escaped = false;
  for (const char of inner) {
    if (escaped) {
      current += char;
      escaped = false;
      continue;
    }
    if (char === "\\") {
      escaped = true;
      continue;
    }
    if (char === "|") {
      cells.push(current.trim());
      current = "";
      continue;
    }
    current += char;
  }
  cells.push(current.trim());
  return cells.length >= 2 ? cells : null;
}
function isMarkdownTableSeparator(cells) {
  return cells.length >= 2 && cells.every(cell => /^:?-{3,}:?$/.test(String(cell || "").trim()));
}
function markdownTableAlignment(value) {
  const text = String(value || "").trim();
  if (/^:-+:$/.test(text)) return "center";
  if (/^-+:$/.test(text)) return "right";
  if (/^:-+$/.test(text)) return "left";
  return "";
}
function renderMarkdownTable(table) {
  const width = table.header.length;
  const header = `<tr>${normalizedMarkdownTableRow(table.header, width).map((cell, index) => renderMarkdownTableCell("th", cell, table.alignments[index])).join("")}</tr>`;
  const body = table.body.length
    ? `<tbody>${table.body.map(row => `<tr>${normalizedMarkdownTableRow(row, width).map((cell, index) => renderMarkdownTableCell("td", cell, table.alignments[index])).join("")}</tr>`).join("")}</tbody>`
    : "";
  return `<div class="markdown-table-wrap"><table class="markdown-table"><thead>${header}</thead>${body}</table></div>`;
}
function normalizedMarkdownTableRow(row, width) {
  return Array.from({ length: width }, (_, index) => row[index] ?? "");
}
function renderMarkdownTableCell(tag, value, alignment) {
  const classAttr = alignment ? ` class="align-${alignment}"` : "";
  return `<${tag}${classAttr}>${inlineMarkdown(value)}</${tag}>`;
}
function inlineMarkdown(value) {
  const parts = [];
  const text = String(value ?? "");
  let cursor = 0;
  text.replace(/`([^`]+)`/g, (match, code, offset) => {
    if (offset > cursor) parts.push(renderInlineMarkdownText(text.slice(cursor, offset)));
    parts.push(`<code>${esc(code)}</code>`);
    cursor = offset + match.length;
    return match;
  });
  if (cursor < text.length) parts.push(renderInlineMarkdownText(text.slice(cursor)));
  return parts.join("");
}
function renderInlineMarkdownText(value) {
  return esc(value)
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/__([^_]+)__/g, "<strong>$1</strong>")
    .replace(/(^|[^\w])\*([^*\s][^*]*?)\*/g, "$1<em>$2</em>")
    .replace(/(^|[^\w])_([^_\s][^_]*?)_/g, "$1<em>$2</em>");
}
render(data());
