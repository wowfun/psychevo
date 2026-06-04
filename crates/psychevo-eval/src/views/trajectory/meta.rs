#[allow(unused_imports)]
use super::*;

pub(crate) fn atif_prompt_unavailable(atif: &AtifTrajectory) -> bool {
    atif.extra
        .as_ref()
        .and_then(|extra| extra.get("prompt_unavailable"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(crate) fn view_trajectory_steps(atif: &AtifTrajectory) -> Vec<ViewTrajectoryStepMeta> {
    let first_timestamp = atif.steps.iter().filter_map(atif_step_timestamp_ms).next();
    atif.steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let timestamp_ms = atif_step_timestamp_ms(step);
            let elapsed_ms = timestamp_ms
                .zip(first_timestamp)
                .map(|(timestamp, first)| timestamp.saturating_sub(first));
            let next_timestamp_ms = atif
                .steps
                .iter()
                .skip(index + 1)
                .find_map(atif_step_timestamp_ms);
            let duration_ms = step_duration_ms(step, timestamp_ms, next_timestamp_ms);
            view_trajectory_step(step, timestamp_ms, elapsed_ms, duration_ms)
        })
        .collect()
}

pub(crate) fn step_duration_ms(
    step: &AtifStep,
    timestamp_ms: Option<u128>,
    next_timestamp_ms: Option<u128>,
) -> Option<u128> {
    let timestamp_ms = timestamp_ms?;
    let grouped_end_ms = grouped_step_end_timestamp_ms(step);
    atif_step_end_timestamp_ms(step)
        .into_iter()
        .chain(grouped_end_ms)
        .max()
        .or_else(|| {
            (step.source == "agent")
                .then_some(next_timestamp_ms)
                .flatten()
        })
        .map(|end| end.saturating_sub(timestamp_ms))
}

pub(crate) fn view_trajectory_step(
    step: &AtifStep,
    timestamp_ms: Option<u128>,
    elapsed_ms: Option<u128>,
    duration_ms: Option<u128>,
) -> ViewTrajectoryStepMeta {
    let tool_error = step
        .observation
        .as_ref()
        .is_some_and(|observation| observation.results.iter().any(observation_result_is_error));
    let (_message_preview, message_truncated) = atif_value_preview(&step.message);
    let reasoning_truncated = step
        .reasoning_content
        .as_deref()
        .map(|reasoning| redacted_preview(reasoning).1)
        .unwrap_or(false);
    let observations = step
        .observation
        .as_ref()
        .map(|observation| {
            observation
                .results
                .iter()
                .map(view_trajectory_observation_meta)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let tool_calls = step
        .tool_calls
        .iter()
        .map(|tool| view_trajectory_tool_meta(tool, &observations))
        .collect::<Vec<_>>();
    ViewTrajectoryStepMeta {
        step_id: step.step_id,
        tool_calls,
        observations,
        tool_error,
        timestamp_ms,
        elapsed_ms,
        duration_ms,
        data_preview: step
            .extra
            .as_ref()
            .and_then(|extra| serde_json::to_string(extra).ok())
            .map(|value| {
                truncate_chars_with_flag(
                    &redact_preview_text(&value),
                    TRAJECTORY_DATA_PREVIEW_CHARS,
                )
                .0
            }),
        truncated: message_truncated || reasoning_truncated,
    }
}

pub(crate) fn view_trajectory_tool_meta(
    tool: &AtifToolCall,
    observations: &[ViewTrajectoryObservationMeta],
) -> ViewTrajectoryToolMeta {
    let raw = serde_json::to_string_pretty(&tool.arguments).unwrap_or_default();
    let (_, truncated) = redacted_preview(&raw);
    let timestamp_ms = extra_u128(tool.extra.as_ref(), "timestamp_ms");
    let execution_start_ms = extra_u128(tool.extra.as_ref(), "execution_start_timestamp_ms");
    let runtime_execution_duration_ms = extra_u128(tool.extra.as_ref(), "execution_duration_ms");
    let observation_timestamp_ms = observations
        .iter()
        .find(|observation| {
            observation
                .source_call_id
                .as_deref()
                .is_some_and(|id| id == tool.tool_call_id)
        })
        .and_then(|observation| observation.timestamp_ms);
    let fallback_execution_duration_ms = execution_start_ms
        .zip(observation_timestamp_ms)
        .map(|(execution_start, finished)| finished.saturating_sub(execution_start));
    let (execution_duration_ms, execution_duration_source) =
        if let Some(duration_ms) = runtime_execution_duration_ms {
            (Some(duration_ms), Some("runtime_meta".to_string()))
        } else if let Some(duration_ms) = fallback_execution_duration_ms {
            (Some(duration_ms), Some("event_timestamps".to_string()))
        } else {
            (None, None)
        };
    ViewTrajectoryToolMeta {
        tool_call_id: tool.tool_call_id.clone(),
        status: extra_string(tool.extra.as_ref(), "status"),
        title: extra_string(tool.extra.as_ref(), "title"),
        timestamp_ms,
        execution_start_ms,
        generation_duration_ms: timestamp_ms
            .zip(execution_start_ms)
            .map(|(start, execution_start)| execution_start.saturating_sub(start)),
        execution_duration_ms,
        execution_duration_source,
        truncated,
    }
}

pub(crate) fn view_trajectory_observation_meta(
    result: &AtifObservationResult,
) -> ViewTrajectoryObservationMeta {
    let content_truncated = result
        .content
        .as_ref()
        .map(atif_value_preview)
        .map(|(_, truncated)| truncated)
        .unwrap_or(false);
    let extra_truncated = result
        .extra
        .as_ref()
        .and_then(|extra| extra.get("truncated"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    ViewTrajectoryObservationMeta {
        source_call_id: result.source_call_id.clone(),
        status: extra_string(result.extra.as_ref(), "status"),
        title: extra_string(result.extra.as_ref(), "title"),
        timestamp_ms: extra_u128(result.extra.as_ref(), "timestamp_ms"),
        tool_error: observation_result_is_error(result),
        truncated: content_truncated || extra_truncated,
    }
}

pub(crate) fn atif_value_preview(value: &Value) -> (String, bool) {
    match value {
        Value::String(text) => redacted_preview(text),
        value => redacted_preview(&serde_json::to_string_pretty(value).unwrap_or_default()),
    }
}

pub(crate) fn redacted_preview(value: &str) -> (String, bool) {
    truncate_chars_with_flag(&redact_preview_text(value), TRAJECTORY_DATA_PREVIEW_CHARS)
}

pub(crate) fn atif_step_timestamp_ms(step: &AtifStep) -> Option<u128> {
    step.extra
        .as_ref()
        .and_then(|extra| extra.get("timestamp_ms"))
        .and_then(json_u128)
}

pub(crate) fn atif_step_end_timestamp_ms(step: &AtifStep) -> Option<u128> {
    step.extra
        .as_ref()
        .and_then(|extra| extra.get("end_timestamp_ms"))
        .and_then(json_u128)
}

pub(crate) fn grouped_step_end_timestamp_ms(step: &AtifStep) -> Option<u128> {
    let observation_end = step
        .observation
        .as_ref()
        .into_iter()
        .flat_map(|observation| observation.results.iter())
        .filter_map(|result| extra_u128(result.extra.as_ref(), "timestamp_ms"))
        .max();
    let tool_end = step
        .tool_calls
        .iter()
        .filter_map(tool_end_timestamp_ms)
        .max();
    observation_end.into_iter().chain(tool_end).max()
}

pub(crate) fn tool_end_timestamp_ms(tool: &AtifToolCall) -> Option<u128> {
    let extra = tool.extra.as_ref();
    let execution_duration_ms = extra_u128(extra, "execution_duration_ms")?;
    extra_u128(extra, "execution_start_timestamp_ms")
        .or_else(|| extra_u128(extra, "timestamp_ms"))
        .map(|started| started.saturating_add(execution_duration_ms))
}

pub(crate) fn extra_string(extra: Option<&Value>, key: &str) -> Option<String> {
    extra
        .and_then(|extra| extra.get(key))
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}

pub(crate) fn extra_u128(extra: Option<&Value>, key: &str) -> Option<u128> {
    extra.and_then(|extra| extra.get(key)).and_then(json_u128)
}

pub(crate) fn json_u128(value: &Value) -> Option<u128> {
    value
        .as_u64()
        .map(u128::from)
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

pub(crate) fn observation_result_is_error(result: &AtifObservationResult) -> bool {
    result
        .extra
        .as_ref()
        .and_then(|extra| extra.get("status"))
        .and_then(Value::as_str)
        .is_some_and(|status| {
            status.eq_ignore_ascii_case("error") || status.eq_ignore_ascii_case("failed")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_duration_uses_current_step_span_not_previous_gap() {
        let atif = AtifTrajectory {
            schema_version: "ATIF-v1.7".to_string(),
            session_id: Some("session".to_string()),
            trajectory_id: Some("trial:t001".to_string()),
            agent: AtifAgent {
                name: "agent".to_string(),
                version: "test".to_string(),
                model_name: None,
                extra: None,
            },
            steps: vec![
                timed_step(1, "user", 1_000, None),
                timed_step(2, "agent", 2_000, Some(2_400)),
                timed_step(3, "agent", 20_000, Some(20_900)),
            ],
            notes: None,
            final_metrics: None,
            extra: None,
        };

        let steps = view_trajectory_steps(&atif);

        assert_eq!(steps[0].duration_ms, None);
        assert_eq!(steps[1].duration_ms, Some(400));
        assert_eq!(steps[2].duration_ms, Some(900));
        assert_eq!(steps[2].elapsed_ms, Some(19_000));
    }

    #[test]
    fn step_duration_uses_grouped_observation_end_before_next_step_fallback() {
        let atif = AtifTrajectory {
            schema_version: "ATIF-v1.7".to_string(),
            session_id: Some("session".to_string()),
            trajectory_id: Some("trial:t001".to_string()),
            agent: AtifAgent {
                name: "agent".to_string(),
                version: "test".to_string(),
                model_name: None,
                extra: None,
            },
            steps: vec![
                AtifStep {
                    step_id: 1,
                    source: "agent".to_string(),
                    message: Value::String(String::new()),
                    reasoning_content: None,
                    tool_calls: vec![AtifToolCall {
                        tool_call_id: "call-1".to_string(),
                        function_name: "read".to_string(),
                        arguments: json!({ "path": "add.py" }),
                        extra: Some(json!({ "timestamp_ms": 1_100 })),
                    }],
                    observation: Some(AtifObservation {
                        results: vec![AtifObservationResult {
                            source_call_id: Some("call-1".to_string()),
                            content: Some(json!({ "ok": true })),
                            extra: Some(json!({ "timestamp_ms": 1_800 })),
                        }],
                    }),
                    metrics: None,
                    extra: Some(json!({ "timestamp_ms": 1_000 })),
                    llm_call_count: Some(1),
                },
                timed_step(2, "agent", 5_000, Some(5_100)),
            ],
            notes: None,
            final_metrics: None,
            extra: None,
        };

        let steps = view_trajectory_steps(&atif);

        assert_eq!(steps[0].duration_ms, Some(800));
        assert_eq!(steps[1].duration_ms, Some(100));
    }

    #[test]
    fn tool_timing_meta_separates_generation_and_execution() {
        let atif = AtifTrajectory {
            schema_version: "ATIF-v1.7".to_string(),
            session_id: Some("session".to_string()),
            trajectory_id: Some("trial:t001".to_string()),
            agent: AtifAgent {
                name: "agent".to_string(),
                version: "test".to_string(),
                model_name: None,
                extra: None,
            },
            steps: vec![AtifStep {
                step_id: 1,
                source: "agent".to_string(),
                message: Value::String(String::new()),
                reasoning_content: Some("Need edit.".to_string()),
                tool_calls: vec![AtifToolCall {
                    tool_call_id: "call-1".to_string(),
                    function_name: "edit".to_string(),
                    arguments: json!({ "path": "add.py" }),
                    extra: Some(json!({
                        "timestamp_ms": 1_100,
                        "execution_start_timestamp_ms": 2_000,
                    })),
                }],
                observation: Some(AtifObservation {
                    results: vec![AtifObservationResult {
                        source_call_id: Some("call-1".to_string()),
                        content: Some(json!({ "success": true })),
                        extra: Some(json!({ "timestamp_ms": 2_005 })),
                    }],
                }),
                metrics: None,
                extra: Some(json!({
                    "timestamp_ms": 1_000,
                    "end_timestamp_ms": 2_005,
                })),
                llm_call_count: Some(1),
            }],
            notes: None,
            final_metrics: None,
            extra: None,
        };

        let steps = view_trajectory_steps(&atif);

        assert_eq!(steps[0].duration_ms, Some(1_005));
        assert_eq!(steps[0].tool_calls[0].generation_duration_ms, Some(900));
        assert_eq!(steps[0].tool_calls[0].execution_duration_ms, Some(5));
        assert_eq!(
            steps[0].tool_calls[0].execution_duration_source.as_deref(),
            Some("event_timestamps")
        );
    }

    #[test]
    fn tool_timing_meta_prefers_runtime_execution_duration() {
        let atif = AtifTrajectory {
            schema_version: "ATIF-v1.7".to_string(),
            session_id: Some("session".to_string()),
            trajectory_id: Some("trial:t001".to_string()),
            agent: AtifAgent {
                name: "agent".to_string(),
                version: "test".to_string(),
                model_name: None,
                extra: None,
            },
            steps: vec![AtifStep {
                step_id: 1,
                source: "agent".to_string(),
                message: Value::String(String::new()),
                reasoning_content: Some("Need edit.".to_string()),
                tool_calls: vec![AtifToolCall {
                    tool_call_id: "call-1".to_string(),
                    function_name: "edit".to_string(),
                    arguments: json!({ "path": "add.py" }),
                    extra: Some(json!({
                        "timestamp_ms": 1_100,
                        "execution_start_timestamp_ms": 2_000,
                        "execution_duration_ms": 321,
                        "execution_duration_source": "runtime_meta",
                    })),
                }],
                observation: Some(AtifObservation {
                    results: vec![AtifObservationResult {
                        source_call_id: Some("call-1".to_string()),
                        content: Some(json!({ "success": true })),
                        extra: Some(json!({ "timestamp_ms": 2_000 })),
                    }],
                }),
                metrics: None,
                extra: Some(json!({
                    "timestamp_ms": 1_000,
                    "end_timestamp_ms": 2_000,
                })),
                llm_call_count: Some(1),
            }],
            notes: None,
            final_metrics: None,
            extra: None,
        };

        let steps = view_trajectory_steps(&atif);

        assert_eq!(steps[0].tool_calls[0].execution_duration_ms, Some(321));
        assert_eq!(
            steps[0].tool_calls[0].execution_duration_source.as_deref(),
            Some("runtime_meta")
        );
    }

    fn timed_step(
        step_id: u64,
        source: &str,
        timestamp_ms: u128,
        end_timestamp_ms: Option<u128>,
    ) -> AtifStep {
        let mut extra = json!({ "timestamp_ms": timestamp_ms });
        if let Some(end_timestamp_ms) = end_timestamp_ms {
            extra["end_timestamp_ms"] = json!(end_timestamp_ms);
        }
        AtifStep {
            step_id,
            source: source.to_string(),
            message: Value::String(String::new()),
            reasoning_content: None,
            tool_calls: Vec::new(),
            observation: None,
            metrics: None,
            extra: Some(extra),
            llm_call_count: None,
        }
    }
}
