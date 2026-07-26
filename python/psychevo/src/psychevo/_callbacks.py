from __future__ import annotations

from collections.abc import Awaitable, Callable, Mapping, Sequence
from dataclasses import dataclass
from typing import Any

JsonValue = (
    None | bool | int | float | str | list["JsonValue"] | dict[str, "JsonValue"]
)


@dataclass(frozen=True, slots=True)
class ToolCall:
    call_id: str
    tool_name: str
    arguments: JsonValue
    thread_id: str
    turn_id: str


@dataclass(frozen=True, slots=True)
class ToolResult:
    result: JsonValue
    model_content: str | None = None
    is_error: bool = False


ToolHandler = Callable[[ToolCall], Awaitable[ToolResult | JsonValue]]


@dataclass(frozen=True, slots=True)
class Tool:
    name: str
    description: str
    parameters: Mapping[str, Any]
    handler: ToolHandler
    execution_mode: str = "parallel"
    timeout: float = 300.0

    def __post_init__(self) -> None:
        if not self.name or self.name.strip() != self.name:
            raise ValueError("Tool name must be non-empty without surrounding whitespace")
        if not self.description.strip():
            raise ValueError("Tool description must be non-empty")
        if self.execution_mode not in {"parallel", "sequential"}:
            raise ValueError("Tool execution_mode must be parallel or sequential")
        if self.timeout <= 0:
            raise ValueError("Tool timeout must be greater than zero")

    def to_wire(self) -> dict[str, object]:
        return {
            "name": self.name,
            "description": self.description,
            "parameters": dict(self.parameters),
            "executionMode": self.execution_mode,
            "timeoutMs": round(self.timeout * 1000),
        }


@dataclass(frozen=True, slots=True)
class ApprovalRequest:
    call_id: str
    thread_id: str
    turn_id: str
    tool_call_id: str
    tool_name: str
    summary: str
    reason: str
    matched_rule: str | None
    suggested_rule: str | None
    allow_always: bool
    filesystem: JsonValue


@dataclass(frozen=True, slots=True)
class ApprovalDecision:
    outcome: str
    filesystem_directory: str | None = None

    def __post_init__(self) -> None:
        if self.outcome not in {
            "allow_once",
            "allow_turn",
            "allow_session",
            "allow_always",
            "deny",
        }:
            raise ValueError("invalid approval outcome")

    def to_wire(self) -> dict[str, object]:
        return {
            "outcome": self.outcome,
            "filesystemDirectory": self.filesystem_directory,
        }


ApprovalHandler = Callable[[ApprovalRequest], Awaitable[ApprovalDecision]]


@dataclass(frozen=True, slots=True)
class ClarifyRequest:
    interaction_id: str
    thread_id: str
    turn_id: str
    questions: JsonValue


ClarifyHandler = Callable[
    [ClarifyRequest], Awaitable[Sequence[Sequence[str]] | None]
]
