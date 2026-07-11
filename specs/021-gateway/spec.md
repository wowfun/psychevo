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
- backend boundary for Psychevo runtime and future peer-agent executors
- Runtime Profile selection and native runtime identity projection, as defined
  by [052 Agent Runtimes](../052-agent-runtimes/spec.md)

Out of scope:

- concrete Web/Desktop UI behavior, Floating capsule behavior, and CLI
  lifecycle commands
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
`GatewayThread.id` is the runtime session id. For Runtime Profiles backed by
Codex, OpenCode, ACP, or future runtimes, `GatewayThread.id` remains the public
Psychevo thread id. Gateway records `runtimeRef`, backend identity, and optional
internal native-session identity in the immutable runtime binding. Public
thread/backend/session projections expose only the Psychevo thread id and an
opaque Gateway session handle; raw adapter-native ids never cross the product
contract.

Gateway HTTP session artifact downloads require an authenticated caller. Browser
surfaces may use the launch-created browser session cookie; Desktop and other
native bridges must use the owner bearer token from native code rather than
placing that token in renderer-visible URLs.

Source identity is distinct from thread identity. A source describes the
transport or adapter origin of input, such as CLI run, TUI session, ACP actor,
Web client, desktop window, or IM chat/thread. Gateway stores a deterministic
`source_key`, raw source identity, an optional visible label, the bound thread
id, optional draft Runtime Profile, and lineage metadata for reset/rebind
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
mentions resolved by the client. The first slice supports text, image, explicit
context parts, and `GatewayMention` records for visible inline references. Text
and images map to runtime prompt and image inputs. Context parts are included
only when the caller explicitly marks them model-visible.
Voice ASR/TTS and provider-native realtime requests are Gateway RPCs owned by
[248 Voice ASR/TTS](../248-voice-asr-tts/spec.md). Realtime audio frames,
partial transcripts, SDP, and output audio are live-only transport data; only
final text may enter the normal thread transcript.

Mentions keep user-visible text separate from the resolved target. A surface may
show `$reviewer`, `@src/main.rs`, or `$acp-agent` in the composer while sending
a structured mention that records the sigil, label, replacement range, target
kind, and target id/path/URI. Skill mentions are mapped to runtime explicit
skill inputs. Agent and ACP-capability mentions provide capability metadata and
disambiguation for the turn, but they do not override the explicit top-level
`agentName` used to choose the executor for the turn.

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
active turn finishes or is interrupted.

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

Transport-level `turn/steer` must include an `expected_turn_id`. Gateway rejects
or ignores stale steering when the expected turn id does not match the active
turn.

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
- `backend/doctor`
- `command/list`
- `command/execute`
- `completion/list`
- `slash/settings/read`
- `slash/settings/update`
- `peerSession/list`
- `peerSession/import`
- `thread/start`
- `thread/resume`
- `thread/read`
- `thread/list`
- `thread/browser`
- `thread/rename`
- `thread/archive`
- `thread/restore`
- `thread/delete`
- `turn/start`
- `turn/steer`
- `turn/interrupt`
- `turn/takeover`
- `source/reset`
- `permission/respond`
- `clarify/respond`

`thread/start` may start a local Psychevo thread or a top-level peer-agent
thread. Peer-agent starts target `agentName`; Gateway resolves generated and
Markdown agent definitions, validates that the definition has the `peer`
entrypoint, and routes to the referenced backend. Direct backend-id task starts
are not supported.

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

`turn/start` returns whether Gateway accepted the turn and the materialized
thread id when a thread is available. Source-started first turns may pass a null
`threadId` request; Gateway creates or resolves the human-visible thread before
returning the accepted result, so compact clients such as Floating can correlate
subsequent events and follow-up turns without first calling `thread/start`.

Transport-level `turn/steer` includes `expected_turn_id` and is rejected when
the supplied id does not match the active turn. `turn/takeover` targets a thread
or source selector; it supersedes stale durable activity directly or records a
cooperative takeover command for a still-leased foreign owner. `thread/resume`
may resolve by source instead of by thread id; reconnecting clients use it to
recover the current Gateway-owned snapshot after WebSocket reconnection. The
snapshot is a transport projection of the current thread transcript, active turn
id, queued turn count, and pending permission/clarify requests. It is not
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
raw id from source kind plus canonical cwd. `thread/start`,
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
running state. A `SessionSummaryView` carries enough display projection for
every surface to render the same row: stable id, cwd/project metadata,
title, fallback display title, preview, visible-entry count, persisted counts,
archive timestamp, and activity.

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
rows, or evidence.

`thread/browser` is the paged session-browser contract for product surfaces. By
default it groups sessions by workspace, shows sessions updated within the last
7 days, caps the initial visible set to 20 sessions per workspace, and returns a
per-workspace cursor plus hidden count for older rows. Current, running, and
explicitly included session ids remain visible even when they fall outside the
default window. Each cursor fetch returns 20 additional sessions for that
workspace without mutating session recency.

Explicit `thread/resume` may target a session from a different cwd than
the caller's current scope. In that case Gateway rebinds the caller's source to
the target session and returns a snapshot whose scope/project is the session's
stored cwd. Subsequent turns, completion, diff, files, agents, skills, and
context operations run in that resumed cwd. Browser-session authorization does
not change because it is profile-global; only the default execution scope for
that client changes. Clients must not append an old project's
history while continuing to operate in the launch directory.
Browser clients may also call `thread/start` for any canonicalizable cwd. Gateway
treats that explicit project-group action as default-scope adoption for the
client, not as a security grant. Invalid or inaccessible cwd values fail during
canonicalization or runtime safety checks rather than browser-session ACL.

Global management RPCs such as Settings, model provider catalogs, slash
settings, automation management, and session browsing are profile-level
surfaces. They may accept an optional `cwd` as a target or filter, but must not
reject a request merely because the requested cwd differs from the browser
launch directory. Execution RPCs continue to carry explicit cwd/scope and remain
bounded by runtime permission, sandbox, and tool-policy enforcement.

Gateway protocol naming follows Codex and Hermes reference products: machine
and wire fields use `cwd` for current working directory. Workbench may continue
to label the same concept as "Workspace" for humans. The old `workdir` name is not accepted
as a JSON-RPC compatibility alias because this product surface has not shipped.

The transport protocol is generated from Rust-owned Gateway wire types. Clients
should consume generated TypeScript types and JSON Schema rather than
maintaining a hand-written second schema.

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

Gateway snapshots expose message-derived transcript entries as the ordinary
transcript. Gateway events include thread lifecycle, turn lifecycle, typed
entry started/updated/completed observations, permission requests and
resolutions, clarify requests and resolutions, status, warnings, and terminal
turn outcomes. Ordinary Gateway events do not include raw runtime event
fallbacks. Raw or unclassified runtime/provider observations are ignored by the
ordinary Gateway stream unless another spec assigns them explicit typed
semantics.

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
live ids with message-derived entry ids.

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

## Runtime Host And Immutable Bindings

Gateway owns public thread identity and crosses the direct-runtime seam through
the three-method psychevo-runtime-host interface defined by
[052 Agent Runtimes](../052-agent-runtimes/spec.md). It does not expose an
adapter command bus or keep adding runtime-specific methods to GatewayBackend.

Before Gateway delivers a first prompt to a non-native runtime, it persists the
thread binding, including runtimeRef, backend/native kinds, cwd, profile
fingerprint, the complete effective execution/safety Profile snapshot, adapter
revision, ownership, and binding revision. The binding is immutable. Source
lanes may point to a new thread, but they cannot rewrite an existing thread's
runtime identity.

runtime/context/read is the coherent read interface for Composer and Channels.
It returns draft or bound selection, cached Profile choices and readiness,
three-state controls, active native-session context, and revisions. It is
cache-only and cannot spawn or contact a provider.

Runtime turns preserve the normal Gateway lifecycle and interaction model.
Every accepted turn receives exactly one terminal. Process exit closes waiters;
uncertain delivery is not retried; direct failures never fall back to native.
Runtime errors and interactions carry typed stage, retry, diagnostic, runtime,
thread, and optional child origin. RuntimeStateChanged and RuntimeChildChanged
are normalized events; raw JSON-RPC, HTTP, SSE, ids, and secrets remain private.

Stable runtime-native children receive read-only public child bindings and lazy
history. They are navigable through the same child-thread projection but are not
controllable through send, steer, stop, or agent/control unless a future
runtime contract explicitly proves that capability.

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
