from __future__ import annotations

from typing import Any

import pandas as pd

from peval_py._inspection.utils import (
    compact_text,
    final_metric,
    idle_duration,
    int_or_zero,
    preview,
    stable_hash,
    token_total,
)

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
