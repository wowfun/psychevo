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

- `pevo tui` command spelling and startup behavior
- fullscreen transcript, composer, and fixed single-line status/hint line
- persisted TUI-local model, variant, mode, thinking visibility, raw transcript
  visibility, and sidebar visibility
- session selection, session archiving/deletion, session renaming, model,
  variant, mode, thinking visibility, stats, and status slash commands, with
  `/status` kept to runtime/session state rather than thinking visibility
- context usage slash command for the latest provider request or current
  session estimate
- design-system rendering for prompts, folded reasoning, tool evidence, final
  answers, timeout-visible tool failures, and turn metadata
- fixed bottom status line with mode, model, and compact context-window usage
  derived from the latest context snapshot
- local stats reporting from persisted accounting columns
- responsive foreground interruption and preservation of every visible
  assistant answer emitted during a multi-tool turn
- direct user shell escape from the composer and scripted input, persisted by
  default as user-provided shell context for subsequent provider requests while
  never exposing `bash` as a plan-mode model tool
- fullscreen composer `@` file path completion for the selected working
  directory
- image attachments from standalone readable image-source paste and `/image`,
  with numbered composer placeholders and local attachment metadata
- mouse expansion for bounded Thinking and tool evidence rows rendered through
  the shared evidence component, including shared evidence title/marker styling;
  V1 does not provide transcript review mode or a keyboard path to expand one
  specific evidence row
- ledger-only active tool status, including pending provider-side tool input
  and persisted assistant tool calls whose tool results have not arrived yet,
  with at least one visible active frame, no stale provisional rows after
  completion, no active-turn metadata blocks while assistant content is still
  streaming, static interrupted evidence after aborted reloads, stable
  transcript scrolling, fullscreen alternate-screen scrollback isolation, and
  hover-routed mouse-wheel scrolling
- local-only row-level expansion for long Thinking bodies and long tool output
  using the same line, display-token, and width collapse thresholds, without
  derived transcript section headers
- debug projection for usage and provider metadata summaries
- deterministic visual-regression projections and local diagnostic screenshots
- terminal-adaptive semantic rendering for prompt, composer, popup, bottom
  panel, fixed status line, selection, and evidence surfaces
- lightweight terminal Markdown projection for assistant answers, plus raw
  transcript display and raw Markdown answer copy
- local session export/share slash commands backed by the same transcript
  artifact boundary as `pevo session export` and `pevo session share`
- hard `plan` / `default` runtime mode selection
- interactive skill listing and explicit skill invocation slash commands

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
- [Sessions](sessions.md) defines session resume, switching, stable activity ordering, history, titles, archive/delete, and undo/redo-adjacent session behavior.
- [State and Models](state-and-models.md) defines TUI-local state, model selection, catalog fetching, variants, and runtime modes.
- [Input and Commands](input-and-commands.md) defines key handling, slash commands, file completion, user shell escapes, and local text selection.
- [Layout and Rendering](layout-and-rendering.md) defines ledger layout, evidence projection, rendering rules, and visual diagnostics.
- [Testing](testing.md) defines deterministic acceptance coverage and validation expectations.

## Related Topics

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
