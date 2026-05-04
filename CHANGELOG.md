# Changelog

## 2026-05-04

### Added

- Added `pevo init` and OpenCode-style `pevo run [message..]` with JSONC provider config, `.env` loading, SQLite state, JSON/default output, `--variant`, and `--continue`.
- Added live-provider specs and deterministic mock SSE/CLI coverage for prompts, tool calls, JSON errors, removed flags, session continuation, plus ignored live-provider tests.

### Changed

- Hardened OpenAI Chat-compatible SSE parsing across byte chunk boundaries, UTF-8 splits, line ending variants, provider stream errors, and premature EOF.
- Replaced `PSYCHEVO_CONFIG_DIR` with `PSYCHEVO_HOME`, `PSYCHEVO_CONFIG`, and `PSYCHEVO_DB`.
- Converted live `pevo run` from low-level provider flags to the positional prompt interface.

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
