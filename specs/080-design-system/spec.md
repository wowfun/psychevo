---
name: 080. Design System
psychevo_self_edit: deny
---

# 080. Design System

Define Psychevo's shared visual and interaction system. This topic is the
source of truth for TUI surface language; implementation-specific TUI rendering
lives in [211 pevo TUI Rendering](../211-pevo-tui-rendering/spec.md), and
interaction behavior lives in
[212 pevo TUI Interaction](../212-pevo-tui-interaction/spec.md).

## Scope

- shared TUI visual language and surface hierarchy
- compact glyph language, color roles, and adaptive terminal fallback
- core transcript, prompt, composer, status line, sidebar, picker, and bottom
  panel surface treatment
- evidence language for inline ledger rows, collapsed details, and active work
- composer-first interaction baseline and default shortcut expectations
- internal measurement-and-rendering contract for TUI components
- deterministic design-system validation expectations

Out of scope:

- concrete TUI command behavior, slash parsing, file completion, and selection
  mechanics; these belong to [212 pevo TUI Interaction](../212-pevo-tui-interaction/spec.md)
- transcript layout, evidence projection, sidebar composition, and terminal
  rendering implementation; these belong to
  [211 pevo TUI Rendering](../211-pevo-tui-rendering/spec.md)
- durable session, model, and fullscreen command state; these belong to
  [210 pevo TUI](../210-pevo-tui/spec.md)
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

## Engineering Contract

TUI components use a small internal measurement-and-rendering contract:

- `desired_height(width)` reports the rows needed for the current state.
- `render(area, buf)` draws into the provided terminal area.
- Components that own cursor state may report cursor position and style.

This is an internal Rust TUI contract, not a public API. Shared list surfaces
should build a display row model for measurement and rendering. Transcript
rendering should flow from semantic rows to render blocks/view models and then
to layout measurement cache. Do not make cached Ratatui `Line` or `Text`
objects the primary architecture; prefer stable row ids, viewport-intersecting
blocks, measured heights, and shared column measurements.

Design-system tests are deterministic. They should verify visual roles,
adaptive theme fallback, component layout, cache invalidation, and interaction
semantics without live providers, API keys, or terminal palette dependence.

## Related Topics

- [005 Durable Evidence](../005-durable-evidence/spec.md) defines the evidence
  semantics that the TUI design makes inspectable.
- [070 Experience](../070-experience/spec.md) defines cross-surface UX and DX
  expectations.
- [210 pevo TUI](../210-pevo-tui/spec.md) defines the fullscreen interactive
  terminal command and shared TUI state.
- [211 pevo TUI Rendering](../211-pevo-tui-rendering/spec.md) defines concrete
  transcript, status-line, sidebar, and terminal rendering behavior.
- [212 pevo TUI Interaction](../212-pevo-tui-interaction/spec.md) defines
  concrete input handling, slash commands, and selection behavior.
