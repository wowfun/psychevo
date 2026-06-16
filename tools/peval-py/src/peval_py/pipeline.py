from __future__ import annotations

from dataclasses import replace

from peval_py.atif import convert_db, convert_path, convert_records
from peval_py.adapters.base import ConversionResult
from peval_py.config import ToolConfig, config_for_adapter
from peval_py.inputs import LoadedInputs, LoadedSession
from peval_py.report import NoteInput, ReportSession, build_multi_report


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
    )


def build_report_from_loaded_inputs(
    loaded_inputs: LoadedInputs,
    config: ToolConfig,
    raw_notes: list[str] | None = None,
) -> dict:
    report_sessions = [
        report_session_for_loaded(session, config)
        for session in loaded_inputs.sessions
    ]
    notes = parse_notes(
        [*(raw_notes or []), *loaded_inputs.notes],
        len(report_sessions),
    )
    return build_multi_report(report_sessions, config, notes)


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
