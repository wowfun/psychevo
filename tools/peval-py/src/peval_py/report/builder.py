from __future__ import annotations

from copy import deepcopy
from typing import Any

from peval_py.analysis import (
    ANALYSIS_REPORT_FIELDS,
    RESERVED_ANALYSIS_METRIC_KEYS,
    cached_analysis_report,
    cached_note_report,
)
from peval_py.adapters.base import (
    ConversionResult,
)
from peval_py.config import ToolConfig
from peval_py.models import NoteInput, ReportSession
from peval_py.redaction import redact_value
from peval_py.report.data_ref import data_ref_for_input
from peval_py.report.metrics import automatic_analysis_metrics
from peval_py.report.timing import step_meta_reports, trial_active_duration_ms

VIEW_SCHEMA_VERSION = 19


def build_report(
    conversion: ConversionResult,
    config: ToolConfig,
    input_label: str,
    input_path: str | None = None,
) -> dict[str, Any]:
    return build_multi_report(
        [ReportSession(conversion, input_label, input_path)],
        config,
        [],
    )


def build_multi_report(
    sessions: list[ReportSession],
    config: ToolConfig,
    notes: list[NoteInput] | None = None,
) -> dict[str, Any]:
    if not sessions:
        raise ValueError("at least one session is required")
    notes = notes or []
    multi = len(sessions) > 1
    prepared: list[dict[str, Any]] = []
    seen_trial_keys: dict[str, int] = {}
    for index, session in enumerate(sessions, start=1):
        prepared.append(prepare_session_report(index, session, config, multi, seen_trial_keys))

    trajectories = [item["trajectory"] for item in prepared]
    metas = [item["meta"] for item in prepared]
    includes = ["core"]
    report: dict[str, Any] = {
        "schema_version": VIEW_SCHEMA_VERSION,
        "includes": includes,
        "trajectory": trajectories,
        "trajectory_meta": metas,
    }
    annotations = annotations_report(
        notes,
        metas,
        cell_note_reports(config, prepared),
        analysis_reports(config, prepared),
    )
    if annotations:
        includes.append("annotations")
        report["annotations"] = annotations
    return report


def build_report_from_snapshots(
    trajectories: list[dict[str, Any]],
    metas: list[dict[str, Any]],
    *,
    input_label: str = "serve",
    source_reports: list[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    if len(trajectories) != len(metas):
        raise ValueError("trajectory and meta snapshot counts differ")
    if not trajectories:
        return empty_report(input_label)
    includes = ["core"]
    report: dict[str, Any] = {
        "schema_version": VIEW_SCHEMA_VERSION,
        "includes": includes,
        "trajectory": trajectories,
        "trajectory_meta": metas,
    }
    notes = note_reports_from_snapshots(source_reports or [], metas)
    analyses = analysis_reports_from_snapshots(source_reports or [], trajectories, metas)
    if notes or analyses:
        includes.append("annotations")
        report["annotations"] = {"report_notes": [], "notes": notes}
        if analyses:
            report["annotations"]["analysis"] = analyses
    return report


def empty_report(input_label: str = "serve") -> dict[str, Any]:
    return {
        "schema_version": VIEW_SCHEMA_VERSION,
        "includes": ["core"],
        "trajectory": [],
        "trajectory_meta": [],
    }


def prepare_session_report(
    index: int,
    session: ReportSession,
    config: ToolConfig,
    multi: bool,
    seen_trial_keys: dict[str, int],
) -> dict[str, Any]:
    conversion = session.conversion
    trajectory = deepcopy(conversion.trajectory)
    session_id = display_session_id(trajectory, session.session_hint)
    if session_id:
        trajectory["session_id"] = session_id
    if config.redact:
        trajectory = redact_value(trajectory)
    trial_key = trial_key_for(index, trajectory, config, multi, seen_trial_keys)
    started = conversion.started_at_ms or 0
    finished = conversion.finished_at_ms or started
    wall_duration = max(0, finished - started)
    steps = step_meta_reports(
        conversion.steps_meta,
        started,
        conversion.timestamp_semantics,
    )
    status = "failed" if conversion.warnings or conversion.unmapped_events else "passed"
    data_ref = data_ref_for_input(session.input_label, session.input_path)
    adapter_id = session.adapter_id or config.adapter
    meta = {
        "trial_key": trial_key,
        "adapter": adapter_id,
        **optional("timestamp_semantics", conversion.timestamp_semantics),
        "started_at_ms": started,
        "finished_at_ms": finished,
        "wall_duration_ms": wall_duration,
        "duration_ms": trial_active_duration_ms(conversion.steps_meta, steps),
        "status": status,
        "failure_class": None if status == "passed" else "conversion",
        "score": None,
        "score_message": "offline session conversion",
        "warnings": conversion.warnings,
        "data_ref": data_ref,
        **optional("source_alias", session.source_alias),
        "total_events": conversion.total_events,
        "unmapped_events": conversion.unmapped_events,
        "prompt_unavailable": not any(
            step.get("source") == "user" for step in trajectory.get("steps", [])
        ),
        "steps": steps,
    }
    return {
        "index": index,
        "input_label": session.input_label,
        "input_path": session.input_path,
        "source_alias": session.source_alias,
        "analysis_agent_id": session.analysis_agent_id or adapter_id,
        "trajectory": trajectory,
        "meta": meta,
    }


def display_session_id(trajectory: dict[str, Any], session_hint: str | None) -> str | None:
    if trajectory.get("session_id") is not None:
        return str(trajectory["session_id"])
    if session_hint:
        return str(session_hint)
    return None


def trial_key_for(
    index: int,
    trajectory: dict[str, Any],
    config: ToolConfig,
    multi: bool,
    seen: dict[str, int],
) -> str:
    if not multi:
        base = str(trajectory.get("trajectory_id") or "session:t001")
    else:
        base = f"session:{safe_key_part(trajectory.get('session_id') or f's{index}')}"
    count = seen.get(base, 0) + 1
    seen[base] = count
    return base if count == 1 else f"{base}:{count}"


def safe_key_part(value: object) -> str:
    text = str(value or "").strip().lower()
    out = "".join(ch if ch.isalnum() or ch in "._-" else "-" for ch in text)
    return out.strip(".-") or "session"


def annotations_report(
    notes: list[NoteInput],
    metas: list[dict[str, Any]],
    cell_notes: list[dict[str, Any]] | None = None,
    analyses: list[dict[str, Any]] | None = None,
) -> dict[str, Any] | None:
    cell_notes = cell_notes or []
    analyses = analyses or []
    if not notes and not cell_notes and not analyses:
        return None
    report_notes: list[dict[str, Any]] = []
    cli_notes_by_trial: dict[str, list[dict[str, Any]]] = {}
    cell_notes_by_trial: dict[str, list[dict[str, Any]]] = {}
    report_count = 0
    trial_counts: dict[str, int] = {}
    for note in cell_notes:
        trial_key = str(note.get("trial_key") or "")
        if not trial_key:
            continue
        cell_notes_by_trial.setdefault(trial_key, []).append(deepcopy(note))
    for note in notes:
        if note.index == 0:
            report_count += 1
            report_notes.append(
                {"label": f"Report note {report_count}", "markdown": note.markdown}
            )
            continue
        meta = metas[note.index - 1]
        trial_key = str(meta["trial_key"])
        trial_counts[trial_key] = trial_counts.get(trial_key, 0) + 1
        cli_notes_by_trial.setdefault(trial_key, []).append(
            {
                "trial_key": trial_key,
                "source": "cli",
                "label": f"CLI note {trial_counts[trial_key]}",
                "markdown": note.markdown,
            }
        )
    trial_notes: list[dict[str, Any]] = []
    for meta in metas:
        trial_key = str(meta["trial_key"])
        trial_notes.extend(cell_notes_by_trial.get(trial_key, []))
        trial_notes.extend(cli_notes_by_trial.get(trial_key, []))
    annotations: dict[str, Any] = {"report_notes": report_notes, "notes": trial_notes}
    if analyses:
        annotations["analysis"] = analyses
    return annotations


def analysis_reports(
    config: ToolConfig,
    prepared: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    reports: list[dict[str, Any]] = []
    for item in prepared:
        meta = item["meta"]
        trajectory = item["trajectory"]
        report = computed_analysis_report(trajectory, meta)
        cached = cached_analysis_report(
            workspace_root=config.workspace_root,
            eval_slug=config.analysis_eval_slug,
            agent_id=item.get("analysis_agent_id"),
            session_id=trajectory.get("session_id"),
            trial_key=str(meta.get("trial_key") or ""),
        )
        if cached is not None:
            report = merge_analysis_report(report, cached)
        reports.append(report)
    return reports


def cell_note_reports(
    config: ToolConfig,
    prepared: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    reports: list[dict[str, Any]] = []
    for item in prepared:
        meta = item["meta"]
        trajectory = item["trajectory"]
        report = cached_note_report(
            workspace_root=config.workspace_root,
            eval_slug=config.analysis_eval_slug,
            agent_id=item.get("analysis_agent_id"),
            session_id=trajectory.get("session_id"),
            trial_key=str(meta.get("trial_key") or ""),
        )
        if report is not None:
            reports.append(report)
    return reports


def note_reports_from_snapshots(
    source_reports: list[dict[str, Any]],
    metas: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    reports: list[dict[str, Any]] = []
    for index, source_report in enumerate(source_reports):
        if index >= len(metas) or not isinstance(source_report, dict):
            continue
        annotations = source_report.get("annotations")
        if not isinstance(annotations, dict):
            continue
        for item in annotations.get("notes") or []:
            if not isinstance(item, dict) or not isinstance(item.get("markdown"), str):
                continue
            remapped = {
                key: deepcopy(value)
                for key, value in item.items()
                if key in {"source", "label", "markdown", "source_ref"}
            }
            remapped["trial_key"] = str(metas[index].get("trial_key") or "")
            reports.append(remapped)
    return reports


def analysis_reports_from_snapshots(
    source_reports: list[dict[str, Any]],
    trajectories: list[dict[str, Any]],
    metas: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    reports: list[dict[str, Any]] = []
    for index, (trajectory, meta) in enumerate(
        zip(trajectories, metas, strict=True)
    ):
        report = computed_analysis_report(trajectory, meta)
        source_report = source_reports[index] if index < len(source_reports) else None
        if not isinstance(source_report, dict):
            reports.append(report)
            continue
        annotations = source_report.get("annotations")
        if not isinstance(annotations, dict):
            reports.append(report)
            continue
        for item in annotations.get("analysis") or []:
            if not isinstance(item, dict):
                continue
            remapped = {
                key: deepcopy(value)
                for key, value in item.items()
                if key
                in {
                    "status",
                    "relative_path",
                    "md_report",
                    "relative_paths",
                    *ANALYSIS_REPORT_FIELDS,
                }
            }
            if remapped.get("status") != "cached" or not remapped.get("relative_path"):
                continue
            remapped["trial_key"] = str(metas[index].get("trial_key") or "")
            report = merge_analysis_report(report, remapped)
        reports.append(report)
    return reports


def computed_analysis_report(
    trajectory: dict[str, Any],
    meta: dict[str, Any],
) -> dict[str, Any]:
    return {
        "trial_key": str(meta.get("trial_key") or ""),
        "status": "computed",
        "analysis_metrics": {
            "auto": automatic_analysis_metrics(trajectory, meta),
        },
    }


def merge_analysis_report(
    base: dict[str, Any],
    overlay: dict[str, Any],
) -> dict[str, Any]:
    merged = deepcopy(base)
    for key, value in overlay.items():
        if key == "trial_key":
            continue
        if key == "analysis_metrics":
            merged["analysis_metrics"] = merge_analysis_metrics(
                merged.get("analysis_metrics"),
                value,
            )
            continue
        merged[key] = deepcopy(value)
    if overlay.get("status") == "cached":
        merged["status"] = "cached"
    return merged


def merge_analysis_metrics(base: Any, overlay: Any) -> dict[str, Any]:
    metrics: dict[str, Any] = {}
    if isinstance(base, dict):
        for key, value in base.items():
            metrics[str(key)] = deepcopy(value)
    if isinstance(overlay, dict):
        for key, value in overlay.items():
            key_text = str(key)
            if key_text in RESERVED_ANALYSIS_METRIC_KEYS:
                continue
            metrics[key_text] = deepcopy(value)
    return metrics



def optional(key: str, value: Any) -> dict[str, Any]:
    return {} if value is None else {key: value}
