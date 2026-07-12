# Web Shell

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
plus canonical cwd unless the client provides an explicit `rawId`. Multiple
managed browser clients for the same cwd share one source/thread, active
queue, event stream, and control surface.

Gateway request scopes remain cwd-scoped and do not carry profile
selectors. Workbench may display the profile reported by `initialize`, but the
first-slice browser UI does not switch profiles inside an existing Gateway
process. Launching another profile requires a separate `pevo -p <name> web` or
equivalent process.

Workspace-management RPCs are UI conveniences, not a second execution scope.
`workspace/create` accepts a display name, creates a direct child directory
under the configured workspace root, returns the canonical cwd and matching
`GatewayRequestScope`, and updates the browser session to that scope. It must
reject empty names, path separators, `.`/`..`, and names that resolve outside
the workspace root. The created cwd then behaves exactly like any other
cwd for sessions, files, diff, skills, agents, and `.psychevo` overlays.

The Web Shell uses the same Gateway agent and command APIs as TUI. Its Agents
panel lists local, generated peer, Markdown-shadowed peer, invalid, and
shadowed definitions from the shared catalog. It can open peer threads, run
subagents, edit Markdown agent definitions, display backend diagnostics, and
execute `/agent:command` namespaced peer slash commands.
Gateway exposes agent and backend management as typed RPCs rather than
Workbench-only JSON shapes. Agent RPCs cover list/read/write/delete/status.
Backend RPCs cover list/write/delete/doctor and always resolve against the
request scope's cwd plus the active profile home. Backend writes must name
an explicit target, `project` or `profile`; project writes update
`<cwd>/.psychevo/config.toml`, while profile writes update the active
profile config, normally `$PSYCHEVO_HOME/config.toml` and the explicit
`PSYCHEVO_CONFIG` file when that environment override is active. Workbench GUI
backend forms are embedded in `Capabilities > Agents > ACP Backends` and only
submit Profile-level writes or deletes; they do not expose the backend target
selector. `backend/write` treats blank label and
description as absent optional metadata, while backend views still expose an
effective label that falls back to the backend id for display. Blank CWD writes
the internal `invocation` sentinel; ACP peer launch resolves empty or
`invocation` CWD to the active request scope cwd, relative CWD values under
that cwd, and absolute values as entered. Workbench exposes backend enabled
state and `peer`/`subagent` entrypoint selection as row-level controls in
`Capabilities > Agents > ACP Backends` and persists them with the same
Profile-level backend write path. Workbench may present Command, Args, and Env as one JSON editor for
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
main-agent metadata. Backend-backed agents, including generated ACP backend
agents, are runnable from the composer only when they support the `peer`
entrypoint; backend registrations that are disabled or `subagent`-only remain
configurable/diagnosable in `Capabilities > Agents` but must not appear in the
composer selector or remain selected there. Ordinary non-backend agent
definitions can remain selectable as current-session agents. Shadowed and
invalid definitions remain visible only in the Agents panel diagnostics, not in
the composer selector. `settings/read` returns the current session's selected
main Agent in `controls.agent` when a `threadId` is supplied, or `null` for a
draft/default session. It also returns model resolution state in
`controls.modelStatus`: `resolved` includes a concrete provider-qualified
`controls.model`, while `unconfigured` and `error` leave `controls.model` null
and expose an explicit unavailable state to Workbench. Workbench must not start
a model turn while model status is unresolved or no concrete `provider/model` is
selected. `controls.variant` represents only the current Workbench reasoning
effort override; the default/no-override state is not filled from the resolved
model's configured default reasoning effort. `settings/update` accepts
`agent: string | null` with a `threadId`, validates concrete Agents against the
active catalog, and writes either concrete main-agent metadata or an explicit
session default marker. It does not write project-local Agent defaults.
The composer model control presents model and reasoning effort as one grouped
selector. Its closed state shows the selected model id plus the current
reasoning label separated by a space, never the provider prefix, and uses the
provider-qualified model only in hover/title affordances. Visible model-row
content shows only the model id plus compact state badges such as a muted green
`Free` badge when catalog metadata marks the model as free; provider identity is
rendered as compact group headings above contiguous visible model rows from the
same provider.
`Select model` is an empty-state label, not an option and must not be submitted
as a model selection. The selector lists concrete models from `settings/read`,
including provider catalog rows fetched in Settings > Models, with recently used
models promoted ahead of the catalog order. Provider grouping is applied after
filtering and recent-model ordering, so non-adjacent runs from the same provider
remain separate groups rather than globally regrouping the list. The model group
includes a name-filter field above the model rows, and the model-row viewport is
capped to five visible model rows with overflow scrolling for the rest. Radio
rows inside the popover fill the available popover width and must not inherit
compact toolbar-button max-width rules that leave unused gutters. The popover
width adapts to the longest visible model, provider heading, or reasoning item,
with a compact minimum and viewport maximum rather than a fixed menu width. The
selector also includes a reasoning effort group for the selected model. Models with
metadata that explicitly disables reasoning expose only `none` displayed as
`Default`; models with reasoning support or unknown reasoning metadata expose
the shared reasoning effort values. Switching models preserves the current
reasoning effort when it is valid for the new model and otherwise resets the
override to `none`.
Settings > Models is Workbench's concrete surface for
[125 Model Config](../125-model-config/spec.md). It exposes provider
configuration, explicit catalog fetches, profile/global default assignment, and
title-generation/context-compression assignment controls through model-specific
RPCs. Web assignment rows reuse the composer model/reasoning picker behavior;
provider rows avoid repeating values already visible in controls; and fetched
catalog rows appear immediately in Settings and the composer without persisting
each row to config. The page must not silently hot-swap the currently running
turn or move session-scoped composer controls into Settings.
The composer presents Agent Definition and Runtime Profile through one grouped
target selector. Before binding, the headless `ThreadController` receives
compatible `RunnableTarget` choices from `thread/context/read`; React does not
pair targets or infer compatibility. After binding, the selector becomes
immutable provenance and changing either identity starts a new public thread.
Visible target labels stay single-line and Agent-first: Native shows
`agentLabel`, ACP shows `agentLabel (ACP)`, and Runtime Profile identity remains
diagnostic provenance rather than repeated secondary copy.

Model, mode, reasoning, and advanced options are typed control descriptors from
Thread Context. An unbound choice becomes a source draft. A bound choice calls
`thread/control/set` as a sticky next-turn preference. Applied and observed
values remain distinct, and unsupported or unavailable controls carry Gateway
reasons. `turn/start` submits structured input, the selected target only when
unbound, typed one-turn overrides, and expected revisions; it has no Native-only
top-level control fields or string `runtimeOptions` map.
Workbench routes Model and Reasoning descriptors through its shared grouped
picker instead of recreating ACP choices from Settings or rendering independent
native selects. Model choices remain target-authoritative; effective Settings
metadata may enrich display names and provider groups only. Missing Reasoning is
omitted, while a read-only descriptor shows only its authoritative value.

Agent mentions and delegation are capability-driven. Workbench offers a
structured Agent mention only when Thread Context declares it accepted for the
selected target. Literal mention text remains ordinary prompt text. Gateway
rejects structured self-delegation or unsupported delegation before delivery.
`thread/action/run` with action kind `interrupt` is a descriptor-gated,
thread-scoped active-turn control, not only a
top-level model-request abort. When a native turn is awaiting a foreground
`spawn_agent` child invocation, interrupting the parent thread must abort that
child invocation and settle the parent turn promptly. Persisted child-agent
edges must close when the child completes, fails, or is interrupted, so
Workbench Status and session reloads do not continue to show stale running
agents.

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
If the selected session is still running, the returned snapshot must also replay
retained live transcript evidence for that session as a transient overlay. This
keeps active tool rows running and preserves output appended while Workbench was
showing another session; the overlay must not create additional persisted
messages.

`thread/start` is a new-source operation, not a session-creation operation. It
clears the current source binding and returns an empty source snapshot with
`thread = null`, without archiving the previously selected thread or inserting a
placeholder session. That empty source snapshot is a detached draft: delayed
events or read-only snapshot refreshes for previously running threads must not
bind it back to an older thread. Only the draft's own first accepted prompt or
shell result may attach the Web view to the newly resolved runtime thread.
When that first accepted prompt is submitted through `turn/start`, Gateway must
materialize and bind a durable thread id before model execution so runtime tools
observe a real current thread. Failed request validation, empty input, and
`thread/start` itself must still leave no placeholder session behind.
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
When a global new-session action starts this detached draft from an app-level
surface such as Settings, Search, or Automations, Workbench must return the main
area to the transcript view so the new draft is immediately visible and usable.
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
Pending permission approvals returned by `thread/read` or `thread/resume` are
scoped interaction requests, not global Web server chrome. The Web shell keeps
optional pending metadata such as thread id, turn id, activity id, owner id, and
lease expiry with each permission request so reconnects can keep still-valid
foreign approvals visible while pruning stale, completed, interrupted, expired,
or wrong-thread requests before snapshot serialization. If
`thread/interaction/respond` rejects a stale, hidden, or already consumed
interaction, Workbench must show a composer-scoped transient error and refresh
the snapshot instead of making the approval buttons appear inert. Successful
responses are accepted exactly once.
Permission and clarify live request events must also update Workbench's current
snapshot immediately. Workbench may then issue a targeted `thread/read` for the
request's thread or activity context, but it must not depend on source-default
`thread/resume` while a draft/source-started turn is still unbound to its
materialized session.
Workbench renders permission requests with once, session, deny, and only the
supported persistent option. It displays the request summary/reason/rule details
as decision context and submits responses using the request's thread/activity
context before falling back to the visible snapshot thread.
Workbench renders clarify requests from their structured question/options
payload instead of raw JSON. It supports the protocol's normal options,
Other/freeform answers, submit, and cancel paths, and routes responses with the
same pending-request context precedence as permission responses.
For peer turns, Gateway also maps Workbench's submitted `model` and
`reasoningEffort` controls to ACP v2 session config options before
`session/prompt` when the peer offers compatible `model` and `effort` select
options. `runtimeOptions.mode` maps to the peer's `mode` option or `mode`
category before the same prompt. Unsupported or unmatched peer options leave
the peer default in place and emit diagnostic events; they do not fail the user
turn.
ACP Agent `usage_update` events are retained as bounded typed Agent facts and
projected into Status observability for the Agent session when they include a
usable `used`/`size` context pair. The Status context total then reflects the
Agent-reported context window rather than the local prompt estimate, and the
session usage summary uses Agent-reported used tokens and cost when no durable
provider accounting exists. The projection is scoped to the immutable binding;
starting a new source draft or switching targets clears it from the visible
observability panel so stale ACP-derived context is not shown for another
thread.

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

`usage/read` returns a display-only historical usage projection for Settings >
Usage. It reads the local state database selected by the running Gateway and
defaults to all persisted history across cwds. It does not use the active
Workbench request scope as a filter, does not contact providers or public model
catalogs, and must not return prompt text, message bodies, tool arguments,
provider payloads, or trace records. The result includes:

- a generated timestamp
- summaries for all history, the last 30 days, and the last 7 days
- token totals for context input, billable input/output, reasoning, cache read,
  cache write, and provider-reported total tokens
- cost totals derived only from persisted accounting columns, with status
  counts and explicit unknown/free/estimated separation
- a cache-read percent defined as `cache_read / (cache_read + billable_input)`
  when the denominator is nonzero
- a local-calendar daily activity series for the last 365 days, suitable for a
  token activity heatmap

The Workbench Settings Usage page renders these summaries independently from
the right Status inspector. It uses compact summary bands for all time, last 30
days, and last 7 days, then a one-year daily token activity heatmap. Heatmap
cells are based on persisted message timestamps in the Gateway host's local
calendar. Empty days remain visible with stable dimensions; nonzero days use a
visibly stepped four-level color scale derived from the nonzero activity
distribution so a single peak day does not flatten ordinary activity into the
same low-contrast shade. Days with unknown cost still contribute token activity
while their cost is counted as unknown.
The page must label costs as local estimates rather than bills and must expose
unknown pricing counts when present.

Creating a new Web thread or selecting an existing history thread rebinds the
current Web source without archiving the previously selected thread. Only an
explicit `source/reset`, archive action, or delete action may remove a thread
from the active history list.

Workbench history is a global session browser. `thread/browser` returns grouped
human-visible sessions from the local state database; the stored session cwd
is used for grouping and for the target scope on resume. Rows are grouped by
cwd, with the current cwd first and all other cwds ordered by latest
session activity. Runtime `source` may appear in diagnostics but must not appear
in history rows/search or decide whether GUI, TUI, ACP, Web, or Desktop
sessions are visible by default.

Each workspace group initially shows sessions updated within the last 7 days,
capped to 20 rows. Current, running, and pinned sessions remain visible even
when older than that default window. Sessions outside the default set are
collapsed behind one older-sessions row per workspace; activating it appends the
next 20 rows for that workspace and preserves the existing group collapse
state. Browsing, expanding, pinning, and selecting rows must not update session
recency.

When Workbench resumes a session from another cwd, it switches the active
scope to that session's stored cwd before accepting more input. The file
tree, `@` completion, diff/status panes, agents, skills, and subsequent turns
refresh against the resumed cwd. Cross-cwd resume must not splice the
old session's transcript into the launch cwd. Archiving, restoring,
renaming, and deleting sessions operate from the same global list and must
respect running/current-session guards across every source.
Starting a new session from another cwd group switches the active browser
scope to that stored cwd and returns an empty source snapshot for that
cwd, without first requiring the user to resume an older session.

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
do not shift horizontally as overflow appears or disappears. The Sessions
browser must not expose horizontal scrolling or swipe panning. Long session
titles truncate with an ellipsis inside the title area; they must not cover or
push out the recent-update timestamp, running indicator, or row actions.

Selecting or creating a Web thread is allowed while another thread is running.
The original thread continues in the background, remains visible in history with
running/queued state, and can be interrupted by selecting it or by thread-scoped
controls. Running threads cannot be archived or deleted until their active turn
finishes or is interrupted.

Workbench receives live activity from every Gateway process that shares the
state database. A TUI-owned turn must appear as running in the session browser,
`thread/read` snapshots, and thread-scoped controls. When Workbench enters a
foreign running session, it subscribes through the Web Gateway's relayed
`gateway/event` stream and updates the visible transcript without requiring the
user to switch away and back. On completion, Workbench still refreshes the
snapshot so committed entries replace the live overlay.

The Web Shell uses Gateway `completion/list` for `/`, `$`, and `@` composer
completion. `$` completion resolves skills, local agents, and ACP capability
mentions; accepted entries keep the visible `$name` text and send structured
Gateway mentions on submission. `@` completion resolves subagent-capable agent
names alongside cwd-scoped file references; accepted agent entries keep the
visible `@agent-name` text and send structured Gateway agent mentions on
submission. Cwd file completion remains scoped to the launched cwd and
must not let the browser read arbitrary host files directly. When the selected
runtime is a peer backend that cannot orchestrate Psychevo agents, `@`
completion omits Psychevo agent candidates but keeps file-reference completion;
manually typed `@agent-name` text remains prompt text.
Long completion lists remain keyboard-operable: ArrowUp/ArrowDown and
Ctrl+P/Ctrl+N update the active option and keep it visible inside the popover
without moving focus out of the composer textarea.

The Web Shell `Search` action opens a center-surface search view. The first
slice searches the current cwd's known session ids, session titles, and
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
local command error. Gateway reads the effective slash alias configuration for
the requested cwd before serving `command/list`, `completion/list`, or
`command/execute`. Alias rows returned to Web carry `source: "custom"` and
`expandsTo`; executing the alias expands to the target line before normal
shared parsing while preserving the original alias line as display text.
The Web/Desktop surface profile is derived by Gateway from the request source
and is not declared by the browser client. `command/list` includes runtime
presentation metadata (`presentationKind`, `destination`, `feedbackAnchor`, and
optional `alternateAction`) for visible commands. Commands hidden because
Workbench cannot represent them are omitted from discovery and slash completion;
GUI `/agents` is one such hidden command because current-session agent selection
is handled by the composer selector and app-level ACP backend configuration
lives in `Capabilities > Agents`. If a hidden command is typed explicitly,
`command/execute` returns `known=true`, `accepted=false`, bounded guidance, and
optional alternate action. Unknown slash-looking input returns `known=false`
with a `passThroughPrompt` host action.
Workbench uses `slash/settings/read` and `slash/settings/update` to manage
profile/global slash aliases and shortcuts from Settings. The methods read and
write only the active profile config, preserve unrelated TOML, return bounded
diagnostics for validation errors, and do not edit project `.psychevo/config.toml`.
`/btw` is a shared `Side chat` command defined by
[250 Thread Navigation](../250-ui-display-model/thread-navigation.md). Web/Desktop
discovery and completion expose it only when the current Workbench surface has
a concrete session id. Executing `/btw` returns a host action that opens a
temporary `Side chat` tab and never adds a command transcript row to the
parent. If the host action includes an inline prompt, Workbench opens the side
tab before submitting the prompt and shows that prompt in the side transcript
through the ordinary thread composer/reconciliation path.

Workbench applies command results by destination rather than by transcript
insertion. Navigation commands switch panels, structured inspection commands
open their domain view such as preview or status, active-turn controls update
local activity state, submit-style slash commands start a normal model turn, and
export commands invoke the host download/share path. Export/share commands honor
the same parsed `-f|--format` and `-i|--include` arguments as TUI after alias
expansion; `/export` path arguments are used only as browser download filename
hints, not host-local write paths. Display-only feedback from commands must not
be persisted as transcript entries. Panel host actions must
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
current session/cwd and are cleared on session switches and new input.
Successful display-only feedback with no follow-up action may auto-dismiss after
a short delay and may be dismissed by clicking outside its panel. Error feedback
and feedback with follow-up actions must remain until explicit dismissal or a
normal transient clear.

Composer model persistence semantics are defined by
[125 Model Config](../125-model-config/spec.md). Workbench implements them
through Gateway model-state RPCs for the active cwd. After Settings > Models
saves the global default model, Workbench refreshes `settings/read.controls`;
active session or cwd composer overrides continue to display instead of the
new global default.

Workbench observes accepted-turn settlement through Gateway live events and
`thread/read` snapshots. Provider/runtime failures after turn acceptance must
settle as a thread-scoped terminal turn status and render as diagnostic/status
projection in the affected thread. `turn/error` is reserved for request-level
fallback or pre-acceptance failures and is not the source of truth for accepted
turn failure. Turn terminal events stop running activity, clear active-turn UI,
and reconcile pending tool/reasoning rows as failed or interrupted.

Interrupt actions are scoped to the current thread or opened child thread.
Workbench may show immediate local interrupting state after sending
the interrupt Thread action, but final UI settlement comes from the same terminal turn
status used by TUI and history reload. Refreshes after interrupt must read the
target thread explicitly; they must not resume an unscoped draft or unrelated
source binding.

Workbench refreshes `observability/read` after `thread/resume`, `thread/read`,
turn completion, undo/redo workspace refresh, and explicit session switches,
including same-cwd resume where the file tree and diff may not otherwise
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
