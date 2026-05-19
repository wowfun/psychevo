---
name: 211. pevo TUI Rendering Evidence Projection
psychevo_self_edit: deny
---

# 211. pevo TUI Rendering Evidence Projection

Define how runtime events become semantic ledger evidence and expandable transcript rows.

## Evidence Projection

TUI renders runtime events into semantic ledger evidence:

- user prompts become unlabeled dark prompt blocks without a left rail
- folded reasoning becomes flat `Thinking` evidence; explicit new paragraphs in
  reasoning content start without label-width indentation
- tool calls become flat tool evidence rows whose visible title starts with
  the actual tool invocation name, for example `read <path>`, `list <path>`,
  `search <query>`, `bash <first command line>`, `write <path>`, or
  `edit <path>`.
- `bash` tool titles must expose the first actual shell command from the tool
  arguments rather than a
  generic `command` placeholder whenever the runtime supplied it. Leading blank
  lines and full-line shell comments, including model-written planning comments
  such as `# Try webcache`, are skipped for title selection so the ledger shows
  the executable command line. Completed tool updates must preserve the command
  title captured from the start event when the end event only contains the
  result.
- before a tool completes, fullscreen TUI may project transient active evidence
  from streaming assistant tool-call blocks, runtime pending tool-call input
  events, and tool-execution start events. Active and completed rows keep the
  same tool-name-first title language; completion updates the same row in place
  and may fill in a more concrete path, query, or command. Active rows should
  not show a redundant body line that says only `running` or `preparing`; the
  spinner/activity marker and right-side elapsed duration carry that state.
- assistant visible output becomes unlabeled answer body text without a left
  rail
- turn-level metadata becomes unlabeled material directly after a visible
  answer, or after a terminal reasoning-only assistant message when no visible
  answer exists, and keeps the metadata left rail

Tool failures remain in their original evidence group and render as failures
instead of being moved into a separate generic error log. Interrupted tool
evidence is distinct from ordinary failure evidence: a tool result with
`outcome: "aborted"` or `error: "aborted"` renders a muted `interrupted` marker
in the existing tool evidence row rather than a red failure body or
`(no output)`. `bash` timeout failures must render an explicit timeout line in
the failed `bash` row even when the command produced partial output.
When the overall turn outcome is `normal`, tool failures are summarized by the
failed tool row and turn metadata, not by an additional red `Error` transcript
row. A red turn-ended error row is reserved for non-normal turn outcomes, so
the TUI must not render contradictory messages such as `turn ended: normal`.
When a non-normal turn includes a terminal reason, the red turn-ended row must
include the reason-specific human message; for model-turn budget exhaustion it
must say the model-turn limit was reached before a final answer and suggest
resuming the session to continue.
User-confirmed interrupted turns show `interrupted` in turn metadata instead of
counting the interrupted tool as `1 failure`.

Active tool evidence is local TUI projection only. Runtime must surface a
named pending tool-call input event as soon as a provider streams the tool name,
before waiting for complete JSON arguments or local execution. While the model
is still producing tool input, the transcript shows a short `preparing` body
with the tool name if arguments are not yet complete, for example `write`,
`read`, or `bash`. Once complete arguments are available, or once the
corresponding `tool_execution_start` arrives, the active title should update to
the concrete path/query/command without inserting a duplicate row. Active tool
rows match primarily by `tool_call_id`; if the id has not arrived yet,
fullscreen TUI uses the assistant tool-call `content_index:call_index` pair
scoped to the current assistant message as a temporary key and migrates it when
the id appears. The same `content_index:call_index` pair may recur in later
assistant messages in a multi-tool turn and must not overwrite prior tool
evidence. Pending active rows that never reach execution because the turn is
interrupted or fails stop their timer and render as static `interrupted`
evidence rather than being persisted as completed history.

When Thinking is visible, fullscreen TUI may show a provisional active tool row
from visible Thinking text that explicitly announces imminent tool use, such as
`Let me write...` or `Let me run...`, because some providers stream long
tool-input generation as reasoning before emitting the structured tool-call
block. This provisional row uses a generic tool-name title such as `write` or
`bash`, never a guessed path or command. Hidden thinking must not
create a provisional row from reasoning text. A concrete assistant tool-call
block, runtime pending tool-call input event, or runtime `tool_execution_start`
must replace the provisional row with the real active row when it arrives; if an
assistant message finishes without a matching tool call, the provisional row is
removed. Once concrete signal arrives, the active tool-name row must be
rendered for at least one frame before later same-turn events can settle it as
completed evidence.
This applies even when local tool execution completes in the same event-drain
tick, such as a 0ms `write` after a long provider-side tool-argument generation
phase.

Transcript folding is row-level only. The renderer must not synthesize
`Thinking` or `Tool calls (N)` section headers. `Thinking` rows and tool
evidence rows are rendered through the same ledger evidence row component and
are individually foldable when they have rendered detail text. Short rows
default open; selecting an open foldable row shows `▾ collapse`, and collapsed
short rows show `▸ details`. Long `bash` command titles stay single-line with
ellipsis in the title row so elapsed time remains visible, but rows with long
command titles can expand to show the complete wrapped command below the title.
Long Thinking bodies and long tool
outputs use the same default collapse threshold: eight logical lines, 200
display tokens, or roughly 1200 display cells. Display-token counting is a
local UI heuristic: ordinary whitespace-delimited spans count as one token, and
long unbroken spans such as table separators, URLs, minified JSON, or CJK runs
are charged in display-cell chunks so they cannot bypass the token threshold.
Line-count previews are still subject to the display-token and display-cell
budgets; if the first visible logical lines would exceed either budget, the row
uses a bounded token/width preview instead of showing all of those lines.
Line-count collapses show `▸ N more lines`; token- or width-only collapses
whose omitted line count is not meaningful show `▸ more output`.

Mouse clicks on expandable rows toggle that row's details. Dragging to select
transcript text must not toggle rows. Transcript-focus `Enter` and `Space`
apply the same toggle to the selected row. Transcript focus selection is a
single-line focus affordance: only the selected row's first visible line uses
the `›` marker, while body and wrapped continuation lines retain their normal
ledger indentation. Mouse text selection remains a separate copy-selection
state and continues to use the semantic selection background. Collapsed rows
contribute their actual rendered height to the transcript scroll model, and
selection movement must walk visible rows.
When fullscreen TUI reconstructs transcript history from persisted messages,
assistant messages whose `finish_reason` is `tool_calls` and whose outcome is
still `normal` must also rehydrate their unmatched tool-call blocks as active
ledger evidence until the matching `tool_result` record is encountered. The
later `tool_result` updates that same row in place rather than appending a
duplicate, so reconnecting or reloading a running session still shows the
active tool row during provider-side or local write gaps. If a persisted
assistant message is already terminally interrupted (`finish_reason=aborted` or
an `aborted`/`failed`/`stopped` outcome), unmatched tool calls from that message
render as static muted `interrupted` evidence with no live timer. History
reload must never turn those aborted tool calls back into active tool rows.
For providers that buffer tool-call input until the end of a long write
argument generation, fullscreen TUI may show a provisional `write` row from
visible assistant preamble text only when that visible text explicitly
announces an imminent write/change action. This fallback is not allowed for
folded reasoning text, must be replaced by the real tool row when a concrete
tool signal arrives, and must be removed if the assistant message finishes
without a write/edit tool call. Repeated message updates for the same visible
preamble must not create additional provisional `write` rows once a concrete
active write/edit row exists. Completion must leave exactly one completed tool
row for the tool call and no orphan active fallback rows.

Tool evidence shows elapsed execution duration on the right side of the tool
title row. Active Thinking rows also show a right-side elapsed value while
reasoning is streaming, but completed Thinking rows do not synthesize a
duration from turn metadata. Active tools refresh elapsed from the local start
instant while the turn is live; completed live rows freeze the larger of the
runtime-supplied `elapsed_ms` and the active ledger duration since the first
concrete tool signal, so a provider-side pending period does not collapse to
`0s` when local execution is instant. Completed
rows must not continue increasing on later redraws. TUI history reload restores
completed tool duration from the tool-result message metadata when available
and does not recompute old completed rows from the current wall clock. Narrow
views preserve the right-side duration first and truncate the title when
needed.
Transcript layout caching must not freeze active tool evidence: rows with a
running local start instant must invalidate cached rendering when their
right-side elapsed label or activity marker changes, while completed rows remain
cache-stable.

Expandable evidence keeps the full stored output available for local inspection
in this TUI process or from persisted message/tool-result material when
available. Expandable title rows use a right-side text affordance instead of
bracket tokens: collapsed rows show `▸ N more lines` when the omitted line
count is known, width-only collapses show `▸ more output`, and expanded rows
show `▾ collapse`. Narrow terminals may shorten those hints, but must not
reintroduce bare `[+]` or `[-]`.

Usage and provider metadata are not transcript content blocks. Provider/model
with an optional resolved variant, elapsed time, failures, debug usage parts,
and allowlisted provider metadata may be projected into turn metadata, but cost
belongs in `/usage` and its `/stats` alias, while context percentage belongs in
`/context` and the bottom status line. Usage and provider metadata must not
appear in sanitized transcript messages, provider replay across incompatible
providers, or `pevo run --format json` by default.

Default metadata projection omits `default` mode and renders elapsed time with
the UI-only compact duration formatter: under 60 seconds as whole seconds
without decimals, for example `12s`, and 60 seconds or more as minutes plus
zero-padded seconds, for example `1m05s` or `2m20s`. The formatter floors
sub-second precision, so `999ms` renders as `0s`. Completed model messages use
the runtime-supplied `elapsed_ms` captured at message completion when
available; fullscreen TUI must not recompute completed elapsed time from later
render or event-drain time, and storage retains millisecond precision.
When runtime resolves an enabled per-turn `reasoning_effort`, assistant message
metadata preserves it as `reasoning_effort`, and TUI renders that value
directly after the model label separated by one space. Missing reasoning effort
and the `none` variant are omitted because they do not produce a provider
request field. Non-default mode is the final metadata item.
Fullscreen TUI history reload restores persisted elapsed time when available
instead of showing only provider/model and response metadata for completed
turns.
Debug projection shows usage parts and an allowlisted provider metadata summary
without `key=value` prefixes and without duplicating `elapsed_ms` or
`reasoning_effort`.
