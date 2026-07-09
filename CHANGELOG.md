# Changelog

## 2026-07-09

- Added Runtime Profiles across config, Gateway/protocol, Workbench, Channels,
  and session controls; direct non-native turns now fail explicitly until
  adapter workers are available.

## 2026-07-08

- Added multi-agent Teams and Missions with Markdown team templates, durable
  run metadata, `/mission`, and team-aware `spawn_agent` across Gateway, ACP,
  channel, and TUI flows.
- Added Workbench team management and mission controls, including Capabilities
  editing, Team workspace actions, and team/mission/member status labels.
- Moved Workbench agent, team, and ACP backend management under
  `Capabilities > Agents`, with local ACP backend auto-creation for detected
  `opencode acp` and `hermes acp` commands.
- Improved `peval-py serve` Source Manager imports, source-list scrolling, and
  Leaderboard-to-step details.

## 2026-07-06

- Added Vision V1 with safe image inputs, generated media artifacts, and
  Workbench transcript/composer previews.
- Added inspected plugin imports with npm materialization, adapter trust,
  catalog RPCs, CLI commands, and Workbench controls.
- Moved Agent management into `Capabilities > Agents` and unified Agent/Skill
  Markdown definition editing, previews, and Project/Profile writes.
- Split Workbench and Desktop production chunks through Rolldown code-splitting
  groups so runtime output stays below Vite's default large-chunk warning.
- Added voice ASR/TTS policy, fake providers, Gateway and realtime RPCs,
  Workbench controls, and `/voice` commands with text fallback.
- Reworked `peval-py serve` source state, Source Manager imports, and
  Leaderboard filtering around cell-local overlays and workspace logs.
- Tightened `peval-py serve` startup discovery, artifact-only cells, import
  provenance handling, and inline Source Manager/Leaderboard controls.
- Refined Workbench voice controls and shell scrolling so dictation stays in
  the composer flow and pinned sessions remain contained.

## 2026-07-05

- Added shared design-system assets, switch/form primitives, and Markdown
  preview affordances used by Workbench, Floating, and Capabilities surfaces.
- Improved skill and agent discovery labels, grouped composer completions, and
  protected the built-in `coding-core` toolset from management writes.
- Consolidated Workbench create/install/connect panels and fixed their desktop
  and narrow-viewport placement.
- Hardened Desktop startup, native smoke, WSLg sizing, app icon generation,
  managed Gateway reuse, and Desktop live-suite coverage.
- Reworked Desktop Floating submission and transcript handling around shared
  Workbench thread controls, including same-thread focus, close behavior, and
  first-visible response timing artifacts.

## 2026-07-04

- Added Workbench Capabilities management for Skills, Plugins, MCP, and Tools,
  including domain Gateway RPCs, profile-scoped mutations, OAuth plumbing, and
  strict catalog/read/uninstall behavior.
- Simplified the Workbench Skills management UI around search-first rows,
  inline switches, shared Markdown previews, and an internally scrolling detail
  panel.
- Refined `peval-py` report and serve comparison views with Leaderboard
  Summary, archived-source controls, batch Archive/Activate actions, and
  consistent single-row behavior.
- Split `peval-py` CLI, serve, state, analysis, report, HTML, JS, and CSS
  internals into focused modules while preserving public imports.
- Added `pevo desktop` source-checkout launching plus Desktop/Floating Linux
  and WSLg capture, visual, live, Gateway, and authenticated-download support.

## 2026-07-03

- Added the first Psychevo Desktop and Floating scaffold with native Gateway
  transport, Workbench reuse, attachment mapping, and focused package tests.
- Added `pevo mcp serve` and expanded MCP tool policy, snapshot, and strict
  reply-schema support.
- Improved Windows Git Bash process handling across install, Gateway/Web
  terminals, ACP, hooks/plugins, LSP, PATH lookup, UTF-8 defaults, and cleanup.
- Added `pevo web start`, `pevo web stop`, and `pevo web restart`, and moved the
  starter `config.toml` into a compiled template with a default `/expr` alias.
- Improved `peval-py view/export tr -p` Trial cell path handling and inspect
  selectors, including `--steps` and bounded previews.
- Fixed shared file-path normalization so literal `#` and `?` remain valid in
  filesystem paths while URL query and fragment parts are still stripped.

## 2026-07-02

- Added Codex-grounded MCP catalog and runtime snapshot plumbing with canonical
  tool identities, profile and selected-root MCP server inputs, deferred
  `tool_search` loadable specs, global resource/prompt utility tools, and
  bounded sampling/elicitation policy metadata.
- Reworked Workbench Models settings into a vertical default/title/compression
  assignment flow, editable available-provider rows, per-model metadata saves,
  and the current `name`/`api` provider schema across setup, auth, TUI, ACP,
  visual fixtures, and explicit `api_key_env` overrides.
- Improved `peval-py serve` source handling so workspace Trial cells are
  discovered on startup or source reload, missing cell artifacts stay visible
  without breaking page load, and source actions load only the selected report.
- Fixed Workbench Terminal tabs in light and warm appearances so xterm uses a
  readable light background and ANSI palette instead of the default black
  terminal surface.
- Fixed Windows Git Bash cwd display, `exec_command` cwd metadata, and
  OpenCode ACP launches by normalizing Windows verbatim paths and resolving
  `PATHEXT` shims before Gateway starts peers.
- Fixed Workbench side-thread submission and transcript recovery so temporary
  source tabs close cleanly, yielded `exec_command` blocks preserve identity
  and titles, and stale live overlays cannot downgrade completed tool rows.
- Added semantic transcript runtime ledgers across Gateway, Web, TUI, and
  Workbench validation to catch duplicate, stale-overlay, status-downgrade, and
  pseudo-running regressions before visual review.
- Added a dedicated installation guide and simplified `scripts/install.sh` to a
  checkout-local install surface with sharper diagnostics for Windows Git Bash,
  enterprise networks, pnpm/Corepack, and Cargo fetch failures.
- Fixed native Windows source installs and release builds by keeping Linux-only
  Landlock and Unix-only helper code out of Windows builds.
- Unified the workspace `reqwest` dependency on `0.13.3` across Runtime,
  Gateway, CLI, and provider HTTP clients.

## 2026-07-01

- Added the canonical Runtime-owned event stream and Gateway action lifecycle,
  including `SessionConfigured`, unified `pendingActions`, and snapshot-first
  reconnect semantics.
- Added source-aware extension assembly for plugins, MCP servers, runtime tools,
  toolsets, hooks, child agents, and Hermes-compatible skill/tool activation
  hints.
- Added Codex-compatible plugin interface metadata, read-only Gateway plugin
  RPCs, CLI plugin summaries, plugin MCP server materialization,
  `PostToolUse` result transforms, and source-qualified hook trust review.
- Added contributor placement guidance for choosing between core runtime,
  skills, agents, hooks, plugins, and owning extension surfaces.
- Added runner-owned xtask entrypoints for deterministic visual and ACP server
  live validation with CI Playwright screenshots and quieter worker color logs.
- Fixed Workbench transcript projection for ordinary `exec_command` result
  displays and failed `write_stdin` updates that target yielded command
  sessions.
- Improved `peval-py` HTML comparison/report UX with stable selection scroll
  positions, `Analysised` filtering, `.xlsx` table export, synced comparison
  scrolling, active-source analysis reloads, and richer cached `analysis.md`
  rendering.

## 2026-06-30

- Fixed TUI ordering for selected-skill turn notices so `skill loaded` rows stay
  before turn work and committed answers.
- Added the Codex-compatible extension upgrade with selected capability roots,
  the runtime extension registry, Psychevo plugin metadata, and package-based
  plugin enablement.
- Added the first Windows Git Bash compatibility slice: runtime path
  normalization, Git Bash shell launch contracts, native cwd persistence, string
  path protocol fields, and Gateway/Workbench input normalization.
- Hardened capability-root path handling so malformed manifests fail closed,
  relative permissions use decoded paths, and host path parsing preserves
  whitespace.

## 2026-06-29

- Hardened `scripts/install.sh` for Windows Git Bash and enterprise networks
  with check/offline/web-dist modes, version and build-tool preflights, repair
  prompts, and proxy/registry/CA diagnostics.
- Documented Windows compatibility lessons from Codex, Hermes Agent, and
  opencode, covering native shell selection, Git Bash discovery, and
  MSYS/Cygwin/WSL path normalization boundaries.
- Fixed Workbench live transcript duplication during tool-call and child-agent
  turns by treating running assistant updates as snapshots and keeping parent
  activity ownership stable.
- Completed the production large-file cleanup by splitting Rust runtime,
  Gateway, CLI/TUI, Workbench, protocol, and `peval-py` monoliths into focused
  modules while preserving public interfaces and behavior.
- Kept hook/plugin runtimes, channel/IM adapters, live and committed transcript
  projectors, settings/observability, automations, session views, RPC dispatch,
  and generated protocol schemas on stable public surfaces after the split.
- Split Workbench and shared transcript code/tests into narrower runtime,
  settings, automation, transcript, e2e, and tool-detail modules, and fixed
  shared live validation registration after the e2e split.
- Split TUI state, rendering, and tests for turn lifecycle, markdown, evidence
  ledger, stream events, composer/popups, model/session panels, and live tool
  reconciliation without changing TUI behavior.
- Split `peval-py` state, inspection, input/report assets, tests, and specs
  into focused modules and normative detail docs while preserving CLI/report
  behavior.
- Fixed `peval-py` workspace snapshot path handling so Git Bash and WSL users
  can pass accessible Windows drive paths for Trial cell artifacts and saved
  workspace snapshots without cwd-relative rewrites.
- Documented `cargo xtask doctor large-files` remediation expectations so
  future large-file work follows semantic module boundaries.

## 2026-06-28

- Added local-first `cargo xtask ci`, `live`, `init dev-env`, and `doctor`
  workflows for deterministic checks, live validation planning, repo-local
  state, dependency diagnostics, artifact cleanup, visual capture, and
  large-file inventory.
- Replaced legacy live, visual, dependency, and large-file shell helpers with
  repo-owned `xtask` commands while keeping `scripts/install.sh` as the
  standalone product installer.
- Made browser sessions profile-global, replaced internal/wire `workdir`
  naming with `cwd`, and kept Settings, Models, Slash settings, and
  Automations as global management surfaces while execution paths carry an
  explicit cwd; old development state is reset with `pevo init --reset-state`
  rather than migrated.
- Fixed Workflow Automations stale `running` recovery so Gateway startup,
  scheduler ticks, and manual run-now recover expired claims to terminal failed
  runs, recompute scheduling, and preserve historical thread evidence.
- Separated Workbench automation lifecycle display from last-run status and
  restored Open Thread fallback to the newest non-empty run thread.
- Added a persistent provider `/models` picker cache under
  `$PSYCHEVO_HOME/cache/provider_models_cache.json` for explicit catalog fetches
  from CLI and Workbench, with credential fingerprints, raw metadata stripping,
  and Settings reads that do not contact providers.
- Added deterministic provider-cache coverage plus opt-in live validation for
  Xiaomi Token Plan catalog fetch and automation execution.

## 2026-06-27

- Implemented the shared hook runtime and `pevo hooks` review commands with
  normalized declarations, profile trust, concurrent command hooks,
  prompt/worker handlers, plugin hook loading, permission feedback, and
  deterministic/live validation.
- Fixed hook inheritance, output truncation, and trust-key stability issues;
  the runtime hook ADR and the hook/plugin specs now document the review and
  execution contract without external decision-record dependencies.
- Fixed prompt image routing so ordinary HTTP(S) links remain text, text-only
  image capability metadata degrades image blocks before provider calls, and
  OpenAI-compatible providers retry once with text-only image fallbacks.

## 2026-06-26

- Added the plugin-system ADR, now superseded by ADR 0004, covering manifest
  compatibility, plugin store sources, policy overlays, and capability mappings.
- Implemented the first plugin runtime slice with native/compat manifest
  loading, path-safe local/Git installs, scoped plugin stores, policy overlay
  parsing, `pevo plugin`, shared hook execution, worker tool adapters, and a
  migrated disk-cleanup plugin fixture.
- Hardened plugin install and worker behavior by rejecting package symlinks,
  bounding worker JSON-RPC waits, passing effective env into worker discovery,
  allowing project-local policy for profile-installed plugins, and aligning
  plugin testing specs with behavior-focused acceptance coverage.
- Fixed first-turn Workbench automation/thread ownership issues and added
  deterministic + live Playwright coverage for composer-driven automation flows.
- Reworked `peval-py` around an inspect-first trajectory workflow:
  `view tr` is now the default bounded path with schema-stable output,
  explicit workspace/adapter DB guidance, and direct `raw` mode/CLI report
  paths moved to references.
- Improved peval-py trajectory ergonomics by reorganizing skill references,
  accepting Trial-cell paths in `view tr`/`export tr`, and clarifying output
  artifact provenance and missing-artifact errors.

## 2026-06-25

- Expanded Workflow Automations with concrete specs, Workbench editing and
  lifecycle controls, one-shot schedules, model-facing management, and separated
  pause/resume RPCs.
- Added profile-level slash alias/keybind settings across Gateway, TUI, and
  Workbench.
- Reorganized model configuration docs into `specs/125-model-config/` and
  tightened Workbench model/settings behavior.
- Improved visible-session titles, transcript replay ownership, TUI rendering,
  and sandbox validation for shell-only compatibility paths.
- Improved `peval-py` input/config/report handling and Trial artifact guidance.
- Moved the Rust broad validation gate under the repo-owned CI workflow runner
  and updated related docs.

## 2026-06-24

- Added Workbench automation drafting via `automation/draft`, returning a user
  editable draft before save.

## 2026-06-23

- Began local Automations with project automation flows, thread heartbeats,
  Gateway scheduling, Workbench UX, and SQLite persistence contracts.
- Expanded model management for Workbench/TUI: Settings assignment pickers,
  shared model-state synchronization, aux-model routing, fetched catalogs, and
  improved picker UX/filtering for faster selection.
- Fixed model controls and provider behavior: default-model persistence,
  stale composer/model sync, no-auth OpenAI execution, and more readable
  session/panel rendering in Workbench.
- Updated shared Channel surface and `/agents` behavior, including cleaner
  Channel detail affordances and IM workspace-binding rotation.
- Advanced `peval-py` Trial workflows with analysis import, ATIF-compatible
  trajectory conversion, reduced metric duplication, and cleaner report
  rendering/interpretation.
- Moved Channels toward a shared user-surface path with normalized input parts,
  shared slash-command handling, WeChat media metadata fallback, and source
  lane diagnostics in Workbench.

## 2026-06-22

- Reorganized channel specs into shared foundations, Channel UX, and platform
  topics for WeChat, Telegram, and Feishu/Lark.
- Enabled config-ready managed Gateway channels (notably WeChat iLink), including
  QR reconnect flow, polling turns, chat reply, and secret-free diagnostics across
  CLI, RPC, and Workbench.
- Refined Workbench channel settings and details into a cleaner staged workflow
  with mobile/desktop coverage and workspace-aware message routing.
- Delivered `peval-py` Trial-cell migration: state schema v3, collapsed trial
  pointer model, and artifact updates for `notes.md`, `analysis.md`, and typed
  `analysis.json`.
- Expanded `peval-py serve` Source Manager to near-full-screen with per-adapter
  default SQLite persistence and report rendering updates.

## 2026-06-21

- Added Channels setup for WeChat, Telegram, Feishu, and Lark across
  `pevo gateway setup`, profile TOML/env config, Gateway RPC/protocol,
  Workbench Settings, docs, fail-closed allowlists, and first real adapters.

## 2026-06-20

- Shared provider setup presets with the TUI `/model` Add Provider flow,
  including fetched model selection for DeepSeek, Z.AI, and Xiaomi Token Plan.
- Reworked undo/redo Git snapshots to use canonical workspace-level stores with
  stable hashed workspace ids and hourly best-effort `git gc --prune=7.days`
  cleanup, avoiding one snapshot directory per session.
- Added Workbench model-resolution status and guarded model turns on resolved
  provider-qualified models, with reasoning-effort controls aligned to TUI
  `default` semantics.

## 2026-06-19

- Improved setup, install, and local validation flows, including provider/model
  onboarding, native/tooling preflights, repo-local dev-home guidance, and
  quieter deterministic validation output.
- Upgraded frontend workspace tooling to `pnpm@11.8.0` with current Workbench,
  protocol, Vite, Vitest, TypeScript, and React package versions.
- Reworked ACP/Gateway live retention around coalesced snapshots while
  preserving peer turn ordering and avoiding stale resume-history updates.
- Refined Workbench profile/backend setup UX with safer defaults for ACP command
  JSON, workspace CWD, and model/provider selection.
- Made `pevo init --reset-state` stop the current profile's managed Web/Gateway
  before recreating SQLite state so later Web launches cannot reuse old
  session data through a stale background process.
- Fixed Rust 1.96 Clippy compatibility across SSE parsing, agent sorting, TUI
  model filtering, and ACP peer notification draining.

## 2026-06-18

- Added a neutral macOS-style Workbench `Light` appearance, renamed existing warm
  light palette to `Warm`, and migrated legacy light-mode preferences.
- Increased shared Workbench font scale across dark/light/warm for sidebar chrome,
  status, composer controls, settings, and empty states.
- Improved `peval-py` serve/CLI/insights behavior: added offline `peval-py`
  trajectory analysis skill support, cached `analysis` overlays for snapshots, and
  `-r/--root` for `view/export tr`.

## 2026-06-17

- Reorganized UI specs around shared `022` UI foundation, `240` Web/Workbench,
  `250` display model, `260` rendering, and `270` interaction ownership, with
  `210` narrowed to TUI-specific state, rendering, interaction, and validation.
- Added Workbench inline diff rendering for successful text-editing tool rows,
  backed by a shared parsed-diff component model and existing Review diff
  styling tokens, with default-open transcript rows showing the rendered diff
  directly instead of edit metadata sections.
- Matched Workbench `read` tool evidence to TUI-style rendering, showing
  `read <path>` rows with file-content-only expanded detail and full-width
  title clipping.
- Fixed Workbench running-session resume so `thread/read`/`thread/resume`
  snapshots retain active tool rows, appended command output, and composer
  elapsed timers after switching away and back.
- Added explicit cost-status accounting, Reasonix-style cache-read hit percent,
  all-history `usage/read` summaries, and a Workbench Settings Usage page with
  total/7-day/30-day token, cache, cost, and yearly activity heatmap views.

## 2026-06-16

- Reworked Agent delegation to `spawn_agent` (`task_name`/`message`/optional
  `agent_type`), with durable child-session lineage and stable `Agent` identity
  across live and committed projection.
- Fixed Agent lifecycle regressions: stable handoff rows, child-thread routing,
  and side-thread navigation behavior for Side chat/subagent flows, with completed
  rows staying openable and transient duplicate/interrupted states suppressed.
- Added pending permission/clarify request handling so live request events route
  correctly to draft/source-started turns with immediate thread-bound responses.
- Added `peval-py serve` analysis/caching usability improvements for source
  aliases and language selection persistence.

## 2026-06-15

- Added shared thread-lineage rules for Side chat and child-agent tabs, with
  parent-scoped side tabs, inline `/btw` side prompts, and open-actions to child
  session tabs.
- Fixed subagent routing so parent/child Thinking and tool rows stay isolated and
  completed Agent details preserve prompts while remaining openable after live or
  reload updates.
- Tightened timing UI by suppressing sub-second tool elapsed noise and aligning
  Workbench/TUI acceptance of failed/interrupted terminal facts.
- Enriched `peval-py` report workflows with workspace `analysis.json`,
  `analysis.md`, and `notes.md` refresh/edit support.

## 2026-06-14

- Added durable Gateway activity and session-browser infrastructure with
  ownership leases, stale takeover, 7-day paged browsing, and matched
  Workbench/TUI running timers.
- Polished running-state UX with spinner-driven session rows, turn timers,
  inline Thinking/tool progress states, improved composer/status controls, and
  concise elapsed rendering.
- Fixed cross-surface live/projection consistency: rejoin durable foreign sessions,
  preserve active tool timing, keep completed Thinking rows clean, and route
  source-owned approvals to the correct turn lifecycle.
- Split major UI/runtime/protocol surfaces into semantic modules and added
  module-based inventory checks.
- Expanded ACP interoperability (session-update streams, v2/v1 negotiation,
  runtime-mode mapping, `@agent` delegation, status telemetry) plus unified
  Workbench Settings and typed agent/backend/status control surfaces.
- Fixed child-agent interruption propagation across Workbench, Gateway, and ACP
  delegates; aligned runtime/spec foundations and retained `peval-py` duration
  heat fixes.

## 2026-06-12

- Reworked Workbench GUI and interaction flow around hidden launch drafts,
  expanded right-column workflows, immersive file/diff previews, PTY terminal
  tabs, safer menu dismissal, and cleaner model/session control semantics.
- Added file-editing and review in Workbench: authenticated saves, conflict-safe
  edits, and turn-scoped Accept/Reject for change reviews.
- Added shared command/session observability and structured transcript tool evidence
  projection across Workbench and TUI, keeping raw tool payloads out of user view.
- Added Gateway terminal RPCs plus protocol/client support, and polished `peval-py
  serve` report UX with stronger source lifecycle, filtered exports, and clearer
  timeline rendering.

## 2026-06-11

- Expanded `peval-py` reporting with Timeline timing fixes, Hermes/OpenCode DB
  timing fusion, single-session Timeline drawer support, and package-backed
  HTML assets.
- Added minimal `peval-py init` and `peval-py serve` saved-workspace support,
  including stdlib localhost APIs, source management, upload snapshots, locale
  defaults, path-token adapter inference, and DB session picking.
- Added `pevo profile` management with named homes, sticky/global selection,
  cloneable setup, local aliases, profile-local managed Gateway state, and
  status display across CLI/TUI/Workbench.
- Added profile-scoped workspace roots and GUI workspace creation through
  Gateway, protocol, and Workbench while keeping runtime scopes cwd-based.
- Polished Workbench Sessions/search/files language and spacing around
  workspace semantics, including the icon-only workspace creation action.

## 2026-06-10

- Added opt-in sandbox enforcement for file writes and shell children, including
  bounded approval grants, `/sandbox` status, config parsing, and broad
  validation cleanup.
- Tightened TUI composer and transcript rendering around wrapped input,
  command metadata, live footer state, and stable exec/status presentation.
- Added runtime-declared GUI slash command presentation metadata, with
  Web/Desktop catalog filtering, typed unsupported-command guidance, Workbench
  command grouping, composer-local feedback, structured destination routing,
  and original slash display text for submit-style commands.
- Completed several Web/Desktop UX and command-flow fixes around safer slash action
  visibility, in-transcript `/help`/`/commands`/`/agents`, and composer alignment
  with the centered reading column plus compact model controls.
- Expanded `peval-py` reporting with Waterfall/flat traces, richer timing
  diagnostics, compact `sessions/<session_id>/events.jsonl` payloads, v18
  timing fields, and shared report-rendering and export flows.
- Split `peval-py` tests into behavior-focused suites with shared fixtures and
  support helpers.

## 2026-06-09

- Improved Workbench dark-mode readability and transcript density with stronger
  sidebar/status text, a distinct user-message bubble, and a centered shared
  transcript reading column.
- Polished Workbench composer model/context fitting, Transcript scrollbar
  visibility, collapsed left-rail icons, sidebar section alignment, and the
  icon-only Settings return path.
- Fixed Workbench/TUI `exec_command` rows to use one clipped invocation title
  and aligned Workbench new-session checks with local draft replacement.

## 2026-06-08

- Reworked the Workbench composer around the `+` menu, Plan chip,
  session-scoped Agent selection, compact model/context controls, circular
  send/interrupt slot, growing input, and collapsed-sidebar cleanup.
- Made managed `pevo web`/`pevo gateway` startup prefer
  `127.0.0.1:58080`, with automatic fallback through `58099` when default
  ports are already in use while keeping explicit `--bind` strict.
- Isolated TUI and Workbench new-session drafts from older still-running turns
  by routing first prompts through draft source lanes while preserving
  background completion for the previous session.
- Fixed exec hardline shutdown/reboot detection so quoted SQL or prose such as
  `system halted` no longer trips the system-destructive command deny.

## 2026-06-07

- Reworked Workbench around global project-grouped sessions, guarded session
  selection chrome, host-backed preferences and pins, Files/Status/Debug
  inspector panes, file/diff previews, composer attachments, and quieter
  transcript hover affordances.
- Tightened the Workbench visual system into a denser ledger surface with dark
  defaults, neutral light-mode highlights, reduced card chrome, softer active
  shadows, stable-on-hover scrollbars, and non-overflowing composer controls.
- Split shared Workbench components and generated protocol schemas into
  semantic modules, then added stable Vite manual chunks so production builds
  stay below the default chunk-size warning threshold.

## 2026-06-06

- Reworked GUI/TUI session history around a shared global session model:
  Workbench lists sessions across projects with project grouping and persistent
  pin/unpin controls, Gateway/TUI hide internal child/side threads instead of
  partitioning by source, and cross-project resume switches the active scope to
  the session's stored cwd.

## 2026-06-05

- Reworked the Web/Desktop Workbench shell toward the v0 ledger layout, with
  collapsible sidebars, global Pinned and project session sections,
  project-scoped Files, Status/Files/Debug inspector tabs, inline file and diff
  previews, composer-local approval and clarify panels, clickable
  permission/mode/model/variant controls, context popovers, Settings appearance
  and Debug toggles, session/message Search, composer attachments, and no
  tokenizer/context-scope UI.
- Made Web/Gateway session creation lazy so startup, reconnect, New, and reset
  no longer create persisted pending sessions before the first valid prompt or
  user shell request.
- Fixed Workbench new-thread drafts so delayed background turn events, shell
  results, and snapshot refreshes no longer jump the Web view back to an older
  real session after clicking New.
- Added a visible Workbench History draft row for new Web sessions so clicking
  New shows the detached draft immediately without creating a persisted runtime
  session.
- Added `peval-py` structured input manifests and improved multi-session HTML
  reports with Leaderboard filters, metric shading, lighter ten-level
  duration-shaded Trajectory Overview rows, and a reserved Step details drawer.

## 2026-06-04

- Added shared Web/Desktop shell mode and runtime-backed slash commands, with
  typed Gateway protocol support, persisted shell evidence, and a cleaner
  icon-light Workbench transcript surface.
- Fixed `pevo acp` Zed visibility by upgrading the ACP SDK to 0.13.1,
  projecting runtime reasoning as ACP thought chunks, improving
  `exec_command` tool cards, and adding opt-in terminal-output display metadata
  without delegating command execution to the client.
- Added `peval-py` adapter auto-registration through Python entry points, with
  path-adapter support, adapter-specific TOML options, custom adapter docs, and
  deterministic plugin registry tests.
- Added adapter-owned `peval-py` input handling, including native Psychevo,
  OpenCode, and Hermes SQLite conversion, latest-session defaults, ATIF JSON
  passthrough, and per-input multi-adapter comparison reports.
- Added config-selected `peval-py` report localization and Simplified Chinese
  docs while preserving selected English report terms and avoiding duplicate
  comparison section labels.
- Improved `peval-py` HTML reports with estimated step token chips,
  proportional timing fills, larger typography, and package CSS/JavaScript
  assets while keeping generated reports self-contained.

## 2026-06-03

- Added the standalone `tools/peval-py` trajectory reporter for
  Psychevo/OpenCode/Hermes JSONL and Psychevo SQLite sessions, with ATIF export,
  JSON/HTML reports, multi-session comparison, notes, defaults, docs, and
  offline tests.
- Shared trajectory semantics across peval and `peval-py`, including grouped
  observations, tool timing, failed-tool styling, token chips, and
  Run/Result/Evidence report sections.
- Split transcript ownership across state, shared display projection, and TUI
  rendering specs while removing durable runtime debug events, legacy timeline
  storage, and durable capability snapshots.
- Tightened Gateway/Web transcript projection and reconciliation so reasoning,
  assistant text, tools, optimistic prompts, snapshots, and yielded
  `write_stdin`/`exec_command` rows keep stable order without duplicate
  Thinking or answer rows.
- Improved Workbench command routing, composer mentions/completions, transcript
  rendering, session switching, boot fallback, managed Gateway reuse, and the
  opt-in Playwright live-skill harness.
- Fixed TUI transcript replacement, history reload, committed footers, Thinking
  folding, tool evidence folding, and live-turn reconciliation around
  message-derived transcript entries.
- Cleaned Gateway protocol generation so Rust DTOs, generated TypeScript, and
  JSON Schema stay aligned without noisy serde warning output.

## 2026-06-02

- Added local startup and recovery flows for `pevo web`, default TTY `pevo`,
  `pevo doctor`, and interactive `pevo setup`, including managed Workbench
  asset installation, launch-required recovery pages, and one-time launch URL
  reuse behavior.
- Added the first Gateway ACP peer-agent foundation with `[agents.backends]`,
  backend-backed agent definitions, `pevo agents backend` commands, ACP stdio
  routing, native session linkage, and shared agent/backend APIs.
- Added shared Gateway completions, structured mentions, command execution
  feedback, and per-session activity metadata for `/`, `$`, and `@` behavior.
- Upgraded Workbench composer and transcript parity with TUI-style sending,
  accepted mentions, live Thinking, optimistic prompts, Markdown/GFM,
  collapsible tools, bottom-follow, and thread-scoped live updates.
- Hardened Web history, settings, session switching, background-turn handling,
  transcript ordering, tool evidence, long tool headers, and startup fallback
  behavior.
- Fixed Gateway/TUI live transcript reconciliation, yielded command title
  retention, `write_stdin` folding, Thinking/tool evidence folding, and
  history reload ordering.

## 2026-06-01

- Combined the `peval view` HTML report Step expand/collapse controls into a
  single stateful toggle button.
- Removed unreleased timeline persistence and moved Gateway, Workbench, ACP,
  `pevo run --format json`, and fullscreen TUI live/reload flows to
  message-derived transcript projections.
- Kept default plain CLI output unchanged while fixing transcript evidence,
  `write_stdin` merging, skill-loaded status, reasoning order, and
  completed-turn metadata footers.
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
  shared runtime prompt cwd handling across CLI, TUI, agents, compaction,
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
- Made scoped config writes cwd-local by default, with `-g`/`--global` for
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

- Added fullscreen TUI `/btw` side chats with hidden temporary side
  sessions, `/side` compatibility input, and `/refresh` cleanup.
- Added runtime context compaction with checkpoints, TUI
  `/compact [instructions]`, automatic compaction, and a `pevo run` overflow
  retry.

### Changed

- Changed subagent model-visible results to compact summaries while preserving
  full runtime metadata for diagnostics, TUI, and exports.
- Removed external reference-project names from user-visible TUI interaction
  specs.
- Promoted runtime permissions to foundation `041-permissions`, archived the
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

- Added the 075 Design System spec for `pevo tui`.

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
