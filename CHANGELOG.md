# Changelog

## 2026-06-01

- Combined the `peval view` HTML report Step expand/collapse controls into a
  single stateful toggle button.
- Replaced the unreleased `display_blocks` direction with schema v15
  runtime-owned typed timeline items, artifacts, and bounded debug events.
- Moved Gateway snapshots, Workbench rendering, and `pevo run --format json` to
  typed timeline projections while keeping default plain CLI output unchanged.
- Moved fullscreen TUI live/reload flows onto Gateway typed events and runtime
  timeline snapshots, fixing tool-row evidence, `write_stdin` merging,
  skill-loaded status, reasoning order, and completed-turn metadata footers.
- Split browser host capabilities into `@psychevo/host`, added a generic
  Gateway IM adapter boundary, and refined the Workbench operator-shell UI with
  Playwright coverage.
- Scrubbed non-evaluation specs and changelog language so product design and
  implementation docs use Psychevo-owned terminology instead of external
  reference-project phrasing.

## 2026-05-31

- Added capability contribution snapshots and Gateway source/thread binding,
  queue/control, permission, clarify, and source-lifetime foundations.
- Added `pevo serve`, `pevo gateway`, the managed Web Workbench, and generated
  Gateway protocol packages for Rust, TypeScript, and JSON Schema.
- Updated capability extension ADR/specs and Gateway/Web validation around
  source identity, selection snapshots, peer-agent boundaries, and typed hook
  evidence.

## 2026-05-30

- Made `edit`/`write` LSP diagnostics best-effort with background client reuse,
  managed npm installs, and no hot-path `npx`.
- Added multi-path `peval view` comparisons and lightweight manual notes,
  including report-level notes, Trial notes, path variants, and selection-hash
  output paths.
- Reworked `peval view` HTML around selected-Trial inspection with clickable
  comparison rows, no report-wide Evidence Ledger, compact tables, Trial-only
  notes/analysis/evidence, improved Step rails, expand/collapse controls, and
  one-decimal timing displays.
- Fixed multi-Trial `peval view` cells so users can switch sibling Trials and
  flat comparison rows average repeated-Trial token and cost values.
- Fixed ACP tool execution timing by carrying runtime timings through ACP
  `_meta`, preserving timing source in view JSON, and rendering `tool exec`
  inline with tool names.
- Refactored `peval view` schema v17 around role-based includes
  `core,comparison,annotations,attachments`, standard ATIF trajectories, and
  peval-only `trajectory_meta` sidecars.
- Fixed clippy blockers across ACP eval, trajectory metadata, prompt assembly,
  and view tests.

## 2026-05-29

- Added configurable pevo project context discovery, `pevo run` overrides, and
  shared runtime prompt workdir handling across CLI, TUI, agents, compaction,
  and ACP paths.
- Added ACP-profile evaluation adapters, a Rust-native Docker Compose runner
  for Harbor/Terminal-Bench tasks, and a `peval init` template built around host
  Pidx plus `psychevo-acp`.
- Split `psychevo-eval` into schema/store, runner, view, CLI, and lifecycle-test
  modules while keeping `peval` behavior and report schemas unchanged.
- Evolved `peval view`/`peval serve` through report schemas v8-v12 with
  transparent ACP trajectories, grouped transcript steps, leaderboard-driven
  heatmaps, de-duplicated evidence, compact Trial panels, and HTML/JSON reports.
- Refined schema v12 trajectory metadata and HTML with corrected Step spans,
  separated model/tool timing, message-only collapsed previews, cleaner Step
  metrics, clearer tool counts, and model labels from ACP runtime metadata.
- Added `peval env create` and `peval env verify` for local human-in-loop task
  environments, plus local HTML workbench prototypes for the next report UI.
- Expanded `peval` with typed source manifests, Harbor-style task directories,
  schema v8 Trial/MatrixCell views, local `serve`, and cached Trial analysis.
- Tightened ACP, auth, provider, and exec-permission surfaces, including
  protocol V1 reporting, `no_auth` providers, and structured exec rules.
- Fixed fullscreen TUI yielded command rows to keep their original command title
  across output and poll updates.

## 2026-05-26

- Added normalized `last-provider-response` exports, default workspace
  `web_fetch` access, and TUI fixes for permission approvals and orphaned tool
  rows after restart.
- Rebuilt `psychevo-eval` around service-backed benchmark, eval config, and
  registry resolution with `benchmark.toml`, manifest v4, artifact v6, and view
  schema v4.
- Changed `peval run` to write reusable cell facts under
  `runs/<benchmark>/<agent>/<task>/<cell-key>/`, with `peval view` as the
  reporting surface and `--overwrite` for selected reruns.
- Removed legacy project, suite, run-root, cache, dashboard, task-local script,
  and live-smoke helper surfaces in favor of task sets, evaluators, eval
  configs, and workspace/user registries.
- Added the `pidx-coding` benchmark, deterministic generated test projects,
  black-box CLI coverage, and fake OpenCode/Hermes wrapper adapter tests.
- Removed `pevo smoke`; deterministic validation now uses test harnesses and
  live evaluation uses explicit `peval` eval configs through selected adapters.
- Updated evaluation specs, docs, README, and repo-local peval dev workspace
  guidance for `.local/.psychevo-dev` and `.local/evals-dev`.

## 2026-05-25

- Reworked permission approvals around the new profile-based config schema,
  fail-closed runtime prompts, persistent project-local grants, TUI approval
  panels, ACP/CLI approval parity, and VHS coverage for the approval panel.
- Added `/diff` across TUI and ACP with shared workspace diff collection,
  structured ACP output, a read-only TUI pager, and deterministic VHS coverage.
- Upgraded local state storage to schema v12 with durable semantic display
  blocks and reset guidance for older state databases.
- Migrated evaluation fixtures from `local-rust-swe` to `local-coding` with
  coding-loop, prompt A/B, and SWE-style task families plus richer diagnostics.
- Added evaluation user docs, a `scripts/install.sh --with-peval` install path,
  and moved live smoke validation to `scripts/eval/live-psychevo-smoke.sh`.
- Split oversized Rust modules into responsibility-named submodules, replacing
  the temporary numbered split files without changing public APIs or command
  behavior.
- Reused injected skill bodies for explicit `$skill` prompts instead of asking
  the model to reload `SKILL.md`.
- Fixed transient TUI turn metadata rows appearing after completed tool output
  before the final assistant message.
- Added shared `StateRuntime` plumbing for runtime, TUI, and ACP paths, reducing
  idle TUI agent reload and WAL checkpoint churn.
- Fixed scoped default-model writes for CLI/TUI model selection, including
  explicit reasoning variants.
- Normalized `edit` tool diffs to Git patch blocks and rendered completed
  inline edit rows with a single line-number gutter while keeping
  `/diff` on its existing dual-column diff display, with deterministic VHS
  coverage for the inline edit row.
- Treated unknown slash-looking TUI input, including absolute paths, as normal
  prompt text while preserving local errors for malformed known commands.

## 2026-05-24

- Added the first `psychevo-eval`/`peval` slice and unified evaluation
  workbench: `peval init`, `$PSYCHEVO_HOME/peval.toml`, `--config/-c`,
  `--root`/`PEVAL_ROOT`, run indexes, latest selectors, dataset inventory,
  static dashboards, live `pevo run --format json` trajectory capture, and
  bounded fake/live smoke validation.
- Added ACP setup docs, testing specs for clarify/compaction/ACP packaging, and
  a README refresh for the current CLI/TUI/ACP surfaces.
- Made scoped config writes workdir-local by default, with `-g`/`--global` for
  global writes, and removed legacy project-scope aliases.
- Fixed the `psychevo-eval` HTML report lint failure under broad validation.
- Cleaned active spec attachment links, retired archived 130-permissions docs,
  and documented the attachment label rule.
- Hardened the fullscreen TUI running-child elapsed test against clock drift.
- Split TUI capture fixtures and mock provider out of the capture script.

## 2026-05-23

### Added

- Added initial ACP stdio support via `psychevo-acp` and `pevo acp`, including
  sessions, cancellation, command projection, provider auth metadata, and
  session-scoped MCP tools.
- Added `027 ACP`, `056 MCP`, and `230 pevo-acp` specs for ACP protocol
  semantics, MCP boundaries, and server packaging.
- Added the canonical Psychevo logo under `assets/` and displayed it in the
  README.
- Added ADR 0002 for the capability extension mechanism.
- Added `web_fetch`, `pevo tool`, and fullscreen TUI `/tools` support for
  built-in and project-local toolsets.

### Changed

- Moved slash-command parsing, availability, and UI-independent effects into a
  shared runtime command path for ACP, TUI, and future text surfaces.
- Reworked Plan and Default tool surfaces around core shell/file tools, the
  adjacent `web` toolset, and managed `rg`/`jq` guidance.
- Renamed the internal editable run mode from `Build` to `Default`.
- Unified TUI slash-command handling and tool evidence rendering for built-in
  and extension tools.
- Removed legacy `list`/`search` coding-tool surfaces and compatibility paths.

### Fixed

- Fixed ACP slash-command parity with capability-filtered advertisements.
- Fixed fullscreen TUI composer, slash-command, paste, and scroll-follow edge
  cases.

## 2026-05-22

### Added

- Added expanded skill management: aggregate tools, richer skill metadata,
  scoped bundles, and dynamic `/<skill-or-bundle>` TUI insertion.
- Added live steer and next-turn queue support across core, runtime, and
  fullscreen TUI, including `/steer`, `/queue`, `/pending cancel`, and fixed
  pending previews with edit/undo controls.

### Changed

- Switched user-editable config and skill bundles to TOML, with legacy config
  and bundle formats ignored by runtime.
- Reworked skill commands, discovery, platform gating, collisions, and scoped
  installs around the singular `pevo skill` command family and TUI hub flows.

### Fixed

- Filled in missing model-visible parameter descriptions across runtime tools.
- Fixed fullscreen TUI Thinking and pending-input rendering edge cases.

## 2026-05-21

### Changed

- Updated core tool contracts for `read`, `edit`, `write`, `exec_command`, and
  `write_stdin`, covering pagination, patch operations, diagnostics, yielded
  sessions, stdin/PTY, bounded output, and permission metadata.
- Moved runtime prompt text into embedded Markdown templates without changing
  prompt assembly semantics.
- Removed external reference-project names from user-visible coding-core and
  changelog wording.

### Fixed

- Fixed TUI rendering for yielded `exec_command` sessions and empty
  `write_stdin` polls.

## 2026-05-20

### Added

- Added fullscreen TUI `/btw` side conversations with hidden temporary side
  sessions, `/side` compatibility input, and `/refresh` cleanup.
- Added runtime context compaction with checkpoints, TUI
  `/compact [instructions]`, automatic compaction, and a `pevo run` overflow
  retry.

### Changed

- Changed subagent model-visible results to compact summaries while preserving
  full runtime metadata for diagnostics, TUI, and exports.
- Removed external reference-project names from user-visible TUI interaction
  specs.
- Promoted runtime permissions to foundation `035-permissions`, archived the
  superseded `130-permissions` topic, and cleaned up active spec links.
- Completed the design-system spec's required scope and related-topic sections.

### Fixed

- Fixed TUI IME anchoring, running metadata visibility, and automatic
  compaction scheduling.

## 2026-05-19

### Added

- Added the fullscreen TUI `clarify` tool, including typed runtime request
  events, control-handle responses, and deterministic panel captures.
- Added composer text selection with `Ctrl+A` and mouse drag editing.

### Changed

- Changed background Agent completion to use mailbox events and timeout-only
  `wait_agent` status, and removed external project comparisons from
  user-visible docs.
- Changed `clarify` prompts to use structured question progress, inline notes,
  and ordered answer results without model-authored question ids.
- Improved TUI sessions, selection, Status rows, and tool evidence titles to
  match the compact ledger design.

### Fixed

- Fixed `clarify` panel navigation, inline Other answers and notes, mouse
  selection, transcript reloads, and result rendering.
- Fixed `last-provider-request` reconstruction so it includes `clarify` when
  the persisted effective tool surface used it.
- Fixed TUI session-panel wrap coverage, plan-mode validation for `clarify`,
  interrupted reasoning metadata, and ongoing tool-failure metadata.
- Fixed selected-text copy over SSH by using terminal-mediated clipboard
  forwarding.
- Fixed last-provider-request reconstruction so background subagent final
  answers appear once.

## 2026-05-18

### Added

- Added effective `config.jsonc` TUI slash aliases and shortcuts, including
  completion rows and `/help` entries.
- Added v1 runtime permissions for tool execution, including `Tool(pattern)`
  permission rules, `default`/`acceptEdits`/`dontAsk`/`bypassPermissions`
  modes, CLI approvals, `/permissions`, and project-local rule management.

### Changed

- Made agent tool policy allowlist-based, including inherited omissions,
  empty-array disablement, agent catalog filters, skill visibility,
  `projectInstructions`, developer/system prompt placement, and no-tools
  prompts.
- Moved session export prompt-prefix metadata into artifact headers,
  reconstructed last-provider-request from persisted prefix snapshots, and
  added `-f` aliases for format flags.
- Split `210-pevo-tui` into rendering and interaction specs, with focused
  attachments and topic-owned testing notes.

### Fixed

- Fixed fullscreen TUI running-session switching so background agent and shell
  output stays scoped to the visible session.
- Fixed fullscreen TUI running status for visible parent and child sessions,
  settled agent turns, and shared ledger-row spinner state.
- Fixed Agent transcript rows to reuse streaming placeholders, restore
  reloaded parent history with `Open`, and keep the `Open` hit target separate
  from expand/collapse clicks.
- Fixed long Thinking rows so expanded preview-collapsed rows can collapse back
  to title-only details.
- Fixed last-provider-request and child-agent exports to verify prompt-prefix
  hashes and disclose persisted prefix, agent catalog, skill index, and
  prompt-scoped evidence snapshots.
- Lengthened default export/share artifact session-id prefixes so sibling
  parent and child sessions do not overwrite the same path.

## 2026-05-17

### Added

- Added typed prompt assembly for stable prefix slots, selected agents,
  provider role fallback, prefix snapshots, and context reload commands.

### Changed

- Split context usage accounting into base, developer, project, history, turn,
  prompt, tools, and free-space categories.

### Fixed

- Included selected-agent descriptions in main-session and child-agent prompts.
- Let `Esc` interrupt inspected child-agent work from child or parent session
  views.

## 2026-05-16

### Added

- Added singular `pevo session`, `pevo model`, `pevo config`, and `pevo auth`
  command families with JSON output, scoped config/auth writes, and provider
  `/models` fetching.
- Added AGENTS project instruction loading for live runs, including
  `.psychevo/AGENTS.md`, `AGENTS.local.md`, context evidence, context-usage
  details, and non-fatal legacy memory migration warnings.
- Added local session export/share artifacts across `pevo session
  export/share` and TUI `/export` `/share`, including Markdown/JSON export,
  opt-in reasoning, provider-input evidence bundles, and sensitive include
  flags such as `last-provider-request`.
- Added first-class agent definitions and child-agent control across runtime,
  CLI, and TUI, including discovery, selected-agent policy, parent-child
  session edges, `/agents`, `/fork`, `pevo agent inspect`, depth controls, and
  session-scoped main-agent switching.

### Changed

- Expanded CLI and TUI discovery copy across `pevo --help`, subcommand help,
  slash `/help`, slash menu summaries, and picker footers so local writes,
  provider calls, skill selection, JSON output, stdin secrets, and sensitive
  export includes are easier to understand.
- Changed hidden project instructions, selected-skill context, and
  export/share content selection to preserve provider boundaries and use exact
  include sets.
- Renamed the CLI skill command family to canonical `pevo skill`, removed the
  obsolete `pevo skills` form, and migrated skill scope flags from `--project`
  to `--local`.
- Changed TUI agent observability to use explicit `Open` actions, shared
  expand/collapse behavior, live head-and-tail previews, transcript keyboard
  focus, and deterministic screenshot coverage.
- Changed Agent tool identity to use canonical `agent_type` definitions, keep
  `name` as a compatibility alias, reject conflicts or unknown explicit names,
  and persist resolved main-agent metadata per assistant turn.

### Fixed

- Fixed `pevo tui` session resume so the bottom status line restores compact
  context-window usage from persisted provider input usage and session context
  limits before the first redraw.
- Fixed subagent inspection so foreground runs keep one parent Agent row,
  running child sessions stream live work when opened, parent previews update
  without duplicate child rows, streamed Thinking chunks coalesce cleanly, and
  latest child tool/token usage remains visible.
- Fixed agent discovery diagnostics and `@agent-name` completion so invalid
  definitions surface visibly and matching agent names take priority at `@`.

## 2026-05-15

### Added

- Added the 080 Design System spec for `pevo tui`.

### Changed

- Reworked `pevo tui` around the V1 design-system direction, including
  composer/status/popup surfaces, evidence wording, `Ctrl+T` reservation, and
  updated snapshots.
- Matched the `pevo tui` user-shell transcript `!` marker color to the
  shell-mode composer marker, and rendered shell transcript rows as prompt
  rows.
- Increased the built-in coding-agent model-turn budget to 128 and surfaced a
  specific diagnostic when that budget is exhausted before a final answer.
- Aligned `pevo tui` Thinking evidence title/marker colors with tool evidence
  rows and added display-token collapse for long evidence bodies, including
  dense table, unbroken output, and oversized line-count previews.
- Changed `pevo tui` shell escapes to require resolvable model/provider config,
  persist bounded `<user_shell_command>` user context, and inject auxiliary
  shell results into active agent turns without exposing `bash` in plan mode.
- Changed `pevo tui` shell mode so `!` is composer state instead of textarea
  text, reuses `@file` completion for shell commands, defers auxiliary shell
  work until the foreground agent session is known, and lets empty shell mode
  exit with `Backspace`.

## 2026-05-13

### Fixed

- Fixed Chat-compatible interleaved reasoning replay for explicit
  `reasoning_content` metadata and `reasoning=true` default fallback.

## 2026-05-12

### Added

- Added TUI raw Markdown viewing/copying with `/show-raw`, `/copy`, and
  `Ctrl+O`, plus improved tables, code blocks, and links.
- Added durable context evidence for injected system instructions and selected
  skills outside the transcript.
- Added TUI troubleshooting docs for terminal and tmux mouse reporting.

### Changed

- Changed TUI image attachments to use `[Image #N]` placeholders from
  `/image <source> [prompt]` or standalone readable image-source paste.
- Changed persisted image prompts to keep submitted composer text as
  `content_text` and store attachment display metadata.

### Fixed

- Fixed `/sessions` recency so viewing, selecting, restoring, or resuming a
  session does not rewrite latest activity.
- Fixed image attachment feedback for missing paths, `/image` errors, sent
  metadata, and `/new` cleanup.
- Fixed interrupted and timed-out `bash` evidence rendering.
- Fixed mouse-wheel routing inside the fullscreen alternate screen.
- Fixed prompt-history `Up`/`Down` behavior at the empty-input boundary.

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

### Fixed

- Fixed `bash` abort and timeout cleanup so Esc terminates foreground command
  process groups and pipe collection does not hang.
- Fixed terminal cleanup and alternate-scroll handling so mouse wheel input
  stays inside the fullscreen app.

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
