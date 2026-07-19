---
name: 246. pevo Desktop
psychevo_self_edit: deny
---

# 246. pevo Desktop

Define Psychevo's native GUI desktop shell for macOS, Windows, and Linux.

## Scope

- Tauri-based native desktop application packaging and lifecycle
- Desktop window model for Workbench and Floating feature surfaces
- Desktop host adapters for Gateway transport, storage, clipboard, file/open,
  notifications, floating capture, platform diagnostics, and window lifecycle
- managed Gateway startup, owner-only token handling, and native bridge
  request/notification routing
- Desktop validation expectations

Out of scope:

- Web/managed-Web packaging and preview-only browser fallback; this belongs to
  [240 pevo Web](../240-pevo-web/spec.md)
- Floating capsule product behavior; this belongs to
  [245 pevo Floating](../245-pevo-floating/spec.md)
- Gateway source/thread semantics; this belongs to
  [021 Gateway](../021-gateway/spec.md)
- provider behavior, runtime execution, or storage schemas

## Product Model

`pevo Desktop` is the native GUI shell for Psychevo. It hosts the full Workbench
main window and lightweight feature windows such as Floating. Desktop is not a
separate runtime path: interactive work still routes through Gateway, and
Workbench/Floating use the same client semantics as other product surfaces.

Desktop support covers macOS, Windows, and Linux. WSL2/WSLg is treated as a
Linux runtime environment, not as a separate product target. Linux capture is
split into X11 and Wayland adapter families, and each capability must report
what the detected display/session can actually provide.

The Desktop shell owns native process concerns. Product feature modules own
their interaction logic. A feature such as Floating may be mounted in a Desktop
window, but it must not own a standalone Tauri application scaffold.

## Workspace Boundary

The Desktop app lives under `apps/desktop/`. Its Tauri project lives at
`apps/desktop/src-tauri/`, because `apps/desktop/` is the Tauri application
root inside the repo workspace.

Shared feature and UI logic live in private workspace packages:

- `@psychevo/floating` owns Floating reducer, attachment mapping, prompt
  compilation, placement helpers, and React capsule UI.
- `@psychevo/workbench` remains the Workbench product implementation. Desktop
  reuses it through an injected host/runtime interface instead of copying or
  forking Workbench source.
- `@psychevo/assets` owns generated design-system tokens from
  [075 `DESIGN.md`](../075-design-system/DESIGN.md), including shared CSS
  variables and typed metadata used by Workbench and Floating.
- `@psychevo/client` owns Gateway request/notification orchestration and
  accepts browser or native bridge transports.
- `@psychevo/host` owns host capability interfaces and browser defaults;
  Desktop supplies native adapters.

The repo root is not a Tauri app root. It remains the Rust and JavaScript
workspace root.

The Desktop development server must not watch Rust build artifacts under
`apps/desktop/src-tauri/target/`. Cargo writes and replaces native executables
inside that directory while Vite is running, and Windows may reject a watcher
that races a locked executable. Tauri source and configuration remain watched;
only the generated Cargo target tree is excluded.

The Desktop Cargo library and binary targets must retain distinct artifact
names after Cargo normalizes hyphens to underscores. This prevents their MSVC
debug-symbol outputs from resolving to the same `.pdb` path on Windows.

## Desktop Shell

The first Desktop slice creates two native webview surfaces:

- Workbench main window: full product shell reused from `@psychevo/workbench`
- Floating capsule window: compact, always-on-top feature window mounted from
  `@psychevo/floating`

The Floating native window starts compact and is resized by the renderer to the
capsule's measured content size as the toolbar, running, expanded, parked, and
error states change. The renderer may request native window dragging from blank
toolbar regions and may request that the native Floating window be hidden when
the user closes the capsule. Desktop grants only the minimum native window
permissions needed for this behavior, including lowering the Floating window's
native minimum size before applying compact content-fit sizes and hiding the
Floating window on close. Floating windows must
not expose transparent webview gutters around the capsule because WSLg/WebKitGTK
may render those transparent pixels as black. When WebKitGTK/WSLg still enforces
a larger native minimum height, Desktop keeps the Floating root or capsule
filling the bounded native viewport with the Floating surface background so
transparent webview space is not rendered as a black block below or around the
controls.

Desktop imports shared assets plus the surface CSS for Workbench and Floating.
Surface packages must keep selectors scoped or prefixed so a shared Desktop
renderer can load both without collisions. Desktop fallback/error chrome uses
the generated `--pevo-*` variables rather than a separate native palette.
Desktop constrains embedded Workbench shells to the native webview height
through parent-relative sizing instead of relying on browser dynamic viewport
units, because WebKitGTK/WSLg can report `dvh` larger than the actual webview
viewport. Desktop also locks the renderer root document to the native webview
viewport so WebKitGTK cannot scroll the `html` element past the bounded
Workbench shell.
The native app icon uses a square bundle icon generated from the shared
`assets/psychevo-logo.png` asset, rather than a Desktop-local source icon, so
Linux AppImage packaging has a square icon candidate.

Both surfaces connect through the native Desktop Gateway bridge. The bridge
keeps the managed bearer token in Rust and never exposes it to webviews. Each
webview connection has an independent sender and event route so a Workbench
window and Floating window cannot overwrite each other's active Gateway
connection. Sender keys are unique per bridge transport instance, not only per
window label, so a stale renderer cleanup cannot remove a newer connection that
uses the same surface.
Request/response frames remain scoped to the initiating bridge connection, but
thread-affecting Gateway notifications are shared across Desktop surfaces.
Desktop fans out thread-affecting `gateway/event` notification frames to other
bridge transports with an origin connection id so the sender can ignore its own
copy. Accepted Turns have no parallel `turn/result` or `turn/error` broadcast;
all surfaces settle from the single authoritative `TurnCompleted`. This keeps
JSON-RPC pending requests isolated while allowing Workbench and Floating to
reconcile the same thread through the shared client transcript pipeline.

Floating is a Desktop-specialized small window, not a second chat
implementation. Desktop must wire Floating and Workbench to shared
Thread/Transcript helpers wherever they submit turns or apply live/completion
events. Desktop supplies Floating with the same current turn controls that
Workbench uses for model, runtime reference, runtime session, runtime options,
reasoning effort, permission mode, and work mode so Floating turns do not drift
to a different provider/runtime path. A Floating "open in main window" runtime
action focuses the Workbench window, opens the current thread in the main
transcript view, and leaves the Floating window open.
Floating close is a dismiss action: Desktop hides the native Floating window and
does not leave a renderer-only logo restore surface visible. Floating park
remains the recoverable minimized-logo state.

The CLI is the sole authority for managed Gateway lifecycle and process
ownership. Outside explicit overrides, Desktop always invokes the idempotent
`pevo gateway start` command and consumes its `baseUrl` plus the owner-only
token file; it does not read `server.json`, mirror `ManagedServerState`, compare
executable fingerprints, or decide whether a recorded pid is reusable. This
keeps Windows Job/HANDLE and Unix process-group ownership policy in one place.
Within a single Tauri process, managed Gateway resolution is serialized and the
verified endpoint is cached so concurrent Workbench and Floating startup calls
cannot race each other into multiple cold-start attempts or observe different
Gateway generations. When a cached endpoint becomes unhealthy, Desktop
delegates to the CLI again instead of reusing persisted state itself.
Explicit `PSYCHEVO_GATEWAY_BASE_URL` and `PSYCHEVO_GATEWAY_TOKEN` overrides are
treated as caller-owned and must fail closed instead of starting a different
Gateway.

Desktop may use deterministic fallback capture in development and tests. Real
macOS, Windows, and Linux selection, bounds, screen capture, and source-app
filtering are adapters behind the Desktop host interface.

## CLI Launcher

`pevo desktop` is the source-checkout developer launcher for the native Desktop
shell. It discovers a Psychevo source checkout that contains `apps/desktop/`
and runs the existing Tauri development entrypoint for `@psychevo/desktop`.
This command is not a Desktop packaging, installer, update, or background
lifecycle surface.

The launcher resolves `pnpm` through the shared host executable boundary before
starting the development entrypoint. On Windows it searches `PATH`/`Path`,
honors `PATHEXT`, and launches `.cmd` or `.bat` shims through the native command
processor instead of treating an extensionless shell command as a directly
executable program.

The repository `packageManager` version remains a recommendation rather than a
Desktop launch gate. Unless the caller explicitly configures the corresponding
variables, the launcher disables Corepack project-version enforcement, download
prompts, and strict version matching for this pnpm child and uses
`pnpm_config_pm_on_fail=warn`. These defaults are subprocess-scoped: the
launcher does not modify user Corepack, pnpm, registry, proxy, or CA
configuration, and pnpm launch or compatibility failures remain visible on the
inherited stderr stream.

On Windows, the launcher also defaults the pnpm child to
`CARGO_HTTP_CHECK_REVOKE=false` when the caller has not explicitly set that
variable. The value is inherited by the Tauri-spawned Cargo process so Desktop
development can fetch crates when Windows Schannel cannot complete certificate
revocation checks. This default applies to every Windows `pevo desktop` launch,
is scoped to the launcher subprocess, and preserves explicit caller values,
including `CARGO_HTTP_CHECK_REVOKE=true`. It does not change Cargo timeout,
retry, multiplexing, proxy, registry, mirror, or CA settings, and direct package
scripts remain caller-owned.

`pevo desktop [--dir <DIR>]` preserves the active Psychevo profile for the
Desktop child process through `PSYCHEVO_HOME`, `PSYCHEVO_PROFILE`, and
`PSYCHEVO_PROFILE_HOME`. The Desktop workspace cwd is the caller's cwd by
default, or `--dir` when provided, and is passed to the Tauri process as
`PSYCHEVO_DESKTOP_CWD`. Desktop uses that value as its fallback cwd before
falling back to the Tauri process cwd. The launcher also passes the current
`pevo` executable path as `PSYCHEVO_PEVO_BIN` so Desktop managed Gateway startup
uses the same CLI build that launched the native shell instead of resolving an
older or missing `pevo` from `PATH`.

On WSL2/WSLg Linux hosts, `pevo desktop` defaults the child process to software
OpenGL with `LIBGL_ALWAYS_SOFTWARE=1` when the caller has not already set that
variable. This avoids Mesa Zink/EGL startup noise in Tauri/WebKitGTK while
preserving explicit caller graphics settings and leaving direct package scripts
caller-owned.

## Platform And Capture

Desktop exposes a platform diagnostics command for renderer and validation
code. It reports the operating system, the Linux session family when relevant
(`x11`, `wayland`, or `unknown`), observed display variable names without
leaking values, and capability snapshots for selection text, pointer location,
region screenshot, and portal screenshot.

Native capture commands return structured capability results. Failures use the
shared host reason taxonomy:

- `unsupported`: the API is not implemented for this host class
- `unavailable`: a required OS service, display backend, portal, or library is
  absent
- `permissionDenied`: the OS or user denied access
- `canceled`: the user canceled a picker or portal flow
- `failed`: an unexpected bounded error with a user-facing message

Linux X11 adapters use PRIMARY selection conversion for selected text,
pointer anchors, and drawable image capture for region screenshots when their
display services are present. Linux Wayland adapters query the XDG desktop
portal screenshot targets and use the portal `Area` target for region capture
when it is advertised. Wayland must not claim X11 selection parity when AT-SPI
or compositor support is unavailable. Psychevo workbench, floating, and picker
windows must be filtered from active-app and screenshot context before
model-visible capture is created.

## Workbench Reuse

Workbench must remain host-aware. Browser Workbench creates a browser host and
cookie WebSocket client by default. Desktop Workbench receives a Desktop host,
native Gateway transport, endpoint, and fallback cwd from the Desktop shell.

Workbench code may depend on host interfaces for platform behavior. It must not
import Tauri APIs directly.

Files-tree external-open and reveal actions remain shared Workbench behavior.
They execute through authenticated Gateway workspace RPCs on the machine that
owns the canonical workspace, so Desktop must not fork the menu or add a second
Tauri-only file-opener contract. The Gateway adapts launch behavior to macOS,
Windows, and Linux while Workbench receives only the host platform plus semantic
capabilities and actions and supplies the matching human-facing reveal label.
On Windows, Gateway system-default and File Explorer operations run in a
blocking STA COM task; the Desktop webview and asynchronous runtime workers
never own those shell calls.

Desktop export/share downloads must use a native authenticated bridge. The
renderer may request a session artifact by thread id, kind, and export options,
but Rust performs the Gateway HTTP request with the managed bearer token and
returns only file content metadata to the renderer. Desktop must not expose the
managed bearer token through URLs, renderer state, storage, or logs.

## Managed Browser Host

Desktop owns Psychevo's managed Browser host for the Browser and Rich Preview
surface defined by [262 Browser and Rich Preview](../262-browser-rich-preview/spec.md).
The host is a native Browser feature, not a Workbench-owned iframe automation
layer. Desktop is responsible for Chromium process/session lifecycle,
workspace-scoped profile storage, CDP connection ownership, page-tab state,
Browser pane visibility, and Browser host cleanup when a thread ends.

Workbench talks to the Browser host through typed host/Gateway requests and
notifications. Workbench must not receive native process handles, raw CDP
connections, or Desktop-managed bearer tokens. If the Browser host is
unavailable, Desktop returns structured capability failures using the shared
host reason taxonomy. Non-Desktop hosts return `Desktop required` for Browser
automation and annotation overlay commands.

Browser profile data is stored under the built-in Browser plugin data root and
scoped by workspace. Closing the right-workspace Browser pane hides the view
without deleting the thread-bound Browser session. Ending the thread releases
the Browser session.

## Validation

Default validation is deterministic and local:

- Desktop bridge tests for CLI-authoritative managed startup, owner-only token
  file handling, bearer WebSocket request construction, serialized/cached
  resolution with CLI re-delegation after health failure, explicit override
  fail-closed behavior, multi-window connection routing, per-instance bridge
  id routing, and disconnect cleanup
- Desktop host tests using fake Tauri command adapters
- Workbench tests proving browser defaults still work and injected Desktop
  runtime can be supplied without forking Workbench UI
- Floating package tests for reducer, prompt, attachment, geometry, and
  assistant-message behavior
- focused package typecheck/build for Desktop, Workbench, Floating, client, and
  host
- deterministic Desktop/Floating visual smoke with screenshot artifacts
- native Desktop/Floating WebdriverIO smoke uses the embedded Tauri driver
  under the test-only `wdio-test` Cargo feature; production Desktop builds must
  not register WebDriver plugins or parse WDIO capability permissions, and the
  WDIO build path skips release bundling because native smoke needs the test
  binary rather than deb/rpm/AppImage artifacts
- Desktop WDIO native smoke resets the process-global Undici dispatcher before
  session creation so Node/WDIO native service ESM imports cannot leave a
  dispatcher wrapper that WebdriverIO rejects before reaching the embedded
  Tauri WebDriver endpoint
- native Desktop Workbench smoke covers a non-fullscreen main window and
  asserts that the document-level shell cannot scroll vertically or reveal
  blank space below the Workbench; Settings is checked as a control surface
- native Desktop startup evidence records ordered `process_start`,
  `window_ready`, `managed_gateway_ready`, `bridge_connected`, `gui_ready`, and
  `draft_context_ready` milestones in a versioned manifest with bounded logs
  and screenshots. Rust-side milestones are emitted only under `wdio-test`;
  production builds expose no test recorder or managed token. Each milestone
  declares its own clock source, and cross-process timestamps are not directly
  subtracted. Browser-side milestones use the Workbench content-free timing
  registry so WebKit and Chromium retain the same readiness semantics
- native Desktop Floating smoke covers content-fit sizing without exposed black
  transparent gutters and provider/live transcript output through the shared
  Transcript DOM
- provider-backed Desktop Floating live records click, accepted/turn-start,
  first assistant Transcript DOM, and final token timestamps in its artifacts;
  the check must expose missing first-response streaming even when the final
  provider answer eventually passes
- registered live Desktop checks that either pass or record a structured
  `skipped` reason with a capability snapshot when the native platform is not
  available

Real OS permission smoke is opt-in. Provider-backed Floating live validation is
part of the Desktop live suite: `cargo xtask live run --suite desktop` triggers
it with the other Desktop live checks, and the live runner's normal invocation
plus credential resolution is the only opt-in boundary. That check must keep
the real Desktop/Floating runtime mounted, submit a Floating `turn/start`
through the native Gateway bridge, and assert the provider answer rather than
switching the Floating window into deterministic visual mode. Missing
credentials or native host prerequisites are reported as `blocked` or
`skipped` with structured artifacts, not hidden behind a second opt-in.
Deterministic fake activation must not be reported as real OS capture.

## Related Topics

- [021 Gateway](../021-gateway/spec.md) defines source identity, thread/turn
  routing, and transport semantics.
- [022 UI](../022-ui/spec.md) defines shared UI taxonomy.
- [220 pevo Gateway](../220-pevo-gateway/spec.md) defines managed Gateway
  lifecycle and token state.
- [240 pevo Web](../240-pevo-web/spec.md) defines browser and managed-Web
  Workbench behavior.
- [245 pevo Floating](../245-pevo-floating/spec.md) defines the Floating
  capsule feature.
