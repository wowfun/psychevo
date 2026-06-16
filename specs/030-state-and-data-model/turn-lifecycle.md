---
name: 030. Turn Lifecycle Attachment
psychevo_self_edit: deny
---

# 030. Turn Lifecycle Attachment

Define the shared semantic state for accepted turns that have reached a
terminal lifecycle outcome.

This attachment is part of [030 State and Data Model](spec.md). It is not an
independently numbered spec and does not introduce a public transport schema.

## Scope

- terminal turn lifecycle facts for accepted turns
- relationship between turn terminal status and ordinary transcript projection
- shared TUI/GUI reload and cross-surface status semantics
- provider/runtime error visibility after live observation settles

Out of scope:

- complete per-turn event logs, debug logs, provider raw event archives, or
  deterministic replay
- model-visible message content, assistant content, or exported transcript
  material
- concrete table names, JSON field names, Rust types, or transport payloads

## Terminal Fact

An accepted turn may finish as `completed`, `failed`, or `interrupted`.
The terminal turn lifecycle fact records that final status, relates it to the
owning thread/session, and may retain a bounded display error message for local
inspection.

The terminal fact is not an ordinary transcript message. It must not be fed
back into model context as assistant text, exported as assistant content, or
counted as a loop-visible message. Product transcript views may project it as a
diagnostic/status row so users can understand why a turn stopped.

Terminal status is the shared fact source for live and reload behavior. If a
turn fails after the caller has accepted and started it, every surface that
observes that thread should converge on the same `failed` terminal status and
display error. If a turn is interrupted, every surface should converge on
`interrupted` and stop showing active tool or reasoning spinners for that turn.

## Relationships

The terminal fact must be relatable to:

- the thread/session that owned the accepted turn
- the turn identity used by live observation and control paths
- any ordinary messages committed before the terminal outcome
- any active Gateway activity that represented the running turn

Normal completed turns may have no diagnostic projection when committed
ordinary transcript messages are enough to explain the result. Failed and
interrupted turns should remain visible after history reload even when no
assistant message was committed.

## Source Boundaries

Runtime messages remain the source of truth for ordinary transcript content.
Gateway live events remain observations for active surfaces. The terminal turn
lifecycle fact is the durable source for cross-surface terminal status after an
accepted turn settles.

Request-acceptance failures that happen before a turn is accepted may be
reported as request errors instead of turn terminal facts. Once a turn has an
accepted turn identity, provider errors, runtime errors, and user interrupts
must settle through terminal lifecycle semantics.

## Related Topics

- [030 State and Data Model](spec.md) defines state ownership and recoverability.
- [030 Transcript State](transcript-state.md) defines ordinary transcript fact
  ownership.
- [031 Storage and Persistence](../031-storage-and-persistence/spec.md) defines
  durable storage boundaries.
- [213 Thread Navigation](../213-pevo-display-model/thread-navigation.md) defines
  child thread display behavior.
- [220 pevo Gateway](../220-pevo-gateway/spec.md) defines the Workbench/Web
  projection.
