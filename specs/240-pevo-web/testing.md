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
- Browser validation samples rendered transcript order against
  message-derived SQLite transcript facts when live rendering correctness is
  under test.
- Workbench renders parseable update-tool diffs as default-visible inline
  transcript evidence without changing Review preview behavior.
- Workbench exposes local Automations as an app-level surface with project
  automation and thread-heartbeat workflows backed by Gateway RPC and durable
  local state.

## Current Implementation Slice

Automation vocabulary and generic validation boundaries follow
[060 Automation](../060-automation/spec.md).

Frontend validation uses deterministic local harnesses by default. Unit tests
cover generated protocol validators, client reconnect/pending request behavior,
host storage, and component rendering.

Browser tests use Playwright against the built Workbench served by
`pevo gateway open --no-browser --print-url`, with isolated config, SQLite
state, and workdir by default.

Live model, live skill, GUI automation, and ACP peer validation are opt-in. They
may use the repo-local development home defined by
[060 Automation](../060-automation/spec.md), but must still set explicit
`PSYCHEVO_CONFIG`, `PSYCHEVO_DB`, workdir, and test artifact paths when
isolation is required. They must not print tokens or secrets.

## Scenario Matrix

- Workbench starts and resumes threads for the authorized scope, reconciles
  draft sessions, and keeps history switching from stealing background turns.
- Composer submit, permission, clarify, command feedback, runtime controls,
  settings, files, review, terminal, status, downloads, and debug panels remain
  functional after reconnect.
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
- Generated protocol schemas and clients preserve public imports and strict
  validation behavior.
- The reusable `live-skill` Playwright spec samples the page every three
  seconds, writes screenshots as test artifacts, and compares rendered DOM
  order against the isolated SQLite message-derived transcript.
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
- Settings > Models assignment tests cover reuse of the same model/reasoning
  selector behavior used by the composer.
- Settings > Usage visual tests cover token-activity heatmap levels with
  distinct computed colors across zero and four nonzero activity levels in the
  light appearance.
- Sessions-browser tests cover long title truncation without horizontal
  scrolling, while preserving recent-update time, running state, and row action
  visibility.

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
