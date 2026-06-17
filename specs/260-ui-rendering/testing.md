---
name: 260. UI Rendering Testing
psychevo_self_edit: deny
---

# 260. UI Rendering Testing

Define acceptance expectations and validation scenarios for shared UI
rendering invariants.

## Long-Term Acceptance Contract

- Transcript rows render only from typed transcript/display facts, not raw
  runtime/provider records or assistant prose that merely describes intended
  work.
- Tool evidence uses tool-name-first titles and preserves original invocation
  identity across live updates, yielded exec sessions, polling, completion, and
  history reload.
- `exec_command` and associated `write_stdin` observations render as one owning
  command row whenever the stdin call targets a yielded session.
- Agent rows preserve shared display-model identity, remain openable across
  live/reload updates, and never duplicate parent rows for one child thread.
- Display-only command feedback, previews, `/diff`, panels, and debug surfaces
  do not become ordinary transcript entries, model-visible context, exports, or
  usage/accounting records.
- Parsed inline diffs for successful update tool results are display-only,
  default visible, and fall back to ordinary detail when malformed.
- Context usage remains the highest-priority compact observability segment
  before cache, token, and cost details.

## Current Implementation Slice

Automation vocabulary and generic validation boundaries follow
[060 Automation](../060-automation/spec.md).

Shared rendering validation should prefer semantic transcript/display facts
over screenshots when possible. Concrete terminal and browser appearance remain
validated by [210 pevo TUI Testing](../210-pevo-tui/testing.md) and
[240 pevo Web Testing](../240-pevo-web/testing.md).

Manual real-provider validation is opt-in only.

## Scenario Matrix

- Tool rows only render from typed tool/execution/display blocks, never from
  reasoning or assistant prose.
- `exec_command` titles preserve the first actual command line and survive
  start-to-end updates that omit arguments.
- Yielded `exec_command` plus matching `write_stdin` output renders as one exec
  chain across live updates and history reload.
- Empty reasoning completions close existing Thinking rows without creating
  placeholder rows.
- Agent invocation handoff, stale pending blocks, child previews, and
  completion metadata leave exactly one parent Agent row.
- Folding preserves useful head/tail previews without turning large outputs
  into ordinary always-open transcript content.
- Completed successful `edit`, `write`, or `apply_patch` rows with parseable
  Git patch `result.diff` show `Edited ... (+A -D)`, default-open parsed diff
  detail, and no bulky edit input fields in normal detail.
- Malformed diff text, failed update tools, running update tools, and `write`
  results without diff preserve existing raw/structured rendering behavior.
- Display-only command feedback and artifacts remain outside ordinary
  transcript projection and model-visible history.
- Context usage remains visible before lower-priority observability segments
  on compact surfaces.

## Validation Boundaries

- Tests should compare semantic rendering invariants, stable row identity, and
  display-only boundaries rather than private DOM, CSS, or ratatui line
  buffers.
- Visual artifacts are required when a concrete TUI or Web change affects
  rendered appearance.
- Snapshot tests should avoid brittle provider-payload, prompt, or full-output
  comparisons unless the tested surface owns that exact text.
