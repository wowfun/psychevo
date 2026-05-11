# Changelog

## 2026-05-11

### Added

- Added a POSIX/Git Bash source install helper with `pevo` verification and
  optional home initialization.
- Added context-window inspection across `pevo context`, TUI `/context`, and
  best-effort JSON `context_snapshot` events with shared runtime snapshots and
  tokenizer-backed estimates.
- Added the shared command contract and registry-backed CLI/TUI command help,
  discovery, aliases, command feedback rows, and skill-command summaries.
- Added a tabbed fullscreen `/model` metadata view plus explicit metadata
  refresh and pruned `models.dev` cache warmup.

### Changed

- Made TUI `/usage` canonical, kept `/stats` as an alias, and expanded local
  usage/cost reporting with token, cache, model, tool, session, and pricing
  details.
- Tightened model metadata configuration by requiring `limit.context`, keeping
  `pevo run` cache-only, and lengthening explicit `models.dev` refreshes.
- Refined fullscreen TUI context/status rendering, command transcript rows,
  tabbed help, stable markers, and compact metadata presentation.

## 2026-05-10

### Added

- Added the public-alpha root README, MIT license file, and contribution guide.
- Added the 070 Experience foundation spec for cross-cutting UX/DX defaults and
  ownership boundaries.
- Added adaptive TUI theme, terminal-palette probing, static motion fallback,
  and lightweight Markdown answer projection for fullscreen `pevo tui`.
- Added a fullscreen TUI `/model` custom-provider flow with global
  OpenAI-compatible providers, `.env`-only API keys, fetched model catalogs,
  TUI-scoped model selection, and display-only provider labels.
- Added cache-first `models.dev` model metadata resolution for context limits,
  capability tags, and pricing, plus SQLite usage accounting and `pevo stats`
  / TUI `/stats` summaries.

### Changed

- Clarified specs guide numbering and topic directory rules, including required
  `testing.md` files for `100+` specs.
- Refreshed fullscreen TUI rendering with adaptive surfaces, lightweight
  Markdown, row-level `Thinking`/tool folding, shared evidence rows, single-line
  transcript focus, and expandable long command/output details.
- Enriched TUI model rows, turn metadata, `/status`, and sidebar copy with
  resolved model metadata, session cost, and clearer `tool calls` labeling.
- Improved fullscreen TUI running-state and tool-evidence projection with
  compact elapsed labels, active tool rows, and safer interrupt queue handling.

### Fixed

- Fixed fullscreen TUI active evidence timing, placement, titles, elapsed
  durations, interruption reloads, and provisional `Changing`/`Running` rows
  for provider-side tool-input gaps and instant completions.
- Fixed turn metadata and bottom scrolling across intermediate tool-call
  messages, reasoning-only final messages, long Markdown/table transcripts, and
  continued `Thinking` streams.
- Fixed sidebar context tokens and redraw clearing so usage stays visible while
  the model answers and stale terminal glyphs cannot corrupt labels or blank
  rows.
- Fixed TUI active tool refresh/keying, skill-first session titles, normal
  tool-failure projection, prompt interruption wakeups, multi-answer
  preservation, model-turn budget, and Esc priority.

## 2026-05-09

### Added

- Added explicit fullscreen TUI `/model` catalog fetching from selectable provider rows, with process-local fetched model caching and fetched model selection through the existing variant flow.
- Added TUI `@` file completion, `/sessions` archive/delete actions, `!`
  shell escapes, and first-class Agent Skills support across runtime, CLI, and
  TUI.

### Changed

- Removed bottom selection pane subtitles, updated `/model` slash help text to reflect model selection and fetching, and made bottom selection pane Up/Down navigation wrap between first and last rows.
- Added fuzzy slash menu matching while keeping Tab completion prefix-only.
- Added skill marker invocation, SQLite schema v4 archive state, focused
  source/spec splits, and cleanup of obsolete plan docs.

### Fixed

- Fixed TUI transcript auto-follow, scroll performance, and mouse
  responsiveness.

## 2026-05-08

### Added

- Added resolved assistant variant metadata to TUI turn rows, restored from persisted per-message metadata beside the model name.

### Changed

- Removed the TUI `/help` slash command, simplified `/variant` and `/mode` value-setting syntax, aggregated `/status` output, improved slash menu hint text, and made slash menu Up/Down navigation wrap between first and last row.

## 2026-05-07

### Added

- Added snapshot-backed `pevo tui` `/undo` and `/redo` commands that restore Git-tracked file state, hide/restore message ranges, and clean reverted messages before the next prompt.
- Added tool-call timing to runtime streams and persistence, with TUI evidence rows that keep actual `bash` titles and stable live/completed durations.

### Changed

- Split `psychevo-runtime` and `psychevo-cli` internals into focused Rust modules while preserving the existing runtime re-exports and `pevo` behavior.
- Tightened fullscreen TUI session, sidebar, history, `/new`, title-generation, and wide-character picker behavior.
- Improved fullscreen TUI text selection and clipboard handling from rendered transcript/sidebar rows, including active highlights and Linux fallbacks.

## 2026-05-06

### Changed

- Reworked fullscreen `pevo tui` session/model flows around searchable bottom panes, session titles, `/rename`, `/show-thinking`, and removed commands.
- Refined TUI ledger rendering, sidebar/composer chrome, prompt surfaces, visible thinking, metadata, usage/context display, and visual snapshots.
- Improved fullscreen TUI keyboard, mouse, auto-follow, long-output, and selected-text copy behavior across transcript, sidebar, and slash menus.
- Required VHS screenshot review for fullscreen TUI visual display changes, backed by deterministic local capture artifacts.

## 2026-05-04

### Added

- Added `pevo init` and `pevo run [message..]` with JSONC provider config, `.env` loading, SQLite state, JSON/default output, `--variant`, and `--continue`.
- Added live-provider specs and deterministic mock SSE/CLI coverage for prompts, tool calls, JSON errors, removed flags, session continuation, plus ignored live-provider tests.
- Added folded local reasoning blocks, opt-in JSON reasoning events, `pevo init --reset-state`, and repo-local live dev tooling.
- Added fullscreen `pevo tui` with evidence-ledger turns, composer/sidebar, persistent TUI state, slash commands, `--debug`, transcript/tool expansion, and non-TTY scripted mode.
- Added TUI `plan` / `default` modes plus automation and visual-regression specs.
- Added TUI visual snapshots and a VHS diagnostic capture workflow.

### Changed

- Hardened OpenAI Chat-compatible SSE parsing across byte chunk boundaries, UTF-8 splits, line ending variants, provider stream errors, and premature EOF.
- Replaced `PSYCHEVO_CONFIG_DIR` with `PSYCHEVO_HOME`, `PSYCHEVO_CONFIG`, and `PSYCHEVO_DB`.
- Converted live `pevo run` from low-level provider flags to the positional prompt interface.
- Moved state to SQLite schema v3 with single-copy reasoning in `message_json`, normalized usage/metadata columns, persistent TUI thinking visibility, and sanitized JSON/session projections.
- Simplified fullscreen TUI chrome, history loading, and keyboard behavior.

## 2026-05-03

### Added

- Added the first Rust workspace slice, implementation-contract attachments, deterministic `pevo smoke`, crate-local tests, and `scripts/validate.sh`.
- Added foundation specs for storage/persistence and capability extensions, including first-slice session/message and SQLite persistence attachments.
- Added capability specs for the built-in `coding-agent` and its default `coding-core` toolset.

### Changed

- Expanded the SQLite persistence attachment with the first-slice internal sessions/messages column contract and ordering rules.
- Reworked foundation specs around session-centered state, agent-invocation assembly, `agent_start`/`agent_end`, and evidence-backed result retrieval.
- Connected runtime, evidence, context, tool, resource, memory, interface, persistence, and extension specs around refreshable tool snapshots, extension selection, and SQLite-backed first-slice persistence.
- Clarified the first coding-agent slice: runtime-owned working-context resolution, default `coding-core`, specialized-mode exceptions, and deferred discovery, skills, and memory behavior.
- Expanded `AGENTS.md` with deterministic, isolated validation guidance.

## 2026-05-02

### Added

- Added `020-interfaces` and `030-state-and-data-model` foundation specs.
- Added foundation specs `006` through `010` for context assembly, tool surface, session continuity, resource surface, and memory system boundaries.

### Changed

- Linked foundation specs to the new context, tool, session, resource, and memory source-of-truth documents.
- Linked foundation specs to state/data model and interface boundaries.
- Simplified `004` through `007` to remove template-driven repetition.
- Deferred standalone workspace policy and generalized it into resource-surface wording.

## 2026-05-01

### Added

- Added foundation specs `001` through `005` for architecture, agent execution, AI protocol, runtime contract, and durable evidence.

### Changed

- Linked early specs around architecture, execution, AI protocol, runtime, and durable evidence boundaries.
- Simplified opening purpose paragraphs and removed external references from stable specs.
