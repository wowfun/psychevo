---
name: 022. UI
psychevo_self_edit: deny
---

# 022. UI

Define Psychevo's shared UI foundation across TUI, Web, future Desktop shells,
and future Mobile shells.

## Scope

- shared UI vocabulary for transcript, composer, status, command feedback,
  display-only artifacts, overlays, panels, and inspection surfaces
- surface taxonomy and ownership boundaries between shared UI contracts and
  concrete TUI/Web product specs
- cross-surface source-of-truth map for display model, rendering, interaction,
  visual language, and product-specific implementation details
- validation expectations for UI-affecting specification changes

Out of scope:

- concrete fullscreen terminal command behavior; this belongs to
  [210 pevo TUI](../210-pevo-tui/spec.md)
- concrete Web/Workbench product behavior, JavaScript workspace boundaries,
  browser host adapters, PWA behavior, and frontend package layout; these
  belong to [240 pevo Web](../240-pevo-web/spec.md)
- managed Gateway lifecycle and browser launch bootstrap; these belong to
  [220 pevo Gateway](../220-pevo-gateway/spec.md)
- runtime execution, persistence schemas, provider behavior, or Gateway
  transport semantics

## Surface Model

Psychevo product surfaces share a common interaction shape even when they use
different host technologies:

- transcript surfaces show message-derived prompt, Thinking, tool/evidence,
  Agent, answer, status, and metadata material
- composers collect user prompts, shell commands, attachments, steer/queue
  input, and structured permission or clarify responses
- status and observability surfaces summarize active scope, running work,
  context-window risk, session usage, cache, and cost without becoming
  transcript content
- command feedback, overlays, bottom panes, previews, `/diff`, and debug panels
  are display-only unless a domain spec defines a separate durable sidecar
- host-specific layout, input gestures, package boundaries, and visual polish
  belong to the concrete product surface spec

## Information Density

Controls are authoritative display surfaces for their current values. If a
control already communicates a selected value, enabled state, placeholder,
credential env name, no-auth toggle, model choice, reasoning choice, filter,
or count, the same information should not be repeated elsewhere on the same
page or panel as secondary text, badges, helper copy, or status labels. Repeat
information only when it is needed for accessibility outside the control,
disambiguates a destructive or high-risk action, explains an error, or appears
in a different workflow context where the original control is no longer
visible.

## Source Of Truth

- [250 UI Display Model](../250-ui-display-model/spec.md) owns semantic
  transcript projection, live overlay reconciliation, committed replacement,
  display-only boundaries, and thread display/navigation.
- [260 UI Rendering](../260-ui-rendering/spec.md) owns cross-surface transcript,
  evidence, status, activity, and observability rendering invariants.
- [270 UI Interaction](../270-ui-interaction/spec.md) owns cross-surface
  composer, command routing, permission/clarify, steer/queue/interrupt, and
  display-only feedback interaction semantics.
- [075 Design System](../075-design-system/spec.md) owns shared visual language
  and high-level interaction principles.
- [210 pevo TUI](../210-pevo-tui/spec.md) owns terminal-specific layout,
  keymaps, terminal rendering, slash panes, TUI state, and TUI validation.
- [240 pevo Web](../240-pevo-web/spec.md) owns Web/Workbench layout, browser
  host behavior, frontend packages, component implementation boundaries, and
  browser validation.

## Validation

UI-affecting changes should update the highest shared source of truth that
actually owns the rule. A change that affects both TUI and Web should start in
`250`, `260`, or `270`; concrete rendering, layout, host, or keybinding details
then link back from `210` or `240`.

Documentation-only changes to this foundation do not require code validation.
Behavioral UI changes should use deterministic local harnesses and fake or test
providers. Rendered evidence such as browser screenshots or terminal captures
is required when the changed behavior is primarily visual.

## Related Topics

- [070 Experience](../070-experience/spec.md) defines shared UX/DX defaults.
- [075 Design System](../075-design-system/spec.md) defines shared visual and
  interaction language.
- [210 pevo TUI](../210-pevo-tui/spec.md) defines the fullscreen terminal
  product surface.
- [220 pevo Gateway](../220-pevo-gateway/spec.md) defines managed local Web
  launch lifecycle.
- [240 pevo Web](../240-pevo-web/spec.md) defines the Web/Workbench product
  surface and frontend platform.
