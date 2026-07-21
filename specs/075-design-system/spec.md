---
name: 075. Design System
psychevo_self_edit: deny
---

# 075. Design System

Define Psychevo's shared visual and interaction language. This topic owns the
canonical Adaptive Workbench design system; concrete transcript rendering,
interaction mechanics, and product layout live in the UI and product specs that
consume it.

## Scope

- shared visual language and surface hierarchy
- compact glyph/icon language, color roles, and adaptive host fallback
- core transcript, prompt, composer, status line, sidebar, picker, and bottom
  panel surface treatment
- shared browser control roles, including actions, navigation, selection,
  disclosure, menus, dialogs, mutation feedback, and management-style switches
- shared browser Markdown metadata treatment, including YAML frontmatter tables
- evidence language for inline ledger rows, collapsed details, and active work
- composer-first interaction baseline and default shortcut expectations
- semantic rendering architecture expectations
- deterministic design-system validation expectations
- `DESIGN.md` source-of-truth structure and generated token outputs

Out of scope:

- semantic display-model ownership; this belongs to
  [250 UI Display Model](../250-ui-display-model/spec.md)
- concrete rendering and interaction invariants; these belong to
  [260 UI Rendering](../260-ui-rendering/spec.md) and
  [270 UI Interaction](../270-ui-interaction/spec.md)
- concrete TUI command behavior, terminal rendering, keymaps, slash parsing,
  file completion, and selection mechanics; these belong to
  [210 pevo TUI](../210-pevo-tui/spec.md)
- concrete Web/Workbench layout and browser implementation; these belong to
  [240 pevo Web](../240-pevo-web/spec.md)
- runtime evidence semantics, storage formats, provider payloads, or public
  Rust APIs

## Source Of Truth

`DESIGN.md` in this directory is the canonical design-system source. Its YAML
front matter owns exact token values and platform mappings; its prose owns the
visual intent and application guidance. This spec owns governance: which
surfaces consume the design system, how direct constants are allowed, and what
validation keeps generated outputs honest.

`@psychevo/assets` consumes `DESIGN.md` and publishes generated browser tokens
through `theme.css` plus a typed TypeScript design-system export. Browser
surfaces should import `@psychevo/assets/theme.css` instead of maintaining
parallel theme files. The generated public CSS variable prefix is `--pevo-`.
Management-style browser switches consume the generated `--pevo-switch-*`
semantic variables and keep sizing in component CSS so Web, Desktop, and
Floating can share visual state without duplicating color decisions.
Browser action controls consume generated `--pevo-control-*` variables. The
shared component package owns their semantic variants, focus, press, pending,
disabled, selected, expanded, and dangerous states; product CSS owns only
layout around those controls and must not restyle descendant `button` elements.
Composer interrupt controls use a dedicated neutral dark treatment rather than
the danger palette: interrupting active work is an immediate runtime control,
not a destructive-data warning.
Ordinary browser fields consume generated `--pevo-field-*` variables through
opt-in shared field, search, and choice-control classes. Product surfaces own
field width and editor geometry, while the shared layer owns their resting,
hover, focus, placeholder, read-only, invalid, and disabled treatments.
Browser Markdown renderers consume the shared Markdown component for document
body rendering and document-start YAML frontmatter. Frontmatter is supporting
metadata: render it as a compact table before the Markdown body, use existing
`--pevo-*` border, panel, code, and ink roles, and do not show the raw `---`
block or add explanatory visible copy around it. Markdown previews may expose a
quiet icon-only copy action through the shared renderer; that action copies raw
Markdown source through the host clipboard boundary and does not add visible
state text. Product GUI surfaces should not create page-local Markdown parsers
or duplicate preview copy affordances when the shared component can render and
copy the same source.

TUI consumes the same semantic role names while preserving terminal-native
adaptation. It may keep ANSI and host-palette fallbacks because terminal
capabilities are part of the rendering contract, but the role intent still
comes from `DESIGN.md`.

Direct hardcoded color, radius, typography, shadow, glyph, or terminal palette
values are allowed only when they are host fallbacks, deterministic test
fixtures, or generated outputs from `DESIGN.md`. New product CSS should use
generated semantic variables, and shared package CSS must be scoped enough that
Desktop can import multiple surfaces without selector collisions.

## Direction

Psychevo uses an Adaptive Workbench design language: compact, evidence-led,
terminal-native, and quiet under repetition. The interface should feel closer to
a working ledger than a dashboard. It uses terminal capabilities conservatively,
adapts to the user's foreground/background palette, and avoids decorative
chrome.

The memorable product trait is Adaptive Evidence. The system should make the
agent's work inspectable without turning every runtime event into a loud log
line. Evidence appears near the answer it supports, starts summarized, and
expands only when the user asks for detail.

## Visual Language

Color is ANSI-first. Truecolor and 256-color terminals may receive adaptive
background steps, but semantic roles must always degrade to readable ANSI
colors. Cyan is the ordinary accent for focus, selection, and actionable hints.
Magenta is reserved for rare identity or mode moments and must not become the
primary theme color. Red marks failure words only when failure is the state.
Terminal palette probing is host-specific: Unix builds may query OSC default
colors, tests may exercise the parser deterministically, and unsupported native
platform builds should compile only the fallback path.

Surface hierarchy uses background steps, indentation, spacing, and dim text
before borders. Borders are exceptional: use them only for hard terminal
boundaries, not as the default way to create components. Nested card surfaces
are not part of the TUI language.

State emphasis stays low intensity. Selection may combine accent foreground and
a background step. Active work uses motion and elapsed time before color.
Failures keep their original evidence row and mark short outcome words such as
`failed`, `interrupted`, or `timeout`; they do not move into a separate error
log.

The shared glyph language is deliberately small:

- `›` marks prompts or focus in text-oriented surfaces; browser navigation
  selection does not add a leading glyph.
- `•` marks evidence and active work.
- `·` marks quiet status or notice rows that should remain visually below
  evidence.
- `▸` and `▾` mark collapsed and expanded detail.

ASCII fallbacks may be added for terminals that cannot render these glyphs, but
the design intent is the compact workbench marker set above.

## Core Surfaces

The transcript is a passive reading surface by default. PageUp/PageDown and
mouse wheel scroll it while the composer remains the primary interaction
center. V1 does not include a transcript review overlay; `Ctrl+T` is reserved
for a future review surface and has no default behavior.

User prompts render as light prompt blocks with no role label. The block uses a
leading dim `›` marker and the same adaptive input surface as the composer.
Continuation rows keep the prompt surface background so wrapped and CJK text
does not visually break the block.

The composer is a quiet input band. It starts at one visible input row, grows
only with content, and uses `Ask pevo...` as the placeholder. Composer-first
interaction means slash menus, file/skill completion, status hints, and bottom
sheets are anchored to the input flow rather than competing with it.

The fixed bottom status line is a shared status-and-hint area. Its stable
priority order is mode, model, and context usage. While a transient hint is more
important, the line may temporarily show queue, interruption, error, or
shortcut hints. Narrow terminals shrink by priority before wrapping; the V1
status line remains a single row.

The right sidebar is a plain utility appendix. It is optional, low contrast,
local-only, and never required for the core prompt-to-answer flow. Sidebar
titles are bold; ordinary content is default or dim text unless color carries
state.

Bottom panels and pickers use selection-sheet behavior: compact header,
optional tabs only when needed, searchable row list, quiet selected-row
treatment, and a contextual footer. Slash command discovery remains a
lightweight menu above the composer instead of becoming a full command palette.

Browser switches are reserved for direct binary state such as capability,
backend, channel, debug, or mode enablement. They use a quiet neutral off track,
a clear accent on track, a raised thumb, and visible focus ring. Switch state
is carried by the control itself; adjacent visible text must name the setting
being controlled, not repeat state words such as `On`, `Off`, `Enabled`, or
`Disabled`. Ordinary checkboxes remain checkboxes when the user is selecting
multiple options, confirming force behavior, or editing form fields.

Browser controls use explicit semantic roles instead of a generic active state.
Commands, icon commands, toggles, disclosures, navigation items, tabs,
segmented choices, menus, links, and switches each expose the native ARIA state
for that role. `current`, `pressed`, `expanded`, and `selected` are never
interchangeable. Compact controls are 28px, ordinary workbench controls are
32px, and coarse-pointer layouts provide at least a 44px hit target. Ordinary
command buttons are transparent and borderless at rest in every appearance,
including the primary command in a local action group. Order, wording,
iconography, and weight communicate command priority without inverting
foreground and background colors. Caution and danger may retain their bounded
semantic tints. The Composer interrupt control is the narrow exception: it uses
a compact deep-gray fill so the active stop affordance remains stable without
presenting an ordinary interruption as an error.

Browser fields use semantic families instead of page-local native styling.
Search and filter fields use a quiet search surface; ordinary text, numeric,
secret, and select controls share one field frame; multiline descriptive input
shares that frame while retaining caller-owned height; high-entropy and
structured values may opt into the shared monospace treatment. Markdown, JSON,
file, and Composer editors keep their specialized geometry but reuse the field
color and focus roles. Checkbox and radio choices use a distinct choice-control
class and must never inherit text-field width, padding, or minimum height.

The browser signature for committed mutations is a compact ledger receipt. It
uses the shared `•` marker, states the completed action in plain language, and
may expose Undo only when the caller supplies a reliable inverse operation.
Receipts are display-only: they never become transcript entries, durable
messages, exports, tool results, accounting input, or provider context.

## Evidence Language

Evidence is inline ledger material inside the transcript. It does not use
section headers such as `Tool calls`, vertical rails, or separate activity logs.
Rows default to a short title plus the most useful detail. Long stdout, JSON,
diffs, raw data, and repeated preparation text collapse.

Tool evidence titles are tool-name first. Fullscreen ledger rows should show
the actual invocation name, such as `read path`, `exec_command rg query`,
or `write path`, rather than category verbs such as `Exploring`, `Explored`,
`Running`, `Ran`, `Updating`, or `Updated`. Active state is carried by the
activity marker, elapsed time, and body suppression rules, not by changing the
title verb.

The code model may keep coarse internal evidence kinds for grouping and style,
but those names are not user-facing design language. Legacy `Changed` naming is
not part of the design system.

Active evidence uses a bullet, elapsed time, and restrained motion. It should
not add redundant body-only lines such as `running` or `preparing` when the
title already communicates the state.

Folded evidence details may be expanded inline by mouse in V1. Keyboard users
retain the main workflow through composer, slash commands, scrolling, copy, and
display toggles, but V1 does not provide a keyboard path to expand one specific
evidence row. `/show-raw` and `/show-thinking` remain display toggles and do
not rewrite stored transcript content, copy results, or provider context.

## Interaction Model

The default model is composer-first and workbench-native. Use a small number of
global shortcuts, rely on contextual footer hints, and avoid hidden modes that
users must memorize.

V1 default keys:

- `?` opens shortcut help when supported by the active surface.
- `Ctrl+O` copies the latest visible assistant answer as raw Markdown.
- `Ctrl+R` opens composer history search.
- `Shift+Tab` cycles runtime mode between `default` and `plan`.
- PageUp/PageDown scroll the transcript unless a bottom sheet owns paging.
- Mouse wheel scrolls the region under the pointer.

The V1 keymap is fixed, but implementation should keep the key handling
organized so a future keymap configuration and conflict-checking layer can be
added without rewriting every component.

## Engineering Direction

Rendering should flow from semantic transcript/display facts to surface-native
view models and then to concrete layout. Cached terminal lines, DOM fragments,
or viewport-dependent wrapping are not durable UI facts. Concrete component
measurement, layout caching, and host-specific rendering contracts belong to
the product surface that implements them.

Design-system tests are deterministic. They should verify visual roles,
adaptive theme fallback, component layout, cache invalidation, and interaction
semantics without live providers, API keys, or terminal palette dependence.

## Attachments

- [DESIGN.md](DESIGN.md) is the canonical design-system source for prose,
  tokens, themes, glyphs, motion, and platform mappings.
- [Brand Assets](brand-assets.md) defines canonical tracked logo and brand
  asset locations and usage rules.

## Related Topics

- [005 Durable Evidence](../005-durable-evidence/spec.md) defines the evidence
  semantics that the TUI design makes inspectable.
- [070 Experience](../070-experience/spec.md) defines cross-surface UX and DX
  expectations.
- [022 UI](../022-ui/spec.md) defines shared UI ownership boundaries.
- [250 UI Display Model](../250-ui-display-model/spec.md) defines semantic
  transcript projection and display-only boundaries.
- [260 UI Rendering](../260-ui-rendering/spec.md) defines cross-surface
  rendering invariants.
- [270 UI Interaction](../270-ui-interaction/spec.md) defines cross-surface
  interaction semantics.
- [210 pevo TUI](../210-pevo-tui/spec.md) defines the fullscreen interactive
  terminal command and TUI-specific rendering/interaction behavior.
- [240 pevo Web](../240-pevo-web/spec.md) defines concrete Web/Workbench
  layout and browser behavior.
- [245 pevo Floating](../245-pevo-floating/spec.md) defines the native floating
  capsule surface that consumes scoped design-system tokens.
- [246 pevo Desktop](../246-pevo-desktop/spec.md) defines the native Desktop
  shell that imports multiple browser-facing surfaces.
