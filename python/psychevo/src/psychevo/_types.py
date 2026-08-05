from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from ._wire import (
    wire_bool,
    wire_enum,
    wire_json_safe_int,
    wire_list,
    wire_nullable_json_safe_int,
    wire_nullable_string,
    wire_object,
    wire_optional_json_safe_int,
    wire_optional_string,
    wire_present,
    wire_string,
)

_TURN_OUTCOMES = frozenset({"completed", "stopped", "failed", "interrupted"})
_ITEM_STAGES = frozenset({"started", "updated", "completed"})


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
    def from_wire(cls, value: object) -> PendingInteraction:
        value = wire_object(value, "pending interaction")
        return cls(
            interaction_id=wire_string(
                value, "interactionId", "pending interaction"
            ),
            thread_id=wire_string(value, "threadId", "pending interaction"),
            turn_id=wire_string(value, "turnId", "pending interaction"),
            kind=wire_string(value, "kind", "pending interaction"),
            status=wire_string(value, "status", "pending interaction"),
            payload=wire_present(value, "payload", "pending interaction"),
            resolution=value.get("resolution"),
            requested_at_ms=wire_json_safe_int(
                value, "requestedAtMs", "pending interaction"
            ),
            resolved_at_ms=wire_optional_json_safe_int(
                value, "resolvedAtMs", "pending interaction"
            ),
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
    def from_wire(cls, value: object) -> CompactionResult:
        value = wire_object(value, "thread/compact result")
        return cls(
            thread_id=wire_string(value, "threadId", "thread/compact result"),
            compacted=wire_bool(value, "compacted", "thread/compact result"),
            reason=wire_string(value, "reason", "thread/compact result"),
            message=wire_string(value, "message", "thread/compact result"),
            checkpoint_id=wire_nullable_json_safe_int(
                value, "checkpointId", "thread/compact result"
            ),
            first_kept_session_seq=wire_nullable_json_safe_int(
                value, "firstKeptSessionSeq", "thread/compact result"
            ),
            tokens_before=wire_nullable_json_safe_int(
                value, "tokensBefore", "thread/compact result", minimum=0
            ),
            tokens_after=wire_nullable_json_safe_int(
                value, "tokensAfter", "thread/compact result", minimum=0
            ),
            summary=wire_nullable_string(value, "summary", "thread/compact result"),
            summary_provider=wire_nullable_string(
                value, "summaryProvider", "thread/compact result"
            ),
            summary_model=wire_nullable_string(
                value, "summaryModel", "thread/compact result"
            ),
        )


@dataclass(frozen=True, slots=True)
class ThreadItem:
    session_seq: int
    message: object
    usage: object
    metadata: object
    accounting: object

    @classmethod
    def from_wire(cls, value: object) -> ThreadItem:
        value = wire_object(value, "Thread item")
        return cls(
            session_seq=wire_json_safe_int(value, "sessionSeq", "Thread item"),
            message=wire_present(value, "message", "Thread item"),
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
    def from_wire(cls, value: object) -> ThreadSummary:
        return cls(**_thread_summary_fields(value, "Thread summary"))


@dataclass(frozen=True, slots=True)
class ThreadPage:
    threads: tuple[ThreadSummary, ...]
    next_cursor: str | None


@dataclass(frozen=True, slots=True)
class ThreadSnapshot(ThreadSummary):
    pending_interactions: tuple[PendingInteraction, ...]
    items: tuple[ThreadItem, ...]

    @classmethod
    def from_wire(cls, value: object) -> ThreadSnapshot:
        value = wire_object(value, "Thread snapshot")
        pending_interactions = wire_list(
            value.get("pendingInteractions", []),
            "Thread snapshot pendingInteractions",
        )
        items = wire_list(value.get("items", []), "Thread snapshot items")
        return cls(
            **_thread_summary_fields(value, "Thread snapshot"),
            pending_interactions=tuple(
                PendingInteraction.from_wire(item)
                for item in pending_interactions
            ),
            items=tuple(ThreadItem.from_wire(item) for item in items),
        )


@dataclass(frozen=True, slots=True)
class TurnReceipt:
    accepted: bool
    thread_id: str
    turn_id: str
    client_turn_id: str | None

    @classmethod
    def from_wire(cls, value: object) -> TurnReceipt:
        value = wire_object(value, "Turn receipt")
        return cls(
            accepted=wire_bool(value, "accepted", "Turn receipt"),
            thread_id=wire_string(value, "threadId", "Turn receipt"),
            turn_id=wire_string(value, "turnId", "Turn receipt"),
            client_turn_id=wire_optional_string(
                value, "clientTurnId", "Turn receipt"
            ),
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
    def from_wire(cls, value: object) -> TurnResult:
        value = wire_object(value, "turn/wait result")
        for key in (
            "contextSnapshot",
            "terminalReason",
            "terminalError",
            "selectedAgent",
        ):
            if key in value and value[key] is not None:
                wire_object(value[key], f"turn/wait result {key}")
        warnings = wire_list(value.get("warnings", []), "turn/wait result warnings")
        selected_skills = wire_list(
            value.get("selectedSkills", []), "turn/wait result selectedSkills"
        )
        for item in warnings:
            wire_object(item, "turn/wait result warnings item")
        for item in selected_skills:
            wire_object(item, "turn/wait result selectedSkills item")
        return cls(
            thread_id=wire_string(value, "threadId", "turn/wait result"),
            outcome=wire_enum(
                value, "outcome", "turn/wait result", _TURN_OUTCOMES
            ),
            final_answer=wire_string(value, "finalAnswer", "turn/wait result"),
            provider=wire_string(value, "provider", "turn/wait result"),
            model=wire_string(value, "model", "turn/wait result"),
            reasoning_effort=wire_optional_string(
                value, "reasoningEffort", "turn/wait result"
            ),
            tool_failures=wire_json_safe_int(
                value, "toolFailures", "turn/wait result", minimum=0
            ),
            context_limit=wire_optional_json_safe_int(
                value, "contextLimit", "turn/wait result", minimum=0
            ),
            context_snapshot=value.get("contextSnapshot"),
            warnings=tuple(warnings),
            terminal_reason=value.get("terminalReason"),
            terminal_error=value.get("terminalError"),
            selected_agent=value.get("selectedAgent"),
            selected_skills=tuple(selected_skills),
        )


@dataclass(frozen=True, slots=True)
class TurnEvent:
    type: str
    data: dict[str, Any]

    @classmethod
    def from_wire(
        cls,
        value: object,
        *,
        thread_id: str | None = None,
        turn_id: str | None = None,
    ) -> TurnEvent:
        event = wire_object(value, "turn/event event")
        event_type = wire_string(event, "type", "turn/event event")
        if event_type == "accepted":
            receipt = TurnReceipt.from_wire(event.get("receipt"))
            _require_event_identity(receipt.thread_id, receipt.turn_id, thread_id, turn_id)
            wire_optional_json_safe_int(
                event, "queuePosition", "accepted event", minimum=0
            )
        elif event_type in {"started", "completed", "failed"}:
            event_thread_id = wire_string(event, "threadId", f"{event_type} event")
            event_turn_id = wire_string(event, "turnId", f"{event_type} event")
            _require_event_identity(
                event_thread_id, event_turn_id, thread_id, turn_id
            )
            if event_type == "completed":
                wire_enum(event, "outcome", "completed event", _TURN_OUTCOMES)
            elif event_type == "failed":
                wire_string(event, "message", "failed event")
        elif event_type == "message":
            wire_enum(event, "stage", "message event", _ITEM_STAGES)
            wire_present(event, "message", "message event")
        elif event_type in {"message_delta", "reasoning_delta"}:
            wire_string(event, "text", f"{event_type} event")
        elif event_type == "reasoning_completed":
            wire_nullable_string(event, "text", "reasoning_completed event")
        elif event_type == "tool":
            wire_enum(event, "stage", "tool event", _ITEM_STAGES)
            wire_present(event, "data", "tool event")
        elif event_type == "interaction_requested":
            wire_string(event, "interactionId", "interaction_requested event")
            wire_string(event, "kind", "interaction_requested event")
            wire_present(event, "payload", "interaction_requested event")
        elif event_type == "interaction_resolved":
            wire_string(event, "interactionId", "interaction_resolved event")
            wire_string(event, "reason", "interaction_resolved event")
        elif event_type == "warning":
            wire_present(event, "data", "warning event")
        elif event_type == "resync_required":
            wire_json_safe_int(event, "missed", "resync_required event", minimum=0)
        else:
            raise ValueError(f"turn/event type is invalid: {event_type}")
        return cls(type=event_type, data=event)


def _thread_summary_fields(value: object, label: str) -> dict[str, Any]:
    value = wire_object(value, label)
    return {
        "id": wire_string(value, "id", label),
        "source": wire_string(value, "source", label),
        "cwd": wire_string(value, "cwd", label),
        "title": wire_optional_string(value, "title", label),
        "started_at_ms": wire_json_safe_int(value, "startedAtMs", label),
        "updated_at_ms": wire_json_safe_int(value, "updatedAtMs", label),
        "archived": wire_bool(value, "archived", label),
        "message_count": wire_json_safe_int(value, "messageCount", label),
        "tool_call_count": wire_json_safe_int(value, "toolCallCount", label),
        "active_turn_id": wire_optional_string(value, "activeTurnId", label),
    }


def _require_event_identity(
    event_thread_id: str,
    event_turn_id: str,
    thread_id: str | None,
    turn_id: str | None,
) -> None:
    if thread_id is not None and event_thread_id != thread_id:
        raise ValueError("turn/event has conflicting Thread identity")
    if turn_id is not None and event_turn_id != turn_id:
        raise ValueError("turn/event has conflicting Turn identity")
