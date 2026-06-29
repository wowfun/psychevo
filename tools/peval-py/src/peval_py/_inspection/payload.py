from __future__ import annotations

from typing import Any

import pandas as pd

from peval_py._inspection.frames import InspectFrames
from peval_py._inspection.utils import (
    compact_json,
    frame_row,
    int_or_none,
    is_nan,
    number_or_none,
    rows_for_source,
    rows_for_step,
)

def parse_source_indexes(raw_values: list[int], frames: InspectFrames) -> list[int]:
    if frames.sources.empty:
        return []
    available = [int(value) for value in frames.sources["source_index"].tolist()]
    if not raw_values:
        return available
    selected = []
    for value in raw_values:
        index = int(value)
        if index not in available:
            raise ValueError(
                f"--source index {index} is out of range for {len(available)} sources"
            )
        selected.append(index)
    return sorted(dict.fromkeys(selected))


def source_payload(
    frames: InspectFrames,
    source_index: int,
    *,
    head: int,
    tail: int,
    top: int,
    step_ids: list[str],
    tool_call_ids: list[str],
) -> dict[str, Any]:
    source = frame_row(frames.sources, "source_index", source_index)
    source_steps = rows_for_source(frames.steps, source_index)
    source_tools = rows_for_source(frames.tools, source_index)
    source_observations = rows_for_source(frames.observations, source_index)
    payload: dict[str, Any] = {
        "session_id": source.get("session_id"),
        "agent": source.get("agent"),
        "model": source.get("model"),
        "total_tokens": source.get("total_tokens"),
        "status": inspect_status(source.get("status")),
        "score": inspect_score(source.get("score")),
        "active_duration": milliseconds_to_seconds(source.get("duration_ms")),
        "total_input_tokens": source.get("total_prompt_tokens"),
        "total_output_tokens": source.get("total_completion_tokens"),
        "total_cached_tokens": source.get("total_cached_tokens"),
        "total_tool_calls": source.get("total_tool_calls"),
        "total_tool_errors": source.get("total_tool_errors"),
        "total_turns": source.get("total_turns"),
        "steps": steps_digest(source_steps, head=head, tail=tail, top=top),
        "tools": tools_digest(source_tools, top=top),
        "selected_steps": selected_step_items(
            source_steps,
            source_tools,
            source_observations,
            step_ids,
        ),
        "selected_tool_calls": selected_tool_call_items(
            source_tools,
            source_observations,
            tool_call_ids,
        ),
    }
    return compact_json(payload)


def inspect_status(value: Any) -> Any:
    if value is None or is_nan(value):
        return None
    text = str(value).strip()
    if not text or text.lower() == "passed":
        return None
    return value


def inspect_score(value: Any) -> Any:
    if value is None or is_nan(value):
        return None
    if isinstance(value, str) and not value.strip():
        return None
    return value


def steps_digest(
    steps: pd.DataFrame,
    *,
    head: int,
    tail: int,
    top: int,
) -> dict[str, Any]:
    if steps.empty:
        return {}
    result: dict[str, Any] = {
        "head": step_preview_items(steps.head(head)),
        "tail": step_preview_items(steps.tail(tail)),
        "top_durations": top_step_duration_items(steps, top),
        "top_tokens": top_step_token_items(steps, top),
        "duration_distribution": duration_distribution_seconds(steps, "duration_ms"),
    }
    return compact_json(result)


def step_preview_items(rows: pd.DataFrame) -> list[dict[str, Any]]:
    if rows.empty:
        return []
    items: list[dict[str, Any]] = []
    for _, row in rows.iterrows():
        items.append(
            compact_json(
                {
                    "step_id": row.get("step_id"),
                    "message_preview": row.get("message_preview"),
                    "duration": milliseconds_to_seconds(row.get("duration_ms")),
                }
            )
        )
    return [item for item in items if item]


def top_step_duration_items(steps: pd.DataFrame, top: int) -> list[dict[str, Any]]:
    if top <= 0 or steps.empty or "duration_ms" not in steps:
        return []
    ranked = steps.copy()
    ranked["_duration_ms"] = pd.to_numeric(ranked["duration_ms"], errors="coerce")
    ranked = ranked.dropna(subset=["_duration_ms"]).sort_values(
        "_duration_ms",
        ascending=False,
    )
    items: list[dict[str, Any]] = []
    for _, row in ranked.head(top).iterrows():
        items.append(
            compact_json(
                {
                    "step_id": row.get("step_id"),
                    "duration": milliseconds_to_seconds(row.get("_duration_ms")),
                }
            )
        )
    return [item for item in items if item]


def top_step_token_items(steps: pd.DataFrame, top: int) -> list[dict[str, Any]]:
    if top <= 0 or steps.empty:
        return []
    rows: list[dict[str, Any]] = []
    for _, row in steps.iterrows():
        input_tokens = int_or_none(row.get("prompt_tokens"))
        output_tokens = int_or_none(row.get("completion_tokens"))
        cached_tokens = int_or_none(row.get("cached_tokens"))
        if input_tokens is None and output_tokens is None and cached_tokens is None:
            continue
        total = (input_tokens or 0) + (output_tokens or 0) + (cached_tokens or 0)
        rows.append(
            {
                "_total": total,
                "step_id": row.get("step_id"),
                "input": input_tokens,
                "output": output_tokens,
                "cached": cached_tokens,
            }
        )
    rows.sort(key=lambda item: item["_total"], reverse=True)
    return compact_json(
        [{key: value for key, value in item.items() if key != "_total"} for item in rows[:top]]
    )


def tools_digest(tools: pd.DataFrame, *, top: int) -> dict[str, Any]:
    if tools.empty:
        return {}
    result: dict[str, Any] = {
        "errors": tool_error_items(tools),
        "top_durations": top_tool_duration_items(tools, top),
        "duration_distribution": duration_distribution_seconds(
            tools,
            "execution_duration_ms",
        ),
    }
    return compact_json(result)


def selected_step_items(
    steps: pd.DataFrame,
    tools: pd.DataFrame,
    observations: pd.DataFrame,
    step_ids: list[str],
) -> list[dict[str, Any]]:
    if steps.empty or not step_ids:
        return []
    selectors = {str(value) for value in step_ids}
    rows = steps[
        steps.apply(
            lambda row: str(row.get("step_id")) in selectors
            or str(row.get("step_index")) in selectors,
            axis=1,
        )
    ]
    items: list[dict[str, Any]] = []
    for _, row in rows.sort_values("step_index").iterrows():
        step_tools = rows_for_step(tools, row.get("step_index"))
        step_observations = rows_for_step(observations, row.get("step_index"))
        items.append(
            compact_json(
                {
                    "step_id": row.get("step_id"),
                    "source": row.get("source"),
                    "message_preview": row.get("message_preview"),
                    "reasoning_preview": row.get("reasoning_preview"),
                    "observation_preview": row.get("observation_preview"),
                    "duration": milliseconds_to_seconds(row.get("duration_ms")),
                    "input": int_or_none(row.get("prompt_tokens")),
                    "output": int_or_none(row.get("completion_tokens")),
                    "cached": int_or_none(row.get("cached_tokens")),
                    "tool_calls": tool_call_rows(step_tools),
                    "tool_results": tool_result_rows(step_observations),
                }
            )
        )
    return [item for item in items if item]


def selected_tool_call_items(
    tools: pd.DataFrame,
    observations: pd.DataFrame,
    tool_call_ids: list[str],
) -> list[dict[str, Any]]:
    if tools.empty or not tool_call_ids:
        return []
    selectors = {str(value) for value in tool_call_ids}
    rows = tools[
        tools["tool_call_id"].fillna("").apply(lambda value: str(value) in selectors)
    ]
    items: list[dict[str, Any]] = []
    for _, row in rows.sort_values(["step_index", "tool_index"]).iterrows():
        result = matching_tool_result(observations, row)
        items.append(
            compact_json(
                {
                    **tool_call_row(row),
                    "tool_result": result,
                }
            )
        )
    return [item for item in items if item]


def tool_call_rows(tools: pd.DataFrame) -> list[dict[str, Any]]:
    if tools.empty:
        return []
    return [
        tool_call_row(row)
        for _, row in tools.sort_values(["step_index", "tool_index"]).iterrows()
    ]


def tool_call_row(row: pd.Series) -> dict[str, Any]:
    return compact_json(
        {
            "step_id": row.get("step_id"),
            "tool_call_id": row.get("tool_call_id"),
            "tool_name": row.get("name"),
            "status": row.get("status"),
            "duration": milliseconds_to_seconds(row.get("execution_duration_ms")),
            "arguments_preview": row.get("arguments_preview"),
        }
    )


def tool_result_rows(observations: pd.DataFrame) -> list[dict[str, Any]]:
    if observations.empty:
        return []
    return [
        tool_result_row(row)
        for _, row in observations.sort_values(["step_index", "observation_index"]).iterrows()
    ]


def tool_result_row(row: pd.Series) -> dict[str, Any]:
    return compact_json(
        {
            "tool_call_id": row.get("source_call_id"),
            "status": row.get("status"),
            "content_preview": row.get("content_preview"),
        }
    )


def matching_tool_result(observations: pd.DataFrame, tool: pd.Series) -> dict[str, Any]:
    if observations.empty:
        return {}
    tool_call_id = str(tool.get("tool_call_id") or "")
    step_index = tool.get("step_index")
    candidates = observations
    if step_index is not None and "step_index" in candidates:
        candidates = candidates[candidates["step_index"] == step_index]
    if tool_call_id and "source_call_id" in candidates:
        matched = candidates[
            candidates["source_call_id"].fillna("").apply(
                lambda value: str(value) == tool_call_id
            )
        ]
        if not matched.empty:
            return tool_result_row(matched.iloc[0])
    if not candidates.empty:
        return tool_result_row(candidates.iloc[0])
    return {}


def tool_error_items(tools: pd.DataFrame) -> list[dict[str, Any]]:
    if tools.empty or "error" not in tools:
        return []
    errors = tools[tools["error"] == True]  # noqa: E712 - pandas mask.
    items: list[dict[str, Any]] = []
    for _, row in errors.sort_values(["step_index", "tool_index"]).iterrows():
        items.append(
            compact_json(
                {
                    "step_id": row.get("step_id"),
                    "tool_call_id": row.get("tool_call_id"),
                    "tool_name": row.get("name"),
                }
            )
        )
    return [item for item in items if item]


def top_tool_duration_items(tools: pd.DataFrame, top: int) -> list[dict[str, Any]]:
    if top <= 0 or tools.empty or "execution_duration_ms" not in tools:
        return []
    ranked = tools.copy()
    ranked["_duration_ms"] = pd.to_numeric(
        ranked["execution_duration_ms"],
        errors="coerce",
    )
    ranked = ranked.dropna(subset=["_duration_ms"]).sort_values(
        "_duration_ms",
        ascending=False,
    )
    items: list[dict[str, Any]] = []
    for _, row in ranked.head(top).iterrows():
        items.append(
            compact_json(
                {
                    "step_id": row.get("step_id"),
                    "tool_call_id": row.get("tool_call_id"),
                    "tool_name": row.get("name"),
                    "duration": milliseconds_to_seconds(row.get("_duration_ms")),
                }
            )
        )
    return [item for item in items if item]


def duration_distribution_seconds(frame: pd.DataFrame, column: str) -> dict[str, Any]:
    if frame.empty or column not in frame:
        return {}
    series = pd.to_numeric(frame[column], errors="coerce").dropna() / 1000
    if series.empty:
        return {}
    return compact_json(
        {
            "count": int(series.count()),
            "min": normalize_number(series.min()),
            "avg": normalize_number(series.mean()),
            "p50": normalize_number(series.quantile(0.5)),
            "p95": normalize_number(series.quantile(0.95)),
            "max": normalize_number(series.max()),
            "sum": normalize_number(series.sum()),
        }
    )


def milliseconds_to_seconds(value: Any) -> float | int | None:
    number = number_or_none(value)
    if number is None:
        return None
    return normalize_number(float(number) / 1000)


def normalize_number(value: Any) -> float | int | None:
    number = number_or_none(value)
    if number is None:
        return None
    rounded = round(float(number), 6)
    return int(rounded) if rounded.is_integer() else rounded
