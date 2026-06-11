from __future__ import annotations

import json
import re
import sqlite3
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any

from peval_py.adapters.base import TIMESTAMP_SEMANTICS_ORDER_ONLY, SessionInfo
from peval_py.adapters.common import CommonMessageAdapter
from peval_py.config import ToolConfig
from peval_py.sources import MessageRecord, read_jsonl

HERMES_AGENT_LOG_TIMING_SOURCE = "hermes_agent_log"


class HermesAdapter(CommonMessageAdapter):
    agent_id = "hermes"
    default_agent_name = "hermes"

    def convert_path(self, path: str, config: ToolConfig):
        source = Path(path).expanduser()
        if source.is_dir() or source.suffix in {".db", ".sqlite", ".sqlite3"}:
            return self.convert_db(str(source), None, config)
        return self.convert(read_jsonl(path), config)

    def convert_db(
        self,
        path: str,
        session_id: str | None,
        config: ToolConfig,
    ):
        records = read_hermes_db(path, session_id)
        return self.convert(records, config)

    def list_sessions(self, path: str) -> list[SessionInfo]:
        return list_hermes_sessions(resolve_hermes_db(path))


def read_hermes_db(path: str, session_id: str | None) -> list[MessageRecord]:
    db_path = resolve_hermes_db(path)
    conn = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row
    try:
        session = select_session(conn, session_id)
        messages = conn.execute(
            """
            SELECT *
            FROM messages
            WHERE session_id = ? AND active = 1
            ORDER BY id ASC
            """,
            (session["id"],),
        ).fetchall()
    except sqlite3.Error as exc:
        raise ValueError(f"failed to read Hermes DB: {exc}") from exc
    finally:
        conn.close()

    aggregate_usage = session_usage(session)
    aggregate_accounting = session_accounting(session)
    has_aggregate_metrics = bool(aggregate_usage or aggregate_accounting)
    pending_usage = aggregate_usage
    pending_accounting = aggregate_accounting
    records: list[MessageRecord] = []
    seq = 1
    system_prompt = row_value(session, "system_prompt")
    if isinstance(system_prompt, str) and system_prompt:
        records.append(
            MessageRecord(
                message={
                    "role": "system",
                    "content": system_prompt,
                    "timestamp_ms": seconds_to_ms(row_value(session, "started_at")),
                    "model": row_string(session, "model"),
                },
                usage=pending_usage or None,
                metadata=metadata_for(session, source="hermes-db"),
                accounting=pending_accounting or None,
                session_seq=seq,
                source_session_id=str(session["id"]),
            )
        )
        pending_usage = {}
        pending_accounting = {}
        seq += 1

    for row in messages:
        record = record_from_message_row(
            row,
            session,
            seq,
            include_row_usage=not has_aggregate_metrics,
        )
        if pending_usage or pending_accounting:
            record = with_metrics(record, pending_usage, pending_accounting)
            pending_usage = {}
            pending_accounting = {}
        records.append(record)
        seq += 1
    return records_with_hermes_log_timing(records, db_path, str(session["id"]))


@dataclass(frozen=True)
class HermesLogEvent:
    name: str | None
    started_at_ms: int
    finished_at_ms: int
    elapsed_ms: int
    failed: bool = False


@dataclass(frozen=True)
class HermesLogTiming:
    model_events: list[HermesLogEvent]
    tool_events: list[HermesLogEvent]


def records_with_hermes_log_timing(
    records: list[MessageRecord],
    db_path: Path,
    session_id: str,
) -> list[MessageRecord]:
    timing = read_hermes_agent_log_timing(hermes_agent_log_path(db_path), session_id)
    if not timing.model_events and not timing.tool_events:
        return records

    enriched = list(records)
    assistant_indexes = [
        index
        for index, record in enumerate(records)
        if message_role(record) in {"assistant", "agent"}
    ]
    if len(timing.model_events) == len(assistant_indexes):
        for index, event in zip(assistant_indexes, timing.model_events, strict=True):
            enriched[index] = record_with_hermes_timing(enriched[index], event)

    tool_indexes = [
        index
        for index, record in enumerate(records)
        if message_role(record) in {"tool", "tool_result"}
    ]
    if len(timing.tool_events) == len(tool_indexes) and all(
        hermes_tool_event_matches_record(event, records[index])
        for index, event in zip(tool_indexes, timing.tool_events, strict=True)
    ):
        for index, event in zip(tool_indexes, timing.tool_events, strict=True):
            enriched[index] = record_with_hermes_timing(enriched[index], event)

    return enriched


def hermes_agent_log_path(db_path: Path) -> Path:
    return db_path.parent / "logs" / "agent.log"


def read_hermes_agent_log_timing(log_path: Path, session_id: str) -> HermesLogTiming:
    model_events: list[HermesLogEvent] = []
    tool_events: list[HermesLogEvent] = []
    if not log_path.exists():
        return HermesLogTiming(model_events, tool_events)

    session = re.escape(session_id)
    api_pattern = re.compile(
        rf"^(?P<ts>\d{{4}}-\d\d-\d\d \d\d:\d\d:\d\d),(?P<ms>\d{{3}})"
        rf".*\[{session}\] agent\.conversation_loop: API call #\d+: "
        rf".* latency=(?P<duration>\d+(?:\.\d+)?)s"
    )
    tool_ok_pattern = re.compile(
        rf"^(?P<ts>\d{{4}}-\d\d-\d\d \d\d:\d\d:\d\d),(?P<ms>\d{{3}})"
        rf".*\[{session}\] agent\.tool_executor: tool (?P<name>\S+) "
        rf"completed \((?P<duration>\d+(?:\.\d+)?)s,"
    )
    tool_error_pattern = re.compile(
        rf"^(?P<ts>\d{{4}}-\d\d-\d\d \d\d:\d\d:\d\d),(?P<ms>\d{{3}})"
        rf".*\[{session}\] agent\.tool_executor: Tool (?P<name>\S+) "
        rf"returned error \((?P<duration>\d+(?:\.\d+)?)s\):"
    )

    try:
        lines = log_path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return HermesLogTiming(model_events, tool_events)

    for line in lines:
        if session_id not in line:
            continue
        if match := api_pattern.match(line):
            model_events.append(hermes_log_event_from_match(match, None))
            continue
        if match := tool_ok_pattern.match(line):
            tool_events.append(hermes_log_event_from_match(match, match.group("name")))
            continue
        if match := tool_error_pattern.match(line):
            tool_events.append(
                hermes_log_event_from_match(match, match.group("name"), failed=True)
            )
    return HermesLogTiming(model_events, tool_events)


def hermes_log_event_from_match(
    match: re.Match[str],
    name: str | None,
    failed: bool = False,
) -> HermesLogEvent:
    finished_at_ms = hermes_log_timestamp_ms(match.group("ts"), match.group("ms"))
    elapsed_ms = int(round(float(match.group("duration")) * 1000))
    return HermesLogEvent(
        name=name,
        started_at_ms=finished_at_ms - elapsed_ms,
        finished_at_ms=finished_at_ms,
        elapsed_ms=elapsed_ms,
        failed=failed,
    )


def hermes_log_timestamp_ms(timestamp: str, milliseconds: str) -> int:
    value = datetime.strptime(f"{timestamp}.{milliseconds}", "%Y-%m-%d %H:%M:%S.%f")
    return int(round(value.timestamp() * 1000))


def hermes_tool_event_matches_record(event: HermesLogEvent, record: MessageRecord) -> bool:
    event_name = normalized_tool_name(event.name)
    record_name = normalized_tool_name(record.message.get("tool_name"))
    return bool(event_name and record_name and event_name == record_name)


def normalized_tool_name(value: object) -> str | None:
    return str(value).lower() if isinstance(value, str) and value else None


def record_with_hermes_timing(
    record: MessageRecord,
    event: HermesLogEvent,
) -> MessageRecord:
    metadata = dict(record.metadata or {})
    metadata.update(
        {
            "started_at_ms": event.started_at_ms,
            "finished_at_ms": event.finished_at_ms,
            "elapsed_ms": event.elapsed_ms,
            "elapsed_ms_source": HERMES_AGENT_LOG_TIMING_SOURCE,
        }
    )
    message = dict(record.message)
    if event.failed:
        message["error"] = message.get("error") or True
    return MessageRecord(
        message=message,
        usage=record.usage,
        metadata=metadata,
        accounting=record.accounting,
        session_seq=record.session_seq,
        source_session_id=record.source_session_id,
    )


def message_role(record: MessageRecord) -> str:
    return str(record.message.get("role", "")).lower()


def resolve_hermes_db(path: str) -> Path:
    source = Path(path).expanduser()
    if source.is_dir():
        source = source / "state.db"
    if not source.exists():
        raise ValueError(f"Hermes DB not found: {source}")
    return source


def select_session(conn: sqlite3.Connection, session_id: str | None) -> sqlite3.Row:
    if session_id:
        row = conn.execute(
            """
            SELECT *
            FROM sessions
            WHERE id = ?
            """,
            (session_id,),
        ).fetchone()
        if row is None:
            raise ValueError(f"Hermes session not found: {session_id}")
        return row
    row = conn.execute(
        """
        SELECT s.*
        FROM sessions s
        LEFT JOIN (
            SELECT session_id, MAX(timestamp) AS last_active
            FROM messages
            WHERE active = 1
            GROUP BY session_id
        ) latest ON latest.session_id = s.id
        ORDER BY COALESCE(latest.last_active, s.ended_at, s.started_at) DESC,
                 s.started_at DESC,
                 s.id DESC
        LIMIT 1
        """
    ).fetchone()
    if row is None:
        raise ValueError("Hermes DB contains no sessions")
    return row


def list_hermes_sessions(path: Path) -> list[SessionInfo]:
    conn = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    try:
        rows = conn.execute(
            """
            SELECT s.id, s.title
            FROM sessions s
            LEFT JOIN (
                SELECT session_id, MAX(timestamp) AS last_active
                FROM messages
                WHERE active = 1
                GROUP BY session_id
            ) latest ON latest.session_id = s.id
            ORDER BY COALESCE(latest.last_active, s.ended_at, s.started_at) DESC,
                     s.started_at DESC,
                     s.id DESC
            """
        ).fetchall()
    except sqlite3.Error as exc:
        raise ValueError(f"failed to read Hermes DB: {exc}") from exc
    finally:
        conn.close()
    return [
        SessionInfo(session_id=str(row[0]), name=str(row[1]) if row[1] else None)
        for row in rows
    ]


def record_from_message_row(
    row: sqlite3.Row,
    session: sqlite3.Row,
    seq: int,
    include_row_usage: bool,
) -> MessageRecord:
    role = row_string(row, "role") or "assistant"
    content = row_value(row, "content")
    message: dict[str, Any] = {
        "role": role,
        "content": content if content is not None else "",
        "timestamp_ms": seconds_to_ms(row_value(row, "timestamp")),
    }
    model = row_string(session, "model")
    if model and role.lower() in {"assistant", "agent"}:
        message["model"] = model
    reasoning = first_non_empty_string(
        row_value(row, "reasoning_content"),
        row_value(row, "reasoning"),
    )
    if reasoning:
        message["reasoning_content"] = reasoning
    tool_calls = tool_calls_from_raw(row_value(row, "tool_calls"))
    if tool_calls:
        if isinstance(content, str) and content:
            message["content"] = [{"type": "text", "text": content}, *tool_calls]
        else:
            message["content"] = tool_calls
    tool_call_id = row_string(row, "tool_call_id")
    if tool_call_id:
        message["tool_call_id"] = tool_call_id
    tool_name = row_string(row, "tool_name")
    if tool_name:
        message["tool_name"] = tool_name
    usage = row_usage(row) if include_row_usage else {}
    return MessageRecord(
        message=message,
        usage=usage or None,
        metadata=metadata_for(
            session,
            message_id=row_value(row, "id"),
            platform_message_id=row_value(row, "platform_message_id"),
            source="hermes-db",
        ),
        session_seq=seq,
        source_session_id=str(session["id"]),
    )


def tool_calls_from_raw(raw: object) -> list[dict[str, Any]]:
    if raw is None or raw == "":
        return []
    value = parse_json_value(raw, "tool_calls")
    if isinstance(value, dict):
        value = [value]
    if not isinstance(value, list):
        raise ValueError("Hermes tool_calls is not a JSON array or object")
    calls: list[dict[str, Any]] = []
    for index, item in enumerate(value, start=1):
        if not isinstance(item, dict):
            continue
        function = item.get("function")
        if not isinstance(function, dict):
            function = {}
        call_id = first_non_empty_string(
            item.get("id"),
            item.get("call_id"),
            item.get("tool_call_id"),
        ) or f"tool-call-{index}"
        name = first_non_empty_string(
            item.get("name"),
            item.get("function_name"),
            item.get("tool"),
            function.get("name"),
        ) or "tool"
        calls.append(
            {
                "type": "tool_call",
                "id": call_id,
                "name": name,
                "arguments": normalize_arguments(
                    item.get("arguments")
                    if item.get("arguments") is not None
                    else function.get("arguments")
                ),
            }
        )
    return calls


def normalize_arguments(value: object) -> dict[str, Any]:
    if value is None:
        return {}
    if isinstance(value, dict):
        return value
    if isinstance(value, str):
        stripped = value.strip()
        if not stripped:
            return {}
        try:
            parsed = json.loads(stripped)
        except json.JSONDecodeError:
            return {"input": value}
        return parsed if isinstance(parsed, dict) else {"input": parsed}
    return {"input": value}


def parse_json_value(raw: object, label: str) -> Any:
    try:
        return json.loads(str(raw))
    except json.JSONDecodeError as exc:
        raise ValueError(f"failed to parse Hermes {label}: {exc.msg}") from exc


def session_usage(session: sqlite3.Row) -> dict[str, Any]:
    usage: dict[str, Any] = {}
    for source, target in [
        ("input_tokens", "input_tokens"),
        ("output_tokens", "output_tokens"),
        ("cache_read_tokens", "cache_read_tokens"),
        ("cache_write_tokens", "cache_write_tokens"),
        ("reasoning_tokens", "reasoning_tokens"),
    ]:
        value = int_or_none(row_value(session, source))
        if value is not None:
            usage[target] = value
    cost = float_or_none(row_value(session, "actual_cost_usd"))
    if cost is None:
        cost = float_or_none(row_value(session, "estimated_cost_usd"))
    if cost is not None:
        usage["cost_usd"] = cost
    return usage


def session_accounting(session: sqlite3.Row) -> dict[str, Any]:
    accounting: dict[str, Any] = {}
    for source, target in [
        ("input_tokens", "billable_input_tokens"),
        ("output_tokens", "billable_output_tokens"),
        ("cache_read_tokens", "cache_read_tokens"),
        ("cache_write_tokens", "cache_write_tokens"),
        ("reasoning_tokens", "reasoning_tokens"),
    ]:
        value = int_or_none(row_value(session, source))
        if value is not None:
            accounting[target] = value
    cost = float_or_none(row_value(session, "actual_cost_usd"))
    if cost is None:
        cost = float_or_none(row_value(session, "estimated_cost_usd"))
    if cost is not None:
        accounting["estimated_cost_nanodollars"] = int(cost * 1_000_000_000)
    pricing_source = first_non_empty_string(
        row_value(session, "pricing_version"),
        row_value(session, "cost_source"),
        row_value(session, "billing_provider"),
    )
    if pricing_source:
        accounting["pricing_source"] = pricing_source
    return accounting


def row_usage(row: sqlite3.Row) -> dict[str, Any]:
    value = int_or_none(row_value(row, "token_count"))
    if value is None:
        return {}
    return {"total_tokens": value}


def with_metrics(
    record: MessageRecord,
    usage: dict[str, Any],
    accounting: dict[str, Any],
) -> MessageRecord:
    return MessageRecord(
        message=record.message,
        usage=usage or record.usage,
        metadata=record.metadata,
        accounting=accounting or record.accounting,
        session_seq=record.session_seq,
        source_session_id=record.source_session_id,
    )


def metadata_for(
    session: sqlite3.Row,
    message_id: object | None = None,
    platform_message_id: object | None = None,
    source: str = "hermes-db",
) -> dict[str, Any]:
    metadata: dict[str, Any] = {
        "session_id": str(session["id"]),
        "source": source,
        "timestamp_semantics": TIMESTAMP_SEMANTICS_ORDER_ONLY,
    }
    for column, key in [
        ("source", "session_source"),
        ("title", "session_title"),
        ("cwd", "session_cwd"),
        ("model", "model_name"),
    ]:
        value = row_value(session, column)
        if value is not None:
            metadata[key] = value
    if message_id is not None:
        metadata["message_id"] = message_id
    if platform_message_id is not None:
        metadata["platform_message_id"] = platform_message_id
    return metadata


def seconds_to_ms(value: object) -> int:
    number = float_or_none(value)
    if number is None:
        return 0
    return int(number * 1000)


def row_value(row: sqlite3.Row, key: str) -> Any:
    return row[key] if key in row.keys() else None


def row_string(row: sqlite3.Row, key: str) -> str | None:
    value = row_value(row, key)
    return value if isinstance(value, str) and value else None


def first_non_empty_string(*values: object) -> str | None:
    for value in values:
        if isinstance(value, str) and value:
            return value
    return None


def int_or_none(value: object) -> int | None:
    if isinstance(value, bool) or value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def float_or_none(value: object) -> float | None:
    if isinstance(value, bool) or value is None:
        return None
    try:
        return float(value)
    except (TypeError, ValueError):
        return None
