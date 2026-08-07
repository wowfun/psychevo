---
name: 054. Plugins
psychevo_self_edit: deny
---

Define Psychevo's declarative Plugin package and marketplace boundary.

A Plugin is a Codex-style manifest-first distribution bundle. It is not an
executable host extension and cannot register code into Psychevo.

## Scope

- Plugin identity, marketplace ownership, and declarative package boundary
- separation of add, enablement, trust, declaration acceptance, and permission
- relationship to Codex-compatible packages and executable Extensions
- profile/project policy and immutable package/data roots

Out of scope:

- concrete manifest fields and path validation, owned by
  [155 Plugin Manifest](../155-plugin-manifest/spec.md)
- store records, marketplace materialization, and CLI operations, owned by
  [150 Plugin Runtime](../150-plugin-runtime/spec.md)
- executable sidecars and direct commands, owned by
  [058 Extensions](../058-extensions/spec.md)
- hosted accounts, signatures, ratings, reviews, or sharing
- in-process third-party runtime or frontend ABIs

## Model

A Plugin is one immutable package directory with one recognized portable base
manifest. It may bundle declarative skills, MCP server descriptors, hook
declarations, Agent roots, toolset descriptors, Apps, and interface metadata.
Every declaration is candidate material. Psychevo host code maps accepted
declarations into their owning modules; a manifest never mutates runtime state,
grants permission, starts arbitrary code, or makes content model-visible by
itself.

Plugin identity is `<plugin>@<marketplace>`. Marketplace identity is part of
the canonical identity even if two marketplaces expose identical display names
or package content. Scope qualifies an installed record only when profile and
project records would otherwise be ambiguous.

Adding a Plugin materializes the selected marketplace release and records its
fingerprint, but leaves the package disabled. Enabling is a separate explicit
action. This makes acquisition inspectable before declarations can affect an
invocation. Enablement still does not grant hook trust, MCP/tool approval,
credentials, provider access, sandbox exceptions, or durable permissions.

Plugin trust, when an owning authority requires it, is bound to the exact
package fingerprint. Replacing package contents invalidates that trust. Plugin
trust is independent of Extension trust even when both manifests are supplied
from one Extension distribution.

## Extension Relationship

A Plugin root must not contain `psychevo.extension.json`. Discovering that
manifest during Plugin add fails closed and points to `pevo install`.

An Extension may contain at most one co-root recognized Plugin base manifest.
Extension installation reports the package but neither adds nor enables it.
Users opt into the declarative package separately through the Plugin surface.
This one-way relationship allows one executable product integration to ship
adjacent skills or MCP metadata without letting an ordinary Plugin acquire a
sidecar implicitly.

Executable workers, direct CLI commands, Channel transports, and rich UI
resources that require a process belong to [058 Extensions](../058-extensions/spec.md).
Plugin manifests do not contain `runtime.worker`, executable command paths,
package-manager lifecycle hooks, or frontend module imports. MCP server
descriptors and MCP Apps remain valid declarative components because their
execution and sandbox are owned by MCP, not by a Plugin runtime ABI.

## Declaration Ownership

Supported Plugin declarations map as follows:

- skills to [055 Skills](../055-skills/spec.md)
- MCP servers and MCP Apps to [056 MCP](../056-mcp/spec.md)
- hooks to [053 Hooks](../053-hooks/spec.md) and
  [140 Hook Runtime](../140-hook-runtime/spec.md)
- Agents to [051 Agents](../051-agents/spec.md)
- toolsets to [007 Tool Surface](../007-tool-surface/spec.md)
- interface metadata to Plugin management and discovery presentation

Each owner resolves conflicts, readiness, trust, permission, execution, and
evidence. Unknown fields remain inspectable raw data and never silently gain
authority. Plugin identity stays attached to accepted declarations for
diagnostics and evidence.

Portable Codex packages use the pinned behavioral compatibility profile
`codex-plugin/8604689e`. `.codex-plugin/plugin.json` is a first-class base;
`.claude-plugin/plugin.json` is a compatibility base. Recognizing a component
does not imply executable compatibility. Each component reports its highest
compatibility level and actionable readiness.

Compatibility levels are `parse`, `inspect`, `add`, `project`, `execute`, and
`delegate`. Readiness values are `ready`, `disabled`, `needs_trust`,
`needs_auth`, `needs_setup`, `unavailable`, and `failed`. Execution owners are
existing Psychevo modules, MCP, an Extension selected independently by its own
identity, the Codex capability broker, or metadata-only presentation.

Psychevo does not execute Codex, Claude Code, Hermes, Pi, or OpenCode in-process
plugin interfaces. Hermes and OpenCode descriptors may be inspected as data
with fixed status `inspection_only`; they cannot be added, enabled, trusted,
projected, or executed as Psychevo Plugins.

## Policy

Effective Plugin policy is active-profile policy overlaid by project-local
policy for the selected canonical cwd. Profile and project policy may enable or
disable a Plugin. An owning external authority may impose a stricter rule; for
Codex authority, project policy may disable an inherited profile Plugin or
remove that override but cannot enable what the profile disallowed.

Plugin policy records package enablement only. Fine-grained decisions stay with
the effect owner:

- hook content trust stays with Hook Runtime
- MCP startup and tool approval stay with MCP and Tool Surface
- provider credentials and policy stay with provider management
- filesystem, process, network, and local writes stay with permission policy

Project-local policy must not mutate inactive profile state or select a
different profile. Disabling a Plugin prevents new invocation leases or
snapshots while already admitted work completes under its frozen selection.

## Storage

Profile Plugin stores live under:

```text
$PSYCHEVO_HOME/plugins/{cache,data}
```

Project Plugin stores live under:

```text
<cwd>/.psychevo/plugins/{cache,data}
```

The cache is immutable code and assets. The identity-qualified data root is the
only Plugin-owned writable state. Plugin code never treats the cache as mutable
runtime state. Marketplace materialization uses staging, bounded extraction,
content fingerprinting, and atomic replacement. Failure leaves the existing
record and policy unchanged.

Codex-owned catalog packages remain owned by Codex. Psychevo preserves their
authority-qualified identity and may read an authority-exposed installed root
in place or delegate service-owned components, but never mirrors that package
into the Psychevo cache or imports the user's active Codex configuration.

## Evidence And Diagnostics

Plugin surfaces must be able to explain:

- marketplace, selected version, materialized fingerprint, and scope
- whether the package is added, enabled, trusted when applicable, and ready
- which declarations were accepted, disabled, invalid, unsupported, omitted by
  conflict, or unavailable through their owner
- which hooks require new trust and which MCP/App components require auth
- whether a co-root Extension manifest caused Plugin add to fail

Diagnostics remain secret-free and payload-light. Ordinary transcript history
does not persist full manifests, unused declaration inventories, credentials,
or package contents.

## Related Topics

- [050 Capability Extensions](../050-capability-extensions/spec.md) defines
  source acceptance and immutable runtime assembly.
- [058 Extensions](../058-extensions/spec.md) defines executable packages.
- [150 Plugin Runtime](../150-plugin-runtime/spec.md) defines stores,
  marketplaces, policy operations, and declaration loading.
- [155 Plugin Manifest](../155-plugin-manifest/spec.md) defines manifest shape.
