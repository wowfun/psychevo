from __future__ import annotations

from typing import Any

from peval_py.inputs import LoadedSession
from peval_py._state.artifacts import normalized_optional_path

def loaded_session_from_source(source: dict[str, Any]) -> LoadedSession:
    return LoadedSession(
        records=None,
        input_label=str(source["label"]),
        adapter_id=str(source["adapter"]),
        input_path=source.get("input_path") or source.get("db_path"),
        db_path=source.get("db_path"),
        session_hint=source.get("session_id"),
        agent_name=source.get("agent_name"),
        agent_version=source.get("agent_version"),
        model=source.get("model"),
        source_alias=source.get("source_alias"),
        source_kind=str(source["kind"]),
    )


def source_row_for_session(session: LoadedSession) -> dict[str, Any]:
    return {
        "kind": session.source_kind,
        "adapter": session.adapter_id,
        "label": session.input_label,
        "input_path": normalized_optional_path(session.input_path),
        "db_path": normalized_optional_path(session.db_path),
        "session_id": session.session_hint,
        "source_alias": session.source_alias,
        "agent_name": session.agent_name,
        "agent_version": session.agent_version,
        "model": session.model,
    }


def trial_payload_from_report(
    report: dict[str, Any],
) -> tuple[dict[str, Any], dict[str, Any]]:
    trajectories = report.get("trajectory") or []
    metas = report.get("trajectory_meta") or []
    if len(trajectories) != 1 or len(metas) != 1:
        raise ValueError("source refresh must produce exactly one Trial")
    trajectory = trajectories[0]
    meta = metas[0]
    if not isinstance(trajectory, dict) or not isinstance(meta, dict):
        raise ValueError("source refresh produced non-object Trial data")
    return trajectory, meta
