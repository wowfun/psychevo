from __future__ import annotations

import math
from typing import Any


def automatic_analysis_metrics(
    trajectory: dict[str, Any],
    meta: dict[str, Any],
) -> dict[str, Any]:
    final_metrics = trajectory.get("final_metrics")
    metrics = final_metrics if isinstance(final_metrics, dict) else {}
    total_tool_calls = int_metric(final_metric(metrics, "total_tool_calls")) or 0
    total_tool_errors = int_metric(final_metric(metrics, "total_tool_errors")) or 0
    tokens = token_total(metrics)
    tool_rows = tool_metric_rows(trajectory, meta)
    return compact_dict(
        {
            "tooling": compact_dict(
                {
                    "tool_error_rate": rate(total_tool_errors, total_tool_calls),
                    "distinct_tools": len({row["name"] for row in tool_rows}),
                }
            ),
            "cost": compact_dict(
                {
                    "cost_per_1k_tokens": rate(
                        number_metric(metrics.get("total_cost_usd")),
                        tokens,
                        scale=1000,
                    ),
                }
            ),
            "latency": latency_metrics(trajectory, meta),
        }
    )


def tool_metric_rows(
    trajectory: dict[str, Any],
    meta: dict[str, Any],
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    meta_steps = meta.get("steps") if isinstance(meta.get("steps"), list) else []
    for step_index, step in enumerate(trajectory_steps(trajectory)):
        if not isinstance(step, dict):
            continue
        meta_step = meta_steps[step_index] if step_index < len(meta_steps) else {}
        meta_tools = meta_step.get("tool_calls") if isinstance(meta_step, dict) else []
        if not isinstance(meta_tools, list):
            meta_tools = []
        meta_tools_by_id = {
            str(tool.get("tool_call_id")): tool
            for tool in meta_tools
            if isinstance(tool, dict) and tool.get("tool_call_id") is not None
        }
        for tool_index, call in enumerate(step.get("tool_calls") or []):
            if not isinstance(call, dict):
                continue
            call_id = str(
                call.get("tool_call_id")
                or call.get("id")
                or f"{step_index}:{tool_index}"
            )
            meta_tool = meta_tools_by_id.get(call_id)
            if meta_tool is None and tool_index < len(meta_tools):
                candidate = meta_tools[tool_index]
                meta_tool = candidate if isinstance(candidate, dict) else None
            name = tool_name(call, meta_tool)
            status = str((meta_tool or {}).get("status") or "").lower()
            rows.append(
                {
                    "name": name,
                    "status": status,
                    "error": "error" in status or "fail" in status,
                    "duration_ms": int_metric(
                        (meta_tool or {}).get("execution_duration_ms")
                    ),
                }
            )
    return rows


def tool_name(call: dict[str, Any], meta_tool: dict[str, Any] | None) -> str:
    function = call.get("function")
    if isinstance(function, dict) and function.get("name"):
        return str(function["name"])
    for key in ("function_name", "name", "tool_name"):
        if call.get(key):
            return str(call[key])
    if meta_tool and meta_tool.get("title"):
        return str(meta_tool["title"])
    return "unknown"


def latency_metrics(trajectory: dict[str, Any], meta: dict[str, Any]) -> dict[str, Any]:
    steps = meta.get("steps") if isinstance(meta.get("steps"), list) else []
    trajectory_step_sources = [
        lower_string(step.get("source")) if isinstance(step, dict) else ""
        for step in trajectory_steps(trajectory)
    ]
    step_durations = [
        int(value)
        for value in (
            int_metric(step.get("duration_ms"))
            for step in steps
            if isinstance(step, dict)
        )
        if value is not None
    ]
    model_durations: list[int] = []
    for index, step in enumerate(steps):
        if not isinstance(step, dict):
            continue
        if index >= len(trajectory_step_sources):
            continue
        if trajectory_step_sources[index] not in {"agent", "assistant"}:
            continue
        if is_estimated_model_duration(step):
            continue
        duration = int_metric(step.get("duration_ms"))
        if duration is not None:
            model_durations.append(duration)
    tool_durations: list[int] = []
    for step in steps:
        if not isinstance(step, dict):
            continue
        for tool in step.get("tool_calls") or []:
            if not isinstance(tool, dict):
                continue
            duration = int_metric(tool.get("execution_duration_ms"))
            if duration is not None:
                tool_durations.append(duration)
    return compact_dict(
        {
            "step_duration_ms": distribution_metrics(step_durations),
            "tool_execution_duration_ms": distribution_metrics(tool_durations),
            "model_duration_ms": distribution_metrics(model_durations),
        }
    )


def is_estimated_model_duration(step: dict[str, Any]) -> bool:
    return "estimate" in lower_string(step.get("duration_source"))


def lower_string(value: Any) -> str:
    return str(value or "").lower()


def distribution_metrics(values: list[int]) -> dict[str, Any] | None:
    if not values:
        return None
    ordered = sorted(values)
    return {
        "min": ordered[0],
        "q1": percentile(ordered, 25),
        "p50": percentile(ordered, 50),
        "q3": percentile(ordered, 75),
        "p95": percentile(ordered, 95),
        "max": ordered[-1],
    }


def percentile(ordered_values: list[int], percentile_value: int) -> int | float:
    if len(ordered_values) == 1:
        return ordered_values[0]
    position = (len(ordered_values) - 1) * (percentile_value / 100)
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered_values[lower]
    value = ordered_values[lower] + (ordered_values[upper] - ordered_values[lower]) * (
        position - lower
    )
    return int(value) if value.is_integer() else round(value, 3)


def trajectory_steps(trajectory: dict[str, Any]) -> list[Any]:
    steps = trajectory.get("steps")
    return steps if isinstance(steps, list) else []


def rate(
    value: int | float | None,
    total: int | float | None,
    *,
    scale: int = 1,
) -> float | None:
    if value is None or total in (None, 0):
        return None
    return round(float(value) * scale / float(total), 6)


def int_metric(value: Any) -> int | None:
    number = number_metric(value)
    return None if number is None else int(number)


def number_metric(value: Any) -> float | int | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value
    if isinstance(value, float) and math.isfinite(value):
        return value
    return None


def compact_dict(values: dict[str, Any]) -> dict[str, Any]:
    return {
        key: value
        for key, value in values.items()
        if value is not None and value != {} and value != []
    }


def token_total(metrics: dict[str, Any]) -> int | None:
    values = [
        metrics.get("total_prompt_tokens"),
        metrics.get("total_completion_tokens"),
    ]
    present = [int(value) for value in values if value is not None]
    if present:
        return sum(present)
    usage = metric_extra(metrics).get("usage")
    if isinstance(usage, dict) and usage.get("total_tokens") is not None:
        return int(usage["total_tokens"])
    return None


def final_metric(metrics: dict[str, Any], key: str) -> Any:
    if key in metrics:
        return metrics.get(key)
    return metric_extra(metrics).get(key)


def metric_extra(metrics: dict[str, Any]) -> dict[str, Any]:
    extra = metrics.get("extra")
    return extra if isinstance(extra, dict) else {}
