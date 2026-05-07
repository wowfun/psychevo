# Changelog

## 2026-05-07

### Changed

- Split `psychevo-runtime` and `psychevo-cli` internals into focused Rust
  modules while preserving the existing runtime re-exports and `pevo` behavior.
- Added tool-call timing to runtime streams and persistence, with TUI evidence
  rows that keep actual `bash` titles and stable live/completed durations.
- Tightened fullscreen TUI session, sidebar, history, `/new`, title-generation,
  and wide-character picker behavior.
- Improved fullscreen TUI text selection and clipboard handling from rendered
  transcript/sidebar rows, including active highlights and Linux fallbacks.

## 2026-05-06

### Changed

- Reworked fullscreen `pevo tui` session/model flows around searchable bottom
  panes, session titles, `/rename`, `/show-thinking`, and removed commands.
- Refined TUI ledger rendering, sidebar/composer chrome, prompt surfaces,
  visible thinking, metadata, usage/context display, and visual snapshots.
- Improved fullscreen TUI keyboard, mouse, auto-follow, long-output, and
  selected-text copy behavior across transcript, sidebar, and slash menus.
- Required VHS screenshot review for fullscreen TUI visual display changes,
  backed by deterministic local capture artifacts.

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
