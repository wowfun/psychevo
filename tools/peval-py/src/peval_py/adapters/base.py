from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Protocol

from peval_py.config import ToolConfig
from peval_py.sources import MessageRecord


ACTIVE_DURATION_FALLBACK_CAP_MS = 600_000


def timestamp_fallback_duration_ms(
    start_ms: int | None,
    end_ms: int | None,
) -> int | None:
    if start_ms is None or end_ms is None:
        return None
    duration = end_ms - start_ms
    if 0 <= duration <= ACTIVE_DURATION_FALLBACK_CAP_MS:
        return duration
    return None


@dataclass
class ToolMeta:
    tool_call_id: str
    status: str | None = None
    title: str | None = None
    timestamp_ms: int | None = None
    generation_duration_ms: int | None = None
    execution_duration_ms: int | None = None
    execution_duration_source: str | None = None
    truncated: bool = False


@dataclass
class ObservationMeta:
    source_call_id: str | None = None
    status: str | None = None
    title: str | None = None
    timestamp_ms: int | None = None
    tool_error: bool = False
    truncated: bool = False


@dataclass
class StepMeta:
    step_id: int
    source: str | None = None
    tool_calls: list[ToolMeta] = field(default_factory=list)
    observations: list[ObservationMeta] = field(default_factory=list)
    tool_error: bool = False
    timestamp_ms: int | None = None
    elapsed_ms: int | None = None
    duration_ms: int | None = None
    duration_source: str | None = None
    data_preview: str | None = None
    truncated: bool = False


@dataclass
class ConversionResult:
    trajectory: dict[str, Any]
    steps_meta: list[StepMeta]
    warnings: list[str]
    total_events: int
    unmapped_events: int
    started_at_ms: int | None
    finished_at_ms: int | None


class Adapter(Protocol):
    agent_id: str


class RecordAdapter(Adapter, Protocol):
    def convert(self, records: list[MessageRecord], config: ToolConfig) -> ConversionResult:
        ...


class PathAdapter(Adapter, Protocol):
    def convert_path(self, path: str, config: ToolConfig) -> ConversionResult:
        ...


class DbAdapter(Adapter, Protocol):
    def convert_db(
        self,
        path: str,
        session_id: str | None,
        config: ToolConfig,
    ) -> ConversionResult:
        ...
