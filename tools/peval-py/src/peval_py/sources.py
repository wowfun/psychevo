from __future__ import annotations

import json
import sqlite3
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .config import DbMapping

ACCOUNTING_COLUMNS = [
    "context_input_tokens",
    "billable_input_tokens",
    "billable_output_tokens",
    "reasoning_tokens",
    "cache_read_tokens",
    "cache_write_tokens",
    "reported_total_tokens",
    "estimated_cost_nanodollars",
    "pricing_source",
    "pricing_tier",
]


@dataclass(frozen=True)
class MessageRecord:
    message: dict[str, Any]
    usage: dict[str, Any] | None = None
    metadata: dict[str, Any] | None = None
    accounting: dict[str, Any] | None = None
    session_seq: int | None = None
    source_session_id: str | None = None


def read_jsonl(path: str) -> list[MessageRecord]:
    records: list[MessageRecord] = []
    source = Path(path)
    with source.open("r", encoding="utf-8") as handle:
        for line_number, raw_line in enumerate(handle, start=1):
            line = raw_line.strip()
            if not line:
                continue
            try:
                value = json.loads(line)
            except json.JSONDecodeError as exc:
                raise ValueError(
                    f"failed to parse JSONL line {line_number}: {exc.msg}"
                ) from exc
            if not isinstance(value, dict):
                raise ValueError(f"JSONL line {line_number} is not an object")
            records.append(_record_from_json_object(value))
    return sorted(records, key=lambda record: record.session_seq or 0)


def read_sqlite_messages(path: str, session_id: str, mapping: DbMapping) -> list[MessageRecord]:
    selected = [
        mapping.sequence_column,
        mapping.message_column,
        mapping.usage_column,
        mapping.metadata_column,
        *ACCOUNTING_COLUMNS,
    ]
    sql = (
        f"SELECT {', '.join(selected)} FROM {mapping.messages_table} "
        f"WHERE {mapping.session_id_column} = ? "
        f"ORDER BY {mapping.sequence_column} ASC"
    )
    conn = sqlite3.connect(f"file:{Path(path)}?mode=ro", uri=True)
    try:
        rows = conn.execute(sql, (session_id,)).fetchall()
    except sqlite3.OperationalError as exc:
        raise ValueError(f"failed to read SQLite messages: {exc}") from exc
    finally:
        conn.close()
    records: list[MessageRecord] = []
    for row in rows:
        seq = int(row[0])
        message = _parse_json_column(row[1], "message_json")
        usage = _parse_optional_json_column(row[2], "usage_json")
        metadata = _parse_optional_json_column(row[3], "metadata_json")
        accounting = {
            name: value
            for name, value in zip(ACCOUNTING_COLUMNS, row[4:], strict=True)
            if value is not None
        }
        records.append(
            MessageRecord(
                message=message,
                usage=usage,
                metadata=metadata,
                accounting=accounting or None,
                session_seq=seq,
                source_session_id=session_id,
            )
        )
    return records


def _record_from_json_object(value: dict[str, Any]) -> MessageRecord:
    message = value.get("message")
    if isinstance(message, dict):
        metadata = _dict_or_none(value.get("metadata"))
        return MessageRecord(
            message=message,
            usage=_dict_or_none(value.get("usage")),
            metadata=metadata,
            accounting=_dict_or_none(value.get("accounting")),
            session_seq=_int_or_none(value.get("session_seq")),
            source_session_id=_session_id_from_sources(value, metadata, message),
        )
    metadata = _dict_or_none(value.get("metadata"))
    return MessageRecord(
        message=value,
        usage=_dict_or_none(value.get("usage")),
        metadata=metadata,
        accounting=_dict_or_none(value.get("accounting")),
        session_seq=_int_or_none(value.get("session_seq")),
        source_session_id=_session_id_from_sources(value, metadata),
    )


def _parse_json_column(value: str, label: str) -> dict[str, Any]:
    parsed = json.loads(value)
    if not isinstance(parsed, dict):
        raise ValueError(f"{label} is not a JSON object")
    return parsed


def _parse_optional_json_column(value: str | None, label: str) -> dict[str, Any] | None:
    if value is None:
        return None
    return _parse_json_column(value, label)


def _dict_or_none(value: Any) -> dict[str, Any] | None:
    return value if isinstance(value, dict) else None


def _int_or_none(value: Any) -> int | None:
    if value is None:
        return None
    return int(value)


def _session_id_from_sources(*sources: dict[str, Any] | None) -> str | None:
    for source in sources:
        if isinstance(source, dict) and source.get("session_id") is not None:
            return str(source["session_id"])
    return None
