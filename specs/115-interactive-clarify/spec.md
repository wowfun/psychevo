---
name: 115. Interactive Clarify Tool
psychevo_self_edit: deny
---

Define the first interactive user-input tool for root coding sessions.

## Scope

- model-visible `clarify` tool behavior
- runtime request/response control contract for interactive clients
- fullscreen TUI behavior for answering clarify requests
- tool availability boundaries for modes, agents, and non-interactive callers
- observable timeout, cancellation, cleanup, and late-response behavior

Out of scope:
- secret or password prompts
- using clarify as permission approval
- `pevo run` terminal prompts
- exposing clarify from subagents or headless API callers by default

## Tool Contract

`clarify` asks the user for one to three short questions and waits for a
response. The visible tool name is `clarify`, and its payload keeps the
structured multi-question/options shape while avoiding redundant model-authored
metadata:

- arguments contain `questions`
- each question has `question` and `options`
- each option has `label` and `description`
- tool results contain an `answers` array in question order

Each request must contain 1-3 questions. Each question must be non-empty. Each
question must contain 2-3 non-empty options. The client always offers an
additional Other/freeform path. When the user chooses Other, the answer value is
the typed text, not the literal Other label. When the user selects a normal
option and adds a note, the note is appended as an answer entry prefixed with
`user_note: `.

The schema does not carry a structured recommended-option flag in v1. When the
model has a recommended choice, it should put that choice first and include
`(Recommended)` in the option label. The TUI should preserve that marker and
style it as part of the option label, not bury it in the description.

`clarify` is a sequential tool. It must not be used to approve dangerous
commands, file writes, or permission escalation; those remain owned by the
permission approval flow defined by [041 Permissions](../041-permissions/spec.md).

## Availability

Plan and Default modes may expose `clarify`, but only for the root session and
only when the caller has explicitly enabled interactive clarify support.
Fullscreen TUI enables it in this slice. Headless runtime callers and `pevo run`
do not expose it by default.

Agent invocation policy may allow or deny `clarify`, but policy cannot override the
hard constraints: root session only and interactive support enabled. Subagents
must not receive `clarify`.

If clarify is explicitly enabled without a working answer path, calling the tool
must return an unavailable tool error instead of hanging indefinitely.

## Runtime Control

Runtime emits typed clarify request events and waits on a pending response keyed
by the tool call id. A control handle submits a single result for the call:
answered with a response payload, or cancelled.

Runtime owns the default wait timeout of 10 minutes. Answered requests return a
JSON result:

```json
{"answers":[{"answers":["selected label","user_note: optional note"]}]}
```

Cancellation, timeout, invalid arguments, and unavailable response channels are
reported as tool errors visible to the model.

Runtime emits a typed resolved/cleanup event when a pending clarify request is
answered, cancelled, times out, or is cleared because the turn ended. Late
answers are no-ops and are reported back to the control caller as not accepted.

## TUI Behavior

Fullscreen TUI displays clarify requests in a bottom-panel overlay. Multiple
questions are answered step by step. The overlay may temporarily take over an
existing bottom panel and should restore it after answer or cancellation when
practical.

The TUI panel title is progress-only, for example
`Question 1/3 (3 unanswered)`. It does not repeat an extra question header or
the tool name. The overlay should size itself to the clarify content instead of
using a tall generic bottom-panel height that leaves large empty gaps.

The TUI supports Up/Down selection movement, Left/Right question navigation
while focus is on the option list, quick numeric selection, Enter to confirm or
advance, Tab to edit a normal option note or the selected Other/custom answer,
and Esc to cancel. Clicking a normal option submits that answer and advances to
the next unanswered question, matching Enter on that row. Clicking Other selects
it and enters inline custom-answer editing.

Other/freeform text is entered inline on the Other row; selecting Other must not
switch to a separate page. Notes are also entered inline while the option list
remains visible. Inline note and custom-answer fields must show a cursor and
support editing in the middle of the text with cursor movement, insertion,
backspace, and delete.
Background-session requests do not steal focus; they surface as pending input
status until the user switches to that session.

Completed requests are rendered in history as ordinary Status transcript
notices with a summary like `Questions N/N answered`; question, `answer:`, and
`note:` lines use the shared Status detail indentation rather than a dedicated
clarify result-cell style. Cancelled or timed-out requests use the same Status
notice shape with `Questions 0/N answered` and unanswered question lines
instead of transient status rows. Questions and answers are recorded in session
history and exports.
The deterministic TUI VHS capture should include clarify panel, inline Other
editing, and answered-result screenshots so visual changes to this flow are
covered by the local regression artifact set.
When a session export reconstructs the last provider request, a prompt whose
persisted `effective_tools` contains `clarify` must include the `clarify`
declaration in the reconstructed provider tool schema. Reconstruction should use
the same tool-surface declaration assembly as live runs, with inert bindings
where execution-only responders are unavailable.

## Attachments

- [Testing](testing.md) defines acceptance scenarios and validation expectations.

## Related Topics

- [002 Agent Execution](../002-agent-execution/spec.md) defines tool execution
  sequencing and tool-result messages.
- [007 Tool Surface](../007-tool-surface/spec.md) defines model-visible tool
  declaration snapshots and execution bindings.
- [110 Coding Core Tools](../110-coding-core-tools/spec.md) defines the core
  coding toolset that clarify augments but does not join.
- [041 Permissions](../041-permissions/spec.md) owns permission approval.
- [210 Pevo TUI](../210-pevo-tui/spec.md) owns fullscreen TUI behavior.
