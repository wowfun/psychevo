---
name: 054. Plugins
psychevo_self_edit: deny
---

Define product-level plugin boundaries before concrete plugin runtime and
manifest details.

## Scope

- plugin as a package, installable source, and policy-controlled capability source
- separation between install, enablement, visibility, execution, permission, and evidence
- relationship between plugin packages and existing capability-owning boundaries

Out of scope:
- concrete manifest field schemas, worker wire messages, store record schemas, or CLI flags
- hosted marketplace accounts, signatures, ratings, sharing, graphical stores, or hot reload
- in-process third-party plugin ABI
- whole-process sandboxing of workers, MCP servers, hooks, provider clients, skills, or agents

## Plugin Model

A plugin package is a directory or materialized Git source with one recognized
manifest. Installing a plugin makes a package available to policy. Installing
does not enable the plugin, make any contribution model-visible, execute worker
code, grant permissions, or create credentials.

A plugin contribution is a candidate capability declared statically in a
manifest or dynamically by a plugin worker. Candidate capabilities must be
normalized by the owning runtime boundary before use:

- skills map to 055 Skills
- MCP servers map to 056 MCP
- tool declarations, tool bindings, and toolsets map to 007 Tool Surface
- hooks map to 053 Hooks and 140 Hook Runtime
- agents and agent backends map to 051 Agents
- commands map to the shared command catalog and CLI/TUI/Web command surfaces
- providers map to the provider manager and AI protocol boundary

Plugin identity must be preserved for diagnostics, conflict handling, data-root
selection, and evidence.

Plugin hook declarations are candidate hook contributions. Installing a plugin
does not trust or run them. Runtime loads plugin hooks only when the plugin and
`hooks` capability family are enabled, then applies the hook system's
normalized-hash trust review before execution.

## Policy

Profile and project configuration declare plugin policy. The effective policy
for one invocation is the profile policy overlaid by project-local policy for
the selected cwd.

Policy can enable or disable a plugin and can enable capability families such
as skills, MCP, tools, hooks, agents, commands, providers, and worker runtime.
Enabling a plugin does not bypass permission or sandbox gates. A manifest or
worker response never grants permission.

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
- [150 Plugin Runtime](../150-plugin-runtime/spec.md) defines store, policy, worker, and contribution loading.
- [053 Hooks](../053-hooks/spec.md) defines the hook capability boundary.
- [140 Hook Runtime](../140-hook-runtime/spec.md) defines hook execution.
