---
name: 260. UI Rendering
psychevo_self_edit: deny
---

# 260. UI Rendering

Define cross-surface rendering invariants for Psychevo transcript, evidence,
status, activity, and observability UI.

## Scope

- shared transcript row semantics for prompts, Thinking, tool/evidence, Agent
  rows, assistant answers, status rows, and turn metadata
- evidence title language, folding/expansion expectations, activity indicators,
  elapsed labels, and display-only artifact rendering boundaries
- shared status and observability presentation rules for context, usage, cache,
  cost, and running work
- reusable rendering expectations that both TUI and Web/Workbench must preserve

Out of scope:

- semantic transcript projection, live overlay reconciliation, and committed
  replacement; these belong to
  [250 UI Display Model](../250-ui-display-model/spec.md)
- terminal layout, alternate screen, terminal palette, TUI Markdown rendering,
  and VHS fixtures; these belong to [210 pevo TUI](../210-pevo-tui/spec.md)
- Web layout, CSS, React component boundaries, browser host behavior, and
  Playwright browser validation; these belong to
  [240 pevo Web](../240-pevo-web/spec.md)

## Transcript Rendering

Rendering surfaces consume typed transcript entries and blocks from
[250 UI Display Model](../250-ui-display-model/spec.md). They must not render
raw runtime/provider records, unclassified stream events, or surface-local
debug material as ordinary transcript rows.

User prompts render as prompt material without a generic role label. Visible
assistant text renders as assistant answer material. Reasoning blocks render as
Thinking material and must not be copied into assistant text rows. Empty
reasoning completion closes existing Thinking state; it must not create an
empty visible row.

Tool and Agent rows require explicit typed transcript blocks, execution
observations, or message-derived tool-result relationships. Reasoning or
assistant prose that describes intended work must not create active `read`,
`write`, `exec_command`, Agent, or similar rows.

Display-only command output and observational artifacts, including `/diff`,
command feedback, previews, and debug panels, must not become model-visible
history, exports, usage/cost accounting, or ordinary transcript projection.

## Activity And Observability

Active Thinking, tool, Agent, and running-session indicators use one shared
activity vocabulary per surface. Elapsed labels are visual status only; timer
updates must not resize stable rows or mutate transcript content.

Context usage is the first observability segment because it communicates the
immediate context-window risk. Cache-read percent, session token totals, and
estimated cost may follow when the surface has room. Narrow renderers drop
later observability segments before dropping context usage.

Session observability details are metric rows derived from persisted
accounting and context assembly. They must not display prompt bodies, message
text, tool arguments, provider request payloads, or raw provider metadata.

## Surface Ownership

Shared rendering rules define semantic invariants, not exact geometry. TUI owns
terminal layout, row measurement, cursor anchoring, and terminal-specific
Markdown projection. Web owns responsive Workbench layout, DOM/component
structure, CSS, and browser-specific visual validation.

## Attachments

- [Evidence](evidence.md) defines shared evidence-row projection, folding,
  failure, `exec_command`, and `write_stdin` rendering rules.
- [Agent Rows](agent-rows.md) defines shared subagent row rendering and
  parent/child transcript preview behavior.
- [Testing](testing.md) defines rendering validation expectations.

## Related Topics

- [022 UI](../022-ui/spec.md) defines the shared UI source-of-truth map.
- [075 Design System](../075-design-system/spec.md) defines shared visual
  language and high-level interaction principles.
- [250 UI Display Model](../250-ui-display-model/spec.md) defines the semantic
  transcript records rendered by UI surfaces.
- [270 UI Interaction](../270-ui-interaction/spec.md) defines the interactions
  that produce command feedback, panels, and active-turn control state.
- [210 pevo TUI](../210-pevo-tui/spec.md) defines concrete terminal rendering.
- [240 pevo Web](../240-pevo-web/spec.md) defines concrete Web/Workbench
  rendering.
