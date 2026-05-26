---
name: 213. pevo Display Model
psychevo_self_edit: deny
---

# 213. pevo Display Model

Define Psychevo's semantic display/transcript model. Runtime `messages` remain
the source of truth for model context, exports, compaction, undo, accounting,
and statistics. Display records are UI artifacts for TUI-first presentation and
future ACP/WebUI/IM projection.

## Scope

- durable semantic display blocks for prompts, answers, thinking, tools,
  status, command results, diffs, and local artifacts
- display storage schema and cutover behavior
- reusable renderable component expectations for transcript-style surfaces
- separation between model-visible history and UI-visible artifacts

Out of scope:

- provider-neutral AI message semantics
- report/export message contents
- client-specific layout details beyond required semantic affordances

## Semantics

Display blocks store semantic content, not terminal-rendered rows. They may
include role, title, body text, structured metadata, ordering, visibility,
folding state, and display category. They must not store ratatui `Line`s,
terminal ANSI color, viewport-dependent wrapping, or layout cache rows.

Display-only command output and observational artifacts, including `/diff`,
must not become model context, session export message content, usage/cost
statistics, or durable loop-visible assistant/user messages.

Model-visible tool results may contain material that also benefits from richer
UI projection. Display readers may parse stable tool-result fields, such as an
`edit.diff` Git patch block, into semantic diff blocks for rendering. The
parsed block is a UI artifact; it must not replace or mutate the model-visible
tool result.

## Storage

The state database schema version is `12`. Psychevo does not migrate state
databases at version `11` or lower in this cutover. Opening an old state
database must fail with explicit guidance to run `pevo init --reset-state` or
set `PSYCHEVO_DB` to a new database.

The display block table is additive to runtime message storage. Runtime
messages remain available for non-display consumers; display readers may build
TUI rows from display blocks and may fall back to live in-memory projection
while a turn is streaming.

## Rendering Contract

TUI transcript rendering should consume semantic blocks through reusable
renderable components with stable `desired_height(width)` and `render(area)`
behavior. Component rendering owns wrapping, highlight roles, selection, and
folding. Layout caches cache semantic block keys and measured heights, not
terminal strings.

ACP/WebUI/IM adapters may map display blocks into client-native update shapes,
but must not require TUI-specific layout fields.

## Related Topics

- [026 Commands](../026-commands/spec.md)
- [211 pevo TUI Rendering](../211-pevo-tui-rendering/spec.md)
- [214 pevo Diff Command](../214-pevo-diff-command/spec.md)
