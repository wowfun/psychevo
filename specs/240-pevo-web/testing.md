---
name: 240. pevo Web Testing
psychevo_self_edit: deny
---

# 240. pevo Web Testing

Define acceptance expectations and validation scenarios for the Web/Workbench
product surface and frontend platform.

## Long-Term Acceptance Contract

- Workbench starts and resumes threads for the authorized scope, reconciles
  draft sessions, and keeps history switching from stealing background turns.
- Workbench preserves transcript projection, live overlay reconciliation,
  command feedback, permission/clarify, runtime controls, settings, files,
  review, terminal, status, downloads, and debug panels across reconnect.
- Browser host capabilities expose endpoint discovery, storage, clipboard,
  file/image picking, notifications, downloads, and unsupported native-only
  operations through typed host contracts.
- Generated protocol schemas and clients preserve public `gatewaySchemas`,
  method names, event names, and wire shape compatibility.
- Desktop and narrow viewports preserve usable navigation and non-overlapping
  primary controls.
- Agent-session import browser coverage uses a short viewport and enough
  discovered sessions to require internal dialog scrolling while keeping the
  dialog header, footer, and document shell fixed. It imports deterministic
  replay containing user and assistant text, reasoning, plan, and tool evidence,
  then verifies that the opened Transcript preserves that durable order.
- Non-fullscreen Desktop main-window validation must assert that the Workbench
  document itself cannot scroll vertically while preserving internal transcript,
  session-list, Settings-content, and long-panel scrolling.
- Browser validation samples rendered transcript order against
  message-derived SQLite transcript facts when live rendering correctness is
  under test.
- Workbench renders parseable update-tool diffs as default-visible inline
  transcript evidence without changing Review preview behavior.
- Workbench exposes local Automations as an app-level surface with project
  automation and thread-heartbeat workflows backed by Gateway RPC and durable
  local state.

## Current Implementation Slice

CI/CD vocabulary and generic validation boundaries follow
[065 CI/CD](../065-ci-cd/spec.md).

Frontend validation uses deterministic local harnesses by default. Unit tests
cover generated protocol validators, client reconnect/pending request behavior,
host storage, and component rendering.

Browser tests use Playwright against the built Workbench served by
`pevo gateway open --no-browser --print-url`, with isolated config, SQLite
state, and cwd by default.

Live model, live skill, GUI automation, and ACP peer validation are opt-in and
selected through `cargo xtask live`. They may use the repo-local development
home defined by [065 CI/CD](../065-ci-cd/spec.md), but `xtask` must own explicit
`PSYCHEVO_CONFIG`, `PSYCHEVO_DB`, cwd, and test artifact paths when isolation is
required. They must not print tokens or secrets.

## Scenario Matrix

- Workbench starts and resumes threads for the authorized scope, reconciles
  draft sessions, and keeps history switching from stealing background turns.
- Composer submit, permission, clarify, command feedback, runtime controls,
  settings, files, review, terminal, status, downloads, and debug panels remain
  functional after reconnect. Shared Attention fixtures assert Runtime Profile
  provenance, public parent/child origin, exact authorization lifetime, and the
  absence of undeclared Session or Always actions.
- Runtime-child transcript fixtures cover reconnect tab registration and lazy
  history reads for Full, Summary, and Partial fidelity. Visual assertions keep
  the compact Summary/Partial gap notice legible without horizontal overflow.
- Automations tests cover natural-language draft creation, empty state
  templates, manual creation, template creation, project automation rows, thread
  heartbeat rows, enable/disable, run-now, delete, and open-thread behavior.
- Automation browser validation covers desktop and narrow viewports and must
  assert that the app-level Automations surface hides composer/right inspector
  chrome without creating horizontal overflow, and that global New Session
  navigation from Automations returns to the transcript draft surface.
- Automation protocol validation covers generated schemas, typed client method
  mappings, strict draft and write payload validation, and run responses for
  accepted, busy, and failed starts.
- Desktop and narrow viewports preserve usable navigation and non-overlapping
  primary controls.
- Short desktop viewport checks cover the main Workbench shell and Settings as
  a control case, and fail when `document.scrollingElement` can scroll past the
  visible app shell.
- Generated protocol schemas and clients preserve public imports and strict
  validation behavior.
- The reusable `live-skill` Playwright check is selected by
  `cargo xtask live run --suite skill`, samples the page every three seconds,
  writes screenshots under the live check artifact root, and compares rendered
  DOM order against the isolated SQLite message-derived transcript.
- GUI automation live validation creates a project automation through the
  composer with the fastest supported interval schedule and asserts the final
  transcript answer is not duplicated before inspecting the Automations surface.
- Browser validation fails on Workbench render error boundaries, stale running
  reasoning rows that duplicate committed reasoning, non-monotonic committed
  row order, tool result JSON in collapsed headers, evidence header overflow,
  empty assistant updates after tool rows, and stale completion popovers after
  prompt submission.
- Inline transcript diff fixtures cover desktop and narrow viewports, including
  direct rendered-diff detail without Input/Change metadata, single-gutter
  rows, clipped long lines, and malformed-diff fallback.
- Settings > Models tests cover the [125 Model Config](../125-model-config/spec.md)
  acceptance scenarios through the concrete Workbench UI, including fake
  provider/catalog flows, OpenCode Zen free-model warning state, independent
  assignment saves, global-vs-project scope, default-save control refresh, and
  scoped composer override preservation.
- Composer model-control tests cover the grouped model/reasoning selector,
  including non-selectable empty state, short model display in the closed
  control, provider-qualified hover/title metadata without visible duplicate
  model names, provider group headings for adjacent visible provider runs,
  no row-level provider metadata, muted green free-model badges from
  `ModelOptionView.free`, model-specific reasoning effort options, recent-model
  promotion, model name filtering, five-row model-list overflow behavior,
  closed control model-plus-reasoning display, longest-visible-option popover
  width adaptation, full-width popover rows without unused right gutters, and
  switching models without submitting an invalid `Select model` value.
- Composer runtime-control tests cover the visible `Permission mode` control
  immediately after `+`, its absence from the Agent target popover, and its
  descriptor-backed effective value after a control receipt. They also cover
  the Native `Psychevo` target label and reasoning display: an explicit
  `high` value renders `High`, an explicit `none` value renders `Default`, and
  an unknown value does not fall back to `Default` or the first choice.
- Settings > Models assignment tests cover reuse of the same model/reasoning
  selector behavior used by the composer.
- Settings > Usage visual tests cover token-activity heatmap levels with
  distinct computed colors across zero and four nonzero activity levels in the
  light appearance.
- Sessions-browser tests cover long title truncation without horizontal
  scrolling, while preserving recent-update time, running state, and row action
  visibility.
- Startup tests delay `thread/start` and auxiliary RPCs to prove Sessions become
  usable as soon as the single initial `thread/browser` completes. They also
  prove initialization and browsing overlap, context is not read before the
  startup Thread stabilizes, and scope-owned auxiliary reads are on-demand and
  coalesced. A delayed `initialize` fixture selects a visible Session before
  initialization completes and proves no stale startup `thread/start` request
  is sent after the resulting `thread/resume`.
- Production-build browser validation records the initial resource set and
  encoded JavaScript total, enforces the 1.8 MB startup budget, and proves
  Mermaid and Terminal chunks are absent until the corresponding content or
  panel is opened. A same-profile reload proves immutable assets are reused.
- Interrupt regressions cover an idle cached descriptor followed by a running
  activity snapshot. Main Composer, command, child Thread, and floating entry
  points send `thread/action/run` for the exact target and surface Gateway's
  precise error if the live action is unavailable.
- Paired GUI/TUI first-token tests use the same deterministic provider and
  prompt. A delayed Codex `plugin/list` cannot delay Web provider dispatch;
  after Gateway initialization the median Web pre-provider overhead stays
  within 150 ms of TUI, while a separate new-session-to-first-token measure
  prevents moving the same wait into Composer loading. Draft `thread/start`
  must return while an inventory prewarm is still in flight.

## Validation Boundaries

- Deterministic tests should use fake or test providers and isolated local
  state, not the user's normal config, browser profile, credentials, or global
  Gateway state.
- Browser tests should assert user-visible behavior and stable protocol
  invariants rather than private DOM structure when possible.
- Screenshots, traces, and live samples are required evidence for visual/live
  changes, but live provider failures must be reported separately from code
  regressions when caused by credentials, provider state, or environment.
- Managed launch lifecycle belongs to
  [220 pevo Gateway Testing](../220-pevo-gateway/testing.md).
