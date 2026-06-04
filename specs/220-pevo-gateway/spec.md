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

Managed server reuse must prove that the running process is the same local
build and asset set that the caller would start now. `open` and `start` may
reuse an existing server only when the pid is alive, `server.json` includes an
executable fingerprint, that fingerprint matches the current `pevo` executable,
the running process executable is not a deleted Unix inode, and the recorded
static asset directory matches the directory resolved for the current command.
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

The managed cookie authorizes only workdirs that were granted by a launch/open
flow in the current server process. Direct Bearer API clients may request any
local workdir accessible to the Psychevo process.

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

If the user submits a prompt from an empty source snapshot, the Web Gateway
creates and binds a concrete thread before starting the turn. All live
transcript events for that turn are emitted with the owning `threadId`, so a
background running turn cannot be projected into whichever thread is currently
visible.
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

Creating a new Web thread or selecting an existing history thread rebinds the
current Web source without archiving the previously selected thread. Only an
explicit `source/reset`, archive action, or delete action may remove a thread
from the active history list.

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

The Web Shell executes shared slash commands through `command/execute` when the
command has a Gateway representation. Host-only results such as copy, export,
share, and download are returned as structured client actions and performed by
the host adapter.

## Workbench Layout

The desktop layout is a dense three-column workbench:

- left: history list and session lifecycle actions
- center: transcript and composer
- right: status/queue, settings/auth/model, diff, and export/share panels

Narrow layouts keep transcript and composer as the primary surface and collapse
history and utility panels into bottom tabs or drawers. The UI should present
as an operational workbench, not a landing page.

First-slice panels include transcript, composer, history, status/queue,
settings/auth/model, diff placeholder, export/share, permission, and clarify.
Memory and resource surfaces are status-only in the first Web slice.

The composer matches TUI keyboard behavior: plain Enter submits, modifier Enter
variants insert newline, IME composition is respected, and running-turn prompt
submission steers by default. Queueing remains available as an explicit composer
mode and via `/queue`.

The transcript renders user and assistant Markdown, streams assistant and
reasoning updates without waiting for turn completion, keeps observed block
order, and follows the bottom while the user has not intentionally scrolled
away. Tool calls render as collapsible evidence rows with parameters and
results shown once.
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
