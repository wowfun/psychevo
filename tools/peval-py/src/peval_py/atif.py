from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from peval_py.adapters import adapter_for
from peval_py.adapters.base import ConversionResult, StepMeta
from peval_py.config import ToolConfig
from peval_py.sources import MessageRecord, read_jsonl, read_sqlite_messages


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


def step_meta_from_atif_step(index: int, step: Any) -> StepMeta:
    if not isinstance(step, dict):
        raise ValueError(f"ATIF JSON step {index} is not an object")
    return StepMeta(
        step_id=int(step.get("step_id") or index),
        source=str(step.get("source")) if step.get("source") is not None else None,
        timestamp_ms=int_or_none(step.get("timestamp_ms")),
    )


def int_or_none(value: Any) -> int | None:
    if value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None
