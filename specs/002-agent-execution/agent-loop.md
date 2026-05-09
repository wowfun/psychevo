---
name: 002. Agent Loop Attachment
psychevo_self_edit: deny
---

Define the first implementation slice contract for the `psychevo-agent-core`
agent loop.

This attachment is part of [002 Agent Execution](spec.md). It is not an
independently numbered spec and does not define a stable public Rust API.

## Scope

- first-slice agent loop control flow
- internal Rust interface shape needed by the first implementation
- event sink behavior
- tool scheduling and tool-result ordering
- stop and abort behavior

Out of scope:
- durable storage schemas
- concrete coding tool behavior
- CLI rendering and exit codes
- real provider transport behavior

## Interface Shape

The first implementation slice uses direct semantic names for internal Rust
interfaces:

- `EventSink` observes canonical agent events and may fail.
- `ToolBinding` executes one model-requested tool call.
- `AgentLoop` or an equivalent function owns the turn loop.

These names are first-slice implementation constraints, not long-term public
API stability promises.

The loop-visible message model is block based:

- `User` messages carry text blocks.
- `Assistant` messages carry text, reasoning, and tool-call blocks.
- `ToolResult` messages carry one tool-call relationship and JSON result text.

## Event Sink

The first implementation awaits `EventSink` delivery. A sink failure is an
agent-invocation failure because the first durable persistence path is wired
through the sink.

Future observer-only sinks may be best effort, but that distinction is not part
of this slice.

## Tool Scheduling

When one assistant message requests multiple tools, the first implementation
uses this scheduling rule:

- If any requested tool in the batch is sequential, the whole batch executes in
  assistant source order.
- If every requested tool in the batch is parallel, tool executions may run
  concurrently.

`tool_execution_end` may be emitted in completion order for parallel batches.
Loop-visible tool-result messages must always be emitted and persisted in the
assistant source order of their tool calls.

## Tool Failures

Tool validation failures, unknown tools, malformed tool-call arguments, and
tool execution failures become JSON error tool results. They complete the tool
execution with failed outcome material unless the whole invocation is stopped,
failed, or aborted for another reason.

Invalid final tool-call JSON produced by the provider is treated as a tool-call
validation failure and returned to the model as an error tool result instead of
failing the invocation immediately.

## Stop And Abort

The first implementation supports two control signals:

- graceful stop
- abort

A graceful stop finishes the current generation or tool batch, emits any
completed loop-visible messages, and prevents the next model generation. The
agent invocation ends with outcome `stopped`.

Abort cancels provider generation and tool execution as soon as practical. The
agent invocation ends with outcome `aborted`. Partial assistant material may be
observable, but no new turn is started after abort is observed.

## Turn Budget

The first implementation keeps a finite model-turn budget to prevent runaway
tool loops. The runtime default for the built-in coding agent is 32 model turns,
so multi-tool workflows can complete after several tool-result feedback cycles
while still failing closed when the model never reaches a terminal assistant
answer.

Budget exhaustion ends the agent invocation with outcome `failed` after all
already-started tool executions and tool-result messages for the previous turn
have been emitted.

## Related Topics

- [002 Agent Execution](spec.md) defines the semantic event families.
- [003 Normalized Stream](../003-ai-protocol/normalized-stream.md) defines the
  stream categories consumed by the loop.
- [100 Runtime Assembly](../100-coding-agent/runtime-assembly.md) defines how
  runtime wires the first coding-agent invocation.
