---
name: 052. Agent Runtimes
psychevo_self_edit: deny
---

# 052. Agent Runtimes

Define the application and execution architecture that makes Native Psychevo
Agents and external ACP Agents first-class across every interactive surface.

## Scope

- Agent Definition and Runtime Profile identity
- Gateway `ThreadApplication` and `AgentSessionHost` Modules
- Native and outbound ACP Agent Adapters
- immutable thread binding and session lifecycle
- typed inputs, controls, actions, interactions, delivery, and history
- Workbench and Channel parity
- managed Codex ACP and local OpenCode ACP product shortcuts
- hard removal and configuration diagnosis of the retired direct Codex/OpenCode architecture

Out of scope:

- treating an Agent as an AI model provider
- using ACP as Psychevo's internal application interface
- requiring every Agent implementation to expose an identical capability set
- provider-owned model resolution inside Gateway
- silently importing or continuing retired direct-runtime sessions

## First-Class Agent Contract

Native Psychevo Agents and ACP Agents are first-class when they share:

- one caller-facing Thread/Turn Interface
- immutable Agent Definition and Runtime Profile binding
- one queue and active-turn state machine
- typed input admission and control resolution
- exactly-one terminal and explicit delivery certainty
- one interaction broker and one product History Interface
- the same Workbench and Channel application use cases
- capability-gated actions with explicit unavailability

First-class does not mean feature equality. An effective capability exists only
when it is negotiated by the Agent, implemented by its Adapter, certified by
Psychevo policy, and granted by the captured binding. A caller must never infer
capability from `runtimeRef`, backend id, executable name, or product branding.

## Runtime Identity And Configuration

Agent Definition and Runtime Profile remain independent selections. A
`RunnableTarget` is the Gateway-validated pair:

```text
{ agentRef, runtimeProfileRef }
```

Agent Definitions own product identity, instructions, entrypoints, skills, MCP
scope, and tool policy. Runtime Profiles own the public execution identity,
implementation kind, backend reference, captured defaults, workspace policy,
and runtime options. Executable `command`, `args`, `env`, and launch cwd belong
only to an ACP backend registration.

`default_agent` selects among real compatible Agent Definitions; it never
creates an Agent Definition or an additional runtime-native control. Agent
names exposed by an ACP implementation, such as OpenCode `build` and `plan`,
are Session modes and remain Adapter-owned `mode` controls.

The only Runtime Profile kinds are:

- `native`: in-process Psychevo runtime execution
- `acp`: an external Agent through an ACP backend

The generated public profiles are `native`, `codex`, and `opencode`. `codex`
references the managed `codex-acp` backend. `opencode` references the
`opencode acp` backend. Arbitrary enabled ACP backends may generate profiles of
the form `acp:<backend-id>`.

The profile catalog resolves compatible Agent Definition/Profile pairs and
cached readiness. Workbench, Channels, and other callers never perform pairing
or executable discovery themselves. Catalog reads are cache-only and must not
spawn a process or create an Agent session. Explicit refresh and Doctor actions
may perform bounded local probes.

The catalog applies one pairing rule for both `ThreadContext.compatibleTargets`
and pre-delivery `turn/start` validation:

- `{ agentRef: null, runtimeProfileRef: "native" }` is the explicit default
  Psychevo Agent target.
- A named Native target requires an active Agent Definition with no
  `backend.ref`. Native top-level selection does not require `peer`: the
  current Agent Definition schema has no separate `main` entrypoint and local
  definitions default to `subagent` for child-callable policy.
- An ACP target requires an active Agent Definition whose `backend.ref` equals
  the Profile `backend_ref` and whose entrypoints include `peer`.
- Definitions that are disabled or shadowed never form a runnable target. ACP
  definitions that are child-only or backed by a different backend are also
  excluded.

The immutable binding captures the canonical nullable Agent Definition id, a
fingerprint and JSON snapshot of that definition (or the explicit default
Agent snapshot), plus the Runtime Profile fingerprint and snapshot. A later
turn may omit its target to inherit the binding, but an explicit Agent or
Profile change requires a new public thread.

The Runtime Profile and Runnable Target catalogs are prospective configuration
for unbound Threads only. Once bound, every turn, control mutation, history or
session lifecycle action resolves the captured Agent Definition and Runtime
Profile snapshots from the binding. It must not require either captured id to
remain in the current catalog or re-apply current `peer`/`subagent` entrypoint
admission. The captured Profile's `backend_ref` selects the current backend
Adapter configuration; a missing, disabled, or unlaunchable backend fails with
a structured pre-delivery unavailable error. Renaming, hiding, deleting, or
changing a current Profile or Agent Definition does not rewrite or invalidate
an otherwise valid captured binding.

An outbound ACP Agent invoked as an Agent Tool owns a child-scoped activity and
turn identity while its parent independently remains active waiting for the
tool result. Child live observations, controls, terminal, and authoritative
history use that child turn identity. The child activity and Agent edge are
settled before its terminal becomes observable. An ACP-bound child never
inherits parent activity as a display or control fallback, and interrupt or
steer addressed to the child cannot target the parent turn.

## Gateway Application Module

Gateway owns one deep `ThreadApplication` Module. Its Interface is:

```text
inspect(source, thread?, prospectiveTarget?) -> ThreadContext
prepare(source, targetId) -> ThreadContext
set_control(source, thread?, mutation) -> ControlReceipt
run_turn(source, thread?, target?, input, overrides) -> TurnReceipt
act(source, thread, action) -> ActionReceipt
respond(source, thread, interaction) -> InteractionReceipt
read_history(source, thread, cursor?) -> HistoryPage
```

The Interface hides source drafts, immutable bindings, profile capture,
queueing, session attachment, control resolution, Adapter selection, delivery
classification, projection, interactions, and history authority.

The concrete `run_turn` input contains typed content, target/control intent,
surface environment and presentation policy, and observation/control sinks.
`ThreadApplication` alone lowers that input into runtime-internal `RunOptions`
and the private active-queue envelope. No caller Adapter may supply runtime
state, native session identity, an external delegate, Adapter selection, or a
preassembled `RunOptions`.

An unbound source draft stores `draft_agent_ref`, `draft_profile_ref`, and
typed `draft_control_values`. These are caller intent for a prospective
`RunnableTarget`; they are not a runtime binding and must not be named or
interpreted as runtime execution options. Only after target validation and
binding may Gateway lower those control values into Adapter-internal
`RunOptions.runtime_options`.

The public Gateway methods are:

- `thread/context/read`
- `thread/draft/prepare`
- `thread/control/set`
- `turn/start`
- `thread/action/run`
- `thread/interaction/respond`
- `thread/history/read`

`runtime/options`, `runtime/context/read`, and `runtime/control/set` are removed.
Backend administration remains a sibling management Module and keeps
`backend/list`, `backend/write`, and `backend/doctor`; managed adapters add
`backend/install`, `backend/repair`, and `backend/upgrade`.

### Thread Context

`ThreadContext` is the single coherent caller snapshot and contains:

- current source identity and source draft
- immutable binding when a public thread is bound
- compatible `RunnableTarget` choices and readiness
- input capability descriptors
- control descriptors and state
- action descriptors
- `sendability { allowed, reason, recoveryAction }`
- history owner, fidelity, resumability, and cursor
- pending interactions
- `contextRevision` and `controlRevision`

`contextRevision` is the opaque admission-freshness token for the selected
target, readiness, immutable binding, Agent identity, and negotiated facts
that can change input admission or sendability. It is not a hash of every
display projection. Commands, history progress, title, usage, and other
observability-only session facts may refresh the returned context without
invalidating an otherwise identical turn or control mutation.
`controlRevision` independently covers the control catalog, mode catalog, and
effective runtime control values. A real change to either admission or control
facts remains stale and fails closed.

Implementation kind may appear as diagnostic provenance but must not require a
caller behavior branch.

For an unbound source, `thread/context/read.target` is the prospective
`RunnableTarget { agentRef, runtimeProfileRef }` the caller intends to use.
Gateway computes controls, input admission, and `sendability` for that exact
pair. A missing target may project the catalog default for discovery, but it is
not sendable. For a bound Thread, Gateway derives the target from the immutable
binding; a conflicting prospective target fails closed.

`thread/draft/prepare` is the only ordinary product read-adjacent operation
that may start an ACP process before a prompt. It resolves the opaque target
through the current catalog, stores that exact Agent/Profile source draft, and
creates or reuses one unpublished resident session. Its returned Thread
Context is projected from the Agent's real config options. Prepared session
identity remains process-local; only source target/control intent is durable.
Replacing the target, resetting the source, process shutdown, or idle expiry
releases the draft. Native preparation never creates a runtime session.

Stable ACP `configOptions` are authoritative for session model controls. For an
older ACP Agent that returns the deprecated `models` session field but no
stable model config option, Gateway may capability-detect that bounded field
and project it through the same product model-control descriptor. A valid
legacy selection is applied with `session/set_model`; stable
`session/set_config_option` always wins when both surfaces are present. This
compatibility is derived from the response shape, not Agent branding, backend
identity, or a user-facing switch, and it does not enable any other deprecated
ACP extension.

Source ingress that has selected an ACP Runtime Profile but has not yet selected
an Agent Definition resolves that profile's `default_agent` only when it names
a compatible catalog Agent, then falls back to the compatible Agent named by
`backend_ref`. This is the same Gateway-owned catalog target exposed to GUI
callers; invalid defaults are never synthesized as targets, and Channels must
not manufacture a `Default Agent` pairing for an ACP profile.

Every compatible target carries a stable opaque `targetId` plus Gateway-owned
`agentLabel`, `profileLabel`, combined `label`, readiness, and unavailable
reason. Product clients treat the target as one selection and must not recreate
pairing by joining Agent Definitions to Runtime Profiles.

### Turn Input

`turn/start` accepts structured input parts. The stable set is text, image,
resource, resource link, and explicit embedded context. Each accepted part is
faithfully lowered by the selected Adapter. Unsupported input fails before
delivery; it is never converted to a textual placeholder or silently omitted.
`ThreadContext.inputCapabilities` describes these input kinds and the separate
`agentMention` admission capability. Every descriptor includes `enabled` and an
exact `unavailableReason`. A client must validate the complete input and
structured mentions against the same target-scoped context before optimistic
submission, while Gateway remains authoritative and repeats the validation.

An unbound `turn/start` requires a valid `RunnableTarget`. Gateway captures the
Agent Definition and Runtime Profile, creates the public thread, persists the
binding, attaches the Agent session, and only then delivers the prompt. A bound
thread rejects a conflicting target. Changing Agent Definition or Runtime
Profile creates a new public thread.

Every `turn/start` supplies non-empty `expectedContextRevision` and
`expectedControlRevision`. Gateway compares both against the same live
`ThreadContext` used by the caller before creating a session, persisting a
binding, or attempting delivery. This applies to both prospective unbound
targets and bound threads; missing or stale revisions fail with
`not_delivered`.

`selectionState` describes how Gateway found the selected target; it is not
part of the target's semantic freshness identity. Resolving the same unbound
target from an explicit prospective selection or from its stored source draft
must therefore produce a turn-compatible `contextRevision`. A successful
unbound control receipt is immediately usable by `turn/start` with that same
target and its returned revisions.

Gateway validates a public turn once before accepting it. Materializing the
Thread, immutable binding, or source association must not invalidate that
accepted snapshot before delivery. The validated source-draft controls are
atomically promoted to initial sticky Thread preferences, while explicit
`turnOverrides` remain one-turn-only. Gateway clears the source draft only
after its values have been captured by the new binding.

### Actions, Interactions, And History

`thread/action/run` accepts a sealed action union, not an open action id plus
JSON payload. The first stable variants are `interrupt`, `steer { text,
expectedTurnId }`, and `compact { instructions? }`. `ThreadContext.actions`
describes only actions implemented for the current bound runtime and marks a
temporarily inapplicable action disabled with a reason.

`thread/interaction/respond` accepts a sealed response union for permission,
clarify answer, and clarify cancellation. The interaction id must name a
currently visible permission or clarify request for the same Thread;
unsupported, hidden, stale, or kind-mismatched interactions fail closed.
`ThreadContext.pendingInteractions` contains only requests accepted by this
public response contract.

`thread/history/read` reads Psychevo's projected transcript for one authorized
Thread. Its opaque cursor is the last returned stable transcript entry id;
unknown cursors fail closed. Pages preserve the `ThreadHistoryView` owner and
fidelity reported by `ThreadContext` and never read an adapter-native session
history directly.

### Controls

Control descriptors are implementation-neutral and include typed value schema,
catalog, semantic role, allowed targets, timing, stability, Channel safety,
dependencies, confirmation semantics, and unavailable reason.
`capabilityRevision` is an opaque Gateway token. Clients preserve and return it
verbatim; they must not parse it as a number or restrict it to decimal text,
because a live Agent surface revision may be a content hash.
Each descriptor explicitly carries `enabled`, `required`, and
`unavailableReason`; a disabled descriptor remains visible when its reason helps
recovery. `choices` are the authoritative selectable values. A generic
presentation hint may choose a renderer, but never authorizes a value or changes
behavior.

Control values have five distinct planes:

1. `profileDefault`
2. `sourceDraft`
3. `threadPreference`
4. `turnOverride`
5. `runtimeObserved`

Gateway resolves one turn value in the order `turnOverride`,
`threadPreference`, captured `profileDefault`, then runtime default. The accepted
turn records the exact control revision. A bound model/mode/reasoning selection
is a sticky thread preference and applies to the next turn unless an explicit
one-turn override is used.

`ControlReceipt.status` is one of `rejected`, `stored`, `applied`, or
`observed`. An idle ACP session mutation is `applied`; it becomes `observed`
only after an authoritative Agent update or readback. While that Thread has an
active turn, Gateway must not serialize a context read or control mutation
behind the long-running ACP prompt. It serves the last resident session
projection, stores the new Thread preference as `stored`, and applies that
preference before the next prompt. The active prompt keeps its already accepted
control revision. Failure to apply a required control before that next prompt
returns `delivery=notDelivered` and blocks prompt delivery.
The receipt also returns the complete authoritative `ThreadContext` for its new
revisions. Headless clients replace their prior context atomically; updating
only the changed control row can leave sendability, dependencies, or other
control capability revisions stale.
For an unbound source, that receipt remains target-scoped to the same
prospective `RunnableTarget` used to validate the mutation; persisting the
source draft must not relabel the receipt into a revision-incompatible context.
Product controls remain disabled while one mutation is in flight, so two UI
changes cannot race using the same expected revisions.
`thread/control/set` carries the selected opaque `targetId`. For an unbound
source, Gateway resolves that id through the same catalog used by
`thread/context/read`, validates revisions against that exact prospective
Agent/Profile pair, and stores both draft identities with the control. Callers
never send only a Runtime Profile and ask Gateway to infer the Agent again.
Applying a stored preference to the resident Agent may advance
`controlRevision` again when the Agent's observed control projection catches up.
Callers therefore compare revisions only as opaque freshness tokens and refresh
the complete context after the turn; they do not require the receipt revision
to remain equal after runtime observation.

Creating a Side Chat snapshots the parent Thread's resolved live control
descriptors at the same command boundary that creates the child binding. Every
non-null effective control becomes an initial sticky preference on the child,
including values projected only from a resident or persisted ACP Session
snapshot. Copying only the parent's persisted preference and observation maps
is insufficient because those maps need not contain the Agent's current model,
mode, or other live effective values. The child starts a fresh runtime-native
session and does not copy the parent's runtime-observation or native-session
identity.

When the unbound source owns a matching prepared ACP session,
`thread/control/set` applies the value to that resident session before
returning. The first accepted turn atomically claims the prepared session,
persists its native id in the immutable binding, and delivers the prompt only
after promotion succeeds. It sends no second `session/new`; a stale revision,
failed promotion, or failed binding remains `notDelivered`.
An asynchronous Agent notification that changes only available commands,
history progress, session title, or usage cannot make the prepare receipt stale
for this mutation or promotion. Clients do not retry a genuinely stale control
mutation automatically.

## Agent Session Module

Gateway owns an internal deep `AgentSessionHost` Module. Its Interface is:

```text
prepare(capturedDraftTarget) -> PreparedAgent
attach(capturedBinding) -> AttachedAgent
AttachedAgent.transact(typedSessionCommand) -> typed result
shutdown(deadline) -> ShutdownReport
```

The sealed command families are context read, control mutation, turn submit,
history read, session action, interaction resolution, interrupt, and steer.
There is no `namespace + operation + JSON` command bus.

Each public thread has exactly one ordering authority. Native turns are ordered
by the Thread Application active queue. ACP session commands are ordered by the
outbound Adapter's resident per-session actor and lock inside its process pool.
`AgentSessionHost` must not add a second mailbox over either authority.

`attach` captures thread id, binding revision, and immutable Agent/Profile
fingerprints. It is idempotent for an identical capture, rejects a conflicting
target for the same thread/revision before dispatch, and never sends a prompt.
The returned `AttachedAgent` is an immutable routing capability to the existing
ordering authority; different threads may execute concurrently. A newly
created or resumed ACP native session id is durably acknowledged through the
captured binding callback before prompt delivery is confirmed.

Cache-only context and history reads never attach or restart an ACP process.
Prepared sessions are keyed by source plus captured target/cwd/fingerprint and
coalesce concurrent identical preparation. Promotion rekeys the resident
ordering identity to the public Thread exactly once. Replacement and expiry
close the native session when supported and otherwise detach it without
claiming remote deletion.
After a completed connection loss, the next explicit turn attaches and loads
Agent-owned history before delivering the new input. If the prior turn has
`delivery=unknown`, that load may reconcile its durable terminal, but the prior
input is never replayed; only the caller's new input is delivered.

The Module has two production Adapters and deterministic fake Adapters:

- `NativeAgentAdapter` lowers typed commands to `psychevo-runtime`
- `AcpAgentAdapter` lowers typed commands to a negotiated ACP Agent

Raw ACP SDK types, requests, notifications, ids, secrets, or process handles do
not cross the Agent Session seam. Dropping a caller subscription does not detach
or terminate a session.

## Native Agent Adapter

Native execution stays in process and retains its richer Psychevo capability
plane, including tools, skills, hooks, evidence, Teams, children, permissions,
and native history. Native is not reimplemented as an ACP Agent and does not
depend on an ACP process.

The Native Adapter converts Thread Application commands to existing runtime
inputs and observations. Adapter selection occurs only inside
`AgentSessionHost`; Gateway callers and product surfaces never match on Native.

## Outbound ACP Agent Adapter

Psychevo acts as the ACP client through stable wire protocol v1. The Adapter
validates the initialize response protocol version and capabilities before
creating a session. Outbound experimental v2 is rejected as unsupported; the
SDK's v2 schema remains enabled only for the separately implemented inbound ACP
server. A v2-first attempt or silent fallback is forbidden.

The Adapter owns:

- structured process launch from the captured effective environment and
  environment redaction; an explicitly supplied `RunOptions.inherited_env` is
  the complete launch baseline and must not be re-expanded from host
  `std::env`
- initialize and capability negotiation
- authentication methods
- session new, load, resume, list, fork, close, and delete when advertised
- stable session config options and authoritative updates
- structured prompt conversion
- available commands, mode, config, usage, and session information updates
- filesystem and terminal callbacks through Gateway policy
- MCP declarations and callbacks
- permission and elicitation brokering
- cooperative cancel followed by bounded process termination
- history replay and live event normalization
- delivery certainty and reconnect diagnostics

MCP handoff is an Adapter-owned session-setup operation, not a product-surface
branch. The resolved server set is the exact intersection of the captured
backend and Agent Definition name sets. It is supplied identically to
`session/new`, `session/load`, `session/resume`, and `session/fork`, after
transport capability and portable-policy validation, and is immutable for the
resulting resident ACP session. Fork inherits the source resident session's
stored declarations; resume receives the declarations reconstituted from the
captured binding, never current mutable configuration. Stable-v1 stdio and
negotiated HTTP are supported. Values with no stable-v1
representation (a distinct stdio cwd or Psychevo-only per-server tool/time
policy) are rejected before delivery; secrets may enter the wire request but
must not enter ThreadContext, events, diagnostics, or hashes exposed to product
surfaces.

Agent-originated request failures expose only their numeric ACP code and a
bounded single-line `message`; arbitrary error `data` is discarded. Stable-v1
`AuthRequired` on new/load/resume/list or pre-prompt config is classified as
`acp_auth_required`, `delivery=notDelivered`, with `backend/doctor` as the
recovery action. An error after prompt dispatch still follows unknown-delivery
rules even when its code is authentication-related.

ACP form elicitation reuses the Thread interaction broker and active turn
control handle. The Adapter maps session-scoped string, number, integer,
boolean, and string multi-select properties to typed clarify questions,
validates the once-only product response against the original schema, and then
returns a typed ACP accept/cancel/decline result. It advertises form support but
not URL elicitation; request-scoped or unknown schemas fail closed because they
cannot be tied to a visible public Thread interaction.

### Process And Session Supervision

ACP processes are resident, not per turn. A supervisor registry is keyed by the
captured backend/profile fingerprint, canonical workspace, and auth scope. A
generation owns one process/connection and may multiplex sessions only when the
Agent contract supports it. Each public thread still owns an independent
session actor and native session id.

Supervisor lookup and actor bootstrap MUST run safely on the default Tokio and
Rust test worker stacks. Lifecycle correctness MUST NOT depend on enlarging
`RUST_MIN_STACK`; large lifecycle dispatch futures are heap-isolated before
they reach synchronous registry lookup and reuse paths.

Generations use leases and reference counts. Startup failure cleans up the
partial generation. Idle eviction may reclaim a zero-reference process but must
not evict an active turn or pending interaction. Process exit wakes every
waiter, classifies accepted turns, and prevents an implicit resend.

The Adapter owns typed, internal lifecycle operations for list, resume, fork,
close, and delete even while the sealed public Thread action union exposes none
of them. Every operation is gated by the initialize capability before sending
a request. Session-scoped operations use the same per-thread lock and ordered
response barrier as prompt/load. Resume installs a fresh session epoch only
after its response barrier; fork installs a new local/native identity and fresh
epoch only after reducing replay through its response barrier. Close/delete
cooperatively cancel first and, after acknowledgement, erase the resident
projection, callback context, and session-owned terminals. Process shutdown
performs the same cleanup even when close is unsupported or times out.

### Product Session Lifecycle

Agent-owned sessions enter the product only through an explicit import action.
Ordinary `thread/list`, `thread/browser`, history navigation, and Session-panel
rendering remain cache-only and never initialize an ACP Agent. Opening the
Workbench import surface calls `thread/import/list`; Gateway probes every
distinct enabled ACP Runtime Profile with bounded concurrency and returns
partial per-Profile results. Profiles sharing a process generation are listed
once, while compatible Agent targets remain explicit binding choices.

`AgentSessionHost` exposes typed discovery for an unbound captured target and
typed resume, fork, close, and delete commands for an attached Agent. ACP native
session ids and cursors stay behind this seam. Public import candidates and
cursors are bounded, expiring, opaque handles; a restart or expiry requires a
new discovery request.

Import reserves an unpublished public Thread and requires stable ACP
`session/load` so the selected Agent session can replay its product-visible
history. Gateway reduces replay through the response barrier, persists every
bounded transcript fact that the public Message model can represent (user and
assistant text, reasoning, tool calls and results, and plan metadata), then
atomically publishes the immutable binding and Thread snapshot. Import never
claims `fidelity=full` when a replay fact was omitted, truncated, or could not
be projected. A stable-v1 content chunk without a non-empty `messageId` is not
given a synthetic durable message identity: its projectable display content
and reliable neighboring facts are published, the Thread remains writable, and
history is explicitly `partial` with a recovery hint. Bounded internal replay
ids may make unidentified content, tool-only, and Plan facts idempotent, but
only real Agent-supplied message ids may participate in delivery
reconciliation.

History replay preserves notification order inside assistant content. Text or
reasoning chunks for the active message may continue after an intervening tool
call, producing the same text/tool/text slot order in durable history; a new
user or message identity closes the prior active segment. Tool updates replace
the state of the original `toolCallId` slot rather than creating another slot.
Stable-v1 Plan notifications are complete replacement snapshots: replay owns
one logical Plan value and persists only the latest update, with no synthetic
Plan identity treated as message delivery evidence.

Import never
falls back to `session/resume`, because that lifecycle operation does not replay
history; an Agent without `session/load` fails explicitly and no empty public
Thread is published. An existing
`(runtime_ref, native_session_id)` binding wins an import race and is returned
instead of being duplicated. Fork follows the same publish-after-ready rule and
records the source Thread as parent. It is exposed only when the negotiated
Agent capability includes `session/fork`; Native and Agents without that
capability do not receive an emulated fork.

Archive closes a resident ACP session before local archival when close is
advertised. A non-resident session or an Agent without close support may still
be archived locally. Restore is an explicit action and resumes the Agent before
making the archived Thread visible when resume is advertised; ordinary Thread
navigation never substitutes for ACP `session/resume`.

Native deletion remains local. An ACP-bound Thread is deletable only when the
Agent advertises `session/delete`. Gateway records a durable delete intent,
waits for remote acknowledgement, and only then removes local state. Failure or
unknown delivery keeps a visible non-success state; it never reports a local
delete while the Agent-owned session may remain. Startup reconciles unfinished
intents before allowing another lifecycle mutation.

### Replay And Projection

History replay and live notifications enter the same typed reducer. Every fact
has history/live origin, process generation, session epoch, ordering identity,
and a bounded product projection. Replay-origin transcript facts retain source
order and stable Agent message or tool identities so repeated loads deduplicate
against durable history. Replay completion uses an explicit barrier; time-based
notification draining is forbidden. Import commits the completed replay before
publishing its Thread and releases both the reserved Thread and resident Agent
session if load, reduction, or persistence fails.

The resident Adapter exposes one immutable per-session snapshot as the only
read interface for negotiated Agent identity/capabilities, prompt input support,
session lifecycle and history resumability, current config/mode, available
commands, and process/session epochs. A `session/load` response is the protocol
ordering fence for replay sent before that response; the Adapter appends a local
barrier to the same ordered notification ingress and reduces through that
barrier before publishing the snapshot. The barrier is appended from a
registered response-dispatch interceptor, which completes before that
connection dispatches a later wire message. Prompt completion uses the same pattern with the
`session/prompt` response. This barrier is deterministic and testable; it is not
a sleep, quiet-period heuristic, or one-shot nonblocking drain.

Unknown notifications are tolerated and retained only as bounded diagnostics.
Available command, mode, config, usage, and session information updates must
update the corresponding typed product state when supported.

### Capability Packs

Standard ACP is the default implementation. Versioned capability packs may
translate product-specific, source-proven metadata without leaking it through
the common Interface:

- `CodexAcpPack` matches reviewed Codex ACP `agentInfo` and metadata schema
- `OpenCodeAcpPack` matches reviewed OpenCode ACP `agentInfo` and schema

Pack activation requires exact name/schema validation and an exact reviewed
stable semantic version: `@agentclientprotocol/codex-acp 1.1.2` or
`OpenCode 1.17.18` for the current source snapshot. A future patch, prerelease,
or build-qualified version is not assumed compatible; outside those exact
versions the Agent receives standard ACP behavior and an explicit extension
diagnostic. Unknown metadata never becomes a generic product action.

Each projected `RuntimeCapabilityView` carries an optional
`unavailableReason`. A disabled capability caused by an incompatible capability
pack version must expose that reason on the capability fact itself so callers
do not infer failure semantics from Adapter identity or a separate diagnostic
list.

Codex may expose its source-proven auth/provider, command, goal, quota, model,
reasoning, mode, and fast-mode semantics. OpenCode may expose its source-proven
commands, model/effort/mode, and session lifecycle. Direct-only steer, children,
todo, diff, or revert behavior is unavailable unless a reviewed ACP contract
later exposes it.

For the exact reviewed Codex ACP identity and version, the Adapter may project
`PromptResponse._meta.quota` into a typed product quota fact. The projection is
limited to the reviewed aggregate `TokenCount` fields and a bounded list of
model/token-count pairs. Unknown top-level `_meta` keys are discarded; a quota
with unknown fields, invalid number types, duplicate or unbounded model entries,
or an unreviewed Agent version is not exposed as product data. Rejection emits
only a bounded diagnostic category and never echoes the rejected metadata.

## History And Delivery

One product History Interface hides implementation differences while preserving
authority and fidelity:

- Native: `owner=psychevo`, durable, resumable
- ACP with load/resume: `owner=agent`, Gateway projection/checkpoint, resumable
- ACP without load/resume: `owner=process`, projected display history,
  non-resumable after process loss

Gateway persists accepted user intent, binding, delivery state, terminal state,
and product-safe projections. It does not reconstruct an Agent-authoritative ACP
session by replaying a synthetic transcript into a new session. Missing Agent
history is explicit. `replayComplete` means both the Agent response barrier and
lossless bounded product projection completed; reaching a replay bound,
encountering unsupported content, or lacking stable message identity keeps the
projection partial across import, context refresh, and later continuation.

Each accepted turn produces exactly one terminal. Delivery is
`notDelivered`, `delivered`, or `unknown`. Unknown delivery enters a
reconciliation-required state and is never automatically retried. Interaction
tokens are opaque, source-scoped, expiring, and single use.

The authoritative interactive terminal covers the Agent turn, delivery ledger,
and required transcript persistence; it does not cover optional display-only
auxiliary work. Native new-session title generation runs independently of that
terminal and reports its persisted result through `titleChanged`. It cannot
extend the active turn, keep the Thread queue occupied, or delay
Session/Composer idle state.

## Managed Codex ACP And OpenCode ACP

The reviewed Codex adapter is `@agentclientprotocol/codex-acp` version `1.1.2`.
Psychevo installs it explicitly under:

```text
$PSYCHEVO_HOME/runtime-adapters/codex-acp/1.1.2
```

The repository owns an exact package/dependency/integrity lock. Install,
repair, and upgrade use a temporary sibling directory, verify package identity,
version, integrity, lock, entrypoint, and launchability, write and re-verify a
deterministic seal covering the complete installed tree, then atomically rename
the directory. The seal hashes regular-file payloads and safe in-tree symlink
targets without following directory links; a missing legacy seal, changed or
additional payload, special file, or out-of-tree symlink makes the install
invalid and repairable. The npm process receives the Gateway's captured
environment baseline and never re-reads ambient process environment.

Ordinary catalog, context, and turn paths are offline. The managed `codex`
target is ready and launchable only when ordinary inspection returns `Ready` and
the configured command is the verified absolute executable path. `Missing`
offers `backend/install`; `Invalid` offers `backend/repair`; both fail before
process launch and prompt delivery. `npx`, `@latest`, PATH replacement, and
implicit download are forbidden. Windows resolves the sealed managed `.cmd`
launcher.

Full payload hashing is mandatory after install/repair promotion, on explicit
Doctor, and before the first process launch for a sealed tree. Repeated ordinary
inspection may use a process-local successful-verification cache only after a
cheap complete entry-metadata fingerprint still matches the seal/root identity;
file replacement, size/time/identity changes, added entries, changed symlink
targets, or a changed seal invalidate the cache and require a new full hash.

Doctor reports Node/npm missing, adapter missing, corrupt install, version
mismatch, authentication required, and protocol incompatibility separately.
After local launch prerequisites pass, Doctor performs one typed initialize-only
protocol probe and records it as a distinct `protocol` check. Authentication is
probed only when that check confirms stable wire v1; an incompatible or
unavailable protocol leaves authentication explicitly unchecked rather than
misclassifying initialization failure as an authentication failure. The
protocol probe never creates, loads, resumes, or prompts a session.
The `codex` target remains discoverable while unavailable and supplies an
install recovery action.

Authentication diagnosis is non-mutating. After an exact reviewed
`@agentclientprotocol/codex-acp` `1.1.2` initialize identity match, Gateway may
send the Adapter's typed `authentication/status` extension and maps only its
bounded `unauthenticated`, `api-key`, `chat-gpt`, or `gateway` result to doctor
status. A generic ACP Agent, including the reviewed OpenCode Adapter, has no
source-proven side-effect-free credential-status request: Gateway therefore
reports `authentication required` only after observing a real stable-v1
`AuthRequired` response from that captured backend launch/auth/workspace scope,
and otherwise reports authentication as `unchecked`. A successfully
delivered turn, or an authoritative exact-pack authenticated status, clears
that observation back to `unchecked`. Doctor must never create/load a session
or send a prompt merely to test credentials.

This boundary is source-backed: `.references/codex-acp/src/AcpExtensions.ts`
declares the closed authentication-status response union and
`CodexAcpServer.ts` dispatches that method without session creation, whereas
`.references/opencode/packages/opencode/src/acp/service.ts` only advertises
`opencode-login`, returns success from that method-id acknowledgement, and
derives `AuthRequired` from real provider-operation failures.

For the reviewed managed Codex backend, an inherited non-empty
`CODEX_API_KEY` or `OPENAI_API_KEY` activates Codex ACP's own
`DEFAULT_AUTH_REQUEST={"methodId":"api-key"}` flow unless the backend already
sets an explicit default request. The request contains no credential: Codex ACP
reads the key from its inherited process environment and authenticates only
when its own `account/read` reports that authentication is required. Psychevo
never copies the key into ACP `_meta`, events, diagnostics, or persisted state.

If `opencode` resolves and no explicit backend shadows it, Gateway materializes
an `opencode` ACP backend with command `opencode` and args `["acp"]`. Existing
Profile or Project definitions are never overwritten.

Known local ACP shortcuts are discovered from the Gateway's captured inherited
environment by resolving the `codex`, `opencode`, and `hermes` CLI commands.
Detection is cache-only: it must not launch a command, create an ACP session,
or access the network. Gateway materializes the corresponding built-in backend
only when its CLI resolves. The managed Codex adapter remains a separate
installation prerequisite: detecting `codex` makes the Codex ACP profile
discoverable with an install recovery action, but does not imply that
`codex-acp` is installed. Existing Profile or Project backend definitions,
including definitions created by an earlier Gateway, are retained when the
current environment no longer resolves the CLI; detection gates only new
automatic materialization.

The Workbench ACP Backend row uses one enablement slot for managed Codex
readiness: a missing adapter shows an explicit `Install` action, an invalid
adapter shows `Repair`, and a verified adapter shows the normal enablement
switch. Import Agent session uses the existing structured `backend/install`
recovery action to offer the same Codex installation without requiring the
user to leave the import surface.

## Retired Direct Runtime Hard Cut

Direct Codex app-server and OpenCode HTTP/SSE Adapters are removed. The
`psychevo-runtime-host` crate, direct worker/process code, direct Runtime Profile
kinds, direct Gateway branches, direct protocol operations, and direct UI/test
fixtures must not remain as dormant fallback paths.

This hard cut does not prohibit an internal Codex capability broker used only
for Codex-owned plugin catalog and Apps behavior. The broker does not submit
model turns, expose a Runtime Profile, own Agent history, or interpret a Codex
thread as a Psychevo Agent session. Its hidden ephemeral thread exists only
when an app-backed MCP call requires Codex thread identity.

Configuration containing `runtime = "codex"` or `runtime = "opencode"` fails
with `adapter_removed` and points to the corresponding ACP profile. This is a
pre-release incompatible cutover: persisted direct bindings, sessions, and
direct-only coordination state are not migrated. An incompatible development
database fails with the standard backup-and-reset recovery instruction.

A direct native session id is never interpreted as an ACP session id. No silent
fallback to Native, automatic session migration, or dormant direct reader is
allowed.

Generic fact reduction, delivery classification, history fidelity, and product
projection work may move into the Thread Application implementation after
direct naming and assumptions are removed.

## Workbench And Channels

The TypeScript client owns a headless `ThreadController` that combines context,
transcript, controls, interactions, revisions, and pending-turn state. React
subscribes to its snapshot and dispatches intents. It does not pair definitions
and profiles, infer sendability, or branch on runtime names.

The production Workbench uses that controller as the sole reducer for the
selected Thread's turn lifecycle. Submission calls `beginTurn` before sending
`turn/start` and `acceptTurnStart` after the response; `gateway/event`,
`turn/result`, and `turn/error` enter `applyGatewayEvent`, `applyTurnResult`, and
`applyTurnError` respectively. A first-turn event or terminal may arrive before
the `turn/start` response, and acceptance must bind the already-reduced snapshot
without discarding streamed entries or resurrecting a settled turn. React does
not independently invoke transcript or terminal snapshot reducers. A paced
live entry still queued when its terminal arrives is stale after that terminal
and cannot reopen or overwrite the settled turn.

Every independently rendered writable Thread surface, including right-workspace
child Agent and side-conversation panels, keeps one lifecycle-stable
`ThreadController` for that visible Thread. History hydration, optimistic prompt
creation, `turn/start` acceptance or rejection, and Gateway event reduction all
enter that same controller. A visible panel must not keep raw React transcript
state while a detached or throwaway controller owns its submitted turn. Headless
commands may target a non-visible Thread, but they do not create a second
optimistic owner for an already rendered Thread surface.

For selected-Thread activation, the entries carried by an authoritative
`thread/resume` or `thread/read` snapshot are sufficient history hydration for
the first render. Workbench binds that snapshot directly to the stable
controller and must not synchronously issue `thread/history/read` before
rendering it. Explicit search and lazy-history surfaces may still page through
`thread/history/read` independently.

Every writable Thread surface performs an authoritative same-Thread refresh
after applying a terminal event. This refresh replaces retained live entries
and activity with the declared history projection even when the terminal wire
payload carries no committed slice. Child and side Thread surfaces must not
remain visually running, nor admit a later optimistic prompt against stale
terminal-era activity, after Gateway already reports the Thread as idle. The
refresh is keyed by the visible Thread identity, not by whether stale local
turn correlation accepted the terminal reducer event.

Opening, starting, resuming, refreshing, or leaving a Thread replaces the
controller snapshot and resets its correlation state atomically before React
renders the new view. A no-op or foreign read-only refresh must not reset an
in-flight controller. A same-Thread authoritative refresh received while
`turn/start` acceptance is pending replaces visible data but preserves the
pending or already-settled acceptance correlation, so a delayed response cannot
resurrect a terminal turn. Detached shell notifications, terminal I/O, history
refresh, and other non-turn surfaces keep their existing owners and must not be
misclassified as Thread turn lifecycle events.

Workbench keys runtime-context reads by client, effective Thread scope, selected
Thread, prospective unbound target, and an explicit capability revision. A
bound Thread activation performs exactly one read for that key; applying the
returned binding or selected target is output, not a reason to repeat the read.
Settings and Workspace state refresh once per selected Thread scope, Agent and
backend catalogs refresh once per working directory or explicit mutation, and
commands refresh for their source, Thread, and running state. These auxiliary
reads must not be duplicated by both scope adoption and React effects.
After changing a bound Thread's Agent target, the resulting unbound draft
refreshes commands explicitly with `threadId: null`; a callback captured from
the prior bound render must not retain session-only commands such as `/btw`.
When a completed shell command refreshes the Agent/backend catalog, Workbench
also invalidates the selected Thread Context so `compatibleTargets` reflects
new or changed backend and Agent definitions.

When immutable Agent provenance changes, Workbench exposes the requested target
immediately in a disabled loading state. For a bound Thread, it then sends
`thread/start` and `thread/draft/prepare` in sequence before awaiting Settings,
Workspace, Observability, history, catalog, or command refreshes. Preparation
failure preserves the requested identity and blocks submission with the
authoritative error.

Workbench renders all negotiated and certified descriptors. The Agent/Profile
selector becomes immutable provenance after binding; changing either starts a
new thread. Unsupported actions remain visible only when a useful recovery or
explanation exists.

Channels invoke the same Thread Application use cases. They render only
descriptors marked `stable && channelSafe`, use expiring short interaction
tokens, and submit the structured input parts the platform can faithfully
provide. `/agent` selects a top-level Agent; `/agents` remains the managed
subagent/team command. `/profile`, `/model`, `/mode`, and related controls are
descriptor-driven and contain no Native/Codex/OpenCode branches.

Context failure is an error, not `Uses runtime default`. Channel outbox and
final-delivery semantics are implementation-neutral.

## Authentication And Security

Credentials remain owned by the Agent or its approved authentication flow.
Psychevo stores no provider credential returned through ACP. Backend command,
args, environment, cwd, logs, events, diagnostics, metadata, and errors pass
through existing secret redaction and safe path handling.

Filesystem and terminal callbacks are limited by backend client capabilities,
captured Agent tool policy, Gateway permission policy, canonical workspace
roots, and source interaction policy. Unknown callbacks and unenforceable
permission scopes fail closed.

Terminal capability is advertised only when both the backend grant and
captured Agent tool policy allow command execution. The Adapter implements
stable-v1 create/output/wait/kill/release with direct argv launch, per-session
terminal ownership, bounded UTF-8 output, canonical in-workspace cwd, bounded
environment overrides, approval before spawn, and child-tree cleanup on every
release or connection shutdown path. A partial terminal implementation must
advertise `terminal=false` rather than accepting only a subset of methods.

## Validation

One deterministic conformance suite runs against Native and ACP fake Adapters
through the Agent Session Interface. It covers binding-before-prompt,
per-thread ordering, concurrent threads, exactly-one terminal, control
resolution, structured input, permissions, interactions, cancel, delivery
certainty, history, process exit, and shutdown.

ACP transcript fixtures cover stable v1, rejected outbound experimental v2, protocol
mismatch, resident process reuse, generation replacement, config
acknowledgement versus observation, typed content, replay/live ordering,
filesystem/terminal policy, unknown notifications, and unsupported capability.
Codex/OpenCode initialize fixtures are generated by
`scripts/generate_acp_capability_fixtures.py` only after exact package
identity/version and reviewed TypeScript capability markers pass. The committed
evidence manifest records SHA256 for every admitted source file. Default Rust
tests consume only the committed stable-v1 fixtures; the explicit generator
`--check` path uses local `.references` and fails on source or fixture drift.

Workbench and Channel tests send equivalent intents through their real ingress
and assert equal binding, control revision, delivery, terminal, and history
semantics. Targeted visual scenarios cover Native, Codex ACP, OpenCode ACP,
managed adapter missing/install recovery, unsupported capability, active-turn
next-turn control, history unavailable, and narrow viewport behavior.
When Workbench opens a bound Thread, it reads context with no prospective
target and adopts the binding-derived `targetId` ahead of any stale source-draft
selection retained by the client.

The xtask runtime live suite uses deterministic ACP processes for Codex/OpenCode
GUI, Channel, history, unknown-delivery, control, and capability-pack coverage.
Broad completion requires the Rust broad gate, full visual profile, full shared
live registry, and independent ACP browser proof. Real provider or credentialed
validation remains opt-in.

## Related Topics

- [001 Architecture](../001-architecture/spec.md)
- [021 Gateway](../021-gateway/spec.md)
- [027 ACP](../027-acp/spec.md)
- [028 Channels](../028-channels/spec.md)
- [051 Agents](../051-agents/spec.md)
