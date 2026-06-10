---
name: 220. pevo Gateway
psychevo_self_edit: deny
---

# 220. pevo Gateway

Define the concrete `pevo gateway` product surface and managed Web Shell
behavior.

## Scope

- `pevo gateway open/start/status/stop/restart` lifecycle behavior
- managed local server state and browser launch bootstrap
- Web Shell layout, panels, source binding, and reconnection behavior
- browser/PWA first-slice behavior

Out of scope:

- public LAN, relay, TLS, account, or hosted service behavior
- native desktop or mobile shell packaging
- arbitrary config-file editing or provider secret storage in the browser
- headless API contract, which belongs to [221 pevo Serve](../221-pevo-serve/spec.md)

## Lifecycle

`pevo gateway` with no subcommand is equivalent to `pevo gateway open`.
Lifecycle commands emit exactly one JSON object to stdout so tests, desktop
shells, and automation can parse them without scraping human text.

`pevo web` is a top-level convenience alias for `pevo gateway open`. It keeps
the same JSON-only stdout contract and defaults to opening the current working
directory.

Managed state lives under `$PSYCHEVO_HOME/gateway/`:

- `server.json`: non-secret pid, address, version, executable fingerprint,
  static asset directory, asset mode, and timestamps
- `token`: the managed server bearer token, owner-readable only
- `lock`: lifecycle mutual-exclusion lock
- `server.log`: appended stdout/stderr from the background server

The directory is owner-only. `server.json` must not contain the token.

`open` and `start` reuse the same server implementation as `pevo serve`.
Managed mode passes internal flags to mount Web Shell assets, generated token
state, and launch bootstrap state. The public `pevo serve` command remains
headless.

Managed `open`, `start`, and `restart` spawn the `serve` child as an
independent long-lived process. The child must keep running after the opener
command exits, so a ready `server.json` cannot immediately become stale because
the caller's shell, terminal, or test harness closed its process group.
When no `--bind` is provided, managed commands prefer `127.0.0.1:58080` and may
fall back through `127.0.0.1:58099` when a lower port is already in use. The
actual bound address is persisted in `server.json` and reported through
`baseUrl`/`readyzUrl`. An explicit `--bind` disables fallback and must either
reuse a matching managed server or start exactly on the requested address.

Managed server reuse must prove that the running process is the same local
build and asset set that the caller would start now. `open` and `start` may
reuse an existing server only when the pid is alive, `server.json` includes an
executable fingerprint, that fingerprint matches the current `pevo` executable,
the running process executable is not a deleted Unix inode, and the recorded
static asset directory matches the directory resolved for the current command.
Default-bind callers may reuse only a server bound inside the managed fallback
range. Explicit-bind callers may reuse only a server whose recorded address
matches the requested address.
Old-style `server.json` files without those fields are stale. A stale managed
server is stopped, its token/state are rotated, and a new `serve` child is
started. `gateway status` reports stale managed state with `stale: true` and a
machine-readable `staleReason` instead of reporting a live pid as healthy only
because it still exists.

## Launch Bootstrap

`pevo gateway open --dir <dir>` canonicalizes the workdir, ensures the managed
server is running, records a launch entry, and opens the browser unless
`--no-browser` is set. `--print-url` prints the one-time launch URL and expiry
metadata in the JSON response for Playwright and desktop shells.

The launch URL carries only opaque launch material. It must not contain the raw
absolute workdir. Launch entries are in-memory, single-use, and expire after 30
seconds. A successful launch sets an HttpOnly SameSite=Lax browser-session
cookie and redirects to a clean Web Shell URL. Reopening a consumed launch URL
with a valid browser-session cookie redirects to the clean shell. Reopening it
without a valid browser-session cookie returns a launch-expired diagnostic page
with the recovery command.

The managed cookie authorizes workdirs granted by a launch/open flow in the
current server process and workdirs explicitly adopted from human-visible
global session projects. A browser session may adopt another project by
resuming a stored session or by starting a new draft from that project group in
the Sessions browser, but it may not request arbitrary workdirs that have no
visible stored session. Direct Bearer API clients may request any local workdir
accessible to the Psychevo process.

Direct browser visits to the managed base URL without a valid browser-session
cookie are not authorized Web Shell launches. They should return a local
launch-required diagnostic page with the recovery command, rather than mounting
the Workbench SPA and letting it fail later with a generic WebSocket error.

## Web Shell

The first Web Shell is `apps/workbench`. Source installs build
`apps/workbench/dist` and copy it to the install-share location beside the
`pevo` binary: `../share/psychevo/web`. Runtime asset resolution prefers an
explicit internal static dir, then `PSYCHEVO_WEB_DIST`, then the install-share
location, then a recognizable source checkout's `apps/workbench/dist`, and
last the legacy current-working-directory `apps/workbench/dist` fallback.

`pevo gateway open` and `pevo web` do not run `pnpm` implicitly. If assets are
missing, lifecycle JSON reports `workbench_dist_missing` with the searched
paths, `PSYCHEVO_WEB_DIST`, the build command, and the install command instead
of only echoing one missing cwd-derived path.

The Web Shell source kind is `web`. Source identity is derived from source kind
plus canonical workdir unless the client provides an explicit `rawId`. Multiple
managed browser clients for the same workdir share one source/thread, active
queue, event stream, and control surface.

The Web Shell uses the same Gateway agent and command APIs as TUI. Its Agents
panel lists local, generated peer, Markdown-shadowed peer, invalid, and
shadowed definitions from the shared catalog. It can open peer threads, run
subagents, edit Markdown agent definitions, display backend diagnostics, and
execute `/agent:command` namespaced peer slash commands.
The composer exposes active, runnable agent definitions from the same catalog
as a main-agent selector. The empty selector value is displayed as
`Default Agent` and submits future turns with no `agentName`; concrete
selections submit that agent name and are persisted to the current session's
main-agent metadata. Shadowed and invalid definitions remain visible only in the
Agents panel diagnostics, not in the composer selector. `settings/read` returns
the current session's selected main Agent in `controls.agent` when a `threadId`
is supplied, or `null` for a draft/default session. `settings/update` accepts
`agent: string | null` with a `threadId`, validates concrete Agents against the
active catalog, and writes either concrete main-agent metadata or an explicit
session default marker. It does not write project-local Agent defaults.

Startup and reconnect call source-default `thread/resume` with `params.scope`.
Gateway returns the current thread snapshot when a binding exists, or an empty
source snapshot before the first turn. The client treats Gateway snapshot data
as authoritative and does not infer active turns, queues, permissions, or
clarify requests from stale local state.
Every `ThreadSnapshot` response includes the `entries` array. A missing
`entries` field is a protocol error and must not be interpreted by Web clients
as an empty transcript.
When `thread/read` or explicit `thread/resume` targets a session whose
`SessionSummaryView.messageCount` is greater than zero, Gateway must project the
persisted messages into non-empty `TranscriptEntry[]` whenever those messages
contain visible user or assistant content. A history selection must not return a
normal empty transcript snapshot for such a session.

`thread/start` is a new-source operation, not a session-creation operation. It
clears the current source binding and returns an empty source snapshot with
`thread = null`, without archiving the previously selected thread or inserting a
placeholder session. That empty source snapshot is a detached draft: delayed
events or read-only snapshot refreshes for previously running threads must not
bind it back to an older thread. Only the draft's own first accepted prompt or
shell result may attach the Web view to the newly resolved runtime thread.
When another turn for the same browser/project source is already running,
`thread/start` creates an internal draft source lane for the returned snapshot.
The draft lane lets the first prompt or shell command start immediately instead
of queueing behind the previous turn. The lane may appear as
`ThreadSnapshot.scope.source.rawId`, but it is internal routing state: it must
not appear in session history rows, grouping, search, display titles, or
runtime `source` classifications. When the draft resolves to a durable thread,
Gateway may bind the canonical project source to that new thread only if no
newer source generation has superseded it; stale completions from previously
running turns must not overwrite the binding.
Web clients may show a local draft row for this detached draft in their History
UI, but that row is not a persisted session and must not be exposed as a
Gateway `SessionSummaryView`.
Starting another new session while an unpersisted local draft is selected
replaces the client-local draft row instead of accumulating multiple draft rows;
only an accepted prompt or shell command can turn a draft into a durable
session.
When the detached draft is started from a project scope, the draft row appears
inside that project's Sessions group rather than as a global item above all
projects. If the target project has no persisted visible sessions yet, the
client may create a temporary project group for the draft.
`source/reset` is stronger: it ends and archives the previously bound thread,
clears the source binding, and returns the same empty source snapshot without
creating a replacement session.

If the user submits a prompt from an empty source snapshot, the Web Gateway
validates the input before resolving or creating a concrete thread. A valid
first prompt starts against the source key; runtime creates the durable session
when it persists the first user request, and Gateway binds the source to that
session after the result resolves. Live events may initially arrive before a
durable thread id is known, but completion and snapshot refreshes must carry the
owning `threadId`, so a background running turn cannot be projected into
whichever thread is currently visible.
Workbench updates the active thread binding from `turnStarted` and live entry
events, but ordinary transcript rendering during the active turn is driven by
Gateway live transcript events. It must not poll `thread/read` on a timer during
the turn and then merge the returned message-derived entries with the same live
overlay. Snapshot reads during reconnect or explicit refresh may return
in-progress message-derived entries; those entries replace any overlapping
same-turn live overlay blocks rather than appearing as a second copy. A live
entry may keep only blocks that remain uncovered by the message-derived
snapshot; covered assistant text, reasoning, and tool blocks are removed even
when another block in that live entry is still running.
If more live events arrive after that snapshot, Workbench continues applying
the same rule to the incoming event. A live tool update for an already
message-derived tool call may update that message-derived tool row's transient
running state, but it must not render as a separate duplicate live row.

`thread/trace` reads the selected thread's persisted observability trace when a
sidecar exists. It accepts `threadId`, optional `afterSeq`, and optional `limit`.
The result returns `available`, bounded `events`, `warnings`, `truncated`, and
`nextAfterSeq`. The API is for debugging and evaluation timing enrichment only:
`events` may include legacy schema v1 records or compact schema v2 facts, and
debug surfaces must render them as generic JSON rather than transcript data.
Trace read failures and missing trace files must not affect transcript reads,
live transcript rendering, turn execution, or ordinary Workbench interaction.
Workbench must not feed `thread/trace` records into transcript rendering.

Creating a new Web thread or selecting an existing history thread rebinds the
current Web source without archiving the previously selected thread. Only an
explicit `source/reset`, archive action, or delete action may remove a thread
from the active history list.

Workbench history is a global session browser. `thread/list` with no workdir
filter returns all human-visible sessions from the local state database; the
stored session workdir is used only for grouping and for the target scope on
resume. Rows are grouped by project, with the current project first and all
other projects ordered by latest session activity. Runtime `source` may appear
in diagnostics but must not appear in history rows/search or decide whether
GUI, TUI, ACP, Web, or Desktop sessions are visible by default.

When Workbench resumes a session from another project, it switches the active
scope to that session's stored workdir before accepting more input. The file
tree, `@` completion, diff/status panes, agents, skills, and subsequent turns
refresh against the resumed project. Cross-project resume must not splice the
old session's transcript into the launch workdir. Archiving, restoring,
renaming, and deleting sessions operate from the same global list and must
respect running/current-session guards across every source.
Starting a new session from another project group switches the active browser
scope to that project's stored workdir and returns an empty source snapshot for
that project, without first requiring the user to resume an older session.

The left Sessions browser owns the session-history controls. It has one
compact header with the history icon to the left of `Sessions`; it does not
render a separate `History` title or an `active across projects` subtitle. The
header includes one expand/collapse-all toggle for project groups; when any
project is collapsed it expands all groups, otherwise it collapses all groups.
Each project group can be collapsed independently, shows only the project label
in its header, and has a right-aligned `+` action for starting a new session in
that project. Group headers do not show session counts. Session rows put the
display title and timestamp on the same line with the timestamp right-aligned;
the row does not show project name or entry count under the title. The archive
history toggle belongs in Settings rather than the Sessions header. The
Sessions scroller reserves a stable scrollbar gutter so project header actions
do not shift horizontally as overflow appears or disappears.

Selecting or creating a Web thread is allowed while another thread is running.
The original thread continues in the background, remains visible in history with
running/queued state, and can be interrupted by selecting it or by thread-scoped
controls. Running threads cannot be archived or deleted until their active turn
finishes or is interrupted.

The Web Shell uses Gateway `completion/list` for `/`, `$`, and `@` composer
completion. `$` completion resolves skills, local agents, and ACP capability
mentions; accepted entries keep the visible `$name` text and send structured
Gateway mentions on submission. `@` completion is scoped to the launched
workdir and must not let the browser read arbitrary host files directly.
Long completion lists remain keyboard-operable: ArrowUp/ArrowDown and
Ctrl+P/Ctrl+N update the active option and keep it visible inside the popover
without moving focus out of the composer textarea.

The Web Shell `Search` action opens a center-surface search view. The first
slice searches the current workdir's known session ids, session titles, and
visible message text from `thread/read` snapshots. Search results resume the
matching session in the transcript surface; they do not create a right-side
utility tab and do not search arbitrary host files.

The Web Shell executes shared slash commands through `command/execute` when the
command has a Gateway representation. Host-only results such as copy, export,
share, and download are returned as structured client actions and performed by
the host adapter.
`command/list` is a typed Gateway protocol method returning the same
capability-filtered catalog used by slash completion. The catalog and
`command/execute` behavior are projected from the runtime command registry;
Web must not carry a separate hard-coded slash inventory beyond applying typed
host actions returned by Gateway. Unknown slash-looking input, including
absolute-path-looking input, is returned as prompt passthrough instead of a
local command error.
The Web/Desktop surface profile is derived by Gateway from the request source
and is not declared by the browser client. `command/list` includes runtime
presentation metadata (`presentationKind`, `destination`, `feedbackAnchor`, and
optional `alternateAction`) for visible commands. Commands hidden because
Workbench cannot represent them are omitted from discovery and slash completion;
if typed explicitly, `command/execute` returns `known=true`, `accepted=false`,
bounded guidance, and optional alternate action. Unknown slash-looking input
returns `known=false` with a `passThroughPrompt` host action.

Workbench applies command results by destination rather than by transcript
insertion. Navigation commands switch panels, structured inspection commands
open their domain view such as preview or status, active-turn controls update
local activity state, submit-style slash commands start a normal model turn, and
export commands invoke the host download/share path. Display-only feedback from
commands must not be persisted as transcript entries. Panel host actions must
reveal their destination in desktop and mobile layouts; focusing Status or
History is not sufficient if the corresponding inspector/sidebar is collapsed.
Composer-triggered help or browse actions for commands and agents use closeable
overlays over the current transcript so the active session and composer remain
visible. Composer-triggered inspect feedback may be mirrored near the composer
while the destination panel is revealed. Queue actions preserve the original
slash line as their display text when they submit expanded prompt text through
`turn/start`. Display-only command feedback and overlays are transient to the
current session/workdir and are cleared on session switches and new input.

The Web Shell supports TUI-compatible shell mode through `shell/start`.
`shell/start` accepts `scope`, optional `threadId`, and a stripped local shell
`command`; it returns whether the command was accepted plus the owning thread id
when known. When a shell command is the first user request for an empty source
snapshot, the accepted response may have `threadId = null`; the later
`shell/result.thread.id` is authoritative. Execution uses the runtime user-shell
executor, not the provider-callable `exec_command` tool surface. Live shell
start/end events are projected through the ordinary Gateway event stream as
shell evidence rows. After completion or error, Gateway sends a notification
that lets Workbench refresh the owning snapshot and history.

Shell mode is an explicit composer mode, not a literal prompt prefix. Entering
`!` in an empty composer switches into shell mode and displays the shell marker;
submitting sends only the stripped command to `shell/start`. Imported, pasted,
or history-restored composer text that begins with `!` enters shell mode with
the bang stripped. Empty shell mode shows bounded shell help, and Escape or
backspace on an empty shell composer exits shell mode. Slash completion is
disabled in shell mode; `@` completion remains available.

If no agent turn is active, `shell/start` runs as the thread's active local
activity and participates in interrupt and queue state. If an agent turn is
active for the same thread, shell mode starts an auxiliary shell task and
injects the bounded result into the active turn context. If a standalone shell
activity is already active, later prompt or shell submissions are queued behind
that activity.

Persisted user-shell context must reload as shell evidence, not as raw
`<user_shell_command>` XML prompt text. The visible command line uses the
prompt-surface `!<command>` label while model-visible context continues to use
the bounded runtime user-shell XML record.

## Workbench Layout

The shared Web and generic Desktop layout follows the v0 workbench direction:
a dark, dense, operator-ledger app shell with no top tab bar. Desktop may later
add native chrome and a native status bar, but the application content layout is
the same Web/Desktop tree.

The desktop layout is a three-surface workbench:

- left: collapse control, `New Session`, `Search`, `Artifacts`, global
  `Pinned`, project-grouped Sessions with expand/collapse and per-project new
  session actions, and a bottom utility rail for Settings. Agents, Skills,
  Tools, and MCP remain deferred utility surfaces and are hidden from the rail
  until the product explicitly enables them.
- center: transcript/workbench, bottom-fixed composer, and optional inline
  preview split
- right: `Status` and `Files` tabs, defaulting to `Status`

The center composer and right inspector are session-scoped. They are hidden
when no persisted session or local new-session draft is selected. Selecting a
history session or creating/selecting a local draft reveals the composer and
right inspector for that session scope.

The right `Files` tab shows only the launched project's file tree. Selecting a
supported text file previews it in the center inline split instead of inserting
a reference or opening an external host file. Unsupported preview formats and
Gateway binary/unreadable file responses do not open a preview pane. Folder rows
are locally expandable and collapsible so users can keep large trees compact.
The inline preview is part of the center surface, not a third right-tab mode,
and can be closed without changing the selected right tab.

The right `Status` tab is ordered as current work state: session identity and
activity first, compact model/permission/mode summaries second, context usage
as a graphical meter, then changed files as ledger rows. Changed-file rows are
clickable. Selecting one opens a read-only unified diff preview in the center
inline split, scoped to that file when Gateway can provide a file-specific
diff. The same preview surface is used for `/diff`, artifact preview, and file
preview so display artifacts do not enter ordinary transcript history.

Settings includes a local appearance control with `dark` and `light` choices.
The default is the dark ledger appearance. The setting is a Workbench host
preference and does not require Gateway to persist provider/runtime
configuration, but Gateway-rendered status/settings data must remain readable
under both appearances.
Settings also includes a local Debug switch. Enabling Debug adds a right-side
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
and send control are inside the input frame, the attachment button and current
Agent selector sit in the lower-left action slot, and model controls are moved
out of the text frame into the status line. The status line mirrors the TUI
footer shape with clickable permission mode, chat mode, model, variant, context
usage ring, project path, and Git branch. Context usage is graphical by
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
The same shared composer provides Web and generic Desktop shell mode. The
generic Desktop shell reuses this Workbench/Gateway behavior and identifies
itself through the host/source scope; this topic does not introduce native
desktop packaging.

The transcript renders user and assistant Markdown, streams assistant and
reasoning updates without waiting for turn completion, keeps observed block
order, and follows the bottom while the user has not intentionally scrolled
away. Tool calls render as collapsible evidence rows with parameters and
results shown once. The center transcript uses a shared reading column: user
messages align right inside that column with a filled neutral bubble, while
assistant text, reasoning rows, and tool rows keep a common left edge and do not
become filled message cards.
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
commands, SQL, arguments, and results stay in expandable detail. Desktop and
mobile headers must keep the subject clipped inside the row without pushing
status markers outside the visible transcript width.

Closing and reopening the browser must call `thread/resume`, hydrate the latest
snapshot, and continue applying live events without losing prior transcript
entries. Snapshot refreshes may replace live ids with message-derived ids but
must not drop optimistic user messages or currently streaming
assistant/reasoning text.
Selecting a historical session uses the same message-derived projection as
startup and reconnect; `No messages yet` is reserved for no selected thread or a
truly empty selected thread, not for a failed transcript projection.

## Validation

Browser validation uses Playwright against the built Workbench served by
`pevo gateway open --no-browser --print-url`, with isolated config, SQLite
state, and workdir by default. It covers desktop and narrow viewport layout,
Gateway connection, source/thread startup, history management, composer
submission, permission/clarify surfaces, and download flows.

Live model validation is explicit opt-in. When enabled, Playwright uses the
configured live provider/model in an isolated workdir and must not print
tokens or secrets.

Live skill validation is a separate opt-in Playwright path. The reusable
`live-skill` spec runs a configured skill prompt, samples the browser every
three seconds, writes screenshots as test artifacts, and compares rendered DOM
order against the isolated SQLite message-derived transcript. Each screenshot
sample prints its sample number, label, and artifact path to stdout so long
live runs expose visible progress. The sampled transcript rows also print their
nonvisual entry id, block id, block kind, turn id, status, and visible text so a
failed screenshot can be tied back to Gateway projection shape. It must fail
immediately if the Workbench
render error boundary is visible, and must fail on stale running reasoning rows
that duplicate committed reasoning, non-monotonic committed row order in the
DOM, tool result JSON in collapsed headers, or evidence header overflow. It
must also fail when an empty assistant update appears after a tool row or when
a stale completion popover remains visible after prompt submission. The default
prompt is `$x-daily`; callers may override the workdir, prompt, interval,
timeout, and model through environment variables.

## Related Topics

- [021 Gateway](../021-gateway/spec.md) defines source/thread/turn transport behavior.
- [022 UI](../022-ui/spec.md) defines shared frontend package boundaries.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines the `pevo` command product surface.
- [221 pevo Serve](../221-pevo-serve/spec.md) defines the headless API server.
