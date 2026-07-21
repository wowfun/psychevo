# Workbench Layout

The shared Web and generic Desktop layout follows the v0 workbench direction:
a dark, dense, operator-ledger app shell with no top tab bar. Desktop may later
add native chrome and a native status bar, but the application content layout is
the same Web/Desktop tree.

The desktop layout is a three-surface workbench:

- left: collapse control, `New Session`, `Search`, `Automations`, global
  `Pinned`, project-grouped active Sessions with expand/collapse and per-project
  new session actions, an alternate imported-and-archived source view, and a
  bottom utility rail for Settings. Settings is the
  persistent app-level configuration center whose left navigation lists
  `Appearance`, `Models`, `Slash Commands`, `Usage`, `Debug`, and `Channels`
  directly. Agent definitions, teams, and ACP backend configuration live in
  `Capabilities > Agents`; Settings does not include an Agents entry. The ordinary
  Workbench left sidebar defaults to active sessions; its explicit header toggle
  reveals archived Threads and asynchronously discovered ACP Agent sessions.
  Composer-triggered `/commands` remains a closeable overlay over the transcript.
  GUI `/agents` is not exposed by Web/Desktop command discovery or panel routing.
- center: transcript/workbench and bottom-fixed composer
- right: a resizable workspace with a status/navigation home and typed tabs for
  `Review`, `Terminal`, `Files`, temporary side chats, and opened
  child-agent threads

The Web and Desktop Workbench shell fills the visible window without using
document-level vertical scrolling. Internal panes such as the transcript,
session list, Settings content, and long form bodies may scroll, but the outer
`html`/`body`/app shell must not reveal blank space below the primary workbench
in normal, non-fullscreen Desktop windows. Pinned sessions in the left sidebar
must stay bounded and scroll within the pinned area when long; they must not
expand the left shell or create document-level scrolling.

On startup, Workbench creates and selects a local detached draft. The launch
scope is preferred; if unavailable, Workbench uses the most recent project
scope from session history, then the initialized default scope. This selected
draft reveals the composer but does not proactively open the right workspace
and does not render as a left Sessions row. Selecting a history session or
explicitly creating/selecting a local draft reveals the same session-scoped
center surface; the right workspace opens only through the right-column control
or an explicit file/diff action.

When the right workspace is revealed without an active tab, its home is a
navigation and status page. It shows connection, current session or draft
state, cwd, context usage, and changed-file summary, then offers bordered
icon-and-label rows to open Review, Terminal, Files, and, for non-draft
sessions, `Side chat` tabs. Each row keeps its icon and label left-aligned;
rows do not carry right-side explanatory copy. Once any tab is open, the tab
strip includes a `+` menu for creating more tabs of those types. Browser is
exposed as a right-workspace peer when the Browser
plugin is enabled, with one independently identified and stateful Browser pane
per thread. Switching threads hides the previous thread's Browser tab without
sharing its URL, and returning restores that thread's navigation state. Web/PWA
Browser behavior is preview-only: it renders ordinary `http://` and `https://`
pages in an unsandboxed iframe, while browser-control RPCs and annotation
overlay injection return `Desktop required` until a Desktop Browser host is
available. Remote embedding headers may still prevent a page from loading.
Host/port shorthand defaults public hosts to HTTPS and localhost or loopback
addresses to HTTP; unsupported explicit schemes are rejected.

Automations is an app-level operational surface for local project automations
and thread heartbeats defined by
[400 Workflow Automations](../400-workflow-automations/spec.md).
Like Settings, it replaces the session shell while active and hides the
composer and right inspector, but it is not a configuration section inside
Settings. The Automations surface includes:

- a compact empty state with a natural-language draft input, template buttons,
  and `New`
- a task list showing enabled/running/error state, target, schedule, next run,
  last run, run-now, open-thread, and delete actions
- one shared create/edit flow for model drafts, templates, and manual edits
- target controls for project automations and, when an active thread exists,
  current-thread heartbeat automations
- schedule controls for interval, daily, and weekly local schedules
- an execution policy control whose default visible label is `Auto in sandbox`
  and whose alternate first-slice option is `Ask first`

Natural-language creation is a draft-and-confirm flow. Workbench sends the
user's description to Gateway, receives a structured draft, and fills the same
create/edit form used by templates and manual edits. The generated title,
target, prompt, schedule, enabled state, and execution policy remain editable
and are not saved until the user presses `Save`. If drafting fails, Workbench
keeps the description visible and leaves the manual controls available.

Workbench follows the shared thread-navigation display contract in
[250 Thread Navigation](../250-ui-display-model/thread-navigation.md). `Side
chat` tabs are temporary side chats equivalent to submitting
`/btw` from the GUI. Agent blocks that identify a child thread open that child
thread in a right-workspace child tab rather than copying its content into the
parent transcript. Workbench routes live entry events by
`TranscriptEntry.threadId`; scoped subagent live entries whose thread id is the
child session id are ignored by the parent snapshot and accepted by the child
tab snapshot. Workbench retains an ordered, bounded per-thread event feed rather
than a single replaceable latest event. Opening a child records a sequence
barrier before `thread/read`; the authoritative snapshot covers earlier events,
and events arriving after the barrier are applied in order so snapshot loading,
React batching, and rapid ACP chunks cannot create gaps. Only the latest
refresh generation may publish a snapshot or loading/error state, so a delayed
read from an older client or thread cannot overwrite a newer child view.
Child-thread composers,
including Side chat and child-agent
thread tabs, use the same shared composer shell as the main transcript
composer. In matching idle/running states, their input frame and full composer
shell heights stay aligned with the main composer. In desktop split view, the
child-thread input frame is bottom-aligned with the main composer input frame
instead of floating immediately after the latest child-thread message.

Review tabs are ordered around current work state and structured diff review.
A Review tab has a top-right Files toggle; when pressed, the tab splits into
left diff preview and right changed-files tree. Selecting a changed file scopes
the active Review tab to that file when Gateway can provide a file-specific
diff. The `/diff` host action opens or updates a Review tab. Files tabs split
into left preview and right tree. A Files-header toggle hides or restores the
tree, expanding the preview to the full pane while hidden. Selected-file actions
keep `Open HTML preview` immediately to the left of `Edit`, below that panel-level
toggle. The left-preview/right-tree split is the desktop information architecture
for both Review and Files, with stacking only for narrow responsive layouts.
Review and Files use the same locally filterable
tree component with folder expand/collapse and selected row state. Preview and
tree regions are immersive right-workspace panes with subtle split dividers
instead of framed card backgrounds. Markdown previews reuse the shared
`@psychevo/components` Markdown renderer with raw HTML escaped, appearance-adapted code
blocks, GitHub-like document-start YAML frontmatter tables, complete fenced
Mermaid rendering, and a quiet copy action that writes the raw Markdown file
source through the host clipboard. Incomplete fenced Mermaid blocks remain code
while streaming. Local HTML previews must be read-only and must not use raw
`file://`; Gateway-authorized workspace content runs immediately in an
interactive opaque-origin iframe with scripts, network access, pointer input,
keyboard input, and scrolling enabled. Workbench retains the CSP and iframe
restrictions that deny base rewriting, forms, nested frames, objects, popups,
same-origin, top navigation, and downloads. There is no locked/trusted state or
confirmation control. Changing the selected document remounts the iframe and
changing its content reloads `srcDoc`. Files and Preview share a single active
HTML execution instance: changing between those views suspends the inactive
iframe while preserving unrelated tab state. Code previews
use a Workbench-local `highlight.js` core
integration with app-token colors. The Files header does not repeat the project
path; the selected file absolute path is shown above the preview. Diff previews
use theme-adapted surfaces so light and warm appearances do not retain dark
diff panels. Diff file headers are compact UI identifiers, not raw Git
metadata:
they show status marker, workspace-relative path, and addition/deletion counts,
while absolute paths are reserved for tooltip text when the active cwd is
available. Unsupported preview formats and Gateway binary/unreadable file
responses stay in the Files tab as unavailable preview states instead of
opening a center preview.

Every real file row in the Files tree has an accessible context menu even when
that file cannot be previewed inside Workbench. Directories retain their normal
expand/collapse interaction and do not expose file actions. The menu opens at
the pointer, or from the focused row through the keyboard context-menu gesture,
stays inside the viewport, closes on Escape or outside interaction, and restores
focus to its file row. It contains only the preferred external-open action, a
useful alternate when one exists, and the OS-adapted reveal action: `Show in
Finder` on macOS, `Show in File Explorer` on Windows, and `Show in File Manager`
on Linux. The Review changed-files tree does not expose these actions because
its entries may describe deleted or otherwise non-present files.

`workspace/file/externalActions` lazily classifies the selected file and returns
its available semantic actions. `workspace/file/openExternal` executes one
returned action. Both requests carry the active scope and workspace-relative
path; Gateway repeats canonical containment and regular-file checks at execution
time. The action vocabulary is closed to `systemDefault`, `vscode`, and
`reveal`, and responses do not expose host executable paths. Absolute paths,
path traversal, symlink escapes, directories, missing files, and unauthenticated
requests are rejected. Browser requests must both match the browser session's
current workspace and belong to that session's trusted external-action grant
set. Gateway seeds the monotonic set from the consumed launch entry and extends
it only after an explicit successful workspace creation or adoption/resume of an
existing stored session. A caller-supplied draft/start cwd may change the
browser's navigation scope, but it cannot grant an OS side effect by itself.
The action runs on the Gateway/workspace host, including when Workbench is
viewed from another machine.

System-default opening delegates category-specific application choice to the
host OS. Classification precedence is webpage, image, audio/video, PDF, Office,
text, then other. Webpages prefer the default browser; images the default image
viewer; audio/video the default player; PDFs the default PDF reader; Office
documents the default Office application; and other files their default OS
association. A file may also be text-like independently of its preferred
category, so HTML, SVG, and textual Office-adjacent formats can retain their
category-specific system default while offering VS Code as the alternate.

When Gateway detects VS Code, a text-category file prefers `Open in VS Code`
and keeps `Open with Default Application` as the alternate; any other text-like
file offers VS Code after its category-specific preferred action. Gateway
detects the `code` launcher through the effective process environment plus
well-known installation locations, caches the capability, and opens the
canonical workspace root and selected absolute file in one launcher invocation.
It does not force a new or reused window, leaving that choice to the user's VS
Code settings. Without VS Code, text files fall back to the system default.
Text-like detection combines known text names/extensions with a bounded content
probe so UTF text, extensionless files, and dotfiles are not limited to a short
extension allowlist. A content-probe read failure degrades an otherwise unknown
regular file to `other` instead of removing its default-open and reveal actions.
Explicit launch failures, including an opener process that exits non-zero during
a bounded early-observation window, surface a bounded error and never silently
execute a different application. Long-running opener processes are accepted
after that window. Windows system-default opening and reveal execute in a
blocking STA COM task rather than on an asynchronous runtime worker.

A file row whose built-in preview is unavailable remains an enabled context-menu
target. Its accessible description communicates that only preview is
unavailable and that external actions remain available; the row must not expose
`aria-disabled`, which would incorrectly disable the whole file interaction.

Review exposes best-effort Gateway observation groups. It states once, compactly,
that Review lists only edits observed through Psychevo-owned mutation paths and
that shell, ACP, external-tool, or concurrent changes may be absent. Groups are
ordered by Turn, may be running or completed, and declare exact or partial
coverage. Partial coverage uses a compact marker rather than a repeated warning.

`workspace/changes` returns the bounded in-memory observation ledger with
file-level pending, accepted, rejected, and conflict states. `write`, `edit`,
and Gateway-mediated text writes can contribute exact before/after deltas;
opaque mutation paths only invalidate Files/Diff. `workspace/change/accept`
marks an observed file accepted. After the group completes,
`workspace/change/reject` restores the content immediately before the first
exact observed mutation for that path, removes an observed creation, or restores
an observed deletion. It does not claim to restore the whole Turn-start
workspace. If the current revision differs from the last observed post-change
revision, Reject is blocked and the row becomes conflicted. Native whole-Turn
undo remains a separate snapshot-backed command.

Files supports authenticated manual text editing for files inside the active
project root. `workspace/file/read` returns text content plus editability
metadata: size, revision/hash, line ending, binary/truncated state, and a
reason when the file cannot be edited. `workspace/file/write` accepts scope,
relative path, full text content, expected revision, and an explicit force flag.
The Gateway must reject absolute paths, path traversal, symlink escapes,
binary content, files over `1 MB`, and unauthenticated browser sessions. GUI
saves are direct user edits, independent of the selected Agent permission
mode, and they do not enter `workspace/changes`. If the expected revision no
longer matches the file on disk, Gateway rejects the save unless force is set.
Workbench surfaces that conflict by offering compare/reload/force-overwrite
actions rather than silently merging.

Workbench persists the right workspace desktop width as a host preference.
Clients default the opened width to about `520px`, clamp restored and dragged
values to a broad desktop range up to about `1200px`, keep a viewport cap so
the center transcript remains usable, and disable the desktop resize handle in
narrow layouts.

Terminal tabs keep the PTY viewport as the primary surface. They do not render
a persistent project title, path, or running badge above the terminal. Apart
from the shared tab strip, the tab behaves as a full-height immersive terminal
canvas: the xterm surface blends with the right workspace instead of rendering
a separate framed code panel or leaving non-terminal background below it.
The xterm theme is appearance-aware and uses an opaque readable background,
foreground, cursor, selection color, and ANSI palette for `dark`, `light`, and
`warm`; light appearances must not inherit xterm's default black surface or
dark-terminal ANSI palette.
Transient startup, error, and exit text may appear inside the terminal panel
only when needed.

The Gateway terminal API backs right-workspace Terminal tabs. It is separate
from composer shell mode and does not create transcript entries. The methods
are:

- `terminal/start`: accepts `scope`, optional `cwd`, terminal `cols`, and
  terminal `rows`; validates the requested cwd against the same scope rules
  as workspace reads; spawns a PTY shell in that directory; returns
  `terminalId`, resolved `cwd`, and optional process id.
- `terminal/write`: accepts `terminalId` and a base64 data chunk to write to
  the PTY.
- `terminal/resize`: accepts `terminalId`, `cols`, and `rows` and resizes the
  PTY.
- `terminal/terminate`: accepts `terminalId` and terminates the PTY session.

Gateway sends `terminal/output` notifications with `terminalId`, stream name,
and base64 output chunks, and `terminal/exited` notifications with
`terminalId`, optional exit code, and reason. Terminal sessions are owned by the
WebSocket connection that created them and are cleaned up on explicit
termination, process exit, or connection close. Terminal output is never
persisted as session history or model-visible context.

Settings is an app-level configuration center, not an embedded session panel.
When active, it replaces the Workbench session shell and hides the session
list, composer, mobile Workbench panel tabs, and right inspector. It does not
show a separate top Settings header or top-right close button; its return
control sits at the top of the Settings left navigation, followed by a settings
search field, and the current project/cwd path is not repeated there. The
internal left navigation lists `Appearance`, `Models`, `Slash Commands`,
`Usage`, `Debug`, and `Channels` directly. `Appearance` includes a local
appearance control with `dark`, `light`, and `warm` choices, and `Debug` owns
the local Debug switch. Archived-session browsing and activation belong only to
the Sessions header toggle. The default is the dark ledger appearance. The setting is a
Workbench host preference and does not require Gateway to persist
provider/runtime configuration. `light` is the neutral paper-warm daytime shell
with a near-white canvas, one shared quiet navigation paper surface for the
Sessions and Settings left rails, soft warm-gray dividers, and low-contrast
selected rows while keeping neutral text and accent semantics.
`warm` is the reading-paper palette formerly exposed as light, with ivory
canvas, warm paper panels, taupe borders, warm charcoal text, and low-chroma
amber/taupe active states. The dark palette keeps the near-black ledger
structure, removes cold blue sidebar bias, and uses higher-luminance primary,
muted, and navigation text so Gateway-rendered status/settings data remains
readable under all appearances. All three appearances share the same Workbench
font scale and row density. Settings creation flows use scoped create/edit
panels inside the selected page instead of bottom-stacked always-visible forms.
Provider setup uses `Connect provider`; channel setup uses `Set up channel`.
Successful saves close the panel and refresh the page data, while failures keep
the panel open with the entered draft and inline error. The opened panel must be
placed in the owning page's scrollable content column, not below the viewport or
outside the visible page bounds; long forms scroll within the page/panel while
header, close, and primary actions remain reachable at desktop and narrow
Workbench widths. Profile ACP backend setup uses `Add backend` inside
`Capabilities > Agents > ACP Backends`, where each listed Profile ACP backend
exposes its enabled state and `peer`/`subagent` entrypoint controls.
Session-scoped Agent,
Model, Variant, and Permission mode controls remain in the composer/status
surfaces and are not duplicated in Settings. Enabling Debug adds a right-side
`Debug` tab after `Files` and displays the current Workbench event stream and
Gateway notifications there. Debug output is diagnostic chrome, hidden by
default, and must not become transcript history or model-visible context.

Narrow layouts keep transcript and composer as the primary surface. Left and
right sidebars collapse to fixed-size icon buttons without allocating extra
empty columns, and the composer/status line remains usable without horizontal
overflow. The UI should present as an operational workbench, not a landing
page.

First-slice panels include transcript, composer, history, status/queue,
settings/auth/model controls, project files, changed-file diff preview,
export/share, permission, and clarify. Memory and resource surfaces are
status-only in the first Web slice.

The composer matches TUI keyboard behavior: plain Enter submits, modifier Enter
variants insert newline, IME composition is respected, and running-turn prompt
submission steers by default. Queueing remains available as an explicit composer
mode and via `/queue`.
The composer panel uses a Copilot-style restrained input surface: the textarea
and send control are inside the input frame, while the attachment button and
current Agent selector sit in the lower-left action slot. Permission mode lives
in the quieter environment line after Workspace and Git branch and is not
repeated in the Agent popover. The status line mirrors the TUI footer shape
with clickable chat mode, model, variant, context usage ring, project path, Git
branch, and permission. A detached draft centers the Composer in the empty conversation
surface; binding the first accepted prompt moves the same Composer to its
ordinary bottom dock with a reduced-motion-aware positional transition.
Responsive footer layout follows the Composer dock's available inline size,
not only the browser viewport. When side workspaces narrow a running Composer,
its environment controls wrap before the elapsed indicator can clip the model
and reasoning label.
Context usage is graphical by
default; detailed text appears on hover or in the same graphical popover used
by the right `Status` context panel. Tokenizer and context-scope details are
not shown in the Workbench UI.
Permission approval and clarify requests appear in the composer area, matching
the TUI's bottom interaction pattern. They may sit above or temporarily replace
the text input while awaiting a decision, but they must not be buried in the
right Status inspector or Debug diagnostics.
The attachment button opens the host file picker. Browser hosts attach images
as Gateway image inputs, text-like files as visible context input, and
non-text files as bounded visible metadata when their contents cannot be
embedded safely. Attachment chips remain in the composer until the next prompt
is accepted or the user removes them; attaching files must not require opening
the right Files tab.
Voice controls share the same compact composer/transcript control vocabulary.
Mic dictation sits immediately before Send/Interrupt in the composer action
cluster, matches the Send button's circular footprint, and uses inline activity
motion while recording instead of a composer feedback bubble. When dictation
successfully inserts text into the draft, it does not show a success popup.
Read-aloud lives on assistant messages. Auto-speak and realtime voice live in
the `+` drawer as labelled switch rows below Plan mode. These controls are
defined by
[248 Voice ASR/TTS](../248-voice-asr-tts/spec.md) and must not create an
additional Settings section or a second transcript model.
The same shared composer provides Web and generic Desktop shell mode. The
generic Desktop shell reuses this Workbench/Gateway behavior and identifies
itself through the host/source scope; this topic does not introduce native
desktop packaging.

The transcript renders user and assistant Markdown, streams assistant and
reasoning updates without waiting for turn completion, keeps observed block
order, and follows the bottom while the user has not intentionally scrolled
away. Snapshot replacement and explicit session switching position the
transcript instantly rather than animating through historical content. New or
unvisited sessions open at the latest message, while sessions already visited
in the current browser tab may restore their in-memory transcript scroll
position without writing that state to Gateway, protocol fields, durable
session metadata, or host storage. Tool calls render as collapsible evidence
rows with parameters and results shown once. The center transcript uses a
shared reading column: user messages align right inside that column with a
filled neutral bubble, while assistant text, reasoning rows, and tool rows keep
a common left edge and do not become filled message cards.
Only real reasoning projections are labeled `Thinking` in the UI; `Reasoning`
and `Preamble` remain internal protocol/projection terms and must not appear as
ordinary transcript headers. Empty reasoning completions and no-text assistant
updates close live state without rendering placeholder rows. Completed rows do
not show a default `completed` badge; running, failed, cancelled, needs-input,
and diagnostic states may still show compact status.

Workbench renders assistant text from a `tool_calls` message as assistant text
in the position where the model emitted it. Such text may describe the next
phase or introduce a tool call, but it is not Thinking and must not be hidden
behind a reasoning accordion. Later assistant text remains ordered by the
message-derived projector rather than by a Web-only final-answer heuristic.
During live streaming, Gateway supplies the same ordering contract: the active
turn has explicit live block order, so pending/running tool observations cannot
float above the visible assistant text that introduced them, and no-text
reasoning completions cannot create empty Thinking cards.
Reasoning deltas shown before assistant `message_end` are real live Thinking
observations. Once `message_end.content[]` arrives for that assistant segment,
Workbench treats the Gateway entry as an authoritative block snapshot and
replaces the previous live blocks for that entry. Gateway marks that entry
metadata with `projection: "assistant_segment"`, `liveOrder`, monotonic
`streamSeq`, and `authoritativeBlocks: true`; non-authoritative updates for the
same segment must not keep that flag set. Because the public runtime
`message_end` projection can hide reasoning, Gateway must preserve any previous
non-empty live reasoning block in the authoritative snapshot unless the final
content supplies its own reasoning block. Text that was earlier misclassified
by a provider and is later confirmed as assistant text must still disappear
from Thinking and render as assistant text.
Gateway treats every assistant `message_end` as a live segment boundary,
including display-hidden assistant messages such as `write_stdin` polls that
merge into an earlier `exec_command`. Hidden assistant messages must not cause
the next assistant reasoning delta to append to the previous Thinking row.
Workbench treats an optimistic submitted prompt as the first live row for its
turn once `turnStarted` supplies the turn id. Authoritative assistant segment
snapshots also remove stale same-turn pending-only tool overlay rows that were
superseded by the final segment tool blocks.

Workbench tool rows match TUI tool projection. A yielded `exec_command` remains
one row while later empty `write_stdin` polling appends output and completion to
that row; the poll itself is hidden from the transcript. Collapsed tool headers
show the tool name and a short argument subject, never full result JSON. Full
commands, SQL, arguments, and results stay in structured expandable detail.
Runtime background lifecycle events for a yielded `exec_command`, including
output deltas and completion, must also merge back into that original tool row.
The row must not remain `running` merely because the model did not explicitly
poll the returned `session_id`.
Ordinary Workbench transcript rendering must not show raw argument/result JSON
in collapsed or expanded tool rows; raw payloads remain available only through
developer diagnostics such as Debug. Workbench consumes the existing
`metadata.display` tool display spec when present and otherwise falls back to
core tool defaults, so Gateway does not need an additional display-hint RPC or
protocol field for this rendering slice. Desktop and mobile headers must keep
the subject clipped inside the row without pushing status markers outside the
visible transcript width.

Closing and reopening the browser must call `thread/resume`, hydrate the latest
snapshot, and continue applying live events without losing prior transcript
entries. Snapshot refreshes may replace live ids with message-derived ids but
must not drop optimistic user messages or currently streaming
assistant/reasoning text.
Selecting a historical session uses the same message-derived projection as
startup and reconnect; `No messages yet` is reserved for no selected thread or a
truly empty selected thread, not for a failed transcript projection.
