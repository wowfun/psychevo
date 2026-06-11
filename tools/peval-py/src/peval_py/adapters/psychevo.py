from __future__ import annotations

import json
import sqlite3
from dataclasses import replace
from pathlib import Path
from typing import Any

from peval_py.adapters.base import SessionInfo
from peval_py.adapters.common import CommonMessageAdapter
from peval_py.config import ToolConfig
from peval_py.sources import MessageRecord, read_jsonl, read_sqlite_messages


class PsychevoAdapter(CommonMessageAdapter):
    agent_id = "psychevo"
    default_agent_name = "psychevo"

    def convert_path(self, path: str, config: ToolConfig):
        source = Path(path).expanduser()
        if source.is_dir() or source.suffix in {".db", ".sqlite", ".sqlite3"}:
            return self.convert_db(str(source), None, config)
        if is_psychevo_trace_jsonl(source):
            records, warnings, total_events = read_psychevo_trace_messages(source)
            result = self.convert(records, config)
            result.warnings.extend(warnings)
            result.total_events = total_events
            return result
        return self.convert(read_jsonl(path), config)

    def convert_db(
        self,
        path: str,
        session_id: str | None,
        config: ToolConfig,
    ):
        records, warnings = read_psychevo_db(path, session_id, config)
        result = self.convert(records, config)
        result.warnings.extend(warnings)
        return result

    def list_sessions(self, path: str) -> list[SessionInfo]:
        return list_psychevo_sessions(resolve_psychevo_db(path))


def read_psychevo_db(
    path: str,
    session_id: str | None,
    config: ToolConfig,
) -> tuple[list[MessageRecord], list[str]]:
    db_path = resolve_psychevo_db(path)
    selected_session_id = select_session_id(db_path, session_id)
    records = read_sqlite_messages(str(db_path), selected_session_id, config.db)
    trace_path = db_path.parent / "sessions" / selected_session_id / "events.jsonl"
    if not trace_path.exists():
        return records, []
    events, warnings = read_psychevo_trace_events(trace_path)
    return apply_trace_timing(records, events), warnings


def is_psychevo_trace_jsonl(path: Path) -> bool:
    try:
        with path.open("r", encoding="utf-8") as handle:
            for raw_line in handle:
                line = raw_line.strip()
                if not line:
                    continue
                value = json.loads(line)
                schema_version = value.get("schema_version") if isinstance(value, dict) else None
                return (
                    isinstance(value, dict)
                    and schema_version in {1, 2}
                    and isinstance(value.get("kind"), str)
                    and isinstance(value.get("payload"), dict)
                )
    except (OSError, json.JSONDecodeError):
        return False
    return False


def read_psychevo_trace_messages(
    path: Path,
) -> tuple[list[MessageRecord], list[str], int]:
    events, warnings = read_psychevo_trace_events(path)
    records: list[MessageRecord] = []
    schema_versions = {
        event.get("schema_version")
        for event in events
        if isinstance(event.get("schema_version"), int)
    }
    for event in events:
        if event.get("kind") != "message_end":
            continue
        payload = as_dict(event.get("payload"))
        message = as_dict(payload.get("message"))
        if not message:
            continue
        metadata = as_dict(payload.get("metadata")) or {}
        if event.get("session_id") is not None:
            metadata = {**metadata, "session_id": str(event["session_id"])}
        records.append(
            MessageRecord(
                message=message,
                usage=as_dict(payload.get("usage")),
                metadata=metadata or None,
                accounting=as_dict(payload.get("accounting")),
                session_seq=int(event.get("seq") or 0) or None,
                source_session_id=str(event["session_id"])
                if event.get("session_id") is not None
                else None,
            )
        )
    if 2 in schema_versions and not records:
        warnings.append(
            "Psychevo compact trace v2 does not contain transcript messages; "
            "convert from state.db to use the SQLite transcript with trace timing."
        )
    return apply_trace_timing(records, events), warnings, len(events)


def read_psychevo_trace_events(path: Path) -> tuple[list[dict[str, Any]], list[str]]:
    warnings: list[str] = []
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as exc:
        return [], [f"failed to read Psychevo trace sidecar: {exc}"]
    events: list[dict[str, Any]] = []
    last_index = len(lines) - 1
    for index, raw_line in enumerate(lines):
        line = raw_line.strip()
        if not line:
            continue
        try:
            value = json.loads(line)
        except json.JSONDecodeError as exc:
            if index == last_index:
                warnings.append(f"ignored malformed final Psychevo trace line: {exc.msg}")
            else:
                warnings.append(
                    f"ignored malformed Psychevo trace line {index + 1}: {exc.msg}"
                )
            continue
        if isinstance(value, dict):
            events.append(value)
        else:
            warnings.append(f"ignored non-object Psychevo trace line {index + 1}")
    return events, warnings


def apply_trace_timing(
    records: list[MessageRecord],
    events: list[dict[str, Any]],
) -> list[MessageRecord]:
    if not events:
        return records
    generation_timings, tool_timings = trace_timings(events)
    generation_index = 0
    enriched: list[MessageRecord] = []
    for record in records:
        role = str(record.message.get("role", "")).lower()
        metadata = dict(record.metadata or {})
        if role in {"assistant", "agent"} and generation_index < len(generation_timings):
            apply_timing_metadata(metadata, generation_timings[generation_index])
            generation_index += 1
        if role in {"tool", "tool_result"}:
            call_id = record.message.get("tool_call_id") or record.message.get("id")
            if call_id is not None and str(call_id) in tool_timings:
                apply_timing_metadata(metadata, tool_timings[str(call_id)])
        enriched.append(replace(record, metadata=metadata or None))
    return enriched


def trace_timings(
    events: list[dict[str, Any]],
) -> tuple[list[dict[str, int]], dict[str, dict[str, int]]]:
    generation_starts: list[tuple[str | None, int]] = []
    used_generation_starts: set[int] = set()
    generation_timings: list[dict[str, int]] = []
    tool_timings: dict[str, dict[str, int]] = {}

    def take_generation_start(generation_id: object) -> int | None:
        wanted = str(generation_id) if generation_id is not None else None
        if wanted is not None:
            for index, (candidate_id, started_at_ms) in enumerate(generation_starts):
                if index not in used_generation_starts and candidate_id == wanted:
                    used_generation_starts.add(index)
                    return started_at_ms
        for index, (_candidate_id, started_at_ms) in enumerate(generation_starts):
            if index not in used_generation_starts:
                used_generation_starts.add(index)
                return started_at_ms
        return None

    for event in events:
        kind = event.get("kind")
        payload = as_dict(event.get("payload"))
        correlation = as_dict(event.get("correlation"))
        if kind == "generation_start":
            started_at_ms = event_start_ms(event, payload)
            if started_at_ms is not None:
                generation_id = correlation.get("generation_id")
                generation_starts.append(
                    (
                        str(generation_id) if generation_id is not None else None,
                        started_at_ms,
                    )
                )
        elif kind == "generation_end":
            elapsed_ms = int_or_none(payload.get("elapsed_ms"))
            finished_at_ms = event_timestamp_ms(event)
            started_at_ms = take_generation_start(correlation.get("generation_id"))
            timing = complete_timing(started_at_ms, finished_at_ms, elapsed_ms)
            if timing:
                generation_timings.append(timing)
        elif kind == "tool_execution_start":
            call_id = correlation.get("tool_call_id")
            started_at_ms = event_start_ms(event, payload)
            if call_id is not None and started_at_ms is not None:
                timing = tool_timings.setdefault(str(call_id), {})
                timing["started_at_ms"] = started_at_ms
        elif kind == "tool_execution_end":
            call_id = correlation.get("tool_call_id")
            if call_id is None:
                continue
            timing = tool_timings.setdefault(str(call_id), {})
            finished_at_ms = event_timestamp_ms(event)
            elapsed_ms = int_or_none(payload.get("elapsed_ms"))
            if finished_at_ms is not None:
                timing["finished_at_ms"] = finished_at_ms
            if elapsed_ms is not None:
                timing["elapsed_ms"] = max(0, elapsed_ms)

    for timing in tool_timings.values():
        complete_timing_in_place(timing)
    return generation_timings, {key: value for key, value in tool_timings.items() if value}


def apply_timing_metadata(metadata: dict[str, Any], timing: dict[str, int]) -> None:
    if "started_at_ms" in timing:
        metadata["started_at_ms"] = timing["started_at_ms"]
        metadata["started_at_ms_source"] = "runtime_trace"
    if "finished_at_ms" in timing:
        metadata["finished_at_ms"] = timing["finished_at_ms"]
        metadata["finished_at_ms_source"] = "runtime_trace"
    if "elapsed_ms" in timing:
        metadata["elapsed_ms"] = timing["elapsed_ms"]
        metadata["elapsed_ms_source"] = "runtime_trace"


def complete_timing(
    started_at_ms: int | None,
    finished_at_ms: int | None,
    elapsed_ms: int | None,
) -> dict[str, int]:
    timing: dict[str, int] = {}
    if started_at_ms is not None:
        timing["started_at_ms"] = started_at_ms
    if finished_at_ms is not None:
        timing["finished_at_ms"] = finished_at_ms
    if elapsed_ms is not None:
        timing["elapsed_ms"] = max(0, elapsed_ms)
    complete_timing_in_place(timing)
    return timing


def complete_timing_in_place(timing: dict[str, int]) -> None:
    started_at_ms = timing.get("started_at_ms")
    finished_at_ms = timing.get("finished_at_ms")
    elapsed_ms = timing.get("elapsed_ms")
    if elapsed_ms is None and started_at_ms is not None and finished_at_ms is not None:
        timing["elapsed_ms"] = max(0, finished_at_ms - started_at_ms)
        elapsed_ms = timing["elapsed_ms"]
    if started_at_ms is None and finished_at_ms is not None and elapsed_ms is not None:
        timing["started_at_ms"] = max(0, finished_at_ms - elapsed_ms)
        started_at_ms = timing["started_at_ms"]
    if finished_at_ms is None and started_at_ms is not None and elapsed_ms is not None:
        timing["finished_at_ms"] = started_at_ms + elapsed_ms


def event_start_ms(event: dict[str, Any], payload: dict[str, Any]) -> int | None:
    return int_or_none(payload.get("started_at_ms")) or event_timestamp_ms(event)


def event_timestamp_ms(event: dict[str, Any]) -> int | None:
    return int_or_none(event.get("timestamp_ms"))


def as_dict(value: Any) -> dict[str, Any]:
    return value if isinstance(value, dict) else {}


def int_or_none(value: Any) -> int | None:
    if isinstance(value, bool) or value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def resolve_psychevo_db(path: str) -> Path:
    source = Path(path).expanduser()
    if source.is_dir():
        source = source / "state.db"
    if not source.exists():
        raise ValueError(f"Psychevo DB not found: {source}")
    return source


def select_session_id(path: Path, session_id: str | None) -> str:
    conn = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    try:
        if session_id:
            row = conn.execute(
                """
                SELECT id
                FROM sessions
                WHERE id = ?
                """,
                (session_id,),
            ).fetchone()
            if row is None:
                raise ValueError(f"Psychevo session not found: {session_id}")
            return str(row[0])
        row = conn.execute(
            """
            SELECT id
            FROM sessions
            ORDER BY updated_at_ms DESC, ended_at_ms DESC, started_at_ms DESC, id DESC
            LIMIT 1
            """
        ).fetchone()
    except sqlite3.Error as exc:
        raise ValueError(f"failed to read Psychevo DB: {exc}") from exc
    finally:
        conn.close()
    if row is None:
        raise ValueError("Psychevo DB contains no sessions")
    return str(row[0])


def list_psychevo_sessions(path: Path) -> list[SessionInfo]:
    conn = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    try:
        rows = conn.execute(
            """
            SELECT id, title
            FROM sessions
            ORDER BY updated_at_ms DESC, ended_at_ms DESC, started_at_ms DESC, id DESC
            """
        ).fetchall()
    except sqlite3.Error as exc:
        raise ValueError(f"failed to read Psychevo DB: {exc}") from exc
    finally:
        conn.close()
    return [
        SessionInfo(session_id=str(row[0]), name=str(row[1]) if row[1] else None)
        for row in rows
    ]
