from __future__ import annotations

import json
import sqlite3
from pathlib import Path
from typing import Any

from peval_py.adapters.common import CommonMessageAdapter
from peval_py.config import ToolConfig
from peval_py.sources import MessageRecord, read_jsonl


class OpencodeAdapter(CommonMessageAdapter):
    agent_id = "opencode"
    default_agent_name = "opencode"

    def convert_path(self, path: str, config: ToolConfig):
        source = Path(path)
        if source.is_dir() or source.suffix in {".db", ".sqlite", ".sqlite3"}:
            return self.convert_db(str(source), None, config)
        return self.convert(read_jsonl(path), config)

    def convert_db(
        self,
        path: str,
        session_id: str | None,
        config: ToolConfig,
    ):
        records = read_opencode_db(path, session_id)
        return self.convert(records, config)


def read_opencode_db(path: str, session_id: str | None) -> list[MessageRecord]:
    db_path = resolve_opencode_db(path)
    conn = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row
    try:
        session = select_session(conn, session_id)
        messages = conn.execute(
            """
            SELECT id, time_created, time_updated, data
            FROM message
            WHERE session_id = ?
            ORDER BY time_created, id
            """,
            (session["id"],),
        ).fetchall()
        parts = conn.execute(
            """
            SELECT id, message_id, time_created, time_updated, data
            FROM part
            WHERE session_id = ?
            ORDER BY time_created, id
            """,
            (session["id"],),
        ).fetchall()
    except sqlite3.Error as exc:
        raise ValueError(f"failed to read OpenCode DB: {exc}") from exc
    finally:
        conn.close()

    grouped_parts: dict[str, list[sqlite3.Row]] = {}
    for part in parts:
        grouped_parts.setdefault(str(part["message_id"]), []).append(part)

    records: list[MessageRecord] = []
    seq = 1
    for row in messages:
        message_data = parse_json_object(row["data"], "message.data")
        record, tool_results = record_from_message_row(
            row,
            message_data,
            grouped_parts.get(str(row["id"]), []),
            session,
            seq,
        )
        records.append(record)
        seq += 1
        for tool_result in tool_results:
            records.append(with_session_seq(tool_result, seq))
            seq += 1
    return records


def resolve_opencode_db(path: str) -> Path:
    source = Path(path).expanduser()
    if source.is_dir():
        source = source / "opencode.db"
    if not source.exists():
        raise ValueError(f"OpenCode DB not found: {source}")
    return source


def select_session(conn: sqlite3.Connection, session_id: str | None) -> sqlite3.Row:
    if session_id:
        row = conn.execute(
            """
            SELECT id, title, directory, agent, model, time_created, time_updated
            FROM session
            WHERE id = ?
            """,
            (session_id,),
        ).fetchone()
        if row is None:
            raise ValueError(f"OpenCode session not found: {session_id}")
        return row
    row = conn.execute(
        """
        SELECT id, title, directory, agent, model, time_created, time_updated
        FROM session
        ORDER BY time_updated DESC, id DESC
        LIMIT 1
        """
    ).fetchone()
    if row is None:
        raise ValueError("OpenCode DB contains no sessions")
    return row


def record_from_message_row(
    row: sqlite3.Row,
    message_data: dict[str, Any],
    parts: list[sqlite3.Row],
    session: sqlite3.Row,
    seq: int,
) -> tuple[MessageRecord, list[MessageRecord]]:
    role = str(message_data.get("role") or "assistant").lower()
    content: list[dict[str, Any]] = []
    tool_results: list[MessageRecord] = []
    usage = usage_from_tokens(message_data.get("tokens"))
    cost = message_data.get("cost")

    for part_row in parts:
        part_data = parse_json_object(part_row["data"], "part.data")
        part_type = str(part_data.get("type") or "").lower()
        if part_type == "text":
            text = part_data.get("text")
            if isinstance(text, str) and text:
                content.append({"type": "text", "text": text})
        elif part_type == "reasoning":
            text = part_data.get("text")
            if isinstance(text, str) and text:
                content.append({"type": "reasoning", "text": text})
        elif part_type == "tool":
            tool_call, tool_result = tool_records_from_part(part_row, part_data, session)
            content.append(tool_call)
            if tool_result:
                tool_results.append(tool_result)
        elif part_type == "step-finish":
            usage = usage or usage_from_tokens(part_data.get("tokens"))
            if cost is None:
                cost = part_data.get("cost")

    message: dict[str, Any] = {
        "role": role,
        "content": content,
        "timestamp_ms": int(row["time_created"]),
    }
    model = model_name(message_data, session)
    if model and role in {"assistant", "agent"}:
        message["model"] = model
    if role == "user" and len(content) == 1 and content[0].get("type") == "text":
        message["content"] = content[0]["text"]

    return (
        MessageRecord(
            message=message,
            usage=usage or None,
            metadata=metadata_for(session, row["id"]),
            accounting=accounting_from_cost(cost),
            session_seq=seq,
            source_session_id=str(session["id"]),
        ),
        tool_results,
    )


def tool_records_from_part(
    part_row: sqlite3.Row,
    part_data: dict[str, Any],
    session: sqlite3.Row,
) -> tuple[dict[str, Any], MessageRecord | None]:
    state = part_data.get("state") if isinstance(part_data.get("state"), dict) else {}
    call_id = str(part_data.get("callID") or part_data.get("id") or part_row["id"])
    tool_name = str(part_data.get("tool") or part_data.get("name") or "tool")
    input_value = state.get("input") if isinstance(state, dict) else None
    arguments = input_value if isinstance(input_value, dict) else {"input": input_value}
    tool_call = {
        "type": "tool_call",
        "id": call_id,
        "name": tool_name,
        "arguments": arguments,
    }
    if not isinstance(state, dict) or "output" not in state:
        return tool_call, None
    status = str(state.get("status") or "completed").lower()
    started_at_ms = int(part_row["time_created"])
    finished_at_ms = int(part_row["time_updated"] or part_row["time_created"])
    metadata = metadata_for(session, part_row["message_id"], part_row["id"])
    metadata.update(
        {
            "started_at_ms": started_at_ms,
            "started_at_ms_source": "opencode_part_timestamps",
            "finished_at_ms": finished_at_ms,
            "finished_at_ms_source": "opencode_part_timestamps",
            "elapsed_ms": max(0, finished_at_ms - started_at_ms),
            "elapsed_ms_source": "opencode_part_timestamps",
        }
    )
    return tool_call, MessageRecord(
        message={
            "role": "tool_result",
            "tool_call_id": call_id,
            "tool_name": tool_name,
            "content": state.get("output"),
            "is_error": status not in {"completed", "success"},
            "timestamp_ms": finished_at_ms,
        },
        metadata=metadata,
        source_session_id=str(session["id"]),
    )


def with_session_seq(record: MessageRecord, seq: int) -> MessageRecord:
    return MessageRecord(
        message=record.message,
        usage=record.usage,
        metadata=record.metadata,
        accounting=record.accounting,
        session_seq=seq,
        source_session_id=record.source_session_id,
    )


def parse_json_object(raw: object, label: str) -> dict[str, Any]:
    try:
        value = json.loads(str(raw))
    except json.JSONDecodeError as exc:
        raise ValueError(f"failed to parse OpenCode {label}: {exc.msg}") from exc
    if not isinstance(value, dict):
        raise ValueError(f"OpenCode {label} is not an object")
    return value


def metadata_for(
    session: sqlite3.Row,
    message_id: object,
    part_id: object | None = None,
) -> dict[str, Any]:
    metadata = {
        "session_id": str(session["id"]),
        "message_id": str(message_id),
        "session_title": session["title"],
        "session_directory": session["directory"],
        "source": "opencode-db",
    }
    if part_id is not None:
        metadata["part_id"] = str(part_id)
    return metadata


def usage_from_tokens(raw: Any) -> dict[str, Any]:
    if not isinstance(raw, dict):
        return {}
    usage: dict[str, Any] = {}
    token_map = [
        ("input", "input_tokens"),
        ("output", "output_tokens"),
        ("reasoning", "reasoning_tokens"),
        ("total", "total_tokens"),
    ]
    for source, target in token_map:
        if raw.get(source) is not None:
            usage[target] = raw[source]
    cache = raw.get("cache")
    if isinstance(cache, dict):
        if cache.get("read") is not None:
            usage["cache_read_tokens"] = cache["read"]
        if cache.get("write") is not None:
            usage["cache_write_tokens"] = cache["write"]
    return usage


def accounting_from_cost(raw: Any) -> dict[str, Any] | None:
    if raw is None:
        return None
    return {"estimated_cost_nanodollars": int(float(raw) * 1_000_000_000)}


def model_name(message_data: dict[str, Any], session: sqlite3.Row) -> str | None:
    direct = message_data.get("modelID") or message_data.get("model")
    if isinstance(direct, str) and direct:
        return direct
    session_model = session["model"]
    if not session_model:
        return None
    try:
        value = json.loads(str(session_model))
    except json.JSONDecodeError:
        return str(session_model)
    if isinstance(value, dict) and value.get("id"):
        return str(value["id"])
    return None
