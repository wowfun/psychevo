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
class FilesystemApprovalTarget:
    requested_path: str
    resolved_path: str

    @classmethod
    def from_wire(cls, value: object) -> "FilesystemApprovalTarget":
        if not isinstance(value, dict):
            raise TypeError("filesystem approval target must be an object")
        requested_path = value.get("requestedPath")
        resolved_path = value.get("resolvedPath")
        if not isinstance(requested_path, str) or not isinstance(resolved_path, str):
            raise TypeError("filesystem approval target paths must be strings")
        return cls(requested_path=requested_path, resolved_path=resolved_path)


@dataclass(frozen=True, slots=True)
class FilesystemApprovalRequest:
    targets: tuple[FilesystemApprovalTarget, ...]
    scope_candidates: tuple[str, ...]

    @classmethod
    def from_wire(cls, value: object) -> "FilesystemApprovalRequest":
        if not isinstance(value, dict):
            raise TypeError("filesystem approval detail must be an object")
        targets = value.get("targets")
        scope_candidates = value.get("scopeCandidates")
        if not isinstance(targets, list) or not isinstance(scope_candidates, list):
            raise TypeError("filesystem approval detail has invalid lists")
        if not all(isinstance(item, str) for item in scope_candidates):
            raise TypeError("filesystem approval scope candidates must be strings")
        return cls(
            targets=tuple(FilesystemApprovalTarget.from_wire(item) for item in targets),
            scope_candidates=tuple(scope_candidates),
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
    if not isinstance(value, dict):
        raise TypeError("MCP startup target must be an object")
    kind = value.get("kind")
    if kind == "stdio":
        command = value.get("command")
        args = value.get("args")
        cwd = value.get("cwd")
        env_names = value.get("envNames")
        if (
            not isinstance(command, str)
            or not isinstance(cwd, str)
            or not isinstance(args, list)
            or not all(isinstance(item, str) for item in args)
            or not isinstance(env_names, list)
            or not all(isinstance(item, str) for item in env_names)
        ):
            raise TypeError("MCP stdio startup target is malformed")
        return McpStdioStartupTarget(
            command=command,
            args=tuple(args),
            cwd=cwd,
            env_names=tuple(env_names),
        )
    if kind == "http":
        url = value.get("url")
        header_names = value.get("headerNames")
        credential_names = value.get("credentialNames")
        if (
            not isinstance(url, str)
            or not isinstance(header_names, list)
            or not all(isinstance(item, str) for item in header_names)
            or not isinstance(credential_names, list)
            or not all(isinstance(item, str) for item in credential_names)
        ):
            raise TypeError("MCP HTTP startup target is malformed")
        return McpHttpStartupTarget(
            url=url,
            header_names=tuple(header_names),
            credential_names=tuple(credential_names),
        )
    raise TypeError("MCP startup target kind is invalid")


@dataclass(frozen=True, slots=True)
class McpStartupApprovalRequest:
    server: str
    source: str
    target: McpStartupTarget

    @classmethod
    def from_wire(cls, value: object) -> "McpStartupApprovalRequest":
        if not isinstance(value, dict):
            raise TypeError("MCP startup approval detail must be an object")
        server = value.get("server")
        source = value.get("source")
        if not isinstance(server, str) or not isinstance(source, str):
            raise TypeError("MCP startup approval detail fields must be strings")
        return cls(
            server=server,
            source=source,
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
