from __future__ import annotations

from typing import Any

from peval_py.adapters.base import (
    ObservationMeta,
    StepMeta,
    ToolMeta,
    timestamp_fallback_allowed,
    timestamp_fallback_duration_ms,
)


def trial_active_duration_ms(
    steps: list[StepMeta],
    step_reports: list[dict[str, Any]],
) -> int | None:
    total = 0
    observed = False
    for step, report in zip(steps, step_reports, strict=True):
        if step.source != "agent":
            continue
        duration = report.get("duration_ms")
        if duration is not None:
            observed = True
            total += int(duration)
        for tool in step.tool_calls:
            if tool.execution_duration_ms is not None:
                observed = True
                total += int(tool.execution_duration_ms)
    return total if observed else None


def step_meta_reports(
    steps: list[StepMeta],
    started: int,
    timestamp_semantics: str | None,
) -> list[dict[str, Any]]:
    reports = []
    for index, step in enumerate(steps):
        timestamp = step.timestamp_ms
        elapsed = timestamp - started if timestamp is not None and started else None
        next_timestamp = next(
            (
                candidate.timestamp_ms
                for candidate in steps[index + 1 :]
                if candidate.timestamp_ms is not None
            ),
            None,
        )
        duration = step_duration_ms(step, next_timestamp, timestamp_semantics)
        reports.append(
            {
                "step_id": step.step_id,
                "tool_calls": [tool_meta_report(tool) for tool in step.tool_calls],
                "observations": [
                    observation_meta_report(observation)
                    for observation in step.observations
                ],
                "tool_error": step.tool_error,
                **optional("timestamp_ms", timestamp),
                **optional("elapsed_ms", elapsed),
                "duration_ms": duration,
                **optional("duration_source", step.duration_source),
                "truncated": step.truncated,
            }
        )
    return reports


def step_duration_ms(
    step: StepMeta,
    next_timestamp_ms: int | None,
    timestamp_semantics: str | None,
) -> int | None:
    timestamp_ms = step.timestamp_ms
    if timestamp_ms is None:
        return step.duration_ms
    if step.duration_ms is not None:
        return max(0, step.duration_ms)
    if (
        step.source == "agent"
        and next_timestamp_ms is not None
        and not step.tool_calls
        and timestamp_fallback_allowed(timestamp_semantics)
    ):
        return timestamp_fallback_duration_ms(timestamp_ms, next_timestamp_ms)
    return None


def grouped_step_end_timestamp_ms(step: StepMeta, start_ms: int) -> int | None:
    end_candidates: list[int] = []
    for observation in step.observations:
        duration = timestamp_fallback_duration_ms(start_ms, observation.timestamp_ms)
        if duration is not None:
            end_candidates.append(start_ms + duration)
    for tool in step.tool_calls:
        tool_end = tool_end_timestamp_ms(tool)
        if tool_end is not None:
            end_candidates.append(tool_end)
    return max(end_candidates) if end_candidates else None


def tool_end_timestamp_ms(tool: ToolMeta) -> int | None:
    if tool.execution_duration_ms is None:
        return None
    start = tool.timestamp_ms
    if start is None:
        return None
    return start + tool.execution_duration_ms


def tool_meta_report(tool: ToolMeta) -> dict[str, Any]:
    return {
        "tool_call_id": tool.tool_call_id,
        **optional("status", tool.status),
        **optional("title", tool.title),
        **optional("timestamp_ms", tool.timestamp_ms),
        **optional("generation_duration_ms", tool.generation_duration_ms),
        **optional("execution_duration_ms", tool.execution_duration_ms),
        **optional("execution_duration_source", tool.execution_duration_source),
        "truncated": tool.truncated,
    }


def observation_meta_report(observation: ObservationMeta) -> dict[str, Any]:
    return {
        **optional("source_call_id", observation.source_call_id),
        **optional("status", observation.status),
        **optional("title", observation.title),
        **optional("timestamp_ms", observation.timestamp_ms),
        "tool_error": observation.tool_error,
        "truncated": observation.truncated,
    }


def trial_wall_duration_ms(meta: dict[str, Any]) -> int | None:
    if meta.get("wall_duration_ms") is not None:
        return int(meta["wall_duration_ms"])
    if meta.get("started_at_ms") is not None and meta.get("finished_at_ms") is not None:
        return max(0, int(meta["finished_at_ms"]) - int(meta["started_at_ms"]))
    return meta.get("duration_ms")


def optional(key: str, value: Any) -> dict[str, Any]:
    return {} if value is None else {key: value}
