# Components

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

Shared GUI control modules live in `@psychevo/components` and are controlled
like the rest of the package. `ActionButton` owns visible commands only;
`IconButton`, `ToggleButton`, `DisclosureButton`, `NavItem`, `ActionLink`,
`Switch`, `Tabs`, `SegmentedControl`, selection popovers, menus, dialogs, and
mutation receipts own their corresponding semantic state, keyboard behavior,
focus lifecycle, and visual treatment. A generic `active` state is forbidden.
`FormField` owns label, hint, error, and ARIA wiring for ordinary input controls.
Opt-in shared field classes own the visual frame for search/filter, ordinary
value, secret/high-entropy, select, multiline, structured, compact-inline, and
native choice controls. Text-field rules exclude checkbox and radio elements.
Composer, Markdown/JSON, and file editors retain specialized geometry but reuse
the shared field color and focus roles.
`CreatePanel` owns non-modal inline and side-panel create/edit shells;
`ModalDialog` separately owns modal behavior. Callers keep business state,
validation, and RPC operations, while shared control modules own pending and
dismissal presentation. These modules consume generated `--pevo-control-*` and
`--pevo-field-*`
semantic variables. Product CSS may arrange their wrappers but must not
duplicate color, border, radius, height, focus, press, pending, disabled, or
selected state systems through descendant `button` selectors.
Ordinary action and icon commands have no visible resting or hover border;
keyboard focus remains visible through the shared focus ring, while selected,
caution, and danger meaning comes from semantic foreground and surface roles.

Create panels and dialogs are viewport-bound interaction surfaces. Long bodies
scroll internally, close and primary actions remain reachable, dialogs trap and
restore focus, backdrop presses do not dismiss, and Escape dismisses only an
idle surface whose caller permits cancellation. A dialog or local action group
contains at most one primary action, and that action remains transparent at
rest with theme foreground text. Dangerous confirmation initially
focuses Cancel and cannot use Enter as an implicit destructive default.

Committed GUI mutations publish display-only ledger receipts. Workbench keeps
at most two receipts visible for eight seconds, pauses expiry while a receipt
has hover or focus, and exposes Undo only for a reliable caller-owned inverse.
Receipt state must not enter transcript, persistence, export, accounting, tool
results, or provider context.

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
idle, and missing pending request lists are rendered as empty. Activity fields
added for cross-Gateway ownership, such as start time, owner, lease, or takeover
state, are optional display metadata; their absence must not prevent an idle
snapshot from rendering. A Gateway
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

Shared Web activity indicators use the same motion vocabulary as TUI: the
8-frame spinner advances every 120ms, and elapsed time is formatted as compact
whole seconds or `1m05s` after one minute. History session rows show only a
transparent inline running spinner beside the session title, not a filled badge;
their elapsed timer is intentionally omitted to keep the browser scannable.
Active turn timers derive from `GatewayActivityView.startedAtMs` and render in
an independent turn-status slot in the composer footer, under the prompt input
and between the left and right control groups while the turn is running. If a
foreign running activity lacks a usable start timestamp, the shell falls back to
the local time at which it first observed that activity as running so the
composer still presents an active-turn timer. Snapshot parsing must preserve
`GatewayActivityView.startedAtMs`; the local fallback is only for genuinely
missing timestamps. Running Thinking and tool rows
derive elapsed time from transcript block timestamps, render the spinner in the
ordinary expand/collapse arrow position, and render the elapsed timer on the
right side once the elapsed duration reaches 1 second. Tool rows with elapsed
duration below 1 second omit the right-side elapsed label. Once tool rows
complete, their persisted `metadata.elapsed_ms` value remains visible in the
same right-side elapsed slot instead of disappearing, subject to the same
1-second display threshold; completed Thinking rows do not show a persisted
elapsed label.
Timer updates are visual status, not transcript content, and must not resize
rows or repeatedly announce through screen readers.

Truncated GUI session titles expose the full title through the browser-native
`title` tooltip without a custom duplicate popover or row-height change. The
Workbench agent/runtime selector uses text plus a chevron, without a robot
icon, and sizes to the current display value with a bounded max width.
Workspaces that do not report a git branch omit the branch pill instead of
rendering a `no-branch` placeholder.

The Sessions surface uses one header and one scroll container in both active
and imported-and-archived modes. Source groups reuse the project-group density,
collapse affordance, title truncation, and secondary-menu placement. Archived
Thread rows and unimported ACP candidate rows remain visually distinguishable
through their group headings rather than per-row badges. Asynchronous Agent
discovery uses quiet loading/error rows inside the affected source area and
does not replace the whole Sessions surface with a blocking dialog or spinner.

Composer request panels are live interaction controls. Permission buttons must
either resolve the request or surface a transient composer error when the
Gateway returns `accepted: false`; a failed response must not leave the UI in a
silent no-op state. Reopened or refreshed Workbench snapshots must not render
stale permission panels for completed, interrupted, expired, or unrelated
activities.

The composer component must match TUI submission ergonomics. Plain Enter submits
unless an IME composition is active or a completion item is being accepted.
Shift+Enter, Ctrl+Enter, Alt+Enter, and Ctrl+J insert a newline. During an
active turn, plain prompt submission steers the running turn by default; queueing
the next turn is an explicit mode or command. The Queue/Steer segmented control
is only rendered while a turn is active and the prompt editor contains non-empty
text, so an empty composer does not present unavailable turn-routing choices.
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
The menu contains icon-led `Add images and files`, `Auto-speak`, and `Realtime
voice` rows. Its width follows the longest row up to the viewport limit, and
switch tracks stay adjacent to their labels instead of occupying a fixed-width
drawer gutter. Composer send and interrupt controls live in the same footer row
as `+` and Agent controls, aligned to the row's right edge with a stable height.
The grouped Model/Reasoning and context-usage controls sit immediately to the
left of that send/interrupt slot. The model picker keeps full
provider-qualified values for submission while showing the compact model and
provider-group presentation shared with Settings > Models. The prompt textarea
does not draw an additional focus outline inside the persistent input frame.
The model control must not display ambiguous placeholder text such as `model` or
`Default model` as if it were the active model. If Gateway cannot resolve an
actual provider and model, the control displays an explicit unavailable or
selection-required state and prompt-turn submission is blocked until the user
chooses a concrete `provider/model`. Reasoning presentation reflects the
selected target descriptor: selectable choices remain interactive, a read-only
effective value is non-interactive, and an absent descriptor produces no
Reasoning group or invented default. The model label and context-usage popover
must not clip their selected value, summary, or visible usage details at desktop
or narrow Workbench widths.
Permission mode is a visible descriptor-driven selection in the environment
line immediately before Workspace. Agent and Mode remain beside the `+`
control. Permission uses the current effective value, remains interactive when
the descriptor is selectable, and is not repeated in the Agent target popover.
If Gateway cannot provide an effective value, the control shows an explicit
unavailable or selection-required state. In a detached draft, Workspace opens
a switcher plus a final `Open workspace...` folder-browser action; Git branch
opens a local branch switcher plus a final `New branch...` action.
The folder browser keeps the editable path field visually distinct while its
surrounding location strip has a transparent background.
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
In a detached draft, Workspace opens a known-workspace switcher whose final
action opens a folder browser. The browser starts at the active cwd, can
traverse the filesystem visible to Gateway, and exposes its current location as
an editable folder-path field. Pasting or typing an absolute path and pressing
Enter browses that location; opening the folder resolves a changed path first.
In a bound Thread, Workspace continues to open Files.
Git branch opens a local-branch switcher with `New branch...` as its final
action; checkout and creation use structured Gateway operations and are
disabled while a turn is running. Path and branch remain in the quieter status
line. The default send control is
a compact circular arrow-up button; during an
active turn, the same slot becomes an interrupt control with a Codex-like filled
square stop glyph inside the same circular button. The prompt textarea grows with
message line count until its bounded maximum height, then scrolls internally.
The composer does not expose the browser's native textarea resize grip.

Workbench presents Agent Definition and Runtime Profile through one compact
target control. Its rows come from Gateway-compatible `RunnableTarget` choices;
the browser does not pair them or infer persona compatibility. The popover
keeps identity and execution provenance visually distinct without nested native
select menus. The popover remains wholly inside the viewport at narrow widths,
including when the compact target trigger sits in the composer's left control
group. After binding it becomes an immutable provenance capsule, and its change
action starts a new thread.

The target control keys rows by opaque `targetId` and renders only the
Gateway-projected `agentLabel`, `profileLabel`, readiness, and unavailable
reason. React does not consume the independent Agent/Profile catalogs to derive
row identity, availability, labels, or an implicit target. Changing an unbound
target re-reads Thread Context with that exact prospective target before any
control or send affordance becomes active.

The existing `Plan mode` affordance renders only a semantic mode descriptor
whose catalog contains the shared default/plan pair. Native and ACP Agents use
the same descriptor path. Additional Agent modes remain an adjacent typed
selector. Model, reasoning, and advanced controls likewise render from Thread
Context roles, provenance, mutability, dependencies, and confirmation state;
no control placement or submission branch uses a runtime name. On narrow
surfaces the Plan chip remains beside the Agent selector and enabling Plan must
not add an otherwise empty row.

Text, attachment, and structured Agent-mention affordances are admitted from
`ThreadContext.inputCapabilities` for the selected target. Disabled affordances
surface the descriptor's recovery reason; submission fails closed when context
is absent, sendability is false, any input part is disabled, or structured
mentions are not admitted. The headless Thread controller performs the same
admission before creating optimistic state so React and command-triggered turns
cannot diverge.

Draft and thread-preference controls are not Settings. The controls stay compact,
list-like, keyboard accessible, and responsive; visual validation covers Native,
Codex ACP, OpenCode ACP, managed-adapter recovery, and narrow mobile width.

Completion popovers are shared controlled components. `/` lists Gateway slash
commands, `$` lists skills, local agents, and ACP capability mentions, and `@`
lists cwd file references plus subagent-capable agent names. Accepted `@`
agent entries keep visible `@agent-name` text and submit structured Gateway
agent mentions only when the selected target's Thread Context declares Agent
delegation support. Literal `@agent-name` remains prompt text for targets that
do not support structured delegation. Arrow keys, Ctrl+N/Ctrl+P, Tab, Enter, Escape, and
pointer selection have the same semantics on every shell that has a keyboard.
Keyboard navigation must keep the active completion option scrolled into view
inside the popover, including long `$` skill/agent lists whose active item moves
beyond the visible panel. Mobile shells may present the same completion state
through touch lists or sheets. Completion ordering is query-aware: exact and
prefix matches against the visible command/skill/agent/file label rank before
substring or description-only matches, so pressing Enter accepts the item the
typed token visibly points at.
Skill and agent rows display origin with the shared `System`, `User`, and `Project`
labels; raw source identifiers remain protocol/detail data and are not shown in
the compact completion row.
Slash completion rows may include a short destination label such as Panel,
Preview, Prompt, Download, or Extension. Those labels are derived from Gateway
command presentation metadata, not from frontend command-name allowlists.
User-configured slash aliases returned by Gateway appear as ordinary slash
completion rows with the alias as the visible and inserted text. Alias rows
show `alias for <target>` as their detail and keep the target command's
destination label.

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
cwd switches and when the user submits new input. Successful feedback with
no follow-up action may auto-dismiss after a short delay and may be dismissed by
clicking outside the feedback panel. Error feedback and feedback with an action
must remain until an explicit clear, context switch, or new input.

The Settings Slash Commands section is the Workbench app-level profile
configuration surface for TUI-compatible slash aliases and shortcuts. It edits
only active profile/global `tui.slash_aliases`, `tui.slash_keybinds`,
`leader_key`, and `leader_timeout_ms` through Gateway slash-settings RPCs. The
page manages compact custom rows with target slash line, alias chips, shortcut
chips, and edit/delete actions; it does not duplicate the full command catalog
and does not define prompt-template commands. Saving refreshes the command
catalog so composer slash completion and the `/commands` overlay reflect aliases
immediately. Web v1 does not register browser-level keyboard shortcuts; shortcut
rows configure TUI behavior only.

The `Capabilities > Agents > ACP Backends` segment is the Workbench app-level
ACP client configuration surface. It shows configurable Profile-level ACP
backend registrations and their diagnostics, but not the current session's
running/background child-agent status. Backend create/edit controls open in a
scoped Capabilities panel rather than a global modal. GUI backend writes are
Profile-only and update the active `$PSYCHEVO_HOME/config.toml`; the form does
not expose a target selector. Project-level backend definitions may still be
read by Gateway and affect runtime behavior, but Workbench does not show, edit,
or delete them from this GUI surface. Workbench does not expose inactive
profiles in this surface.
Each listed Profile ACP backend exposes its enabled state as a row-level switch,
so users can enable or disable configured backends without opening the editor.
The row also exposes ordinary checkbox controls for the backend's `peer` and
`subagent` entrypoints. The backend editor does not duplicate the enabled or
entrypoint controls. The add control opens a generic ACP backend editor; users
can configure OpenCode or any other ACP-compatible backend by filling the
backend id, a single JSON command configuration, and capabilities.
The command JSON input replaces separate Command, Args, and Env fields. New
backend drafts prefill it with the generic OpenCode ACP template
`{"command":"opencode","args":["acp"],"env":{}}`, which users can edit for any
ACP-compatible backend.
It writes through the existing `backend/write` `command`, `args`, and `env`
fields after validation; no Workbench-specific wire shape is introduced. The
editor treats Label and Description as optional metadata, so only backend ID and
a JSON `command` string are required to save. The CWD field presents the default
workspace as an empty value with a `Defaults to workspace` placeholder and no
resolved-path helper. Empty CWD and the internal `invocation` sentinel resolve
to the active Gateway request scope cwd; relative CWD values resolve under
that cwd, and absolute values resolve as entered.

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

Transcript rendering uses the shared `@psychevo/components` Markdown renderer
for user and assistant text, including
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
Preview-oriented Markdown surfaces such as Files and Capabilities use the
shared Markdown renderer's copy affordance instead of transcript message
metadata. That preview action copies raw Markdown source and remains an
icon-only control so it does not compete with the rendered document.
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

Workbench transcript evidence renders successful update-tool diffs inline when
the tool result contains a strict-parseable Git patch. The collapsed evidence
title summarizes edited paths and addition/deletion counts, and the completed
row defaults open with only the compact single-gutter diff in its expanded
content. Workbench does not show ordinary Input/Change metadata or a `Diff`
section label above the rendered diff. Review tabs reuse the same parsed diff
model for full workspace previews, but keep their dual-gutter
Review presentation and right-workspace layout.

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

TUI Agent-authoritative runtime rendering follows the same transcript rule as Gateway
transcript rendering. Assistant text that is later confirmed to be part of a
`tool_calls` message remains assistant text in its observed position; only
provider/model reasoning is allowed to become a visible Thinking row.
