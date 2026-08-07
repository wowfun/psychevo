---
name: 155. Plugin Manifest
psychevo_self_edit: deny
---

Define Plugin package manifest discovery, normalization, and path validation.

## Scope

- recognized portable base and companion manifest paths
- Codex-compatible fields and Psychevo declarative additions
- path containment, unknown-field diagnostics, and compatibility inspection
- rejection of executable Extension content in a Plugin root

Out of scope:

- installation, marketplaces, store records, enablement, or CLI commands
- sidecars, direct commands, workers, Channels, or UI process protocols
- compatibility with foreign in-process plugin ABIs

## Manifest Paths

Plugin loading checks these portable bases in order:

```text
.codex-plugin/plugin.json
.claude-plugin/plugin.json
```

The first existing base wins. If both exist, the Codex base wins and the
Claude-compatible base is reported as shadowed. An optional root
`psychevo.plugin.json` is a companion overlay, never an alternative base.
Malformed recognized JSON fails closed.

`psychevo.extension.json` is not a Plugin manifest. A Plugin source containing
it fails Plugin add before materialization or policy mutation and reports the
exact `pevo install <source>` alternative. An Extension installer may recognize
at most one co-root Plugin base under the rules in
[058 Extensions](../058-extensions/spec.md).

Hermes `plugin.yaml`/`plugin.yml`, OpenCode package descriptors, and Pi
JavaScript extension entrypoints are foreign inspection inputs. They do not
enter normal Plugin loading.

## Portable Base

The normalized `codex-plugin/8604689e` base preserves:

- `name`, optional `version`, `description`, and `keywords`
- `skills`
- `mcpServers`
- `hooks`
- `apps`
- `interface`

An absent or blank `name` normalizes to the package-root basename for Codex
compatibility. Marketplace identity still comes from the selected
`<plugin>@<marketplace>` row rather than display metadata. The marketplace owns
the installable version; local marketplace development packages may use
`local`.

Codex defaults are `skills/`, `hooks/hooks.json`, `.mcp.json`, and `.app.json`.
`config.toml` belongs to `CODEX_HOME` and is not a package default. Explicit and
synthesized resource paths pass through the same containment rules.

`interface.capabilities` and other interface fields are display metadata, not
permissions or runtime gates. Supported normalized fields are `displayName`,
`shortDescription`, `longDescription`, `developerName`, `category`,
`capabilities`, `websiteUrl`, `privacyPolicyUrl`, `termsOfServiceUrl`,
`brandColor`, `composerIcon`, `logo`, `logoDark`, `screenshots`, and
`defaultPrompt`, including the pinned Codex `*URL` aliases. `defaultPrompt`
accepts one string or at most three strings, collapses whitespace, and ignores
entries over 128 characters. Invalid display metadata is diagnostic and does
not invalidate an otherwise usable base.

## Psychevo Companion Overlay

`psychevo.plugin.json` may add only declarative Psychevo-owned sources:

- `agents`
- `toolsets`

It cannot repeat or replace `skills`, `mcpServers`, `hooks`, `apps`, or
`interface` from the portable base. It does not support `runtime`, `worker`,
`commands`, `channels`, `providers`, executable paths, or frontend modules.
Unknown or duplicate component fields remain visible for inspection and make
the overlay unavailable for projection.

Agent entries are package-relative roots or descriptors mapped into the Agent
owner. Toolset entries use the configured custom toolset shape with optional
`description`, `tools`, and `includes`; expansion and acceptance stay with Tool
Surface.

Executable capability must be packaged as an Extension manifest and may be
associated only through the one-way co-root Extension relationship. A Plugin
overlay never points to or activates that Extension.

## Component Loading

Hook paths and default `hooks/hooks.json` produce candidate hook declarations.
Manifest loading does not trust or execute handlers. Hook Runtime normalizes
and reviews them after Plugin enablement.

`mcpServers` may be an object of server descriptors or an explicit
package-relative JSON path. If absent, `.mcp.json` may be used. Malformed
siblings are diagnosed independently. MCP owns startup, auth, tool listing,
approval, and execution.

`apps` are service- or MCP-owned components. Local Psychevo projection reports
an actionable owning-runtime readiness; Codex-owned packages may delegate Apps
inventory, authentication, elicitation, and MCP calls through the Codex broker.
No App script is imported into the Workbench bundle.

Unknown fields are retained in the raw document and reported as a newer
contract diagnostic. Known components remain inspectable, but the package is
not reported fully compatible until the profile is upgraded and its conformance
suite passes.

## Path Safety

Every local resource path must:

- begin with `./`
- be relative and contain no `..` component
- remain below the canonical package root
- resolve to the expected file or directory kind

Invalid paths omit the affected declaration and produce a source-qualified
diagnostic. Loading never makes an unsafe path acceptable by dropping
components or following a link outside the root.

## Inspection And Source Metadata

Raw, normalized, and effective projection documents remain distinct. Unknown
data stays in the raw layer and never gains authority through normalization.
Inspection reports manifest kind and path, portable profile, components,
unsupported fields, diagnostics, and detection of an Extension manifest.

Marketplace records preserve marketplace identity, selected version,
materialized fingerprint, and authority. Those values are identity and
diagnostic facts, not permission grants.

Hermes and OpenCode inspection reports fixed support state `inspection_only`
and never imports Python or JavaScript. Users execute those systems through
their separate Agent runtime profiles.

## Related Topics

- [054 Plugins](../054-plugins/spec.md) defines Plugin boundaries.
- [058 Extensions](../058-extensions/spec.md) defines the separate executable
  manifest and one-way co-root relationship.
- [150 Plugin Runtime](../150-plugin-runtime/spec.md) defines store,
  marketplace, policy, and declaration loading.
