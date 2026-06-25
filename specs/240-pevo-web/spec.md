---
name: 240. pevo Web
psychevo_self_edit: deny
---

# 240. pevo Web

Define the concrete Web/Workbench product surface and the JavaScript frontend
platform used by managed Web and future generic Desktop/Mobile shells.

## Scope

- Web Shell and Workbench product behavior
- JavaScript/TypeScript workspace boundaries for Web product UI clients
- shared protocol, client runtime, React components, and web-consumable assets
- host runtime, shell capability, and host storage abstraction for browser,
  managed Web, PWA, and future shell builds
- Workbench layout, visual direction, settings, status, files, review,
  terminal, command, and debug surfaces
- PWA and generic app-shell build boundaries
- browser/frontend validation expectations

Out of scope:

- shared TUI/GUI UI foundation; this belongs to [022 UI](../022-ui/spec.md)
- shared transcript display model, rendering, and interaction contracts; these
  belong to [250 UI Display Model](../250-ui-display-model/spec.md),
  [260 UI Rendering](../260-ui-rendering/spec.md), and
  [270 UI Interaction](../270-ui-interaction/spec.md)
- managed Gateway lifecycle and launch bootstrap; this belongs to
  [220 pevo Gateway](../220-pevo-gateway/spec.md)
- native desktop or mobile project scaffolding
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
- `@psychevo/assets`: web-consumable theme tokens, CSS variables, syntax theme
  defaults, icon mapping, and references to canonical brand assets.

All first-slice packages are `private: true`. Product code may change these
interfaces without semver compatibility until a later SDK or package publishing
topic declares otherwise.

The root `assets/` directory remains the canonical tracked brand asset source
defined by [075 Brand Assets](../075-design-system/brand-assets.md). `@psychevo/assets`
packages those assets and theme tokens for Web consumers; it does not replace
the canonical asset location.

## Host Runtime

Client runtime code is host-aware. Browser/PWA and generic app-shell builds use
the same application source, but host-specific behavior goes through
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

## Web, PWA, And Shell Builds

The Web build may enable PWA installation and service-worker caching for static
app-shell assets only. API routes, WebSocket routes, session state, tokenized
URLs, and stateful responses are never service-worker cached.
Workbench Vite production builds use stable manual chunk boundaries for
third-party vendor code, icons, workspace packages, and generated protocol
schema groups so no ordinary production chunk exceeds Vite's default chunk-size
warning threshold. The build must not silence this warning by raising the
threshold when a maintainable chunk split is available.

The generic shell build reuses the same React/Vite source with an explicit
Gateway endpoint requirement. Shell builds disable service workers, PWA install
prompts, and browser-only origin inference. Native Android, iOS, Harmony, or
desktop bridge projects are deferred.
Generic Desktop shell capability is therefore implemented first by sharing the
same Workbench source, protocol client, host adapter contract, and components
used by managed Web. A feature that works in the shared Workbench path is
available to future Desktop shells when the shell host supplies an explicit
Gateway endpoint and source scope; native packaging remains outside this topic.

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

## Validation

Frontend validation uses deterministic local harnesses by default. Unit tests
cover generated protocol validators, client reconnect/pending request behavior,
host storage, and component rendering. Browser tests use Playwright against the
built Workbench served by `pevo gateway open --no-browser --print-url`, with
isolated config, SQLite state, and workdir by default. They cover desktop and
narrow viewport layout, Gateway connection, source/thread startup, history
management, composer submission, permission/clarify surfaces, and download
flows.

Live provider browser validation is opt-in only. It may reuse the user's real
Psychevo config and credentials, but must still use an isolated workdir and
repo-local test state unless the caller explicitly chooses otherwise.
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
