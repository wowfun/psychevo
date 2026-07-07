---
name: 150. Plugin Runtime
psychevo_self_edit: deny
---

Define the current implementation slice for plugin installation, package
enablement, diagnostics, static declarations, and Psychevo worker execution.

## Scope

- plugin store roots for profile and project scopes
- local directory and Git installation
- npm package materialization with lifecycle scripts disabled
- marketplace catalogs for local, Git, and npm sources
- plugin package enablement overlay in `RunConfig`
- static declaration loading
- manifest-only and adapter-host foreign package inspection
- package fingerprint trust state
- stdio JSON-RPC worker execution for tools
- CLI- and Gateway-facing read, diagnostic, install, uninstall, enable,
  disable, inspect, trust, and catalog operations

Out of scope:
- hosted marketplaces, signatures, review workflows, ratings, accounts, or graphical stores
- in-process plugin ABI or unbounded host facades
- worker hot reload, long-lived health checks, streaming worker diagnostics, or whole-process sandboxing
- provider credential storage inside plugin packages
- Codex app runtime execution, Hermes Dashboard routes, or OpenCode TUI/UI slots

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
materialize the repository or selected package path into the cache root. Npm
installs run `npm pack --ignore-scripts` into a staging directory, validate the
tarball package name and version against the requested source when supplied,
enforce archive and extracted-size limits, and copy the unpacked package into
the cache root. Deterministic tests must use local temporary Git repositories
and fake npm fixtures, not network repositories.

Install and inspect operations preserve source kind as `local`, `git`, or
`npm`. Source identity includes the package locator, selected version or Git
ref when present, and registry for npm sources when present.

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

Adapter policy records framework default mode and per-plugin mode. Supported
adapter modes are:

- `adapter_host`: allow the framework adapter host after install, enablement,
  and current fingerprint trust
- `manifest_only`: inspect static metadata and report target lanes without
  importing or executing foreign code
- `disabled`: do not inspect beyond source/manifest detection

Trust state is keyed by normalized plugin identity plus source identity and
stores the package fingerprint last approved by the user. A mismatched
fingerprint changes the readiness state to `Needs trust` and prevents adapter
host execution.

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

Plugin MCP descriptors are parsed into source-scoped MCP server inputs before
MCP startup. Invalid descriptors omit only the affected server and produce
plugin diagnostics. Accepted plugin MCP inputs still pass through MCP startup
permission, MCP tool listing, MCP tool naming, and tool-surface conflict
handling.

Plugin toolset descriptors are parsed as source-scoped toolset candidates.
They are accepted only by 007 Tool Surface after include resolution, disabled
toolset subtraction, mode filtering, and execution-binding checks. A toolset
descriptor does not create a model-visible tool by itself.

Command descriptors, provider descriptors, and apps remain inert or descriptive
in the current implementation slice unless another owning module defines
acceptance semantics. Typed interface metadata is supported for package display
only. `plugin doctor` may report inert descriptors as recognized and
unsupported without implying runtime support.

Codex app descriptors are readiness facts. They may report `Needs setup` when
the manifest declares an app/auth surface, but the runtime must not execute the
app connector or call a remote auth flow in this slice.

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

Worker tools are accepted through the shared extension assembly and tool
surface. Plugin worker tools are source-qualified as plugin tools. When
synthetic `tool_search` is enabled, direct plugin worker tools enter the router
as deferred bindings by default; explicit invocation configuration may disable
`tool_search` and expose otherwise direct worker tools directly. Plugin
manifests do not decide direct model visibility by themselves.

## Adapter Host V1

Foreign adapters run out of process and return a structured inspection result.
Psychevo owns the host protocol and maps returned declarations into existing
runtime lanes. Adapter host output must include stage diagnostics for
`resolve/fetch`, `inspect manifest`, `compatibility`, `target lanes`, `policy`,
`trust`, and `readiness`.

Hermes uses a Python adapter host. OpenCode uses a Node adapter host. Adapter
hosts may report supported tools, hooks, skills, and MCP/toolset declarations.
They may also report unsupported provider, UI/TUI, dashboard/auth, theme, route,
slot, and command-execution lanes. Unsupported lanes are displayed in read,
doctor, CLI, Gateway, and Workbench diagnostics but are not executed.

The default safe path is manifest-only inspection. Adapter host execution is
allowed only after install, enablement, policy mode `adapter_host`, and matching
package fingerprint trust.

## CLI Operations

`pevo plugin` owns:

- `list`
- `view`
- `doctor`
- `inspect`
- `install`
- `uninstall`
- `enable`
- `disable`
- `trust`
- `marketplace list`
- `marketplace add`
- `marketplace remove`

All read and diagnostic commands accept `--json` and emit secret-free
structured output. Human output is concise and action-oriented, using typed
interface metadata when present for display name, short description, category,
developer, and capabilities.

`plugin inspect` is a dry-run operation over a local path, Git source, npm
source, or catalog row. It materializes into a temporary staging directory when
needed, detects native, Codex, Hermes, and OpenCode package shapes, reports the
canonical identity, source kind, adapter framework, package fingerprint, target
lanes, unsupported lanes, readiness, and diagnostics, and does not install,
enable, trust, import foreign code, or mutate profile/project state.

`plugin trust` records trust for the current installed package fingerprint.

`plugin doctor` reports discovered packages, manifest path, supported and
ignored fields, manifest resources, Psychevo extensions, install source, source
kind, active version, fingerprint, enabled state, adapter policy/trust state,
skipped reason, owning-surface policy state, worker failures, adapter stage
diagnostics, and data root.

Gateway exposes plugin metadata and package-management methods for product
surfaces: `plugin/list`, `plugin/read`, `plugin/doctor`, `plugin/install`,
`plugin/uninstall`, `plugin/setEnabled`, `plugin/import/inspect`,
`plugin/setTrust`, `plugin/catalog/list`, `plugin/catalog/add`, and
`plugin/catalog/remove`. These methods use the same runtime helpers as the CLI,
honor resolved scope/profile rules, return typed interface metadata, and keep
responses secret-free. GUI install overwrite, trust writes, and other
force-worthy actions require an explicit request supplied by the caller.

## Related Topics

- [054 Plugins](../054-plugins/spec.md) defines product plugin boundaries.
- [155 Plugin Manifest](../155-plugin-manifest/spec.md) defines manifest loading.
- [140 Hook Runtime](../140-hook-runtime/spec.md) defines hook execution and trust-aware plugin hook loading.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines command spelling.
- [150 Plugin Runtime Adapter Hosts](./adapter-hosts.md) defines foreign adapter host inspection boundaries.
