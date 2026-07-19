---
name: 150. Plugin Runtime
psychevo_self_edit: deny
---

Define the current implementation slice for plugin installation, package
enablement, diagnostics, static declarations, and Psychevo worker execution.

## Scope

- plugin store roots for profile and project scopes
- local directory and Git installation
- npm package materialization with lifecycle scripts disabled
- marketplace catalogs for local, Git, and npm sources
- plugin package enablement overlay in `RunConfig`
- static declaration loading
- manifest-only and adapter-host foreign package inspection
- package fingerprint trust state
- default-off, profile-isolated Codex plugin authority
- stdio JSON-RPC worker execution for tools
- CLI- and Gateway-facing read, diagnostic, install, uninstall, enable,
  disable, inspect, trust, and catalog operations

Out of scope:
- reimplementation of hosted marketplace accounts, reviews, ratings, or sharing
- in-process plugin ABI or unbounded host facades
- worker hot reload, long-lived health checks, streaming worker diagnostics, or whole-process sandboxing
- provider credential storage inside plugin packages
- Hermes Dashboard routes or OpenCode TUI/UI slots

## Store

A plugin store is scoped to either the active profile home or the current
cwd. Profile stores use `$PSYCHEVO_HOME/plugins/{cache,data}`. Project
stores use `<cwd>/.psychevo/plugins/{cache,data}`.

An install record preserves:

- plugin name
- version
- source identity
- install scope
- package root
- data root
- manifest path and manifest kind
- diagnostics

Local directory installs copy the package into the cache root. Git installs
materialize the repository or selected package path into the cache root. Npm
installs run `npm pack --ignore-scripts` into a staging directory, validate the
tarball package name and version against the requested source when supplied,
enforce archive and extracted-size limits, and copy the unpacked package into
the cache root. Deterministic tests must use local temporary Git repositories
and fake npm fixtures, not network repositories.

Install and inspect operations preserve source kind as `local`, `git`, or
`npm`. Source identity includes the package locator, selected version or Git
ref when present, and registry for npm sources when present.

Uninstall removes the install record and cache materialization for that scope.
V1 may leave the data root unless the command explicitly removes data in a
future spec.

## Policy Overlay

`RunConfig` includes plugin package policy. Profile config and project config
overlay produce the effective plugin policy for an invocation.

The profile-only authority configuration is:

```toml
[codex_plugins]
enabled = false
binary = "/optional/path/to/codex"

[plugins."codex:review@openai"]
enabled = true
```

`codex_plugins.enabled` defaults to false. An empty binary resolves `codex`
from `PATH`. Project configuration containing `[codex_plugins]` is invalid.
Profile Codex plugin policy accepts `true` or `false`; project Codex plugin
policy accepts only `false`, while deleting the entry restores inheritance.
Install and explicit upgrade write profile allow and trust for the returned
current package fingerprint.

Plugin policy records package enabled state only. Enabling a package makes its
accepted declarations available to the host-owned mapping step. The owning
runtime module then decides whether the effect is usable:

- MCP server and tool approval policy belongs to MCP and tool surfaces.
- hook trust belongs to 140 Hook Runtime.
- provider policy and credentials belong to provider management.
- sandbox and permission decisions belong to permission runtime.

CLI `install`, `enable`, and `disable` write the active profile scope by
default. `--local` writes the current cwd `.psychevo/config.toml`.
`--global` is an alias for the active profile scope and conflicts with
`--local`.

Adapter policy records framework default mode and per-plugin mode. Supported
adapter modes are:

- `adapter_host`: allow the framework adapter host after install, enablement,
  and current fingerprint trust
- `manifest_only`: inspect static metadata and report target lanes without
  importing or executing foreign code
- `disabled`: do not inspect beyond source/manifest detection

Trust state is keyed by normalized plugin identity plus source identity and
stores the package fingerprint last approved by the user. A mismatched
fingerprint changes the readiness state to `Needs trust` and prevents adapter
host execution.

Installed package identity is scope-qualified as `profile:name@source` or
`project:name@source`; commands project and accept that canonical selector.
Bare `name` and unscoped `name@source` remain shorthand only when they resolve
to one record across both installation scopes.

## Declaration Loading

Runtime loads enabled plugins before agent and skill discovery. Static manifest
declarations can add:

- skill roots
- MCP server descriptors
- hook sources
- agent roots
- command descriptors
- toolset descriptors
- provider descriptors

The owning runtime boundary decides whether each descriptor is usable. Duplicate
model-visible tool names are conflict-resolved by the existing tool surface
rules, with plugin identity included in diagnostics.

Plugin MCP descriptors are parsed into source-scoped MCP server inputs before
MCP startup. Invalid descriptors omit only the affected server and produce
plugin diagnostics. Accepted plugin MCP inputs still pass through MCP startup
permission, MCP tool listing, MCP tool naming, and tool-surface conflict
handling.

Plugin toolset descriptors are parsed as source-scoped toolset candidates.
They are accepted only by 007 Tool Surface after include resolution, disabled
toolset subtraction, mode filtering, and execution-binding checks. A toolset
descriptor does not create a model-visible tool by itself.

Command descriptors, provider descriptors, and apps remain inert or descriptive
in the current implementation slice unless another owning module defines
acceptance semantics. Typed interface metadata is supported for package display
only. `plugin doctor` may report inert descriptors as recognized and
unsupported without implying runtime support.

Portable Codex components are projected independently. For an installed,
enabled Codex catalog package, the authority resolves an exposed Codex-owned
package root without copying it into the Psychevo plugin store. A local skill is
owned by the Psychevo skill runtime, a local hook by the Psychevo hook runtime,
and an ordinary local MCP declaration by the Psychevo MCP runtime. Remote or
app-backed MCP and Apps are owned by the Codex broker. App templates, scheduled
tasks, interface/default-prompt metadata, browser extensions, and unknown
manifest fields are metadata-only diagnostics: scheduled tasks and browser
extensions are explicitly unsupported, and interface prompts are never injected
into a turn. A remote skill or hook without a package root is not downloaded or
reported as portable. Every component has exactly one owner.

`CodexPluginAuthority` is the deep module for this boundary. Its external
interface exposes only authority state, management operations, a non-blocking
turn snapshot, a turn lease, and shutdown. Internally it owns process/profile
negotiation, broker multiplexing, auth-link setup, inventory generation, policy
digests, redacted diagnostics, and draining mutations. No capability snapshot,
generation, policy digest, or owner table is persisted; the transcript records
only actual calls.

The broker is one managed external child launched as
`codex app-server --strict-config -c cli_auth_credentials_store="file" --listen
stdio://`. It inherits the Psychevo subprocess environment, forcibly replaces
`CODEX_HOME` with `$PSYCHEVO_HOME/codex/`, and removes binary-selection test
variables. Profile configuration selects an optional binary path and otherwise
uses `codex` from `PATH`. Psychevo never reads or imports inherited
`CODEX_HOME`, and feature-off operation spawns no Codex process.

The reviewed compatibility profile `codex-plugin/8604689e` initially accepts
exactly `codex-cli 0.144.1`. Negotiation extracts a semantic version from any
originator-shaped `userAgent`, verifies canonical `codexHome`, and uses requests
that must fail during parameter validation to prove required methods exist
without performing a normal `plugin/list` or network-backed catalog request.
Fixed app-server fixtures validate every successful response shape. A binary
missing required methods, returning an unknown version or shape, or reporting a
different home remains diagnosable but cannot serve catalog, mutation, auth, or
execution operations.

The authority creates a Unix symlink from private `auth.json` to the user's
`~/.codex/auth.json`; Windows uses a hardlink only when both files are on the
same volume. It never reads auth contents, reads keyrings, copies tokens, or
synchronizes credentials. Missing global auth, cross-volume Windows paths, or
link failure leaves the feature enabled with `auth: unavailable`; local
unauthenticated components remain usable.

One app-server child has one stdout reader, one writer, and a request-id pending
map. Server requests route elicitation by `threadId` and `turnId`, so a turn
waiting for user input does not block catalog, auth, or another turn. Stderr is
bounded, redacted, and structured. Install, uninstall, upgrade, and tool calls
are never retried after delivery.

Runtime inventory is keyed by profile plus canonical cwd and carries a
generation. Initialize, draft creation, and management refresh may prewarm it in
the background. The provider-dispatch hot path reads only a ready in-memory
snapshot and never performs app-server RPC. Loading, stale, or failed inventory
immediately contributes an empty Codex set, continues provider dispatch, and
writes only a structured diagnostic—never a transcript item, toast, or visual
delay.

Each admitted turn snapshots the current inventory generation and policy digest
and keeps that selection stable for that turn; the next turn observes the newest
ready generation. Project disables filter a cached inventory without discovery.
Install, account change, broker replacement, package mutation, and authority
refresh publish a new generation.

Turn admission obtains a lease for the generation it actually uses. A policy
disable immediately prevents new leases while existing turns continue.
Uninstall, upgrade, marketplace removal/upgrade, binary switch, and feature-off
enter `draining`, reject new leases, wait for active leases, then perform the
physical mutation. Destructive draining mutations are mutually exclusive; one
mutation cannot clear the draining state while another physical mutation is
still running. Install and marketplace add may finish concurrently but
appear only in a new generation. Workbench exposes the draining reason and lets
the user stop related turns.

The broker advertises the pinned app-server elicitation capabilities that the
Workbench can faithfully answer: MCP standard forms (including titled single-
and multi-select values), URL elicitations, and the pinned `openai/form`
`openai/imagePicker` shape. Answers are returned as typed form content with
Codex `_meta`; arbitrary future custom controls are not inferred from unknown
schema fields.

The broker retries only requests proven not delivered. Install, uninstall, and
tool calls are not retried after delivery because they may have side effects.
Missing, disabled, auth-unavailable, or incompatible Codex processes make only
the affected Codex-owned components unavailable; native package components and
provider dispatch continue to work.

Plugin hook sources are loaded only when the plugin package is enabled. Loading
a plugin hook source does not trust or execute its handlers. Runtime passes
plugin hook declarations to 140 Hook Runtime, where they normalize to the
canonical hook shape and require hook trust review before execution.

## Worker V1

A plugin worker is an external stdio JSON-RPC process declared by Psychevo
manifest metadata under `psychevo.runtime`. Runtime starts workers only when the
plugin package is enabled and an owning runtime path needs the worker.

Startup context includes plugin identity, plugin root, plugin data root,
invocation scope, manifest resources, Psychevo extension metadata, and host
capability descriptors.

Worker V1 supports:

- `initialize`
- `contributions/list`
- `tools/call`
- `hooks/call`
- `shutdown`

The first executable worker declaration is a tool. Worker tool descriptors
become `ToolBinding` adapters and enter `ToolSurfaceAssembly.extension_tools`.
Worker-provided hook handlers are candidate hook declarations for the hook
runtime handler family. A `worker` hook handler calls `hooks/call` through the
plugin worker adapter after package enablement and hook trust review select that
handler. Missing worker metadata, missing worker process, method-not-found
responses, malformed worker responses, and worker timeouts become structured
hook diagnostics and affect only the hook run.

Worker tools are accepted through the shared extension assembly and tool
surface. Plugin worker tools are source-qualified as plugin tools. When
synthetic `tool_search` is enabled, direct plugin worker tools enter the router
as deferred bindings by default; explicit invocation configuration may disable
`tool_search` and expose otherwise direct worker tools directly. Plugin
manifests do not decide direct model visibility by themselves.

## Adapter Host V1

Foreign adapters run out of process and return a structured inspection result.
Psychevo owns the host protocol and maps returned declarations into existing
runtime lanes. Adapter host output must include stage diagnostics for
`resolve/fetch`, `inspect manifest`, `compatibility`, `target lanes`, `policy`,
`trust`, and `readiness`.

Hermes uses a Python adapter host. OpenCode uses a Node adapter host. Adapter
hosts may report supported tools, hooks, skills, and MCP/toolset declarations.
They may also report unsupported provider, UI/TUI, dashboard/auth, theme, route,
slot, and command-execution lanes. Unsupported lanes are displayed in read,
doctor, CLI, Gateway, and Workbench diagnostics but are not executed.

The default safe path is manifest-only inspection. Adapter host execution is
allowed only after install, enablement, policy mode `adapter_host`, and matching
package fingerprint trust.

## CLI Operations

`pevo plugin` owns:

- `list`
- `view`
- `doctor`
- `inspect`
- `install`
- `uninstall`
- `enable`
- `disable`
- `trust`
- `marketplace list`
- `marketplace add`
- `marketplace remove`

All read and diagnostic commands accept `--json` and emit secret-free
structured output. Human output is concise and action-oriented, using typed
interface metadata when present for display name, short description, category,
developer, and capabilities.

`plugin inspect` is a dry-run operation over a local path, Git source, npm
source, or catalog row. It materializes into a temporary staging directory when
needed, detects native, Codex, Hermes, and OpenCode package shapes, reports the
canonical identity, source kind, adapter framework, package fingerprint, target
lanes, unsupported lanes, readiness, and diagnostics, and does not install,
enable, trust, import foreign code, or mutate profile/project state.

`plugin trust` records trust for the current installed package fingerprint.
CLI commands never mutate the Codex authority, its packages, catalogs, binary,
or connector sessions; Codex mutation and connection are GUI/Gateway operations.

`plugin doctor` reports discovered packages, manifest path, supported and
ignored fields, manifest resources, Psychevo extensions, install source, source
kind, active version, fingerprint, enabled state, adapter policy/trust state,
skipped reason, owning-surface policy state, worker failures, adapter stage
diagnostics, and data root.

Gateway exposes plugin metadata and package-management methods for product
surfaces: `plugin/list`, `plugin/read`, `plugin/doctor`, `plugin/install`,
`plugin/uninstall`, `plugin/setEnabled`, `plugin/import/inspect`,
`plugin/setTrust`, `plugin/authority/write`, `plugin/authority/refresh`,
`plugin/catalog/list`, `plugin/catalog/add`, `plugin/catalog/remove`,
`plugin/catalog/upgrade`, `plugin/connect/start`, and `plugin/connect/status`.
`plugin/setEnabled.enabled` is `boolean | null`, where null deletes the selected
scope override. Catalog requests are authority-qualified; Codex sources support
source, ref, and sparse paths. These methods use the same runtime helpers as the CLI,
honor resolved scope/profile rules, return typed interface metadata, and keep
responses secret-free. GUI install overwrite, trust writes, and other
force-worthy actions require an explicit request supplied by the caller.

`plugin/list` returns authority views plus partitioned installed and catalog
rows. `CodexAuthorityView` separates runtime (`disabled`, `starting`, `ready`,
`incompatible`, `unavailable`, `draining`) from auth (`available`,
`unavailable`) and reports the resolved binary, Codex version, compatibility
profile, canonical private home, platform, generation, inventory readiness, and
security notes without exposing environment values or content.

Requests and responses use authority-qualified records. Psychevo authority uses
the existing profile/project canonical selector. Codex authority uses
`codex:<plugin>@<marketplace>`. Catalog lists may aggregate both authorities, but
must not merge by display name. Each component record reports compatibility
profile, highest level, execution owner, readiness, and a short actionable
reason.

`plugin/connect/start|status` is a generic five-minute, process-local connection
session. Apps open the validated Codex `installUrl` and complete when
`app/list.isAccessible` becomes true. Ordinary MCP starts
`mcpServer/oauth/login` and consumes its completion notification. Gateway restart
expires outstanding sessions. Uninstall never logs out a separate connector,
and v1 provides no Codex logout UI.

## Related Topics

- [054 Plugins](../054-plugins/spec.md) defines product plugin boundaries.
- [155 Plugin Manifest](../155-plugin-manifest/spec.md) defines manifest loading.
- [140 Hook Runtime](../140-hook-runtime/spec.md) defines hook execution and trust-aware plugin hook loading.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines command spelling.
- [150 Plugin Runtime Adapter Hosts](./adapter-hosts.md) defines foreign adapter host inspection boundaries.
