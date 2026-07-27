---
name: 080. Framework and SDK
psychevo_self_edit: deny
---

Define Psychevo's native Framework boundary and its Rust and Python SDKs.

## Scope

- the public `psychevo` Rust crate
- one in-process Application and Client authority for Thread and Turn work
- public Thread, Turn, event, control, and interaction semantics
- the cross-process App Server used by non-Rust clients
- the async Python SDK and its client-hosted callback boundary
- Rust crate and Python distribution packaging
- protocol negotiation, transport parity, and compatibility policy

Out of scope:
- provider-specific request and response schemas
- CLI argument syntax and terminal rendering
- Web, desktop, channel, automation, and ACP wire projection
- built-in tool implementation
- OpenTelemetry, outbound telemetry, or a telemetry extension

## One Framework Authority

`psychevo::Application` is the sole process-local authority for Thread and Turn
work. It owns the state Module, accepted-work supervision, active-turn control,
interaction rendezvous, durable event projection, Native Agent execution, and
the injected external Agent Session Adapter boundary.
The builder's explicit home is authoritative for configuration, extensions,
managed tools, and state; a Turn cannot accidentally fall back to the host
process's unrelated `PSYCHEVO_HOME`.

The active Turn and queued Turn count exposed to first-party projections come
from this Application authority. Gateway does not retain a shadow Turn queue or
control registry. Interrupt, steer, compaction serialization, and mutation
guards resolve the Thread through the Framework Client before acting.

An Application yields a cheap cloneable `psychevo::Client`. Every first-party
interactive surface uses that Client:

- `pevo run` and the TUI use it in process;
- Gateway adapters use it in process for Web, desktop, channels, and
  automations;
- inbound ACP maps ACP requests to it in process;
- the App Server maps protocol requests to it for Python and other
  cross-process clients.

No first-party surface constructs runtime-internal execution options, invokes
the Native run loop directly, or owns a parallel Thread queue. A loopback
WebSocket is not used by in-process Rust callers.

The Framework's public domain model uses only Thread and Turn terminology.
Native provider session identifiers and external Agent session identifiers are
private implementation and persistence facts.

Application implements this authority through one private deep
`ApplicationRuntime`. Its internal `ThreadCell`, `TurnSlot`, and
`InteractionWaiter` records are data-only state owned by that Module, not peer
Managers, actors, public extension points, or independently supervised tasks.
The Module has one in-memory index for per-Thread admission/mutation
serialization, accepted-Turn execution and finalization, and interaction
rendezvous. Each Thread cell contains one FIFO operation queue shared by Turns
and archive, fork, and compact reservations. A queued mutation therefore waits
for earlier accepted Turns and precedes later Turn admissions without a
per-Thread actor or resident worker. An idle Thread cell is removed when it has
no running or queued Turn, mutation reservation, or waiter. Correctness must
not depend on a process-lifetime map entry per Thread.

## Rust Crate Boundary

The current source workspace has the following product crates:

```text
psychevo-agent-core -> psychevo-ai
psychevo -> psychevo-agent-core + psychevo-ai
psychevo-gateway -> psychevo + psychevo-gateway-protocol
psychevo-acp -> psychevo
psychevo-cli -> psychevo + psychevo-gateway + psychevo-acp
```

`psychevo-gateway-protocol` is a private wire-schema crate used by Gateway and
development tooling. `psychevo-gateway`, `psychevo-acp`, and `psychevo-cli` are
private product crates. The TUI remains an internal `psychevo-cli` module.

The invariant is the acyclic dependency direction
protocol/core → Framework → transport/host, not an exact workspace-member
inventory or an exact snapshot of every dependency edge. A private
implementation crate may be inserted or moved without updating an inventory
allowlist, provided it creates no reverse dependency or cycle and does not
change the public distribution contract. The product crates and direct edges
shown above describe the current required surfaces, not a prohibition on
deepening one of their implementations.

Only `psychevo-ai`, `psychevo-agent-core`, and `psychevo` are Rust SDK crates
published to crates.io. Their public dependency manifests use released
versions; path dependencies may additionally be present for workspace
development.

The published `psychevo` crate has an empty default feature set and exports the
Framework interface. Private first-party product crates enable its `product`
feature to assemble Gateway, ACP, CLI, and TUI behavior through one
`__product` facade. The feature is an unsupported workspace bridge rather than
part of the stable SDK interface. It does not make the raw `state`, `run`,
`tools`, configuration, or other implementation modules public; an external
crate that enables it can reach only the deliberately re-exported facade.

`psychevo` is the successor of the pre-release `psychevo-runtime` package.
There is no `psychevo_runtime` compatibility crate or crate-name alias.

## Rust Interface

The normal entrypoint is:

```rust
let application = psychevo::Application::builder()
    .home(home)
    .build()
    .await?;
let client = application.client();
```

The stable high-level interface contains:

- `Application::builder`, `Application::client`, and graceful or forced
  shutdown;
- `Client::start_thread`, `Client::resume_thread`, and keyset-paged,
  summary-only `Client::list_threads` with a default of 50 and maximum of 200;
- Thread identity and authoritative snapshot access plus `start_turn`,
  `respond`, `compact`, `fork`, and `archive`; an ordinary snapshot includes a
  bounded latest transcript page and its older-history cursor;
- `TurnHandle::receipt`, a bounded event stream, `wait`, `steer`, and
  `interrupt`;
- a bounded `HistoryReader` for latest-page, older-page, streaming search, and
  streaming export reads;
- typed approval and clarify interactions exposed both as durable pending
  interactions and as optional convenience handlers.

`TurnRequest` keeps its raw field layout private. Callers use `new`, cohesive
domain configuration methods for input, identity, model/runtime selection,
permissions, environment, Agent/Skill/MCP selection, and `tool`; Adapter-only
facts remain behind the private first-party bridge. This preserves every
supported input while preventing callers from depending on the storage layout
of execution policy and capability assembly.

The high-level interface does not expose the state store, a SQLite pool,
runtime-internal run options, a Native session id, event persistence sinks, or
the Native run-loop entrypoints.

Advanced Rust integrations may provide a model Provider, a Tool, or an
`AgentSessionAdapter`. These extension points are accepted by the Application
builder and become private captured dependencies of accepted turns. They do not
grant callers access to the internal queue or state Module.

Private Adapter input may carry only facts required to finish a captured
execution target, such as an already prepared ACP source key, initial
source-draft controls, and a workspace snapshot root. Adapter observations enter
one Application-owned bounded `TurnEventStream`; there is no synchronous raw
observer callback beside that stream. Rich first-party projection, usage, and
provider metadata must be represented by typed stream variants rather than a
second callback authority. Application lifecycle events are the single source
for first-party Turn started, completed, interaction, and terminal
notifications; Adapter terminal observations are projection fences and are not
published as a second terminal.

Application admission persists a delivery intent but does not confirm delivery
before the selected Adapter reaches its own dispatch boundary. The Adapter is
the authority for delivered versus unknown delivery because only it can know
whether an external request crossed that boundary. Application terminal
projection may finish a definitely delivered or definitely not-delivered row,
but it must not overwrite an Adapter-owned unknown state or erase the retained
input needed for explicit reconciliation.

Durable terminal projection keeps lifecycle status separate from execution
outcome. Public `TurnOutcome` values map to persisted Gateway terminal facts as
follows: `Completed -> (completed, normal)`, `Stopped -> (interrupted,
stopped)`, `Failed -> (failed, failed)`, and `Interrupted -> (interrupted,
aborted)`. Storing `completed` in both fields loses this distinction and breaks
the shared Native/Gateway terminal contract.

A Framework interaction id is scoped to its owning Turn. Durable identity is
the pair `(turn_id, interaction_id)`, even when an Adapter reuses a local Tool
call id such as `call_1` in another Turn. Request, resolution, cancellation,
and pending-interaction queries must never move a record between Turns or let a
prior Turn's terminal interaction state suppress a new pending interaction.

The terminal fact, delivery transition, and cancellation of still-pending
interactions form one semantic commit. Application does not publish a terminal
event or resolve `TurnHandle::wait` with a semantic outcome until that commit
succeeds. A terminal-store failure releases the Thread execution slot and
public running/control projection, retains the same-process typed result in a
private `PendingTerminal`, and resolves the current waiter with a typed
persistence failure. `resume_turn` and shutdown retry that same pending commit
once; the idempotent semantic transaction is the only terminal writer.

Psychevo deliberately does not add a recovery table, outbox, or staged terminal
payload. After process loss, a durable nonterminal delivery row is reported as
`OutcomeIndeterminate`; Application never replays the input, guesses a terminal
outcome, or fabricates Failed merely because the prior process disappeared.
Adapter failures persist their error terminal without requiring a successful
`TurnResult`. A durably committed terminal remains resumable as that exact
outcome.

## Accepted Turn Lifetime

Starting a turn reserves its Thread queue slot and an Application-owned
`TurnSlot` before the first accepted-Turn write. One transaction materializes
the public Thread and Turn identity, delivery intent, and retained
`clientTurnId` receipt. The slot is registered with control, event, completion,
and abort ownership before admission can close or the receipt can escape.
Application then returns the acceptance receipt and `TurnHandle`. Accepted work
is owned by Application supervision:

- dropping a Thread, TurnHandle, event receiver, App Server connection, or
  transport never cancels accepted work;
- only an explicit interrupt, forced Application shutdown, or execution policy
  may cancel it;
- graceful shutdown closes admission, stops producers, drains accepted turns,
  flushes durable projection, shuts down Agent Session Adapters, and closes
  state last.

Admission is acquired before the first accepted-Turn write and held through
active-slot registration. Caller cancellation during or after the acceptance
transaction drops only that caller's receipt receiver. A caller therefore
observes either an accepted, supervised Turn whose durable delivery and receipt
facts agree, or a rejection with neither fact; shutdown cannot leave a
never-executed ghost Turn.

The Thread cell is also the linearization point for archive, fork, compact, and
Turn admission. Turn execution acquires its FIFO operation permit before
constructing the Adapter execution context. That context is O(1) in transcript
size and carries Thread identity, cwd, and binding facts; an Adapter that needs
history uses the bounded `HistoryReader`. Archive, fork, compact, and a racing
Turn cannot pass independent check-then-write guards. Dropping a queued
operation removes its reservation and releases its successor; completion or
cancellation cannot strand the Thread queue.

Event receivers are bounded. A slow receiver may observe an explicit lag or
resync condition instead of applying unbounded backpressure to execution. The
durable Thread snapshot is authoritative after reconnect, lag, or event loss.

A transport reconnect can reattach to an active Turn while its Application
process remains alive and can read a durable result after completion. A process
restart cannot recreate an in-flight provider or external Agent request because
those protocols do not provide a common durable execution identity or
exactly-once resume contract. Delivery state remains durable and the Framework
does not guess by replaying an unknown request.

`AgentSessionAdapter::run_turn` and `shutdown` are async contracts: one poll may
not perform unbounded synchronous blocking, dropping/aborting the future must be
cancellation-safe, and the Adapter owns cleanup of every socket, child process,
reader, and blocking worker it creates. Graceful shutdown may drain accepted
work without the force deadline. Force shutdown has one ten-second total
deadline and orders work as follows:

1. close Application admission and signal every active control;
2. invoke bounded Adapter shutdown so owned sockets and children can exit;
3. join accepted tasks within the remaining deadline;
4. abort residual cancellation-safe tasks and settle their Turn slots;
5. drain pending interaction, event, and terminal work;
6. close state last.

The shutdown report distinguishes Adapter timeout, task panic, abort, pending
terminal persistence, and Adapter contract violation. A non-conforming
in-process Adapter that blocks inside one poll cannot be safely killed; the
host may terminate the process, but Application must not report a clean
shutdown. A forced shutdown requested while another caller owns a graceful
drain upgrades that same drain immediately; it does not wait behind the
unbounded graceful task join. Every first-party host treats a non-clean report
as failed teardown and preserves its details in the returned error or process
failure instead of treating `Ok(ShutdownReport)` as unconditional success.

## App Server

The existing `psychevo-gateway` package provides a standalone
`psychevo-app-server` binary target. It reuses the same dispatcher and generated
protocol schema for:

- newline-delimited JSON-RPC over stdin/stdout; and
- authenticated WebSocket connections exposed explicitly by Gateway.

Stdio is the default local SDK transport. Protocol output is the only stdout
content; diagnostics use stderr.

The first request must be `initialize`. Its request declares client product
version, protocol minimum and maximum, and capabilities. The response declares
server product version, the selected protocol version, the server-supported
range, and capabilities. The client then sends `initialized` before normal
requests. Version 1 accepts only the intersection `[1, 1]` and rejects a
missing or incompatible handshake with a structured error.

Each connection applies protocol-state transitions in wire receive order.
`initialize`, `initialized`, and connection-scoped registration complete their
state mutation before a later normal request is dispatched. After that ordered
prefix, independent ordinary requests and reverse-callback responses may run
concurrently. Stdio and WebSocket use the same rule.

Each inbound text frame or stdio line is decoded exactly once into either a
callback response or a typed request. Capacity classification, receive-order
handling, method policy, and dispatch consume that parsed value; they do not
parse the raw JSON again or clone request parameters. Both transports reject a
frame larger than the same 16 MiB maximum used by the Python transport.

Each connection admits at most 64 concurrent requests. Ordinary data, read, and
start requests may occupy at most 63 slots; shutdown, interrupt, steer, and
interaction-response control requests may use the reserved final slot. This
keeps control reachable during an ordinary-request flood without a global
scheduler or priority queue. A JSON-RPC response to a server-initiated
`tool/call` or `approval/request` is not a client request: it is correlated
immediately and does not consume either request quota. One
connection-local task owner continuously reaps completed requests, reverse
callbacks, and event relays while the connection remains open. Disconnect
aborts connection-local waits, callbacks, and relays, but not accepted
Application Turns or a durable mutation already adopted by Application/State.
Terminal observation removes the connection's live Turn handle. Relay ids
remain as bounded tombstones for the connection lifetime so repeated
`turn/resume` does not duplicate a relay in the cursor-less v1 protocol.
Completed tombstones use a finite FIFO capacity; active relays remain live
ownership rather than tombstones, and evicting an old completed id cannot
remove a currently active relay.

The App Server exposes typed Thread, Turn, snapshot, event subscription,
interaction response, custom-tool registration, and shutdown operations. HTTP,
WebSocket, and stdio projections must not implement a second Application.
Its public Turn-event union includes incremental `message_delta` observations
with their text payload. The Rust wire enum, generated TypeScript union, and
JSON Schema are one contract and must expose the same variant.

Generated JSON Schema validates the same camelCase object fields that serde
places on the wire, including fields inside tagged-enum variants. Generation
tests serialize representative values and verify their keys and required
fields against the matching schema branch.

One connection owns at most one event relay per Turn. Repeating `turn/resume`
returns the current receipt without replaying the Turn's retained event log
again on that connection or multiplying delivery of future events. A new
connection may establish its own single relay and then reconcile against the
authoritative snapshot.

`turn/start` and `turn/resume` require a caller-generated Turn id. The server
validates and uses that identity for relay registration before work can emit an
event; it does not allocate an unknown id after dispatch. Application reserves
the Turn identity and the per-Thread operation lane in one atomic registration.
An active or pending-terminal duplicate is rejected before durable acceptance
and cannot replace another Turn's handle. `turn/event` notifications carry both
`threadId` and `turnId`, including retained replay established by
`turn/resume`, so callback routing never depends on a provisional empty Thread
identity.

Framework Thread archive and delete own both State mutation and Thread-scoped
runtime release. Gateway and App Server lifecycle routes call those operations
instead of mutating State behind Application, so restore cannot reuse an MCP
runtime retained across archive or delete.

There is no telemetry capability, telemetry notification, or outbound
telemetry protocol. Existing explicitly enabled local journey profiles, local
aggregate skill counters, and token/accounting evidence remain local product
state rather than telemetry.

## Client-Hosted Callbacks

App Server clients may register custom tools and optional approval or clarify
handlers. Registration is connection-scoped and contains schema plus routing
metadata, never executable source.

For each turn, the App Server captures the concrete connection that supplied
the Thread handle and registrations. Callback requests for that turn route only
to that captured connection; they are never broadcast. Resuming a Thread does
not restore executable handlers. A client must explicitly reattach current
handlers before starting a new turn.

Tool callback requests carry a unique call id, tool name, validated arguments,
Thread id, and Turn id. Results or structured failures correlate by call id.
Disconnect, timeout, malformed results, or unknown calls fail the Tool
invocation without guessing a different client.

Custom-tool registration compiles the supplied JSON Schema once and rejects an
invalid or unsupported schema. Every execution validates the complete argument
value against that compiled schema before sending a reverse callback. Validation
failure is a typed Tool invocation error and never invokes client code.

Registering a reverse callback returns a private cancellation guard. Resolving
the callback disarms the guard; dropping the request future for abort,
disconnect, timeout, or task cancellation removes its pending correlation
immediately. Cleanup never waits for a late client response.

Approval and clarify facts are always durable pending interactions. A live
handler is only a convenience responder. Disconnect, timeout, or handler error
fails closed and leaves or resolves the durable interaction according to the
owning interaction spec; it never silently approves.

Clarify Tool availability is independent from convenience-handler
registration. An App Server Turn always exposes Framework durable clarify when
the selected runtime supports it. A registered handler may answer the same
pending interaction automatically; without one, the caller reads
`pending_interactions` and uses `interaction/respond`.

Application wraps every Runtime approval handler at Turn acceptance. The
wrapper registers a private waiter, durably records the typed permission
request, and only then publishes and waits. A caller may answer through async
`Thread::respond` or `TurnHandle::respond`; when a client-hosted convenience
handler is present, its result races the same Framework rendezvous.

Response handling is adopted by Application before the caller awaits its
receipt. One compare-and-set from pending to resolved persists the full tagged
Permission, Clarify answer, or Clarify cancellation payload. Only the winner
wakes the waiter, synchronously from that committed payload or its
durable-kind-defined cancellation mapping. Caller or
connection cancellation after adoption drops only the receipt; it cannot split
commit from wake. Completion, interruption, timeout, and forced shutdown race
through the same terminal cancellation transaction. Adapter-local permission
maps are not authoritative for Framework Turns. Application resolves the
durable interaction kind before committing: a typed response for the wrong kind
is rejected without changing the row, while a generic cancellation adopts and
wakes the waiter selected by the durable kind.

## Python SDK

The `psychevo` Python package requires Python 3.11 or newer and is async-only.
Its public object model mirrors the high-level Rust boundary:

```python
async with psychevo.Client() as client:
    thread = await client.start_thread(cwd=workspace)
    turn = await thread.start_turn("Inspect the repository")
    async for event in turn.events():
        ...
    result = await turn.wait()
```

The Python SDK supports:

- local stdio transport using the exact-version App Server binary dependency;
- an explicitly configured App Server executable path;
- an explicitly configured remote WebSocket URI and token;
- async custom tools and approval or clarify handlers;
- Thread listing, resume, snapshots, turns, controls, interactions, compact,
  fork, and archive.

The local stdio transport configures its subprocess stream reader and its
explicit byte check to the same 16 MiB JSON-line maximum. A larger line is a
terminal transport error rather than a partial read or `LimitOverrunError`
leaking through the SDK.

Python registers a Turn event sink under the caller-generated Turn id before it
sends `turn/start` or `turn/resume`. The response attaches the handle to that
sink. There is no unbounded early-event dictionary keyed by as-yet unknown
server Turn ids.

It does not search `PATH`, download a binary, discover or create a daemon,
connect to raw TCP or Unix sockets, load Rust through FFI, expose a Python
Provider or Agent backend, or expose arbitrary runtime hooks.

The Client has one persistent terminal error. EOF, malformed protocol,
transport failure, callback-loop failure, or a transport exception while
sending a request, notification, or callback response records that error,
fails every pending request and Turn waiter, and makes every later operation
fail immediately with the same cause. Delivery-unknown mutations are not
replayed on the half-closed transport. Turn handles are removed after their
terminal settles. The Client does not implement automatic reconnect or request
replay.

Remote WebSocket framing, masking, fragmentation, ping/pong, close, and message
size enforcement are delegated to the maintained `websockets` Python library.
The Psychevo transport owns only authentication headers, the JSON-RPC
handshake, bounded text-message decoding, and terminal error propagation; it
does not maintain a second RFC 6455 state machine.

## Python Distribution

PyPI uses three distributions with one product version:

- `psychevo`: pure Python SDK with an exact dependency on
  `psychevo-app-server-bin==V`;
- `psychevo-app-server-bin`: wheel-only platform package containing the exact
  `psychevo-app-server` executable;
- `psychevo-cli-bin`: wheel-only platform package containing `pevo`, its TUI,
  and required Workbench assets.

`psychevo[cli]` is the only CLI extra and pins `psychevo-cli-bin==V`. There is
no `telemetry` or `all` extra. The binary packages do not silently fall back to
another installed version.

The App Server binary wheel builds
`psychevo-gateway --bin psychevo-app-server --no-default-features` plus only
the explicit features required by that binary. Pure-Python packages use one
standard PEP 517 backend configuration rather than repository-local duplicate
backend implementations. Package validation discovers both SDK test
directories, installs the built artifacts into a clean environment, and runs a
fake-provider stdio smoke through the installed binary and installed Python
client.

## Compatibility

Psychevo is pre-release. This boundary replaces the old public runtime package
and Gateway application alias without a compatibility facade.

The App Server protocol version is independent from the product/package
version. An incompatible stored pre-release state schema fails with an
actionable reset or migration instruction; the product never silently deletes
state.

## Evidence and Verification

The architecture is grounded in the existing implementation:

- `crates/psychevo/src/application.rs` owns accepted Turn identity, bounded
  event delivery, reconnectable handles, interaction persistence, and ordered
  shutdown;
- `crates/psychevo-gateway/src/framework_adapter.rs` implements the private
  Native/ACP Agent Session Adapter without adding a second accepted-turn queue;
- `crates/psychevo-gateway/src/server/thread_application.rs` lowers Web,
  channel, and automation requests into the same Framework Client path;
- `crates/psychevo/src/types/run_options.rs` shows the internal
  execution options currently leaking into first-party callers;
- `crates/psychevo-acp/src/stdio/runtime_options.rs` and
  `crates/psychevo-cli/src/commands/common.rs` are concrete callers to remove;
- `crates/psychevo-gateway/src/server/rpc_dispatch.rs` and its transport modules
  are the dispatcher and transport reuse points for App Server parity.

The design also follows the source comparisons recorded in
`.local/notes/0725-sdk/codex-copilot-pi-sdk-research.md`: Codex demonstrates a
long-lived bidirectional App Server and bounded client queues, Copilot
demonstrates reverse custom-tool callbacks and generated protocol types, and Pi
demonstrates one library runtime shared by CLI, TUI, and RPC entrypoints.

Acceptance requires:

- compile-time dependency checks enforcing the crate graph above;
- deterministic fake-provider conformance tests through Rust Client, stdio App
  Server, WebSocket App Server, and Python Client;
- parity tests comparing acceptance, events, snapshots, controls,
  interactions, and completion across transports;
- callback routing, disconnect, timeout, lag/resync, reconnect, shutdown, and
  protocol-negotiation tests;
- standalone `psychevo --no-default-features --all-targets` compilation without
  workspace feature unification;
- Rust package and Python sdist/wheel content and install tests;
- all repository visual checks and all explicitly configured live checks.

## Related Topics

- [001 Architecture](../001-architecture/spec.md) defines workspace ownership
  and dependency direction.
- [020 Interfaces](../020-interfaces/spec.md) defines caller-facing invocation
  and control semantics.
- [021 Gateway](../021-gateway/spec.md) defines transport and product Adapter
  semantics over the Framework.
- [027 ACP](../027-acp/spec.md) defines ACP projection.
- [031 Storage and Persistence](../031-storage-and-persistence/spec.md) defines
  durable state boundaries.
- [035 Event Stream](../035-event-stream/spec.md) defines runtime observation
  and delivery semantics.
- [041 Permissions](../041-permissions/spec.md) and
  [115 Interactive Clarify](../115-interactive-clarify/spec.md) define the
  interaction policies projected by SDK handlers.
- [200 pevo CLI](../200-pevo-cli/spec.md),
  [210 pevo TUI](../210-pevo-tui/spec.md), and
  [230 pevo ACP](../230-pevo-acp/spec.md) define first-party Client callers.
