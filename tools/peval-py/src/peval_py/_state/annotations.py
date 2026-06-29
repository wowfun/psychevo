from __future__ import annotations

import json
import math
from copy import deepcopy
from typing import Any

from peval_py.analysis import (
    MERGEABLE_ANALYSIS_LIST_FIELDS,
    cached_analysis_report,
    cached_note_report,
)
from peval_py.config import ToolConfig

def matching_annotation_items(
    annotations: dict[str, Any],
    key: str,
    trial_key: str,
) -> list[dict[str, Any]]:
    return [
        deepcopy(item)
        for item in annotations.get(key) or []
        if isinstance(item, dict) and str(item.get("trial_key") or "") == trial_key
    ]


def merged_note_markdown(notes: list[dict[str, Any]]) -> str | None:
    sections: list[str] = []
    for index, note in enumerate(notes, start=1):
        markdown = optional_str(note.get("markdown"))
        if not markdown or not markdown.strip():
            continue
        label = optional_str(note.get("label"))
        source = optional_str(note.get("source"))
        if label or source:
            heading = label or f"Note {index}"
            lines = [f"## {heading}"]
            if source:
                lines.extend(["", f"Source: {source}"])
            lines.extend(["", markdown.strip()])
            sections.append("\n".join(lines))
        else:
            sections.append(markdown.strip())
    if not sections:
        return None
    return "\n\n".join(sections) + "\n"


def merged_analysis_json(analyses: list[dict[str, Any]]) -> dict[str, Any] | None:
    if not analyses:
        return None
    summaries = [
        summary.strip()
        for summary in (optional_str(item.get("summary")) for item in analyses)
        if summary and summary.strip()
    ]
    payload: dict[str, Any] = {
        "summary": "\n\n".join(summaries) if summaries else "",
        "items": deepcopy(analyses),
    }
    for key in MERGEABLE_ANALYSIS_LIST_FIELDS:
        values: list[Any] = []
        for item in analyses:
            value = item.get(key)
            if isinstance(value, list) and value:
                values.extend(deepcopy(value))
        if values:
            payload[key] = values
    for source_key, target_key in (
        ("analysis_status", "status"),
        ("subject", "subject"),
        ("analysis_metrics", "metrics"),
        ("confidence", "confidence"),
    ):
        value = unique_analysis_value(analyses, source_key)
        if value is not None:
            payload[target_key] = value
    return payload


def unique_analysis_value(items: list[dict[str, Any]], key: str) -> Any:
    values: list[Any] = []
    for item in items:
        if key not in item:
            continue
        value = item[key]
        if key == "analysis_status":
            if isinstance(value, str) and value.strip():
                values.append(value)
        elif key in {"subject", "analysis_metrics"}:
            if isinstance(value, dict) and value:
                values.append(deepcopy(value))
        elif key == "confidence":
            if isinstance(value, str) and value.strip():
                values.append(value)
            elif (
                isinstance(value, (int, float))
                and not isinstance(value, bool)
                and math.isfinite(float(value))
            ):
                values.append(value)
    unique_values: list[Any] = []
    for value in values:
        if not any(value == existing for existing in unique_values):
            unique_values.append(value)
    if len(unique_values) == 1:
        return unique_values[0]
    return None


def merged_analysis_markdown(analyses: list[dict[str, Any]]) -> str | None:
    sections = [
        markdown.strip()
        for markdown in (optional_str(item.get("md_report")) for item in analyses)
        if markdown and markdown.strip()
    ]
    if not sections:
        return None
    return "\n\n".join(sections) + "\n"


def is_report_json(value: Any) -> bool:
    return (
        isinstance(value, dict)
        and isinstance(value.get("trajectory"), list)
        and isinstance(value.get("trajectory_meta"), list)
        and value.get("schema_version") is not None
    )


def uniquify_trial_keys(metas: list[dict[str, Any]]) -> list[dict[str, Any]]:
    seen: dict[str, int] = {}
    out: list[dict[str, Any]] = []
    for meta in metas:
        copy = dict(meta)
        base = str(copy.get("trial_key") or "trial")
        count = seen.get(base, 0) + 1
        seen[base] = count
        if count > 1:
            copy["trial_key"] = f"{base}:{count}"
        out.append(copy)
    return out


def meta_with_source_alias(meta: dict[str, Any], alias: Any) -> dict[str, Any]:
    if alias:
        copy = dict(meta)
        copy["source_alias"] = str(alias)
        return copy
    if "source_alias" in meta:
        copy = dict(meta)
        copy.pop("source_alias", None)
        return copy
    return meta


def source_report_with_current_annotations(
    source: dict[str, Any],
    trajectory: dict[str, Any],
    meta: dict[str, Any],
    config: ToolConfig | None,
) -> dict[str, Any]:
    if config is None:
        return {}

    trial_key = str(meta.get("trial_key") or "")
    session_id = (
        optional_str(trajectory.get("session_id"))
        or source.get("session_id")
        or optional_str(meta.get("trial_key"))
    )
    agent_id = annotation_agent_id(source, trajectory)
    current_note = cached_note_report(
        workspace_root=config.workspace_root,
        eval_slug=config.analysis_eval_slug,
        agent_id=agent_id,
        session_id=session_id,
        trial_key=trial_key,
    )
    current_analysis = cached_analysis_report(
        workspace_root=config.workspace_root,
        eval_slug=config.analysis_eval_slug,
        agent_id=agent_id,
        session_id=session_id,
        trial_key=trial_key,
    )
    notes: list[dict[str, Any]] = []
    if current_note is not None:
        notes.append(current_note)

    next_annotations: dict[str, Any] = {
        "report_notes": [],
        "notes": notes,
    }
    if current_analysis is not None:
        next_annotations["analysis"] = [current_analysis]

    if (
        next_annotations["report_notes"]
        or next_annotations["notes"]
        or next_annotations.get("analysis")
    ):
        return {"annotations": next_annotations}
    return {}


def annotation_agent_id(source: dict[str, Any], trajectory: dict[str, Any]) -> str | None:
    agent = trajectory.get("agent")
    trajectory_agent = agent.get("name") if isinstance(agent, dict) else None
    return (
        optional_str(source.get("agent_name"))
        or optional_str(trajectory_agent)
        or optional_str(source.get("adapter"))
    )


def parsed_object(value: Any) -> dict[str, Any]:
    if isinstance(value, dict):
        return value
    if not isinstance(value, str):
        return {}
    try:
        parsed = json.loads(value)
    except json.JSONDecodeError:
        return {}
    return parsed if isinstance(parsed, dict) else {}


def optional_str(value: Any) -> str | None:
    return None if value is None else str(value)


def optional_int(value: Any) -> int | None:
    if value is None:
        return None
    return int(value)
