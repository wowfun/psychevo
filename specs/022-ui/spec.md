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
Generic Desktop shell capability is therefore implemented first by sharing the
same Workbench source, protocol client, host adapter contract, and components
used by managed Web. A feature that works in the shared Workbench path is
available to future Desktop shells when the shell host supplies an explicit
Gateway endpoint and source scope; native packaging remains outside this topic.

## Components

Shared components are controlled. They receive state and callbacks from an app
or client store and do not instantiate Gateway clients, read localStorage, or
write global config.

First-slice component families include transcript, tool evidence,
artifact preview/detail, composer, history, status/queue,
settings, diff/export/share, permission, clarify, tabs, buttons, inputs, and
layout primitives. Components should support desktop density and mobile/shell
collapse without requiring a separate native component tree.

The client package owns headless UX state machines that must behave the same
across Web, Desktop, and Mobile shells:

- a transcript reducer that keeps canonical snapshot entries, live overlay
  entries, and optimistic user messages separate, then reconciles them when
  committed entries or snapshots arrive
- a completion reducer that detects `/`, `$`, and `@` ranges, discards stale
  async results, and exposes selection/navigation state independent of DOM
- a bottom-follow controller that follows new output only while the user is at
  the bottom or has just submitted input
- mention encoding helpers that preserve visible composer text while carrying
  structured Gateway mention targets through immediate follow-up edits and
  submission

Shared app stores and components must tolerate missing fields only when those
fields have true idle or empty semantics. Missing activity state is rendered as
idle, and missing pending request lists are rendered as empty. A Gateway
`ThreadSnapshot` without `entries` is a protocol/projection error, not an empty
ordinary transcript, because `entries: []` is the only valid representation of a
real empty transcript. Snapshot defaulting for idle fields is applied at the
client app-store boundary before strict protocol validation; transcript entries
are not defaulted.
For an explicitly selected history session whose summary reports
`messageCount > 0`, `No messages yet` is not an acceptable ordinary transcript
state. The client must either render the message-derived entries returned by
`thread/read` or `thread/resume`, or surface a clear snapshot/projection error
instead of silently presenting the session as empty.
Transcript block child arrays such as artifact ids are rendered as empty when
omitted by live or partially upgraded payloads.

The composer component must match TUI submission ergonomics. Plain Enter submits
unless an IME composition is active or a completion item is being accepted.
Shift+Enter, Ctrl+Enter, Alt+Enter, and Ctrl+J insert a newline. During an
active turn, plain prompt submission steers the running turn by default; queueing
the next turn is an explicit mode or command.
The composer also owns an explicit shell input mode shared by Web and generic
Desktop shells. Typing `!` in an empty composer switches to shell mode without
putting a literal bang in the editable text; imported, pasted, or restored text
that begins with `!` is interpreted as shell mode and edited without that
prefix. Shell submission calls a dedicated shell callback with the stripped
command. Empty shell mode presents bounded shell help, and Escape or backspace
on an empty shell editor returns to prompt mode. Slash completion is suppressed
in shell mode, while `@` file completion remains available.

Completion popovers are shared controlled components. `/` lists Gateway slash
commands, `$` lists skills, local agents, and ACP capability mentions, and `@`
lists workdir file references. Arrow keys, Ctrl+N/Ctrl+P, Tab, Enter, Escape,
and pointer selection have the same semantics on every shell that has a
keyboard. Keyboard navigation must keep the active completion option scrolled
into view inside the popover, including long `$` skill/agent lists whose active
item moves beyond the visible panel. Mobile shells may present the same
completion state through touch lists or sheets. Completion ordering is
query-aware: exact and prefix matches against the visible command/skill/agent/file
label rank before substring or description-only matches, so pressing Enter
accepts the item the typed token visibly points at.

Ordinary transcript components consume typed transcript entries/blocks and typed
Gateway events. They must not display raw runtime event names such as
`runtimeRaw`, `entryCompleted`, or `turnCompleted` as user-facing transcript content. Raw
diagnostics belong in logs, tests, or explicit developer tooling, not the
ordinary Workbench transcript surface. Empty prompt or assistant placeholder
blocks are not visible transcript rows; they must not render as standalone user
or bot icons while the optimistic/canonical prompt reconciliation is settling.
Only real reasoning blocks render under the user-facing label `Thinking`;
protocol labels such as `Reasoning` and projection markers such as `Preamble`
are never ordinary transcript header text. Assistant text remains assistant
text even when the same assistant message also contains tool calls.

Transcript rendering supports Markdown for user and assistant text, including
CommonMark block structure, GFM tables/task lists, links, inline code, fenced
code blocks, and streaming caret placement at the end of the final rendered
block. The transcript panel header, user text, assistant text, reasoning rows,
and tool/evidence rows do not render decorative role, cognition, or kind icons.
Reasoning blocks are collapsible, default-expanded while running, and render
incrementally before assistant text. Tool/evidence rows render one summary
header and one expandable detail body; arguments or results must not be
duplicated between preview and detail.
Completed protocol status is not shown as a default badge for ordinary
completed transcript rows. Status badges are reserved for actionable states such
as running, failed, cancelled, needs-input, or informational diagnostics.

Assistant text blocks emitted before or between tool calls are visible assistant
message text, not Thinking content and not a separate `Preamble` row. Projection
metadata may mark such text as a phase note, but the renderer must keep it in
the assistant text channel and preserve its canonical position before the tool
calls it introduces. A live assistant update that carries no visible text or a
no-text reasoning completion must not render an empty row. If a visible
assistant text update arrives after one or more trailing live tool rows for the
same turn, the client transcript reducer must anchor that text before those
tool rows until the committed transcript slice supplies canonical message/block
ordering.
Live reasoning deltas are real live Thinking observations until the committed
turn slice replaces the same-turn overlay. Gateway may still emit an
authoritative assistant segment snapshot derived from `message_end.content[]`,
marked by `metadata.authoritativeBlocks === true`; when it does, the client
reducer replaces the entry's block array with the supplied blocks instead of
merging by id. Because the public runtime `message_end` payload can hide
assistant reasoning, Gateway must include any preserved non-empty live
reasoning block in that authoritative snapshot when final content omits
reasoning. Blocks that disappear from the authoritative snapshot, such as text
that was earlier misclassified by a provider and is later supplied as
assistant text, must disappear from the UI immediately.
An optimistic prompt submitted before `turnStarted` is still part of the active
turn live overlay. Once the turn id is known, Workbench binds that prompt to
the turn and keeps it before same-turn assistant and tool live rows until a
committed user entry replaces it. Authoritative assistant segment snapshots
also clear stale same-turn pending-only tool overlay rows that have been
superseded by the segment's final tool blocks.
Workbench must not continuously refresh ordinary transcript snapshots during a
normal active turn just to discover live text. Live Gateway events own that
display path until `turnCompleted` supplies committed entries. If a reconnect,
manual refresh, or explicit `thread/read` returns message-derived entries for a
still-active turn, reconciliation treats those message-derived entries as
authoritative for any covered prompt, assistant text, reasoning, or tool blocks
and removes overlapping same-turn live overlay blocks instead of rendering both
the snapshot projection and the live projection. This replacement is block
level: a live entry with one uncovered running block may be retained, but any
covered assistant text, reasoning, or tool blocks inside that entry are removed
before rendering. A covered block must not survive only because a different
block in the same live entry was not covered by the snapshot.
The same rule applies to live events that arrive after a reconnect or explicit
snapshot refresh. If an incoming live block is already represented by a
message-derived block, the client either uses that message-derived block as the
display anchor for transient tool status/output or drops the covered live block;
it must not append a second live row with the same assistant text or tool call.

Tool/evidence rows follow the same reducer semantics as the TUI: a tool row's
collapsed header is derived from the call name plus argument subject, while
results stay in the expanded detail or secondary summary. Continuation tools
such as an empty `write_stdin` poll for a yielded `exec_command` are not
independent transcript rows; their output, terminal status, and elapsed time are
merged into the owning `exec_command` row. Only non-empty stdin input is allowed
to render as its own compact terminal interaction. Long command, path, or query
headers must elide inside the row so the optional status marker and row border
remain visible at desktop and narrow widths. Full commands, SQL, arguments,
results, and JSON belong in expandable detail, not in the collapsed header.

Transcript components must render message-derived entries in canonical order
even when an app store or reconnect path provides a temporarily shuffled array.
Durable entries are ordered by `TranscriptEntry.messageSeq`; blocks inside an
entry are ordered by `TranscriptBlock.order`, then creation time and id for
deterministic rendering. Live-only entries must carry explicit turn-local
ordering from Gateway, either as block order inside a single live assistant
entry or as comparable live order metadata. Clients must not infer semantic
order from timestamp/id tie-breaks. Multiple model steps inside one turn keep
their observed segment order: real Thinking, assistant text, tools, and later
assistant text stay where they happened. Clients replace the live overlay with
the committed entries instead of matching rows by text overlap.
Workbench transcript rows carry nonvisual diagnostic attributes for entry id,
block id, block kind, and turn id so browser validation can report the actual
projection shape behind screenshots without exposing those internal labels to
users.

When a snapshot replace arrives while live rows are still running, the client
keeps unmatched live overlay entries for the active turn. When a turn completion
event carries committed entries, the client removes live overlay entries for
that turn and merges the committed slice by stable entry id/message sequence.
The replacement must not leave empty live reasoning, stale assistant updates, or
duplicate tool rows behind.
Live entries that remain after snapshot reconciliation must still be coherent
standalone observations. If covered blocks are removed and no visible block is
left, the entire live entry is dropped.
Selecting a historical session with persisted messages must apply the same
message-derived transcript visibility rules as reconnect and live completion:
non-empty user and assistant text blocks remain visible, while only truly empty
reasoning/text blocks are filtered from the rendered transcript.

TUI direct runtime rendering follows the same transcript rule as Gateway
transcript rendering. Assistant text that is later confirmed to be part of a
`tool_calls` message remains assistant text in its observed position; only
provider/model reasoning is allowed to become a visible Thinking row.

## Visual Direction

The first Workbench visual direction is an operator ledger: quiet, dense,
light-mode workspace chrome with a restrained ink/teal/brass palette,
transcript rows as the primary surface, and status details held in secondary
panes. It is an app shell, not a landing page; the first viewport orients the
user, shows connection/thread status, and enables the next turn without hero
copy or decorative backgrounds.

Surface hierarchy uses background-color steps, fine dividers, and restrained
shadow. Cards are reserved for bounded repeated items, evidence rows,
requests, and drawers; page sections should read as panes or rows rather than
generic floating cards. Buttons use a consistent radius scale and press feedback
without resizing their layout footprint.

Mobile uses the same component tree with compact chrome: top status must not
crowd the composer or tab rail, and the active panel owns the viewport.
Desktop uses a persistent
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
Long-running live skill validation uses a reusable Playwright spec that samples
the page every three seconds, stores screenshots, and checks each sampled
transcript against the message-derived SQLite transcript so transient row-order
regressions cannot be hidden by a correct final screenshot. It must also fail
when tool result JSON appears in a collapsed header, long evidence headers
overflow, a committed turn slice fails to replace live overlay rows, an empty
assistant update appears after a tool row, or a stale completion popover remains
after prompt submission.

## Related Topics

- [021 Gateway](../021-gateway/spec.md) defines Gateway thread, turn, source,
  and transport semantics.
- [070 Experience](../070-experience/spec.md) defines shared UX/DX defaults.
- [080 Design System](../080-design-system/spec.md) defines current TUI design
  language and shared experience constraints.
- [085 Brand Assets](../085-brand-assets/spec.md) defines canonical brand asset
  locations.
- [220 pevo Gateway](../220-pevo-gateway/spec.md) defines the concrete Web Shell.
