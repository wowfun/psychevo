---
name: 150. Foreign Plugin Inspection
psychevo_self_edit: deny
---

# Foreign Plugin Inspection

Define how Psychevo inspects non-native plugin packages without importing
foreign plugin code into the main Rust process.

## Scope

- package-shape detection for Codex, Claude, Hermes, and OpenCode
- dry-run inspection for local, Git, npm, and catalog-row sources
- static Hermes and OpenCode inspection
- structured diagnostics and declared-lane classification

Out of scope:

- embedding Python or Node plugin runtimes in the Rust process
- executing foreign provider clients, dashboard routes, TUI slots, themes, auth
  flows, or arbitrary commands
- installing, enabling, trusting, importing, executing, or projecting Hermes
  and OpenCode plugin declarations

## Detection

Inspection reports one detected base framework:

- `codex` for `.codex-plugin/plugin.json`
- `claude` for `.claude-plugin/plugin.json`
- `hermes` for `plugin.yaml`, `plugin.yml`, or `.hermes-plugin/plugin.yaml`
- `opencode` for OpenCode package descriptors, including package exports for
  `./server` or `./tui`
- `unknown` when no recognized package shape exists

Codex and Claude manifests use the normal manifest loader. A package may add a
root `psychevo.plugin.json` overlay for Psychevo-owned runtime, Agent, and
toolset declarations, but the overlay is never a third base-manifest shape.
Hermes and OpenCode packages are not accepted as plugin ABI packages. Their
static metadata may be inspected and displayed, but the detected source remains
inspection-only.

## Inspection Support

Hermes and OpenCode inspection reports fixed support state `inspection_only`,
package identity and metadata, the manifest path, declared lanes, unsupported
lanes, and diagnostics. It never starts a Python or Node host and never imports
package code.

## Stage Diagnostics

Inspection may include diagnostics for:

- `resolve/fetch`
- `inspect manifest`
- `compatibility`
- `target lanes`

Diagnostics are structured as status, message, and optional path. They must not
include secrets, resolved bearer tokens, provider credentials, or environment
values.

## Execution

Inspection does not create runtime declarations or an install record. A
Hermes/OpenCode install request is rejected before persistent mutation. Hermes
and OpenCode execution belongs to their separate ACP Agent runtime profiles,
not the plugin runtime.
