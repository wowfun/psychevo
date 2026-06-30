---
name: 150. Plugin Runtime
psychevo_self_edit: deny
---

Define the current implementation slice for plugin installation, package
enablement, diagnostics, static declarations, and Psychevo worker execution.

## Scope

- plugin store roots for profile and project scopes
- local directory and Git installation
- marketplace catalogs for local and Git sources
- plugin package enablement overlay in `RunConfig`
- static declaration loading
- stdio JSON-RPC worker execution for tools
- CLI-facing read, diagnostic, install, uninstall, enable, disable, and catalog operations

Out of scope:
- hosted marketplaces, signatures, review workflows, ratings, accounts, or graphical stores
- in-process plugin ABI or unbounded host facades
- worker hot reload, long-lived health checks, streaming worker diagnostics, or whole-process sandboxing
- provider credential storage inside plugin packages

## Store

A plugin store is scoped to either the active profile home or the current
cwd. Profile stores use `$PSYCHEVO_HOME/plugins/{cache,data}`. Project
stores use `<cwd>/.psychevo/plugins/{cache,data}`.

An install record preserves:

- plugin name
- version
- source identity
- install scope
- package root
- data root
- manifest path and manifest kind
- diagnostics

Local directory installs copy the package into the cache root. Git installs
materialize the repository or selected package path into the cache root.
Deterministic tests must use local temporary Git repositories, not network
repositories.

Uninstall removes the install record and cache materialization for that scope.
V1 may leave the data root unless the command explicitly removes data in a
future spec.

## Policy Overlay

`RunConfig` includes plugin package policy. Profile config and project config
overlay produce the effective plugin policy for an invocation.

Plugin policy records package enabled state only. Enabling a package makes its
accepted declarations available to the host-owned mapping step. The owning
runtime module then decides whether the effect is usable:

- MCP server and tool approval policy belongs to MCP and tool surfaces.
- hook trust belongs to 140 Hook Runtime.
- provider policy and credentials belong to provider management.
- sandbox and permission decisions belong to permission runtime.

CLI `install`, `enable`, and `disable` write the active profile scope by
default. `--local` writes the current cwd `.psychevo/config.toml`.
`--global` is an alias for the active profile scope and conflicts with
`--local`.

When multiple installed plugins match the same selector, commands require
`name@source`.

## Declaration Loading

Runtime loads enabled plugins before agent and skill discovery. Static manifest
declarations can add:

- skill roots
- MCP server descriptors
- hook sources
- agent roots
- command descriptors
- toolset descriptors
- provider descriptors

The owning runtime boundary decides whether each descriptor is usable. Duplicate
model-visible tool names are conflict-resolved by the existing tool surface
rules, with plugin identity included in diagnostics.

Plugin hook sources are loaded only when the plugin package is enabled. Loading
a plugin hook source does not trust or execute its handlers. Runtime passes
plugin hook declarations to 140 Hook Runtime, where they normalize to the
canonical hook shape and require hook trust review before execution.

## Worker V1

A plugin worker is an external stdio JSON-RPC process declared by Psychevo
manifest metadata under `psychevo.runtime`. Runtime starts workers only when the
plugin package is enabled and an owning runtime path needs the worker.

Startup context includes plugin identity, plugin root, plugin data root,
invocation scope, manifest resources, Psychevo extension metadata, and host
capability descriptors.

Worker V1 supports:

- `initialize`
- `contributions/list`
- `tools/call`
- `hooks/call`
- `shutdown`

The first executable worker declaration is a tool. Worker tool descriptors
become `ToolBinding` adapters and enter `ToolSurfaceAssembly.extension_tools`.
Worker-provided hook handlers are candidate hook declarations for the hook
runtime handler family. A `worker` hook handler calls `hooks/call` through the
plugin worker adapter after package enablement and hook trust review select that
handler. Missing worker metadata, missing worker process, method-not-found
responses, malformed worker responses, and worker timeouts become structured
hook diagnostics and affect only the hook run.

## CLI Operations

`pevo plugin` owns:

- `list`
- `view`
- `doctor`
- `install`
- `uninstall`
- `enable`
- `disable`
- `marketplace list`
- `marketplace add`
- `marketplace remove`

All read and diagnostic commands accept `--json` and emit secret-free
structured output. Human output is concise and action-oriented.

`plugin doctor` reports discovered packages, manifest path, supported and
ignored fields, manifest resources, Psychevo extensions, install source, active
version, enabled state, skipped reason, owning-surface policy state, worker
failures, and data root.

## Related Topics

- [054 Plugins](../054-plugins/spec.md) defines product plugin boundaries.
- [155 Plugin Manifest](../155-plugin-manifest/spec.md) defines manifest loading.
- [140 Hook Runtime](../140-hook-runtime/spec.md) defines hook execution and trust-aware plugin hook loading.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines command spelling.
