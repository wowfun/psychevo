---
name: 022. UI
psychevo_self_edit: deny
---

# 022. UI

Define Psychevo's shared client-side UI platform for Web, future Desktop
shells, and future Mobile shells.

## Scope

- JavaScript/TypeScript workspace boundaries for product UI clients
- shared protocol, client runtime, components, and web-consumable assets
- host runtime, shell capability, and host storage abstraction for browser,
  managed Web, PWA, and future shell builds
- PWA and generic app-shell build boundaries
- frontend validation expectations

Out of scope:

- concrete Web Shell product behavior; this belongs to
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
defined by [085 Brand Assets](../085-brand-assets/spec.md). `@psychevo/assets`
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

## Web, PWA, And Shell Builds

The Web build may enable PWA installation and service-worker caching for static
app-shell assets only. API routes, WebSocket routes, session state, tokenized
URLs, and stateful responses are never service-worker cached.

The generic shell build reuses the same React/Vite source with an explicit
Gateway endpoint requirement. Shell builds disable service workers, PWA install
prompts, and browser-only origin inference. Native Android, iOS, Harmony, or
desktop bridge projects are deferred.

## Components

Shared components are controlled. They receive state and callbacks from an app
or client store and do not instantiate Gateway clients, read localStorage, or
write global config.

First-slice component families include timeline transcript, tool evidence,
artifact preview/detail, debug drawer, composer, history, status/queue,
settings, diff/export/share, permission, clarify, tabs, buttons, inputs, and
layout primitives. Components should support desktop density and mobile/shell
collapse without requiring a separate native component tree.

Ordinary transcript components consume typed timeline items and typed Gateway
events. They must not display raw runtime event names such as `runtimeRaw`,
`itemCompleted`, or `turnCompleted` as user-facing transcript content. Raw
diagnostics belong in the debug drawer.

## Visual Direction

The first Workbench visual direction is an operator ledger: quiet, dense,
light-mode workspace chrome with a restrained ink/teal/brass palette, typed
timeline rows as the primary surface, and debug/status details held in secondary
panes. It is an app shell, not a landing page; the first viewport orients the
user, shows connection/thread status, and enables the next turn without hero
copy or decorative backgrounds.

Surface hierarchy uses background-color steps, fine dividers, and restrained
shadow. Cards are reserved for bounded repeated items, timeline evidence,
requests, and drawers; page sections should read as panes or rows rather than
generic floating cards. Buttons use a consistent radius scale and press feedback
without resizing their layout footprint.

Mobile uses the same component tree with compact chrome: top status must not
crowd the composer or tab rail, the active panel owns the viewport, and debug
details remain opt-in through the drawer. Desktop uses a persistent
history/transcript/status layout where the transcript stretches to the available
height and the composer is a bottom control dock.

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

## Related Topics

- [021 Gateway](../021-gateway/spec.md) defines Gateway thread, turn, source,
  and transport semantics.
- [070 Experience](../070-experience/spec.md) defines shared UX/DX defaults.
- [080 Design System](../080-design-system/spec.md) defines current TUI design
  language and shared experience constraints.
- [085 Brand Assets](../085-brand-assets/spec.md) defines canonical brand asset
  locations.
- [220 pevo Gateway](../220-pevo-gateway/spec.md) defines the concrete Web Shell.
