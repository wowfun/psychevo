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
- reimplementing hosted Codex Apps without their owning service or credentials

## Plugin Model

A plugin package is a directory, materialized Git source, or materialized npm
package with one recognized Psychevo or Codex manifest. A recognized Hermes or
OpenCode descriptor is an inspection source, not an installable Psychevo plugin.
Installing a Psychevo-owned plugin makes a package available to policy but does
not enable or trust it. Installing a Codex-authority plugin is one explicit GUI
operation whose successful result enables the authority-qualified package in
the active profile and trusts that exact installed fingerprint. Neither path
makes a declaration model-visible, bypasses an owning runtime policy, grants
permission, or creates connector credentials.

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

Codex-compatible manifest fields keep the pinned behavioral contract
`codex-plugin/8604689e`. `.codex-plugin` packages are first-class compatibility
packages. `skills`, `mcpServers`, `hooks`, `apps`, and `interface.*` are parsed,
inspected, installed, projected, and either executed natively or delegated to
their owning Codex runtime. Each component reports its highest compatibility
level and readiness; recognizing a field is never reported as executable
compatibility.

The Codex manifest is the portable package base. Optional Psychevo-only
behavior lives in a companion `psychevo.plugin.json` overlay and may declare
only Psychevo-owned worker, agent, and toolset sources. The overlay must not
repeat or replace shared Codex components. Duplicate declarations make the
overlay invalid instead of creating a second precedence system.

Plugin hook declarations are candidate hook declarations. Installing or enabling
a plugin does not trust or run them. Runtime passes accepted plugin hook
declarations to the hook runtime, then applies the hook system's normalized-hash
trust review before execution.

## Policy

Profile and project configuration declare plugin policy. The effective policy
for one invocation is the profile policy overlaid by project-local policy for
the selected cwd.

Codex-authority policy is intentionally asymmetric. A profile may enable or
disable `codex:<plugin>@<marketplace>`. Project policy may only disable an
inherited Codex plugin or remove its override; a project must never enable a
plugin that its profile did not allow. The Codex authority itself is a
profile-only, default-off feature and project configuration must not contain its
feature or binary selection.

Hermes and OpenCode descriptors have no plugin execution policy or plugin trust
state. They may be inspected statically without importing or executing foreign
runtime code. Inspection does not make the source installable, enabled, or
executable.

An explicit Codex install or upgrade trusts only the fingerprint returned by
that operation. Background content changes, externally performed mutations, or
an unexpected Codex version invalidate that trust rather than silently
extending it.

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

The cache root contains materialized packages from local, Git, and npm sources.
Npm package materialization must use an install-time staging directory and must
not run lifecycle scripts. The data root is the only plugin-owned writable state
directory granted by plugin identity. Runtime must not treat the install cache
as mutable plugin state.

## Compatibility

Psychevo preserves three immutable layers: the raw package document, the
normalized `codex-plugin/8604689e` package, and the effective turn projection.
Unknown data remains attached to the raw document; it does not silently become
runtime authority. A package is fully compatible only when every declared
component reaches `execute` or `delegate`, or reports an explicit actionable
readiness blocker.

Compatibility levels are `parse`, `inspect`, `install`, `project`, `execute`,
and `delegate`. Execution owners are Psychevo native modules, the isolated
Psychevo worker, the Codex capability broker, or metadata-only presentation.
Readiness values are `ready`, `disabled`, `needs_trust`, `needs_auth`,
`needs_setup`, `unavailable`, and `failed`.

Codex catalog packages remain owned by Codex. Psychevo aggregates them with
profile/project packages in one management surface but keeps authority-qualified
identity and delegates catalog mutation to Codex. It does not mirror a Codex
package into the Psychevo cache.

Runtime may read a Codex-owned installed package in place when `plugin/read` or
Codex hook metadata exposes its materialized root. The resulting selected root
keeps Codex plugin and marketplace authority in its turn identity. This is a
read-only projection, not a second installation. When no root is exposed,
component status must not claim native execution; effective MCP servers may be
delegated to Codex, while path-backed skills or hooks report unavailable.

Codex compatibility is an external capability authority, not an import of the
user's active Codex environment. It uses an external reviewed binary with the
private home `$PSYCHEVO_HOME/codex/`, ignores inherited `CODEX_HOME`, and never
mutates the plugin, marketplace, or configuration state used by Codex CLI or
third-party applications.

Psychevo does not execute Codex, Claude Code, Hermes, Pi, or OpenCode in-process
plugin interfaces directly.

Hermes or OpenCode manifests may be inspected as data. Inspection reports the
framework, package metadata, manifest path, declared lanes, unsupported lanes,
and diagnostics with fixed support state `inspection_only`. Psychevo does not
materialize, install, enable, trust, import, execute, or project those foreign
plugin declarations. Users who want to run Hermes or OpenCode use their
separate ACP Agent runtime profiles.

## Related Topics

- [155 Plugin Manifest](../155-plugin-manifest/spec.md) defines package manifest loading.
- [150 Plugin Runtime](../150-plugin-runtime/spec.md) defines store, policy, worker, and declaration loading.
- [150 Foreign Plugin Inspection](../150-plugin-runtime/foreign-inspection.md)
  defines foreign package inspection boundaries.
- [053 Hooks](../053-hooks/spec.md) defines the hook declaration boundary.
- [140 Hook Runtime](../140-hook-runtime/spec.md) defines hook execution.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines
  source/declaration boundaries and runtime extension registry mapping.
