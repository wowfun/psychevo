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
  primary controls. Expanded History assertions keep the `New Session` icon
  aligned with the `Search` navigation icon while collapsed action icons remain
  centered. Settings assertions keep `Back to app` aligned with section-row
  icons and reject a visible `›` navigation indicator. Folder-picker visual
  assertions keep the location strip background transparent while preserving
  the editable path field. Workspace-create tests address its name field by the
  stable `Workspace name` accessibility contract rather than placeholder text.
- Right-workspace Home visual assertions keep each icon-and-label navigation
  row left-aligned rather than inheriting centered shared-button content.
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
state, cwd, and an operating-system-assigned loopback port by default. Isolated
test instances must not share the bounded user-facing managed port range, so a
stale server from an interrupted run cannot exhaust later test launches. The
harness treats a nonzero `gateway stop` exit as a cleanup failure and retains
that test root for diagnosis instead of silently deleting its ownership state.

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
- Browser helpers address stable accessible labels and assert toggle state via
  `aria-expanded` or `aria-checked`; they do not depend on obsolete dynamic
  names such as `Show ...`, `Disable ...`, or renamed button copy.
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
- Files-tree context-menu tests cover pointer and keyboard opening, focus
  restoration, Escape/outside dismissal, viewport clamping, directories without
  file actions, and binary or unsupported-preview files that remain externally
  actionable. Fake Gateway responses verify preferred/alternate ordering and
  OS-specific Finder/File Explorer/File Manager labels without launching a real
  application.
- Gateway workspace tests use injected detector and launcher fakes to cover
  category precedence, content-aware text detection, VS Code present/absent
  behavior, exact workspace-root-plus-file arguments, system-default and reveal
  platform commands, unreadable content-probe fallback, early non-zero opener
  exits, bounded launch failures, and repeated containment checks.
  Tests reject absolute paths, traversal, symlink escapes, directories, and
  missing files. Browser-authenticated tests reject both a non-current cwd and a
  two-step scope pivot in which an arbitrary draft/start cwd becomes current but
  was never added through a trusted external-action grant flow. UI assertions
  distinguish preview-unavailable semantics from a disabled file row. Tests
  never invoke a real desktop application.
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
- Composer integration tests cover explicit and cached-metadata name hydration
  for Native and ACP cold startup before Settings is opened. They retain Model
  plus Reasoning while an accepted first Turn is waiting for its authoritative
  bound context and assert that a running existing Thread still fits the full
  label when side workspaces narrow the Composer independently of the viewport.
- Composer permission tests cover the compact filesystem request hierarchy:
  tool and source remain visible, policy reason appears once, requested and
  distinct resolved paths each appear once, and duplicated summary or
  suggested-rule rows are absent without changing submitted scope decisions.
- Composer runtime-control tests cover the visible `Permission mode` control
  after Workspace and Git branch, its absence from the Agent target popover,
  and its descriptor-backed effective value after a control receipt. They also cover
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
- Startup tests delay `thread/draft/open` and auxiliary RPCs to prove Sessions become
  usable as soon as the single initial `thread/browser` completes. They also
  prove initialization and browsing overlap, context is not read before the
  startup Thread stabilizes, and scope-owned auxiliary reads are on-demand and
  coalesced. A delayed `initialize` fixture selects a visible Session before
  initialization completes and proves no stale startup `thread/draft/open` request
  is sent after the resulting `thread/resume`.
- Production-build browser validation records the initial resource set and
  encoded JavaScript total, enforces the 1.8 MB startup budget, and proves
  Mermaid and Terminal chunks are absent until the corresponding content or
  panel is opened. A same-profile reload proves immutable assets are reused.
- Interrupt regressions cover an idle cached descriptor followed by a running
  activity snapshot. Main Composer, command, child Thread, and floating entry
  points send `thread/action/run` for the exact target and surface Gateway's
  precise error if the live action is unavailable.
- The Gateway-side dispatch microbenchmark proves that a delayed Codex
  `plugin/list` cannot delay Web runtime contribution assembly. It does not
  stand in for a TUI/Workbench comparison because it bypasses the managed
  Gateway transport, browser client, DOM, and terminal paint paths. Draft
  `thread/draft/open` must return while an inventory prewarm is still in flight.
- Composer real-Gateway tests preserve discovery as unselected, prove a default
  draft open becomes exactly selected/sendable, and allow one pending first-turn
  click during preparation. Editing or retargeting cancels auto-submit without
  clearing input; accepted submission clears once and refreshes history once.
  `cargo xtask live run --check web-composer-draft-open-first-send` exercises
  that boundary against a real local Gateway and deterministic provider while a
  deliberately large Agent catalog keeps the atomic draft open in flight.
- Request-log tests prove New Session performs one draft open plus one
  concurrent `workspace/git/branches` read and no history, Settings, old-scope
  context, or ordinary-text completion request. Agent, Mode, Model, Reasoning,
  Permission, Workspace, and current branch retain the last committed values
  while either a same-workspace or cross-workspace request is pending, then
  replace in one
  Composer-environment commit only if the authoritative result differs. A cold
  startup paints no placeholder Composer before that first complete commit.
  The same-workspace request also preserves the complete source identity. A
  cross-workspace request retains its default target intent rather than sending
  the previously rendered Agent as an exact target, and the active scope remains
  the visible workspace-path authority.
- Composer presentation tests prove the environment line renders Workspace,
  branch, then Permission; abbreviates only an exact host user-home path prefix to
  `~`; retains the canonical path in the title and request; and allows a wider
  bounded path label before ellipsis. Popover tests open Add, Agent, Mode,
  Model/Reasoning, Context, Workspace, Permission, Branch, and completion
  surfaces and prove a common rendered-popup contract, one-open-popup behavior,
  Escape/outside-pointer dismissal, intrinsic bounded width for compact
  selectors, exact message-input-width alignment for completion, truncated
  labels with full titles, and right-aligned switches. Runtime Mode and
  Permission tests explicitly reject native `select` elements.
- Composer visual tests prove Mic and Send/Interrupt retain the same circular
  footprint, including the shared minimum hit target under coarse-pointer
  mobile emulation.
- Pending-send tests prove only an active Composer readiness token can expose
  `Preparing`; a deferred context read or control mutation keeps Send disabled.
- Transport tests use barriers rather than timing sleeps: a held `initialize`
  cannot delay draft open or `thread/browser`; different sources overlap, same
  source mutations remain ordered, and disconnect/error releases the fixed
  per-connection in-flight permit. A saturated connection with 32 held requests
  must still observe the following WebSocket Close/error without first releasing
  a request permit.
- Opt-in Windows Git Bash performance evidence records cached Native/default
  draft-open p95 at or below 40 ms and click-to-controls/send-ready p95 at or
  below 100 ms. Cold catalog bootstrap is tracked separately at or below 500 ms;
  ACP provider handshake is a separate metric and never blocks input or pending
  Send.
- The deterministic critical first-turn journey covers Native and ACP across
  ready-then-send and pending-draft-send scenarios. Each visual run produces a
  versioned manifest and screenshots for `gui_ready`, `draft_context_ready`,
  `send_clicked`, `runtime_request_dispatched`, `first_output_visible`, and
  `turn_settled`; a missing, duplicate, or out-of-order checkpoint fails the
  proof while preserving partial artifacts.
- The corresponding profiling run uses one warmup and twenty measured turns
  per scenario, a fresh draft/Thread per turn, and no screenshot or visual
  barrier delay. Warm send-path samples report raw values plus p50 and p95.
  Cold navigation and first draft readiness remain individual raw samples until
  enough independent process starts exist for meaningful percentiles.
- Journey diagnostics retain runner-observed transport, draft, turn-start,
  runtime request, first runtime output, Gateway/client output, completion, and
  UI-idle marks. Durations are computed only within one declared clock domain;
  manifest validation rejects prompt, response, credential, and token fields.
- Playwright owns the shared Workbench-to-runtime journey. Native Desktop WDIO
  owns process, window, managed-Gateway, bridge, GUI, and draft readiness rather
  than duplicating the full Native/ACP turn matrix.
- A dedicated Native ready-send comparison runs fullscreen TUI and Workbench
  sequentially against the same deterministic provider fixture. It retains a
  raw cold first-turn observation, excludes warmup and trace samples, then
  reports twenty measured warm continuation turns per surface by default.
  Main-turn and title requests are classified separately and every measured
  sample proves exactly one main request.
- Both surfaces use the same Agent/Skill feature policy and an isolated
  synthetic Git workspace outside the source checkout. The manifest records a
  configurable deterministic tracked-dirty-file count so workspace-review
  scaling is reproducible without inheriting the developer's worktree state.
- Comparison manifest v2 reports send-to-feedback-commit, send-to-provider,
  provider-to-first-surface-commit, first-commit-to-settled-commit, and
  send-to-settled-commit. Diagnostic spans distinguish Workbench `turn/start`
  admission, first non-empty assistant receipt, controller batch application,
  DOM commit, and optional post-frame observation from TUI event drain and
  terminal draw commit. It emits raw samples, p50/p95, GUI-minus-TUI deltas,
  ratios, Long Task and cross-sample RPC-overlap diagnostics, a content-free TUI
  JSONL trace, and a Playwright trace. Comparison v1 inputs are rejected.
- Every sample associates the ordered main provider request with exactly one
  Gateway Turn. The manifest and Markdown report recompute p50/p95 sub-
  waterfalls for the shared Gateway/runtime stages and for surface
  receipt/application/paint stages; a missing, negative, cross-clock, or
  ambiguous stage fails validation.
- The deterministic Native fixture leaves a fixed recorded interval between
  first output and completion. A completion event must not discard queued
  assistant output and force the UI to wait for the delayed snapshot refresh.
  The browser probe resets for each sample and uses the DOM submit boundary,
  not the runner timestamp before `locator.click()`.
- Lifecycle tests prove each accepted Native and ACP Turn emits one public
  `TurnStarted` and one `TurnCompleted`, regardless of duplicate or late raw
  runtime stages, and that no `turn/result` or `turn/error` notification exists.
- Submission tests prove a ready click commits provisional running and `0s`
  before Gateway acceptance. A pending-readiness click commits `Preparing` on
  the next DOM update, preserves input, sends no request before readiness, and
  transfers the original timer into running. Accept, reject, delayed start, and
  terminal-before-acceptance cannot duplicate or stick activity.
- Pending-submission tests mutate attachment identity after the initial click
  and before draft readiness, then prove no stale attachment set is submitted,
  the current input is preserved, and a later explicit click submits once.
- Review tests exercise exact write/edit add, update, delete, move, repeated and
  net-zero deltas through the observation Interface. Shell and ACP-owned writes
  produce partial invalidation without a workspace scan or a rejectable fake
  file. Oversize, binary, memory eviction, and post-revision conflict fail
  closed without affecting the Turn.
- A hidden-surface bound Native Turn performs exactly one `turn/start` and zero
  `thread/read`, `thread/browser`, `thread/context/read`, `workspace/*`, and
  `observability/read` requests after acceptance. A first detached Turn browses
  history once and refreshes context once; an ACP completion refreshes context
  at most once. One hundred same-entry updates commit first non-empty assistant
  text without waiting for RAF, converge through one snapshot publication per
  frame, and cannot be overtaken by terminal application.
- Workspace-demand tests prove closed, Home, Review, Files, and unresolved
  Transcript-file-link states request only their specified resource facets.
  Samples begin only after the preceding sample's allowed auxiliary reads have
  settled, and the comparison reports any cross-sample overlap as a structural
  failure.
- Cross-surface validation hard-fails duplicate lifecycle, missing feedback,
  any Web Review Git/status/diff/show work on admission/relay/completion, or
  hidden-surface request amplification. Latency values remain report-only until
  three stable canonical-runner baselines receive explicit budget approval.

## Validation Boundaries

- Deterministic tests should use fake or test providers and isolated local
  state, not the user's normal config, browser profile, credentials, or global
  Gateway state.
- Browser tests should assert user-visible behavior and stable protocol
  invariants rather than private DOM structure when possible.
- Cross-surface comparison failures preserve partial manifests and traces.
  Validators reject prompt/response/token/credential fields, missing samples,
  mixed clock arithmetic, ambiguous provider requests, and percentile inputs
  contaminated by screenshots, visual gates, Playwright tracing, or warmup.
- Visual scenarios that exercise post-startup Composer geometry or labels must
  await the initial draft's Agent/Model controls before opening another draft;
  they must not race catalog bootstrap or use timing sleeps. The dedicated
  draft-open startup/live cases retain ownership of pre-ready interactions.
- Screenshots, traces, and live samples are required evidence for visual/live
  changes, but live provider failures must be reported separately from code
  regressions when caused by credentials, provider state, or environment.
- Managed launch lifecycle belongs to
  [220 pevo Gateway Testing](../220-pevo-gateway/testing.md).
