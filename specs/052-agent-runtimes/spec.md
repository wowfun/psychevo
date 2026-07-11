---
name: 052. Agent Runtimes
psychevo_self_edit: deny
---

# 052. Agent Runtimes

Define the Runtime Profile, runtime-host, native-session, and direct-adapter
contract shared by Gateway, Workbench, Channels, and Psychevo-managed teams.

## Scope

- one Runtime Profile identity model for native Psychevo, direct Codex, direct
  OpenCode, and ACP compatibility
- immutable public-thread runtime bindings and movable source-lane preferences
- the internal psychevo-runtime-host module and its adapter contracts
- cached runtime context, readiness, controls, sessions, and diagnostics
- direct Codex app-server and direct OpenCode serve adapters
- runtime-native history, child activity, interaction, and terminal projection
- Runtime Profile pairing with Agent Definitions and Team members

Out of scope:

- treating Codex or OpenCode as model providers
- exposing raw native ids, secrets, protocol payloads, or event streams as
  product contracts
- direct control of runtime-native child sessions that the native runtime does
  not authorize
- a Codex/OpenCode-leader bridge to Psychevo Team tools
- real provider, account, or remote service checks in deterministic default
  validation

Codex and OpenCode form one stable product milestone. A row may report a
precise unavailable or experimental state while its adapter cannot satisfy the
contract, but neither direct runtime is labelled Ready as a stable capability
until both pass the shared conformance and runtime-specific gates.

## Runtime Profiles

A Runtime Profile is the only public execution identity. Its id is runtimeRef
in Gateway, Workbench, Channels, source preferences, and Team members. Runtime
Profiles are configured under runtime_profiles.<id> in profile and project
configuration, with the existing project-over-profile deep merge.

Native execution is the singleton built-in Runtime Profile id `native`.
Configuration may refine that Profile, but a second id may not declare
`runtime = native`, and `native` may not be changed to another runtime kind.
This keeps native provider/model/permission semantics identity-safe while
custom Profiles remain available for Codex, OpenCode, and ACP.

Fields:

- label, runtime, enabled, command, args, env
- default_model, default_mode, default_agent
- approval_mode, sandbox, workspace_roots, and structured options
- backend_ref, required only when runtime is acp and forbidden for direct
  native, Codex, and OpenCode profiles

Runtime is one of native, codex, opencode, or acp. Profile validation is
fail-closed. An ACP profile whose backend_ref is missing or unknown is not
runnable. A direct profile carrying backend_ref is invalid.

Gateway generates non-persisted built-in native, codex, and opencode rows when
they are absent from effective configuration. Each enabled ACP backend that is
not explicitly referenced also receives a generated compatibility profile with
the reserved id acp:<backend-id> and label <backend-label> (ACP). Product
surfaces never use a raw backend id as runtimeRef and never depend on same-id
routing precedence.

Ordinary list, selector, and context reads are cache-only. They may inspect
configuration and cached executable discovery but must not spawn a process,
contact a provider, trigger login, or mutate runtime state. Refresh Catalog and
Doctor are explicit bounded actions.

Gateway caches runtime-host snapshots by effective Profile fingerprint and
snapshot scope. Snapshot queries carry an explicit Cached, BoundedProbe, or
CatalogRefresh mode. Ordinary reads consume only a matching cached observation
and use Cached; a cache miss is Unchecked. Doctor alone uses BoundedProbe. A
bounded probe may launch a fresh adapter and complete only its local transport
handshake; it must not enumerate a provider-backed catalog. Refresh Catalog
uses CatalogRefresh, which may perform the same bounded handshake and then read
only the adapter's stable catalog surfaces. Neither explicit mode sends a
prompt, triggers login, or repairs authentication. Resolving an executable or
hydrating a catalog alone proves only that stage and never fabricates a
correlated Stable turn.

Editing a profile changes only future thread bindings. A bound thread keeps the
effective profile fingerprint, the complete execution/safety configuration
snapshot, and adapter revision captured at bind time. Later turns reconstruct
the adapter request from that persisted snapshot rather than re-reading the
mutable Profile row. Reconnect and profile refresh must not expand that
thread's authority.

## Identity And Persistence

Gateway owns the public thread. A thread binding is persisted before the first
native prompt and is immutable for the life of that public thread.

The binding is keyed by thread_id and stores:

- runtime_ref and backend_kind
- native_kind and optional native_session_id
- canonical cwd
- profile fingerprint and profile revision
- the complete effective Profile execution/safety snapshot
- adapter kind and adapter revision
- read-write or read-only ownership
- optional parent public thread id
- monotonically increasing binding revision

The pair (runtime_ref, native_session_id) is unique when native_session_id is
present. The profile fingerprint covers every execution- and safety-relevant
effective field. Native session ids remain internal adapter identities. Public
Gateway and product contracts use a deterministic opaque sessionHandle that is
scoped by Runtime Profile and canonical cwd; they never expose, mislabel, or
accept the raw native id.

Source bindings retain only a lane-to-thread pointer and optional draft Runtime
Profile preference. A source lane may move to a newly bound thread; the old
thread remains unchanged. A pre-thread /profile use stores the draft preference.

Legacy peer_agent source data migrates only when persisted backend ownership
proves the corresponding ACP backend. It becomes acp:<backend-id>. Ambiguous
records remain unresolved and require explicit user selection.

Any legacy binding created before the complete effective Profile snapshot
existed is migrated to unresolved with an explicit snapshot-required reason.
Execution and Runtime Context fail closed; they never reconstruct that binding
from a mutable current Profile, because doing so could expand permissions.

BackendKind includes Runtime for direct runtime-host execution. Thread,
settings, and snapshot projections restore backend identity from the thread
binding; they must not project a hard-coded native backend.

## Runtime Host Module

psychevo-runtime-host is a deep internal module. Gateway crosses one interface:

- snapshot(query): cached Profile, Workspace, or Session observations;
  Cached never spawns, while the explicit BoundedProbe mode may launch and
  handshake within its fixed deadline without provider work
- execute(request, observer/control): typed Turn, Session, Interaction, Mcp,
  Control, Auth, or Extension intent
- shutdown(mode): bounded and idempotent worker or generation shutdown

A host-wide graceful or forced shutdown attempts every registered adapter even
when an earlier adapter fails, then returns the first structured failure after
all attempts complete.
Adapter-wide shutdown initiates every selected worker/generation concurrently
before awaiting any one of them. Once initiated, those shutdown tasks continue
even if an outer server deadline cancels the adapter shutdown future, so a
hanging first generation cannot strand later generations. A graceful OpenCode
shutdown retains each selected generation in the adapter registry until that
generation actually finishes shutdown. If the outer graceful deadline expires,
the following forced shutdown can therefore select, remove, and force every
remaining generation; completion cleanup is identity-checked so it cannot
remove a replacement generation launched under the same key.
The same cancellation guarantee applies to Codex's per-thread workers: one
hanging worker cannot prevent shutdown from being initiated for every other
selected worker, and a cancelled outer wait does not cancel those already
initiated worker shutdowns.

The execute observer exposes one acknowledged bindNativeSession operation.
After native thread creation or resume, an adapter must await this operation
with the runtimeRef, public thread id, canonical cwd, native session id, and
epochs before invoking turn/start, prompt_async, or any equivalent prompt
delivery. Gateway validates that provenance and atomically attaches the native
id to the already-persisted public binding. Repeating the same attachment is
idempotent even when the caller still holds the pre-attach revision; a different
native id or provenance fails closed.

Adapters sit behind this seam. GatewayBackend is not expanded into a parallel
command bus.

Snapshots carry:

- profile and capability revisions
- adapter and native runtime versions
- stability and provenance
- staged readiness and last observation time
- typed capability and control descriptors
- process, instance, binding, and generation epochs

Every public `profileRevision` and `capabilityRevision`, including
`expectedCapabilityRevision` on a control mutation, is a canonical unsigned
decimal string. Gateway parses it to u64 only after strict decimal validation,
so values above JavaScript's `Number.MAX_SAFE_INTEGER` round-trip without
precision loss. `bindingRevision` remains a JSON number because its existing
public optimistic-concurrency contract is intentionally unchanged. Workbench
keeps profile/capability revisions as strings and never coerces them through a
JavaScript number.

A direct Runtime Profile is Ready only when every observed readiness stage is
Ready, the adapter snapshot itself is Stable, and the required `turn.start`
capability is both enabled and Stable. Profile and Runtime Context results expose
the adapter stability plus the typed capability descriptors that justify that
decision. An experimental optional capability does not make an otherwise stable
adapter unavailable, but it is never promoted into the stable default path or a
Channel-safe action.

Ready also requires the complete stable matrix for that adapter revision, not
only `turn.start`. Codex requires its stable session/turn/permission baseline
plus a hydrated `model/list` catalog, native compaction, goal read/write/clear,
thread usage, account rate-limit read, and plan/diff timeline capabilities.
OpenCode requires its stable
session/turn/interaction baseline plus todo and diff timeline capabilities. A
missing, disabled, or Experimental mandatory descriptor keeps the Profile out
of Ready and names the missing capability in diagnostics.

The Codex matrix is guarded by an explicit compatibility manifest, not by a
non-empty `initialize.userAgent` or a successful basic turn. For the protocol
surface pinned by this specification, `codex_cli_rs` versions below `0.143.0`
and unparseable identities are Unsupported: the goal request family entered the
reference tree at `81b00042` and first appears in the containing
`rust-v0.143.0-alpha.10` tag. A compatible identity plus correlated turn
hydration enables the manifest; a lower or unknown version cannot promote the
complete matrix to Ready.

OpenCode uses the same explicit manifest discipline. The Stable HTTP/SSE and
session-action matrix in this specification is pinned to local reference
revision `08096b5e` / OpenCode `1.17.17`; an unparseable version or a version
below `1.17.17` is Unsupported. A non-empty health version plus a successful
basic turn is not proof of the full fork/revert/interaction matrix. Compatible
version proof and the per-capability todo/diff HTTP+SSE evidence are both
required before the complete matrix is enabled.

Codex runs `model/list` after an explicitly requested provider turn completes
or during an explicit CatalogRefresh; it does not turn a cached snapshot or
bounded Doctor probe into an online catalog refresh. Stable-turn evidence and
catalog-hydration evidence remain separate, so a catalog-only refresh cannot
claim the complete matrix. A failed or malformed catalog read leaves the
catalog stage unhydrated. Workspace/profile controls expose the validated model catalog as
choices. For the effective catalog model (the Profile default when one is
configured and is an exact visible catalog choice, otherwise the single
upstream `isDefault` row when no Profile default is configured),
the adapter also exposes exact `effort` choices from
`supportedReasoningEfforts`, the closed `none`/`friendly`/`pragmatic`
`personality` choices only when `supportsPersonality` is true, and exact
`serviceTier` choices from `serviceTiers`. It does not invent a reasoning
summary catalog or arbitrary feature controls because `model/list` exposes
neither. An exact session keeps its observed model read-only and omits
unobserved per-turn current values.

The Stable Codex policy mapping accepts only approval values `untrusted`,
`on-request`, and `never`, plus sandbox values `read-only`, `workspace-write`,
and `danger-full-access`. Upstream granular approval and workspace-root fields
are experimental and remain unavailable. An unknown value is
`policy_not_enforceable` before process spawn and the same validator keeps the
Doctor policy stage out of Ready.

Core descriptors are typed. Only namespaced adapter extensions may carry
schema-validated metadata. A cached snapshot that is stale stays explicit; it
does not invent dynamic agents, modes, auth state, or session capabilities.
`ReadOnlyCurrent` and a descriptor's `currentValue` require a value observed
from the native runtime for that exact session. Profile defaults are execution
inputs, not observed current state; when the stable native surface cannot
observe a current value, the adapter omits that control instead of presenting a
Profile default as current.

`Selectable` is likewise an observed adapter contract, not a prospective UI
catalog. In an unbound workspace context, it means the adapter can apply the
choice through the typed Turn intent. In a bound session context it remains
selectable only when the adapter also declares a stable `control.<id>.set`
capability, can apply the typed Control intent, and can read back the requested
value. Otherwise Gateway projects an observed current value read-only or omits
the control. Controls chosen for a new unbound thread are serialized by
descriptor id into `runtimeOptions`; Workbench must preserve every selected
descriptor rather than reducing the map to `mode`.

A selectable descriptor may carry one typed `dependsOn` predicate naming
another control id and exact value. Workbench shows and serializes the dependent
control only while that predicate matches, and immediately clears its pending
selection when the dependency stops matching. Codex `effort`, `personality`,
and `serviceTier` descriptors depend on `model=<controlModel>` from the same
validated catalog observation; changing model can never retain or send stale
choices from the previous model. `currentValue` remains reserved for observed
runtime state and is never abused as a dependency or default sentinel.

The global Psychevo provider model, work mode, and reasoning selector apply only
to the native Runtime Profile. They neither block a direct-runtime turn nor
cross its execution boundary. Direct model, mode, and features come only from
the immutable Profile default or an observed Runtime control serialized in
`runtimeOptions`; a native Composer selection must never silently override a
Codex or OpenCode Profile.

For the native Runtime Profile, Workbench projects the Runtime Context `mode`
descriptor through the existing shared Plan control. It does not also render
that descriptor as a second `default`/`plan` selector: the shared control is the
single editable native work-mode source and its selected value is the one sent
as the native top-level Turn mode.

The same boundary applies to ACP compatibility Profiles. `Runtime default`
means that Psychevo leaves the ACP session's runtime-reported current/default
control unchanged; it must not copy the global native model, mode, or reasoning
selection into ACP. An explicit ACP Profile default or selectable observed
control crosses the turn boundary through `runtimeOptions` and is applied with
the matching ACP session config option before prompt delivery. A forged legacy
top-level native control cannot override that Profile-owned value.

When a non-native Profile is selected, Workbench omits the native provider
model/reasoning picker, native Plan switch, and editable native permission-mode
selector instead of presenting controls that execution will ignore. The status
line shows the selected Runtime Profile's approval/sandbox policy as read-only
provenance. Adapter-declared controls remain in the Runtime Profile control
cluster.

The shared conformance suite runs the native, ACP, Codex, OpenCode, and fake
adapters through the same interface and verifies cache-only snapshot reads,
binding-before-prompt, exactly-one terminal, no fallback, bounded shutdown,
epoch isolation, and safety-policy narrowing.

## Public Gateway Contract

runtime/context/read returns one coherent view for a source and optional thread:

- draft or bound Runtime Profile selection
- runnable Profile choices and provenance
- cached readiness and diagnostic reference
- control descriptors with Runtime default, read-only current, or selectable
  state
- active native-session summary and immutable binding revision
- persisted runtime-native child summaries for reconnect discovery

Persisted runtime-native child summaries may expose an optional `status` only
when it is the short, schema-safe product status previously projected into the
public child thread metadata. Gateway drops malformed or oversized values and
never forwards an adapter event object, native payload, or native session field
through this property. The scalar must match `[A-Za-z][A-Za-z0-9_-]{0,63}`.

For an unbound Composer draft, the request may name a prospective `runtimeRef`.
Gateway resolves that Profile's matching cached snapshot without spawning and
returns only its observed controls; a cache miss returns no invented controls.
A bound thread rejects a different prospective Runtime Profile because its
runtime identity is immutable.

Existing turn/start, interaction, source, and session actions retain product
semantics. Internal runtime-host intents and raw protocol methods are not public.

Runtime management includes:

- runtime/profile/list, read, write, delete, and setEnabled
- runtime/snapshot and runtime/health/check
- runtime/session/list, read, attach, resume, archive, unarchive, rename, and
  delete
- capability-gated runtime/session/fork, revert, and unrevert
- runtime/control/set with expected capability and binding revisions
- capability-gated runtime/auth actions
- capability-gated runtime/goal/read, runtime/goal/set, and runtime/goal/clear
- capability-gated runtime/account/rateLimits/read

Goal RPCs resolve the public thread first and derive the Runtime Profile, native
thread id, canonical cwd, and binding revision exclusively from that immutable
binding. They do not accept a caller-selected runtimeRef or native session id.
`runtime/goal/set` accepts only the closed goal-status enum plus typed objective
and token-budget updates; clearing a token budget is an explicit boolean rather
than an adapter-shaped null payload. Account rate-limit reads may select an
unbound direct Codex Profile explicitly, or derive it from an optional bound
thread; a supplied runtimeRef that disagrees with that binding fails closed.
The four methods invoke only the strict `codex.goal` and `codex.account`
runtime-host extensions and return product DTOs, never a public generic command
or extension bus.

Successful goal and account rate-limit reads or mutations persist the same
schema-validated product DTOs used by native observations. A subsequent
runtime/context/read exposes them as optional `goal` and `accountRateLimits`
fields for the bound thread. Goal clear removes the cached goal only after the
native clear result confirms success. No native thread, request, event, or
account credential identifier enters these results or Runtime Context.

The ambiguous runtime/session/rollback method does not exist. Control changes
are serialized. Gateway commits the new value only after the adapter observes
the requested value; stale revisions or epochs fail without mutation.
An adapter without a stable observed mutation surface returns a structured
`unsupported` error and exact guidance; equality with the already-observed value
is a no-op, not a fabricated successful mutation.

Authentication actions cross the typed Auth intent. Codex supports stable
`account/read`, managed login start, login cancel, and logout operations; Codex
continues to own and persist every credential. Gateway may return safe login
metadata such as an authorization URL, login id, verification URL, or user code,
but never returns API keys or tokens. OpenCode provider authentication remains
CLI-owned until a bounded probe observes a compatible stable native contract;
before then the Auth intent fails explicitly with `opencode auth login` guidance
and never reports placeholder success.

Workbench `Repair auth` first reads authoritative status. When Codex reports
`login_required`, Workbench offers an explicit managed-login action, renders only
the allowlisted safe URL/code metadata returned by Gateway, and lets the user
cancel that login by its opaque login id. It never collects credentials itself.

Session results carry an opaque sessionHandle, cursor, opaque dedup key, Full,
Summary, or Partial history fidelity, read-only or active ownership, public
parent-thread identity when resolved through a persisted binding, and
capability-gated actions. A persisted runtime-native child may additionally
carry the same optional schema-safe public `status` returned by Runtime Context;
ordinary native-session enumeration does not invent a status. History gaps
remain visible. Active native sessions
attach read-only unless the adapter proves they are idle and safely transferable;
otherwise the product offers Fork when supported.

`runtime/session/attach` is the explicit Gateway-owned read-only path for a
Direct root session currently observed as Active. It accepts only the opaque
`sessionHandle`, re-reads and validates the exact native session and canonical
cwd, and never invokes a native takeover mutation. Gateway then creates or
reuses one public root thread with an immutable read-only binding, imports the
validated history idempotently, moves the requesting source lane to that public
thread, and returns its public `threadId`. Active unbound sessions advertise the
Gateway-derived `attach` action only when native read is available. A repeated
attach reuses the same read-only binding. `runtime/session/resume` continues to
reject Active sessions and never substitutes for attach.

Direct-runtime revision points cross the public contract only as
`RuntimeSessionRevisionView` values. Each view contains a Gateway-derived opaque
`revisionHandle`, the safe message role, and an optional creation time; it never
contains the runtime's native message, part, or event id. Gateway derives the
handle from the Runtime Profile, canonical cwd, validated native session, and a
native message id observed in that session's freshly read history. Revert accepts
only this `revisionHandle`, resolves it back to a native message id internally
against another validated session read, and rejects unknown, stale, cross-session,
or cross-workspace handles without invoking the mutation. The existing opaque
`sessionHandle` contract is unchanged. Unrevert accepts no revision handle.
`runtime/session/read` accepts a Gateway-derived opaque history cursor and
returns `nextCursor`. Gateway resolves that handle to the adapter cursor only
inside the validated session-read path; a native message boundary used as an
adapter cursor is never exposed or accepted directly. Revision views are
derived only from the validated history page that produced the returned
cursor.
Session-list, history-cursor, and revision handles for unbound native sessions
are registered in a bounded Gateway-lifetime lookup. Normal pagination and
revision actions resolve in constant time regardless of history depth; there is
no fixed native-page search limit. After Gateway restart or bounded-cache
eviction, an unrecognized handle fails as stale with reload guidance. Durable
bound-thread `sessionHandle` resolution remains backed by the persisted binding
and is unchanged.

OpenCode exposes revision views only when its stable session read and staged
revert/unrevert operations are available. Codex does not advertise or execute
revert/unrevert because its stable app-server contract has no equivalent session
operation. Public revision RPC parameters reject raw `itemId`, `messageID`, or
other undeclared native-id fields rather than ignoring them.

Runtime errors contain code, stage, retryClass, safe user text, and a diagnostic
reference. Defined classes include missing, auth required, unsupported, stale
revision, stale epoch, busy, process exit, event gap, and policy not enforceable.

RuntimeStateChanged and RuntimeChildChanged are typed Gateway events.
Interactions include runtime, profile, public-thread, parent, and child origin.
Stable auxiliary runtime observations are also typed: `PlanUpdated`,
`DiffUpdated`, `UsageUpdated`, `GoalChanged`, `CompactionChanged`, and
`AccountRateLimitsUpdated`. Usage distinguishes total and last-turn token
breakdowns. Goal and
rate-limit DTOs expose only schema-validated product fields; sparse rolling
rate-limit notifications merge non-null fields into the last full account
snapshot and never erase previously observed account metadata. Plan steps use a
closed pending/in-progress/completed/cancelled status set.
Every runtime interaction also carries a typed interaction kind, stability, and
exposure policy. Product callers set the highest exposure allowed for the turn;
they must not infer policy from adapter labels, prompts, native event names, or
opaque metadata. `Standard` interactions may reach ordinary GUI and Channel
surfaces. `GuiAdvancedOnly` interactions may be projected only when the turn
was created by a real GUI Advanced context. No current Workbench or Channel
turn creates that context, so those interactions fail closed: Gateway declines
the native request before publishing an action, emits only a safe cancellation
notice, and preserves terminal progress. It must never hide an interaction
while leaving the native runtime waiting for a response.
Question interactions carry an ordered typed question list. Each question keeps
its text, option labels and descriptions, multiple-selection, custom-answer,
and secret-input policy; adapters must not replace multiple native questions
with one prompt or reconstruct them from opaque metadata. Gateway projects this
typed list unchanged to Shared Attention. Workbench returns one answer array per
question in the same order and supports both single- and multiple-selection
questions plus custom answers. Channels keep the one-question `/answer` flow,
but a request with multiple questions must direct the user to Shared Attention
in the Psychevo GUI instead of submitting a partial first-question answer.
Lifecycle, interaction, and terminal events are lossless under backpressure.

## Turn And Safety Invariants

- Persist the immutable binding before native thread/start or prompt delivery.
- Persist and acknowledge the native session id after native create/resume and
  before turn/start or prompt_async. A post-bind delivery error retains that id,
  and the next explicit turn resumes it instead of creating another session.
- Every accepted turn produces exactly one terminal.
- An uncertain prompt delivery is never automatically resent.
- Direct runtime failure never falls back to native Psychevo.
- A process exit or protocol EOF wakes every waiter and closes each accepted
  turn with one failed terminal.
- Only an explicit overload rejection may be retried automatically.
- Native events from an old process, instance, binding, or generation epoch
  cannot mutate current state.
- Adapter terminal metadata stays internal. Durable and public terminal
  projection retains only the product-safe message plus classified code,
  stage, retry class, and diagnostic reference; native event and message ids
  are never copied into transcript metadata.
- Failed direct-runtime results carry that safe classification in a typed
  terminal-error field on the runtime-host result. Gateway projects only that
  field; it never derives public failure details by searching adapter metadata
  or native terminal payloads. A failed result without a typed terminal error
  is treated as an adapter-contract violation and receives a generic safe
  classification rather than exposing the unclassified payload.
- Runtime tool observations cross the public Gateway boundary with a derived
  opaque tool-call id and an allowlisted product detail shape. The adapter's
  raw item/state payload and native item, message, session, or event ids never
  enter live events or durable transcript metadata.
- Safety policy translation is exact, narrowed, or unsupported. It never
  expands tool, filesystem, network, approval, or sandbox authority.

## Direct Codex Adapter

Each active public/native Codex thread owns one codex app-server --stdio worker.
The adapter orders initialize, initialized, thread/start or thread/resume, then
turn/start. It uses one stdout reader, buffers early notifications, demultiplexes
requests and notifications, and retains bounded stderr diagnostics.

The stable adapter covers:

- thread create, resume, list, read, rename, archive, unarchive, delete, and fork
- turn start, steer, interrupt, and terminal observation
- model, permission profile, feature, goals, compaction, usage, rate-limit, and
  supported command, file, and additional-permission flows

The corresponding stable capability ids are `thread.compact`,
`thread.goal.read`, `thread.goal.set`, `thread.goal.clear`, `thread.usage`,
`account.rate_limits.read`, `timeline.plan`, and `timeline.diff`. Goal operations
cross the namespaced `codex.goal` extension only as strict read/set/clear
schemas; account rate limits cross `codex.account` only as strict read output.
Unknown fields, statuses, and malformed numeric values fail closed rather than
being forwarded as native JSON.

`thread.compact` calls native `thread/compact/start`, but its empty RPC response
is only an acknowledgement. The runtime-host compaction result completes only
after the matching native `contextCompaction` item completes. Process exit or
protocol EOF wakes the compaction waiter with a typed transport failure; an ack
alone must never report successful compaction. Plan, unified diff, thread token
usage, goal, compaction lifecycle, and account rate-limit updates are cached and
projected through the typed observation matrix.

Codex `requestUserInput` and per-turn collaboration mode remain experimental
capabilities. They are version-gated Advanced features rather than part of the
stable adapter claim. Until a real GUI Advanced turn context exists,
`requestUserInput` is declined through Codex's native typed response with an
empty answer map; its prompt and questions are not projected to ordinary Shared
Attention. When it is eventually enabled by a real Advanced context, the
adapter requires exactly one answer row per native question, enforces single-
and multiple-selection cardinality, and projects native `autoResolutionMs` as
the interaction expiry. An ordinary turn that selects Codex plan collaboration
mode is rejected before worker launch or prompt delivery with GUI Advanced
guidance; Gateway never sends experimental `collaborationMode` from a Standard
turn.

The Stable adapter initializes Codex with `experimentalApi: false`. Profile
`workspace_roots` are rejected before worker launch because upstream exposes
`thread/start.runtimeWorkspaceRoots` only through the experimental API. The
stable full-access shape uses the ordinary `sandbox = danger-full-access`
thread policy and never substitutes the experimental named `permissions`
field. These profile shapes remain unavailable until a real GUI Advanced
execution context and version gate exist.

The adapter does not invent plan approval. It excludes unsandboxed
thread/shellCommand. Rate limits are account metadata, not readiness.
Deprecated numTurns rollback is not projected.

Codex session mutations first read native session metadata and require its cwd
to match the Gateway request's canonical cwd. A mismatch fails at the binding
stage before the mutation method is sent.

Codex history may be lossy and is labelled with the observed fidelity. A stable
native child identity becomes a read-only Gateway child as soon as it is known;
history loads on demand. Direct child start, steer, and stop remain unavailable
when app-server rejects child control. Archive and delete confirmation names the
descendant cascade.

## Direct OpenCode Adapter

OpenCode workers are leased generations keyed by executable, structured
args/env/options, profile revision, and adapter version. Cwd is not part of the
generation key because serve routes each request by directory.

The adapter forces --hostname 127.0.0.1 --port 0 --no-mdns, generates a
process-scoped OPENCODE_SERVER_PASSWORD, uses Basic authentication, and verifies
the returned loopback URL and runtime version after launch. Secrets are never
projected or logged.

Before prompt delivery, the adapter establishes global SSE, observes
server.connected, then hydrates messages, status, children, permissions, and
questions for the directory instance. Once the root and current children are
known, the same pre-prompt hydration reads each session's stable `session.todo`
snapshot and the `session.diff` snapshot for its latest observed user message,
when one exists; a session without a prior user message begins with an empty
diff. Buffered `todo.updated` and `session.diff` events then reconcile over those
snapshots before prompt delivery, matching OpenCode's own HTTP-plus-event
synchronization model. Durable events and sync duplicates are deduplicated.
Each Gateway turn owns a native user message id and accepts only assistant and
terminal events correlated to that parent id.

Process epoch and per-directory instance epoch are distinct. Instance disposal
or restart reconciles agents, models, MCP state, and pending interactions.
In-memory permission or question requests from the old instance expire.
Todo and diff observations are accepted only from the current process,
directory instance, and the active root or one of its known children. An old
instance event or an event for another session cannot overwrite the current
timeline. The adapter emits a typed plan or diff observation only when it has
exact public turn/thread provenance. An active root turn is projected under its
public thread; a hydrated native-child snapshot is retained for reconciliation
but is never mislabeled as a parent-thread update while no public child-turn
provenance exists. Raw native session ids, native event ids, and native payloads
never cross that observation interface.

Gateway keeps only the latest correlated plan and diff observation for the
active direct-runtime turn and persists those typed product observations with
the final assistant message. Transcript projection renders them as completed
Plan and Diff evidence beside the final answer. The terminal committed-entry
replacement and a later session reload therefore expose the same observations;
they must not depend on a transient live-stream overlay, retain an obsolete
earlier update, or contain raw native ids or payloads.

Gateway emits one public TurnStarted lifecycle event for a direct-runtime turn
before any live assistant, plan, or diff observation. This binds the optimistic
prompt to the accepted public turn. Terminal committed entries replace that
prompt rather than duplicating it; if a reconnect misses TurnStarted, a matching
unbound optimistic prompt is still superseded by the committed user entry.

The stable adapter covers:

- dynamic visible primary/all agents and subagent catalog
- exact session-observed provider/model as read-only current state, plus todos,
  diff, fork, staged revert/unrevert, and abort
- child sessions, child permission, and child question routing
- stale terminal suppression after bounded abort

Stable enabled capability rows `timeline.todos` and `timeline.diff` mean both
the pre-prompt HTTP snapshots and correlated SSE updates passed adapter
validation. A runtime that cannot satisfy either half must not advertise that
timeline capability as Stable.

Hidden agents are excluded from selectors. Stable history uses session.messages
plus global SSE, but is labelled Partial until every non-text native part used
by the product is faithfully projected. Experimental /api replay/history
appears only in GUI Advanced after version gating. It is never exposed in
Channels. Share and unshare are excluded because they publish transcript, diff,
or model data externally. Delete confirmation names recursive child deletion.
Dynamic provider/model catalogs are not part of the Stable milestone: current
upstream `/api/provider` and `/api/model` groups explicitly describe themselves
as Experimental. The adapter does not call those routes or invent selectable
choices. A future GUI Advanced surface may version-gate them.

## Agent Definitions, Teams, And Native Children

Agent Definition and Runtime Profile are independent choices. Instructions,
tools, MCP servers, and skills are required contributions unless the definition
marks an individual contribution optional. Pairing validation happens in
Composer and Team configuration. If an adapter cannot inject a required
contribution faithfully, that pairing is disabled with a precise reason.

For the stable direct-runtime path, the selected Agent Definition and its
effective instructions are captured before the first native prompt and remain
immutable with the public thread. Changing the Agent Definition starts a new
public thread. Codex's stable thread/start developerInstructions field supports
that contract; its per-turn collaborationMode developer-instruction override is
experimental and is not used by the ordinary Composer path.

Team members store runtimeRef, optional Advanced runtimeOptions, and the
runtimeProfileRevision captured by the Team management/configuration seam.
Gateway JSON represents runtimeProfileRevision as an unsigned decimal string
so the u64 fingerprint-derived revision survives JavaScript round-trips
without precision loss; Markdown may use the equivalent YAML integer.
Structured management writes preserve all three fields in Markdown and
Workbench form/Markdown round-trips. A manually authored definition may omit
the revision, but Team activation must resolve and capture it before creating
the durable Team run; an explicitly stored stale revision is rejected.

Managed Team child turns run with Standard interaction exposure. Codex
`mode=plan` requires GUI Advanced upstream, so Team configuration rejects that
option instead of saving a member that is guaranteed to fail at execution.
Stable `default` and `auto-review` remain available; `full-access` additionally
requires the exact `danger-full-access` Profile sandbox.

Team write and activation both resolve the Runtime Profile and Agent Definition
and fail closed when the Profile is missing, disabled, or incompatible with a
required Agent Definition contribution. Profile defaults are inherited.
Model, mode, catalog-backed per-turn, and safety overrides are validated at
configuration time and revalidated against the captured Profile revision and
the adapter's current capability contract immediately before execution. A
Codex model override requires an exact `model` choice in the cached catalog;
`effort`, `personality`, and `serviceTier` require exact selectable choices for
the effective catalog model. Codex Team members do not accept `summary`,
arbitrary feature keys, or output schemas because those values have no stable
enumerable `model/list` contract. Per-member safety values are accepted only
when the adapter exposes an exact selectable control; otherwise the user must
configure the safety policy on the Runtime Profile. No Team override is
silently ignored or reinterpreted as an unrelated feature.

The first stable bridge allows only a native Psychevo leader to dispatch
Runtime Profile-backed managed members. Codex/OpenCode leaders do not receive
Psychevo Team tools.

The leader-first surface distinguishes:

- Psychevo-managed members, controlled through agent/control
- runtime-native activity, owned by the adapter

A stable runtime-native child session immediately receives a read-only Gateway
child binding and can be opened with lazy history. It has no send, steer, stop,
or agent/control affordance. Child permission and question requests enter
Shared Attention with their parent/child and runtime origin. Provider-owned
activity lacking stable identity remains a bounded activity row rather than a
false Gateway child.

Runtime Context enumerates persisted child bindings so reconnect does not rely
on a transient event. Opening a child resolves its opaque sessionHandle from
the binding, reads native history on demand, imports messages idempotently by
native dedup key with an explicit fidelity marker, and then renders the public
Gateway child transcript. Repeated opens never duplicate imported messages.

Workbench consumes RuntimeChildChanged only when Gateway provides a stable
public child thread id. It registers a navigable runtime-child tab, while the
Thread Panel derives whether send, steer, and interrupt exist from the child's
persisted runtime/context binding ownership. The event's readOnly flag is not
an authorization source. The tab preserves the child's Full, Summary, or
Partial history fidelity across reconnect. The Thread Panel always names the
observed fidelity after lazy history read; Summary and Partial add a compact,
honest notice that original detail or messages may be missing.

For a stable direct thread, the Agent Definition name, effective instructions,
and their fingerprint are captured before the first native prompt. Continuation
and reconnect use that captured snapshot; editing or deleting the mutable Agent
file cannot change the bound persona. An explicitly different Agent selection
still requires a new thread, while a missing or malformed legacy snapshot fails
closed instead of re-reading mutable instructions.

## Workbench UX

Capabilities > Agents > Runtime Profiles provides:

- a structured editor with Direct, ACP, and source provenance
- cached readiness, last checked, Refresh Catalog, bounded Doctor, and auth
  repair
- Native Sessions grouped by Profile and cwd, with fidelity and capability
  actions. A capability-gated history action explicitly reads a session before
  showing its opaque revision points. Revert submits the selected
  `revisionHandle`; Unrevert never asks for or submits a native message id.

Composer reads runtime/context/read, not backend/list. A new thread independently
selects Agent Definition and Runtime Profile. Controls use the three-state
descriptor. A bound thread renders one immutable provenance capsule such as
Codex · Direct. Changing either the bound Runtime Profile or stable direct
Agent Definition offers Start a new thread with ... instead of rewriting the
existing binding.

The Composer exposes Steer only when both the selected runtime advertises a
stable `turn.steer` capability and Gateway has projected the exact active public
turn id. A running activity that has not yet received TurnStarted may accept a
queued follow-up, but it must not show a Steer affordance whose handler can only
silently return. Once TurnStarted arrives, keyboard submit and the visible Steer
mode target that same active turn id.
An accepted public `turn/steer` is not terminal evidence: Gateway must drain the
message exactly once into the same Host runtime control used by the active direct
turn, and the adapter must prove the correlated native steer reached that turn.

The stable direct-runtime thread also fixes the Agent Definition whose required
contributions created the native thread. Codex carries its
instructions through the stable thread/start or thread/resume
developerInstructions field. Upstream exposes per-turn developer instructions
only inside experimental collaborationMode, so an ordinary bound thread must
start a new public/native thread to change Agent Definition. OpenCode follows
the same product invariant even though its prompt system field could vary per
turn. Experimental collaboration modes are GUI Advanced-only and never become
an implicit default-mode persona switch.

The provenance capsule is the only new visual signature. It uses existing
Workbench typography, spacing, and semantic status colours; runtimes do not
introduce brand-colour noise.

Shared Attention shows runtime kind, Runtime Profile id/label, and public
parent/child thread origin plus the real authorization lifetime. A native child
origin is resolved to its read-only public Gateway child thread before it is
rendered; native session ids are not an Attention identity. `Session` and
`Always` actions appear only when the adapter both declares a matching native
choice and Gateway can enforce that choice. Each visible action states its
actual scope: once means this request only, Codex session means the current
Codex session, and an OpenCode instance-scoped grant says that it lasts until
the runtime instance restarts. `Always` is reserved for a declared and enforced
permanent lifetime.

## Channels

Channel settings use cached runtime/context/read choices; they do not hard-code
native, codex, or opencode.

- /profile use <id> stores a draft preference when no thread exists.
- On a bound lane, /profile use creates and binds a new public thread; the old
  thread remains unchanged.
- /profile sessions lists resumable opaque handles without native or Gateway
  ids, making /profile resume <short-handle> discoverable. Resume resolves only
  a handle returned for the lane's current Runtime Profile.
- An active native session rejects write takeover and directs the user to GUI
  or Fork.
- Model and mode appear only when the adapter declares a Channel-safe control;
  otherwise Channels say Uses runtime default.
- The native Channel permission-mode control appears only for native execution.
  A direct Profile shows its bound approval/sandbox policy read-only; changing
  a native RunOptions permission value must never imply that direct safety was
  changed.
- Every delivered permission or question is assigned a short-lived, one-use,
  opaque Channel-local token. The token registry maps it to the Gateway action
  only for the originating connection and source lane; neither Gateway action
  ids nor runtime-native ids are rendered. Permission and question responses
  allow only allow once, deny, answer, or cancel. Durable allow is not exposed.
- `GuiAdvancedOnly` interactions are declined upstream before Channel
  projection. Channels receive only a safe cancellation notice and eventual
  final or terminal progress; they receive no interaction token, `/answer`
  command, native prompt, or raw question content.
- Delivery is limited to necessary progress, attention, and final summary. It
  omits raw native ids, native event payloads, child transcript dumps, and
  multi-pane state.

## Authentication And Diagnostics

Credentials remain owned by the native runtime. Codex auth uses supported
account/login actions. OpenCode OAuth is Advanced-only and appears only after
the adapter detects a compatible contract; otherwise Doctor returns exact CLI
repair guidance.

Doctor is side-effect-bounded and reports readiness stages separately:
configuration, executable, launch, transport, version, authentication, and
capability hydration. Provider calls and real auth remain opt-in.

## Validation

Deterministic validation includes:

- FakeRuntimeModule shared conformance for native, ACP, Codex, and OpenCode
- fake Codex stdio executable covering framing, early notifications,
  interactions, history fidelity, child rejection, EOF, exactly-one terminal,
  and cancellation-safe concurrent shutdown of multiple selected workers
- fake OpenCode HTTP/SSE server covering secure spawn, multi-cwd generation
  sharing with one observed server spawn for two workspace directories,
  SSE-before-prompt, root and child todo/diff hydration, correlated
  typed todo/diff SSE updates, stale-session and stale-instance suppression,
  dedup, disposal, MCP reconciliation, child questions, abort, fork/revert, and
  recursive delete
- Gateway tests for immutable bindings, binding-before-prompt, no fallback,
  revisions/epochs, read-only takeover, history dedup/fidelity, and child
  ownership/replay isolation
- Workbench tests for Runtime Context, provenance, three-state controls,
  incompatible pairing, Doctor/auth repair, Native Sessions, leader-first
  ownership, read-only child tabs, and Shared Attention provenance/lifetime;
  targeted visual proofs capture Runtime Profile readiness/editor, Native
  Sessions ownership, OpenCode typed timeline and revert state, and Shared
  Attention provenance on desktop and mobile
- Channel tests for pre-thread preference, bound-lane rotation, reset, resume,
  list-to-resume discovery, one-use short interaction tokens, raw-id
  non-disclosure, and explicit failures

The xtask live registry has a runtimes suite with deterministic direct Codex and
OpenCode GUI and Channel smoke checks. Channel checks enter through a real
Telegram polling adapter against a loopback fake API and assert its real
outbound `sendMessage`; a forged `turn/start` source is not Channel evidence. A
separate Codex control check primes the observed Stable matrix, starts a second
turn, then proves the public `turn/steer` request reaches native `turn/steer`
and its correlated terminal. A dual-runtime readiness check runs Codex and
OpenCode in one Workbench instance and requires both cached Profile details to
show `Ready`; a single passing adapter is not milestone evidence. Real binary,
provider, and auth checks are opt-in and are not part of the default gate.
When generated ACP compatibility and direct Profiles share a base product name,
the deterministic GUI checks select the intended row by its exact accessible
name; substring matching is not identity evidence.

Delivery runs closest tests first, Workbench test and typecheck, rust-broad,
the complete visual profile, the complete shared live registry, and the direct
ACP browser proof. Artifacts and structured skips are retained as evidence.

## Related Topics

- [021 Gateway](../021-gateway/spec.md)
- [028 Channels](../028-channels/spec.md)
- [051 Agents](../051-agents/spec.md)
- [247 Capability Management](../247-capability-management/spec.md)
- [280 Channel UX](../280-channel-ux/spec.md)
