---
name: 240. pevo Web
psychevo_self_edit: deny
---

# 240. pevo Web

Define the concrete Web/Workbench product surface and the JavaScript frontend
platform used by browser and managed-Web shells.

## Scope

- Web Shell and Workbench product behavior
- JavaScript/TypeScript workspace boundaries for Web product UI clients
- shared protocol, client runtime, React components, and web-consumable assets
- host runtime, shell capability, and host storage abstraction for browser,
  managed Web, and PWA builds
- Workbench layout, visual direction, settings, status, files, review,
  terminal, command, capabilities, and debug surfaces
- PWA and browser app-shell build boundaries
- browser/frontend validation expectations

Out of scope:

- shared TUI/GUI UI foundation; this belongs to [022 UI](../022-ui/spec.md)
- shared transcript display model, rendering, and interaction contracts; these
  belong to [250 UI Display Model](../250-ui-display-model/spec.md),
  [260 UI Rendering](../260-ui-rendering/spec.md), and
  [270 UI Interaction](../270-ui-interaction/spec.md)
- managed Gateway lifecycle and launch bootstrap; this belongs to
  [220 pevo Gateway](../220-pevo-gateway/spec.md)
- native desktop or mobile project scaffolding; native Desktop belongs to
  [246 pevo Desktop](../246-pevo-desktop/spec.md)
- runtime execution, persistence schemas, provider behavior, or Gateway
  semantics
- public npm publishing guarantees

## Workspace Boundary

Psychevo uses a root JavaScript workspace with `apps/*` and `packages/*`.
The first app is `apps/workbench`. Shared packages are private workspace
packages in the first slice:

- `@psychevo/protocol`: generated strict JSON-RPC 2.0 envelopes, Gateway wire
  types, JSON Schema artifacts, and Ajv-backed runtime validators. Rust
  Gateway protocol types are the source of truth.
  Generated TypeScript schema modules are split by protocol domain under
  `src/generated/schemas/` and re-aggregated through `gatewaySchemas`; callers
  must not depend on a monolithic generated schema file. If schema artifacts
  grow past maintainable source-file limits, the Rust protocol generator must
  split them into finer domain modules or generated `$ref` companions while
  preserving the public `gatewaySchemas` lookup surface.
- `@psychevo/client`: typed Gateway WebSocket client, event store, host runtime
  reconnect handling, and request/notification orchestration. It does not own
  endpoint discovery, host storage, browser download/open helpers, clipboard,
  file pickers, notifications, or native shell lifecycle.
- `@psychevo/host`: host capability contract and first browser/managed-Web
  implementation. It owns endpoint discovery, download/open helpers, host
  storage, clipboard, file and image picking, notification requests, theme
  preference plumbing, platform information, window lifecycle hooks, and typed
  unsupported results for native-only capabilities such as arbitrary local
  file read/write and reveal-in-folder.
- `@psychevo/components`: controlled React panels and UI primitives. It does
  not own RPC, routing, local storage, or process startup.
- `@psychevo/assets`: web-consumable theme tokens, generated CSS variables,
  typed design-system metadata, syntax theme defaults, icon mapping, and
  references to canonical brand assets. Its generated outputs come from
  [075 `DESIGN.md`](../075-design-system/DESIGN.md).

All first-slice packages are `private: true`. Product code may change these
interfaces without semver compatibility until a later SDK or package publishing
topic declares otherwise.

The root `assets/` directory remains the canonical tracked brand asset source
defined by [075 Brand Assets](../075-design-system/brand-assets.md). `@psychevo/assets`
packages those assets and theme tokens for Web consumers; it does not replace
the canonical asset location.

## Host Runtime

Client runtime code is host-aware. Browser/PWA builds use the same application
source, but host-specific behavior goes through
`@psychevo/host` adapters:

- endpoint discovery and explicit endpoint overrides
- WebSocket and download URL construction
- local host storage
- clipboard and file/image chooser capabilities
- notification permission and display requests
- native file contract for future desktop/mobile shells
- shell-only flags such as service-worker and install affordance disabling

The first storage implementation defines a `HostStorage` interface and uses
localStorage for endpoint profiles, source selection, UI preferences, and
non-secret client state. Provider API keys, Gateway bearer tokens, and other
provider secrets must not be persisted in frontend storage. Settings forms may
temporarily hold user input, but durable secret persistence belongs to
Gateway/runtime configuration APIs and must return redacted views.

The browser/managed-Web host implements web-standard capabilities when
available and returns typed `unsupported` results for native-only operations.
The first slice does not introduce Tauri, Electron, Capacitor, Android, iOS,
Harmony, or desktop bridge dependencies.
Browser file picking is a web-standard host capability. It may return selected
`File` objects for Workbench attachments, but it must not expose arbitrary host
paths. Native shells may later provide path or bookmark based file contracts
through the same host boundary.

Host capability failures use a shared reason taxonomy across browser,
managed-Web, Desktop, and future shells: `unsupported`, `unavailable`,
`permissionDenied`, `canceled`, and `failed`. Browser hosts should keep
native-only capture methods as typed `unsupported`, report denied browser
permissions as `permissionDenied`, report user-canceled chooser flows as
`canceled`, and include a bounded message only when it helps the product surface
show the next useful action.

## Web And PWA Builds

The Web build may enable PWA installation and service-worker caching for static
app-shell assets only. API routes, WebSocket routes, session state, tokenized
URLs, and stateful responses are never service-worker cached.
Workbench Vite production builds use stable manual chunk boundaries for
third-party vendor code, icons, workspace packages, and generated protocol
schema groups so no ordinary production chunk exceeds Vite's default chunk-size
warning threshold. The build must not silence this warning by raising the
threshold when a maintainable chunk split is available. Workspace package
chunks must use explicit boundaries so client runtime chunks do not absorb
generated protocol schema modules or their validation vendor dependencies.
Lazy-loaded rich-renderer dependencies, including Mermaid and its parser,
layout, math, and diagram-rendering packages, must stay in named async vendor
groups instead of being absorbed into a monolithic fallback `vendor` chunk.
The initial production navigation must not request Mermaid, Terminal, or
off-screen Settings, Capabilities, Automations, Search, and right-workspace
implementation chunks. Those chunks load only when their owning surface or
content becomes visible. Under the deterministic production-build Chromium
harness, initial JavaScript encoded size is capped at 1.8 MB; the harness
records the resource set and byte total so intentional budget changes remain
reviewable rather than silently expanding the startup graph.
If an upstream lazy dependency ships a single generated ESM module that cannot
be split by maintainable package or feature boundaries, the warning limit may
be raised narrowly to the smallest stable value that admits that module after
all other large chunks have been split.

Native Desktop reuses Workbench through the host/runtime interface, but native
packaging, Tauri bridge behavior, and Desktop window lifecycle belong to
[246 pevo Desktop](../246-pevo-desktop/spec.md).

## Web Shell

The concrete Web Shell behavior is specified in [Web Shell](web-shell.md). The
attachment owns source binding, Workbench startup/reconnect behavior, runtime
controls, settings, files, status, commands, browser host interactions, global
session browsing, and live cross-surface session visibility.
Global session-start actions remain active from app-level surfaces such as
Settings and Automations. Starting a new session from those surfaces must create
the detached draft and return the main work area to the transcript instead of
leaving the user on the previous configuration page.
Workbench model controls are backed by explicit Gateway model resolution state:
only a resolved provider-qualified `provider/model` is a usable model-turn
target. Unconfigured or errored model resolution remains visible in the shell
as an explicit selection/unavailable state and must block prompt-turn startup
until the user chooses a concrete provider/model.
Profile/default model configuration, shared composer model state, and explicit
catalog-fetch UX are defined by [125 Model Config](../125-model-config/spec.md).

## Workbench Layout

Workbench layout, navigation, inspector, file review, terminal, settings, and
responsive shell behavior are specified in [Workbench Layout](workbench-layout.md).
The split keeps this product surface maintainable without changing the managed
Gateway lifecycle or transport contract.
Files-tree external-open actions use authenticated Gateway workspace RPCs rather
than browser `file://` URLs or a Workbench-native bridge. The Gateway owns the
real workspace path, file classification, available opener detection, and OS
launch, so the same Workbench behavior applies to Web and Desktop and executes
on the machine that owns the Gateway workspace. The frontend sends only a
workspace scope, a workspace-relative path, and a closed semantic action; it
never sends an executable path or shell command.
Capability management is specified in
[247 Capability Management](../247-capability-management/spec.md). Workbench
must compose skills, plugins, MCP, and toolset management through those domain
RPCs instead of introducing a generic capability aggregation RPC.

The Web implementation follows the architecture large-file limit from
[001 Architecture](../001-architecture/spec.md). Workbench app entrypoints
should remain composition roots over state, session, command, composer,
runtime, settings, and right-workspace modules rather than owning those domains
inline.

## Components

Web component boundaries, Workbench composition-root expectations,
app-local module layout, and client-side state-machine requirements are
specified in [Components](components.md). This includes shared session-title
overflow behavior and Web/TUI-aligned running activity indicators for history,
composer, and transcript Thinking/tool rows, including persisted elapsed labels
after tool blocks complete. Large app and package entrypoints should remain
thin aggregators around semantic modules.
Workbench resource creation uses shared component primitives for action
buttons, form fields, and create/edit panel shells so `New`, `Add`, `Install`,
`Connect`, and `Set up` flows stay visually and behaviorally consistent across
Sessions, Workspace, Automations, Settings, and Capabilities.

## Runtime Provenance And Attention

Composer obtains one atomic draft snapshot/context and presents one Agent
Definition entry point before the first turn. `ComposerSessionCoordinator`
owns only the draft-open/prepare readiness epoch and the one pending first-turn
waiter. `ThreadController` remains the sole owner of Thread context, transcript,
activity, admission, and turn acceptance. The compact target control
renders one visible Agent identity per compatible target: Native uses
`agentLabel`, while ACP appends `(ACP)` to that same Agent label without
repeating Runtime Profile provenance. After binding, the selector becomes
immutable provenance. Its only identity switch action opens a new draft; it
never mutates the current thread.

Selecting an unbound ACP target explicitly calls `thread/draft/prepare` and
adopts the returned complete context before delivering a turn. Loading disables
runtime controls but an input-backed Send action may capture one pending intent.
The same coordinator readiness boundary covers both atomic draft open and
explicit target preparation: a captured intent waits for the matching receipt
and can never fall through to the previously selected target context. An
explicit Session or view switch cancels the pending readiness token and releases
any waiting submit immediately; a stale open/prepare response cannot keep Send
disabled in the newly selected view.
Failure keeps the requested Agent identity visible, cancels automatic delivery,
blocks subsequent submit, and shows the bounded Gateway error. Reconnect may
reuse a cached prepared projection, but ordinary context refresh remains
side-effect free.

Opening a Session treats the `thread/resume` or `thread/read` snapshot as the
authoritative first-render Transcript. Workbench must not synchronously repeat
`thread/history/read` before committing that snapshot; paginated history reads
remain available to explicit search and lazy-history consumers. Runtime context
reads are keyed by the selected Thread, effective scope, prospective unbound
target, and an explicit capability revision. A bound Session performs one
context read on activation, and applying that response must not schedule the
same read again. Auxiliary Settings, Workspace, Observability, Agent catalog,
and command refreshes are deduplicated by their owning scope and do not block
the selected Transcript or Composer controls.

Workbench boot treats initialization and the first global session browse as
one bounded transaction. It starts `initialize` and the single initial
`thread/browser` request as soon as transport is connected, commits the
Sessions result without waiting for `thread/draft/open`, and suppresses
reactive context and auxiliary refreshes until the startup Thread snapshot is
stable.
Initialization carries both the canonical startup `cwd` used by requests and
its authoritative home-relative display form. Composer uses that display value
for the cold first frame; it does not wait for Settings or infer the process
home in the browser.
If the user explicitly selects a Session after browsing completes but before
initialization does, the startup view epoch is stale and Workbench must omit
the draft `thread/draft/open` request itself, not merely ignore its response. The
explicit `thread/resume` binding remains authoritative and cannot be cleared by
late startup work.
The stable target receives one context read. Workspace, Agent/backend, and
command catalogs load on demand for the surface that consumes them and coalesce
concurrent reads for the same scope. Capabilities owns its Agent administration
load: it completes the explicit `backend/list` materialization boundary before
reading Agent, Team, and Runtime Profile catalogs, and the parent shell does not
start a duplicate catalog load when the page mounts.

Changing immutable Agent provenance on a bound Session opens a new draft with
one exact target intent. The critical path is one `thread/draft/open` response;
Settings, Workspace, Observability, history, catalog, and command refreshes do
not run as part of New Session. The requested Agent identity appears in its
loading state as soon as the user selects it and remains visible if preparation
fails.

New Session keeps the last committed Composer environment rendered while draft
preparation is pending. This applies both within one workspace and when the user
chooses New Session for another workspace: Agent, Mode, Model, Reasoning,
Permission, Workspace, and branch must not reset to placeholders or defaults.
The previous complete environment remains rendered and disabled until the new
draft context and branch are ready, then all values are replaced in one commit
if the authoritative result differs. The request for another workspace still
uses that workspace's default target intent; retaining the previous rendering
must not turn the old Agent into an exact target request. Input and an
input-backed Send action remain usable while draft preparation is pending; the
first click captures one local submission intent. If text, attachments, target,
control, workspace, or draft epoch changes before readiness, Workbench cancels
automatic submission and preserves the current draft. Otherwise it rechecks
the authoritative context and calls `turn/start` once. Input clears only after
Gateway accepts the turn, and uncertain delivery is never retried
automatically. A newer draft supersedes any unaccepted local intent and stale
open/prepare results cannot replace its state.
Attachment identity and order are part of the captured input signature: adding,
removing, or replacing an attachment while the click is pending invalidates the
captured submission just like a text edit does.

New Session does not refresh history or omnibus Settings. History refreshes
after the first accepted turn; cwd renders from canonical origin immediately;
the lightweight current Git branch read starts alongside `thread/draft/open`,
while non-visible model/settings metadata loads only when its owning popover
opens. Workbench applies the draft context and current branch in one Composer
environment commit so Agent, Mode, Model, Reasoning, Permission, Workspace, and
branch do not visibly pop in as separate startup generations.
Completion performs a local active-token check and calls `completion/list`
only for `/`, `$`, or `@` tokens.

The capsule is the only new runtime visual signature. It uses existing
Workbench type, spacing, and semantic status colours rather than runtime brand
colours. Runtime controls keep effective value/source, local draft override,
mutability, apply scope, stability, and Channel safety distinct. Shared
Attention carries runtime/profile and parent/child origin and states the exact
authorization lifetime.

An unbound draft Session presents Composer as the center-stage action instead
of pinning it to the bottom of an otherwise empty Transcript. After the first
prompt is accepted and the Thread becomes bound, the same Composer moves to its
ordinary bottom dock with a short positional transition. The transition uses
the existing Composer surface, does not remount or clear its draft state, and
is omitted when reduced motion is requested.

Composer keeps Agent and Mode beside the `+` attachment control; the grouped
Model/Reasoning selector plus Context remain on the right. Permission mode moves
to the quieter environment line after Workspace and Git branch. The environment
line orders these controls as Workspace, branch, then Permission. A displayed
workspace path replaces an exact host user-home prefix with `~`, while its title
and request value retain the canonical path. The visible path may use a wider
bounded measure than the other status controls before applying a single-line
ellipsis. Permission is not duplicated inside the Agent target popover. On an
unbound draft, Workspace opens
a switcher of known workspaces and ends with `Open workspace...`; choosing a
workspace starts a detached draft in that cwd, while opening a new workspace
opens a folder-selection panel at the active cwd. The panel supports traversing
folders across the filesystem visible to the Gateway process, including parent
folders up to the filesystem root, and does not fall back to free-form path
entry. Once a Thread is bound, Workspace keeps its
existing Files-opening behavior and cannot retarget the Thread.

The Git branch control opens a local-branch switcher for the active workspace
and ends with `New branch...`. Branch checkout and creation use structured
Gateway workspace operations, refresh the displayed project state on success,
and surface bounded Git failures without manufacturing a successful selection.
Mutation is unavailable while a turn is running. These footer selection triggers are borderless,
chevron-free, and intrinsically sized to the current value with bounded narrow
viewport truncation; action controls such as add, dictation, and Send retain
their existing icon-button treatment. Mode focus removes the rectangular
outline and uses the same muted background state as the Agent trigger, keeping
keyboard focus visible without restoring a frame. An enabled Mode trigger also
uses the Agent trigger's pointer cursor and muted hover background; disabled
Mode retains the not-allowed cursor. Neither trigger adds a hover box shadow.
Focusing the prompt textarea does not add
a nested focus outline; the persistent input frame remains its only visible
boundary. Model and Reasoning render from the
selected target's Thread Context descriptors through the same picker used by
Settings assignment rows. Reasoning is selectable only when its descriptor is
selectable, renders the effective value without interaction when read-only, and
is omitted when absent. The display priority is a local pending value, then the
Thread Context `effectiveValue`, then an authoritative runtime readback; the
client never invents a `Default` or first-choice reasoning value. `none` renders
as `Default` only when the descriptor explicitly returns `none`.

Compact Workbench popovers use one interaction and appearance contract across
Add, Agent, Mode, Model/Reasoning, Context, Workspace, Permission, Branch, and
completion surfaces. Selection controls use rendered popups instead of native
`select` popups (including select-only combobox/listbox semantics where
appropriate) so panel, row, selected, hover, and focus states are consistent
and opening one popup dismisses another. Escape, outside pointer, selection,
and loss of an owning surface dismiss the popup. Compact selection panels size
intrinsically to their content up to a surface and viewport maximum. Completion
is the input's suggestion surface rather than a compact selector, so its left and
right edges align with the owning message-input frame at every viewport size.
Row labels remain one line, truncate with an ellipsis at their available maximum,
and expose the full value as a title when truncation is possible. Switch rows
reserve the final column for the switch and align every switch to the panel's
right edge.
Model shows a proven effective value or an explicit unavailable reason. An ACP
draft renders only Agent-provided config choices; Settings metadata may enrich
labels and grouping but never synthesizes values. An unbound draft sends only
explicit user choices. A bound change uses
`thread/control/set` as a sticky next-turn preference; an applied value is not
labelled observed until an authoritative update or readback confirms it.
Display-only Agent updates may refresh commands, history, title, or usage while
a picker is open without invalidating that picker. A genuine target, admission,
or control revision change remains an explicit error; Workbench does not hide it
behind an automatic refresh-and-retry.
Context fidelity is `exact`, `estimated`, `partial`, or `unavailable`. A known
token count without a limit renders the count plus `Limit unavailable`, never a
fabricated `0%`. Ordinary GUI exposes an experimental runtime control only when
the exact runtime-version gate and capability gate both match.

Agent-authoritative transcript blocks may keep public phase ordinals. One phase
stays visually flat. Multiple phases expose a collapsed `Show agent phases`
action and neutral `Phase N` labels when expanded. Raw Adapter phase names and
Thinking are never reused as Channel delivery content.

The leader-first panel visually separates controllable Psychevo-managed members
from capability-gated Agent-native activity. A read-only Agent child tab omits
composer and control affordances and loads history lazily.

React is a composition root. Runtime target pairing, sendability, draft open
epochs, pending submission, control resolution, revisions, turn serialization,
and runtime-name decisions do not live in `App.tsx` or presentation Modules.
Native and ACP use the same semantic control placement and
disabled/unavailable treatment.

The Sessions header contains one `Imported and archived sessions` view toggle.
The toggle is the user intent that permits Agent session discovery; ordinary
active-history rendering does not scan ACP Agents. Switching views immediately
keeps the sidebar usable while archived Psychevo Threads and enabled ACP Agent
session catalogs load independently. The alternate list is ordered as one
`Archived` group followed by one group per ACP Runtime Profile. One failing or
slow Profile does not hide archived Threads or successful Profile groups, and
empty/error rows state the next action plainly.

Selecting an archived Thread reads and renders its Transcript without restoring
it. Selecting an ACP candidate atomically imports its durable `session/load`
replay as an archived Thread and then renders the same Transcript surface. An
`Activate` item in either row's secondary menu restores or imports the Thread
into the ordinary active list. Sending the first new message from an archived
Transcript performs that same activation before `turn/start`, so a read-only
visit never silently changes history membership. When a Profile has multiple
compatible targets, the group exposes only the necessary target selector and
defaults to the first ready target rather than the first syntactic target.

Session menus render Gateway lifecycle descriptors. Fork is visible only for a
negotiated fork-capable Agent. Delete remains visible but disabled with its
Gateway reason when an ACP Agent cannot delete its session. Remote deletion uses
an explicit confirmation that names both the Psychevo Thread and Agent-owned
session; Native deletion retains its existing local meaning. Current selection
alone does not disable Archive or Delete: both actions remain available for the
idle current Thread and become unavailable while that Thread is running.
Archiving the current Thread keeps its Transcript visible as an archived visit;
deleting it clears the selected Transcript and returns Workbench to an empty
new-session draft after confirmation.

## Critical First-Turn Journey Evidence

Workbench maintains one deterministic critical-journey proof for the first
turn. The proof records six user-facing checkpoints: `gui_ready`,
`draft_context_ready`, `send_clicked`, `runtime_request_dispatched`,
`first_output_visible`, and `turn_settled`. Draft readiness means the atomic
`thread/draft/open` context has been applied; it does not imply that a durable
Thread already exists. Runtime dispatch is proven at the receiving runtime
boundary: a Native test provider receives the HTTP request or an ACP test Agent
accepts `session/prompt`. `turnStarted` alone is not dispatch evidence.

The visible first-output checkpoint is the first non-empty assistant text
confirmed by the visual pass. The profiling pass measures the corresponding
surface commit: DOM commit in Workbench and terminal draw commit in TUI. A RAF
callback is only a presentation observation and must not be labelled as paint.
Raw runtime chunks, Gateway events, client receipt, controller application,
and an optional post-frame observation remain diagnostic marks. The settled
checkpoint requires authoritative `turnCompleted` application, stable final
content, an idle turn state, and a restored Composer.

Workbench state-application milestones are retained in a content-free internal
browser timing registry and mirrored to the User Timing API when the engine
supports reliable mark retention. Profiling derives Composer and Transcript DOM
commits from observed DOM conditions without adding a fixed frame delay. Native
browser drivers may read the registry without depending on engine-specific
`performance` entry buffering.

The journey covers a ready-then-send path and an input-backed Send captured
after New Session while its replacement draft is pending. Their required
checkpoint orders are respectively
`gui_ready -> draft_context_ready -> send_clicked ->
runtime_request_dispatched -> first_output_visible -> turn_settled` and
`gui_ready -> send_clicked -> draft_context_ready ->
runtime_request_dispatched -> first_output_visible -> turn_settled`. The
pending path must preserve input and deliver exactly one `turn/start` after the
same draft becomes ready.

Performance and visual evidence are separate passes over the same scenario
contract. The profiling pass introduces no screenshots or staged runtime waits
and retains raw samples, a latency waterfall, and an automation trace. The
visual pass deterministically holds request, first-output, and completion
boundaries long enough to capture all six checkpoints; screenshot capture time
is recorded separately and never contributes to product latency. The first
slice establishes a baseline without hard latency budgets.

Journey artifacts use an internal versioned manifest. Each event records its
clock source, epoch observation, and monotonic observation. Measurements never
subtract timestamps from different clock domains. Artifacts may contain
bounded request, Thread, Turn, runtime, provider, and model correlation ids,
but never prompts, generated text, credentials, tokens, or secrets. This
evidence contract does not add test timestamps to the public Gateway protocol.

## TUI And Workbench Profiling Comparison

The deterministic cross-surface profile compares the real fullscreen TUI and
desktop-Chromium Workbench against one shared Native provider fixture. TUI uses
the in-process Gateway API while Workbench uses managed Gateway,
WebSocket/JSON-RPC, client reconciliation, React commit, and DOM commit. The
report therefore treats the shared provider boundary as the control and
attributes additional Workbench time to the Web admission, transport, and
frontend stages rather than claiming the two surface call graphs are identical.

The common warm-path checkpoints are `send_committed`,
`runtime_request_dispatched`, `first_output_surface_committed`, and
`turn_settled_surface_committed`. `send_feedback_surface_committed` measures the
first optimistic running surface separately. Workbench-only diagnostics include
`turn/start` frame and acceptance, Gateway event receipt, controller batch
application, DOM commit, optional post-frame observation, and
`draft_context_ready`. TUI-only diagnostics include terminal-event drain and
terminal draw commit. Where
available, both surfaces retain the shared `Gateway::run_turn` entry and
Gateway-event emission boundaries.

Every measured sample has an explicit correlation that distinguishes the main
turn provider request from asynchronous title generation. Exactly one main
request may satisfy a sample. Browser timing state is reset per sample, the
interaction start is captured inside the browser submit handler rather than
before Playwright actionability checks, and trace collection uses a separate
diagnostic sample excluded from percentiles. A provider response schedule must
leave an observable interval between first output and completion so completion
cannot collapse queued streaming evidence before the first surface commit.

Comparison manifest schema v2 contains raw TUI and Workbench samples,
p50/p95 waterfalls, Workbench-minus-TUI deltas and ratios, environment and
fixture fingerprints, content-free diagnostic marks, the TUI JSONL trace, and
the Workbench Playwright trace. Durations are calculated only between marks in
one clock domain; cross-process boundaries use observations made by the common
runner and remain labelled as runner-observed. The initial baseline has no
absolute latency gate. Missing artifacts, ambiguous request correlation,
duplicate main requests, unsafe fields, or an incomplete waterfall fail the
profile while preserving partial evidence. Comparison v1 and v2 samples must
never be aggregated because v1 included browser frame waits in its visible
metrics.

Each raw sample also correlates one Gateway Turn and derives two diagnostic
sub-waterfalls. The shared Gateway/runtime sub-waterfall covers Gateway entry,
Native Adapter submission, runtime configuration, Agent start, prompt
projection, first visible assistant event, and authoritative completion. The
surface sub-waterfall covers first non-empty assistant receipt, controller
application, surface commit, completion receipt/application, and settled
surface commit. These
sub-waterfalls use only their owning Gateway or surface clock and are summarized
independently from runner-observed cross-process spans.
The comparison hard-fails duplicate public lifecycle, legacy terminal
notifications, Web review scans on admission/relay/completion, missing first
feedback, or hidden-surface request amplification. Native runtime undo snapshot
work remains a shared TUI/Workbench stage. Latency p50/p95 stays report-only
until three stable canonical-runner baselines are explicitly approved for a
separate ratchet.

Workbench does not paint a cold-start Composer with placeholder Agent or empty
runtime controls. Session history never gates Composer visibility, but the
initial atomic draft context does: the first visible editable Composer is one
committed environment containing Agent, Mode, Model, Reasoning, Permission,
Workspace, and current branch. That commit defines both `gui_ready` and the
initial `draft_context_ready`; the app shell may paint earlier without claiming
Composer readiness. Initialization and the first session browse overlap, and
draft open waits for history only when no usable launch scope exists.
Same-workspace New Session retains the active source identity and the last
committed Composer environment while replacement readiness is pending. The
active scope, rather than a stale Settings snapshot, owns the visible workspace
path. Runtime and environment controls are disabled during that interval, and
the replacement context commits atomically when ready.

Submitting a ready draft performs one local Thread transaction before network
delivery: it appends the optimistic user entry and sets provisional activity to
running with a local start time and no authoritative Turn id. The next DOM
commit therefore shows running feedback and `0s`. If submission is captured
while draft readiness is pending, Composer instead shows `Preparing` with an
elapsed timer, preserves the input, and does not expose Interrupt or call
`turn/start`. Readiness transfers the original click time into the single local
Thread transaction so elapsed time never resets. Gateway acceptance or the
single `TurnStarted` binds the real id; pre-acceptance failure restores the
previous snapshot, and terminal-before-acceptance still settles without
reviving provisional activity. Only a coordinator-owned draft open or target
preparation may enable this pending submission path; unrelated context or
control mutations keep Send disabled until their authoritative result applies.

`ThreadController` is the only Thread reducer. Its batch application reduces
replaceable observations and publishes one snapshot notification. The
Workbench-local `useGatewayLiveEvents` hook owns the one scheduling queue: the
first non-empty assistant text bypasses frame pacing, later replaceable updates
coalesce per entry, and terminal observations flush same-Turn output before
completion. The hook does not own session, resource, or selector state.

Session rows patch only fields carried authoritatively by acceptance,
`activityChanged`, `titleChanged`, or `turnCompleted`. A bound Native
continuation performs no `thread/read`, `thread/browser`, or context read. The
first detached Turn performs one history browse after acceptance and one
context read after completion. ACP completion may perform one context read for
Agent-session controls and capabilities. Fixed settle delays do not exist.

Resource reads follow visible demand. A closed right Workspace reads nothing
unless the visible Transcript contains an unresolved file link. Workspace Home
reads Diff plus Observability, Review reads Diff plus Changes, Files reads the
file inventory, and other tabs read none of those resources. Reads remain
single-flight, latest-wins, and view-epoch guarded. Turn completion reevaluates
the visible transcript together with committed entries; when either contains
workspace-file demand, the same-workspace inventory refreshes once even while
Files is closed, so created and deleted paths do not leave transcript actions
stale. A completed supported file-tool entry triggers the same refresh
immediately, before the enclosing turn completes. A completion without file
demand does not add a hidden workspace read. The always-visible Composer
environment owns one lightweight `workspace/git/branches` read for a new draft;
this is not a right-Workspace demand or an omnibus Settings refresh.

## Visual Direction

The Workbench visual language, density expectations, token usage, responsive
behavior, and browser validation expectations are specified in
[Visual Direction](visual-direction.md). Shared visual principles remain in
[075 Design System](../075-design-system/spec.md). Workbench appearance is a
frontend preference with three concrete palettes: `dark`, `light`, and `warm`.
`warm` preserves the original reading-paper light palette, while `light` is a
neutral paper-warm light shell with very low-chroma warm white surfaces, a
paper-warm sidebar that avoids gray drift, and low-contrast warm-gray borders.
`light` keeps neutral text and accent semantics, and remains much less warm
than the ivory/taupe `warm` palette. All appearances share the same readable
Workbench typographic scale so theme switching does not change font size, line
height, or row density.
Workbench, shared components, and embedded terminal panels consume generated
`@psychevo/assets` tokens. Product CSS should use `--pevo-*` semantic
variables; embedded xterm palettes should come from the typed design-system
export rather than isolated Workbench literals.

## Validation

Workbench Settings includes a compact profile-scoped Web Search section and
never receives raw search credentials. Assistant URL citations are clickable;
remote image results load thumbnails only after expansion. See
[111 Web Search](../111-web-search/spec.md).

Frontend validation uses deterministic local harnesses by default. Unit tests
cover generated protocol validators, client reconnect/pending request behavior,
host storage, and component rendering. Browser tests use Playwright against the
built Workbench served by `pevo gateway open --no-browser --print-url`, with
isolated config, SQLite state, and cwd by default. They cover desktop and
narrow viewport layout, Gateway connection, source/thread startup, history
management, composer submission, permission/clarify surfaces, and download
flows.

Browser fixtures that run as separate processes are maintained as standalone
sources under `apps/workbench/e2e/fixtures`. Specs and support modules may copy
those sources into isolated artifact roots and parameterize them through
arguments, environment, or state files, but must not embed complete executable
program bodies in multiline string literals.

Live provider browser validation is opt-in only. It may reuse the user's real
Psychevo config and credentials, but must still use an isolated cwd and
repo-local test state unless the caller explicitly chooses otherwise.

Targeted desktop and narrow-viewport proofs cover the import surface, grouped
partial results, target selection, refresh, fork/delete capability differences,
and remote-delete confirmation. Permanent spec filenames, describe labels,
request ids, and required-proof names describe behavior and never include an
implementation date or planning-batch id.
Long-running live skill validation uses a reusable Playwright spec that samples
the page every three seconds, stores screenshots, and checks each sampled
transcript against the message-derived SQLite transcript so transient row-order
regressions cannot be hidden by a correct final screenshot. It must also fail
when tool result JSON appears in a collapsed header, long evidence headers
overflow, a committed turn slice fails to replace live overlay rows, an empty
assistant update appears after a tool row, or a stale completion popover remains
after prompt submission.

## Attachments

- [Components](components.md)
- [Visual Direction](visual-direction.md)
- [Web Shell](web-shell.md)
- [Workbench Layout](workbench-layout.md)
- [Testing](testing.md) defines Web/Workbench validation expectations.
- [247 Capability Management](../247-capability-management/spec.md) defines the
  Workbench skills, plugins, MCP, and toolset management surface.

## Related Topics

- [021 Gateway](../021-gateway/spec.md) defines Gateway thread, turn, source,
  and transport semantics.
- [022 UI](../022-ui/spec.md) defines shared UI foundation and source-of-truth
  boundaries.
- [070 Experience](../070-experience/spec.md) defines shared UX/DX defaults.
- [075 Design System](../075-design-system/spec.md) defines shared visual and
  interaction language.
- [075 Brand Assets](../075-design-system/brand-assets.md) defines canonical brand asset
  locations.
- [125 Model Config](../125-model-config/spec.md) defines saved model defaults,
  shared composer model state, provider setup UX, and catalog-fetch UX.
- [249 Vision and Image Artifacts](../249-vision-and-image-artifacts/spec.md)
  defines image attachment thumbnails and generated-image transcript cards.
- [250 UI Display Model](../250-ui-display-model/spec.md) defines shared
  transcript projection and display-only boundaries.
- [260 UI Rendering](../260-ui-rendering/spec.md) defines shared transcript,
  evidence, status, and observability rendering invariants.
- [270 UI Interaction](../270-ui-interaction/spec.md) defines shared composer,
  command, permission/clarify, and interrupt semantics.
- [280 Channel UX](../280-channel-ux/spec.md) defines Settings > Channels
  behavior.
- [220 pevo Gateway](../220-pevo-gateway/spec.md) defines managed local Web
  launch lifecycle.
- [221 pevo Serve](../221-pevo-serve/spec.md) defines the headless API server.
