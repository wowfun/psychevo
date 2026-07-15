---
name: 150. Plugin Runtime Adapter Hosts
psychevo_self_edit: deny
---

# Plugin Runtime Adapter Hosts

Define how Psychevo inspects non-native plugin packages without importing
foreign plugin code into the main Rust process.

## Scope

- package-shape detection for Codex, Claude, Hermes, and OpenCode
- dry-run inspection for local, Git, npm, and catalog-row sources
- manifest-only inspection before trust
- out-of-process Hermes Python and OpenCode Node adapter hosts
- structured diagnostics and projection into existing Psychevo lanes

Out of scope:

- embedding Python or Node plugin runtimes in the Rust process
- executing foreign provider clients, dashboard routes, TUI slots, themes, auth
  flows, or arbitrary commands
- hot reload or current-session mutation after install, enable, or trust writes

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
Hermes and OpenCode packages are not accepted as in-process ABI packages.
Their static metadata may be inspected and displayed even when adapter
execution is disabled.

## Adapter Modes

`manifest_only` is the safe default for foreign frameworks. It reports package
identity, static target lanes, unsupported lanes, and readiness without running
foreign code.

`adapter_host` may run only when the package is installed, enabled, and trusted
for its current fingerprint. The host process receives the package root and a
minimal read-only inspection request, then returns declarations for supported
lanes and diagnostics for unsupported lanes.

`disabled` skips framework-specific inspection and reports only source,
fingerprint, and package-shape diagnostics.

## Stage Diagnostics

Every inspection result includes stage diagnostics for:

- `resolve/fetch`
- `inspect manifest`
- `compatibility`
- `target lanes`
- `policy`
- `trust`
- `readiness`

Diagnostics are structured as status, message, and optional path. They must not
include secrets, resolved bearer tokens, provider credentials, or environment
values.

## Projection

Adapter host declarations enter existing owning lanes:

- tools use the normal plugin runtime tool path and shared tool surface
- hooks use 053 Hooks and 140 Hook Runtime trust review
- skills use 055 Skills roots or generated read-only adapter skill descriptors
- MCP/toolsets use 056 MCP and 007 Tool Surface

Provider execution, UI/TUI routes, slots, themes, dashboard auth, and arbitrary
command execution remain unsupported diagnostics until those owning modules
define acceptance semantics.
