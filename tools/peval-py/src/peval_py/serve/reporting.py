from __future__ import annotations

from typing import Any
from urllib.parse import parse_qs

from peval_py.config import ToolConfig
from peval_py.state import ServeStateStore


def mutation_payload(
    store: ServeStateStore,
    config: ToolConfig,
    *,
    source_key: str | None = None,
    source_state: str = "active",
) -> dict[str, Any]:
    sources = store.source_payload()
    payload: dict[str, Any] = {"sources": sources}
    report_key = readable_source_key(
        sources,
        source_key,
        source_state=source_state,
    ) or first_readable_source_key(sources, source_state=source_state)
    payload["report"] = store.active_report(config, source_state=source_state)
    payload["report_source_key"] = report_key
    payload["report_source_state"] = source_state
    return payload


def readable_source_key(
    sources: list[dict[str, Any]],
    source_key: str | None,
    *,
    source_state: str = "active",
) -> str | None:
    if not source_key:
        return None
    for source in sources:
        candidate = str(source.get("source_key") or "")
        if candidate == source_key and source_is_readable(source, source_state):
            return candidate
    return None


def source_is_readable(source: dict[str, Any], source_state: str = "active") -> bool:
    active = source.get("active") is not False
    if source_state == "archived":
        if active:
            return False
    elif not active:
        return False
    return (
        bool(source.get("source_key"))
        and bool(source.get("artifact_dir"))
        and source.get("last_status") != "missing"
    )


def first_readable_source_key(
    sources: list[dict[str, Any]],
    *,
    exclude: set[str] | None = None,
    source_state: str = "active",
) -> str | None:
    excluded = exclude or set()
    for source in sources:
        source_key = str(source.get("source_key") or "")
        if not source_key or source_key in excluded:
            continue
        if not source_is_readable(source, source_state):
            continue
        return source_key
    return None


def single_query_value(query: str, key: str) -> str | None:
    values = parse_qs(query).get(key) or []
    for value in values:
        text = str(value).strip()
        if text:
            return text
    return None
