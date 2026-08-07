---
name: 150. Plugin Runtime
psychevo_self_edit: deny
---

Define Plugin marketplace acquisition, store and policy operations,
declaration loading, diagnostics, and external Codex authority integration.

There is no Plugin executable runtime. Executable sidecars belong to
[058 Extensions](../058-extensions/spec.md).

## Scope

- profile and project Plugin stores and policy overlays
- local, Git, npm, and HTTPS marketplace indexes
- bounded package materialization and atomic replacement
- Plugin add, remove, enable, disable, inspect, and diagnostics
- host-owned static declaration loading
- default-off, profile-isolated Codex Plugin authority
- static Hermes and OpenCode inspection

Out of scope:

- in-process Plugin ABI or stdio Plugin workers
- executable commands, Channels, or UI modules
- hosted marketplace accounts, ratings, reviews, signatures, or sharing
- provider credentials stored in packages

## Store And Materialization

Profile stores use `$PSYCHEVO_HOME/plugins/{cache,data}`. Project stores use
`<cwd>/.psychevo/plugins/{cache,data}`. An install record preserves Plugin and
marketplace identity, selected version, source descriptor, resolved Git
revision when applicable, scope, immutable package root, data root, manifest
kind/path, content fingerprint, and diagnostics.

Users add a selected `<plugin>@<marketplace>` row rather than installing an
arbitrary package source directly. A marketplace index may itself be a local
directory, Git source, or npm package. `marketplace add` infers those source
classes; Codex-style `owner/repo[@ref]` is normalized to an HTTPS GitHub URL,
while an explicit `--kind` remains available for ambiguous inputs. Its normalized
rows name one Plugin, version, package source, optional subpath, integrity
metadata when supplied, and display metadata. Marketplace source credentials
and sensitive query values never enter records or output.

Materialization uses one staging root and one private bounded policy:

- 120-second subprocess/materialization deadline
- 50 MiB archive input
- 200 MiB total unpacked or installed bytes
- 10,000 entries
- 50 MiB per regular file
- 1,024-byte relative path and 64 path components

Git acquisition is shallow, no-tags, and single-branch. Runtime records
`rev-parse HEAD` then removes only the staging root's top-level `.git`. Npm uses
`npm pack --ignore-scripts`, validates requested package/version, and never runs
lifecycle scripts. Archive extraction rejects absolute and parent-traversal
paths, links, devices, and non-regular entries. Every subprocess owns a process
tree and one deadline covers exit, descendant termination, and bounded output
drains.

Before publishing a record, Plugin loading rejects
`psychevo.extension.json`, validates the selected manifest, fingerprints the
bounded materialization, and atomically replaces the destination. Failure
removes staging and preserves the previous install and policy. Add always
writes `enabled = false`; callers must enable separately after inspection.

Remove deletes the record and cache materialization for the selected scope. It
retains the identity-qualified data root. A future explicit data-removal action
may change that, but removal must not silently erase state.

## Marketplace Operations

`pevo plugin marketplace` owns:

- `add <source>`
- `list`
- `upgrade [marketplace]`
- `remove <marketplace>`

Marketplace add and upgrade validate the complete index before atomic
publication. Upgrading a marketplace changes discovery metadata only; it does
not upgrade installed Plugins. Removing a marketplace is rejected while its
installed Plugins exist unless a future explicit force flow defines the
consequence.

Marketplace identity is stable and source-qualified. Names collide within one
scope rather than silently shadowing by insertion order. Catalog aggregation
never merges rows from different authorities or marketplaces by display name.

## Policy Overlay

Profile and project config combine into effective per-invocation Plugin policy.
Add, enable, and disable use the active profile by default; `--local` uses the
current canonical cwd. `--global` is an alias for profile scope where exposed
and conflicts with `--local`.

Plugin policy contains package enabled state only. Enabling makes static
declarations eligible for host mapping. Owning modules still control hook
trust, MCP startup/tool approval, provider credentials, permission, sandbox,
and evidence.

Installed selectors preserve `<plugin>@<marketplace>` identity and add a scope
qualifier only when profile and project rows are ambiguous. Bare Plugin names
are accepted only when they resolve to one installed row.

For Codex authority, profile config may enable or disable
`codex:<plugin>@<marketplace>`. Project config may disable an inherited Codex
Plugin or remove its override but cannot enable one the profile disallows.
`codex_plugins.enabled` is profile-only and defaults off. Plugin add through
that authority records the returned fingerprint but still leaves Psychevo
policy disabled until explicit enable.

## Declaration Loading

The host loads enabled Plugin manifests before Agent and Skill discovery and
maps static candidates directly into owning modules:

- skill roots
- MCP server and MCP App descriptors
- hook sources
- Agent roots
- toolset descriptors
- typed interface metadata

Plugin manifests do not supply executable workers, direct commands, Channel
adapters, providers, or frontend modules. A recognized obsolete
`runtime.worker` or executable field is an unsupported diagnostic and makes the
overlay unavailable; it is never started. Authors move that capability to an
Extension.

MCP candidates enter the source-aware MCP catalog before startup, listing,
naming, approval, and Tool Surface conflict resolution. Hook candidates enter
Hook Runtime only after package enablement and still require normalized-content
trust. Toolsets enter Tool Surface only after include resolution, mode
filtering, disabled subtraction, and binding checks. No declaration creates an
accepted runtime effect merely by being parsed.

Every component has exactly one execution owner and retains Plugin identity in
diagnostics. Unknown and invalid siblings do not grant authority; when an owning
schema permits partial loading they omit only their affected component.

## Codex Authority

`CodexPluginAuthority` is the deep module for Codex-owned package inventory and
service components. Its external interface exposes authority state, management
operations, a non-blocking turn snapshot, a turn lease, and shutdown. It owns
process/profile negotiation, broker multiplexing, auth-link setup, inventory
generation, policy digests, redacted diagnostics, and draining mutations.

The broker is one managed child:

```text
codex app-server --strict-config -c cli_auth_credentials_store="file" --listen stdio://
```

It replaces `CODEX_HOME` with `$PSYCHEVO_HOME/codex/`, ignores inherited
`CODEX_HOME`, and spawns nothing when disabled. Startup validates a semantic
version identity and canonical private home. It does not pin one Codex patch or
probe unrelated methods; each called operation validates its own response.

The authority links private `auth.json` to the user's Codex auth without
reading or copying its contents: Unix uses a symlink; Windows uses a hardlink
only on the same volume. Missing auth or link failure affects service-owned
components, not local declarative package components.

One child has one stdout reader, writer, and request-id map. Server requests
route elicitation by Thread and Turn, so one waiting turn cannot block catalog,
auth, or another turn. Stderr is bounded, redacted, and structured. Mutating
requests and tool calls are never retried after delivery.

Inventory is keyed by profile plus canonical cwd and carries a generation. The
provider hot path reads only a ready memory snapshot. Loading, stale, or failed
inventory contributes an empty Codex set with structured diagnostics and never
delays provider dispatch or writes transcript/UI noise.

Each admitted turn freezes generation and policy digest and holds a lease. A
disable rejects new leases while admitted work completes. Remove, package
upgrade, marketplace removal/upgrade, binary switch, and feature-off drain
active leases before physical mutation. Destructive mutations are mutually
exclusive. A management refresh publishes a new generation only after success.

The broker advertises only elicitation and MCP Apps capabilities Workbench can
render faithfully. Apps inventory, authentication, resource reads, and MCP
tool calls remain broker-owned; local Plugin code is never imported into the
Workbench. Missing, incompatible, or auth-unavailable Codex affects only its
owned components.

## Foreign Inspection

`plugin inspect` can examine a local path or marketplace row without adding,
enabling, or executing it. Hermes and OpenCode results report framework,
source, canonical identity, manifest path, declared/unsupported lanes,
diagnostics, and fixed support state `inspection_only`. Inspection never starts
Python or Node or imports package code.

Adding a detected Hermes or OpenCode source is rejected before store or config
mutation and directs the caller to the corresponding ACP Agent runtime profile.

## CLI And Gateway Operations

`pevo plugin` owns:

- `add <plugin>@<marketplace>`
- `list`
- `view <selector>`
- `doctor [selector]`
- `inspect <source-or-row>`
- `remove <selector>`
- `enable <selector>`
- `disable <selector>`
- `marketplace add|list|upgrade|remove`

There are no `plugin install` or `plugin uninstall` aliases. Read and diagnostic
commands accept `--json` and return secret-free structured output. Human output
uses typed display metadata and emphasizes the next actionable state. Add
materializes disabled; enable/disable are never implicit consequences of
inspection or marketplace operations.

Gateway exposes equivalent typed methods:

- `plugin/list`, `plugin/read`, `plugin/doctor`, `plugin/inspect`
- `plugin/add`, `plugin/remove`, `plugin/setEnabled`
- `plugin/marketplace/list`, `add`, `upgrade`, and `remove`
- `plugin/authority/read`, `write`, `refresh`, and `setTrust`
- `plugin/connect/start` and `plugin/connect/status`

`plugin/setEnabled.enabled` is `boolean | null`; null removes the selected
scope override. Authority-qualified responses keep Codex and Psychevo results
typed rather than collapsing method-specific results into all-optional
objects. Destructive or overwrite behavior requires an explicit caller field.

`plugin/list` returns authority views plus partitioned installed and
marketplace rows. Each component reports compatibility profile, highest level,
owner, readiness, and a short reason. Codex authority view separates runtime
and auth status and reports binary/version/private-home/generation facts without
environment values or content.

`plugin/connect/start|status` is a five-minute process-local connection
session. Apps open the validated install URL; ordinary MCP uses its OAuth owner.
Gateway restart expires sessions. Plugin removal does not log out a separate
connector.

## Acceptance Criteria

- add requires a marketplace-qualified row and leaves it disabled
- unsafe or oversized materialization cannot publish partial state
- a Plugin containing an Extension manifest fails before state mutation
- no Plugin inspection, listing, or declaration loading starts executable
  package code
- enablement never bypasses an owning trust, permission, auth, or conflict gate
- marketplace and authority identities remain distinct in all selectors/output
- Codex feature-off spawns no child; inventory failure does not delay turns
- static foreign inspection never imports or executes the foreign package

## Related Topics

- [054 Plugins](../054-plugins/spec.md) defines the product boundary.
- [058 Extensions](../058-extensions/spec.md) defines executable sidecars.
- [155 Plugin Manifest](../155-plugin-manifest/spec.md) defines manifest shape.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines command spelling.
- [Foreign Plugin Inspection](./foreign-inspection.md) defines foreign static
  inspection details.
