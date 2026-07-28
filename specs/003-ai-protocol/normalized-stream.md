---
name: 003. Normalized Stream Attachment
psychevo_self_edit: deny
---

Define the public normalized generation contract implemented by
`psychevo-ai`.

This attachment is part of [003 AI Protocol](spec.md). Rust interface and
packaging requirements are defined by [080 Framework and SDK](../080-sdk/spec.md).

## Scope

- normalized language-generation event categories and lifecycle
- the seam between capability Adapters and the SDK generation Module
- deterministic fake Adapter behavior
- streamed content identity, validation, and final assembly
- abort, error, partial-result, and completion observation semantics

Out of scope:

- concrete provider HTTP, SSE, WebSocket, authentication, or wire fields
- provider retry, billing, pricing, or model catalog policy
- CLI rendering or persistence schemas

## Adapter and Caller Seams

Language providers implement a capability-specific streaming Adapter. An
Adapter receives the bound model id, the provider-neutral request, and an
invocation context containing only the resolved runtime dependencies needed for
that call. It returns a pull-side stream of Adapter events. It does not receive
an event sink or agent tool executor.

The Adapter event type is separate from the caller-visible generation event
type. The SDK generation Module owns local start, event validation, complete
assistant assembly, abort synthesis, error snapshots, and the unique terminal
result. An Adapter cannot author those caller lifecycle facts directly.

Starting a language invocation is eager. The returned generation handle is the
sole event consumer and also exposes a completion observer. A convenience
whole-response operation collects the same streaming path; there is no second
provider invocation implementation.

The caller-visible stream always starts with one local `Started` event. This
means that the invocation and its bound model were established locally; it does
not mean that an HTTP request was sent or accepted remotely. Invocation-time
configuration, runtime, credential, or preflight errors therefore still follow
`Started`.

The normalized event families are:

- local invocation start
- text start, delta, and end
- reasoning start, delta, and end
- tool-call start, argument delta, and end
- provider-hosted tool start and end
- source addition
- normalized usage
- allowlisted provider metadata
- warnings
- one finish event for a completed or explicitly aborted generation

Content-bearing events use one `content_index` space across text, reasoning,
tool calls, provider tools, and sources. The SDK rejects conflicting indices,
gaps, duplicate starts, end-before-start, identity changes, and other known
lifecycle violations. Provider events after the first terminal fact do not
create a second terminal outcome.
After a caller abort becomes observable, an agent consumer forwards it once and
waits on the generation completion authority without draining queued
non-terminal deltas. Cancellation latency and terminal delivery therefore do
not scale with the amount of already-buffered progress.

The stream preserves deltas instead of cloning a complete partially assembled
message on every chunk. Final output contains an ordered typed assistant
message, usage, warnings, and allowlisted provider metadata. Delta history is
not retained in the final result. Consumers that persist or replay an assistant
message use this ordered final snapshot as the authority; they must not regroup
text, reasoning, and tool blocks by kind or reconstruct a different order from
parallel accumulators.

Provider evidence belongs to the reasoning block at the same `content_index`.
Evidence is opaque provider-native continuity data rather than a
provider-neutral wrapper. When a provider supplies evidence in multiple delta
fragments, the SDK accumulates those fragments into that block's final evidence
without moving them to another block or changing the provider-native shape.

## Tool-Call Assembly

Tool-call deltas have stable `content_index`, call id, and name identity. Delta
arguments exist for real-time display only. The complete raw argument string on
the tool-call end event is the sole final authority.

An Adapter with a provider-native final raw string forwards that value. An
Adapter whose protocol only supplies deltas accumulates them and moves the
complete raw string into the end event. An Adapter whose protocol only supplies
a structured object serializes that object deterministically.

The SDK parses only the complete end value and accepts only a complete JSON
object. It does not repair partial JSON, guess omitted bytes, or replace an
invalid value with an empty object. Invalid arguments remain a completed tool
call containing the full raw value and a structured argument error; they do not
fail the generation. A downstream agent executor preserves that structured
error and produces an invalid-arguments tool result without invoking the tool.
Tool schema validation belongs to the downstream executor.
Missing call id or name, conflicting identity, or an invalid event lifecycle is
a protocol failure with the accumulated partial result.

## Completion, Failure, and Abort

The Adapter must explicitly finish. EOF before finish is a protocol failure.
Provider and transport failures are Rust errors, not synthetic successful
finish events. Each failure carries the same accumulated snapshot shape as a
successful result.

An explicit abort is first-wins. The SDK signals the Adapter, aborts its local
invocation task, and synthesizes exactly one aborted terminal result from the
events already accepted. It cannot promise provider-side cancellation or
billing reversal. Dropping the sole generation handle performs the same local
abort. A cloneable completion observer does not keep the invocation alive.

Language generation uses an unbounded retained event queue so that a provider
task cannot deadlock behind a caller that temporarily stops polling. The public
documentation must warn that retaining a generation without consuming it can
grow memory without bound.

## Fake Provider

The SDK includes deterministic fake Adapters for every capability. The fake
language Adapter accepts scripted Adapter events and failures and crosses the
same validation seam as a real provider.

Fake Adapters must not read API keys, use network services, or depend on host
user configuration.

## Related Topics

- [003 AI Protocol](spec.md) defines provider-neutral generation semantics.
- [002 Agent Loop](../002-agent-execution/agent-loop.md) defines how the agent
  loop consumes the stream.
