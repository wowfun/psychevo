from __future__ import annotations

import argparse
import hashlib
import json
from copy import deepcopy
from pathlib import Path
from typing import Any

import pandas as pd

from peval_py.inputs import AdapterAssignments, load_inputs
from peval_py.outputs import DEFAULT_OUTPUT, unique_timestamped_name
from peval_py.pipeline import build_report_from_loaded_inputs

INSPECT_SCHEMA_VERSION = 2


def build_inspect_payload(
    args: argparse.Namespace,
    adapter_assignments: AdapterAssignments,
    config: object,
) -> dict[str, Any]:
    report = inspect_report_for_args(args, adapter_assignments, config)
    preview_chars = positive_int(getattr(args, "preview_chars", None), 240)
    frames = InspectFrames.from_report(report, preview_chars=preview_chars)
    source_indexes = parse_source_indexes(getattr(args, "source", None) or [], frames)
    head = positive_int(getattr(args, "head", None), 2)
    tail = positive_int(getattr(args, "tail", None), 2)
    top = positive_int(getattr(args, "top", None), 5)
    step_ids = [str(value) for value in getattr(args, "step", None) or []]
    tool_call_ids = [str(value) for value in getattr(args, "tool_call", None) or []]
    return {
        "inspect_schema_version": INSPECT_SCHEMA_VERSION,
        "sources": [
            source_payload(
                frames,
                source_index,
                head=head,
                tail=tail,
                top=top,
                step_ids=step_ids,
                tool_call_ids=tool_call_ids,
            )
            for source_index in source_indexes
        ],
    }


def inspect_report_for_args(
    args: argparse.Namespace,
    adapter_assignments: AdapterAssignments,
    config: object,
) -> dict[str, Any]:
    direct_reports, remaining_paths = direct_inspect_reports(getattr(args, "path", None) or [])
    reports = direct_reports[:]
    if remaining_paths or getattr(args, "db", None) or getattr(args, "input_table", None):
        load_args = argparse.Namespace(**{**vars(args), "path": remaining_paths})
        loaded_inputs = load_inputs(load_args, adapter_assignments, config=config)
        if loaded_inputs.sessions:
            reports.append(
                build_report_from_loaded_inputs(
                    loaded_inputs,
                    config,
                    getattr(args, "note", None) or [],
                )
            )
    if not reports:
        raise ValueError("missing input source; pass --path, --db, or --input-table")
    return merge_reports(reports)


def direct_inspect_reports(paths: list[str]) -> tuple[list[dict[str, Any]], list[str]]:
    reports: list[dict[str, Any]] = []
    remaining: list[str] = []
    for raw_path in paths:
        path = Path(raw_path)
        parsed = read_json_object(path)
        if parsed is None:
            remaining.append(raw_path)
            continue
        report = report_from_direct_json(parsed, path)
        if report is None:
            remaining.append(raw_path)
        else:
            reports.append(report)
    return reports, remaining


def read_json_object(path: Path) -> Any:
    if path.suffix.lower() != ".json":
        return None
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError):
        return None


def report_from_direct_json(parsed: Any, path: Path) -> dict[str, Any] | None:
    if is_report_json(parsed):
        return {
            "schema_version": parsed.get("schema_version"),
            "includes": parsed.get("includes", []),
            "trajectory": list(parsed.get("trajectory") or []),
            "trajectory_meta": list(parsed.get("trajectory_meta") or []),
        }
    if is_atif_trajectory(parsed):
        return {
            "schema_version": None,
            "includes": ["core"],
            "trajectory": [parsed],
            "trajectory_meta": [meta_from_trajectory(parsed, path)],
        }
    metas = meta_list_from_json(parsed)
    if metas is not None:
        return {
            "schema_version": None,
            "includes": ["core"],
            "trajectory": [empty_trajectory_for_meta(meta, path) for meta in metas],
            "trajectory_meta": metas,
        }
    return None


def is_report_json(value: Any) -> bool:
    return (
        isinstance(value, dict)
        and isinstance(value.get("trajectory"), list)
        and isinstance(value.get("trajectory_meta"), list)
    )


def is_atif_trajectory(value: Any) -> bool:
    return isinstance(value, dict) and str(value.get("schema_version") or "").startswith(
        "ATIF-"
    ) and isinstance(value.get("agent"), dict)


def meta_list_from_json(value: Any) -> list[dict[str, Any]] | None:
    if isinstance(value, dict) and looks_like_meta(value):
        return [value]
    if isinstance(value, list) and all(isinstance(item, dict) for item in value):
        items = [item for item in value if isinstance(item, dict)]
        return items if items and all(looks_like_meta(item) for item in items) else None
    return None


def looks_like_meta(value: dict[str, Any]) -> bool:
    keys = {"trial_key", "adapter", "status", "steps", "duration_ms", "wall_duration_ms"}
    return bool(keys & set(value))


def meta_from_trajectory(trajectory: dict[str, Any], path: Path) -> dict[str, Any]:
    steps = trajectory.get("steps") if isinstance(trajectory.get("steps"), list) else []
    return {
        "trial_key": str(trajectory.get("trajectory_id") or trajectory.get("session_id") or path.stem),
        "adapter": "atif",
        "status": "passed",
        "warnings": [],
        "data_ref": {"label": path.name, "path": str(path)},
        "steps": [
            {
                "step_id": step.get("step_id", index)
                if isinstance(step, dict)
                else index,
                "tool_calls": [],
                "observations": [],
                "tool_error": False,
                "truncated": False,
            }
            for index, step in enumerate(steps, start=1)
        ],
    }


def empty_trajectory_for_meta(meta: dict[str, Any], path: Path) -> dict[str, Any]:
    return {
        "schema_version": "ATIF-v1.7",
        "session_id": meta.get("session_id") or meta.get("trial_key") or path.stem,
        "trajectory_id": meta.get("trial_key") or path.stem,
        "agent": {"name": meta.get("adapter") or "metadata-only"},
        "steps": [],
        "final_metrics": {},
    }


def merge_reports(reports: list[dict[str, Any]]) -> dict[str, Any]:
    trajectories: list[dict[str, Any]] = []
    metas: list[dict[str, Any]] = []
    for report in reports:
        trajectories.extend(
            item for item in report.get("trajectory", []) if isinstance(item, dict)
        )
        metas.extend(
            item for item in report.get("trajectory_meta", []) if isinstance(item, dict)
        )
    while len(metas) < len(trajectories):
        metas.append({})
    while len(trajectories) < len(metas):
        trajectories.append(
            {
                "schema_version": "ATIF-v1.7",
                "agent": {},
                "steps": [],
                "final_metrics": {},
            }
        )
    return {
        "schema_version": None,
        "includes": ["core"],
        "trajectory": trajectories,
        "trajectory_meta": metas,
    }


class InspectFrames:
    def __init__(
        self,
        *,
        sources: pd.DataFrame,
        steps: pd.DataFrame,
        tools: pd.DataFrame,
        observations: pd.DataFrame,
    ) -> None:
        self.sources = sources
        self.steps = steps
        self.tools = tools
        self.observations = observations

    @classmethod
    def from_report(cls, report: dict[str, Any], *, preview_chars: int) -> "InspectFrames":
        source_rows: list[dict[str, Any]] = []
        step_rows: list[dict[str, Any]] = []
        tool_rows: list[dict[str, Any]] = []
        observation_rows: list[dict[str, Any]] = []
        trajectories = report.get("trajectory") or []
        metas = report.get("trajectory_meta") or []
        for source_index, trajectory in enumerate(trajectories, start=1):
            if not isinstance(trajectory, dict):
                continue
            meta = metas[source_index - 1] if source_index - 1 < len(metas) else {}
            if not isinstance(meta, dict):
                meta = {}
            steps = trajectory.get("steps") if isinstance(trajectory.get("steps"), list) else []
            meta_steps = meta.get("steps") if isinstance(meta.get("steps"), list) else []
            final_metrics = trajectory.get("final_metrics")
            final_metrics = final_metrics if isinstance(final_metrics, dict) else {}
            source_rows.append(
                source_row(source_index, trajectory, meta, final_metrics, steps)
            )
            for step_index, step in enumerate(steps, start=1):
                if not isinstance(step, dict):
                    continue
                step_meta = meta_steps[step_index - 1] if step_index - 1 < len(meta_steps) else {}
                if not isinstance(step_meta, dict):
                    step_meta = {}
                step_rows.append(
                    step_row(source_index, step_index, step, step_meta, preview_chars)
                )
                tool_rows.extend(
                    tool_rows_for_step(source_index, step_index, step, step_meta, preview_chars)
                )
                observation_rows.extend(
                    observation_rows_for_step(source_index, step_index, step, step_meta, preview_chars)
                )
        return cls(
            sources=pd.DataFrame(source_rows),
            steps=pd.DataFrame(step_rows),
            tools=pd.DataFrame(tool_rows),
            observations=pd.DataFrame(observation_rows),
        )


def source_row(
    index: int,
    trajectory: dict[str, Any],
    meta: dict[str, Any],
    final_metrics: dict[str, Any],
    steps: list[Any],
) -> dict[str, Any]:
    agent = trajectory.get("agent") if isinstance(trajectory.get("agent"), dict) else {}
    warnings = meta.get("warnings") if isinstance(meta.get("warnings"), list) else []
    return {
        "source_index": index,
        "kind": data_ref_value(meta, "kind"),
        "label": data_ref_value(meta, "label"),
        "path": data_ref_value(meta, "path"),
        "artifact_ref": meta.get("artifact_ref")
        if isinstance(meta.get("artifact_ref"), dict)
        else None,
        "adapter": meta.get("adapter"),
        "agent": agent.get("name"),
        "model": agent.get("model_name"),
        "session_id": trajectory.get("session_id"),
        "trial_key": meta.get("trial_key"),
        "source_alias": meta.get("source_alias"),
        "status": meta.get("status"),
        "score": meta.get("score"),
        "warning_count": len(warnings),
        "warnings": warnings,
        "unmapped_event_count": int_or_zero(meta.get("unmapped_events")),
        "total_events": meta.get("total_events"),
        "step_count": len(steps),
        "started_at_ms": meta.get("started_at_ms"),
        "finished_at_ms": meta.get("finished_at_ms"),
        "duration_ms": meta.get("duration_ms"),
        "wall_duration_ms": meta.get("wall_duration_ms"),
        "idle_duration_ms": idle_duration(meta),
        "prompt_unavailable": bool(meta.get("prompt_unavailable")),
        "final_metrics": final_metrics,
        "total_prompt_tokens": final_metrics.get("total_prompt_tokens"),
        "total_completion_tokens": final_metrics.get("total_completion_tokens"),
        "total_cached_tokens": final_metrics.get("total_cached_tokens"),
        "total_cost_usd": final_metrics.get("total_cost_usd"),
        "total_tokens": token_total(final_metrics),
        "total_turns": final_metric(final_metrics, "total_turns"),
        "total_tool_calls": final_metric(final_metrics, "total_tool_calls"),
        "total_tool_errors": final_metric(final_metrics, "total_tool_errors"),
    }


def data_ref_value(meta: dict[str, Any], key: str) -> Any:
    data_ref = meta.get("data_ref")
    if isinstance(data_ref, dict):
        return data_ref.get(key)
    return None


def step_row(
    source_index: int,
    step_index: int,
    step: dict[str, Any],
    step_meta: dict[str, Any],
    preview_chars: int,
) -> dict[str, Any]:
    tool_calls = step.get("tool_calls") if isinstance(step.get("tool_calls"), list) else []
    observation = step.get("observation") if isinstance(step.get("observation"), dict) else {}
    observations = observation.get("results") if isinstance(observation.get("results"), list) else []
    meta_tool_calls = step_meta.get("tool_calls") if isinstance(step_meta.get("tool_calls"), list) else []
    has_tool_error = bool(step_meta.get("tool_error")) or any(
        str(item.get("status") or "").lower() == "error"
        for item in meta_tool_calls
        if isinstance(item, dict)
    )
    metrics = step.get("metrics") if isinstance(step.get("metrics"), dict) else {}
    return {
        "source_index": source_index,
        "step_index": step_index,
        "step_id": step.get("step_id") or step_meta.get("step_id") or step_index,
        "source": step.get("source"),
        "timestamp": step.get("timestamp"),
        "timestamp_ms": step_meta.get("timestamp_ms"),
        "duration_ms": step_meta.get("duration_ms"),
        "elapsed_ms": step_meta.get("elapsed_ms"),
        "message_preview": preview(step.get("message"), preview_chars),
        "reasoning_preview": preview(step.get("reasoning_content"), preview_chars),
        "observation_preview": preview(observations, preview_chars),
        "tool_call_count": len(tool_calls),
        "observation_count": len(observations),
        "tool_error": has_tool_error,
        "truncated": bool(step_meta.get("truncated")),
        "prompt_tokens": metrics.get("prompt_tokens"),
        "completion_tokens": metrics.get("completion_tokens"),
        "cached_tokens": metrics.get("cached_tokens"),
        "cost_usd": metrics.get("cost_usd"),
        "contains_text": compact_text(step),
    }


def tool_rows_for_step(
    source_index: int,
    step_index: int,
    step: dict[str, Any],
    step_meta: dict[str, Any],
    preview_chars: int,
) -> list[dict[str, Any]]:
    calls = step.get("tool_calls") if isinstance(step.get("tool_calls"), list) else []
    meta_calls = step_meta.get("tool_calls") if isinstance(step_meta.get("tool_calls"), list) else []
    rows = []
    for tool_index, call in enumerate(calls, start=1):
        if not isinstance(call, dict):
            continue
        meta = meta_calls[tool_index - 1] if tool_index - 1 < len(meta_calls) else {}
        if not isinstance(meta, dict):
            meta = {}
        args = call.get("arguments")
        rows.append(
            {
                "source_index": source_index,
                "step_index": step_index,
                "step_id": step.get("step_id") or step_meta.get("step_id") or step_index,
                "tool_index": tool_index,
                "tool_call_id": call.get("tool_call_id") or meta.get("tool_call_id"),
                "name": call.get("function_name") or meta.get("title"),
                "status": meta.get("status"),
                "error": str(meta.get("status") or "").lower() == "error",
                "timestamp_ms": meta.get("timestamp_ms"),
                "generation_duration_ms": meta.get("generation_duration_ms"),
                "execution_duration_ms": meta.get("execution_duration_ms"),
                "arguments_hash": stable_hash(args),
                "arguments_preview": preview(args, preview_chars),
                "truncated": bool(meta.get("truncated")),
            }
        )
    return rows


def observation_rows_for_step(
    source_index: int,
    step_index: int,
    step: dict[str, Any],
    step_meta: dict[str, Any],
    preview_chars: int,
) -> list[dict[str, Any]]:
    observation = step.get("observation") if isinstance(step.get("observation"), dict) else {}
    results = observation.get("results") if isinstance(observation.get("results"), list) else []
    meta_observations = step_meta.get("observations") if isinstance(step_meta.get("observations"), list) else []
    rows = []
    for observation_index, result in enumerate(results, start=1):
        if not isinstance(result, dict):
            continue
        meta = (
            meta_observations[observation_index - 1]
            if observation_index - 1 < len(meta_observations)
            else {}
        )
        if not isinstance(meta, dict):
            meta = {}
        rows.append(
            {
                "source_index": source_index,
                "step_index": step_index,
                "observation_index": observation_index,
                "source_call_id": (
                    result.get("source_call_id")
                    or result.get("tool_call_id")
                    or meta.get("source_call_id")
                    or meta.get("tool_call_id")
                ),
                "status": meta.get("status"),
                "tool_error": bool(meta.get("tool_error")),
                "timestamp_ms": meta.get("timestamp_ms"),
                "content_preview": preview(result.get("content"), preview_chars),
                "truncated": bool(meta.get("truncated")),
            }
        )
    return rows


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


def resolve_inspect_output(args: argparse.Namespace) -> str | None:
    if args.output is DEFAULT_OUTPUT:
        return unique_timestamped_name("inspect.json")
    return args.output


def validate_inspect_args(args: argparse.Namespace) -> None:
    validate_inspect_raw_only_args(args)
    if getattr(args, "format", None) == "html":
        raise ValueError("view tr inspect mode supports only JSON output; use -m raw for HTML reports")
    output = getattr(args, "output", None)
    if output and output is not DEFAULT_OUTPUT and Path(str(output)).suffix.lower() == ".html":
        raise ValueError("view tr inspect mode writes JSON; use -m raw for HTML reports")


def validate_inspect_raw_only_args(args: argparse.Namespace) -> None:
    raw_flags = [
        ("agent_name", getattr(args, "agent_name", None) is not None),
        ("agent_version", getattr(args, "agent_version", None) is not None),
        ("model", getattr(args, "model", None) is not None),
        ("no_redact", bool(getattr(args, "no_redact", False))),
    ]
    raw_used = [name for name, was_used in raw_flags if was_used]
    if raw_used:
        raise ValueError(
            "raw-only option(s) require -m raw: "
            + ", ".join(f"--{name.replace('_', '-')}" for name in raw_used)
        )


def validate_raw_args(args: argparse.Namespace) -> None:
    inspect_flags = [
        ("head", getattr(args, "head", None) is not None),
        ("tail", getattr(args, "tail", None) is not None),
        ("top", getattr(args, "top", None) is not None),
        ("step", getattr(args, "step", None) is not None),
        ("tool_call", getattr(args, "tool_call", None) is not None),
        ("source", getattr(args, "source", None) is not None),
        ("preview_chars", getattr(args, "preview_chars", None) is not None),
    ]
    used = [name for name, was_used in inspect_flags if was_used]
    if used:
        raise ValueError(
            "inspect-only option(s) cannot be used with -m raw: "
            + ", ".join(f"--{name.replace('_', '-')}" for name in used)
        )


def positive_int(value: Any, default: int) -> int:
    if value is None:
        return default
    result = int(value)
    if result < 0:
        raise ValueError("inspect count options must be non-negative")
    return result


def positive_list(values: list[Any], label: str) -> list[int]:
    parsed = []
    for value in values:
        number = int(value)
        if number <= 0:
            raise ValueError(f"{label} values must be positive one-based indexes")
        parsed.append(number)
    return parsed


def frame_row(frame: pd.DataFrame, key: str, value: int) -> dict[str, Any]:
    row = frame[frame[key] == value]
    if row.empty:
        raise ValueError(f"missing inspect source row: {value}")
    return row.iloc[0].to_dict()


def rows_for_source(frame: pd.DataFrame, source_index: int) -> pd.DataFrame:
    if frame.empty or "source_index" not in frame:
        return frame
    return frame[frame["source_index"] == source_index]


def rows_for_step(frame: pd.DataFrame, step_index: Any) -> pd.DataFrame:
    if frame.empty or "step_index" not in frame:
        return frame
    return frame[frame["step_index"] == step_index]


def preview(value: Any, limit: int) -> str | None:
    if value is None:
        return None
    if isinstance(value, str):
        text = value
    else:
        try:
            text = json.dumps(value, ensure_ascii=False, sort_keys=True)
        except TypeError:
            text = str(value)
    text = text.replace("\n", "\\n")
    if not text:
        return None
    if len(text) <= limit:
        return text
    return text[: max(0, limit)] + "...[truncated]"


def compact_text(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, str):
        return value
    if isinstance(value, dict):
        return " ".join(compact_text(item) for item in value.values())
    if isinstance(value, list):
        return " ".join(compact_text(item) for item in value)
    return str(value)


def stable_hash(value: Any) -> str | None:
    if value is None:
        return None
    try:
        text = json.dumps(value, ensure_ascii=False, sort_keys=True)
    except TypeError:
        text = str(value)
    return hashlib.sha1(text.encode("utf-8")).hexdigest()[:12]


def int_or_zero(value: Any) -> int:
    parsed = int_or_none(value)
    return parsed if parsed is not None else 0


def int_or_none(value: Any) -> int | None:
    if value is None or is_nan(value):
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def number_or_none(value: Any) -> float | int | None:
    if value is None or is_nan(value):
        return None
    try:
        number = float(value)
    except (TypeError, ValueError):
        return None
    if number.is_integer():
        return int(number)
    return number


def safe_rate(numerator: Any, denominator: Any, *, scale: float = 1.0) -> float | None:
    top = number_or_none(numerator)
    bottom = number_or_none(denominator)
    if top is None or bottom in {None, 0}:
        return None
    return round((float(top) / float(bottom)) * scale, 6)


def idle_duration(meta: dict[str, Any]) -> int | None:
    wall = int_or_none(meta.get("wall_duration_ms"))
    active = int_or_none(meta.get("duration_ms"))
    if wall is None or active is None:
        return None
    return max(0, wall - active)


def metric_extra(metrics: dict[str, Any]) -> dict[str, Any]:
    extra = metrics.get("extra")
    return extra if isinstance(extra, dict) else {}


def final_metric(metrics: dict[str, Any], key: str) -> Any:
    if key in metrics:
        return metrics[key]
    return metric_extra(metrics).get(key)


def token_total(metrics: dict[str, Any]) -> int | None:
    prompt = int_or_none(metrics.get("total_prompt_tokens"))
    completion = int_or_none(metrics.get("total_completion_tokens"))
    if prompt is not None or completion is not None:
        return (prompt or 0) + (completion or 0)
    usage = metric_extra(metrics).get("usage")
    if isinstance(usage, dict):
        return int_or_none(usage.get("total_tokens"))
    return None


def compact_json(value: Any) -> Any:
    if isinstance(value, dict):
        result = {}
        for key, item in value.items():
            if item is None or is_nan(item):
                continue
            compacted = compact_json(item)
            if compacted in ({}, []):
                continue
            result[str(key)] = compacted
        return result
    if isinstance(value, list):
        result = []
        for item in value:
            if item is None or is_nan(item):
                continue
            compacted = compact_json(item)
            if compacted in ({}, []):
                continue
            result.append(compacted)
        return result
    if isinstance(value, float) and value.is_integer():
        return int(value)
    if is_nan(value):
        return None
    return deepcopy(value)


def is_nan(value: Any) -> bool:
    try:
        result = pd.isna(value)
    except (TypeError, ValueError):
        return False
    if isinstance(result, bool):
        return result
    return False
