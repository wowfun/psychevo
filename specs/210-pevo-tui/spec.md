---
name: 210. pevo TUI
psychevo_self_edit: deny
---

Define the first interactive terminal surface for `pevo`.

This topic implements the terminal-specific surface defined by
[080 Design System](../080-design-system/spec.md). It also builds on
[200 pevo CLI](../200-pevo-cli/spec.md) and [026 Commands](../026-commands/spec.md),
and routes live coding-agent turns through `psychevo-gateway`. Runtime remains
the execution and persistence kernel behind Gateway. For interactive
terminals, `pevo tui` is a fullscreen terminal UI. For non-terminal
stdin/stdout, it keeps the deterministic line-by-line scripted behavior.

## Scope

- `pevo tui` command spelling, startup behavior, and non-terminal fallback
- persisted TUI-local model, variant, mode, thinking visibility, raw transcript
  visibility, and sidebar visibility
- user-configured slash command aliases and shortcuts loaded from effective
  `config.toml`
- session resume, switching, archiving/deletion, titles, running-session list
  indicators, undo/redo-adjacent session behavior, and history loading
- history-only reload treatment for unfinished tool calls, including
  process-restart orphan rows that must not animate as live work
- model, variant, mode, thinking visibility, raw transcript visibility, local
  stats, context-usage, and status state surfaces
- explicit scoped default-model writes from the model picker
- responsive foreground interruption and preservation of every visible
  assistant answer emitted during a multi-tool turn
- direct user shell escapes from fullscreen and scripted input, persisted as
  user-provided shell context without exposing `exec_command` as a plan-mode
  model tool
- live exec-session rendering for yielded `exec_command` processes, including
  background output updates and interruption cleanup within the current runtime
  process
- wrap-aware bottom approval panels that preserve all approval choices even
  when long tool/action/grant details wrap across many terminal rows
- shared ownership boundaries for the rendered TUI surface, interaction model,
  sessions, state, and validation
- long-lived process-scoped Gateway ownership for thread/source binding,
  active-turn queueing, steering, interrupt, permission, clarify, and typed
  timeline projection

Rendering-specific rules live in [211 pevo TUI Rendering](../211-pevo-tui-rendering/spec.md).
Input, slash-command, popup, panel, and selection rules live in
[212 pevo TUI Interaction](../212-pevo-tui-interaction/spec.md). This topic
keeps the parent command contract and cross-cutting TUI state/session behavior.

Out of scope:

- plugins, user-configurable statusline fields, TUI theme configuration, or
  full rich document rendering beyond bounded Markdown projection
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

TUI reads slash command customization from the effective `config.toml` using
the same global/project merge and explicit `PSYCHEVO_CONFIG` behavior as
provider configuration. The optional shape is:

```toml
[tui]
leader_key = "ctrl+x"
leader_timeout_ms = 2000

[tui.slash_aliases]
"/model" = ["/m"]
"/sessions" = ["/s"]
"/export -f json -i messages" = ["/xj"]

[tui.slash_keybinds]
"/model" = "<leader>m"
"/status" = "ctrl+s"
"/variant high" = "<leader>h"
"/copy" = ["<leader>y", "ctrl+shift+c"]
"/usage" = "none"
"/export -f json -i messages" = "<leader>x"
```

This configuration is local UI behavior only: it does not change CLI command
spelling, persisted session content, provider payloads, or `tui-state.json`.
`slash_aliases` keys and `slash_keybinds` keys are concrete slash input lines
validated by the normal slash parser. Alias input expands to that concrete
slash input before parsing; if the alias is followed by additional text, that
text is appended to the configured target line and then parsed. Invalid alias
or keybinding configuration rejects TUI startup with a bounded configuration
error. Configured aliases participate in slash menu completion as alias rows,
and configured concrete slash lines appear in `/help` `Custom commands`.

## Gateway Ownership

Fullscreen TUI owns one long-lived `Gateway` instance for the process. Its
source lifetime is `Process`, so the process can remember the current thread
without creating durable source bindings. Normal prompts, queued prompts,
steer, interrupt, permission responses, clarify responses, source reset, and
thread switching go through Gateway APIs.

The TUI slash parser remains local UI behavior, but slash command effects must
map to typed Gateway/runtime APIs. TUI must not add a generic `slash/exec`
Gateway method and must not shell out to `pevo run` for normal prompting or
control.

## Attachments

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
