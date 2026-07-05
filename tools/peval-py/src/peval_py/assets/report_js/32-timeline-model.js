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
