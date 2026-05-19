---
name: 210. pevo TUI Testing
psychevo_self_edit: deny
---

# 210. pevo TUI Testing

Define deterministic acceptance coverage for the parent `pevo tui` topic:
startup, state, session ownership, model/runtime integration, and cross-topic
validation. Rendering-specific coverage lives in
[211 pevo TUI Rendering Testing](../211-pevo-tui-rendering/testing.md).
Interaction-specific coverage lives in
[212 pevo TUI Interaction Testing](../212-pevo-tui-interaction/testing.md).

Automation vocabulary and generic validation boundaries follow
[060 Automation](../060-automation/spec.md).

## Scope

- TUI-local state persistence and startup behavior
- session selection, resume, archive/delete, title, and history ownership
- model, variant, mode, context usage, and local accounting integration
- scripted/non-terminal `pevo tui` behavior that is not purely rendering or
  interaction-specific
- runtime stream projection contracts that affect TUI state across rendering
  and interaction boundaries

Out of scope:

- transcript, status-line, sidebar, Markdown, Agent-row, and visual-regression
  rendering coverage; see
  [211 testing](../211-pevo-tui-rendering/testing.md)
- keymaps, slash commands, popups, panels, mouse routing, selection, clipboard,
  user shell interaction, and agent controls; see
  [212 testing](../212-pevo-tui-interaction/testing.md)

## Deterministic Tests

Required parent-topic coverage:

- TUI state read/write, version tolerance, per-workdir model and variant
  precedence, per-workdir mode persistence, global thinking persistence, global
  raw transcript visibility persistence, global sidebar visibility persistence,
  and recent-model bounding.
- Default TUI session selection of the latest `run` or `tui` session by
  canonical workdir, explicit `--session`, `--new`, and startup-history loading
  for selected/latest sessions.
- Session title persistence, `/rename <title>` effects on the current session,
  automatic title generation with model success, deterministic first-prompt
  fallback when title generation fails or returns an unusable title, selected
  skill context in non-persisted title requests, and sidebar/session picker
  refresh after detached post-`agent_end` title generation completes.
- Session history loading and replacement without synthetic status rows,
  preserving persisted folded reasoning as local Thinking evidence and
  persisted elapsed time in turn metadata when available.
- Running-session switching isolates stream projection: output from the
  previous running session must not appear in the newly displayed session,
  switching back replays that session's live buffered events, background
  completion must not steal `current_session`, and `/new` must remain free of
  stale running output.
- `--new` and fullscreen `/new` session creation/clearing behavior, including
  no transcript status row, reset of context usage state, pending images, and
  stale terminal glyphs.
- Non-terminal scripted input with prompt lines and slash commands, including
  line-by-line handling and deterministic output for local state commands.
- Runtime stream projection that never leaks folded reasoning into sanitized
  message events while still delivering dedicated TUI thinking events.
- Runtime metrics projection that can expose usage and allowlisted metadata to
  TUI without putting them in sanitized transcript messages.
- Runtime Plan mode toolset: exposes `read`, `list`, `search`, and
  fullscreen-interactive `clarify`; does not expose `bash`, `write`, or `edit`.
- Mode instruction is sent to providers for the current turn and is not
  persisted in `messages`.
- Local stats and accounting projection from persisted columns, including
  unknown-priced messages and known free messages.
- Context usage state sourced from the latest context snapshot or latest
  provider input usage when a context limit is known, restored on resume and
  session switching before first draw.
- Agent runtime identity tests for `@agent-name` delegations resolving to that
  definition name rather than `general`, `name` alias compatibility,
  `name`/`agent_type` conflict errors, explicit unknown agent errors, and
  omitted agent names defaulting to `general`.
- Nested-agent runtime tests for effective `max_spawn_depth`: unset, `null`,
  and `0` make a direct child a leaf; `1` allows one grandchild level and
  decrements the remaining depth to `0`; pause-new-spawns rejects new `Agent`
  calls without interrupting already running children.

## Cross-Topic Acceptance

Parent tests should assert invariants that span rendering and interaction
without duplicating detailed coverage from 211 or 212:

- Normal completion drains queued input FIFO; interrupted fullscreen turns
  restore queued prompt and shell inputs to the composer instead of starting
  the next queue item automatically.
- Multi-tool turns that emit visible assistant text before and after tool calls
  preserve each visible assistant message as a separate answer block; later
  streaming updates must not replace earlier answer text from the same turn.
- Cleanup after undo/redo, session switching, `/new`, and interrupted turns
  must keep persisted session state, visible transcript state, and next provider
  context aligned.

## Validation

Relevant narrow validation:

- `cargo test -p psychevo-ai`
- `cargo test -p psychevo-agent-core`
- `cargo test -p psychevo-runtime`
- `cargo test -p psychevo-cli`

Broad validation remains:

- `scripts/validate.sh`
