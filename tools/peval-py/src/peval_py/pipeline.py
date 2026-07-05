from __future__ import annotations

from copy import deepcopy
from dataclasses import replace
from typing import Any

from peval_py.atif import convert_db, convert_path, convert_records
from peval_py.adapters.base import ConversionResult
from peval_py.config import ToolConfig, config_for_adapter
from peval_py.models import LoadedInputs, LoadedSession
from peval_py.report import (
    NoteInput,
    ReportSession,
    annotations_report,
    build_multi_report,
    build_report_from_snapshots,
    empty_report,
)


def config_for_session(session: LoadedSession, config: ToolConfig) -> ToolConfig:
    session_config = config_for_adapter(config, session.adapter_id)
    updates: dict[str, str] = {}
    if session.agent_name is not None:
        updates["agent_name"] = session.agent_name
    if session.agent_version is not None:
        updates["agent_version"] = session.agent_version
    if session.model is not None:
        updates["model"] = session.model
    return replace(session_config, **updates) if updates else session_config


def convert_session(session: LoadedSession, config: ToolConfig) -> ConversionResult:
    if session.snapshot_trajectory is not None:
        raise ValueError("workspace snapshot sessions are already converted")
    session_config = config_for_session(session, config)
    if session.db_path is not None:
        return convert_db(session.db_path, session.session_hint, session_config)
    if session.records is None:
        if not session.input_path:
            raise ValueError("path input is missing a source path")
        return convert_path(session.input_path, session_config)
    return convert_records(session.records, session_config)


def report_session_for_loaded(
    session: LoadedSession,
    config: ToolConfig,
) -> ReportSession:
    return ReportSession(
        conversion=convert_session(session, config),
        input_label=session.input_label,
        input_path=session.input_path,
        session_hint=session.session_hint,
        adapter_id=session.adapter_id,
        analysis_agent_id=session.agent_name or config.agent_name or session.adapter_id,
        source_alias=session.source_alias,
    )


def build_report_from_loaded_inputs(
    loaded_inputs: LoadedInputs,
    config: ToolConfig,
    raw_notes: list[str] | None = None,
) -> dict:
    if not loaded_inputs.sessions:
        return empty_report("view")
    if any(session.snapshot_trajectory is not None for session in loaded_inputs.sessions):
        return build_report_from_loaded_snapshots(loaded_inputs, config, raw_notes)
    report_sessions = [
        report_session_for_loaded(session, config)
        for session in loaded_inputs.sessions
    ]
    notes = parse_notes(
        [*(raw_notes or []), *loaded_inputs.notes],
        len(report_sessions),
    )
    return build_multi_report(report_sessions, config, notes)


def build_report_from_loaded_snapshots(
    loaded_inputs: LoadedInputs,
    config: ToolConfig,
    raw_notes: list[str] | None = None,
) -> dict:
    trajectories: list[dict[str, Any]] = []
    metas: list[dict[str, Any]] = []
    source_reports: list[dict[str, Any]] = []
    for session in loaded_inputs.sessions:
        if session.snapshot_trajectory is not None and session.snapshot_meta is not None:
            trajectories.append(deepcopy(session.snapshot_trajectory))
            metas.append(meta_with_loaded_alias(session.snapshot_meta, session.source_alias))
            source_reports.append(deepcopy(session.snapshot_source_report or {}))
            continue
        report = build_multi_report([report_session_for_loaded(session, config)], config, [])
        trajectories.append(deepcopy(report["trajectory"][0]))
        metas.append(deepcopy(report["trajectory_meta"][0]))
        source_reports.append(report)
    metas = uniquify_trial_keys(metas)
    notes = parse_notes(
        [*(raw_notes or []), *loaded_inputs.notes],
        len(loaded_inputs.sessions),
    )
    report = build_report_from_snapshots(
        trajectories,
        metas,
        input_label="view",
        source_reports=source_reports,
    )
    note_annotations = annotations_report(notes, metas, [], [])
    if note_annotations:
        merge_annotations(report, note_annotations)
    return report


def meta_with_loaded_alias(meta: dict[str, Any], alias: str | None) -> dict[str, Any]:
    copied = deepcopy(meta)
    if alias:
        copied["source_alias"] = alias
    elif "source_alias" in copied:
        copied.pop("source_alias", None)
    return copied


def uniquify_trial_keys(metas: list[dict[str, Any]]) -> list[dict[str, Any]]:
    seen: dict[str, int] = {}
    out: list[dict[str, Any]] = []
    for meta in metas:
        copied = deepcopy(meta)
        base = str(copied.get("trial_key") or "trial")
        count = seen.get(base, 0) + 1
        seen[base] = count
        if count > 1:
            copied["trial_key"] = f"{base}:{count}"
        out.append(copied)
    return out


def merge_annotations(report: dict[str, Any], annotations: dict[str, Any]) -> None:
    existing = report.setdefault("annotations", {"report_notes": [], "notes": []})
    includes = report.setdefault("includes", ["core"])
    if "annotations" not in includes:
        includes.append("annotations")
    existing.setdefault("report_notes", []).extend(annotations.get("report_notes") or [])
    existing.setdefault("notes", []).extend(annotations.get("notes") or [])
    if annotations.get("analysis"):
        existing.setdefault("analysis", []).extend(annotations["analysis"])


def parse_notes(raw_notes: list[str], session_count: int) -> list[NoteInput]:
    notes: list[NoteInput] = []
    for raw in raw_notes:
        if "=" not in raw:
            raise ValueError("--note must use N=TEXT syntax")
        raw_index, markdown = raw.split("=", 1)
        if not raw_index.isdigit():
            raise ValueError("--note index must be a non-negative integer")
        index = int(raw_index)
        if index > session_count:
            raise ValueError(
                f"--note index {index} is out of range for {session_count} sessions"
            )
        notes.append(NoteInput(index=index, markdown=markdown))
    return notes
