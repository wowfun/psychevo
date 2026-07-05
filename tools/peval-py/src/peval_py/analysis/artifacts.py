from __future__ import annotations

import json
import math
from copy import deepcopy
from pathlib import Path
from typing import Any

from peval_py.analysis.constants import (
    MAX_NOTE_BYTES,
    MERGEABLE_ANALYSIS_LIST_FIELDS,
    RESERVED_ANALYSIS_METRIC_KEYS,
)

def write_note_file(path: Path, root: Path, markdown: str) -> str:
    if not isinstance(markdown, str):
        raise ValueError("markdown must be a string")
    if len(markdown.encode("utf-8")) > MAX_NOTE_BYTES:
        raise ValueError("notes.md exceeds 1 MiB limit")
    target = path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(markdown, encoding="utf-8")
    try:
        return target.relative_to(root).as_posix()
    except ValueError as exc:
        raise ValueError("notes.md target is outside the workspace root") from exc


def task_root_for(
    *,
    workspace_root: str | None,
    eval_slug: str,
    agent_id: str | None,
    session_id: str | None,
) -> tuple[Path, Path] | None:
    root = safe_root(workspace_root)
    eval_part = safe_segment(eval_slug)
    agent_part = safe_segment(agent_id)
    session_part = safe_segment(session_id)
    if root is None or eval_part is None or agent_part is None or session_part is None:
        return None
    return root, root / "runs" / eval_part / agent_part / session_part


def cell_root_for(
    *,
    workspace_root: str | None,
    eval_slug: str,
    agent_id: str | None,
    session_id: str | None,
    cell_key: str | None,
) -> tuple[Path, Path] | None:
    roots = task_root_for(
        workspace_root=workspace_root,
        eval_slug=eval_slug,
        agent_id=agent_id,
        session_id=session_id,
    )
    cell_part = safe_segment(cell_key)
    if roots is None or cell_part is None:
        return None
    root, task_root = roots
    return root, task_root / cell_part


def read_json_analysis(path: Path, root: Path) -> tuple[str, dict[str, Any]] | None:
    if not path.is_file():
        return None
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError, UnicodeDecodeError):
        return None
    if not isinstance(payload, dict):
        return None
    try:
        relative_path = path.relative_to(root).as_posix()
    except ValueError:
        return None
    return relative_path, analysis_fields_from_json(payload)


def analysis_fields_from_json(payload: dict[str, Any]) -> dict[str, Any]:
    fields: dict[str, Any] = {}
    summary = payload.get("summary")
    if isinstance(summary, str) and summary.strip():
        fields["summary"] = summary
    status = payload.get("status")
    if isinstance(status, str) and status.strip():
        fields["analysis_status"] = status
    subject = payload.get("subject")
    if isinstance(subject, dict) and subject:
        fields["subject"] = deepcopy(subject)
    for key in MERGEABLE_ANALYSIS_LIST_FIELDS:
        value = payload.get(key)
        if isinstance(value, list) and value:
            fields[key] = deepcopy(value)
    metrics = imported_metrics_from_json(payload)
    if metrics:
        fields["analysis_metrics"] = metrics
    confidence = payload.get("confidence")
    if isinstance(confidence, str) and confidence.strip():
        fields["confidence"] = confidence
    elif (
        isinstance(confidence, (int, float))
        and not isinstance(confidence, bool)
        and math.isfinite(float(confidence))
    ):
        fields["confidence"] = confidence
    return fields


def imported_metrics_from_json(payload: dict[str, Any]) -> dict[str, Any]:
    metrics: dict[str, Any] = {}
    extra = payload.get("extra")
    if isinstance(extra, dict):
        merge_imported_metric_source(metrics, extra.get("metrics"))
    merge_imported_metric_source(metrics, payload.get("metrics"))
    return metrics


def merge_imported_metric_source(target: dict[str, Any], value: Any) -> None:
    if not isinstance(value, dict):
        return
    for key, item in value.items():
        if key in RESERVED_ANALYSIS_METRIC_KEYS:
            continue
        target[str(key)] = deepcopy(item)


def read_markdown_report(path: Path, root: Path) -> tuple[str, str | None] | None:
    if not path.is_file():
        return None
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        return None
    try:
        relative_path = path.relative_to(root).as_posix()
    except ValueError:
        return None
    return relative_path, text if text.strip() else None


def read_note_report(path: Path, root: Path, trial_key: str) -> dict[str, Any] | None:
    if not path.is_file():
        return None
    try:
        if path.stat().st_size > MAX_NOTE_BYTES:
            return None
        markdown = path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        return None
    try:
        relative_path = path.relative_to(root).as_posix()
    except ValueError:
        return None
    return {
        "trial_key": str(trial_key),
        "source": "cell",
        "label": "notes.md",
        "markdown": markdown,
        "source_ref": {
            "kind": "note",
            "label": "notes.md",
            "relative_path": relative_path,
        },
    }


def safe_root(value: str | Path | None) -> Path | None:
    if value is None:
        return None
    try:
        root = Path(value).expanduser().resolve()
    except (OSError, RuntimeError):
        return None
    return root if root.is_dir() else None


def safe_cell_dir(root: Path, value: str | Path) -> Path | None:
    try:
        path = Path(value).expanduser()
        if not path.is_absolute():
            path = root / path
        cell_dir = path.resolve()
        cell_dir.relative_to(root)
    except (OSError, RuntimeError, ValueError):
        return None
    return cell_dir


def safe_segment(value: object) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    safe = "".join(
        char if char.isalnum() or char in {"-", "_", "."} else "_"
        for char in text
    ).strip("._")
    if not safe:
        return None
    return safe
