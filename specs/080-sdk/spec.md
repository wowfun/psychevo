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

In-process Gateway hosts obtain Application, Client, and Gateway from the one
product composition owner defined by [001 Architecture](../001-architecture/spec.md);
they do not reopen or retain the internal state Module beside those handles.

`ApplicationBuilder` accepts one explicit inherited environment or captures
the process environment once during `build`. In the absence of a per-operation
override, Configuration, Turn, lifecycle, and import operations clone that
Application-owned base rather than rereading the process. Every derived
environment overwrites `PSYCHEVO_HOME` with the Application's authoritative
home.

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

Application defaults to at most 64 accepted or queued operations in total and
32 for one Thread. The owning `ApplicationBuilder` may replace both positive
ceilings through one typed `ApplicationLimits`; the per-Thread limit cannot
exceed the total limit. These ceilings cover Turn admissions and
start/archive/fork/compact/delete mutations. Overload is rejected before any
durable write with structured kind `application_overloaded`, scope
`application` or `thread`, the applicable limit and current occupancy, a
retryable flag, the oldest queued operation identity and age, and the affected
Thread id for a Thread-scoped rejection. Work below the ceilings preserves FIFO
order.

`Application::operational_snapshot` is a content-free, read-only observation
of open/closed state, configured ceilings, total occupancy, tracked Thread and
task counts, the oldest queued operation identity and age, and bounded
actor-panic diagnostics. It does not expose prompts, messages, tool arguments,
provider payloads, credentials, or a mutation handle. Actor panic diagnostics retain only the bounded actor
name, task id, payload summary, and captured backtrace already emitted at the
actor boundary; the runtime retains a fixed-size recent window rather than an
unbounded diagnostic log.

## Rust Crate Boundary

The dependency and ownership rules are defined by
[001 Architecture](../001-architecture/spec.md). This SDK specification does
not duplicate an exact workspace-member or dependency-edge inventory.
First-party crates consume named semantic Framework interfaces for Thread,
Turn, interaction, history, and lifecycle behavior. They do not obtain raw
state, execution options, or implementation modules through a hidden feature,
glob facade, or internal bridge. A private implementation crate may be inserted
or moved only when it preserves the dependency direction and public
distribution contract defined by 001.

`psychevo-gateway-protocol` remains a private wire-schema crate used by Gateway
and development tooling. `psychevo-gateway`, `psychevo-acp`, and `psychevo-cli`
remain private first-party crates, and the TUI remains an internal
`psychevo-cli` module.

Only `psychevo-ai`, `psychevo-agent-core`, `psychevo-extension-protocol`, and
`psychevo` are Rust crates published to crates.io. The protocol crate is a
small distribution dependency for the Framework's Extension host boundary,
not a second Framework authority or general SDK entrypoint. Their public
dependency manifests use released versions; path dependencies may additionally
be present for workspace development.

Package validation materializes and compiles every publishable workspace crate
in dependency order, patching each already-materialized archive into later
packages. Adding a publishable workspace dependency without adding it to that
topological package validation is a release-blocking error.

`psychevo-ai` is independently usable through path and git dependencies and
must produce a publish-ready crate archive. This implementation slice validates
packaging but does not publish a release.

The published `psychevo` crate has no default-enabled capability and exports
the named Framework interface. A manifest with no `[features]` table is the
minimal empty feature set; when a table exists, `default` is empty and every
other feature selects a real dependency or compiled capability. Its first-party
consumers use the same semantic interfaces; there is no `product` assembly
feature or `__product`, `__ai`, or `__agent_core` facade. Transport-owned
behavior stays in its transport crate, and Framework persistence remains
reachable only through Application, Client, Thread, and their explicitly named
extension interfaces.

`psychevo`'s default-off `native-keyring` feature is the sole Framework
platform-cost seam. It compiles the operating-system credential backend used
by `SystemMcpOAuthCredentialStore`; without it, callers may still inject any
`McpOAuthCredentialStore` through `ApplicationBuilder`. That instance-owned
store is shared by configuration views, MCP diagnostics, Agent handoffs, and
Native or child-Agent MCP launches, while a direct system-store load, save, or clear
returns an error naming the missing feature. Psychevo's first-party Gateway,
ACP, and CLI compositions enable `native-keyring` explicitly so shipped MCP
OAuth behavior is unchanged. The feature does not expose additional Framework
implementation modules or alter the semantic interface.
On Linux, this feature statically builds its DBus client dependency while still
using the host Secret Service at runtime. Building the CLI, Gateway, ACP, or
their installable artifacts therefore does not require distribution-specific
DBus development headers or a mutable system package installation step.

The repository SDK architecture gate derives the workspace dependency graph
instead of locking an exact crate or edge inventory. It preserves cycle, layer,
and distribution checks and rejects empty taxonomy features, hidden broad
facade exports, first-party adapter imports from Framework persistence or
run-assembly implementation modules, and the production module-layout
violations defined by [001 Architecture](../001-architecture/spec.md).

`psychevo` is the successor of the pre-release `psychevo-runtime` package.
There is no `psychevo_runtime` compatibility crate or crate-name alias.

## Provider SDK

`psychevo-ai` is a provider-neutral Rust SDK as well as the AI protocol Module
used by Psychevo. It owns deployment construction, credentials resolved for one
invocation, provider protocol translation, normalized capability results,
timeouts, cancellation, and provider diagnostics. It does not own agent loops,
tool execution, product provider profiles or aliases, `.env` discovery, secret
persistence, OAuth, model catalog fetching, pricing, or fallback routing.

The normal direct entrypoint is a built-in provider facade:

```rust
let openai = psychevo_ai::OpenAi::builder(config)
    .with_api_key(secret)
    .build()?;
let model = openai.responses("gpt-5")?;
let output = model.generate(request).await?;
```

The equivalent dynamic entrypoint is an immutable Registry. It resolves exact
`deployment/model` strings into capability-specific model handles. Deployment
ids are lowercase ASCII identifiers; the first slash separates deployment from
the exact provider model id, which may itself contain slashes. Registry lookup
does not infer aliases or defaults and rejects an unregistered deployment or
capability before dispatch.

Custom providers are assembled with a provider builder and capability-specific
Adapters. There is no wide provider interface:

```rust
let provider = psychevo_ai::Provider::builder(deployment)
    .language_adapter(my_language_adapter)
    .image_adapter(my_image_adapter)
    .build()?;
```

The same provider is directly usable or registerable before Registry freeze.
Built-in Adapter types for OpenAI Chat, OpenAI Responses, OpenAI Image,
Anthropic Messages, and Xiaomi voice are public and composable. Their raw wire
encoders and parsers remain private.

Public Adapter methods return boxed futures and pull-side streams so a
downstream crate can implement them without `async_trait`. Each invocation
receives a model id and an SDK-created Adapter context containing the immutable
model descriptor, optional caller-bound advisory Model Profile, endpoint,
merged safe headers, shared HTTP client, resolved credential snapshot, abort
signal, and effective timeout policy. It does not expose the credential
resolver, lifecycle handle, SDK task, queue, or output accumulator.

Language models provide streaming as the primary operation and whole-response
generation by collecting that same stream. Image, transcription, speech, and
realtime connect use eager abortable invocation futures. Every invocation
requires an ambient Tokio runtime; starting outside one returns the same handle
shape and settles with a typed runtime-unavailable error.

Provider configurations and provider-neutral requests, messages, events, and
results are serde data. Secrets, runtime objects, capability handles, and
Adapters are not serializable. Provider configuration contains no secret or
constructed HTTP client.

Credentials use named slots bound to credential references. An async resolver
resolves every configured slot exactly once at invocation start and returns one
immutable secret snapshot. A built-in facade may instead accept an explicit
redacted secret or an explicitly captured process-environment snapshot. The SDK
does not read ambient process environment implicitly.

An SDK-created HTTP client uses a ten-second connection timeout. Language and
unary calls use a 300-second progress-idle timeout by default and no total
deadline; zero disables the corresponding SDK timer. An injected HTTP client
owns its connection policy while SDK idle and total-deadline policy still
applies. Active realtime sessions have no default event-idle timeout. The SDK
does not perform generic automatic retries.

Built-in provider support is controlled by independent default-off `openai`,
`anthropic`, and `xiaomi` features. Core types, Registry, custom Adapter
interfaces, and deterministic fake Adapters for all capabilities remain
available without default features. First-party product assembly enables all
three explicitly; embedding only the provider-neutral SDK does not compile the
built-in HTTP/media families. The first release includes no real realtime
provider.

Model Profile data is caller-supplied advisory metadata. `psychevo-ai` does not
ship a volatile model inventory and does not contact `/models`. Unsupported
typed preferences are omitted with warnings; unsupported semantic requirements
such as structured output, required tool choice, input modality, or hosted tool
support fail before dispatch. Namespaced JSON extensions and per-request safe
headers are the only raw extension mechanism.

SDK errors expose a stable category, provider status and code when available,
retry-after, failure phase, a bounded safe summary, and the partial normalized
result. They do not expose credentials, authentication headers, complete
request bodies, unbounded provider bodies, or a public dispatch-certainty
claim. Built-in HTTP Adapters preserve non-success response status, bounded
provider error code, and parseable retry metadata at the HTTP boundary so
authentication and rate-limit failures cannot be downgraded to an unclassified
provider string.
Realtime command admission and command execution share the configured bounded
deadline but retain distinct failure authority: queue-admission expiry is
`Timeout`, a closed command channel is `Aborted`, and an accepted command waits
for its Adapter acknowledgement or timeout.

OpenAI and Anthropic support include credential-free high-level request
previews and canonical endpoint resolution that reuse the production encoder
and endpoint rules. Product-only token-category accounting remains in
`psychevo`; the SDK does not expose raw wire builders to support it.

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
- the Thread summary is the bounded administration read model for first-party
  surfaces: it includes parent identity, provider/model selection, start/update/
  terminal/archive facts, fork provenance, message and Tool-call counts, title,
  and the live Application Turn identity. A caller never reopens persistence
  to complete that view or issues one metadata query per listed Thread;
- human-visible Thread list and workspace-browser reads are typed Framework
  queries over that administration model. Framework owns the visibility,
  display-title fallback, backend, lifecycle-action, fork, and workspace
  grouping/ordering semantics and preserves the existing bounded keyset or
  per-workspace SQL plans; a surface receives neither Store projection records
  nor raw Thread metadata and does not issue one query per returned Thread.
  Per-Thread presentation observations likewise expose typed backend,
  relationship, and bounded Turn-start-receipt facts through `Thread`. A
  persisted presentation field with the wrong shape fails with a bounded
  structured corruption error instead of silently becoming an unavailable
  capability or default label;
- Thread identity and authoritative snapshot access plus `start_turn`,
  `respond`, `compact`, `fork`, and `archive`; an ordinary snapshot includes a
  bounded latest transcript page and its older-history cursor;
- the Application-owned title fallback converts caller-visible prompt text to
  the same bounded Thread title used by Adapter projections. Adapter-proposed
  and fallback titles pass through the same Application-owned visible-Thread
  policy: internal Threads and parent-linked child Threads are never
  auto-titled;
- `TurnHandle::receipt`, a bounded event stream, `wait`, `steer`, and
  `interrupt`; queued steer identity is a Framework-owned opaque value so a
  caller can edit or cancel the exact pending input without importing the
  agent-loop crate, compare it with the numeric identity projected in durable
  message metadata through a semantic matcher, and cancel all still-pending
  steers without retaining those identities itself; the public Application
  owner also exposes the shared queued-steer count and serialized-byte limits
  so a transport that must retain input before a `TurnHandle` is published can
  enforce the same admission contract without importing agent-core;
- a bounded `HistoryReader` for latest-page, older-page, forward replay,
  streaming search, and streaming export reads. Forward replay is keyset-paged
  after a message sequence with a default of 100 and maximum of 200 items,
  respects the current revert boundary, and isolates an undecodable persisted
  row as a typed unavailable item plus a typed page warning instead of failing
  the remaining history. Valid replay items reuse the public `ThreadItem` and
  `application::Message` vocabulary; callers receive neither raw storage rows
  nor a second message model;
- typed approval and clarify interactions exposed both as durable pending
  interactions and as optional convenience handlers.

Turn execution has exactly four ownership stages:

```text
caller TurnRequest
    -> Framework-private ResolvedTurnPlan
    -> bounded AgentSessionAdapter::prepare_turn
    -> single-use PreparedAgentTurn
    -> durable acceptance + typed AgentTurnInvocation
    -> PreparedAgentTurn::invoke
```

`TurnRequest` keeps its raw field layout private and contains caller-controlled
intent only. Callers use `new` and cohesive domain configuration methods for
input, identity, model/runtime selection, permissions, environment, and
Agent/Skill/MCP/tool selection. Framework validation consumes it once into a
private `ResolvedTurnPlan`; target/profile intent resolution and capability
assembly occur once in that stage. Bounded Adapter preparation validates or
captures the concrete execution target and returns admission facts plus a
single-use `PreparedAgentTurn`; it starts no execution or background owner.
Application calls that preparation itself from the resolved target intent.
First-party surfaces may supply semantic Agent/Profile/control intent, but they
must not prebuild `RunOptions`, read or materialize a `StateRuntime` binding, or
smuggle an already-prepared target through an opaque token on the ordinary Turn
admission path. For a new or previously unbound Thread, Application persists
the Adapter's typed `InitialAgentBinding` in the durable admission it owns.
If a source draft owns a prepared resident session, its semantic draft source
key is part of the typed new-Thread execution context so the Adapter can
promote that session without an opaque preparation reservation. This
execution-only key is distinct from the canonical durable source association
committed by the same admission and is not persisted as the Thread's source.
Durable acceptance then consumes the plan into one typed
`AgentTurnInvocation`, and Application invokes that exact prepared value once.
The invocation contains typed input parts, the immutable captured execution
target, semantic control/interaction/event and persistence handles, and no
caller request or generic option bag.

Native backend dependencies belong to the Native Adapter and are captured by
its `PreparedAgentTurn`; they are not hidden on `AgentTurnInvocation`. MCP
handoff resolution is one narrow semantic handle whose captured values are
limited to effective configuration identity, environment, selected capability
roots, and explicit MCP declarations. It stores neither `RunOptions` nor
`StateRuntime`. Native child delegation carries a semantic child-Turn template
directly into Framework admission; it never clones a parent `RunOptions` bag or
reconstructs a public `TurnRequest` from one.

The semantic `TurnControl` retains the invocation's single bounded control-input
owner for the entire `PreparedAgentTurn::invoke` lifetime, including for an
Adapter that only observes interruption and never drains steering itself.
Native execution consumes that same owner through the Framework-private side of
`TurnControl`; there is no second raw runtime-control field on the invocation.
Consequently a reconnect may continue to admit bounded steering until the
Adapter invocation actually terminates, rather than until an unused private
field happens to be dropped.

There is no `AdapterTurnOptions`, type-erased preparation payload, raw JSON
input trampoline, synchronous observer callback, `StateRuntime`, or
`RunOptions` in the Adapter contract. A value belongs to one stage only;
cross-crate `__set_*`/`__take_*` protocols and bidirectional request conversion
are forbidden. Native and ACP execution consume the same invocation shape.
An external Agent receives selected MCP configuration only through a resolved
MCP handoff value. The handoff redacts credentials from debug output and
exposes a resolved bearer token through a narrow accessor; an Adapter never
imports the provider layer's secret wrapper.

Configuration queries and mutations used by first-party surfaces are
Application-bound semantic operations. They receive the caller's cwd and
explicit Turn-level overrides, use the Application's authoritative home and
config path, and return named model/provider, permission, toolset, hook,
plugin, channel, MCP, voice, image, or workspace views. A mutation selects its
global or workspace scope semantically; callers do not manufacture a config
directory merely to cross the Framework boundary. These operations never
expose or ask a caller to manufacture `RunOptions`. Model-metadata refresh is
the same kind of Configuration operation: the caller supplies only typed
model targets, while Configuration supplies its captured home and environment.
Voice execution follows the same boundary. `Configuration` resolves effective
ASR, TTS, and realtime settings and credentials, validates provider-neutral
audio input, assembles the selected `psychevo-ai` capability, and returns only
typed Framework voice results, realtime events, and an opaque realtime control.
First-party transports never import provider SDK types, secrets, media handles,
or realtime senders. A transport owns authorization, connection-local session
identity, routing, and wire projection; Framework owns provider selection and
invocation. Realtime command methods acknowledge only after the underlying
Adapter command sink has accepted the command.
Durable Thread
administration likewise uses named `Client` and `Thread` operations for title,
context refresh, undo/redo, bounded observability-trace reads, context and usage
reads, export, and Agent mission evidence. `Thread::usage_summary` returns the
existing bounded per-Thread token,
provider/model, accounting, cache, and estimated-cost projection; no caller
obtains `StateRuntime` to perform those operations.
`Client::usage_overview` returns the existing fixed all-time, 30-day, and
7-day aggregate windows plus a caller-bounded daily activity projection; the
caller supplies only the positive activity-day count.
`Thread::agent_usage_observation` distinguishes a Native Thread from an
external Agent that has not reported context yet and, when values are present,
returns only typed token counts, context limit, and normalized USD nanodollar
cost. Framework owns binding and metadata interpretation, and surfaces receive
neither raw Thread metadata nor a durable binding row.
The same Thread owner returns the effective context limit with the existing
parent-Agent fallback, reads and writes a typed main-Agent selection, and
reads and atomically persists the composer provider/model/reasoning selection
with one Store query per read or write. Surfaces
do not interpret or perform split writes to session metadata for those
behaviors.
Thread administration also owns a typed parent/child Agent relationship read
model. It exposes relationship status and normalized Agent, task, and team
identity without leaking durable edge rows or metadata keys; lookup by child,
Agent id, or task name is one indexed persistence query rather than an
in-memory scan of every edge. A Thread reads its relationship, children, and
siblings through this owner.
The same owner exposes an optional typed Agent binding state: either the
resolved immutable binding plus whether the external session is writable,
sticky preferences, runtime-observed controls, and both revisions, or an
unresolved reason. Its compare-and-set control mutation returns that typed
state and does not expose the persistence patch type. A surface does not import
binding status/ownership storage enums or infer resolution from nullable
durable columns.
An explicit mailbox wait is likewise a Thread observation. It returns only the
typed `ready` or `timed_out` outcome defined by the Agent contract, checks for
an already-pending event before applying even a zero timeout, and neither
claims, consumes, injects, nor reveals mailbox content. CLI presentation owns
its human message; no caller opens persistence or receives a mailbox row.

Thread also owns the structural history needed to place compaction checkpoints
and failed or interrupted Turn terminals around message boundaries.
`Thread::structural_history` returns typed `ThreadCompaction` and
`ThreadTurnTerminal` values, while `structural_history_window` uses the existing
indexed persistence reads with an explicit per-kind limit and the half-open
session-sequence interval `[lower, before)`. A zero limit performs no read and
returns an empty history. The terminal window uses a matching partial index for
failed and interrupted rows, so completed Turns outside the projection cannot
turn a bounded page into a scan of the Thread's terminal history. Framework interprets terminal metadata into the first
committed sequence and structural boundary, preserves the complete compaction
and terminal metadata values needed by transcript projection, and omits raw
record identity already owned by the Thread. Surfaces never import
`SessionCompactionRecord` or `GatewayTurnTerminalRecord`, interpret camel- or
snake-case boundary keys, or scan all structural history to assemble a bounded
transcript page.
An exact checkpoint response uses `Thread::compaction(checkpoint_id)`, which
performs one indexed read and rejects a checkpoint owned by another Thread; it
does not load the Thread's complete structural history to select one row.

Persisted `ThreadItem` presentation metadata is decoded by Framework. Surfaces
read typed prompt-display and User Shell display values and never import the
durable metadata keys. The Shell projection preserves the current XML context
fallback for otherwise undecorated user-shell messages, while new writes have
one metadata representation.

Creating a temporary side conversation is a typed parent-Thread operation. It
copies the complete parent snapshot, applies the hidden inherited-message marker
and boundary prompt once, and records semantic model/mode/permission/Agent
preferences without exposing their metadata keys. Cleanup is a Client
operation scoped by canonical cwd and the known TUI or Web side-conversation
surface; callers cannot supply an arbitrary source to bulk-delete Threads.
The child Thread, inherited transcript, boundary prompt, and optional resolved
Agent binding snapshot are committed in one transaction. A failed operation
must not publish a partially initialized child Thread. The binding snapshot
retains immutable Agent/Profile identity and caller-supplied effective control
preferences while clearing runtime session identity and observed controls.
The Thread owner also evaluates automatic-compaction eligibility from a typed
context snapshot and configuration intent, supplying its own state, cwd,
Thread id, config path, and captured environment.
Interactive provider setup uses a semantic configure operation that replaces
legacy provider option fields while preserving the provider's existing model
entries; create-provider remains a distinct operation that rejects an existing
provider. Selecting the default model is one semantic mutation of the model id
and optional reasoning effort, so a surface never performs a second raw config
write to preserve reasoning UX.

Starting a standalone background Agent task is likewise an Application-bound
Client use case. The caller supplies only cwd, optional parent Thread identity,
prompt, Agent/model/policy intent, and capability inputs. Application resolves
configuration, creates or reuses the parent Thread, and supplies its own state
and supervisor; the request never carries `StateRuntime`, `RunOptions`, or an
Agent supervisor/control handle.

User Shell execution is an Application-bound Client use case but not a
Framework Turn. Its request contains command, cwd, optional Thread/continuation,
model, mode, environment, and optional active `TurnHandle` injection intent;
it contains no state or runtime-control handle. Framework returns one typed
command value with an Application-issued interrupt control and typed
start/completion/warning events. Running the command future does not spawn a
detached Framework task: TUI owns its foreground future and Gateway keeps its
existing Shell activity supervision and serialization.
The request's one effective environment governs configuration, model, sandbox,
shell discovery, and the launched child process; User Shell execution does not
reread the ambient process environment after Application captures or overrides
that environment.

`TurnRequest::with_approval` accepts only the interaction handler and whether
clarify is supported. Reviewer selection comes from effective
`approvals_reviewer` configuration; no Rust `ApprovalMode` or ignored
`approval_mode` argument crosses the Framework interface.

The high-level interface does not expose the state store, a SQLite pool,
runtime-internal run options, a Native session id, event persistence sinks, or
the Native run-loop entrypoints.

Advanced Rust integrations may provide a model Provider, a Tool, or an
`AgentSessionAdapter`. These extension points are accepted by the Application
builder and become private captured dependencies of accepted turns. They do not
grant callers access to the internal queue or state Module.

Adapter observations enter one Application-owned bounded `TurnEventStream`.
Rich first-party projection, usage, and provider metadata are typed stream
variants rather than a second callback authority. Application lifecycle events
are the single source for first-party Turn started, completed, interaction, and
terminal notifications; Adapter terminal observations are projection fences
and are not published as a second terminal.
`TurnEvent::ActivityChanged` is a host activity-state projection rather than
transcript content; Gateway consumes it for session activity, while CLI and ACP
message/tool renderers explicitly ignore it.

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
the shared Native/Gateway terminal contract. Both facts, delivery state, and
interaction kind/state remain typed across Framework and state-module
boundaries under [001 Architecture](../001-architecture/spec.md); only SQL and
wire projections use their textual spellings.

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

The accepted Turn task owns one private finalizer from active-slot
registration through completion settlement. The finalizer stages the exact
`PendingTerminal` before the first terminal persistence attempt and applies the
following order once:

1. persist the semantic terminal;
2. cancel pending permission waiters and finish the interaction broker;
3. release the Thread lane and either remove the active slot or retain that
   same `PendingTerminal`;
4. publish the terminal event only after durable commit;
5. close the event log and settle the typed completion.

Any ordinary finalization error continues through the remaining in-memory
cleanup and becomes a typed persistence or lifecycle failure; it cannot strand
the lane or waiter. If finalization panics, task-local unwinding performs the
same cleanup synchronously and retains the staged terminal for same-process
retry. A Tokio forced abort does not run the panic outcome: it leaves the
active slot to `shutdown_force_owned`, which persists the existing
`Interrupted` outcome. Cleanup operations are idempotent and recover their
private poisoned mutexes instead of panicking again.

## Accepted Turn Lifetime

Starting a turn reserves its Thread queue slot and an Application-owned
`TurnSlot` before the first accepted-Turn write. One transaction materializes
the public Thread and Turn identity, delivery intent, and retained
`clientTurnId` receipt. The slot is registered with control, event, completion,
and abort ownership before admission can close or the receipt can escape. The
slot remains `pendingAcceptance` and is excluded from public Thread activity
until that transaction commits. Commit changes it to `accepted` under the
Application runtime lock, increments the Application activity revision, and
emits the resulting complete activity snapshot. Rejection removes the
never-visible reservation. Registration also captures a non-zero one-based
`queuePosition` when the Turn does not own the physical Thread lane. The first
typed acceptance event carries this optional notification fact before the same
Turn can emit `Started`; a ready Turn omits it. Positions from independently
observed Turns are not an ordered aggregate queue snapshot.
Thread mutations share the private serialization lane but are excluded from
the public Framework Turn activity snapshot and activity revision.
Application then returns the acceptance receipt and `TurnHandle`. Accepted work
is owned by Application supervision:

- dropping a Thread, TurnHandle, event receiver, App Server connection, or
  transport never cancels accepted work;
- only an explicit interrupt, forced Application shutdown, or execution policy
  may cancel it;
- graceful shutdown closes admission, stops producers, drains accepted turns,
  flushes durable projection, shuts down Agent Session Adapters, and closes
  state last.

The same supervision rule applies to an accepted durable mutation. Dropping the
caller, result receiver, App Server request, or WebSocket does not cancel it.
Application owns the task and settles its FIFO reservation on success, error,
panic, or forced shutdown. There is no caller-owned mutation future beside the
Application task owner.

Admission is acquired before the first accepted-Turn write and held through
active-slot registration. Caller cancellation during or after the acceptance
transaction drops only that caller's receipt receiver. A caller therefore
observes either an accepted, supervised Turn whose durable delivery and receipt
facts agree, or a rejection with neither fact; shutdown cannot leave a
never-executed ghost Turn.

A surface that owns an interactive pre-acceptance operation may attach an
explicit `TurnAdmissionCancellation` to its `TurnRequest`. Cancellation while
Adapter preparation is still pending rejects the admission and releases its
gate without creating a Thread, Turn delivery, or active slot. Once slot
registration has begun, the same signal becomes an explicit Turn interrupt;
Application still completes the atomic acceptance decision and supervises the
result to a terminal state. Dropping the caller future alone retains the normal
non-cancelling lifetime contract. This phase distinction lets a surface stop
pending preparation without orphaning an acceptance that raced with cancel.

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
generation stream may losslessly coalesce adjacent incremental deltas before a
consumer observes them; provider chunk boundaries are not public identity, and
consumers rely on ordered content plus the authoritative terminal snapshot
rather than an exact delta-event count. The
durable Thread snapshot is authoritative after reconnect, lag, or event loss.

A transport reconnect can reattach to an active Turn while its Application
process remains alive and can read a durable result after completion. A process
restart cannot recreate an in-flight provider or external Agent request because
those protocols do not provide a common durable execution identity or
exactly-once resume contract. Delivery state remains durable and the Framework
does not guess by replaying an unknown request. After restart, a durable
delivery without an authoritative terminal has no process-local activity and
`Client::resume_turn` reports `OutcomeIndeterminate`. An Adapter with
authoritative history may reconcile that delivery only from evidence loaded by
a later explicit Turn, as defined by [Agent Runtimes](../052-agent-runtimes/spec.md);
the old input is never dispatched again and repeated reconciliation is a no-op.
Crash/restart validation uses real child-process termination at both durable
boundaries: after acceptance while the Turn is still queued before Adapter
invocation, and after the Adapter records unknown delivery. Neither case may
replay the accepted input after restart.

`AgentSessionAdapter::prepare_turn` completes bounded admission work and returns
one single-use `PreparedAgentTurn`; it must not start background execution that
can outlive a rejected admission. Application consumes
`PreparedAgentTurn::invoke` exactly once after durable acceptance. `invoke` and
`shutdown` are async contracts: one poll may not perform unbounded synchronous
blocking, cancellation enters through semantic `TurnControl`, and the Adapter
owns cleanup of every socket, child process, reader, and blocking worker it
creates. Application's accepted-Turn actor owns the invocation future until its
typed terminal path completes; dropping a caller never becomes an accidental
Adapter cancellation boundary. Graceful shutdown may drain accepted work
without the force deadline. Force shutdown has one ten-second total deadline
and orders work as follows:

For an existing unbound Thread, first-binding materialization is part of that
same durable acceptance: the immutable binding, initial sticky preferences,
delivery row, and optional client-Turn receipt commit in one transaction after
the process-local Turn slot is reserved. Capacity, duplicate delivery identity,
receipt serialization or persistence, and delivery failures leave all four
absent; no rejected Turn may bind a Thread. Concurrent first Turns that capture
the same binding and initial preferences converge on the single immutable
binding winner and each commit their own delivery. A conflicting capture is
rejected without committing its delivery or changing the winner.
The durable delivery records the effective runtime from that immutable binding;
an omitted runtime target inherits it. An explicit target that disagrees with
an existing binding is rejected before Adapter preparation; disagreement with
a prepared initial binding is rejected after bounded preparation but before
durable acceptance.

The same Adapter receives typed Thread lifecycle requests for archive, restore,
and delete when a Thread has an external Agent binding. Application invokes the
bounded lifecycle operation inside the Thread FIFO before committing its own
archive/restore/delete state transition. The request contains immutable Thread
and binding facts plus the current Framework-owned Agent lifecycle projection;
it contains no persistence handle, `RunOptions`, state runtime, or arbitrary
callback. The Adapter returns a typed lifecycle outcome rather than writing
Thread metadata itself. Application serializes and commits that outcome before
its local archive/restore/delete transition, so an acknowledged remote delete
can be retried without invoking the remote delete again if the local transition
fails. Adapter failure leaves the Framework transition unapplied. Archive may
additionally carry a semantic end reason, which Application writes before its
archive commit. Native and unbound Threads use the same operation with the
built-in no-op Adapter behavior.

Startup reconciliation is one Framework operation: it selects only Threads
whose durable external-delete state is already acknowledged and re-enters each
Thread's delete FIFO. Gateway neither scans all sessions nor interprets raw
metadata, and the acknowledged Adapter state prevents a second remote delete.

Lifecycle and import requests expose one Framework-owned semantic operation for
resolving a captured external Agent's requested MCP server names. Application
binds that resolver to the authoritative Thread cwd and identity plus its own
state, home, configuration path, and environment; those implementation facts
remain opaque. The Adapter supplies only the captured names and receives typed,
credential-redacting MCP handoffs. It does not construct or retain
`RunOptions`, `StateRuntime`, configuration paths, or an MCP runtime.

Agent-owned history enters the Framework through `Client`'s typed import
operation. Its request carries cwd, source, and one opaque single-use Adapter
preparation token. Application reserves an unpublished Thread, invokes
`AgentSessionAdapter` import inside that Thread's FIFO, then atomically commits
the returned immutable binding, ordered Messages with usage/metadata, optional
title, lifecycle capabilities, history ownership/fidelity, and Adapter
metadata. The Adapter interface contains no `RunOptions`, `StateRuntime`,
Gateway protocol types, repository abstraction, arbitrary callback, or
caller-supplied future.

Discovery and import deduplication query the Framework owner by captured
`(runtime profile, native session)` identity and receive at most the matching
`Thread`; a surface never scans or imports the binding table. This query is a
single indexed read and carries no binding record or nullable persistence
shape across the boundary.

Adapter/load or projection failure releases its own resident import before
returning. If the Application commit fails, or another concurrent import has
already published the same `(runtime_ref, native_session_id)`, Application
sends the Adapter's typed abort/release request. It rolls back only the local
unpublished Thread and never converts rollback into remote Agent deletion. The
successful binding winner is returned to every concurrent caller. The import
FIFO is the only mutation lane involved, so Adapter work cannot recursively
queue a second mutation on the same Thread.

An external-Agent fork is the same publication protocol, initiated from the
source `Thread` rather than an opaque discovery token. Framework reserves the
destination identity inside the source Thread FIFO, passes immutable source
Thread and binding facts plus the destination identity to the Adapter, and
atomically publishes the returned persistable Thread facts with the source as
its parent. The destination does not exist durably before the Adapter reports
ready. Import and fork share one persistable publication value and one commit
path; neither Gateway nor an Adapter creates a pending Thread or writes its
binding, lifecycle metadata, or native-session identity piecemeal. A failed
Adapter operation publishes nothing. A failed or losing commit releases the
resident destination through the typed Adapter abort operation and leaves the
Agent-owned remote session untouched.

1. close Application admission and signal every active control;
2. invoke bounded Adapter shutdown so owned sockets and children can exit;
3. join accepted tasks within the remaining deadline;
4. abort residual cancellation-safe tasks and settle their Turn slots;
5. drain pending interaction, event, and terminal work;
6. close state last.

Forced shutdown partitions one absolute deadline and reserves a final bounded
window for State close before it starts the pre-close terminal drain. Adapter,
task, Agent-terminal, and Turn-terminal work cannot consume that reserved
window. The shutdown report records Adapter shutdown and State close as
orthogonal statuses; a State close timeout makes teardown non-clean without
overwriting the Adapter result.

The shutdown report distinguishes Adapter timeout, State close timeout, task
panic, abort, pending terminal persistence, and Adapter contract violation. A
non-conforming in-process Adapter that blocks inside one poll cannot be safely
killed; the host may terminate the process, but Application must not report a
clean shutdown. A forced shutdown requested while another caller owns a
graceful drain upgrades that same drain immediately; it does not wait behind
the unbounded graceful task join. Every first-party host treats a non-clean
report as failed teardown and preserves its details in the returned error or
process failure instead of treating `Ok(ShutdownReport)` as unconditional
success. Shutdown race tests synchronize on explicit ownership and Adapter
barriers; arbitrary sleeps are not evidence that either side reached the race.

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
JSON Schema are one contract and must expose the same variant. The relay
exhaustively projects Framework events into that protocol-owned union rather
than serializing `TurnEvent` directly. Host-only `ActivityChanged`, raw
`Runtime`, and child-scoped `Scoped` observations do not enter the App Server
Turn stream; the latter cannot be flattened under the parent identity carried
by a v1 notification.

Generated JSON Schema validates the same camelCase object fields that serde
places on the wire, including fields inside tagged-enum variants. Generation
tests serialize representative values and verify their keys and required
fields against the matching schema branch.

`thread/compact` returns the protocol-owned `AppThreadCompactResult`, not a
serialized Framework persistence type. Its wire fields are `threadId`,
`compacted`, `reason`, `message`, `checkpointId`, `firstKeptSessionSeq`,
`tokensBefore`, `tokensAfter`, `summary`, `summaryProvider`, and
`summaryModel`. Optional values are present as nullable fields so Rust,
generated TypeScript, and the hand-owned Python decoder share one exact result
shape. One repository-owned valid/invalid JSON corpus covers every public
hand-owned Python `from_wire` decoder, including nested callback targets and
every closed Turn-event variant. Rust Serde, generated TypeScript schema
validation, and Python decoding must agree on fixture acceptance and on the
canonical normalized JSON. The retired snake_case Framework compaction shape
is invalid.

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

## TypeScript Client

`GatewayClient` is the public facade, not the owner of every transport concern.
Its internal connection controller exclusively owns connect/reconnect state and
timers; the pending-request registry exclusively owns request ids, deadlines,
abort listeners, and settlement cleanup; the RPC decoder exclusively validates
JSON-RPC envelopes and method results; and the browser WebSocket transport owns
only socket I/O. These modules communicate through typed results and callbacks,
not an internal event bus, and no connection fact or pending request has two
mutable owners.

TypeScript notification, connection-state, Thread-session-view, and
capability-state subscriptions deliver independently. One subscriber throwing
does not prevent later subscribers from observing the same committed state and
does not escape into transport or state-transition control flow. Subscriber
failures are reduced to a bounded message and reported through the owning
client or application diagnostic callback. A diagnostic callback failure is
contained so diagnostics cannot become a recursive client failure path.

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

`Client` is the public facade rather than a second owner of protocol state.
Internally, one connection module owns start, terminal transition, bounded
close, and the explicit no-reconnect policy; one pending-request registry owns
request ids, deadlines, caller cancellation, settlement, and cleanup; one RPC
decoder owns envelope classification and method-result validation; stdio and
WebSocket Transport adapters own only their I/O and framing; and one callback
runtime owns bounded scheduling plus typed Tool, approval, and clarify
delivery. These owners exchange direct typed calls and results, not an internal
event bus or compatibility shim. The public `Client`, `Thread`, and
`TurnHandle` interface and behavior remain unchanged.

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

The transport owns only framing, the 16 MiB limit, JSON decoding, and the
top-level object check. `_RpcClient` is the one strict JSON-RPC decoder. Each
pending request retains its method, future, and hand-owned typed result decoder,
so an invalid result fails the connection at the reader boundary instead of
escaping later as a caller-local type error. Invalid JSON-RPC envelopes,
malformed error objects, malformed known notifications, and `server/error`
notifications enter the same terminal transition. A well-formed response whose
numeric id is no longer pending is a late response and is ignored. Unknown
well-formed notifications are ignored for forward extensibility; unknown
well-formed callback requests receive JSON-RPC `-32601`. Python does not add a
second generated schema or protocol engine for these checks. Its hand-owned
public wire decoders consume the protocol-owned fixture corpus under
`packages/protocol/fixtures/` and map the camelCase values to public snake_case
Python attributes. Nested public
decoders own their field validation, so the transport does not validate and
then decode the same object twice.

Ordinary Python RPC requests have a 30-second default timeout and each request
may override it. `TurnHandle.wait()` retains long-operation semantics with no
default total timeout, while accepting an explicit timeout. A timed-out request
removes its pending correlation; a late response is discarded and the SDK does
not retry. It raises
`RequestTimeoutError(method, timeout, delivery_unknown)`, where delivery is
unknown only when the mutation may have crossed the transport boundary.

Reverse callbacks lazily start eight fixed workers when the first callback or
clarify job arrives and use a bounded backlog of 64. An idle connection that
never receives reverse work owns no callback tasks. A full queue returns an
overload JSON-RPC error for a callback request and reports a notification
overload through the event loop's exception handler. It never creates an
unbounded callback task set.

`Client.close_timeout` defaults to ten seconds. One deadline covers the
shutdown RPC, callback worker cancellation, reader termination, transport
close, and local stdio terminate-then-kill escalation. Close is idempotent and
preserves the terminal error when the bounded shutdown cannot complete.

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
the explicit features required by that binary. The two wheel-only binary
projects use one repository-owned PEP 517 implementation through thin
project-local entry modules; they do not copy metadata, platform-tag, archive,
or permission logic and do not introduce a fourth build-backend distribution.
That implementation may depend on repository-level Rust, Workbench, and
license inputs because these projects intentionally reject source
distributions. The pure Python SDK uses its standard external backend. Package
validation discovers both SDK test directories, installs the built artifacts
into a clean environment, and runs a fake-provider stdio smoke through the
installed binary and installed Python client.

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
- bounded deterministic generated-fragmentation tests for provider stream
  normalizers: changing valid byte boundaries, including boundaries inside
  UTF-8 and tool-call argument payloads, must not panic or change concatenated
  argument content, tool termination, or the single terminal outcome;
- finite table-driven tests in the owning protocol and SDK crates cover empty,
  one-byte, maximum accepted, malformed, split-UTF-8, and every generated
  boundary partition relevant to Gateway JSON round trips, provider stream
  normalization, and tool-call argument assembly. These tests enter the
  production parser and accumulator through existing fake or in-memory
  provider seams, use no randomized or mutation-based runner, and prove exact
  concatenation, lossless raw arguments, explicit parse errors, one tool-call
  termination, and one terminal generation outcome;
- one deterministic table-driven invariant over every public `TurnOutcome`
  proving that each durably accepted Turn publishes one terminal event, keeps
  one authoritative Framework terminal row, and remains idempotent across
  repeated resume;
- one real child-process crash/restart invariant that crosses durable
  acceptance and the Adapter dispatch-intent write, uses an explicit barrier
  before the first Adapter event, kills that process, and reopens the complete
  Application/Gateway composition. It proves the indeterminate pre-reconcile
  view, rejection of duplicate delivery without Adapter reinvocation, and
  evidence-backed idempotent reconciliation on the next explicit Turn without
  replaying the old input;
- standalone `psychevo --no-default-features --all-targets` compilation without
  workspace feature unification;
- Rust package and Python sdist/wheel content and install tests;
- extracted Rust package doctests and examples compile against only their
  declared published dependencies, including both provider-neutral and
  all-built-in-provider `psychevo-ai` feature sets;
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
