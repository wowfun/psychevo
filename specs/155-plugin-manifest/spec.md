---
name: 155. Plugin Manifest
psychevo_self_edit: deny
---

Define plugin package manifest loading and validation.

## Scope

- recognized manifest paths
- native required fields and compatibility loading
- shared Codex-compatible manifest fields and Psychevo namespaced extensions
- descriptive detection of Hermes and OpenCode package shapes for adapter inspection
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
Hermes `plugin.yaml`/`plugin.yml` and OpenCode package descriptors are adapter
descriptors, not native manifests, and are handled by plugin inspection rather
than by the normal manifest loader.

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
Runtime parses Codex-compatible `interface` metadata into a typed display
record. Supported display fields are `displayName`, `shortDescription`,
`longDescription`, `developerName`, `category`, `capabilities`, `websiteUrl`,
`privacyPolicyUrl`, `termsOfServiceUrl`, `brandColor`, `composerIcon`, `logo`,
`logoDark`, and `screenshots`, including Codex's `*URL` aliases. Path-bearing
media fields use the same package-relative path safety rules as other manifest
resources. Invalid display fields are diagnostics and do not make the package
invalid when the core manifest remains usable.

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

The `mcpServers` field declares Codex-compatible MCP server descriptors. It may
be an object of server descriptors or a package-relative path to a JSON file. If
`mcpServers` is absent, a default `.mcp.json` file may be recognized when
present. Manifest loading records malformed server descriptors as diagnostics
without discarding valid sibling servers.

The `psychevo.toolsets` field uses the same shape as configured custom
toolsets: each key names a toolset with optional `description`, `tools`, and
`includes`. Manifest loading parses the descriptors and leaves expansion and
acceptance to 007 Tool Surface.

`psychevo.commands`, `psychevo.providers`, and `apps` may be recorded as inert
descriptors until their owning runtime modules define category-specific
acceptance semantics. `interface` is not executable, but it is supported as
typed package display metadata for CLI and Gateway read surfaces. Runtime must
not claim inert descriptors are executable or supported merely because the
manifest recognized their fields.

`apps` is a Codex-compatible descriptive readiness field in this slice. It may
produce `Needs setup` diagnostics, but it does not authorize app connector
execution, OAuth launch, remote auth calls, or UI extension loading.

## Compatibility Manifests

`.codex-plugin/plugin.json` is a first-class Codex-compatible package path.
`.claude-plugin/plugin.json` is a compatibility manifest path. They may load as
local development packages when native-required fields are missing, but install
requires a resolvable name and version.

Compatibility fields are mapped only when their semantics match Psychevo's
shared package-resource semantics. Compatibility does not imply command, hook,
app, UI, LSP, theme, or SDK runtime compatibility.

Hermes `plugin.yaml` packages and OpenCode package descriptors are adapter
inspection inputs, not executable compatibility packages in the manifest loader.
If surfaced by diagnostics, their manifest fields are descriptive only until
adapter policy, enablement, and fingerprint trust allow an out-of-process
adapter host. Psychevo must not import or execute Hermes Python `register(ctx)`
or OpenCode server/TUI modules in the Rust process.

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

Plugin manifests do not support a static `psychevo.tools` field. Executable
plugin tools must come from worker `contributions/list`, MCP tool listing, or a
future static tool path that can prove each declaration has a registered
execution binding.

## Source Metadata

Install and inspect records preserve source kind as `local`, `git`, or `npm`.
Npm package descriptors record the requested package, selected version when
known, registry when present, and the package fingerprint computed from the
materialized package contents. These fields are identity and diagnostic facts;
they are not permission grants.

## Related Topics

- [054 Plugins](../054-plugins/spec.md) defines plugin package boundaries.
- [150 Plugin Runtime](../150-plugin-runtime/spec.md) defines store, policy, and worker behavior.
- [150 Plugin Runtime Adapter Hosts](../150-plugin-runtime/adapter-hosts.md) defines foreign adapter inspection boundaries.
