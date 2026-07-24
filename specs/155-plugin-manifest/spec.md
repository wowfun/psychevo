---
name: 155. Plugin Manifest
psychevo_self_edit: deny
---

Define plugin package manifest loading and validation.

## Scope

- recognized manifest paths
- native required fields and compatibility loading
- shared Codex-compatible manifest fields and Psychevo namespaced extensions
- descriptive detection of Hermes and OpenCode package shapes for static inspection
- path safety for local package resources
- diagnostics for supported, ignored, and invalid fields

Out of scope:
- installation, catalogs, store records, policy overlays, worker lifecycle, or CLI commands
- compatibility with foreign in-process plugin ABIs
- hosted marketplace metadata validation beyond local manifest fields

## Manifest Paths

Runtime checks these portable package bases in order from a package root:

```text
.codex-plugin/plugin.json
.claude-plugin/plugin.json
```

The first existing base wins. If both exist, runtime loads the Codex base and
reports the Claude-compatible base as shadowed. An optional root-level
`psychevo.plugin.json` is a companion overlay, not an alternative base.
Malformed recognized bases or overlays fail closed for that package.
Hermes `plugin.yaml`/`plugin.yml` and OpenCode package descriptors are foreign
inspection inputs, not native manifests, and are handled by plugin inspection
rather than by the normal manifest loader.

## Codex Package Base

The normalized `codex-plugin/8604689e` package preserves `name`, optional
`version`, optional `description`, `keywords`, and these component fields:

- `skills`
- `mcpServers`
- `hooks`
- `apps`
- `interface`

An absent or blank `name` normalizes to the package-root basename, matching the
pinned Codex loader; it is not an install failure by itself.

Local development packages without a version use active version `local`.
Marketplace packages retain `<plugin>@<marketplace>` identity and the version
selected by the owning marketplace. Package identity is not reconstructed from
display metadata or a Psychevo source slug.

Codex component defaults are behavior: `skills/`, `hooks/hooks.json`,
`.mcp.json`, and `.app.json`. `config.toml` belongs to `CODEX_HOME`; it is not a
package component default. Explicit resource paths must begin with `./`,
contain no `..`, and remain below the package root. Synthesized defaults pass
through the same containment check.

`interface.capabilities` is descriptive model/UI metadata. It is not a
permission grant, runtime capability gate, or fine-grained policy selector.
Runtime parses Codex-compatible `interface` metadata into a typed display
record. Supported display fields are `displayName`, `shortDescription`,
`longDescription`, `developerName`, `category`, `capabilities`, `websiteUrl`,
`privacyPolicyUrl`, `termsOfServiceUrl`, `brandColor`, `composerIcon`, `logo`,
`logoDark`, `screenshots`, and `defaultPrompt`, including Codex's `*URL`
aliases. `defaultPrompt` accepts one string or at most three strings, collapses
whitespace, and ignores entries over 128 characters. Path-bearing media fields
use the same package-relative path safety rules as other manifest resources.
Invalid display fields are diagnostics and do not make the package invalid
when the core manifest remains usable.

## Psychevo Companion Overlay

Psychevo-only plugin behavior lives in `psychevo.plugin.json`. Supported
overlay fields are:

- `runtime`
- `agents`
- `toolsets`

`commands`, `providers`, and any shared Codex component are unsupported in this
profile. Unknown or duplicated component fields are preserved for inspection
and make the overlay unavailable for projection. The overlay never changes the
normalized Codex base.

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

The overlay `toolsets` field uses the same shape as configured custom
toolsets: each key names a toolset with optional `description`, `tools`, and
`includes`. Manifest loading parses the descriptors and leaves expansion and
acceptance to 007 Tool Surface.

`interface` is metadata-only. `apps` are service-owned components: local
Psychevo-owned imports report `needs_codex_install` or `unavailable`; a
Codex-owned installed package delegates Apps inventory, authentication,
elicitation, and MCP tool calls through the Codex capability broker.

## Compatibility Manifests

`.codex-plugin/plugin.json` is a first-class Codex-compatible package path.
`.claude-plugin/plugin.json` is a compatibility manifest path. Local
development packages may use active version `local`; marketplace packages take
their installable version from the owning catalog.

Compatibility fields are mapped only under the pinned profile. Unknown fields
are retained in the raw document and reported as a newer-contract diagnostic;
known components may still be inspected, but the package is not labeled fully
compatible until the profile is upgraded and its conformance suite passes.

Hermes `plugin.yaml` packages and OpenCode package descriptors are static
inspection inputs, not executable compatibility packages in the manifest
loader. If surfaced by diagnostics, their manifest fields remain descriptive
only. Psychevo must not install, enable, trust, import, or execute Hermes Python
`register(ctx)` or OpenCode server/TUI modules through the plugin runtime.

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

`runtime.worker` in the companion overlay declares a Psychevo stdio worker:

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
and do not grant shell evaluation. The overlay is Psychevo-owned behavior and
is never interpreted as part of the Codex compatibility profile.

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
- [150 Foreign Plugin Inspection](../150-plugin-runtime/foreign-inspection.md)
  defines foreign package inspection boundaries.
