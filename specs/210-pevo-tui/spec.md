---
name: 210. pevo TUI
psychevo_self_edit: deny
---

Define the first interactive terminal surface for `pevo`.

This topic implements the terminal-specific surface defined by
[075 Design System](../075-design-system/spec.md). It also builds on
[200 pevo CLI](../200-pevo-cli/spec.md) and [026 Commands](../026-commands/spec.md),
and routes live coding-agent turns through the in-process Framework Client.
The Framework remains the execution and persistence authority. For interactive
terminals, `pevo tui` is a fullscreen terminal UI. For non-terminal
stdin/stdout, it keeps the deterministic line-by-line scripted behavior.
TUI visual roles map to `075` `DESIGN.md` roles such as accent, identity,
danger, dim, thinking, surface, and selection. The implementation may keep
ANSI16/ANSI256/truecolor fallbacks and host palette probing, but local color
constants should express those semantic roles rather than a separate TUI theme.

## Scope

- `pevo tui` command spelling, startup behavior, and non-terminal fallback
- persisted TUI-local model, variant, mode, thinking visibility, raw transcript
  visibility, and sidebar visibility
- user-configured slash command aliases and shortcuts loaded from effective
  `config.toml`
- session resume, switching, archiving/deletion, titles, running-session list
  indicators, undo/redo-adjacent session behavior, and history loading
- history-only reload treatment for unfinished tool calls, including
  process-restart orphan rows that must not animate as live work
- model, variant, mode, thinking visibility, raw transcript visibility, local
  stats, context-usage, session observability, and status state surfaces
- explicit scoped default-model writes from the model picker
- responsive foreground interruption and preservation of every visible
  assistant answer emitted during a multi-tool turn
- direct user shell escapes from fullscreen and scripted input, persisted as
  user-provided shell context without exposing `exec_command` as a plan-mode
  model tool
- live exec-session rendering for yielded `exec_command` processes, including
  background output updates and interruption cleanup within the current runtime
  process
- wrap-aware bottom approval panels that preserve all approval choices even
  when long tool/action/grant details wrap across many terminal rows
- shared ownership boundaries for the rendered TUI surface, interaction model,
  sessions, state, and validation
- long-lived process-scoped Framework ownership for Thread identity,
  active-turn queueing, steering, interrupt, permission, clarify, and typed
  timeline projection

Rendering-specific rules live in [Rendering](rendering.md).
Input, slash-command, popup, panel, and selection rules live in
[Interaction](interaction.md). This topic keeps the parent command contract
and cross-cutting TUI state/session behavior.

Out of scope:

- plugins, user-configurable statusline fields, TUI theme configuration, or
  full rich document rendering beyond bounded Markdown projection
- approvals, auth, provider login, or model probing
- structured `@file` references, automatic file-content attachment, custom
  slash commands, or command-template files
- transcript review overlay, remote session publishing, or external editor
  integration; history Edit and user-owned Fork are defined by
  [290 History Editing and Thread Fork](../290-history-editing-and-thread-fork/spec.md)

## Command

`pevo tui [message..]` starts the interactive terminal surface for the selected
working directory.

Accepted first-slice flags are:

- `-C, --cd <path>` selects the working directory. The root form
  `pevo -C <path>` selects the same cwd for the default TUI. The removed
  `--dir` spelling is rejected.
- `-m, --model <provider/model>` selects the model for this TUI process only.
- `--variant <none|minimal|low|medium|high|xhigh|max>` selects the reasoning
  effort for this TUI process only.
- `-s, --session <id>` starts from an explicit session.
- `--new` starts from a new session on the first submitted prompt.
- `--debug` enables debug-only local projections, including usage parts and
  allowlisted provider metadata summaries.
- `--no-skills` disables default and configured skill discovery.
- `--skill <name-or-path>` is repeatable and explicitly adds a skill by name or
  path.

When positional message text is supplied, TUI submits it immediately and then
continues the prompt loop. If that text begins with `!` after leading
whitespace, it is processed as a user shell escape instead of a provider
prompt. In non-terminal stdin, each input line is processed as one prompt,
slash command, or user shell escape. Non-terminal stdin is not appended to the
positional prompt, and the fullscreen alternate screen is not used.

`pevo tui` requires initialized `PSYCHEVO_HOME`, because TUI-local state lives
under that home. `PSYCHEVO_CONFIG` and `PSYCHEVO_DB` may still override provider
configuration and SQLite state path, but they do not bypass the home
initialization requirement.

TUI reads slash command customization from the effective `config.toml` using
the same global/project merge and explicit `PSYCHEVO_CONFIG` behavior as
provider configuration. The optional shape is:

```toml
[tui]
leader_key = "ctrl+x"
leader_timeout_ms = 2000

[tui.slash_aliases]
"/model" = ["/m"]
"/sessions" = ["/s"]
"/export -f json -i messages" = ["/xj"]

[tui.slash_keybinds]
"/model" = "<leader>m"
"/status" = "ctrl+s"
"/variant high" = "<leader>h"
"/copy" = ["<leader>y", "ctrl+shift+c"]
"/usage" = "none"
"/export -f json -i messages" = "<leader>x"
```

This configuration is local UI behavior only: it does not change CLI command
spelling, persisted session content, provider payloads, or `tui-state.json`.
`slash_aliases` keys and `slash_keybinds` keys are concrete slash input lines
validated by the normal slash parser. Alias input expands to that concrete
slash input before parsing; if the alias is followed by additional text, that
text is appended to the configured target line and then parsed. Invalid alias
or keybinding configuration rejects TUI startup with a bounded configuration
error. Configured aliases participate in slash menu completion as alias rows,
and configured concrete slash lines appear in `/help` `Custom commands`.
Workbench Settings may write the profile/global form of this same configuration
through typed Gateway slash-settings methods. TUI still reads the effective
merged configuration at startup, so project-local overrides can affect TUI and
Gateway command discovery even though the GUI v1 Settings page edits only the
active profile config.

## Framework Client Ownership

Fullscreen TUI owns one long-lived in-process `psychevo::Client` for the
process. Its
source lifetime is `Process`, so the process can remember the current thread
without creating durable source bindings. Normal prompts, queued prompts,
steer, interrupt, permission responses, clarify responses, source reset, and
thread switching go through Client APIs.

The foreground agent task retains its Application-issued `TurnHandle` as the
single owner of steer, interrupt, clarify, and contextual shell-result
injection. A foreground user-shell task retains only its own runtime control;
the TUI does not pass that control through `TurnRequest`. Terminal rendering
subscribes to the handle's bounded `TurnEvent` stream. The fullscreen running
view, non-terminal `TurnPrinter`, and journey probe consume that same typed
event directly; they must not convert it back through the internal
`RunStreamEvent` model or a Gateway projection. Typed message, reasoning, Tool,
interaction, scope, warning, and lifecycle variants retain their meaning
through the terminal presentation seam. `TurnEvent::Runtime` retains only
runtime detail that has no stronger typed variant, including usage or
allowlisted provider metadata needed by the existing presentation. The TUI also
supplies its profile-local workspace snapshot root so the existing `/undo` and
`/redo` contract remains available. Queue and lifecycle authority, including
accepted, started, and terminal Turn state, remains in Application.
On `ResyncRequired`, both fullscreen and non-fullscreen presentation follow the
projection-invalidation and authoritative reload contract in
[035 Event Stream](../035-event-stream/spec.md); neither settles from its
pre-gap Tool or message state.

When session switching transfers an accepted Turn to auxiliary ownership, its
TUI approval receiver moves with that same Turn. The event loop multiplexes
foreground and auxiliary approval receivers into one visible decision queue;
starting another foreground Turn cannot close or replace a background Turn's
approval path. That queue preserves the monotonic request and cancellation
order established across every foreground and auxiliary receiver; receiver
scan order must not reprioritize a later decision. Permission cancellation
removes the matching queued or visible decision, and a closed decision response
is reaped even if its cancellation event races with auxiliary-task completion;
neither path may leave a stale approval panel. A session or side-surface switch
preserves an already visible permission decision panel until it is resolved or
cancelled; it may clear other session-local panels but must never hide a live
response sender.

An explicit session switch validates the destination Thread, resumability, and
canonical cwd before detaching a starting or running foreground Turn. If any
destination validation fails, the current session, foreground owner, queued
input, optimistic rows, and cwd remain unchanged; switching is not a partial
mutation that requires best-effort rollback.

When a local draft is submitted, TUI binds `current_session` to the selected or
newly created Framework Thread before installing its foreground event stream.
The first assistant event therefore belongs to the visible session immediately;
it is never diverted into the background-session backlog while the same Turn is
already displayed as the foreground running task.

Thread discovery, resume or creation, and Turn admission run in one thin
background `StartingTurn` task. Submission paints the optimistic user row and
returns control to the terminal event loop immediately; redraw, typing, and
paste handling remain responsive while admission is pending. TUI installs the
Application-issued control and typed `TurnEvent` stream only after admission
succeeds. If admission fails or its task panics, fullscreen stays open, the
uncommitted optimistic row is removed, the submitted text and attachments are
restored ahead of any newer composer draft, and the error is shown in the UI.
Resolving a remembered Thread distinguishes an authoritative missing result
from every other Client or State error; only a missing Thread may fall back to
the latest eligible Thread or a new Thread. A new draft uses atomic
`Client::start_thread_with_turn` admission, with a preallocated Thread identity
when the approval handler needs it, so rejected or cancelled pre-acceptance
work cannot leave an empty Thread. `/mission` prepares its registration before
submission and includes the team and mission rows in the same durable Turn
admission transaction, before the accepted actor can enter its execution lane.
The registration binds only to that admission's authoritative Thread identity;
it never creates or rebinds a parent Thread in a parallel preflight path. If a
foreground Turn is starting or running, the mission stays only in the TUI input
queue until its own next Turn admission; it is not pre-registered and is not
converted into a steer. Inputs queued behind a starting Turn use that admission's
private owner identity, never an unscoped `None` wildcard. Successful admission
rebinds them to its authoritative Thread. Failure or foreground cancellation
restores only inputs owned by that admission; another Thread's queued inputs or
steers stay attached to that Thread. Cancelling or switching away removes that
owner so a queued mission cannot be admitted on a different Thread. `/compact`
submitted during a new Thread's `StartingTurn` is queued against that private
owner and follows the same rebind or removal rule instead of being rejected for
lack of an authoritative Thread identity.

A contextual shell escape submitted after Turn acceptance but before its
`Started` event is owned by that exact Turn. Session switching transfers the
pending command with the auxiliary Turn owner; a different foreground Turn's
`Started` event cannot launch it. Interrupt and teardown similarly clear or
settle only the commands attached to the affected owner.

`StartingTurn` is owned only by the foreground session. Esc or Ctrl+C removes
it from the foreground immediately and restores its text and attachments;
explicit session changes, including `/new`, Agent-panel Run, direct session
selection, and entering or leaving `/btw`, remove it without restoring the old
draft into the new composer. Both paths transfer the admission task to one
retained asynchronous cleanup owner instead of aborting or dropping its caller
future. That owner signals Application's phase-aware admission cancellation:
pending Adapter preparation rejects promptly, while a raced accepted Turn is
interrupted and awaited to terminal settlement. Finished cleanup owners are
reaped without blocking input, and fullscreen teardown joins every retained
owner so neither pending preparation nor an accepted Application actor is
orphaned.

Fullscreen teardown signals every foreground and auxiliary Agent or Shell
control, releases visible and queued permission decisions, and joins every task
owner before returning. A background Turn created by session switching cannot
outlive or indefinitely block TUI exit.

Returning from `/btw` restores the parent surface before deleting the temporary
side Thread. The deletion remains owned by a retained asynchronous cleanup task,
finished tasks are reaped without blocking input, and fullscreen teardown joins
all retained side deletions.

Fullscreen and scripted user-shell presentation consumes the Framework's typed
Shell command, control, events, and result directly. The terminal presentation
seam maps Shell start, completion, and warning events to the existing user-shell
ledger or plain output without fabricating general Turn stream events. Session
switching moves the same running Shell task, typed event receiver, and interrupt
control to background ownership. When an auxiliary Shell command contributes
context to the active local Turn, TUI supplies that `TurnHandle` once through
the Shell request's injection intent and does not perform a second manual
injection after completion.

Clarify answers and cancellation for the TUI's own foreground Turn are submitted
through that Application-issued running Turn control. Gateway clarify
submission is only a fallback for a foreign Gateway activity projected into the
current TUI session; the TUI must not try to rediscover its local Framework
Turn through a Gateway source selector.

Launching a background Agent from the Agent panel uses the Framework Client's
standalone Agent-task use case. The TUI supplies intent and consumes the returned
parent Thread and Agent identities; it does not construct runtime options or
pass Framework state/supervision handles back into Framework.

Editing an unsent steer updates the exact pending Control input. Only an
`UnknownInput` result means the old steer has already left Control and may be
resubmitted as a new input. Count, byte, closed-input, and validation failures
keep the editor and old preview intact and report the error; they never delete
the old preview or submit a duplicate instruction.

The TUI slash parser remains local UI behavior, but slash command effects must
map to typed Client or interface-neutral Framework helpers. TUI must not add a
generic `slash/exec` transport method, construct private execution options,
call the Native run loop directly, or shell out to `pevo run` for normal
prompting or control. The TUI remains an internal module of `psychevo-cli`; it
is not a separate crate or package.

File completion owns one `FileSearchWorker` for the fullscreen state lifetime.
Input replaces a single latest-request slot; an unstarted older request is
discarded rather than receiving its own thread. The running scan checks stop,
generation, and replacement state for every filesystem entry. It retains only
the best eight candidates in a fixed-size heap and sorts those eight for
display. Fullscreen teardown signals stop and joins the worker. Rapid input
therefore has one worker thread, bounded retained results, and no detached
search work after the UI ends.

## Cross-Surface Journey Profiling

Fullscreen TUI participates in the deterministic TUI-versus-Workbench journey
profile through its real terminal event loop and its process-local Client.
The comparable warm send path is `input_ready -> send_committed ->
runtime_request_dispatched -> first_output_visible -> turn_settled`. TUI has no
equivalent of Workbench `draft_context_ready`: an unbound TUI draft is local
state and its durable Session is created lazily after submission.

The profile records `send_feedback_surface_committed` independently from model
output. That mark is the first completed terminal draw after the optimistic
user row, running state, and elapsed timer have been installed. First output is
the first completed draw containing non-empty assistant text. Settlement requires
the authoritative completion event to be applied, the foreground task to be
joined, running state to be cleared, Composer focus to be restored, and the
resulting draw to complete. These surface-commit boundaries align with
Workbench DOM commit; neither claims to observe physical pixels.

An internal, opt-in TUI probe may write content-free JSONL observations for
startup, input, foreground Framework execution-event receipt/application,
authoritative Turn task completion, queue depth, frame paint, and settlement.
The probe must observe the same typed `TurnEvent` path and completed
`TurnHandle::wait` task that the running view and printer actually consume; it
must not require or fabricate a public Gateway Turn lifecycle for a
Client-owned prompt. It must be inert unless an explicit test artifact path is
supplied, must use a monotonic process clock, and must never write prompt text,
response text, tokens, tool arguments, credentials, or provider request bodies.
This probe is diagnostics only and does not extend the public Framework or
Gateway protocol or the persisted transcript.

## Session Observability

TUI status and usage surfaces consume the shared runtime session observability
projection defined by [006 Context Assembly](../006-context-assembly/spec.md).
The projection is display-only: it never becomes a transcript row, copied
history, prompt text, or model-visible context.

When `load_current_session_history` rebuilds a resumed session, it restores the
session-level usage/cache/cost summary from persisted visible message
accounting and keeps latest-turn context pressure separate. The bottom status
line renders a compact, width-aware sequence with context first, then
cache-read percent, visible-branch session tokens, and cost only when space
permits. Partial totals render as lower bounds and unavailable totals do not
render as exact zero.

`/usage` shows a current-session summary above the existing cwd/global
stats. The session summary must respect the same session and revert visibility
boundaries as history reload, distinguish reported, derived, partial, and
unavailable token totals, and avoid rendering raw prompt, message, tool
argument, or provider request text.

## Attachments

- [Sessions](sessions.md) defines session resume, switching, stable activity
  ordering, history, titles, archive/delete, and undo/redo-adjacent session
  behavior.
- [State and Models](state-and-models.md) defines TUI-local state, model
  selection, catalog fetching, variants, and runtime modes.
- [Rendering](rendering.md) defines terminal-specific layout and rendering
  projection.
- [Interaction](interaction.md) defines terminal-specific key handling, slash
  panes, popups, and local selection behavior.
- [Testing](testing.md) defines deterministic acceptance coverage and validation expectations.

## Related Topics

- [Rendering](rendering.md) defines ledger layout, evidence projection,
  rendering rules, and visual diagnostics.
- [Interaction](interaction.md) defines key handling, slash commands, file
  completion, user shell escapes, panels, and local text selection.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines the product CLI surface.
- [080 Framework and SDK](../080-sdk/spec.md) defines the in-process Client
  used by the TUI.
- [026 Commands](../026-commands/spec.md) defines shared command contract
  conventions.
- [200 pevo run](../200-pevo-cli/pevo-run.md) defines non-interactive live run.
- [003 AI Protocol](../003-ai-protocol/spec.md) defines provider message
  translation boundaries.
- [120 Provider Registry](../120-provider-registry/spec.md) defines
  provider/model resolution.
- [031 SQLite Persistence](../031-storage-and-persistence/sqlite-persistence.md)
  defines session and message persistence.
- [055 Skills](../055-skills/spec.md) defines skill discovery, model visibility,
  tools, and lifecycle behavior.
- [051 Agents](../051-agents/spec.md) defines agent definition discovery.
- [051 Subagents](../051-agents/subagents.md) defines subagent run control semantics.
