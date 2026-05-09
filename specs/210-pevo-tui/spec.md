---
name: 210. pevo TUI
psychevo_self_edit: deny
---

Define the first interactive terminal surface for `pevo`.

This topic builds on [200 pevo CLI](../200-pevo-cli/spec.md) and routes live
coding-agent turns through `psychevo-runtime`. For interactive terminals,
`pevo tui` is a fullscreen terminal UI. For non-terminal stdin/stdout, it keeps
the deterministic line-by-line scripted behavior.

## Scope

- `pevo tui` command spelling and startup behavior
- fullscreen transcript, composer, and minimal bottom state line
- persisted TUI-local model, variant, mode, thinking visibility, and sidebar
  visibility
- session selection, session archiving/deletion, session renaming, model,
  variant, mode, thinking visibility, and status slash commands
- evidence-ledger rendering for prompts, folded reasoning, tool evidence,
  final answers, and turn metadata
- responsive foreground interruption and preservation of every visible
  assistant answer emitted during a multi-tool turn
- direct user shell escape from the composer and scripted input
- fullscreen composer `@` file path completion for the selected working
  directory
- transcript selection and keyboard expansion for bounded tool evidence
- debug projection for usage and provider metadata summaries
- deterministic visual-regression projections and local diagnostic screenshots
- hard `plan` / `default` runtime mode selection
- interactive skill listing and explicit skill invocation slash commands

Out of scope:

- panes, plugins, custom keymaps, or heavy markdown rendering
- approvals, auth, provider login, provider catalogs, or model probing
- structured `@file` references, automatic file-content attachment, custom
  slash commands, or command-template files
- compaction, rollback, share/fork UI, external editor integration, or TUI
  theme configuration

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

- [Sessions](sessions.md) defines session resume, switching, history, titles, archive/delete, and undo/redo-adjacent session behavior.
- [State and Models](state-and-models.md) defines TUI-local state, model selection, catalog fetching, variants, and runtime modes.
- [Input and Commands](input-and-commands.md) defines key handling, slash commands, file completion, user shell escapes, and local text selection.
- [Layout and Rendering](layout-and-rendering.md) defines ledger layout, evidence projection, rendering rules, and visual diagnostics.
- [Testing](testing.md) defines deterministic acceptance coverage and validation expectations.

## Related Topics

- [200 pevo CLI](../200-pevo-cli/spec.md) defines the product CLI surface.
- [200 pevo run](../200-pevo-cli/pevo-run.md) defines non-interactive live run.
- [003 AI Protocol](../003-ai-protocol/spec.md) defines provider message
  translation boundaries.
- [120 Provider Registry](../120-provider-registry/spec.md) defines
  provider/model resolution.
- [040 SQLite Persistence](../040-storage-and-persistence/sqlite-persistence.md)
  defines session and message persistence.
- [055 Skills](../055-skills/spec.md) defines skill discovery, model visibility,
  tools, and lifecycle behavior.
