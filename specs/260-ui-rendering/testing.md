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

CI/CD vocabulary and generic validation boundaries follow
[065 CI/CD](../065-ci-cd/spec.md).

Shared rendering validation should prefer semantic transcript/display facts
over screenshots when possible. Concrete terminal and browser appearance remain
validated by [210 pevo TUI Testing](../210-pevo-tui/testing.md) and
[240 pevo Web Testing](../240-pevo-web/testing.md).

Manual real-provider validation is opt-in only.

## Runtime Transcript Oracle

Transcript runtime tests use normalized semantic ledgers as the primary oracle.
Each sampled Gateway, Web, TUI, or Workbench checkpoint should capture
`turnId`, `entryId`, `blockId`, `source`, `toolName`, `toolCallId`, `status`,
`order`, `title`, `hasResult`, and `activeElapsedOwner` when the surface can
observe the field.

The core assertions are stable tool identity, no duplicate live overlay for one
tool, monotonic `pending < running < terminal` status, terminal facts preserved
after late previews, committed snapshots as the authoritative truth, and no
active spinner or elapsed owner after a row has reached terminal state. These
checks run before screenshots are evaluated.

Workbench Playwright specs that exercise running turns should sample transcript
rows every 250-500ms while the task is active and attach JSON samples on
failure. The samples must include DOM identity attributes such as
`data-entry-id`, `data-block-id`, `data-block-kind`, `data-turn-id`, and
`data-source` when present. Screenshot and VHS artifacts remain layout evidence,
not the sole transcript correctness oracle.

Projection diagnostics use the vocabulary from
[035 Event Stream](../035-event-stream/spec.md). Deterministic tests fail on any
undeclared diagnostic count. Live sweeps may attach analyzer summaries for real
provider or transport drift, but fake-provider replay remains the correctness
gate.

## Scenario Matrix

- Tool rows only render from typed tool/execution/display blocks, never from
  reasoning or assistant prose.
- `exec_command` titles preserve the first actual command line and survive
  start-to-end updates that omit arguments.
- A pending `exec_command` row whose visible title is only bare `exec_command`
  and whose sampled data lacks recoverable `args.cmd`/`arguments.cmd` is a
  projection defect, not an acceptable loading state.
- Yielded `exec_command` plus matching `write_stdin` output renders as one exec
  chain across live updates and history reload.
- Failed `write_stdin` calls that target an existing yielded `exec_command`
  session do not render as standalone primary transcript rows.
- Empty reasoning completions close existing Thinking rows without creating
  placeholder rows.
- Running Thinking defaults to a visible body or bounded live preview, then
  collapses once on completion, failure, or cancellation. Manual collapse
  during streaming survives later deltas, manual expansion after completion
  survives repeated terminal rendering, and already-terminal history defaults
  collapsed.
- A streaming core `write` row decodes partial top-level `path` and `content`
  arguments into one bounded preview owned by its tool position/call id.
  Parallel writes remain distinct; duplicate cumulative snapshots do not
  duplicate content; escaped JSON and Unicode chunk boundaries decode exactly.
- The first non-empty write preview opens automatically, later snapshots honor
  manual collapse, successful completion removes and collapses it once, and
  failed or cancelled settlement preserves it with the failure. Preview text
  never enters persisted messages, provider context, exports, or accounting.
- Agent invocation handoff, stale pending blocks, child previews, and
  completion metadata leave exactly one parent Agent row.
- Folding preserves useful head/tail previews without turning large outputs
  into ordinary always-open transcript content.
- Completed successful `edit`, `write`, or `apply_patch` rows with parseable
  Git patch `result.diff` show `Edited ... (+A -D)`, default-open parsed diff
  detail, and no ordinary Input/Result metadata or `Diff` label above the
  rendered diff.
- Malformed diff text, failed update tools, running update tools, and `write`
  results without diff preserve existing raw/structured rendering behavior.
- Display-only command feedback and artifacts remain outside ordinary
  transcript projection and model-visible history.
- Context usage remains visible before lower-priority observability segments
  on compact surfaces.
- Evidence rows without a secondary summary give the title all remaining text
  width, while rows with a summary preserve the summary/status columns. Both
  layouts expose the complete title through native hover disclosure, including
  tool, status, failure, and diagnostic titles.
- Parallel live Web Search rows use their query-bearing titles as the sole
  title column on desktop and mobile widths; provider metadata stays in detail,
  titles truncate within the row, and elapsed/status content does not overflow.

## Validation Boundaries

- Tests should compare semantic rendering invariants, stable row identity, and
  display-only boundaries rather than private DOM, CSS, or ratatui line
  buffers.
- Visual artifacts are required when a concrete TUI or Web change affects
  rendered appearance.
- Snapshot tests should avoid brittle provider-payload, prompt, or full-output
  comparisons unless the tested surface owns that exact text.
