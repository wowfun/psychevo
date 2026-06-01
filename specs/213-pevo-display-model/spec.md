---
name: 213. pevo Display Model
psychevo_self_edit: deny
---

# 213. pevo Display Model

Define Psychevo's semantic timeline/transcript model. Runtime `messages` remain
the source of truth for model context, exports, compaction, undo, accounting,
and statistics. Timeline records are runtime-owned UI artifacts for TUI,
Gateway, ACP, Web, and future IM projection.

## Scope

- durable semantic timeline items for prompts, answers, thinking, tools,
  status, command results, diffs, local artifacts, permissions, clarify,
  skills, agents, mailbox, and MCP activity
- timeline storage schema and cutover behavior
- reusable renderable component expectations for transcript surfaces
- separation between model-visible history and UI-visible artifacts

Out of scope:

- provider-neutral AI message semantics
- report/export message contents
- client-specific layout details beyond required semantic affordances

## Semantics

Timeline items store semantic content, not terminal-rendered rows. They include
stable item identity, session id, turn id when known, source kind, status,
ordering, title, body text, preview/detail metadata, artifact references, and
typed kind-specific fields. They must not store ratatui `Line`s, terminal ANSI
color, viewport-dependent wrapping, or layout cache rows.

Runtime writes transcript timeline items in semantic message order. For an
assistant message, visible reasoning, visible assistant text, and tool-call
items follow the original assistant `content[]` order. An assistant message may
still use one consolidated assistant-text item, but that item is first inserted
at the first text block position rather than ahead of earlier reasoning or tool
blocks.

Timeline items store the latest or terminal state of an item. Runtime may emit
live `started`, `updated`, and `completed` observations while a turn is active,
but the durable timeline is not a complete event-sourcing log. Restarted
clients rebuild ordinary transcript state from timeline item rows plus runtime
messages; they do not need every transient progress update.

Timeline-only command output and observational artifacts, including `/diff`,
must not become model context, session export message content, usage/cost
statistics, or durable loop-visible assistant/user messages.

Model-visible tool results may contain material that also benefits from richer
UI projection. Runtime may parse stable tool-result fields, such as an
`edit.diff` Git patch block, into semantic timeline items or artifact records
for rendering. The parsed item is a UI artifact; it must not replace or mutate
the model-visible tool result.

Tool timeline rows merge pending tool-call metadata with later execution and
tool-result metadata. Result upserts must preserve stable call identity fields,
including arguments, content index, call index, and message sequence, so display
projections can associate terminal updates with the row created when the tool
call first appeared.

Ordinary tool rows require an explicit typed tool call, execution observation,
or durable tool timeline item. Reasoning or assistant text that merely says the
model is about to run, read, write, search, or create something is not evidence
of tool execution and must not create a primary active tool row.

`write_stdin` is a model-visible tool call but not a primary transcript item
when it targets an existing yielded `exec_command` session. Its output and
completion state are appended to the owning `exec_command` timeline/projection
row by session identity. The binding uses the `write_stdin` call arguments when
the terminal result has a null `session_id`. Unmatched `write_stdin`
observations are diagnostic material rather than ordinary transcript rows.

Reasoning completion observations without text close an existing live Thinking
item; they must not create an empty Thinking item. Completed reasoning timeline
rows with body text are rendered as finished history rows and must not keep
active timers after reload.

Selected skill activation is represented in the typed timeline as a quiet status
item at turn start. Live Gateway events and history snapshots must carry enough
typed information for TUI, Web, ACP, and future IM clients to show the same
skill-loaded notice without consuming raw runtime events.

Assistant answer timeline items carry the metadata needed to render the
turn-level footer: provider, model, finish reason, outcome, usage, elapsed or
reasoning metadata, and accounting when available. Clients use that metadata to
decide whether a completed assistant item is a terminal user-visible answer;
assistant messages that continue into tool calls do not create a turn metadata
footer.

Raw runtime/provider payloads, unclassified observations, and verbose hook or
transport records are debug material. They may be captured in bounded debug
records for diagnostics, but they must not appear in ordinary timeline items or
ordinary Gateway/Web/TUI transcript streams.

## Storage

The state database schema version is `15`. Psychevo does not migrate state
databases at version `14` or lower in this cutover. Opening an old state
database must fail with explicit guidance to run `pevo init --reset-state` or
set `PSYCHEVO_DB` to a new database.

Runtime owns three timeline tables in this slice:

- `timeline_items` stores the latest typed item projection per stable item id.
- `timeline_artifacts` stores bounded preview/detail metadata and local
  artifact references for large outputs, diffs, images, and downloads.
- `timeline_debug_events` stores bounded debug summaries that are hidden from
  ordinary transcript surfaces.

These tables replace the previous `display_blocks` direction. Runtime messages
remain available for non-display consumers; display readers build TUI, Gateway,
Web, ACP, and IM projections from timeline items and may overlay live in-memory
updates while a turn is streaming.

## Rendering Contract

TUI transcript rendering should consume semantic timeline items through reusable
renderable components with stable `desired_height(width)` and `render(area)`
behavior. Component rendering owns wrapping, highlight roles, selection, and
folding. Layout caches cache semantic block keys and measured heights, not
terminal strings.

Gateway exposes typed timeline items in snapshots and typed item lifecycle
events for live updates. Unknown raw runtime events are available only through
debug APIs.

ACP/WebUI/IM adapters may map timeline items into client-native update shapes,
but must not require TUI-specific layout fields.

## Related Topics

- [026 Commands](../026-commands/spec.md)
- [211 pevo TUI Rendering](../211-pevo-tui-rendering/spec.md)
- [214 pevo Diff Command](../214-pevo-diff-command/spec.md)
