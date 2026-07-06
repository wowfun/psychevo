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
  const title = isMarker ? `#${item.number} ${item.name}` : `#${item.number} ${item.stage}`;
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
