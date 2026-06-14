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
- provider secret storage in the browser, and arbitrary host-file editing
  outside the active project root
- headless API contract, which belongs to [221 pevo Serve](../221-pevo-serve/spec.md)

## Lifecycle

`pevo gateway` with no subcommand is equivalent to `pevo gateway open`.
Lifecycle commands emit exactly one JSON object to stdout so tests, desktop
shells, and automation can parse them without scraping human text.

`pevo web` is a top-level convenience alias for `pevo gateway open`. It keeps
the same JSON-only stdout contract and defaults to opening the current working
directory. GUI or desktop-shell no-project entrypoints may request the default
workspace workdir instead of the launcher cwd.

Managed state lives under `$PSYCHEVO_HOME/gateway/`:

- `server.json`: non-secret pid, address, version, executable fingerprint,
  static asset directory, asset mode, and timestamps
- `token`: the managed server bearer token, owner-readable only
- `lock`: lifecycle mutual-exclusion lock
- `server.log`: appended stdout/stderr from the background server

The directory is owner-only. `server.json` must not contain the token.
`$PSYCHEVO_HOME` is the resolved active profile home from
[057 Profiles](../057-profiles/spec.md). One managed Gateway server belongs to
one active profile; lifecycle commands do not start, stop, or reuse managed
servers from other profiles.

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
`--no-browser` is set. `pevo gateway open --default-workspace` resolves the
configured workspace root, creates `<root>/general` on demand, and launches it
as an ordinary workdir. `--print-url` prints the one-time launch URL and expiry
metadata in the JSON response for Playwright and desktop shells.

The launch URL carries only opaque launch material. It must not contain the raw
absolute workdir. Launch entries are in-memory, single-use, and expire after 30
seconds. A successful launch sets an HttpOnly SameSite=Lax browser-session
cookie and redirects to a clean Web Shell URL. Reopening a consumed launch URL
with a valid browser-session cookie redirects to the clean shell. Reopening it
without a valid browser-session cookie returns a launch-expired diagnostic page
with the recovery command.

The managed cookie authorizes workdirs granted by a launch/open flow in the
current server process, workdirs created by browser workspace-management RPCs,
and workdirs explicitly adopted from human-visible global session groups. A
browser session may adopt another workdir by resuming a stored session or by
starting a new draft from that workdir group in the Sessions browser, but it
may not request arbitrary workdirs that have no visible stored session. Direct
Bearer API clients may request any local workdir accessible to the Psychevo
process.

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

Gateway request scopes remain workdir-scoped and do not carry profile
selectors. Workbench may display the profile reported by `initialize`, but the
first-slice browser UI does not switch profiles inside an existing Gateway
process. Launching another profile requires a separate `pevo -p <name> web` or
equivalent process.

Workspace-management RPCs are UI conveniences, not a second execution scope.
`workspace/create` accepts a display name, creates a direct child directory
under the configured workspace root, returns the canonical workdir and matching
`GatewayRequestScope`, and updates the browser session to that scope. It must
reject empty names, path separators, `.`/`..`, and names that resolve outside
the workspace root. The created workdir then behaves exactly like any other
workdir for sessions, files, diff, skills, agents, and `.psychevo` overlays.

The Web Shell uses the same Gateway agent and command APIs as TUI. Its Agents
panel lists local, generated peer, Markdown-shadowed peer, invalid, and
shadowed definitions from the shared catalog. It can open peer threads, run
subagents, edit Markdown agent definitions, display backend diagnostics, and
execute `/agent:command` namespaced peer slash commands.
Gateway exposes agent and backend management as typed RPCs rather than
Workbench-only JSON shapes. Agent RPCs cover list/read/write/delete/status.
Backend RPCs cover list/write/delete/doctor and always resolve against the
request scope's workdir plus the active profile home. Backend writes must name
an explicit target, `project` or `profile`; project writes update
`<workdir>/.psychevo/config.toml`, while profile writes update the active
profile config, normally `$PSYCHEVO_HOME/config.toml` and the explicit
`PSYCHEVO_CONFIG` file when that environment override is active. Workbench GUI backend forms are embedded in
Settings > Agents and only submit Profile-level writes or deletes; they do not
expose the backend target selector. `backend/write` treats blank label and
description as absent optional metadata, while backend views still expose an
effective label that falls back to the backend id for display. Blank CWD writes
the internal `invocation` sentinel; ACP peer launch resolves empty or
`invocation` CWD to the active request scope workdir, relative CWD values under
that workdir, and absolute values as entered. Workbench exposes backend enabled
state and `peer`/`subagent` entrypoint selection as row-level controls in
Settings > Agents and persists them with the same Profile-level backend write
path. Workbench may present Command, Args, and Env as one JSON editor for
usability, but it still submits the existing `backend/write` `command`, `args`,
and `env` fields after client-side validation. A single Gateway process never
reads or writes inactive profiles.
These RPCs expose Gateway-owned camelCase views. They must not leak runtime
internal snake_case status records or arbitrary `serde_json::Value` projections
into Workbench-facing contracts. `agent/list` returns active and shadowed agent
views plus diagnostics; `agent/status` returns structured run views and control
state for the selected thread or all runs; `backend/list` returns effective ACP
backend views with source targets and diagnostics.
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
For ACP peer-agent turns selected from the Workbench composer, Gateway is the
ACP client and uses `agent-client-protocol` 0.14.0. It must prefer ACP protocol
v2 and fall back to v1 when initialization cannot negotiate the newer version.
Gateway streams the peer's standard `session/update` notifications through this
same live transcript path. `agent_message_chunk` updates appear as incremental
assistant text, `agent_thought_chunk` updates appear as a live `Thinking`
reasoning block, ACP tool updates appear as live tool rows, and v1 `plan` plus
v2 `plan_update` item updates appear as a live plan/status row. `usage_update`
is retained and forwarded as a live usage event; available-command, mode,
config, and unknown/future updates are retained structurally for diagnostics
and future surfaces. The final snapshot persists accumulated text and reasoning
and must not erase already-rendered ACP peer tool or plan blocks from the live
entry. Persisted ACP peer tool results carry the peer display title and source
metadata so a post-turn snapshot or reload does not collapse a peer-provided
tool title back to a generic local tool command label. Web/Desktop behavior
must therefore match ACP event-stream semantics rather than showing only a
synthesized final answer.
Workbench must make running ACP peer text visually incremental even when the
peer emits coarse chunks or the browser receives several gateway events in one
render frame. It may use a presentation-only reveal buffer for running
assistant text and reasoning blocks, but Gateway event state, persisted
transcript entries, copy text, and snapshot reconciliation remain canonical.
For peer turns, Gateway also maps Workbench's submitted `model` and
`reasoningEffort` controls to ACP v2 session config options before
`session/prompt` when the peer offers compatible `model` and `effort` select
options. Unsupported or unmatched peer options leave the peer default in place
and emit diagnostic events; they do not fail the user turn.
ACP peer `usage_update` events are retained as structured ACP peer events and
projected into Status observability for the peer session when they include a
usable `used`/`size` context pair. The Status context total then reflects the
peer-reported context window rather than the local prompt estimate, and the
session usage summary uses the peer-reported used tokens and cost when no
durable provider accounting exists for that peer turn. That projection is not a
long-lived session identity: starting a non-ACP-peer turn on the same Psychevo
session clears the retained peer usage projection while preserving the peer
native session id so a later peer turn can still resume the ACP backend session.

`thread/trace` reads the selected thread's persisted observability trace when a
sidecar exists. It accepts `threadId`, optional `afterSeq`, and optional `limit`.
The result returns `available`, bounded `events`, `warnings`, `truncated`, and
`nextAfterSeq`. The API is for debugging and evaluation timing enrichment only:
`events` may include legacy schema v1 records or compact schema v2 facts, and
debug surfaces must render them as generic JSON rather than transcript data.
Trace read failures and missing trace files must not affect transcript reads,
live transcript rendering, turn execution, or ordinary Workbench interaction.
Workbench must not feed `thread/trace` records into transcript rendering.

`observability/read` returns the shared UI observability projection for a
request `scope` and optional `threadId`. Its `context` field uses the same shape
as `context/read`, including safe structured per-category counting details when
available; its `usage` field is a session-level summary of persisted visible
message/accounting facts for context input, billable input/output, reasoning,
cache read/write, provider-reported total tokens, estimated cost,
unknown-pricing message count, provider/model, and derived cache-read percent.
The method is display-only and must not return prompt text, message bodies,
tool argument bodies, provider request text, raw trace records, or other raw
provider payloads. Category details are limited to numeric/counting facts such
as skill token estimates, history role counts, project-context counts,
selected-skill-context counts, and tool counts. It respects the selected session
and any resume/authorization boundary used by `thread/read` and
`thread/resume`. If no session is active or selected, `context` is unavailable
and `usage.available` is false.

`context/read` remains supported for compatibility. Workbench and future GUI
status/detail surfaces should prefer `observability/read` so context-window,
token, cache, and cost displays stay consistent across resume and session
switches.

Creating a new Web thread or selecting an existing history thread rebinds the
current Web source without archiving the previously selected thread. Only an
explicit `source/reset`, archive action, or delete action may remove a thread
from the active history list.

Workbench history is a global session browser. `thread/list` with no workdir
filter returns all human-visible sessions from the local state database; the
stored session workdir is used only for grouping and for the target scope on
resume. Rows are grouped by workdir, with the current workdir first and all
other workdirs ordered by latest session activity. Runtime `source` may appear
in diagnostics but must not appear in history rows/search or decide whether
GUI, TUI, ACP, Web, or Desktop sessions are visible by default.

When Workbench resumes a session from another workdir, it switches the active
scope to that session's stored workdir before accepting more input. The file
tree, `@` completion, diff/status panes, agents, skills, and subsequent turns
refresh against the resumed workdir. Cross-workdir resume must not splice the
old session's transcript into the launch workdir. Archiving, restoring,
renaming, and deleting sessions operate from the same global list and must
respect running/current-session guards across every source.
Starting a new session from another workdir group switches the active browser
scope to that stored workdir and returns an empty source snapshot for that
workdir, without first requiring the user to resume an older session.

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
GUI `/agents` is one such hidden command because current-session agent selection
is handled by the composer selector and app-level ACP backend configuration
lives in Settings > Agents. If a hidden command is typed explicitly,
`command/execute` returns `known=true`, `accepted=false`, bounded guidance, and
optional alternate action. Unknown slash-looking input returns `known=false`
with a `passThroughPrompt` host action.

Workbench applies command results by destination rather than by transcript
insertion. Navigation commands switch panels, structured inspection commands
open their domain view such as preview or status, active-turn controls update
local activity state, submit-style slash commands start a normal model turn, and
export commands invoke the host download/share path. Display-only feedback from
commands must not be persisted as transcript entries. Panel host actions must
reveal their destination in desktop and mobile layouts; focusing Status or
History is not sufficient if the corresponding inspector/sidebar is collapsed.
Undo and redo slash commands execute through `command/execute` and return host
actions instead of adding transcript rows. `sessionUndo` includes the current
thread id, restored prompt text, and reverted message count; `sessionRedo`
includes the current thread id, restored message count, and whether redo
completed the revert chain. Workbench refreshes the thread snapshot, history,
and workspace-derived views after either action; `/undo` places the restored
prompt in the composer and `/redo` clears it. If a turn is running, Gateway
does not restore snapshots and instead returns a local interrupt action with
bounded feedback asking the user to rerun the command after the turn settles.
Composer-triggered help or browse actions for commands and agents use closeable
overlays over the current transcript so the active session and composer remain
visible. Composer-triggered inspect feedback may be mirrored near the composer
while the destination panel is revealed. Queue actions preserve the original
slash line as their display text when they submit expanded prompt text through
`turn/start`. Display-only command feedback and overlays are transient to the
current session/workdir and are cleared on session switches and new input.
Successful display-only feedback with no follow-up action may auto-dismiss after
a short delay and may be dismissed by clicking outside its panel. Error feedback
and feedback with follow-up actions must remain until explicit dismissal or a
normal transient clear.

Workbench refreshes `observability/read` after `thread/resume`, `thread/read`,
turn completion, undo/redo workspace refresh, and explicit session switches,
including same-workdir resume where the file tree and diff may not otherwise
need to change. New detached drafts or no-active-session states clear stale
session usage metrics, and delayed observability responses from a previous
selected thread or view epoch must not reapply to the current Status panel.
Compact UI surfaces may show context percent, session tokens, cache-read
percent, and estimated cost; richer details belong in the right Status view.
Opening the compact composer context/status popover is a local display action
and must not reveal, focus, or change the open state of the right Status
inspector.

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

- left: collapse control, `New Session`, `Search`, global
  `Pinned`, project-grouped Sessions with expand/collapse and per-project new
  session actions, and a bottom utility rail for Settings. Settings is the
  persistent app-level configuration center whose left navigation lists
  `Appearance`, `Debug`, and `Agents` directly, with bottom-aligned
  `Archived sessions` for archived-session management; the Agents section
  contains embedded Profile-level ACP backend configuration. The ordinary
  Workbench left sidebar always lists active sessions and does not become an
  archived-session filter.
  Composer-triggered `/commands` remains a closeable overlay over the transcript.
  GUI `/agents` is not exposed by Web/Desktop command discovery or panel routing.
- center: transcript/workbench and bottom-fixed composer
- right: a resizable workspace with a status/navigation home and typed tabs for
  `Review`, `Terminal`, and `Files`

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
state, workdir, context usage, and changed-file summary, then offers bordered
icon-and-label rows to open Review, Terminal, and Files tabs. Rows do not carry
right-side explanatory copy. Once any tab is open, the tab strip includes a `+`
menu for creating more tabs of those types. Browser is not exposed in this
slice.

Review tabs are ordered around current work state and structured diff review.
A Review tab has a top-right Files toggle; when pressed, the tab splits into
left diff preview and right changed-files tree. Selecting a changed file scopes
the active Review tab to that file when Gateway can provide a file-specific
diff. The `/diff` host action opens or updates a Review tab. Files tabs split
into left preview and right tree. That left-preview/right-tree split is the
desktop information architecture for both Review and Files, with stacking only
for narrow responsive layouts. Review and Files use the same locally filterable
tree component with folder expand/collapse and selected row state. Preview and
tree regions are immersive right-workspace panes with subtle split dividers
instead of framed card backgrounds. Markdown previews reuse the shared
transcript Markdown renderer with raw HTML escaped and light/dark theme-adapted
code blocks. Code previews use a Workbench-local `highlight.js` core
integration with app-token colors. The Files header does not repeat the project
path; the selected file absolute path is shown above the preview. Diff previews
use theme-adapted surfaces so light appearance does not retain dark diff
panels. Diff file headers are compact UI identifiers, not raw Git metadata:
they show status marker, workspace-relative path, and addition/deletion counts,
while absolute paths are reserved for tooltip text when the active workdir is
available. Unsupported preview formats and Gateway binary/unreadable file
responses stay in the Files tab as unavailable preview states instead of
opening a center preview.

Review also exposes Gateway review groups when available. `workspace/changes`
returns groups ordered by turn with file-level pending, accepted, rejected, and
conflict states. `workspace/change/accept` only marks a file accepted.
`workspace/change/reject` restores that file to the turn-start baseline,
removes files created by that turn, and restores files deleted by that turn.
Reject must preserve file content that existed before the selected turn,
including pre-existing dirty or untracked files. If the current file revision
differs from the stored post-turn revision, Reject is blocked and the file row
is reported as conflicted.

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
Transient startup, error, and exit text may appear inside the terminal panel
only when needed.

The Gateway terminal API backs right-workspace Terminal tabs. It is separate
from composer shell mode and does not create transcript entries. The methods
are:

- `terminal/start`: accepts `scope`, optional `cwd`, terminal `cols`, and
  terminal `rows`; validates the requested workdir against the same scope rules
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
search field, and the current project/workdir path is not repeated there. The
internal left navigation lists `Appearance`, `Debug`, and `Agents` directly,
with `Archived sessions` pinned to the bottom.
`Appearance` includes a local appearance control with `dark` and `light`
choices, `Archived sessions` directly lists archived sessions for restore/delete
workflows, and `Debug` owns the local Debug switch. The ordinary Workbench left
sidebar remains an active-session list and must not switch to archived sessions.
The default is the dark ledger appearance. The setting is a Workbench host
preference and does not require Gateway to persist provider/runtime
configuration. The light palette is a warm reading-paper treatment with ivory
canvas, warm paper panels, taupe borders, warm charcoal text, and low-chroma
amber/taupe active states rather than cool blue chrome. The dark palette keeps
the near-black ledger structure, removes cold blue sidebar bias, and uses
higher-luminance primary, muted, and navigation text so Gateway-rendered
status/settings data remains readable under both appearances. The `Agents`
section shows only configurable Profile-level ACP backend registrations and
diagnostics; it does not list the read-only effective agent catalog or
Project-level backend definitions because those are not configurable from the
GUI. Its icon-only add control opens a generic empty ACP backend editor rather
than an OpenCode-specific preset. Each listed Profile ACP backend exposes its
enabled state as a row-level switch in Settings > Agents plus ordinary
checkboxes for the `peer` and `subagent` entrypoints. The editor does not
duplicate those row controls. The editor only requires ID and a valid command
string inside its Command JSON input; Label and Description are optional
metadata, and default CWD is shown as the invocation workdir with a
resolved-path helper instead of the raw `invocation` sentinel.
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
commands, SQL, arguments, and results stay in structured expandable detail.
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
