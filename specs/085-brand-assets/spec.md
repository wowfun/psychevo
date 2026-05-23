---
name: 085. Brand Assets
psychevo_self_edit: deny
---

# 085. Brand Assets

Define Psychevo's tracked brand asset locations and usage rules. This topic
keeps public-facing logo assets separate from ignored local design exploration
and from the terminal-native design-system rules.

## Scope

- canonical tracked logo and brand asset locations
- README, documentation, website, package, favicon, and social-preview asset
  source rules
- derivative asset provenance expectations
- accessibility expectations for documented logo use

Out of scope:

- TUI rendering, terminal color roles, and glyph language; those belong to
  [080 Design System](../080-design-system/spec.md)
- runtime behavior, CLI commands, package publishing mechanics, or installer
  behavior
- exploratory design candidates under ignored local workspaces

## Canonical Assets

The root `assets/` directory is Psychevo's tracked public brand asset location.
The canonical logo asset is `assets/psychevo-logo.svg`.

Ignored local files under `.local/design/` may be used to explore, generate, or
stage candidate artwork, but public documentation and packaging must not
reference `.local/` paths. Once a logo is approved, it must be promoted into
`assets/` before it is used by tracked project surfaces.

Derivative brand outputs, including PNG previews, favicons, social cards, and
package registry icons, should be generated from `assets/psychevo-logo.svg`
unless a later spec explicitly replaces the canonical source.

## Usage

Tracked Markdown and documentation should reference the canonical SVG directly
when the target surface supports SVG. Logo images must use descriptive alt text,
with `Psychevo` as the default alt text for ordinary brand placement.

The canonical logo is an icon-only asset. Wordmarks, lockups, and raster
fallbacks are separate derived assets and should not replace
`assets/psychevo-logo.svg` as the source logo.

## Related Topics

- [080 Design System](../080-design-system/spec.md)
