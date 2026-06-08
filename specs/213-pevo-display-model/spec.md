---
name: 213. pevo Display Model
psychevo_self_edit: deny
---

# 213. pevo Display Model

Define Psychevo's shared transcript projection contract. Transcript fact
ownership is defined by
[030 Transcript State](../030-state-and-data-model/transcript-state.md):
runtime `messages` are the only durable ordinary transcript source. This topic
defines how product surfaces project those facts into shared transcript entries
and how live presentation events are reconciled with committed history.

## Scope

- message-derived transcript entries for prompts, answers, thinking, and tool
  calls/results
- transcript projection, protocol semantics, and committed replacement behavior
- reusable renderable component expectations for transcript surfaces
- separation between model-visible history and display-only UI state

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

Committed history is rebuilt by projecting messages ordered by `session_seq`.
For an assistant message, visible reasoning, visible assistant text, and
tool-call blocks follow the original assistant `content[]` order. A later
`tool_result` message is attached to the matching assistant tool-call block by
`tool_call_id` and carries `resultMessageSeq`, content, error state, status,
metadata, and timestamps.

Runtime may emit live `started`, `updated`, and `completed` observations while
a turn is active. These events are presentation-only. On turn completion the
server sends the committed transcript entries for the completed turn; clients
discard live overlay state for that turn and replace it with entries projected
from durable messages. Client-local optimistic prompt rows are part of the same
live overlay and must be replaced by the committed user entry for the turn.
If a committed slice includes entries that were already loaded from history,
the client must skip those older message sequences instead of rendering them a
second time. During an active turn, ordinary transcript snapshot refreshes must
not be used as the primary live display mechanism. If a snapshot or reconnect
does include message-derived entries for the in-progress turn, the client
reconciler removes any same-turn live overlay blocks whose visible text or tool
signature is already covered by those message-derived entries. A live entry is
retained only for the uncovered blocks that still represent new active
observations; covered prompt, reasoning, assistant text, and tool blocks must
not remain as a second copy merely because another block in the same live entry
is still running. Full snapshot replacement is reserved for reload, resume,
rewind, and session switching.
After such a snapshot is loaded, later live updates for an already
message-derived block use the message-derived entry as the display anchor.
Covered live text/reasoning blocks are dropped; covered live tool updates may
refresh the matching message-derived tool block's transient status/output, but
must not create a second live tool row.

Display-only command output and observational artifacts, including `/diff`,
must not become model context, session export message content, usage/cost
statistics, loop-visible assistant/user messages, or ordinary main transcript
projection.

Model-visible tool results may contain material that also benefits from richer
UI projection. Runtime may parse stable tool-result fields, such as an
`edit.diff` Git patch block, inside the message-derived tool result projection
for rendering. The parsed projection is a UI view; it must not replace or
mutate the model-visible tool result.

Tool-call blocks merge call metadata with later tool-result message metadata.
Projection must preserve stable call identity fields, including arguments,
content index, call index, assistant message sequence, and result message
sequence, so display surfaces can associate terminal updates with the block
created when the tool call first appeared.
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

ACP/WebUI/IM adapters may map transcript entries into client-native update shapes,
but must not require TUI-specific layout fields.

## Related Topics

- [026 Commands](../026-commands/spec.md)
- [030 Transcript State](../030-state-and-data-model/transcript-state.md)
- [211 pevo TUI Rendering](../211-pevo-tui-rendering/spec.md)
- [214 pevo Diff Command](../214-pevo-diff-command/spec.md)
