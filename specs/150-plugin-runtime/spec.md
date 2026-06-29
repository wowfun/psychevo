---
name: 150. Plugin Runtime
psychevo_self_edit: deny
---

Define the first implementation slice for plugin installation, policy,
diagnostics, static contributions, and worker execution.

## Scope

- plugin store roots for profile and project scopes
- local directory and Git installation
- marketplace catalogs for local and Git sources
- plugin policy overlay in `RunConfig`
- static contribution loading
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

`RunConfig` includes plugin policy. Profile config and project config overlay
produce the effective plugin policy for an invocation.

Policy records plugin enabled state and capability-family enabled state. The
capability families are:

- `skills`
- `mcp`
- `tools`
- `hooks`
- `agents`
- `commands`
- `providers`
- `runtime`

CLI `install`, `enable`, and `disable` write the active profile scope by
default. `--local` writes the current cwd `.psychevo/config.toml`.
`--global` is an alias for the active profile scope and conflicts with
`--local`. Enabling a plugin enables every declared capability family unless
explicit family flags are supplied in a later spec.

When multiple installed plugins match the same selector, commands require
`name@source`.

## Contribution Loading

Runtime loads enabled plugins before agent and skill discovery. Static manifest
contributions can add:

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

Plugin hook sources are loaded only when the plugin and `hooks` capability
family are enabled. Loading a plugin hook source does not trust or execute its
handlers. Runtime passes plugin hook declarations to 140 Hook Runtime, where
they normalize to the canonical hook shape and require hook trust review before
execution.

## Worker V1

A plugin worker is an external stdio JSON-RPC process declared by the manifest.
Runtime starts workers only when the plugin and `runtime` family are enabled.

Startup context includes plugin identity, plugin root, plugin data root,
invocation scope, enabled capability families, and host capability descriptors.

Worker V1 supports:

- `initialize`
- `contributions/list`
- `tools/call`
- `hooks/call`
- `shutdown`

The first executable worker contribution is a tool. Worker tool descriptors
become `ToolBinding` adapters and enter `ToolSurfaceAssembly.extension_tools`.
Worker-provided hook handlers are candidate hook contributions for the hook
runtime handler family. A `worker` hook handler calls `hooks/call` through the
plugin worker adapter after plugin policy enables both `runtime` and `hooks`.
Missing runtime capability, missing worker process, method-not-found responses,
malformed worker responses, and worker timeouts become structured hook
diagnostics and affect only the hook run.

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
ignored fields, install source, active version, enabled state, skipped reason,
loaded capabilities, policy overlays, worker failures, and data root.

## Related Topics

- [054 Plugins](../054-plugins/spec.md) defines product plugin boundaries.
- [155 Plugin Manifest](../155-plugin-manifest/spec.md) defines manifest loading.
- [140 Hook Runtime](../140-hook-runtime/spec.md) defines hook execution and trust-aware plugin hook loading.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines command spelling.
