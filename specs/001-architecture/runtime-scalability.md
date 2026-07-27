---
name: 001. Runtime Scalability And Ownership
psychevo_self_edit: deny
---

Define the bounded-work and lifetime rules for the current pre-release runtime.
This attachment replaces implementation-shaped caches, registries, and
projections with the smallest owner that preserves existing features and UX.

## Runtime Ownership

`Application` remains the public Framework facade. Its implementation is split
only at three existing semantic owners:

- the application runtime owns accepted task supervision, per-Thread operation
  serialization, active Turn slots, pending terminal retries, and shutdown;
- the interaction broker owns durable request/response ordering and waiter
  rendezvous;
- the event log owns one bounded Turn event journal and lag/resync behavior.

These modules are private. Public Thread, Turn, result, event, request, and
adapter types stay in `application.rs`; the split must not add a second facade,
controller hierarchy, actor system, or item-level re-export layer.

## Bounded Work

Every list, history, transport, and client journal operation must have a bound
that depends on the requested result, current context window, or documented
connection capacity rather than total process lifetime.

- Last-provider-request reconstruction selects the final eligible
  assistant/prompt boundary first and reconstructs that request once.
- Projected context and compaction reads apply the latest valid compaction and
  revert boundary in Store queries before message JSON is decoded.
- Session usage is aggregated from structured usage columns in SQL after the
  same visibility boundary. It does not load every Message payload.
- Framework `thread/list` is keyset paged with a default of 50 and maximum of
  200. The opaque cursor encodes the existing stable descending sort tuple.
- App Server keeps a total connection request limit while reserving one slot
  for shutdown, interrupt, steer, and interaction response control traffic.
  Reverse-callback responses are correlation traffic rather than requests and
  never consume or wait behind that request capacity.
- Python stdio accepts at most one 16 MiB JSON line and configures the
  subprocess stream reader for that same boundary.
- Browser event journals use bounded O(1) global and per-Thread rings. A
  subscription receives only the scope it requested.

No second database, read replica, materialized usage table, outbox, global
scheduler, or process-wide actor runtime is introduced for these rules.

## Invocation And Thread Lifetimes

Resources live at the narrowest real reuse boundary:

- one plugin worker session is shared by contribution discovery, worker tools,
  and worker hooks for one invocation, then shut down once;
- one skill runtime scans and preprocesses the accepted skill roots for one
  invocation and refreshes only on an explicit skill mutation;
- one MCP runtime belongs to one materialized Framework Thread, is created
  lazily when MCP inputs are present, and is released on archive, delete, or
  Application shutdown. Empty-MCP Turns do not create registry entries. A
  successful snapshot is reused only while the complete resolved transport,
  credential, cwd, policy, source, and permission-environment identity is
  unchanged; failed startup snapshots are retried at the next safe boundary.
  An active Turn's accepted MCP bindings do not mutate;
- the Codex plugin authority keeps its private home, compatibility profile,
  response-shape validation, delivery-aware retry rules, and timeouts, but does
  not pin an exact Codex CLI patch version or send parameter-error probes for
  every method during startup.

A process-wide plugin-worker or MCP pool is not part of this contract.

## One Assembly Path

Extension assembly is a direct, invocation-scoped value built by host code.
It contains accepted source-qualified inputs for the existing owning modules.
There is no runtime registry, extension-private data store, generic contributor
slot family, or contribution projection.

Tool selection is compiled once into a canonical `ToolSelectionPlan`. Config
tools, accepted plugin tools, MCP tools, and hosted Web tools all pass through
that plan before the provider request is assembled. The plan is the single
source for direct/deferred/hidden exposure, lookup aliases, and conflict
diagnostics; a separate accepted-tool-name inventory is prohibited.

## Client Projection

Gateway protocol notifications are validated once at the TypeScript Client
boundary and delivered downstream as the generated typed notification. The
turn-start receipt is required by the protocol schema.

`ThreadSession` rejects unrelated Thread/Turn notifications before enqueueing.
It retains only current lifecycle state, not an unbounded set of historical
first-assistant Turn identifiers.

Workbench transcript state is a committed prefix plus an invocation-bounded
`liveEntries` overlay. Streaming updates mutate only the overlay. Terminal
reconciliation replaces the overlay with the committed result without cloning
or sorting the entire transcript per token.

Team status refreshes only for Team/mission/subagent lifecycle events. Refresh
is single-flight with exactly one trailing refresh when relevant facts arrive
while a request is in flight.

App Server start/resume requires a caller-generated Turn id. Python registers
the Turn sink before sending the request and therefore needs no early-event map.
Application atomically registers that identity with its Thread lane and rejects
an active or pending-terminal duplicate before durable acceptance. Every
App Server Turn event carries both Thread and Turn identity, so a resumed
interaction callback cannot observe a provisional empty Thread id.

Workbench identity subscribers read the committed Session view rather than
materializing the committed prefix plus live overlay. Transcript visibility and
virtual-layout work cache the committed projection independently; a live delta
may scan and lay out the bounded live overlay but not all retained history.
Terminal workspace-file demand is evaluated from the materialized Session view,
including any live overlay retained when the terminal has no committed slice.

## Evidence And Validation

Closest behavior tests must prove the bound or lifetime, not inspect a source
inventory. Performance claims require a before/after artifact using the same
surface-profile harness. The complete validation path is Rust broad, Web,
Desktop Rust, package/install, full visual, the current full live plan, and the
direct ACP browser live spec.
