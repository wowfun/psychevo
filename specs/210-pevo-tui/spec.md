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
- `/agents`, `@agent-name`, and `/fork` are the interactive projections for
  agent definition discovery and first-class child-agent runs. Bare `@word`
  completion prefers agent names; path-shaped tokens continue to use file
  completion. `/fork` creates a background forked child agent.
- `/agents` opens a two-tab console. `Running` lists live child agents for the
  current session tree, shows the current depth/concurrency cap state, and
  offers `Open`, `Pause/Resume spawning`, and `Stop subtree`. `Available` lists
  callable agent definitions from supported discovery sources, marks active and
  shadowed duplicates, surfaces supported definition parse failures as disabled
  diagnostics, and exposes a session-scoped `Use as main` action for active
  definitions, a `Default main agent` row for clearing the current session's
  main agent, local `.psychevo` create/update/delete, plus read-only view/run
  actions for other sources. Completed, errored,
  interrupted, and closed child agents are not listed in `/agents`; they remain
  reachable from `Agent` rows in the parent transcript. `Stop subtree` first
  requests cooperative shutdown for the selected child and descendants, waits a
  short grace window, then force-interrupts and closes any still-running child
  edge.
- `Use as main` changes the selected main-session agent for future turns in
  the current session only. It does not rewrite history, does not start a child
  run, and is unavailable for shadowed or diagnostic definitions. The selected
  main agent is restored when reopening the session; if no session setting is
  present, the TUI falls back to the startup `--agent` value, then to the
  default unselected identity. Successful `Use as main` and `Default main
  agent` actions close the `/agents` panel. The bottom status line does not
  show main-agent text; instead, the existing transcript/composer separator
  embeds the effective session identity when it is non-default.
- The session identity separator applies to every TUI session view. Root
  sessions show a label only when a non-default main agent is active. Child and
  forked agent sessions use their persisted `agent.name` as the default
  identity, so opening a `translate` child shows `translate` in the separator.
  If the user selects another main agent inside a child session, the separator
  shows that effective main agent; selecting `Default main agent` restores the
  child session's own agent identity. The label is just the agent name, without
  `main` or `Agent` prefixes.
- Opening an agent enters the original child session and preserves its identity
  and policy. The active composer follows the displayed session. Returning to
  the parent/root session uses explicit TUI navigation; child completions still
  notify the original parent while the edge remains open.
- Foreground `Agent` tool calls render as a single compact subagent block
  in the parent transcript. The collapsed row uses the callable definition name
  plus the task summary, keeps one explicit `Open` action for entering the child
  session as soon as runtime creates that child session, and shows
  status/elapsed time, child tool-use count, and the child session's latest
  assistant token usage when available. Row clicks follow the same transcript
  rule as Thinking and tool rows: clicking the row body toggles details, and
  only clicking `Open` enters the child session. In transcript focus, `Space`
  toggles details while `Enter` or `O` opens an Agent row. Parent rows may show
  a bounded live tail preview of child Thinking/tool/message activity, but never
  duplicate the full child transcript. Streaming child Thinking deltas are
  coalesced into one preview segment per contiguous Thinking block, so provider
  chunking does not create one `Thinking:` line per token or fragment. Expanded
  rows reveal the original prompt and response summary for quick inspection.
  Hidden contextual completion notifications are not rendered as separate TUI
  rows, so a foreground subagent never creates two clickable `Agent` entries for
  the same child session.
- Entering a running child agent session shows that child session's live
  Thinking, tool, and message stream using the normal session transcript
  renderer. While the parent remains active, scoped child events are buffered
  for that child session as well as summarized in the parent Agent row, so
  opening the child during a live run immediately replays the current work
  surface before future scoped events continue streaming. Child-session views
  reuse the regular composer and transcript behavior; only parent/sibling
  navigation hints are added. Parent navigation is available through `Alt+Left`
  and the mnemonic `Alt+P`.
- The child-session status line keeps the normal session context-usage segment.
  If a child session lacks its own context-limit metadata, the TUI falls back to
  the parent session metadata. The parent navigation hint is compact and is
  appended to the same right-side context/workdir/branch segment instead of
  displacing context usage.
- Long Thinking, tool, and Agent preview bodies use a middle-folding preview:
  the first 2 lines/tokens plus the last 4 lines/tokens stay visible and the
  omitted middle is represented by a compact marker. Streaming rows recompute
  the preview from full text so the trailing window updates live.
- Thinking, tool, and Agent rows share the same evidence-row behavior for
  active elapsed timing, row-level expand/collapse, and live middle-folded
  previews. Agent-specific affordances, such as `Open`, are title actions on
  top of that shared row behavior rather than a separate row interaction model.
- Running an available definition from `/agents` prompts for a task, starts a
  background fresh-context child agent, writes a concise clickable parent
  status row, and leaves the user in the parent session.
- Local `.psychevo` definition create/update forms include `name`,
  `description`, instruction body, `model`, `tools`, `permission mode`,
  `background`, and `max_spawn_depth` with a default of `0`. Compatible
  imported and built-in definitions are read-only in this slice. Additional
  legacy directory schemas are not scanned in this slice.

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
- [051 Agents](../051-agents/spec.md) defines agent definition discovery.
- [051 Subagents](../051-agents/subagents.md) defines subagent run control semantics.
