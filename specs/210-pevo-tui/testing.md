---
name: 210. pevo TUI Testing
psychevo_self_edit: deny
---

# 210. pevo TUI Testing

Define acceptance expectations and validation scenarios for the concrete
`pevo tui` terminal surface.

## Long-Term Acceptance Contract

- `pevo tui` starts the fullscreen terminal surface for interactive terminals
  and keeps deterministic line-by-line behavior for non-terminal stdin/stdout.
- TUI-local model, variant, mode, thinking visibility, raw transcript
  visibility, sidebar visibility, and configured slash aliases/keybindings are
  persisted and restored without changing CLI command spelling or provider
  payloads.
- Session selection, resume, switching, archive/delete, title, history reload,
  running-session indicators, undo/redo-adjacent behavior, and cross-surface
  Gateway activity remain consistent with shared session/display contracts.
- Terminal rendering projects shared display-model entries through the TUI
  ledger without persisting viewport-wrapped terminal lines as durable display
  state.
- TUI key handling remains recoverable: active local UI state clears before
  interruption, and interruption wakes provider waits, shell waits, and pending
  approval/clarify state.
- TUI-specific slash menus, bottom panes, file/image/agent/skill popups,
  approval panels, and `/diff` overlays project shared UI command results
  without creating ordinary transcript content unless the shared contract says
  a transcript turn is being submitted.
- Deterministic terminal visual captures keep running-agent, clarification,
  permission, and tool states observable without depending on real provider
  latency.

## Current Implementation Slice

Automation vocabulary and generic validation boundaries follow
[060 Automation](../060-automation/spec.md).

Relevant narrow validation:

- `cargo test -p psychevo-ai`
- `cargo test -p psychevo-agent-core`
- `cargo test -p psychevo-runtime`
- `cargo test -p psychevo-cli`

Broad validation remains `scripts/validate.sh`. Documentation-only changes to
this topic do not require code tests unless executable examples, generated
artifacts, or validation instructions change.

Manual real-provider validation is opt-in only.

## Scenario Matrix

- TUI state read/write, version tolerance, per-workdir model and variant
  precedence, mode persistence, thinking/raw/sidebar visibility persistence,
  configured alias/shortcut parsing, and startup rejection for invalid
  keybinding configuration.
- Default session selection, explicit `--session`, `--new`, startup-history
  loading, session switching, live buffered event replay, background completion
  isolation, and cross-surface running activity visibility.
- Fullscreen ledger projection for prompt blocks, Thinking, tool/evidence,
  Agent rows, assistant answers, compact metadata, context usage, status line,
  sidebars, terminal Markdown, raw display, and non-terminal plain rendering.
- Terminal palette fallback, adaptive prompt/composer surfaces, passive redraw
  cadence, active elapsed labels, deterministic reduced-motion behavior, and
  VHS capture of running/permission/clarify/tool states.
- Composer behavior for submit, newline insertion, history recall, shell mode,
  local selection, bracketed paste, file/image completion, transcript focus,
  mouse routing, clipboard fallback, and contextual help.
- Slash command parsing, registry-backed discovery, terminal menu navigation,
  `/help`, `/status`, `/context`, `/usage`, `/diff`, `/model`, `/variant`,
  `/mode`, `/permissions`, `/sandbox`, `/copy`, `/export`, `/share`,
  `/image`, `/undo`, `/redo`, dynamic skill/bundle commands, and bounded
  feedback for unsupported or invalid command states.
- Pending steer/queue UI, normal queue FIFO drain, interrupted foreground turns
  restoring queued prompt and shell inputs to the composer, and stale steer
  rejection feedback.
- TUI approval and clarify panels: FIFO request display, keyboard/mouse
  resolution, supported option filtering, interruption cleanup, and bounded
  stale-response feedback.
- `/agents`, `@agent`, `/fork`, Agent row open/toggle behavior, child-thread
  foreground entry, parent navigation, and selected-main-agent controls.

## Validation Boundaries

- Tests should compare semantic transcript/display facts and stable terminal
  behavior, not viewport-private line buffers or provider payload shapes.
- Rendering tests may use terminal snapshots and VHS captures, but should avoid
  brittle full-screen snapshots when structured row facts are available.
- TUI tests must use fake or test providers and isolated `PSYCHEVO_HOME`,
  config, SQLite state, workdir, timers, sockets, and terminal fixtures.
- Live provider failures must be reported separately from deterministic TUI
  regressions.
