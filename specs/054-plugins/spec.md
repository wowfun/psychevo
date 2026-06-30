---
name: 054. Plugins
psychevo_self_edit: deny
---

Define product-level plugin package boundaries before concrete plugin runtime
and manifest details.

## Scope

- plugin as a manifest-first package, installable source, and policy-controlled
  declaration source
- separation between install, enablement, visibility, execution, permission, and evidence
- relationship between plugin packages, host-owned declaration mapping, and the
  runtime extension registry

Out of scope:
- concrete manifest field schemas, worker wire messages, store record schemas, or CLI flags
- hosted marketplace accounts, signatures, ratings, sharing, graphical stores, or hot reload
- in-process third-party plugin ABI
- whole-process sandboxing of workers, MCP servers, hooks, provider clients, skills, or agents

## Plugin Model

A plugin package is a directory or materialized Git source with one recognized
manifest. Installing a plugin makes a package available to policy. Installing
does not enable the plugin, make any declaration model-visible, execute worker
code, trust hooks, grant permissions, create credentials, or mutate the runtime
extension registry.

A plugin declaration is candidate material declared statically in a manifest or
reported by a runtime helper such as a Psychevo worker. Candidate declarations
must be mapped by Psychevo host code before use:

- skills map to 055 Skills
- MCP servers map to 056 MCP
- tool declarations, tool bindings, and toolsets map to 007 Tool Surface
- hooks map to 053 Hooks and 140 Hook Runtime
- agents and agent backends map to 051 Agents
- commands map to the shared command catalog and CLI/TUI/Web command surfaces
- providers map to the provider manager and AI protocol boundary
- accepted runtime effects map into the typed contributors defined by 050
  Runtime Extension Registry

Plugin identity must be preserved for diagnostics, conflict handling, data-root
selection, and evidence.

Codex-compatible manifest fields keep their Codex semantics. `skills`,
`mcpServers`, `hooks`, `apps`, and `interface.*` declare package resources or
model/UI metadata. Psychevo-only plugin behavior must live under a Psychevo
namespace such as `psychevo.runtime`; it must not redefine a shared Codex field
as executable authority.

Plugin hook declarations are candidate hook declarations. Installing or enabling
a plugin does not trust or run them. Runtime passes accepted plugin hook
declarations to the hook runtime, then applies the hook system's normalized-hash
trust review before execution.

## Policy

Profile and project configuration declare plugin policy. The effective policy
for one invocation is the profile policy overlaid by project-local policy for
the selected cwd.

Policy can enable or disable a plugin package. Enabling a plugin makes its
accepted declarations available to the owning runtime modules, but it does not
bypass permission, hook trust, tool approval, MCP policy, provider policy, or
sandbox gates. A manifest or worker response never grants permission.

Fine-grained policy belongs to the runtime module that owns the effect. For
example, MCP server/tool policy belongs to MCP and tool approval surfaces; hook
trust belongs to the hook runtime; provider credentials and provider policy
belong to provider management. Plugin policy must not grow per-declaration
gates that duplicate those owning surfaces.

Profile-global state remains profile-local. Project-local plugin state remains
under the current cwd's `.psychevo` tree and must not select or mutate the
active profile.

## Storage

Profile plugin stores live under:

```text
$PSYCHEVO_HOME/plugins/cache
$PSYCHEVO_HOME/plugins/data
```

Project plugin stores live under:

```text
<cwd>/.psychevo/plugins/cache
<cwd>/.psychevo/plugins/data
```

The cache root contains materialized packages. The data root is the only
plugin-owned writable state directory granted by plugin identity. Runtime must
not treat the install cache as mutable plugin state.

## Compatibility

Psychevo can recognize native Psychevo plugin manifests and selected
compatibility manifest paths. Compatibility means package-entry and field-subset
compatibility only. Psychevo does not execute Codex, Claude Code, Hermes, or
OpenCode in-process plugin APIs directly.

Hermes or OpenCode plugin business logic may be adapted into a Psychevo worker,
skill, MCP server, command, or agent backend.

## Related Topics

- [155 Plugin Manifest](../155-plugin-manifest/spec.md) defines package manifest loading.
- [150 Plugin Runtime](../150-plugin-runtime/spec.md) defines store, policy, worker, and declaration loading.
- [053 Hooks](../053-hooks/spec.md) defines the hook declaration boundary.
- [140 Hook Runtime](../140-hook-runtime/spec.md) defines hook execution.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines
  source/declaration boundaries and runtime extension registry mapping.
