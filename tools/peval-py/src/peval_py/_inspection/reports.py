from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

from peval_py.inputs import AdapterAssignments, load_inputs
from peval_py.pipeline import build_report_from_loaded_inputs

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
