---
name: 003. Normalized Stream Attachment
psychevo_self_edit: deny
---

Define the first implementation slice contract for normalized generation
streams in `psychevo-ai`.

This attachment is part of [003 AI Protocol](spec.md). It is not an
independently numbered spec and does not define a stable public Rust API.

## Scope

- first-slice normalized stream event categories
- first-slice provider interface shape
- deterministic fake provider behavior
- streamed tool-call assembly keys

Out of scope:
- real provider HTTP, SSE, auth, retry, billing, or model catalogs
- OpenAI-compatible or Anthropic-compatible concrete wire fields
- CLI rendering or persistence schemas

## Interface Shape

The first implementation uses a `GenerationProvider` interface that returns a
normalized stream. It does not expose a whole-response generation shortcut.

The normalized stream supports:

- assistant text deltas
- assistant reasoning deltas
- assistant tool-call deltas
- optional usage metadata
- optional extension metadata
- one terminal outcome

The provider boundary is internal. Concrete Rust names may evolve with later
attachments, but the first implementation should keep this semantic shape.

## Tool-Call Assembly

Streamed tool-call deltas are assembled by:

- `content_index`
- `call_index`

The assembled call preserves the latest id and name material plus concatenated
argument deltas until the tool call ends. A provider stream that ends with
invalid JSON tool-call arguments does not directly fail the invocation; the
agent loop receives an invalid tool call and returns a JSON error tool result.

## Fake Provider

The only first-slice provider implementation is a deterministic `FakeProvider`.
It accepts scripted raw stream events and emits normalized stream events.

The fake provider exists for local validation, smoke tests, and deterministic
agent-loop tests. It must not read API keys, use network services, or depend on
host user configuration.

No OpenAI-compatible unsupported stub is implemented in this slice. The main
AI protocol spec still names compatibility families as future adapter design
guidance.

## Related Topics

- [003 AI Protocol](spec.md) defines provider-neutral generation semantics.
- [002 Agent Loop](../002-agent-execution/agent-loop.md) defines how the agent
  loop consumes the stream.
