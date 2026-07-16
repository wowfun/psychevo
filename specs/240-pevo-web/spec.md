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

Composer reads `thread/context/read` through the headless client
`ThreadController` and presents one Agent Definition entry point before the
first turn. The compact target control renders one visible Agent identity per
compatible target: Native uses `agentLabel`, while ACP appends `(ACP)` to that
same Agent label without repeating Runtime Profile provenance. The visible
popover omits redundant target headings and management links; its accessible
name remains `Agent target` and full provenance remains available as a title.
After binding, the selector becomes immutable provenance. Its only identity
switch action starts a new thread; it never mutates the current thread.

Selecting an unbound ACP target explicitly calls `thread/draft/prepare` and
adopts the returned complete context before enabling submit. Loading disables
runtime controls. Failure keeps the requested Agent identity visible, blocks
submit, and shows the bounded Gateway error. Reconnect may reuse a cached
prepared projection, but ordinary context refresh remains side-effect free.

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
Sessions result without waiting for `thread/start`, and suppresses reactive
context and auxiliary refreshes until the startup Thread snapshot is stable.
If the user explicitly selects a Session after browsing completes but before
initialization does, the startup view epoch is stale and Workbench must omit
the draft `thread/start` request itself, not merely ignore its response. The
explicit `thread/resume` binding remains authoritative and cannot be cleared by
late startup work.
The stable target receives one context read. Workspace, Agent/backend, and
command catalogs load on demand for the surface that consumes them and coalesce
concurrent reads for the same scope.

Changing immutable Agent provenance on a bound Session starts a new Thread and
immediately prepares the requested draft target. The critical path is
`thread/start` followed by `thread/draft/prepare`; Settings, Workspace,
Observability, history, catalog, and command refreshes run after or alongside
that path without being awaited between those two requests. The requested
Agent identity appears in its loading state as soon as the user selects it and
remains visible if preparation fails.

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
to the quieter environment line immediately before Workspace. It is not
duplicated inside the Agent target popover. On an unbound draft, Workspace opens
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

React is a composition root. Runtime target pairing, sendability, control
resolution, revisions, turn serialization, and runtime-name decisions do not
live in `App.tsx` or presentation Modules. Native and ACP use the same semantic
control placement and disabled/unavailable treatment.

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
