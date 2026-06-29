from __future__ import annotations

import hashlib
import json
from copy import deepcopy
from typing import Any

import pandas as pd

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
