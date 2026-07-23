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
ACP, Web, Desktop, native Floating, IM adapters, and peer-agent backends.

## Scope

- transport-neutral thread and turn model
- source identity and source-to-thread mapping
- active-turn queue, steer, interrupt, and reset semantics
- gateway-owned permission and clarify request routing
- canonical caller-facing item and event projection
- typed live observation projection without generic raw debug persistence
- local loopback HTTP/WebSocket facade for product and API clients
- generic IM source adapter boundary for first-party Gateway integration
- Gateway application Module and Native/ACP Agent Session seam
- Runtime Profile selection and native runtime identity projection, as defined
  by [052 Agent Runtimes](../052-agent-runtimes/spec.md)

Out of scope:

- concrete Web/Desktop UI behavior, Floating capsule behavior, and CLI
  lifecycle commands
- public internet, LAN, relay, TLS, or installer service behavior
- concrete IM platform SDKs, stdio, native desktop bridge, or mobile shell
  transport adapters
- external ACP Agent implementation behind the Agent Session seam
- provider/model resolution semantics owned by runtime and provider specs
- capability selection semantics owned by runtime and capability specs

## Architecture Boundary

`psychevo-gateway` is the application kernel. Product entrypoints such as CLI,
TUI, inbound ACP, Web/Desktop, and Channels call one `ThreadApplication`
Interface instead of assembling turns, controls, interactions, or history
themselves.

The concrete turn Interface accepts typed input plus caller intent and lowers
it inside `ThreadApplication` to runtime-internal `RunOptions` and a private
queue envelope. CLI, TUI, inbound ACP, Web/Desktop, Channels, and automation
ingress must not construct either internal type or call a lower send primitive.

Gateway owns the `AgentSessionHost` seam with two production Adapters: Native
Psychevo runtime and outbound ACP Agents. `psychevo-runtime` remains the Native
execution kernel and owns Native agent-loop, provider, tool, context, and
durable-evidence semantics. Gateway owns public thread identity, immutable
binding, queueing, delivery classification, interactions, and product
projection. ACP is external at the Adapter seam and is not Gateway's internal
application Interface.

## Internal Journey Profiling

Deterministic local profiling may enable a content-free internal Gateway probe
to distinguish surface admission, the shared `Gateway::run_turn` boundary,
Native Adapter dispatch, live assistant projection, and authoritative turn
completion. The probe writes only to an explicit artifact path, uses a
process-local monotonic clock and clock-domain id, and is inert otherwise.
For Web-owned Turns it also records observed workspace-mutation delivery and
asserts that no workspace-review scan runs in the admission, event-relay, or
completion path. Runtime undo snapshots remain a shared Native stage and are
reported separately from Web projection work.

Profiling observations may include bounded request, Thread, Turn, source,
Adapter, event-kind, sequence, and queue-depth correlations. They must not
include prompt or response text, tokens, tool arguments/results, credentials,
provider request bodies, or arbitrary event payloads. The probe is not a public
Gateway event, does not alter transport schemas, and cannot become persisted
transcript or runtime evidence. Comparisons never subtract its monotonic values
from browser, TUI, fixture, or runner clocks; cross-process spans remain
runner-observed.

## Threads, Turns, And Identity

Gateway exposes one public Thread/Turn model for Native and ACP Agents.
`GatewayThread.id` is always the Psychevo public thread id. Gateway records the
captured Agent Definition, Runtime Profile, implementation kind, backend
identity, and optional internal native-session identity in an immutable
binding. Public projections expose only the public thread id and opaque Gateway
handles; raw Adapter-native ids never cross the product contract.

Each accepted Turn has one public lifecycle. Gateway emits exactly one
`TurnStarted` after admission and exactly one authoritative `TurnCompleted`
after terminal persistence and committed-entry projection. Runtime-native
`run_start`, `agent_start`, `task_started`, and ACP start observations are
internal Adapter/profiling stages and cannot create additional public starts.
`TurnStarted` carries the Gateway admission time; selected Skill evidence stays
in committed Transcript metadata rather than delaying or repeating lifecycle.

`turn/start` success returns the accepted Thread and Turn identity. Validation,
authorization, or binding failures before acceptance are JSON-RPC errors. Once
accepted, success, failure, interruption, and cancellation all terminate
through `TurnCompleted.turn`; the Web/Desktop protocol has no parallel
`turn/result` or `turn/error` terminal notification. A local event-delivery
lane preserves Turn order and may coalesce replaceable entry updates, but it
never drops or reorders lifecycle, action, entry-completed, or terminal events.
Runtime callbacks enqueue into this lane in bounded constant work and never run
filesystem, Git, Review, or auxiliary projection reads before local delivery.
The Web server owns one process-level Event Hub per `WebState`, not one durable
store tailer per WebSocket. Locally produced `gateway/event` notifications are
recorded into pending-interaction and Review projections and published to that
Hub immediately. One process-level tailer polls retained SQLite live events and
snapshots for foreign owner ids and publishes those observations to the same
Hub. The tailer uses the existing 250 ms poll interval, ten-minute retention
window, and sixty-second cleanup cadence; this delivery change does not add a
storage schema or make the durable store the local fast path.

Every WebSocket subscribes to the shared Event Hub through a bounded connection
outbox. The Hub retains at most 512 notifications. Each connection outbox
retains at most 128 frames and 8 MiB of serialized payload. A required response
larger than the byte budget may be admitted only when the outbox is empty.
Only replaceable `EntryUpdated` notifications with the same Thread, Turn, and
entry identity may replace an older queued frame in place. JSON-RPC responses,
Turn lifecycle, action lifecycle, `EntryCompleted`, terminal, voice, and shell
frames are delivery-required and preserve FIFO order. Hub lag or outbox
saturation closes the affected connection so the client performs snapshot
recovery; it never silently discards a required frame. Runtime event callbacks
perform only fixed, bounded projection and enqueue work.
This guarantee includes every error returned after the transport has accepted
and detached Turn execution: the Turn shell persists and emits one failed
`TurnCompleted`, unless a terminal for that Turn already exists, in which case
it emits no duplicate terminal.

Workspace Review is observation-based. A typed internal mutation sink accepts
exact UTF-8 before/after deltas from owned write/edit paths and opaque
invalidations from mutation-capable paths that cannot prove their file effects.
It never scans the workspace at Turn start or completion. Gateway may publish a
`workspaceChanged` event containing either a content-free observed Review group
or an opaque invalidation. File contents remain private in the bounded in-memory
Review ledger and cannot enter Gateway events, profiling, logs, or transcript.
Exact deltas support Review and Reject for add, update, delete, and move
operations. Opaque invalidations remain visibly attributable to their mutation
boundary but never claim a reversible patch or offer a false Reject action.

Gateway HTTP session artifact downloads require an authenticated caller. Browser
surfaces may use the launch-created browser session cookie; Desktop and other
native bridges must use the owner bearer token from native code rather than
placing that token in renderer-visible URLs.

Source identity is distinct from thread identity. A source describes the
transport or adapter origin of input, such as CLI run, TUI session, ACP actor,
Web client, desktop window, or IM chat/thread. Gateway stores a deterministic
`source_key`, raw source identity, an optional visible label, the bound thread
id, optional draft top-level Agent Definition and Runtime Profile, typed
control drafts, and lineage metadata for reset/rebind
operations. Backend/native identity belongs only to the thread runtime binding;
legacy source columns are migration evidence and are cleared on every new lane
write.

Every source declares a lifetime:

- `Invocation`: the source is recorded for the request but is not automatically
  resolved or persisted. `pevo run` uses this lifetime so the default CLI
  continuation semantics remain controlled by explicit session flags and
  `continue_latest`.
- `Process`: the source is bound only inside one `Gateway` instance. The TUI
  uses this lifetime so a long-running process can remember its current thread
  without creating durable source bindings.
- `Persistent`: the source is resolved from and written to
  `gateway_source_bindings`. ACP, Web/Desktop surfaces that need source
  continuation, IM adapters, and reconnectable sources use this lifetime.

Floating capsules hosted by Desktop use `source.kind = "floating"` with
per-activation raw ids and process lifetime. That lets `turn/start` materialize
and bind the first thread inside the current Gateway instance while avoiding a
durable source binding. The Phase 1 Floating product still passes explicit
thread ids for follow-up messages, and a later fresh capsule receives a new
activation raw id so it does not silently continue a prior selection-bound
thread.

Raw source identity is not model-visible by default. A surface may provide an
explicit model-visible context input part when it wants the model to know
platform, channel, thread, or participant context.

## Input And Control

Gateway turn input is a list of transport-neutral parts plus optional structured
mentions resolved by the client. The stable set supports text, image, resource,
resource link, explicit embedded context, and `GatewayMention` records for
visible inline references. Each part is faithfully lowered by the selected
Adapter or rejected before delivery; textual placeholders and silent omission
are forbidden. Context parts are included only when the caller explicitly
marks them model-visible.
Voice ASR/TTS and provider-native realtime requests are Gateway RPCs owned by
[248 Voice ASR/TTS](../248-voice-asr-tts/spec.md). Realtime audio frames,
partial transcripts, SDP, and output audio are live-only transport data; only
final text may enter the normal thread transcript.

Mentions keep user-visible text separate from the resolved target. A surface may
show `$reviewer`, `@src/main.rs`, or `$acp-agent` in the composer while sending
a structured mention that records the sigil, label, replacement range, target
kind, and target id/path/URI. Skill mentions are mapped to runtime explicit
skill inputs. Agent and ACP-capability mentions provide capability metadata and
disambiguation for the turn, but they do not override the unbound turn's
explicit `RunnableTarget`.

Each gateway thread has at most one active turn. Normal inputs submitted while
a turn is active enter a Gateway-owned FIFO queue for the same source/thread
selector. Queued callers wait for their own turn result; Gateway serializes
execution before invoking the backend. Steer input targets the active turn and
may be updated or canceled until runtime commits it. Interrupt aborts the active
turn and clears pending in-memory control state for that turn.

Gateway active state is observable across processes. The owning Gateway records
a durable activity claim with thread id when known, source key, turn id,
activity kind, owner id, generation, start/update timestamps, lease expiry, and
queued-turn count. The in-process `RunControlHandle` remains the fast path for
the owner, but every Gateway must merge local active state with durable activity
when reporting `GatewayActivityView`, mutation guards, and session summaries.
Expired leases are stale rather than authoritative. A stale owner may be
superseded by a newer generation; late completion or release from the old
generation must not clear the new owner.

Another Gateway may take over stale or cooperatively released work by claiming a
new generation and continuing from persisted transcript state plus bounded turn
intent. Takeover is continuation, not hot migration: runtime futures, provider
streams, tool processes, and `RunControlHandle`s are not moved between
processes. If continuation is impossible, Gateway exposes a bounded failure and
does not start a duplicate owner for the same generation.

Control APIs first try the local owner. For a foreign owner, Gateway records a
durable control command addressed to that activity owner. The live owner polls
and applies interrupt, steer, permission, clarify, and cooperative takeover
commands against its in-memory controls. If the owner lease expires before the
command is applied, the caller may retry through takeover or receive a bounded
stale-owner error.

Starting a new thread or resuming a history thread rebinds the source key
without archiving, ending, or deleting the previously bound thread. Historical
threads remain visible in the ordinary active history list unless the user
explicitly archives or deletes them.

Live turn projection is thread-scoped. A transport that accepts a prompt while
no source thread is bound must first create or select a concrete thread id, then
start the turn against that id. `entryStarted`, `entryUpdated`, and
`entryCompleted` events must carry transcript entries whose `threadId` is the
owning thread id; clients must not assign live entries to the currently visible
thread as a fallback.
When runtime wraps a stream event in an explicit child-thread scope, Gateway
must project that event with the scoped child thread id, not with the visible
parent thread id. The parent Agent entry may still be updated by parent-owned
Agent lifecycle/tool events, but scoped child transcript entries remain child
thread entries so clients can route or retain them without leaking them into
the parent transcript.
If an open child edge is executing inside a non-stale parent activity, a
`thread/read` for that child derives a running child view from the parent turn
and replays retained live snapshots whose thread id is the child id. The parent
activity remains bound to the parent thread. Closed edges, terminal parent
activities, and expired leases must not revive retained child overlays.
Gateway is the shared display projector for Agent invocation blocks exposed to
GUI and TUI clients. `spawn_agent` begin/end events, `agent_session_start`,
committed history, and durable parent/child edges upsert one
`AgentInvocationBlock` by `tool_call_id`; Gateway must enrich the block with
`child_thread_id` before clients replace live state with committed snapshots.
Clients must not reconstruct Agent row identity or open targets from display
labels, task summaries, prompts, result text, or agent definition names.
During live streaming, a `spawn_agent` block may be created from partial
tool-call content before its arguments are complete. That provisional block may
only be upgraded by the same `tool_call_id`, or by the same assistant-segment
stream position (`content_index` plus `call_index`) when a provider first emits
an empty or generated id and later resolves the real id. Gateway must not alias
one `spawn_agent` event to another block by tool name, agent name, task label,
prompt, result summary, or child-thread metadata. A later execution event with
an unknown id creates an independent provisional/diagnostic block instead of
stealing an existing Agent block.
Live block metadata for Agent invocations is typed state, not a generic shallow
JSON merge. A later partial tool-call frame may refresh the display preview,
but it must not overwrite an already-bound invocation identity, child-thread
target, terminal status, or result metadata from another `tool_call_id`.

Live Gateway observations are also relayed across Gateway processes. The owner
stores low-frequency boundary observations with a monotonic sequence and short
retention, and stores high-frequency transcript presentation as coalesced
latest-entry snapshots keyed by activity, turn, and entry identity. Other
Gateway servers may watch or poll both retained sources and re-emit ordinary
`gateway/event` notifications to their clients. Committed runtime messages
remain the durable transcript source of truth; retained live storage is only a
cross-process delivery buffer and may be discarded after completion.
Local interactive surfaces such as TUI may also poll retained boundary events
and latest-entry snapshots directly when they share the same state database.
They must filter events by thread/activity identity, skip observations owned by
their own Gateway process, and use durable activity leases to decide whether an
unfinished history tool call is still live or should be rendered as an
interrupted orphan.

Assistant messages whose runtime finish reason is `tool_calls`, or whose
content includes tool-call blocks, are tool-call preambles rather than final
assistant answers. Gateway projects their visible text as a non-answer
transcript entry/block with `metadata.projection = "assistant_preamble"` so the
preamble remains in observed order before the associated tool rows, while final
assistant answer entries are reserved for assistant messages that do not
continue into tool calls.
`assistant_preamble` is machine-readable projection metadata, not a user-facing
label. Clients should render it as ordinary reasoning/Thinking content and must
normalize legacy `title = "Preamble"` items to the same display.

Ordinary thread navigation is allowed while the previously bound thread has an
active turn. The active turn remains keyed by its thread id and continues in the
background. Source-scoped control resolves against the currently bound thread;
thread-scoped control can still interrupt, steer, or answer requests for a
background thread. Running threads may not be archived or deleted until their
active turn finishes or is interrupted. An idle thread may be archived or
deleted even when it is the thread currently bound to the requesting source.
Deleting that current thread clears the source binding only after the lifecycle
delete succeeds, so the source cannot remain bound to a missing Thread and a
failed Agent-owned delete keeps the current binding intact.

Resetting a source is stronger than ordinary thread navigation: it creates or
selects a new thread and rebinds the source key. The old runtime session
remains inspectable and is marked ended/archived with the reset reason instead
of being deleted.

Control APIs use a `GatewayThreadSelector` instead of a raw thread id. A
selector can target a concrete thread id or a source key. Gateway resolves the
selector against active in-memory state, process bindings, and persistent
bindings according to the source lifetime.

`clear_queue(selector)` removes queued turns for a selector and resolves their
waiting callers with a queue-cleared error. ACP cancel/stop semantics map to
`interrupt` plus `clear_queue`.

Public steering crosses `thread/action/run` with action kind `steer` and an
`expectedTurnId`. Gateway rejects or ignores stale steering when that id does
not match the active turn. Interrupt and compact use the same descriptor-gated
Thread Application action boundary; there are no parallel public turn-control
RPCs.

## Interaction Requests

Gateway owns the caller-facing interaction request semantics for permissions
and clarify/user-input requests. Runtime permission decisions remain
authoritative; Gateway only provides a request/response rendezvous.

Gateway interaction projections must carry enough context for clients to render
and answer the request without guessing from the currently visible source. A
pending request may include its materialized thread id, active turn id, durable
activity id, source key, owner id, lease expiry, and request-specific display
details. Permission requests expose the summary, reason, matched/suggested rule,
timeout, and whether persistent approval is offered. Clarify requests expose the
structured raw request plus the same routing context. Missing context is allowed
only for legacy or in-process-only requests; clients must prefer request context
over the current snapshot thread when submitting a response.

Permission responses use the existing runtime decision vocabulary:
`allow_once`, `allow_session`, `allow_always`, and `deny`.
Permission response routing must tolerate source-started turns that materialize
a concrete thread id after execution begins. If a pending permission was
registered against a source queue key, a later thread-scoped response for that
same active turn must resolve through the active thread alias instead of
returning a silent rejection.

Clarify responses must be explicitly associated with a request id. Natural
language adapters may implement a source-aware resolver that converts the next
non-command message into a `submit_clarify` call, but Gateway treats that
as explicit request resolution rather than a new turn.
Clarify response routing follows the same context precedence as permission
responses so source-started draft turns do not hang when the source binding is
not yet committed to the materialized thread.

## Local Transport Facade

Gateway may expose a local transport facade for reconnectable Web, Desktop,
shell, and API clients. The first facade is local-only: it binds loopback by
default and does not create a public LAN listener, relay, TLS endpoint, or
installer service. The foreground headless process is owned by
[221 pevo Serve](../221-pevo-serve/spec.md). Managed Web launch lifecycle is
owned by [220 pevo Gateway](../220-pevo-gateway/spec.md), while concrete Web
Shell behavior is owned by [240 pevo Web](../240-pevo-web/spec.md).

The WebSocket facade uses strict JSON-RPC 2.0 with singular resource method
names. Every request, response, and notification contains
`jsonrpc: "2.0"`, and transport payload fields use camelCase. Static product
assets and download responses may use HTTP; thread, turn, permission, clarify,
source, settings, and related commands use WebSocket requests and server
notifications.

The facade dispatches requests with bounded concurrency instead of awaiting one
handler in the socket receive loop. Each connection permits at most 32 active
requests and writes responses through one writer; responses may complete out of
order and are correlated by JSON-RPC id. Ordering belongs to the owning
Application Module: draft mutations serialize by canonical source generation,
bound Thread mutations serialize through `ThreadApplication`, and unrelated
reads or sources remain concurrent. The transport does not maintain a second
global method-classification scheduler. Completed per-request tasks are reaped
while the connection remains open; the task registry is bounded by active work,
not by the lifetime request count of a long-lived socket. When all permits are
occupied, waiting for the next permit remains concurrent with polling socket
Close/error and completed request tasks; a saturated connection therefore
disconnects promptly without waiting for one of its requests to finish.

The central JSON-RPC dispatcher owns method matching, typed parameter parsing,
calling the owning Application Module, and serializing its typed result. It
does not assemble the `thread/draft/open` or `turn/start` workflows. Those two
methods live in the existing thread Application Module, which owns their
authorization, lock ordering, runtime prewarming, event delivery setup,
asynchronous execution spawn, and response construction. This is a static
module boundary, not a dynamic handler registry or a new crate; plugin method
dispatch remains unchanged.

JSON-RPC responses and connection-private terminal, voice, and shell frames are
sent only to the initiating connection. Shared `gateway/event` notifications
flow through the process-level Event Hub. A disconnected client never has an
unknown request replayed by Gateway; it recovers authoritative Thread state
through `thread/resume` and related snapshot reads.

The unauthenticated HTTP readiness endpoint is `/readyz`. It returns only
non-sensitive readiness and version information. WebSocket, download, and
detailed status routes require authentication. Direct API clients authenticate
with `Authorization: Bearer <token>`. Managed browser clients authenticate with
an HttpOnly SameSite session cookie set by the managed launch bootstrap; query
string tokens are not a supported auth mechanism.

Native Desktop webviews do not receive the managed Gateway bearer token. The
Desktop shell owns token resolution and attaches authorization on the native
side, while renderer code sends typed bridge requests and receives routed
Gateway messages.

First-slice JSON-RPC methods include:

- `initialize`
- `agent/list`
- `agent/read`
- `agent/write`
- `agent/delete`
- `backend/list`
- `backend/write`
- `backend/doctor`
- `backend/install`
- `backend/repair`
- `backend/upgrade`
- `command/list`
- `command/execute`
- `completion/list`
- `slash/settings/read`
- `slash/settings/update`
- `thread/draft/open`
- `thread/resume`
- `thread/read`
- `thread/context/read`
- `thread/draft/prepare`
- `thread/control/set`
- `thread/action/run`
- `thread/interaction/respond`
- `thread/history/read`
- `thread/import/list`
- `thread/import`
- `thread/list`
- `thread/browser`
- `thread/rename`
- `thread/archive`
- `thread/restore`
- `thread/delete`
- `turn/start`
- `source/reset`

`thread/draft/open` atomically opens an unbound source draft, resolves either an
explicit opaque target or the Gateway default into an exact selection, prepares
that target when required, and returns one empty `ThreadSnapshot` plus its
coherent `ThreadContext`. It does not publish a durable Thread. Expected target
or preparation failures return the same snapshot and a blocked context with a
typed `RuntimeErrorView`; malformed, unauthorized, or storage-failed requests
remain RPC errors.
`turn/start` against an unbound thread supplies one Gateway-validated
`RunnableTarget`; Gateway captures the Agent Definition and Runtime Profile,
persists the immutable binding, attaches the Native or ACP Agent, and only then
delivers the prompt. Direct backend-id task starts are not supported.

`thread/import/list` is the only public discovery entrypoint for Agent-owned
sessions. It accepts a normal Gateway scope, probes enabled ACP Runtime Profiles
only after this explicit request, and returns per-Profile partial results with
opaque candidate/cursor handles. `thread/import` accepts one candidate plus a
compatible opaque `targetId`, publishes a public Thread only after Agent resume
and history replay complete, and returns its `ThreadSnapshot`. Raw ACP ids and
SDK payloads are never public protocol fields. `thread/action/run` carries the
capability-gated `fork` action for an existing bound Thread.

`completion/list` is the shared input-completion endpoint for Web, Desktop,
Mobile, and other GUI clients. It accepts `scope`, optional `threadId`, `text`,
and `cursor`, and returns ranked completion items plus the text range to
replace. The first slice recognizes `/` for shared slash commands, `@` for
cwd-local file references, and `$` for skills, local agents, and ACP
capability mentions. Completion responses are transport data only; accepting an
item does not execute a command or start a turn.

Each completion item may include display-only grouping metadata:
`group`, `groupLabel`, and `scopeLabel`. `group` is a stable bucket id such as
`commands`, `skills`, `agents`, `directories`, `files`, `capabilities`, or
`options`; `groupLabel` is the user-facing section header; `scopeLabel` is a
short source/scope badge for items such as skills and agents. Gateway should
fill these fields when it owns the candidate source, keep groups contiguous in
the returned ordering, and omit `scopeLabel` when no trustworthy source value
exists. Clients may infer a fallback group from `kind` or `target` only when
older Gateway responses omit the display fields.

`command/execute` executes shared surface commands that can be represented by
Gateway operations. Prompt-submission commands resolve to `turn/start` inputs;
session commands resolve to thread/source operations; control commands resolve
to turn control operations. Commands that require host-only side effects such as
clipboard or download may return a structured client action for the surface to
perform. Unsupported commands return structured feedback rather than silently
falling back to prompt text.

`turn/start` requires a caller-generated `clientTurnId` used only to correlate
that submission across an unknown response; it does not make the request
replayable or idempotent. Gateway persists a bounded receipt mapping the
`clientTurnId` to its Gateway-allocated `turnId` before reporting acceptance,
and `thread/read` and `thread/resume` include the retained receipts in the
authoritative Thread snapshot. `turn/start` returns whether Gateway accepted
the turn plus the required Gateway-allocated `turnId`, materialized thread id,
and authoritative Thread.
The materialized id is non-null and must equal the authoritative Thread's id;
clients fail closed instead of selecting between conflicting identities.
Source-started first turns may pass a null
`threadId` request; Gateway creates or resolves the human-visible thread before
returning the accepted result, so compact clients such as Floating can correlate
subsequent events and follow-up turns without first opening a separate draft.

`thread/action/run` steering includes `expectedTurnId` and is rejected when the
supplied id does not match the active turn. Durable activity takeover remains
an internal Gateway ownership mechanism: it may supersede stale activity or
record a cooperative command for a still-leased foreign owner, but it is not a
public client RPC. `thread/resume`
may resolve by source instead of by thread id; reconnecting clients use it to
recover the current Gateway-owned snapshot after WebSocket reconnection. The
snapshot is a transport projection of the current thread transcript, its
explicit history owner/fidelity/cursor, active turn id, queued turn count,
bounded `turn/start` receipts, and pending permission/clarify requests. It is not
durable evidence.
For source-started turns whose concrete thread id materializes before source
binding is committed, Gateway must make pending interaction requests recoverable
through the materialized thread/activity context instead of requiring clients to
rediscover them via source-default `thread/resume`.

For Web and future shell clients, browser-session authorization proves access
to the active profile and Gateway process. It is not scoped to the directory
from which the browser was launched. The launch directory is only the default
current working directory (`cwd`) for execution RPCs and project filters.
Multiple authenticated local clients may select different `cwd` values on the
same profile-global session; client connection ids are transport state and do
not affect source continuity.

Gateway thread ids remain Psychevo-local identifiers. Runtime bindings store
native session ids internally. Imported-session UX and public control APIs use
opaque Gateway session handles or `GatewayThreadSelector`; they never compose a
display value from the raw native id.

Source keys should avoid exposing raw local paths. A cwd source key uses a
stable hash of the canonical cwd, while raw identity metadata may retain
canonical and display paths for local diagnostics and UI display.

Transport requests that introduce or select a source carry a request-scoped
`scope` object inside `params`. The scope contains `cwd` plus source intent.
`source.kind` is an open namespace string such as `web`, `floating`,
`desktop`, `im.platform`, or `agent.peer`. `rawId` may be omitted; Gateway derives a stable
raw id from source kind plus canonical cwd. `thread/draft/open`,
source-default `thread/resume`, `turn/start`, and completion requests require
`params.scope`. Methods anchored by a thread id or active selector authorize
through the stored thread/cwd binding.

Session history is global across interactive surfaces. `thread/list` accepts
an optional `cwd` filter: a concrete cwd returns only that project's sessions,
while a missing or `null` cwd returns the human-visible session set across all
cwds in the local state database. Runtime `source` is an
internal persistence/runtime classification and is not part of the user-facing
session summary. Human-facing lists include top-level sessions, exclude
internal/noisy sessions such as `tui-side-conversation`, and keep empty top-level sessions
manageable instead of using message count as a visibility gate. They also
include per-session activity so multi-client shells can show background
running state. A `SessionSummaryView` is a lightweight list and action
projection: stable id, cwd/project metadata, title, fallback display title,
persisted counts, archive timestamp, activity, Agent target label, and lifecycle
action availability. It does not carry transcript preview or derived visible
entry counts; content search and transcript rendering use the authoritative
thread read instead. Availability is a product projection with an explanatory
reason; clients do not infer it from runtime names.

After the first successful turn of a newly created human-visible top-level
session, Gateway/runtime persists a concise `title` when the title is still
empty. This applies across visible interactive sources such as `run`, `tui`,
`web`, `automation`, `channel/*`, and top-level `peer_agent` sessions. Internal
side conversations, child/parent-linked sessions, resumed sessions, and failed
or aborted turns do not auto-title. Native runtime sessions may use the
configured auxiliary title-generation model and then fall back to the first user
prompt; peer-agent sessions prefer the peer-provided title and otherwise use the
prompt fallback without invoking a local title model. Title generation is
display metadata only and must not append transcript messages, tool rows, usage
rows, or evidence. For streaming interactive turns, the main Agent terminal
releases Thread activity without waiting for auxiliary title generation. Title
generation continues as detached work and publishes
`titleChanged` after the new title is persisted; its latency or failure must not
keep Session activity, the Composer interrupt state, or the per-thread turn
queue running. Non-streamed `pevo run` may continue to await its title before
returning.

`thread/browser` is the paged session-browser contract for product surfaces. By
default it groups sessions by workspace, shows sessions updated within the last
7 days, caps the initial visible set to 20 sessions per workspace, and returns a
per-workspace cursor plus hidden count for older rows. Current, running, and
explicitly included session ids remain visible even when they fall outside the
default window. Each cursor fetch returns 20 additional sessions for that
workspace without mutating session recency. The Store owns visibility filtering,
workspace grouping, ordering, and pagination. Gateway projects the selected rows
with bounded batch reads plus one in-memory activity snapshot; list latency and
Store read count scale with the returned page, not with the total candidate set.
Title fallback may use the first displayable user text without loading the full
transcript.

Explicit `thread/resume` may target a session from a different cwd than
the caller's current scope. In that case Gateway rebinds the caller's source to
the target session and returns a snapshot whose scope/project is the session's
stored cwd. Subsequent turns, completion, diff, files, agents, skills, and
context operations run in that resumed cwd. Browser-session authorization does
not change because it is profile-global; only the default execution scope for
that client changes. Clients must not append an old project's
history while continuing to operate in the launch directory.
Browser clients may also call `thread/draft/open` for any canonicalizable cwd. Gateway
treats that explicit project-group action as default-scope adoption for the
client, not as a security grant. Invalid or inaccessible cwd values fail during
canonicalization or runtime safety checks rather than browser-session ACL.

Global management RPCs such as Settings, model provider catalogs, slash
settings, automation management, and session browsing are profile-level
surfaces. They may accept an optional `cwd` as a target or filter, but must not
reject a request merely because the requested cwd differs from the browser
launch directory. Execution RPCs continue to carry explicit cwd/scope and remain
bounded by runtime permission, sandbox, and tool-policy enforcement.

Management catalogs are not turn dependencies. In particular, a Web
`turn/start` must not synchronously enumerate the Codex plugin marketplace or
repeat an installed-capability scan already prepared for the canonical cwd.
Gateway may bind a new Thread to the current prepared runtime inventory and may
initialize that Thread's delegated runtime, but unrelated catalog latency must
not delay provider dispatch or first-token delivery. Repeated turns reuse the
Thread's frozen capability and delegated-tool descriptors.

Gateway protocol naming follows Codex and Hermes reference products: machine
and wire fields use `cwd` for current working directory. Workbench may continue
to label the same concept as "Workspace" for humans. The old `workdir` name is not accepted
as a JSON-RPC compatibility alias because this product surface has not shipped.

The transport protocol is generated from Rust-owned Gateway wire types. Clients
should consume generated TypeScript types and JSON Schema rather than
maintaining a hand-written second schema.

Fixed client request signatures have one Rust-owned registry. Each registry
entry binds the wire method name to its request parameter and response result
types. That registry generates the closed Rust `ClientRequest` union and the
TypeScript `GatewayRequestParams`, `GatewayRequestResults`, and `GatewayMethod`
maps. A response that is intentionally not yet structurally typed is declared
explicitly as a JSON object in the registry instead of acquiring a fabricated
wire type. Client packages must consume these generated maps rather than repeat
the method inventory.

The request-signature registry is a compile-time protocol source, not a runtime
handler registry. Gateway keeps explicit method dispatch, authorization,
Application Module calls, and error behavior in its transport implementation.
Generating dispatcher handlers or changing JSON-RPC execution behavior is not
required by this contract.

Generated protocol validation must be free of `ts-rs` serde-attribute parse
warnings. If a Rust wire field is omitted during serialization, the generated
TypeScript type and JSON Schema must also model that field as optional.
Otherwise Gateway should serialize the field explicitly, using `null` for absent
optional values and `[]` for empty collections, so Rust JSON, generated
TypeScript, and generated schema describe the same wire shape.

The transport facade passes session-scoped source inputs and dynamic tool
candidates to Gateway; Gateway remains responsible for validation,
normalization, conflict handling, selection, snapshotting, queueing, and
execution delegation.

## Events And Transcript Entries

Gateway projects runtime observations into the Psychevo transcript entry model
defined by [250 UI Display Model](../250-ui-display-model/spec.md). The
model uses a Psychevo-owned thread/turn/entry contract and omits backend-specific
fields unless they are required for Psychevo semantics.

Gateway snapshots expose owner-derived transcript entries as the ordinary
transcript. Native history is Psychevo-authoritative. ACP history is
Agent-authoritative when load/resume is negotiated and process-ephemeral
otherwise. Every snapshot identifies owner, fidelity, resumability, and cursor;
unavailability preserves public thread metadata without fabricating content.
Gateway events include thread lifecycle, turn lifecycle, typed entry
started/updated/completed observations, interaction requests and resolutions,
status, warnings, and terminal outcomes. Ordinary events do not include raw
runtime or ACP fallbacks. Raw or unclassified observations are ignored unless
another spec assigns them explicit typed semantics.

Gateway events are live observations, not durable evidence. Durable records
remain owned by runtime and storage specs. [035 Event
Stream](../035-event-stream/spec.md) defines the canonical runtime event stream,
blocking-action lifecycle, and projection/delivery separation that Gateway
implements.

Web and GUI clients must apply typed live entry observations to their in-memory
transcript while a turn is running. `entryStarted`, `entryUpdated`, and
`entryCompleted` upsert the event entry by id; the completed entry replaces the
running entry until the committed turn slice or next snapshot refresh arrives.
Preview text and incremental updates may only update an existing live entry by
id; clients must not invent durable records from previews alone. A subsequent
`thread/read` or `thread/resume` snapshot remains authoritative and may replace
live ids with authoritative owner-derived entry ids.

When `thread/read` or explicit `thread/resume` targets a non-stale running
thread, the snapshot must include the durable `GatewayActivityView` timestamps
and a display-only replay of retained live transcript observations for that
thread. The replay overlays `entryStarted`, `entryUpdated`, and
`entryCompleted` evidence on top of persisted entries without creating durable
messages, so switching away from and back to a running session preserves active
tool rows, spinners, elapsed timers, and incremental tool output.

Gateway must project reasoning as typed live entries, not anonymous deltas.
Reasoning streams use a stable entry id for the current assistant segment, such
as `live:{turn}:reasoning:{segment}`. The first reasoning delta in that segment
starts a running reasoning entry, later deltas update the same entry, and
reasoning completion marks it completed. Assistant text for the same segment
uses a paired `live:{turn}:assistant:{segment}` entry id. The segment
increments after the assistant message closes, so later model steps do not
overwrite or move earlier Thinking/answer rows.

If a live assistant-text entry is later confirmed by `message_end` to be a
tool-call message, Gateway keeps the same assistant entry id and changes the
completed entry to a `Reasoning` block with
`metadata.projection = "assistant_preamble"`. Clients must apply that
kind/body replacement in place by id, not keep both the provisional assistant
row and the completed preamble row. Non-tool assistant completions remain
assistant text entries and must never be projected into a Thinking row.

## Agent Session Host And Immutable Bindings

Gateway owns public thread identity and crosses one `AgentSessionHost` seam
through Native and outbound ACP Adapters as defined by [052 Agent
Runtimes](../052-agent-runtimes/spec.md). It does not expose an Adapter command
bus or runtime-name-specific methods.

`AgentSessionHost.attach` captures the public thread id, binding revision, and
immutable binding fingerprints. Reattaching the same capture is idempotent;
reusing the same thread/revision for a different target is rejected before an
Adapter command. Ordering has exactly one owner: the Thread Application active
queue for Native execution and the outbound ACP process pool's resident
per-session actor for ACP execution. The Host is the identity-and-routing seam,
not a second mailbox layered over either authority.

Before Gateway delivers a first prompt, it persists the thread binding,
including Agent Definition and Runtime Profile snapshots, implementation kind,
backend reference, cwd, profile fingerprint, safety policy, Adapter revision,
ownership, and binding revision. The binding is immutable. A newly created or
resumed ACP native session id is also persisted before delivery. Source lanes
may point to a new thread, but cannot rewrite an existing thread identity.

`thread/context/read` is the coherent cache-only read for every caller. For an
unbound source it accepts an optional prospective `RunnableTarget` and scopes
input admission, controls, and sendability to that exact pair; for a bound
Thread the immutable binding is authoritative. It returns draft or bound
target, compatible choices and readiness, typed input, control and action
descriptors, sendability, history state, interactions, and
revisions. It cannot create a session or contact a provider; explicit refresh
or Doctor owns probes.

Thread Context distinguishes `selectedTargetId` from `suggestedTargetId`.
Discovery may return a suggested default but must keep selection null and
sendability false. `thread/draft/open`, `thread/draft/prepare`, and a bound
Thread return the exact selected id. Callers never infer selection or
sendability from a catalog row or Runtime Profile reference.

Runtime catalog projection uses one immutable snapshot per canonical cwd and
Gateway-owned configuration generation. A hot context/open/prepare operation
loads or receives that snapshot once and passes it through target resolution
and projection. Cache reads never materialize backend configuration, scan PATH,
write config, run Git, recursively verify a managed installation, or start a
provider. Gateway bootstrap and explicit backend management own discovery and
invalidate the snapshot; external filesystem or PATH changes require restart
or an explicit management action rather than a watcher or TTL.

`thread/draft/open` accepts an `origin` scope plus `targetIntent` of either
`default` or `exact { targetId }`; null is not a target intent. `origin` denotes
the canonical app source. An internal draft lane records its canonical parent
source explicitly and never derives a new parent by concatenating or parsing a
returned draft `rawId`. Concurrent opens allocate source generations; only the
current generation may commit preparation or later bind the canonical source.

`thread/draft/prepare` is the explicit side-effecting preparation boundary for
an unbound source and one opaque `targetId`. Native targets only replace the
source draft. ACP targets create or reuse one unpublished resident Agent
session and return its authoritative config-option projection as a complete
Thread Context. Repeating the same source/target/cwd/fingerprint is idempotent;
preparing a different target replaces and releases the prior draft. The native
draft session id is process-local, is never persisted or exposed, and is
promoted into the immutable binding by the first accepted `turn/start` without
a second `session/new`. If ACP preparation fails after selecting the draft
target, Gateway persists that blocking problem on the source lane. Subsequent
cache-only context reads keep sendability false with the same problem until an
explicit prepare retry or target replacement clears it.

`thread/control/set` names the same opaque selected target id returned by that
context.
For an unbound source, Gateway resolves and stores the complete prospective
Agent/Profile pair before returning the authoritative receipt; a Runtime
Profile id alone is not sufficient control identity. When that source owns a
prepared ACP draft, the mutation is also sent to the resident Agent and the
receipt reflects its config-option acknowledgement.

Every accepted turn receives exactly one terminal. Process exit closes
waiters; uncertain delivery is not retried; one Adapter never falls back to the
other. Errors carry typed stage, retry, delivery, diagnostic, thread, and
optional child origin. Raw ACP messages, native ids, process handles, and
secrets remain private.

Opening or reading an uncertain Thread remains cache-only. For Agent-owned
resumable history, the next explicit turn attaches and loads the Agent session,
reconciles any terminal already owned by the Agent, and then delivers only the
new input. Gateway never replays the input whose delivery was uncertain.

Capability-proven Agent-native children may receive read-only public child
bindings and lazy history. They are navigable through the same child-thread
projection but are not controllable unless a negotiated, implemented,
certified, and granted action descriptor proves that capability.

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
  turn inputs and receive typed transcript events for delivery.
- Task-scoped routing state is explicit in the request context; process-global
  mutable session state is not part of the boundary.
- Platform message editing, rate limits, mentions, pairing, credentials,
  webhook servers, and SDK-specific delivery behavior are owned by
  [028 Channels](../028-channels/spec.md) and concrete platform specs such as
  [281 WeChat Channel](../281-wechat-channel/spec.md),
  [282 Telegram Channel](../282-telegram-channel/spec.md), and
  [283 Feishu / Lark Channel](../283-feishu-lark-channel/spec.md).

## Related Topics

- [001 Architecture](../001-architecture/spec.md) defines crate boundaries and dependency direction.
- [020 Interfaces](../020-interfaces/spec.md) defines caller-facing interface semantics.
- [027 ACP](../027-acp/spec.md) defines the ACP projection boundary.
- [249 Vision and Image Artifacts](../249-vision-and-image-artifacts/spec.md)
  defines authenticated media reads for generated image artifacts.
- [030 State and Data Model](../030-state-and-data-model/spec.md) defines state relationships.
- [031 Storage and Persistence](../031-storage-and-persistence/spec.md) defines SQLite persistence boundaries.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines
  runtime capability-extension declaration and registry semantics.
- [028 Channels](../028-channels/spec.md) defines shared channel behavior.
- [281 WeChat Channel](../281-wechat-channel/spec.md),
  [282 Telegram Channel](../282-telegram-channel/spec.md), and
  [283 Feishu / Lark Channel](../283-feishu-lark-channel/spec.md) define
  first-party platform behavior.
