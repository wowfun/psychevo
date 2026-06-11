from __future__ import annotations

from peval_py.adapters import adapter_for
from peval_py.adapters.base import SessionInfo


def list_adapter_sessions(adapter_id: str, path: str) -> list[SessionInfo]:
    adapter = adapter_for(adapter_id)
    list_sessions = getattr(adapter, "list_sessions", None)
    if not callable(list_sessions):
        raise ValueError(f"adapter {adapter_id} does not support session listing")
    return list(list_sessions(path))


def resolve_session_selectors(
    adapter_id: str,
    path: str,
    selectors: list[str],
) -> list[str]:
    return [
        resolve_session_selector(adapter_id, path, selector)
        for selector in selectors
    ]


def resolve_session_selector(adapter_id: str, path: str, selector: str) -> str:
    text = str(selector)
    if text.startswith("#"):
        return session_by_index(adapter_id, path, text[1:]).session_id
    if text.isdigit():
        sessions = list_adapter_sessions(adapter_id, path)
        for session in sessions:
            if session.session_id == text:
                return text
        return session_by_index(adapter_id, path, text, sessions=sessions).session_id
    return text


def session_by_index(
    adapter_id: str,
    path: str,
    raw_index: str,
    *,
    sessions: list[SessionInfo] | None = None,
) -> SessionInfo:
    if not raw_index.isdigit():
        raise ValueError(f"session index must be a positive integer: #{raw_index}")
    index = int(raw_index)
    if index < 1:
        raise ValueError(f"session index out of range: #{index}")
    available = sessions if sessions is not None else list_adapter_sessions(adapter_id, path)
    if index > len(available):
        raise ValueError(
            f"session index out of range: #{index} "
            f"(available sessions: {len(available)})"
        )
    return available[index - 1]


def format_session_table(sessions: list[SessionInfo]) -> str:
    headers = ["#", "session_id", "name"]
    rows = [
        [str(index), session.session_id, session.name or "-"]
        for index, session in enumerate(sessions, start=1)
    ]
    widths = [
        max(len(row[column]) for row in [headers, *rows])
        for column in range(len(headers))
    ]
    lines = [format_table_row(headers, widths)]
    lines.extend(format_table_row(row, widths) for row in rows)
    return "\n".join(lines) + "\n"


def format_table_row(row: list[str], widths: list[int]) -> str:
    return "  ".join(value.ljust(widths[index]) for index, value in enumerate(row))


def parse_session_selection(text: str, session_count: int) -> list[int]:
    stripped = text.strip().lower()
    if not stripped:
        return []
    if stripped == "all":
        return list(range(1, session_count + 1))
    selected: list[int] = []
    seen: set[int] = set()
    for raw_part in stripped.split(","):
        part = raw_part.strip()
        if not part:
            continue
        if "-" in part:
            raw_start, raw_end = part.split("-", 1)
            start = parse_selection_index(raw_start, session_count)
            end = parse_selection_index(raw_end, session_count)
            if end < start:
                raise ValueError(f"invalid descending session range: {part}")
            indexes = range(start, end + 1)
        else:
            indexes = [parse_selection_index(part, session_count)]
        for index in indexes:
            if index not in seen:
                selected.append(index)
                seen.add(index)
    return selected


def parse_selection_index(text: str, session_count: int) -> int:
    if not text.isdigit():
        raise ValueError(f"session selection must use indexes, ranges, or all: {text}")
    index = int(text)
    if index < 1 or index > session_count:
        raise ValueError(
            f"session index out of range: {index} "
            f"(available sessions: {session_count})"
        )
    return index
