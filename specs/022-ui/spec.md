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
  Generated TypeScript schema modules are split by protocol domain under
  `src/generated/schemas/` and re-aggregated through `gatewaySchemas`; callers
  must not depend on a monolithic generated schema file.
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

## Components

Shared components are controlled. They receive state and callbacks from an app
or client store and do not instantiate Gateway clients, read localStorage, or
write global config.

Package implementation files follow the same component-family boundary as the
public package contract. Large `packages/*/src/index.*` files must remain thin
package entrypoints that re-export semantic modules; transcript, composer,
history/sessions, status, host runtime, and client reducers should live in
dedicated files so feature work does not keep expanding a shared monolith.
App packages follow the same rule. Files such as `apps/*/src/App.*` are
composition roots for wiring state, host/client calls, and high-level layout;
semantic UI surfaces, inspector panes, sidebar chrome, composer controls,
data normalization, and storage helpers must live in separate app-local modules.
Large app CSS entrypoints should aggregate smaller style files by surface or
layout area instead of accumulating all page, pane, and component overrides in
one stylesheet.

First-slice component families include transcript, tool evidence,
artifact preview/detail, composer, history, status/queue,
settings, diff/export/share, workspace file review/editing, permission,
clarify, tabs, buttons, inputs, and layout primitives. Components should
support desktop density and mobile/shell collapse without requiring a separate
native component tree.

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
Transcript message action hit areas must not cover or intercept adjacent
reasoning, tool, status, or message rows; visually stacked rows remain
independently clickable after live updates reconcile to committed snapshots.

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
The composer supports bounded attachments supplied by the host file picker.
Attachment chips are controlled component state with remove actions, and the
submit button is enabled when either prompt text or pending attachments are
present. Attachments are submitted through typed Gateway input parts rather
than through ordinary transcript history or browser-only file path access.
Workbench shells center the visible composer input band on the same reading
column as ordinary transcript rows on wide surfaces so prompt entry remains
visually connected to the transcript.
Attachment entry is exposed as a compact `+` menu in the composer action row.
The menu contains an `Add images and files` file/image picker action and the
runtime mode switch. Plan mode is toggled from this menu rather than from the
footer: default mode renders the switch off, plan mode renders it on and shows a
quiet `Plan` chip immediately to the right of the Agent selector. Hovering or
focusing that chip reveals a close control that returns the session to default
mode. Composer send and interrupt controls live in the same footer row as the
`+`, Agent, and `Plan` controls, aligned to the row's right edge with a stable
height so the composer does not gain an extra row when Plan is active. Model,
Variant, and context-usage controls sit immediately to the left of that
send/interrupt slot; the compact model indicator displays provider-qualified
model values using the segment after the final `/`, retains the full value for
submission, and leaves native selector options as full `provider/model` values.
The compact model label must size from that short model segment while reserving
space for the native selector affordance so selected characters are not covered,
and the model label and context-usage popover must not clip their selected
value, summary, or visible usage details at desktop or narrow Workbench widths.
Context and session observability controls are display-only chrome. Compact
surfaces may show context percent, session tokens, cache-read percent, and
estimated cost. The composer context popover remains compact and must not show
prompt/category detail breakdowns and opening it must not reveal, focus, or
change the open state of the right Status inspector. Full breakdowns belong in
the right Status inspector: they summarize the session usage facts first, then
show prompt/context token categories in the same category order and labels as
TUI `/context` where possible. The right Status view uses a stacked prompt-token
bar for category proportions, with hover/focus text showing each category token
count and percent. Category rows themselves are not independently collapsible
and do not carry per-row meter bars; instead, all prompt-token category detail
rows live under one `Prompt tokens` disclosure that is closed by default.
Expanded category details may show only numeric/counting facts, such as skill
entry token estimates, history role counts, project-context counts,
selected-skill-context counts, and tool counts. These values come from Gateway
`observability/read`, clear on new detached drafts or no-active-session states,
and must not become transcript content, command feedback, local prompt text, or
model-visible context. Observability refreshes are scoped to the selected view
epoch/session: a delayed response for a previously selected session must be
discarded after the user creates or selects a different session or detached
draft.
Permission,
path, and branch remain in the quieter status line. The default send control is
a compact circular arrow-up button; during an
active turn, the same slot becomes an interrupt control with a Codex-like filled
square stop glyph inside the same circular button. The prompt textarea grows with
message line count until its bounded maximum height, then scrolls internally.
The composer does not expose the browser's native textarea resize grip.

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
Slash completion rows may include a short destination label such as Panel,
Preview, Prompt, Download, or Extension. Those labels are derived from Gateway
command presentation metadata, not from frontend command-name allowlists.

Workbench applies slash command results to the region that owns the result.
Commands/help, sessions/history, and status commands switch the relevant
Workbench panel. Diff opens the preview surface. Context, usage, and status
details focus the status area. Export and share invoke host download/share
actions. Active-turn control commands update turn state and show display-only
feedback near the trigger. Dynamic skill and bundle slash commands submit a
model turn while the transcript-visible user input remains the original slash
line. Panel-targeted slash actions reveal collapsed regions, such as the right
Status inspector or left History sidebar, so the command has an immediate
visible result. Composer-triggered `/help` and `/commands` open closeable
overlays over the current transcript instead of replacing the active session
surface. GUI `/agents` is not exposed by Web/Desktop discovery, completion, or
panel routing; current-session agent selection belongs to the composer agent
selector. Other command feedback is display-only, session-scoped transient UI
and must not become ordinary transcript history; it is cleared on session or
workdir switches and when the user submits new input. Successful feedback with
no follow-up action may auto-dismiss after a short delay and may be dismissed by
clicking outside the feedback panel. Error feedback and feedback with an action
must remain until an explicit clear, context switch, or new input.

The Settings Agents section is the Workbench app-level ACP client configuration
surface. It shows configurable Profile-level ACP backend registrations and their
diagnostics, but not the read-only effective agent catalog or the current
session's running/background child-agent status. Backend create/edit controls
are embedded inside Settings > Agents rather than opened in a modal. GUI backend
writes are Profile-only and update the active `$PSYCHEVO_HOME/config.toml`; the
form does not expose a target selector. Project-level backend definitions may
still be read by Gateway and affect runtime behavior, but Workbench does not
show, edit, or delete them from Settings because they are not configurable from
this GUI surface. Workbench does not expose inactive profiles in this surface.
Each listed Profile ACP backend exposes its enabled state as a row-level switch
in Settings > Agents, so users can enable or disable configured backends without
opening the editor. The row also exposes ordinary checkbox controls for the
backend's `peer` and `subagent` entrypoints. The embedded backend editor does
not duplicate the enabled or entrypoint controls.
The create control is an icon-only add button that opens a generic empty ACP
backend editor; users can configure OpenCode or any other ACP-compatible backend
by filling the backend id, a single JSON command configuration, and capabilities.
The command JSON input replaces separate Command, Args, and Env fields and uses
an in-field placeholder such as `{"command":"opencode","args":["acp"],"env":{}}`.
It writes through the existing `backend/write` `command`, `args`, and `env`
fields after validation; no Workbench-specific wire shape is introduced. The
editor treats Label and Description as optional metadata, so only backend ID and
a JSON `command` string are required to save. The CWD field presents the default
invocation workdir as an empty value with an `Invocation workdir` placeholder and a compact
`Resolves to <path>` helper. Empty CWD and the internal `invocation` sentinel
resolve to the active Gateway request scope workdir; relative CWD values resolve
under that workdir, and absolute values resolve as entered.

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
block. Running assistant text and running reasoning blocks must reveal newly
available text through a local display buffer so a peer or provider that sends
large chunks, or multiple chunks in one browser frame, still produces visible
incremental progress. This reveal layer is presentation-only: transcript state,
copy actions, persistence, and reconciliation continue to use the canonical
Gateway text. The transcript panel header, user text, assistant text, reasoning rows,
and tool/evidence rows do not render decorative role, cognition, or kind icons.
Assistant text renders on the ordinary page background. Transcript rows share a
centered reading column so user and assistant turns remain visually related on
wide Workbench surfaces. User text aligns right within that reading column and
is the only ordinary text message with a filled bubble; fenced code blocks and
code previews keep a dedicated filled code surface. Inline code remains a
typographic monospace distinction and does not render as a filled chip.
Ordinary chat text, reasoning rows, tool/evidence summaries, and assistant
messages do not use accent fill or permanent card fill. Each text block exposes
a quiet hover/focus affordance row anchored just outside the bubble, so hidden
controls do not reserve layout height or inflate the message geometry. User
rows include a copy action and the block timestamp. Assistant rows include
copy, compact elapsed
duration immediately left of the timestamp, and the timestamp, using the same
completed-turn format as TUI metadata. The action copies the raw Markdown source
for that text block through the host clipboard boundary. Feedback controls such
as thumbs up or thumbs down are not part of this first interaction.
The hover affordance must keep a continuous pointer path from the message block
to the action row; moving from the text to Copy must not pass through a dead
zone that hides the controls before the pointer reaches the button.
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
results stay in structured expanded detail or a secondary summary. Continuation
tools such as an empty `write_stdin` poll for a yielded `exec_command` are not
independent transcript rows; their output, terminal status, and elapsed time are
merged into the owning `exec_command` row. Only non-empty stdin input is allowed
to render as its own compact terminal interaction. Long command, path, or query
headers must elide inside the row so the optional status marker and row border
remain visible at desktop and narrow widths. Full commands, SQL, arguments, and
results belong in structured expandable sections such as Command, Input, Output,
Files, Diff, Web, Error, or Status. Ordinary transcript rendering must not show
raw argument/result JSON in either collapsed headers or expanded details; raw
event payloads belong only in explicit developer diagnostics such as Debug.
Shell-command rows render the invocation-style subject `exec_command <cmd>` in
both TUI and Workbench. Workbench must not split an `exec_command` row into a
truncated tool-name column such as `exec_command p...` plus a second command
summary column; the collapsed row uses one clipped invocation title with the
status marker kept visible. Full commands, arguments, results, and internal
tool metadata remain available through structured expandable detail or Debug.

Workbench tool display projection consumes `metadata.display` when present,
using the same `ToolDisplaySpec` concepts as the runtime/TUI (`category`,
title-argument keys, title-result keys, summary keys, body keys, and body
policy). When no display spec is present, Workbench applies built-in defaults
for core tools such as `exec_command`, `write_stdin`, file tools, web tools,
clarify, Agent, MCP tools, and generic extension tools. This projection is a UI
rendering concern and does not change transcript ordering or tool execution
semantics.

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

The first Workbench visual direction is a dark precision ledger: quiet, dense,
local-agent workspace chrome with transcript rows as the primary surface,
evidence-oriented status details in secondary panes, and only black, white, or
transparent button/logo backgrounds. It is an app shell, not a landing page;
the first viewport orients the user, shows current work state, and enables the
next turn without hero copy or decorative backgrounds.

Surface hierarchy uses compact spacing, fine dividers, ledger rows, and
restrained shadow. Cards are reserved for bounded repeated items, requests,
drawers, explicit previews, and code surfaces; ordinary page sections should
read as continuous panes or rows rather than generic floating cards. Buttons use
a consistent radius scale and press feedback without resizing their layout
footprint. Workbench controls prefer icon-first expression for familiar actions
such as navigation, close, add, refresh, scroll, copy, and panel toggles; these
controls expose clear `aria-label` text and hover tooltips rather than permanent
long labels. Text buttons are reserved for commands whose wording is the
primary affordance or where an icon would be ambiguous. The transcript
jump-to-latest affordance is therefore an icon-only control with an accessible
label and hover tooltip. The left
navigation/sidebar reads as one continuous navigation
surface: Actions, Pinned, Sessions, and Settings use spacing, typography, and
soft selection indicators rather than prominent boxed outlines, heavy divider
lines, left rails, underline rails, or resting card-like row fills. Section
headers in that sidebar align their icons and labels to the same grid as the
action rows above them, with matching text scale and weight. Active
navigation rows, session rows, tabs, and segmented controls use a shallow tonal
shadow with a quiet surface step instead of inset rail effects. Logo containers
are transparent when the mark itself is visible against the dark chrome.
Settings and Status surfaces follow the same rule: setting rows, status metric
groups, context usage, and changed-file lists are list-like content on the
ordinary pane background. They should not render permanent row cards, heavy
outer panel borders, or filled containers just to separate adjacent controls.

Desktop uses a persistent left history/workdir pane, center
transcript/composer, and a resizable right workspace. On Web startup, Workbench
creates a local detached draft for the launch scope, falling back to the most
recent project scope and then the initialized default scope. The draft is
selected so the composer is ready immediately, but it is hidden from the left
Sessions browser until the first accepted prompt or shell command creates a
durable session. Startup does not proactively open the right workspace; users
reveal it from the right-column control or by taking an explicit file/diff
action. If a user navigates to another primary surface while startup is still
finishing, startup must not force the main surface back to Transcript.

When revealed without an active tab, the right workspace shows a
status/navigation home. The home summarizes current connection, session,
workdir, context, and changed-file state, then offers compact bordered rows for
Review, Terminal, and Files. Those rows use icon plus label only, with no
right-side explanatory copy. Selecting a row creates and activates a tab of
that type. After one tab exists, the right workspace shows a compact tab strip
plus a `+` menu for opening additional Review, Terminal, or Files tabs. Browser
is not part of this slice. Debug remains a developer-only tab when the local
Debug preference is enabled, but is not shown as a normal home navigation
target.

Review tabs own changed-file review and structured unified diff display. A
Review tab exposes a top-right Files toggle; when pressed, the tab splits into
left diff preview and right changed-files tree. Files tabs split into left file
preview and right workspace tree. This left-preview/right-tree structure is the
desktop information architecture for both tab types and must not be inverted;
only narrow responsive layouts may stack the tree below the preview. Review and
Files share the same filterable tree behavior: case-insensitive path filtering,
preserved ancestor folders, local folder expand/collapse, selected row state,
and compact status/count metadata where available. Their preview and tree
regions are immersive panes on the right-workspace background, separated by
subtle dividers rather than card frames, filled panel backgrounds, or rounded
outer containers. Selecting a changed file or a workspace file activates an
existing compatible tab when practical, otherwise it creates a new
right-workspace tab.

Review tabs may show Gateway-provided review groups in addition to the current
workspace diff. Review groups are ordered by turn, list changed files, and
allow only file-level Accept or Reject actions. Accept marks the file reviewed
without changing disk content. Reject asks Gateway to restore that file to the
baseline captured before the selected turn while preserving user changes that
already existed before that turn. If the file no longer matches the turn's
post-change revision, the row enters a conflict state and the user must reload
or inspect before retrying.

File and diff previews no longer open an inline center split; the transcript
surface remains the center workspace. Diff previews render parsed file headers,
hunks, line-number gutters, and addition/deletion/context rows instead of raw
plain `<pre>` output. Diff file headers use a compact Codex-style identifier:
status marker, workspace-relative path, and addition/deletion counts. They do
not show the raw `diff --git`, `index`, `---`, or `+++` metadata block as
visible header copy, and absolute paths are reserved for title/tooltip text
when the active workdir can be joined with the changed file path. Files
previews render text files as syntax-highlighted code and Markdown files
through the shared transcript Markdown renderer, with raw HTML escaped. The
Files tab header keeps only the tab title; the selected file absolute path
appears above the preview. Code highlighting uses the Workbench-local
`highlight.js` core integration with a hand-picked language set and app-token
`.hljs-*` colors, not a stock theme stylesheet.
Files tabs support bounded manual text editing in Workbench/Web. A text file
preview exposes an explicit Edit action. Edit mode uses a plain textarea with
line numbers, current line/column, Tab indentation, wrap toggle, find, go to
line, a dirty state, and `Cmd`/`Ctrl+S` save. Saves are explicit manual user
edits and do not enter the Review changed-file queue. Workbench blocks
navigation, file switching, and edit-mode exit while unsaved edits exist unless
the user confirms discarding them. If the file revision has changed since it
was opened, saving is blocked and the user can inspect the conflict, reload, or
force overwrite.
Terminal tabs are real
interactive local terminal sessions scoped to the active project workdir.
Terminal output is UI-only and is not transcript history or model-visible
context. Terminal tabs keep the xterm viewport primary and do not render a
persistent project title, path, or running badge above it. Apart from the
shared tab strip, a Terminal tab is a full-height terminal canvas; it must not
leave ordinary right-workspace background visible below a shorter xterm
viewport. Diff and code panes must remain readable in both dark and light
appearances; light appearance uses light diff and Markdown code surfaces rather
than retaining dark diff panels or dark Markdown code blocks, while dark code
surfaces use dedicated code text tokens rather than inheriting ordinary page
ink. Permission approval and clarify requests render in the composer area,
where TUI-style bottom interaction lives, and must not be displaced into
Review, Terminal, Files, Debug, or passive metrics.

The right workspace width is a Workbench host preference. Desktop users can
resize it from the left edge of the right column; the chosen width defaults to
about `520px` so Review and Files can open directly into their split working
layout, is clamped to a broad desktop range up to about `1200px` with a
viewport cap that protects the center transcript, persists on drag end, and is
restored on the next launch. Narrow/mobile layouts ignore the persisted desktop
width and keep one active surface visible at a time.

The Workbench web build keeps large, stable browser dependencies in named
vendor chunks instead of one monolithic vendor asset. React runtime,
Markdown/remark processing, syntax highlighting, terminal rendering, icons,
schema validation, generated protocol schemas, and local workspace packages may
be split independently so production builds stay inspectable without raising
the default chunk-size warning threshold.
Workdir-group ordering in the Sessions pane is based on actual session or
local draft recency, with label as a deterministic tie-breaker. Selecting or
resuming a session in a lower workdir marks that row active but must not lift
the workdir group to the top of the Sessions pane. Collapsed workdir groups
remain a compact top-stacked list with stable row spacing; empty available space
belongs below the list and must not be distributed between collapsed projects.
Workdir group labels set the hierarchy for the Sessions list; session titles
must not render larger or visually heavier than their workdir label. Active
session rows use a quiet background step for selection instead of oversized
typography. Session titles are visually nested under their workdir group label
with a minimal child-row indent instead of starting to the left of the group
label; the indent should clarify ownership without making the list feel
stair-stepped.
The Sessions and Transcript scrollers reserve a stable gutter so surrounding
layout does not shift, but their scrollbar thumbs stay hidden until the
scrolling surface itself is hovered, keyboard-focused, or actively scrolling.
Persisted session rows keep the row body focused on the session title. Time
metadata appears as compact relative days such as `0d` or `3d`, and the time
metadata plus More affordance appear on row hover or keyboard-visible focus
instead of staying visible in the resting list. Pointer-only focus must not keep
those affordances visible after the pointer leaves the row, and leaving the
hover/focus-visible area hides them immediately rather than fading them out.
Pin, rename, export, share, archive/restore, and
delete controls live behind that secondary More menu instead of rendering as a
permanent action strip under the session name. Local draft rows do not expose
session management actions until they become persisted sessions. Secondary
More and `+` menus in Workbench chrome must close when the user clicks outside
the menu, close on Escape with focus restored to the trigger, and remain open
when the user clicks inert space inside the menu. This outside-click behavior
applies to menu popovers, not slash, skill, or file completion listboxes.
Workbench chrome uses `Psychevo` as the visible product name. Project identity
belongs in the workdir/session grouping and settings detail surfaces, not as a
subtitle under the product brand. GUI-created workspaces and opened projects
are both ordinary workdirs; UI may show project affordances such as Git branch
only when the current workdir supports them. Creating a GUI workspace is an
icon-only Sessions header action immediately to the left of the
expand/collapse-all Sessions control, not a standalone primary left-nav item.
The Settings center page exposes an explicit return control at the top of its
own left navigation, followed by a settings search field. It does not show a
separate top Settings header, top-right close button, or current
project/workdir path. Settings is a compact app-level configuration center
rather than a single-column preference list or an embedded session panel. When
Settings is active, it replaces the Workbench session shell: the session list,
composer, mobile Workbench panel tabs, and right inspector are hidden. The
internal Settings navigation lists app-level settings directly in the left
column: `Appearance`, `Debug`, `Agents`, and bottom-aligned
`Archived sessions`. The right side renders only the selected item.
`Appearance` owns the light/dark Workbench preference, `Debug` owns the local
developer-diagnostics switch, `Agents` owns embedded Profile-level ACP backend
management, and `Archived sessions` directly displays archived sessions with
restore/delete actions. Outside Settings, the Workbench left sidebar always
shows active sessions; archived sessions are not a sidebar filter state.
Session-scoped controls such as Agent,
Model, Variant, and Permission mode do not appear in Settings; the
current-session agent can only be chosen through the composer agent selector.
Command catalog browsing is a transient transcript overlay, not a Settings
section, and MCP/integration or observability placeholders do not appear in
Settings until they become app-level configuration surfaces. The internal
Settings navigation becomes horizontal tabs on narrow layouts and follows the
same low-emphasis selected-row treatment as the left sidebar. The left sidebar collapse
control sits in the
same brand row as the logo/name and is icon-only; it must align to the right
edge of the session column. When the left sidebar is collapsed, the same
control becomes the expand affordance and uses a scaled Psychevo logo mark
instead of the generic panel icon. Collapsed sidebar chrome keeps the primary
action icons, such as New Session and Search, visible directly below
the logo toggle while hiding their text labels, and keeps the Settings utility
icon in the bottom utility rail at its normal vertical position. It must not
keep Pinned or Sessions list components mounted. The transcript surface starts
directly with conversation content rather than a redundant `Transcript` title row, and the
right inspector starts directly with Status/Files/Debug tabs instead of a
separate connection endpoint header. The right inspector expand/collapse
control is fixed to the top-right edge of the transcript column, above the
transcript surface, so inspector tabs remain only tab choices and collapsed
inspector state does not reserve a separate right-side rail.
The Status inspector treats the full, unshortened session id as the header
subtitle directly under `Status`; it does not repeat the project path there and
does not render a separate Session/Connection/Turn/Queued metric grid. Context
window, session-token, cache-read, cost, reasoning, and other useful usage
totals are compact Status facts, not transcript rows. They sit directly below
the session id as a single observability group instead of being duplicated in a
primary metric grid. The compact usage grid must not repeat Messages, Provider,
or Model once the context label/status already identifies the active provider
usage source.

Appearance is a frontend/host preference, not a provider or secret setting.
In light appearance, Workbench uses a warm reading-paper palette rather than a
cool blue or icy gray shell. The canvas is ivory, panels are warm paper,
borders are taupe, primary text is warm charcoal, and selected controls,
status accents, and active UI state use low-chroma amber/taupe tokens so they
read as quiet application chrome instead of a saturated brand color.
The bottom Settings utility entry is a location marker, not a primary action;
when Settings is active it uses the ordinary sidebar selected surface instead
of an accent fill.
Workbench defaults to the dark ledger appearance, and Settings provides a
light/dark appearance toggle. The choice may be persisted by the host storage
adapter and applied before ordinary panel rendering when available. Theme
switching must preserve the same layout, density, button background rules, and
status/diff preview behavior.
In dark appearance, primary shell labels such as `New Session`, `Search`,
`Pinned`, `Sessions`, `Settings`, and transcript state labels such
as `Thinking` must use readable foreground tokens rather than the faintest
muted text color. Dark surfaces use neutral warm-black tokens rather than a
cold blue sidebar hue, and primary text should sit in a higher luminance range
so navigation labels, empty transcript states, and transcript copy remain
legible while preserving the dark ledger structure. Filled user bubbles and
selected navigation rows must remain visibly separated from the page background
without becoming saturated accent surfaces.

Settings Debug provides a local Debug switch. When enabled, the right
inspector adds a `Debug` tab after `Files`; when disabled, the tab is absent.
The Debug tab shows the current Workbench event stream and Gateway notifications
as developer diagnostics, separate from ordinary transcript content and hidden
by default.

Mobile uses the same component tree with compact chrome: top status must not
crowd the composer or tab rail, collapsed sidebars must keep fixed-size icon
buttons, and the active panel owns the viewport.

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
- [075 Design System](../075-design-system/spec.md) defines current TUI design
  language and shared experience constraints.
- [075 Brand Assets](../075-design-system/brand-assets.md) defines canonical brand asset
  locations.
- [220 pevo Gateway](../220-pevo-gateway/spec.md) defines the concrete Web Shell.
