from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True, slots=True)
class PendingInteraction:
    interaction_id: str
    thread_id: str
    turn_id: str
    kind: str
    status: str
    payload: object
    resolution: object
    requested_at_ms: int
    resolved_at_ms: int | None

    @classmethod
    def from_wire(cls, value: dict[str, Any]) -> PendingInteraction:
        return cls(
            interaction_id=value["interactionId"],
            thread_id=value["threadId"],
            turn_id=value["turnId"],
            kind=value["kind"],
            status=value["status"],
            payload=value.get("payload"),
            resolution=value.get("resolution"),
            requested_at_ms=value["requestedAtMs"],
            resolved_at_ms=value.get("resolvedAtMs"),
        )


@dataclass(frozen=True, slots=True)
class CompactionResult:
    thread_id: str
    compacted: bool
    reason: str
    message: str
    checkpoint_id: int | None
    first_kept_session_seq: int | None
    tokens_before: int | None
    tokens_after: int | None
    summary: str | None
    summary_provider: str | None
    summary_model: str | None

    @classmethod
    def from_wire(cls, value: dict[str, Any]) -> CompactionResult:
        return cls(
            thread_id=value["session_id"],
            compacted=value["compacted"],
            reason=value["reason"],
            message=value["message"],
            checkpoint_id=value.get("checkpoint_id"),
            first_kept_session_seq=value.get("first_kept_session_seq"),
            tokens_before=value.get("tokens_before"),
            tokens_after=value.get("tokens_after"),
            summary=value.get("summary"),
            summary_provider=value.get("summary_provider"),
            summary_model=value.get("summary_model"),
        )


@dataclass(frozen=True, slots=True)
class ThreadItem:
    session_seq: int
    message: object
    usage: object
    metadata: object
    accounting: object

    @classmethod
    def from_wire(cls, value: dict[str, Any]) -> ThreadItem:
        return cls(
            session_seq=value["sessionSeq"],
            message=value["message"],
            usage=value.get("usage"),
            metadata=value.get("metadata"),
            accounting=value.get("accounting"),
        )


@dataclass(frozen=True, slots=True)
class ThreadSummary:
    id: str
    source: str
    cwd: str
    title: str | None
    started_at_ms: int
    updated_at_ms: int
    archived: bool
    message_count: int
    tool_call_count: int
    active_turn_id: str | None

    @classmethod
    def from_wire(cls, value: dict[str, Any]) -> ThreadSummary:
        return cls(
            id=value["id"],
            source=value["source"],
            cwd=value["cwd"],
            title=value.get("title"),
            started_at_ms=value["startedAtMs"],
            updated_at_ms=value["updatedAtMs"],
            archived=value["archived"],
            message_count=value["messageCount"],
            tool_call_count=value["toolCallCount"],
            active_turn_id=value.get("activeTurnId"),
        )


@dataclass(frozen=True, slots=True)
class ThreadPage:
    threads: tuple[ThreadSummary, ...]
    next_cursor: str | None


@dataclass(frozen=True, slots=True)
class ThreadSnapshot(ThreadSummary):
    pending_interactions: tuple[PendingInteraction, ...]
    items: tuple[ThreadItem, ...]

    @classmethod
    def from_wire(cls, value: dict[str, Any]) -> ThreadSnapshot:
        return cls(
            id=value["id"],
            source=value["source"],
            cwd=value["cwd"],
            title=value.get("title"),
            started_at_ms=value["startedAtMs"],
            updated_at_ms=value["updatedAtMs"],
            archived=value["archived"],
            message_count=value["messageCount"],
            tool_call_count=value["toolCallCount"],
            active_turn_id=value.get("activeTurnId"),
            pending_interactions=tuple(
                PendingInteraction.from_wire(item)
                for item in value.get("pendingInteractions", ())
            ),
            items=tuple(ThreadItem.from_wire(item) for item in value.get("items", ())),
        )


@dataclass(frozen=True, slots=True)
class TurnReceipt:
    accepted: bool
    thread_id: str
    turn_id: str
    client_turn_id: str | None

    @classmethod
    def from_wire(cls, value: dict[str, Any]) -> TurnReceipt:
        return cls(
            accepted=value["accepted"],
            thread_id=value["threadId"],
            turn_id=value["turnId"],
            client_turn_id=value.get("clientTurnId"),
        )


@dataclass(frozen=True, slots=True)
class TurnResult:
    thread_id: str
    outcome: str
    final_answer: str
    provider: str
    model: str
    reasoning_effort: str | None
    tool_failures: int
    context_limit: int | None
    context_snapshot: dict[str, Any] | None
    warnings: tuple[dict[str, Any], ...]
    terminal_reason: dict[str, Any] | None
    terminal_error: dict[str, Any] | None
    selected_agent: dict[str, Any] | None
    selected_skills: tuple[dict[str, Any], ...]

    @classmethod
    def from_wire(cls, value: dict[str, Any]) -> TurnResult:
        return cls(
            thread_id=value["threadId"],
            outcome=value["outcome"],
            final_answer=value["finalAnswer"],
            provider=value["provider"],
            model=value["model"],
            reasoning_effort=value.get("reasoningEffort"),
            tool_failures=value["toolFailures"],
            context_limit=value.get("contextLimit"),
            context_snapshot=value.get("contextSnapshot"),
            warnings=tuple(value.get("warnings", ())),
            terminal_reason=value.get("terminalReason"),
            terminal_error=value.get("terminalError"),
            selected_agent=value.get("selectedAgent"),
            selected_skills=tuple(value.get("selectedSkills", ())),
        )


@dataclass(frozen=True, slots=True)
class TurnEvent:
    type: str
    data: dict[str, Any]

    @classmethod
    def from_wire(cls, value: dict[str, Any]) -> TurnEvent:
        return cls(type=value["type"], data=value)
