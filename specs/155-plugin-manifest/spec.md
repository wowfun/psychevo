---
name: 155. Plugin Manifest
psychevo_self_edit: deny
---

Define plugin package manifest loading and validation.

## Scope

- recognized manifest paths
- native required fields and compatibility loading
- supported field families
- path safety for local package resources
- diagnostics for supported, ignored, and invalid fields

Out of scope:
- installation, catalogs, store records, policy overlays, worker lifecycle, or CLI commands
- compatibility with foreign in-process plugin ABIs
- hosted marketplace metadata validation beyond local manifest fields

## Manifest Paths

Runtime checks these paths in order from a package root:

```text
.psychevo-plugin/plugin.json
.codex-plugin/plugin.json
.claude-plugin/plugin.json
```

The first existing path wins. If more than one recognized manifest exists,
runtime loads the first path and reports the others as ignored diagnostics.

## Native Manifest

A native manifest requires:

- `name`
- `version`
- `description`

The supported field families are:

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

Unknown fields are ignored with diagnostics. Unsupported supported-field shapes
are invalid diagnostics and the affected contribution is skipped.

The `hooks` field declares candidate hook contributions only. Manifest loading
does not trust or execute hook handlers; hook declarations are normalized and
reviewed by 053 Hooks and 140 Hook Runtime after plugin policy enables the
plugin and `hooks` capability family.

Plugin hooks may be declared inline with the canonical hook object shape or by
package-relative paths listed under `hooks`. A default `hooks/hooks.json` file
is also recognized when present. Path-based hook files use the same path safety
rules as other local plugin resources. Loading a hook file does not trust the
hook definition.

## Compatibility Manifests

`.codex-plugin/plugin.json` and `.claude-plugin/plugin.json` are compatibility
manifest paths. They may load as local development packages when native-required
fields are missing, but marketplace install requires a resolvable name and
version.

Compatibility fields are mapped only when their semantics match Psychevo field
families. Compatibility does not imply command, hook, app, UI, LSP, theme, or
SDK runtime compatibility.

## Path Safety

All local paths in a manifest must be explicit package-relative paths:

- path starts with `./`
- path is not absolute
- path contains no `..` component
- resolved path remains inside the plugin root

Invalid paths skip the affected contribution and produce diagnostics. Runtime
must not canonicalize an invalid path into an accepted path by silently dropping
unsafe components.

## Worker Manifest Fields

`runtime.worker` declares a stdio worker:

```json
{
  "runtime": {
    "worker": {
      "command": "./worker.py",
      "args": ["--stdio"]
    }
  }
}
```

`command` uses the same local path safety rules. `args` are literal argv values
and do not grant shell evaluation.

## Related Topics

- [054 Plugins](../054-plugins/spec.md) defines plugin package boundaries.
- [150 Plugin Runtime](../150-plugin-runtime/spec.md) defines store, policy, and worker behavior.
