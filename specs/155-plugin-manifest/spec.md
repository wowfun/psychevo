---
name: 155. Plugin Manifest
psychevo_self_edit: deny
---

Define plugin package manifest loading and validation.

## Scope

- recognized manifest paths
- native required fields and compatibility loading
- shared Codex-compatible manifest fields and Psychevo namespaced extensions
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

The shared Codex-compatible package fields are:

- `skills`
- `mcpServers`
- `hooks`
- `apps`
- `interface`

`interface.capabilities` is descriptive model/UI metadata. It is not a
permission grant, runtime capability gate, or fine-grained policy selector.

Psychevo-only plugin behavior must live under the top-level `psychevo` object.
The supported Psychevo extension fields are:

- `psychevo.runtime`
- `psychevo.commands`
- `psychevo.providers`
- `psychevo.agents`
- `psychevo.toolsets`

Unknown top-level fields are ignored with diagnostics. Unsupported supported
field shapes are invalid diagnostics and the affected declaration is skipped.

The `hooks` field declares candidate hook declarations only. Manifest loading
does not trust or execute hook handlers; hook declarations are normalized and
reviewed by 053 Hooks and 140 Hook Runtime after plugin package enablement makes
the declaration available.

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

Compatibility fields are mapped only when their semantics match Psychevo's
shared package-resource semantics. Compatibility does not imply command, hook,
app, UI, LSP, theme, or SDK runtime compatibility.

## Path Safety

All local paths in a manifest must be explicit package-relative paths:

- path starts with `./`
- path is not absolute
- path contains no `..` component
- resolved path remains inside the plugin root

Invalid paths skip the affected declaration and produce diagnostics. Runtime
must not canonicalize an invalid path into an accepted path by silently dropping
unsafe components.

## Worker Manifest Fields

`psychevo.runtime.worker` declares a Psychevo stdio worker:

```json
{
  "psychevo": {
    "runtime": {
      "worker": {
        "command": "./worker.py",
        "args": ["--stdio"]
      }
    }
  }
}
```

`command` uses the same local path safety rules. `args` are literal argv values
and do not grant shell evaluation. A top-level `runtime` field is not a
Codex-compatible worker field and must not be used for new Psychevo packages.

## Related Topics

- [054 Plugins](../054-plugins/spec.md) defines plugin package boundaries.
- [150 Plugin Runtime](../150-plugin-runtime/spec.md) defines store, policy, and worker behavior.
