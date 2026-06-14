---
name: 214. pevo Diff Command Testing
psychevo_self_edit: deny
---

Define acceptance expectations and validation scenarios for the `/diff` slash
command and shared workspace diff model.

## Long-Term Acceptance Contract

- `/diff` is an observational display artifact and does not append runtime
  messages, affect model context, change exports, or affect usage/cost
  statistics.
- Diff acquisition uses an isolated git worktree snapshot containing tracked
  changes and untracked files.
- Git diff exit code `1` is treated as a successful differences-found result.
- Empty, non-git, binary, unreadable, and truncated states are represented with
  structured display material and visible user-facing status.
- Truncation is bounded by `256 KiB` or `3000` lines, whichever is reached
  first, and includes explicit metadata.
- Fullscreen TUI `/diff` opens a read-only static overlay pager titled
  `D I F F`; `Esc` closes that overlay before it interrupts an active turn.
- Inline edit-result diff rendering may reuse the parsing model but remains a
  separate surface from fullscreen `/diff`.
- ACP projects `/diff` as structured diff tool-call content and does not fall
  back to plain assistant text.
- Web and desktop shells expose the same structured diff model through Gateway
  review surfaces without adding ordinary transcript rows.

## Current Implementation Slice

Automation vocabulary and generic validation boundaries follow
[060 Automation](../060-automation/spec.md).

Deterministic tests should use temporary git repositories and local fixtures.
They should not depend on the caller's worktree, global git configuration,
provider credentials, or live services.

Visual validation is required when a change affects fullscreen TUI diff
rendering or Workbench diff presentation. Real-provider validation is not
required for diff-only behavior unless the same change alters live transcript
projection.

## Scenario Matrix

- `/diff` outside a git worktree returns the non-git display state.
- `/diff` in a clean git worktree returns the empty-diff display state.
- Tracked modifications, deletions, and untracked files appear in the unified
  diff snapshot.
- Binary and unreadable files produce placeholders instead of raw bytes.
- Byte and line truncation both produce visible truncation notices and metadata.
- Fullscreen TUI overlay renders file headers, hunk headers, add/delete/context
  lines, old/new line numbers, binary placeholders, unreadable placeholders,
  and truncation notices.
- Inline edit rows render a single visible line-number gutter while fullscreen
  `/diff` keeps old/new dual line numbers.
- `/diff` remains available during an active turn but does not create durable
  transcript, message, or accounting records.
- ACP `/diff` emits structured diff content after command acceptance.
- Workbench changed-file rows and review previews use the same structured diff
  states as the command.

## Validation Boundaries

- Tests should assert structured diff semantics and surface ownership rather
  than comparing entire raw git patch strings where a smaller invariant is
  sufficient.
- Fixture repositories must be isolated from the developer's current checkout.
- Visual fixtures should cover framing, line numbering, and color roles, but
  screenshots are diagnostics rather than durable transcript facts.
