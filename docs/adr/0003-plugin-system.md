---
name: 0003. Plugin System
status: proposed
date: 2026-06-26
psychevo_self_edit: deny
---

## Context

Psychevo is pre-release and can still refactor the extension surface before it
hardens into product behavior. ADR 0002 defines the lower-level capability
contribution mechanism: source identity, contribution selection, conflicts,
visibility, dispatch, hooks, and evidence. That mechanism is necessary, but it
does not define plugin packages, marketplaces, installation, runtime extension
processes, or user-facing plugin policy.

The reference systems point to different useful boundaries. Codex and Claude
Code show that plugin packages should be manifest-first, path-constrained, and
marketplace-ready. OpenCode and Hermes show that contributor ergonomics matter,
but their direct in-process runtime APIs would give third-party code too much
ambient authority for Psychevo's first public plugin surface. MCP is a useful
tool and protocol boundary, but it is not broad enough to be the only plugin
runtime API because plugins may also contribute hooks, providers, commands,
agents, and toolsets.

Psychevo therefore needs a product-level plugin system above capability
contributions. The system should let plugins be installed, enabled, diagnosed,
and updated while ensuring that every contributed capability still passes
through the owning runtime boundary, permission policy, sandbox status, and
evidence model.

Specifications remain self-contained. This ADR records architectural decisions;
future specs must define concrete manifest schemas, CLI/API surfaces, storage
schemas, command names, hook payloads, and worker protocol messages without
depending on this ADR as normative text.

## Decision

Psychevo will add a plugin system as a first-class product layer. The plugin
system has four ordered layers:

1. Marketplace and source catalogs discover plugin packages from local and Git
   sources.
2. Installed plugin stores copy or materialize selected packages into profile
   or project plugin cache roots and create plugin data roots.
3. External-process plugin runtimes register dynamic capabilities through a
   Psychevo-owned stdio JSON-RPC worker protocol.
4. Runtime capability contribution assembly normalizes all plugin-provided
   skills, MCP servers, tools, hooks, agents, backends, commands, toolsets, and
   providers before any capability becomes visible or executable.

Plugin installation is separate from activation, enablement, visibility, and
execution permission. Installing a package only makes it available to policy.
Profile and project configuration decide whether the plugin and each relevant
capability family are enabled for an invocation. Permission and sandbox policy
remain separate runtime gates after a capability is selected.

The first version includes marketplace support for local directories and Git
sources. It does not include hosted accounts, signing, review workflows,
ratings, sharing services, remote registry ownership, or a graphical plugin
store.

## Package And Manifest

The Psychevo-native plugin manifest path is:

```text
.psychevo-plugin/plugin.json
```

Psychevo also recognizes these compatibility manifest paths:

```text
.codex-plugin/plugin.json
.claude-plugin/plugin.json
```

The native manifest requires `name`, `version`, and `description`. A
compatibility manifest may load as a local development package when it lacks
native-required fields, but marketplace installation requires a resolvable
plugin name and version. Compatibility does not mean ABI compatibility:
Psychevo consumes only fields it explicitly maps and reports unsupported fields
as diagnostics when practical.

The supported manifest field families are:

- `skills`
- `mcpServers`
- `tools`
- `hooks`
- `agents`
- `agentBackends`
- `commands`
- `toolsets`
- `providers`
- `runtime`
- `interface`

`interface` is display metadata only in V1. It may describe name, category,
capabilities, default prompts, icons, screenshots, and URLs for local UIs or
marketplace lists, but it does not create executable capability or permission
authority.

All local manifest paths must be explicit package-relative paths. They must
start with `./`, must not contain `..`, must not be absolute, and must resolve
inside the plugin root. Default capability paths may be defined by a later
spec, but any default is still resolved under the same plugin root.

## Marketplace And Store

Marketplace catalogs are discovery documents. V1 catalogs may live on the
local filesystem or in Git sources. A catalog entry may name a plugin package
path, a Git URL, an optional ref, and product metadata. Catalog presence does
not install, enable, start, or trust a plugin.

Installed profile plugins live under the active profile home:

```text
$PSYCHEVO_HOME/plugins/cache
$PSYCHEVO_HOME/plugins/data
```

Installed project plugins live under the current project overlay:

```text
<workdir>/.psychevo/plugins/cache
<workdir>/.psychevo/plugins/data
```

The cache root contains materialized plugin packages. The data root is the only
plugin-owned writable state directory granted by plugin identity. A plugin must
not treat its install cache as writable runtime state.

Version selection and update semantics are owned by the plugin store. The
store must preserve source identity, plugin identity, version identity,
install scope, and data root identity so runtime diagnostics can explain which
package contributed a capability. Because Psychevo is pre-release, the first
implementation may reset or rebuild development plugin stores instead of
carrying migrations for unstable internal layouts.

## Configuration And Policy Overlay

Plugin packages declare possible capabilities. Profile and project
configuration declare policy. The overlay model must support:

- plugin enabled or disabled state
- capability-family enablement, for example skills, MCP, hooks, agents,
  commands, providers, or runtime workers
- per-MCP startup and tool policy
- per-tool or tool-family approval policy
- provider adapter enablement without embedding credentials in the plugin
- project-local overrides that do not mutate inactive profiles

Project configuration overlays profile configuration for the current workdir,
following the existing profile and project config boundary. Profile-global
state remains profile-local. Workdir-local plugin state remains under
`<workdir>/.psychevo` and must not select or override the active profile.

No plugin manifest, marketplace entry, or runtime worker response grants
permission. Permission policy in 041 decides whether a selected operation may
run. Sandbox policy in 045 describes what is and is not confined when a selected
operation runs.

## Runtime API

Third-party executable plugin code runs out of process in V1. The runtime API
is a Psychevo-owned stdio JSON-RPC worker protocol, not a direct Rust, Python,
or JavaScript in-process registration API.

The worker process receives explicit startup context such as plugin identity,
plugin root, plugin data root, invocation scope, enabled capability families,
and host capability descriptors. It may return typed contribution descriptors
for the capability families enabled by policy. It may not mutate the registry,
provider payload, session state, permissions, sandbox policy, or persistent
configuration directly.

Worker-provided tools, hooks, providers, commands, and agent backends are
candidate contributions. Runtime must normalize them through the same source
identity, conflict, selection, visibility, dispatch, permission, and evidence
vocabulary used for built-in and configured capabilities.

MCP remains a supported capability source and may be contributed by plugins,
but MCP is not the only plugin runtime API. MCP servers contribute MCP protocol
objects, mainly tools in the first slice. The Psychevo worker protocol
contributes broader runtime extension metadata and may itself request that
runtime start or expose MCP sources.

Worker lifecycle is not hot reload in V1. A later spec may define restart,
healthcheck, streaming diagnostics, and shutdown behavior. Until then, workers
should be treated as activation-scoped helper processes whose failures degrade
or omit their contributions rather than crashing the host.

## Capability Mapping

Plugin-provided capabilities do not own their final semantics. Each family maps
into the existing owning boundary:

- Skills map to 055 Skills.
- MCP sources and MCP tools map to 056 MCP.
- Tool declarations, execution bindings, and toolsets map to 007 Tool Surface.
- Agent definitions, peer-agent backends, selected-agent policy, skills, MCP
  scope, and hooks map to 051 Agents.
- Provider adapters map to the provider manager and AI protocol boundary; the
  provider manager keeps provider/model resolution and credential handling.
- Commands map to the runtime command catalog and shared slash-command surface;
  they are not frontend allowlists and do not grant permissions.
- Permissions map to 041 Permissions.
- Sandbox status and confinement limits map to 045 Sandbox.

Hooks are plugin capabilities only after runtime accepts them as typed
contributions. A hook may observe, request, or contribute through the owning
boundary for that hook point. Tool hooks may affect the current tool call only
within the tool hook contract; they must not rewrite provider payloads, mutate
future capability snapshots, or persist new permission grants.

Provider adapters may be contributed by plugins, but secrets and credentials
remain outside plugin packages. A provider plugin may declare required
environment variables, setup guidance, and provider metadata. Runtime and the
provider manager decide whether the adapter is enabled and whether the selected
model can use it.

## Compatibility

Psychevo aims for compatibility at package-entry and capability-subset level,
not full runtime compatibility.

Claude Code and Codex compatibility means Psychevo can read compatible
manifest paths and map supported fields such as skills, MCP servers, hooks, and
display metadata where semantics align. It does not mean Psychevo can execute
Claude Code commands, agents, LSP servers, monitors, themes, settings, or
other host-specific components without an explicit Psychevo mapping.

OpenCode and Hermes plugins are not runtime-compatible. Their JavaScript,
TypeScript, and Python registration APIs must be adapted through a Psychevo
worker, MCP server, skill package, command, or agent backend. Shared business
logic may be reused, but plugin packages are not directly executable across
hosts.

## Security And Evidence

The plugin system must fail closed at trust boundaries:

- an installed plugin is not automatically enabled
- an enabled plugin is not automatically model-visible
- a model-visible tool is not execution approval
- a worker process is not sandboxed merely because it is a plugin
- plugin package metadata is not a credential source

Sandbox V1 does not confine MCP servers, plugin workers, provider clients,
skill loading, agent loading, hooks, LSP helpers, or managed helper installers
as whole processes. Status and diagnostics must describe these helper paths as
not confined unless a later sandbox spec provides real enforcement.

Evidence should stay compact. Runtime should record selected plugin identity,
source scope, visible capability names, omitted conflicting contributions,
degraded or unavailable sources that affected assembly, worker startup or
registration failures, permission outcomes, and dispatch trace summaries when
they affect an invocation. Runtime should not persist every discovered catalog
candidate or full manifest payload by default.

## Non-Goals

V1 does not define:

- hosted marketplace accounts, ratings, reviews, signatures, or moderation
- remote plugin sharing service or publishing workflow
- Workbench or TUI route, slot, theme, or layout plugin runtime
- in-process third-party plugin ABI
- stable JSON-RPC worker wire schema
- plugin hot reload
- plugin-provided credential storage
- whole-process sandboxing for plugin workers or MCP servers

## Milestones

1. Add manifest discovery and diagnostics for Psychevo-native, Codex-compatible,
   and Claude-compatible plugin roots.
2. Add profile and project plugin stores for local and Git source installation,
   with install separate from enablement.
3. Add profile/project policy overlay for plugin and capability-family
   enablement.
4. Add the external-process worker protocol behind runtime-owned capability
   normalization.
5. Map plugin-provided skills, MCP, tools, hooks, agents, backends, commands,
   toolsets, and providers into their existing owning runtime boundaries.
6. Add compact diagnostics and evidence for selected, omitted, degraded, and
   failed plugin contributions.

## Tradeoffs

This design is heavier than a direct `register(ctx)` API. It requires manifest
validation, stores, policy overlay, worker lifecycle, and capability
normalization before third-party code can be useful. The benefit is that
Psychevo can expose a plugin ecosystem without letting packages bypass
permissions, sandbox status, provider resolution, context projection, or
durable evidence.

Prioritizing a Psychevo worker API over pure Codex or Claude Code package
compatibility means some external plugins need adapters. The benefit is that
Psychevo can still read compatible package metadata and share skills or MCP
servers while keeping its own runtime contract coherent.

Including marketplace decisions in the first plugin ADR increases the product
surface early. The benefit is that installation, activation, data roots, and
diagnostics are designed together instead of being retrofitted after local
development plugins already depend on accidental paths.
