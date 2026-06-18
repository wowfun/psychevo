from __future__ import annotations

import hashlib
import os
from copy import deepcopy
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from peval_py.analysis import cached_analysis_report, cached_note_report
from peval_py.adapters.base import (
    ConversionResult,
    ObservationMeta,
    StepMeta,
    ToolMeta,
    timestamp_fallback_allowed,
    timestamp_fallback_duration_ms,
)
from peval_py.config import ToolConfig
from peval_py.redaction import redact_value

VIEW_SCHEMA_VERSION = 18


@dataclass(frozen=True)
class ReportSession:
    conversion: ConversionResult
    input_label: str
    input_path: str | None = None
    session_hint: str | None = None
    adapter_id: str | None = None
    analysis_agent_id: str | None = None
    source_alias: str | None = None


@dataclass(frozen=True)
class NoteInput:
    index: int
    markdown: str


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
    input_label = sessions[0].input_label if len(sessions) == 1 else "sessions"
    includes = ["core"]
    report: dict[str, Any] = {
        "schema_version": VIEW_SCHEMA_VERSION,
        "includes": includes,
        "scope": {
            "workspace_root": ".",
            "path": input_label,
            "benchmark": None,
        },
        "path_selections": path_selections(prepared),
        "trajectory": trajectories,
        "trajectory_meta": metas,
    }
    if multi:
        includes.append("comparison")
        report["comparison"] = comparison_report(prepared)
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
        "scope": {
            "workspace_root": ".",
            "path": input_label,
            "benchmark": None,
        },
        "path_selections": [
            {
                "id": "input" if len(trajectories) == 1 else f"input-{index}",
                "label": str(
                    trajectory.get("session_id")
                    or meta.get("trial_key")
                    or f"session-{index}"
                ),
                "path": str(
                    trajectory.get("session_id")
                    or meta.get("trial_key")
                    or f"session-{index}"
                ),
                "cell_count": 1,
            }
            for index, (trajectory, meta) in enumerate(
                zip(trajectories, metas, strict=True),
                start=1,
            )
        ],
        "trajectory": trajectories,
        "trajectory_meta": metas,
    }
    if len(trajectories) > 1:
        includes.append("comparison")
        report["comparison"] = comparison_report(
            [
                {"index": index, "trajectory": trajectory, "meta": meta}
                for index, (trajectory, meta) in enumerate(
                    zip(trajectories, metas, strict=True),
                    start=1,
                )
            ]
        )
    notes = note_reports_from_snapshots(source_reports or [], metas)
    analyses = analysis_reports_from_snapshots(source_reports or [], metas)
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
        "scope": {
            "workspace_root": ".",
            "path": input_label,
            "benchmark": None,
        },
        "path_selections": [],
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
        base = str(trajectory.get("trajectory_id") or config.trajectory_id or "session:t001")
    else:
        base = f"session:{safe_key_part(trajectory.get('session_id') or f's{index}')}"
    count = seen.get(base, 0) + 1
    seen[base] = count
    return base if count == 1 else f"{base}:{count}"


def safe_key_part(value: object) -> str:
    text = str(value or "").strip().lower()
    out = "".join(ch if ch.isalnum() or ch in "._-" else "-" for ch in text)
    return out.strip(".-") or "session"


def path_selections(prepared: list[dict[str, Any]]) -> list[dict[str, Any]]:
    selections = []
    for item in prepared:
        single = len(prepared) == 1
        label = item["input_label"]
        selections.append(
            {
                "id": "input" if single else f"input-{item['index']}",
                "label": label,
                "path": label,
                "cell_count": 1,
            }
        )
    return selections


def comparison_report(prepared: list[dict[str, Any]]) -> dict[str, Any]:
    rows = [comparison_row(item) for item in prepared]
    selected = next((row for row in rows if row["status"] != "passed"), rows[0])
    total_cost = sum(row["cost_usd"] for row in rows if row["cost_usd"] is not None)
    have_cost = any(row["cost_usd"] is not None for row in rows)
    return {
        "selected_trial_key": selected["trial_key"],
        "summary": {
            "session_count": len(rows),
            "passed": sum(1 for row in rows if row["status"] == "passed"),
            "failed": sum(1 for row in rows if row["status"] != "passed"),
            "warnings": sum(row["warnings"] for row in rows),
            "turns": sum(row["turns"] or 0 for row in rows),
            "tools": sum(row["total_tool_calls"] or 0 for row in rows),
            "tool_errors": sum(row["total_tool_errors"] or 0 for row in rows),
            "tokens": sum(row["tokens"] or 0 for row in rows),
            "cost_usd": round(total_cost, 12) if have_cost else None,
        },
        "leaderboard": {"entries": [dict(row) for row in rows]},
    }


def comparison_row(item: dict[str, Any]) -> dict[str, Any]:
    trajectory = item["trajectory"]
    meta = item["meta"]
    metrics = trajectory.get("final_metrics", {})
    total_tool_calls = int(metrics.get("total_tool_calls") or 0)
    total_tool_errors = int(metrics.get("total_tool_errors") or 0)
    return {
        "trial_key": meta["trial_key"],
        "session_id": trajectory.get("session_id") or "-",
        **optional("source_alias", meta.get("source_alias")),
        "adapter": meta.get("adapter"),
        "model": trajectory.get("agent", {}).get("model_name"),
        "status": meta.get("status"),
        "finished_at_ms": meta.get("finished_at_ms"),
        "duration_ms": meta.get("duration_ms"),
        "wall_duration_ms": trial_wall_duration_ms(meta),
        "turns": metrics.get("total_turns"),
        "total_tool_calls": total_tool_calls,
        "total_tool_errors": total_tool_errors,
        "tokens": token_total(metrics),
        "cost_usd": metrics.get("total_cost_usd"),
        "warnings": len(meta.get("warnings") or []),
    }


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
        report = cached_analysis_report(
            workspace_root=config.workspace_root,
            eval_slug=config.analysis_eval_slug,
            agent_id=item.get("analysis_agent_id"),
            session_id=trajectory.get("session_id"),
            trial_key=str(meta.get("trial_key") or ""),
        )
        if report is not None:
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
    metas: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    reports: list[dict[str, Any]] = []
    for index, source_report in enumerate(source_reports):
        if index >= len(metas) or not isinstance(source_report, dict):
            continue
        annotations = source_report.get("annotations")
        if not isinstance(annotations, dict):
            continue
        for item in annotations.get("analysis") or []:
            if not isinstance(item, dict):
                continue
            remapped = {
                key: value
                for key, value in item.items()
                if key in {"status", "relative_path", "summary", "md_report", "relative_paths"}
            }
            if remapped.get("status") != "cached" or not remapped.get("relative_path"):
                continue
            remapped["trial_key"] = str(metas[index].get("trial_key") or "")
            reports.append(remapped)
    return reports


def trial_active_duration_ms(
    steps: list[StepMeta],
    step_reports: list[dict[str, Any]],
) -> int | None:
    total = 0
    observed = False
    for step, report in zip(steps, step_reports, strict=True):
        if step.source != "agent":
            continue
        duration = report.get("duration_ms")
        if duration is not None:
            observed = True
            total += int(duration)
        for tool in step.tool_calls:
            if tool.execution_duration_ms is not None:
                observed = True
                total += int(tool.execution_duration_ms)
    return total if observed else None


def step_meta_reports(
    steps: list[StepMeta],
    started: int,
    timestamp_semantics: str | None,
) -> list[dict[str, Any]]:
    reports = []
    for index, step in enumerate(steps):
        timestamp = step.timestamp_ms
        elapsed = timestamp - started if timestamp is not None and started else None
        next_timestamp = next(
            (
                candidate.timestamp_ms
                for candidate in steps[index + 1 :]
                if candidate.timestamp_ms is not None
            ),
            None,
        )
        duration = step_duration_ms(step, next_timestamp, timestamp_semantics)
        reports.append(
            {
                "step_id": step.step_id,
                "tool_calls": [tool_meta_report(tool) for tool in step.tool_calls],
                "observations": [
                    observation_meta_report(observation)
                    for observation in step.observations
                ],
                "tool_error": step.tool_error,
                **optional("timestamp_ms", timestamp),
                **optional("elapsed_ms", elapsed),
                "duration_ms": duration,
                **optional("duration_source", step.duration_source),
                **optional("data_preview", step.data_preview),
                "truncated": step.truncated,
            }
        )
    return reports


def step_duration_ms(
    step: StepMeta,
    next_timestamp_ms: int | None,
    timestamp_semantics: str | None,
) -> int | None:
    timestamp_ms = step.timestamp_ms
    if timestamp_ms is None:
        return step.duration_ms
    if step.duration_ms is not None:
        return max(0, step.duration_ms)
    if (
        step.source == "agent"
        and next_timestamp_ms is not None
        and not step.tool_calls
        and timestamp_fallback_allowed(timestamp_semantics)
    ):
        return timestamp_fallback_duration_ms(timestamp_ms, next_timestamp_ms)
    return None


def grouped_step_end_timestamp_ms(step: StepMeta, start_ms: int) -> int | None:
    end_candidates: list[int] = []
    for observation in step.observations:
        duration = timestamp_fallback_duration_ms(start_ms, observation.timestamp_ms)
        if duration is not None:
            end_candidates.append(start_ms + duration)
    for tool in step.tool_calls:
        tool_end = tool_end_timestamp_ms(tool)
        if tool_end is not None:
            end_candidates.append(tool_end)
    return max(end_candidates) if end_candidates else None


def tool_end_timestamp_ms(tool: ToolMeta) -> int | None:
    if tool.execution_duration_ms is None:
        return None
    start = tool.timestamp_ms
    if start is None:
        return None
    return start + tool.execution_duration_ms


def tool_meta_report(tool: ToolMeta) -> dict[str, Any]:
    return {
        "tool_call_id": tool.tool_call_id,
        **optional("status", tool.status),
        **optional("title", tool.title),
        **optional("timestamp_ms", tool.timestamp_ms),
        **optional("generation_duration_ms", tool.generation_duration_ms),
        **optional("execution_duration_ms", tool.execution_duration_ms),
        **optional("execution_duration_source", tool.execution_duration_source),
        "truncated": tool.truncated,
    }


def observation_meta_report(observation: ObservationMeta) -> dict[str, Any]:
    return {
        **optional("source_call_id", observation.source_call_id),
        **optional("status", observation.status),
        **optional("title", observation.title),
        **optional("timestamp_ms", observation.timestamp_ms),
        "tool_error": observation.tool_error,
        "truncated": observation.truncated,
    }


def trial_wall_duration_ms(meta: dict[str, Any]) -> int | None:
    if meta.get("wall_duration_ms") is not None:
        return int(meta["wall_duration_ms"])
    if meta.get("started_at_ms") is not None and meta.get("finished_at_ms") is not None:
        return max(0, int(meta["finished_at_ms"]) - int(meta["started_at_ms"]))
    return meta.get("duration_ms")


def token_total(metrics: dict[str, Any]) -> int | None:
    usage = metrics.get("usage")
    if isinstance(usage, dict) and usage.get("total_tokens") is not None:
        return int(usage["total_tokens"])
    values = [
        metrics.get("total_prompt_tokens"),
        metrics.get("total_completion_tokens"),
        metrics.get("total_cached_tokens"),
    ]
    present = [int(value) for value in values if value is not None]
    return sum(present) if present else None


def data_ref_for_input(label: str, input_path: str | None) -> dict[str, Any]:
    relative = label
    size = 0
    digest: str | None = None
    modified_ms: int | None = None
    if input_path:
        path = Path(input_path)
        if path.exists():
            stat = path.stat()
            size = stat.st_size
            modified_ms = int(stat.st_mtime * 1000)
            digest = file_hash(path)
            try:
                relative_path = Path(os.path.relpath(path, Path.cwd()))
                relative = str(relative_path) if not str(relative_path).startswith("..") else path.name
            except ValueError:
                relative = path.name
    ref = {
        "kind": "input",
        "label": label,
        "relative_path": relative,
        "mime": "application/jsonl" if label.endswith(".jsonl") else "application/octet-stream",
        "size_bytes": size,
    }
    if digest:
        ref["content_hash"] = digest
    if modified_ms is not None:
        ref["modified_ms"] = modified_ms
    return ref


def file_hash(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 64), b""):
            hasher.update(chunk)
    return f"sha256:{hasher.hexdigest()}"


def optional(key: str, value: Any) -> dict[str, Any]:
    return {} if value is None else {key: value}
