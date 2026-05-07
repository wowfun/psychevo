---
name: 002. Agent Execution
psychevo_self_edit: deny
---

Define the execution semantics owned by `psychevo-agent-core`.

## Scope

- agent-core execution concepts
- agent-invocation lifecycle semantics
- canonical event families emitted by agent execution
- ordering rules between core execution events
- boundaries between agent execution, runtime coordination, and provider stream handling

Out of scope:
- event payload schemas or wire formats
- Rust traits, structs, functions, or module APIs
- provider-specific stream events or request/response formats
- concrete tool names, tool behavior, or resource gate semantics
- durable trace, replay, session, or persistence formats
- CLI rendering, process behavior, or transport behavior

## Execution Concepts

An agent invocation is one accepted caller prompt or continuation after runtime has resolved a session boundary and assembled the required execution inputs. An agent invocation enters the agent loop, emits `agent_start`, performs one or more ordered turns, and ends with `agent_end`.

An agent invocation is not a session lifecycle boundary. `agent_end` completes the current accepted prompt or continuation, but it does not close, reset, or persistently end the surrounding session.

A turn is one model response plus any tool executions and tool-result messages produced before the next model response. A turn may complete without tool execution.

A turn may expose a live `turn_index` for observation. This spec does not require that index to become a durable field.

A message is a loop-visible artifact with one of these kinds:
- `user`
- `assistant`
- `tool_result`

Instruction context, attached context, summary context, resource facts, and other runtime-supplied inputs are not core message kinds unless a later spec promotes them into loop-visible messages.

A tool execution is the execution of one assistant-requested tool call through a runtime-supplied execution binding from the agent-invocation scoped tool surface. Agent execution observes the tool call, invokes the supplied binding, and records the resulting tool-result message.

Tool-result material is the loop-visible result material produced by the execution binding. Agent execution preserves the tool-result material and an execution outcome summary, but it does not own capability-specific result schemas.

An outcome describes how an agent invocation, turn, message, or tool execution ends. Core outcomes are:
- normal
- stopped
- failed
- aborted

Outcomes are represented by end-event semantics. They do not require separate failed, stopped, or aborted event families.

## Event Families

Agent execution defines these canonical event families:
- `agent_start`, `agent_end`
- `turn_start`, `turn_end`
- `message_start`, `message_update`, `message_end`
- `tool_execution_start`, `tool_execution_update`, `tool_execution_end`

`agent_start` and `agent_end` bound one accepted agent invocation. `agent_end` supports a semantic projection of the invocation terminal outcome and final messages or final material needed by observers. That projection may be derived from loop messages, provider results, runtime completion facts, or interface settlement facts; this spec does not require a low-level core event payload to natively carry every projected field. `agent_end` does not indicate that the session has ended.

`turn_start` and `turn_end` bound one turn within an agent invocation.

`message_start`, `message_update`, and `message_end` describe loop-visible message production. `message_update` is for assistant streaming. User and tool-result messages may emit only start and end events.

Assistant reasoning/thinking progress is not final visible assistant text. An
implementation may retain it as folded local transcript material and expose it
through separate observation events, but reasoning-only provider progress must
not force an otherwise empty `message_update`.

`tool_execution_start`, `tool_execution_update`, and `tool_execution_end` describe tool execution through a runtime-supplied abstraction. `tool_execution_start` includes the execution start timestamp reported by the local runtime clock. `tool_execution_update` is optional and exists for tools that report progress.

`tool_execution_end` may expose the raw tool-result material, elapsed execution duration, and outcome summary needed by observers. The elapsed duration covers actual tool binding execution, not time spent waiting behind other sequential tools. Capability or tool specs may define structured result material, but this spec does not freeze those payload schemas.

## Event Ordering

Every started agent invocation emits `agent_start` before its inner events and `agent_end` after its loop events.

Turns are ordered within an agent invocation. A turn emits `turn_start` before its turn-local messages or tool executions and `turn_end` after them.

Each message emits `message_start` before `message_update` or `message_end`. `message_end` completes that message's lifecycle.

Each tool execution emits `tool_execution_start` before any matching `tool_execution_update` or `tool_execution_end`. `tool_execution_end` completes that tool execution's lifecycle.

When one assistant message requests multiple tools, agent execution may execute those tool bindings sequentially or concurrently. The spec fixes causal ordering for each tool execution, not scheduling across different tool executions.

Regardless of scheduling, each tool execution result must remain causally associated with the assistant tool call that requested it. This spec does not define cross-tool sorting policy, batching strategy, or concurrency policy.

Tool-result messages are loop-visible messages and must be ordered so the next model response can consume them as part of the loop-visible transcript.

When a runtime-supplied binding reports a tool failure as a tool result, agent execution records it as a completed tool execution with a failed outcome summary unless the agent invocation itself is stopped, failed, or aborted for another reason.

## Boundaries

This spec owns agent-core execution semantics and event families.

`psychevo-runtime` owns session coordination, model context assembly, resource surface wiring, agent-invocation scoped tool surface assembly, durable execution records, persistence, and replay wiring.

`psychevo-ai` owns provider protocol normalization. Provider stream events may be converted into core message and tool execution events, but provider event shapes are not part of this spec.

`psychevo-cli` owns terminal rendering and process behavior. CLI output must not define core execution semantics.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream project foundation and implementation-neutral principles.
- [001 Architecture](../001-architecture/spec.md) defines Rust workspace layout, crate boundaries, runtime coordination, and dependency direction.
- [002 Agent Loop](agent-loop.md) defines the first implementation slice loop contract.
- [003 AI Protocol](../003-ai-protocol/spec.md) defines provider-neutral generation semantics consumed by agent execution.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines agent-invocation assembly and evidence sink wiring.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable evidence semantics for finalized execution facts.
- [006 Context Assembly](../006-context-assembly/spec.md) defines model context assembly and its relationship to loop-visible messages.
- [007 Tool Surface](../007-tool-surface/spec.md) defines agent-invocation scoped tool declarations and execution bindings.
- [009 Resource Surface](../009-resource-surface/spec.md) defines resource facts and resource gate semantics outside agent execution.
- [030 State and Data Model](../030-state-and-data-model/spec.md) defines how execution facts relate to other state families.
