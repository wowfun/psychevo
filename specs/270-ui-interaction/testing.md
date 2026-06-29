---
name: 270. UI Interaction Testing
psychevo_self_edit: deny
---

# 270. UI Interaction Testing

Define acceptance expectations and validation scenarios for shared UI
interaction semantics.

## Long-Term Acceptance Contract

- Composer submission distinguishes ordinary prompts, active-turn steer,
  explicit queueing, shell mode, attachments, command execution, and
  display-only feedback.
- Pending steer and queued input previews are display-only and are replaced or
  cleared when committed transcript entries, cancellation, or interruption
  settles the input.
- Shell mode remains a user shell-context interaction, not a model-visible
  `exec_command` tool request.
- UI command results route by destination into transcript flow, panels,
  overlays, previews, status/observability, downloads/share, composer state, or
  bounded display-only feedback.
- Permission and clarify responses are scoped by thread/source context and
  reject stale or wrong-thread requests.
- Interrupt and undo/redo interactions keep transcript, workspace,
  observability, and composer state aligned.

## Current Implementation Slice

CI/CD vocabulary and generic validation boundaries follow
[065 CI/CD](../065-ci-cd/spec.md).

Shared interaction validation should assert behavior at the command/result,
thread/source, request, and transcript projection boundaries before checking
surface-specific presentation. Concrete TUI and Web controls are validated by
[210 pevo TUI Testing](../210-pevo-tui/testing.md) and
[240 pevo Web Testing](../240-pevo-web/testing.md).

Manual real-provider validation is opt-in only.

## Scenario Matrix

- Active-turn prompt submission steers when supported and queueing remains an
  explicit next-turn action.
- Pending previews are replaced by committed transcript entries and do not
  become durable session messages.
- Shell mode remains distinct from model-visible `exec_command` tool calls.
- `/diff` opens a structured display artifact without appending ordinary
  transcript rows.
- `/btw [prompt]` opens a side thread before submitting inline prompt text and
  does not add a command row to the parent transcript.
- Command results route to panels, overlays, previews, status, downloads, or
  transcript flow according to destination.
- Permission and clarify responses reject stale or wrong-thread requests.
- Interruption releases pending request state and restores pending composer
  inputs according to the owning surface's local editing model.
- Undo restores prompt text into the composer when the command result includes
  restored prompt content.

## Validation Boundaries

- Tests should compare shared behavior and typed command/result state before
  checking surface-specific DOM, keymap, or terminal presentation.
- TUI key chords, terminal panes, and local clipboard behavior belong to
  [210 pevo TUI Testing](../210-pevo-tui/testing.md).
- Workbench menus, popovers, browser host actions, and responsive panels belong
  to [240 pevo Web Testing](../240-pevo-web/testing.md).
