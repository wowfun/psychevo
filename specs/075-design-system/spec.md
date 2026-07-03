---
name: 075. Design System
psychevo_self_edit: deny
---

# 075. Design System

Define Psychevo's shared visual and interaction language. This topic owns the
high-level Adaptive Workbench direction; concrete transcript rendering,
interaction mechanics, and product layout live in the UI and product specs that
consume it.

## Scope

- shared visual language and surface hierarchy
- compact glyph/icon language, color roles, and adaptive host fallback
- core transcript, prompt, composer, status line, sidebar, picker, and bottom
  panel surface treatment
- evidence language for inline ledger rows, collapsed details, and active work
- composer-first interaction baseline and default shortcut expectations
- semantic rendering architecture expectations
- deterministic design-system validation expectations

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

- `›` marks prompt, focus, or selected rows.
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
optional tabs only when needed, searchable row list, selected row marker, and a
contextual footer. Slash command discovery remains a lightweight menu above the
composer instead of becoming a full command palette.

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
