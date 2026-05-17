---
name: 210. pevo TUI
psychevo_self_edit: deny
---

Define the first interactive terminal surface for `pevo`.

This topic implements the terminal-specific surface defined by
[080 Design System](../080-design-system/spec.md). It also builds on
[200 pevo CLI](../200-pevo-cli/spec.md) and [026 Commands](../026-commands/spec.md),
and routes live coding-agent turns through `psychevo-runtime`. For interactive
terminals, `pevo tui` is a fullscreen terminal UI. For non-terminal
stdin/stdout, it keeps the deterministic line-by-line scripted behavior.

## Scope

- `pevo tui` command spelling, startup behavior, and non-terminal fallback
- persisted TUI-local model, variant, mode, thinking visibility, raw transcript
  visibility, and sidebar visibility
- session resume, switching, archiving/deletion, titles, undo/redo-adjacent
  session behavior, and history loading
- model, variant, mode, thinking visibility, raw transcript visibility, local
  stats, context-usage, and status state surfaces
- responsive foreground interruption and preservation of every visible
  assistant answer emitted during a multi-tool turn
- direct user shell escapes from fullscreen and scripted input, persisted as
  user-provided shell context without exposing `bash` as a plan-mode model tool
- shared ownership boundaries for the rendered TUI surface, interaction model,
  sessions, state, and validation

Rendering-specific rules live in [211 pevo TUI Rendering](../211-pevo-tui-rendering/spec.md).
Input, slash-command, popup, panel, and selection rules live in
[212 pevo TUI Interaction](../212-pevo-tui-interaction/spec.md). This topic
keeps the parent command contract and cross-cutting TUI state/session behavior.

Out of scope:

- plugins, user-configurable keymaps, user-configurable statusline fields, TUI
  theme configuration, or full rich document rendering beyond bounded Markdown
  projection
- approvals, auth, provider login, or model probing
- structured `@file` references, automatic file-content attachment, custom
  slash commands, or command-template files
- transcript review overlay, compaction, rollback, fork UI, remote session
  publishing, or external editor integration

## Command

`pevo tui [message..]` starts the interactive terminal surface for the selected
working directory.

Accepted first-slice flags are:

- `--dir <path>` selects the working directory.
- `-m, --model <provider/model>` selects the model for this TUI process only.
- `--variant <none|minimal|low|medium|high|xhigh|max>` selects the reasoning
  effort for this TUI process only.
- `-s, --session <id>` starts from an explicit session.
- `--new` starts from a new session on the first submitted prompt.
- `--debug` enables debug-only local projections, including usage parts and
  allowlisted provider metadata summaries.
- `--no-skills` disables default and configured skill discovery.
- `--skill <name-or-path>` is repeatable and explicitly adds a skill by name or
  path.

When positional message text is supplied, TUI submits it immediately and then
continues the prompt loop. If that text begins with `!` after leading
whitespace, it is processed as a user shell escape instead of a provider
prompt. In non-terminal stdin, each input line is processed as one prompt,
slash command, or user shell escape. Non-terminal stdin is not appended to the
positional prompt, and the fullscreen alternate screen is not used.

`pevo tui` requires initialized `PSYCHEVO_HOME`, because TUI-local state lives
under that home. `PSYCHEVO_CONFIG` and `PSYCHEVO_DB` may still override provider
configuration and SQLite state path, but they do not bypass the home
initialization requirement.

## Topic Attachments

- [080 Design System](../080-design-system/spec.md) is the source of truth for
  visual language, shared TUI component contracts, and interaction principles.
- [Sessions](sessions.md) defines session resume, switching, stable activity
  ordering, history, titles, archive/delete, and undo/redo-adjacent session
  behavior.
- [State and Models](state-and-models.md) defines TUI-local state, model
  selection, catalog fetching, variants, and runtime modes.
- [Testing](testing.md) defines deterministic acceptance coverage and validation expectations.

## Related Topics

- [211 pevo TUI Rendering](../211-pevo-tui-rendering/spec.md) defines ledger
  layout, evidence projection, rendering rules, and visual diagnostics.
- [212 pevo TUI Interaction](../212-pevo-tui-interaction/spec.md) defines key
  handling, slash commands, file completion, user shell escapes, panels, and
  local text selection.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines the product CLI surface.
- [026 Commands](../026-commands/spec.md) defines shared command contract
  conventions.
- [200 pevo run](../200-pevo-cli/pevo-run.md) defines non-interactive live run.
- [003 AI Protocol](../003-ai-protocol/spec.md) defines provider message
  translation boundaries.
- [120 Provider Registry](../120-provider-registry/spec.md) defines
  provider/model resolution.
- [040 SQLite Persistence](../040-storage-and-persistence/sqlite-persistence.md)
  defines session and message persistence.
- [055 Skills](../055-skills/spec.md) defines skill discovery, model visibility,
  tools, and lifecycle behavior.
- [051 Agents](../051-agents/spec.md) defines agent definition discovery.
- [051 Subagents](../051-agents/subagents.md) defines subagent run control semantics.
