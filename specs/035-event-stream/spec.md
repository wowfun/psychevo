---
name: 035. Event Stream
psychevo_self_edit: deny
---

# 035. Event Stream

Define Psychevo's canonical runtime event stream and its projection contract.

## Scope

- runtime-owned canonical session event interface
- event identity, ordering, bootstrap, and terminal invariants
- live preview, materialized fact, and delivery diagnostic separation
- blocking action request and response lifecycle
- Gateway, ACP, TUI, Web, and channel projection boundaries
- reconnect and snapshot recovery semantics

Out of scope:
- provider wire formats, SSE framing, or raw chunk schemas
- durable full-event-log replay or deterministic provider replay
- UI layout, wording, animation, or notification styling
- concrete permission rule languages or sandbox enforcement mechanics
- storage table names, indexes, or retention jobs

## Canonical Runtime Stream

Runtime owns the canonical event stream as a typed `SessionEvent` timeline.
Provider streams, tool callbacks, control inputs, permission waits, clarify
waits, and execution lifecycle observations must be normalized before they cross
the runtime stream seam. Public Gateway, ACP, TUI, Web, CLI, and channel streams
are projections of this timeline and must not become the runtime source of
truth.

Each `SessionEvent` must carry a stable `event_id`, a `session_id` or
`thread_id` once known, and a monotonic per-session ordering fact. Turn-scoped
events must carry `turn_id`. Message, block, tool, and blocking-action events
must carry their corresponding `message_id`, `block_id`, `tool_call_id`, or
`action_id` when those identities are needed for projection, de-duplication, or
recovery.

The first event in a newly opened or resumed session stream must be
`SessionConfigured`. It establishes session identity, thread identity when
known, cwd/root context, model/provider display facts, permission profile,
selected capability roots, selected skills when applicable, and any resume seed
needed before clients process turn events.

The core taxonomy includes:

- session lifecycle and configuration
- turn lifecycle and control
- message and block lifecycle
- reasoning or thinking progress
- tool call, execution, stdout, stderr, and result materialization
- generation start, completion, retryable stream errors, and failed generation
- unified blocking actions for permission, clarify, custom tool, and user input
- usage, accounting, warnings, and terminal errors
- projection and delivery diagnostics

Projection and delivery diagnostics may be represented in the taxonomy so edge
adapters can be tested and inspected. They are not transcript facts, do not
participate in transcript reducers, and must not redefine conversation truth.

## Materialization And Replay

Normal replay uses materialized facts, not token deltas. Persisted messages,
turn terminals, tool result facts, blocking-action outcomes, usage/accounting,
and other durable evidence are the authoritative source for history and
snapshots. Live deltas and preview updates may be retained briefly for
cross-process delivery repair, but clients must be able to recover by loading an
authoritative `thread/read` or `thread/resume` snapshot.

For an Agent-authoritative ACP thread, materialized transcript facts belong to
the bound Agent session rather than Psychevo `messages`. The ACP Adapter
normalizes replay and live updates into bounded typed Agent facts before
crossing the Agent Session seam. Gateway applies them through one reducer; it
does not feed Adapter-shaped JSON through the legacy projector or keep a second
final-answer accumulator. Live, Channel, and history projection derive from the
same typed state. Terminal completion clears retained live facts; reconnect
loads Agent history when that capability was negotiated.

`thread/read` and `thread/resume` snapshots are authoritative. Each snapshot
names its history owner, fidelity, and resumability. An Agent history owner that
cannot be reached returns thread metadata with `fidelity=unavailable`; it
does not substitute local transcript content. A client that
detects a live sequence gap, stale owner, expired retained buffer, or unknown
live identity must refresh the snapshot and discard stale live overlay state for
that thread. Snapshot refresh must not require replaying missed token deltas.

`ThreadHistoryOwnerView` names the content authority as `psychevo`, `agent`, or
`process`. `runtime` is not a history owner: a Runtime Profile selects an
execution Adapter but does not itself own conversation content. Public surfaces
must preserve these owner values without branching on Native versus ACP
implementation kind.

Live entry events are previews. They may update an existing live entry or block
by stable identity, and they may be coalesced into latest-entry snapshots.
Previews must never create durable transcript facts by themselves. Completion
or active-snapshot replay uses the same monotonic overlay merge as ordinary
streaming: a retained live `pending` or `running` block may fill missing
preview fields on a matching message-derived tool block, but it must not
downgrade terminal status, replace result/body/title facts, or remove
invocation metadata. Message-derived tool-call preview blocks must carry enough
identity and invocation data, including `tool_call_id`, `tool_name`, and
`args`/`arguments`, for surfaces to render a stable invocation title before a
tool result exists.
events and materialized snapshots replace previews for the same turn, message,
entry, or block identity.

Live transcript projection must be monotonic per stable block identity. A tool
preview without a provider/runtime `tool_call_id` must receive a non-colliding
temporary identity derived from its stream position, content position, or another
session-local ordering fact; it must never fall back to the bare tool name. If a
later event supplies the durable tool identity, projection may alias the
temporary identity to it.

For a known entry or block identity, later live previews may fill in title,
arguments, order, output, result, and metadata, but they must not downgrade a
tool block from running or terminal state back to pending, nor remove already
observed output or result facts. Surface reducers may defensively drop stale
live overlay, but Gateway projection owns the primary identity and monotonicity
contract.

## Transcript Runtime Testing Oracle

Transcript runtime correctness is judged by semantic ledgers, not screenshots or
raw provider text. Deterministic replay fixtures for Gateway, Web, TUI, and
Workbench tests should normalize each checkpoint to rows with these fields:
`turnId`, `entryId`, `blockId`, `source`, `toolName`, `toolCallId`, `status`,
`order`, `title`, `hasResult`, and `activeElapsedOwner`.

The ledger invariants are:

- entry, block, and tool identities are stable and do not collide inside a turn
- tool status is monotonic: `pending < running < terminal`
- terminal facts keep result, body, title, elapsed, and diagnostic metadata once
  observed
- authoritative snapshots and materialized transcript entries replace same-turn
  live overlay
- stale live overlay cannot recreate, duplicate, or reactivate a completed row
- yielded `exec_command` and matching `write_stdin` observations stay on the
  owning command row

Visual artifacts, terminal VHS captures, screenshots, and live provider sweeps
are supporting signals. They may reveal defects, but transcript correctness must
be asserted through semantic ledgers and deterministic fake-provider replay
first.

Tests and development builds should expose projection diagnostics that can fail
a run without human screenshot inspection. The shared diagnostic vocabulary is
`duplicateLiveToolIdentity`, `statusDowngradePrevented`,
`staleOverlayDropped`, `liveAfterTerminalIgnored`, and
`activeRowAfterTerminal`. `activeRowAfterTerminal` applies only when the
checkpoint has no active turn owner; during active snapshot replay, a
message-derived tool row may own the visible running state for a matching live
tool update. Diagnostics are edge evidence, not transcript facts; expected
diagnostics must be declared by a test, otherwise non-zero counts fail the
checkpoint.

## Blocking Actions

Permission approvals, clarify prompts, custom tool calls requiring caller
execution, and user-input requests use one blocking-action lifecycle:

- `action_requested`
- `action_updated`
- `action_resolved`
- `action_cancelled`

The action `kind` identifies the concrete interaction. Payload shape is
kind-specific, but routing and lifecycle fields are shared: `action_id`,
`thread_id`, `turn_id`, `activity_id`, `source_key`, `owner_id`, expiry, status,
and accepted response metadata when available.

Runtime remains authoritative for permission and tool execution decisions.
Gateway and other surfaces provide request/response rendezvous and recovery.
Late, duplicate, expired, or stale-owner responses must not resume work; they
must be reported to the caller as not accepted.

When a turn ends, every unresolved action for that turn must be resolved or
cancelled through the same lifecycle so surfaces can clear pending state without
guessing from transcript content.

## Projection And Delivery

Gateway exposes a curated typed projection. Ordinary Gateway events must not
carry raw runtime JSON fallback payloads. Public projection events must use
stable names and stable identities for turns, entries, blocks, tools, actions,
warnings, activity, title, and terminal outcomes.

Presentation transports own delivery state. WebSocket, ACP, TUI, channel, and
dashboard adapters may track whether a preview was sent, whether final content
was delivered, whether a final was transformed after streaming, whether stale
preview material was removed, and whether fallback final delivery was required.
Those facts are delivery diagnostics and support evidence; they are not ordinary
transcript content.

Streams must define delivery policy by category:

- terminal turn, action resolution, and materialized snapshot signals are
  delivery-required or recoverable through snapshot refresh
- live previews are best-effort and may be coalesced or dropped with a gap signal
- optional mirrors may be lossy if they expose loss diagnostics or force a
  snapshot refresh on reconnect

## Related Topics

- [002 Agent Execution](../002-agent-execution/spec.md) defines execution event
  families and message semantics.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines runtime
  evidence sink wiring and interface-neutral metadata.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable final
  facts and their relationship to event streams.
- [021 Gateway](../021-gateway/spec.md) defines Gateway projection and local
  transport semantics.
- [031 Storage and Persistence](../031-storage-and-persistence/spec.md) defines
  persistence substrate boundaries and bounded live buffers.
- [041 Permissions](../041-permissions/spec.md) defines permission policy and
  approval outcomes.
- [115 Interactive Clarify](../115-interactive-clarify/spec.md) defines clarify
  tool semantics.
- [250 UI Display Model](../250-ui-display-model/spec.md) defines transcript
  ownership and live overlay reconciliation.
