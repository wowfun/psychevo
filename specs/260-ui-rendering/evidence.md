---
name: 260. UI Rendering Evidence
psychevo_self_edit: deny
---

# 260. UI Rendering Evidence

Define shared evidence-row rendering rules for transcript blocks, live Gateway
observations, and display-only artifacts.

## Evidence Projection

UI surfaces render transcript blocks and live observations into semantic
evidence rows:

- user prompts become unlabeled prompt blocks without generic role labels
- visible reasoning becomes `Thinking` evidence
- assistant visible output becomes unlabeled answer body text
- tool calls become flat tool evidence rows whose visible title starts with
  the actual invocation name, such as `read <path>`,
  `exec_command <first command line>`, `write <path>`, `edit <path>`, or
  `web_fetch <url>`
- turn-level metadata renders only after a terminal visible answer,
  terminal reasoning-only assistant message, or terminal failure summary
- status and command-feedback rows are display-only unless their domain spec
  defines a separate durable sidecar

Built-in and extension tools use the same tool-name-first title shape. Unknown
tools fall back to `tool_name <primary argument>` or `tool_name`; surfaces must
not render a separate generic `Tool <name>` label when a better invocation
title is available.

Tool display metadata is UI-only. Runtime or Gateway may attach a display
snapshot to pending and execution observations so surfaces can choose title,
summary, and detail fields without hardcoding every tool name. That metadata
must not enter provider-facing tool declarations, system prompts, or
model-visible tool results.

Successful core `read` rows render with the invocation title `read <path>` and
no trailing result summary. Their expanded transcript detail is the file
content itself; read-result metadata such as `path`, `file_size`,
`output_bytes`, pagination fields, `hint`, and `similar_files` stays out of the
ordinary row. Failed or missing reads still show the error reason in the tool
detail.

Completed successful update-category tools may render a richer diff view when
their model-visible result contains a parseable Git patch field such as
`result.diff`. The row title uses the edited-path summary, for example
`Edited <path> (+A -D)` for one file or `Edited N files (+A -D)` for multiple
files. These rows default open so the change is visible without an additional
click, while still keeping a collapsed summary available. The expanded content
for this path is the rendered diff itself; ordinary Input/Result metadata and
section labels such as `Diff` stay out of the transcript row. The parsed diff is
a display-only view over the original result: malformed diffs, failed tools,
running tools, or update tools without a diff use the ordinary raw/structured
detail path.

When a rendered diff is available, edit input fields such as `old_string`,
`new_string`, `patch`, and `content` and result metadata such as `status` remain
out of normal transcript detail. The model-visible tool result remains
unchanged and raw diagnostic payloads remain limited to explicit debug
surfaces.

## Active And Completed Tool Rows

Before a tool completes, surfaces may project transient active evidence from
streaming assistant tool-call blocks, pending tool-call input observations, and
tool-execution start observations. Active and completed rows keep the same
tool-name-first title language; completion updates the same row in place.
Active rows should not show a redundant body line that says only `running` or
`preparing`; the activity marker and elapsed duration carry that state.

Rows match primarily by `tool_call_id`. If the id has not arrived yet, the
projector may use an assistant-message-scoped stream position as a temporary
key and migrate it when the id appears. The same stream position may recur in
later assistant messages in a multi-tool turn and must not overwrite prior
tool evidence. Pending active rows that never reach execution because the turn
is interrupted or fails settle as static interrupted evidence rather than as
completed history.

Reasoning or assistant prose that merely says the model is about to read,
write, run, search, or create something is not enough to create an ordinary
tool row. A surface may show a provisional local affordance only when that
surface explicitly owns such presentation; a concrete assistant tool-call
block, runtime pending tool-call input event, or runtime `tool_execution_start`
must replace the provisional affordance when it arrives.

## Exec Sessions And Stdin

`exec_command` titles expose the first actual shell command from tool
arguments whenever runtime supplied it. Leading blank lines and full-line shell
comments are skipped for title selection. Completed tool updates preserve the
command title captured from the start event when the end event omits arguments.

A yielded `exec_command` is not complete merely because the model-visible tool
invocation returned. When runtime emits `exec_session_yielded`, the original
row remains active, keeps its activity marker, and continues showing elapsed
time from the original start. Background output appends to that same row.
`exec_session_finished` freezes elapsed time and settles the row as completed
or interrupted.

`write_stdin` calls targeting an associated yielded session do not create
primary transcript rows for empty polls. Their model-visible results are still
used for provider context and persisted history, while visible output comes
from the owning `exec_command` row. Non-empty stdin writes may render as
compact terminal interaction evidence, but still remain associated with the
owning exec chain.

History reload rebuilds yielded exec chains from persisted messages. A root
`exec_command` result with a non-null `session_id` and null `exit_code` starts
an exec session chain. Later `write_stdin` results with the same session id are
merged into that root row in chunk order. If no final chunk is present, the row
renders as last-seen-running rather than pretending a current OS process is
still attached.

## Folding And Failure

Thinking, tool, and Agent evidence rows share row-level folding behavior.
Short rows default open and may collapse to title-only. Long body rows may use
a middle-folded preview, full body, and title-only state. Folding is a safety
valve after the presenter has selected a visible body; tools that return large
content should display a compact summary by default and keep the returned
content available only as expandable detail.

Failures remain in their original evidence group. Interrupted evidence is
distinct from ordinary failure evidence: aborted or interrupted tool results
render a muted `interrupted` marker in the existing row rather than moving to a
separate generic error log. `exec_command` timeouts render an explicit timeout
line in the failed command row even when partial output exists.

When the overall turn outcome is `normal`, tool failures are summarized by the
failed tool row and turn metadata, not by an additional red error transcript
row. A red terminal error row is reserved for non-normal turn outcomes.

## Related Topics

- [Spec](spec.md) defines the parent rendering contract.
- [250 UI Display Model](../250-ui-display-model/spec.md) defines transcript
  entries, live overlay reconciliation, and display-only boundaries.
- [270 UI Interaction](../270-ui-interaction/spec.md) defines command feedback
  and active-turn control interactions.
