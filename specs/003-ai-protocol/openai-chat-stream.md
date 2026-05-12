---
name: 003. OpenAI Chat Stream Attachment
psychevo_self_edit: deny
---

Define the first live provider adapter contract for OpenAI Chat
Completions-compatible streaming.

This attachment is part of [003 AI Protocol](spec.md). It is not an
independently numbered spec and does not define a stable public Rust API.

## Scope

- Chat Completions-compatible request shape used by the first live provider
- server-sent event parsing requirements
- tool-call delta normalization
- provider error normalization

Out of scope:

- alternate provider-specific APIs, OAuth providers, external portal auth
  flows, model catalogs, billing, or retries
- non-streaming whole-response generation
- stable raw provider payload exposure

## Request Contract

The live adapter posts to a Chat Completions-compatible endpoint:

- base URL values ending in `/chat/completions` are used as-is
- other base URL values append `/chat/completions`
- auth uses `Authorization: Bearer <token>`
- requests set `stream: true`
- tools are sent as function tool declarations when present
- optional model thinking intensity metadata may be sent as `reasoning_effort`
  when runtime resolves it

The adapter translates loop-visible messages to mainstream Chat roles:

- user text becomes `role: "user"`
- assistant text and tool calls become `role: "assistant"` with optional
  `tool_calls`
- tool results become `role: "tool"` with `tool_call_id`

Assistant messages without tool calls omit the `tool_calls` field.
Interleaved reasoning replay is a provider wire projection, not provider-neutral
message semantics. When target model metadata declares
`capabilities.interleaved.field = "reasoning_content"`, the adapter may project
retained folded reasoning back as `reasoning_content` on replayed assistant
messages. A target metadata value of `capabilities.interleaved = false`
explicitly disables that projection. When `capabilities.reasoning = true` and
`capabilities.interleaved` is missing, null, or true, the adapter may default the
wire projection field to `reasoning_content`. Other explicit interleaved fields
must not be rewritten to `reasoning_content`.

Provider and base-URL fallbacks may enable `reasoning_content` for configured
targets that lack usable metadata. If the target requires the field but no
retained reasoning text is available, the adapter may send a compatibility
placeholder.

## SSE Parsing

The adapter parses `text/event-stream` frames from streamed byte chunks.

The parser must handle:

- LF, CRLF, and bare CR line endings
- UTF-8 BOM at stream start
- comment frames
- multi-line `data:` fields
- `[DONE]` terminal data
- provider error objects carried in `data:`
- chunk boundaries that split UTF-8 codepoints
- chunk boundaries that split frames or JSON payloads

The parser must not require a third-party eventsource runtime.

## Normalization

Text deltas become normalized text deltas.

Chat `delta.tool_calls[]` entries become normalized tool-call deltas. The
adapter maps the provider tool call index to `call_index` and uses the same
value for `content_index` in this slice.

When a tool call first provides an id and function name, the adapter emits
`tool_call_start`. Argument fragments become `tool_call_delta`. Completion of
a tool-call generation emits `tool_call_end` for every started call before the
terminal `done` event.

Usage chunks become normalized usage metadata when present.

`finish_reason` is carried to the terminal `done` event. A provider or protocol
failure becomes a provider error and causes the invocation to fail through the
normal agent-loop error path.

## Related Topics

- [003 AI Protocol](spec.md) defines provider-neutral generation semantics.
- [003 Normalized Stream](normalized-stream.md) defines the first normalized
  stream categories consumed by the agent loop.
- [120 Provider Registry](../120-provider-registry/spec.md) defines provider
  selection and configuration for the live entrypoint.
