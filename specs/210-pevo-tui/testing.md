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
- Interactive Session title changes emit sanitized, deduplicated `Pevo | ...`
  terminal-tab OSC output and terminal restoration clears it; scripted mode
  emits none.
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

CI/CD vocabulary and generic validation boundaries follow
[065 CI/CD](../065-ci-cd/spec.md).

Relevant narrow validation:

- `cargo test -p psychevo-ai`
- `cargo test -p psychevo-agent-core`
- `cargo test -p psychevo-runtime`
- `cargo test -p psychevo-cli`

Rust broad validation remains `cargo xtask ci run --profile rust-broad`.
Documentation-only changes to this topic do not require code tests unless
executable examples, generated artifacts, or validation instructions change.

Deterministic TUI/VHS visual diagnostics run through:

- `cargo xtask ci run --profile visual`

This workflow uses a fake local provider and writes reviewable artifacts under
the CI artifact root.

Deterministic cross-surface profiling runs the real fullscreen TUI through a
pseudo-terminal and the real Workbench through desktop Chromium. Both surfaces
use the same local Native provider fixture, cwd, model configuration, fixed
prompt class, response schedule, warmup policy, and measured sample count. A
cold run also requires identical Agent and Skill enablement; disabling either
only on the TUI side invalidates the comparison. The shared cwd is an isolated,
synthetic Git workspace rather than a child of the source checkout. A
cold first turn is retained as raw evidence; warm continuation turns use one
excluded warmup, one separately excluded traced diagnostic sample, and twenty
untraced measured samples by default.

Manual real-provider validation is opt-in only.

## Scenario Matrix

- TUI state read/write, version tolerance, per-cwd model and variant
  precedence, mode persistence, thinking/raw/sidebar visibility persistence,
  configured alias/shortcut parsing, and startup rejection for invalid
  keybinding configuration.
- Default session selection, explicit `--session`, `--new`, startup-history
  loading, session switching, live buffered event replay, background completion
  isolation, and cross-surface running activity visibility.
- Fullscreen ledger projection for prompt blocks, Thinking, tool/evidence,
  Agent rows, assistant answers, compact metadata, context usage, status line,
  sidebars, terminal Markdown, raw display, and non-terminal plain rendering.
- Resumed local Web Search results decode the persisted untrusted wrapper into
  the existing foldable tool row and never expose wrapper text as the result.
- Context status uses the latest completed provider turn while Session tokens
  aggregate only the visible branch, preserve partial/unavailable state, and
  compute cache read percentage against context input including cache writes.
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
- Cross-surface profiling proves that the TUI is interactive after a painted
  Composer frame, captures Enter at the real key-handler boundary, paints
  optimistic feedback before model output, dispatches exactly one main turn
  request per sample, paints non-empty assistant output, and restores the
  Composer after authoritative completion. Title-generation requests are
  classified separately and cannot advance a main-turn sample.

## Validation Boundaries

- Tests should compare semantic transcript/display facts and stable terminal
  behavior, not viewport-private line buffers or provider payload shapes.
- Rendering tests may use terminal snapshots and VHS captures, but should avoid
  brittle full-screen snapshots when structured row facts are available.
- TUI tests must use fake or test providers and isolated `PSYCHEVO_HOME`,
  config, SQLite state, cwd, timers, sockets, and terminal fixtures.
- Profiling must not use piped scripted mode as a substitute for fullscreen
  paint. PTY output is drained but not stored in the manifest; only allowlisted
  content-free marks may be retained.
- TUI Add Provider tests should drive provider-preset, provider-wizard, and
  model-catalog fetch behavior with fake local `/models` endpoints and must
  assert that raw API keys are not rendered or written to TOML.
- Live provider failures must be reported separately from deterministic TUI
  regressions.
