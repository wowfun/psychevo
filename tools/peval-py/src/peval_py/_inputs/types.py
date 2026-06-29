from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from peval_py.sources import MessageRecord

@dataclass(frozen=True)
class AdapterAssignments:
    default_adapter: str
    path_adapters: dict[int, str]
    db_adapters: dict[int, str]
    default_explicit: bool = False


@dataclass(frozen=True)
class LoadedSession:
    records: list[MessageRecord] | None
    input_label: str
    adapter_id: str
    input_path: str | None = None
    db_path: str | None = None
    session_hint: str | None = None
    agent_name: str | None = None
    agent_version: str | None = None
    model: str | None = None
    source_alias: str | None = None
    source_kind: str = "path"
    workspace_source_key: str | None = None
    snapshot_trajectory: dict[str, Any] | None = None
    snapshot_meta: dict[str, Any] | None = None
    snapshot_source_report: dict[str, Any] | None = None


@dataclass(frozen=True)
class LoadedInputs:
    sessions: list[LoadedSession]
    notes: list[str]
