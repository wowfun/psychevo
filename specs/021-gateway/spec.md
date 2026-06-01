---
name: 021. Gateway
psychevo_self_edit: deny
---

# 021. Gateway

Define Psychevo's transport-neutral gateway layer for current and future
interactive surfaces.

Gateway is the caller-facing orchestration layer above
`psychevo-runtime`. It normalizes source identity, thread and turn requests,
active-turn control, interaction requests, and observation events for CLI, TUI,
ACP, future Web/Desktop surfaces, IM adapters, and peer-agent backends.

## Scope

- transport-neutral thread and turn model
- source identity and source-to-thread mapping
- active-turn queue, steer, interrupt, and reset semantics
- gateway-owned permission and clarify request routing
- canonical caller-facing item and event projection
- bounded debug observation access separate from ordinary transcript events
- local loopback HTTP/WebSocket facade for product and API clients
- generic IM source adapter boundary for first-party Gateway integration
- backend boundary for Psychevo runtime and future peer-agent executors

Out of scope:

- concrete Web/Desktop UI behavior and CLI lifecycle commands
- public internet, LAN, relay, TLS, or installer service behavior
- concrete IM platform SDKs, stdio, native desktop bridge, or mobile shell
  transport adapters
- external peer-agent implementation beyond the backend boundary
- provider/model resolution semantics owned by runtime and provider specs
- capability selection semantics owned by runtime and capability specs

## Architecture Boundary

`psychevo-gateway` depends on `psychevo-runtime`. Product entrypoints such as
CLI, TUI, ACP, future Web/Desktop daemons, and IM adapters should call Gateway
for interactive work instead of assembling runtime turns directly.

Runtime remains the execution and persistence kernel. Gateway does not own
agent-loop behavior, provider behavior, tool semantics, permission policy,
capability selection, context assembly, or durable evidence schemas. Gateway
normalizes caller inputs and delegates execution to a backend.

The first backend is the Psychevo runtime backend. Gateway defines a backend
trait so future external peer agents can be added as agent executors without
treating them as AI providers.

## Threads, Turns, And Identity

Gateway exposes a native Thread/Turn model. For Psychevo-native threads,
`GatewayThread.id` is the runtime session id. Gateway records backend identity
separately as `backend_kind` and optional `backend_native_id`, so future
peer-agent backends can keep their native session identifiers without changing
the public thread id contract.

Source identity is distinct from thread identity. A source describes the
transport or adapter origin of input, such as CLI run, TUI session, ACP actor,
Web client, desktop window, or IM chat/thread. Gateway stores a deterministic
`source_key`, raw source identity, an optional visible label, the bound thread
id, backend identity, and lineage metadata for reset/rebind operations.

Every source declares a lifetime:

- `Invocation`: the source is recorded for the request but is not automatically
  resolved or persisted. `pevo run` uses this lifetime so the default CLI
  continuation semantics remain controlled by explicit session flags and
  `continue_latest`.
- `Process`: the source is bound only inside one `Gateway` instance. The TUI
  uses this lifetime so a long-running process can remember its current thread
  without creating durable source bindings.
- `Persistent`: the source is resolved from and written to
  `gateway_source_bindings`. ACP, future Web/Desktop surfaces, IM adapters, and
  reconnectable sources use this lifetime.

Raw source identity is not model-visible by default. A surface may provide an
explicit model-visible context input part when it wants the model to know
platform, channel, thread, or participant context.

## Input And Control

Gateway turn input is a list of transport-neutral parts. The first slice
supports text, image, and explicit context parts. Text and images map to
runtime prompt and image inputs. Context parts are included only when the
caller explicitly marks them model-visible.

Each gateway thread has at most one active turn. Normal inputs submitted while
a turn is active enter a Gateway-owned in-memory FIFO queue for the same
source/thread selector. Queued callers wait for their own turn result; Gateway
serializes execution before invoking the backend. Steer input targets the
active turn and may be updated or canceled until runtime commits it. Interrupt
aborts the active turn and clears pending in-memory control state for that
turn.

The queue is not durable in the first slice. Completed history and
`Persistent` source-to-thread mappings are durable; active process state and
`Process` source bindings are not.

Resetting a source creates or selects a new thread and rebinds the source key.
The old runtime session remains inspectable and is marked ended/archived with
the reset reason instead of being deleted.

Control APIs use a `GatewayThreadSelector` instead of a raw thread id. A
selector can target a concrete thread id or a source key. Gateway resolves the
selector against active in-memory state, process bindings, and persistent
bindings according to the source lifetime.

`clear_queue(selector)` removes queued turns for a selector and resolves their
waiting callers with a queue-cleared error. ACP cancel/stop semantics map to
`interrupt` plus `clear_queue`.

Transport-level `turn/steer` must include an `expected_turn_id`. Gateway rejects
or ignores stale steering when the expected turn id does not match the active
turn.

## Interaction Requests

Gateway owns the caller-facing interaction request semantics for permissions
and clarify/user-input requests. Runtime permission decisions remain
authoritative; Gateway only provides a request/response rendezvous.

Permission responses use the existing runtime decision vocabulary:
`allow_once`, `allow_session`, `allow_always`, and `deny`.

Clarify responses must be explicitly associated with a request id. Natural
language adapters may implement a source-aware resolver that converts the next
non-command message into a `submit_clarify` call, but Gateway treats that
as explicit request resolution rather than a new turn.

## Local Transport Facade

Gateway may expose a local transport facade for reconnectable Web, Desktop,
shell, and API clients. The first facade is local-only: it binds loopback by
default and does not create a public LAN listener, relay, TLS endpoint, or
installer service. The foreground headless process is owned by
[221 pevo Serve](../221-pevo-serve/spec.md). The managed Web Shell lifecycle is
owned by [220 pevo Gateway](../220-pevo-gateway/spec.md).

The WebSocket facade uses strict JSON-RPC 2.0 with singular resource method
names. Every request, response, and notification contains
`jsonrpc: "2.0"`, and transport payload fields use camelCase. Static product
assets and download responses may use HTTP; thread, turn, permission, clarify,
source, settings, and related commands use WebSocket requests and server
notifications.

The unauthenticated HTTP readiness endpoint is `/readyz`. It returns only
non-sensitive readiness and version information. WebSocket, download, and
detailed status routes require authentication. Direct API clients authenticate
with `Authorization: Bearer <token>`. Managed browser clients authenticate with
an HttpOnly SameSite session cookie set by the managed launch bootstrap; query
string tokens are not a supported auth mechanism.

First-slice JSON-RPC methods include:

- `initialize`
- `thread/start`
- `thread/resume`
- `thread/read`
- `thread/list`
- `thread/rename`
- `thread/archive`
- `thread/restore`
- `thread/delete`
- `turn/start`
- `turn/steer`
- `turn/interrupt`
- `source/reset`
- `permission/respond`
- `clarify/respond`
- `debug/events`

Transport-level `turn/steer` includes `expected_turn_id` and is rejected when
the supplied id does not match the active turn. `thread/resume` may resolve by
source instead of by thread id; reconnecting clients use it to recover the
current Gateway-owned snapshot after WebSocket reconnection. The snapshot is a
transport projection of the current thread transcript, active turn id, queued
turn count, and pending permission/clarify requests. It is not durable evidence.

For Web and future shell clients, the persistent source key is derived from the
canonical workdir rather than from a browser tab or device profile. Multiple
authenticated local clients for the same workdir share the same source/thread,
queue, event stream, and control surface. Client connection ids are transport
state and do not affect source continuity.

Source keys should avoid exposing raw local paths. A workdir source key uses a
stable hash of the canonical workdir, while raw identity metadata may retain
canonical and display paths for local diagnostics and UI display.

Transport requests that introduce or select a source carry a request-scoped
`scope` object inside `params`. The scope contains `workdir` plus source intent.
`source.kind` is an open namespace string such as `web`, `desktop`,
`im.platform`, or `agent.peer`. `rawId` may be omitted; Gateway derives a stable
raw id from source kind plus canonical workdir. `thread/start`, source-default
`thread/resume`, and `turn/start` require `params.scope`. Methods anchored by a
thread id or active selector authorize through the stored thread/workdir
binding. `thread/list` uses an explicit workdir filter instead of a full source
scope.

The transport protocol is generated from Rust-owned Gateway wire types. Clients
should consume generated TypeScript types and JSON Schema rather than
maintaining a hand-written second schema.

The transport facade passes session-scoped source inputs and dynamic tool
candidates to Gateway; Gateway remains responsible for validation,
normalization, conflict handling, selection, snapshotting, queueing, and
execution delegation.

## Events And Items

Gateway projects runtime observations into a Psychevo canonical item model.
The model uses a Psychevo-owned thread/turn/item contract and omits
backend-specific fields unless they are required for Psychevo semantics.

Gateway snapshots expose runtime-owned timeline items as the ordinary
transcript. Gateway events include thread lifecycle, turn lifecycle, typed
item started/updated/completed observations, permission requests and
resolutions, clarify requests and resolutions, status, warnings, and terminal
turn outcomes. Ordinary Gateway events do not include raw runtime event
fallbacks.

Debuggability is provided by bounded debug records. Raw or unclassified
runtime/provider observations may be stored as debug summaries and exposed
through `debug/events`, but they must not be delivered as ordinary
`gateway/event` notifications or ordinary transcript items.

Gateway events are live observations, not durable evidence. Durable records
remain owned by runtime and storage specs.

## IM Adapter Boundary

The first IM slice is implemented inside `psychevo-gateway` as a generic
adapter boundary and fake adapter test harness, not as concrete Slack,
Telegram, Discord, or other platform integrations. The boundary uses
deterministic source/session routing while preserving Psychevo's Gateway core:

- IM source identity records platform kind, chat id, chat type, optional
  thread id, optional user id, visible labels, and raw platform metadata.
- Gateway derives a stable persistent source key from normalized source fields
  without exposing raw local paths or raw platform identifiers in public keys.
- IM adapters submit inbound text/images/context as Gateway source-scoped
  turn inputs and receive typed timeline events for delivery.
- Task-scoped routing state is explicit in the request context; process-global
  mutable session state is not part of the boundary.
- Platform message editing, rate limits, mentions, pairing, credentials,
  webhook servers, and SDK-specific delivery behavior are deferred to future
  concrete IM specs.

## Related Topics

- [001 Architecture](../001-architecture/spec.md) defines crate boundaries and dependency direction.
- [020 Interfaces](../020-interfaces/spec.md) defines caller-facing interface semantics.
- [027 ACP](../027-acp/spec.md) defines the ACP projection boundary.
- [030 State and Data Model](../030-state-and-data-model/spec.md) defines state relationships.
- [040 Storage and Persistence](../040-storage-and-persistence/spec.md) defines SQLite persistence boundaries.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines runtime capability contribution semantics.
