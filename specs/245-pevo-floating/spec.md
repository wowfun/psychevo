---
name: 245. pevo Floating
psychevo_self_edit: deny
---

# 245. pevo Floating

Define Psychevo's floating question capsule feature.

## Scope

- first-class floating capsule feature for the native Desktop shell
- selection-anchored question capsule lifecycle, attachment display, answer
  expansion, and mini-chat follow-up behavior
- floating-specific host capture, placement, and Gateway source semantics
- validation expectations for deterministic fake-host and fake-Gateway testing

Out of scope:

- Workbench, TUI, ACP, and Channel layout or interaction behavior
- Tauri app scaffolding, native Desktop packaging, and multi-window lifecycle;
  these belong to [246 pevo Desktop](../246-pevo-desktop/spec.md)
- browser-only overlays that cannot capture or avoid occluding other apps
- passive background sensing, external-app text replacement, clicking, typing,
  shortcuts, or visual automation in Phase 1
- provider behavior, runtime execution semantics, or Gateway transport internals

## Product Model

`pevo Floating` is a Desktop feature surface, not a compact Workbench. Its
primary job is to keep the user at the current work locus: select text, an
image, or a file; summon a compact Psychevo action bar beside that object; ask
or choose a contextual action; read and refine the answer in place.

Floating does not own a standalone Tauri application. It is implemented as a
private workspace feature package mounted by `pevo Desktop`.
Floating consumes shared visual tokens from [075 `DESIGN.md`](../075-design-system/DESIGN.md)
through `@psychevo/assets/theme.css`. Its package CSS must be scoped under the
`.pevo-floating` root so Desktop can import Floating and Workbench styles in
the same renderer without global selector collisions.

Phase 1 uses explicit capture only. The user must summon the capsule or choose
an attachment action before selected text, image content, file metadata, or a
region screenshot becomes model-visible. Phase 2 may add opt-in passive sensing
and strategy recommendations. Phase 3 may add source-app actions, but those
actions require separate read/write permissions, user-visible action lists, and
auditable consent.

## Surface Lifecycle

The floating shell has these semantic states:

- `hidden`: no visible capsule
- `toolbar`: selection-anchored action bar with `Ask`, `Explain`, `Translate`,
  `Rewrite`, a compact text input, and visible attachment chips
- `submitting`: the first prompt or a follow-up is being accepted by Gateway
- `expanded`: the capsule has morphed into a compact answer and mini-chat panel
- `running`: the active thread has in-flight work and may be interrupted
- `parked`: the capsule is minimized while retaining draft, attachments,
  running state, and thread id
- `error`: a bounded failure is visible with the next useful action

Fresh summon and restore are different operations. A fresh summon clears the
previous capsule state and creates a new activation id. Restore keeps the
current draft, captured attachments, thread id, running state, and answer
history. Closing the capsule ends the UI state but does not delete the saved
Gateway thread.

The first answer appears by morphing the toolbar into the expanded panel. The
expanded panel supports mini-chat follow-up against the same thread id. The UI
must avoid becoming a full Workbench: history browsing, settings, source
management, and broad diagnostics remain outside this surface.

## Gateway Source And Threads

Floating is an interactive Gateway caller. It must route work through Gateway
instead of assembling runtime turns directly. Floating requests use
`source.kind = "floating"` with a per-activation raw id and lifetime
`process`. The process lifetime is required because the first `turn/start`
materializes a thread and Gateway must be able to bind that thread for the
current Desktop/Gateway process; Floating must not request persistent lifetime
unless a later spec adds durable floating-source continuation.

Each fresh activation starts a new human-visible top-level thread. Follow-up
messages inside the expanded mini-chat pass the explicit thread id returned by
the first submit. Later activations do not resume the previous floating source
implicitly because they receive new activation raw ids and no persistent source
binding is written. Raw source identity is not model-visible; if the model needs
source context, Floating submits it as explicit context input parts.

Floating does not open a Workbench draft to force a first-submit thread id.
It starts the first turn with `turn/start`, `threadId: null`, and its
floating scope, then records the `threadId` from the accepted result. Follow-up
turns pass that recorded id.

The Desktop shell connects to the managed local Gateway with bearer
authorization on the native side. The bearer token must not be exposed to the
webview. Floating communicates with Gateway through Desktop's typed native
bridge transport. Each bridge transport instance must use an instance-unique
native connection id so a stale cleanup cannot disconnect a newer Floating
client.

## Context And Attachments

Floating attachments are controlled capsule state. Captured context must be
visible and removable before submit whenever practical.

Phase 1 attachment kinds:

- `textSelection`: selected text, optional source app name, optional bounds,
  and a compact preview; submitted as a `context` input part when
  model-visible
- `image`: pasted image, selected image, or region screenshot; submitted as an
  `image` input part when encoded
- `file`: local file selection; text files submit bounded text context, image
  files submit image input, and other files submit visible metadata-only
  context until a durable resource-upload spec exists

Display limits and model-context limits are separate. A chip may show a short
preview while the submitted context contains a larger bounded payload. Hidden
ambient prompt mutation is not allowed.

## Actions

The toolbar exposes four first-slice actions:

- `Ask`: preserve the user's typed question as the main prompt
- `Explain`: ask Psychevo to explain the selected or attached object
- `Translate`: translate the selected or attached object into the app locale
- `Rewrite`: produce a rewritten version of selected text or attached text

Actions compile into visible prompt text. They may prefill or submit the
composer, but they must not add hidden instruction text that the user cannot
inspect. `Rewrite` produces answer text only in Phase 1; applying it back to the
source app belongs to Phase 3.

## Native Host Responsibilities

The Desktop native adapter owns:

- macOS, Windows, and Linux selection bounds, focused app identity, and capture
  errors
- selection-toolbar placement near selected text or pointer position
- image/file picker integration
- explicit region screenshot capture
- self-window filtering so Floating does not capture its own UI
- permission errors for screen recording, accessibility, file access, or
  unsupported platform behavior
- a hotkey fallback when OS right-click integration is unavailable

Linux capture uses capability-specific X11 and Wayland paths. X11 may report
PRIMARY selection, pointer anchors, focused/window-at-point metadata, and
region screenshots from drawable image capture when the display services are
present. Wayland may use XDG desktop portals for screenshots and AT-SPI for
focused text when available; free-region capture requires the portal `Area`
target and otherwise returns a typed unavailable result. Wayland must return
typed unavailable or permission results when the compositor or app does not
expose the requested data.

Every native-only capture result must be represented as one of:
`unsupported`, `unavailable`, `permissionDenied`, `canceled`, or `failed`.
Unsupported, unavailable, denied, and canceled results must produce a visible
Floating status or error row while preserving the current draft, attachments,
and thread id.

Placement is semantic and testable. Helpers should clamp the toolbar and
expanded panel to the active monitor, prefer the selection bounds when present,
fall back to pointer position, and use a stable top-center fallback only when
no richer anchor exists.

The visible floating window must fit the capsule's current content height
instead of reserving a large transparent webview area. The capsule may grow for
running, answer, parked, and error states, but the native window should resize
to the measured content bounds with a small padding budget. Desktop may lower
the native minimum size for the Floating window so WSLg and other Linux
compositors do not clamp compact capsules to a large unused black background.
Floating must not rely on transparent webview pixels around the capsule: on
WSLg/WebKitGTK, exposed transparent margins or rounded-corner gutters may render
as black. If the compositor still enforces a larger native minimum, the capsule
or Desktop floating root must fill that bounded viewport with the Floating
surface background instead of leaving transparent webview area for the
compositor to render as black.

The toolbar uses a logo-only brand mark without visible product text. Floating
must not show a dedicated drag-grip icon; blank toolbar regions initiate native
window drag on primary-button pointer down when the Desktop host supports it,
while action buttons and other interactive controls remain clickable.

Floating answer rendering and thread lifecycle reuse the shared Thread and
Transcript pipeline used by Workbench. Floating may apply compact scoped
styles, but submit, accepted-thread binding, live Gateway events, `turn/result`
completion reconciliation, Markdown, code, reasoning, tool evidence, copy
actions, and final transcript ordering must come from shared client transcript
helpers and shared Transcript components rather than a Floating-only
projection. Floating turn requests must use the same selected model, runtime,
reasoning effort, permission mode, work mode, and runtime options semantics as
the Workbench submit path when Desktop supplies those controls. Floating must
not mask missing cross-surface updates with local fake assistant messages.
Floating must not render a separate Floating-only answer copy button; copy
affordances come from the shared Transcript component rules.
Floating must not render a separate Floating-only `Working` transcript row;
running feedback comes from shared Transcript activity state and the capsule's
existing interrupt control.

The expanded answer transcript must keep the newest rendered message inside its
scroll viewport. Compact message-action hit areas must remain available without
removing message height from scroll geometry or clipping that message's first
or last line when the transcript follows the newest entry.

On first submit, Floating must accept the live `turnStarted` event that
establishes the active turn even when the accepted `threadId` has not returned
to the renderer yet or the event's `threadId` is `null`. Once that event is
accepted, subsequent live transcript observations for that turn are matched by
`turnId` and rendered immediately through the shared transcript pipeline.
Applying the later `turn/start` response must bind the existing snapshot to the
accepted thread without replacing live transcript entries that arrived while the
request was in flight.

When a Floating turn materializes or continues a thread that is also visible in
Workbench, both Desktop surfaces must converge on the same running/completed
state. Floating-originated Gateway notifications must therefore be observable
by Workbench through Desktop's shared bridge/event routing, not only by the
Floating client that initiated the turn.

The expanded Floating toolbar includes a compact "open in main window" control
when a thread id is known. Activating it focuses the Workbench window and opens
that thread in the main transcript view while leaving Floating open in its
current state.

`Park` and `Close` are distinct. Parking preserves the capsule state and may
show a compact logo restore button. Closing dismisses the current capsule UI and
must not render a parked/logo fallback; in Desktop it asks the native host to
hide the Floating window while leaving any already-started Gateway work and
saved thread intact.

## Validation

Default validation is deterministic and local:

- reducer tests for fresh show, restore, close, park, submit, running,
  interruption, error, and stale-event guards
- action-to-prompt tests for `Ask`, `Explain`, `Translate`, and `Rewrite`
- attachment mapping tests for text selection, image, text file, image file,
  and metadata-only binary file behavior
- geometry tests for selection placement, pointer fallback, viewport clamping,
  and expanded-panel sizing
- bridge tests proving the webview receives no managed bearer token
- UI tests with fake Gateway/fake host for toolbar, chips, morph expansion,
  mini-chat follow-up, cancellation, and error messages
- UI tests proving blank toolbar drag, logo-only branding, non-dragging action
  buttons, and shared Transcript Markdown rendering
- UI tests proving first-submit live events received before the `turn/start`
  response are adopted, render streaming transcript content before final
  `turn/result`, and are not overwritten by late thread binding
- deterministic Desktop visual tests for toolbar, expanded answer, running,
  parked, and capture-error states with screenshot artifacts; the expanded and
  error states assert that the newest rendered assistant message remains fully
  contained by the transcript viewport
- provider-backed Desktop Floating live records click, turn-start, first
  assistant Transcript DOM, and final-token timings so first-response
  regressions are visible even when the provider eventually returns a valid
  answer

Real macOS, Windows, Linux/X11, Linux/Wayland, and WSLg capture smoke tests are
opt-in. Real provider tests are not part of default Floating validation.

## Related Topics

- [020 Interfaces](../020-interfaces/spec.md) defines caller-facing entrypoint
  and control-signal semantics.
- [021 Gateway](../021-gateway/spec.md) defines source identity, thread/turn
  routing, and bearer-authorized WebSocket transport.
- [022 UI](../022-ui/spec.md) defines shared UI surface taxonomy.
- [041 Permissions](../041-permissions/spec.md) defines runtime permission
  policy for future source-app actions.
- [220 pevo Gateway](../220-pevo-gateway/spec.md) defines managed Gateway
  lifecycle and token state.
- [246 pevo Desktop](../246-pevo-desktop/spec.md) defines the native desktop
  shell, Tauri project, bridge, and window lifecycle.
- [249 Vision and Image Artifacts](../249-vision-and-image-artifacts/spec.md)
  defines generated-image artifact metadata and compact media rendering.
