from __future__ import annotations

import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from peval_py.adapters import adapter_for
from peval_py.adapters.base import ConversionResult, StepMeta
from peval_py.config import ToolConfig
from peval_py.sources import MessageRecord, read_jsonl, read_sqlite_messages

ATIF_TRAJECTORY_KEYS = {
    "schema_version",
    "session_id",
    "trajectory_id",
    "agent",
    "steps",
    "notes",
    "final_metrics",
    "continued_trajectory_ref",
    "extra",
    "subagent_trajectories",
}
ATIF_AGENT_KEYS = {"name", "version", "model_name", "tool_definitions", "extra"}
ATIF_STEP_KEYS = {
    "step_id",
    "timestamp",
    "source",
    "model_name",
    "reasoning_effort",
    "message",
    "reasoning_content",
    "tool_calls",
    "observation",
    "metrics",
    "is_copied_context",
    "llm_call_count",
    "extra",
}
ATIF_TOOL_CALL_KEYS = {"tool_call_id", "function_name", "arguments", "extra"}
ATIF_OBSERVATION_KEYS = {"results"}
ATIF_OBSERVATION_RESULT_KEYS = {
    "source_call_id",
    "content",
    "subagent_trajectory_ref",
    "extra",
}
ATIF_METRICS_KEYS = {
    "prompt_tokens",
    "completion_tokens",
    "cached_tokens",
    "cost_usd",
    "prompt_token_ids",
    "completion_token_ids",
    "logprobs",
    "extra",
}
ATIF_FINAL_METRICS_KEYS = {
    "total_prompt_tokens",
    "total_completion_tokens",
    "total_cached_tokens",
    "total_cost_usd",
    "total_steps",
    "extra",
}


def convert_records(records: list[MessageRecord], config: ToolConfig) -> ConversionResult:
    adapter = adapter_for(config.adapter)
    convert = getattr(adapter, "convert", None)
    if not callable(convert):
        raise ValueError(f"adapter {config.adapter} does not support record input")
    return convert(records, config)


def convert_path(path: str, config: ToolConfig) -> ConversionResult:
    atif = convert_atif_json_path(path)
    if atif is not None:
        return atif
    adapter = adapter_for(config.adapter)
    adapter_convert_path = getattr(adapter, "convert_path", None)
    if callable(adapter_convert_path):
        return adapter_convert_path(path, config)
    convert = getattr(adapter, "convert", None)
    if not callable(convert):
        raise ValueError(f"adapter {config.adapter} does not support path input")
    return convert(read_jsonl(path), config)


def convert_db(
    path: str,
    session_id: str | None,
    config: ToolConfig,
) -> ConversionResult:
    adapter = adapter_for(config.adapter)
    adapter_convert_db = getattr(adapter, "convert_db", None)
    if callable(adapter_convert_db):
        return adapter_convert_db(path, session_id, config)
    if not session_id:
        raise ValueError(f"adapter {config.adapter} requires --session-id for DB input")
    convert = getattr(adapter, "convert", None)
    if not callable(convert):
        raise ValueError(f"adapter {config.adapter} does not support DB input")
    return convert(read_sqlite_messages(path, session_id, config.db), config)


def convert_atif_json_path(path: str) -> ConversionResult | None:
    parsed = read_atif_json_path(path)
    if parsed is None:
        return None
    return convert_atif_trajectory(parsed)


def convert_atif_trajectory(parsed: dict[str, Any]) -> ConversionResult:
    validate_atif_trajectory_shape(parsed)
    steps = parsed.get("steps")
    if not isinstance(steps, list):
        raise ValueError("ATIF JSON path input must contain a steps list")
    meta = [step_meta_from_atif_step(index, step) for index, step in enumerate(steps, start=1)]
    timestamps = [step.timestamp_ms for step in meta if step.timestamp_ms is not None]
    return ConversionResult(
        trajectory=parsed,
        steps_meta=meta,
        warnings=[],
        total_events=len(steps),
        unmapped_events=0,
        started_at_ms=min(timestamps) if timestamps else None,
        finished_at_ms=max(timestamps) if timestamps else None,
    )


def is_atif_json_path(path: str) -> bool:
    return read_atif_json_path(path) is not None


def read_atif_json_path(path: str) -> dict[str, Any] | None:
    source = Path(path)
    try:
        parsed = json.loads(source.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError):
        return None
    if not is_atif_trajectory(parsed):
        return None
    return parsed


def is_atif_trajectory(value: Any) -> bool:
    if not isinstance(value, dict):
        return False
    schema = str(value.get("schema_version") or "")
    return schema.startswith("ATIF-") and isinstance(value.get("agent"), dict)


def validate_atif_trajectory_shape(
    trajectory: dict[str, Any],
    path: str = "trajectory",
) -> None:
    reject_unknown_keys(path, trajectory, ATIF_TRAJECTORY_KEYS)
    agent = trajectory.get("agent")
    if isinstance(agent, dict):
        reject_unknown_keys(f"{path}.agent", agent, ATIF_AGENT_KEYS)
    steps = trajectory.get("steps")
    if not isinstance(steps, list):
        raise ValueError(f"{path}.steps must be a list")
    final_metrics = trajectory.get("final_metrics")
    if isinstance(final_metrics, dict):
        reject_unknown_keys(
            f"{path}.final_metrics",
            final_metrics,
            ATIF_FINAL_METRICS_KEYS,
        )
    for index, step in enumerate(steps, start=1):
        if not isinstance(step, dict):
            raise ValueError(f"{path}.steps[{index}] must be an object")
        validate_atif_step_shape(step, f"{path}.steps[{index}]")
    subagents = trajectory.get("subagent_trajectories")
    if isinstance(subagents, list):
        for index, subagent in enumerate(subagents, start=1):
            if not isinstance(subagent, dict):
                raise ValueError(
                    f"{path}.subagent_trajectories[{index}] must be an object"
                )
            validate_atif_trajectory_shape(
                subagent,
                f"{path}.subagent_trajectories[{index}]",
            )


def validate_atif_step_shape(step: dict[str, Any], path: str) -> None:
    reject_unknown_keys(path, step, ATIF_STEP_KEYS)
    metrics = step.get("metrics")
    if isinstance(metrics, dict):
        reject_unknown_keys(f"{path}.metrics", metrics, ATIF_METRICS_KEYS)
    tool_calls = step.get("tool_calls")
    if isinstance(tool_calls, list):
        for index, call in enumerate(tool_calls, start=1):
            if not isinstance(call, dict):
                raise ValueError(f"{path}.tool_calls[{index}] must be an object")
            reject_unknown_keys(
                f"{path}.tool_calls[{index}]",
                call,
                ATIF_TOOL_CALL_KEYS,
            )
    observation = step.get("observation")
    if isinstance(observation, dict):
        reject_unknown_keys(f"{path}.observation", observation, ATIF_OBSERVATION_KEYS)
        results = observation.get("results")
        if isinstance(results, list):
            for index, result in enumerate(results, start=1):
                if not isinstance(result, dict):
                    raise ValueError(
                        f"{path}.observation.results[{index}] must be an object"
                    )
                reject_unknown_keys(
                    f"{path}.observation.results[{index}]",
                    result,
                    ATIF_OBSERVATION_RESULT_KEYS,
                )


def reject_unknown_keys(path: str, value: dict[str, Any], allowed: set[str]) -> None:
    extra = sorted(set(value) - allowed)
    if extra:
        keys = ", ".join(extra)
        raise ValueError(f"{path} contains non-ATIF field(s): {keys}")


def step_meta_from_atif_step(index: int, step: Any) -> StepMeta:
    if not isinstance(step, dict):
        raise ValueError(f"ATIF JSON step {index} is not an object")
    return StepMeta(
        step_id=int(step.get("step_id") or index),
        source=str(step.get("source")) if step.get("source") is not None else None,
        timestamp_ms=atif_timestamp_ms(step),
    )


def atif_timestamp_ms(step: dict[str, Any]) -> int | None:
    value = step.get("timestamp")
    if isinstance(value, str) and value:
        try:
            parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
        except ValueError:
            parsed = None
        if parsed is not None:
            if parsed.tzinfo is None:
                parsed = parsed.replace(tzinfo=timezone.utc)
            return int(parsed.timestamp() * 1000)
    return None
