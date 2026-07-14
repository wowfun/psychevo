---
name: 250. UI Display Model
psychevo_self_edit: deny
---

# 250. UI Display Model

Define Psychevo's shared transcript projection contract. Transcript fact
ownership is defined by
[030 Transcript State](../030-state-and-data-model/transcript-state.md): Native
history is Psychevo-authoritative, while an outbound ACP Agent that implements
load/resume owns its native history and Gateway stores a product-safe
projection/checkpoint. This topic defines how product surfaces project either
owner into shared transcript entries and reconcile live presentation events
with authoritative history.

## Scope

- message-derived transcript entries for prompts, answers, thinking, and tool
  calls/results
- transcript projection, protocol semantics, and committed replacement behavior
- reusable renderable component expectations for transcript surfaces
- separation between model-visible history and display-only UI state
- shared thread display/navigation behavior for Side chat and
  child-agent thread inspection

Out of scope:

- durable transcript fact ownership, which belongs to
  [030 Transcript State](../030-state-and-data-model/transcript-state.md)
- provider-neutral AI message semantics
- report/export message contents
- generic durable display rows for command results, diffs, or local artifacts
- client-specific layout details beyond required semantic affordances

## Semantics

Transcript entries are semantic projection records, not terminal-rendered rows
or durable facts. Each committed transcript entry corresponds to one runtime
message and contains nested blocks in message `content[]` order. They include
stable entry and block identity, session id, turn id when known, message
sequence, role, status, ordering, title/body/preview/detail text, and typed
metadata. They must not store ratatui `Line`s, terminal ANSI color,
viewport-dependent wrapping, or layout cache rows.

Committed Native history is rebuilt by projecting messages ordered by
`session_seq`. Agent-authoritative ACP history is rebuilt by projecting typed
replay facts in declared order. The owners use the same public entry/block
vocabulary but are never merged as content sources.
For an assistant message, visible reasoning, visible assistant text, and
tool-call blocks follow the original assistant `content[]` order. A later
`tool_result` message is attached to the matching assistant tool-call block by
`tool_call_id` and carries `resultMessageSeq`, content, error state, status,
metadata, and timestamps.

Provider-executed hosted-tool blocks and provider-neutral assistant sources are
message-derived content. They persist and reconcile by stable provider identity
but do not create local tool-result relationships. URL citations preserve text
indices; image sources preserve remote metadata. See
[111 Web Search](../111-web-search/spec.md).

Runtime may emit live `started`, `updated`, and `completed` observations while
a turn is active. These events are presentation-only. On turn completion the
Gateway transcript projection materializes the completed turn from the owning
history source; clients discard live overlay state for that turn and replace it
with those authoritative entries. Client-local optimistic prompt rows are part
of the same live overlay and must be replaced by the owner's user entry for the
turn. A terminal clears retained live snapshots; later recovery loads the
declared history owner rather than inventing a local content copy.

A message-derived committed user entry replaces its optimistic owner as soon as
that entry is observed, including through `entryStarted`, `entryUpdated`, or
`entryCompleted`; the client does not wait for the terminal event to remove the
duplicate prompt. Replacement first matches the same non-empty Thread and turn
identity. Exact normalized text is only a fallback for the most recent detached
optimistic prompt that has not received a turn identity. Text equality never
merges two committed user entries, so separate turns may intentionally repeat
the same prompt.
The canonical runtime event stream, live-preview contract, snapshot recovery,
and delivery diagnostics are defined by [035 Event
Stream](../035-event-stream/spec.md).
Live assistant observations carry the current assistant
segment snapshot. Gateway must not treat them as additive block deltas: when
the current snapshot changes block positions, removed or moved provisional
text, reasoning, and tool blocks must not remain visible beside the current
snapshot.
Committed entries emitted for a turn completion or for an active-turn snapshot
must carry the real turn identity. Assistant entries from the same turn must
also carry the same assistant segment ordinal used by live assistant entries.
The owner invariant is identity-based: a visible assistant segment is owned by
either the committed message projection or the live overlay, never both, and
clients must not infer ownership by comparing rendered Markdown or approximate
text. The Gateway projection is the owner seam: retained live buffers may
hydrate an active turn, but they must not become an independent transcript
owner once a same-turn committed projection exists.
If a committed slice includes entries that were already loaded from history,
the client must skip those older message sequences instead of rendering them a
second time. A completed turn is terminal for presentation state: after the
client applies `turnCompleted` for a turn, later live entry or delta
observations for that same turn must be ignored and must not recreate a
`runtime.stream` overlay beside the committed message-derived entries. A
snapshot refresh is also a terminal presentation barrier when the incoming
snapshot reports no active turn: the client must not inherit the previous
client snapshot's stale `activeTurnId` to retain same-thread live overlay rows.
When a terminal carries no committed slice, every writable surface refreshes
the authoritative Thread snapshot and history before treating its retained
live projection as settled history. The refreshed activity is authoritative;
an idle Thread cannot keep a local running timer or active-turn affordance. A
stale local active-turn id cannot suppress this same-Thread terminal refresh.
During an active turn, ordinary transcript snapshot refreshes must not be used
as the primary live display mechanism. If a snapshot or reconnect does include
message-derived entries for the in-progress turn, the client reconciler removes
any same-turn live overlay segment whose turn and assistant segment identity is
already owned by those message-derived entries. Covered prompt, reasoning,
assistant text, and tool blocks must not remain as a second copy merely because
another block in the same live entry is still running. Full snapshot
replacement is reserved for reload, resume, rewind, and session switching.
When live observations cross a process boundary, the owning Gateway may retain
low-frequency boundary events and coalesced latest-entry snapshots as a
short-lived delivery buffer. That buffer does not change transcript fact
ownership: Native committed messages or ACP Agent replay remain the declared
ordinary history source, and live entries must still be discarded or reconciled
when authoritative entries arrive.
The owning Gateway activity for a Web turn is rooted in the parent transcript
thread. Scoped in-process child-agent live observations keep their own child
thread ids for inspection, but they must not rebind the parent turn's durable
activity thread or active queue alias. An outbound ACP Agent Tool child also
owns a distinct child activity and turn identity; the parent remains active
independently while it waits for the tool result. Child entries and controls use
the child identity, and the child activity is idle before its terminal is
observable. A renderer must never infer child running state from parent
activity. Parent transcript snapshots still depend on root ownership to stamp
committed in-progress parent messages before replaying any remaining overlay.
Workbench and TUI both consume retained boundary events and latest-entry
snapshots for sessions they display. A foreign live observation is applied only
when its thread identity matches the visible session or an explicitly tracked
running session; otherwise it is ignored or left for that session's later
replay. Replaying retained observations must use the same block, turn, and
message identity reconciliation as ordinary streaming events so a
history-derived tool row is refreshed in place instead of duplicated or marked
interrupted while a valid foreign owner is still running.
After such a snapshot is loaded, later live updates for an already
message-derived block use the message-derived entry as the display anchor.
Covered live text/reasoning blocks are dropped; covered live tool updates may
refresh the matching message-derived tool block's transient status/output, but
must not create a second live tool row.
Active snapshot replay follows the same monotonic rule as streaming. A
message-derived tool block that already reached completed, failed, or cancelled
status must not be reactivated by retained live `pending` or `running` overlay.
Retained overlay may only fill missing display fields. It must not replace
existing result/body/title data, and pending message-derived tool blocks must
retain `tool_call_id`, `tool_name`, and `args`/`arguments` so an invocation such
as `exec_command <cmd>` can be rendered without waiting for the final
`tool_result`.
Runtime transcript behavior is tested with the normalized ledger and diagnostics
contract in [035 Event Stream](../035-event-stream/spec.md). Browser and TUI
rendering tests may sample DOM rows or `TranscriptRow`s, but they assert these
semantic display facts before relying on screenshots or terminal frame output.

Display-only command output and observational artifacts, including `/diff`,
must not become model context, session export message content, usage/cost
statistics, loop-visible assistant/user messages, or ordinary main transcript
projection.

Model-visible tool results may contain material that also benefits from richer
UI projection. Runtime may parse stable tool-result fields, such as an
`edit.diff` Git patch block, inside the message-derived tool result projection
for rendering. The parsed projection is a UI view; it must not replace or
mutate the model-visible tool result. Parse failures keep the original
tool-result text available to the UI and must not change transcript semantics.

Tool-call blocks merge call metadata with later tool-result message metadata.
Projection must preserve stable call identity fields, including arguments,
content index, call index, assistant message sequence, and result message
sequence, so display surfaces can associate terminal updates with the block
created when the tool call first appeared.
Within one assistant segment, a positioned tool observation is identified by
its `content_index` and `call_index` before any tool-name fallback is applied.
Different positions remain different calls even when they have the same tool
name, both already carry provider call ids, and their initial argument JSON is
empty or incomplete. A temporary id may migrate to a provider id only within
the same position. Tool-name matching is reserved for observations that carry
no usable position and must never collapse simultaneous same-name calls.
When a tool result carries execution timing, projection normalizes it onto the
tool block as `metadata.elapsed_ms`. Message metadata `elapsed_ms` is
authoritative; otherwise stable result fields such as `elapsed_ms` or
`duration_ms` are used. This keeps completed tool elapsed display consistent
across ordinary completed commands and yielded `exec_command` completions.
Display surfaces keep the normalized timing fact even when it is shorter than
1 second, but tool-row right-side elapsed labels are omitted until the duration
reaches 1 second.
Live projection has the same argument-preservation requirement: if a yielded
`exec_command` result arrives without `args.cmd`, Gateway must recover the
cached arguments from the earlier tool call event before emitting the live
`exec_command` entry or any later `write_stdin`-merged update. Display titles
for yielded exec rows are based on that original command invocation, not on the
polling session id or the bare tool name.
Display surfaces render shell-command rows with the invocation-style title
`exec_command <cmd>`. Workbench may use a single clipped title column for that
combined invocation to avoid duplicating the command across tool-name and
summary columns. The original invocation must remain available in row metadata
or expanded detail.
Committed tool-result projection must not let arbitrary result `display`
strings replace invocation-derived titles for built-in tools. Source-scoped
display titles, such as ACP peer titles that carry `source: "acp_peer"`, may be
promoted into display metadata; ordinary tool result payload fields stay result
detail and must not become explicit tool titles.

Ordinary tool rows require an explicit typed tool call, execution observation,
or message-derived tool-result relationship. Reasoning or assistant text that
merely says the model is about to run, read, write, search, or create something
is not evidence of tool execution and must not create a primary active tool row.

`write_stdin` is a model-visible tool call but not a primary transcript block
when it targets an existing yielded `exec_command` session. Its output and
completion state are appended to the owning `exec_command` tool-call block by
session identity. The binding uses the `write_stdin` call arguments when the
terminal result has a null `session_id`. Unmatched `write_stdin` observations
are diagnostic material rather than ordinary transcript blocks unless they
represent a failed tool call that must be surfaced to explain the turn failure.
Failed `write_stdin` calls that target a known yielded `exec_command` session
remain auxiliary exec-chain diagnostics and must not create standalone primary
transcript rows.

Reasoning completion observations without text close an existing live Thinking
block; they must not create an empty Thinking block. Completed reasoning blocks
with body text are rendered as finished history blocks and must not keep active
timers after reload.

Live transcript projection must preserve semantic order inside a turn without
depending on wall-clock arrival order. Gateway keeps turn-local live projection
state for the active assistant segment and emits ordered blocks for real
reasoning, visible assistant text, and tool progress. If tool execution
observations arrive before the assistant `message_end` that contains the
visible phase text and tool-call content, the live projector still anchors the
assistant text before the owning tool block once that content is known.
Assistant text in a tool-call message remains a text block; only provider/model
reasoning events may create reasoning blocks.
For assistant message snapshots, render order follows the actual
`message.content[]` order in the current snapshot. Provider `content_index` and
`call_index` remain tool-call assembly and identity metadata, but they must not
act as transcript text block identity or override visual ordering for the
current assistant snapshot.

Entry `liveOrder` is turn-local. It may order an optimistic user entry before
assistant/tool streaming from the same non-empty turn, but it must not move a
later turn ahead of visible entries retained from an earlier turn. Across turn
identities, or while either entry is not yet turn-bound, clients use the
timeline before stable identity fallback. Committed `messageSeq` remains the
authoritative cross-turn ordering signal.

Agent-authoritative transcript blocks may carry a positive `phaseOrdinal`.
Phase/item identity, not rendered text, owns replacement and order. The UI
groups a single phase without extra chrome. A turn containing more than one
phase shows one collapsed `Show agent phases` affordance; expansion labels
groups only as `Phase 1`, `Phase 2`, and so on. Adapter protocol phase names and
ids never enter the public display model.

Agent Tool blocks consume the canonical closed status set `pending`,
`running`, `completed`, `failed`, and `cancelled`; terminal materialization
closes every unfinished tool. Plan and Diff blocks are replacement values.
Clearing either removes the block, and empty values never render synthetic
placeholder evidence.

Agent-authoritative history uses the same ordering and replacement semantics as
live projection. Within one assistant segment, reasoning, text, and tool slots
render in replay notification order, including text/tool/text sequences. Plan
history exposes one stable logical block whose content is the latest complete
ACP snapshot; prior pending or running Plan snapshots do not become additional
entries. If the Adapter cannot project replay losslessly, surfaces render the
declared `partial` fidelity and hint rather than presenting an apparently full
transcript. Projectable text without an Agent message identity remains visible;
its internal replay identity is not presented as delivery evidence.

Tool-call assistant text may be marked as an `assistant_phase` live projection
while the turn is active. It is a block-level projection hint inside the live
assistant segment, not a cross-owner dedupe rule. Client reconciliation must use
the turn and assistant segment identity described above rather than comparing
`assistant_phase` text with later Markdown or plain-text output.
Reasoning deltas emitted before `message_end` are live observations for the
active assistant segment. The public runtime `message_end` payload may hide
assistant reasoning, so absence of a reasoning block in `message_end.content[]`
does not prove that no reasoning exists. When assistant `message_end.content[]`
arrives, that content array is authoritative for final assistant text and tool
block kinds/order. Gateway must rebuild those text/tool blocks from that
content array, preserve any previous non-empty live reasoning block for the
segment when the final content omits reasoning, replace it only when final
content contains a reasoning block, and mark the emitted entry metadata with
`projection: "assistant_segment"`, `liveOrder`, monotonic `streamSeq`, and
`authoritativeBlocks: true` so clients replace the prior block set instead of
merging stale provisional blocks. Non-authoritative live updates for the same
segment carry the same ordering metadata with `authoritativeBlocks: false` or
the field omitted. Preserved reasoning must be completed when `message_end`
closes the segment, must keep its original order and body, and must not be
duplicated when final content supplies its own reasoning block.
Every assistant `message_end` closes the current live assistant segment, even
when all blocks in that assistant message are display-hidden, such as a
`write_stdin` poll that is merged into an earlier `exec_command`. Hidden
assistant messages still define model-message boundaries. A later reasoning
delta or assistant update must start a new live segment instead of appending to
the hidden message's previous Thinking block.
Clients must also remove same-turn stale pending-only tool overlay entries
that are superseded by the authoritative segment snapshot, even when an early
tool observation used a provisional fallback id before the final provider
`tool_call_id` was known.

Selected skill activation is represented as prompt/message metadata and may be
shown as a quiet surface notice. It is not a separate durable transcript entry.

Assistant answer entries carry the metadata needed to render the turn-level
footer: provider, model, finish reason, outcome, usage, elapsed or reasoning
metadata, and accounting when available. Clients use that metadata to decide
whether a completed assistant entry is a terminal user-visible answer;
assistant messages that continue into tool calls do not create a turn metadata
footer.

Raw runtime/provider payloads, unclassified observations, and verbose hook or
transport records are diagnostic material. They may be available through live
logs, tests, or explicit developer tooling, but they are not ordinary
transcript facts and must not appear in ordinary transcript entries or
ordinary Gateway/Web/TUI transcript streams.

## Debug And Display-Only Boundaries

Runtime diagnostics are not ordinary transcript projection records. Unknown raw
runtime/provider payloads, unclassified observations, verbose hooks, and
transport records must not appear in ordinary Gateway, Web, TUI, ACP, IM, or
CLI transcript streams. Durable diagnostic sidecars require a domain-specific
spec and retention policy; the display model does not define a generic runtime
debug table.

Command feedback, bottom panes, overlays, `/diff`, completion popovers, and
surface-local status messages are display-only unless a domain spec defines a
separate durable sidecar. They must not be folded into transcript entries as a
generic artifact block.

## Rendering Contract

TUI transcript rendering should consume semantic transcript entries through reusable
renderable components with stable `desired_height(width)` and `render(area)`
behavior. Component rendering owns wrapping, highlight roles, selection, and
folding. Layout caches cache semantic block keys and measured heights, not
terminal strings.

Gateway exposes typed transcript entries in snapshots, committed turn results,
and typed live lifecycle events. Live entry events are overlay records; the
committed entries returned at turn completion are authoritative for that turn's
ordinary transcript projection.

Session-list projection may include a target label and lifecycle descriptors
for `fork` and `delete`. These are display-ready product facts: each descriptor
contains an enabled state and optional unavailable reason. Renderers must not
derive lifecycle availability from provider, model, Runtime Profile id, or ACP
Agent name. Agent-native ids and list cursors are never display-model fields.

ACP/WebUI/IM adapters may map transcript entries into client-native update shapes,
but must not require TUI-specific layout fields.

## Attachments

- [Thread Navigation](thread-navigation.md) defines shared display/navigation
  behavior for Side chat and child-agent thread inspection.
- [Testing](testing.md) defines display-model validation expectations.

## Related Topics

- [026 Commands](../026-commands/spec.md)
- [030 Transcript State](../030-state-and-data-model/transcript-state.md)
- [260 UI Rendering](../260-ui-rendering/spec.md)
- [270 UI Interaction](../270-ui-interaction/spec.md)
- [210 pevo TUI Rendering](../210-pevo-tui/rendering.md)
- [240 pevo Web](../240-pevo-web/spec.md)
- [214 pevo Diff Command](../214-pevo-diff-command/spec.md)
