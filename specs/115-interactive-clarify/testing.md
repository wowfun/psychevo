---
name: 115. Interactive Clarify Tool Testing
psychevo_self_edit: deny
---

# 115. Interactive Clarify Tool Testing

Define deterministic acceptance coverage for the `clarify` tool contract,
runtime control path, fullscreen TUI answering flow, and persisted/exported
evidence.

Automation vocabulary and generic validation boundaries follow
[060 Automation](../060-automation/spec.md).

## Long-Term Acceptance Contract

- `clarify` tool declarations expose one to three questions, two to three
  options per question, option labels and descriptions, and no model-authored
  question ids.
- Valid tool calls return answers in question order. Other/freeform answers use
  the typed text, and notes on normal options appear as `user_note: ` entries.
- Invalid arguments, unavailable response channels, cancellation, timeout, and
  turn-finished cleanup return model-visible tool errors.
- `clarify` is sequential and cannot be used as a permission approval path.
- Plan and Default modes may expose `clarify` only for root sessions whose
  caller enabled an interactive answer path.
- Subagents, headless callers, and `pevo run` do not receive `clarify` by
  default.
- Runtime emits typed request and resolved/cleanup events for every pending
  clarify call. Late answers are rejected without changing the completed tool
  result.
- Fullscreen TUI lets the user answer all questions through the bottom-panel
  overlay, including inline Other text and inline notes.
- Clarify requests from background sessions do not steal focus.
- Completed, cancelled, and timed-out requests render in transcript history as
  shared Status notices with the question and answer detail.
- Session export and last-provider-request reconstruction include the
  `clarify` declaration when persisted `effective_tools` included it.

## Deterministic Tests

Required runtime coverage:

- Tool schema and declaration snapshot shape for Plan and Default surfaces.
- Agent invocation policy allow/deny behavior without overriding root-session and
  interactive-support constraints.
- Argument validation for empty questions, empty options, too few or too many
  questions, and too few or too many options.
- Successful answer round trip through `RunControlHandle`, including answer
  ordering, Other/freeform text, notes, and resolved event emission.
- Cancellation, timeout, response-channel closure, invalid arguments,
  unavailable control path, abort, and late-response rejection.
- Tool surface reconstruction for `last-provider-request` with inert execution
  bindings when only declarations are needed.

Required fullscreen TUI coverage:

- Opening a clarify request displays a bottom-panel overlay sized to the
  question content, with progress-only title text and no repeated tool name.
- Up/Down, Left/Right, numeric selection, Enter, Tab, Esc, mouse option clicks,
  and mouse Other selection behave as specified.
- Inline Other answers and normal-option notes support cursor movement,
  insertion, backspace, delete, and middle-of-text edits while the options
  remain visible.
- Answering advances through multiple questions and submits exactly one result
  for the call id.
- Cancellation restores any previous bottom panel when practical and emits the
  cancelled result shape.
- Background-session clarify requests surface as pending input status until the
  user switches to that session.
- Transcript rendering covers answered, cancelled, timed-out, and history-load
  cases using shared Status detail indentation.
- Snapshot fixtures cover the question panel, Other inline editing, note inline
  editing, answered result, and declined result.

## Validation

Relevant narrow validation:

- `cargo test -p psychevo-runtime`
- `cargo test -p psychevo-cli`

Broad deterministic validation:

- `scripts/validate.sh broad`

VHS capture is required only for fullscreen TUI visual changes that affect the
clarify panel or result rendering:

- `scripts/pevo-tui-capture.sh demo`

VHS uses a deterministic local mock provider and isolated local state. It
remains outside broad validation and must not require live provider credentials.

## Validation Boundaries

- Tests should compare structured events, stable JSON result fields, and
  rendered Status/overlay invariants rather than provider wire payloads or full
  prompt text.
- Clarify tests must not use permission approval, secret prompts, real provider
  calls, or persistent user configuration.
- Visual snapshots may assert stable text and style roles. VHS screenshots are
  diagnostic artifacts, not checked-in pixel goldens.

## Related Topics

- [115 Interactive Clarify Tool](spec.md) defines the functional contract.
- [210 pevo TUI Testing](../210-pevo-tui/testing.md) covers parent TUI/runtime
  integration.
- [211 pevo TUI Rendering Testing](../211-pevo-tui-rendering/testing.md) owns
  shared rendering and visual-regression rules.
- [212 pevo TUI Interaction Testing](../212-pevo-tui-interaction/testing.md)
  owns shared input and bottom-panel interaction coverage.
