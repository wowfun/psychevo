---
name: 002. Agent Execution
psychevo_self_edit: deny
---

Define the execution semantics owned by `psychevo-agent-core`.

## Scope

- agent-core execution concepts
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

A run is one caller-invoked agent execution. A run contains one or more ordered turns and ends with an outcome.

A turn is one model response plus any tool executions and tool-result messages produced before the next model response. A turn may complete without tool execution.

A message is a loop-visible artifact with one of these kinds:
- `user`
- `assistant`
- `tool_result`

Instruction context, attached context, summary context, resource facts, and other runtime-supplied inputs are not core message kinds unless a later spec promotes them into loop-visible messages.

A tool execution is the execution of one assistant-requested tool call through a runtime-supplied execution binding from the run-scoped tool surface. Agent execution observes the tool call, invokes the supplied binding, and records the resulting tool-result message.

An outcome describes how a run, turn, message, or tool execution ends. Core outcomes are:
- normal
- stopped
- failed
- aborted

Outcomes are represented by end-event semantics. They do not require separate failed, stopped, or aborted event families.

## Event Families

Agent execution defines these canonical event families:
- `run_start`, `run_end`
- `turn_start`, `turn_end`
- `message_start`, `message_update`, `message_end`
- `tool_execution_start`, `tool_execution_update`, `tool_execution_end`

`run_start` and `run_end` bound one caller-invoked execution.

`turn_start` and `turn_end` bound one turn within a run.

`message_start`, `message_update`, and `message_end` describe loop-visible message production. `message_update` is for assistant streaming. User and tool-result messages may emit only start and end events.

`tool_execution_start`, `tool_execution_update`, and `tool_execution_end` describe tool execution through a runtime-supplied abstraction. `tool_execution_update` is optional and exists for tools that report progress.

## Event Ordering

Every run emits `run_start` before its inner events and `run_end` after its loop events.

Turns are ordered within a run. A turn emits `turn_start` before its turn-local messages or tool executions and `turn_end` after them.

Each message emits `message_start` before `message_update` or `message_end`. `message_end` completes that message's lifecycle.

Each tool execution emits `tool_execution_start` before any matching `tool_execution_update` or `tool_execution_end`. `tool_execution_end` completes that tool execution's lifecycle.

When one assistant message requests multiple tools, agent execution may run those tool executions sequentially or concurrently. The spec fixes causal ordering for each tool execution, not scheduling across different tool executions.

Tool-result messages are loop-visible messages and must be ordered so the next model response can consume them as part of the loop-visible transcript.

## Boundaries

This spec owns agent-core execution semantics and event families.

`psychevo-runtime` owns model context assembly, resource surface wiring, run-scoped tool surface assembly, durable execution records, and replay wiring.

`psychevo-ai` owns provider protocol normalization. Provider stream events may be converted into core message and tool execution events, but provider event shapes are not part of this spec.

`psychevo-cli` owns terminal rendering and process behavior. CLI output must not define core execution semantics.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream project foundation and implementation-neutral principles.
- [001 Architecture](../001-architecture/spec.md) defines Rust workspace layout, crate boundaries, runtime coordination, and dependency direction.
- [003 AI Protocol](../003-ai-protocol/spec.md) defines provider-neutral generation semantics consumed by agent execution.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines runtime run assembly and evidence sink wiring.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable evidence semantics for finalized execution facts.
- [006 Context Assembly](../006-context-assembly/spec.md) defines model context assembly and its relationship to loop-visible messages.
- [007 Tool Surface](../007-tool-surface/spec.md) defines run-scoped tool declarations and execution bindings.
- [009 Resource Surface](../009-resource-surface/spec.md) defines resource facts and resource gate semantics outside agent execution.
