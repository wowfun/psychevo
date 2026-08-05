from __future__ import annotations

from collections.abc import Awaitable, Callable, Mapping, Sequence
from dataclasses import dataclass
from typing import Any

from ._wire import wire_list, wire_object, wire_string, wire_string_tuple

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
class FilesystemApprovalTarget:
    requested_path: str
    resolved_path: str

    @classmethod
    def from_wire(cls, value: object) -> "FilesystemApprovalTarget":
        value = wire_object(value, "filesystem approval target")
        return cls(
            requested_path=wire_string(
                value, "requestedPath", "filesystem approval target"
            ),
            resolved_path=wire_string(
                value, "resolvedPath", "filesystem approval target"
            ),
        )


@dataclass(frozen=True, slots=True)
class FilesystemApprovalRequest:
    targets: tuple[FilesystemApprovalTarget, ...]
    scope_candidates: tuple[str, ...]

    @classmethod
    def from_wire(cls, value: object) -> "FilesystemApprovalRequest":
        value = wire_object(value, "filesystem approval detail")
        targets = wire_list(value.get("targets"), "filesystem approval targets")
        scope_candidates = wire_string_tuple(
            value.get("scopeCandidates"),
            "filesystem approval scopeCandidates",
        )
        return cls(
            targets=tuple(FilesystemApprovalTarget.from_wire(item) for item in targets),
            scope_candidates=scope_candidates,
        )


@dataclass(frozen=True, slots=True)
class McpStdioStartupTarget:
    command: str
    args: tuple[str, ...]
    cwd: str
    env_names: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class McpHttpStartupTarget:
    url: str
    header_names: tuple[str, ...]
    credential_names: tuple[str, ...]


McpStartupTarget = McpStdioStartupTarget | McpHttpStartupTarget


def _mcp_startup_target_from_wire(value: object) -> McpStartupTarget:
    value = wire_object(value, "MCP startup target")
    kind = value.get("kind")
    if kind == "stdio":
        command = wire_string(value, "command", "MCP stdio startup target")
        args = wire_string_tuple(value.get("args"), "MCP stdio startup target args")
        cwd = wire_string(value, "cwd", "MCP stdio startup target")
        env_names = wire_string_tuple(
            value.get("envNames"), "MCP stdio startup target envNames"
        )
        return McpStdioStartupTarget(
            command=command,
            args=args,
            cwd=cwd,
            env_names=env_names,
        )
    if kind == "http":
        url = wire_string(value, "url", "MCP HTTP startup target")
        header_names = wire_string_tuple(
            value.get("headerNames"), "MCP HTTP startup target headerNames"
        )
        credential_names = wire_string_tuple(
            value.get("credentialNames"),
            "MCP HTTP startup target credentialNames",
        )
        return McpHttpStartupTarget(
            url=url,
            header_names=header_names,
            credential_names=credential_names,
        )
    raise ValueError("MCP startup target kind is invalid")


@dataclass(frozen=True, slots=True)
class McpStartupApprovalRequest:
    server: str
    source: str
    target: McpStartupTarget

    @classmethod
    def from_wire(cls, value: object) -> "McpStartupApprovalRequest":
        value = wire_object(value, "MCP startup approval detail")
        return cls(
            server=wire_string(value, "server", "MCP startup approval detail"),
            source=wire_string(value, "source", "MCP startup approval detail"),
            target=_mcp_startup_target_from_wire(value.get("target")),
        )


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
    filesystem: FilesystemApprovalRequest | None
    mcp_startup: McpStartupApprovalRequest | None


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
